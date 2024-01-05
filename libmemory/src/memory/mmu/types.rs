//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

use {
    crate::{
        memory::{Address, AddressType, Physical},
        mm,
        platform::{self, memory::mmu::KernelGranule},
    },
    core::{
        fmt::{self, Formatter},
        iter::Step,
        num::NonZeroUsize,
        ops::Range,
    },
};

/// A wrapper type around [Address] that ensures page alignment.
#[derive(Copy, Clone, Debug, Eq, PartialOrd, PartialEq)]
pub struct PageAddress<ATYPE: AddressType> {
    inner: Address<ATYPE>,
}

/// A type that describes a region of memory in quantities of pages.
#[derive(Copy, Clone, Debug, Eq, PartialOrd, PartialEq)]
pub struct MemoryRegion<ATYPE: AddressType> {
    start: PageAddress<ATYPE>,
    end_exclusive: PageAddress<ATYPE>,
}

/// Architecture agnostic memory attributes.
#[derive(Copy, Clone, Debug, Eq, PartialOrd, PartialEq)]
pub enum MemAttributes {
    /// Regular memory
    CacheableDRAM,
    /// Memory without caching
    NonCacheableDRAM,
    /// Device memory
    Device,
}

/// Architecture agnostic memory region access permissions.
#[derive(Copy, Clone, Debug, Eq, PartialOrd, PartialEq)]
pub enum AccessPermissions {
    /// Read-only access
    ReadOnly,
    /// Read-write access
    ReadWrite,
}

/// Summary structure of memory region properties.
#[derive(Copy, Clone, Debug, Eq, PartialOrd, PartialEq)]
pub struct AttributeFields {
    /// Attributes
    pub mem_attributes: MemAttributes,
    /// Permissions
    pub acc_perms: AccessPermissions,
    /// Disable executable code in this region
    pub execute_never: bool,
}

/// An MMIO descriptor for use in device drivers.
#[derive(Copy, Clone)]
pub struct MMIODescriptor {
    start_addr: Address<Physical>,
    end_addr_exclusive: Address<Physical>,
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

//------------------------------------------------------------------------------
// PageAddress
//------------------------------------------------------------------------------
impl<ATYPE: AddressType> PageAddress<ATYPE> {
    /// Unwraps the value.
    pub fn into_inner(self) -> Address<ATYPE> {
        self.inner
    }

    /// Calculates the offset from the page address.
    ///
    /// `count` is in units of [PageAddress]. For example, a count of 2 means `result = self + 2 *
    /// page_size`.
    pub fn checked_offset(self, count: isize) -> Option<Self> {
        if count == 0 {
            return Some(self);
        }

        let delta = count.unsigned_abs().checked_mul(KernelGranule::SIZE)?;
        let result = if count.is_positive() {
            self.inner.as_usize().checked_add(delta)?
        } else {
            self.inner.as_usize().checked_sub(delta)?
        };

        Some(Self {
            inner: Address::new(result),
        })
    }
}

impl<ATYPE: AddressType> From<usize> for PageAddress<ATYPE> {
    fn from(addr: usize) -> Self {
        assert!(
            mm::is_aligned(addr, KernelGranule::SIZE),
            "Input usize not page aligned"
        );

        Self {
            inner: Address::new(addr),
        }
    }
}

impl<ATYPE: AddressType> From<Address<ATYPE>> for PageAddress<ATYPE> {
    fn from(addr: Address<ATYPE>) -> Self {
        assert!(addr.is_page_aligned(), "Input Address not page aligned");

        Self { inner: addr }
    }
}

impl<ATYPE: AddressType> Step for PageAddress<ATYPE> {
    fn steps_between(start: &Self, end: &Self) -> Option<usize> {
        if start > end {
            return None;
        }

        // Since start <= end, do unchecked arithmetic.
        Some((end.inner.as_usize() - start.inner.as_usize()) >> KernelGranule::SHIFT)
    }

    fn forward_checked(start: Self, count: usize) -> Option<Self> {
        start.checked_offset(count as isize)
    }

    fn backward_checked(start: Self, count: usize) -> Option<Self> {
        start.checked_offset(-(count as isize))
    }
}

//------------------------------------------------------------------------------
// MemoryRegion
//------------------------------------------------------------------------------
impl<ATYPE: AddressType> MemoryRegion<ATYPE> {
    /// Create an instance.
    pub fn new(start: PageAddress<ATYPE>, end_exclusive: PageAddress<ATYPE>) -> Self {
        assert!(start <= end_exclusive);

        Self {
            start,
            end_exclusive,
        }
    }

    fn as_range(&self) -> Range<PageAddress<ATYPE>> {
        self.into_iter()
    }

    /// Returns the start page address.
    pub fn start_page_addr(&self) -> PageAddress<ATYPE> {
        self.start
    }

    /// Returns the start address.
    pub fn start_addr(&self) -> Address<ATYPE> {
        self.start.into_inner()
    }

    /// Returns the exclusive end page address.
    pub fn end_exclusive_page_addr(&self) -> PageAddress<ATYPE> {
        self.end_exclusive
    }

    /// Returns the exclusive end page address.
    pub fn end_inclusive_page_addr(&self) -> PageAddress<ATYPE> {
        self.end_exclusive.checked_offset(-1).unwrap()
    }

    /// Checks if self contains an address.
    pub fn contains(&self, addr: Address<ATYPE>) -> bool {
        let page_addr = PageAddress::from(addr.align_down_page());
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
        PageAddress::steps_between(&self.start, &self.end_exclusive).unwrap()
    }

    /// Returns the size in bytes of this region.
    pub fn size(&self) -> usize {
        // Invariant: start <= end_exclusive, so do unchecked arithmetic.
        let end_exclusive = self.end_exclusive.into_inner().as_usize();
        let start = self.start.into_inner().as_usize();

        end_exclusive - start
    }

    /// Splits the MemoryRegion like:
    ///
    /// --------------------------------------------------------------------------------
    /// |   |   |   |   |   |   |   |   |   |   |   |   |   |   |   |   |   |   |
    /// --------------------------------------------------------------------------------
    ///   ^                               ^                                       ^
    ///   |                               |                                       |
    ///   left_start     left_end_exclusive                                       |
    ///                                                                           |
    ///                                   ^                                       |
    ///                                   |                                       |
    ///                                   right_start           right_end_exclusive
    ///
    /// Left region is returned to the caller. Right region is the new region for this struct.
    pub fn take_first_n_pages(&mut self, num_pages: NonZeroUsize) -> Result<Self, &'static str> {
        let count: usize = num_pages.into();

        let left_end_exclusive = self.start.checked_offset(count as isize);
        let left_end_exclusive = match left_end_exclusive {
            None => return Err("Overflow while calculating left_end_exclusive"),
            Some(x) => x,
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

impl<ATYPE: AddressType> IntoIterator for MemoryRegion<ATYPE> {
    type Item = PageAddress<ATYPE>;
    type IntoIter = Range<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        Range {
            start: self.start,
            end: self.end_exclusive,
        }
    }
}

impl From<MMIODescriptor> for MemoryRegion<Physical> {
    fn from(desc: MMIODescriptor) -> Self {
        let start = PageAddress::from(desc.start_addr.align_down_page());
        let end_exclusive = PageAddress::from(desc.end_addr_exclusive().align_up_page());

        Self {
            start,
            end_exclusive,
        }
    }
}

//------------------------------------------------------------------------------
// MMIODescriptor
//------------------------------------------------------------------------------

impl MMIODescriptor {
    /// Create an instance.
    pub const fn new(start_addr: Address<Physical>, size: usize) -> Self {
        assert!(size > 0);
        let end_addr_exclusive = Address::new(start_addr.as_usize() + size);

        Self {
            start_addr,
            end_addr_exclusive,
        }
    }

    /// Return the start address.
    pub const fn start_addr(&self) -> Address<Physical> {
        self.start_addr
    }

    /// Return the exclusive end address.
    pub fn end_addr_exclusive(&self) -> Address<Physical> {
        self.end_addr_exclusive
    }
}

//------------------------------------------------------------------------------
// AttributeFields
//------------------------------------------------------------------------------

impl Default for AttributeFields {
    fn default() -> AttributeFields {
        AttributeFields {
            mem_attributes: MemAttributes::CacheableDRAM,
            acc_perms: AccessPermissions::ReadWrite,
            execute_never: true,
        }
    }
}

/// Human-readable output of AttributeFields
impl fmt::Display for AttributeFields {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let attr = match self.mem_attributes {
            MemAttributes::CacheableDRAM => "C",
            MemAttributes::NonCacheableDRAM => "NC",
            MemAttributes::Device => "Dev",
        };

        let acc_p = match self.acc_perms {
            AccessPermissions::ReadOnly => "RO",
            AccessPermissions::ReadWrite => "RW",
        };

        let xn = if self.execute_never { "PXN" } else { "PX" };

        write!(f, "{: <3} {} {: <3}", attr, acc_p, xn)
    }
}

//--------------------------------------------------------------------------------------------------
// Testing
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use {super::*, crate::memory::Virtual};

    /// Sanity of [PageAddress] methods.
    #[test_case]
    fn pageaddress_type_method_sanity() {
        let page_addr: PageAddress<Virtual> = PageAddress::from(KernelGranule::SIZE * 2);

        assert_eq!(
            page_addr.checked_offset(-2),
            Some(PageAddress::<Virtual>::from(0))
        );

        assert_eq!(
            page_addr.checked_offset(2),
            Some(PageAddress::<Virtual>::from(KernelGranule::SIZE * 4))
        );

        assert_eq!(
            PageAddress::<Virtual>::from(0).checked_offset(0),
            Some(PageAddress::<Virtual>::from(0))
        );
        assert_eq!(PageAddress::<Virtual>::from(0).checked_offset(-1), None);

        let max_page_addr = Address::<Virtual>::new(usize::MAX).align_down_page();
        assert_eq!(
            PageAddress::<Virtual>::from(max_page_addr).checked_offset(1),
            None
        );

        let zero = PageAddress::<Virtual>::from(0);
        let three = PageAddress::<Virtual>::from(KernelGranule::SIZE * 3);
        assert_eq!(PageAddress::steps_between(&zero, &three), Some(3));
    }

    /// Sanity of [MemoryRegion] methods.
    #[test_case]
    fn memoryregion_type_method_sanity() {
        let zero = PageAddress::<Virtual>::from(0);
        let zero_region = MemoryRegion::new(zero, zero);
        assert_eq!(zero_region.num_pages(), 0);
        assert_eq!(zero_region.size(), 0);

        let one = PageAddress::<Virtual>::from(KernelGranule::SIZE);
        let one_region = MemoryRegion::new(zero, one);
        assert_eq!(one_region.num_pages(), 1);
        assert_eq!(one_region.size(), KernelGranule::SIZE);

        let three = PageAddress::<Virtual>::from(KernelGranule::SIZE * 3);
        let mut three_region = MemoryRegion::new(zero, three);
        assert!(three_region.contains(zero.into_inner()));
        assert!(!three_region.contains(three.into_inner()));
        assert!(three_region.overlaps(&one_region));

        let allocation = three_region
            .take_first_n_pages(NonZeroUsize::new(2).unwrap())
            .unwrap();
        assert_eq!(allocation.num_pages(), 2);
        assert_eq!(three_region.num_pages(), 1);

        for (i, alloc) in allocation.into_iter().enumerate() {
            assert_eq!(alloc.into_inner().as_usize(), i * KernelGranule::SIZE);
        }
    }
}
