use crate::{api::domain::CAPTBL_SELF, key::Key};

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

pub struct UntypedKey {
    key: Key<Untyped>,
}

// Errors that can occur during retype operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum RetypeError {
    /// Invalid object type specified
    InvalidObjectType = 1,
    /// Requested size is invalid for object type
    InvalidSize = 2,
    /// Not enough memory remaining in untyped
    InsufficientMemory = 3,
    /// Alignment requirements not met
    AlignmentError = 4,
    /// Destination slot already occupied
    SlotOccupied = 5,
    /// Invalid destination slot
    InvalidSlot = 6,
    /// Untyped has already been retyped (has children)
    AlreadyRetyped = 7,
    /// Object type requires specific size_bits
    SizeMismatch = 8,
    /// Maximum number of objects reached
    ObjectLimitReached = 9,
    /// Internal kernel error
    InternalError = 10,
}

impl RetypeError {
    pub fn code(self) -> u32 {
        self as u32
    }
}

// ┌─────────────────────────────────────────────────────────────────┐
// │  ALLOWED OPERATIONS ON UNTYPED                                  │
// ├─────────────────────────────────────────────────────────────────┤
// │  ✓ seL4_Untyped_Retype  → Create children (objects/sub-untypeds)│
// │  ✓ seL4_CNode_Revoke    → Delete all children, reset watermark  │
// │  ✓ seL4_CNode_Delete    → Delete this cap (if no children)      │
// │  ✓ seL4_CNode_Move      → Move cap to different slot            │
// ├─────────────────────────────────────────────────────────────────┤
// │  DISALLOWED                                                     │
// ├─────────────────────────────────────────────────────────────────┤
// │  ✗ seL4_CNode_Copy      → Cannot duplicate                      │
// │  ✗ seL4_CNode_Mint      → Cannot derive with reduced rights     │
// │  ✗ seL4_CNode_Mutate    → Cannot modify                         │
// └─────────────────────────────────────────────────────────────────┘

impl UntypedKey {
    /// Retype untyped memory into a typed nucleus object.
    ///
    /// This is how ALL nucleus objects are created (seL4 pattern).
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

// ===============================================
// == Nucleus space object and syscall handling ==
// ===============================================

// ┌─────────────────────────────────────────────────────────────────────┐
// │                    cap_untyped_cap  (64-bit systems)                │
// ├──────────────────┬──────────────────────────────────────────────────┤
// │  capType (4 bits)│ = cap_untyped_cap                                │
// ├──────────────────┼──────────────────────────────────────────────────┤
// │  capPtr          │ Physical address of untyped region               │
// ├──────────────────┼──────────────────────────────────────────────────┤
// │  capBlockSize    │ Total size in bits (region = 2^capBlockSize)     │
// ├──────────────────┼──────────────────────────────────────────────────┤
// │  capFreeIndex    │ Watermark: next free byte offset (>> MIN_BITS)   │
// ├──────────────────┼──────────────────────────────────────────────────┤
// │  capIsDevice     │ Boolean: is this device memory (not normal RAM)? │
// └──────────────────┴──────────────────────────────────────────────────┘

// The key insight: an Untyped capability IS the kernel object.
// There's no separate in-kernel structure — just the capability itself carrying all the metadata.
// This is extremely minimal!

/// State of an untyped memory region - FIXME prolly not needed
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UntypedState {
    /// Fresh, no children created
    Free = 0,
    /// Has been split into children
    HasChildren = 1,
    /// Has been retyped to a typed object
    Retyped = 2,
}

/// Represents a region of physical memory that can be retyped
#[repr(C, align(64))]
pub struct UntypedObject {
    /// Physical base address of the memory region
    pub paddr: PhysAddr,
    /// Size as log2 (actual size = 1 << size_bits)
    pub size_bits: u8,
    /// Current state
    pub state: UntypedState,
    /// Number of child untypeds (if split)
    pub child_count: u16,
    /// Watermark: offset of next free byte within region
    /// Only valid when state == Free
    pub watermark: u64,
}

const _: () = assert!(size_of::<UntypedObject>() <= 64);

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

// ═══════════════════════════════════════════════════════════════════
// EXTENDED RETYPE WITH ARCH OBJECTS
// ═══════════════════════════════════════════════════════════════════

//                    UNTYPED REGION (2^capBlockSize bytes)
//      ┌────────────────────────────────────────────────────────────┐
//      │█████████████████████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░│
//      │      ALLOCATED          │           FREE                   │
//      └────────────────────────────────────────────────────────────┘
//      ↑                         ↑                                  ↑
//    capPtr          capPtr + FREE_INDEX_TO_OFFSET(capFreeIndex)  capPtr + 2^capBlockSize
//                           (watermark)

// VALIDATION:
//     Type valid? Size within bounds?
//     Device untyped → only Frames/Untypeds allowed
//     num ≤ CONFIG_RETYPE_FAN_OUT_LIMIT (default 256)?
//     Destination slots empty?
//
// CALCULATE ALIGNED FREE POINTER
//
//     freeRef = GET_FREE_REF(capPtr, freeIndex)
//     objectSize = 1 << size_bits
//     alignedFreeRef = ROUND_UP(freeRef, objectSize)    ← CRITICAL!
//
//     (alignment can waste memory — the "fragmentation" issue)
//
// CHECK SUFFICIENT SPACE
//
//     untypedFreeBytes = regionEnd - alignedFreeRef
//     if (untypedFreeBytes >> objectSizeBits) < num:
//         return seL4_NotEnoughMemory  (with memoryLeft in MR)
//
// RESET CHECK (if children were revoked)
//
//     if (cap_untyped_cap_get_capFreeIndex(cap) != 0
//         && children_revoked):
//         • Zero memory (preemptible!)
//         • Reset capFreeIndex to 0
//
// CREATE OBJECTS
//
//     for i in 0..num:
//         objectAddr = alignedFreeRef + (i * objectSize)
//         cap = createObject(type, objectAddr, size_bits, ...)
//         insert_cap_into_cnode(destSlots[i], cap)
//         establish_CDT_relationship(untypedCap → newCap)
//
// UPDATE WATERMARK
//
//     newFreeIndex = OFFSET_TO_FREE_INDEX(
//         alignedFreeRef + num*objectSize - capPtr
//     )
//     cap_untyped_cap_set_capFreeIndex(cap, newFreeIndex)

impl Untyped {
    /// Retype this untyped memory into a nucleus object.
    ///
    /// Handles both core and architecture-specific object types.
    pub fn retype<A: ArchObjects>(
        &mut self,
        obj_type: ObjectType,
        size_bits: u8,
        dest_slot: KeySlot,
        dest_keytable: &mut KeyTable,
        pools: &mut NucleusPools<A>,
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
        pools: &mut NucleusPools<A>,
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
        pools: &mut NucleusPools<A>,
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

impl NucleusObject for Untyped {
    const TYPE: ObjectType = ObjectType::Untyped;
}
