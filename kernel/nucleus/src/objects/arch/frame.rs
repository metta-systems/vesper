// ─────────────────────────────────────────────────────────────────
// AArch64 Frame (Physical Page)
// ─────────────────────────────────────────────────────────────────

use {crate::api::key_table::KeySlot, libmemory::virt_addr::VirtAddr, libsyscall::CapError};

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
