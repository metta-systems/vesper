use crate::{CapError, Key, Rights, decode_syscall_result};

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

#[repr(u8)]
pub enum KeyTableOp {
    CopyDerive = 0, // copy cap between slots or create derived cap with reduced rights
    Move = 1,       // move cap between slots
    Delete = 2,     // delete cap at slot
    Revoke = 4,     // revoke all children of cap
}

// Userspace KeyMaster must track parent→child relationships,
// kernel only manages flat key tables.

// Contract status: the KeyMaster/kernel derivation and revocation trust boundary
// remains an open decision (D2).

impl KeyTableKey {
    // This naturally supports cross-domain derivation:
    // "Create a read-only view of my buffer in their cspace"
    // derive(&my_captbl, buffer_slot, &their_captbl, their_slot, Rights::READ)?;
    /// Copy with derivation in single syscall
    pub fn copy_derive(
        &self,
        src_slot: u32,
        dst_captbl: &KeyTableKey, // Could be same or different!
        dst_slot: u32,
        rights: Rights,
    ) -> Result<(), CapError> {
        // SAFETY: Unsafe call.
        let result = unsafe {
            protected_call4(
                self.key.slot(),
                KeyTableOp::CopyDerive as u32,
                u64::from(src_slot),
                u64::from(dst_captbl.key.slot()),
                u64::from(dst_slot),
                u64::from(rights.bits()),
            )
        };
        decode_syscall_result(result).map(|_| ())
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
    pub fn transfer() {}

    pub fn delete(&mut self, slot: u32) -> Result<(), CapError> {
        // TODO: Must invoke on self-captbl cap
        // SAFETY: Unsafe call.
        let result =
            unsafe { protected_call1(self.key.slot(), KeyTableOp::Delete as u32, u64::from(slot)) };
        decode_syscall_result(result).map(|_| ())
    }

    // Revoke all children of cap in slot
    /// Implementation status: invokes on `self`; the legacy `_captbl` argument is
    /// unused. Revocation semantics remain unresolved and nucleus rejects this operation.
    pub fn revoke(&self, _captbl: &KeyTableKey, slot: u32) -> Result<(), CapError> {
        // SAFETY: Unsafe call.
        let result =
            unsafe { protected_call1(self.key.slot(), KeyTableOp::Revoke as u32, u64::from(slot)) };
        decode_syscall_result(result).map(|_| ())
    }

    // User code to copy cap to another domain (if you have their captbl cap):
    pub fn grant_to(
        &self,
        my_slot: u32,
        their_captbl: &KeyTableKey,
        their_slot: u32,
    ) -> Result<(), CapError> {
        self.copy_derive(my_slot, their_captbl, their_slot, Rights::all())
    }
}
