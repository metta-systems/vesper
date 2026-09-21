use {
    crate::{
        CapError, Key, KeySlot, RawKey, decode_syscall_result,
        domain::{DcbView, DomainId, DomainState},
    },
    core::sync::atomic::Ordering,
};

#[cfg(not(test))]
use libsyscall::{protected_call0, protected_call2};
#[cfg(test)]
use tests::{protected_call0, protected_call2};

#[cfg(test)]
#[path = "../tests/support/thread.rs"]
mod tests;

/// `Thread` operations (the execution/scheduling remainder of the former
/// `Domain`, split 2026-09-21).
///
/// Operation `0` is unassigned: the former `Domain.Activate` moved to the
/// `AddressSpace` kind (`AddressSpace.Activate` `0`). Do not silently reuse
/// the freed number.
#[repr(u8)]
pub enum ThreadOp {
    Grant = 1,   // Grant a capability to this thread's table
    Suspend = 2, // Suspend the thread
    Resume = 3,  // Resume a suspended thread
    Retire = 4,  // Tear down: cancel pending, reclaim the pool slot
}

impl TryFrom<u64> for ThreadOp {
    type Error = CapError;

    fn try_from(op: u64) -> Result<Self, Self::Error> {
        match op {
            1 => Ok(Self::Grant),
            2 => Ok(Self::Suspend),
            3 => Ok(Self::Resume),
            4 => Ok(Self::Retire),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

/// Thread capability — handle to an execution thread.
/// State queries use the shared DCB (no syscall), mutations use `CapInvoke`.
/// `Retire` is dispatched (2026-09-19 as `Domain.Retire`; moved to the Thread
/// kind 2026-09-21): it tears a non-current Thread down under `RETIRE`
/// authority. `Grant`, `Suspend`, and `Resume` remain unsupported by nucleus
/// dispatch and their wrappers preserve the kernel's errors; they do not
/// establish DCB mapping or lifetime.
///
/// Implementation status: public construction is available for the mutation
/// path (`from_key`), mirroring the other non-owning key wrappers. The safe
/// observation methods still assume a valid DCB mapping and lifetime
/// internally; a raw key alone cannot establish those prerequisites, so
/// callers must not rely on them until DCB mapping is a supported operation
/// (D5 — DCBs become thread scheduling pages shared with the userspace
/// scheduler). Packed-key mutation encoding does not make these observation
/// methods safe for arbitrary handles.
pub struct ThreadKey {
    key: Key<ThreadType>,
    id: DomainId,
}

enum ThreadType {}

impl ThreadKey {
    /// Construct a non-owning handle without installing or validating
    /// authority. The observation methods still require an established DCB
    /// mapping and lifetime (see the type-level implementation-status note).
    pub const fn from_key(key: RawKey, id: DomainId) -> Self {
        Self {
            key: Key::new(key),
            id,
        }
    }

    /// Get thread state from the shared DCB
    #[inline]
    pub fn state(&self) -> DomainState {
        // SAFETY: Unsafe call.
        let dcb_view = unsafe { DcbView::from_user_mapping() };
        let dcb = dcb_view.get(self.id).expect("oh well");
        DomainState::try_from(dcb.state.load(Ordering::Acquire)).unwrap_or(DomainState::Inactive)
    }

    /// Get time used from the shared DCB
    #[inline]
    pub fn time_used_ns(&self) -> u64 {
        // SAFETY: Unsafe call.
        let dcb_view = unsafe { DcbView::from_user_mapping() };
        let dcb = dcb_view.get(self.id).expect("oh well");
        dcb.time_consumed_ns.load(Ordering::Relaxed)
    }

    /// Get pending notifications from the shared DCB (NO SYSCALL!)
    #[inline]
    pub fn pending_notifications(&self) -> u64 {
        // SAFETY: Unsafe call.
        let dcb_view = unsafe { DcbView::from_user_mapping() };
        let dcb = dcb_view.get(self.id).expect("oh well");
        dcb.pending_notifications.load(Ordering::Relaxed)
    }

    /// Grant a capability to this thread
    ///
    /// Implementation status: the kernel operation remains excluded. This carries
    /// the source incarnation and a vacant destination slot, not an installation
    /// or an approved replacement for `KeyTable` delegation.
    pub fn grant<T>(&self, key: &Key<T>, dest_slot: KeySlot) -> Result<(), CapError> {
        // SAFETY: Unsafe call.
        let response = unsafe {
            protected_call2(
                self.key.to_wire(),
                ThreadOp::Grant as u64,
                key.to_wire(),
                u64::from(dest_slot.0),
            )
        };
        decode_syscall_result(response).map(|_| ())
    }

    /// Suspend thread - requires syscall
    pub fn suspend(&self) -> Result<(), CapError> {
        // SAFETY: Unsafe call.
        let response = unsafe { protected_call0(self.key.to_wire(), ThreadOp::Suspend as u64) };
        decode_syscall_result(response).map(|_| ())
    }

    /// Resume suspended thread - requires syscall
    pub fn resume(&self) -> Result<(), CapError> {
        // SAFETY: Unsafe call.
        let response = unsafe { protected_call0(self.key.to_wire(), ThreadOp::Resume as u64) };
        decode_syscall_result(response).map(|_| ())
    }

    /// Retire this thread: tear it down — cancel every pending record naming
    /// it as waiter, purge its queued wakeup — and reclaim its Thread-pool
    /// slot.
    ///
    /// Wire schema (selected 2026-09-19; moved to the Thread kind 2026-09-21):
    /// no arguments. Authority: `RETIRE` on the invoked Thread capability. The
    /// current Thread may not retire itself (`InvalidOperation`): the caller
    /// must be a surviving Thread and this invocation returns normally.
    /// Never-returns self-retirement is recorded in the contract as wanted as
    /// soon as feasible (it needs terminal entry-path work); until then a
    /// Thread's final exit is userspace policy.
    ///
    /// Subsequent invocations of the retired Thread's capabilities fail pool
    /// validation with a defined error. Carved backing (keytable, kernel
    /// stack) stays leaked per accepted-leak; the `AddressSpace` is untouched —
    /// its teardown is the separate `AddressSpace.Retire`; the DCB is
    /// untouched (D5).
    pub fn retire(&self) -> Result<(), CapError> {
        // SAFETY: Unsafe call.
        let response = unsafe { protected_call0(self.key.to_wire(), ThreadOp::Retire as u64) };
        decode_syscall_result(response).map(|_| ())
    }
}
