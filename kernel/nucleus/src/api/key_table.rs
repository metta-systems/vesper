use libsyscall::CapError;

use crate::{
    SyscallError,
    api::{protected_call1, protected_call4},
};

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

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
        KeyTableOp::Move => {
            // Copy cap from captbl[src] to dst_captbl[dst_slot]
            // Delete cap in captbl[src]
        }
        KeyTableOp::Delete => {
            // Delete cap in captbl[src]
        }
        KeyTableOp::Revoke => {
            // Revoke cap, by bumping it's epoch and making derived accesses invalid
        }
    }
}

////==========
////==========
////==========
////==========
////==========

// ═══════════════════════════════════════════════════════════════════
// KEY TABLE (CAPABILITY TABLE / CNODE)
// ═══════════════════════════════════════════════════════════════════

/// Slot index in a KeyTable
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct KeySlot(pub u16);

impl KeySlot {
    pub const NULL: KeySlot = KeySlot(0);
    pub const SELF_DOMAIN: KeySlot = KeySlot(1);
    pub const PARENT_DOMAIN: KeySlot = KeySlot(2);
    // ... other well-known slots
}

/// A capability table for a domain.
///
/// This is what seL4 calls a CNode. Each domain has one.
/// The table itself is a kernel object that can be referenced
/// by capabilities (for capability space manipulation).
pub struct KeyTable {
    /// The actual capability entries
    entries: [KeyEntry; Self::NUM_SLOTS],
    /// Domain that owns this table
    owner: DomainId,
    /// Number of valid entries (for iteration)
    count: u16,
}

impl KeyTable {
    /// Number of slots per table (power of 2 for fast indexing)
    pub const NUM_SLOTS: usize = 256;

    /// Create a new empty capability table
    pub fn new(owner: DomainId) -> Self {
        Self {
            entries: [const { KeyEntry::null() }; Self::NUM_SLOTS],
            owner,
            count: 0,
        }
    }

    /// Lookup a capability by slot index
    #[inline]
    pub fn lookup(&self, slot: KeySlot) -> Result<&KeyEntry, CapError> {
        let idx = slot.0 as usize;
        if idx >= Self::NUM_SLOTS {
            return Err(CapError::InvalidSlot(slot));
        }

        let entry = &self.entries[idx];
        if !entry.is_valid() {
            return Err(CapError::EmptySlot(slot));
        }

        Ok(entry)
    }

    /// Lookup a capability mutably
    #[inline]
    pub fn lookup_mut(&mut self, slot: KeySlot) -> Result<&mut KeyEntry, CapError> {
        let idx = slot.0 as usize;
        if idx >= Self::NUM_SLOTS {
            return Err(CapError::InvalidSlot(slot));
        }

        let entry = &mut self.entries[idx];
        if !entry.is_valid() {
            return Err(CapError::EmptySlot(slot));
        }

        Ok(entry)
    }

    /// Insert a capability at a specific slot
    pub fn insert(&mut self, slot: KeySlot, entry: KeyEntry) -> Result<(), CapError> {
        let idx = slot.0 as usize;
        if idx >= Self::NUM_SLOTS {
            return Err(CapError::InvalidSlot(slot));
        }

        if self.entries[idx].is_valid() {
            return Err(CapError::SlotOccupied(slot));
        }

        self.entries[idx] = entry;
        self.count += 1;
        Ok(())
    }

    /// Remove a capability from a slot
    pub fn remove(&mut self, slot: KeySlot) -> Result<KeyEntry, CapError> {
        let idx = slot.0 as usize;
        if idx >= Self::NUM_SLOTS {
            return Err(CapError::InvalidSlot(slot));
        }

        let entry = core::mem::replace(&mut self.entries[idx], KeyEntry::null());
        if entry.is_valid() {
            self.count -= 1;
            Ok(entry)
        } else {
            Err(CapError::EmptySlot(slot))
        }
    }
}

// KeyTable is itself a kernel object
impl NucleusObject for KeyTable {
    const TYPE: ObjectType = ObjectType::KeyTable;
}
