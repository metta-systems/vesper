//! Notification kernel object: a word-sized coalescing signal bitmap with
//! one-consumer waiter delivery (selected 2026-09-16).
//!
//! Realizes the excluded sketch's intent — bitmap state, blocked waiters,
//! and `signal`/`wait`/`poll` semantics ("if bits are already set, clear and
//! immediately return; otherwise block the domain") — on the completion
//! foundation's pending-invocation records. The sketch's handler-side intent
//! (SEND/RECV rights and badge-or-argument signal bits) stays recorded in
//! the excluded `api/notification.rs` until the Signal authority decision
//! (D4) is settled; this object is the state core only.
//!
//! Contract (see `doc/nucleus_capabilities.md`): `Signal` ORs authorized
//! bits and wakes at most one waiter, which consumes the delivered bitmap
//! (one-consumer; broadcast-style observation uses `EventCount` independent
//! readers); `Wait` blocks until bits are pending and consumes them; `Poll`
//! consumes immediately or returns zero (no pending bits). Repeated signals
//! to a bit coalesce.

use {
    crate::objects::{
        NucleusObject,
        completion::{PendingKind, PendingPool, WaitQueue},
    },
    libobject::{CapError, ObjectType},
};

use crate::objects::access::{ObjectId, PoolTag};

/// Notification kernel object.
///
/// Pool-backed kernel metadata (Retype-creatable, selected 2026-09-16): the
/// capability is a checked pool identity, like page-table metadata.
pub struct Notification {
    /// Pending signal bits (coalescing bitmap).
    ///
    /// Invariant: nonzero only while no waiter is queued — a queued waiter
    /// either consumed the pending bits or awaits the next signal, which is
    /// delivered to it directly. Single-core execution under the kernel lock
    /// keeps this invariant race-free.
    state: u64,
    /// Blocked waiters, FIFO, bounded (the per-object wait reservation).
    waiters: WaitQueue,
}

/// Outcome of a `Wait` attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitOutcome {
    /// Already-satisfied wait: these bits were pending and are consumed;
    /// the invocation returns immediately with them.
    Ready(u64),
    /// No bits pending: the caller blocks. The identity names its
    /// pending-invocation record.
    Blocked(ObjectId),
}

impl NucleusObject for Notification {
    const TYPE: ObjectType = ObjectType::NOTIFICATION;
    const POOL: PoolTag = PoolTag::Notification;
}

impl Notification {
    pub const fn new() -> Self {
        Self {
            state: 0,
            waiters: WaitQueue::new(),
        }
    }

    /// Pending bits (diagnostics and tests).
    pub fn pending_bits(&self) -> u64 {
        self.state
    }

    /// `Signal`: OR `bits` into the pending state; if a waiter is queued,
    /// deliver the pending bitmap to the front waiter by completing its
    /// pending record, consuming the bits.
    ///
    /// One-consumer contract: at most one waiter wakes per signal. Returns
    /// the woken record's identity for the scheduler to resume, if any.
    pub fn signal(
        &mut self,
        bits: u64,
        pending: &mut PendingPool,
    ) -> Result<Option<ObjectId>, CapError> {
        if let Some(record) = self.waiters.pop_front() {
            // The queue invariant guarantees `state` is zero here; OR it in
            // defensively so no pending bit is lost if the invariant is ever
            // revisited.
            let delivered = self.state | bits;
            self.state = 0;
            // A queued record is always `Waiting` under the kernel lock
            // (teardown removes cancelled records from the queue), so this
            // terminal transition is the winner by construction; an error
            // here would indicate a kernel bookkeeping bug.
            pending.complete(record, delivered, 0)?;
            Ok(Some(record))
        } else {
            self.state |= bits;
            Ok(None)
        }
    }

    /// `Wait`: consume pending bits immediately if any (already-satisfied
    /// wait); otherwise register the caller's pending record in the queue
    /// and report the caller as blocked.
    ///
    /// The queue reservation is validated before the record is registered,
    /// so a full queue rejects the attempt before admission with no record
    /// to roll back.
    pub fn wait(
        &mut self,
        waiter: ObjectId,
        pending: &mut PendingPool,
    ) -> Result<WaitOutcome, CapError> {
        if self.state != 0 {
            let bits = self.state;
            self.state = 0;
            Ok(WaitOutcome::Ready(bits))
        } else {
            if self.waiters.is_full() {
                return Err(CapError::PoolExhausted);
            }
            let record = pending.block(waiter, PendingKind::NotificationWait)?;
            // The reservation was validated above and the kernel lock
            // excludes concurrent admission, so this cannot fail; if it ever
            // does, abandon the registered record rather than leaking its
            // pool slot.
            match self.waiters.push(record) {
                Ok(()) => Ok(WaitOutcome::Blocked(record)),
                Err(error) => {
                    pending.abandon(record);
                    Err(error)
                }
            }
        }
    }

    /// `Poll`: consume and return pending bits, or zero when none are
    /// pending. Never blocks.
    pub fn poll(&mut self) -> u64 {
        let bits = self.state;
        self.state = 0;
        bits
    }

    /// Teardown: cancel every queued waiter and clear the queue.
    ///
    /// Each record receives its single terminal transition (`Cancelled`);
    /// the cancelled waiters resume with a cancellation error. The pending
    /// records are released by the resumption path, not here.
    pub fn cancel_waiters(&mut self, pending: &mut PendingPool) -> Result<(), CapError> {
        while let Some(record) = self.waiters.pop_front() {
            // A queued record is `Waiting` by construction; an error here
            // would mean two terminal transitions raced, which the kernel
            // lock excludes.
            pending.cancel(record)?;
        }
        Ok(())
    }

    /// Domain-teardown cancellation: stop holding the torn-down `waiter`'s
    /// queued records, preserving the FIFO order of the remaining waiters.
    ///
    /// Distinct from object teardown ([`Self::cancel_waiters`]): the waiter's
    /// Domain is going away, so its records are cancelled and released by
    /// the pending-pool teardown sweep (`PendingPool::teardown_waiter`)
    /// instead of being left terminal for a resume that never happens.
    /// Returns how many of the waiter's records were unqueued.
    pub fn remove_waiter(&mut self, waiter: ObjectId, pending: &PendingPool) -> usize {
        self.waiters.remove_waiter(waiter, pending)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::{Notification, WaitOutcome},
        crate::objects::{
            access::{ObjectId, PoolTag},
            completion::{PendingKind, PendingPool, PendingState, WaitQueue},
        },
        libobject::{CapError, syscall_status},
    };

    fn waiter(index: u16) -> ObjectId {
        ObjectId {
            pool: PoolTag::Thread,
            index,
            generation: 1,
        }
    }

    /// Block a waiter and return its pending-record identity.
    fn block_waiter(
        notification: &mut Notification,
        pending: &mut PendingPool,
        index: u16,
    ) -> ObjectId {
        match notification.wait(waiter(index), pending) {
            Ok(WaitOutcome::Blocked(record)) => record,
            _ => panic!("wait should block"),
        }
    }

    #[test_case]
    fn signal_without_waiter_coalesces_bits() {
        let mut pending = PendingPool::new();
        let mut notification = Notification::new();

        assert!(matches!(notification.signal(0b1, &mut pending), Ok(None)));
        assert!(matches!(notification.signal(0b10, &mut pending), Ok(None)));
        // Repeated signals to the same bit coalesce; distinct bits OR.
        assert!(matches!(notification.signal(0b1, &mut pending), Ok(None)));
        assert_eq!(notification.pending_bits(), 0b11);
        assert!(pending.is_empty());
    }

    #[test_case]
    fn wait_consumes_pending_bits_immediately() {
        let mut pending = PendingPool::new();
        let mut notification = Notification::new();

        assert!(matches!(notification.signal(0b101, &mut pending), Ok(None)));
        // Already-satisfied wait: the bits are consumed and returned without
        // registering a record.
        assert_eq!(
            notification.wait(waiter(0), &mut pending).ok(),
            Some(WaitOutcome::Ready(0b101))
        );
        assert_eq!(notification.pending_bits(), 0);
        assert!(pending.is_empty());
    }

    #[test_case]
    fn wait_without_bits_blocks_and_signal_delivers() {
        let mut pending = PendingPool::new();
        let mut notification = Notification::new();

        let record = block_waiter(&mut notification, &mut pending, 3);
        assert_eq!(pending.state(record).ok(), Some(PendingState::Waiting));
        assert_eq!(pending.waiter(record).ok(), Some(waiter(3)));

        // The signal is delivered to the blocked waiter (one consumer): its
        // record completes with the delivered bitmap and the scheduler is
        // told which record to resume.
        assert!(matches!(
            notification.signal(0b110, &mut pending),
            Ok(Some(woken)) if woken == record
        ));
        assert_eq!(
            pending.state(record).ok(),
            Some(PendingState::Completed {
                status: syscall_status::SUCCESS,
                result0: 0b110,
                result1: 0
            })
        );
        // The delivered bitmap was consumed, not left pending.
        assert_eq!(notification.pending_bits(), 0);
    }

    #[test_case]
    fn one_consumer_wakes_one_waiter_per_signal() {
        let mut pending = PendingPool::new();
        let mut notification = Notification::new();

        let first = block_waiter(&mut notification, &mut pending, 0);
        let second = block_waiter(&mut notification, &mut pending, 1);

        // One signal wakes exactly the front (oldest) waiter.
        assert!(matches!(
            notification.signal(0b1, &mut pending),
            Ok(Some(woken)) if woken == first
        ));
        assert_eq!(pending.state(second).ok(), Some(PendingState::Waiting));
        // The next signal wakes the next waiter.
        assert!(matches!(
            notification.signal(0b10, &mut pending),
            Ok(Some(woken)) if woken == second
        ));
        assert_eq!(
            pending.state(second).ok(),
            Some(PendingState::Completed {
                status: syscall_status::SUCCESS,
                result0: 0b10,
                result1: 0
            })
        );
    }

    #[test_case]
    fn poll_consumes_or_reports_no_pending_bits() {
        let mut pending = PendingPool::new();
        let mut notification = Notification::new();

        // No bits pending: zero, never blocking.
        assert_eq!(notification.poll(), 0);
        assert!(matches!(notification.signal(0b1, &mut pending), Ok(None)));
        assert_eq!(notification.poll(), 0b1);
        assert_eq!(notification.poll(), 0);
    }

    #[test_case]
    fn full_queue_rejects_before_admission_without_leaking_records() {
        let mut pending = PendingPool::new();
        let mut notification = Notification::new();

        // Fill the wait reservation.
        for i in 0..WaitQueue::CAPACITY {
            let index = u16::try_from(i).unwrap();
            block_waiter(&mut notification, &mut pending, index);
        }
        let before = pending.len();

        // The next wait is rejected before admission: no record registered,
        // nothing to roll back.
        assert!(matches!(
            notification.wait(waiter(255), &mut pending),
            Err(CapError::PoolExhausted)
        ));
        assert_eq!(pending.len(), before);
    }

    #[test_case]
    fn teardown_cancels_queued_waiters() {
        let mut pending = PendingPool::new();
        let mut notification = Notification::new();

        let first = block_waiter(&mut notification, &mut pending, 0);
        let second = block_waiter(&mut notification, &mut pending, 1);

        assert!(notification.cancel_waiters(&mut pending).is_ok());
        assert_eq!(pending.state(first).ok(), Some(PendingState::Cancelled));
        assert_eq!(pending.state(second).ok(), Some(PendingState::Cancelled));

        // After teardown the queue is empty: a further signal just sets bits.
        assert!(matches!(notification.signal(0b1, &mut pending), Ok(None)));
        assert_eq!(notification.pending_bits(), 0b1);
    }

    #[test_case]
    fn domain_teardown_unqueues_only_the_torn_down_waiter() {
        let mut pending = PendingPool::new();
        let mut notification = Notification::new();

        // Two waiters: the one torn down is at the front of the queue.
        let gone = block_waiter(&mut notification, &mut pending, 0);
        let survivor = block_waiter(&mut notification, &mut pending, 1);

        // Unqueue the torn-down Domain's record; the sweep (not the object)
        // gives it its terminal transition and releases it.
        assert_eq!(notification.remove_waiter(waiter(0), &pending), 1);
        assert_eq!(pending.teardown_waiter(waiter(0)).ok(), Some(1));
        assert!(matches!(
            pending.state(gone),
            Err(CapError::InvalidOperation)
        ));

        // The next signal delivers to the surviving waiter, not to the dead
        // one: one-consumer delivery continues over the remaining queue.
        assert!(matches!(
            notification.signal(0b110, &mut pending),
            Ok(Some(woken)) if woken == survivor
        ));
        assert_eq!(
            pending.state(survivor).ok(),
            Some(PendingState::Completed {
                status: syscall_status::SUCCESS,
                result0: 0b110,
                result1: 0
            })
        );

        // A Domain with nothing queued removes nothing.
        assert_eq!(notification.remove_waiter(waiter(0), &pending), 0);
    }
}
