#![no_std]
#![no_main]

//! Dispatch-test EL0 component: the dispatcher of a child Scheduler. Its
//! parent dispatches it with a limit, the grant; it runs one Thread inside
//! the grant and reports what it saw. Its loader passes the address of a page
//! mapped writable in this component only (`x0`): parameters first, then
//! observations. The layout is shared with `dispatch-test`; keep the two in
//! sync.

use {
    core::sync::atomic::{AtomicU64, Ordering},
    libobject::{
        CapError, RawKey,
        scheduler::{SchedulerKey, activation_reason},
        scheduler_record::{SchedulerRecord, ThreadRecord},
    },
};

libuser::entry!(main);

/// Parameter words: where the child's record table is mapped, its stride,
/// the child Scheduler's key, the Thread's key and its record slot.
const RECORDS: usize = 0;
const STRIDE: usize = 1;
const SCHEDULER: usize = 2;
const THREAD: usize = 3;
const THREAD_SLOT: usize = 4;

/// Observation words.
/// The end of the first grant, as the Scheduler record showed it.
const GRANT_ONE_END_NS: usize = 8;
/// What the first `Dispatch` returned when the parent dispatched this
/// dispatcher again after that grant ended.
const FIRST_RESUME_REASONS: usize = 9;
/// The end of the second grant.
const GRANT_TWO_END_NS: usize = 10;
/// How many short dispatches ended at their own limit inside the second
/// grant, and the reasons they returned, `OR`ed.
const INNER_LIMITS: usize = 11;
const INNER_REASONS: usize = 12;
const WORDS: usize = 16;

const MILLISECOND: u64 = 1_000_000;
const SECOND: u64 = 1_000 * MILLISECOND;
const SNAPSHOT_RETRIES: u32 = 1_000;

/// Stop on an unexpected error (`CapError` has no `Debug`).
fn fail(what: &str, error: CapError) -> ! {
    panic!("dispatch child: {what} failed: {:?}", error.code())
}

fn main(page: u64) -> ! {
    // SAFETY: the loader maps the page for this component before it starts;
    // only this component writes the observation words.
    let page = unsafe { core::slice::from_raw_parts(page as *const AtomicU64, WORDS) };
    let word = |index: usize| page[index].load(Ordering::Relaxed);
    let records = word(RECORDS) as *const u8;
    let stride = usize::try_from(word(STRIDE)).unwrap_or(usize::MAX);
    let scheduler = SchedulerKey::from_key(RawKey::from_wire(word(SCHEDULER)));
    let thread = RawKey::from_wire(word(THREAD));
    let slot = usize::try_from(word(THREAD_SLOT)).unwrap_or(usize::MAX);
    // SAFETY: the loader maps the child's record table for this component,
    // 64-byte aligned; records are read through atomics.
    let scheduler_record = unsafe { SchedulerRecord::at(records) };
    // SAFETY: as above; the slot lies inside the table.
    let thread_record = unsafe { ThreadRecord::at(records.add(slot * stride)) };
    let generation = || {
        thread_record
            .try_snapshot(SNAPSHOT_RETRIES)
            .expect("a consistent record")
            .generation
    };
    let grant_end = || {
        scheduler_record
            .try_snapshot(SNAPSHOT_RETRIES)
            .expect("a consistent scheduler record")
            .grant_end_ns
    };

    // The parent's grant is in force and shows in the record.
    let first_end = grant_end();
    assert_ne!(first_end, 0, "no grant shows in the record");
    page[GRANT_ONE_END_NS].store(first_end, Ordering::Release);

    // Run the Thread for ten seconds: far longer than the grant. The
    // parent's limit stops it, the parent is activated, and this call
    // returns only when the parent dispatches this dispatcher again.
    let resumed = scheduler
        .dispatch(Some(thread), 10 * SECOND, generation())
        .unwrap_or_else(|error| fail("Dispatch (long)", error));
    page[FIRST_RESUME_REASONS].store(resumed.reasons, Ordering::Release);

    // A new grant: later than the first.
    let second_end = grant_end();
    assert!(second_end > first_end, "the second grant is not later");
    page[GRANT_TWO_END_NS].store(second_end, Ordering::Release);

    // Short dispatches end at their own limit, inside the grant, without
    // the parent being involved.
    for _ in 0..3 {
        let activation = scheduler
            .dispatch(Some(thread), 5 * MILLISECOND, generation())
            .unwrap_or_else(|error| fail("Dispatch (short)", error));
        page[INNER_REASONS].fetch_or(activation.reasons, Ordering::Release);
        page[INNER_LIMITS].fetch_add(
            u64::from(activation.reasons == activation_reason::LIMIT),
            Ordering::Release,
        );
    }

    // Nothing more to run: wait, which gives the CPU back to the parent.
    loop {
        scheduler.dispatch(None, u64::MAX, 0).ok();
    }
}
