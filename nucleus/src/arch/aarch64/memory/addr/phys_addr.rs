/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

use {
    crate::{
        memory::VirtAddr,
        mm::{align_down, align_up},
    },
    bit_field::BitField,
    core::{
        convert::{From, Into},
        fmt,
        ops::{Add, AddAssign, Shl, Shr, Sub, SubAssign},
    },
    usize_conversions::FromUsize,
};

/// A 64-bit physical memory address.
///
/// This is a wrapper type around an `u64`, so it is always 8 bytes, even when compiled
/// on non 64-bit systems. The `UsizeConversions` trait can be used for performing conversions
/// between `u64` and `usize`.
///
/// On `aarch64`, only the 52 lower bits of a physical address can be used. The top 12 bits need
/// to be zero. This type guarantees that it always represents a valid physical address.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct PhysAddr(u64);

/// A passed `u64` was not a valid physical address.
///
/// This means that bits 52 to 64 were not all null.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysAddrNotValid(u64);

impl PhysAddr {
    /// Creates a new physical address.
    ///
    /// Panics if any bits in the bit position 52 to 64 is set.
    pub fn new(addr: u64) -> PhysAddr {
        assert_eq!(
            addr.get_bits(52..64),
            0,
            "physical addresses must not have any set bits in positions 52 to 64"
        );
        PhysAddr(addr)
    }

    /// Tries to create a new physical address.
    ///
    /// Fails if any bits in the bit positions 52 to 64 are set.
    pub fn try_new(addr: u64) -> Result<PhysAddr, PhysAddrNotValid> {
        match addr.get_bits(52..64) {
            0 => Ok(PhysAddr(addr)), // address is valid
            _ => Err(PhysAddrNotValid(addr)),
        }
    }

    /// Creates a physical address that points to `0`.
    pub const fn zero() -> PhysAddr {
        PhysAddr(0)
    }

    /// Converts the address to an `u64`.
    pub fn as_u64(self) -> u64 {
        self.0
    }

    /// Convenience method for checking if a physical address is null.
    pub fn is_null(&self) -> bool {
        self.0 == 0
    }

    /// Aligns the physical address upwards to the given alignment.
    ///
    /// See the `align_up` function for more information.
    pub fn aligned_up<U>(self, align: U) -> Self
    where
        U: Into<usize>,
    {
        PhysAddr(align_up(self.0, align.into()))
    }

    /// Aligns the physical address downwards to the given alignment.
    ///
    /// See the `align_down` function for more information.
    pub fn aligned_down<U>(self, align: U) -> Self
    where
        U: Into<usize>,
    {
        PhysAddr(align_down(self.0, align.into()))
    }

    /// Checks whether the physical address has the demanded alignment.
    pub fn is_aligned<U>(self, align: U) -> bool
    where
        U: Into<usize>,
    {
        self.aligned_down(align) == self
    }

    /// Convert physical memory address into a kernel virtual address.
    pub fn user_to_kernel(&self) -> VirtAddr {
        use super::PHYSICAL_MEMORY_OFFSET;
        assert!(self.0 < !PHYSICAL_MEMORY_OFFSET); // Can't have phys address over 1GiB then
        VirtAddr::new(self.0 + PHYSICAL_MEMORY_OFFSET)
    }
}

impl fmt::Debug for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PhysAddr({:#x})", self.0)
    }
}

impl fmt::Binary for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::LowerHex for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Octal for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::UpperHex for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<u64> for PhysAddr {
    fn from(value: u64) -> Self {
        PhysAddr::new(value)
    }
}

impl From<PhysAddr> for u64 {
    fn from(value: PhysAddr) -> Self {
        value.as_u64()
    }
}

impl From<PhysAddr> for u128 {
    fn from(value: PhysAddr) -> Self {
        value.as_u64() as u128
    }
}

impl Add<u64> for PhysAddr {
    type Output = Self;
    fn add(self, rhs: u64) -> Self::Output {
        PhysAddr::new(self.0 + rhs)
    }
}

impl AddAssign<u64> for PhysAddr {
    fn add_assign(&mut self, rhs: u64) {
        *self = *self + rhs;
    }
}

impl Add<usize> for PhysAddr
where
    u64: FromUsize,
{
    type Output = Self;
    fn add(self, rhs: usize) -> Self::Output {
        self + u64::from_usize(rhs)
    }
}

impl AddAssign<usize> for PhysAddr
where
    u64: FromUsize,
{
    fn add_assign(&mut self, rhs: usize) {
        self.add_assign(u64::from_usize(rhs))
    }
}

impl Sub<u64> for PhysAddr {
    type Output = Self;
    fn sub(self, rhs: u64) -> Self::Output {
        PhysAddr::new(self.0.checked_sub(rhs).unwrap())
    }
}

impl SubAssign<u64> for PhysAddr {
    fn sub_assign(&mut self, rhs: u64) {
        *self = *self - rhs;
    }
}

impl Sub<usize> for PhysAddr
where
    u64: FromUsize,
{
    type Output = Self;
    fn sub(self, rhs: usize) -> Self::Output {
        self - u64::from_usize(rhs)
    }
}

impl SubAssign<usize> for PhysAddr
where
    u64: FromUsize,
{
    fn sub_assign(&mut self, rhs: usize) {
        self.sub_assign(u64::from_usize(rhs))
    }
}

impl Sub<PhysAddr> for PhysAddr {
    type Output = u64;
    fn sub(self, rhs: PhysAddr) -> Self::Output {
        self.as_u64().checked_sub(rhs.as_u64()).unwrap()
    }
}

impl Shr<usize> for PhysAddr {
    type Output = PhysAddr;

    fn shr(self, shift: usize) -> Self::Output {
        PhysAddr::new(self.0 >> shift)
    }
}

impl Shl<usize> for PhysAddr {
    type Output = PhysAddr;

    fn shl(self, shift: usize) -> Self::Output {
        PhysAddr::new(self.0 << shift)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test_case]
    pub fn test_invalid_phys_addr() {
        let result = PhysAddr::try_new(0xfafa_0123_3210_3210);
        if let Err(e) = result {
            assert_eq!(e, PhysAddrNotValid(0xfafa_0123_3210_3210));
        } else {
            assert!(false)
        }
    }
}
