//! Host tests of the ticket lock: mutual exclusion under contention, first-come
//! first-served order, the holder record, and the checks that catch a holder taking the
//! lock again or a release by someone else.

#[cfg(test)]
mod tests {
    use {
        std::{
            cell::UnsafeCell,
            sync::{
                Arc, Mutex,
                atomic::{AtomicBool, Ordering},
            },
            thread,
        },
        vesper_locking::TicketLock,
    };

    /// A counter only the lock holder touches, with a flag that catches two holders at
    /// once.
    struct Guarded {
        lock: TicketLock,
        inside: AtomicBool,
        count: UnsafeCell<u64>,
    }

    // SAFETY: `count` is touched only by the lock holder.
    unsafe impl Sync for Guarded {}

    #[test]
    fn contended_lock_excludes_and_loses_no_update() {
        const HOLDERS: u32 = 8;
        const ROUNDS: u64 = 20_000;
        let guarded = Arc::new(Guarded {
            lock: TicketLock::new(),
            inside: AtomicBool::new(false),
            count: UnsafeCell::new(0),
        });
        let workers: Vec<_> = (1..=HOLDERS)
            .map(|holder| {
                let guarded = Arc::clone(&guarded);
                thread::spawn(move || {
                    for _ in 0..ROUNDS {
                        guarded.lock.acquire(holder);
                        assert!(guarded.lock.is_held_by(holder));
                        assert!(
                            !guarded.inside.swap(true, Ordering::Relaxed),
                            "two holders inside at once"
                        );
                        // SAFETY: only the holder touches the counter.
                        unsafe {
                            *guarded.count.get() += 1;
                        }
                        guarded.inside.store(false, Ordering::Relaxed);
                        guarded.lock.release(holder);
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().expect("a holder panicked");
        }
        // SAFETY: every holder has finished.
        let count = unsafe { *guarded.count.get() };
        assert_eq!(count, u64::from(HOLDERS) * ROUNDS);
        assert_eq!(guarded.lock.holder(), None);
    }

    /// The capabilities contract's payload rule (D7) on several cores: a producer
    /// writes its payload, then advances a counter under the lock (its `Advance`); a
    /// consumer that reads the advanced counter under the lock (its `Read`) then sees
    /// the payload after releasing it. The payload is written before the producer
    /// takes the lock and read after the consumer let it go, as user code does around
    /// its kernel entries.
    #[test]
    fn writes_before_a_release_are_seen_after_the_next_acquire() {
        const ROUNDS: u64 = 50_000;
        struct Channel {
            lock: TicketLock,
            /// The producer's payload, written outside the lock.
            payload: UnsafeCell<u64>,
            /// The counter, touched under the lock only.
            progress: UnsafeCell<u64>,
        }
        // SAFETY: `progress` is touched under the lock; `payload` is written by the
        // producer only before it advances, and read by the consumer only after it saw
        // that advance.
        unsafe impl Sync for Channel {}
        let channel = Arc::new(Channel {
            lock: TicketLock::new(),
            payload: UnsafeCell::new(0),
            progress: UnsafeCell::new(0),
        });
        let producer_channel = Arc::clone(&channel);
        let producer = thread::spawn(move || {
            for round in 1..=ROUNDS {
                // Wait until the consumer took the previous round.
                loop {
                    producer_channel.lock.acquire(1);
                    // SAFETY: under the lock.
                    let taken = unsafe { *producer_channel.progress.get() } % 2 == 0;
                    producer_channel.lock.release(1);
                    if taken {
                        break;
                    }
                    thread::yield_now();
                }
                // SAFETY: the consumer reads the payload only after this round's advance.
                unsafe {
                    *producer_channel.payload.get() = round * 3;
                }
                producer_channel.lock.acquire(1);
                // SAFETY: under the lock.
                unsafe {
                    *producer_channel.progress.get() += 1;
                }
                producer_channel.lock.release(1);
            }
        });
        for round in 1..=ROUNDS {
            loop {
                channel.lock.acquire(2);
                // SAFETY: under the lock.
                let advanced = unsafe { *channel.progress.get() } % 2 == 1;
                channel.lock.release(2);
                if advanced {
                    break;
                }
                thread::yield_now();
            }
            // SAFETY: the producer wrote it before the advance just seen, and writes it
            // again only after the next acknowledgement.
            let payload = unsafe { *channel.payload.get() };
            assert_eq!(payload, round * 3, "payload of round {round} not visible");
            channel.lock.acquire(2);
            // SAFETY: under the lock.
            unsafe {
                *channel.progress.get() += 1;
            }
            channel.lock.release(2);
        }
        producer.join().expect("the producer panicked");
    }

    #[test]
    fn waiters_enter_in_the_order_they_asked() {
        let lock = Arc::new(TicketLock::new());
        let order = Arc::new(Mutex::new(Vec::new()));
        lock.acquire(1);
        let mut waiters = Vec::new();
        for holder in 2..=4 {
            let (waiter_lock, waiter_order) = (Arc::clone(&lock), Arc::clone(&order));
            waiters.push(thread::spawn(move || {
                waiter_lock.acquire(holder);
                waiter_order.lock().expect("order list").push(holder);
                waiter_lock.release(holder);
            }));
            // The waiter has its ticket once the count of waiters grows.
            while lock.waiting() < holder - 1 {
                thread::yield_now();
            }
        }
        lock.release(1);
        for waiter in waiters {
            waiter.join().expect("a waiter panicked");
        }
        assert_eq!(*order.lock().expect("order list"), [2, 3, 4]);
    }

    #[test]
    fn the_lock_records_its_holder() {
        let lock = TicketLock::new();
        assert_eq!(lock.holder(), None);
        assert!(!lock.is_held_by(1));
        lock.acquire(3);
        assert_eq!(lock.holder(), Some(3));
        assert!(lock.is_held_by(3));
        assert!(!lock.is_held_by(1));
        assert_eq!(lock.waiting(), 0);
        lock.release(3);
        assert_eq!(lock.holder(), None);
        lock.acquire(1);
        assert!(lock.is_held_by(1));
        lock.release(1);
    }

    #[test]
    #[should_panic(expected = "taken again by its holder 2")]
    fn a_holder_asking_again_panics_instead_of_waiting_for_itself() {
        let lock = TicketLock::new();
        lock.acquire(2);
        lock.acquire(2);
    }

    #[test]
    #[should_panic(expected = "released by 5, which does not hold it")]
    fn a_release_by_another_holder_panics() {
        let lock = TicketLock::new();
        lock.acquire(4);
        lock.release(5);
    }

    #[test]
    #[should_panic(expected = "holder is nonzero")]
    fn holder_zero_is_refused() {
        TicketLock::new().acquire(0);
    }
}
