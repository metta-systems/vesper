// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2018-2022 Andre Richter <andre.o.richter@gmail.com>

//! Memory Management.

use {
    crate::{mm, platform},
    core::{
        fmt,
        marker::PhantomData,
        ops::{Add, Sub},
    },
};

pub mod mmu;

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// Metadata trait for marking the type of an address.
pub trait AddressType: Copy + Clone + PartialOrd + PartialEq + Ord + Eq {}

/// Zero-sized type to mark a physical address.
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq, Ord, Eq)]
pub enum Physical {}

/// Zero-sized type to mark a virtual address.
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq, Ord, Eq)]
pub enum Virtual {}

/// Generic address type.
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq, Ord, Eq)]
pub struct Address<ATYPE: AddressType> {
    value: usize,
    _address_type: PhantomData<fn() -> ATYPE>,
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

impl AddressType for Physical {}
impl AddressType for Virtual {}

impl<ATYPE: AddressType> Address<ATYPE> {
    /// Create an instance.
    pub const fn new(value: usize) -> Self {
        Self {
            value,
            _address_type: PhantomData,
        }
    }

    /// Convert to usize.
    pub const fn as_usize(self) -> usize {
        self.value
    }

    /// Align down to page size.
    #[must_use]
    pub const fn align_down_page(self) -> Self {
        let aligned = mm::align_down(self.value, platform::memory::mmu::KernelGranule::SIZE);

        Self::new(aligned)
    }

    /// Align up to page size.
    #[must_use]
    pub const fn align_up_page(self) -> Self {
        let aligned = mm::align_up(self.value, platform::memory::mmu::KernelGranule::SIZE);

        Self::new(aligned)
    }

    /// Checks if the address is page aligned.
    pub const fn is_page_aligned(&self) -> bool {
        mm::is_aligned(self.value, platform::memory::mmu::KernelGranule::SIZE)
    }

    /// Return the address' offset into the corresponding page.
    pub const fn offset_into_page(&self) -> usize {
        self.value & platform::memory::mmu::KernelGranule::MASK
    }
}

impl<ATYPE: AddressType> Add<usize> for Address<ATYPE> {
    type Output = Self;

    #[inline(always)]
    fn add(self, rhs: usize) -> Self::Output {
        match self.value.checked_add(rhs) {
            None => panic!("Overflow on Address::add"),
            Some(x) => Self::new(x),
        }
    }
}

impl<ATYPE: AddressType> Sub<Address<ATYPE>> for Address<ATYPE> {
    type Output = Self;

    #[inline(always)]
    fn sub(self, rhs: Address<ATYPE>) -> Self::Output {
        match self.value.checked_sub(rhs.value) {
            None => panic!("Overflow on Address::sub"),
            Some(x) => Self::new(x),
        }
    }
}

impl fmt::Display for Address<Physical> {
    // Don't expect to see physical addresses greater than 40 bit.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let q3: u8 = ((self.value >> 32) & 0xff) as u8;
        let q2: u16 = ((self.value >> 16) & 0xffff) as u16;
        let q1: u16 = (self.value & 0xffff) as u16;

        write!(f, "0x")?;
        write!(f, "{:02x}_", q3)?;
        write!(f, "{:04x}_", q2)?;
        write!(f, "{:04x}", q1)
    }
}

impl fmt::Display for Address<Virtual> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let q4: u16 = ((self.value >> 48) & 0xffff) as u16;
        let q3: u16 = ((self.value >> 32) & 0xffff) as u16;
        let q2: u16 = ((self.value >> 16) & 0xffff) as u16;
        let q1: u16 = (self.value & 0xffff) as u16;

        write!(f, "0x")?;
        write!(f, "{:04x}_", q4)?;
        write!(f, "{:04x}_", q3)?;
        write!(f, "{:04x}_", q2)?;
        write!(f, "{:04x}", q1)
    }
}

//--------------------------------------------------------------------------------------------------
// Testing
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity of [Address] methods.
    #[test_case]
    fn address_type_method_sanity() {
        let addr = Address::<Virtual>::new(platform::memory::mmu::KernelGranule::SIZE + 100);

        assert_eq!(
            addr.align_down_page().as_usize(),
            platform::memory::mmu::KernelGranule::SIZE
        );

        assert_eq!(
            addr.align_up_page().as_usize(),
            platform::memory::mmu::KernelGranule::SIZE * 2
        );

        assert!(!addr.is_page_aligned());

        assert_eq!(addr.offset_into_page(), 100);
    }
}
