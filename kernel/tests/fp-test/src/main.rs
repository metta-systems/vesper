#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! fp-test: FP/SIMD state per `AddressSpace`, end to end.
//!
//! The kernel loads the hard-float `fp-worker` component twice, as workers A
//! and B, each in its own FP `AddressSpace` (image-build's scan set the
//! flag), and the integer-only `fp-probe` once. Then it runs:
//!
//! - a **spinner** in each worker: it fills every V register, FPCR and FPSR
//!   with sentinels of its own and checks them in a loop forever, so only
//!   preemption stops it, and every switch to another Thread must save and
//!   restore its registers;
//! - a **scenario** Thread in worker A, with sentinels of its own:
//!   - a Call into worker B, whose entry must see every register zero, and
//!     after which A must get back `v8..v15`, FPCR and FPSR, with every other
//!     V register zero;
//!   - the same Call into `fp-probe`, whose FP/SIMD instruction must trap
//!     (and be handled by `fp-probe`'s own fault handler);
//!   - a `brk`, handled by worker A's fault handler, which must see every
//!     register zero, after which A must get back every register;
//! - a **checker**, an interruptible `EL1t` Thread, which wakes the boot
//!   Thread once enough ticks have passed with both spinners counting and
//!   the scenarios done.
//!
//! The boot Thread then checks the reports, the Threads' FP counters and
//! that both spinners were stopped by preemption.
//!
//! Runs on the real Kickstart boot path; in-guest assertions and the QEMU
//! semihosting exit status are the result (`just _test-fp`).

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    core::{
        cell::UnsafeCell,
        sync::atomic::{AtomicPtr, AtomicU64, Ordering},
    },
    fp_test_protocol::{
        CHECK_PASSED, FpInit, FpReport, INIT_VA, ROLE_SCENARIOS, ROLE_SPIN, STACK_REGION,
    },
    fp_trap_protocol::{ProbeInit, TRAPPED_BIT},
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
    libexception::arch::aarch64::ExceptionOrigin,
    libimage::ComponentImage,
    libkicktest::threads,
    libobject::{
        KeySlot, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, StackPoolHandle,
        UntypedKey, address_space::AddressSpaceKey,
    },
    libqemu::semihosting as semi,
    nucleus::objects::{
        ArchObjectsImpl, ExecutionContext, Nucleus, Thread, arch_objects::AddressSpaceObject,
        thread::ThreadId,
    },
};

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

const BUILDER_ARCHIVE_GUARD: u32 = 0x0A_4C01;
const WORKER_A_GUARD: u32 = 0x0F_A001;
const WORKER_B_GUARD: u32 = 0x0F_B001;
const PROBE_GUARD: u32 = 0x0F_9701;
/// The workers' instance seeds: every sentinel differs between them.
const SEED_A: u64 = 0x0A0A_0000_0000_0001;
const SEED_B: u64 = 0x0B0B_0000_0000_0002;
/// Pages in each stack (each also gets a guard page on both sides).
const STACK_PAGES: u32 = 4;
/// Fixture boot-table slots start clear of the well-known bootstrap slots.
const FIRST_FIXTURE_SLOT: u32 = 64;
/// Where worker A finds its Invocations of worker B and of `fp-probe`.
const CALL_FP_SLOT: u32 = 5;
const CALL_INTEGER_SLOT: u32 = 6;
/// Ticks the spinners must run across before the checker wakes the boot
/// Thread.
const REQUIRED_TICKS: u64 = 8;
/// The checker gives up after this many ticks.
const TIMEOUT_TICKS: u64 = 2000;
/// Bits the checker signals.
const WAKE_BITS: u64 = 0b1;
/// `ESR_EL1.EC` of a `brk` instruction.
const EC_BRK: u64 = 0x3C;

/// The live Nucleus, for the checker to read the tick count.
static NUCLEUS: AtomicPtr<Nucleus<ArchObjectsImpl>> = AtomicPtr::new(core::ptr::null_mut());
/// The workers' reports, through the kernel's direct map.
static REPORTS: [AtomicPtr<FpReport>; 2] = [
    AtomicPtr::new(core::ptr::null_mut()),
    AtomicPtr::new(core::ptr::null_mut()),
];
/// The wake-up Notification key (wire form), for the checker.
static WAKE_KEY: AtomicU64 = AtomicU64::new(0);
/// The tick count when the Threads were started.
static START_TICKS: AtomicU64 = AtomicU64::new(0);

const STACK_BYTES: usize = 16 * 1024;

/// The checker's stack.
#[repr(C, align(16))]
struct Stack(UnsafeCell<[u8; STACK_BYTES]>);

// SAFETY: used only by the checker Thread, through its SP.
unsafe impl Sync for Stack {}

static CHECKER_STACK: Stack = Stack(UnsafeCell::new([0; STACK_BYTES]));

/// The kernel's tick count. The nucleus updates it under its lock on another
/// path; a volatile read of the word is all the checker needs.
fn ticks() -> u64 {
    let nucleus = NUCLEUS.load(Ordering::Acquire);
    // SAFETY: the Nucleus outlives every fixture Thread and is mapped for EL1.
    unsafe { core::ptr::read_volatile(&raw const (*nucleus).ticks) }
}

/// Worker `index`'s report.
fn report(index: usize) -> &'static FpReport {
    // SAFETY: set before any fixture Thread starts; the page is mapped in the
    // kernel's direct map for the whole test and every field is atomic.
    unsafe { &*REPORTS[index].load(Ordering::Acquire) }
}

/// Wait (without blocking) until both spinners counted across
/// [`REQUIRED_TICKS`] ticks and the scenarios are done, or the time is up;
/// then wake the boot Thread and idle.
extern "C" fn checker() -> ! {
    let start = START_TICKS.load(Ordering::Relaxed);
    loop {
        let elapsed = ticks() - start;
        let ready = elapsed >= REQUIRED_TICKS
            && (0..2).all(|index| report(index).spin_passes.load(Ordering::Relaxed) > 0)
            && report(0).done.load(Ordering::Acquire) == 1;
        if ready || elapsed >= TIMEOUT_TICKS {
            break;
        }
        core::hint::spin_loop();
    }
    let wake = NotificationKey::from_key(RawKey::from_wire(WAKE_KEY.load(Ordering::Acquire)));
    wake.signal(WAKE_BITS)
        .unwrap_or_else(|error| panic!("checker signal failed: {:?}", error.code()));
    loop {
        aarch64_cpu::asm::wfi();
    }
}

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

/// Map a worker's init and report pages, write its init record, and return
/// its report through the kernel's direct map.
fn worker_pages(
    builder: &Builder<'_>,
    worker: &Component,
    init: FpInit,
    slots: &mut SlotCursor,
) -> *mut FpReport {
    let pages = builder.private_pages(worker.address_space_key, worker.l2, INIT_VA, 2, slots);
    write_init(pages, init);
    PhysAddr::new(pages + 0x1000)
        .user_to_kernel()
        .as_mut_ptr::<FpReport>()
}

pub fn run() -> ! {
    semi::println!("fp-test: enabled MMU and dropped to EL1");
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
    // The fixture Threads run on the root Scheduler's dispatcher.
    libkicktest::dispatcher::init(nucleus, &builder, builder_as, boot.root_scheduler_key);

    // ── The components: the scan's flags pick the AddressSpaces ───────────
    assert!(components::WORKER.fp_simd, "fp-worker must be flagged FP");
    assert!(!components::PROBE.fp_simd, "fp-probe must be integer-only");
    let worker_a = load(
        nucleus,
        &builder,
        &components::WORKER,
        WORKER_A_GUARD,
        &mut slots,
    );
    let worker_b = load(
        nucleus,
        &builder,
        &components::WORKER,
        WORKER_B_GUARD,
        &mut slots,
    );
    let probe = load(
        nucleus,
        &builder,
        &components::PROBE,
        PROBE_GUARD,
        &mut slots,
    );
    for (component, fp_simd) in [(&worker_a, true), (&worker_b, true), (&probe, false)] {
        let space = nucleus
            .address_space(component.address_space)
            .expect("component AddressSpace missing");
        assert_eq!(space.fp_simd(), fp_simd);
    }

    // Stacks: a spinner in each worker, the scenario Thread and the fault
    // handler in A, the Call targets in B and the probe, the probe's fault
    // handler.
    let mut stacks_a = builder.stack_region(&worker_a, STACK_REGION, &mut slots);
    let spin_stack_a = builder.user_stack(&mut stacks_a, STACK_PAGES, &mut slots);
    let scenario_stack = builder.user_stack(&mut stacks_a, STACK_PAGES, &mut slots);
    let handler_pool_a = builder.stack_pool(&mut stacks_a, STACK_PAGES, 1, &mut slots);
    let mut stacks_b = builder.stack_region(&worker_b, STACK_REGION, &mut slots);
    let spin_stack_b = builder.user_stack(&mut stacks_b, STACK_PAGES, &mut slots);
    let call_pool_b = builder.stack_pool(&mut stacks_b, STACK_PAGES, 1, &mut slots);
    let mut stacks_probe = builder.stack_region(&probe, STACK_REGION, &mut slots);
    let call_pool_probe = builder.stack_pool(&mut stacks_probe, STACK_PAGES, 1, &mut slots);
    let handler_pool_probe = builder.stack_pool(&mut stacks_probe, STACK_PAGES, 1, &mut slots);

    // Worker A calls B and the probe; A and the probe handle their own faults.
    let table_a = worker_a.table();
    let call_fp = invocation(
        &worker_b,
        &components::WORKER,
        "zero_check_entry",
        &table_a,
        KeySlot(CALL_FP_SLOT),
        call_pool_b,
    );
    let call_integer = invocation(
        &probe,
        &components::PROBE,
        "probe_call_entry",
        &table_a,
        KeySlot(CALL_INTEGER_SLOT),
        call_pool_probe,
    );
    invocation(
        &worker_a,
        &components::WORKER,
        "fault_check_entry",
        &table_a,
        KeySlot::FAULT_HANDLER,
        handler_pool_a,
    );
    invocation(
        &probe,
        &components::PROBE,
        "fault_entry",
        &probe.table(),
        KeySlot::FAULT_HANDLER,
        handler_pool_probe,
    );

    let report_a = worker_pages(
        &builder,
        &worker_a,
        FpInit {
            guard: u64::from(WORKER_A_GUARD),
            size_bits: u64::from(BOOT_TABLE_SIZE_BITS),
            seed: SEED_A,
            call_fp: call_fp.to_wire(),
            call_integer: call_integer.to_wire(),
        },
        &mut slots,
    );
    let report_b = worker_pages(
        &builder,
        &worker_b,
        FpInit {
            guard: u64::from(WORKER_B_GUARD),
            size_bits: u64::from(BOOT_TABLE_SIZE_BITS),
            seed: SEED_B,
            call_fp: 0,
            call_integer: 0,
        },
        &mut slots,
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
    REPORTS[0].store(report_a, Ordering::Release);
    REPORTS[1].store(report_b, Ordering::Release);

    let wake_key = untyped
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
    WAKE_KEY.store(wake_key.to_wire(), Ordering::Release);

    // ── Run ───────────────────────────────────────────────────────────────
    NUCLEUS.store(&raw mut *nucleus, Ordering::Release);
    START_TICKS.store(ticks(), Ordering::Relaxed);
    let entry = components::WORKER.entry.expect("fp-worker has an entry");
    let spinner_a = threads::spawn_el0(
        nucleus,
        &builder,
        worker_a.address_space,
        entry,
        spin_stack_a.top,
        ROLE_SPIN,
    );
    let spinner_b = threads::spawn_el0(
        nucleus,
        &builder,
        worker_b.address_space,
        entry,
        spin_stack_b.top,
        ROLE_SPIN,
    );
    let scenario = threads::spawn_el0(
        nucleus,
        &builder,
        worker_a.address_space,
        entry,
        scenario_stack.top,
        ROLE_SCENARIOS,
    );
    threads::spawn_interruptible(
        nucleus,
        &builder,
        builder_as,
        checker as *const () as u64,
        CHECKER_STACK.0.get() as u64 + STACK_BYTES as u64,
    );
    semi::println!("fp-test: spinners, scenarios and checker started; parking");
    let bits = NotificationKey::from_key(wake_key)
        .await_bits(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("Notification.Await failed: {:?}", error.code()));
    assert_eq!(bits, WAKE_BITS);
    let elapsed = ticks() - START_TICKS.load(Ordering::Relaxed);
    assert!(elapsed < TIMEOUT_TICKS, "timed out after {elapsed} ticks");

    // ── Check: the spinners kept their registers across preemption ────────
    for (name, index, thread) in [("A", 0, spinner_a), ("B", 1, spinner_b)] {
        let passes = report(index).spin_passes.load(Ordering::Relaxed);
        let failure = report(index).spin_failure.load(Ordering::Relaxed);
        let stats = thread_of(nucleus, thread).fp_stats;
        semi::println!(
            "fp-test: spinner {name}: {passes} passes, check code {failure}, FP counters {stats:?}"
        );
        assert_eq!(failure, CHECK_PASSED, "spinner {name} lost a register");
        assert!(passes > 0, "spinner {name} never ran");
        // It never makes a system call: it can only have stopped through a
        // tick taken at EL0, its registers saved and loaded again.
        let ExecutionContext::Preempted { saved } = thread_of(nucleus, thread).context else {
            panic!("spinner {name} is not preempted");
        };
        assert_eq!(saved.origin, ExceptionOrigin::LowerAarch64);
        assert!(stats.fp_saves > 0 && stats.fp_loads > 0);
    }
    semi::println!(
        "fp-test: both FP spinners kept their V registers, FPCR and FPSR across preemption"
    );

    // ── Check: the scenarios ──────────────────────────────────────────────
    let report_a = report(0);
    let read = |word: &AtomicU64| word.load(Ordering::Relaxed);
    assert_eq!(
        report_a.done.load(Ordering::Acquire),
        1,
        "the scenarios never finished"
    );
    let stats = thread_of(nucleus, scenario).fp_stats;
    semi::println!("fp-test: scenario Thread FP counters {stats:?}");
    assert_eq!(
        read(&report_a.call_fp_callee),
        CHECK_PASSED,
        "the FP callee saw state"
    );
    assert_eq!(
        read(&report_a.call_fp),
        CHECK_PASSED,
        "a Call into an FP component lost state"
    );
    semi::println!(
        "fp-test: a Call into an FP component entered it zeroed and preserved v8..v15, FPCR and FPSR"
    );
    assert_eq!(
        read(&report_a.call_integer_callee),
        TRAPPED_BIT,
        "FP/SIMD did not trap in the integer-only callee"
    );
    assert_eq!(
        read(&report_a.call_integer),
        CHECK_PASSED,
        "a Call into an integer-only component lost state"
    );
    semi::println!(
        "fp-test: FP/SIMD trapped in an integer-only callee; the FP caller got v8..v15, FPCR and FPSR back"
    );
    assert_eq!(
        read(&report_a.fault_handler),
        CHECK_PASSED,
        "the fault handler saw state"
    );
    assert_eq!(read(&report_a.fault_syndrome) >> 26, EC_BRK);
    assert_eq!(
        read(&report_a.fault),
        CHECK_PASSED,
        "a handled fault lost state"
    );
    semi::println!(
        "fp-test: a handled fault entered its handler zeroed and resumed with every register"
    );
    assert!(
        stats.preserved_saves >= 2,
        "both Calls save the preserved registers"
    );
    for component in [&worker_a, &worker_b, &probe] {
        let space = nucleus
            .address_space(component.address_space)
            .expect("component AddressSpace missing");
        assert_eq!(space.unhandled_faults(), 0);
    }

    semi::println!("fp-test: ✅ FP/SIMD state per AddressSpace passed");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}

/// The Thread `id`, which the test keeps alive.
fn thread_of(nucleus: &Nucleus<ArchObjectsImpl>, id: ThreadId) -> &Thread {
    nucleus.thread(id).expect("fixture Thread missing")
}
