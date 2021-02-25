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
#[derive(Debug, Snafu)]
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
