//! A ticket spinlock: cores take it in the order they asked (SMP design, "Kernel lock").
//!
//! The lock records which holder has it, so a holder that asks again is caught instead
//! of waiting for itself forever, and code that needs the lock can check that its caller
//! holds it. Holders are named by a nonzero id (the nucleus uses its core's logical index
//! plus one).
//!
//! It needs atomic read-modify-write on coherent memory: on the Raspberry Pi's cores, the
//! MMU and caches on. The Cortex-A53 and A72 have no LSE, so the ticket is taken with an
//! exclusive load/store pair.

use core::{
    hint::spin_loop,
    sync::atomic::{AtomicU32, Ordering},
};

/// The holder value of a free lock.
const NO_HOLDER: u32 = 0;

/// A ticket spinlock guarding no data of its own.
#[derive(Debug)]
pub struct TicketLock {
    /// The next ticket handed out.
    next: AtomicU32,
    /// The ticket allowed in.
    serving: AtomicU32,
    /// Who holds it, or [`NO_HOLDER`].
    holder: AtomicU32,
}

impl Default for TicketLock {
    fn default() -> Self {
        Self::new()
    }
}

impl TicketLock {
    /// A free lock.
    pub const fn new() -> Self {
        Self {
            next: AtomicU32::new(0),
            serving: AtomicU32::new(0),
            holder: AtomicU32::new(NO_HOLDER),
        }
    }

    /// Take the lock for `holder`, waiting for the holders that asked before it.
    ///
    /// # Panics
    ///
    /// If `holder` is zero, or already holds the lock: it would wait for itself.
    pub fn acquire(&self, holder: u32) {
        // Only `holder` itself stores its id, so the check in `acquire_while` sees its
        // own last store.
        self.acquire_while(holder, || true);
    }

    /// Take the lock for `holder` as [`acquire`](Self::acquire) does, but stop waiting
    /// once `keep_waiting` says no; whether the lock was taken. A holder that stops
    /// leaves its ticket behind, so the lock is never free again: for a system that has
    /// stopped, such as after a panic.
    ///
    /// # Panics
    ///
    /// If `holder` is zero, or already holds the lock.
    pub fn acquire_while(&self, holder: u32, keep_waiting: impl Fn() -> bool) -> bool {
        assert_ne!(holder, NO_HOLDER, "a ticket lock holder is nonzero");
        assert_ne!(
            self.holder.load(Ordering::Relaxed),
            holder,
            "ticket lock taken again by its holder {holder}"
        );
        let ticket = self.next.fetch_add(1, Ordering::Relaxed);
        while self.serving.load(Ordering::Acquire) != ticket {
            if !keep_waiting() {
                return false;
            }
            spin_loop();
        }
        self.holder.store(holder, Ordering::Relaxed);
        true
    }

    /// Take the lock for `holder` if it is free right now, without waiting; whether it
    /// was taken. For a path that must not wait on a lock that may never be released,
    /// such as a panic.
    ///
    /// # Panics
    ///
    /// If `holder` is zero, or already holds the lock.
    pub fn try_acquire(&self, holder: u32) -> bool {
        assert_ne!(holder, NO_HOLDER, "a ticket lock holder is nonzero");
        assert_ne!(
            self.holder.load(Ordering::Relaxed),
            holder,
            "ticket lock taken again by its holder {holder}"
        );
        // Free means no ticket is out: `next == serving`. Taking ticket `serving` then
        // makes it ours at once; a ticket taken by anyone in between fails the exchange.
        let serving = self.serving.load(Ordering::Acquire);
        if self
            .next
            .compare_exchange(
                serving,
                serving.wrapping_add(1),
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_err()
        {
            return false;
        }
        self.holder.store(holder, Ordering::Relaxed);
        true
    }

    /// Let the next waiter in.
    ///
    /// # Panics
    ///
    /// If `holder` does not hold the lock.
    pub fn release(&self, holder: u32) {
        assert!(
            self.is_held_by(holder),
            "ticket lock released by {holder}, which does not hold it"
        );
        self.holder.store(NO_HOLDER, Ordering::Relaxed);
        // Only the holder moves `serving`.
        let serving = self.serving.load(Ordering::Relaxed);
        self.serving
            .store(serving.wrapping_add(1), Ordering::Release);
    }

    /// Whether `holder` holds the lock. Exact for the asking holder itself; for anyone
    /// else a snapshot.
    pub fn is_held_by(&self, holder: u32) -> bool {
        holder != NO_HOLDER && self.holder.load(Ordering::Relaxed) == holder
    }

    /// The holder, if any (a snapshot).
    pub fn holder(&self) -> Option<u32> {
        match self.holder.load(Ordering::Relaxed) {
            NO_HOLDER => None,
            holder => Some(holder),
        }
    }

    /// How many holders wait for the lock, besides the one that has it (a snapshot).
    pub fn waiting(&self) -> u32 {
        let next = self.next.load(Ordering::Relaxed);
        let serving = self.serving.load(Ordering::Relaxed);
        next.wrapping_sub(serving).saturating_sub(1)
    }
}
