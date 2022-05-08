use {
    crate::{
        memory::mmu::{
            arch_mmu::{mair, Granule512MiB, Granule64KiB},
            AccessPermissions, AttributeFields, MemAttributes,
        },
        platform,
    },
    core::convert,
    tock_registers::{
        interfaces::{Readable, Writeable},
        register_bitfields,
        registers::InMemoryRegister,
    },
};

//--------------------------------------------------------------------------------------------------
// Private Definitions
//--------------------------------------------------------------------------------------------------

register_bitfields! {
    u64,
    // AArch64 Reference Manual page 2150, D5-2445
    STAGE1_TABLE_DESCRIPTOR [
        /// Physical address of the next descriptor.
        NEXT_LEVEL_TABLE_ADDR_64KiB OFFSET(16) NUMBITS(32) [], // [47:16]
        NEXT_LEVEL_TABLE_ADDR_4KiB OFFSET(12) NUMBITS(36) [], // [47:12]

        TYPE  OFFSET(1) NUMBITS(1) [
            Block = 0,
            Table = 1
        ],

        VALID OFFSET(0) NUMBITS(1) [
            False = 0,
            True = 1
        ]
    ]
}

register_bitfields! {
    u64,
    // AArch64 Reference Manual page 2150, D5-2445
    STAGE1_PAGE_DESCRIPTOR [
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

        /// Physical address of the next table descriptor (lvl2) or the page descriptor (lvl3).
        LVL2_OUTPUT_ADDR_64KiB   OFFSET(16) NUMBITS(32) [], // [47:16]
        LVL2_OUTPUT_ADDR_4KiB    OFFSET(21) NUMBITS(27) [], // [47:21]

        /// Access flag
        AF       OFFSET(10) NUMBITS(1) [
            NotAccessed = 0,
            Accessed = 1
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

        /// Memory attributes index into the MAIR_EL1 register
        AttrIndx OFFSET(2) NUMBITS(3) [],

        TYPE     OFFSET(1) NUMBITS(1) [
            Reserved_Invalid = 0,
            Page = 1
        ],

        VALID    OFFSET(0) NUMBITS(1) [
            False = 0,
            True = 1
        ]
    ]
}

/// A table descriptor with 64 KiB aperture.
///
/// The output points to the next table.
#[derive(Copy, Clone)]
#[repr(C)]
struct TableDescriptor {
    value: u64,
}

/// A page descriptor with 64 KiB aperture.
///
/// The output points to physical memory.
#[derive(Copy, Clone)]
#[repr(C)]
struct PageDescriptor {
    value: u64,
}

trait BaseAddr {
    fn base_addr_u64(&self) -> u64;
    fn base_addr_usize(&self) -> usize;
}

const NUM_LVL2_TABLES: usize = platform::memory::mmu::KernelAddrSpace::SIZE >> Granule512MiB::SHIFT;

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// Big monolithic struct for storing the translation tables. Individual levels must be 64 KiB
/// aligned, so the lvl3 is put first.
#[repr(C)]
#[repr(align(65536))]
pub struct FixedSizeTranslationTable<const NUM_TABLES: usize> {
    /// Page descriptors, covering 64 KiB windows per entry.
    lvl3: [[PageDescriptor; 8192]; NUM_TABLES],

    /// Table descriptors, covering 512 MiB windows.
    lvl2: [TableDescriptor; NUM_TABLES],
}

/// A translation table type for the kernel space.
pub type KernelTranslationTable = FixedSizeTranslationTable<NUM_LVL2_TABLES>;

//--------------------------------------------------------------------------------------------------
// Private Implementations
//--------------------------------------------------------------------------------------------------

// The binary is still identity mapped, so we don't need to convert here.
impl<T, const N: usize> BaseAddr for [T; N] {
    fn base_addr_u64(&self) -> u64 {
        self as *const T as u64
    }

    fn base_addr_usize(&self) -> usize {
        self as *const T as usize
    }
}

impl TableDescriptor {
    /// Create an instance.
    ///
    /// Descriptor is invalid by default.
    pub const fn new_zeroed() -> Self {
        Self { value: 0 }
    }

    /// Create an instance pointing to the supplied address.
    pub fn from_next_lvl_table_addr(phys_next_lvl_table_addr: usize) -> Self {
        let val = InMemoryRegister::<u64, STAGE1_TABLE_DESCRIPTOR::Register>::new(0);

        let shifted = phys_next_lvl_table_addr >> Granule64KiB::SHIFT;
        val.write(
            STAGE1_TABLE_DESCRIPTOR::NEXT_LEVEL_TABLE_ADDR_64KiB.val(shifted as u64)
                + STAGE1_TABLE_DESCRIPTOR::TYPE::Table
                + STAGE1_TABLE_DESCRIPTOR::VALID::True,
        );

        TableDescriptor { value: val.get() }
    }
}

impl PageDescriptor {
    /// Create an instance.
    ///
    /// Descriptor is invalid by default.
    pub const fn new_zeroed() -> Self {
        Self { value: 0 }
    }

    /// Create an instance.
    pub fn from_output_addr(phys_output_addr: usize, attribute_fields: &AttributeFields) -> Self {
        let val = InMemoryRegister::<u64, STAGE1_PAGE_DESCRIPTOR::Register>::new(0);

        let shifted = phys_output_addr as u64 >> Granule64KiB::SHIFT;
        val.write(
            STAGE1_PAGE_DESCRIPTOR::LVL2_OUTPUT_ADDR_64KiB.val(shifted)
                + STAGE1_PAGE_DESCRIPTOR::AF::Accessed
                + STAGE1_PAGE_DESCRIPTOR::TYPE::Page
                + STAGE1_PAGE_DESCRIPTOR::VALID::True
                + (*attribute_fields).into(),
        );

        Self { value: val.get() }
    }
}

/// Convert the kernel's generic memory attributes to HW-specific attributes of the MMU.
impl convert::From<AttributeFields>
    for tock_registers::fields::FieldValue<u64, STAGE1_PAGE_DESCRIPTOR::Register>
{
    fn from(attribute_fields: AttributeFields) -> Self {
        // Memory attributes
        let mut desc = match attribute_fields.mem_attributes {
            MemAttributes::CacheableDRAM => {
                STAGE1_PAGE_DESCRIPTOR::SH::InnerShareable
                    + STAGE1_PAGE_DESCRIPTOR::AttrIndx.val(mair::attr::NORMAL)
            }
            MemAttributes::NonCacheableDRAM => {
                STAGE1_PAGE_DESCRIPTOR::SH::InnerShareable
                    + STAGE1_PAGE_DESCRIPTOR::AttrIndx.val(mair::attr::NORMAL_NON_CACHEABLE)
            }
            MemAttributes::Device => {
                STAGE1_PAGE_DESCRIPTOR::SH::OuterShareable
                    + STAGE1_PAGE_DESCRIPTOR::AttrIndx.val(mair::attr::DEVICE_NGNRE)
            }
        };

        // Access Permissions
        desc += match attribute_fields.acc_perms {
            AccessPermissions::ReadOnly => STAGE1_PAGE_DESCRIPTOR::AP::RO_EL1,
            AccessPermissions::ReadWrite => STAGE1_PAGE_DESCRIPTOR::AP::RW_EL1,
        };

        // The execute-never attribute is mapped to PXN in AArch64.
        desc += if attribute_fields.execute_never {
            STAGE1_PAGE_DESCRIPTOR::PXN::NeverExecute
        } else {
            STAGE1_PAGE_DESCRIPTOR::PXN::Execute
        };

        // Always set unprivileged execute-never as long as userspace is not implemented yet.
        desc += STAGE1_PAGE_DESCRIPTOR::UXN::NeverExecute;

        desc
    }
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

impl<const NUM_TABLES: usize> FixedSizeTranslationTable<NUM_TABLES> {
    /// Create an instance.
    pub const fn new() -> Self {
        // Can't have a zero-sized address space.
        assert!(NUM_TABLES > 0);

        Self {
            lvl3: [[PageDescriptor::new_zeroed(); 8192]; NUM_TABLES],
            lvl2: [TableDescriptor::new_zeroed(); NUM_TABLES],
        }
    }

    /// Iterates over all static translation table entries and fills them at once.
    ///
    /// See also: https://armv8-ref.codingbelief.com/en/chapter_d4/d4_the_aarch64_virtual_memory_system_archi.html
    ///
    /// # Safety
    ///
    /// - Modifies a `static mut`. Ensure it only happens from here.
    pub unsafe fn populate_translation_table_entries(&mut self) -> Result<(), &'static str> {
        for (l2_nr, l2_entry) in self.lvl2.iter_mut().enumerate() {
            *l2_entry =
                TableDescriptor::from_next_lvl_table_addr(self.lvl3[l2_nr].base_addr_usize());

            for (l3_nr, l3_entry) in self.lvl3[l2_nr].iter_mut().enumerate() {
                let virt_addr = (l2_nr << Granule512MiB::SHIFT) + (l3_nr << Granule64KiB::SHIFT);

                let (phys_output_addr, attribute_fields) =
                    platform::memory::mmu::virt_mem_layout().virt_addr_properties(virt_addr)?;

                *l3_entry = PageDescriptor::from_output_addr(phys_output_addr, &attribute_fields);
            }
        }

        Ok(())
    }

    /// The translation table's base address to be used for programming the MMU.
    pub fn phys_base_address(&self) -> u64 {
        self.lvl2.base_addr_u64()
    }
}

//--------------------------------------------------------------------------------------------------
// wait: my extended code
//--------------------------------------------------------------------------------------------------

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
                + STAGE1_DESCRIPTOR::AF::Enabled
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
                + STAGE1_DESCRIPTOR::AF::Enabled
                + STAGE1_DESCRIPTOR::TYPE::Block
                + STAGE1_DESCRIPTOR::LVL2_OUTPUT_ADDR_4KiB.val(shifted as u64)
                + attribute_fields.into(),
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
                + STAGE1_DESCRIPTOR::AF::Enabled
                + STAGE1_DESCRIPTOR::TYPE::Table
                + STAGE1_DESCRIPTOR::NEXT_LVL_TABLE_ADDR_4KiB.val(shifted as u64)
                + attribute_fields.into(),
        ))
    }
}

impl From<u64> for PageTableEntry {
    fn from(_val: u64) -> PageTableEntry {
        // xx00 -> Invalid
        // xx10 -> Block Entry in L1 and L2
        // xx11 -> TableDescriptor in L0, L1 and L2
        // xx11 -> PageDescriptor in L3
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

static mut LVL1_TABLE: Table<PageUpperDirectory> = Table::<PageUpperDirectory> {
    entries: [0; NUM_ENTRIES_4KIB as usize],
    level: PhantomData,
};

static mut LVL2_TABLE: Table<PageDirectory> = Table::<PageDirectory> {
    entries: [0; NUM_ENTRIES_4KIB as usize],
    level: PhantomData,
};

static mut LVL3_TABLE: Table<PageTable> = Table::<PageTable> {
    entries: [0; NUM_ENTRIES_4KIB as usize],
    level: PhantomData,
};
