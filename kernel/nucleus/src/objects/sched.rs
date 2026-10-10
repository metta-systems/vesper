//! Minimal kernel scheduling substrate (completion foundation, 2026-09-16).
//!
//! This is kernel mechanism only — a FIFO of runnable Threads. Scheduling
//! *policy* stays in userspace (the contract's userspace-scheduling
//! boundary): the kernel only needs "run someone else" when a thread blocks,
//! and a place to queue threads whose pending invocations completed.
//!
//! Threads are carved from Untyped memory, so their number is bounded only by
//! memory. The queue therefore links through the Thread objects themselves
//! (`Thread::run_next`), as seL4 does, and needs no storage of its own.

use crate::objects::{Thread, carved::Carved, thread::ThreadId};

/// FIFO of runnable Threads, linked through the queued Threads.
///
/// A thread enters the queue when its pending invocation completes (wakeup)
/// or when it is created and awaits its first start. It leaves the queue
/// when the kernel switches to it. A Thread is queued at most once.
pub struct RunQueue {
    head: Option<ThreadId>,
    tail: Option<ThreadId>,
    len: usize,
}

/// The live Thread behind `id`, or `None` when the identity no longer
/// validates.
fn thread<'a>(id: ThreadId) -> Option<&'a mut Thread> {
    // SAFETY: Thread identities are minted by Retype or boot code for carved
    // records that are never freed, and the kernel lock serializes every
    // queue operation; no other reference to the Thread is live across one.
    unsafe {
        Carved::<Thread>::validate(id)
            .ok()
            .map(|thread| &mut *thread)
    }
}

impl RunQueue {
    pub const fn new() -> Self {
        Self {
            head: None,
            tail: None,
            len: 0,
        }
    }

    /// Number of queued threads.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether no thread is queued.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Enqueue a runnable Thread at the back. Returns `false` when the
    /// identity no longer validates or the Thread is already queued.
    pub fn push(&mut self, id: ThreadId) -> bool {
        let Some(entering) = thread(id) else {
            return false;
        };
        if entering.queued {
            return false;
        }
        entering.queued = true;
        entering.run_next = None;
        match self.tail.and_then(thread) {
            Some(last) => last.run_next = Some(id),
            None => self.head = Some(id),
        }
        self.tail = Some(id);
        self.len += 1;
        true
    }

    /// Inspect the front without consuming it. Scheduling validates the
    /// selected context before dequeueing under the same kernel lock.
    pub fn peek(&self) -> Option<ThreadId> {
        self.head
    }

    /// Dequeue the front (oldest) runnable Thread.
    pub fn pop(&mut self) -> Option<ThreadId> {
        let id = self.head?;
        // A queued Thread stays live: teardown removes it from the queue
        // before retiring it.
        let leaving = thread(id).expect("queued Thread retired while queued");
        self.head = leaving.run_next.take();
        leaving.queued = false;
        if self.head.is_none() {
            self.tail = None;
        }
        self.len -= 1;
        Some(id)
    }

    /// Remove `id` from the queue (thread-teardown cancellation), preserving
    /// the FIFO order of the remaining entries. A torn-down Thread must leave
    /// no queued wakeup behind. Returns whether it was queued.
    pub fn remove(&mut self, id: ThreadId) -> bool {
        let Some(removed) = thread(id) else {
            return false;
        };
        if !removed.queued {
            return false;
        }
        let next = removed.run_next.take();
        removed.queued = false;
        if self.head == Some(id) {
            self.head = next;
        } else {
            // Walk to the predecessor; the queue holds `id`, so it exists.
            let mut cursor = self.head;
            while let Some(current) = cursor {
                let previous = thread(current).expect("queued Thread retired while queued");
                if previous.run_next == Some(id) {
                    previous.run_next = next;
                    if self.tail == Some(id) {
                        self.tail = Some(current);
                    }
                    break;
                }
                cursor = previous.run_next;
            }
        }
        if self.head.is_none() {
            self.tail = None;
        }
        self.len -= 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use {
        super::RunQueue,
        crate::objects::{
            Thread,
            access::{ObjectId, PoolTag},
            carved::Carved,
            thread::{ExecutionContext, ThreadId},
        },
    };

    const STRIDE: usize = Carved::<Thread>::SIZE.next_multiple_of(64);
    const THREADS: usize = 4;

    #[repr(C, align(64))]
    struct Backing([u8; THREADS * STRIDE]);

    /// Carve `THREADS` fresh Threads into `backing`.
    fn threads(backing: &mut Backing) -> [ThreadId; THREADS] {
        let base = backing.0.as_mut_ptr();
        core::array::from_fn(|index| {
            // SAFETY: the backing is exclusively owned, aligned, and each
            // stride holds one carved Thread.
            unsafe {
                Carved::<Thread>::initialize(
                    base.add(index * STRIDE),
                    Thread::new(
                        ObjectId {
                            pool: PoolTag::AddressSpace,
                            index: 0,
                            generation: 1,
                        },
                        ExecutionContext::Running,
                    ),
                )
            }
        })
    }

    #[test_case]
    fn run_queue_is_fifo_and_queues_a_thread_once() {
        let mut backing = Backing([0; THREADS * STRIDE]);
        let [a, b, ..] = threads(&mut backing);
        let mut queue = RunQueue::new();
        assert!(queue.is_empty());
        assert_eq!(queue.pop(), None);
        assert_eq!(queue.peek(), None);

        assert!(queue.push(a));
        assert!(queue.push(b));
        // A queued Thread is not queued twice.
        assert!(!queue.push(a));
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.peek(), Some(a));
        // FIFO: the oldest entry leaves first, and can be queued again.
        assert_eq!(queue.pop(), Some(a));
        assert!(queue.push(a));
        assert_eq!(queue.pop(), Some(b));
        assert_eq!(queue.pop(), Some(a));
        assert!(queue.is_empty());
    }

    #[test_case]
    fn remove_purges_any_position_and_preserves_fifo_order() {
        let mut backing = Backing([0; THREADS * STRIDE]);
        let [a, b, c, d] = threads(&mut backing);
        let mut queue = RunQueue::new();
        for id in [a, b, c, d] {
            assert!(queue.push(id));
        }

        // Middle, head and tail removals keep the survivors in order.
        assert!(queue.remove(b));
        assert!(queue.remove(a));
        assert!(queue.remove(d));
        assert_eq!(queue.len(), 1);
        // The tail moved back to `c`: a new push links after it.
        assert!(queue.push(a));
        assert_eq!(queue.pop(), Some(c));
        assert_eq!(queue.pop(), Some(a));
        assert_eq!(queue.pop(), None);

        // Removing a Thread that is not queued is a no-op.
        assert!(!queue.remove(b));
    }

    #[test_case]
    fn retired_threads_are_not_queued() {
        let mut backing = Backing([0; THREADS * STRIDE]);
        let [a, ..] = threads(&mut backing);
        // SAFETY: the identity was issued for this backing; nothing holds it.
        unsafe { Carved::<Thread>::retire(a) }.ok().unwrap();
        let mut queue = RunQueue::new();
        assert!(!queue.push(a));
        assert!(queue.is_empty());
    }
}
