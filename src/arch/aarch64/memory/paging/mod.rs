// mod arch::aarch64::memory::paging

//! Some code was borrowed from [Phil Opp's Blog](https://os.phil-opp.com/page-tables/)
//! Paging is mostly based on https://os.phil-opp.com/page-tables/ and ARM ARM

// AArch64:
// Table D4-8-2021: check supported granule sizes, select alloc policy based on results.
// TTBR_ELx is the pdbr for specific page tables

// Page 2068 actual page descriptor formats

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
 */

pub use self::entry::*;
use core::ptr::Unique;
use self::table::{Level0, Table};
use super::{Frame, FrameAllocator, PhysicalAddress, VirtualAddress};

mod entry;
mod table;

pub const PAGE_SIZE: usize = 4096;

pub const ENTRY_COUNT: usize = 512;

/**
 * Page is an addressable unit of the virtual address space.
 */
pub struct Page {
    number: usize,
}

impl Page {
    pub fn containing_address(address: VirtualAddress) -> Page {
        assert!(
            address < 0x0000_8000_0000_0000 || address >= 0xffff_8000_0000_0000,
            "invalid address: 0x{:x}",
            address
        );
        Page {
            number: address / PAGE_SIZE,
        }
    }

    fn start_address(&self) -> usize {
        self.number * PAGE_SIZE
    }

    fn l0_index(&self) -> usize {
        (self.number >> 27) & 0o777
    }
    fn l1_index(&self) -> usize {
        (self.number >> 18) & 0o777
    }
    fn l2_index(&self) -> usize {
        (self.number >> 9) & 0o777
    }
    fn l3_index(&self) -> usize {
        (self.number >> 0) & 0o777
    }
}

pub struct ActivePageTable {
    l0: Unique<Table<Level0>>,
}

impl ActivePageTable {
    pub unsafe fn new() -> ActivePageTable {
        ActivePageTable {
            l0: Unique::new_unchecked(table::L0),
        }
    }

    fn l0(&self) -> &Table<Level0> {
        unsafe { self.l0.as_ref() }
    }

    fn l0_mut(&mut self) -> &mut Table<Level0> {
        unsafe { self.l0.as_mut() }
    }

    pub fn translate(&self, virtual_address: VirtualAddress) -> Option<PhysicalAddress> {
        let offset = virtual_address % PAGE_SIZE;
        self.translate_page(Page::containing_address(virtual_address))
            .map(|frame| frame.number * PAGE_SIZE + offset)
    }

    fn translate_page(&self, page: Page) -> Option<Frame> {
        use self::entry::EntryFlags;

        let l1 = self.l0().next_table(page.l0_index());

        let huge_page = || {
            l1.and_then(|l1| {
                let l1_entry = &l1[page.l1_index()];
                // 1GiB page?
                if let Some(start_frame) = l1_entry.pointed_frame() {
                    if !l1_entry.flags().contains(EntryFlags::TABLE) {
                        // address must be 1GiB aligned
                        assert!(start_frame.number % (ENTRY_COUNT * ENTRY_COUNT) == 0);
                        return Some(Frame {
                            number: start_frame.number + page.l2_index() * ENTRY_COUNT
                                + page.l3_index(),
                        });
                    }
                }
                if let Some(l2) = l1.next_table(page.l1_index()) {
                    let l2_entry = &l2[page.l2_index()];
                    // 2MiB page?
                    if let Some(start_frame) = l2_entry.pointed_frame() {
                        if !l2_entry.flags().contains(EntryFlags::TABLE) {
                            // address must be 2MiB aligned
                            assert!(start_frame.number % ENTRY_COUNT == 0);
                            return Some(Frame {
                                number: start_frame.number + page.l3_index(),
                            });
                        }
                    }
                }
                None
            })
        };

        l1.and_then(|l1| l1.next_table(page.l1_index()))
            .and_then(|l2| l2.next_table(page.l2_index()))
            .and_then(|l3| l3[page.l3_index()].pointed_frame())
            .or_else(huge_page)
    }

    pub fn map_to<A>(&mut self, page: Page, frame: Frame, flags: EntryFlags, allocator: &mut A)
    where
        A: FrameAllocator,
    {
        let l0 = self.l0_mut();
        let mut l1 = l0.next_table_create(page.l0_index(), allocator);
        let mut l2 = l1.next_table_create(page.l1_index(), allocator);
        let mut l3 = l2.next_table_create(page.l2_index(), allocator);

        assert!(l3[page.l3_index()].is_unused());
        l3[page.l3_index()].set(frame, flags | EntryFlags::VALID);
    }

    pub fn map<A>(&mut self, page: Page, flags: EntryFlags, allocator: &mut A)
    where
        A: FrameAllocator,
    {
        let frame = allocator.allocate_frame().expect("out of memory");
        self.map_to(page, frame, flags, allocator)
    }

    pub fn identity_map<A>(&mut self, frame: Frame, flags: EntryFlags, allocator: &mut A)
    where
        A: FrameAllocator,
    {
        let page = Page::containing_address(frame.start_address());
        self.map_to(page, frame, flags, allocator)
    }

    fn unmap<A>(&mut self, page: Page, allocator: &mut A)
    where
        A: FrameAllocator,
    {
        // use aarch64::instructions::tlb;
        // use x86_64::VirtualAddress;

        assert!(self.translate(page.start_address()).is_some());

        let l3 = self.l0_mut()
            .next_table_mut(page.l0_index())
            .and_then(|l1| l1.next_table_mut(page.l1_index()))
            .and_then(|l2| l2.next_table_mut(page.l2_index()))
            .expect("mapping code does not support huge pages");
        let frame = l3[page.l3_index()].pointed_frame().unwrap();
        l3[page.l3_index()].set_unused();
        // tlb::flush(VirtualAddress(page.start_address()));
        // TODO free p(1,2,3) table if empty
        //allocator.deallocate_frame(frame);
    }
}
