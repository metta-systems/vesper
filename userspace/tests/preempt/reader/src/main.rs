#![no_std]
#![no_main]

//! Preemption-test EL0 component acting as a scheduler agent's reader: it
//! reads one Thread's scheduling record from the table mapped for it, with no
//! system call, and reports what it saw. Its loader passes the address of a
//! page mapped writable in this component only (`x0`). The first words are
//! its parameters, set by the loader: the address of the record table, the
//! table's stride in bytes and the slot to watch. The component writes its
//! observations after them and never blocks or yields.

use {
    core::sync::atomic::{AtomicU64, Ordering},
    libobject::scheduler_record::ThreadRecord,
};

libuser::entry!(main);

/// Words of the page: parameters, then observations.
const RECORDS: usize = 0;
const STRIDE: usize = 1;
const SLOT: usize = 2;
/// Consistent snapshots read.
const READS: usize = 8;
/// The highest `generation` seen.
const NEWEST_GENERATION: usize = 9;
/// Whether a snapshot showed the Thread waiting for budget.
const SAW_AWAITING: usize = 10;
/// Whether a snapshot showed it no longer waiting after it had.
const SAW_REFILLED: usize = 11;
/// The `incarnation` and `state` of the latest snapshot.
const INCARNATION: usize = 12;
const STATE: usize = 13;
const WORDS: usize = 16;

fn main(page: u64) -> ! {
    // SAFETY: the loader maps the page for this component before it starts;
    // only this component writes the observation words.
    let page = unsafe { core::slice::from_raw_parts(page as *const AtomicU64, WORDS) };
    let records = page[RECORDS].load(Ordering::Relaxed) as *const u8;
    let stride = usize::try_from(page[STRIDE].load(Ordering::Relaxed)).unwrap_or(usize::MAX);
    let slot = usize::try_from(page[SLOT].load(Ordering::Relaxed)).unwrap_or(usize::MAX);
    // SAFETY: the loader maps the shared record table for this component,
    // 64-byte aligned, and the slot lies inside it; records are accessed only
    // through atomics.
    let record = unsafe { ThreadRecord::at(records.add(slot * stride)) };

    let mut reads = 0_u64;
    let mut awaiting_seen = false;
    loop {
        let Some(snapshot) = record.try_snapshot(1_000) else {
            continue;
        };
        reads += 1;
        page[READS].store(reads, Ordering::Relaxed);
        page[NEWEST_GENERATION].fetch_max(snapshot.generation, Ordering::Relaxed);
        page[INCARNATION].store(u64::from(snapshot.incarnation), Ordering::Relaxed);
        page[STATE].store(u64::from(snapshot.state), Ordering::Relaxed);
        if snapshot.awaits_budget() {
            awaiting_seen = true;
            page[SAW_AWAITING].store(1, Ordering::Release);
        } else if awaiting_seen {
            page[SAW_REFILLED].store(1, Ordering::Release);
        }
    }
}
