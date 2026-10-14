//! Checked translation preparation and the wait/resume scheduling transaction.
//!
//! Implementation status: this is the bounded fixture scheduler's mechanism,
//! not public Thread control, PPC dispatch, Time policy, or fault recovery.

use {
    crate::objects::{
        ArchObjects, ExecutionContext, Nucleus,
        access::{Access, ObjectId, PoolTag},
        arch_objects::{AddressSpaceId, AddressSpaceObject},
        completion::{PendingKind, PendingState},
        key_table::KeyTableBinding,
        thread::ThreadId,
    },
    libexception::arch::aarch64::{ExceptionOrigin, SavedContext},
    libobject::CapError,
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
        self.switch_away(access, current, &ExecutionContext::Parked { saved, record })
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
        self.switch_away(access, current, &ExecutionContext::Faulted { saved })
    }

    /// Preemption-timer tick: if another Thread is runnable, stop the current
    /// one as `Preempted` with its interrupted state `saved`, select the FIFO
    /// front and requeue the current Thread behind it (the idle Thread is
    /// never queued). `None` means the current Thread keeps running.
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
        if self.run_queue.is_empty() {
            return Ok(None);
        }
        let current_is_idle = self.idle_thread == Some(current);
        let resumed = self.switch_away(access, current, &ExecutionContext::Preempted { saved })?;
        // switch_away dequeued the front, and a running Thread is never queued.
        if !current_is_idle {
            assert!(
                self.run_queue.push(current),
                "preempted Thread did not requeue"
            );
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
    ) -> Result<PreparedResume, CapError> {
        // Peek is sufficient only because validation and dequeue share this
        // exclusive transaction.
        let queued = self.run_queue.peek();
        let next = match (queued, self.idle_thread) {
            (Some(next), _) => next,
            // Nothing is runnable: fall back to the idle Thread.
            (None, Some(idle)) if idle != current => idle,
            (None, _) => return Err(CapError::InvalidOperation),
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
        self.thread_mut(current)
            .expect("validated current Thread")
            .context = *parked;
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
