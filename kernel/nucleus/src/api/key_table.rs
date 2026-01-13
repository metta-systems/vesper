// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

use crate::{
    SyscallError,
    api::{protected_call1, protected_call4},
};

pub struct KeyTableKey {
    key: Key<KeyTable>,
}

// Userspace KeyManager must track parent→child relationships,
// kernel only manages flat key tables.

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
    ) -> Result<()> {
        protected_call4(
            self.key.slot(),
            KeyTableOp::CopyDerive,
            src_slot,
            dst_captbl.slot(),
            dst_slot,
            rights.bits(),
        )
    }

    // fn activate(&self, slot: u32, object: KernelObject) -> Result<()> {
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

    fn r#move() {}

    fn delete(&mut self, slot: u32) -> Result<()> {
        // TODO: Must invoke on self-captbl cap
        protected_call1(self.key.slot(), KeyTableOp::Delete, slot)
    }

    // Revoke all children of cap in slot
    fn revoke(&self, captbl: &CaptblCap, slot: u32) -> Result<()> {
        protected_call1(self.key.slot(), KeyTableOp::Revoke, slot)
    }

    // User code to copy cap to another domain (if you have their captbl cap):
    fn grant_to(my_slot: u32, their_captbl: &CaptblCap, their_slot: u32) -> Result<()> {
        protected_call3(
            self.key.slot(),
            KeyTableOp::CopyDerive,
            my_slot,
            their_captbl.slot(),
            their_slot,
            same_rights,
        )
    }
}

// ==============================================
// == Kernel space object and syscall handling ==
// ==============================================

#[repr(u8)]
enum KeyTableOp {
    CopyDerive = 0, // copy cap between slots or  create derived cap with reduced rights
    Move = 1,       // move cap between slots
    Delete = 2,     // delete cap at slot
    Revoke = 4,     // revoke all children of cap
}

impl KeyTableOp {
    fn try_from(op: u32) -> Result<KeyTableOp, SyscallResult> {
        match op {
            0 => Ok(KeyTableOp::CopyDerive),
            1 => Ok(KeyTableOp::Move),
            2 => Ok(KeyTableOp::Delete),
            3 => Ok(KeyTableOp::Revoke),
            _ => Err(SyscallError::InvalidOp),
        }
    }
}

struct KeyTable {
    slots: [u64; 256],
    len: usize,
}

impl KeyTable {
    fn delete(&self, slot: u32) -> Result<()> {
        if slot >= self.len() {
            return Err(Error::SlotOutOfRange);
        }

        if !self.slots[slot].is_valid() {
            return Err(Error::SlotEmpty);
        }

        let cap = self.slots[slot].take();
        cap.destroy()?;

        Ok(())
    }
}

// =====================
// == Syscall handler ==
// =====================

pub fn invoke(key: &Key, op: u32, args: &[u64]) -> SyscallResult {
    let captbl = key.as_keytable()?;
    match KeyTableOp::try_from(op)? {
        KeyTableOp::CopyDerive => {
            let (src, dst_captbl, dst_slot) = (args[0], args[1], args[2]);
            // Copy cap from this captbl[src] to dst_captbl[dst_slot]
            //...
        }
        KeyTableOp::Move => {}
        KeyTableOp::Delete => {}
        KeyTableOp::Revoke => {}
    }
}
