//! Checked translation preparation and the wait/resume scheduling transaction.
//!
//! Implementation status: this is the bounded fixture scheduler's mechanism,
//! not public Thread control, PPC dispatch, Time policy, or fault recovery.

use {
    crate::objects::{
        ArchObjects, ExecutionContext, Nucleus, Thread,
        access::{Access, ObjectId, PoolTag},
        arch_objects::{AddressSpaceId, AddressSpaceObject},
        carved::Carved,
        completion::{PendingKind, PendingState},
        key_table::KeyTableBinding,
        thread::ThreadId,
    },
    libexception::arch::aarch64::{ExceptionOrigin, SavedContext},
    libobject::{CapError, scheduler::activation_reason, scheduler_record::StopReason},
};

/// Copied, checked activation metadata; contains no object guard or reference.
///
/// This is not a lifetime pin or authority token. Install immediately after
/// the serialized preparation/commit scope ends, before returning to execution.
/// The single-core, non-reentrant trap path must prevent retirement, unmapping,
/// ASID release/reuse, or another scheduling transaction in that interval.
/// Keeping this value for a later invocation requires fresh validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedTranslationContext {
    address_space: AddressSpaceId,
    keytable: KeyTableBinding,
    root: u64,
    asid: u16,
}

impl PreparedTranslationContext {
    pub const fn address_space(self) -> AddressSpaceId {
        self.address_space
    }

    pub const fn keytable(self) -> KeyTableBinding {
        self.keytable
    }

    pub const fn root(self) -> u64 {
        self.root
    }

    pub const fn asid(self) -> u16 {
        self.asid
    }
}

/// Resolve the AS incarnation before reading its root, ASID, or table binding.
/// No allocation, hardware write, or object mutation occurs on this path.
/// An unbound AS is `NotBound`; missing root/ASID is `NotMapped`; invalid/stale
/// AS is `InvalidAddressSpace`;
/// hardware-format rejection uses the architecture's existing explicit errors.
pub fn prepare_translation_context<A: ArchObjects>(
    access: &Access,
    id: AddressSpaceId,
) -> Result<PreparedTranslationContext, CapError> {
    let address_space = access
        .resolve_object::<A::AddressSpace>(id)
        .map_err(|_invalid_identity| CapError::InvalidAddressSpace)?;
    let keytable = address_space.keytable().ok_or(CapError::NotBound)?;
    let root = address_space
        .translation_root()
        .ok_or(CapError::NotMapped)?;
    let asid = address_space.asid().ok_or(CapError::NotMapped)?;
    A::validate_translation_context(root, asid)?;
    Ok(PreparedTranslationContext {
        address_space: id,
        keytable,
        root,
        asid,
    })
}

/// Terminal wait result delivered by the scheduling commit, for entry tracing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResumeCompletion {
    pub kind: PendingKind,
    pub status: u64,
    pub result0: u64,
}

/// Committed selection whose hardware installation/frame restore is still owed.
/// No borrowed Thread, `AddressSpace`, pending record, or trap frame survives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedResume {
    pub current: ThreadId,
    pub next: ThreadId,
    pub saved: SavedContext,
    pub translation: PreparedTranslationContext,
    pub completion: Option<ResumeCompletion>,
}

impl<A: ArchObjects> Nucleus<A> {
    /// Validate the FIFO front and park/select under one scheduling lock.
    ///
    /// Every fallible check precedes commitment: an error preserves both
    /// Threads' contexts, all pending records, current selection, and FIFO.
    /// The incoming record was already admitted by dispatch; errors here do
    /// not undo that admission or report fake completion to its blocked caller.
    /// Entry must install the returned translation after guards/lock end.
    pub fn park_and_select(
        &mut self,
        access: &Access,
        saved: SavedContext,
        record: ObjectId,
    ) -> Result<PreparedResume, CapError> {
        if !execution_origin(saved.origin) {
            return Err(CapError::InvalidThread);
        }
        let current = self.current_thread_id()?;
        let current_thread = self.thread(current).ok_or(CapError::InvalidThread)?;
        if current_thread.context != ExecutionContext::Running {
            return Err(CapError::InvalidOperation);
        }
        if self.pending.waiter(record)? != current
            || self.pending.state(record)? != PendingState::Waiting
            || self.pending.state(record)? != PendingState::Waiting
        {
            return Err(CapError::InvalidOperation);
        }
        let forced = self.dispatch_next.take();
        let resumed = self.switch_away(
            access,
            current,
            &ExecutionContext::Parked { saved, record },
            forced.map(|dispatched| dispatched.thread),
        )?;
        // A dispatch names its Thread, which is now the dispatched one. A stop
        // of the dispatched Thread has already settled `dispatched` (for a
        // child domain, to the child dispatcher's entry): it only stays
        // stale when nothing was activated.
        if forced.is_some() {
            self.dispatched = forced;
        } else if self
            .dispatched
            .is_some_and(|dispatched| dispatched.thread == current)
        {
            self.dispatched = None;
        }
        Ok(resumed)
    }

    /// Stop the current Thread for good after an unhandled fault (or a fault
    /// handler's terminate action) and select the next runnable Thread.
    ///
    /// `saved` is the faulting state, kept in the `Faulted` context for
    /// inspection. Like [`Nucleus::park_and_select`], every fallible check
    /// precedes commitment, and entry installs the returned translation.
    pub fn park_faulted_and_select(
        &mut self,
        access: &Access,
        saved: SavedContext,
    ) -> Result<PreparedResume, CapError> {
        if !execution_origin(saved.origin) {
            return Err(CapError::InvalidThread);
        }
        let current = self.current_thread_id()?;
        let current_thread = self.thread(current).ok_or(CapError::InvalidThread)?;
        if current_thread.context != ExecutionContext::Running {
            return Err(CapError::InvalidOperation);
        }
        self.switch_away(access, current, &ExecutionContext::Faulted { saved }, None)
    }

    /// `Thread.Retire` on the current Thread: select the next Thread (or the
    /// idle Thread), then tear the current Thread down as any retire does.
    /// Every fallible check precedes commitment, so a failure here (nothing
    /// else can run) leaves the Thread untouched for its caller to see the
    /// error. A dispatched Thread activates its dispatcher with `EXITED`.
    pub fn exit_current_thread(&mut self, access: &Access) -> Result<PreparedResume, CapError> {
        let current = self.current_thread_id()?;
        let current_thread = self.thread(current).ok_or(CapError::InvalidThread)?;
        if current_thread.context != ExecutionContext::Running || self.idle_thread == Some(current)
        {
            return Err(CapError::InvalidOperation);
        }
        // `Inactive` stands for "gone": it is neither a wait nor a fault, and
        // `switch_away` reports it as an exit.
        let resumed = self.switch_away(access, current, &ExecutionContext::Inactive, None)?;
        self.cancel_thread_pending(current)?;
        self.release_thread_fault(current);
        self.release_thread_placements(access, current);
        self.leave_scheduler(current);
        self.publish_thread(current, true);
        // SAFETY: Thread identities name carved records that are never freed;
        // the Thread is stopped and nothing else holds a reference to it.
        unsafe { Carved::<Thread>::retire(current) }
            .unwrap_or_else(|_| panic!("a stopped current Thread failed to retire"));
        Ok(resumed)
    }

    /// Preemption-timer tick: charge the interval to the current Thread, then,
    /// if another Thread is runnable or the current Thread's Time ran out,
    /// stop the current one as `Preempted` with its interrupted state
    /// `saved`, select the FIFO front (or the idle Thread) and requeue the
    /// current Thread behind it unless its Time is exhausted (the idle
    /// Thread is never queued). `None` means the current Thread keeps
    /// running.
    ///
    /// Like [`Nucleus::park_and_select`], every fallible check precedes
    /// commitment, and entry installs the returned translation.
    pub fn preempt_and_select(
        &mut self,
        access: &Access,
        saved: SavedContext,
    ) -> Result<Option<PreparedResume>, CapError> {
        if !execution_origin(saved.origin) {
            return Err(CapError::InvalidThread);
        }
        let current = self.current_thread_id()?;
        let current_thread = self.thread(current).ok_or(CapError::InvalidThread)?;
        if current_thread.context != ExecutionContext::Running {
            return Err(CapError::InvalidOperation);
        }
        // Charge the interval whether or not the Thread is switched away from,
        // so a Thread running alone is metered too.
        self.charge_current(current);
        // A tick consumes a preemption request: the dispatcher it asked for
        // is at the front of the queue now.
        self.preempt_requested = false;
        // A dispatched Thread that reached its limit or ran out of budget
        // activates the dispatcher.
        if let Some(dispatched) = self.dispatched.filter(|d| d.thread == current) {
            let now = self.counter_ticks();
            // The Thread of a child domain stops for the parent when the
            // grant is over, whatever its own limit says.
            let grant_over = self
                .scheduler_mut(dispatched.scheduler)
                .and_then(|scheduler| scheduler.grant())
                .and_then(|grant| grant.deadline_ticks)
                .is_some_and(|end| now >= end);
            if grant_over {
                self.end_grant(dispatched.scheduler);
            } else {
                if dispatched.deadline_ticks.is_some_and(|limit| now >= limit) {
                    self.request_activation(dispatched.scheduler, activation_reason::LIMIT, 0);
                }
                if self.thread_exhausted(current) {
                    self.request_activation(dispatched.scheduler, activation_reason::EXHAUSTED, 0);
                }
            }
        }
        // A child domain's Thread that the next Thread in line is not the
        // child's dispatcher for is stopped by the parent: the grant ends.
        if let Some(dispatched) = self.dispatched.filter(|d| d.thread == current) {
            let child_dispatcher = self
                .scheduler_mut(dispatched.scheduler)
                .filter(|scheduler| scheduler.grant().is_some())
                .map(|scheduler| scheduler.dispatcher());
            if let Some(child_dispatcher) = child_dispatcher
                && !self.run_queue.is_empty()
                && self.run_queue.peek() != child_dispatcher
            {
                self.end_grant(dispatched.scheduler);
            }
        }
        let current_is_idle = self.idle_thread == Some(current);
        // An exhausted Thread must stop even when nothing is queued, if the
        // idle Thread can take over.
        let can_switch = !self.run_queue.is_empty()
            || (self.thread_exhausted(current) && !current_is_idle && self.idle_thread.is_some());
        if !can_switch {
            return Ok(None);
        }
        let resumed = self.switch_away(
            access,
            current,
            &ExecutionContext::Preempted { saved },
            None,
        )?;
        // switch_away dequeued the front, and a running Thread is never
        // queued. An exhausted Thread waits off the queue for budget.
        if !current_is_idle {
            self.requeue_preempted(current);
        }
        Ok(Some(resumed))
    }

    /// Whether the idle Thread is current.
    pub fn idle_is_current(&self) -> bool {
        self.idle_thread.is_some() && self.idle_thread == self.current_thread
    }

    /// Validate the FIFO front (or, if nothing is queued, the idle Thread),
    /// then store `parked` as the current Thread's context and make the
    /// selected Thread current. The caller has validated the current Thread;
    /// nothing is mutated before the last fallible check.
    fn switch_away(
        &mut self,
        access: &Access,
        current: ThreadId,
        parked: &ExecutionContext,
        forced: Option<ThreadId>,
    ) -> Result<PreparedResume, CapError> {
        // The Thread a dispatcher dispatched stops: that activates the
        // dispatcher, which is then at the front of the queue. A tick stop
        // (limit, budget) raised its activation already.
        // A child dispatcher that faults or exits takes its domain down; the
        // parent hears of it together with the stop itself.
        let failed_domain = if matches!(
            parked,
            ExecutionContext::Faulted { .. } | ExecutionContext::Inactive
        ) {
            self.thread(current)
                .and_then(|thread| thread.dispatches)
                .and_then(|child| self.fail_domain_state(child))
        } else {
            None
        };
        let mut failed_reported = false;
        if let Some(dispatched) = self.dispatched.filter(|d| d.thread == current) {
            // A child's dispatcher handing the CPU to one of its Threads is
            // not a stop: the grant stays in force. Any other way the child
            // dispatcher leaves the CPU ends the grant.
            let handing_on = forced.is_some()
                && matches!(parked, ExecutionContext::Parked { .. })
                && self
                    .thread(current)
                    .is_some_and(|thread| thread.dispatches.is_some());
            if !handing_on {
                self.drop_grant_of(current);
                let reasons = match parked {
                    ExecutionContext::Parked { .. } => activation_reason::BLOCKED,
                    ExecutionContext::Faulted { .. } => activation_reason::FAULTED,
                    ExecutionContext::Inactive => activation_reason::EXITED,
                    _ => 0,
                };
                let reasons = if failed_domain.is_some() {
                    failed_reported = true;
                    reasons | activation_reason::SCHED_DOMAIN_FAILED
                } else {
                    reasons
                };
                if reasons != 0 {
                    self.request_activation(dispatched.scheduler, reasons, 0);
                }
            }
        }
        if let (Some(parent), false) = (failed_domain, failed_reported) {
            self.request_activation(parent, activation_reason::SCHED_DOMAIN_FAILED, 0);
        }
        // Peek is sufficient only because validation and dequeue share this
        // exclusive transaction. A dispatch names its Thread, which is not
        // queued.
        let queued = if forced.is_some() {
            None
        } else {
            self.run_queue.peek()
        };
        let next = match (forced, queued, self.idle_thread) {
            (Some(next), _, _) | (None, Some(next), _) => next,
            // Nothing is runnable: fall back to the idle Thread.
            (None, None, Some(idle)) if idle != current => idle,
            (None, None, _) => return Err(CapError::InvalidOperation),
        };
        // The selected Thread becomes current as a checked identity.
        let target = self.thread(next).ok_or(CapError::InvalidThread)?;
        let translation = prepare_translation_context::<A>(access, target.address_space)?;
        let (mut restored, terminal) = match target.context {
            ExecutionContext::NotStarted { saved } | ExecutionContext::Preempted { saved } => {
                (saved, None)
            }
            ExecutionContext::Parked { saved, record } => {
                // Wakeup enqueues the Thread only after the pending record's
                // single terminal transition. Deliver its full result shape,
                // including operation-specific errors such as Await overflow.
                if self.pending.waiter(record)? != next {
                    return Err(CapError::InvalidOperation);
                }
                let (status, result0, result1) = match self.pending.state(record)? {
                    PendingState::Completed {
                        status,
                        result0,
                        result1,
                    } => (status, result0, result1),
                    // Cancellation outcomes still need their D9 encoding.
                    // Thread teardown releases records without resuming the
                    // torn-down waiter; object teardown has no resume caller.
                    PendingState::Cancelled | PendingState::Waiting => {
                        return Err(CapError::InvalidOperation);
                    }
                };
                let completion = ResumeCompletion {
                    kind: self.pending.kind(record)?,
                    status,
                    result0,
                };
                (saved, Some((record, completion, result1)))
            }
            // Faulted, inert and configured-but-unstarted Threads are never
            // queued; finding one is corruption.
            ExecutionContext::Running
            | ExecutionContext::Faulted { .. }
            | ExecutionContext::Inactive
            | ExecutionContext::Configured { .. } => {
                return Err(CapError::InvalidOperation);
            }
        };
        if !execution_origin(restored.origin) {
            return Err(CapError::InvalidThread);
        }

        // Commit: all identities/state/translation metadata were checked, and
        // exclusive access prevents any change between validation and release.
        // release itself rejects without mutation if its precondition fails.
        let completion = if let Some((record, completion, result1)) = terminal {
            self.pending.release(record)?;
            restored.gpr[0] = completion.status;
            restored.gpr[1] = completion.result0;
            restored.gpr[2] = result1;
            Some(completion)
        } else {
            None
        };
        // Charge the interval the current Thread just ran. Every switch away
        // (block, preempt, fault, Return) commits here, and a tick that keeps
        // the Thread running charges it too, so this is where execution time
        // is accounted.
        let elapsed = self.charge_current(current);
        // Why it stopped, for its record: blocked, faulted, out of budget
        // or at the end of its slice.
        let stop = match parked {
            ExecutionContext::Parked { .. } => StopReason::Blocked,
            ExecutionContext::Faulted { .. } => StopReason::Faulted,
            ExecutionContext::Inactive => StopReason::Exited,
            ExecutionContext::Preempted { .. } if self.thread_exhausted(current) => {
                StopReason::BudgetExhausted
            }
            _ => StopReason::SliceEnd,
        };
        let leaving = self.thread_mut(current).expect("validated current Thread");
        leaving.last_stop = stop as u32;
        leaving.last_run_ticks = elapsed;
        leaving.context = *parked;
        if queued.is_some() {
            assert_eq!(
                self.run_queue.pop(),
                Some(next),
                "validated scheduler front changed"
            );
        }
        self.thread_mut(next)
            .expect("validated target Thread")
            .context = ExecutionContext::Running;
        self.current_thread = Some(next);
        self.preempt_requested = false;
        // Publish the two state changes last: a record write can wake the
        // dispatcher, which must see the switch complete.
        self.publish_thread(current, false);
        self.publish_thread(next, false);
        Ok(PreparedResume {
            current,
            next,
            saved: restored,
            translation,
            completion,
        })
    }
}

pub(crate) fn execution_origin(origin: ExceptionOrigin) -> bool {
    matches!(
        origin,
        ExceptionOrigin::CurrentSp0 | ExceptionOrigin::LowerAarch64
    )
}
