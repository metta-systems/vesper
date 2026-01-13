use crate::{api::domain::CAPTBL_SELF, key::Key};

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

pub struct UntypedKey {
    key: Key<Untyped>,
}

enum RetypeError {}

impl UntypedKey {
    /// Retype untyped memory into a typed kernel object.
    ///
    /// This is how ALL kernel objects are created (seL4 pattern).
    /// The untyped capability is consumed/reduced by the operation.
    pub fn retype(
        &self,
        object_type: ObjectType,
        size_bits: u8, // log2 of size (for variable-size objects)
        dest_slot: CapSlot,
    ) -> Result<(), RetypeError> {
        let ret = unsafe {
            crate::syscall::protected_call3(
                self.key.slot as u64,
                UntypedOp::Retype,
                object_type as u64,
                dest_slot as u64,
                size_bits as u64,
            )
        };
        Error::from_code(ret)
    }
}

// ==============================================
// == Kernel space object and syscall handling ==
// ==============================================

struct Untyped;

#[repr(u8)]
enum UntypedOp {
    Retype = 0,
}

// =====================
// == Syscall handler ==
// =====================

pub fn invoke(cap: u32, op: u32, args: &[u64]) -> SyscallResult {
    // fn captbl_activate(captbl: u32, op: KeyTableOp, slot: u32) -> Result<()> {
    // CAPTBL_ACTIVATE
    let ct = lookup_captbl(CAPTBL_SELF)?;
    if ct.slots[slot].is_valid() {
        return Err(SyscallError::SlotOccupied);
    }
    // ... create object at slot via retype..
    // }
}

// ===========
// == Tests ==
// ===========

#[cfg(test)]
mod untyped_tests {
    use {super::*, crate::buffer::BufferCap};

    #[test]
    fn create_notification() {
        let mem = UntypedKey::new(0);
        // (~16 bytes)
        mem.retype(ObjectType::Notification, 4, slot_a)?;
        let notify = NotifyCap::from_slot(slot_a);
    }

    #[test]
    fn create_event_count() {
        // (~24 bytes)
        untyped_retype(mem, ObjectType::EventCount, 5, slot_b)?;
        let ec = EventCountCap::from_slot(slot_b);
    }

    #[test]
    fn create_domain() {
        // (~4KB typically, includes kernel stack + metadata)
        untyped_retype(mem, ObjectType::Domain, 12, slot_c)?;
        let domain = DomainCap::from_slot(slot_c);
    }

    #[test]
    fn create_buffer() {
        // (64KB buffer)
        untyped_retype(mem, ObjectType::Buffer, 16, slot_d)?;
        let buf = BufferCap::<ReadWrite>::from_slot(slot_d, 1 << 16);
    }
}
