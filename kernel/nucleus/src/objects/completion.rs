//! Completion foundation: pending-invocation records and bounded wait queues
//! (Phase 6, selected 2026-09-16).
//!
//! A blocking invocation does not return until completion or cancellation
//! (selected D7 model): no "blocked" wire status exists, and the eventual
//! return is the completion or a cancellation error. While a caller is
//! blocked, its invocation is represented here as a pending-invocation
//! record — the bounded, kernel-private state the contract requires.
//!
//! The record is also the closed-wait identity: a blocked `Call`'s reply
//! phase is named by its record, never by a bare slot number or domain
//! index. Record identities reuse the checked `ObjectId` shape (pool tag +
//! slot + non-wrapping generation, the D3 kernel-allocation-identity
//! pattern), so stale identities cannot resolve after release and reuse.
//!
//! The terminal-transition rule (adopted 2026-09-16): reply, timeout,
//! cancellation, and teardown compete for exactly one terminal transition
//! of a record; the losers observe a defined error instead of mutating it.
//! This module enforces that rule as a state machine.

use {
    crate::objects::access::{ObjectId, PoolTag},
    libobject::CapError,
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
    /// in the first result word (one-consumer delivery, selected 2026-09-16).
    NotificationWait,
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
    /// Completed: the eventual result is stored. The waiter must still be
    /// resumed with it before the record is released.
    Completed { result0: u64, result1: u64 },
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
    /// The blocked domain (domains-pool identity, stored verbatim and
    /// validated against the pool when the record is resolved at wake time).
    waiter: ObjectId,
    /// What the invocation waits on.
    kind: PendingKind,
    state: PendingState,
}

impl PendingInvocation {
    /// The blocked domain's identity (validated at wake time).
    pub fn waiter(&self) -> ObjectId {
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
    /// against the domains pool when the record is resolved at wake time.
    /// Fails with `PoolExhausted` when every slot is live or exhausted.
    pub fn block(&mut self, waiter: ObjectId, kind: PendingKind) -> Result<ObjectId, CapError> {
        let slot = (0..Self::CAPACITY)
            .map(|i| (self.next_free + i) % Self::CAPACITY)
            .find(|&slot| self.records[slot].is_none() && self.generations[slot] != u32::MAX)
            .ok_or(CapError::PoolExhausted)?;
        self.next_free = (slot + 1) % Self::CAPACITY;

        // Exhausted generations were filtered above, so this cannot wrap.
        self.generations[slot] += 1;
        self.records[slot] = Some(PendingInvocation {
            waiter,
            kind,
            state: PendingState::Waiting,
        });
        Ok(ObjectId {
            pool: PoolTag::Pending,
            index: u16::try_from(slot).map_err(|_error| CapError::PoolExhausted)?,
            generation: self.generations[slot],
        })
    }

    /// Terminal transition to `Completed`.
    ///
    /// Errors if the record is not `Waiting`: reply, timeout, cancellation,
    /// and teardown compete for exactly one terminal transition (adopted
    /// 2026-09-16); the losing attempt observes this error instead of
    /// mutating the record.
    pub fn complete(&mut self, id: ObjectId, result0: u64, result1: u64) -> Result<(), CapError> {
        let record = self.live_mut(id)?;
        match record.state {
            PendingState::Waiting => {
                record.state = PendingState::Completed { result0, result1 };
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

    /// The record's state, if the identity is live.
    pub fn state(&self, id: ObjectId) -> Result<PendingState, CapError> {
        self.live(id).map(PendingInvocation::state)
    }

    /// The waiter identity stored in a live record.
    pub fn waiter(&self, id: ObjectId) -> Result<ObjectId, CapError> {
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
}

#[cfg(test)]
mod tests {
    use {
        super::{PendingKind, PendingPool, PendingState, WaitQueue},
        crate::objects::access::{ObjectId, PoolTag},
        libobject::CapError,
    };

    /// A stand-in waiter identity: a domains-pool identity as stored by
    /// `block`.
    fn waiter(index: u16) -> ObjectId {
        ObjectId {
            pool: PoolTag::Domain,
            index,
            generation: 1,
        }
    }

    fn block(pool: &mut PendingPool, index: u16) -> ObjectId {
        match pool.block(waiter(index), PendingKind::NotificationWait) {
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
                result0: 0xBEEF,
                result1: 0
            })
        );
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
            pool.block(waiter(255), PendingKind::NotificationWait),
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
}
