#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! fp-trap-test: the integer-only FP/SIMD policy is enforced by hardware.
//!
//! Kickstart configures `CPTR_EL2`/`CPACR_EL1` so FP/SIMD instructions trap to
//! EL1 from EL1 and EL0. This kernel executes one FP/SIMD instruction (see
//! `fp-trap-protocol`) twice:
//!
//! - in the trusted `EL1t` boot Thread;
//! - in the EL0 `fp-probe` component, in its own `AddressSpace`.
//!
//! Each must trap with `ESR_EL1.EC` 0x07 — an execution fault, not an
//! Invocation error and never an automatic enable. The nucleus is built with
//! its test-only `fp_trap_test` hook, which hands the syndrome back to the
//! probe instead of halting; real fault delivery is the open D1 decision.
//!
//! Runs on the real Kickstart boot path; in-guest assertions and the QEMU
//! semihosting exit status are the result (`just test-fp-trap`).

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    aarch64_cpu::registers::{Readable, TTBR0_EL1},
    fp_trap_protocol::{TRAPPED_BIT, probe, trapped},
    kickstart::{
        bootstrap::{PoolCapacities, bootstrap_nucleus, retained_init_memory},
        kickstart_init_el2,
    },
    libkicktest::{
        builder::{Builder, ImageArchive, ImageTarget, verify_retained_image},
        component::ttbr,
        keys::{SlotCursor, boot_key},
        loader::verify_component,
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
        objects::{KeyTable, Notification},
    },
};

const BUILDER_ARCHIVE_GUARD: u32 = 0x0A_4C01;
const PROBE_GUARD: u32 = 0x0F_9701;
/// The probe's stack span, clear of its image at the link base.
const STACK_REGION: u64 = 0x3000_0000;
/// Pages in the probe's stack (it also gets a guard page on both sides).
const STACK_PAGES: u32 = 4;
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
        // stack region
        page_tables: 3 + image_tables + 5,
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
    let mut stacks = builder.stack_region(&probe_component, STACK_REGION, &mut slots);
    let stack = builder.user_stack(&mut stacks, STACK_PAGES, &mut slots);
    for guard in [stack.bottom - PAGE, stack.top] {
        assert!(
            find_leaf(probe_ttbr, guard).is_none(),
            "stack guard page at {guard:#x} must be unmapped"
        );
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
    threads::spawn_el0(
        nucleus,
        probe_component.address_space,
        components::PROBE.entry.expect("fp-probe has an entry"),
        stack.top,
        probe_key.to_wire(),
    );
    let bits = NotificationKey::from_key(result_notification)
        .wait(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("probe wait failed: {:?}", error.code()));
    assert_eq!(
        bits, TRAPPED_BIT,
        "an FP/SIMD instruction at EL0 must trap as an FP/SIMD access"
    );
    semi::println!("FP/SIMD instruction trapped at EL0");

    semi::println!("FP/SIMD trapping at EL1t and EL0 passed");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}
