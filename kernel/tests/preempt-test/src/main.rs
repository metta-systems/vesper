#![no_std]
#![no_main]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! preempt-test: interrupts and the kernel-owned tick end to end.
//!
//! Kickstart selects and loads the privileged interrupt-controller component
//! from the device tree; the nucleus installs it and arms the physical timer.
//! The boot Thread (non-interruptible `EL1t`) then starts:
//!
//! - two **`EL1t` spinners**: interruptible Threads that count forever and
//!   never block or yield;
//! - an **EL0 spinner**: the `preempt-spinner` component, in its own
//!   `AddressSpace`, counting forever in a page mapped for it only;
//! - a **checker**: an interruptible Thread that waits (busy, never blocking)
//!   until every spinner has counted across enough ticks, then signals the
//!   boot Thread's Notification;
//! - **idle** time: when nothing is runnable the kernel waits for an interrupt.
//!
//! On one core, Threads that never yield can all make progress only through
//! preemption. The boot Thread parks on the Notification and checks the
//! counters, the tick count, the clock and the EL0 spinner's preempted
//! context once it is woken.
//!
//! Runs on the real Kickstart boot path; in-guest assertions and the QEMU
//! semihosting exit status are the result (`just test-preempt`).

mod components {
    //! Bundled userspace components (generated from `image.toml`).
    include!(concat!(env!("OUT_DIR"), "/components.rs"));
}

use {
    core::{
        cell::UnsafeCell,
        sync::atomic::{AtomicPtr, AtomicU64, Ordering},
    },
    kickstart::{
        bootstrap::{bootstrap_nucleus, retained_init_memory},
        kickstart_init_el2,
    },
    libaddress::PhysAddr,
    libcomponent_loader::{
        builder::{Builder, ImageArchive, ImageTarget, verify_retained_image},
        component::ttbr,
        keys::BOOT_TABLE_GUARD,
        keys::{SlotCursor, boot_key, boot_slot},
        loader::verify_component,
        paging::image_table_count,
    },
    libexception::arch::aarch64::ExceptionOrigin,
    // `libkicktest` supplies the panic handler.
    libkicktest::{devices, threads},
    libobject::{
        EventCountKey, KeySlot, KeyTableKey, NotificationKey, ObjectType, RawKey, Rights, TimeKey,
        UntypedKey,
        address_space::AddressSpaceKey,
        scheduler::{SchedulerKey, ThreadStart},
        scheduler_record::{
            FLAG_AWAITING_BUDGET, SchedulerRecord, StopReason, ThreadRecord, ThreadSnapshot,
        },
        thread::ThreadState,
        time::INFINITE_BUDGET,
    },
    libqemu::semihosting as semi,
    nucleus::{
        api::key_entry::KeyEntry,
        objects::{ArchObjectsImpl, ExecutionContext, KeyTable, Nucleus, thread::ThreadId},
    },
};

libboot::entry!(boot_main);

fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, run as *const u8 as u64)
}

/// Ticks both spinners must run across before the checker wakes the boot Thread.
const REQUIRED_TICKS: u64 = 8;

/// Boot `KeyTable` slot of the boot Thread's wake-up Notification.
const WAKE_SLOT: u32 = 16;

/// First boot `KeyTable` slot the builder carves fixtures into.
const FIRST_FIXTURE_SLOT: u32 = 64;

/// Guard of the `KeyTable` archiving the builder's image Frames.
const BUILDER_ARCHIVE_GUARD: u32 = 0x0A_4C01;

/// Guard of the EL0 spinner component's `KeyTable`.
const SPINNER_GUARD: u32 = 0x05_7101;

/// Where the EL0 spinner's counter page is mapped, in its `AddressSpace` only.
const COUNTER_VA: u64 = 0x2000_0000;

/// Where the EL0 spinner's guarded stacks are carved.
const STACK_REGION: u64 = 0x3000_0000;

/// Pages of the EL0 spinner's stack.
const STACK_PAGES: u32 = 4;

/// Bits the checker signals.
const WAKE_BITS: u64 = 0b1;

/// Per-spinner progress counters.
static SPINNER_COUNTS: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

/// The live Nucleus, for the checker to read the tick count.
static NUCLEUS: AtomicPtr<Nucleus<ArchObjectsImpl>> = AtomicPtr::new(core::ptr::null_mut());

/// The wake-up Notification key (wire form), for the checker.
static WAKE_KEY: AtomicU64 = AtomicU64::new(0);

/// The EL0 spinner's counter, through the kernel's physical window.
static EL0_COUNT: AtomicPtr<u64> = AtomicPtr::new(core::ptr::null_mut());

/// The budget-limited EL0 spinner's counter, in the next page.
static LIMITED_COUNT: AtomicPtr<u64> = AtomicPtr::new(core::ptr::null_mut());

/// The limited spinner's Time and the boot Time (wire form), and the limited
/// spinner Thread's identity, for the checker's exhaustion scenario.
static LIMITED_TIME_KEY: AtomicU64 = AtomicU64::new(0);
static BOOT_TIME_KEY: AtomicU64 = AtomicU64::new(0);
static LIMITED_THREAD_ADDRESS: AtomicU64 = AtomicU64::new(0);
static LIMITED_THREAD_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Where the scheduler's record table lives (through the kernel's window),
/// and the slots of the two EL0 spinners' records.
static RECORDS: AtomicPtr<u8> = AtomicPtr::new(core::ptr::null_mut());
const UNLIMITED_SLOT: u32 = 1;
const LIMITED_SLOT: u32 = 2;

/// Guard of the EL0 reader component's `KeyTable`.
const READER_GUARD: u32 = 0x05_7102;

/// The EL0 reader's page, through the kernel's window: parameters, then the
/// observations (see `preempt-reader`).
static READER_WORDS: AtomicPtr<u64> = AtomicPtr::new(core::ptr::null_mut());

/// Observation words of the reader page.
const READER_READS: usize = 8;
const READER_NEWEST_GENERATION: usize = 9;
const READER_SAW_AWAITING: usize = 10;
const READER_SAW_REFILLED: usize = 11;
const READER_INCARNATION: usize = 12;

/// Stride of the record table.
const RECORD_STRIDE: u32 = 192;

/// Where the record Frame is mapped in the builder's `AddressSpace`.
const RECORD_VA: u64 = 0x2200_0000;

/// The `EventCount` bound to the root scheduler (wire form).
static EVENT_COUNT_KEY: AtomicU64 = AtomicU64::new(0);

/// The Notification the checker signals when the exhaustion scenario is over.
static DONE_KEY: AtomicU64 = AtomicU64::new(0);

/// Budget the limited spinner's Time starts with (3 ticks of CPU time).
const LIMITED_BUDGET_NS: u64 = 30_000_000;

/// Budget transferred to the limited spinner after it ran out.
const REFILL_NS: u64 = 40_000_000;

/// The checker gives up waiting for an event after this many ticks.
const EVENT_TIMEOUT_TICKS: u64 = 400;

/// The tick count when the Threads were started.
static START_TICKS: AtomicU64 = AtomicU64::new(0);

const STACK_BYTES: usize = 16 * 1024;

/// One execution stack per fixture Thread.
#[repr(C, align(16))]
struct Stack(UnsafeCell<[u8; STACK_BYTES]>);

// SAFETY: each stack is used only by its own Thread, through its SP.
unsafe impl Sync for Stack {}

impl Stack {
    const fn new() -> Self {
        Self(UnsafeCell::new([0; STACK_BYTES]))
    }

    fn top(&self) -> u64 {
        self.0.get() as u64 + STACK_BYTES as u64
    }
}

static STACKS: [Stack; 3] = [Stack::new(), Stack::new(), Stack::new()];

/// The kernel's tick count. The nucleus updates it under its lock on another
/// path; a volatile read of the word is all the checker needs.
fn ticks() -> u64 {
    let nucleus = NUCLEUS.load(Ordering::Acquire);
    // SAFETY: the Nucleus outlives every fixture Thread and is mapped for EL1.
    unsafe { core::ptr::read_volatile(&raw const (*nucleus).ticks) }
}

/// How far the EL0 spinner counted.
fn el0_count() -> u64 {
    // SAFETY: the counter page is mapped in the kernel's physical window and
    // outlives the test; only the EL0 spinner writes it.
    unsafe { core::ptr::read_volatile(EL0_COUNT.load(Ordering::Acquire)) }
}

/// How far the budget-limited EL0 spinner counted.
fn limited_count() -> u64 {
    // SAFETY: as `el0_count`; only the limited spinner writes this word.
    unsafe { core::ptr::read_volatile(LIMITED_COUNT.load(Ordering::Acquire)) }
}

/// A consistent copy of the record in `slot`, read the way a scheduler does.
fn snapshot(slot: u32) -> ThreadSnapshot {
    let base = RECORDS.load(Ordering::Acquire);
    // SAFETY: the table is the record Frame shared in `run`, 64-byte
    // aligned; the nucleus writes records through atomics.
    let record = unsafe { ThreadRecord::at(base.add(slot as usize * RECORD_STRIDE as usize)) };
    record
        .try_snapshot(1_000_000)
        .expect("a consistent Thread record")
}

/// Scheduling events the nucleus published, from the scheduler record.
fn published_events() -> u64 {
    let base = RECORDS.load(Ordering::Acquire);
    // SAFETY: slot 0 of the record Frame, as above.
    let record = unsafe { SchedulerRecord::at(base) };
    record
        .try_snapshot(1_000_000)
        .expect("a consistent scheduler record")
        .events
}

/// An observation word the EL0 reader reported.
fn reader_word(index: usize) -> u64 {
    // SAFETY: the reader's page is mapped in the kernel's window for the
    // whole test; only the reader writes its observation words.
    unsafe {
        READER_WORDS
            .load(Ordering::Acquire)
            .add(index)
            .read_volatile()
    }
}

/// The value of the `EventCount` the nucleus advances.
fn event_count_value() -> u64 {
    EventCountKey::from_key(RawKey::from_wire(EVENT_COUNT_KEY.load(Ordering::Acquire)))
        .read()
        .unwrap_or_else(|error| panic!("EventCount read failed: {:?}", error.code()))
}

/// Spin (without blocking) until `condition` holds, failing the test after
/// [`EVENT_TIMEOUT_TICKS`] ticks.
fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let started = ticks();
    while !condition() {
        assert!(
            ticks() - started < EVENT_TIMEOUT_TICKS,
            "timed out waiting for {what}"
        );
        core::hint::spin_loop();
    }
}

fn spin(counter: &AtomicU64) -> ! {
    loop {
        counter.fetch_add(1, Ordering::Relaxed);
        core::hint::spin_loop();
    }
}

extern "C" fn spinner_a() -> ! {
    spin(&SPINNER_COUNTS[0])
}

extern "C" fn spinner_b() -> ! {
    spin(&SPINNER_COUNTS[1])
}

/// Wait (without blocking) until every spinner counted across
/// [`REQUIRED_TICKS`] ticks, then wake the boot Thread and idle.
extern "C" fn checker() -> ! {
    let start = START_TICKS.load(Ordering::Relaxed);
    while ticks() < start + REQUIRED_TICKS
        || el0_count() == 0
        || SPINNER_COUNTS
            .iter()
            .any(|count| count.load(Ordering::Relaxed) == 0)
    {
        core::hint::spin_loop();
    }
    let wake = NotificationKey::from_key(RawKey::from_wire(WAKE_KEY.load(Ordering::Acquire)));
    wake.signal(WAKE_BITS)
        .unwrap_or_else(|error| panic!("checker signal failed: {:?}", error.code()));
    exhaustion_scenario();
    let done = NotificationKey::from_key(RawKey::from_wire(DONE_KEY.load(Ordering::Acquire)));
    done.signal(WAKE_BITS)
        .unwrap_or_else(|error| panic!("checker done signal failed: {:?}", error.code()));
    loop {
        aarch64_cpu::asm::wfi();
    }
}

/// The budget-limited spinner runs on its Time until it is used up, is
/// switched away from and kept off the run queue, and runs again once a
/// Transfer brings budget. Runs in the interruptible checker, which sees
/// ticks; a failed assertion fails the test.
fn exhaustion_scenario() {
    // SAFETY: the Nucleus outlives every fixture Thread; reads here happen
    // between kernel entries, which complete before this Thread resumes.
    let nucleus = unsafe { &*NUCLEUS.load(Ordering::Acquire) };
    let limited_time =
        TimeKey::from_key(RawKey::from_wire(LIMITED_TIME_KEY.load(Ordering::Acquire)));
    let boot_time = TimeKey::from_key(RawKey::from_wire(BOOT_TIME_KEY.load(Ordering::Acquire)));
    let limited_spinner = ThreadId {
        kind: ObjectType::THREAD,
        address: LIMITED_THREAD_ADDRESS.load(Ordering::Acquire),
        generation: u32::try_from(LIMITED_THREAD_GENERATION.load(Ordering::Acquire))
            .expect("generation fits"),
    };

    wait_until("the limited spinner's Time to run out", || {
        limited_time.query().is_ok_and(|time| time.budget_ns == 0)
    });
    let frozen_at = limited_count();
    assert!(frozen_at > 0, "the limited spinner never ran");
    let limited = nucleus
        .thread(limited_spinner)
        .expect("limited spinner Thread missing");
    assert!(limited.awaiting_budget, "an exhausted Thread must wait");
    assert!(
        matches!(limited.context, ExecutionContext::Preempted { .. }),
        "the exhausted spinner is not preempted: {:?}",
        limited.context
    );
    // Its record shows the same: runnable but awaiting budget, out of
    // budget, stopped for exhaustion, and tied to this Thread incarnation.
    let exhausted = snapshot(LIMITED_SLOT);
    assert_eq!(exhausted.thread_state(), Some(ThreadState::Runnable));
    assert_eq!(exhausted.flags & FLAG_AWAITING_BUDGET, FLAG_AWAITING_BUDGET);
    assert_eq!(exhausted.budget_remaining_ns, 0);
    assert_eq!(exhausted.stop_reason(), Some(StopReason::BudgetExhausted));
    assert!(exhausted.run_time_ns >= LIMITED_BUDGET_NS);
    assert_eq!(exhausted.incarnation, limited_spinner.generation);
    // The unlimited spinner is not waiting for anything.
    let unlimited = snapshot(UNLIMITED_SLOT);
    assert!(!unlimited.awaits_budget());
    assert!(matches!(
        unlimited.thread_state(),
        Some(ThreadState::Running | ThreadState::Runnable)
    ));
    // The EventCount follows the published events; the record is written
    // first, so the count never runs ahead of it.
    let counted = event_count_value();
    let events_before = published_events();
    assert!(counted > 0, "the EventCount never advanced");
    assert!(counted <= events_before);
    let others_before = el0_count();
    let stopped_until = ticks() + 6;
    while ticks() < stopped_until {
        core::hint::spin_loop();
    }
    assert_eq!(limited_count(), frozen_at, "an exhausted Thread ran");
    assert!(
        el0_count() > others_before,
        "the unlimited spinner stopped with it"
    );
    // It ran its whole budget, and the one-shot timer stopped it where the
    // budget ran out: no more than the budget plus the latency of taking the
    // interrupt (a few ms of slack for the emulated timer).
    let ran_ns = nucleus
        .run_time_ns(limited_spinner)
        .expect("limited spinner Thread missing");
    semi::println!("preempt-test: limited spinner ran {ran_ns} ns of {LIMITED_BUDGET_NS} ns");
    assert!(
        ran_ns >= LIMITED_BUDGET_NS,
        "stopped before its budget: {ran_ns}"
    );
    assert!(
        ran_ns <= LIMITED_BUDGET_NS + 5_000_000,
        "ran far past its budget: {ran_ns}"
    );

    // The EL0 reader, reading the record with no system call, sees the
    // Thread wait for budget; only then is the budget refilled.
    wait_until("the EL0 reader to see the exhausted record", || {
        reader_word(READER_SAW_AWAITING) == 1
    });

    // Budget arriving through Time.Transfer queues it again.
    boot_time
        .transfer(&limited_time, REFILL_NS)
        .unwrap_or_else(|error| panic!("refill Transfer failed: {:?}", error.code()));
    wait_until("the refilled spinner to run again", || {
        limited_count() > frozen_at
    });
    // The refill is published: no longer waiting, with its budget shown, as
    // a new state of the same incarnation.
    wait_until(
        "the limited spinner's record to leave the waiting state",
        || !snapshot(LIMITED_SLOT).awaits_budget(),
    );
    let refilled = snapshot(LIMITED_SLOT);
    assert!(refilled.generation > exhausted.generation);
    assert_eq!(refilled.incarnation, exhausted.incarnation);
    assert!(refilled.budget_remaining_ns <= REFILL_NS);
    assert!(published_events() > events_before);
    // The reader then sees the refill, in the same incarnation, with
    // snapshots it read consistently.
    wait_until("the EL0 reader to see the refill", || {
        reader_word(READER_SAW_REFILLED) == 1
    });
    assert!(reader_word(READER_READS) > 0);
    assert!(reader_word(READER_NEWEST_GENERATION) > exhausted.generation);
    assert_eq!(
        reader_word(READER_INCARNATION),
        u64::from(limited_spinner.generation)
    );
    assert!(
        limited_time
            .query()
            .is_ok_and(|time| time.budget_ns <= REFILL_NS),
        "the refilled Time grew"
    );
}

pub fn run() -> ! {
    semi::println!("preempt-test: enabled MMU and dropped to EL1");
    let retained = retained_init_memory();
    let image_tables = image_table_count(&retained);
    let boot = bootstrap_nucleus();
    let (nucleus, keytable_addr, boot_as_id) = (boot.nucleus, boot.keytable_addr, boot.boot_as_id);
    let untyped = UntypedKey::from_key(boot.boot_untyped_key);
    let self_table = KeyTableKey::from_key(boot.self_table_key);
    let builder = Builder {
        untyped: &untyped,
        self_table: &self_table,
        boot_table_addr: keytable_addr,
        retained: &retained,
    };
    let mut slots = SlotCursor::starting_at(FIRST_FIXTURE_SLOT);

    // The builder's own AddressSpace maps the boot image (code, statics and
    // fixture stacks): every Thread here runs in it, so the scheduler can
    // select them with a real root and ASID.
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
    verify_retained_image(&retained, &[ttbr(nucleus, boot_as_id)]);
    AddressSpaceKey::from_key(builder_as_key)
        .activate()
        .unwrap_or_else(|error| panic!("builder Activate failed: {:?}", error.code()));

    let wake_key = untyped
        .retype(
            ObjectType::NOTIFICATION,
            0,
            0,
            1,
            &self_table,
            KeySlot(WAKE_SLOT).0,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("Notification Retype failed: {:?}", error.code()));
    assert_eq!(wake_key.slot(), boot_slot(WAKE_SLOT));
    WAKE_KEY.store(wake_key.to_wire(), Ordering::Release);
    let wake = NotificationKey::from_key(wake_key);
    // The checker signals this one when the exhaustion scenario is over.
    let done_key = untyped
        .retype(
            ObjectType::NOTIFICATION,
            0,
            0,
            1,
            &self_table,
            slots.take(1),
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("done Notification Retype failed: {:?}", error.code()));
    DONE_KEY.store(done_key.to_wire(), Ordering::Release);
    let done = NotificationKey::from_key(done_key);

    // ── The EL0 spinner: its own AddressSpace, image, stack and counter ───
    let spinner = builder.component(SPINNER_GUARD, &mut slots);
    builder.load_component(
        &spinner,
        &components::SPINNER,
        SPINNER_GUARD ^ 0x80_0000,
        &mut slots,
    );
    verify_component(ttbr(nucleus, spinner.address_space), &components::SPINNER);
    assert_eq!(spinner.asid, 2);
    let mut stacks = builder.stack_region(&spinner, STACK_REGION, &mut slots);
    let stack = builder.user_stack(&mut stacks, STACK_PAGES, &mut slots);
    // Two counter pages: the first for the unlimited spinner, the second for
    // the budget-limited one.
    let counter_page = builder.private_pages(
        spinner.address_space_key,
        spinner.l2,
        COUNTER_VA,
        2,
        &mut slots,
    );
    EL0_COUNT.store(
        PhysAddr::new(counter_page)
            .user_to_kernel()
            .as_mut_ptr::<u64>(),
        Ordering::Release,
    );
    LIMITED_COUNT.store(
        PhysAddr::new(counter_page + 0x1000)
            .user_to_kernel()
            .as_mut_ptr::<u64>(),
        Ordering::Release,
    );
    let limited_stack = builder.user_stack(&mut stacks, STACK_PAGES, &mut slots);

    // ── The EL0 reader: a dispatcher's view of the record table ──────
    let reader = builder.component(READER_GUARD, &mut slots);
    builder.load_component(
        &reader,
        &components::READER,
        READER_GUARD ^ 0x80_0000,
        &mut slots,
    );
    verify_component(ttbr(nucleus, reader.address_space), &components::READER);
    assert_eq!(reader.asid, 3);
    let mut reader_stacks = builder.stack_region(&reader, STACK_REGION, &mut slots);
    let reader_stack = builder.user_stack(&mut reader_stacks, STACK_PAGES, &mut slots);
    let reader_page = builder.private_pages(
        reader.address_space_key,
        reader.l2,
        COUNTER_VA,
        1,
        &mut slots,
    );
    // The record table: one Frame, mapped read-only in the reader's
    // AddressSpace, which is where the scheduler reads its records.
    Builder::map_table(builder.carve_tables(slots.take(1), 1), reader.l2, RECORD_VA);
    let record_first = slots.take(1);
    let record_frames = builder.retype_frames(record_first, 1);
    Builder::map_frames(
        record_first,
        record_frames.incarnation(),
        1,
        reader.address_space_key,
        RECORD_VA,
        Rights(Rights::READ),
    );
    let record_frame = boot_key(record_first, record_frames.incarnation());
    RECORDS.store(
        PhysAddr::new(Builder::frame_paddr(record_frame))
            .user_to_kernel()
            .as_mut_ptr::<u8>(),
        Ordering::Release,
    );
    // The reader's page: where the table is mapped, its stride and the slot
    // to watch, then the reader's observations.
    let reader_words = PhysAddr::new(reader_page)
        .user_to_kernel()
        .as_mut_ptr::<u64>();
    READER_WORDS.store(reader_words, Ordering::Release);
    // SAFETY: the page is mapped in the kernel's window and the reader has
    // not started.
    unsafe {
        reader_words.write_volatile(RECORD_VA);
        reader_words.add(1).write_volatile(u64::from(RECORD_STRIDE));
        reader_words.add(2).write_volatile(u64::from(LIMITED_SLOT));
    }

    NUCLEUS.store(&raw mut *nucleus, Ordering::Release);
    START_TICKS.store(ticks(), Ordering::Relaxed);
    let time_before = nucleus.current_time_ns();

    // The fixture Threads run on the root Scheduler's dispatcher.
    libkicktest::dispatcher::init(nucleus, &builder, boot_as_id, boot.root_scheduler_key);
    let entries = [
        spinner_a as *const () as u64,
        spinner_b as *const () as u64,
        checker as *const () as u64,
    ];
    for (entry, stack) in entries.into_iter().zip(&STACKS) {
        threads::spawn_interruptible(nucleus, &builder, boot_as_id, entry, stack.top());
    }
    // Each EL0 spinner goes through the public path: Retype an inert Thread
    // and a Time, fund the Time from the boot Time, then the root Scheduler
    // configures the Thread on it and starts it. The first spinner's Time is
    // infinite; the second's holds a few ticks of CPU time.
    let boot_time = TimeKey::from_key(boot.boot_time_key);
    let mut retype_time = |budget_ns: u64| {
        let key = untyped
            .retype(
                ObjectType::TIME,
                0,
                0,
                1,
                &self_table,
                slots.take(1),
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("Time Retype failed: {:?}", error.code()));
        boot_time
            .transfer(&TimeKey::from_key(key), budget_ns)
            .unwrap_or_else(|error| panic!("Time.Transfer failed: {:?}", error.code()));
        key
    };
    let spinner_time_key = retype_time(INFINITE_BUDGET);
    let limited_time_key = retype_time(LIMITED_BUDGET_NS);
    let reader_time_key = retype_time(INFINITE_BUDGET);
    let limited_time = TimeKey::from_key(limited_time_key);
    LIMITED_TIME_KEY.store(limited_time.to_wire(), Ordering::Release);
    BOOT_TIME_KEY.store(boot_time.to_wire(), Ordering::Release);
    let spinner_thread_slot = slots.take(1);
    let spinner_thread_key = untyped
        .retype(
            ObjectType::THREAD,
            0,
            0,
            1,
            &self_table,
            spinner_thread_slot,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("Thread Retype failed: {:?}", error.code()));
    let root_scheduler = SchedulerKey::from_key(boot.root_scheduler_key);
    // The scheduler's record table is the Frame mapped for the EL0 reader
    // above: share it with the nucleus, and bind the EventCount the nucleus
    // advances whenever it publishes a change to a record.
    root_scheduler
        .share_region(record_frame, 0, RECORD_STRIDE)
        .unwrap_or_else(|error| panic!("ShareRegion failed: {:?}", error.code()));
    let event_count_key = untyped
        .retype(
            ObjectType::EVENT_COUNT,
            0,
            0,
            1,
            &self_table,
            slots.take(1),
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("EventCount Retype failed: {:?}", error.code()));
    EVENT_COUNT_KEY.store(event_count_key.to_wire(), Ordering::Release);
    root_scheduler
        .bind_event_count(event_count_key)
        .unwrap_or_else(|error| panic!("BindEventCount failed: {:?}", error.code()));
    root_scheduler
        .configure_thread(
            spinner_thread_key,
            spinner.address_space_key,
            ThreadStart {
                entry: components::SPINNER.entry.expect("the spinner has an entry"),
                stack_pointer: stack.top,
                argument: COUNTER_VA,
            },
            spinner_time_key,
        )
        .unwrap_or_else(|error| panic!("ConfigureThread failed: {:?}", error.code()));
    root_scheduler
        .start_thread(spinner_thread_key)
        .unwrap_or_else(|error| panic!("StartThread failed: {:?}", error.code()));
    libkicktest::dispatcher::register_key(nucleus, &builder, spinner_thread_key);
    // The budget-limited spinner: the same component and AddressSpace, its
    // own stack and counter page, on the finite Time.
    let limited_thread_key = untyped
        .retype(
            ObjectType::THREAD,
            0,
            0,
            1,
            &self_table,
            slots.take(1),
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("limited Thread Retype failed: {:?}", error.code()));
    root_scheduler
        .configure_thread(
            limited_thread_key,
            spinner.address_space_key,
            ThreadStart {
                entry: components::SPINNER.entry.expect("the spinner has an entry"),
                stack_pointer: limited_stack.top,
                argument: COUNTER_VA + 0x1000,
            },
            limited_time_key,
        )
        .unwrap_or_else(|error| panic!("limited ConfigureThread failed: {:?}", error.code()));
    root_scheduler
        .start_thread(limited_thread_key)
        .unwrap_or_else(|error| panic!("limited StartThread failed: {:?}", error.code()));
    libkicktest::dispatcher::register_key(nucleus, &builder, limited_thread_key);
    // The EL0 reader: the same public path, on an infinite Time.
    let reader_thread_key = untyped
        .retype(
            ObjectType::THREAD,
            0,
            0,
            1,
            &self_table,
            slots.take(1),
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("reader Thread Retype failed: {:?}", error.code()));
    root_scheduler
        .configure_thread(
            reader_thread_key,
            reader.address_space_key,
            ThreadStart {
                entry: components::READER.entry.expect("the reader has an entry"),
                stack_pointer: reader_stack.top,
                argument: COUNTER_VA,
            },
            reader_time_key,
        )
        .unwrap_or_else(|error| panic!("reader ConfigureThread failed: {:?}", error.code()));
    root_scheduler
        .start_thread(reader_thread_key)
        .unwrap_or_else(|error| panic!("reader StartThread failed: {:?}", error.code()));
    libkicktest::dispatcher::register_key(nucleus, &builder, reader_thread_key);
    // The scheduler takes over both EL0 spinners' records.
    root_scheduler
        .assign_record(spinner_thread_key, UNLIMITED_SLOT)
        .unwrap_or_else(|error| panic!("AssignRecord failed: {:?}", error.code()));
    root_scheduler
        .assign_record(limited_thread_key, LIMITED_SLOT)
        .unwrap_or_else(|error| panic!("limited AssignRecord failed: {:?}", error.code()));
    // SAFETY: the boot table is the live carved boot KeyTable; this read-only
    // borrow ends before any capability invocation.
    let el0_spinner = unsafe { &*KeyTable::at_carve(keytable_addr) }
        .lookup(spinner_thread_key, BOOT_TABLE_GUARD)
        .and_then(KeyEntry::thread_id)
        .unwrap_or_else(|error| panic!("spinner Thread key lookup failed: {:?}", error.code()));
    // SAFETY: as above.
    let limited_spinner = unsafe { &*KeyTable::at_carve(keytable_addr) }
        .lookup(limited_thread_key, BOOT_TABLE_GUARD)
        .and_then(KeyEntry::thread_id)
        .unwrap_or_else(|error| panic!("limited Thread key lookup failed: {:?}", error.code()));
    LIMITED_THREAD_ADDRESS.store(limited_spinner.address, Ordering::Release);
    LIMITED_THREAD_GENERATION.store(u64::from(limited_spinner.generation), Ordering::Release);
    // Kickstart withheld the interrupt controllers, the local interrupt
    // block included, from the device memory it handed out.
    devices::assert_device_untypeds_avoid_the_kernel(keytable_addr);
    semi::println!("preempt-test: spinners and checker started; parking");

    // Park until the checker wakes us: only preemption lets it run.
    let bits = wake
        .wait(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("Notification.Wait failed: {:?}", error.code()));
    assert_eq!(bits, WAKE_BITS);

    let elapsed_ticks = ticks() - START_TICKS.load(Ordering::Relaxed);
    let counts = SPINNER_COUNTS
        .each_ref()
        .map(|count| count.load(Ordering::Relaxed));
    let el0_counted = el0_count();
    let time_after = nucleus.current_time_ns();
    semi::println!(
        "preempt-test: woken after {elapsed_ticks} ticks, {} ms; EL1t spinner counts {counts:?}, EL0 spinner count {el0_counted}",
        (time_after - time_before) / 1_000_000
    );
    assert!(
        elapsed_ticks >= REQUIRED_TICKS,
        "too few ticks: {elapsed_ticks}"
    );
    assert!(
        counts.iter().all(|&count| count > 0),
        "a spinner never ran: {counts:?}"
    );
    assert!(time_after > time_before, "the clock did not advance");
    // At least the required ticks of real time passed (10 ms each).
    assert!(
        time_after - time_before >= REQUIRED_TICKS * 10_000_000,
        "the clock ran slower than the ticks"
    );
    assert!(el0_counted > 0, "the EL0 spinner never ran");

    // The EL0 spinner never blocks: it can only have stopped through a tick
    // taken at EL0, its complete user state saved for the next slice.
    let el0_thread = nucleus
        .thread(el0_spinner)
        .expect("EL0 spinner Thread missing");
    let ExecutionContext::Preempted { saved } = el0_thread.context else {
        panic!("the EL0 spinner is not preempted: {:?}", el0_thread.context);
    };
    assert_eq!(saved.origin, ExceptionOrigin::LowerAarch64);
    assert_eq!(saved.spsr_el1 & 0xf, 0, "the spinner must run as EL0t");
    assert_eq!(
        saved.gpr[0], COUNTER_VA,
        "the spinner's counter pointer survives"
    );
    assert_eq!(el0_thread.address_space, spinner.address_space);

    // The checker now runs the budget-exhaustion scenario (it must be
    // interruptible to see ticks); wait for it to finish.
    let done_bits = done
        .wait(NotificationKey::WAIT_INFINITE)
        .unwrap_or_else(|error| panic!("done Notification.Wait failed: {:?}", error.code()));
    assert_eq!(done_bits, WAKE_BITS);

    semi::println!("preempt-test: ✅ preemption, tick, clock and budget checks passed");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            libcpu::endless_sleep()
        }
    }
}
