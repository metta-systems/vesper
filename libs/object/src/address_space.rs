use crate::{CapError, Key, KeySlot, KeyTableKey, RawKey, decode_syscall_result};

#[cfg(not(test))]
use libsyscall::{protected_call0, protected_call6};
#[cfg(test)]
use tests::{protected_call0, protected_call6};

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
    /// Construct an `Invocation` for an entry point in this `AddressSpace`.
    CreateInvocation = 3,
}

impl TryFrom<u64> for AddressSpaceOp {
    type Error = CapError;

    fn try_from(op: u64) -> Result<Self, Self::Error> {
        match op {
            0 => Ok(Self::Activate),
            1 => Ok(Self::Retire),
            3 => Ok(Self::CreateInvocation),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

/// `AddressSpace` capability — handle to a protection/mapping context
/// (Vesper's equivalent of seL4's `VSpace`).
///
/// `Activate` and `Retire` are dispatched. `CreateInvocation` installs an
/// `Invocation` capability into a destination `KeyTable`; PPC invocation itself
/// remains unsupported.
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

    /// Construct an `Invocation` capability for `function_address` in this
    /// `AddressSpace` and install it into `destination` at `destination_slot`.
    ///
    /// Wire schema: `x2` function address, `x3` destination `KeyTable`
    /// capability, `x4` vacant destination slot, `x5` stack base, `x6` exclusive
    /// stack end, and `x7` positive minimum downward headroom in bytes. Requires
    /// `GRANT` on this `AddressSpace` and `INSTALL` on the destination
    /// `KeyTable`. The installed `Invocation` capability has `CALL` authority only. The
    /// address is stored as supplied; construction does not check mapping or
    /// executable permission. Returns the destination-table-local key.
    ///
    /// The target publishes `[stack_base, stack_end)` and `minimum_headroom`;
    /// all three must be multiples of 16 bytes, with a nonempty user-range
    /// extent and a positive minimum that fits it. Kernel validation owns these
    /// invariants; this wrapper forwards supplied values unchanged. Numeric
    /// admission does not prove mapped/writable backing or stack exclusivity.
    pub fn create_invocation(
        &self,
        function_address: u64,
        destination: &KeyTableKey,
        destination_slot: KeySlot,
        stack_base: u64,
        stack_end: u64,
        minimum_headroom: u64,
    ) -> Result<RawKey, CapError> {
        // SAFETY: protected_call6 encapsulates the SVC ABI; these are raw
        // register operands from the selected CreateInvocation wire schema.
        let response = unsafe {
            protected_call6(
                self.key.to_wire(),
                AddressSpaceOp::CreateInvocation as u64,
                function_address,
                destination.to_wire(),
                u64::from(destination_slot.0),
                stack_base,
                stack_end,
                minimum_headroom,
            )
        };
        let (key, _) = decode_syscall_result(response)?;
        Ok(RawKey::from_wire(key))
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
