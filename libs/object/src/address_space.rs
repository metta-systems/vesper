use crate::{
    CapError, Key, KeySlot, KeyTableKey, PlacementWindow, RawKey, decode_syscall_result,
    stack_pool::StackPoolHandle,
};

#[cfg(not(test))]
use libsyscall::{protected_call0, protected_call1, protected_call3, protected_call6_with_x8};
#[cfg(test)]
use tests::{protected_call0, protected_call1, protected_call3, protected_call6_with_x8};

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
    /// root/ASID fields, retire the carved object.
    Retire = 1,
    /// Bind a `KeyTable` to an inert `AddressSpace`, once.
    Bind = 2,
    /// Construct an `Invocation` for an entry point in this `AddressSpace`.
    CreateInvocation = 3,
    /// Make a stack pool: the stacks calls into this `AddressSpace` run on.
    CreateStackPool = 4,
    /// Destroy a stack pool that has no stack in use.
    DestroyStackPool = 5,
}

impl TryFrom<u64> for AddressSpaceOp {
    type Error = CapError;

    fn try_from(op: u64) -> Result<Self, Self::Error> {
        match op {
            0 => Ok(Self::Activate),
            1 => Ok(Self::Retire),
            2 => Ok(Self::Bind),
            3 => Ok(Self::CreateInvocation),
            4 => Ok(Self::CreateStackPool),
            5 => Ok(Self::DestroyStackPool),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

/// `AddressSpace` capability — handle to a protection/mapping context
/// (Vesper's equivalent of seL4's `VSpace`).
///
/// `Activate`, `Retire` and `Bind` are dispatched. `CreateInvocation` installs an
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

    /// Make a stack pool in this `AddressSpace`: `count` (1 to 64) stacks of
    /// `stack_bytes` each, back to back from `base`. A call through an
    /// Invocation that names the pool starts with `SP` at the end of a free
    /// stack, which the kernel takes for the call and gives back at its
    /// `Thread.Return`.
    ///
    /// Wire schema: `x2` base, `x3` `stack_bytes`, `x4` `count`, `x5..x7` zero.
    /// Requires `GRANT`. `[base, base + count * stack_bytes)` must be a nonempty
    /// user-range extent with 16-byte aligned ends, and `stack_bytes` a multiple
    /// of 16 and at least 16 (`InvalidStack`). A full pool table is
    /// `PoolExhausted`. The kernel does not touch the stack memory or prove it
    /// mapped; mapping the stacks, and leaving a guard region below each stack
    /// and above the last unmapped, is the exporter's job.
    pub fn create_stack_pool(
        &self,
        base: u64,
        stack_bytes: u64,
        count: u64,
    ) -> Result<StackPoolHandle, CapError> {
        // SAFETY: protected_call3 encapsulates the SVC ABI; these are raw
        // register operands from the CreateStackPool wire schema.
        let response = unsafe {
            protected_call3(
                self.key.to_wire(),
                AddressSpaceOp::CreateStackPool as u64,
                base,
                stack_bytes,
                count,
            )
        };
        let (handle, _) = decode_syscall_result(response)?;
        StackPoolHandle::from_wire(handle).ok_or(CapError::InvalidOperation)
    }

    /// Destroy the stack pool `pool`. Fails with `InvalidOperation` while any of
    /// its stacks is in use, and with `StaleGeneration` for a handle that no
    /// longer names a pool; otherwise the pool's slot is freed and every
    /// Invocation that named it fails its next call with `StaleGeneration`.
    ///
    /// Wire schema: `x2` handle, `x3..x7` zero. Requires `GRANT`.
    pub fn destroy_stack_pool(&self, pool: StackPoolHandle) -> Result<(), CapError> {
        // SAFETY: protected_call1 encapsulates the SVC ABI; the operand is the
        // handle from the DestroyStackPool wire schema.
        let response = unsafe {
            protected_call1(
                self.key.to_wire(),
                AddressSpaceOp::DestroyStackPool as u64,
                pool.to_wire(),
            )
        };
        decode_syscall_result(response).map(|_| ())
    }

    /// Construct an `Invocation` capability for `function_address` in this
    /// `AddressSpace` and install it into `destination` at `destination_slot`.
    /// Its calls run on stacks from `pool`, one of this `AddressSpace`'s stack
    /// pools. The Invocation accepts no lent key: `CallWithKey` on it fails
    /// with `KeyNotAccepted`. Use [`create_invocation_with_window`] to accept
    /// keys.
    ///
    /// Wire schema: `x2` function address, `x3` destination `KeyTable`
    /// capability, `x4` vacant destination slot, `x5` the pool handle,
    /// `x6..x7` zero and `x8` the key-placement window (zero here). Requires
    /// `GRANT` on this `AddressSpace` and `INSTALL` on the destination
    /// `KeyTable`. The installed `Invocation` capability has `CALL` and `BADGE`
    /// authority; a holder that does not mint copies it with `BADGE` masked off.
    /// The address is stored as supplied; construction does not check mapping or
    /// executable permission. Returns the destination-table-local key.
    ///
    /// [`create_invocation_with_window`]: Self::create_invocation_with_window
    pub fn create_invocation(
        &self,
        function_address: u64,
        destination: &KeyTableKey,
        destination_slot: KeySlot,
        pool: StackPoolHandle,
    ) -> Result<RawKey, CapError> {
        self.create_invocation_with_window(
            function_address,
            destination,
            destination_slot,
            pool,
            PlacementWindow::NONE,
        )
    }

    /// As [`create_invocation`](Self::create_invocation), declaring the
    /// key-placement window `CallWithKey` uses: `placement_window.count()`
    /// consecutive slots of this `AddressSpace`'s `KeyTable` starting at
    /// `placement_window.first()`, in `x8`. The window must lie within the
    /// table's capacity. The kernel does not reserve it; the exporter keeps it
    /// vacant by convention, and its size bounds the number of simultaneous
    /// `CallWithKey` calls into the Invocation.
    pub fn create_invocation_with_window(
        &self,
        function_address: u64,
        destination: &KeyTableKey,
        destination_slot: KeySlot,
        pool: StackPoolHandle,
        placement_window: PlacementWindow,
    ) -> Result<RawKey, CapError> {
        // SAFETY: protected_call6_with_x8 encapsulates the SVC ABI; these are
        // raw register operands from the selected CreateInvocation wire schema.
        let response = unsafe {
            protected_call6_with_x8(
                self.key.to_wire(),
                AddressSpaceOp::CreateInvocation as u64,
                function_address,
                destination.to_wire(),
                u64::from(destination_slot.0),
                pool.to_wire(),
                0,
                0,
                placement_window.to_wire(),
            )
        };
        let (key, _) = decode_syscall_result(response)?;
        Ok(RawKey::from_wire(key))
    }

    /// Bind `table` to this inert `AddressSpace`: Threads running in it then
    /// resolve their keys through `table`.
    ///
    /// Wire schema: `x2` the `KeyTable` capability, `x3..x7` zero. Authority:
    /// `MAP` on this `AddressSpace` and `INSTALL` on `table`. A binding is
    /// made once (`AlreadyMapped` on a bound `AddressSpace`) and lasts for the
    /// `AddressSpace`'s lifetime; one table may be bound to several
    /// `AddressSpace`s. The table's `SELF_KEYTABLE` capability must name the
    /// table itself; Bind installs the `THREAD_RETURN` sentinel and nothing
    /// else. Until bound, an `AddressSpace` cannot be activated, called into
    /// or receive Threads (`NotBound`).
    pub fn bind(&self, table: &KeyTableKey) -> Result<(), CapError> {
        // SAFETY: protected_call1 encapsulates the SVC ABI; the operand is
        // the table key from the Bind wire schema.
        let response = unsafe {
            protected_call1(
                self.key.to_wire(),
                AddressSpaceOp::Bind as u64,
                table.to_wire(),
            )
        };
        decode_syscall_result(response).map(|_| ())
    }

    /// Activate this address space: install its bound translation root as
    /// the current hardware translation context (requires syscall).
    ///
    /// Wire schema: no arguments. The `AddressSpace` must be bound
    /// (`NotBound` otherwise), must
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
    /// root/ASID fields, and retire the carved object (its generation is
    /// bumped, so every capability to it fails).
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
