/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! MMU initialisation.
//!
//! Paging is mostly based on [previous version](https://os.phil-opp.com/page-tables/) of
//! Phil Opp's [paging guide](https://os.phil-opp.com/paging-implementation/) and
//! [ARMv8 ARM memory addressing](https://static.docs.arm.com/100940/0100/armv8_a_address%20translation_100940_0100_en.pdf).
//! It includes ideas from Sergio Benitez' cs140e OSdev course material on type-safe access.

#![allow(dead_code)]

use {
    crate::memory::{
        page_size::{Size1GiB, Size2MiB, Size4KiB},
        PageSize,
        //virt_page::Page,
        PhysAddr,
        PhysFrame,
        VirtAddr,
    },
    core::{
        marker::PhantomData,
        ops::{Index, IndexMut},
        ptr::Unique,
    },
    cortex_a::barrier,
    register::register_bitfields,
    snafu::Snafu,
};

#[derive(Debug, Snafu)]
enum MmuError {}

pub fn init() -> Result<(), MmuError> {
    // Prepare the memory attribute indirection register.
    mair::set_up();

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
    unsafe {
        barrier::isb(barrier::SY);
    }

    // use cortex_a::regs::RegisterReadWrite;
    // Enable the MMU and turn on data and instruction caching.
    SCTLR_EL1.modify(SCTLR_EL1::M::Enable + SCTLR_EL1::C::Cacheable + SCTLR_EL1::I::Cacheable);

    // Force MMU init to complete before next instruction
    /*
     * Invalidate the local I-cache so that any instructions fetched
     * speculatively from the PoC are discarded, since they may have
     * been dynamically patched at the PoU.
     */
    unsafe {
        barrier::isb(barrier::SY);
    }

    Ok(())
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
 *    Lv1:   7FC0000000
 *    off:     3FFFFFFF       1G
 *    Lv2:     3FE00000
 *    off:       1FFFFF       2M
 *    Lv3:       1FF000
 *    off:          FFF       4K
 *
 * RPi3 supports 64K and 4K granules, also 40-bit physical addresses.
 * It also can address only 1G physical memory, so these 40-bit phys addresses are a fake.
 *
 * 48-bit virtual address space; different mappings in VBAR0 (EL0) and VBAR1 (EL1+).
 */

register_bitfields! {
    u64,
    VA_INDEX [
        LEVEL0 OFFSET(39) NUMBITS(9) [],
        LEVEL1 OFFSET(30) NUMBITS(9) [],
        LEVEL2 OFFSET(21) NUMBITS(9) [],
        LEVEL3 OFFSET(12) NUMBITS(9) [],
        OFFSET OFFSET(0) NUMBITS(12) []
    ]
}

register_bitfields! {
    u64,
    // AArch64 Reference Manual page 2150, D5-2445
    TABLE_DESCRIPTOR [
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

        /// Share-ability field
        SH OFFSET(8) NUMBITS(2) [
            OuterShareable = 0b10,
            InnerShareable = 0b11
        ],

        /// Access Permissions
        AP OFFSET(6) NUMBITS(2) [
            RW_EL1 = 0b00,
            RW_EL1_EL0 = 0b01,
            RO_EL1 = 0b10,
            RO_EL1_EL0 = 0b11
        ],

        NS_EL3 OFFSET(5) NUMBITS(1) [],

        /// Memory attributes index into the MAIR_EL1 register
        AttrIndx OFFSET(2) NUMBITS(3) [],

        TYPE OFFSET(1) NUMBITS(1) [
            Block = 0,
            Table = 1
        ],

        VALID OFFSET(0) NUMBITS(1) [
            False = 0,
            True = 1
        ]
    ]
}

// type VaIndex = register::FieldValue<u64, VA_INDEX::Register>;
type VaType = register::LocalRegisterCopy<u64, VA_INDEX::Register>;
type EntryFlags = register::FieldValue<u64, TABLE_DESCRIPTOR::Register>;
type EntryRegister = register::LocalRegisterCopy<u64, TABLE_DESCRIPTOR::Register>;

// Possible mappings:
// * TTBR0 pointing to user page global directory
// * TTBR0 pointing to user page upper directory (only if mmu is set up differently)
// * TTBR1 pointing to kernel page global directory with full physmem access

// * Paging system uses a separate address space in top kernel region (TTBR1) to access
// * entire physical memory contents.
// * This mapping is not available to user space (user space uses TTBR0).
// *
// * Use the largest possible granule size to map physical memory since we want to use
// * the least amount of memory for these mappings.

// TTBR0 Page Global Directory

// Level 0 descriptors can only output the address of a Level 1 table.
// Level 3 descriptors cannot point to another table and can only output block addresses.
// The format of the table is therefore slightly different for Level 3.
//
// this means:
// - in level 0 page table can be only TableDescriptors
// - in level 1,2 page table can be TableDescriptors, Lvl2BlockDescriptors (PageDescriptors)
// - in level 3 page table can be only PageDescriptors

// Level / Types | Table Descriptor | Lvl2BlockDescriptor (PageDescriptor)
// --------------+------------------+--------------------------------------
//   0           |        X         |                       (with 4KiB granule)
//   1           |        X         |          X            (1GiB range)
//   2           |        X         |          X            (2MiB range)
//   3           |                  |          X            (4KiB range) -- called PageDescriptor
//                                                          encoding actually the same as in Table Descriptor

// Translation granule affects the size of the block addressed.
// Lets use 4KiB granule on RPi3 for simplicity.

// 1, set 4KiB granule size to use the PGD - we could use 16KiB granule instead?
//                                        - need to measure waste level
//                                        - but lets stick with 4KiB for now
//

// If I have, for example, Table<Level0> I can get from it N `Table<Level1>` (via impl HierarchicalTable)
// From Table<Level1> I can get either `Table<Level2>` (via impl HierarchicalTable) or `BlockDescriptor<Size1GiB>`
// From Table<Level2> I can get either `Table<Level3>` (via impl HierarchicalTable) or `BlockDescriptor<Size2MiB>`
// From Table<Level3> I can only get `PageDescriptor<Size4KiB>` (because no impl HierarchicalTable exists)

/// GlobalDirectory [ UpperDirectory entries ]
/// UpperDirectory [ PageDirectory | GiantPage ]
/// PageDirectory [ PageTable | LargePage ]
/// PageTable [ PageFrames ]

// do those as separate types, then in accessors allow only certain combinations
// e.g.
// struct UpperDirectoryEntry; // DirectoryEntry<L0>
// struct PageDirectoryEntry; // DirectoryEntry<L1>
// struct GiantPageFrame; // PageFrame<Size1GiB>
// struct PageTableEntry; // DirectoryEntry<L2>
// struct LargePageFrame; // PageFrame<Size2MiB>
// struct PageFrame; // PageFrame<Size4KiB>

// enum PageTableEntry { Page(&mut PageDescriptor), Block(&mut BlockDescriptor), Etc(&mut u64), Invalid(&mut u64) }
// impl PageTabelEntry { fn new_from_entry_addr(&u64) }
// return enum PageTableEntry constructed from table bits in u64

enum L0Entries {
    UpperDirectoryEntry(VirtAddr),
}
enum L1Entries {
    PageDirectoryEntry(VirtAddr),
    GiantPageFrame(PhysFrame<Size1GiB>),
}
enum L2Entries {
    PageTableEntry(VirtAddr),
    LargePageFrame(PhysFrame<Size2MiB>),
}
enum L3Entries {
    PageFrame(PhysFrame<Size4KiB>),
}

enum Frames {
    GiantPageFrame,
    LargePageFrame,
    PageFrame,
}

// ----
// ----
// ---- Table levels
// ----
// ----

/// L0 table -- only pointers to L1 tables
pub enum L0PageGlobalDirectory {}
/// L1 tables -- pointers to L2 tables or giant 1GiB pages
pub enum L1PageUpperDirectory {}
/// L2 tables -- pointers to L3 tables or huge 2MiB pages
pub enum L2PageDirectory {}
/// L3 tables -- only pointers to 4/16KiB pages
pub enum L3PageTable {}

/// Shared trait for specific table levels.
pub trait TableLevel {}

/// Shared trait for hierarchical table levels.
///
/// Specifies what is the next level of page table hierarchy.
pub trait HierarchicalLevel: TableLevel {
    /// Level of the next translation table below this one.
    type NextLevel: TableLevel;

    // fn translate() -> Directory<NextLevel>;
}

/// Specify allowed page size for each level.
pub trait HierarchicalPageLevel: TableLevel {
    /// Size of the page that can be contained in this table level.
    type PageLevel: PageSize;
}

impl TableLevel for L0PageGlobalDirectory {}
impl TableLevel for L1PageUpperDirectory {}
impl TableLevel for L2PageDirectory {}
impl TableLevel for L3PageTable {}

impl HierarchicalLevel for L0PageGlobalDirectory {
    type NextLevel = L1PageUpperDirectory;
}
impl HierarchicalLevel for L1PageUpperDirectory {
    type NextLevel = L2PageDirectory;
}
impl HierarchicalLevel for L2PageDirectory {
    type NextLevel = L3PageTable;
}
// L3PageTables do not have next level, therefore they are not HierarchicalLevel

// L0PageGlobalDirectory does not contain pages, so they are not HierarchicalPageLevel
impl HierarchicalPageLevel for L1PageUpperDirectory {
    type PageLevel = Size1GiB;
}
impl HierarchicalPageLevel for L2PageDirectory {
    type PageLevel = Size2MiB;
}
impl HierarchicalPageLevel for L3PageTable {
    type PageLevel = Size4KiB;
}

// ----
// ----
// ---- Directory
// ----
// ----

// Maximum OA is 48 bits.
//
// Level 0 table descriptor has Output Address in [47:12] --> level 1 table
// Level 0 descriptor cannot be block descriptor.
//
// Level 1 table descriptor has Output Address in [47:12] --> level 2 table
// Level 1 block descriptor has Output Address in [47:30]
//
// Level 2 table descriptor has Output Address in [47:12] --> level 3 table
// Level 2 block descriptor has Output Address in [47:21]
//
// Level 3 block descriptor has Output Address in [47:12]
// Upper Attributes [63:51]
// Res0 [50:48]
// Lower Attributes [11:2]
// 11b [1:0]

// Each table consists of 2**9 entries
const TABLE_BITS: usize = 9;
const INDEX_MASK: usize = (1 << TABLE_BITS) - 1;

static_assertions::const_assert!(INDEX_MASK == 0x1ff);

// @todo Table in mmu.rs
/// MMU address translation table.
/// Contains just u64 internally, provides enum interface on top
#[repr(C)]
#[repr(align(4096))]
struct Directory<Level: TableLevel> {
    entries: [u64; 1 << TABLE_BITS],
    level: PhantomData<Level>,
}

impl Directory<L0PageGlobalDirectory> {
    fn next(&self, address: VirtAddr) -> Option<L0Entries> {
        let va = VaType::new(address.as_u64());
        let index = va.read(VA_INDEX::LEVEL0);
        match self.next_table_address(index as usize) {
            Some(phys_addr) => Some(L0Entries::UpperDirectoryEntry(phys_addr.user_to_kernel())),
            None => None,
        }
    }
}

impl Directory<L1PageUpperDirectory> {
    fn next(&self, address: VirtAddr) -> Option<L1Entries> {
        let va = VaType::new(address.as_u64());
        let index = va.read(VA_INDEX::LEVEL1);
        match self.next_table_address(index as usize) {
            Some(phys_addr) => Some(L1Entries::PageDirectoryEntry(phys_addr.user_to_kernel())),
            None => None, // @todo could be 1GiB frame
        }
    }
}

impl Directory<L2PageDirectory> {
    fn next(&self, address: VirtAddr) -> Option<L2Entries> {
        let va = VaType::new(address.as_u64());
        let index = va.read(VA_INDEX::LEVEL2);
        match self.next_table_address(index as usize) {
            Some(phys_addr) => Some(L2Entries::PageTableEntry(phys_addr.user_to_kernel())),
            None => None, // @todo could be 2MiB frame
        }
    }
}

impl Directory<L3PageTable> {
    fn next(&self, address: VirtAddr) -> Option<L3Entries> {
        let va = VaType::new(address.as_u64());
        let _index = va.read(VA_INDEX::LEVEL3);
        // @fixme wrong function
        // match self.next_table_address(index as usize) {
        //     Some(phys_addr) => Some(L3Entries::PageFrame(phys_addr.user_to_kernel())),
        //     None => None, // Nothing there
        // }
        None
    }
}

// Implementation code shared for all levels of page tables
impl<Level> Directory<Level>
where
    Level: TableLevel,
{
    /// Construct a zeroed table at given physical location.
    // unsafe fn at(location: PhysAddr) -> &Self {}

    /// Construct and return zeroed table.
    fn zeroed() -> Self {
        Self {
            entries: [0; 1 << TABLE_BITS],
            level: PhantomData,
        }
    }

    /// Zero out entire table.
    pub fn zero(&mut self) {
        for entry in self.entries.iter_mut() {
            *entry = 0;
        }
    }
}

impl<Level> Index<usize> for Directory<Level>
where
    Level: TableLevel,
{
    type Output = u64;

    fn index(&self, index: usize) -> &Self::Output {
        &self.entries[index]
    }
}

impl<Level> IndexMut<usize> for Directory<Level>
where
    Level: TableLevel,
{
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.entries[index]
    }
}

impl<Level> Directory<Level>
where
    Level: HierarchicalLevel,
{
    fn next_table_address(&self, index: usize) -> Option<PhysAddr> {
        let entry_flags = EntryRegister::new(self[index]);
        // If table entry has 0b11 mask set, it is a valid table entry.
        // Address of the following table may be extracted from bits 47:12
        if entry_flags.matches_all(TABLE_DESCRIPTOR::VALID::True + TABLE_DESCRIPTOR::TYPE::Table) {
            Some(PhysAddr::new(
                entry_flags.read(TABLE_DESCRIPTOR::NEXT_LVL_TABLE_ADDR_4KiB) << Size4KiB::SHIFT,
            ))
        } else {
            None
        }
    }

    pub fn next_table(&self, index: usize) -> Option<&Directory<Level::NextLevel>> {
        self.next_table_address(index)
            .map(|address| unsafe { &*(address.user_to_kernel().as_ptr()) })
    }

    pub fn next_table_mut(&mut self, index: usize) -> Option<&mut Directory<Level::NextLevel>> {
        self.next_table_address(index)
            .map(|address| unsafe { &mut *(address.user_to_kernel().as_mut_ptr()) })
    }

    pub fn translate_levels(&self, _address: VirtAddr) -> Option<Frames> {
        None
    }
}

// ----
// ----
// ---- VirtSpace
// ----
// ----

/// Errors from mapping layer
#[derive(Debug, Snafu)]
pub enum TranslationError {
    /// No page found. @todo
    NoPage,
}

/// Virtual address space. @todo
pub struct VirtSpace {
    l0: Unique<Directory<L0PageGlobalDirectory>>,
}

// translation steps:
// l0: upper page directory or Err()
// l1: lower page directory or 1Gb aperture or Err()
// l2: page table or 2MiB aperture or Err()
// l3: 4KiB aperture or Err()

impl VirtSpace {
    // Translate translates address all the way down to physical address or error.
    // On each level there's next_table() fn that resolves to the next level table if possible.
    // pub fn translate(&self, virtual_address: VirtAddr) -> Result<PhysAddr, TranslationError> {
    //     // let offset = virtual_address % Self::PageLevel::SIZE as usize; // use the size of the last page?
    //     self.translate_page(Page::<Self::PageLevel>::containing_address(virtual_address))?
    //         .map(|frame, offset| frame.start_address() + offset)
    // }
}

// pageglobaldirectory.translate() {
//     get page index <- generic over page level (xx << (10 + (3 - level) * 9))
//     return page[index]?.translate(rest);
// }

#[cfg(test)]
mod tests {
    use super::*;

    #[test_case]
    fn table_construction() {
        let mut level0_table = Directory::<L0PageGlobalDirectory>::zeroed();
        let level1_table = Directory::<L1PageUpperDirectory>::zeroed();
        let level2_table = Directory::<L2PageDirectory>::zeroed();
        let level3_table = Directory::<L3PageTable>::zeroed();

        assert!(level0_table.next_table_address(0).is_none());

        // Make entry map to a level1 table
        level0_table[0] = EntryFlags::from(
            TABLE_DESCRIPTOR::VALID::True
                + TABLE_DESCRIPTOR::TYPE::Table
                + TABLE_DESCRIPTOR::NEXT_LVL_TABLE_ADDR_4KiB.val(0x424242),
        )
        .into();

        assert!(level0_table.next_table_address(0).is_some());

        let addr = level0_table.next_table_address(0).unwrap();
        assert_eq!(addr, (0x424242 << 12));
    }
}
