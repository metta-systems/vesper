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
