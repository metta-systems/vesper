use {
    crate::{
        api::key_entry::KeyEntry,
        objects::{NucleusObject, arch::ArchPools, nucleus::Nucleus, object_ref::ObjectRef},
    },
    libaddress::PhysAddr,
    libobject::{ArchType, CapError, Rights},
};

// ═══════════════════════════════════════════════════════════════════
// ARCH OBJECTS TRAIT WITH INVOKE METHODS
// ═══════════════════════════════════════════════════════════════════

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

    pub fn from_bits(bits: usize) -> Result<FrameSize, ()> {
        match bits {
            12 => Ok(FrameSize::Small),
            21 => Ok(FrameSize::Large),
            30 => Ok(FrameSize::Huge),
            _ => Err(()),
        }
    }

    pub const fn size(&self) -> usize {
        1 << self.bits()
    }
}

/// Architecture abstraction trait - extended with invoke methods.
///
/// Frame capabilities are arch-independent (inline RegionPayload in KeyEntry),
/// so there is no `type Frame` associated type. Frame size validation is
/// arch-specific via `validate_frame_size`.
pub trait ArchObjects: Sized + 'static {
    // ─── Associated Types (pool-backed arch objects only) ───
    type PageTable: NucleusObject;
    type VSpace: NucleusObject;
    type ASIDPool: NucleusObject;
    type ASID: NucleusObject;

    // ─── Constants ───
    const FRAME_SIZES: &'static [FrameSize];
    const PT_LEVELS: usize;
    const PT_INDEX_BITS: usize;

    // ─── Validation ───

    /// Validate frame size_bits for this architecture.
    /// Returns the frame size in bytes on success.
    fn validate_frame_size(size_bits: u8) -> Result<usize, CapError>;

    /// Validate and return object size for pool-backed arch types.
    fn validate_retype(arch_type: ArchType, size_bits: u8) -> Result<usize, CapError>;

    // ─── Object Creation (pool-backed arch types only) ───
    /// Create a pool-backed arch object. Frame is NOT handled here —
    /// it is created inline via KeyEntry::new_frame() in the retype path.
    fn create_arch_object(
        arch_type: ArchType,
        phys_addr: PhysAddr,
        size_bits: u8,
        pools: &mut ArchPools<Self>,
    ) -> Result<ObjectRef, CapError>;

    // ─── Invocation Handlers ───

    /// Handle frame operations. The frame data is inline in `entry` as a RegionPayload.
    fn invoke_frame(
        entry: &mut KeyEntry,
        op: u32,
        args: &[u64; 6],
        nucleus: &mut Nucleus<Self>,
    ) -> Result<(u64, u64), CapError>;

    fn invoke_page_table(
        pt: &mut Self::PageTable,
        rights: Rights,
        op: u32,
        args: &[u64; 6],
        nucleus: &mut Nucleus<Self>,
    ) -> Result<(u64, u64), CapError>;

    fn invoke_vspace(
        vspace: &mut Self::VSpace,
        rights: Rights,
        op: u32,
        args: &[u64; 6],
        nucleus: &mut Nucleus<Self>,
    ) -> Result<(u64, u64), CapError>;

    fn invoke_asid_pool(
        pool: &mut Self::ASIDPool,
        rights: Rights,
        op: u32,
        args: &[u64; 6],
        nucleus: &mut Nucleus<Self>,
    ) -> Result<(u64, u64), CapError>;

    fn invoke_asid(
        asid: &mut Self::ASID,
        rights: Rights,
        op: u32,
        args: &[u64; 6],
    ) -> Result<(u64, u64), CapError>;

    // Optional - default implementations return UnsupportedArchType
    fn invoke_io_space(
        _entry: &mut KeyEntry,
        _op: u32,
        _args: &[u64; 6],
        _nucleus: &mut Nucleus<Self>,
    ) -> Result<(u64, u64), CapError> {
        Err(CapError::UnsupportedArchType(ArchType::IOSpace))
    }

    fn invoke_irq_handler(
        _entry: &mut KeyEntry,
        _op: u32,
        _args: &[u64; 6],
        _nucleus: &mut Nucleus<Self>,
    ) -> Result<(u64, u64), CapError> {
        Err(CapError::UnsupportedArchType(ArchType::IRQHandler))
    }

    fn invoke_irq_control(
        _entry: &mut KeyEntry,
        _op: u32,
        _args: &[u64; 6],
        _nucleus: &mut Nucleus<Self>,
    ) -> Result<(u64, u64), CapError> {
        Err(CapError::UnsupportedArchType(ArchType::IRQControl))
    }
}
