// Verbatim from https://github.com/rust-osdev/x86_64/blob/aa9ae54657beb87c2a491f2ab2140b2332afa6ba/src/structures/paging/page.rs
// Abstractions for default-sized and huge virtual memory pages.

// @fixme x86_64 page level numbering: P4 -> P3 -> P2 -> P1
// @fixme armv8a page level numbering: L0 -> L1 -> L2 -> L3

use {
    crate::memory::{
        page_size::{NotGiantPageSize, PageSize, Size1GiB, Size2MiB, Size4KiB},
        VirtAddr,
    },
    core::{
        fmt,
        marker::PhantomData,
        ops::{Add, AddAssign, Sub, SubAssign},
    },
    ux::u9,
};

/// A virtual memory page.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Page<S: PageSize = Size4KiB> {
    start_address: VirtAddr,
    size: PhantomData<S>,
}

pub enum Error {
    NotAligned,
}

impl<S: PageSize> Page<S> {
    /// The page size in bytes.
    pub const SIZE: usize = S::SIZE;

    /// Returns the page that starts at the given virtual address.
    ///
    /// Returns an error if the address is not correctly aligned (i.e. is not a valid page start).
    pub fn from_start_address(address: VirtAddr) -> Result<Self, Error> {
        if !address.is_aligned(S::SIZE) {
            Err(Error::NotAligned)
        } else {
            Ok(Page::containing_address(address))
        }
    }

    /// Returns the page that contains the given virtual address.
    pub fn containing_address(address: VirtAddr) -> Self {
        Page {
            start_address: address.aligned_down(S::SIZE),
            size: PhantomData,
        }
    }

    /// Returns the start address of the page.
    pub fn start_address(&self) -> VirtAddr {
        self.start_address
    }

    /// Returns the size the page (4KB, 2MB or 1GB).
    pub const fn size(&self) -> usize {
        S::SIZE
    }

    /// Returns the level 0 page table index of this page.
    pub fn l0_index(&self) -> u9 {
        self.start_address().l0_index()
    }

    /// Returns the level 1 page table index of this page.
    pub fn l1_index(&self) -> u9 {
        self.start_address().l1_index()
    }

    /// Returns a range of pages, exclusive `end`.
    pub fn range(start: Self, end: Self) -> PageRange<S> {
        PageRange { start, end }
    }

    /// Returns a range of pages, inclusive `end`.
    pub fn range_inclusive(start: Self, end: Self) -> PageRangeInclusive<S> {
        PageRangeInclusive { start, end }
    }
}

impl<S: NotGiantPageSize> Page<S> {
    /// Returns the level 2 page table index of this page.
    pub fn l2_index(&self) -> u9 {
        self.start_address().l2_index()
    }
}

impl Page<Size1GiB> {
    /// Returns the 1GiB memory page with the specified page table indices.
    pub fn from_page_table_indices_1gib(l0_index: u9, l1_index: u9) -> Self {
        use bit_field::BitField;

        let mut addr = 0;
        addr.set_bits(39..48, u64::from(l0_index));
        addr.set_bits(30..39, u64::from(l1_index));
        Page::containing_address(VirtAddr::new(addr))
    }
}

impl Page<Size2MiB> {
    /// Returns the 2MiB memory page with the specified page table indices.
    pub fn from_page_table_indices_2mib(l0_index: u9, l1_index: u9, l2_index: u9) -> Self {
        use bit_field::BitField;

        let mut addr = 0;
        addr.set_bits(39..48, u64::from(l0_index));
        addr.set_bits(30..39, u64::from(l1_index));
        addr.set_bits(21..30, u64::from(l2_index));
        Page::containing_address(VirtAddr::new(addr))
    }
}

impl Page<Size4KiB> {
    /// Returns the 4KiB memory page with the specified page table indices.
    pub fn from_page_table_indices(l0_index: u9, l1_index: u9, l2_index: u9, l3_index: u9) -> Self {
        use bit_field::BitField;

        let mut addr = 0;
        addr.set_bits(39..48, u64::from(l0_index));
        addr.set_bits(30..39, u64::from(l1_index));
        addr.set_bits(21..30, u64::from(l2_index));
        addr.set_bits(12..21, u64::from(l3_index));
        Page::containing_address(VirtAddr::new(addr))
    }

    /// Returns the level 3 page table index of this page.
    pub fn l3_index(&self) -> u9 {
        self.start_address().l3_index()
    }
}

impl<S: PageSize> fmt::Debug for Page<S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_fmt(format_args!(
            "Page<{}>({:#x})",
            S::SIZE_AS_DEBUG_STR,
            self.start_address().as_u64()
        ))
    }
}

impl<S: PageSize> Add<u64> for Page<S> {
    type Output = Self;
    // @todo should I add pages or just bytes here?
    fn add(self, rhs: u64) -> Self::Output {
        Page::containing_address(self.start_address() + rhs * S::SIZE as u64)
    }
}

impl<S: PageSize> AddAssign<u64> for Page<S> {
    fn add_assign(&mut self, rhs: u64) {
        *self = self.clone() + rhs;
    }
}

impl<S: PageSize> Sub<u64> for Page<S> {
    type Output = Self;
    /// Subtracts `rhs` same-sized pages from the current address.
    // @todo should I sub pages or just bytes here?
    fn sub(self, rhs: u64) -> Self::Output {
        Page::containing_address(self.start_address() - rhs * S::SIZE as u64)
    }
}

impl<S: PageSize> SubAssign<u64> for Page<S> {
    fn sub_assign(&mut self, rhs: u64) {
        *self = self.clone() - rhs;
    }
}

impl<S: PageSize> Sub<Self> for Page<S> {
    type Output = usize;
    fn sub(self, rhs: Self) -> Self::Output {
        (self.start_address - rhs.start_address) as usize / S::SIZE
    }
}

/// A range of pages with exclusive upper bound.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PageRange<S: PageSize = Size4KiB> {
    /// The start of the range, inclusive.
    pub start: Page<S>,
    /// The end of the range, exclusive.
    pub end: Page<S>,
}

impl<S: PageSize> PageRange<S> {
    /// Returns whether this range contains no pages.
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    pub fn num_pages(&self) -> usize {
        (self.end - self.start) as usize / S::SIZE
    }
}

impl<S: PageSize> Iterator for PageRange<S> {
    type Item = Page<S>;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.is_empty() {
            let page = self.start.clone();
            self.start += 1;
            Some(page)
        } else {
            None
        }
    }
}

impl PageRange<Size2MiB> {
    /// Converts the range of 2MiB pages to a range of 4KiB pages.
    // @todo what about range of 1GiB pages?
    pub fn as_4kib_page_range(&self) -> PageRange<Size4KiB> {
        PageRange {
            start: Page::containing_address(self.start.start_address()),
            // @fixme end is calculated incorrectly, add test
            end: Page::containing_address(self.end.start_address()),
        }
    }
}

impl<S: PageSize> fmt::Debug for PageRange<S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("PageRange")
            .field("start", &self.start)
            .field("end", &self.end)
            .finish()
    }
}

/// A range of pages with inclusive upper bound.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PageRangeInclusive<S: PageSize = Size4KiB> {
    /// The start of the range, inclusive.
    pub start: Page<S>,
    /// The end of the range, inclusive.
    pub end: Page<S>,
}

impl<S: PageSize> PageRangeInclusive<S> {
    /// Returns whether this range contains no pages.
    pub fn is_empty(&self) -> bool {
        self.start > self.end
    }
}

impl<S: PageSize> Iterator for PageRangeInclusive<S> {
    type Item = Page<S>;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.is_empty() {
            let page = self.start.clone();
            self.start += 1;
            Some(page)
        } else {
            None
        }
    }
}

impl<S: PageSize> fmt::Debug for PageRangeInclusive<S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("PageRangeInclusive")
            .field("start", &self.start)
            .field("end", &self.end)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test_case]
    pub fn test_page_ranges() {
        let page_size = Size4KiB::SIZE;
        let number = 1000;

        let start_addr = VirtAddr::new(0xdeadbeaf);
        let start: Page = Page::containing_address(start_addr);
        let end = start.clone() + number;

        let mut range = Page::range(start.clone(), end.clone());
        for i in 0..number {
            assert_eq!(
                range.next(),
                Some(Page::containing_address(start_addr + page_size * i))
            );
        }
        assert_eq!(range.next(), None);

        let mut range_inclusive = Page::range_inclusive(start, end);
        for i in 0..=number {
            assert_eq!(
                range_inclusive.next(),
                Some(Page::containing_address(start_addr + page_size * i))
            );
        }
        assert_eq!(range_inclusive.next(), None);
    }

    #[test_case]
    fn test_page_range_conversion() {
        let page_size = Size2MiB::SIZE;
        let number = 10;

        let start_addr = VirtAddr::new(0xdeadbeaf);
        let start: Page = Page::containing_address(start_addr);
        let end = start.clone() + number;

        let range = Page::range(start.clone(), end.clone()).as_4kib_page_range();

        // 10 2MiB pages is 512 4KiB pages
        aseert_eq!(range.num_pages(), 512);
    }
}
