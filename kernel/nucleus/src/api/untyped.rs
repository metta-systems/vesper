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

// ═══════════════════════════════════════════════════════════════════
// EXTENDED RETYPE WITH ARCH OBJECTS
// ═══════════════════════════════════════════════════════════════════

impl Untyped {
    /// Retype this untyped memory into a kernel object.
    ///
    /// Handles both core and architecture-specific object types.
    pub fn retype<A: ArchObjects>(
        &mut self,
        obj_type: ObjectType,
        size_bits: u8,
        dest_slot: KeySlot,
        dest_keytable: &mut KeyTable,
        pools: &mut KernelPools<A>,
    ) -> Result<(), CapError> {
        // Determine object size based on type
        let obj_size = if obj_type.is_core() {
            core_object_size(obj_type, size_bits)?
        } else {
            A::validate_retype(obj_type, size_bits)?
        };

        // Check we have enough memory
        if self.watermark + obj_size > self.size {
            return Err(CapError::InsufficientMemory);
        }

        // Allocate from untyped
        let obj_addr = self.phys_addr.offset(self.watermark);
        self.watermark += obj_size;

        // Create the object based on type
        let entry = if obj_type.is_core() {
            self.create_core_object(obj_type, obj_addr, size_bits, pools)?
        } else {
            self.create_arch_object::<A>(obj_type, obj_addr, size_bits, pools)?
        };

        // Insert capability into destination slot
        dest_keytable.insert(dest_slot, entry)?;

        Ok(())
    }

    fn create_core_object<A: ArchObjects>(
        &mut self,
        obj_type: ObjectType,
        phys_addr: PhysAddr,
        size_bits: u8,
        pools: &mut KernelPools<A>,
    ) -> Result<KeyEntry, CapError> {
        match obj_type {
            ObjectType::Notification => {
                let notify = Notification::new();
                let obj = pools
                    .notifications
                    .allocate(notify)
                    .ok_or(CapError::PoolExhausted)?;
                Ok(KeyEntry::new(obj, Rights::all(), 0, None))
            }

            ObjectType::EventCount => {
                let ec = EventCount::new();
                let obj = pools
                    .event_counts
                    .allocate(ec)
                    .ok_or(CapError::PoolExhausted)?;
                Ok(KeyEntry::new(obj, Rights::all(), 0, None))
            }

            ObjectType::Domain => {
                let domain = Domain::new(phys_addr);
                let obj = pools
                    .domains
                    .allocate(domain)
                    .ok_or(CapError::PoolExhausted)?;
                Ok(KeyEntry::new(obj, Rights::all(), 0, None))
            }

            ObjectType::Time => {
                let time = TimeSlice::new_default();
                let obj = pools
                    .time_slices
                    .allocate(time)
                    .ok_or(CapError::PoolExhausted)?;
                Ok(KeyEntry::new(obj, Rights::all(), 0, None))
            }

            ObjectType::Endpoint => {
                let ep = Endpoint::new();
                let obj = pools
                    .endpoints
                    .allocate(ep)
                    .ok_or(CapError::PoolExhausted)?;
                Ok(KeyEntry::new(obj, Rights::all(), 0, None))
            }

            ObjectType::Buffer => {
                let size = 1usize << size_bits;
                let buffer = Buffer::new(phys_addr, size);
                let obj = pools
                    .buffers
                    .allocate(buffer)
                    .ok_or(CapError::PoolExhausted)?;
                Ok(KeyEntry::new(obj, Rights::READ | Rights::WRITE, 0, None))
            }

            ObjectType::KeyTable => {
                let kt = KeyTable::new_empty();
                let obj = pools
                    .keytables
                    .allocate(kt)
                    .ok_or(CapError::PoolExhausted)?;
                Ok(KeyEntry::new(obj, Rights::all(), 0, None))
            }

            ObjectType::Untyped => {
                // Split: create a child untyped
                let child_size = 1usize << size_bits;
                let child = Untyped {
                    phys_addr,
                    size: child_size,
                    watermark: 0,
                    is_device: self.is_device,
                };
                let obj = pools
                    .untypeds
                    .allocate(child)
                    .ok_or(CapError::PoolExhausted)?;
                Ok(KeyEntry::new(obj, Rights::all(), 0, None))
            }

            ObjectType::Reply => {
                let reply = Reply::new();
                let obj = pools
                    .replies
                    .allocate(reply)
                    .ok_or(CapError::PoolExhausted)?;
                // Reply caps have restricted rights
                Ok(KeyEntry::new(obj, Rights::WRITE, 0, None))
            }

            _ => Err(CapError::InvalidObjectType),
        }
    }

    fn create_arch_object<A: ArchObjects>(
        &mut self,
        obj_type: ObjectType,
        phys_addr: PhysAddr,
        size_bits: u8,
        pools: &mut KernelPools<A>,
    ) -> Result<KeyEntry, CapError> {
        let obj_ref = A::create_arch_object(obj_type, phys_addr, size_bits, &mut pools.arch)?;

        // Default rights based on type
        let rights = match obj_type {
            ObjectType::Frame => Rights::READ | Rights::WRITE,
            ObjectType::PageTable => Rights::all(),
            ObjectType::VSpace => Rights::all(),
            ObjectType::ASIDPool => Rights::all(),
            ObjectType::ASID => Rights::all(),
            _ => Rights::all(),
        };

        Ok(KeyEntry::from_ref(obj_ref, rights, 0, None))
    }
}

fn core_object_size(obj_type: ObjectType, size_bits: u8) -> Result<usize, CapError> {
    match obj_type {
        ObjectType::Notification => Ok(core::mem::size_of::<Notification>()),
        ObjectType::EventCount => Ok(core::mem::size_of::<EventCount>()),
        ObjectType::Time => Ok(core::mem::size_of::<TimeSlice>()),
        ObjectType::Endpoint => Ok(core::mem::size_of::<Endpoint>()),
        ObjectType::Reply => Ok(core::mem::size_of::<Reply>()),
        ObjectType::Domain => Ok(4096),
        ObjectType::KeyTable => Ok(core::mem::size_of::<KeyTable>()),
        ObjectType::Buffer | ObjectType::Untyped => {
            if size_bits > 30 {
                Err(CapError::InvalidSize)
            } else {
                Ok(1usize << size_bits)
            }
        }
        _ => Err(CapError::InvalidObjectType),
    }
}
