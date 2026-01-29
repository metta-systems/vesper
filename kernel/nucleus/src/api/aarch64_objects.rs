// ═══════════════════════════════════════════════════════════════════
// AARCH64 ARCHITECTURE OBJECTS
// ═══════════════════════════════════════════════════════════════════

#[cfg(target_arch = "aarch64")]
pub mod aarch64 {
    use super::*;

    /// AArch64-specific kernel objects
    pub struct AArch64;

    impl ArchObjects for AArch64 {
        type Frame = AArch64Frame;
        type PageTable = AArch64PageTable;
        type VSpace = AArch64VSpace;
        type ASIDPool = AArch64ASIDPool;
        type ASID = AArch64ASID;

        /// AArch64 supports 4KB, 2MB, and 1GB pages
        const FRAME_SIZES: &'static [FrameSize] = &[
            FrameSize::Small, // 4KB
            FrameSize::Large, // 2MB
            FrameSize::Huge,  // 1GB
        ];

        /// 4-level page tables (Sv48 equivalent)
        const PT_LEVELS: usize = 4;

        /// 9 bits per level (512 entries)
        const PT_INDEX_BITS: usize = 9;

        fn validate_retype(obj_type: ObjectType, size_bits: u8) -> Result<usize, CapError> {
            match obj_type {
                ObjectType::Frame => {
                    // Validate frame size
                    match size_bits {
                        12 => Ok(4096),               // 4KB
                        21 => Ok(2 * 1024 * 1024),    // 2MB
                        30 => Ok(1024 * 1024 * 1024), // 1GB
                        _ => Err(CapError::InvalidFrameSize(size_bits)),
                    }
                }
                ObjectType::PageTable => {
                    // Page tables are always 4KB on AArch64
                    if size_bits != 12 {
                        return Err(CapError::InvalidSize);
                    }
                    Ok(4096)
                }
                ObjectType::VSpace => {
                    // VSpace object is small metadata, but needs a root PT
                    Ok(core::mem::size_of::<AArch64VSpace>())
                }
                ObjectType::ASIDPool => {
                    // Pool of 256 ASIDs
                    Ok(core::mem::size_of::<AArch64ASIDPool>())
                }
                ObjectType::ASID => Ok(core::mem::size_of::<AArch64ASID>()),
                _ => Err(CapError::NotArchType),
            }
        }

        fn create_arch_object(
            obj_type: ObjectType,
            phys_addr: PhysAddr,
            size_bits: u8,
            pools: &mut ArchPools<Self>,
        ) -> Result<ObjectRef, CapError> {
            match obj_type {
                ObjectType::Frame => {
                    let size = Self::validate_retype(obj_type, size_bits)?;
                    let frame = AArch64Frame::new(phys_addr, FrameSize::from_bits(size_bits)?);
                    let obj = pools
                        .frames
                        .allocate(frame)
                        .ok_or(CapError::PoolExhausted)?;
                    Ok(ObjectRef::new(obj))
                }
                ObjectType::PageTable => {
                    let pt = AArch64PageTable::new(phys_addr);
                    let obj = pools
                        .page_tables
                        .allocate(pt)
                        .ok_or(CapError::PoolExhausted)?;
                    Ok(ObjectRef::new(obj))
                }
                ObjectType::VSpace => {
                    let vspace = AArch64VSpace::new();
                    let obj = pools
                        .vspaces
                        .allocate(vspace)
                        .ok_or(CapError::PoolExhausted)?;
                    Ok(ObjectRef::new(obj))
                }
                ObjectType::ASIDPool => {
                    let pool = AArch64ASIDPool::new();
                    let obj = pools
                        .asid_pools
                        .allocate(pool)
                        .ok_or(CapError::PoolExhausted)?;
                    Ok(ObjectRef::new(obj))
                }
                ObjectType::ASID => {
                    // ASIDs are allocated from pools, not directly
                    Err(CapError::InvalidOperation)
                }
                _ => Err(CapError::NotArchType),
            }
        }
    }

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

    // ─────────────────────────────────────────────────────────────────
    // AArch64 Page Table
    // ─────────────────────────────────────────────────────────────────

    /// A page table on AArch64 (any level: L0/L1/L2/L3).
    ///
    /// Each table has 512 entries (9 bits of index).
    /// The level is tracked for validation during mapping.
    #[derive(Debug)]
    pub struct AArch64PageTable {
        /// Physical address of the table (4KB aligned)
        phys_addr: PhysAddr,
        /// Which level (0 = root, 3 = leaf for 4KB pages)
        level: u8,
        /// Number of valid entries
        mapped_count: u16,
    }

    impl AArch64PageTable {
        /// Number of entries per table
        pub const NUM_ENTRIES: usize = 512;

        pub fn new(phys_addr: PhysAddr) -> Self {
            Self {
                phys_addr,
                level: 0, // Set when attached to VSpace
                mapped_count: 0,
            }
        }

        /// Get the raw entries (for kernel manipulation)
        ///
        /// # Safety
        /// Caller must ensure the physical memory is mapped.
        pub unsafe fn entries(&self) -> &[PageTableEntry; Self::NUM_ENTRIES] {
            let virt = phys_to_virt(self.phys_addr);
            &*(virt.as_ptr() as *const [PageTableEntry; Self::NUM_ENTRIES])
        }

        pub unsafe fn entries_mut(&mut self) -> &mut [PageTableEntry; Self::NUM_ENTRIES] {
            let virt = phys_to_virt(self.phys_addr);
            &mut *(virt.as_mut_ptr() as *mut [PageTableEntry; Self::NUM_ENTRIES])
        }
    }

    impl KernelObject for AArch64PageTable {
        const TYPE: ObjectType = ObjectType::PageTable;
    }

    /// AArch64 page table entry (64 bits)
    #[repr(transparent)]
    #[derive(Copy, Clone)]
    pub struct PageTableEntry(u64);

    impl PageTableEntry {
        pub const VALID: u64 = 1 << 0;
        pub const TABLE: u64 = 1 << 1; // vs block
        pub const AF: u64 = 1 << 10; // Access flag
        pub const AP_RO: u64 = 1 << 7; // Read-only
        pub const AP_EL0: u64 = 1 << 6; // User accessible
        pub const UXN: u64 = 1 << 54; // User execute never
        pub const PXN: u64 = 1 << 53; // Privileged execute never

        pub const fn empty() -> Self {
            Self(0)
        }

        pub const fn is_valid(&self) -> bool {
            self.0 & Self::VALID != 0
        }

        pub fn set_table(&mut self, phys: PhysAddr) {
            self.0 = phys.as_u64() | Self::VALID | Self::TABLE;
        }

        pub fn set_block(&mut self, phys: PhysAddr, attrs: u64) {
            self.0 = phys.as_u64() | Self::VALID | attrs;
        }

        pub fn set_page(&mut self, phys: PhysAddr, attrs: u64) {
            self.0 = phys.as_u64() | Self::VALID | Self::TABLE | attrs;
        }
    }

    // ─────────────────────────────────────────────────────────────────
    // AArch64 VSpace (Virtual Address Space)
    // ─────────────────────────────────────────────────────────────────

    /// A virtual address space on AArch64.
    ///
    /// Contains the root page table (L0) and associated ASID.
    /// Each Domain has one VSpace for its user-space mappings (TTBR0).
    /// The kernel VSpace (TTBR1) is shared.
    #[derive(Debug)]
    pub struct AArch64VSpace {
        /// Root page table (L0)
        root_pt: Option<KeySlot>,
        /// Assigned ASID (for TLB tagging)
        asid: Option<u16>,
        /// Is this active on any CPU?
        active_cpus: u32,
    }

    impl AArch64VSpace {
        pub fn new() -> Self {
            Self {
                root_pt: None,
                asid: None,
                active_cpus: 0,
            }
        }

        /// Attach a root page table
        pub fn set_root(&mut self, pt_slot: KeySlot) {
            self.root_pt = Some(pt_slot);
        }

        /// Assign an ASID
        pub fn set_asid(&mut self, asid: u16) {
            self.asid = Some(asid);
        }

        /// Get TTBR0 value for this VSpace
        pub fn ttbr0(&self, root_pt_phys: PhysAddr) -> u64 {
            let asid = self.asid.unwrap_or(0) as u64;
            (asid << 48) | root_pt_phys.as_u64()
        }
    }

    impl KernelObject for AArch64VSpace {
        const TYPE: ObjectType = ObjectType::VSpace;
    }

    // ─────────────────────────────────────────────────────────────────
    // AArch64 ASID Management
    // ─────────────────────────────────────────────────────────────────

    /// Pool of ASIDs for address space tagging.
    ///
    /// AArch64 supports 8-bit or 16-bit ASIDs (we assume 16-bit).
    /// Each pool manages a range of 256 ASIDs.
    #[derive(Debug)]
    pub struct AArch64ASIDPool {
        /// Base ASID for this pool
        base: u16,
        /// Bitmap of allocated ASIDs (256 bits = 4 u64s)
        allocated: [u64; 4],
        /// Number allocated
        count: u16,
    }

    impl AArch64ASIDPool {
        pub fn new() -> Self {
            Self {
                base: 0,
                allocated: [0; 4],
                count: 0,
            }
        }

        /// Allocate an ASID from this pool
        pub fn allocate(&mut self) -> Option<u16> {
            for (i, word) in self.allocated.iter_mut().enumerate() {
                if *word != !0 {
                    let bit = word.trailing_ones() as u16;
                    *word |= 1 << bit;
                    self.count += 1;
                    return Some(self.base + (i as u16 * 64) + bit);
                }
            }
            None
        }

        /// Release an ASID back to the pool
        pub fn release(&mut self, asid: u16) -> bool {
            let offset = asid - self.base;
            if offset >= 256 {
                return false;
            }
            let word = (offset / 64) as usize;
            let bit = offset % 64;
            if self.allocated[word] & (1 << bit) != 0 {
                self.allocated[word] &= !(1 << bit);
                self.count -= 1;
                true
            } else {
                false
            }
        }
    }

    impl KernelObject for AArch64ASIDPool {
        const TYPE: ObjectType = ObjectType::ASIDPool;
    }

    /// An allocated ASID (capability wrapper)
    #[derive(Debug)]
    pub struct AArch64ASID {
        /// The actual ASID value
        value: u16,
        /// Pool it came from
        pool_slot: KeySlot,
    }

    impl KernelObject for AArch64ASID {
        const TYPE: ObjectType = ObjectType::ASID;
    }
}

#[cfg(target_arch = "aarch64")]
pub use aarch64::*;
