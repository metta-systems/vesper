use crate::{CapError, Key, RawKey, decode_syscall_result};

#[cfg(not(test))]
use libsyscall::protected_call0;
#[cfg(test)]
use tests::protected_call0;

#[cfg(test)]
#[path = "../tests/support/address_space.rs"]
mod tests;

/// `AddressSpace` operations (the translation-root holder — Vesper's
/// equivalent of seL4's `VSpace`).
#[repr(u8)]
pub enum AddressSpaceOp {
    /// Install the bound translation root as the current hardware
    /// translation context.
    Activate = 0,
    /// Tear the address space down: release the bound ASID, clear the
    /// root/ASID fields, reclaim the pool slot.
    Retire = 1,
}

impl TryFrom<u64> for AddressSpaceOp {
    type Error = CapError;

    fn try_from(op: u64) -> Result<Self, Self::Error> {
        match op {
            0 => Ok(Self::Activate),
            1 => Ok(Self::Retire),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

/// `AddressSpace` capability — handle to a protection/mapping context
/// (Vesper's equivalent of seL4's `VSpace`).
///
/// `Activate` is dispatched: it installs the bound translation root as
/// the hardware translation context. `Retire` is dispatched: it
/// tears a non-current `AddressSpace` down under `RETIRE` authority.
pub struct AddressSpaceKey {
    key: Key<AddressSpaceType>,
}

enum AddressSpaceType {}

impl AddressSpaceKey {
    /// Construct a non-owning handle without installing or validating
    /// authority.
    pub const fn from_key(key: RawKey) -> Self {
        Self { key: Key::new(key) }
    }

    /// The wire encoding of the underlying key.
    pub const fn to_wire(&self) -> u64 {
        self.key.to_wire()
    }

    /// Activate this address space: install its bound translation root as
    /// the current hardware translation context (requires syscall).
    ///
    /// Wire schema: no arguments. The `AddressSpace` must
    /// have a translation root installed and an ASID bound (`NotMapped`
    /// otherwise) and must be the current caller's own `AddressSpace`
    /// (`InvalidOperation` otherwise). Authority: `MAP` on the `AddressSpace`
    /// capability. This is the translation-context installation step of
    /// activation only; full Thread Start/Suspend/Resume with execution
    /// contexts and budget remains Phase 7 work.
    pub fn activate(&self) -> Result<(), CapError> {
        // SAFETY: Unsafe call.
        let response =
            unsafe { protected_call0(self.key.to_wire(), AddressSpaceOp::Activate as u64) };
        decode_syscall_result(response).map(|_| ())
    }

    /// Retire this address space: release its bound ASID back to the
    /// originating pool (after the whole-ASID TLB invalidation), clear the
    /// root/ASID fields, and reclaim its AddressSpace-pool slot.
    ///
    /// Wire schema: no arguments. Authority: `RETIRE`
    /// on the invoked `AddressSpace` capability. The current caller's own
    /// `AddressSpace` may not be retired (`InvalidOperation`), and a
    /// translation root must not still be installed (`InvalidOperation`) —
    /// the root is torn down first through the empty-table-gated
    /// `PageTable.Unmap` path.
    ///
    /// Threads still referencing the retired `AddressSpace` fail generation
    /// validation on their next resolution.
    pub fn retire(&self) -> Result<(), CapError> {
        // SAFETY: Unsafe call.
        let response =
            unsafe { protected_call0(self.key.to_wire(), AddressSpaceOp::Retire as u64) };
        decode_syscall_result(response).map(|_| ())
    }
}
