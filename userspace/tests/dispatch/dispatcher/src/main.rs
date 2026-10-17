#![no_std]
#![no_main]

//! Dispatch-test EL0 component: the root dispatcher of the Scheduler. It
//! reads the workers' records from the table mapped for it, runs the workers
//! round-robin with `Scheduler.Dispatch`, and reports what it saw. Its loader
//! passes the address of a page mapped writable in this component only
//! (`x0`): the parameters first (set by the loader, wire keys of the
//! dispatcher's own table), the observations after them. The layout is
//! shared with `dispatch-test`; keep the two in sync.

use {
    core::sync::atomic::{AtomicU64, Ordering},
    libobject::{
        CapError, NotificationKey, RawKey, ThreadKey,
        scheduler::{SchedulerKey, activation_mask, activation_reason},
        scheduler_record::ThreadRecord,
        thread::ThreadState,
    },
};

libuser::entry!(main);

/// Parameter words.
const RECORDS: usize = 0;
const STRIDE: usize = 1;
const SCHEDULER: usize = 2;
/// Signalled when the first phase is over; the test then refills a Time.
const PHASE_ONE: usize = 3;
/// Signalled when the refilled worker has run again.
const PHASE_TWO: usize = 4;
/// Wire keys of the workers' Threads, then their record slots.
const WORKER_KEYS: usize = 5;
const WORKER_SLOTS: usize = 8;
/// Rounds of dispatching per worker in the first phase.
const ROUNDS: usize = 11;
/// Nanoseconds each dispatch may run before the dispatcher is activated.
const LIMIT_NS: usize = 12;

/// Observation words.
const DISPATCHES: usize = 16;
/// Every activation reason seen, `OR`ed.
const REASONS: usize = 17;
/// Dispatches refused with `StaleGeneration` carrying the current generation.
const STALE_REFUSED: usize = 18;
/// Dispatches refused with `InvalidOperation`.
const INVALID_REFUSED: usize = 19;
/// Dispatches of each worker.
const PER_WORKER: usize = 20;
/// The reasons the idle-waiting dispatcher was woken with.
const WAKE_REASONS: usize = 23;
/// Parameter: index of the worker the test refills.
const REFILLED: usize = 13;
/// Phase three parameters: wire keys of the Threads (three waiters, the
/// kicker, the spare to retire, the one never configured), then the record
/// slots of the first five, the Notifications the dispatcher signals itself
/// and the one signalled at the end.
const P3_THREADS: usize = 32;
const P3_SLOTS: usize = 38;
const P3_NOTIFICATIONS: usize = 43;
const PHASE_THREE: usize = 45;
/// Phase three observations: the reasons the kicker's wakeup activated the
/// dispatcher with, the reasons of the call that returned at once for a
/// burst of wakeups, the reasons of the next dispatch, and the refusals.
const P3_WAKE_REASONS: usize = 48;
const P3_BURST_REASONS: usize = 49;
const P3_AFTER_REASONS: usize = 50;
const P3_RETIRED_REFUSED: usize = 51;
const P3_FOREIGN_REFUSED: usize = 52;
const P3_BURST_RUNNABLE: usize = 53;
const WORDS: usize = 64;

const WORKERS: usize = 3;

/// Stop on an unexpected error (`CapError` has no `Debug`).
fn fail(what: &str, error: CapError) -> ! {
    panic!("{what} failed: {:?}", error.code())
}
/// How many times a record read may retry before giving up.
const SNAPSHOT_RETRIES: u32 = 1_000;

/// Phase three's Threads, by index, and a second in nanoseconds.
const WAITERS: [usize; 3] = [0, 1, 2];
const KICKER_INDEX: usize = 3;
const SPARE_INDEX: usize = 4;
const NEVER_CONFIGURED_INDEX: usize = 5;
const SECOND: u64 = 1_000_000_000;

fn main(page: u64) -> ! {
    // SAFETY: the loader maps the page for this component before it starts;
    // only this component writes the observation words.
    let page = unsafe { core::slice::from_raw_parts(page as *const AtomicU64, WORDS) };
    let word = |index: usize| page[index].load(Ordering::Relaxed);
    let records = word(RECORDS) as *const u8;
    let stride = usize::try_from(word(STRIDE)).unwrap_or(usize::MAX);
    let scheduler = SchedulerKey::from_key(RawKey::from_wire(word(SCHEDULER)));
    let limit_ns = word(LIMIT_NS);

    let thread_key = |worker: usize| RawKey::from_wire(word(WORKER_KEYS + worker));
    let record = |worker: usize| {
        let slot = usize::try_from(word(WORKER_SLOTS + worker)).unwrap_or(usize::MAX);
        // SAFETY: the loader maps the shared record table for this component,
        // 64-byte aligned, and the slot lies inside it; records are accessed
        // only through atomics.
        unsafe { ThreadRecord::at(records.add(slot * stride)) }
    };
    let snapshot = |worker: usize| {
        record(worker)
            .try_snapshot(SNAPSHOT_RETRIES)
            .expect("a consistent worker record")
    };
    // Run `worker` until the nucleus activates this dispatcher again.
    let dispatch = |worker: usize| {
        let generation = snapshot(worker).generation;
        let activation = scheduler
            .dispatch(Some(thread_key(worker)), limit_ns, generation)
            .unwrap_or_else(|error| fail("Dispatch of a runnable worker", error));
        page[REASONS].fetch_or(activation.reasons, Ordering::Release);
        page[PER_WORKER + worker].fetch_add(1, Ordering::Release);
        page[DISPATCHES].fetch_add(1, Ordering::Release);
        activation
    };

    // Refusals first, while the workers are stopped and their generations
    // are stable: a stale generation names the current one, a zero limit is
    // invalid.
    let current = snapshot(0).generation;
    match scheduler.dispatch(Some(thread_key(0)), limit_ns, current.wrapping_sub(1)) {
        Err(CapError::StaleGeneration { current: reported }) if reported == current => {
            page[STALE_REFUSED].fetch_add(1, Ordering::Release);
        }
        _ => panic!("stale Dispatch not refused with the current generation"),
    }
    if let Err(CapError::InvalidOperation) = scheduler.dispatch(Some(thread_key(0)), 0, current) {
        page[INVALID_REFUSED].fetch_add(1, Ordering::Release);
    } else {
        panic!("a zero limit was not refused");
    }

    // Round-robin: every worker in turn, skipping one that waits for budget.
    // Workers that never block or yield only stop at the limit; the finite
    // one stops at its exhaustion.
    for round in 0..usize::try_from(word(ROUNDS)).unwrap_or(0) * WORKERS {
        let worker = round % WORKERS;
        if snapshot(worker).awaits_budget() {
            continue;
        }
        dispatch(worker);
    }

    // The test now refills the exhausted worker's Time; waking for that is
    // what `TIME_AVAILABLE` is for.
    scheduler
        .configure_activation(activation_mask::TIME_AVAILABLE)
        .unwrap_or_else(|error| fail("ConfigureActivation", error));
    NotificationKey::from_key(RawKey::from_wire(word(PHASE_ONE)))
        .signal(1)
        .unwrap_or_else(|error| fail("phase one signal", error));
    let woken = scheduler
        .dispatch(None, u64::MAX, 0)
        .unwrap_or_else(|error| fail("idle-waiting Dispatch", error));
    page[WAKE_REASONS].store(woken.reasons, Ordering::Release);
    assert_eq!(
        woken.reasons & activation_reason::EVENT,
        activation_reason::EVENT
    );

    // The refilled worker is runnable again and runs once more.
    let refilled = usize::try_from(word(REFILLED)).unwrap_or(0);
    assert!(!snapshot(refilled).awaits_budget());
    dispatch(refilled);
    NotificationKey::from_key(RawKey::from_wire(word(PHASE_TWO)))
        .signal(1)
        .unwrap_or_else(|error| fail("phase two signal", error));

    phase_three(page, &scheduler, records, stride);
    loop {
        // Nothing left to run; wait for nothing in particular.
        scheduler.dispatch(None, u64::MAX, 0).ok();
    }
}

/// Waiters, a kicker, a burst and refusals, with the mask asking for
/// `THREAD_RUNNABLE`.
fn phase_three(page: &[AtomicU64], scheduler: &SchedulerKey, records: *const u8, stride: usize) {
    let word = |index: usize| page[index].load(Ordering::Relaxed);
    let key = |index: usize| RawKey::from_wire(word(P3_THREADS + index));
    let slot = |index: usize| usize::try_from(word(P3_SLOTS + index)).unwrap_or(usize::MAX);
    let snapshot = |index: usize| {
        // SAFETY: the record table is mapped for this component, 64-byte
        // aligned, and the slot lies inside it; records are read through
        // atomics.
        unsafe { ThreadRecord::at(records.add(slot(index) * stride)) }
            .try_snapshot(SNAPSHOT_RETRIES)
            .expect("a consistent record")
    };
    // Run Thread `index` for up to `limit_ns`.
    let run = |index: usize, limit_ns: u64| {
        let generation = snapshot(index).generation;
        scheduler
            .dispatch(Some(key(index)), limit_ns, generation)
            .unwrap_or_else(|error| fail("phase three Dispatch", error))
    };

    scheduler
        .configure_activation(activation_mask::THREAD_RUNNABLE)
        .unwrap_or_else(|error| fail("ConfigureActivation", error));

    // Each waiter runs until it blocks on its Notification.
    for waiter in WAITERS {
        let activation = run(waiter, SECOND);
        assert_eq!(activation.reasons, activation_reason::BLOCKED);
        assert_eq!(snapshot(waiter).thread_state(), Some(ThreadState::Blocked));
    }

    // Wake preemption: the kicker runs for up to a second, signals the first
    // waiter's Notification, and the wakeup activates the dispatcher at once.
    let kicker_before = snapshot(KICKER_INDEX).generation;
    let activation = run(KICKER_INDEX, 10 * SECOND);
    page[P3_WAKE_REASONS].store(activation.reasons, Ordering::Release);
    assert_eq!(activation.reasons, activation_reason::EVENT);
    assert!(snapshot(KICKER_INDEX).generation > kicker_before);
    assert_eq!(snapshot(0).thread_state(), Some(ThreadState::Runnable));
    // The woken waiter runs and blocks again.
    assert_eq!(run(0, SECOND).reasons, activation_reason::BLOCKED);

    // A burst: waking two waiters while the dispatcher runs raises two
    // events that coalesce into one pending activation, which the next
    // Dispatch returns at once without running the Thread it names.
    for notification in 0..2 {
        NotificationKey::from_key(RawKey::from_wire(word(P3_NOTIFICATIONS + notification)))
            .signal(1)
            .unwrap_or_else(|error| fail("burst signal", error));
    }
    let kicker_before = snapshot(KICKER_INDEX).generation;
    let burst = run(KICKER_INDEX, SECOND);
    page[P3_BURST_REASONS].store(burst.reasons, Ordering::Release);
    assert_eq!(burst.reasons, activation_reason::EVENT);
    assert_eq!(snapshot(KICKER_INDEX).generation, kicker_before);
    let runnable = WAITERS[1..]
        .iter()
        .filter(|&&waiter| snapshot(waiter).thread_state() == Some(ThreadState::Runnable))
        .count();
    page[P3_BURST_RUNNABLE].store(runnable as u64, Ordering::Release);
    assert_eq!(runnable, 2);
    // Both events were one activation: running a waiter now ends with its
    // own block, not with a second pending event.
    for waiter in &WAITERS[1..] {
        let after = run(*waiter, SECOND);
        page[P3_AFTER_REASONS].fetch_or(after.reasons, Ordering::Release);
        assert_eq!(after.reasons, activation_reason::BLOCKED);
    }

    // A retired Thread cannot be dispatched, and its record says why.
    ThreadKey::from_key(key(SPARE_INDEX))
        .retire()
        .unwrap_or_else(|error| fail("Thread.Retire", error));
    assert_eq!(
        snapshot(SPARE_INDEX).thread_state(),
        Some(ThreadState::Dying)
    );
    if scheduler
        .dispatch(Some(key(SPARE_INDEX)), SECOND, 0)
        .is_ok()
    {
        panic!("a retired Thread was dispatched");
    }
    page[P3_RETIRED_REFUSED].store(1, Ordering::Release);

    // A Thread no Scheduler configured is not this Scheduler's to dispatch.
    assert!(matches!(
        scheduler.dispatch(Some(key(NEVER_CONFIGURED_INDEX)), SECOND, 0),
        Err(CapError::InvalidOperation)
    ));
    page[P3_FOREIGN_REFUSED].store(1, Ordering::Release);

    NotificationKey::from_key(RawKey::from_wire(word(PHASE_THREE)))
        .signal(1)
        .unwrap_or_else(|error| fail("phase three signal", error));
}
