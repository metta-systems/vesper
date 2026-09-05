use {
    crate::{
        Nucleus,
        api::key_entry::KeyEntry,
        objects::{
            ArchObjects,
            arch::{AArch64ASID, AArch64ASIDPool, AArch64PageTable, AArch64VSpace, ArchPools},
            arch_objects::FrameSize,
            object_ref::ObjectRef,
        },
    },
    libaddress::PhysAddr,
    libobject::{ArchType, CapError, Rights},
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

    fn create_arch_object(
        arch_type: ArchType,
        phys_addr: PhysAddr,
        size_bits: u8,
        pools: &mut ArchPools<Self>,
    ) -> Result<ObjectRef, CapError> {
        // match arch_type {
        //     ArchType::Frame => {
        //         let frame_size = FrameSize::from_bits(size_bits as usize)
        //             .map_err(|_| CapError::InvalidSize(size_bits as usize))?;
        //         let frame = AArch64Frame::new(phys_addr, frame_size);
        //         let obj = pools
        //             .frames
        //             .allocate(frame)
        //             .ok_or(CapError::PoolExhausted)?;
        //         Ok(ObjectRef::new(obj))
        //     }
        //     ArchType::PageTable => {
        //         let pt = AArch64PageTable::new(phys_addr);
        //         let obj = pools
        //             .page_tables
        //             .allocate(pt)
        //             .ok_or(CapError::PoolExhausted)?;
        //         Ok(ObjectRef::new(obj))
        //     }
        //     ArchType::VSpace => {
        //         let vspace = AArch64VSpace::new();
        //         let obj = pools
        //             .vspaces
        //             .allocate(vspace)
        //             .ok_or(CapError::PoolExhausted)?;
        //         Ok(ObjectRef::new(obj))
        //     }
        //     ArchType::ASIDPool => {
        //         let pool = AArch64ASIDPool::new();
        //         let obj = pools
        //             .asid_pools
        //             .allocate(pool)
        //             .ok_or(CapError::PoolExhausted)?;
        //         Ok(ObjectRef::new(obj))
        //     }
        //     _ => Err(CapError::UnsupportedArchType(arch_type)),
        // }
        Err(CapError::UnsupportedArchType(arch_type))
    }

    // ─────────────────────────────────────────────────────────────────
    // Frame Operations
    // ─────────────────────────────────────────────────────────────────

    fn invoke_frame(
        entry: &mut KeyEntry,
        op: u32,
        args: &[u64; 6],
        nucleus: &mut Nucleus<Self>,
    ) -> Result<(u64, u64), CapError> {
        // crate::api::arch::frame::invoke(entry, op, args, nucleus)
        Err(CapError::InvalidOperation)
    }

    // ─────────────────────────────────────────────────────────────────
    // Page Table Operations
    // ─────────────────────────────────────────────────────────────────

    fn invoke_page_table(
        pt: &mut AArch64PageTable,
        rights: Rights,
        op: u32,
        args: &[u64; 6],
        nucleus: &mut Nucleus<Self>,
    ) -> Result<(u64, u64), CapError> {
        // crate::api::arch::page_table::invoke(pt, rights, op, args)
        Err(CapError::InvalidOperation)
    }

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
