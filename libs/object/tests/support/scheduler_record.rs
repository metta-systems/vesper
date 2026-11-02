use {
    super::{
        BlockReason, FLAG_AWAITING_BUDGET, FLAG_HAS_TIME, LAYOUT_VERSION, MAX_REGIONS, MIN_STRIDE,
        NO_CPU, RECORD_ALIGN, RECORD_SIZE, RecordRegion, SCHEDULER_SLOT, SchedulerRecord,
        SchedulerSnapshot, StopReason, ThreadRecord, ThreadSnapshot, valid_stride,
    },
    std::{
        sync::atomic::{AtomicBool, Ordering},
        thread,
    },
    vesper_objects::thread::ThreadState,
};

/// A page-aligned page for a region of records.
#[repr(C, align(4096))]
struct Page([u8; 4096]);

fn sample(seed: u64) -> ThreadSnapshot {
    ThreadSnapshot {
        generation: seed,
        incarnation: u32::try_from(seed).expect("small seed"),
        state: ThreadState::Runnable as u32,
        flags: FLAG_HAS_TIME,
        block_reason: 0,
        last_stop: StopReason::SliceEnd as u32,
        cpu: NO_CPU,
        run_time_ns: seed * 3,
        last_run_ns: seed * 5,
        budget_remaining_ns: seed * 7,
    }
}

#[test]
fn constants_match_the_contract() {
    assert_eq!(RECORD_SIZE, 128);
    assert_eq!(RECORD_ALIGN, 64);
    assert_eq!(MIN_STRIDE, 128);
    assert_eq!(MAX_REGIONS, 8);
    assert_eq!(LAYOUT_VERSION, 1);
    assert_eq!(SCHEDULER_SLOT, 0);
    assert_eq!(NO_CPU, u32::MAX);
    assert_eq!(FLAG_AWAITING_BUDGET, 1);
    assert_eq!(FLAG_HAS_TIME, 2);
}

#[test]
fn thread_record_offsets_match_the_literal_layout() {
    use core::mem::offset_of;
    let offsets = [
        offset_of!(ThreadRecord, sequence),
        offset_of!(ThreadRecord, generation),
        offset_of!(ThreadRecord, incarnation),
        offset_of!(ThreadRecord, state),
        offset_of!(ThreadRecord, flags),
        offset_of!(ThreadRecord, block_reason),
        offset_of!(ThreadRecord, last_stop),
        offset_of!(ThreadRecord, cpu),
        offset_of!(ThreadRecord, run_time_ns),
        offset_of!(ThreadRecord, last_run_ns),
        offset_of!(ThreadRecord, budget_remaining_ns),
    ];
    assert_eq!(offsets, [0, 8, 16, 20, 24, 28, 32, 36, 40, 48, 56]);
    assert_eq!(size_of::<ThreadRecord>(), 128);
    assert_eq!(align_of::<ThreadRecord>(), 64);
}

#[test]
fn scheduler_record_offsets_match_the_literal_layout() {
    use core::mem::offset_of;
    let offsets = [
        offset_of!(SchedulerRecord, sequence),
        offset_of!(SchedulerRecord, layout),
        offset_of!(SchedulerRecord, record_size),
        offset_of!(SchedulerRecord, events),
        offset_of!(SchedulerRecord, timer_expirations),
        offset_of!(SchedulerRecord, dispatcher_state),
        offset_of!(SchedulerRecord, flags),
    ];
    assert_eq!(offsets, [0, 8, 12, 16, 24, 32, 36]);
    assert_eq!(size_of::<SchedulerRecord>(), 128);
    assert_eq!(align_of::<SchedulerRecord>(), 64);
}

#[test]
fn a_zeroed_record_reads_as_all_zero() {
    let record = ThreadRecord::zeroed();
    assert_eq!(record.sequence(), 0);
    assert_eq!(record.try_snapshot(1), Some(ThreadSnapshot::default()));
}

#[test]
fn the_scheduler_record_starts_with_its_layout_header() {
    let record = SchedulerRecord::initial();
    assert_eq!(record.layout(), (1, 128));
    assert_eq!(record.try_snapshot(1), Some(SchedulerSnapshot::default()));
}

#[test]
fn publish_then_snapshot_round_trips_and_advances_the_sequence_by_two() {
    let record = ThreadRecord::zeroed();
    record.publish(&sample(9));
    assert_eq!(record.sequence(), 2);
    assert_eq!(record.try_snapshot(1), Some(sample(9)));
    record.publish(&sample(11));
    assert_eq!(record.sequence(), 4);
    assert_eq!(record.try_snapshot(1), Some(sample(11)));

    let scheduler = SchedulerRecord::initial();
    let state = SchedulerSnapshot {
        events: 5,
        timer_expirations: 2,
        dispatcher_state: 1,
        flags: 0,
        interrupt_ns: 123_456,
        grant_end_ns: 987_654,
        idle_ns: 555_000,
    };
    scheduler.publish(&state);
    assert_eq!(scheduler.sequence(), 2);
    assert_eq!(scheduler.try_snapshot(1), Some(state));
    // Publishing leaves the header alone.
    assert_eq!(scheduler.layout(), (1, 128));
}

#[test]
fn a_corrupted_sequence_cannot_break_the_parity_readers_rely_on() {
    let record = ThreadRecord::zeroed();
    // The scheduler wrote garbage, odd and even, into the sequence.
    for corrupted in [1_u64, 7, 100, 101, u64::MAX, u64::MAX - 1] {
        record.sequence.store(corrupted, Ordering::Relaxed);
        record.publish(&sample(3));
        assert_eq!(record.sequence() & 1, 0, "left odd after {corrupted}");
        assert_eq!(record.try_snapshot(1), Some(sample(3)));
    }
}

#[test]
fn a_reader_gives_up_while_a_publication_is_in_progress() {
    let record = ThreadRecord::zeroed();
    record.sequence.store(5, Ordering::Relaxed);
    assert_eq!(record.try_snapshot(1_000), None);
    assert_eq!(record.try_snapshot(0), None);
    record.sequence.store(6, Ordering::Relaxed);
    assert!(record.try_snapshot(1).is_some());
}

#[test]
fn snapshots_are_never_torn_under_a_concurrent_writer() {
    // Every field of snapshot n derives from n, so a torn read mixes values.
    let record = ThreadRecord::zeroed();
    let done = AtomicBool::new(false);
    // Before the first publication the record reads as all zero.
    let consistent = |snapshot: &ThreadSnapshot| {
        *snapshot == ThreadSnapshot::default() || *snapshot == sample(snapshot.generation)
    };
    thread::scope(|scope| {
        scope.spawn(|| {
            for n in 1..=200_000_u64 {
                record.publish(&sample(n));
            }
            done.store(true, Ordering::Release);
        });
        let mut reads = 0_u64;
        let mut last = 0_u64;
        while !done.load(Ordering::Acquire) {
            if let Some(snapshot) = record.try_snapshot(1_000_000) {
                assert!(consistent(&snapshot), "torn snapshot {snapshot:?}");
                assert!(snapshot.generation >= last, "snapshots went backwards");
                last = snapshot.generation;
                reads += 1;
            }
        }
        assert!(reads > 0);
    });
    assert_eq!(record.try_snapshot(1), Some(sample(200_000)));
}

#[test]
fn decoding_helpers_name_the_raw_values() {
    let blocked = ThreadSnapshot {
        state: ThreadState::Blocked as u32,
        flags: FLAG_AWAITING_BUDGET,
        block_reason: BlockReason::EventCountAwait as u32,
        last_stop: StopReason::Blocked as u32,
        ..ThreadSnapshot::default()
    };
    assert_eq!(blocked.thread_state(), Some(ThreadState::Blocked));
    assert_eq!(blocked.block_reason(), Some(BlockReason::EventCountAwait));
    assert_eq!(blocked.stop_reason(), Some(StopReason::Blocked));
    assert!(blocked.awaits_budget());
    assert!(!blocked.has_time());

    assert_eq!(ThreadState::try_from(6), Ok(ThreadState::Dying));
    assert_eq!(ThreadState::try_from(7), Ok(ThreadState::Donated));
    assert_eq!(ThreadState::try_from(8), Err(()));
    assert_eq!(BlockReason::from_raw(3), Some(BlockReason::PpcCall));
    assert_eq!(BlockReason::from_raw(4), Some(BlockReason::WaitSetAwait));
    assert_eq!(BlockReason::from_raw(5), None);
    assert_eq!(StopReason::from_raw(7), Some(StopReason::Faulted));
    assert_eq!(StopReason::from_raw(8), None);
    let unknown = ThreadSnapshot {
        state: 99,
        ..ThreadSnapshot::default()
    };
    assert_eq!(unknown.thread_state(), None);
}

#[test]
fn stride_must_be_a_multiple_of_64_at_least_a_record_and_within_the_frame() {
    assert!(valid_stride(128, 4096));
    assert!(valid_stride(192, 4096));
    assert!(valid_stride(4096, 4096));
    assert!(!valid_stride(64, 4096), "smaller than a record");
    assert!(!valid_stride(0, 4096));
    assert!(!valid_stride(130, 4096), "not a multiple of 64");
    assert!(!valid_stride(4160, 4096), "larger than the Frame");
}

#[test]
fn a_region_places_records_by_slot_and_stride() {
    let mut page = Page([0; 4096]);
    let base = page.0.as_mut_ptr();
    // 4096 / 192 = 21 slots, starting at slot 0.
    let region = RecordRegion::new(base, 4096, 192, 0).expect("valid region");
    assert_eq!(region.slots(), 0..21);
    // SAFETY: the page is live, aligned and used only through atomics here.
    unsafe {
        let scheduler = region.scheduler_record().expect("slot 0 is here");
        let first = region.thread_record(1).expect("slot 1");
        let last = region.thread_record(20).expect("slot 20");
        assert_eq!(std::ptr::from_ref(scheduler).addr(), base.addr());
        assert_eq!(std::ptr::from_ref(first).addr(), base.addr() + 192);
        assert_eq!(std::ptr::from_ref(last).addr(), base.addr() + 20 * 192);
        // The scheduler slot is not a Thread record; slots beyond the region
        // are not here.
        assert!(region.thread_record(SCHEDULER_SLOT).is_none());
        assert!(region.thread_record(21).is_none());
        // A published record is visible through the region.
        first.publish(&sample(4));
        assert_eq!(first.try_snapshot(1), Some(sample(4)));
    }
}

#[test]
fn a_later_region_covers_the_slots_after_the_first() {
    let mut page = Page([0; 4096]);
    let base = page.0.as_mut_ptr();
    let region = RecordRegion::new(base, 4096, 128, 32).expect("valid region");
    assert_eq!(region.slots(), 32..64);
    // SAFETY: as above.
    unsafe {
        assert!(region.scheduler_record().is_none());
        assert!(region.thread_record(31).is_none());
        assert_eq!(
            std::ptr::from_ref(region.thread_record(32).unwrap()).addr(),
            base.addr()
        );
        assert!(region.thread_record(63).is_some());
        assert!(region.thread_record(64).is_none());
    }
}

#[test]
fn a_region_rejects_a_bad_stride_a_misaligned_base_and_slot_overflow() {
    let mut page = Page([0; 4096]);
    let base = page.0.as_mut_ptr();
    assert!(RecordRegion::new(base, 4096, 100, 0).is_none());
    assert!(RecordRegion::new(base, 4096, 64, 0).is_none());
    // SAFETY: the offset stays inside the page.
    let misaligned = unsafe { base.add(8) };
    assert!(RecordRegion::new(misaligned, 4096, 128, 0).is_none());
    assert!(RecordRegion::new(base, 4096, 128, u32::MAX).is_none());
}
