// =====================
// == Syscall handler ==
// =====================

#[inline]
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
        // (~4KB typically, includes nucleus stack + metadata)
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
