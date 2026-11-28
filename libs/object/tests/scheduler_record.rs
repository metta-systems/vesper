//! Host tests of the scheduler records' sequence lock under concurrency (P-SMP-2, payload
//! publication): one writer publishes while readers on other threads take snapshots, as
//! the nucleus on one core and a scheduler on another do. Every field of a published
//! snapshot is derived from one counter, so a snapshot that mixes two publications is
//! caught. On a weakly ordered host (an Arm Mac) a missing fence shows up as such a
//! torn snapshot.

#[cfg(test)]
mod tests {
    use {
        std::{
            sync::{
                Arc,
                atomic::{AtomicBool, AtomicUsize, Ordering},
            },
            thread,
        },
        vesper_objects::scheduler_record::{
            FpTotals, SchedulerRecord, SchedulerSnapshot, ThreadFpCounters, ThreadRecord,
            ThreadSnapshot,
        },
    };

    const PUBLICATIONS: u64 = 200_000;
    const READERS: usize = 3;

    /// The Thread snapshot of publication `round`: every field a different multiple of
    /// `round`, so round 0 is the zeroed record readers may see before the first
    /// publication.
    fn thread_snapshot(round: u64) -> ThreadSnapshot {
        let word = u32::try_from(round & 0xffff_ffff).expect("masked to 32 bits");
        ThreadSnapshot {
            generation: round,
            incarnation: word,
            state: word.wrapping_mul(2),
            flags: word.wrapping_mul(3),
            block_reason: word.wrapping_mul(4),
            last_stop: word.wrapping_mul(5),
            cpu: word.wrapping_mul(6),
            run_time_ns: round.wrapping_mul(7),
            last_run_ns: round.wrapping_mul(8),
            budget_remaining_ns: round.wrapping_mul(9),
            fp: ThreadFpCounters {
                fp_entries: round.wrapping_mul(10),
                fp_saves: round.wrapping_mul(11),
                fp_loads: round.wrapping_mul(12),
                fp_zeroes: round.wrapping_mul(13),
                preserved_saves: round.wrapping_mul(14),
                first_use_traps: round.wrapping_mul(15),
                decays: round.wrapping_mul(16),
                owner_hits: round.wrapping_mul(17),
            },
        }
    }

    /// The Scheduler snapshot of publication `round`: every field a different multiple of
    /// `round`, so round 0 is the initial record.
    fn scheduler_snapshot(round: u64) -> SchedulerSnapshot {
        let word = u32::try_from(round & 0xffff_ffff).expect("masked to 32 bits");
        SchedulerSnapshot {
            events: round,
            timer_expirations: round.wrapping_mul(2),
            dispatcher_state: word.wrapping_mul(3),
            flags: word.wrapping_mul(4),
            interrupt_ns: round.wrapping_mul(5),
            grant_end_ns: round.wrapping_mul(6),
            idle_ns: round.wrapping_mul(7),
            fp: FpTotals {
                thread_switches: round.wrapping_mul(8),
                fp_thread_switches: round.wrapping_mul(9),
                fp_entry_toggles: round.wrapping_mul(10),
            },
        }
    }

    /// Run one writer publishing `PUBLICATIONS` rounds through `publish` while `READERS`
    /// readers snapshot through `read`, which returns the round a snapshot came from
    /// after checking it is whole. The writer starts once every reader runs; each reader
    /// sees rounds that never go back, and, once the writer is done, the last round.
    fn race(
        publish: impl Fn(u64) + Send + 'static,
        read: impl Fn() -> Option<u64> + Send + Sync + 'static,
    ) {
        let done = Arc::new(AtomicBool::new(false));
        let running = Arc::new(AtomicUsize::new(0));
        let read = Arc::new(read);
        let readers: Vec<_> = (0..READERS)
            .map(|_| {
                let (done, running, read) =
                    (Arc::clone(&done), Arc::clone(&running), Arc::clone(&read));
                thread::spawn(move || {
                    running.fetch_add(1, Ordering::Relaxed);
                    let mut last = 0;
                    while !done.load(Ordering::Acquire) {
                        if let Some(round) = read() {
                            assert!(round >= last, "round {round} after {last}");
                            last = round;
                        }
                    }
                    read()
                })
            })
            .collect();
        while running.load(Ordering::Relaxed) < READERS {
            thread::yield_now();
        }
        for round in 1..=PUBLICATIONS {
            publish(round);
        }
        done.store(true, Ordering::Release);
        for reader in readers {
            let last = reader.join().expect("a reader found a torn snapshot");
            assert_eq!(last, Some(PUBLICATIONS), "a reader missed the last round");
        }
    }

    #[test]
    fn thread_record_snapshots_are_never_torn() {
        let record = Arc::new(ThreadRecord::zeroed());
        let writer = Arc::clone(&record);
        race(
            move |round| writer.publish(&thread_snapshot(round)),
            move || {
                let snapshot = record.try_snapshot(1000)?;
                assert_eq!(
                    snapshot,
                    thread_snapshot(snapshot.generation),
                    "torn Thread record snapshot"
                );
                Some(snapshot.generation)
            },
        );
    }

    #[test]
    fn scheduler_record_snapshots_are_never_torn() {
        let record = Arc::new(SchedulerRecord::initial());
        let writer = Arc::clone(&record);
        race(
            move |round| writer.publish(&scheduler_snapshot(round)),
            move || {
                let snapshot = record.try_snapshot(1000)?;
                assert_eq!(
                    snapshot,
                    scheduler_snapshot(snapshot.events),
                    "torn Scheduler record snapshot"
                );
                Some(snapshot.events)
            },
        );
    }
}
