// ─────────────────────────────────────────────────────────────────
// AArch64 Frame (Physical Page)
// ─────────────────────────────────────────────────────────────────

use libmemory::virt_addr::VirtAddr;
use libsyscall::CapError;

use crate::api::key_table::KeySlot;

/// A physical memory frame on AArch64.
///
/// Frames can be 4KB, 2MB, or 1GB and can be mapped into VSpaces.
#[derive(Debug)]
pub struct AArch64Frame {
    /// Physical address (aligned to frame size)
    phys_addr: PhysAddr,
    /// Frame size
    size: FrameSize,
    /// Is this device memory? (affects cacheability)
    is_device: bool,
    /// Mapping count (for shared frames)
    map_count: u16,
}

impl AArch64Frame {
    pub fn new(phys_addr: PhysAddr, size: FrameSize) -> Self {
        // Verify alignment
        debug_assert!(phys_addr.as_u64() & ((1 << size.bits()) - 1) == 0);

        Self {
            phys_addr,
            size,
            is_device: false,
            map_count: 0,
        }
    }

    pub fn phys_addr(&self) -> PhysAddr {
        self.phys_addr
    }

    pub fn size(&self) -> FrameSize {
        self.size
    }

    pub fn is_mapped(&self) -> bool {
        self.map_count > 0
    }
}

impl NucleusObject for AArch64Frame {
    const TYPE: ObjectType = ObjectType::Frame;
}

//===================
//===================
//===================
// TODO: move this from api/arch to objects/arch?
// TODO: keep only invoke() in api/
//===================
//===================

// pub trait FrameInvoke { fn invoke() }

#[repr(u8)]
pub enum FrameOp {
    /// Map frame into a VSpace at given virtual address
    Map = 0,
    /// Unmap frame from VSpace
    Unmap = 1,
    /// Get physical address (requires special rights)
    GetAddress = 2,
    /// Remap with different attributes
    Remap = 3,
}

pub fn invoke<A: ArchObjects>(
    frame: &mut A::Frame,
    rights: Rights,
    op: u32,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    let op = FrameOp::try_from(op as u8).map_err(|_| CapError::InvalidOperation)?;

    match op {
        FrameOp::Map => {
            // args[0] = vspace_slot
            // args[1] = virt_addr
            // args[2] = rights (R/W/X bits)
            // args[3] = attrs (cacheability, etc.)

            if !rights.contains(Rights::READ) {
                return Err(CapError::InsufficientRights);
            }

            let vspace_slot = KeySlot(args[0] as u16);
            let virt_addr = VirtAddr::new(args[1]);
            let map_rights = MapRights::from_bits(args[2] as u8);
            let attrs = MemAttrs::from_bits(args[3] as u8);

            // Get the VSpace from the slot
            let domain = kernel.current_domain()?;
            let vspace_entry = domain.keytable.lookup(vspace_slot)?;
            let vspace = vspace_entry.as_object::<AArch64VSpace>()?;

            // Perform the mapping
            // aarch64_map_frame(frame, vspace, virt_addr, map_rights, attrs, kernel)?;

            Ok((0, 0))
        }

        FrameOp::Unmap => {
            if frame.map_count == 0 {
                return Err(CapError::NotMapped);
            }
            // ... unmap logic
            todo!("frame unmap")
            Ok((0, 0))
        }

        FrameOp::GetAddress => {
            // Requires Grant right to expose physical address
            if !rights.contains(Rights::GRANT) {
                return Err(CapError::InsufficientRights);
            }
            Ok((frame.phys_addr.as_u64(), frame.size.size() as u64))
        }

        FrameOp::Remap => {
            // Change attributes on existing mapping
            todo!("frame remap")
        }
    }
}
