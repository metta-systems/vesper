use {
    crate::objects::{
        ArchObjects, EventCount, KeyTable, Notification, PendingPool, RunQueue, Thread,
        arch::ArchPools,
        arch_objects::{AddressSpaceId, AddressSpaceObject, PageTableId},
        carved::Carved,
        domain::DcbPages,
        irq::IrqTable,
        key_table::KeyTableBinding,
        thread::{ExecutionContext, ThreadId},
    },
    core::sync::atomic::Ordering,
    libobject::{
        CapError, KeySlot,
        domain::{BlockReason, DomainControlBlock, DomainId, DomainState},
    },
};

// ┌─────────────────────────────────────────────────────────────────────┐
// │                    KERNEL TYPE STRUCTURE                            │
// ├─────────────────────────────────────────────────────────────────────┤
// │                                                                     │
// │  Kernel<A: ArchObjects>                                             │
//  │  │                                                                  │
//  │  ├── pools: KernelPools<A>                                          │
//  │  │   ├── untypeds: ObjectPool<Untyped>                              │
//  │  │   ├── threads: carved by Retype                                  │
//  │  │   ├── keytables: carved by Retype (no pool; see api::untyped)     │
//  │  │   ├── notifications, event_counts, times: carved by Retype     │
//  │  │   │                                                              │
//  │  │   └── arch: ArchPools<A>                                         │
//  │  │       ├── frames: inline regions (no pool)                       │
//  │  │       ├── page_tables, address_spaces: carved by Retype/boot     │
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
    // Threads, Notifications, EventCounts and Times are carved from Untyped
    // memory by Retype and named by their carved identity.

    // ─── Architecture-Specific Pools ───
    pub arch: ArchPools<A>,
}

/// Complete nucleus state (parameterized by architecture)
pub struct Nucleus<A: ArchObjects> {
    /// All object pools
    pub pools: NucleusPools<A>,
    /// Currently running thread, as a checked identity: every read validates
    /// its generation against the Thread's header, so a retired Thread is
    /// never mistaken for the current Thread.
    pub current_thread: Option<ThreadId>, // FIXME: not option, always something (Idle or other)
    /// DCB shared pages
    pub dcb_pages: DcbPages,
    /// Pending-invocation records for blocked callers (completion
    /// foundation, 2026-09-16): bounded kernel-private storage, the
    /// closed-wait identity for blocked invocations.
    pub pending: PendingPool,
    /// Runnable-domain queue: the minimal "run someone else" substrate
    /// (completion foundation, 2026-09-16). Kernel mechanism only;
    /// scheduling policy stays in userspace.
    pub run_queue: RunQueue,
    /// The idle Thread, if boot created one: selected when nothing else is
    /// runnable and never queued.
    pub idle_thread: Option<ThreadId>,
    /// The interrupt lines' records: live `IRQHandler`s and their bindings.
    pub irq_table: IrqTable,
    /// Preemption-timer ticks taken since boot.
    pub ticks: u64,
}

// ═══════════════════════════════════════════════════════════════════
// KERNEL INTEGRATION
// ═══════════════════════════════════════════════════════════════════

impl<A: ArchObjects> Nucleus<A> {
    #[expect(clippy::unused_self)]
    pub fn current_cpu(&self) -> usize {
        0
    }

    /// Monotonic time since the counter started, from the kernel-owned
    /// physical counter.
    #[expect(clippy::unused_self)]
    pub fn current_time_ns(&self) -> u64 {
        crate::timer::now_ns()
    }

    /// The current Thread's identity. `InvalidDomain` when no Thread is
    /// current or the recorded identity no longer names a live Thread.
    pub fn current_thread_id(&self) -> Result<ThreadId, CapError> {
        let id = self.current_thread.ok_or(CapError::InvalidDomain)?;
        self.thread(id).ok_or(CapError::InvalidDomain)?;
        Ok(id)
    }

    /// The current Thread, after identity validation; see
    /// [`Self::current_thread_id`].
    pub fn current_thread_ref(&self) -> Result<&Thread, CapError> {
        self.thread(self.current_thread_id()?)
            .ok_or(CapError::InvalidDomain)
    }

    /// Nucleus-private thread data, like keytables
    pub fn current_thread_mut(&mut self) -> Option<&mut Thread> {
        let id = self.current_thread_id().ok()?;
        self.thread_mut(id)
    }

    /// The live Thread behind `id`, or `None` once it is retired.
    #[expect(
        clippy::unused_self,
        reason = "the receiver ties the reference to the locked nucleus"
    )]
    pub fn thread(&self, id: ThreadId) -> Option<&Thread> {
        // SAFETY: Thread identities name carved records that are never freed;
        // the kernel lock serializes access, and the borrow of `self` bounds
        // the reference.
        unsafe { Carved::<Thread>::validate(id).ok().map(|thread| &*thread) }
    }

    /// Exclusive access to the live Thread behind `id`.
    #[expect(
        clippy::unused_self,
        reason = "the receiver ties the reference to the locked nucleus"
    )]
    pub fn thread_mut(&mut self, id: ThreadId) -> Option<&mut Thread> {
        // SAFETY: as in `thread`; `&mut self` makes the reference exclusive.
        unsafe {
            Carved::<Thread>::validate(id)
                .ok()
                .map(|thread| &mut *thread)
        }
    }

    /// Shared access to the current thread's capability table.
    pub fn current_thread_table(&self) -> Option<&KeyTable> {
        let addr = self.current_thread_table_addr()?;
        // SAFETY: validated AddressSpace identity supplies an issued binding;
        // its full kernel-private carve stays stable under binding's contract.
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
        Some(self.current_thread_table_binding()?.address())
    }

    fn current_thread_table_binding(&self) -> Option<KeyTableBinding> {
        let thread = self.current_thread_ref().ok()?;
        self.address_space(thread.address_space)?.keytable()
    }

    /// The live `AddressSpace` behind `id`, or `None` once it is retired.
    #[expect(
        clippy::unused_self,
        reason = "the receiver ties the reference to the locked nucleus"
    )]
    pub fn address_space(&self, id: AddressSpaceId) -> Option<&A::AddressSpace> {
        // SAFETY: AddressSpace identities name carved records that are never
        // freed; the kernel lock serializes access, and the borrow of `self`
        // bounds the reference.
        unsafe {
            Carved::<A::AddressSpace>::validate(id)
                .ok()
                .map(|address_space| &*address_space)
        }
    }

    /// Exclusive access to the live `AddressSpace` behind `id`.
    #[expect(
        clippy::unused_self,
        reason = "the receiver ties the reference to the locked nucleus"
    )]
    pub fn address_space_mut(&mut self, id: AddressSpaceId) -> Option<&mut A::AddressSpace> {
        // SAFETY: as in `address_space`; `&mut self` makes the reference
        // exclusive.
        unsafe {
            Carved::<A::AddressSpace>::validate(id)
                .ok()
                .map(|address_space| &mut *address_space)
        }
    }

    /// The live page-table metadata record behind `id`, or `None` once it is
    /// retired.
    #[expect(
        clippy::unused_self,
        reason = "the receiver ties the reference to the locked nucleus"
    )]
    pub fn page_table(&self, id: PageTableId) -> Option<&A::PageTable> {
        // SAFETY: as in `address_space`: page-table identities name carved
        // records that are never freed.
        unsafe {
            Carved::<A::PageTable>::validate(id)
                .ok()
                .map(|page_table| &*page_table)
        }
    }

    /// Unit-test fixture: carve a running Thread in `address_space` from a
    /// test arena standing in for Untyped memory.
    #[cfg(test)]
    pub fn create_thread(&mut self, address_space: AddressSpaceId) -> Option<ThreadId> {
        self.address_space(address_space)?;
        let id = test_arena::carve(Thread::new(address_space, ExecutionContext::Running))?;
        test_arena::record_thread(id);
        Some(id)
    }

    /// Unit-test fixture: the `n`-th Thread [`Self::create_thread`] carved
    /// since the last `test_arena::reset_threads`, while it is live.
    #[cfg(test)]
    pub fn fixture_thread(&self, n: usize) -> Option<ThreadId> {
        test_arena::thread(n).filter(|id| self.thread(*id).is_some())
    }

    /// Unit-test fixture: how many fixture Threads are live.
    #[cfg(test)]
    pub fn fixture_thread_count(&self) -> usize {
        (0..16)
            .filter(|n| self.fixture_thread(*n).is_some())
            .count()
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
    pub fn cancel_thread_pending(&mut self, thread: ThreadId) -> Result<(), CapError> {
        // Teardown cancels pending records while the Thread slot is live; a
        // stale identity means the caller tore the Thread down out of order.
        self.thread(thread).ok_or(CapError::InvalidOperation)?;

        // Every synchronization object holding one of the Thread's waiting
        // records stops holding it. Each record names its object, so no scan
        // over all objects is needed; the walk is bounded by the pending pool.
        let mut objects = [None; PendingPool::CAPACITY];
        for (slot, object) in objects.iter_mut().zip(self.pending.waited_objects(thread)) {
            *slot = Some(object);
        }
        for object in objects.into_iter().flatten() {
            // SAFETY: the identity comes from a pending record, which only a
            // wait on a resolved carved object creates; carved memory is never
            // freed. A retired object fails validation and holds no queue.
            unsafe {
                if let Ok(notification) = Carved::<Notification>::validate(object) {
                    (*notification).remove_waiter(thread, &self.pending);
                } else if let Ok(event_count) = Carved::<EventCount>::validate(object) {
                    (*event_count).remove_waiter(thread, &self.pending);
                }
            }
        }

        // Every record naming the Thread reaches its terminal disposition
        // and is released; every queued wakeup for it is purged.
        self.pending.teardown_waiter(thread)?;
        self.run_queue.remove(thread);
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
        super::{Nucleus, NucleusPools, test_arena},
        crate::objects::{
            ArchObjectsImpl, EventCount, Notification, ObjectPool, PendingPool, RunQueue, Thread,
            access::ObjectId,
            arch::{AArch64ASIDPool, ArchPools},
            carved::{self, Carved, CarvedId},
            completion::PendingState,
            event_count::{AdvanceOutcome, AwaitOutcome},
            notification::WaitOutcome,
            thread::{ExecutionContext, ThreadId},
        },
        core::mem::MaybeUninit,
        libobject::syscall_status,
    };

    // Backing bytes for the fixture's one ASID-pool slot
    // (structural only; these tests never touch the arch pools). u128
    // elements give the 16-byte minimum carve alignment; the fixture
    // asserts the carve fits.
    static mut ARCH_POOL_MEM: [u128; 8] = [0; 8];

    /// Backing for the carved Notification and `EventCount`, standing in for
    /// Untyped memory.
    #[repr(C, align(64))]
    struct CarveBacking([u8; 1024]);
    static mut CARVE_MEM: CarveBacking = CarveBacking([0; 1024]);

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
            let arch_ptr = (&raw mut ARCH_POOL_MEM).cast::<u8>();
            assert!(
                ObjectPool::<AArch64ASIDPool>::carve_size(1) <= core::mem::size_of::<[u128; 8]>()
            );
            nucleus_ptr.write(Nucleus {
                current_thread: None,
                dcb_pages: crate::objects::domain::DcbPages::new(),
                pending: PendingPool::new(),
                run_queue: RunQueue::new(),
                idle_thread: None,
                irq_table: crate::objects::irq::IrqTable::empty(),
                ticks: 0,
                pools: NucleusPools {
                    // SAFETY: the arch backings are exclusively owned by the
                    // fixture; the pools are structural for these tests.
                    arch: ArchPools::new(ObjectPool::initialize(arch_ptr, 1)),
                },
            });
            &mut *nucleus_ptr
        }
    }

    /// Carve a fresh Notification and `EventCount` into the fixture backing.
    fn carve_objects() -> (CarvedId, CarvedId) {
        // SAFETY: the backing is exclusively owned by the sequential tests and
        // large and aligned enough for both records.
        unsafe {
            let base = (&raw mut CARVE_MEM).cast::<u8>();
            let notification = Carved::<Notification>::initialize(base, Notification::new());
            let event_count = Carved::<EventCount>::initialize(base.add(512), EventCount::new());
            (notification, event_count)
        }
    }

    /// The live object behind a carved identity from [`carve_objects`].
    fn object<'a, T: crate::objects::carved::CarvedObject>(id: CarvedId) -> &'a mut T {
        // SAFETY: the identity names a record carved into the fixture
        // backing, and the tests hold one reference at a time.
        unsafe {
            &mut *Carved::<T>::validate(id)
                .ok()
                .expect("carved object is live")
        }
    }

    /// Carve a running Thread in a structural `AddressSpace` (these tests
    /// never resolve it) from the test arena.
    fn carve_thread() -> ThreadId {
        test_arena::carve(Thread::new(
            carved::test_id(libobject::ObjectType::ADDRESS_SPACE, 0x1000),
            ExecutionContext::Running,
        ))
        .expect("test arena exhausted")
    }

    /// Block `waiter` on the notification `notification`, returning its
    /// pending-record identity.
    fn block_on_notification(
        nucleus: &mut Nucleus<ArchObjectsImpl>,
        notification: CarvedId,
        waiter: ThreadId,
    ) -> ObjectId {
        match object::<Notification>(notification).wait(notification, waiter, &mut nucleus.pending)
        {
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
        let a = carve_thread();
        let b = carve_thread();
        let c = carve_thread();
        let (n, e) = carve_objects();

        // C queues first, then A: a signal delivers to C (one consumer).
        let record_c = block_on_notification(nucleus, n, c);
        let record_a = block_on_notification(nucleus, n, a);
        let record_b = match object::<EventCount>(e).await_ge(e, 10, b, &mut nucleus.pending) {
            Ok(AwaitOutcome::Blocked(record)) => record,
            Ok(AwaitOutcome::Ready(_)) => panic!("await should block"),
            Err(_) => panic!("await failed"),
        };

        // Wake C without resuming it: its record is Completed and its index
        // is queued runnable.
        match object::<Notification>(n).signal(0b1, &mut nucleus.pending) {
            Ok(Some(woken)) => assert_eq!(woken, record_c),
            _ => panic!("signal should wake C"),
        }
        assert!(nucleus.run_queue.push(c));
        assert_eq!(nucleus.run_queue.len(), 1);

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
        assert_eq!(nucleus.run_queue.len(), 1);

        // The notification no longer holds A: the next signal delivers to no
        // one (the queue is empty), so the bits stay pending.
        let notification = object::<Notification>(n);
        assert!(matches!(
            notification.signal(0b10, &mut nucleus.pending),
            Ok(None)
        ));
        assert_eq!(notification.pending_bits(), 0b10);

        // Teardown of C: the completed-but-undelivered record is released and
        // the queued wakeup is purged.
        nucleus
            .cancel_thread_pending(c)
            .unwrap_or_else(|_| panic!("teardown of C failed"));
        assert!(nucleus.pending.state(record_c).is_err());
        assert!(nucleus.run_queue.is_empty());

        // Teardown of B: the queued await is cancelled and released.
        nucleus
            .cancel_thread_pending(b)
            .unwrap_or_else(|_| panic!("teardown of B failed"));
        assert!(nucleus.pending.state(record_b).is_err());
        assert!(nucleus.pending.is_empty());

        // The event count is fully usable: an advance wakes no one.
        match object::<EventCount>(e).advance(5, &mut nucleus.pending) {
            Ok(AdvanceOutcome::Advanced { new_value, woken }) => {
                assert_eq!(woken.iter().count(), 0);
                assert_eq!(new_value, 5);
            }
            Ok(AdvanceOutcome::Overflow { .. }) => panic!("advance should not overflow"),
            Err(_) => panic!("advance after teardown failed"),
        }
    }

    #[test_case]
    fn thread_teardown_with_nothing_pending_is_a_no_op() {
        let nucleus = fixture_nucleus();
        let a = carve_thread();

        // A live thread with nothing pending tears down cleanly.
        nucleus
            .cancel_thread_pending(a)
            .unwrap_or_else(|_| panic!("teardown with nothing pending failed"));
        assert!(nucleus.pending.is_empty());
        assert!(nucleus.run_queue.is_empty());
    }

    #[test_case]
    fn thread_teardown_rejects_a_stale_thread_identity() {
        let nucleus = fixture_nucleus();
        let a = carve_thread();

        // Teardown cancels pending records while the thread slot is live;
        // after retirement the identity is stale and out-of-order teardown
        // is rejected.
        nucleus
            .cancel_thread_pending(a)
            .unwrap_or_else(|_| panic!("teardown before retirement failed"));
        // SAFETY: the identity names a test-arena record; nothing holds it.
        unsafe { Carved::<Thread>::retire(a) }
            .unwrap_or_else(|_| panic!("thread retirement failed"));
        assert!(nucleus.cancel_thread_pending(a).is_err());
    }
}

/// Unit-test arena standing in for Untyped memory: fixtures carve Threads
/// and other objects here. It only grows; each carve is a fresh record.
#[cfg(test)]
pub(crate) mod test_arena {
    use {
        crate::objects::carved::{Carved, CarvedId, CarvedObject},
        core::sync::atomic::{AtomicUsize, Ordering},
    };

    const BYTES: usize = 1 << 20;

    #[repr(C, align(64))]
    struct Arena([u8; BYTES]);

    static mut ARENA: Arena = Arena([0; BYTES]);
    static USED: AtomicUsize = AtomicUsize::new(0);

    /// Fixture Threads carved since the last [`reset_threads`], in creation
    /// order.
    static mut THREADS: [Option<CarvedId>; 16] = [None; 16];

    /// Forget the recorded fixture Threads (the arena itself never reuses
    /// memory).
    pub(crate) fn reset_threads() {
        // SAFETY: tests run sequentially; nothing borrows the list across this.
        unsafe { *(&raw mut THREADS) = [None; 16] };
    }

    /// Record a carved fixture Thread.
    pub(crate) fn record_thread(id: CarvedId) {
        // SAFETY: as in `reset_threads`.
        let threads = unsafe { &mut *(&raw mut THREADS) };
        let slot = threads
            .iter_mut()
            .find(|slot| slot.is_none())
            .expect("too many fixture Threads");
        *slot = Some(id);
    }

    /// The `n`-th fixture Thread carved since the last reset.
    pub(crate) fn thread(n: usize) -> Option<CarvedId> {
        // SAFETY: as in `reset_threads`.
        unsafe { (*(&raw const THREADS)).get(n).copied().flatten() }
    }

    /// Carve `object` into the arena; `None` once the arena is full.
    pub(crate) fn carve<T: CarvedObject>(object: T) -> Option<CarvedId> {
        let size = Carved::<T>::SIZE.next_multiple_of(64);
        let offset = USED.fetch_add(size, Ordering::Relaxed);
        if offset + size > BYTES {
            return None;
        }
        // SAFETY: each carve takes a fresh, 64-byte aligned range of the
        // arena that nothing else names; tests run sequentially.
        Some(unsafe { Carved::<T>::initialize((&raw mut ARENA).cast::<u8>().add(offset), object) })
    }
}
