// ─────────────────────────────────────────────────────────────────
// AArch64 Frame (Physical Page)
// ─────────────────────────────────────────────────────────────────

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

impl KernelObject for AArch64Frame {
    const TYPE: ObjectType = ObjectType::Frame;
}

//===================
//===================
//===================
//===================
//===================

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
            // args[2] = attrs (R/W/X)
            if !rights.contains(Rights::READ) {
                return Err(CapError::InsufficientRights);
            }
            // Implementation depends on A::Frame
            todo!("frame map")
        }
        FrameOp::Unmap => {
            todo!("frame unmap")
        }
        FrameOp::GetAddress => {
            if !rights.contains(Rights::GRANT) {
                return Err(CapError::InsufficientRights);
            }
            // Return physical address
            todo!("frame get_address")
        }
        FrameOp::Remap => {
            todo!("frame remap")
        }
    }
}
