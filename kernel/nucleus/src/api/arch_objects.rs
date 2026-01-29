// ═══════════════════════════════════════════════════════════════════
// ARCHITECTURE ABSTRACTION TRAIT
// ═══════════════════════════════════════════════════════════════════

/// Trait defining architecture-specific kernel object types and operations.
///
/// Each architecture implements this trait to provide:
/// - Concrete types for frames, page tables, etc.
/// - Size/alignment requirements
/// - Retype validation
pub trait ArchObjects: Sized + 'static {
    /// Physical memory frame type
    type Frame: KernelObject;
    /// Page table type (single level)
    type PageTable: KernelObject;
    /// Virtual address space root
    type VSpace: KernelObject;
    /// ASID pool type
    type ASIDPool: KernelObject;
    /// ASID type
    type ASID: KernelObject;

    /// Supported frame sizes for this architecture
    const FRAME_SIZES: &'static [FrameSize];

    /// Number of page table levels
    const PT_LEVELS: usize;

    /// Bits per page table level
    const PT_INDEX_BITS: usize;

    /// Validate that an object type can be created with given size_bits
    fn validate_retype(obj_type: ObjectType, size_bits: u8) -> Result<usize, CapError>;

    /// Create an architecture-specific object
    fn create_arch_object(
        obj_type: ObjectType,
        phys_addr: PhysAddr,
        size_bits: u8,
        pools: &mut ArchPools<Self>,
    ) -> Result<ObjectRef, CapError>;
}

/// Frame size enumeration (common across architectures)
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FrameSize {
    /// 4KB (standard page)
    Small, // 12 bits
    /// 2MB (large page / section)
    Large, // 21 bits
    /// 1GB (huge page / supersection)
    Huge, // 30 bits
}

impl FrameSize {
    pub const fn bits(&self) -> u8 {
        match self {
            FrameSize::Small => 12,
            FrameSize::Large => 21,
            FrameSize::Huge => 30,
        }
    }

    pub const fn size(&self) -> usize {
        1 << self.bits()
    }
}
