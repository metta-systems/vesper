/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2018-2019 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 */

//! MMU initialisation.
//!
//! Paging is mostly based on [previous version](https://os.phil-opp.com/page-tables/) of
//! Phil Opp's [paging guide](https://os.phil-opp.com/paging-implementation/) and
//! [ARMv8 ARM memory addressing](https://static.docs.arm.com/100940/0100/armv8_a_address%20translation_100940_0100_en.pdf).

use {
    crate::{
        arch::aarch64::memory::{
            get_virt_addr_properties, AttributeFields, /*FrameAllocator, PhysAddr, VirtAddr,*/
        },
        println,
    },
    // bitflags::bitflags,
    core::{
        // convert::TryInto,
        // fmt,
        marker::PhantomData,
        ops::{Index, IndexMut},
        // ptr::Unique,
    },
    cortex_a::{
        asm::barrier,
        registers::{ID_AA64MMFR0_EL1, SCTLR_EL1, TCR_EL1, TTBR0_EL1},
    },
    tock_registers::{
        fields::FieldValue,
        interfaces::{ReadWriteable, Readable, Writeable},
        register_bitfields,
    },
    // ux::*,
};

mod mair {
    use cortex_a::registers::MAIR_EL1;
    use tock_registers::interfaces::Writeable;

    /// Setup function for the MAIR_EL1 register.
    pub fn set_up() {
        // Define the three memory types that we will map. Normal DRAM, Uncached and device.
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

    // Three descriptive consts for indexing into the correct MAIR_EL1 attributes.
    pub mod attr {
        pub const NORMAL: u64 = 0;
        pub const NORMAL_NON_CACHEABLE: u64 = 1;
        pub const DEVICE_NGNRE: u64 = 2;
        // DEVICE_GRE
        // DEVICE_NGNRNE
    }
}

register_bitfields! {
    u64,
    // AArch64 Reference Manual page 2150, D5-2445
    STAGE1_DESCRIPTOR [
        // In table descriptors

        NSTable_EL3   OFFSET(63) NUMBITS(1) [],

        /// Access Permissions for subsequent tables
        APTable  OFFSET(61) NUMBITS(2) [
            RW_EL1 = 0b00,
            RW_EL1_EL0 = 0b01,
            RO_EL1 = 0b10,
            RO_EL1_EL0 = 0b11
        ],

        // User execute-never for subsequent tables
        UXNTable OFFSET(60) NUMBITS(1) [
            Execute = 0,
            NeverExecute = 1
        ],

        /// Privileged execute-never for subsequent tables
        PXNTable OFFSET(59) NUMBITS(1) [
            Execute = 0,
            NeverExecute = 1
        ],

        // In block descriptors

        // OS-specific data
        OSData      OFFSET(55) NUMBITS(4) [],

        // User execute-never
        UXN      OFFSET(54) NUMBITS(1) [
            Execute = 0,
            NeverExecute = 1
        ],

        /// Privileged execute-never
        PXN      OFFSET(53) NUMBITS(1) [
            Execute = 0,
            NeverExecute = 1
        ],

        // @fixme ?? where is this described
        CONTIGUOUS OFFSET(52) NUMBITS(1) [
            False = 0,
            True = 1
        ],

        // @fixme ?? where is this described
        DIRTY OFFSET(51) NUMBITS(1) [
            False = 0,
            True = 1
        ],

        /// Various address fields, depending on use case
        LVL2_OUTPUT_ADDR_4KiB    OFFSET(21) NUMBITS(27) [], // [47:21]
        NEXT_LVL_TABLE_ADDR_4KiB OFFSET(12) NUMBITS(36) [], // [47:12]

        // @fixme ?? where is this described
        NON_GLOBAL OFFSET(11) NUMBITS(1) [
            False = 0,
            True = 1
        ],

        /// Access flag
        AF       OFFSET(10) NUMBITS(1) [
            False = 0,
            True = 1
        ],

        /// Shareability field
        SH       OFFSET(8) NUMBITS(2) [
            OuterShareable = 0b10,
            InnerShareable = 0b11
        ],

        /// Access Permissions
        AP       OFFSET(6) NUMBITS(2) [
            RW_EL1 = 0b00,
            RW_EL1_EL0 = 0b01,
            RO_EL1 = 0b10,
            RO_EL1_EL0 = 0b11
        ],

        NS_EL3   OFFSET(5) NUMBITS(1) [],

        /// Memory attributes index into the MAIR_EL1 register
        AttrIndx OFFSET(2) NUMBITS(3) [],

        TYPE     OFFSET(1) NUMBITS(1) [
            Block = 0,
            Table = 1
        ],

        VALID    OFFSET(0) NUMBITS(1) [
            False = 0,
            True = 1
        ]
    ]
}

/// A function that maps the generic memory range attributes to HW-specific
/// attributes of the MMU.
fn into_mmu_attributes(
    attribute_fields: AttributeFields,
) -> FieldValue<u64, STAGE1_DESCRIPTOR::Register> {
    use super::{AccessPermissions, MemAttributes};

    // Memory attributes
    let mut desc = match attribute_fields.mem_attributes {
        MemAttributes::CacheableDRAM => {
            STAGE1_DESCRIPTOR::SH::InnerShareable
                + STAGE1_DESCRIPTOR::AttrIndx.val(mair::attr::NORMAL)
        }
        MemAttributes::NonCacheableDRAM => {
            STAGE1_DESCRIPTOR::SH::InnerShareable
                + STAGE1_DESCRIPTOR::AttrIndx.val(mair::attr::NORMAL_NON_CACHEABLE)
        }
        MemAttributes::Device => {
            STAGE1_DESCRIPTOR::SH::OuterShareable
                + STAGE1_DESCRIPTOR::AttrIndx.val(mair::attr::DEVICE_NGNRE)
        }
    };

    // Access Permissions
    desc += match attribute_fields.acc_perms {
        AccessPermissions::ReadOnly => STAGE1_DESCRIPTOR::AP::RO_EL1,
        AccessPermissions::ReadWrite => STAGE1_DESCRIPTOR::AP::RW_EL1,
    };

    // Execute Never
    desc += if attribute_fields.execute_never {
        STAGE1_DESCRIPTOR::PXN::NeverExecute
    } else {
        STAGE1_DESCRIPTOR::PXN::Execute
    };

    desc
}

/*
 *  With 4k page granule, a virtual address is split into 4 lookup parts
 *  spanning 9 bits each:
 *
 *    _______________________________________________
 *   |       |       |       |       |       |       |
 *   | signx |  Lv0  |  Lv1  |  Lv2  |  Lv3  |  off  |
 *   |_______|_______|_______|_______|_______|_______|
 *     63-48   47-39   38-30   29-21   20-12   11-00
 *
 *             mask        page size
 *
 *    Lv0: FF8000000000       --
 *    Lv1:   7FC0000000       1G
 *    Lv2:     3FE00000       2M
 *    Lv3:       1FF000       4K
 *    off:          FFF
 *
 * RPi3 supports 64K and 4K granules, also 40-bit physical addresses.
 * It also can address only 1G physical memory, so these 40-bit phys addresses are a fake.
 *
 * 48-bit virtual address space; different mappings in VBAR0 (EL0) and VBAR1 (EL1+).
 */

/// Number of entries in a 4KiB mmu table.
pub const NUM_ENTRIES_4KIB: u64 = 512;

/// Trait for abstracting over the possible page sizes, 4KiB, 16KiB, 2MiB, 1GiB.
pub trait PageSize: Copy + Eq + PartialOrd + Ord {
    /// The page size in bytes.
    const SIZE: u64;

    /// A string representation of the page size for debug output.
    const SIZE_AS_DEBUG_STR: &'static str;

    /// The page shift in bits.
    const SHIFT: usize;

    /// The page mask in bits.
    const MASK: u64;
}

/// This trait is implemented for 4KiB, 16KiB, and 2MiB pages, but not for 1GiB pages.
pub trait NotGiantPageSize: PageSize {} // @todo doesn't have to be pub??

/// A standard 4KiB page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Size4KiB {}

impl PageSize for Size4KiB {
    const SIZE: u64 = 4096;
    const SIZE_AS_DEBUG_STR: &'static str = "4KiB";
    const SHIFT: usize = 12;
    const MASK: u64 = 0xfff;
}

impl NotGiantPageSize for Size4KiB {}

/// A “huge” 2MiB page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Size2MiB {}

impl PageSize for Size2MiB {
    const SIZE: u64 = Size4KiB::SIZE * NUM_ENTRIES_4KIB;
    const SIZE_AS_DEBUG_STR: &'static str = "2MiB";
    const SHIFT: usize = 21;
    const MASK: u64 = 0x1fffff;
}

impl NotGiantPageSize for Size2MiB {}

type EntryFlags = tock_registers::fields::FieldValue<u64, STAGE1_DESCRIPTOR::Register>;
// type EntryRegister = register::LocalRegisterCopy<u64, STAGE1_DESCRIPTOR::Register>;

/// L0 table -- only pointers to L1 tables
pub enum PageGlobalDirectory {}
/// L1 tables -- pointers to L2 tables or giant 1GiB pages
pub enum PageUpperDirectory {}
/// L2 tables -- pointers to L3 tables or huge 2MiB pages
pub enum PageDirectory {}
/// L3 tables -- only pointers to 4/16KiB pages
pub enum PageTable {}

/// Shared trait for specific table levels.
pub trait TableLevel {}

/// Shared trait for hierarchical table levels.
///
/// Specifies what is the next level of page table hierarchy.
pub trait HierarchicalLevel: TableLevel {
    /// Level of the next translation table below this one.
    type NextLevel: TableLevel;
}

impl TableLevel for PageGlobalDirectory {}
impl TableLevel for PageUpperDirectory {}
impl TableLevel for PageDirectory {}
impl TableLevel for PageTable {}

impl HierarchicalLevel for PageGlobalDirectory {
    type NextLevel = PageUpperDirectory;
}
impl HierarchicalLevel for PageUpperDirectory {
    type NextLevel = PageDirectory;
}
impl HierarchicalLevel for PageDirectory {
    type NextLevel = PageTable;
}
// PageTables do not have next level, therefore they are not HierarchicalLevel

/// MMU address translation table.
/// Contains just u64 internally, provides enum interface on top
#[repr(C)]
#[repr(align(4096))]
pub struct Table<L: TableLevel> {
    entries: [u64; NUM_ENTRIES_4KIB as usize],
    level: PhantomData<L>,
}

// Implementation code shared for all levels of page tables
impl<L> Table<L>
where
    L: TableLevel,
{
    /// Zero out entire table.
    pub fn zero(&mut self) {
        for entry in self.entries.iter_mut() {
            *entry = 0;
        }
    }
}

impl<L> Index<usize> for Table<L>
where
    L: TableLevel,
{
    type Output = u64;

    fn index(&self, index: usize) -> &u64 {
        &self.entries[index]
    }
}

impl<L> IndexMut<usize> for Table<L>
where
    L: TableLevel,
{
    fn index_mut(&mut self, index: usize) -> &mut u64 {
        &mut self.entries[index]
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
    TableDescriptor(EntryFlags),
    /// A Level2 block descriptor with 2 MiB aperture.
    ///
    /// The output points to physical memory.
    Lvl2BlockDescriptor(EntryFlags),
    /// A page PageTableEntry::descriptor with 4 KiB aperture.
    ///
    /// The output points to physical memory.
    PageDescriptor(EntryFlags),
}

/// A descriptor pointing to the next page table. (within PageTableEntry enum)
// struct TableDescriptor(register::FieldValue<u64, STAGE1_DESCRIPTOR::Register>);

impl PageTableEntry {
    fn new_table_descriptor(next_lvl_table_addr: usize) -> Result<PageTableEntry, &'static str> {
        if next_lvl_table_addr % Size4KiB::SIZE as usize != 0 {
            // @todo SIZE must be usize
            return Err("TableDescriptor: Address is not 4 KiB aligned.");
        }

        let shifted = next_lvl_table_addr >> Size4KiB::SHIFT;

        Ok(PageTableEntry::TableDescriptor(
            STAGE1_DESCRIPTOR::VALID::True
                + STAGE1_DESCRIPTOR::TYPE::Table
                + STAGE1_DESCRIPTOR::NEXT_LVL_TABLE_ADDR_4KiB.val(shifted as u64),
        ))
    }
}

/// A Level2 block descriptor with 2 MiB aperture.
///
/// The output points to physical memory.
// struct Lvl2BlockDescriptor(register::FieldValue<u64, STAGE1_DESCRIPTOR::Register>);

impl PageTableEntry {
    fn new_lvl2_block_descriptor(
        output_addr: usize,
        attribute_fields: AttributeFields,
    ) -> Result<PageTableEntry, &'static str> {
        if output_addr % Size2MiB::SIZE as usize != 0 {
            return Err("BlockDescriptor: Address is not 2 MiB aligned.");
        }

        let shifted = output_addr >> Size2MiB::SHIFT;

        Ok(PageTableEntry::Lvl2BlockDescriptor(
            STAGE1_DESCRIPTOR::VALID::True
                + STAGE1_DESCRIPTOR::AF::True
                + into_mmu_attributes(attribute_fields)
                + STAGE1_DESCRIPTOR::TYPE::Block
                + STAGE1_DESCRIPTOR::LVL2_OUTPUT_ADDR_4KiB.val(shifted as u64),
        ))
    }
}

/// A page descriptor with 4 KiB aperture.
///
/// The output points to physical memory.

impl PageTableEntry {
    fn new_page_descriptor(
        output_addr: usize,
        attribute_fields: AttributeFields,
    ) -> Result<PageTableEntry, &'static str> {
        if output_addr % Size4KiB::SIZE as usize != 0 {
            return Err("PageDescriptor: Address is not 4 KiB aligned.");
        }

        let shifted = output_addr >> Size4KiB::SHIFT;

        Ok(PageTableEntry::PageDescriptor(
            STAGE1_DESCRIPTOR::VALID::True
                + STAGE1_DESCRIPTOR::AF::True
                + into_mmu_attributes(attribute_fields)
                + STAGE1_DESCRIPTOR::TYPE::Table
                + STAGE1_DESCRIPTOR::NEXT_LVL_TABLE_ADDR_4KiB.val(shifted as u64),
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
            PageTableEntry::TableDescriptor(x)
            | PageTableEntry::Lvl2BlockDescriptor(x)
            | PageTableEntry::PageDescriptor(x) => x.value,
        }
    }
}

static mut LVL2_TABLE: Table<PageDirectory> = Table::<PageDirectory> {
    entries: [0; NUM_ENTRIES_4KIB as usize],
    level: PhantomData,
};

static mut LVL3_TABLE: Table<PageTable> = Table::<PageTable> {
    entries: [0; NUM_ENTRIES_4KIB as usize],
    level: PhantomData,
};

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
    mair::set_up();

    // Point the first 2 MiB of virtual addresses to the follow-up LVL3
    // page-table.
    LVL2_TABLE.entries[0] =
        PageTableEntry::new_table_descriptor(LVL3_TABLE.entries.base_addr_usize())?.into();

    // Fill the rest of the LVL2 (2 MiB) entries as block descriptors.
    //
    // Notice the skip(1) which makes the iteration start at the second 2 MiB
    // block (0x20_0000).
    for (block_descriptor_nr, entry) in LVL2_TABLE.entries.iter_mut().enumerate().skip(1) {
        let virt_addr = block_descriptor_nr << Size2MiB::SHIFT;

        let (output_addr, attribute_fields) = match get_virt_addr_properties(virt_addr) {
            Err(s) => return Err(s),
            Ok((a, b)) => (a, b),
        };

        let block_desc =
            match PageTableEntry::new_lvl2_block_descriptor(output_addr, attribute_fields) {
                Err(s) => return Err(s),
                Ok(desc) => desc,
            };

        *entry = block_desc.into();
    }

    // Finally, fill the single LVL3 table (4 KiB granule).
    for (page_descriptor_nr, entry) in LVL3_TABLE.entries.iter_mut().enumerate() {
        let virt_addr = page_descriptor_nr << Size4KiB::SHIFT;

        let (output_addr, attribute_fields) = match get_virt_addr_properties(virt_addr) {
            Err(s) => return Err(s),
            Ok((a, b)) => (a, b),
        };

        let page_desc = match PageTableEntry::new_page_descriptor(output_addr, attribute_fields) {
            Err(s) => return Err(s),
            Ok(desc) => desc,
        };

        *entry = page_desc.into();
    }

    // Point to the LVL2 table base address in TTBR0.
    TTBR0_EL1.set_baddr(LVL2_TABLE.entries.base_addr_u64()); // User (lo-)space addresses

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
