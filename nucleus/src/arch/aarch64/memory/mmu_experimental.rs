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
        barrier,
        regs::{ID_AA64MMFR0_EL1, SCTLR_EL1, TCR_EL1, TTBR0_EL1},
    },
    register::{
        cpu::{RegisterReadOnly, RegisterReadWrite},
        register_bitfields,
    },
    // ux::*,
};

mod mair {
    use cortex_a::regs::MAIR_EL1;

    /// Setup function for the MAIR_EL1 register.
    pub fn set_up() {
        use cortex_a::regs::RegisterReadWrite;

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

/// A function that maps the generic memory range attributes to HW-specific
/// attributes of the MMU.
fn into_mmu_attributes(
    attribute_fields: AttributeFields,
) -> register::FieldValue<u64, STAGE1_DESCRIPTOR::Register> {
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

#[derive(Snafu, Debug)]
enum PageTableError {
    #[snafu(display("BlockDescriptor: Address is not 2 MiB aligned."))]
    //"PageDescriptor: Address is not 4 KiB aligned."
    NotAligned(&'static str),
}

/// A Level2 block descriptor with 2 MiB aperture.
///
/// The output points to physical memory.
// struct Lvl2BlockDescriptor(register::FieldValue<u64, STAGE1_DESCRIPTOR::Register>);

impl PageTableEntry {
    fn new_lvl2_block_descriptor(
        output_addr: usize,
        attribute_fields: AttributeFields,
    ) -> Result<PageTableEntry, PageTableError> {
        if output_addr % Size2MiB::SIZE as usize != 0 {
            return Err(PageTableError::NotAligned(Size2MiB::SIZE_AS_DEBUG_STR));
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
    ) -> Result<PageTableEntry, PageTableError> {
        if output_addr % Size4KiB::SIZE as usize != 0 {
            return Err(PageTableError::NotAligned(Size4KiB::SIZE_AS_DEBUG_STR));
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

// to get L0 we must allocate a few frames from boot region allocator.
// So, first we init the dtb, parse mem-regions from there, then init boot_info page and start mmu,
// this part will be inited in mmu::init():

// // @todo do NOT keep these statically, always allocate from available bump memory
// static mut LVL2_TABLE: Table<PageDirectory> = Table::<PageDirectory> {
//     entries: [0; NUM_ENTRIES_4KIB as usize],
//     level: PhantomData,
// };
//
// // @todo do NOT keep these statically, always allocate from available bump memory
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
    mair::set_up();

    // should receive in args an obtained memory map from DT
    let memory_map = Regions {
        start: 0x1000,
        size: 0x10000,
    };

    // bump-allocate page tables for entire memory
    // also allocate phys memory to kernel space!
    //
    // separate regions - regular memory, device mmaps,
    // initial thread maps ALL the memory??
    // instead
    // init thread may map only necessary mem
    // boot time only map kernel physmem space, and currently loaded kernel data
    // PROBABLY only kernel mapping TTBR1 is needed, the rest is not very useful?
    // take over protected memory space though anyway.

    // Point the first 2 MiB of virtual addresses to the follow-up LVL3
    // page-table.
    // LVL2_TABLE.entries[0] =
    //     PageTableEntry::new_table_descriptor(LVL3_TABLE.entries.base_addr_usize())?.into();

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
}

// AArch64:
// Table D4-8-2021: check supported granule sizes, select alloc policy based on results.
// TTBR_ELx is the pdbr for specific page tables

// Page 2068 actual page descriptor formats

// Pointer to currently active page table
// Could be either user space (TTBR0) or kernel space (TTBR1) -- ??
pub struct ActivePageTable {
    l0: Unique<Table<PageGlobalDirectory>>,
}

impl ActivePageTable {
    pub unsafe fn new() -> ActivePageTable {
        ActivePageTable {
            l0: Unique::new_unchecked(0 as *mut _),
        }
    }

    fn l0(&self) -> &Table<PageGlobalDirectory> {
        unsafe { self.l0.as_ref() }
    }

    fn l0_mut(&mut self) -> &mut Table<PageGlobalDirectory> {
        unsafe { self.l0.as_mut() }
    }

    // pub fn translate(&self, virtual_address: VirtAddr) -> Result<PhysAddr, TranslationError> {
    //     let offset = virtual_address % Size4KiB::SIZE as usize; // @todo use the size of the last page of course
    //     self.translate_page(Page::containing_address(virtual_address))?
    //         .map(|frame| frame.start_address() + offset)
    // }

    fn translate_page(&self, page: Page) -> Result<PhysFrame, TranslationError> {
        // @todo translate only one level of hierarchy per impl function...
        let l1 = self.l0().next_table(u64::from(page.l0_index()) as usize);
        /*
                let huge_page = || {
                    l1.and_then(|l1| {
                        let l1_entry = &l1[page.l1_index() as usize];
                        // 1GiB page?
                        if let Some(start_frame) = l1_entry.pointed_frame() {
                            if l1_entry.flags().read(STAGE1_DESCRIPTOR::TYPE)
                                != STAGE1_DESCRIPTOR::TYPE::Table.value
                            {
                                // address must be 1GiB aligned
                                //start_frame.is_aligned()
                                assert!(start_frame.number % (NUM_ENTRIES_4KIB * NUM_ENTRIES_4KIB) == 0);
                                return Ok(PhysFrame::from_start_address(
                                    start_frame.number
                                        + page.l2_index() * NUM_ENTRIES_4KIB
                                        + page.l3_index(),
                                ));
                            }
                        }
                        if let Some(l2) = l1.next_table(page.l1_index()) {
                            let l2_entry = &l2[page.l2_index()];
                            // 2MiB page?
                            if let Some(start_frame) = l2_entry.pointed_frame() {
                                if l2_entry.flags().read(STAGE1_DESCRIPTOR::TYPE)
                                    != STAGE1_DESCRIPTOR::TYPE::Table
                                {
                                    // address must be 2MiB aligned
                                    assert!(start_frame.number % NUM_ENTRIES_4KIB == 0);
                                    return Ok(PhysFrame::from_start_address(
                                        start_frame.number + page.l3_index(),
                                    ));
                                }
                            }
                        }
                        Err(TranslationError::NoPage)
                    })
                };
        */
        let v = l1
            .and_then(|l1| l1.next_table(u64::from(page.l1_index()) as usize))
            .and_then(|l2| l2.next_table(u64::from(page.l2_index()) as usize))
            .and_then(|l3| Some(l3[u64::from(page.l3_index()) as usize])); //.pointed_frame())
                                                                           //            .ok_or(TranslationError::NoPage)
                                                                           // .or_else(huge_page)
        Ok(v.unwrap().into())
    }

    pub fn map_to<A>(&mut self, page: Page, frame: PhysFrame, flags: EntryFlags, allocator: &mut A)
    where
        A: FrameAllocator,
    {
        let l0 = self.l0_mut();
        let l1 = l0.next_table_create(u64::from(page.l0_index()) as usize, allocator);
        let l2 = l1.next_table_create(u64::from(page.l1_index()) as usize, allocator);
        let l3 = l2.next_table_create(u64::from(page.l2_index()) as usize, allocator);

        assert_eq!(
            l3[u64::from(page.l3_index()) as usize],
            0 /*.is_unused()*/
        );
        l3[u64::from(page.l3_index()) as usize] = PageTableEntry::PageDescriptor(
            STAGE1_DESCRIPTOR::NEXT_LVL_TABLE_ADDR_4KiB.val(u64::from(frame))
                + flags // @todo properly extract flags
                + STAGE1_DESCRIPTOR::VALID::True,
        )
        .into();
    }

    pub fn map<A>(&mut self, page: Page, flags: EntryFlags, allocator: &mut A)
    where
        A: FrameAllocator,
    {
        // @todo fail mapping if table is not allocated, causing client to allocate and restart
        // @todo problems described in preso - chicken&egg problem of allocating first allocations
        let frame = allocator.allocate_frame().expect("out of memory");
        self.map_to(page, frame, flags, allocator)
    }

    pub fn identity_map<A>(&mut self, frame: PhysFrame, flags: EntryFlags, allocator: &mut A)
    where
        A: FrameAllocator,
    {
        let page = Page::containing_address(VirtAddr::new(frame.start_address().as_u64()));
        self.map_to(page, frame, flags, allocator)
    }

    fn unmap<A>(&mut self, page: Page, _allocator: &mut A)
    where
        A: FrameAllocator,
    {
        // use aarch64::instructions::tlb;
        // use x86_64::VirtAddr;

        assert!(self.translate(page.start_address()).is_ok());

        let l3 = self
            .l0_mut()
            .next_table_mut(u64::from(page.l0_index()) as usize)
            .and_then(|l1| l1.next_table_mut(u64::from(page.l1_index()) as usize))
            .and_then(|l2| l2.next_table_mut(u64::from(page.l2_index()) as usize))
            .expect("mapping code does not support huge pages");
        let _frame = l3[u64::from(page.l3_index()) as usize];
        //            .pointed_frame()
        //            .unwrap();
        l3[u64::from(page.l3_index()) as usize] = 0; /*.set_unused(); */
        // tlb::flush(VirtAddr(page.start_address()));
        // TODO free p(1,2,3) table if empty
        //allocator.deallocate_frame(frame);
        // @todo do NOT deallocate frames either, but need to signal client that it's unused
    }
}

// Abstractions for page table entries.

/// The error returned by the `PageTableEntry::frame` method.
#[derive(Snafu, Debug, Clone, Copy, PartialEq)]
pub enum FrameError {
    /// The entry does not have the `PRESENT` flag set, so it isn't currently mapped to a frame.
    FrameNotPresent,
    /// The entry has the `HUGE_PAGE` flag set. The `frame` method has a standard 4KiB frame
    /// as return type, so a huge frame can't be returned. @todo
    HugeFrame,
}

/// A 64-bit page table entry.
// pub struct PageTableEntry {
//     entry: u64,
// }

const ADDR_MASK: u64 = 0x0000_ffff_ffff_f000;
/*
impl PageTableEntry {
    /// Creates an unused page table entry.
    pub fn new() -> Self {
        PageTableEntry::Invalid
    }

    /// Returns whether this entry is zero.
    pub fn is_unused(&self) -> bool {
        self.entry == 0
    }

    /// Sets this entry to zero.
    pub fn set_unused(&mut self) {
        self.entry = 0;
    }

    /// Returns the flags of this entry.
    pub fn flags(&self) -> EntryFlags {
        EntryFlags::new(self.entry)
    }

    /// Returns the physical address mapped by this entry, might be zero.
    pub fn addr(&self) -> PhysAddr {
        PhysAddr::new(self.entry & ADDR_MASK)
    }

    /// Returns the physical frame mapped by this entry.
    ///
    /// Returns the following errors:
    ///
    /// - `FrameError::FrameNotPresent` if the entry doesn't have the `PRESENT` flag set.
    /// - `FrameError::HugeFrame` if the entry has the `HUGE_PAGE` flag set (for huge pages the
    ///    `addr` function must be used)
    pub fn frame(&self) -> Result<PhysFrame, FrameError> {
        if !self.flags().read(STAGE1_DESCRIPTOR::VALID) {
            Err(FrameError::FrameNotPresent)
        // } else if self.flags().contains(EntryFlags::HUGE_PAGE) {
        // Err(FrameError::HugeFrame)
        } else {
            Ok(PhysFrame::containing_address(self.addr()))
        }
    }

    /// Map the entry to the specified physical address with the specified flags.
    pub fn set_addr(&mut self, addr: PhysAddr, flags: EntryFlags) {
        assert!(addr.is_aligned(Size4KiB::SIZE));
        self.entry = addr.as_u64() | flags.bits();
    }

    /// Map the entry to the specified physical frame with the specified flags.
    pub fn set_frame(&mut self, frame: PhysFrame, flags: EntryFlags) {
        // assert!(!flags.contains(EntryFlags::HUGE_PAGE));
        self.set_addr(frame.start_address(), flags)
    }

    /// Sets the flags of this entry.
    pub fn set_flags(&mut self, flags: EntryFlags) {
        // Todo: extract ADDR from self and replace all flags completely (?)
        self.entry = self.addr().as_u64() | flags.bits();
    }
}

impl fmt::Debug for PageTableEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut f = f.debug_struct("PageTableEntry");
        f.field("addr", &self.addr());
        f.field("flags", &self.flags());
        f.finish()
    }
}*/

impl<Level> Table<Level>
where
    Level: HierarchicalLevel,
{
    pub fn next_table_create<Alloc>(
        &mut self,
        index: usize,
        allocator: &mut Alloc,
    ) -> &mut Table<Level::NextLevel>
    where
        Alloc: FrameAllocator,
    {
        if self.next_table(index).is_none() {
            assert!(
                EntryRegister::new(self.entries[index]).read(STAGE1_DESCRIPTOR::TYPE)
                    == STAGE1_DESCRIPTOR::TYPE::Table.value,
                "mapping code does not support huge pages"
            );
            let frame = allocator.allocate_frame().expect("no frames available");
            self.entries[index] = PageTableEntry::TableDescriptor(
                STAGE1_DESCRIPTOR::NEXT_LVL_TABLE_ADDR_4KiB.val(u64::from(frame))
                    + STAGE1_DESCRIPTOR::VALID::True,
            )
            .into();
            //            self.entries[index]
            //                .set_frame(frame, STAGE1_DESCRIPTOR::VALID::True /*| WRITABLE*/);
            self.next_table_mut(index).unwrap().zero();
        }
        self.next_table_mut(index).unwrap()
    }
}
