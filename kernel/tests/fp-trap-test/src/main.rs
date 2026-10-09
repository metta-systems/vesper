#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! fp-trap-test: the integer-only FP/SIMD policy is enforced by hardware.
//!
//! The boot path (`libboot`, Kickstart) configures `CPTR_EL2`/`CPACR_EL1` so
//! FP/SIMD instructions trap to EL1 from EL1 and EL0. This kernel executes one
//! FP/SIMD instruction (see `fp-trap-protocol`) twice:
//!
//! - in the trusted `EL1t` boot Thread, where faults halt the kernel, so the
//!   nucleus is built with its test-only `fp_trap_test` hook that hands the
//!   syndrome back instead;
//! - in the EL0 `fp-probe` component, in its own `AddressSpace`, where the
//!   trap is delivered to the component's own fault handler (installed at
//!   `KeySlot::FAULT_HANDLER`), which records the syndrome and skips the
//!   instruction.
//!
//! Each must trap with `ESR_EL1.EC` 0x07 — an execution fault, not an
//! Invocation error and never an automatic enable.
//!
//! Runs on the real Kickstart boot path; in-guest assertions and the QEMU
//! semihosting exit status are the result (`just test-fp-trap`).

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    aarch64_cpu::registers::{Readable, TTBR0_EL1},
    fp_trap_protocol::{INIT_VA, ProbeInit, STACK_REGION, TRAPPED_BIT, probe, trapped},
    kickstart::{
        bootstrap::{
            BOOT_TABLE_SIZE_BITS, PoolCapacities, bootstrap_nucleus, retained_init_memory,
        },
        kickstart_init_el2,
    },
    libkicktest::{
        builder::{Builder, ImageArchive, ImageTarget, verify_retained_image},
        component::ttbr,
        keys::{SlotCursor, boot_key},
        loader::{verify_component, write_init},
        paging::{PAGE, find_leaf, image_table_count},
        threads,
    },
    libobject::{
        KeySlot, KeyTableKey, NotificationKey, ObjectType, Rights, UntypedKey,
        address_space::AddressSpaceKey,
    },
    libqemu::semihosting as semi,
    nucleus::{
        api::key_entry::KeyEntry,
        objects::{KeyTable, Notification, arch_objects::AddressSpaceObject},
    },
};

const BUILDER_ARCHIVE_GUARD: u32 = 0x0A_4C01;
const PROBE_GUARD: u32 = 0x0F_9701;
/// Pages in each probe stack (each also gets a guard page on both sides).
const STACK_PAGES: u32 = 4;
/// Headroom the fault handler's Invocation requires below its SP.
const MINIMUM_HEADROOM: u64 = 0x400;
/// Fixture boot-table slots start clear of the well-known bootstrap slots.
const FIRST_FIXTURE_SLOT: u32 = 64;
/// Where the probe finds its Notification key.
const PROBE_NOTIFICATION_SLOT: u32 = 5;

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

pub fn run() -> ! {
    semi::println!("fp-trap-test: enabled MMU and dropped to EL1");
    libkicktest::assert_fp_simd_trapped();
    libkicktest::assert_el0_visible_state();

    // ── EL1t: the trusted boot Thread itself ──────────────────────────────
    let result = probe();
    assert!(
        trapped(result),
        "an FP/SIMD instruction at EL1t must trap as an FP/SIMD access (x0 = {result:#x})"
    );
    semi::println!("FP/SIMD instruction trapped at EL1t");

    // ── EL0: the probe component in its own AddressSpace ──────────────────
    let retained = retained_init_memory();
    let image_tables = image_table_count(&retained);
    let boot = bootstrap_nucleus(&PoolCapacities {
        // builder, probe
        threads: 2,
        // builder, probe
        address_spaces: 2,
        // probe result
        notifications: 1,
        event_counts: 0,
        // builder: root/L1/L2 and image L3s; probe: root/L1/L2, image span,
        // stack region, init page
        page_tables: 3 + image_tables + 6,
        asid_pools: 1,
    });
    let (nucleus, keytable_addr, builder_as) = (boot.nucleus, boot.keytable_addr, boot.boot_as_id);
    let untyped = UntypedKey::from_key(boot.boot_untyped_key);
    let self_table = KeyTableKey::from_key(boot.self_table_key);
    let builder = Builder {
        untyped: &untyped,
        self_table: &self_table,
        boot_table_addr: keytable_addr,
        retained: &retained,
    };
    let mut slots = SlotCursor::starting_at(FIRST_FIXTURE_SLOT);

    let builder_as_key = boot_key(KeySlot::SELF_ADDRESS_SPACE.0, 1);
    let builder_l2 = builder.root_chain(builder_as_key, &mut slots);
    assert_eq!(Builder::assign_asid(builder_as_key), 1);
    builder.map_retained_image(
        &[ImageTarget {
            address_space: builder_as_key,
            l2: builder_l2,
            first_table_slot: slots.take(u32::try_from(image_tables).unwrap_or(u32::MAX)),
        }],
        &ImageArchive {
            slot: slots.take(1),
            guard: BUILDER_ARCHIVE_GUARD,
            grant_scratch: slots.take(1),
            copy_scratch: slots.take(1),
        },
    );
    let builder_ttbr = ttbr(nucleus, builder_as);
    verify_retained_image(&retained, &[builder_ttbr]);
    AddressSpaceKey::from_key(builder_as_key)
        .activate()
        .unwrap_or_else(|error| panic!("builder Activate failed: {:?}", error.code()));
    assert_eq!(TTBR0_EL1.get(), builder_ttbr);

    let probe_component = builder.component(nucleus, PROBE_GUARD, &mut slots);
    builder.load_component(
        &probe_component,
        &components::PROBE,
        PROBE_GUARD ^ 0x80_0000,
        &mut slots,
    );
    let probe_ttbr = ttbr(nucleus, probe_component.address_space);
    verify_component(probe_ttbr, &components::PROBE);
    // The probe's main stack and its fault handler's stack, each guarded.
    let mut stacks = builder.stack_region(&probe_component, STACK_REGION, &mut slots);
    let main_stack = builder.user_stack(&mut stacks, STACK_PAGES, &mut slots);
    let handler_stack = builder.user_stack(&mut stacks, STACK_PAGES, &mut slots);
    for stack in [main_stack, handler_stack] {
        for guard in [stack.bottom - PAGE, stack.top] {
            assert!(
                find_leaf(probe_ttbr, guard).is_none(),
                "stack guard page at {guard:#x} must be unmapped"
            );
        }
    }

    let result_notification = untyped
        .retype(
            ObjectType::NOTIFICATION,
            0,
            0,
            1,
            &self_table,
            slots.take(1),
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("Notification Retype failed: {:?}", error.code()));
    // Notification is off the `CopyDerive` allowlist, so the builder installs
    // the probe's capability kernel-privately.
    // SAFETY: the probe's retained initialized table, exclusively borrowed for
    // this bootstrap grant; no capability invocation overlaps it.
    let probe_key = unsafe { &mut *(probe_component.table_addr as *mut KeyTable) }
        .insert(
            KeySlot(PROBE_NOTIFICATION_SLOT),
            KeyEntry::new::<Notification>(builder.object_id(result_notification), Rights::all(), 0),
            probe_component.guard,
        )
        .unwrap_or_else(|failure| panic!("Notification grant failed: {:?}", failure.error.code()));
    let init_page = builder.private_pages(
        probe_component.address_space_key,
        probe_component.l2,
        INIT_VA,
        1,
        &mut slots,
    );
    write_init(
        init_page,
        ProbeInit {
            result: probe_key.to_wire(),
            guard: u64::from(PROBE_GUARD),
            size_bits: u64::from(BOOT_TABLE_SIZE_BITS),
        },
    );
    // The probe's own fault handler, at the well-known slot of its table.
    AddressSpaceKey::from_key(probe_component.address_space_key)
        .create_invocation(
            components::PROBE
                .export("fault_entry")
                .expect("fp-probe exports its fault handler"),
            &probe_component.table(),
            KeySlot::FAULT_HANDLER,
            handler_stack.bottom,
            handler_stack.top,
            MINIMUM_HEADROOM,
        )
        .unwrap_or_else(|error| {
            panic!("fault handler CreateInvocation failed: {:?}", error.code())
        });

    let probe_thread = threads::spawn_el0(
        nucleus,
        probe_component.address_space,
        components::PROBE.entry.expect("fp-probe has an entry"),
        main_stack.top,
        0,
    );
    let bits = NotificationKey::from_key(result_notification)
        .wait(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("probe wait failed: {:?}", error.code()));
    assert_eq!(
        bits, TRAPPED_BIT,
        "an FP/SIMD instruction at EL0 must trap as an FP/SIMD access"
    );
    // The handler skipped the instruction and Returned: nothing is left of
    // the fault, and nothing went unhandled.
    let thread = nucleus
        .pools
        .threads
        .get_live(usize::from(probe_thread.index))
        .expect("probe Thread missing");
    assert_eq!(thread.fault, None, "the handled fault must be released");
    assert_eq!(thread.address_space, probe_component.address_space);
    assert_eq!(thread.invocation_stack.len(), 0);
    let space = nucleus
        .pools
        .arch
        .address_spaces
        .get_live(usize::from(probe_component.address_space.index))
        .expect("probe AddressSpace missing");
    assert!(!space.fault_handler_busy());
    assert_eq!(space.unhandled_faults(), 0);
    semi::println!("FP/SIMD instruction trapped at EL0 and was handled by its component");

    semi::println!("FP/SIMD trapping at EL1t and EL0 passed");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}
