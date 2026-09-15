use {
    crate::{
        Nucleus,
        api::key_entry::KeyEntry,
        objects::{
            ArchObjects,
            access::ObjectId,
            arch::{AArch64ASID, AArch64ASIDPool, AArch64PageTable, AArch64VSpace, ArchPools},
            arch_objects::FrameSize,
        },
    },
    libaddress::PhysAddr,
    libobject::{ArchType, CapError, ObjectType, Rights},
};

// ═══════════════════════════════════════════════════════════════════
// AARCH64 IMPLEMENTATION
// ═══════════════════════════════════════════════════════════════════

pub struct AArch64;

impl ArchObjects for AArch64 {
    type PageTable = AArch64PageTable;
    type VSpace = AArch64VSpace;
    type ASIDPool = AArch64ASIDPool;
    type ASID = AArch64ASID;

    const FRAME_SIZES: &'static [FrameSize] =
        &[FrameSize::Small, FrameSize::Large, FrameSize::Huge];

    const PT_LEVELS: usize = 4;
    const PT_INDEX_BITS: usize = 9;

    fn validate_frame_size(size_bits: u8) -> Result<usize, CapError> {
        match size_bits {
            12 => Ok(4096),               // 4KB
            21 => Ok(2 * 1024 * 1024),    // 2MB
            30 => Ok(1024 * 1024 * 1024), // 1GB
            _ => Err(CapError::InvalidFrameSize(size_bits as usize)),
        }
    }

    fn validate_retype(arch_type: ArchType, size_bits: u8) -> Result<usize, CapError> {
        match arch_type {
            ArchType::PageTable => {
                if size_bits == 12 {
                    Ok(4096)
                } else {
                    Err(CapError::InvalidSize(size_bits as usize))
                }
            }
            ArchType::VSpace => Ok(core::mem::size_of::<AArch64VSpace>()),
            ArchType::ASIDPool => Ok(core::mem::size_of::<AArch64ASIDPool>()),
            ArchType::ASID => Ok(core::mem::size_of::<AArch64ASID>()),
            _ => Err(CapError::UnsupportedArchType(arch_type)),
        }
    }

    fn new_page_table(paddr: u64) -> AArch64PageTable {
        AArch64PageTable::new(paddr)
    }

    fn install_table_entry(
        parent_paddr: u64,
        parent_level: u8,
        vaddr: u64,
        child_paddr: u64,
    ) -> Result<u16, CapError> {
        super::page_table::install_table_entry(parent_paddr, parent_level, vaddr, child_paddr)
    }

    fn clear_table_entry(parent_paddr: u64, slot: u16, child_paddr: u64) -> Result<(), CapError> {
        super::page_table::clear_table_entry(parent_paddr, slot, child_paddr)
    }

    fn page_table_is_empty(paddr: u64) -> bool {
        super::page_table::table_is_empty(paddr)
    }

    fn install_frame_pte(
        root_paddr: u64,
        vaddr: u64,
        frame_paddr: u64,
        size_bits: u8,
        writable: bool,
    ) -> Result<(), CapError> {
        super::page_table::install_frame_pte(root_paddr, vaddr, frame_paddr, size_bits, writable)
    }

    fn clear_frame_pte(
        root_paddr: u64,
        vaddr: u64,
        frame_paddr: u64,
        size_bits: u8,
    ) -> Result<(), CapError> {
        super::page_table::clear_frame_pte(root_paddr, vaddr, frame_paddr, size_bits)
    }

    fn create_arch_object(
        arch_type: ArchType,
        phys_addr: PhysAddr,
        size_bits: u8,
        pools: &mut ArchPools<Self>,
    ) -> Result<(ObjectType, ObjectId), CapError> {
        // match arch_type {
        //     ArchType::Frame => {
        //         let frame_size = FrameSize::from_bits(size_bits as usize)
        //             .map_err(|_| CapError::InvalidSize(size_bits as usize))?;
        //         let frame = AArch64Frame::new(phys_addr, frame_size);
        //         let (id, _obj) = pools
        //             .frames
        //             .allocate(frame)
        //             .ok_or(CapError::PoolExhausted)?;
        //         Ok((ObjectType::FRAME, id))
        //     }
        //     ArchType::PageTable => {
        //         let pt = AArch64PageTable::new(phys_addr);
        //         let (id, _obj) = pools
        //             .page_tables
        //             .allocate(pt)
        //             .ok_or(CapError::PoolExhausted)?;
        //         Ok((ObjectType::PAGE_TABLE, id))
        //     }
        //     ArchType::VSpace => {
        //         let vspace = AArch64VSpace::new();
        //         let (id, _obj) = pools
        //             .vspaces
        //             .allocate(vspace)
        //             .ok_or(CapError::PoolExhausted)?;
        //         Ok((ObjectType::VSPACE, id))
        //     }
        //     ArchType::ASIDPool => {
        //         let pool = AArch64ASIDPool::new();
        //         let (id, _obj) = pools
        //             .asid_pools
        //             .allocate(pool)
        //             .ok_or(CapError::PoolExhausted)?;
        //         Ok((ObjectType::ASID_POOL, id))
        //     }
        //     _ => Err(CapError::UnsupportedArchType(arch_type)),
        // }
        Err(CapError::UnsupportedArchType(arch_type))
    }

    // ─────────────────────────────────────────────────────────────────
    // Frame and PageTable operations are dispatched directly to their API
    // handlers; see `crate::api::arch::{frame,page_table}`.
    // ─────────────────────────────────────────────────────────────────

    // ─────────────────────────────────────────────────────────────────
    // VSpace Operations
    // ─────────────────────────────────────────────────────────────────

    fn invoke_vspace(
        vspace: &mut AArch64VSpace,
        rights: Rights,
        op: u32,
        args: &[u64; 6],
        nucleus: &mut Nucleus<Self>,
    ) -> Result<(u64, u64), CapError> {
        // crate::api::arch::vspace::invoke(vspace, rights, op, args)
        Err(CapError::InvalidOperation)
    }

    // ─────────────────────────────────────────────────────────────────
    // ASID Pool Operations
    // ─────────────────────────────────────────────────────────────────

    fn invoke_asid_pool(
        pool: &mut AArch64ASIDPool,
        rights: Rights,
        op: u32,
        args: &[u64; 6],
        _nucleus: &mut Nucleus<Self>,
    ) -> Result<(u64, u64), CapError> {
        // Most ASID operations go through VSpace.AssignASID
        // Direct pool operations are rare
        Err(CapError::InvalidOperation)
    }

    fn invoke_asid(
        asid: &mut AArch64ASID,
        rights: Rights,
        op: u32,
        args: &[u64; 6],
    ) -> Result<(u64, u64), CapError> {
        // ASID capabilities are mostly just tokens
        // Operations would be for explicit invalidation
        Err(CapError::InvalidOperation)
    }
}
