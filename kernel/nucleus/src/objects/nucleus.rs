use {
    crate::{
        api::wake_waiter,
        objects::{
            ArchObjects, EventCount, KeyTable, Notification, PendingPool, RunQueue, Scheduler,
            Thread, Time,
            access::ObjectId,
            arch::ArchPools,
            arch_objects::{AddressSpaceId, AddressSpaceObject, PageTableId},
            carved::{Carved, CarvedId},
            completion::{PendingKind, PendingState},
            event_count::AdvanceOutcome,
            irq::IrqTable,
            key_table::KeyTableBinding,
            scheduler::Region,
            thread::{ExecutionContext, RecordSlot, ThreadId},
            time::{ns_to_ticks_ceil, ticks_to_ns_ceil},
            timers::SchedulerTimers,
        },
    },
    libaddress::PhysAddr,
    libobject::{
        CapError,
        scheduler::{activation_mask, activation_reason},
        scheduler_record::{
            BlockReason, FLAG_AWAITING_BUDGET, FLAG_HAS_TIME, NO_CPU, SCHEDULER_SLOT,
            SchedulerRecord, SchedulerSnapshot, StopReason, ThreadRecord, ThreadSnapshot,
        },
        syscall_status,
        thread::ThreadState,
    },
};

/// The kernel's pointer to the physical memory at `paddr`, through the
/// direct map.
#[cfg(not(test))]
fn kernel_window(paddr: u64) -> *mut u8 {
    PhysAddr::new(paddr).user_to_kernel().as_mut_ptr::<u8>()
}

/// The unit-test binaries run without translation, where physical and kernel
/// addresses coincide.
#[cfg(test)]
fn kernel_window(paddr: u64) -> *mut u8 {
    paddr as *mut u8
}

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
//  │  └── pending: PendingPool  │
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
    /// Pending-invocation records for blocked callers.
    /// Bounded kernel-private storage: the closed-wait identity for blocked invocations.
    pub pending: PendingPool,
    /// Runnable-Thread queue: the minimal "run someone else" substrate
    /// Kernel mechanism only:
    /// scheduling policy stays in userspace.
    pub run_queue: RunQueue,
    /// The idle Thread, if boot created one: selected when nothing else is
    /// runnable and never queued.
    pub idle_thread: Option<ThreadId>,
    /// The interrupt lines' records: live `IRQHandler`s and their bindings.
    pub irq_table: IrqTable,
    /// Preemption-timer ticks taken since boot.
    pub ticks: u64,
    /// When the current Thread was last made current, in counter ticks on the
    /// [`Nucleus::counter_ticks`] timeline. The next switch away or tick charges
    /// the elapsed ticks to that Thread and its bound Time. Starts at zero, so
    /// the boot Thread is charged from counter start.
    pub current_since_ticks: u64,

    /// The Thread running under a dispatcher's `Dispatch`, and the
    /// dispatcher's limit for it.
    pub dispatched: Option<Dispatched>,
    /// The dispatch the dispatcher just asked for, taken when it parks.
    pub dispatch_next: Option<Dispatched>,
    /// An activation made the dispatcher runnable ahead of the Thread that
    /// is running: the timer fires at once so the nucleus switches.
    pub preempt_requested: bool,
    /// The dispatchers' armed timers (`Scheduler.ArmTimer`).
    pub timers: SchedulerTimers,
}

/// A Thread that a dispatcher's `Dispatch` runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dispatched {
    /// The dispatched Thread.
    pub thread: ThreadId,
    /// The Scheduler whose dispatcher dispatched it.
    pub scheduler: CarvedId,
    /// The counter value at which its limit is reached, if it has one.
    pub deadline_ticks: Option<u64>,
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

    /// The kernel-owned physical counter, in ticks.
    #[expect(clippy::unused_self)]
    pub fn counter_ticks(&self) -> u64 {
        crate::timer::counter()
    }

    /// The counter frequency, in Hz.
    #[expect(clippy::unused_self)]
    pub fn counter_frequency(&self) -> u64 {
        crate::timer::frequency()
    }

    /// Counter ticks `id` has executed as of its last charge; `None` when
    /// the identity no longer names a live Thread. The interval of a Thread
    /// that is running now is charged when it is next switched away from or
    /// ticked.
    pub fn run_time_ticks(&self, id: ThreadId) -> Option<u64> {
        self.thread(id).map(|thread| thread.run_time_ticks)
    }

    /// Nanoseconds `id` has executed as of its last charge, rounded up.
    pub fn run_time_ns(&self, id: ThreadId) -> Option<u64> {
        let frequency = self.counter_frequency();
        self.run_time_ticks(id)
            .map(|ticks| ticks_to_ns_ceil(ticks, frequency))
    }

    /// The bound Time of the Thread `id`, if it has one that still
    /// validates.
    fn thread_time(&mut self, id: ThreadId) -> Option<&mut Time> {
        let time = self.thread(id)?.time?;
        // SAFETY: the recorded identity was a live carved Time when
        // `Scheduler.ConfigureThread` bound it; validation checks its header
        // first, and the kernel lock serializes every access, so this is the
        // only reference to the Time while `self` is borrowed.
        unsafe { Carved::<Time>::validate(time).ok().map(|time| &mut *time) }
    }

    /// Whether the Thread `id` has a bound Time that is exhausted. A Thread
    /// with no bound Time is never exhausted.
    pub fn thread_exhausted(&mut self, id: ThreadId) -> bool {
        let frequency = self.counter_frequency();
        self.thread_time(id)
            .is_some_and(|time| time.is_exhausted(frequency))
    }

    /// Charge the interval since `current_since_ticks` to the Thread
    /// `current`: its kernel-private run time and, if it has one, its bound
    /// Time. Restarts the interval at now and returns the ticks charged.
    pub fn charge_current(&mut self, current: ThreadId) -> u64 {
        let now = self.counter_ticks();
        let elapsed = now.saturating_sub(self.current_since_ticks);
        self.current_since_ticks = now;
        if let Some(thread) = self.thread_mut(current) {
            thread.run_time_ticks = thread.run_time_ticks.saturating_add(elapsed);
        }
        if let Some(time) = self.thread_time(current) {
            time.charge(elapsed);
        }
        elapsed
    }

    /// The `Scheduler` behind `id`, if it still validates.
    #[allow(
        clippy::unused_self,
        reason = "ties the Scheduler borrow to the Nucleus"
    )]
    fn scheduler_mut(&mut self, id: CarvedId) -> Option<&mut Scheduler> {
        // SAFETY: Scheduler identities name carved records that are never
        // freed; validation checks the header, and the kernel lock
        // serializes every access, so this is the only reference to the
        // Scheduler while `self` is borrowed.
        unsafe { Carved::<Scheduler>::validate(id).ok().map(|s| &mut *s) }
    }

    /// The Scheduler whose bound dispatcher runs the Thread `id`, if there
    /// is one and `id` is not the dispatcher itself. Such a Thread never
    /// enters the first-in, first-out queue: it waits to be dispatched.
    pub fn dispatching_scheduler(&mut self, id: ThreadId) -> Option<CarvedId> {
        let scheduler = self.thread(id)?.scheduler?;
        let dispatcher = self.scheduler_mut(scheduler)?.dispatcher()?;
        (dispatcher != id).then_some(scheduler)
    }

    /// Raise activation `reasons` for the dispatcher of `scheduler`.
    /// `mask_bit` is the activation-mask bit that selects the event, or 0
    /// for an event that always activates. A dispatcher that dispatched
    /// nothing is activated by any event, whatever the mask. A waiting
    /// dispatcher's `Dispatch` call completes and the dispatcher is queued to
    /// run next; otherwise the reasons stay pending for its next `Dispatch`.
    pub fn request_activation(&mut self, scheduler: CarvedId, reasons: u64, mask_bit: u64) {
        let Some(object) = self.scheduler_mut(scheduler) else {
            return;
        };
        let waiting_idle = object.parked().is_some_and(|(_record, idle)| idle);
        if mask_bit != 0 && object.activation_mask() & mask_bit == 0 && !waiting_idle {
            return;
        }
        object.add_pending(reasons);
        let Some(record) = object.take_parked() else {
            return;
        };
        let bits = object.take_pending();
        let (dispatcher, events) = (object.dispatcher(), object.events());
        if self
            .pending
            .complete_with_status(record, syscall_status::SUCCESS, bits, events)
            .is_err()
        {
            return;
        }
        if self
            .dispatched
            .is_some_and(|dispatched| dispatched.scheduler == scheduler)
        {
            self.dispatched = None;
        }
        if let Some(dispatcher) = dispatcher {
            self.run_queue.push_front(dispatcher);
            self.preempt_requested = true;
        }
    }

    /// Queue the runnable Thread `id` at the back, unless its bound Time is
    /// exhausted: then it waits off the queue until budget arrives. A Thread
    /// a dispatcher runs is not queued either: it is published as runnable and
    /// the dispatcher is activated if its mask asks for it. Returns whether
    /// it was queued.
    pub fn make_runnable(&mut self, id: ThreadId) -> bool {
        let managed = self.dispatching_scheduler(id);
        if self.thread_exhausted(id) {
            if let Some(thread) = self.thread_mut(id) {
                thread.awaiting_budget = true;
            }
            self.publish_thread(id, false);
            return false;
        }
        if let Some(scheduler) = managed {
            self.publish_thread(id, false);
            self.request_activation(
                scheduler,
                activation_reason::EVENT,
                activation_mask::THREAD_RUNNABLE,
            );
            return false;
        }
        assert!(self.run_queue.push(id), "runnable Thread did not queue");
        self.publish_thread(id, false);
        true
    }

    /// Make the Thread `id`, which a tick just stopped, runnable again.
    /// A Thread a dispatcher runs waits for its next dispatch; the
    /// dispatcher has already been activated.
    pub fn requeue_preempted(&mut self, id: ThreadId) {
        if self.dispatching_scheduler(id).is_none() {
            self.make_runnable(id);
            return;
        }
        if self.thread_exhausted(id)
            && let Some(thread) = self.thread_mut(id)
        {
            thread.awaiting_budget = true;
        }
        self.publish_thread(id, false);
    }

    /// Budget arrived in the Time bound to the Thread `id`: if it was
    /// waiting for budget and the Time is no longer exhausted, queue it at
    /// the back, or, if a dispatcher runs it, activate the dispatcher when
    /// the mask asks for time availability. Returns whether it was queued.
    pub fn budget_arrived(&mut self, id: ThreadId) -> bool {
        let waiting = self.thread(id).is_some_and(|thread| thread.awaiting_budget);
        if !waiting || self.thread_exhausted(id) {
            return false;
        }
        if let Some(thread) = self.thread_mut(id) {
            thread.awaiting_budget = false;
        }
        if let Some(scheduler) = self.dispatching_scheduler(id) {
            self.publish_thread(id, false);
            self.request_activation(
                scheduler,
                activation_reason::EVENT,
                activation_mask::TIME_AVAILABLE,
            );
            return false;
        }
        let queued = self.run_queue.push(id);
        self.publish_thread(id, false);
        queued
    }

    /// Bind `thread` as the dispatcher of `scheduler` and start it. The
    /// Scheduler's Threads the first-in, first-out queue already holds leave
    /// it: they run only when dispatched from now on. The dispatcher runs next.
    pub fn bind_dispatcher(
        &mut self,
        scheduler: CarvedId,
        thread: ThreadId,
    ) -> Result<(), CapError> {
        let ExecutionContext::Configured { saved } = self
            .thread(thread)
            .ok_or(CapError::InvalidOperation)?
            .context
        else {
            return Err(CapError::InvalidOperation);
        };
        self.scheduler_mut(scheduler)
            .ok_or(CapError::InvalidOperation)?
            .bind_dispatcher(thread)?;
        let mut cursor = self.run_queue.peek();
        while let Some(id) = cursor {
            let queued = self.thread(id);
            let next = queued.and_then(|queued| queued.run_next);
            if queued.is_some_and(|queued| queued.scheduler == Some(scheduler)) {
                self.run_queue.remove(id);
            }
            cursor = next;
        }
        self.thread_mut(thread)
            .ok_or(CapError::InvalidOperation)?
            .context = ExecutionContext::NotStarted { saved };
        self.run_queue.push_front(thread);
        self.publish_thread(thread, false);
        Ok(())
    }

    /// The counter value at which the nucleus next needs the timer, if it
    /// does: the end of the current slice when another Thread is runnable,
    /// or the instant the current Thread's budget runs out. `slice_ticks` is
    /// the slice length. The idle Thread, waiting for interrupts, and a
    /// Thread running alone on an unlimited budget need no timer.
    pub fn timer_deadline_ticks(&mut self, slice_ticks: u64) -> Option<u64> {
        // A wait timeout or a dispatcher's timer needs the timer even when
        // nothing else does, the idle Thread running included.
        let wakeup = self.next_wakeup_ticks();
        let own = self.running_deadline_ticks(slice_ticks);
        match (own, wakeup) {
            (Some(own), Some(wakeup)) => Some(own.min(wakeup)),
            (own, wakeup) => own.or(wakeup),
        }
    }

    /// The earliest instant a wait times out or a dispatcher's timer is due.
    pub fn next_wakeup_ticks(&self) -> Option<u64> {
        match (self.pending.next_deadline(), self.timers.next_deadline()) {
            (Some(wait), Some(timer)) => Some(wait.min(timer)),
            (wait, timer) => wait.or(timer),
        }
    }

    /// Give the waiting record `record` a timeout of `timeout_ns` from now.
    pub fn set_wait_timeout(&mut self, record: ObjectId, timeout_ns: u64) -> Result<(), CapError> {
        let deadline = self
            .counter_ticks()
            .saturating_add(ns_to_ticks_ceil(timeout_ns, self.counter_frequency()));
        self.pending.set_deadline(record, deadline)
    }

    /// Arm (or, for `deadline_ns` 0, disarm) `scheduler`'s dispatcher timer
    /// for the absolute `deadline_ns` on the monotonic clock. Returns the time
    /// now and the previous deadline, both in nanoseconds, the latter 0 if
    /// the timer was not armed.
    pub fn arm_dispatcher_timer(
        &mut self,
        scheduler: CarvedId,
        deadline_ns: u64,
    ) -> Result<(u64, u64), CapError> {
        let frequency = self.counter_frequency();
        let previous = if deadline_ns == 0 {
            self.timers.disarm(scheduler)
        } else {
            self.timers
                .arm(scheduler, ns_to_ticks_ceil(deadline_ns, frequency))?
        };
        Ok((
            ticks_to_ns_ceil(self.counter_ticks(), frequency),
            previous.map_or(0, |ticks| ticks_to_ns_ceil(ticks, frequency)),
        ))
    }

    /// End a dispatcher's timer with its dispatcher.
    pub fn disarm_dispatcher_timer(&mut self, scheduler: CarvedId) {
        self.timers.disarm(scheduler);
    }

    /// Process everything that is due: waits whose timeout has passed
    /// complete with `TIMED_OUT`, and dispatcher timers activate their
    /// dispatchers. The timer interrupt calls this.
    pub fn expire_deadlines(&mut self) {
        let now = self.counter_ticks();
        while let Some(record) = self.pending.earliest_expired(now) {
            self.time_out_wait(record);
        }
        while let Some(scheduler) = self.timers.take_expired(now) {
            self.request_activation(scheduler, activation_reason::TIMER, 0);
        }
    }

    /// Give the waiting record `record` its terminal transition by timeout:
    /// the waiter leaves the object's wait queue, completes with
    /// `TIMED_OUT` and the object's current value, and becomes runnable.
    fn time_out_wait(&mut self, record: ObjectId) {
        let Ok(waiter) = self.pending.waiter(record) else {
            return;
        };
        let object = self.pending.object(record);
        let mut value = 0;
        if let Some(object) = object {
            // SAFETY: the identity comes from a pending record, which only a
            // wait on a resolved carved object creates; carved memory is never
            // freed. A retired object fails validation and holds no queue.
            unsafe {
                if let Ok(notification) = Carved::<Notification>::validate(object) {
                    (*notification).remove_waiter(waiter, &self.pending);
                    value = (*notification).pending_bits();
                } else if let Ok(event_count) = Carved::<EventCount>::validate(object) {
                    (*event_count).remove_waiter(waiter, &self.pending);
                    value = (*event_count).read();
                }
            }
        }
        // Only a `Waiting` record has a deadline, so this transition wins.
        if self
            .pending
            .complete_with_status(record, syscall_status::TIMED_OUT, value, 0)
            .is_err()
        {
            return;
        }
        // A completed record's waiter is parked, so it is not already queued.
        // A waiter that was torn down will never resume: release the record.
        if self.thread(waiter).is_some() {
            self.make_runnable(waiter);
        } else {
            self.pending.release(record).ok();
        }
    }

    /// The earliest counter value at which the running Thread needs the
    /// timer, apart from timeouts and dispatcher timers.
    fn running_deadline_ticks(&mut self, slice_ticks: u64) -> Option<u64> {
        let current = self.current_thread?;
        // An activation made the dispatcher runnable ahead of the running
        // Thread: fire at once so the nucleus switches.
        if self.preempt_requested {
            return Some(self.current_since_ticks);
        }
        if self.idle_thread == Some(current) {
            return None;
        }
        let since = self.current_since_ticks;
        let frequency = self.counter_frequency();
        let exhaustion = |nucleus: &mut Self| {
            nucleus
                .thread_time(current)
                .and_then(|time| time.ticks_until_exhausted(frequency))
                .map(|left| since.saturating_add(left))
        };
        // A dispatched Thread runs until its limit or its budget runs out:
        // the dispatcher, not the queue's slice, bounds it.
        if let Some(dispatched) = self.dispatched.filter(|d| d.thread == current) {
            return match (dispatched.deadline_ticks, exhaustion(self)) {
                (Some(limit), Some(exhaustion)) => Some(limit.min(exhaustion)),
                (limit, exhaustion) => limit.or(exhaustion),
            };
        }
        let contended = !self.run_queue.is_empty();
        let mut deadline = contended.then(|| since.saturating_add(slice_ticks));
        // An exhausted Thread can only stop if something can take over.
        let can_stop = contended || self.idle_thread.is_some();
        if can_stop {
            deadline = match (deadline, exhaustion(self)) {
                (Some(slice), Some(exhaustion)) => Some(slice.min(exhaustion)),
                (slice, exhaustion) => slice.or(exhaustion),
            };
        }
        deadline
    }

    /// Initialize the records a newly shared `region` holds: the scheduler
    /// record, if the region covers slot 0. Thread slots are initialized
    /// when a Thread is assigned to them.
    pub fn initialize_region(&mut self, region: &Region) {
        if region.first_slot != SCHEDULER_SLOT {
            return;
        }
        // SAFETY: the region is a 64-byte-aligned RAM Frame the scheduler
        // shared; records are accessed only through atomics.
        unsafe { SchedulerRecord::at(kernel_window(region.frame_paddr)) }.initialize();
    }

    /// Link the Thread `thread` to `slot` of `scheduler`'s table: zero the
    /// slot's record, remember the link and publish the Thread's state. The
    /// caller checked that the Scheduler runs the Thread and covers the slot.
    /// The nucleus does not check that no other Thread uses the slot: the
    /// table is the scheduler's own memory.
    pub fn assign_record(
        &mut self,
        thread: ThreadId,
        scheduler: CarvedId,
        slot: u32,
    ) -> Result<(), CapError> {
        let paddr = Self::record_paddr(scheduler, slot).ok_or(CapError::InvalidOperation)?;
        let linked = self.thread_mut(thread).ok_or(CapError::InvalidOperation)?;
        linked.record = Some(RecordSlot { scheduler, slot });
        // SAFETY: a Thread slot of a shared region, 64-byte aligned; any
        // earlier occupant's record is the scheduler's to discard.
        unsafe { ThreadRecord::at(kernel_window(paddr)) }.clear();
        self.publish_thread(thread, false);
        Ok(())
    }

    /// The physical address of the record in `slot` of `scheduler`'s table,
    /// if a shared region covers it.
    fn record_paddr(scheduler: CarvedId, slot: u32) -> Option<u64> {
        // SAFETY: Scheduler identities name carved records that are never
        // freed; validation checks the header, and the kernel lock
        // serializes every access.
        let scheduler = unsafe { Carved::<Scheduler>::validate(scheduler) }.ok()?;
        // SAFETY: as above; no other reference to the Scheduler is live.
        unsafe { (*scheduler).record_paddr(slot) }
    }

    /// Publish the state of the Thread `id` to its record, if the scheduler
    /// assigned it one, then count a scheduling event. `dying` marks a Thread
    /// being retired: its final record.
    ///
    /// The nucleus is the only writer of a record and never reads one back.
    /// A region's Frame stays valid because the nucleus does not reclaim
    /// Frame memory; once Frames carry a retirement identity, it is checked
    /// here before the write.
    pub fn publish_thread(&mut self, id: ThreadId, dying: bool) {
        let Some(thread) = self.thread(id) else {
            return;
        };
        let Some(record) = thread.record else {
            return;
        };
        let state = if dying {
            ThreadState::Dying
        } else {
            match thread.context {
                ExecutionContext::Inactive | ExecutionContext::Configured { .. } => {
                    ThreadState::Inactive
                }
                ExecutionContext::NotStarted { .. } | ExecutionContext::Preempted { .. } => {
                    ThreadState::Runnable
                }
                ExecutionContext::Running => ThreadState::Running,
                // A woken waiter is parked until it resumes, but its call has
                // completed: it can run, so it is not blocked any more.
                ExecutionContext::Parked { record, .. }
                    if matches!(
                        self.pending.state(record),
                        Ok(PendingState::Completed { .. })
                    ) =>
                {
                    ThreadState::Runnable
                }
                ExecutionContext::Parked { .. } => ThreadState::Blocked,
                ExecutionContext::Faulted { .. } => ThreadState::Faulted,
            }
        };
        let block_reason = match thread.context {
            ExecutionContext::Parked { record, .. }
                if !dying && !matches!(state, ThreadState::Runnable) =>
            {
                match self.pending.kind(record) {
                    Ok(PendingKind::NotificationWait) => BlockReason::NotificationWait,
                    Ok(PendingKind::EventCountAwait) => BlockReason::EventCountAwait,
                    Ok(PendingKind::SchedulerDispatch) | Err(_) => BlockReason::None,
                }
            }
            _ => BlockReason::None,
        };
        let frequency = self.counter_frequency();
        let (awaiting_budget, last_stop, run_ticks, last_run_ticks) = (
            thread.awaiting_budget,
            thread.last_stop,
            thread.run_time_ticks,
            thread.last_run_ticks,
        );
        let running = state == ThreadState::Running;
        let (budget_remaining_ns, has_time) = match self.thread_time(id) {
            Some(time) => (time.remaining_ns(frequency), true),
            None => (u64::MAX, false),
        };
        let Some(thread) = self.thread_mut(id) else {
            return;
        };
        thread.generation = thread.generation.wrapping_add(1);
        let snapshot = ThreadSnapshot {
            generation: thread.generation,
            incarnation: id.generation,
            state: state as u32,
            flags: if awaiting_budget && !dying {
                FLAG_AWAITING_BUDGET
            } else {
                0
            } | if has_time { FLAG_HAS_TIME } else { 0 },
            block_reason: block_reason as u32,
            last_stop,
            cpu: if running { 0 } else { NO_CPU },
            run_time_ns: ticks_to_ns_ceil(run_ticks, frequency),
            last_run_ns: ticks_to_ns_ceil(last_run_ticks, frequency),
            budget_remaining_ns,
        };
        let Some(paddr) = Self::record_paddr(record.scheduler, record.slot) else {
            return;
        };
        // SAFETY: the address is a Thread slot of a region the scheduler
        // shared as a mapped RAM Frame; it is 64-byte aligned by
        // construction, and records are accessed only through atomics.
        unsafe { ThreadRecord::at(kernel_window(paddr)) }.publish(&snapshot);
        self.count_scheduler_event(record.scheduler);
    }

    /// Count a published scheduling event of the Scheduler `scheduler`:
    /// update its record and advance its bound `EventCount`, waking the
    /// dispatcher waiting on it.
    fn count_scheduler_event(&mut self, scheduler: CarvedId) {
        // SAFETY: as in `record_paddr`.
        let Ok(object) = (unsafe { Carved::<Scheduler>::validate(scheduler) }) else {
            return;
        };
        // SAFETY: as in `record_paddr`.
        let (events, timer_expirations, event_count, record) = unsafe {
            let object = &mut *object;
            let events = object.count_event();
            (
                events,
                object.timer_expirations(),
                object.event_count(),
                object.record_paddr(SCHEDULER_SLOT),
            )
        };
        if let Some(paddr) = record {
            // SAFETY: slot 0 of a shared region; see `publish_thread`.
            unsafe { SchedulerRecord::at(kernel_window(paddr)) }.publish(&SchedulerSnapshot {
                events,
                timer_expirations,
                dispatcher_state: 0,
                flags: 0,
            });
        }
        let Some(event_count) = event_count else {
            return;
        };
        // SAFETY: EventCount identities name carved records that are never
        // freed; no API guard on it is live in the callers of this method.
        let Ok(event_count_object) = (unsafe { Carved::<EventCount>::validate(event_count) })
        else {
            // The EventCount was retired: the binding ends.
            // SAFETY: as above.
            unsafe { (*object).unbind_event_count() }
            return;
        };
        // SAFETY: as above.
        let outcome = unsafe { (*event_count_object).advance(1, &mut self.pending) };
        let Ok(AdvanceOutcome::Advanced { woken, .. } | AdvanceOutcome::Overflow { woken }) =
            outcome
        else {
            return;
        };
        for record in woken.iter() {
            // The woken dispatcher becomes runnable; its own record, if
            // it has one, publishes in turn.
            wake_waiter(self, record).ok();
        }
    }

    /// Account interrupt handling: add each handled line's ticks to its
    /// record and move the start of the current interval past the
    /// `total_ticks` spent, so the running Thread's Time is not charged for
    /// them.
    pub fn account_interrupts(&mut self, handled: &[(u32, u64)], total_ticks: u64) {
        for &(line, ticks) in handled {
            self.irq_table.record_handled(line, ticks);
        }
        let now = self.counter_ticks();
        self.current_since_ticks = self
            .current_since_ticks
            .saturating_add(total_ticks)
            .min(now);
    }

    /// The current Thread's identity. `InvalidThread` when no Thread is
    /// current or the recorded identity no longer names a live Thread.
    pub fn current_thread_id(&self) -> Result<ThreadId, CapError> {
        let id = self.current_thread.ok_or(CapError::InvalidThread)?;
        self.thread(id).ok_or(CapError::InvalidThread)?;
        Ok(id)
    }

    /// The current Thread, after identity validation; see
    /// [`Self::current_thread_id`].
    pub fn current_thread_ref(&self) -> Result<&Thread, CapError> {
        self.thread(self.current_thread_id()?)
            .ok_or(CapError::InvalidThread)
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
    /// `None` once the table is retired.
    pub fn current_thread_table(&self) -> Option<&KeyTable> {
        let binding = self.current_thread_table_binding()?;
        // SAFETY: the binding's identity names a carved record that is never
        // freed; validation rejects a retired table.
        unsafe {
            Carved::<KeyTable>::validate(binding.id())
                .ok()
                .map(|table| &*table)
        }
    }

    /// Exclusive access to the current thread's capability table; `None` once
    /// the table is retired.
    pub fn current_thread_table_mut(&mut self) -> Option<&mut KeyTable> {
        let binding = self.current_thread_table_binding()?;
        // SAFETY: as in `current_thread_table`; `&mut self` makes the
        // reference exclusive.
        unsafe {
            Carved::<KeyTable>::validate(binding.id())
                .ok()
                .map(|table| &mut *table)
        }
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

    /// Thread-teardown-driven cancellation (the D7 cancellation
    /// trigger: cancel every pending-invocation record naming
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
    /// waiters do resume). Thread-slot deallocation is the teardown path's
    /// separate step (Phase 7 Thread control).
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
        // A dispatcher torn down while it waits leaves no parked call behind.
        let scheduler = self
            .thread(thread)
            .and_then(|torn_down| torn_down.scheduler);
        if let Some(scheduler_id) = scheduler
            && let Some(scheduler) = self.scheduler_mut(scheduler_id)
            && scheduler.dispatcher() == Some(thread)
        {
            scheduler.take_parked();
            // Its timer ends with it.
            self.timers.disarm(scheduler_id);
        }
        self.run_queue.remove(thread);
        Ok(())
    }
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
            scheduler::Region,
            thread::{ExecutionContext, RecordSlot, ThreadId},
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
                pending: PendingPool::new(),
                run_queue: RunQueue::new(),
                idle_thread: None,
                irq_table: crate::objects::irq::IrqTable::empty(),
                ticks: 0,
                current_since_ticks: 0,
                dispatched: None,
                dispatch_next: None,
                preempt_requested: false,
                timers: crate::objects::timers::SchedulerTimers::new(),
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
    fn a_notification_wait_past_its_deadline_times_out_and_leaves_the_queue() {
        let nucleus = fixture_nucleus();
        let waiter = carve_thread();
        let (n, _e) = carve_objects();
        let record = block_on_notification(nucleus, n, waiter);
        // No deadline: nothing to expire.
        assert_eq!(nucleus.next_wakeup_ticks(), None);
        nucleus.expire_deadlines();
        assert_eq!(
            nucleus.pending.state(record).ok(),
            Some(PendingState::Waiting)
        );

        // A deadline in the past: the wait completes with TIMED_OUT, the
        // waiter is runnable, and a later signal finds nobody queued.
        nucleus.pending.set_deadline(record, 0).ok().unwrap();
        assert_eq!(nucleus.next_wakeup_ticks(), Some(0));
        nucleus.expire_deadlines();
        assert_eq!(
            nucleus.pending.state(record).ok(),
            Some(PendingState::Completed {
                status: syscall_status::TIMED_OUT,
                result0: 0,
                result1: 0
            })
        );
        assert_eq!(nucleus.run_queue.len(), 1);
        assert_eq!(nucleus.next_wakeup_ticks(), None);
        let notification = object::<Notification>(n);
        assert!(matches!(
            notification.signal(0b1, &mut nucleus.pending),
            Ok(None)
        ));
        assert_eq!(notification.pending_bits(), 0b1);
    }

    #[test_case]
    fn an_await_past_its_deadline_times_out_with_the_current_value() {
        let nucleus = fixture_nucleus();
        let waiter = carve_thread();
        let (_n, e) = carve_objects();
        match object::<EventCount>(e).advance(3, &mut nucleus.pending) {
            Ok(AdvanceOutcome::Advanced { .. }) => {}
            _ => panic!("advance failed"),
        }
        let record = match object::<EventCount>(e).await_ge(e, 10, waiter, &mut nucleus.pending) {
            Ok(AwaitOutcome::Blocked(record)) => record,
            _ => panic!("await should block"),
        };
        nucleus.pending.set_deadline(record, 0).ok().unwrap();
        nucleus.expire_deadlines();
        assert_eq!(
            nucleus.pending.state(record).ok(),
            Some(PendingState::Completed {
                status: syscall_status::TIMED_OUT,
                result0: 3,
                result1: 0
            })
        );
        // The wait left the queue: reaching the target wakes no one.
        match object::<EventCount>(e).advance(7, &mut nucleus.pending) {
            Ok(AdvanceOutcome::Advanced { woken, new_value }) => {
                assert_eq!(woken.iter().count(), 0);
                assert_eq!(new_value, 10);
            }
            _ => panic!("advance failed"),
        }
    }

    #[test_case]
    fn a_completion_before_the_deadline_wins_and_the_deadline_is_forgotten() {
        let nucleus = fixture_nucleus();
        let waiter = carve_thread();
        let (n, _e) = carve_objects();
        let record = block_on_notification(nucleus, n, waiter);
        nucleus.pending.set_deadline(record, 0).ok().unwrap();
        // The signal completes the record first.
        match object::<Notification>(n).signal(0b10, &mut nucleus.pending) {
            Ok(Some(woken)) => assert_eq!(woken, record),
            _ => panic!("signal should wake the waiter"),
        }
        assert_eq!(nucleus.next_wakeup_ticks(), None);
        nucleus.expire_deadlines();
        assert_eq!(
            nucleus.pending.state(record).ok(),
            Some(PendingState::Completed {
                status: syscall_status::SUCCESS,
                result0: 0b10,
                result1: 0
            })
        );
        assert!(nucleus.run_queue.is_empty());
    }

    #[test_case]
    fn the_next_wakeup_is_the_earliest_wait_deadline() {
        let nucleus = fixture_nucleus();
        let (a, b) = (carve_thread(), carve_thread());
        let (n, e) = carve_objects();
        let first = block_on_notification(nucleus, n, a);
        let second = match object::<EventCount>(e).await_ge(e, 1, b, &mut nucleus.pending) {
            Ok(AwaitOutcome::Blocked(record)) => record,
            _ => panic!("await should block"),
        };
        nucleus.pending.set_deadline(first, 900).ok().unwrap();
        nucleus.pending.set_deadline(second, 400).ok().unwrap();
        assert_eq!(nucleus.next_wakeup_ticks(), Some(400));
        assert_eq!(nucleus.pending.earliest_expired(399), None);
        assert_eq!(nucleus.pending.earliest_expired(500), Some(second));
        // A record that is not waiting takes no deadline.
        nucleus.pending.cancel(first).ok().unwrap();
        assert!(nucleus.pending.set_deadline(first, 1).is_err());
        assert_eq!(nucleus.next_wakeup_ticks(), Some(400));
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

    const BYTES: usize = 1 << 22;

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
