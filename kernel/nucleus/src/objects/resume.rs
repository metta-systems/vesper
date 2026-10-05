//! Checked translation preparation and the wait/resume scheduling transaction.
//!
//! Implementation status: this is the bounded fixture scheduler's mechanism,
//! not public Thread control, PPC dispatch, Time policy, or fault recovery.

use {
    crate::objects::{
        ArchObjects, ExecutionContext, Nucleus, ObjectPool,
        access::{Access, ObjectId, PoolTag},
        arch_objects::AddressSpaceObject,
        completion::{PendingKind, PendingState},
        key_table::KeyTableBinding,
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
    address_space: ObjectId,
    keytable: KeyTableBinding,
    root: u64,
    asid: u16,
}

impl PreparedTranslationContext {
    pub const fn address_space(self) -> ObjectId {
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
/// Missing root/ASID is `NotMapped`; invalid/stale AS is `InvalidDomain`;
/// hardware-format rejection uses the architecture's existing explicit errors.
pub fn prepare_translation_context<A: ArchObjects>(
    access: &Access,
    pool: &ObjectPool<A::AddressSpace>,
    id: ObjectId,
) -> Result<PreparedTranslationContext, CapError> {
    let address_space = access
        .resolve(pool, id)
        .map_err(|_invalid_identity| CapError::InvalidDomain)?;
    let root = address_space
        .translation_root()
        .ok_or(CapError::NotMapped)?;
    let asid = address_space.asid().ok_or(CapError::NotMapped)?;
    A::validate_translation_context(root, asid)?;
    Ok(PreparedTranslationContext {
        address_space: id,
        keytable: address_space.keytable(),
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
    pub current: u32,
    pub next: u16,
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
            return Err(CapError::InvalidDomain);
        }
        let current = self.current_thread.ok_or(CapError::InvalidDomain)?;
        let current_index =
            usize::try_from(current).map_err(|_invalid_index| CapError::InvalidDomain)?;
        let current_thread = self
            .pools
            .threads
            .get_live(current_index)
            .ok_or(CapError::InvalidDomain)?;
        if current_thread.context != ExecutionContext::Running {
            return Err(CapError::InvalidOperation);
        }
        let current_waiter = self.pending.waiter(record)?;
        self.pools
            .threads
            .validate(current_waiter)
            .map_err(|_invalid_waiter| CapError::InvalidDomain)?;
        if current_waiter.pool != PoolTag::Thread
            || usize::from(current_waiter.index) != current_index
            || self.pending.state(record)? != PendingState::Waiting
        {
            return Err(CapError::InvalidOperation);
        }

        // Peek is sufficient only because validation and dequeue share this
        // exclusive transaction. The queue still carries indices, not Thread
        // incarnations; general scheduler identity/reuse remains D3/D5 work.
        let next = self.scheduler.peek().ok_or(CapError::InvalidOperation)?;
        let next_index = usize::from(next);
        let target = self
            .pools
            .threads
            .get_live(next_index)
            .ok_or(CapError::InvalidDomain)?;
        let translation = prepare_translation_context::<A>(
            access,
            &self.pools.arch.address_spaces,
            target.address_space,
        )?;
        let (mut restored, terminal) = match target.context {
            ExecutionContext::NotStarted { saved } => (saved, None),
            ExecutionContext::Parked { saved, record } => {
                // Wakeup enqueues the Thread only after the pending record's
                // single terminal transition. Deliver its full result shape,
                // including operation-specific errors such as Await overflow.
                let waiter = self.pending.waiter(record)?;
                self.pools
                    .threads
                    .validate(waiter)
                    .map_err(|_invalid_waiter| CapError::InvalidDomain)?;
                if usize::from(waiter.index) != next_index {
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
            ExecutionContext::Running => return Err(CapError::InvalidOperation),
        };
        if !execution_origin(restored.origin) {
            return Err(CapError::InvalidDomain);
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
        self.pools
            .threads
            .get_live_mut(current_index)
            .expect("validated current Thread")
            .context = ExecutionContext::Parked { saved, record };
        assert_eq!(
            self.scheduler.pop(),
            Some(next),
            "validated scheduler front changed"
        );
        self.pools
            .threads
            .get_live_mut(next_index)
            .expect("validated target Thread")
            .context = ExecutionContext::Running;
        self.current_thread = Some(u32::from(next));
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
