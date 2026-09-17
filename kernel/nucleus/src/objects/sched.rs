//! Minimal kernel scheduling substrate (completion foundation, 2026-09-16).
//!
//! This is kernel mechanism only — a bounded FIFO of runnable domain indices.
//! Scheduling *policy* stays in userspace (the contract's userspace-scheduling
//! boundary): the kernel only needs "run someone else" when a domain blocks,
//! and a place to queue domains whose pending invocations completed.
//!
//! The queue is bounded by the domain-pool slot count, so a completed record
//! can always enqueue its waiter; a full queue would be a kernel bookkeeping
//! bug, not an expected condition.

/// Bounded FIFO of runnable domain indices.
///
/// A domain enters the queue when its pending invocation completes (wakeup)
/// or when it is created and awaits its first start. It leaves the queue
/// when the kernel switches to it.
pub struct Scheduler {
    queue: [u16; Self::CAPACITY],
    head: usize,
    len: usize,
}

impl Scheduler {
    /// Bounded by the domain-pool slot count: every live domain could be
    /// runnable at once, and a wake must never be dropped for lack of queue.
    pub const CAPACITY: usize = crate::objects::ObjectPool::<crate::objects::Domain>::MAX_SLOTS;

    pub const fn new() -> Self {
        Self {
            queue: [0; Self::CAPACITY],
            head: 0,
            len: 0,
        }
    }

    /// Number of queued domains.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether no domain is queued.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Enqueue a runnable domain index. Returns `false` only when the queue
    /// is full — unreachable while capacity matches the domain pool, so a
    /// `false` return indicates a kernel bookkeeping bug.
    pub fn push(&mut self, index: u16) -> bool {
        if self.len == Self::CAPACITY {
            return false;
        }
        let back = (self.head + self.len) % Self::CAPACITY;
        self.queue[back] = index;
        self.len += 1;
        true
    }

    /// Dequeue the front (oldest) runnable domain index.
    pub fn pop(&mut self) -> Option<u16> {
        if self.len == 0 {
            return None;
        }
        let index = self.queue[self.head];
        self.queue[self.head] = 0;
        self.head = (self.head + 1) % Self::CAPACITY;
        self.len -= 1;
        Some(index)
    }
}

#[cfg(test)]
mod tests {
    use super::Scheduler;

    #[test_case]
    fn scheduler_is_fifo_and_bounded() {
        let mut scheduler = Scheduler::new();
        assert!(scheduler.is_empty());
        assert_eq!(scheduler.pop(), None);

        assert!(scheduler.push(3));
        assert!(scheduler.push(7));
        assert_eq!(scheduler.len(), 2);
        // FIFO: the oldest entry leaves first.
        assert_eq!(scheduler.pop(), Some(3));
        assert_eq!(scheduler.pop(), Some(7));
        assert!(scheduler.is_empty());
    }

    #[test_case]
    fn scheduler_ring_wraps_around() {
        let mut scheduler = Scheduler::new();
        // Cycle through the whole ring once, then verify the wrap preserves
        // FIFO order across the boundary.
        for round in 0..2_u16 {
            for i in 0..16_u16 {
                assert!(scheduler.push(round * 16 + i));
            }
            for i in 0..16_u16 {
                assert_eq!(scheduler.pop(), Some(round * 16 + i));
            }
        }
        assert!(scheduler.is_empty());
    }
}
