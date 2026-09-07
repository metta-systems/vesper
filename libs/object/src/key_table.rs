use crate::{CapError, CoreType, Key, RawKey, Rights, decode_syscall_result};

#[cfg(not(test))]
use libsyscall::{protected_call1, protected_call4};
#[cfg(test)]
use tests::{protected_call1, protected_call4};

#[cfg(test)]
#[path = "../tests/support/key_table.rs"]
mod tests;

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Slot index in a `KeyTable`
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct KeySlot(pub u32);

impl KeySlot {
    pub const NULL: KeySlot = KeySlot(0);
    pub const SELF_DOMAIN: KeySlot = KeySlot(1);
    pub const PARENT_DOMAIN: KeySlot = KeySlot(2);
    // CSpace layout with self-reference
    pub const CAPTBL_SELF: KeySlot = KeySlot(3); // Every domain has cap to own captbl here - or rather to KeyMaster
    // ... other well-known slots
    pub const DEBUG_CONSOLE: KeySlot = KeySlot(127); // FIXME: randomly chosen for now
}

/// Userspace handle to a capability table.
///
/// Nucleus dispatch currently rejects `KeyTable` operations as unsupported. The
/// syscall-backed wrappers preserve its errors; they do not implement lifecycle
/// or delegation policy.
pub struct KeyTableKey {
    key: Key<KeyTableType>,
}

enum KeyTableType {}

/// Existing operation vocabulary; decoding an ID does not imply kernel support.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum KeyTableOp {
    CopyDerive = 0, // copy cap between slots or create derived cap with reduced rights
    Move = 1,       // move cap between slots
    Delete = 2,     // delete cap at slot
    Revoke = 4,     // revoke all children of cap
}

impl TryFrom<u64> for KeyTableOp {
    type Error = CapError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::CopyDerive),
            1 => Ok(Self::Move),
            2 => Ok(Self::Delete),
            4 => Ok(Self::Revoke),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

impl TryFrom<u32> for KeyTableOp {
    type Error = CapError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::try_from(u64::from(value))
    }
}

// Userspace KeyMaster must track parent→child relationships,
// kernel only manages flat key tables.

// Contract status: the KeyMaster/kernel derivation and revocation trust boundary
// remains an open decision (D2).
// Contract status update: the management-authority/bookkeeping split is approved;
// selective revocation mechanisms and the table-rights matrix remain unresolved.

impl KeyTableKey {
    /// Construct a non-owning handle without installing or validating authority.
    pub const fn from_key(key: RawKey) -> Self {
        Self { key: Key::new(key) }
    }

    // This naturally supports cross-domain derivation:
    // "Create a read-only view of my buffer in their cspace"
    // derive(&my_captbl, buffer_slot, &their_captbl, their_slot, Rights::READ)?;
    /// Copy with derivation in single syscall
    ///
    /// Returns the issued destination-table-local selector, directly invocable
    /// only if that table is the caller's implicit table. The rights encoding is
    /// provisional pending the table-rights matrix; this does not enable dispatch.
    ///
    /// The client decodes errors through `decode_syscall_result`, then returns
    /// the key from the first success word and ignores the second. The producer
    /// must emit zero in `x2`; no receiver reserved-zero rejection rule is approved.
    pub fn copy_derive(
        &self,
        src_key: RawKey,
        dst_captbl: &KeyTableKey, // Could be same or different!
        dst_slot: u32,
        rights: Rights,
    ) -> Result<RawKey, CapError> {
        // SAFETY: Unsafe call.
        let result = unsafe {
            protected_call4(
                self.key.to_wire(),
                KeyTableOp::CopyDerive as u64,
                src_key.to_wire(),
                dst_captbl.key.to_wire(),
                u64::from(dst_slot),
                u64::from(rights.bits()),
            )
        };
        let (key, _) = decode_syscall_result(result)?;
        Ok(RawKey::from_wire(key))
    }

    // fn activate(&self, slot: u32, object: NucleusObject) -> Result<()> {
    //     let captbl = self.get_captbl_mut()?;
    //     // SAFETY: User specifies slot, but kernel validates
    //     if slot >= captbl.len() {
    //         return Err(Error::SlotOutOfRange);
    //     }
    //     if captbl.slots[slot].is_valid() {
    //         return Err(Error::SlotOccupied);  // User's bookkeeping was wrong
    //     }
    //     // Kernel creates the cap - user never touches this
    //     captbl.slots[slot] = Cap::new(object);
    //     Ok(())
    // }

    /// Move the key, named "transfer" to avoid clashing with Rust's reserved word.
    ///
    /// Implementation status: unimplemented placeholder; does not invoke Move.
    /// Status update: explicitly returns unsupported instead of a successful
    /// no-op. Move's interface and wire schema remain unapproved.
    pub fn transfer() -> Result<(), CapError> {
        Err(CapError::UnsupportedCoreType(CoreType::KeyTable))
    }

    /// Select an entry by its incarnation in the invoked table. Kernel support
    /// remains excluded pending the complete Delete schema and rights matrix.
    pub fn delete(&mut self, key: RawKey) -> Result<(), CapError> {
        // TODO: Must invoke on self-captbl cap
        // Contract status update: authorized table management is not restricted
        // to the caller's own table; exact permission bits remain provisional.
        // SAFETY: Unsafe call.
        let result = unsafe {
            protected_call1(self.key.to_wire(), KeyTableOp::Delete as u64, key.to_wire())
        };
        decode_syscall_result(result).map(|_| ())
    }

    // Revoke all children of cap in slot
    /// Implementation status: invokes on `self`; the legacy `_captbl` argument is
    /// unused. Revocation semantics remain unresolved and nucleus rejects this operation.
    /// The selector now retains its full incarnation, without defining a new scope
    /// or revocation protocol.
    pub fn revoke(&self, _captbl: &KeyTableKey, key: RawKey) -> Result<(), CapError> {
        // SAFETY: Unsafe call.
        let result = unsafe {
            protected_call1(self.key.to_wire(), KeyTableOp::Revoke as u64, key.to_wire())
        };
        decode_syscall_result(result).map(|_| ())
    }

    // User code to copy cap to another domain (if you have their captbl cap):
    /// Implementation status: retains the provisional all-rights request; no
    /// authority amplification policy is implied before the rights matrix settles.
    ///
    /// Shares `copy_derive`'s result handling, including ignoring the second
    /// success word.
    pub fn grant_to(
        &self,
        my_key: RawKey,
        their_captbl: &KeyTableKey,
        their_slot: u32,
    ) -> Result<RawKey, CapError> {
        self.copy_derive(my_key, their_captbl, their_slot, Rights::all())
    }
}
