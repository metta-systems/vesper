use {
    crate::{
        api::key_entry::KeyEntry,
        objects::{NucleusObject, access::ObjectId, arch::ArchPools, nucleus::Nucleus},
    },
    libaddress::PhysAddr,
    libobject::{ArchType, CapError, ObjectType, Rights},
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
    pub const fn bits(self) -> u8 {
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

    pub const fn size(self) -> usize {
        1 << self.bits()
    }
}

/// Installation record of a page table (architecture-neutral).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtParent {
    /// Not installed anywhere.
    Uninstalled,
    /// Installed as the translation root of a Domain (checked identity).
    Root { domain: ObjectId },
    /// Installed in the parent table (physical address) at `slot`.
    Table { parent_paddr: u64, slot: u16 },
}

/// Kernel metadata operations for page-table objects, implemented per
/// architecture alongside `ArchObjects::PageTable`.
///
/// The metadata (carve address, walk level, installation record) is the
/// mapping identity for a translation table: enough to locate and retire the
/// real descriptor.
pub trait PageTableObject: NucleusObject {
    /// Physical address of the carved table.
    fn paddr(&self) -> u64;
    /// Walk level: 0 is the translation root, 3 the leaf-level table.
    fn level(&self) -> u8;
    /// Whether this table is currently installed.
    fn is_installed(&self) -> bool;
    /// The installation record.
    fn parent(&self) -> PtParent;
    /// Record root installation into `domain` at walk level 0.
    fn install_root(&mut self, domain: ObjectId);
    /// Record intermediate installation into the parent at `parent_level`.
    fn install_table(&mut self, parent_paddr: u64, parent_level: u8, slot: u16);
    /// Clear the installation record.
    fn uninstall(&mut self);
}

/// Architecture abstraction trait - extended with invoke methods.
///
/// Frame capabilities are arch-independent (inline `RegionPayload` in `KeyEntry`),
/// so there is no `type Frame` associated type. Frame size validation is
/// arch-specific via `validate_frame_size`.
pub trait ArchObjects: Sized + 'static {
    // ─── Associated Types (pool-backed arch objects only) ───
    type PageTable: PageTableObject;
    type VSpace: NucleusObject;
    type ASIDPool: NucleusObject;
    type ASID: NucleusObject;

    // ─── Constants ───
    const FRAME_SIZES: &'static [FrameSize];
    const PT_LEVELS: usize;
    const PT_INDEX_BITS: usize;

    // ─── Validation ───

    /// Validate frame `size_bits` for this architecture.
    /// Returns the frame size in bytes on success.
    fn validate_frame_size(size_bits: u8) -> Result<usize, CapError>;

    /// Validate and return object size for pool-backed arch types.
    fn validate_retype(arch_type: ArchType, size_bits: u8) -> Result<usize, CapError>;

    /// Construct the kernel metadata object for a freshly carved page table
    /// at physical address `paddr` (uninstalled).
    fn new_page_table(paddr: u64) -> Self::PageTable;

    // ─── Mapping mechanics (hardware descriptor installation) ───
    // The arch layer owns the descriptor format, walk, and vacancy checks;
    // the API handlers own authority and the transaction order.

    /// Install a table descriptor for `child_paddr` in the parent table at
    /// `parent_paddr` (walk level `parent_level`), at the slot selected by
    /// `vaddr`. The selected slot must be vacant. Returns the installed slot
    /// index, part of the installation record.
    fn install_table_entry(
        parent_paddr: u64,
        parent_level: u8,
        vaddr: u64,
        child_paddr: u64,
    ) -> Result<u16, CapError>;

    /// Clear the table descriptor at `slot` in the parent table, verifying it
    /// still points at `child_paddr` first.
    fn clear_table_entry(parent_paddr: u64, slot: u16, child_paddr: u64) -> Result<(), CapError>;

    /// Whether every descriptor in the table at `paddr` is zero.
    fn page_table_is_empty(paddr: u64) -> bool;

    /// Install the page/block descriptor for a frame mapping. `vaddr` must be
    /// inside the supported virtual-address width and aligned to the frame
    /// size; the leaf slot must be vacant. `writable` selects user
    /// read/write versus read-only; execute is not grantable yet.
    fn install_frame_pte(
        root_paddr: u64,
        vaddr: u64,
        frame_paddr: u64,
        size_bits: u8,
        writable: bool,
    ) -> Result<(), CapError>;

    /// Clear the frame mapping descriptor at `vaddr`, verifying it still
    /// points at `frame_paddr` first.
    fn clear_frame_pte(
        root_paddr: u64,
        vaddr: u64,
        frame_paddr: u64,
        size_bits: u8,
    ) -> Result<(), CapError>;

    /// Alias-policy check: find a live leaf descriptor in the walk from
    /// `root_paddr` whose physical extent overlaps
    /// `[paddr, paddr + (1 << size_bits))`, returning its physical base.
    /// The enforcement representation is physical overlap, not capability
    /// identity: any installed page/block descriptor covering any byte of
    /// the candidate extent conflicts, whatever capability installed it.
    fn find_physical_overlap(root_paddr: u64, paddr: u64, size_bits: u8) -> Option<u64>;

    // ─── Object Creation (pool-backed arch types only) ───
    /// Create a pool-backed arch object. Frame is NOT handled here —
    /// it is created inline via `KeyEntry::new_frame()` in the retype path.
    ///
    /// Returns the created object's type and its checked identity.
    /// Dereferencing the identity requires the owning access context
    /// (see `doc/lifetime-and-authority.md` §3).
    fn create_arch_object(
        arch_type: ArchType,
        phys_addr: PhysAddr,
        size_bits: u8,
        pools: &mut ArchPools<Self>,
    ) -> Result<(ObjectType, ObjectId), CapError>;

    // ─── Invocation Handlers ───
    // Frame and PageTable invocations are dispatched directly to their API
    // handlers (`crate::api::arch::{frame,page_table}`), which resolve the
    // invoked capability, the caller's table, and the operand pools through
    // the guarded `Access` context; no trait shim is needed. The remaining
    // handlers below serve the deferred kinds.

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
