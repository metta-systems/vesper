use {
    crate::{
        Address, Physical,
        arch::mmu::translation_table::{
            PageFlags, PageSize, STAGE1_PAGE_DESCRIPTOR, STAGE1_TABLE_DESCRIPTOR, Size2MiB,
            Size4KiB, TableFlags,
        },
        mmu::{AddressSpace, AttributeFields, MMUEnableError, TranslationGranule, interface},
    },
    aarch64_cpu::{
        asm::{self, barrier},
        registers::{ID_AA64MMFR0_EL1, SCTLR_EL1, TCR_EL1},
    },
    core::intrinsics::unlikely,
    liblog::println,
    tock_registers::interfaces::{ReadWriteable, Readable, Writeable},
};

pub(crate) mod translation_table;

//--------------------------------------------------------------------------------------------------
// Private Definitions
//--------------------------------------------------------------------------------------------------

/// Memory Management Unit type.
struct MemoryManagementUnit;

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

pub type Granule512MiB = TranslationGranule<{ 512 * 1024 * 1024 }>;
pub type Granule64KiB = TranslationGranule<{ 64 * 1024 }>;

/// Constants for indexing the MAIR_EL1.
#[allow(dead_code)]
pub mod mair {
    // Three descriptive consts for indexing into the correct MAIR_EL1 attributes.
    pub mod attr {
        pub const NORMAL: u64 = 0;
        pub const NORMAL_NON_CACHEABLE: u64 = 1;
        pub const DEVICE_NGNRE: u64 = 2;
        // DEVICE_GRE
        // DEVICE_NGNRNE
    }
}

//--------------------------------------------------------------------------------------------------
// Global instances
//--------------------------------------------------------------------------------------------------

static MMU: MemoryManagementUnit = MemoryManagementUnit;

//--------------------------------------------------------------------------------------------------
// Private Implementations
//--------------------------------------------------------------------------------------------------

impl<const AS_SIZE: usize> AddressSpace<AS_SIZE> {
    /// Checks for architectural restrictions.
    pub const fn arch_address_space_size_sanity_checks() {
        // Size must be at least one full 512 MiB table.
        assert!(AS_SIZE.is_multiple_of(Granule512MiB::SIZE)); // assert!() is const-friendly

        // Check for 48 bit virtual address size as maximum, which is supported by any ARMv8
        // version.
        assert!(AS_SIZE <= (1 << 48));
    }
}

impl MemoryManagementUnit {
    /// Setup function for the MAIR_EL1 register.
    fn set_up_mair(&self) {
        use aarch64_cpu::registers::MAIR_EL1;
        // Define the three memory types that we will map: Normal DRAM, Uncached and device.
        MAIR_EL1.write(
            // Attribute 2 -- Device Memory
            MAIR_EL1::Attr2_Device::nonGathering_nonReordering_EarlyWriteAck
                // Attribute 1 -- Non Cacheable DRAM
                + MAIR_EL1::Attr1_Normal_Outer::NonCacheable
                + MAIR_EL1::Attr1_Normal_Inner::NonCacheable
                // Attribute 0 -- Regular Cacheable
                + MAIR_EL1::Attr0_Normal_Outer::WriteBack_NonTransient_ReadWriteAlloc
                + MAIR_EL1::Attr0_Normal_Inner::WriteBack_NonTransient_ReadWriteAlloc,
        );
    }

    /// Configure various settings of stage 1 of the EL1 translation regime.
    fn configure_translation_control(&self) {
        // TCR_EL1.{SH0, ORGN0, IRGN0, SH1, ORGN1, IRGN1} fields define memory region attributes for the
        // translation table walk, for each of TTBR0_EL1 and TTBR1_EL1.
        // For the Secure and Non-secure EL1&0 stage 1 translations, each of TTBR0_EL1 and TTBR1_EL1
        // contains an ASID field, and the TCR_EL1.A1 field selects which ASID to use.

        // Two-level tables with a 4Kb granule size may address ONLY 1Gb of virtual addresses.
        // This seems to be not enough for RPi4? Try using tables from level 1 (TxSZ=below 34 bits), up to 512Gb

        // Configure various settings of stage 1 of the EL1 translation regime.
        // PARange is 4 bits, ips is 3 bits @todo validate the range is acceptable.
        let ips = ID_AA64MMFR0_EL1.read(ID_AA64MMFR0_EL1::PARange);

        // Maximum 8Gb user VA
        let user_va_bits = 33; // ARMv8ARM Table D5-11 minimum TxSZ for starting table level 1

        // Maximum 8Gb kernel VA
        let kernel_va_bits = 33; // ARMv8ARM Table D5-11 minimum TxSZ for starting table level 1

        TCR_EL1.write(
            TCR_EL1::TBI0::Ignored // Top byte ignored, can be used for tagging.
                + TCR_EL1::IPS.val(ips) // Intermediate Physical Address Size
                // ttbr0 user memory addresses
                + TCR_EL1::TG0::KiB_4 // 4 KiB granule
                + TCR_EL1::SH0::Inner
                + TCR_EL1::ORGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
                + TCR_EL1::IRGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
                + TCR_EL1::EPD0::EnableTTBR0Walks
                + TCR_EL1::T0SZ.val(64 - user_va_bits) // ARMv8ARM Table D5-11 minimum TxSZ for starting table level 2
                // ttbr1 kernel memory addresses
                + TCR_EL1::TBI1::Ignored // Top byte ignored, can be used for tagging. @todo remove!
                + TCR_EL1::TG1::KiB_4 // 4 KiB granule
                + TCR_EL1::SH1::Inner
                + TCR_EL1::ORGN1::WriteBack_ReadAlloc_WriteAlloc_Cacheable
                + TCR_EL1::IRGN1::WriteBack_ReadAlloc_WriteAlloc_Cacheable
                + TCR_EL1::EPD1::DisableTTBR1Walks // @fixme disabled for now
                + TCR_EL1::T1SZ.val(64 - kernel_va_bits), // ARMv8ARM Table D5-11 minimum TxSZ for starting table level 2
        );
    }
}

//--------------------------------------------------------------------------------------------------
// Public Implementations
//--------------------------------------------------------------------------------------------------

/// Return a reference to the MMU instance.
pub fn mmu() -> &'static impl interface::MMU {
    &MMU
}

//------------------------------------------------------------------------------
// OS Interface Code
//------------------------------------------------------------------------------

impl interface::MMU for MemoryManagementUnit {
    unsafe fn enable_mmu_and_caching(
        &self,
        _phys_tables_base_addr: Address<Physical>,
    ) -> Result<(), MMUEnableError> {
        if unlikely(self.is_enabled()) {
            return Err(MMUEnableError::AlreadyEnabled);
        }

        // Fail early if translation granule is not supported.
        if unlikely(!ID_AA64MMFR0_EL1.matches_all(ID_AA64MMFR0_EL1::TGran64::Supported)) {
            return Err(MMUEnableError::Other {
                err: "Translation granule not supported by hardware",
            });
        }

        // Prepare the memory attribute indirection register.
        self.set_up_mair();

        // // Populate translation tables.
        // KERNEL_TABLES
        //     .populate_translation_table_entries()
        //     .map_err(|err| MMUEnableError::Other { err })?;

        // from https://lore.kernel.org/all/db9612a7-9354-2357-9083-1d923b4d11e1@linaro.org/T/
        // The ARMv8.2-TTCNP extension allows an implementation to optimize by
        // sharing TLB entries between multiple cores, provided that software
        // declares that it's ready to deal with this by setting a CnP bit in
        // the TTBRn_ELx.  It is mandatory from ARMv8.2 onward.

        // support feature flag is in ID_AA64MMFR2
        // https://developer.arm.com/documentation/ddi0601/2022-03/AArch64-Registers/ID-AA64MMFR2-EL1--AArch64-Memory-Model-Feature-Register-2?lang=en
        // CnP bits 3:0
        // From Armv8.2, the only permitted value is 0b0001.
        // (this should be set to share the TLBs across cores.)

        // Point to the LVL2 table base address in TTBR0.
        // TODO: USER_TABLES, not KERNEL_TABLES here?
        // TTBR0_EL1.set_baddr(KERNEL_TABLES.entries.base_addr_u64()); // User (lo-)space addresses
        // TTBR0_EL1.modify(TTBR0_EL1::CnP.val(1));

        // TODO: also do kernel level tables (same mappings but at higher table addresses? need to update ttt to do it)
        // TTBR1_EL1.set_baddr(LVL1_TABLE.entries.base_addr_u64()); // Kernel (hi-)space addresses
        // TTBR1_EL1.modify(TTBR1_EL1::CnP.val(1));

        // upper half, kernel space
        // asm volatile ("msr ttbr1_el1, %0" : : "r" ((unsigned long)&_end + TTBR_CNP + PAGESIZE));

        self.configure_translation_control();

        // Switch the MMU on.
        //
        // First, force all previous changes to be seen before the MMU is enabled.
        // See [ARM ARM](https://developer.arm.com/documentation/den0024/a/The-Memory-Management-Unit/The-Translation-Lookaside-Buffer).
        barrier::dsb(barrier::ISHST); // ensure write has completed

        // core::arch::asm!("tlbi alle1"); // invalidate all TLB entries -- must do it from EL2/EL3

        barrier::dsb(barrier::ISH); // ensure completion of TLB invalidation
        barrier::isb(barrier::SY); // synchronize context and ensure that no instructions are
        // fetched using the old translation

        // use cortex_a::regs::RegisterReadWrite;
        // Enable the MMU and turn on data and instruction caching.

        SCTLR_EL1.modify(
            SCTLR_EL1::EE::LittleEndian // Endianness select in EL1
                + SCTLR_EL1::E0E::LittleEndian // Endianness select in EL0
                + SCTLR_EL1::WXN::Disable // Writable means Execute Never
                + SCTLR_EL1::SA::Disable // SP Alignment check in EL1, 16 byte align
                + SCTLR_EL1::SA0::Disable // SP Alignment check in EL0, 16 byte align
                + SCTLR_EL1::A::Disable // No alignment checks
                + SCTLR_EL1::UCI::Trap // Unified Cache instructions trap
                + SCTLR_EL1::UCT::Trap // CTR_EL0 instructions trap
                + SCTLR_EL1::UMA::Trap // User Mask Access, trap on DAIF access
                + SCTLR_EL1::NTWE::Trap // WFE/WFET instruction trap
                + SCTLR_EL1::NTWI::Trap // WFI/WFIT instruction trap
                + SCTLR_EL1::DZE::Trap // DC ZVA/GVA/GZVA instructions trap
                + SCTLR_EL1::C::Cacheable
                + SCTLR_EL1::I::Cacheable
                + SCTLR_EL1::M::Enable,
        );

        // from https://forums.raspberrypi.com/viewtopic.php?t=320120#p1917769
        // Another hint: once the MMU has been activated you should let 2 CPU cycles pass and then call
        // `tlbi alle2` to ensure the MMU related cache will be invalidated and the new settings are picked up.

        asm::nop();
        asm::nop();
        //TODO: tlbi

        // Force MMU init to complete before next instruction
        /*
         * Invalidate the local I-cache so that any instructions fetched
         * speculatively from the PoC are discarded, since they may have
         * been dynamically patched at the PoU.
         */
        // core::arch::asm!("tlbi alle1"); // invalidate all TLB entries -- must do it from EL2/EL3

        // FIXME compiler happily inserts an instruction before this one... perhaps a compiler_fence()?
        barrier::dsb(barrier::ISH); // ensure completion of TLB invalidation
        barrier::isb(barrier::SY); // synchronize context and ensure that no instructions are fetched using the old translation

        println!("MMU activated");

        Ok(())
    }

    #[inline(always)]
    fn is_enabled(&self) -> bool {
        SCTLR_EL1.matches_all(SCTLR_EL1::M::Enable)
    }

    fn print_features(&self) {
        todo!()
    }
}

/// Type-safe enum wrapper covering Table<L>'s 64-bit entries.
#[derive(Clone)]
// #[repr(transparent)]
enum PageTableEntry {
    /// Empty page table entry.
    Invalid,
    /// Table descriptor is a L0, L1 or L2 table pointing to another table.
    /// L0 tables can only point to L1 tables.
    /// A descriptor pointing to the next page table.
    TableDescriptor(TableFlags),
    /// A Level2 block descriptor with 2 MiB aperture.
    ///
    /// The output points to physical memory.
    Lvl2BlockDescriptor(TableFlags),
    /// A page PageTableEntry::descriptor with 4 KiB aperture.
    ///
    /// The output points to physical memory.
    PageDescriptor(PageFlags),
}

// A descriptor pointing to the next page table. (within PageTableEntry enum)
// struct TableDescriptor(register::FieldValue<u64, STAGE1_DESCRIPTOR::Register>);

impl PageTableEntry {
    fn new_table_descriptor(next_lvl_table_addr: usize) -> Result<PageTableEntry, &'static str> {
        if next_lvl_table_addr % Size4KiB::SIZE as usize != 0 {
            // @todo SIZE must be usize
            return Err("TableDescriptor: Address is not 4 KiB aligned.");
        }

        let shifted = next_lvl_table_addr >> Size4KiB::SHIFT;

        Ok(PageTableEntry::TableDescriptor(
            STAGE1_TABLE_DESCRIPTOR::VALID::True
                + STAGE1_TABLE_DESCRIPTOR::TYPE::Table
                + STAGE1_TABLE_DESCRIPTOR::NEXT_LEVEL_TABLE_ADDR_4KiB.val(shifted as u64),
        ))
    }
}

#[derive(Debug)] //Snafu,
enum PageTableError {
    // #[snafu(display("BlockDescriptor: Address is not 2 MiB aligned."))]
    //"PageDescriptor: Address is not 4 KiB aligned."
    NotAligned(&'static str),
}

// A Level2 block descriptor with 2 MiB aperture.
//
// The output points to physical memory.
// struct Lvl2BlockDescriptor(register::FieldValue<u64, STAGE1_DESCRIPTOR::Register>);

impl PageTableEntry {
    fn new_lvl2_block_descriptor(
        output_addr: usize,
        _attribute_fields: AttributeFields,
    ) -> Result<PageTableEntry, PageTableError> {
        if output_addr % Size2MiB::SIZE as usize != 0 {
            return Err(PageTableError::NotAligned(Size2MiB::SIZE_AS_DEBUG_STR));
        }

        let shifted = output_addr >> Size2MiB::SHIFT;

        Ok(PageTableEntry::Lvl2BlockDescriptor(
            STAGE1_TABLE_DESCRIPTOR::VALID::True
                // + STAGE1_TABLE_DESCRIPTOR::AF::Accessed
                // + into_mmu_attributes(attribute_fields)
                + STAGE1_TABLE_DESCRIPTOR::TYPE::Block
                + STAGE1_TABLE_DESCRIPTOR::NEXT_LEVEL_TABLE_ADDR_4KiB.val(shifted as u64),
        ))
    }
}

// A page descriptor with 4 KiB aperture.
//
// The output points to physical memory.

impl PageTableEntry {
    fn new_page_descriptor(
        output_addr: usize,
        _attribute_fields: AttributeFields,
    ) -> Result<PageTableEntry, PageTableError> {
        if output_addr % Size4KiB::SIZE as usize != 0 {
            return Err(PageTableError::NotAligned(Size4KiB::SIZE_AS_DEBUG_STR));
        }

        let shifted = output_addr >> Size4KiB::SHIFT;

        Ok(PageTableEntry::PageDescriptor(
            STAGE1_PAGE_DESCRIPTOR::VALID::True
                // + STAGE1_TABLE_DESCRIPTOR::AF::Accessed
                // + into_mmu_attributes(attribute_fields)
                + STAGE1_PAGE_DESCRIPTOR::TYPE::Page
                + STAGE1_PAGE_DESCRIPTOR::OUTPUT_ADDR_4KiB.val(shifted as u64),
        ))
    }
}

impl From<u64> for PageTableEntry {
    fn from(_val: u64) -> PageTableEntry {
        // xxx0 -> Invalid
        // xx11 -> TableDescriptor on L0, L1 and L2
        // xx10 -> Block Entry L1 and L2
        // xx11 -> PageDescriptor L3
        PageTableEntry::Invalid
    }
}

impl From<PageTableEntry> for u64 {
    fn from(val: PageTableEntry) -> u64 {
        match val {
            PageTableEntry::Invalid => 0,
            PageTableEntry::TableDescriptor(x) | PageTableEntry::Lvl2BlockDescriptor(x) => x.value,
            PageTableEntry::PageDescriptor(x) => x.value,
        }
    }
}

// to get L0 we must allocate a few frames from boot region allocator.
// So, first we init the dtb, parse mem-regions from there, then init boot_info page and start mmu,
// this part will be inited in mmu::init():

// @todo do NOT keep these statically, always allocate from available bump memory
// static mut LVL2_TABLE: Table<PageDirectory> = Table::<PageDirectory> {
//     entries: [0; NUM_ENTRIES_4KIB as usize],
//     level: PhantomData,
// };

// @todo do NOT keep these statically, always allocate from available bump memory
// static mut LVL3_TABLE: Table<PageTable> = Table::<PageTable> {
//     entries: [0; NUM_ENTRIES_4KIB as usize],
//     level: PhantomData,
// };

trait BaseAddr {
    fn base_addr_u64(&self) -> u64;
    fn base_addr_usize(&self) -> usize;
}

impl BaseAddr for [u64; 512] {
    fn base_addr_u64(&self) -> u64 {
        self as *const u64 as u64
    }

    fn base_addr_usize(&self) -> usize {
        self as *const u64 as usize
    }
}

/// Set up identity mapped page tables for the first 1 gigabyte of address space.
/// default: 880 MB ARM ram, 128MB VC
///
/// # Safety
///
/// Completely unsafe, we're in the hardware land! Incorrectly initialised tables will just
/// restart the CPU.
pub unsafe fn init() -> Result<(), &'static str> {
    // Prepare the memory attribute indirection register.
    // mair::set_up();

    // Point the first 2 MiB of virtual addresses to the follow-up LVL3
    // page-table.
    // LVL2_TABLE.entries[0] =
    //     PageTableEntry::new_table_descriptor(LVL3_TABLE.entries.base_addr_usize())?.into();

    // Fill the rest of the LVL2 (2 MiB) entries as block descriptors.
    //
    // Notice the skip(1) which makes the iteration start at the second 2 MiB
    // block (0x20_0000).
    // for (block_descriptor_nr, entry) in LVL2_TABLE.entries.iter_mut().enumerate().skip(1) {
    //     let virt_addr = block_descriptor_nr << Size2MiB::SHIFT;

    //     let (output_addr, attribute_fields) = match get_virt_addr_properties(virt_addr) {
    //         Err(s) => return Err(s),
    //         Ok((a, b)) => (a, b),
    //     };

    //     let block_desc =
    //         match PageTableEntry::new_lvl2_block_descriptor(output_addr, attribute_fields) {
    //             Err(s) => return Err(s),
    //             Ok(desc) => desc,
    //         };

    //     *entry = block_desc.into();
    // }

    // Finally, fill the single LVL3 table (4 KiB granule).
    // for (page_descriptor_nr, entry) in LVL3_TABLE.entries.iter_mut().enumerate() {
    //     let virt_addr = page_descriptor_nr << Size4KiB::SHIFT;

    //     let (output_addr, attribute_fields) = match get_virt_addr_properties(virt_addr) {
    //         Err(s) => return Err(s),
    //         Ok((a, b)) => (a, b),
    //     };

    //     let page_desc = match PageTableEntry::new_page_descriptor(output_addr, attribute_fields) {
    //         Err(s) => return Err(s),
    //         Ok(desc) => desc,
    //     };

    //     *entry = page_desc.into();
    // }

    // Point to the LVL2 table base address in TTBR0.
    // TTBR0_EL1.set_baddr(LVL2_TABLE.entries.base_addr_u64()); // User (lo-)space addresses

    // TTBR1_EL1.set_baddr(LVL2_TABLE.entries.base_addr_u64()); // Kernel (hi-)space addresses

    // Configure various settings of stage 1 of the EL1 translation regime.
    let ips = ID_AA64MMFR0_EL1.read(ID_AA64MMFR0_EL1::PARange);
    TCR_EL1.write(
        TCR_EL1::TBI0::Ignored // @todo TBI1 also set to Ignored??
            + TCR_EL1::IPS.val(ips) // Intermediate Physical Address Size
            // ttbr0 user memory addresses
            + TCR_EL1::TG0::KiB_4 // 4 KiB granule
            + TCR_EL1::SH0::Inner
            + TCR_EL1::ORGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
            + TCR_EL1::IRGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
            + TCR_EL1::EPD0::EnableTTBR0Walks
            + TCR_EL1::T0SZ.val(34) // ARMv8ARM Table D5-11 minimum TxSZ for starting table level 2
            // ttbr1 kernel memory addresses
            + TCR_EL1::TG1::KiB_4 // 4 KiB granule
            + TCR_EL1::SH1::Inner
            + TCR_EL1::ORGN1::WriteBack_ReadAlloc_WriteAlloc_Cacheable
            + TCR_EL1::IRGN1::WriteBack_ReadAlloc_WriteAlloc_Cacheable
            + TCR_EL1::EPD1::EnableTTBR1Walks
            + TCR_EL1::T1SZ.val(34), // ARMv8ARM Table D5-11 minimum TxSZ for starting table level 2
    );

    // Switch the MMU on.
    //
    // First, force all previous changes to be seen before the MMU is enabled.
    barrier::isb(barrier::SY);

    // use cortex_a::regs::RegisterReadWrite;
    // Enable the MMU and turn on data and instruction caching.
    SCTLR_EL1.modify(SCTLR_EL1::M::Enable + SCTLR_EL1::C::Cacheable + SCTLR_EL1::I::Cacheable);

    // Force MMU init to complete before next instruction
    /*
     * Invalidate the local I-cache so that any instructions fetched
     * speculatively from the PoC are discarded, since they may have
     * been dynamically patched at the PoU.
     */
    barrier::isb(barrier::SY);

    Ok(())
}

// A function that maps the generic memory range attributes to HW-specific
// attributes of the MMU.
// fn into_mmu_attributes(
//     attribute_fields: AttributeFields,
// ) -> FieldValue<u64, STAGE1_DESCRIPTOR::Register> {
//     use super::{AccessPermissions, MemAttributes};

//     // Memory attributes
//     let mut desc = match attribute_fields.mem_attributes {
//         MemAttributes::CacheableDRAM => {
//             STAGE1_DESCRIPTOR::SH::InnerShareable
//                 + STAGE1_DESCRIPTOR::AttrIndx.val(mair::attr::NORMAL)
//         }
//         MemAttributes::NonCacheableDRAM => {
//             STAGE1_DESCRIPTOR::SH::InnerShareable
//                 + STAGE1_DESCRIPTOR::AttrIndx.val(mair::attr::NORMAL_NON_CACHEABLE)
//         }
//         MemAttributes::Device => {
//             STAGE1_DESCRIPTOR::SH::OuterShareable
//                 + STAGE1_DESCRIPTOR::AttrIndx.val(mair::attr::DEVICE_NGNRE)
//         }
//     };

//     // Access Permissions
//     desc += match attribute_fields.acc_perms {
//         AccessPermissions::ReadOnly => STAGE1_DESCRIPTOR::AP::RO_EL1,
//         AccessPermissions::ReadWrite => STAGE1_DESCRIPTOR::AP::RW_EL1,
//     };

//     // Execute Never
//     desc += if attribute_fields.execute_never {
//         STAGE1_DESCRIPTOR::PXN::NeverExecute
//     } else {
//         STAGE1_DESCRIPTOR::PXN::Execute
//     };

//     desc
// }
