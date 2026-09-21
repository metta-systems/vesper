use {
    crate::objects::{
        ArchObjects, EventCount, KeyTable, Notification, ObjectPool, PendingPool, Scheduler,
        Thread, access::ObjectId, arch::ArchPools, domain::DcbPages, thread::ExecutionContext,
    },
    core::sync::atomic::Ordering,
    libobject::{
        CapError, KeySlot, RawKey,
        domain::{BlockReason, DomainControlBlock, DomainId, DomainState},
    },
};

#[cfg(feature = "debug_kernel")]
use crate::{api::key_entry::KeyEntry, objects::DebugConsole};

// ┌─────────────────────────────────────────────────────────────────────┐
// │                    KERNEL TYPE STRUCTURE                            │
// ├─────────────────────────────────────────────────────────────────────┤
// │                                                                     │
// │  Kernel<A: ArchObjects>                                             │
//  │  │                                                                  │
//  │  ├── pools: KernelPools<A>                                          │
//  │  │   ├── untypeds: ObjectPool<Untyped>                              │
//  │  │   ├── threads: ObjectPool<Thread>                                │
//  │  │   ├── keytables: carved by Retype (no pool; see api::untyped)     │
//  │  │   ├── notifications: ObjectPool<Notification>                    │
//  │  │   ├── event_counts: ObjectPool<EventCount>                       │
//  │  │   ├── endpoints: ObjectPool<Endpoint>                            │
//  │  │   ├── time slices: ObjectPool<TimeSlice>                         │
//  │  │   ├── replies: ObjectPool<Reply>                                 │
//  │  │   │                                                              │
//  │  │   └── arch: ArchPools<A>                                         │
//  │  │       ├── frames: inline regions (no pool)                       │
//  │  │       ├── page_tables: ObjectPool<A::PageTable>                  │
//  │  │       ├── address_spaces: ObjectPool<A::AddressSpace>            │
//  │  │       ├── asid_pools: ObjectPool<A::ASIDPool>                    │
//  │  │       └── asid_controls: reserved with the kind                   │
//  │  │                                                                  │
//  │  ├── current_thread: Option<ThreadId>                              │
//  │  ├── dcb_pages: DcbPages (thread scheduling pages, D5)              │
//  │  └── pending: PendingPool (blocked-invocation records, 2026-09-16)  │
//  │                                                                     │
//  └─────────────────────────────────────────────────────────────────┘

// ═══════════════════════════════════════════════════════════════════
// UNIFIED KERNEL OBJECT MANAGEMENT
// ═══════════════════════════════════════════════════════════════════

/// All kernel object pools - both core and architecture-specific
pub struct NucleusPools<A: ArchObjects> {
    // ─── Core Object Pools ───
    // pub untypeds: ObjectPool<Untyped>,
    /// Threads: the execution/scheduling remainder of the former Domain
    /// (split 2026-09-21). Each Thread references its `AddressSpace` and its
    /// carved `KeyTable`.
    pub threads: ObjectPool<Thread>,
    /// Notification synchronization objects: pure kernel state, allocated by
    /// `Untyped.Retype` (allowlisted 2026-09-16) from this bootstrap-carved
    /// pool; the capability is a checked pool identity.
    pub notifications: ObjectPool<Notification>,
    /// `EventCount` synchronization objects: pure kernel state, allocated by
    /// `Untyped.Retype` (allowlisted 2026-09-18) from this bootstrap-carved
    /// pool; the capability is a checked pool identity.
    pub event_counts: ObjectPool<EventCount>,
    // pub endpoints: ObjectPool<Endpoint>,
    // pub time_slices: ObjectPool<TimeSlice>,
    // pub replies: ObjectPool<Reply>,

    // ─── Architecture-Specific Pools ───
    pub arch: ArchPools<A>,
}

/// Complete nucleus state (parameterized by architecture)
pub struct Nucleus<A: ArchObjects> {
    /// All object pools
    pub pools: NucleusPools<A>,
    /// Currently running thread
    pub current_thread: Option<u32 /*ThreadId*/>, // FIXME: not option, always something (Idle or other)
    /// DCB shared pages
    pub dcb_pages: DcbPages,
    /// Pending-invocation records for blocked callers (completion
    /// foundation, 2026-09-16): bounded kernel-private storage, the
    /// closed-wait identity for blocked invocations.
    pub pending: PendingPool,
    /// Runnable-domain queue: the minimal "run someone else" substrate
    /// (completion foundation, 2026-09-16). Kernel mechanism only;
    /// scheduling policy stays in userspace.
    pub scheduler: Scheduler,
}

// ═══════════════════════════════════════════════════════════════════
// KERNEL INTEGRATION
// ═══════════════════════════════════════════════════════════════════

impl<A: ArchObjects> Nucleus<A> {
    #[expect(clippy::unused_self)]
    pub fn current_cpu(&self) -> usize {
        0
    }

    #[expect(clippy::unused_self)]
    pub fn current_time_ns(&self) -> u64 {
        0
    }

    /// Nucleus-private thread data, like keytables
    pub fn current_thread_mut(&mut self) -> Option<&mut Thread> {
        // need objects::Thread here, not DCB! or a tuple
        let id = self.current_thread?;
        self.pools.threads.get_live_mut(usize::try_from(id).ok()?)
    }

    /// Shared access to the current thread's capability table.
    pub fn current_thread_table(&self) -> Option<&KeyTable> {
        let addr = self.current_thread_table_addr()?;
        // SAFETY: the thread's table address is kernel-issued (carved by Retype
        // or the boot carve) and the region is never freed under accepted-leak.
        Some(unsafe { &*(addr as *const KeyTable) })
    }

    /// Exclusive access to the current thread's capability table.
    pub fn current_thread_table_mut(&mut self) -> Option<&mut KeyTable> {
        let addr = self.current_thread_table_addr()?;
        // SAFETY: see current_thread_table; &mut self guarantees exclusivity.
        Some(unsafe { &mut *(addr as *mut KeyTable) })
    }

    /// Address of the current thread's capability table (a carved `KeyTable`).
    pub fn current_thread_table_addr(&self) -> Option<u64> {
        let id = self.current_thread?;
        let thread = self.pools.threads.get_live(usize::try_from(id).ok()?)?;
        Some(thread.keytable_addr)
    }

    /// User-visible DCB
    pub fn current_dcb_mut(&mut self) -> Option<&mut DomainControlBlock> {
        // need objects::Thread here, not DCB! or a tuple
        let id = self.current_thread?;
        self.dcb_pages.get_mut(DomainId(id))
    }

    // TODO: Testing fixture
    #[cfg_attr(
        feature = "debug_kernel",
        expect(
            clippy::unnecessary_wraps,
            reason = "feature-off bootstrap has no console key"
        )
    )]
    pub fn create_thread(&mut self, keytable_addr: u64, address_space: ObjectId) -> Option<RawKey> {
        // Allocate the Thread itself; its capability table is a carved KeyTable
        // provided by the caller (Retype or the boot carve), and its address
        // space is the checked identity of a live AddressSpace.
        let (_thread_id, _thread) = self.pools.threads.allocate(Thread {
            keytable_addr,
            address_space,
            context: ExecutionContext::Running,
        })?;
        #[cfg(feature = "debug_kernel")]
        {
            // The debug console is a stateless singleton, not a pool object;
            // its entry carries a null identity and is validated by type only
            // (see the debug-only exception in doc/nucleus_capabilities.md).
            // SAFETY: the caller supplied a live carved table address; the
            // region is never freed under the accepted-leak model.
            let keytable = unsafe { &mut *(keytable_addr as *mut KeyTable) };
            let key = keytable
                .insert(
                    KeySlot::DEBUG_CONSOLE,
                    KeyEntry::from_id(
                        libobject::ObjectType::DEBUG_CONSOLE,
                        crate::objects::access::ObjectId {
                            pool: crate::objects::access::PoolTag::Region,
                            index: 0,
                            generation: 0,
                        },
                        libobject::Rights::all(),
                        0,
                    ),
                )
                .unwrap_or_else(|failure| {
                    panic!(
                        "bootstrap console installation failed: {:?}",
                        failure.error.code()
                    )
                });
            Some(key)
        }
        #[cfg(not(feature = "debug_kernel"))]
        None
    }

    /// Update the DCB when a thread is activated
    pub fn activate_thread(&mut self, id: DomainId, time_budget_ns: u64) {
        let cpu = self.current_cpu();
        let time = self.current_time_ns();
        if let Some(dcb) = self.dcb_pages.get_mut(id) {
            // Update time budget
            dcb.time_remaining_ns
                .store(time_budget_ns, Ordering::Relaxed);
            dcb.last_activated_ns.store(time, Ordering::Relaxed);
            dcb.activation_count.fetch_add(1, Ordering::Relaxed);
            dcb.cpu
                .store(u32::try_from(cpu).unwrap(), Ordering::Relaxed);

            // Set state last (Release ensures all above writes are visible)
            dcb.state
                .store(DomainState::Running as u32, Ordering::Release);
        }
    }

    /// Update the DCB when a thread yields/blocks/faults
    pub fn deactivate_thread(&mut self, id: DomainId, reason: DeactivateReason) {
        let elapsed = 0; //self.time_since_activation(id);

        if let Some(dcb) = self.dcb_pages.get_mut(id) {
            // Update time accounting
            dcb.time_consumed_ns.fetch_add(elapsed, Ordering::Relaxed);
            dcb.time_remaining_ns.fetch_sub(
                elapsed.min(dcb.time_remaining_ns.load(Ordering::Relaxed)),
                Ordering::Relaxed,
            );

            // Update state based on reason
            match reason {
                DeactivateReason::TimeExhausted | DeactivateReason::Yielded => {
                    dcb.state
                        .store(DomainState::Runnable as u32, Ordering::Release);
                }

                DeactivateReason::Blocked { reason, slot } => {
                    dcb.block_reason.store(reason as u32, Ordering::Relaxed);
                    dcb.blocked_on_slot.store(slot.0, Ordering::Relaxed);
                    dcb.state
                        .store(DomainState::Blocked as u32, Ordering::Release);
                }

                DeactivateReason::Faulted {
                    fault_type,
                    code,
                    addr,
                    slot,
                } => {
                    dcb.fault_type.store(fault_type as u32, Ordering::Relaxed);
                    dcb.fault_code.store(code, Ordering::Relaxed);
                    dcb.fault_addr.store(addr, Ordering::Relaxed);
                    dcb.fault_slot.store(slot.0, Ordering::Relaxed);
                    dcb.state
                        .store(DomainState::Faulted as u32, Ordering::Release);
                }

                DeactivateReason::Suspended => {
                    dcb.state
                        .store(DomainState::Suspended as u32, Ordering::Release);
                }
            }
        }
    }

    /// Thread-teardown-driven cancellation (the selected D7 cancellation
    /// trigger, 2026-09-16): cancel every pending-invocation record naming
    /// `thread` as its waiter and purge every queued wakeup for it.
    ///
    /// Call this while the Thread slot is still live — before the teardown
    /// path deallocates it — so the identity check below catches out-of-order
    /// teardown. The steps:
    ///
    /// 1. Every live synchronization object stops holding the Thread's
    ///    records (wait-queue removal; the FIFO order of other waiters is
    ///    preserved).
    /// 2. The pending-pool teardown sweep gives each of the Thread's records
    ///    its single terminal transition — `Cancelled` for a still-`Waiting`
    ///    record (teardown wins the terminal-transition rule) — and releases
    ///    it. A torn-down Thread never resumes, so nothing else would release
    ///    the bounded slots. Already-terminal records (a wakeup delivered
    ///    but not yet resumed) are released unchanged.
    /// 3. The runnable queue loses the Thread's index: a woken-but-not-yet
    ///    -resumed Thread must leave no wakeup behind for the next context
    ///    switch.
    ///
    /// No cancellation status crosses the wire: the torn-down waiter is
    /// gone, so its outcome is never delivered (the D9 cancellation
    /// encodings stay open for the timeout and object-teardown paths, whose
    /// waiters do resume). DCB release and thread-slot deallocation are the
    /// teardown path's separate steps (Phase 7 Thread control).
    pub fn cancel_thread_pending(&mut self, thread: ObjectId) -> Result<(), CapError> {
        // Teardown cancels pending records while the Thread slot is live; a
        // stale identity means the caller tore the Thread down out of order.
        self.pools.threads.validate(thread)?;

        // Every live synchronization object stops holding the Thread's
        // records. The scan is bounded by the pool slot count; `get_live_mut`
        // rejects slots beyond the carved capacity.
        for slot in 0..ObjectPool::<Notification>::MAX_SLOTS {
            if let Some(notification) = self.pools.notifications.get_live_mut(slot) {
                notification.remove_waiter(thread, &self.pending);
            }
        }
        for slot in 0..ObjectPool::<EventCount>::MAX_SLOTS {
            if let Some(event_count) = self.pools.event_counts.get_live_mut(slot) {
                event_count.remove_waiter(thread, &self.pending);
            }
        }

        // Every record naming the Thread reaches its terminal disposition
        // and is released; every queued wakeup for it is purged.
        self.pending.teardown_waiter(thread)?;
        self.scheduler.remove(thread.index);
        Ok(())
    }

    /// Update DCB when notification is signaled to a domain
    pub fn signal_notification(&mut self, id: DomainId, slot: KeySlot, bits: u64) {
        if let Some(dcb) = self.dcb_pages.get_mut(id) {
            // OR the notification bits
            dcb.pending_notifications
                .fetch_or(1 << slot.0, Ordering::Release);

            // If domain was blocked on notifications, make it runnable
            let state = dcb.state.load(Ordering::Acquire);
            let block_reason = dcb.block_reason.load(Ordering::Relaxed);

            if state == DomainState::Blocked as u32
                && block_reason == BlockReason::Notification as u32
            {
                dcb.state
                    .store(DomainState::Runnable as u32, Ordering::Release);
            }
        }
    }
}

pub enum DeactivateReason {
    TimeExhausted,
    Blocked {
        reason: BlockReason,
        slot: KeySlot,
    },
    Faulted {
        fault_type: FaultType,
        code: u32,
        addr: u64,
        slot: KeySlot,
    },
    Suspended,
    Yielded,
}

#[repr(u32)]
pub enum FaultType {
    None = 0,
    PageFault = 1,
    CapFault = 2,
    UnknownSyscall = 3,
    UserException = 4,
    VMFault = 5,
}

#[cfg(test)]
mod tests {
    use {
        super::{Nucleus, NucleusPools},
        crate::objects::{
            ArchObjectsImpl, EventCount, Notification, ObjectPool, PendingPool, Scheduler, Thread,
            access::{ObjectId, PoolTag},
            arch::{AArch64PageTable, ArchPools},
            completion::PendingState,
            event_count::{AdvanceOutcome, AwaitOutcome},
            notification::WaitOutcome,
            thread::ExecutionContext,
        },
        core::mem::MaybeUninit,
        libobject::syscall_status,
    };

    // Backing bytes for the fixture's pools: three Thread slots, two
    // Notification slots, one EventCount slot, and one page-table metadata
    // slot (structural only; these tests never touch the arch pools).
    static mut THREAD_POOL_MEM: [u64; 16] = [0; 16];
    static mut NOTIFICATION_POOL_MEM: [u64; 32] = [0; 32];
    static mut EVENT_COUNT_POOL_MEM: [u64; 32] = [0; 32];
    static mut ARCH_POOL_MEM: [u64; 16] = [0; 16];

    /// A minimal fixture nucleus. Tests run sequentially and the fixture
    /// re-initializes the same static storage before use.
    fn fixture_nucleus() -> &'static mut Nucleus<ArchObjectsImpl> {
        static mut NUCLEUS_MEM: MaybeUninit<Nucleus<ArchObjectsImpl>> = MaybeUninit::uninit();
        // SAFETY: the pool backings and NUCLEUS_MEM are exclusively owned by
        // the fixture; initialization happens before any use, and tests are
        // sequential. Raw pointers avoid mutable references to statics
        // (edition 2024).
        unsafe {
            let nucleus_ptr = (&raw mut NUCLEUS_MEM).cast::<Nucleus<ArchObjectsImpl>>();
            let thread_ptr = (&raw mut THREAD_POOL_MEM).cast::<u8>();
            let notification_ptr = (&raw mut NOTIFICATION_POOL_MEM).cast::<u8>();
            let event_count_ptr = (&raw mut EVENT_COUNT_POOL_MEM).cast::<u8>();
            let arch_ptr = (&raw mut ARCH_POOL_MEM).cast::<u8>();
            nucleus_ptr.write(Nucleus {
                current_thread: None,
                dcb_pages: crate::objects::domain::DcbPages::new(),
                pending: PendingPool::new(),
                scheduler: Scheduler::new(),
                pools: NucleusPools {
                    threads: ObjectPool::new(thread_ptr, core::mem::size_of::<Thread>() * 3),
                    notifications: ObjectPool::new(
                        notification_ptr,
                        core::mem::size_of::<Notification>() * 2,
                    ),
                    event_counts: ObjectPool::new(
                        event_count_ptr,
                        core::mem::size_of::<EventCount>(),
                    ),
                    // SAFETY: the arch backings are exclusively owned by the
                    // fixture; the pools are structural for these tests.
                    arch: ArchPools::new(
                        ObjectPool::new(arch_ptr, core::mem::size_of::<AArch64PageTable>()),
                        ObjectPool::new(arch_ptr, 0),
                        ObjectPool::new(arch_ptr, 0),
                    ),
                },
            });
            &mut *nucleus_ptr
        }
    }

    fn thread_fixture() -> Thread {
        Thread {
            keytable_addr: 0x1000,
            // Structural placeholder identity: these tests never resolve the
            // address space.
            address_space: ObjectId {
                pool: PoolTag::AddressSpace,
                index: 0,
                generation: 1,
            },
            context: ExecutionContext::Running,
        }
    }

    /// Block `waiter` on the notification at `notification`, returning its
    /// pending-record identity.
    fn block_on_notification(
        nucleus: &mut Nucleus<ArchObjectsImpl>,
        notification: ObjectId,
        waiter: ObjectId,
    ) -> ObjectId {
        let n = nucleus
            .pools
            .notifications
            .get_live_mut(usize::from(notification.index))
            .expect("notification is live");
        match n.wait(waiter, &mut nucleus.pending) {
            Ok(WaitOutcome::Blocked(record)) => record,
            _ => panic!("wait should block"),
        }
    }

    #[test_case]
    fn thread_teardown_cancels_parked_waits_and_queued_wakeups() {
        let nucleus = fixture_nucleus();

        // Three threads: A and C block on a notification, B awaits an event
        // count. C is then woken but not resumed: its record completes and its
        // index is enqueued runnable (what `wake_waiter` does).
        let (a, _) = nucleus.pools.threads.allocate(thread_fixture()).unwrap();
        let (b, _) = nucleus.pools.threads.allocate(thread_fixture()).unwrap();
        let (c, _) = nucleus.pools.threads.allocate(thread_fixture()).unwrap();
        let (n, _) = nucleus
            .pools
            .notifications
            .allocate(Notification::new())
            .unwrap();
        let (e, _) = nucleus
            .pools
            .event_counts
            .allocate(EventCount::new())
            .unwrap();

        // C queues first, then A: a signal delivers to C (one consumer).
        let record_c = block_on_notification(nucleus, n, c);
        let record_a = block_on_notification(nucleus, n, a);
        let record_b = {
            let ec = nucleus
                .pools
                .event_counts
                .get_live_mut(usize::from(e.index))
                .expect("event count is live");
            match ec.await_ge(10, b, &mut nucleus.pending) {
                Ok(AwaitOutcome::Blocked(record)) => record,
                Ok(AwaitOutcome::Ready(_)) => panic!("await should block"),
                Err(_) => panic!("await failed"),
            }
        };

        // Wake C without resuming it: its record is Completed and its index
        // is queued runnable.
        {
            let ec = nucleus
                .pools
                .notifications
                .get_live_mut(usize::from(n.index))
                .expect("notification is live");
            match ec.signal(0b1, &mut nucleus.pending) {
                Ok(Some(woken)) => assert_eq!(woken, record_c),
                _ => panic!("signal should wake C"),
            }
        }
        assert!(nucleus.scheduler.push(c.index));
        assert_eq!(nucleus.scheduler.len(), 1);

        // Teardown of A: its queued wait is cancelled and released; C's
        // completed record and queued wakeup, and B's await, are untouched.
        nucleus
            .cancel_thread_pending(a)
            .unwrap_or_else(|_| panic!("teardown of A failed"));
        assert!(nucleus.pending.state(record_a).is_err());
        assert_eq!(
            nucleus.pending.state(record_c).ok(),
            Some(PendingState::Completed {
                status: syscall_status::SUCCESS,
                result0: 0b1,
                result1: 0
            })
        );
        assert_eq!(
            nucleus.pending.state(record_b).ok(),
            Some(PendingState::Waiting)
        );
        assert_eq!(nucleus.scheduler.len(), 1);

        // The notification no longer holds A: the next signal delivers to no
        // one (the queue is empty), so the bits stay pending.
        {
            let ec = nucleus
                .pools
                .notifications
                .get_live_mut(usize::from(n.index))
                .expect("notification is live");
            assert!(matches!(ec.signal(0b10, &mut nucleus.pending), Ok(None)));
            assert_eq!(ec.pending_bits(), 0b10);
        }

        // Teardown of C: the completed-but-undelivered record is released and
        // the queued wakeup is purged.
        nucleus
            .cancel_thread_pending(c)
            .unwrap_or_else(|_| panic!("teardown of C failed"));
        assert!(nucleus.pending.state(record_c).is_err());
        assert!(nucleus.scheduler.is_empty());

        // Teardown of B: the queued await is cancelled and released.
        nucleus
            .cancel_thread_pending(b)
            .unwrap_or_else(|_| panic!("teardown of B failed"));
        assert!(nucleus.pending.state(record_b).is_err());
        assert!(nucleus.pending.is_empty());

        // The event count is fully usable: an advance wakes no one.
        {
            let ec = nucleus
                .pools
                .event_counts
                .get_live_mut(usize::from(e.index))
                .expect("event count is live");
            match ec.advance(5, &mut nucleus.pending) {
                Ok(AdvanceOutcome::Advanced { new_value, woken }) => {
                    assert_eq!(woken.iter().count(), 0);
                    assert_eq!(new_value, 5);
                }
                Ok(AdvanceOutcome::Overflow { .. }) => panic!("advance should not overflow"),
                Err(_) => panic!("advance after teardown failed"),
            }
        }
    }

    #[test_case]
    fn thread_teardown_with_nothing_pending_is_a_no_op() {
        let nucleus = fixture_nucleus();
        let (a, _) = nucleus.pools.threads.allocate(thread_fixture()).unwrap();

        // A live thread with nothing pending tears down cleanly.
        nucleus
            .cancel_thread_pending(a)
            .unwrap_or_else(|_| panic!("teardown with nothing pending failed"));
        assert!(nucleus.pending.is_empty());
        assert!(nucleus.scheduler.is_empty());
    }

    #[test_case]
    fn thread_teardown_rejects_a_stale_thread_identity() {
        let nucleus = fixture_nucleus();
        let (a, _) = nucleus.pools.threads.allocate(thread_fixture()).unwrap();

        // Teardown cancels pending records while the thread slot is live;
        // after deallocation the identity is stale and out-of-order teardown
        // is rejected.
        nucleus
            .cancel_thread_pending(a)
            .unwrap_or_else(|_| panic!("teardown before deallocation failed"));
        nucleus
            .pools
            .threads
            .deallocate(a)
            .unwrap_or_else(|_| panic!("thread deallocation failed"));
        assert!(nucleus.cancel_thread_pending(a).is_err());
    }
}
