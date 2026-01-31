// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

#[repr(u8)]
pub enum DomainOp {
    Activate = 0, // Make domain runnable
    Grant = 1,    // Grant capability to domain
    Suspend = 2,  // Suspend domain
    Resume = 3,   // Resume suspended domain
}

// CSpace layout with self-reference
pub const CAPTBL_SELF: u32 = 0; // Every domain has cap to own captbl here

/// Domain capability - handle to a protection domain.
/// State queries use shared DCB (no syscall), mutations use CapInvoke.
pub struct DomainKey {
    cap: Key<Domain>,
    id: DomainId,
}

impl DomainKey {
    /// Create a new domain from untyped memory.
    /// Convenience wrapper around UntypedRetype.
    pub fn create(untyped: &mut UntypedCap, dest_slot: CapSlot) -> Result<Self, Error> {
        // Domains need ~4KB (12 bits) for kernel structures
        untyped_retype(
            untyped.split(12)?, // Carve off 4KB
            ObjectType::Domain,
            12,
            dest_slot,
        )?;

        // Domain ID is returned in secondary return value
        // (or we query it from the newly created DCB)
        Ok(DomainKey {
            cap: Cap::new(dest_slot),
            id: DomainId(/* ... */),
        })
    }

    /// Get domain state from shared DCB
    #[inline]
    pub fn state(&self) -> DomainState {
        let dcb = DCB.get(self.id);
        DomainState::try_from(dcb.state.load(Ordering::Acquire)).unwrap_or(DomainState::Inactive)
    }

    /// Get time used from shared DCB
    #[inline]
    pub fn time_used_ns(&self) -> u64 {
        let dcb = DCB.get(self.id);
        dcb.time_used_ns.load(Ordering::Relaxed)
    }

    /// Get pending notifications from shared DCB (NO SYSCALL!)
    #[inline]
    pub fn pending_notifications(&self) -> u64 {
        let dcb = DCB.get(self.id);
        dcb.pending_notifications.load(Ordering::Relaxed)
    }

    /// Activate domain (make runnable) - requires syscall
    pub fn activate(&self) -> Result<(), Error> {
        let ret = unsafe { syscall3(self.cap.slot as u64, DomainOp::Activate as u64, 0) };
        Error::from_code(ret)
    }

    /// Grant a capability to this domain - requires syscall
    pub fn grant<T>(&self, cap: &Cap<T>, dest_slot: CapSlot) -> Result<(), Error> {
        let ret = unsafe {
            syscall4(
                self.cap.slot as u64,
                DomainOp::Grant as u64,
                cap.slot() as u64,
                dest_slot as u64,
            )
        };
        Error::from_code(ret)
    }

    /// Suspend domain - requires syscall
    pub fn suspend(&self) -> Result<(), Error> {
        let ret = unsafe { syscall3(self.cap.slot as u64, DomainOp::Suspend as u64, 0) };
        Error::from_code(ret)
    }

    /// Resume suspended domain - requires syscall
    pub fn resume(&self) -> Result<(), Error> {
        let ret = unsafe { syscall3(self.cap.slot as u64, DomainOp::Resume as u64, 0) };
        Error::from_code(ret)
    }
}
