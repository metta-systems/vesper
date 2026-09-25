use {
    crate::{
        api::key_entry::KeyEntry,
        objects::{NucleusObject, access::ObjectId, arch::ArchPools, nucleus::Nucleus},
    },
    libaddress::PhysAddr,
    libobject::{ArchType, CapError, ObjectType},
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
    /// Installed as the translation root of an `AddressSpace` (checked
    /// identity).
    Root { address_space: ObjectId },
    /// Installed in the parent table (physical address) at `slot`.
    Table { parent_paddr: u64, slot: u16 },
}

/// Kernel metadata operations for page-table objects, implemented per
/// architecture alongside `ArchObjects::PageTable`.
///
/// The metadata (carve address, walk level, installation record) is the
/// mapping identity for a translation table: enough to locate and retire the
/// real descriptor.
/// Behavior of an architecture's ASID-pool object: allocation from the
/// hardware ASID namespace (binding is the API handler's commit step).
pub trait AsidPoolObject: NucleusObject {
    /// Allocate the lowest free ASID, or `None` when the pool is exhausted.
    /// ASID 0 stays reserved for the kernel's boot context.
    fn allocate(&mut self) -> Option<u16>;

    /// Release `asid` back to this pool (`AddressSpace.Retire`). The caller
    /// performs the whole-ASID TLB invalidation before releasing; ASID 0 is
    /// never released (it is the kernel's own reserved boot context).
    fn release(&mut self, asid: u16);
}

pub trait PageTableObject: NucleusObject {
    /// Physical address of the carved table.
    fn paddr(&self) -> u64;
    /// Walk level: 0 is the translation root, 3 the leaf-level table.
    fn level(&self) -> u8;
    /// Whether this table is currently installed.
    fn is_installed(&self) -> bool;
    /// The installation record.
    fn parent(&self) -> PtParent;
    /// Record root installation into `address_space` at walk level 0.
    fn install_root(&mut self, address_space: ObjectId);
    /// Record intermediate installation into the parent at `parent_level`.
    fn install_table(&mut self, parent_paddr: u64, parent_level: u8, slot: u16);
    /// Clear the installation record.
    fn uninstall(&mut self);
}

/// Behavior of an architecture's address-space object: the translation
/// context state (mapping foundation).
pub trait AddressSpaceObject: NucleusObject {
    /// Physical address of the installed translation root, if any.
    fn translation_root(&self) -> Option<u64>;
    /// Record or clear the translation root (root installation/withdrawal).
    fn set_translation_root(&mut self, root: Option<u64>);
    /// The bound hardware ASID, if any.
    fn asid(&self) -> Option<u16>;
    /// Record or clear the bound ASID (`ASIDPool.Assign` /
    /// `AddressSpace.Retire`).
    fn set_asid(&mut self, asid: Option<u16>);
}

/// Architecture abstraction trait - extended with invoke methods.
///
/// Frame capabilities are arch-independent (inline `RegionPayload` in `KeyEntry`),
/// so there is no `type Frame` associated type. Frame size validation is
/// arch-specific via `validate_frame_size`.
pub trait ArchObjects: Sized + 'static {
    // ─── Associated Types (pool-backed arch objects only) ───
    type PageTable: PageTableObject;
    type AddressSpace: AddressSpaceObject;
    type ASIDPool: AsidPoolObject;
    type ASIDControl: NucleusObject;

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

    /// Construct a fresh ASID pool (ASID 0 reserved for the kernel's boot
    /// context). Boot-provisioned only: ASID pools are not Retype-creatable.
    fn new_asid_pool() -> Self::ASIDPool;

    /// Construct a fresh, unbound `AddressSpace` (no translation root, no
    /// ASID). Boot-provisioned only: address spaces are not
    /// Retype-creatable yet.
    fn new_address_space() -> Self::AddressSpace;

    // ─── TLB maintenance (hardware translation withdrawal) ───

    /// Invalidate cached translations for `vaddr` under `asid` (inner
    /// shareable), completing before the caller proceeds. Called after the
    /// leaf descriptor is cleared, so a stale translation cannot survive the
    /// unmap that withdrew it.
    fn invalidate_tlb_by_vaddr(asid: u16, vaddr: u64);

    /// Invalidate every cached translation for `asid` (inner shareable),
    /// completing before the caller proceeds. Called when an `AddressSpace`'s
    /// translation root is withdrawn.
    fn invalidate_tlb_asid(asid: u16);

    // ─── Translation-context installation (hardware activation) ───

    /// Install `root_paddr` with `asid` as the current hardware
    /// translation context (`TTBR0_EL1` on `AArch64`: base address with the
    /// ASID in bits 63:48), completing before the caller proceeds. The
    /// caller (the API handler) has already established the mapping-context
    /// authority and that the root and ASID are bound to the same
    /// `AddressSpace`; this is the hardware mechanism only, not an authority
    /// decision. Re-installing the same context is idempotent; switching
    /// between contexts that share an ASID requires invalidation by the
    /// caller (ASID reuse safety remains open, D6).
    fn install_translation_context(root_paddr: u64, asid: u16);

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
    /// size; the leaf slot must be vacant. `writable` selects read/write
    /// versus read-only; `executable` clears the execute-never bits (the
    /// `EXECUTE` right, selected 2026-09-15 — without it every mapping stays
    /// UXN|PXN). A writable executable mapping is kernel-privilege (EL0
    /// denied): EL1 cannot execute EL0-writable pages.
    fn install_frame_pte(
        root_paddr: u64,
        vaddr: u64,
        frame_paddr: u64,
        size_bits: u8,
        writable: bool,
        executable: bool,
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
    // Frame, PageTable, AddressSpace, and ASIDPool invocations are dispatched
    // directly to their API handlers
    // (`crate::api::arch::{frame,page_table,address_space,asid_pool}`),
    // which resolve the invoked capability, the caller's table, and the
    // operand pools through the guarded `Access` context; no trait shim is
    // needed. The remaining handlers below serve the deferred kinds.

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
