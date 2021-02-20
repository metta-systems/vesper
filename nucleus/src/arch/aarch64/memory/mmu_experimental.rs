// 1: use Table<Level> for sure
// 2: in tables use typed descriptors over generic u64 entries?? how to pick right type...
// -- TableDescriptor
// -- Lvl2BlockDescriptor
// -- PageDescriptor
// Use them instead of PageTableEntry
// 3: Use PhysFrame<Size> and Page<Size> as flexible versions of various-sized pages

// Level 0 descriptors can only output the address of a Level 1 table.
// Level 3 descriptors cannot point to another table and can only output block addresses.
// The format of the table is therefore slightly different for Level 3.
//
// this means:
// - level 0 page table can be only TableDescriptors
// - level 1,2 page table can be TableDescriptors, Lvl2BlockDescriptors (PageDescriptors)
// - level 3 page table can be only PageDescriptors

// Level / Types | Table Descriptor | Lvl2BlockDescriptor (PageDescriptor)
// --------------+------------------+--------------------------------------
//   0           |        X         |            (with 4KiB granule)
//   1           |        X         |          X (1GiB range)
//   2           |        X         |          X (2MiB range)
//   3           |                  |          X (4KiB range) -- called PageDescriptor
//                                         encoding actually the same as in Table Descriptor

// Translation granule affects the size of the block addressed.
// Lets use 4KiB granule on RPi3 for simplicity.

// This gives the following address format:
//
// Maximum OA is 48 bits.
//
// Level 0 descriptor cannot be block descriptor.
// Level 0 table descriptor has Output Address in [47:12]
//
// Level 1 block descriptor has Output Address in [47:30]
// Level 2 block descriptor has Output Address in [47:21]
//
// Level 1 table descriptor has Output Address in [47:12]
// Level 2 table descriptor has Output Address in [47:12]
//
// Level 3 Page Descriptor:
// Upper Attributes [63:51]
// Res0 [50:48]
// Output Address [47:12]
// Lower Attributes [11:2]
// 11b [1:0]

// enum PageTableEntry { Page(&mut PageDescriptor), Block(&mut BlockDescriptor), Etc(&mut u64), Invalid(&mut u64) }
// impl PageTabelEntry { fn new_from_entry_addr(&u64) }

// If I have, for example, Table<Level0> I can get from it N `Table<Level1>` (via impl HierarchicalTable)
// From Table<Level1> I can get either `Table<Level2>` (via impl HierarchicalTable) or `BlockDescriptor<Size1GiB>`
// From Table<Level2> I can get either `Table<Level3>` (via impl HierarchicalTable) or `BlockDescriptor<Size2MiB>`
// From Table<Level3> I can only get `PageDescriptor<Size4KiB>` (because no impl HierarchicalTable exists)

// enum PageTableEntry { Page(&mut PageDescriptor), Block(&mut BlockDescriptor), Etc(&mut u64), Invalid(&mut u64) }
// return enum PageTableEntry constructed from table bits in u64

/*!
 * Paging system uses a separate address space in top kernel region (TTBR1) to access
 * entire physical memory contents.
 * This mapping is not available to user space (user space uses TTBR0).
 * Use the largest possible granule size to map physical memory since we want to use
 * the least amount of memory for these mappings.
 */

// Check largest VA supported, calculate physical_memory_offset
//
const PHYSICAL_MEMORY_OFFSET: u64 = 0xffff_8000_0000_0000; // Last 1GiB of VA space

// AArch64:
// Table D4-8-2021: check supported granule sizes, select alloc policy based on results.
// TTBR_ELx is the pdbr for specific page tables

// Page 2068 actual page descriptor formats

/// A standard 16KiB page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Size16KiB {}

impl PageSize for Size16KiB {
    const SIZE: u64 = 16384;
    const SIZE_AS_DEBUG_STR: &'static str = "16KiB";
    const SHIFT: usize = 14;
    const MASK: u64 = 0x3fff;
}

impl NotGiantPageSize for Size16KiB {}

/// A “giant” 1GiB page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Size1GiB {}

impl PageSize for Size1GiB {
    const SIZE: u64 = Size2MiB::SIZE * NUM_ENTRIES_4KIB;
    const SIZE_AS_DEBUG_STR: &'static str = "1GiB";
    const SHIFT: usize = 59; // @todo
    const MASK: u64 = 0xfffaaaaa; // @todo
}

/// Errors from mapping layer (@todo use anyhow/snafu? thiserror?)
pub enum TranslationError {
    NoPage,
}

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

    pub fn translate(&self, virtual_address: VirtAddr) -> Result<PhysAddr, TranslationError> {
        let offset = virtual_address % Size4KiB::SIZE as usize; // @todo use the size of the last page of course
        self.translate_page(Page::containing_address(virtual_address))
            .map(|frame| frame.start_address() + offset)
    }

    fn translate_page(&self, page: Page) -> Result<PhysFrame, TranslationError> {
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
    }
}

// Abstractions for page table entries.

/// The error returned by the `PageTableEntry::frame` method.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FrameError {
    /// The entry does not have the `PRESENT` flag set, so it isn't currently mapped to a frame.
    FrameNotPresent,
    /// The entry has the `HUGE_PAGE` flag set. The `frame` method has a standard 4KiB frame
    /// as return type, so a huge frame can't be returned.
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

/*
 */
/*
 */
/*
 */
/*
 */
/*
 */
/*
 */
/*
 */
/*
 */
/*
 */
/*
 */

/*
 * SPDX-License-Identifier: BSL-1.0 - todo this is from Sergio Benitez cs140e
 */
// Abstractions for page tables.

// to get L0 we must allocate a few frames from boot region allocator.
// So, first we init the dtb, parse mem-regions from there, then init boot_info page and start mmu,
// this part will be inited in mmu::init():
//pub const L0: *mut Table<PageGlobalDirectory> = &mut LVL0_TABLE as *mut _; // was Table<Level0>
// @fixme this is for recursive page tables!!

impl<L> Table<L>
where
    L: HierarchicalLevel,
{
    fn next_table_address(&self, index: usize) -> Option<usize> {
        let entry_flags = EntryRegister::new(self[index]);
        if entry_flags.matches_all(STAGE1_DESCRIPTOR::VALID::True + STAGE1_DESCRIPTOR::TYPE::Table)
        {
            let table_address = self as *const _ as usize;
            Some((table_address << 9) | (index << 12))
        } else {
            None
        }
    }

    pub fn next_table(&self, index: usize) -> Option<&Table<L::NextLevel>> {
        self.next_table_address(index)
            .map(|address| unsafe { &*(address as *const _) })
    }

    pub fn next_table_mut(&mut self, index: usize) -> Option<&mut Table<L::NextLevel>> {
        self.next_table_address(index)
            .map(|address| unsafe { &mut *(address as *mut _) })
    }

    pub fn next_table_create<A>(
        &mut self,
        index: usize,
        allocator: &mut A,
    ) -> &mut Table<L::NextLevel>
    where
        A: FrameAllocator,
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

// ORIGINAL MMU.RS CODE

//static mut LVL0_TABLE: Table<PageGlobalDirectory> = Table {
//    entries: [0; NUM_ENTRIES_4KIB],
//    level: PhantomData,
//};
