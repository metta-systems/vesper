//! Completion: pending-invocation records and bounded wait queues
//!
//! A blocking invocation does not return until completion or cancellation. No
//! "blocked" wire status exists, and the eventual
//! return is the completion or a cancellation error. While a caller is
//! blocked, its invocation is represented here as a pending-invocation
//! record — the bounded, kernel-private state the contract requires.
//!
//! The record is the identity for a pending blocking operation, never a bare
//! slot number or Thread index. Record identities reuse the checked `ObjectId` shape (pool tag +
//! slot + non-wrapping generation, the D3 kernel-allocation-identity
//! pattern), so stale identities cannot resolve after release and reuse.
//!
//! Completion, timeout, cancellation, and teardown compete for exactly one
//! terminal transition of a record; the losers observe a defined error instead
//! of mutating it.
//! This module enforces that rule as a state machine.

use {
    crate::objects::{
        access::{ObjectId, PoolTag},
        carved::CarvedId,
        thread::ThreadId,
    },
    libobject::{CapError, syscall_status},
};

// ═══════════════════════════════════════════════════════════════════
// PENDING-INVOCATION RECORDS
// ═══════════════════════════════════════════════════════════════════

/// What a blocked invocation is waiting on.
///
/// The kind records how the invocation's completion is produced and how
/// teardown cancellation treats it. One variant exists per blocking
/// operation family as those families activate; the vocabulary is
/// kernel-internal and never crosses the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingKind {
    /// `Notification.Wait`: completion delivers the consumed signal bitmap
    /// in the first result word.
    NotificationWait,
    /// `EventCount.Await`: completion delivers the observed counter value
    /// in the first result word, or the shared `CounterOverflow` error when
    /// an advance's overflow wakes the waiter.
    EventCountAwait,
    /// `Scheduler.Dispatch`: the dispatcher waits to be activated;
    /// completion delivers the activation reason bits in the first result
    /// word and the scheduler's event count in the second.
    SchedulerDispatch,
}

/// State of a pending invocation.
///
/// `Waiting` is the only non-terminal state. Exactly one terminal
/// transition out of `Waiting` succeeds (see the module docs); the
/// competing transition attempts after that observe an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingState {
    /// Blocked; waiting for its terminal transition.
    Waiting,
    /// Completed: the eventual result is stored — the full syscall result
    /// shape (`status` plus both result words). A completed outcome may be
    /// an operation-specific failure (the adopted aborted-work vocabulary:
    /// "Completed" includes a known failure), e.g. an `EventCount.Await`
    /// woken by an overflowing advance. The waiter must still be resumed
    /// with it before the record is released.
    Completed {
        status: u64,
        result0: u64,
        result1: u64,
    },
    /// Cancelled before commit (teardown): the waiter resumes with a
    /// cancellation error.
    Cancelled,
}

/// One blocked invocation: the waiter's incarnation-checked identity, the
/// wait kind, and the terminal state machine. Fields are private: the
/// terminal-transition rule is enforced by [`PendingPool`]'s methods, not
/// by direct field mutation.
#[derive(Clone, Copy, Debug)]
pub struct PendingInvocation {
    /// The blocked Thread (Thread identity, stored verbatim and
    /// validated against the pool when the record is resolved at wake time).
    waiter: ThreadId,
    /// What the invocation waits on.
    kind: PendingKind,
    /// The carved synchronization object whose wait queue holds the record,
    /// so teardown can find it without scanning every object.
    object: CarvedId,
    state: PendingState,
    /// When a finite timeout ends the wait, in counter ticks: an entry of
    /// the kernel's deadline queue, which the pool's bounded size keeps
    /// small enough to scan.
    deadline_ticks: Option<u64>,
}

impl PendingInvocation {
    /// The blocked Thread's identity (validated at wake time).
    pub fn waiter(&self) -> ThreadId {
        self.waiter
    }

    /// What the invocation waits on.
    pub fn kind(&self) -> PendingKind {
        self.kind
    }

    /// The terminal state machine state.
    pub fn state(&self) -> PendingState {
        self.state
    }

    /// The synchronization object whose wait queue holds the record.
    pub fn object(&self) -> CarvedId {
        self.object
    }
}

/// Bounded pool of pending-invocation records.
///
/// Kernel-private state with explicit bounded storage (the contract's
/// "typed pools or equivalently explicit bounded storage"): the capacity is
/// the wait-resource reservation, so blocking cannot allocate unbounded
/// kernel memory. Record identities are checked `ObjectId`s with the
/// `Pending` pool tag; generations are retained after release and never
/// wrap, so a stale identity cannot resolve to a replacement record.
pub struct PendingPool {
    records: [Option<PendingInvocation>; Self::CAPACITY],
    /// Retained per-slot allocation generations; the first live generation
    /// of a slot is 1. A slot at `u32::MAX` is exhausted and never reused.
    generations: [u32; Self::CAPACITY],
    /// Next-fit cursor: the slot to start scanning from on the next
    /// registration. Advances monotonically around the pool.
    next_free: usize,
}

impl PendingPool {
    /// Bounded reservation for simultaneously blocked invocations.
    pub const CAPACITY: usize = 32;

    pub const fn new() -> Self {
        Self {
            records: [const { None }; Self::CAPACITY],
            generations: [0; Self::CAPACITY],
            next_free: 0,
        }
    }

    /// Number of live records.
    pub fn len(&self) -> usize {
        self.records.iter().filter(|r| r.is_some()).count()
    }

    /// Whether no record is live.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Register a blocked invocation, returning the new record's identity.
    ///
    /// The waiter identity is stored verbatim; it is incarnation-checked
    /// against the Thread pool when the record is resolved at wake time.
    /// Fails with `PoolExhausted` when every slot is live or exhausted.
    pub fn block(
        &mut self,
        waiter: ThreadId,
        kind: PendingKind,
        object: CarvedId,
    ) -> Result<ObjectId, CapError> {
        let slot = (0..Self::CAPACITY)
            .map(|i| (self.next_free + i) % Self::CAPACITY)
            .find(|&slot| self.records[slot].is_none() && self.generations[slot] != u32::MAX)
            .ok_or(CapError::PoolExhausted)?;
        self.next_free = (slot + 1) % Self::CAPACITY;

        // Exhausted generations were filtered above, so this cannot wrap.
        self.generations[slot] += 1;
        self.records[slot] = Some(PendingInvocation {
            waiter,
            object,
            kind,
            state: PendingState::Waiting,
            deadline_ticks: None,
        });
        Ok(ObjectId {
            pool: PoolTag::Pending,
            index: u16::try_from(slot).map_err(|_error| CapError::PoolExhausted)?,
            generation: self.generations[slot],
        })
    }

    /// Give the waiting record `id` a deadline: the counter value at which
    /// its wait times out. Only a `Waiting` record has one.
    pub fn set_deadline(&mut self, id: ObjectId, deadline_ticks: u64) -> Result<(), CapError> {
        let record = self.live_mut(id)?;
        if record.state != PendingState::Waiting {
            return Err(CapError::InvalidOperation);
        }
        record.deadline_ticks = Some(deadline_ticks);
        Ok(())
    }

    /// The earliest deadline among the waiting records.
    pub fn next_deadline(&self) -> Option<u64> {
        self.records
            .iter()
            .flatten()
            .filter(|record| record.state == PendingState::Waiting)
            .filter_map(|record| record.deadline_ticks)
            .min()
    }

    /// The waiting record with the earliest deadline at or before
    /// `now_ticks`. Its terminal transition is the caller's.
    pub fn earliest_expired(&self, now_ticks: u64) -> Option<ObjectId> {
        let (slot, _deadline) = self
            .records
            .iter()
            .enumerate()
            .filter_map(|(slot, record)| {
                let record = record.as_ref()?;
                let deadline = record.deadline_ticks?;
                (record.state == PendingState::Waiting && deadline <= now_ticks)
                    .then_some((slot, deadline))
            })
            .min_by_key(|&(_slot, deadline)| deadline)?;
        Some(ObjectId {
            pool: PoolTag::Pending,
            index: u16::try_from(slot).ok()?,
            generation: self.generations[slot],
        })
    }

    /// Terminal transition to `Completed` with success status.
    ///
    /// Errors if the record is not `Waiting`: completion, timeout,
    /// cancellation, and teardown compete for exactly one terminal
    /// transition; a losing attempt cannot mutate the record.
    pub fn complete(&mut self, id: ObjectId, result0: u64, result1: u64) -> Result<(), CapError> {
        self.complete_with_status(id, syscall_status::SUCCESS, result0, result1)
    }

    /// Terminal transition to `Completed` with an explicit status — the
    /// full syscall result shape. An error status completes the invocation
    /// with a known failure (e.g. an `EventCount.Await` woken by an
    /// overflowing advance); it is a completion, not a cancellation.
    ///
    /// Same single-terminal-transition rule as [`Self::complete`].
    pub fn complete_with_status(
        &mut self,
        id: ObjectId,
        status: u64,
        result0: u64,
        result1: u64,
    ) -> Result<(), CapError> {
        let record = self.live_mut(id)?;
        match record.state {
            PendingState::Waiting => {
                record.state = PendingState::Completed {
                    status,
                    result0,
                    result1,
                };
                Ok(())
            }
            _ => Err(CapError::InvalidOperation),
        }
    }

    /// Terminal transition to `Cancelled` (teardown).
    ///
    /// Same single-terminal-transition rule as [`Self::complete`].
    pub fn cancel(&mut self, id: ObjectId) -> Result<(), CapError> {
        let record = self.live_mut(id)?;
        match record.state {
            PendingState::Waiting => {
                record.state = PendingState::Cancelled;
                Ok(())
            }
            _ => Err(CapError::InvalidOperation),
        }
    }

    /// The synchronization object whose wait queue holds the record.
    pub fn object(&self, id: ObjectId) -> Option<CarvedId> {
        self.live(id).ok().map(PendingInvocation::object)
    }

    /// The record's state, if the identity is live.
    pub fn state(&self, id: ObjectId) -> Result<PendingState, CapError> {
        self.live(id).map(PendingInvocation::state)
    }

    /// The waiter identity stored in a live record.
    pub fn waiter(&self, id: ObjectId) -> Result<ThreadId, CapError> {
        self.live(id).map(PendingInvocation::waiter)
    }

    /// What a live record's invocation waits on.
    pub fn kind(&self, id: ObjectId) -> Result<PendingKind, CapError> {
        self.live(id).map(PendingInvocation::kind)
    }

    /// Discard a record whose admission failed before the waiter blocked.
    ///
    /// Rollback half of validate → reserve → commit: the record was never
    /// externally visible, no terminal transition is owed, and the waiter is
    /// still executing and will observe the admission error. Returns whether
    /// a live record was discarded. Not a general release: after a terminal
    /// transition, use [`Self::release`] once the waiter has resumed.
    pub fn abandon(&mut self, id: ObjectId) -> bool {
        let slot = usize::from(id.index);
        if slot >= Self::CAPACITY || self.generations[slot] != id.generation {
            return false;
        }
        // Retain the generation: the identity is stale either way.
        self.records[slot].take().is_some()
    }

    /// Release a terminal record after the waiter has resumed and consumed
    /// its outcome. Retains the slot's generation for stale-identity
    /// rejection; the slot may be reused with an advanced generation.
    ///
    /// Releasing a `Waiting` record is an error: it would silently drop a
    /// still-blocked invocation. Admission-failure rollback must instead
    /// avoid registering the record (validate before reserve).
    pub fn release(&mut self, id: ObjectId) -> Result<(), CapError> {
        let record = self.live(id)?;
        match record.state {
            PendingState::Waiting => Err(CapError::InvalidOperation),
            PendingState::Completed { .. } | PendingState::Cancelled => {
                let slot = usize::from(id.index);
                self.records[slot] = None;
                Ok(())
            }
        }
    }

    /// The objects whose wait queues hold `waiter`'s still-waiting records,
    /// in pool order. A Thread waits on at most one object at a time today,
    /// but nothing here relies on that.
    pub fn waited_objects(&self, waiter: ThreadId) -> impl Iterator<Item = CarvedId> + '_ {
        self.records
            .iter()
            .flatten()
            .filter(move |record| record.waiter == waiter && record.state == PendingState::Waiting)
            .map(|record| record.object)
    }

    /// Thread-teardown sweep: give every live record naming `waiter` its
    /// terminal disposition and release it.
    ///
    /// The teardown orchestrator (`Nucleus::cancel_thread_pending`) first
    /// removes the waiter's records from every object wait queue, so a
    /// `Waiting` record found here is cancelled — teardown wins the
    /// terminal-transition rule — and released. Already-terminal records
    /// (completed or cancelled, but not yet delivered to the waiter) are
    /// released unchanged: a torn-down waiter never resumes, so nothing else
    /// would release them and the bounded slots must return to the
    /// reservation. Returns how many records were released.
    ///
    /// This is the Thread-teardown path, distinct from object teardown
    /// (`Notification::cancel_waiters` and `EventCount::cancel_waiters`),
    /// whose waiters are still alive and resume with their outcome.
    pub fn teardown_waiter(&mut self, waiter: ThreadId) -> Result<usize, CapError> {
        let mut released = 0;
        for slot in 0..Self::CAPACITY {
            // `PendingInvocation` is `Copy`: extract by value so the pool can
            // be mutated below without holding a borrow.
            let Some(record) = self.records[slot] else {
                continue;
            };
            if record.waiter != waiter {
                continue;
            }
            let id = ObjectId {
                pool: PoolTag::Pending,
                index: u16::try_from(slot).expect("pool capacity fits u16 slots"),
                generation: self.generations[slot],
            };
            if record.state == PendingState::Waiting {
                // Teardown's terminal transition. The orchestrator removed the
                // record from its wait queue, so no completion can compete;
                // an error here would mean a kernel bookkeeping bug.
                self.cancel(id)?;
            }
            // The released record retains its generation: the identity is
            // stale forever, and the slot may be reused with an advanced
            // generation.
            self.release(id)?;
            released += 1;
        }
        Ok(released)
    }

    /// Shared access to a live record after identity validation.
    fn live(&self, id: ObjectId) -> Result<&PendingInvocation, CapError> {
        if id.pool != PoolTag::Pending {
            return Err(CapError::InvalidOperation);
        }
        let slot = usize::from(id.index);
        if slot >= Self::CAPACITY {
            return Err(CapError::InvalidOperation);
        }
        match &self.records[slot] {
            // A live record with a matching generation is the only success
            // case; stale identities name a released slot and are rejected.
            Some(record) if self.generations[slot] == id.generation => Ok(record),
            _ => Err(CapError::InvalidOperation),
        }
    }

    /// Exclusive access to a live record after identity validation.
    fn live_mut(&mut self, id: ObjectId) -> Result<&mut PendingInvocation, CapError> {
        if id.pool != PoolTag::Pending {
            return Err(CapError::InvalidOperation);
        }
        let slot = usize::from(id.index);
        if slot >= Self::CAPACITY {
            return Err(CapError::InvalidOperation);
        }
        let generation = self.generations[slot];
        match &mut self.records[slot] {
            Some(record) if generation == id.generation => Ok(record),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// WAIT QUEUES
// ═══════════════════════════════════════════════════════════════════

/// Bounded FIFO of blocked waiters (pending-invocation record identities).
///
/// The bound is the per-object wait reservation: a full queue rejects the
/// blocking attempt before admission (the adopted rejected-before-admission
/// vocabulary) with `PoolExhausted`, rather than admitting unbounded
/// waiters. Callers validate queue space before registering the record, so
/// a pushed identity never needs rollback.
pub struct WaitQueue {
    queue: [ObjectId; Self::CAPACITY],
    head: usize,
    len: usize,
}

impl WaitQueue {
    /// Waiters per synchronization object. Bounded, explicit reservation.
    pub const CAPACITY: usize = 8;

    pub const fn new() -> Self {
        Self {
            queue: [ObjectId {
                pool: PoolTag::Region,
                index: 0,
                generation: 0,
            }; Self::CAPACITY],
            head: 0,
            len: 0,
        }
    }

    /// Number of queued waiters.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether no waiter is queued.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether the reservation is exhausted; check before registering a
    /// record so admission failure needs no rollback.
    pub fn is_full(&self) -> bool {
        self.len == Self::CAPACITY
    }

    /// Enqueue a waiter at the back. Fails with `PoolExhausted` when full.
    pub fn push(&mut self, id: ObjectId) -> Result<(), CapError> {
        if self.is_full() {
            return Err(CapError::PoolExhausted);
        }
        let back = (self.head + self.len) % Self::CAPACITY;
        self.queue[back] = id;
        self.len += 1;
        Ok(())
    }

    /// Dequeue the front (oldest) waiter.
    pub fn pop_front(&mut self) -> Option<ObjectId> {
        if self.len == 0 {
            return None;
        }
        let id = self.queue[self.head];
        self.queue[self.head] = ObjectId {
            pool: PoolTag::Region,
            index: 0,
            generation: 0,
        };
        self.head = (self.head + 1) % Self::CAPACITY;
        self.len -= 1;
        Some(id)
    }

    /// Remove a specific waiter (cancellation/teardown), preserving the
    /// order of the remaining waiters. Returns whether it was queued.
    pub fn remove(&mut self, id: ObjectId) -> bool {
        let mut found = None;
        for i in 0..self.len {
            if self.queue[(self.head + i) % Self::CAPACITY] == id {
                found = Some(i);
                break;
            }
        }
        let Some(pos) = found else {
            return false;
        };
        // Shift the waiters behind the removed entry forward, preserving
        // FIFO order.
        for i in pos..self.len - 1 {
            self.queue[(self.head + i) % Self::CAPACITY] =
                self.queue[(self.head + i + 1) % Self::CAPACITY];
        }
        self.queue[(self.head + self.len - 1) % Self::CAPACITY] = ObjectId {
            pool: PoolTag::Region,
            index: 0,
            generation: 0,
        };
        self.len -= 1;
        true
    }

    /// Remove every queued record naming `waiter` (Thread-teardown
    /// cancellation), preserving the FIFO order of the remaining waiters.
    ///
    /// A queue holds record identities; the waiter each record blocks is
    /// resolved through the pending pool. The removed records are not
    /// touched here — the teardown sweep (`PendingPool::teardown_waiter`)
    /// gives them their terminal transition and releases them, because a
    /// torn-down waiter never resumes to consume an outcome. Returns how
    /// many records were unqueued.
    pub fn remove_waiter(&mut self, waiter: ThreadId, pending: &PendingPool) -> usize {
        let live = self.len;
        let mut kept = 0;
        let mut removed = 0;
        for i in 0..live {
            let slot = (self.head + i) % Self::CAPACITY;
            let record = self.queue[slot];
            if matches!(pending.waiter(record), Ok(blocked) if blocked == waiter) {
                removed += 1;
            } else {
                // The compaction only writes to already-processed slots
                // (`kept <= i`), so it never loses an unprocessed entry.
                self.queue[(self.head + kept) % Self::CAPACITY] = record;
                kept += 1;
            }
        }
        // Clear the vacated tail slots and shrink the live region.
        for i in kept..live {
            self.queue[(self.head + i) % Self::CAPACITY] = ObjectId {
                pool: PoolTag::Region,
                index: 0,
                generation: 0,
            };
        }
        self.len = kept;
        removed
    }
}

#[cfg(test)]
mod tests {
    use {
        super::{PendingKind, PendingPool, PendingState, WaitQueue},
        crate::objects::{
            access::{ObjectId, PoolTag},
            carved::{CarvedId, test_id},
            thread::ThreadId,
        },
        libobject::{CapError, ObjectType, syscall_status},
    };

    /// The Notification the records wait on (never resolved here).
    fn object() -> CarvedId {
        test_id(ObjectType::NOTIFICATION, 0x1000)
    }

    /// A stand-in waiter identity: a Thread identity as stored by
    /// `block`.
    fn waiter(index: u16) -> ThreadId {
        test_id(
            libobject::ObjectType::THREAD,
            0x10_0000 + u64::from(index) * 0x1000,
        )
    }

    fn block(pool: &mut PendingPool, index: u16) -> ObjectId {
        match pool.block(waiter(index), PendingKind::NotificationWait, object()) {
            Ok(id) => id,
            Err(_) => panic!("block failed"),
        }
    }

    #[test_case]
    fn block_registers_waiting_record() {
        let mut pool = PendingPool::new();
        let id = block(&mut pool, 0);
        assert_eq!(id.pool, PoolTag::Pending);
        assert_eq!(id.generation, 1);
        assert_eq!(pool.state(id).ok(), Some(PendingState::Waiting));
        assert_eq!(pool.waiter(id).ok(), Some(waiter(0)));
        assert_eq!(pool.len(), 1);
    }

    #[test_case]
    fn exactly_one_terminal_transition_wins() {
        let mut pool = PendingPool::new();
        let id = block(&mut pool, 0);

        // The first terminal transition succeeds...
        assert!(pool.complete(id, 0xBEEF, 0).is_ok());
        assert_eq!(
            pool.state(id).ok(),
            Some(PendingState::Completed {
                status: syscall_status::SUCCESS,
                result0: 0xBEEF,
                result1: 0
            })
        );
        // ...and every competing transition observes an error instead of
        // mutating the record: the adopted terminal-transition rule.
        assert!(matches!(
            pool.complete(id, 1, 0),
            Err(CapError::InvalidOperation)
        ));
        assert!(matches!(pool.cancel(id), Err(CapError::InvalidOperation)));
        // The stored result is unchanged.
        assert_eq!(
            pool.state(id).ok(),
            Some(PendingState::Completed {
                status: syscall_status::SUCCESS,
                result0: 0xBEEF,
                result1: 0
            })
        );
    }

    #[test_case]
    fn error_completion_is_terminal_too() {
        let mut pool = PendingPool::new();
        let id = block(&mut pool, 0);

        // An error status completes the invocation with a known failure
        // (e.g. an Await woken by an overflowing advance): a completion,
        // not a cancellation.
        assert!(
            pool.complete_with_status(id, syscall_status::COUNTER_OVERFLOW, 0, 0)
                .is_ok()
        );
        assert_eq!(
            pool.state(id).ok(),
            Some(PendingState::Completed {
                status: syscall_status::COUNTER_OVERFLOW,
                result0: 0,
                result1: 0
            })
        );
        // The single-terminal-transition rule applies unchanged.
        assert!(matches!(
            pool.complete(id, 1, 0),
            Err(CapError::InvalidOperation)
        ));
        assert!(matches!(
            pool.complete_with_status(id, syscall_status::SUCCESS, 0, 0),
            Err(CapError::InvalidOperation)
        ));
        assert!(matches!(pool.cancel(id), Err(CapError::InvalidOperation)));
        // An error completion is releasable like any terminal record.
        assert!(pool.release(id).is_ok());
    }

    #[test_case]
    fn cancellation_is_terminal_too() {
        let mut pool = PendingPool::new();
        let id = block(&mut pool, 1);
        assert!(pool.cancel(id).is_ok());
        assert_eq!(pool.state(id).ok(), Some(PendingState::Cancelled));
        assert!(matches!(
            pool.complete(id, 0, 0),
            Err(CapError::InvalidOperation)
        ));
    }

    #[test_case]
    fn release_requires_terminal_and_rejects_stale_identities() {
        let mut pool = PendingPool::new();
        let id = block(&mut pool, 0);
        // Releasing a still-waiting record would drop a blocked invocation.
        assert!(matches!(pool.release(id), Err(CapError::InvalidOperation)));

        assert!(pool.complete(id, 7, 8).is_ok());
        assert!(pool.release(id).is_ok());
        assert_eq!(pool.len(), 0);
        // The released identity is stale and must not resolve.
        assert!(matches!(pool.state(id), Err(CapError::InvalidOperation)));

        // Force reuse of the released slot: fill every other slot so only the
        // released one remains free. The next registration must take it,
        // advancing the generation; the stale identity still must not resolve
        // to the replacement record. (The pool allocates next-fit, so an
        // unfilled pool would take a later slot instead.)
        for i in 1..PendingPool::CAPACITY {
            let index = u16::try_from(i).unwrap();
            block(&mut pool, index);
        }
        let next = block(&mut pool, 2);
        assert_eq!(next.index, id.index);
        assert_eq!(next.generation, id.generation + 1);
        assert!(matches!(pool.state(id), Err(CapError::InvalidOperation)));
        assert_eq!(pool.state(next).ok(), Some(PendingState::Waiting));
    }

    #[test_case]
    fn pool_exhaustion_rejects_new_records() {
        let mut pool = PendingPool::new();
        for i in 0..PendingPool::CAPACITY {
            let index = u16::try_from(i).unwrap();
            block(&mut pool, index);
        }
        assert_eq!(pool.len(), PendingPool::CAPACITY);
        assert!(matches!(
            pool.block(waiter(255), PendingKind::NotificationWait, object()),
            Err(CapError::PoolExhausted)
        ));
    }

    #[test_case]
    fn wait_queue_is_fifo_and_bounded() {
        let mut queue = WaitQueue::new();
        let first = ObjectId {
            pool: PoolTag::Pending,
            index: 0,
            generation: 1,
        };
        let second = ObjectId {
            pool: PoolTag::Pending,
            index: 1,
            generation: 1,
        };
        assert!(queue.push(first).is_ok());
        assert!(queue.push(second).is_ok());
        assert_eq!(queue.len(), 2);

        // Fill the remaining reservation; the next push is rejected before
        // admission rather than growing unbounded.
        for i in 2..WaitQueue::CAPACITY {
            let id = ObjectId {
                pool: PoolTag::Pending,
                index: u16::try_from(i).unwrap(),
                generation: 1,
            };
            assert!(queue.push(id).is_ok());
        }
        assert!(queue.is_full());
        let overflow = ObjectId {
            pool: PoolTag::Pending,
            index: 255,
            generation: 1,
        };
        assert!(matches!(queue.push(overflow), Err(CapError::PoolExhausted)));

        // FIFO: the oldest waiter is delivered first.
        assert_eq!(queue.pop_front(), Some(first));
        assert_eq!(queue.pop_front(), Some(second));
    }

    #[test_case]
    fn wait_queue_remove_preserves_order() {
        let mut queue = WaitQueue::new();
        let ids: [ObjectId; 3] = [
            ObjectId {
                pool: PoolTag::Pending,
                index: 0,
                generation: 1,
            },
            ObjectId {
                pool: PoolTag::Pending,
                index: 1,
                generation: 1,
            },
            ObjectId {
                pool: PoolTag::Pending,
                index: 2,
                generation: 1,
            },
        ];
        for id in ids {
            assert!(queue.push(id).is_ok());
        }
        // Remove the middle waiter; the remaining order is preserved.
        assert!(queue.remove(ids[1]));
        assert!(!queue.remove(ids[1]));
        assert_eq!(queue.pop_front(), Some(ids[0]));
        assert_eq!(queue.pop_front(), Some(ids[2]));
        assert_eq!(queue.pop_front(), None);
    }

    #[test_case]
    fn wait_queue_remove_waiter_unqueues_matching_records() {
        // Three distinct waiters, one record each, in queue order.
        let mut pending = PendingPool::new();
        let mut queue = WaitQueue::new();
        let mut records = [ObjectId {
            pool: PoolTag::Pending,
            index: 0,
            generation: 0,
        }; 3];
        for (i, record) in records.iter_mut().enumerate() {
            let index = u16::try_from(i).unwrap();
            *record = block(&mut pending, index);
            assert!(queue.push(*record).is_ok());
        }

        // Thread teardown of waiter 1: only its record leaves the queue, and
        // the FIFO order of the surviving waiters is preserved.
        assert_eq!(queue.remove_waiter(waiter(1), &pending), 1);
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.pop_front(), Some(records[0]));
        assert_eq!(queue.pop_front(), Some(records[2]));
        assert_eq!(queue.pop_front(), None);

        // A waiter with nothing queued removes nothing.
        assert_eq!(queue.remove_waiter(waiter(1), &pending), 0);
    }

    #[test_case]
    fn teardown_waiter_cancels_and_releases_the_waiters_records() {
        let mut pool = PendingPool::new();
        let gone = block(&mut pool, 0);
        let survivor = block(&mut pool, 1);

        // Teardown of waiter 0: its `Waiting` record receives its `Cancelled`
        // terminal transition (teardown wins) and is then released — a
        // torn-down waiter never resumes to consume the outcome.
        assert_eq!(pool.teardown_waiter(waiter(0)).ok(), Some(1));
        assert!(matches!(pool.state(gone), Err(CapError::InvalidOperation)));
        // The released reservation is not lost: a new blocking invocation
        // registers fine, and the released identity stays stale (the next-fit
        // cursor picks the slot, so reuse is not necessarily immediate).
        let replacement = block(&mut pool, 2);
        assert_eq!(pool.state(replacement).ok(), Some(PendingState::Waiting));
        assert!(matches!(pool.state(gone), Err(CapError::InvalidOperation)));

        // Another waiter's record is untouched.
        assert_eq!(pool.state(survivor).ok(), Some(PendingState::Waiting));
        assert_eq!(pool.len(), 2);

        // Releasing the replacement by hand leaves waiter 2 with nothing
        // pending: a further teardown sweep is an empty no-op.
        pool.cancel(replacement)
            .unwrap_or_else(|_| panic!("cancel failed"));
        pool.release(replacement)
            .unwrap_or_else(|_| panic!("release failed"));
        assert_eq!(pool.teardown_waiter(waiter(2)).ok(), Some(0));
    }

    #[test_case]
    fn teardown_waiter_releases_terminal_records_without_mutating_them() {
        let mut pool = PendingPool::new();
        let woken = block(&mut pool, 0);
        let cancelled = block(&mut pool, 1);
        // A completed-but-not-yet-delivered record (the waiter was woken but
        // has not resumed) and an object-teardown-cancelled record: both are
        // terminal, and teardown must release them unchanged rather than
        // compete for a second terminal transition.
        pool.complete_with_status(woken, syscall_status::SUCCESS, 0xaa, 0)
            .unwrap_or_else(|_| panic!("complete failed"));
        pool.cancel(cancelled)
            .unwrap_or_else(|_| panic!("cancel failed"));

        assert_eq!(pool.teardown_waiter(waiter(0)).ok(), Some(1));
        assert!(matches!(pool.state(woken), Err(CapError::InvalidOperation)));
        assert_eq!(pool.teardown_waiter(waiter(1)).ok(), Some(1));
        assert!(matches!(
            pool.state(cancelled),
            Err(CapError::InvalidOperation)
        ));
        assert!(pool.is_empty());
    }
}
