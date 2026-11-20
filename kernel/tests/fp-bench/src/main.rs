#![no_std]
#![no_main]
// `libqemu::semihosting::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! fp-bench: what FP/SIMD state per `AddressSpace` costs, on the board.
//!
//! The kernel loads `fp-bench-worker` twice, each instance in its own FP
//! `AddressSpace`, and the integer-only `fp-probe` once, then runs the
//! benchmark in the first instance: Doom's vertex projection over E1M1, in
//! chunks, with nothing, a `Notification.Signal`, a Call into `fp-probe` or a
//! Call into the second instance per chunk. Both Call targets Return at
//! once. The boot Thread waits for the report and prints, on the board's
//! UART (semihosting under QEMU):
//!
//! - each variant's fastest and slowest run, and what one chunk's system
//!   call adds over the compute-only variant;
//! - the benchmark Thread's FP counters;
//! - the nucleus-wide FP totals from the root Scheduler's record, the entry
//!   guard's `FPEN` toggles among them.
//!
//! Built without and with the nucleus's `fp_entry_trap` guard, the two runs
//! are the two profiles (`just boot-fp-bench`, `just boot-fp-bench
//! fp_entry_trap`). `just _test-fp-bench` runs a short version in QEMU.

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    aarch64_cpu::registers::{CNTFRQ_EL0, Read},
    core::sync::atomic::Ordering,
    fp_test_protocol::{
        BENCH_COMPUTE, BENCH_NAMES, BENCH_VARIANTS, BenchInit, BenchReport, INIT_VA, STACK_REGION,
    },
    fp_trap_protocol::ProbeInit,
    kickstart::{
        bootstrap::{bootstrap_nucleus, retained_init_memory},
        kickstart_init_el2,
    },
    libaddress::PhysAddr,
    libcomponent_loader::{
        builder::{Builder, ImageArchive, ImageTarget, verify_retained_image},
        component::{Component, ttbr},
        keys::{BOOT_TABLE_SIZE_BITS, SlotCursor, boot_key},
        loader::{verify_component, write_init},
        paging::image_table_count,
    },
    libimage::ComponentImage,
    libkicktest::threads,
    libobject::{
        KeySlot, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, StackPoolHandle,
        UntypedKey, address_space::AddressSpaceKey, scheduler::SchedulerKey,
        scheduler_record::SchedulerRecord,
    },
    nucleus::objects::{ArchObjectsImpl, KeyTable, Nucleus, arch_objects::AddressSpaceObject},
};

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

/// Print a report line: on semihosting under QEMU, on the board's UART
/// otherwise.
macro_rules! report_line {
    ($($argument:tt)+) => {{
        #[cfg(feature = "qemu")]
        libqemu::semihosting::println!($($argument)+);
        #[cfg(not(feature = "qemu"))]
        kickstart::early_console::print_line(format_args!($($argument)+));
    }};
}

const BUILDER_ARCHIVE_GUARD: u32 = 0x0A_4C01;
const RUNNER_GUARD: u32 = 0x0F_B501;
const TARGET_GUARD: u32 = 0x0F_B502;
const PROBE_GUARD: u32 = 0x0F_9701;
/// Pages in each stack (each also gets a guard page on both sides).
const STACK_PAGES: u32 = 4;
/// Fixture boot-table slots start clear of the well-known bootstrap slots.
const FIRST_FIXTURE_SLOT: u32 = 64;
/// Where the runner finds its keys.
const BUSY_SLOT: u32 = 5;
const DONE_SLOT: u32 = 6;
const CALL_INTEGER_SLOT: u32 = 7;
const CALL_FP_SLOT: u32 = 8;
/// Frames per run: a short run under QEMU, about a second per variant on the
/// board.
const FRAMES: u64 = if cfg!(feature = "qemu") { 20 } else { 10_000 };
/// Stride of the scheduler's record table, and where its Frame is mapped in
/// the builder's `AddressSpace`.
const RECORD_STRIDE: u32 = 192;
const RECORD_VA: u64 = 0x2200_0000;

/// Provision `image` as a component with `guard`, load and verify it.
fn load(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    builder: &Builder<'_>,
    image: &ComponentImage,
    guard: u32,
    slots: &mut SlotCursor,
) -> Component {
    let component = builder.component(image, guard, slots);
    builder.load_component(&component, image, guard ^ 0x80_0000, slots);
    verify_component(ttbr(nucleus, component.address_space), image);
    component
}

/// Create the Invocation of `export` of `image` in `target`, on `pool`, and
/// install it into `destination` at `slot`.
fn invocation(
    target: &Component,
    image: &ComponentImage,
    export: &str,
    destination: &KeyTableKey,
    slot: KeySlot,
    pool: StackPoolHandle,
) -> RawKey {
    let entry = image
        .export(export)
        .unwrap_or_else(|| panic!("no export `{export}`"));
    AddressSpaceKey::from_key(target.address_space_key)
        .create_invocation(entry, destination, slot, pool)
        .unwrap_or_else(|error| panic!("CreateInvocation `{export}` failed: {:?}", error.code()))
}

/// A new Notification in the boot table, also installed in `component`'s
/// table at `slot`; returns both keys.
fn notification(
    builder: &Builder<'_>,
    component: &Component,
    slot: u32,
    slots: &mut SlotCursor,
) -> (RawKey, RawKey) {
    let key = builder
        .untyped
        .retype(
            ObjectType::NOTIFICATION,
            0,
            0,
            1,
            builder.self_table,
            slots.take(1),
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("Notification Retype failed: {:?}", error.code()));
    // Notification is off the `CopyDerive` allowlist, so the builder installs
    // the component's capability kernel-privately.
    // SAFETY: the component's retained initialized table, exclusively
    // borrowed for this bootstrap grant; no capability invocation overlaps it.
    let installed = unsafe { &mut *KeyTable::at_carve(component.table_addr) }
        .insert(KeySlot(slot), builder.entry(key), component.guard)
        .unwrap_or_else(|failure| panic!("Notification grant failed: {:?}", failure.error.code()));
    (key, installed)
}

/// `ticks` of the counter at `frequency`, in picoseconds.
fn picoseconds(ticks: u64, frequency: u64) -> u64 {
    u64::try_from(u128::from(ticks) * 1_000_000_000_000 / u128::from(frequency.max(1)))
        .unwrap_or(u64::MAX)
}

/// Picoseconds as nanoseconds with three decimals.
struct Nanoseconds(u64);

impl core::fmt::Display for Nanoseconds {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{}.{:03} ns", self.0 / 1000, self.0 % 1000)
    }
}

pub fn run() -> ! {
    report_line!("fp-bench: enabled MMU and dropped to EL1");
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
    verify_retained_image(&retained, &[ttbr(nucleus, builder_as)]);
    AddressSpaceKey::from_key(builder_as_key)
        .activate()
        .unwrap_or_else(|error| panic!("builder Activate failed: {:?}", error.code()));
    libkicktest::dispatcher::init(nucleus, &builder, builder_as, boot.root_scheduler_key);

    // The root Scheduler's record table, for the nucleus-wide FP totals: one
    // Frame, mapped read-only in the builder's AddressSpace.
    Builder::map_table(
        builder.carve_tables(slots.take(1), 1),
        builder_l2,
        RECORD_VA,
    );
    let record_first = slots.take(1);
    let record_frames = builder.retype_frames(record_first, 1);
    Builder::map_frames(
        record_first,
        record_frames.incarnation(),
        1,
        builder_as_key,
        RECORD_VA,
        Rights(Rights::READ),
    );
    let record_frame = boot_key(record_first, record_frames.incarnation());
    SchedulerKey::from_key(boot.root_scheduler_key)
        .share_region(record_frame, 0, RECORD_STRIDE)
        .unwrap_or_else(|error| panic!("ShareRegion failed: {:?}", error.code()));
    let records = PhysAddr::new(Builder::frame_paddr(record_frame))
        .user_to_kernel()
        .as_mut_ptr::<u8>();

    // ── The components ────────────────────────────────────────────────────
    assert!(
        components::BENCH.fp_simd,
        "fp-bench-worker must be flagged FP"
    );
    assert!(!components::PROBE.fp_simd, "fp-probe must be integer-only");
    let runner = load(
        nucleus,
        &builder,
        &components::BENCH,
        RUNNER_GUARD,
        &mut slots,
    );
    let target = load(
        nucleus,
        &builder,
        &components::BENCH,
        TARGET_GUARD,
        &mut slots,
    );
    let probe = load(
        nucleus,
        &builder,
        &components::PROBE,
        PROBE_GUARD,
        &mut slots,
    );
    for (component, fp_simd) in [(&runner, true), (&target, true), (&probe, false)] {
        let space = nucleus
            .address_space(component.address_space)
            .expect("component AddressSpace missing");
        assert_eq!(space.fp_simd(), fp_simd);
    }
    let mut runner_stacks = builder.stack_region(&runner, STACK_REGION, &mut slots);
    let runner_stack = builder.user_stack(&mut runner_stacks, STACK_PAGES, &mut slots);
    let mut target_stacks = builder.stack_region(&target, STACK_REGION, &mut slots);
    let target_pool = builder.stack_pool(&mut target_stacks, STACK_PAGES, 1, &mut slots);
    let mut probe_stacks = builder.stack_region(&probe, STACK_REGION, &mut slots);
    let probe_pool = builder.stack_pool(&mut probe_stacks, STACK_PAGES, 1, &mut slots);

    let runner_table = runner.table();
    let call_integer = invocation(
        &probe,
        &components::PROBE,
        "nop_entry",
        &runner_table,
        KeySlot(CALL_INTEGER_SLOT),
        probe_pool,
    );
    let call_fp = invocation(
        &target,
        &components::BENCH,
        "nop_entry",
        &runner_table,
        KeySlot(CALL_FP_SLOT),
        target_pool,
    );
    let (_, busy) = notification(&builder, &runner, BUSY_SLOT, &mut slots);
    let (done_key, done) = notification(&builder, &runner, DONE_SLOT, &mut slots);

    let runner_pages =
        builder.private_pages(runner.address_space_key, runner.l2, INIT_VA, 2, &mut slots);
    write_init(
        runner_pages,
        BenchInit {
            guard: u64::from(RUNNER_GUARD),
            size_bits: u64::from(BOOT_TABLE_SIZE_BITS),
            frames: FRAMES,
            busy: busy.to_wire(),
            done: done.to_wire(),
            call_integer: call_integer.to_wire(),
            call_fp: call_fp.to_wire(),
        },
    );
    let target_pages =
        builder.private_pages(target.address_space_key, target.l2, INIT_VA, 2, &mut slots);
    write_init(
        target_pages,
        BenchInit {
            guard: u64::from(TARGET_GUARD),
            size_bits: u64::from(BOOT_TABLE_SIZE_BITS),
            frames: 0,
            busy: 0,
            done: 0,
            call_integer: 0,
            call_fp: 0,
        },
    );
    let probe_init = builder.private_pages(
        probe.address_space_key,
        probe.l2,
        fp_trap_protocol::INIT_VA,
        1,
        &mut slots,
    );
    write_init(
        probe_init,
        ProbeInit {
            result: 0,
            guard: u64::from(PROBE_GUARD),
            size_bits: u64::from(BOOT_TABLE_SIZE_BITS),
        },
    );
    // SAFETY: the runner's report page, through the kernel's direct map for
    // the whole run; every field is atomic.
    let report = unsafe {
        &*PhysAddr::new(runner_pages + 0x1000)
            .user_to_kernel()
            .as_ptr::<BenchReport>()
    };

    // ── Run ───────────────────────────────────────────────────────────────
    let guard = if cfg!(feature = "fp_entry_trap") {
        "with the entry guard"
    } else {
        "without the entry guard"
    };
    report_line!("fp-bench: profile {guard}, {FRAMES} frames per run; running");
    let bench_thread = threads::spawn_el0(
        nucleus,
        &builder,
        runner.address_space,
        components::BENCH
            .entry
            .expect("fp-bench-worker has an entry"),
        runner_stack.top,
        0,
    );
    NotificationKey::from_key(done_key)
        .await_bits(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("Notification.Await failed: {:?}", error.code()));
    assert_eq!(report.done.load(Ordering::Acquire), 1);

    // ── Report ────────────────────────────────────────────────────────────
    let frequency = CNTFRQ_EL0.get();
    let vertices = report.vertices.load(Ordering::Relaxed);
    let chunks = report.chunks.load(Ordering::Relaxed);
    report_line!(
        "fp-bench: {vertices} vertices and {chunks} chunks per run, counter at {frequency} Hz"
    );
    let checksum = report.checksum[BENCH_COMPUTE].load(Ordering::Relaxed);
    let fastest = |variant: usize| {
        report.ticks[variant]
            .iter()
            .map(|ticks| ticks.load(Ordering::Relaxed))
            .min()
            .unwrap_or(0)
    };
    let compute = picoseconds(fastest(BENCH_COMPUTE), frequency);
    for variant in 0..BENCH_VARIANTS {
        assert_eq!(
            report.checksum[variant].load(Ordering::Relaxed),
            checksum,
            "{} computed something else",
            BENCH_NAMES[variant]
        );
        let slowest = report.ticks[variant]
            .iter()
            .map(|ticks| ticks.load(Ordering::Relaxed))
            .max()
            .unwrap_or(0);
        let best = picoseconds(fastest(variant), frequency);
        if variant == BENCH_COMPUTE {
            report_line!(
                "fp-bench: {}: fastest {}, slowest {}, {} per vertex",
                BENCH_NAMES[variant],
                Nanoseconds(best),
                Nanoseconds(picoseconds(slowest, frequency)),
                Nanoseconds(best / vertices.max(1)),
            );
        } else {
            report_line!(
                "fp-bench: {}: fastest {}, slowest {}, {} per call over compute only",
                BENCH_NAMES[variant],
                Nanoseconds(best),
                Nanoseconds(picoseconds(slowest, frequency)),
                Nanoseconds(best.saturating_sub(compute) / chunks.max(1)),
            );
        }
    }
    let stats = nucleus
        .thread(bench_thread)
        .expect("benchmark Thread missing")
        .fp_stats;
    report_line!("fp-bench: benchmark Thread FP counters {stats:?}");
    // SAFETY: slot 0 of the shared record table, through the kernel's
    // direct map; the nucleus writes it through atomics.
    let totals = unsafe { SchedulerRecord::at(records) }
        .try_snapshot(1_000_000)
        .expect("a consistent scheduler record")
        .fp;
    report_line!("fp-bench: nucleus FP totals {totals:?}");
    report_line!("fp-bench: ✅ done ({guard})");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}
