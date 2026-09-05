use {
    crate::{MMIODescriptor, PageAddress},
    core::{iter::Step, num::NonZeroUsize, ops::Range},
    libaddress::{Address, AddressType, Physical},
};

/// A type that describes a region of memory in quantities of pages.
#[derive(Copy, Clone, Debug, Eq, PartialOrd, PartialEq)]
pub struct MemoryRegion<ATYPE: const AddressType, const PAGE_SIZE: usize> {
    start: PageAddress<ATYPE, PAGE_SIZE>,
    end_exclusive: PageAddress<ATYPE, PAGE_SIZE>,
}

impl<ATYPE: const AddressType, const PAGE_SIZE: usize> MemoryRegion<ATYPE, PAGE_SIZE> {
    /// Create an instance.
    pub fn new(
        start: PageAddress<ATYPE, PAGE_SIZE>,
        end_exclusive: PageAddress<ATYPE, PAGE_SIZE>,
    ) -> Self {
        assert!(start <= end_exclusive);

        Self {
            start,
            end_exclusive,
        }
    }

    fn as_range(&self) -> Range<PageAddress<ATYPE, PAGE_SIZE>> {
        self.into_iter()
    }

    /// Returns the start page address.
    pub fn start_page_addr(&self) -> PageAddress<ATYPE, PAGE_SIZE> {
        self.start
    }

    /// Returns the start address.
    pub fn start_addr(&self) -> Address<ATYPE> {
        self.start.into_inner()
    }

    /// Returns the exclusive end page address.
    pub fn end_exclusive_page_addr(&self) -> PageAddress<ATYPE, PAGE_SIZE> {
        self.end_exclusive
    }

    /// Returns the exclusive end page address.
    pub fn end_inclusive_page_addr(&self) -> PageAddress<ATYPE, PAGE_SIZE> {
        self.end_exclusive.checked_page_offset(-1).unwrap()
    }

    /// Checks if self contains an address.
    pub fn contains(&self, addr: Address<ATYPE>) -> bool {
        let page_addr = PageAddress::<ATYPE, PAGE_SIZE>::from(addr.align_down_page(&PAGE_SIZE));
        self.as_range().contains(&page_addr)
    }

    /// Checks if there is an overlap with another memory region.
    pub fn overlaps(&self, other_region: &Self) -> bool {
        let self_range = self.as_range();

        self_range.contains(&other_region.start_page_addr())
            || self_range.contains(&other_region.end_inclusive_page_addr())
    }

    /// Returns the number of pages contained in this region.
    pub fn num_pages(&self) -> usize {
        PageAddress::steps_between(&self.start, &self.end_exclusive).0
    }

    /// Returns the size in bytes of this region.
    pub fn size(&self) -> usize {
        // Invariant: start <= end_exclusive, so do unchecked arithmetic.
        let end_exclusive = self.end_exclusive.into_inner().as_usize();
        let start = self.start.into_inner().as_usize();

        end_exclusive - start
    }

    /// Splits the `MemoryRegion` like in the following diagram.
    /// Left region is returned to the caller. Right region is the new region for this struct.
    ///
    /// --------------------------------------------------------------------------------
    /// |   |   |   |   |   |   |   |   |   |   |   |   |   |   |   |   |   |   |
    /// --------------------------------------------------------------------------------
    ///   ^                               ^                                       ^
    ///   |                               |                                       |
    ///  `left_start`   `left_end_exclusive`                                      |
    ///                                                                           |
    ///                                   ^                                       |
    ///                                   |                                       |
    ///                                  `right_start`          `right_end_exclusive`
    ///
    pub fn take_first_n_pages(&mut self, num_pages: NonZeroUsize) -> Result<Self, &'static str> {
        let count: usize = num_pages.into();

        let left_end_exclusive = self.start.checked_page_offset(count.cast_signed());
        let Some(left_end_exclusive) = left_end_exclusive else {
            return Err("Overflow while calculating left_end_exclusive");
        };

        if left_end_exclusive > self.end_exclusive {
            return Err("Not enough free pages");
        }

        let allocation = Self {
            start: self.start,
            end_exclusive: left_end_exclusive,
        };
        self.start = left_end_exclusive;

        Ok(allocation)
    }
}

impl<ATYPE: const AddressType, const PAGE_SIZE: usize> IntoIterator
    for MemoryRegion<ATYPE, PAGE_SIZE>
{
    type Item = PageAddress<ATYPE, PAGE_SIZE>;
    type IntoIter = Range<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        Range {
            start: self.start,
            end: self.end_exclusive,
        }
    }
}

impl<const PAGE_SIZE: usize> From<MMIODescriptor> for MemoryRegion<Physical, PAGE_SIZE> {
    fn from(desc: MMIODescriptor) -> Self {
        let start = PageAddress::from(desc.start_addr().align_down_page(&PAGE_SIZE));
        let end_exclusive = PageAddress::from(desc.end_addr_exclusive().align_up_page(&PAGE_SIZE));

        Self {
            start,
            end_exclusive,
        }
    }
}
