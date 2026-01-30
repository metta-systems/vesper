use {
    crate::api::{
        arch::frame::AArch64Frame,
        object_type::{ArchType, ObjectType},
    },
    libmemory::{phys_addr::PhysAddr, virt_addr::VirtAddr},
};

// ═══════════════════════════════════════════════════════════════════
// AARCH64 IMPLEMENTATION
// ═══════════════════════════════════════════════════════════════════

pub struct AArch64;

impl ArchObjects for AArch64 {
    type Frame = AArch64Frame;
    type PageTable = AArch64PageTable;
    type VSpace = AArch64VSpace;
    type ASIDPool = AArch64ASIDPool;
    type ASID = AArch64ASID;

    const FRAME_SIZES: &'static [FrameSize] =
        &[FrameSize::Small, FrameSize::Large, FrameSize::Huge];

    const PT_LEVELS: usize = 4;
    const PT_INDEX_BITS: usize = 9;

    fn validate_retype(arch_type: ArchType, size_bits: u8) -> Result<usize, CapError> {
        match arch_type {
            ArchType::Frame => match size_bits {
                12 => Ok(4096),
                21 => Ok(2 * 1024 * 1024),
                30 => Ok(1024 * 1024 * 1024),
                _ => Err(CapError::InvalidFrameSize(size_bits)),
            },
            ArchType::PageTable => {
                if size_bits != 12 {
                    Err(CapError::InvalidSize)
                } else {
                    Ok(4096)
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
        match arch_type {
            ArchType::Frame => {
                let frame_size = FrameSize::from_bits(size_bits)?;
                let frame = AArch64Frame::new(phys_addr, frame_size);
                let obj = pools
                    .frames
                    .allocate(frame)
                    .ok_or(CapError::PoolExhausted)?;
                Ok(ObjectRef::new(obj))
            }
            ArchType::PageTable => {
                let pt = AArch64PageTable::new(phys_addr);
                let obj = pools
                    .page_tables
                    .allocate(pt)
                    .ok_or(CapError::PoolExhausted)?;
                Ok(ObjectRef::new(obj))
            }
            ArchType::VSpace => {
                let vspace = AArch64VSpace::new();
                let obj = pools
                    .vspaces
                    .allocate(vspace)
                    .ok_or(CapError::PoolExhausted)?;
                Ok(ObjectRef::new(obj))
            }
            ArchType::ASIDPool => {
                let pool = AArch64ASIDPool::new();
                let obj = pools
                    .asid_pools
                    .allocate(pool)
                    .ok_or(CapError::PoolExhausted)?;
                Ok(ObjectRef::new(obj))
            }
            _ => Err(CapError::UnsupportedArchType(arch_type)),
        }
    }

    // ─────────────────────────────────────────────────────────────────
    // Frame Operations
    // ─────────────────────────────────────────────────────────────────

    fn invoke_frame(
        frame: &mut AArch64Frame,
        rights: Rights,
        op: u32,
        args: &[u64; 6],
        kernel: &mut Kernel<Self>,
    ) -> Result<(u64, u64), CapError> {
        #[repr(u8)]
        enum FrameOp {
            Map = 0,
            Unmap = 1,
            GetAddress = 2,
            Remap = 3,
        }

        let op = match op {
            0 => FrameOp::Map,
            1 => FrameOp::Unmap,
            2 => FrameOp::GetAddress,
            3 => FrameOp::Remap,
            _ => return Err(CapError::InvalidOperation),
        };

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
                aarch64_map_frame(frame, vspace, virt_addr, map_rights, attrs, kernel)?;

                Ok((0, 0))
            }

            FrameOp::Unmap => {
                if frame.map_count == 0 {
                    return Err(CapError::NotMapped);
                }
                // ... unmap logic
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

    // ─────────────────────────────────────────────────────────────────
    // Page Table Operations
    // ─────────────────────────────────────────────────────────────────

    fn invoke_page_table(
        pt: &mut AArch64PageTable,
        rights: Rights,
        op: u32,
        args: &[u64; 6],
        kernel: &mut Kernel<Self>,
    ) -> Result<(u64, u64), CapError> {
        #[repr(u8)]
        enum PageTableOp {
            Map = 0,   // Map this PT into a parent PT or VSpace
            Unmap = 1, // Unmap from parent
        }

        let op = match op {
            0 => PageTableOp::Map,
            1 => PageTableOp::Unmap,
            _ => return Err(CapError::InvalidOperation),
        };

        match op {
            PageTableOp::Map => {
                // args[0] = vspace_slot
                // args[1] = virt_addr (determines which slot in parent)
                let vspace_slot = KeySlot(args[0] as u16);
                let virt_addr = VirtAddr::new(args[1]);

                // ... mapping logic
                todo!("page_table map")
            }

            PageTableOp::Unmap => {
                todo!("page_table unmap")
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────
    // VSpace Operations
    // ─────────────────────────────────────────────────────────────────

    fn invoke_vspace(
        vspace: &mut AArch64VSpace,
        rights: Rights,
        op: u32,
        args: &[u64; 6],
        kernel: &mut Kernel<Self>,
    ) -> Result<(u64, u64), CapError> {
        #[repr(u8)]
        enum VSpaceOp {
            AssignASID = 0,
            GetASID = 1,
        }

        let op = match op {
            0 => VSpaceOp::AssignASID,
            1 => VSpaceOp::GetASID,
            _ => return Err(CapError::InvalidOperation),
        };

        match op {
            VSpaceOp::AssignASID => {
                // args[0] = asid_pool_slot
                let pool_slot = KeySlot(args[0] as u16);

                let domain = kernel.current_domain_mut()?;
                let pool_entry = domain.keytable.lookup_mut(pool_slot)?;
                let pool = pool_entry.as_object_mut::<AArch64ASIDPool>()?;

                let asid = pool.allocate().ok_or(CapError::ASIDPoolExhausted)?;

                vspace.asid = Some(asid);
                Ok((asid as u64, 0))
            }

            VSpaceOp::GetASID => {
                let asid = vspace.asid.ok_or(CapError::NoASIDAssigned)?;
                Ok((asid as u64, 0))
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────
    // ASID Pool Operations
    // ─────────────────────────────────────────────────────────────────

    fn invoke_asid_pool(
        pool: &mut AArch64ASIDPool,
        rights: Rights,
        op: u32,
        args: &[u64; 6],
        _kernel: &mut Kernel<Self>,
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
