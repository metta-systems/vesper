//! `EventCount` kernel object: a monotonic counter with broadcast target
//! waits (Phase 6 vertical, 2026-09-18).
//!
//! Realizes the excluded sketch's intent — a monotonically increasing
//! counter with domains waiting for `value >= target` — on the completion
//! foundation's pending-invocation records. Unlike Notification's
//! one-consumer delivery, an advance completes *every* queued `Await` whose
//! target the new value satisfies: readers hold independent positions and
//! awaiting/reading does not consume the counter (broadcast-style
//! observation, per the contract's Notification/EventCount selection).
//!
//! Overflow policy (selected 2026-09-18, D7/D9): `Advance` requires a
//! nonzero delta; an advance whose sum would exceed `u64::MAX` completes
//! with the shared `CounterOverflow` error, leaves the counter unchanged,
//! and completes every queued `Await` with the same error so waiters
//! observe the producer's failure instead of blocking indefinitely — a
//! woken waiter may re-`Await`. The counter never wraps and never
//! saturates.

use {
    crate::objects::{
        NucleusObject,
        completion::{PendingKind, PendingPool},
    },
    libobject::{CapError, ObjectType, syscall_status},
};

use crate::objects::access::{ObjectId, PoolTag};

/// Bounded FIFO of queued awaits: pending-record identities with their
/// targets, in arrival order.
///
/// The per-object wait reservation (same bounded capacity as Notification's
/// queue): a full queue rejects a new await before admission rather than
/// growing unbounded kernel state.
pub struct AwaitQueue {
    entries: [(ObjectId, u64); Self::CAPACITY],
    head: usize,
    len: usize,
}

impl AwaitQueue {
    /// Waiters per synchronization object. Bounded, explicit reservation.
    pub const CAPACITY: usize = 8;

    pub const fn new() -> Self {
        Self {
            entries: [(
                ObjectId {
                    pool: PoolTag::Region,
                    index: 0,
                    generation: 0,
                },
                0,
            ); Self::CAPACITY],
            head: 0,
            len: 0,
        }
    }

    fn is_full(&self) -> bool {
        self.len == Self::CAPACITY
    }

    fn push(&mut self, record: ObjectId, target: u64) -> Result<(), CapError> {
        if self.is_full() {
            return Err(CapError::PoolExhausted);
        }
        let back = (self.head + self.len) % Self::CAPACITY;
        self.entries[back] = (record, target);
        self.len += 1;
        Ok(())
    }

    fn pop_front(&mut self) -> Option<(ObjectId, u64)> {
        if self.len == 0 {
            return None;
        }
        let entry = self.entries[self.head];
        self.entries[self.head] = (
            ObjectId {
                pool: PoolTag::Region,
                index: 0,
                generation: 0,
            },
            0,
        );
        self.head = (self.head + 1) % Self::CAPACITY;
        self.len -= 1;
        Some(entry)
    }

    /// Remove every queued record naming `waiter` (domain-teardown
    /// cancellation), preserving the FIFO order of the remaining awaits.
    ///
    /// As `WaitQueue::remove_waiter`: the queue holds record identities, the
    /// pool resolves the waiter, and the teardown sweep
    /// (`PendingPool::teardown_waiter`) gives the unqueued records their
    /// terminal transition and releases them. Returns how many records were
    /// unqueued.
    fn remove_waiter(&mut self, waiter: ObjectId, pending: &PendingPool) -> usize {
        let live = self.len;
        let mut kept = 0;
        let mut removed = 0;
        for i in 0..live {
            let slot = (self.head + i) % Self::CAPACITY;
            let entry = self.entries[slot];
            if matches!(pending.waiter(entry.0), Ok(blocked) if blocked == waiter) {
                removed += 1;
            } else {
                // The compaction only writes to already-processed slots
                // (`kept <= i`), so it never loses an unprocessed entry.
                self.entries[(self.head + kept) % Self::CAPACITY] = entry;
                kept += 1;
            }
        }
        // Clear the vacated tail slots and shrink the live region.
        for i in kept..live {
            self.entries[(self.head + i) % Self::CAPACITY] = (
                ObjectId {
                    pool: PoolTag::Region,
                    index: 0,
                    generation: 0,
                },
                0,
            );
        }
        self.len = kept;
        removed
    }
}

/// Records completed by one advance, in queue (arrival) order.
///
/// Bounded by the queue capacity: an advance can never wake more waiters
/// than the per-object reservation holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WokenRecords {
    records: [Option<ObjectId>; AwaitQueue::CAPACITY],
}

impl WokenRecords {
    const fn empty() -> Self {
        Self {
            records: [const { None }; AwaitQueue::CAPACITY],
        }
    }

    fn push(&mut self, record: ObjectId) {
        let slot = self
            .records
            .iter_mut()
            .find(|slot| slot.is_none())
            .expect("woken records bounded by the queue capacity");
        *slot = Some(record);
    }

    /// The completed records, in queue order.
    pub fn iter(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.records.iter().flatten().copied()
    }
}

/// Outcome of an `Advance` attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdvanceOutcome {
    /// The counter advanced to `new_value`; `woken` names every satisfied
    /// waiter completed with the new value, in queue order.
    Advanced { new_value: u64, woken: WokenRecords },
    /// The advance overflowed: the counter is unchanged, the advancer
    /// reports `CounterOverflow`, and every queued waiter was completed
    /// with the same error (selected 2026-09-18).
    Overflow { woken: WokenRecords },
}

/// Outcome of an `Await` attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AwaitOutcome {
    /// Already-satisfied await: the current value meets the target and is
    /// returned immediately.
    Ready(u64),
    /// The target is not met: the caller blocks. The identity names its
    /// pending-invocation record.
    Blocked(ObjectId),
}

/// `EventCount` kernel object.
///
/// Pool-backed kernel metadata (Retype-creatable, like Notification): the
/// capability is a checked pool identity, and no Untyped bytes are carved.
pub struct EventCount {
    /// The monotonic counter. Never wraps, never saturates: an advance that
    /// would exceed `u64::MAX` is rejected (selected 2026-09-18).
    value: u64,
    /// Blocked awaits, FIFO, bounded (the per-object wait reservation).
    waiters: AwaitQueue,
}

impl NucleusObject for EventCount {
    const TYPE: ObjectType = ObjectType::EVENT_COUNT;
    const POOL: PoolTag = PoolTag::EventCount;
}

impl EventCount {
    pub const fn new() -> Self {
        Self {
            value: 0,
            waiters: AwaitQueue::new(),
        }
    }

    /// The current counter value: the `Read` operation's state access
    /// (also diagnostics and tests).
    pub fn read(&self) -> u64 {
        self.value
    }

    /// `Advance`: add a nonzero delta, completing every queued `Await` whose
    /// target the new value satisfies (broadcast wakeups, each delivered the
    /// new value).
    ///
    /// Zero is invalid (an advance must strictly increase). An advance that
    /// would exceed `u64::MAX` reports overflow: the counter is unchanged
    /// and every queued waiter completes with the shared `CounterOverflow`
    /// error, so waiters observe the producer's failure instead of blocking
    /// indefinitely.
    pub fn advance(
        &mut self,
        delta: u64,
        pending: &mut PendingPool,
    ) -> Result<AdvanceOutcome, CapError> {
        if delta == 0 {
            return Err(CapError::InvalidOperation);
        }
        let Some(new_value) = self.value.checked_add(delta) else {
            // Overflow: drain the whole queue, completing each waiter with
            // the shared error. A queued record is always `Waiting` under the
            // kernel lock (teardown removes cancelled records from the
            // queue), so each terminal transition is the winner by
            // construction; an error here would indicate a kernel
            // bookkeeping bug.
            let mut woken = WokenRecords::empty();
            while let Some((record, _target)) = self.waiters.pop_front() {
                pending.complete_with_status(record, syscall_status::COUNTER_OVERFLOW, 0, 0)?;
                woken.push(record);
            }
            return Ok(AdvanceOutcome::Overflow { woken });
        };
        self.value = new_value;

        // Wake every satisfied waiter, preserving the queue order of the
        // rest. The compaction below only writes to already-processed slots
        // (`kept <= i`), so it never loses an unprocessed entry.
        let live = self.waiters.len;
        let mut woken = WokenRecords::empty();
        let mut kept = 0;
        for i in 0..live {
            let slot = (self.waiters.head + i) % AwaitQueue::CAPACITY;
            let (record, target) = self.waiters.entries[slot];
            if target <= new_value {
                // Same single-terminal-transition reasoning as the overflow
                // drain: a queued record is `Waiting` by construction.
                pending.complete_with_status(record, syscall_status::SUCCESS, new_value, 0)?;
                woken.push(record);
            } else {
                self.waiters.entries[(self.waiters.head + kept) % AwaitQueue::CAPACITY] =
                    (record, target);
                kept += 1;
            }
        }
        // Clear the vacated tail slots and shrink the live region.
        for i in kept..live {
            self.waiters.entries[(self.waiters.head + i) % AwaitQueue::CAPACITY] = (
                ObjectId {
                    pool: PoolTag::Region,
                    index: 0,
                    generation: 0,
                },
                0,
            );
        }
        self.waiters.len = kept;

        Ok(AdvanceOutcome::Advanced { new_value, woken })
    }

    /// `Await`: return the current value immediately when it already meets
    /// `target`; otherwise register the caller's pending record (with its
    /// target) in the queue and report the caller as blocked.
    ///
    /// The queue reservation is validated before the record is registered,
    /// so a full queue rejects the attempt before admission with no record
    /// to roll back.
    pub fn await_ge(
        &mut self,
        target: u64,
        waiter: ObjectId,
        pending: &mut PendingPool,
    ) -> Result<AwaitOutcome, CapError> {
        if self.value >= target {
            return Ok(AwaitOutcome::Ready(self.value));
        }
        if self.waiters.is_full() {
            return Err(CapError::PoolExhausted);
        }
        let record = pending.block(waiter, PendingKind::EventCountAwait)?;
        // The reservation was validated above and the kernel lock excludes
        // concurrent admission, so this cannot fail; if it ever does,
        // abandon the registered record rather than leaking its pool slot.
        match self.waiters.push(record, target) {
            Ok(()) => Ok(AwaitOutcome::Blocked(record)),
            Err(error) => {
                pending.abandon(record);
                Err(error)
            }
        }
    }

    /// Teardown: cancel every queued await and clear the queue.
    ///
    /// Each record receives its single terminal transition (`Cancelled`);
    /// the cancelled waiters resume with a cancellation error. The pending
    /// records are released by the resumption path, not here.
    pub fn cancel_waiters(&mut self, pending: &mut PendingPool) -> Result<(), CapError> {
        while let Some((record, _target)) = self.waiters.pop_front() {
            // A queued record is `Waiting` by construction; an error here
            // would mean two terminal transitions raced, which the kernel
            // lock excludes.
            pending.cancel(record)?;
        }
        Ok(())
    }

    /// Domain-teardown cancellation: stop holding the torn-down `waiter`'s
    /// queued awaits, preserving the FIFO order of the remaining waiters.
    ///
    /// Distinct from object teardown ([`Self::cancel_waiters`]): the
    /// waiter's Domain is going away, so its records are cancelled and
    /// released by the pending-pool teardown sweep
    /// (`PendingPool::teardown_waiter`) instead of being left terminal for a
    /// resume that never happens. Returns how many of the waiter's records
    /// were unqueued.
    pub fn remove_waiter(&mut self, waiter: ObjectId, pending: &PendingPool) -> usize {
        self.waiters.remove_waiter(waiter, pending)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::{AdvanceOutcome, AwaitOutcome, EventCount, WokenRecords},
        crate::objects::{
            access::{ObjectId, PoolTag},
            completion::{PendingPool, PendingState},
        },
        libobject::{CapError, syscall_status},
    };

    fn waiter(index: u16) -> ObjectId {
        ObjectId {
            pool: PoolTag::Domain,
            index,
            generation: 1,
        }
    }

    /// Block a waiter on `target` and return its pending-record identity.
    fn block_waiter(
        event_count: &mut EventCount,
        pending: &mut PendingPool,
        index: u16,
        target: u64,
    ) -> ObjectId {
        match event_count.await_ge(target, waiter(index), pending) {
            Ok(AwaitOutcome::Blocked(record)) => record,
            _ => panic!("await should block"),
        }
    }

    /// Assert the outcome's woken records equal `expected`, in queue order.
    fn assert_woken(outcome: &AdvanceOutcome, expected: &[ObjectId]) {
        let woken = match outcome {
            AdvanceOutcome::Advanced { woken, .. } | AdvanceOutcome::Overflow { woken } => woken,
        };
        assert_eq!(woken.iter().count(), expected.len());
        for (got, want) in woken.iter().zip(expected.iter().copied()) {
            assert_eq!(got, want);
        }
    }

    #[test_case]
    fn advance_updates_value_and_wakes_no_one_when_empty() {
        let mut pending = PendingPool::new();
        let mut event_count = EventCount::new();

        let outcome = event_count
            .advance(5, &mut pending)
            .ok()
            .expect("advance failed");
        assert!(matches!(
            outcome,
            AdvanceOutcome::Advanced { new_value: 5, .. }
        ));
        assert_woken(&outcome, &[]);
        assert_eq!(event_count.read(), 5);
        assert!(pending.is_empty());
    }

    #[test_case]
    fn advance_zero_is_invalid_and_leaves_the_counter_unchanged() {
        let mut pending = PendingPool::new();
        let mut event_count = EventCount::new();

        assert!(event_count.advance(7, &mut pending).is_ok());
        assert!(matches!(
            event_count.advance(0, &mut pending),
            Err(CapError::InvalidOperation)
        ));
        assert_eq!(event_count.read(), 7);
    }

    #[test_case]
    fn await_target_zero_is_trivially_satisfied() {
        let mut pending = PendingPool::new();
        let mut event_count = EventCount::new();

        assert_eq!(
            event_count.await_ge(0, waiter(0), &mut pending).ok(),
            Some(AwaitOutcome::Ready(0))
        );
        assert!(pending.is_empty());
    }

    #[test_case]
    fn await_already_satisfied_returns_the_current_value() {
        let mut pending = PendingPool::new();
        let mut event_count = EventCount::new();

        assert!(event_count.advance(12, &mut pending).is_ok());
        assert_eq!(
            event_count.await_ge(10, waiter(0), &mut pending).ok(),
            Some(AwaitOutcome::Ready(12))
        );
        // Awaiting does not consume the counter.
        assert_eq!(event_count.read(), 12);
        assert!(pending.is_empty());
    }

    #[test_case]
    fn await_blocks_and_satisfied_advance_wakes_with_the_new_value() {
        let mut pending = PendingPool::new();
        let mut event_count = EventCount::new();

        let record = block_waiter(&mut event_count, &mut pending, 3, 10);
        assert_eq!(pending.state(record).ok(), Some(PendingState::Waiting));

        let outcome = event_count
            .advance(10, &mut pending)
            .ok()
            .expect("advance failed");
        assert_woken(&outcome, &[record]);
        assert_eq!(
            pending.state(record).ok(),
            Some(PendingState::Completed {
                status: syscall_status::SUCCESS,
                result0: 10,
                result1: 0
            })
        );
    }

    #[test_case]
    fn advance_below_the_target_wakes_no_one() {
        let mut pending = PendingPool::new();
        let mut event_count = EventCount::new();

        let record = block_waiter(&mut event_count, &mut pending, 0, 10);
        let outcome = event_count
            .advance(5, &mut pending)
            .ok()
            .expect("advance failed");
        assert_woken(&outcome, &[]);
        assert_eq!(pending.state(record).ok(), Some(PendingState::Waiting));

        // The unsatisfied waiter stays queued; a later advance that reaches
        // its target completes it with that advance's value.
        let outcome = event_count
            .advance(5, &mut pending)
            .ok()
            .expect("second advance failed");
        assert_woken(&outcome, &[record]);
        assert_eq!(
            pending.state(record).ok(),
            Some(PendingState::Completed {
                status: syscall_status::SUCCESS,
                result0: 10,
                result1: 0
            })
        );
    }

    #[test_case]
    fn advance_wakes_every_satisfied_waiter_in_queue_order() {
        let mut pending = PendingPool::new();
        let mut event_count = EventCount::new();

        // Independent readers with distinct targets, in arrival order.
        let first = block_waiter(&mut event_count, &mut pending, 0, 5);
        let second = block_waiter(&mut event_count, &mut pending, 1, 10);
        let third = block_waiter(&mut event_count, &mut pending, 2, 20);

        // One advance to 10 satisfies the first two (broadcast), not the
        // third; the delivered value is the advance's new value.
        let outcome = event_count
            .advance(10, &mut pending)
            .ok()
            .expect("advance failed");
        assert_woken(&outcome, &[first, second]);
        for record in [first, second] {
            assert_eq!(
                pending.state(record).ok(),
                Some(PendingState::Completed {
                    status: syscall_status::SUCCESS,
                    result0: 10,
                    result1: 0
                })
            );
        }
        assert_eq!(pending.state(third).ok(), Some(PendingState::Waiting));

        // The next advance satisfies the remaining reader.
        let outcome = event_count
            .advance(10, &mut pending)
            .ok()
            .expect("second advance failed");
        assert_woken(&outcome, &[third]);
    }

    #[test_case]
    fn overflow_rejects_leaves_the_counter_and_error_wakes_all_waiters() {
        let mut pending = PendingPool::new();
        let mut event_count = EventCount::new();

        assert!(event_count.advance(3, &mut pending).is_ok());
        let first = block_waiter(&mut event_count, &mut pending, 0, 10);
        let second = block_waiter(&mut event_count, &mut pending, 1, 20);

        // The overflowing advance: counter unchanged, every queued waiter
        // completed with the shared error (selected 2026-09-18).
        let outcome = event_count
            .advance(u64::MAX, &mut pending)
            .ok()
            .expect("overflowing advance failed");
        assert!(matches!(outcome, AdvanceOutcome::Overflow { .. }));
        assert_woken(&outcome, &[first, second]);
        assert_eq!(event_count.read(), 3);
        for record in [first, second] {
            assert_eq!(
                pending.state(record).ok(),
                Some(PendingState::Completed {
                    status: syscall_status::COUNTER_OVERFLOW,
                    result0: 0,
                    result1: 0
                })
            );
        }

        // The counter still works afterwards: a woken waiter may re-await.
        let record = block_waiter(&mut event_count, &mut pending, 0, 13);
        let outcome = event_count
            .advance(10, &mut pending)
            .ok()
            .expect("advance after overflow failed");
        assert_woken(&outcome, &[record]);
        assert_eq!(event_count.read(), 13);
    }

    #[test_case]
    fn full_queue_rejects_before_admission_without_leaking_records() {
        let mut pending = PendingPool::new();
        let mut event_count = EventCount::new();

        // Fill the wait reservation.
        for i in 0..crate::objects::event_count::AwaitQueue::CAPACITY {
            let index = u16::try_from(i).unwrap();
            block_waiter(&mut event_count, &mut pending, index, 100);
        }
        let before = pending.len();

        // The next await is rejected before admission: no record registered,
        // nothing to roll back.
        assert!(matches!(
            event_count.await_ge(100, waiter(255), &mut pending),
            Err(CapError::PoolExhausted)
        ));
        assert_eq!(pending.len(), before);
    }

    #[test_case]
    fn teardown_cancels_queued_waiters() {
        let mut pending = PendingPool::new();
        let mut event_count = EventCount::new();

        let first = block_waiter(&mut event_count, &mut pending, 0, 10);
        let second = block_waiter(&mut event_count, &mut pending, 1, 20);

        assert!(event_count.cancel_waiters(&mut pending).is_ok());
        assert_eq!(pending.state(first).ok(), Some(PendingState::Cancelled));
        assert_eq!(pending.state(second).ok(), Some(PendingState::Cancelled));

        // After teardown the queue is empty: a further advance just moves
        // the counter.
        let outcome = event_count
            .advance(1, &mut pending)
            .ok()
            .expect("advance after teardown failed");
        assert_woken(&outcome, &[]);
        assert_eq!(event_count.read(), 1);
    }

    #[test_case]
    fn domain_teardown_unqueues_only_the_torn_down_waiter() {
        let mut pending = PendingPool::new();
        let mut event_count = EventCount::new();

        // Two readers with distinct targets; the torn-down one is at the
        // front of the queue.
        let gone = block_waiter(&mut event_count, &mut pending, 0, 5);
        let survivor = block_waiter(&mut event_count, &mut pending, 1, 10);

        // Unqueue the torn-down Domain's await; the sweep (not the object)
        // gives it its terminal transition and releases it.
        assert_eq!(event_count.remove_waiter(waiter(0), &pending), 1);
        assert_eq!(pending.teardown_waiter(waiter(0)).ok(), Some(1));
        assert!(matches!(
            pending.state(gone),
            Err(CapError::InvalidOperation)
        ));

        // A satisfying advance wakes only the surviving reader, in queue
        // order, with the new value.
        let outcome = event_count
            .advance(10, &mut pending)
            .ok()
            .expect("advance after domain teardown failed");
        assert_woken(&outcome, &[survivor]);
        assert_eq!(
            pending.state(survivor).ok(),
            Some(PendingState::Completed {
                status: syscall_status::SUCCESS,
                result0: 10,
                result1: 0
            })
        );

        // A Domain with nothing queued removes nothing.
        assert_eq!(event_count.remove_waiter(waiter(0), &pending), 0);
    }

    #[test_case]
    fn woken_records_iterate_in_push_order() {
        let mut woken = WokenRecords::empty();
        let first = ObjectId {
            pool: PoolTag::Pending,
            index: 1,
            generation: 1,
        };
        let second = ObjectId {
            pool: PoolTag::Pending,
            index: 2,
            generation: 1,
        };
        woken.push(first);
        woken.push(second);
        assert_eq!(woken.iter().count(), 2);
        let mut iter = woken.iter();
        assert_eq!(iter.next(), Some(first));
        assert_eq!(iter.next(), Some(second));
        assert_eq!(iter.next(), None);
    }
}
