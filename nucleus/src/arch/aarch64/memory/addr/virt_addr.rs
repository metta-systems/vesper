/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

use {
    crate::{
        memory::PhysAddr,
        mm::{align_down, align_up},
    },
    bit_field::BitField,
    core::{
        convert::{From, Into, TryInto},
        fmt,
        ops::{Add, AddAssign, Rem, RemAssign, Sub, SubAssign},
    },
    usize_conversions::{usize_from, FromUsize},
    ux::*,
};

/// A canonical 64-bit virtual memory address.
///
/// This is a wrapper type around an `u64`, so it is always 8 bytes, even when compiled
/// on non 64-bit systems. The `UsizeConversions` trait can be used for performing conversions
/// between `u64` and `usize`.
///
/// On `x86_64`, only the 48 lower bits of a virtual address can be used. The top 16 bits need
/// to be copies of bit 47, i.e. the most significant bit. Addresses that fulfil this criterium
/// are called “canonical”. This type guarantees that it always represents a canonical address.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct VirtAddr(u64);

/// A passed `u64` was not a valid virtual address.
///
/// This means that bits 48 to 64 are not
/// a valid sign extension and are not null either. So automatic sign extension would have
/// overwritten possibly meaningful bits. This likely indicates a bug, for example an invalid
/// address calculation.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtAddrNotValid(u64);

impl VirtAddr {
    /// Creates a new canonical virtual address.
    ///
    /// This function performs sign extension of bit 47 to make the address canonical. Panics
    /// if the bits in the range 48 to 64 contain data (i.e. are not null and not a sign extension).
    ///
    /// @todo Support ASID byte in top bits of the address.
    pub fn new(addr: u64) -> VirtAddr {
        Self::try_new(addr).expect(
            "address passed to VirtAddr::new must not contain any data \
             in bits 48 to 64",
        )
    }

    /// Tries to create a new canonical virtual address.
    ///
    /// This function tries to performs sign extension of bit 47 to make the address canonical.
    /// It succeeds if bits 48 to 64 are either a correct sign extension (i.e. copies of bit 47)
    /// or all null. Else, an error is returned.
    pub fn try_new(addr: u64) -> Result<VirtAddr, VirtAddrNotValid> {
        match addr.get_bits(47..64) {
            0 | 0x1ffff => Ok(VirtAddr(addr)),      // address is canonical
            1 => Ok(VirtAddr::new_unchecked(addr)), // address needs sign extension
            _ => Err(VirtAddrNotValid(addr)),
        }
    }

    /// Creates a new canonical virtual address without checks.
    ///
    /// This function performs sign extension of bit 47 to make the address canonical, so
    /// bits 48 to 64 are overwritten. If you want to check that these bits contain no data,
    /// use `new` or `try_new`.
    pub fn new_unchecked(mut addr: u64) -> VirtAddr {
        if addr.get_bit(47) {
            addr.set_bits(48..64, 0xffff);
        } else {
            addr.set_bits(48..64, 0);
        }
        VirtAddr(addr)
    }

    /// Creates a virtual address that points to `0`.
    pub const fn zero() -> VirtAddr {
        VirtAddr(0)
    }

    /// Converts the address to an `u64`.
    pub fn as_u64(self) -> u64 {
        self.0
    }

    /// Creates a virtual address from the given pointer
    pub fn from_ptr<T>(ptr: *const T) -> Self {
        Self::new(u64::from_usize(ptr as usize))
    }

    /// Converts the address to a raw pointer.
    #[cfg(target_pointer_width = "64")]
    pub fn as_ptr<T>(self) -> *const T {
        usize_from(self.as_u64()) as *const T
    }

    /// Converts the address to a mutable raw pointer.
    #[cfg(target_pointer_width = "64")]
    pub fn as_mut_ptr<T>(self) -> *mut T {
        self.as_ptr::<T>() as *mut T
    }

    /// Aligns the virtual address upwards to the given alignment.
    ///
    /// See the `align_up` free function for more information.
    pub fn aligned_up<U>(self, align: U) -> Self
    where
        U: Into<usize>,
    {
        VirtAddr(align_up(self.0, align.into()))
    }

    /// Aligns the virtual address downwards to the given alignment.
    ///
    /// See the `align_down` free function for more information.
    pub fn aligned_down<U>(self, align: U) -> Self
    where
        U: Into<usize>,
    {
        VirtAddr(align_down(self.0, align.into()))
    }

    /// Checks whether the virtual address has the demanded alignment.
    pub fn is_aligned<U>(self, align: U) -> bool
    where
        U: Into<usize>,
    {
        self.aligned_down(align) == self
    }

    /// Returns the 12-bit page offset of this virtual address.
    pub fn page_offset(&self) -> u12 {
        u12::new((self.0 & 0xfff).try_into().unwrap())
    }
    // ^ @todo this only works for 4KiB pages

    /// Returns the 9-bit level 3 page table index.
    pub fn l3_index(&self) -> u9 {
        u9::new(((self.0 >> 12) & 0o777).try_into().unwrap())
    }

    /// Returns the 9-bit level 2 page table index.
    pub fn l2_index(&self) -> u9 {
        u9::new(((self.0 >> 12 >> 9) & 0o777).try_into().unwrap())
    }

    /// Returns the 9-bit level 1 page table index.
    pub fn l1_index(&self) -> u9 {
        u9::new(((self.0 >> 12 >> 9 >> 9) & 0o777).try_into().unwrap())
    }

    /// Returns the 9-bit level 0 page table index.
    pub fn l0_index(&self) -> u9 {
        u9::new(((self.0 >> 12 >> 9 >> 9 >> 9) & 0o777).try_into().unwrap())
    }

    /// Convert kernel-space virtual address into a physical memory address.
    pub fn kernel_to_user(&self) -> PhysAddr {
        use super::PHYSICAL_MEMORY_OFFSET;
        assert!(self.0 > PHYSICAL_MEMORY_OFFSET);
        PhysAddr::new(self.0 - PHYSICAL_MEMORY_OFFSET)
    }
}

impl fmt::Debug for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "VirtAddr({:#x})", self.0)
    }
}

impl fmt::Binary for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::LowerHex for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Octal for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::UpperHex for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<u64> for VirtAddr {
    fn from(value: u64) -> Self {
        VirtAddr::new(value)
    }
}

impl From<VirtAddr> for u64 {
    fn from(value: VirtAddr) -> Self {
        value.as_u64()
    }
}

impl<T: num::PrimInt + num::ToPrimitive> Add<T> for VirtAddr {
    type Output = Self;
    /// Add a given offset to the current virtual address. Never wraps.
    fn add(self, rhs: T) -> Self::Output {
        // @todo runtime cost of unwrap() here
        VirtAddr::new(self.0.saturating_add(num::cast(rhs).unwrap()))
    }
}

impl<T: num::PrimInt> AddAssign<T> for VirtAddr {
    fn add_assign(&mut self, rhs: T) {
        *self = *self + rhs;
    }
}

impl<T: num::PrimInt> Sub<T> for VirtAddr {
    type Output = Self;
    /// Subtract a given offset from the current virtual address. Never wraps.
    fn sub(self, rhs: T) -> Self::Output {
        // @todo runtime cost of unwrap() here
        VirtAddr::new(self.0.saturating_sub(num::cast(rhs).unwrap()))
    }
}

impl<T: num::PrimInt> SubAssign<T> for VirtAddr {
    fn sub_assign(&mut self, rhs: T) {
        *self = *self - rhs;
    }
}

impl Sub for VirtAddr {
    type Output = u64;
    /// Produce a difference between two virtual addresses.
    fn sub(self, rhs: VirtAddr) -> Self::Output {
        self.as_u64().checked_sub(rhs.as_u64()).unwrap() // @todo use i64?
    }
}

impl<T: num::PrimInt> Rem<T> for VirtAddr {
    type Output = u64;
    fn rem(self, rhs: T) -> Self::Output {
        num::traits::CheckedRem::checked_rem(&self.0, &num::cast(rhs).unwrap()).unwrap()
    }
}

// @todo this is not very useful...
impl<T: num::PrimInt> RemAssign<T> for VirtAddr {
    fn rem_assign(&mut self, rhs: T) {
        *self = VirtAddr::new(num::traits::CheckedRem::checked_rem(&self.0, &num::cast(rhs).unwrap()).unwrap());
    }
}
