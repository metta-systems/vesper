// ====================
// == Nucleus object ==
// ====================

/// Buffer kernel object - represents a contiguous memory region
pub struct Buffer {
    phys_base: PhysAddr, // Physical address (fixed at creation)
    size: usize,         // Size in bytes (fixed at creation)
    flags: BufferFlags,  // CACHED, DEVICE, SHARED, etc.

                         // Mapping tracking (for single-address-space)
                         // mappings: SmallVec<Mapping, 4>, // Who has it mapped where
}

struct Mapping {
    domain_id: DomainId,
    virt_addr: VirtAddr,
    permissions: Rights, // May be less than cap rights (derived cap)
}

bitflags::bitflags! {
    pub struct BufferFlags: u32 {
        const CACHED   = 1 << 0;  // Normal cacheable memory
        const DEVICE   = 1 << 1;  // Device memory (uncached, no speculative)
        const SHARED   = 1 << 2;  // Multi-domain sharing expected
        const DMA      = 1 << 3;  // DMA-capable (physically contiguous)
        const EXEC     = 1 << 4;  // Executable (if supported)
    }
}

impl NucleusObject for Buffer {
    const TYPE: ObjectType = ObjectType::Buffer;
}
