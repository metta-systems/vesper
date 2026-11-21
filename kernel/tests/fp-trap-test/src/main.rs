#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! fp-trap-test: FP/SIMD access follows the `AddressSpace`, enforced by
//! hardware.
//!
//! The boot path (`libboot`, Kickstart) configures `CPTR_EL2`/`CPACR_EL1` so
//! FP/SIMD instructions trap to EL1 from EL1 and EL0, and the nucleus keeps
//! them trapped for every integer-only `AddressSpace`. This kernel executes
//! one FP/SIMD instruction (see `fp-trap-protocol`):
//!
//! - in the trusted `EL1t` boot Thread, where faults halt the kernel, so the
//!   nucleus is built with its test-only `fp_trap_test` hook that hands the
//!   syndrome back instead;
//! - in the EL0 `fp-probe` component, in its own integer-only
//!   `AddressSpace`, where the trap is delivered to the component's own fault
//!   handler (installed at `KeySlot::FAULT_HANDLER`), which records the
//!   syndrome and skips the instruction;
//! - in a second copy of `fp-probe`, in an FP `AddressSpace`, where it must
//!   execute; that probe then has the nucleus execute it at EL1 while serving
//!   the FP frame, where it traps with the `fp_entry_trap` guard and executes
//!   without it.
//!
//! Each trap must be `ESR_EL1.EC` 0x07 — an execution fault, not an
//! Invocation error and never an automatic enable. `just _test-fp-trap` runs
//! the kernel without the guard and `just _test-fp-entry-trap` with it.
//!
//! Runs on the real Kickstart boot path; in-guest assertions and the QEMU
//! semihosting exit status are the result.

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    aarch64_cpu::registers::{Read, TTBR0_EL1},
    fp_trap_protocol::{
        INIT_VA, KERNEL_NOT_TRAPPED_BIT, KERNEL_TRAPPED_BIT, NOT_TRAPPED_BIT, ProbeInit, ROLE_FP,
        ROLE_INTEGER, STACK_REGION, TRAPPED_BIT, probe, trapped,
    },
    kickstart::{
        bootstrap::{bootstrap_nucleus, retained_init_memory},
        kickstart_init_el2,
    },
    libcomponent_loader::{
        builder::{Builder, ImageArchive, ImageTarget, verify_retained_image},
        component::ttbr,
        keys::{BOOT_TABLE_SIZE_BITS, SlotCursor, boot_key},
        loader::{verify_component, write_init},
        paging::{PAGE, find_leaf, image_table_count},
    },
    libkicktest::threads,
    libobject::{
        KeySlot, KeyTableKey, NotificationKey, ObjectType, Rights, UntypedKey,
        address_space::AddressSpaceKey,
    },
    libqemu::semihosting as semi,
    nucleus::objects::{ArchObjectsImpl, KeyTable, Nucleus, arch_objects::AddressSpaceObject},
};

const BUILDER_ARCHIVE_GUARD: u32 = 0x0A_4C01;
const PROBE_GUARD: u32 = 0x0F_9701;
const FP_PROBE_GUARD: u32 = 0x0F_9702;
/// Pages in each probe stack (each also gets a guard page on both sides).
const STACK_PAGES: u32 = 4;
/// Headroom the fault handler's Invocation requires below its SP.
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
    let boot = bootstrap_nucleus();
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
    // The fixture Threads run on the root Scheduler's dispatcher.
    libkicktest::dispatcher::init(nucleus, &builder, builder_as, boot.root_scheduler_key);

    let bits = run_probe(nucleus, &builder, PROBE_GUARD, false, &mut slots);
    assert_eq!(
        bits, TRAPPED_BIT,
        "an FP/SIMD instruction at EL0 must trap as an FP/SIMD access"
    );
    semi::println!("FP/SIMD instruction trapped at EL0 and was handled by its component");

    // ── EL0 in an FP AddressSpace: the instruction executes ───────────────
    let bits = run_probe(nucleus, &builder, FP_PROBE_GUARD, true, &mut slots);
    assert_eq!(
        bits & (TRAPPED_BIT | NOT_TRAPPED_BIT),
        NOT_TRAPPED_BIT,
        "an FP/SIMD instruction in an FP AddressSpace must execute"
    );
    semi::println!("FP/SIMD instruction executed at EL0 in an FP AddressSpace");
    // Serving that FP frame, the kernel runs with FP/SIMD trapped only with
    // the entry guard.
    let kernel_bits = bits & (KERNEL_TRAPPED_BIT | KERNEL_NOT_TRAPPED_BIT);
    if cfg!(feature = "fp_entry_trap") {
        assert_eq!(
            kernel_bits, KERNEL_TRAPPED_BIT,
            "with the entry guard, kernel FP/SIMD use must trap at EL1"
        );
        semi::println!("FP/SIMD instruction trapped at EL1 serving an FP frame (entry guard)");
    } else {
        assert_eq!(
            kernel_bits, KERNEL_NOT_TRAPPED_BIT,
            "without the entry guard, the kernel runs with the FP frame's access"
        );
        semi::println!("FP/SIMD instruction executed at EL1 serving an FP frame (no entry guard)");
    }

    semi::println!("FP/SIMD trapping at EL1t and EL0 passed");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}

/// Load the probe component into a new `AddressSpace` (an FP one with
/// `fp_simd`), with its fault handler, run it in the role that matches, and
/// return the bits it signalled once it has checked that nothing of a fault
/// is left over.
fn run_probe(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    builder: &Builder<'_>,
    guard: u32,
    fp_simd: bool,
    slots: &mut SlotCursor,
) -> u64 {
    let untyped = builder.untyped;
    let self_table = builder.self_table;
    let probe_component = builder.component_with_fp(guard, fp_simd, slots);
    builder.load_component(
        &probe_component,
        &components::PROBE,
        guard ^ 0x80_0000,
        slots,
    );
    let probe_ttbr = ttbr(nucleus, probe_component.address_space);
    verify_component(probe_ttbr, &components::PROBE);
    // The probe's main stack and its fault handler's stack, each guarded.
    let mut stacks = builder.stack_region(&probe_component, STACK_REGION, slots);
    let main_stack = builder.user_stack(&mut stacks, STACK_PAGES, slots);
    let handler_pool = builder.fault_handler_pool(&mut stacks, STACK_PAGES, 1, slots);
    for stack in [main_stack] {
        for guard_page in [stack.bottom - PAGE, stack.top] {
            assert!(
                find_leaf(probe_ttbr, guard_page).is_none(),
                "stack guard page at {guard_page:#x} must be unmapped"
            );
        }
    }

    let result_notification = untyped
        .retype(
            ObjectType::NOTIFICATION,
            0,
            0,
            1,
            self_table,
            slots.take(1),
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("Notification Retype failed: {:?}", error.code()));
    // Notification is off the `CopyDerive` allowlist, so the builder installs
    // the probe's capability kernel-privately.
    // SAFETY: the probe's retained initialized table, exclusively borrowed for
    // this bootstrap grant; no capability invocation overlaps it.
    let probe_key = unsafe { &mut *KeyTable::at_carve(probe_component.table_addr) }
        .insert(
            KeySlot(PROBE_NOTIFICATION_SLOT),
            builder.entry(result_notification),
            probe_component.guard,
        )
        .unwrap_or_else(|failure| panic!("Notification grant failed: {:?}", failure.error.code()));
    let init_page = builder.private_pages(
        probe_component.address_space_key,
        probe_component.l2,
        INIT_VA,
        1,
        slots,
    );
    write_init(
        init_page,
        ProbeInit {
            result: probe_key.to_wire(),
            guard: u64::from(guard),
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
            handler_pool,
        )
        .unwrap_or_else(|error| {
            panic!("fault handler CreateInvocation failed: {:?}", error.code())
        });

    let probe_thread = threads::spawn_el0(
        nucleus,
        builder,
        probe_component.address_space,
        components::PROBE.entry.expect("fp-probe has an entry"),
        main_stack.top,
        if fp_simd { ROLE_FP } else { ROLE_INTEGER },
    );
    let bits = NotificationKey::from_key(result_notification)
        .await_bits(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("probe wait failed: {:?}", error.code()));
    // Any fault was handled (skipped and Returned): nothing is left of it,
    // and nothing went unhandled.
    let thread = nucleus.thread(probe_thread).expect("probe Thread missing");
    assert_eq!(thread.fault, None, "a handled fault must be released");
    assert_eq!(thread.address_space, probe_component.address_space);
    assert_eq!(thread.invocation_stack.len(), 0);
    let space = nucleus
        .address_space(probe_component.address_space)
        .expect("probe AddressSpace missing");
    assert_eq!(space.fp_simd(), fp_simd);
    assert!(!space.fault_handler_busy());
    assert_eq!(space.unhandled_faults(), 0);
    bits
}
