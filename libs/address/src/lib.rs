//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

#![no_std]

use {
    // bit_field::BitField,
    bitfield_struct::bitfield,
    core::{
        convert::{From, TryInto},
        fmt,
        marker::PhantomData,
        ops::{Add, AddAssign, Rem, RemAssign, Shl, Shr, Sub, SubAssign},
    },
    usize_conversions::FromUsize,
    ux::*,
};

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

pub mod align;

/// A 64-bit physical memory address.
///
/// This is a wrapper type around an `u64`, so it is always 8 bytes, even when compiled
/// on non 64-bit systems. The `UsizeConversions` trait can be used for performing conversions
/// between `u64` and `usize`.
///
/// On `aarch64`, only the 52 lower bits of a physical address can be used. The top 12 bits need
/// to be zero. This type guarantees that it always represents a valid physical address.
pub type PhysAddr = Address<Physical>;

pub type VirtAddr = Address<Virtual>;

/// Address of all physical memory mapping, so that kernel can operate anywhere.
pub const PHYSICAL_KERNEL_WINDOW: u64 = 0xffff_f000_0000_0000;

/// Metadata trait for marking the type of an address.
pub trait AddressType: Copy + Clone + PartialOrd + PartialEq + Ord + Eq {}

/// Zero-sized type to mark a physical address.
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq, Ord, Eq)]
pub enum Physical {}

impl AddressType for Physical {}

/// Zero-sized type to mark a virtual address.
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq, Ord, Eq)]
pub enum Virtual {}

impl AddressType for Virtual {}

/// Generic address type.
///
/// This is a wrapper type around an `u64`, so it is always 8 bytes, even when compiled
/// on non 64-bit systems. The `UsizeConversions` trait can be used for performing conversions
/// between `u64` and `usize`.
#[derive(Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Address<ATYPE: AddressType> {
    pub(crate) value: u64,
    pub(crate) _address_type: PhantomData<fn() -> ATYPE>,
}

/// A passed `u64` was not a valid address.
///
/// What this means exactly depends on architecture assumptions.
/// Arch crate is expected to implement this trait and provide
/// additional information about why this address is invalid.
pub struct AddressNotValid<ATYPE: AddressType> {
    pub(crate) value: u64,
    pub(crate) _address_type: PhantomData<fn() -> ATYPE>,
}

/// A passed `u64` was not a valid physical address.
/// TODO: AddressNotValid<Physical>
///
/// This means that bits 52 to 64 were not all null.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysAddrNotValid(pub u64);

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

impl<T: AddressType> AddressNotValid<T> {
    pub const fn new(value: u64) -> Self {
        Self {
            value,
            _address_type: PhantomData,
        }
    }
}

impl<ATYPE: AddressType> Address<ATYPE> {
    /// Create an instance.
    pub const fn new(value: u64) -> Self {
        Self {
            value,
            _address_type: PhantomData,
        }
    }

    /// Convert to usize.
    pub const fn as_u64(self) -> u64 {
        self.value
    }

    // TODO: With pageSize parameterized we can move it out to platform-independent code.

    /// Align down to page size.
    #[must_use]
    pub const fn align_down_page(&self, pageSize: impl PageSize) -> Self {
        let aligned = align::align_down(self.value, pageSize.alignment());
        Self::new(aligned)
    }

    /// Align up to page size.
    #[must_use]
    pub const fn align_up_page(&self, pageSize: impl PageSize) -> Self {
        let aligned = align::align_up(self.value, pageSize.alignment());
        Self::new(aligned)
    }

    /// Checks if the address is page aligned.
    pub const fn is_page_aligned(&self, pageSize: impl PageSize) -> bool {
        align::is_aligned(self.value, pageSize.alignment())
    }

    /// Return the address' offset into the corresponding page.
    pub const fn offset_into_page(&self, pageSize: impl PageSize) -> usize {
        self.value & pageSize.mask()
    }

    /// Aligns the physical address upwards to the given alignment.
    ///
    /// See the `align_up` function for more information.
    #[must_use]
    pub fn aligned_up<U>(self, align: U) -> Self
    where
        U: Into<u64>,
    {
        Self {
            value: align::align_up(self.value, align.into()),
            _address_type: PhantomData,
        }
    }

    /// Aligns the physical address downwards to the given alignment.
    ///
    /// See the `align_down` function for more information.
    #[must_use]
    pub fn aligned_down<U>(self, align: U) -> Self
    where
        U: Into<u64>,
    {
        Self {
            value: align::align_down(self.value, align.into()),
            _address_type: PhantomData,
        }
    }

    /// Checks whether the physical address has the demanded alignment.
    pub fn is_aligned<U>(self, align: U) -> bool
    where
        U: Into<u64>,
    {
        align::is_aligned(self.value, align)
    }

    /// Creates a virtual address from the given pointer
    pub fn from_ptr<T>(ptr: *const T) -> Self {
        Self::new(u64::from_usize(ptr as usize))
    }

    /// Converts the address to a raw pointer.
    #[cfg(target_pointer_width = "64")]
    pub fn as_ptr<T>(self) -> *const T {
        // @fixme should be into_ptr
        self.as_u64() as *const T
    }

    /// Converts the address to a mutable raw pointer.
    #[cfg(target_pointer_width = "64")] // @fixme this config needs to be passed in as a feature
    pub fn as_mut_ptr<T>(self) -> *mut T {
        // @fixme should be into_mut_ptr
        self.as_ptr::<T>() as *mut T
    }
}

// +
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

// -
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
    #[allow(clippy::cast_possible_truncation)]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let q3: u8 = ((self.value >> 32) & 0xff) as u8;
        let q2: u16 = ((self.value >> 16) & 0xffff) as u16;
        let q1: u16 = (self.value & 0xffff) as u16;

        write!(f, "0x")?;
        write!(f, "{q3:02x}_")?;
        write!(f, "{q2:04x}_")?;
        write!(f, "{q1:04x}")
    }
}

impl fmt::Display for Address<Virtual> {
    #[allow(clippy::cast_possible_truncation)]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let q4: u16 = ((self.value >> 48) & 0xffff) as u16;
        let q3: u16 = ((self.value >> 32) & 0xffff) as u16;
        let q2: u16 = ((self.value >> 16) & 0xffff) as u16;
        let q1: u16 = (self.value & 0xffff) as u16;

        write!(f, "0x")?;
        write!(f, "{q4:04x}_")?;
        write!(f, "{q3:04x}_")?;
        write!(f, "{q2:04x}_")?;
        write!(f, "{q1:04x}")
    }
}

//=======
//=======
//=======
//=======
// PhysAddr
//=======
//=======
//=======
//=======

#[bitfield(u64)]
struct TopBits {
    #[bits(52)]
    address: i64,
    #[bits(12)] //get_bits(52..64),
    top_bits: u16,
}

impl PhysAddr {
    /// Creates a new physical address.
    pub const fn new(addr: u64) -> PhysAddr {
        PhysAddr(addr)
    }

    /// Creates a new physical address.
    ///
    /// Panics if any bits in the bit position 52 to 64 is set (TODO: this is arch-dependent!).
    pub const fn new_checked(addr: u64) -> PhysAddr {
        assert!(
            TopBits(addr).top_bits() == 0,
            "physical addresses must not have any set bits in positions 52 to 64"
        );
        PhysAddr(addr)
    }

    /// Tries to create a new physical address.
    ///
    /// Fails if any bits in the bit positions 52 to 64 are set.
    pub fn try_new(addr: u64) -> Result<PhysAddr, PhysAddrNotValid> {
        match TopBits(addr).top_bits() {
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

    pub const fn as_ptr<T>(self) -> *const T {
        self.0 as *const T
    }

    pub fn as_mut_ptr<T>(self) -> *mut T {
        self.0 as *mut T
    }

    /// Convenience method for checking if a physical address is null.
    pub fn is_null(&self) -> bool {
        self.0 == 0
    }

    /// Aligns the physical address upwards to the given alignment.
    ///
    /// See the `align_up` function for more information.
    #[must_use]
    pub fn aligned_up(self, align: usize) -> Self {
        PhysAddr(
            align::align_up(self.0.try_into().unwrap(), align)
                .try_into()
                .unwrap(),
        )
    }

    /// Aligns the physical address downwards to the given alignment.
    ///
    /// See the `align_down` function for more information.
    #[must_use]
    pub fn aligned_down(self, align: usize) -> Self {
        PhysAddr(
            align::align_down(self.0.try_into().unwrap(), align)
                .try_into()
                .unwrap(),
        )
    }

    /// Checks whether the physical address has the demanded alignment.
    pub fn is_aligned(self, align: usize) -> bool {
        self.aligned_down(align) == self
    }

    /// Convert physical memory address into a kernel-view virtual address for physical memory.
    pub fn user_to_kernel(&self) -> VirtAddr {
        assert!(self.0 < !PHYSICAL_KERNEL_WINDOW);
        VirtAddr::new(self.0 + PHYSICAL_KERNEL_WINDOW)
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
        u128::from(value.as_u64())
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
        self.add_assign(u64::from_usize(rhs));
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
        self.sub_assign(u64::from_usize(rhs));
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

//==========
//==========
//==========
//==========
// VirtAddr
//==========
//==========
//==========
//==========

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
pub struct VirtAddr(pub u64);

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

    //
    // @todo Support ASID byte in top bits of the address.
    // pub fn with_asid(addr: u64, asid: ASID) -> Address<Virtual> {}

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
    pub const fn new_unchecked(addr: u64) -> VirtAddr {
        // FIXME: Constness!
        // if addr.get_bit(47) {
        //     addr.set_bits(48..64, 0xffff);
        // } else {
        //     addr.set_bits(48..64, 0);
        // }
        VirtAddr(addr)
    }

    /// Creates a virtual address that points to `0`.
    pub const fn zero() -> VirtAddr {
        VirtAddr(0)
    }

    /// Converts the address to an `u64`.
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Creates a virtual address from the given pointer
    pub fn from_ptr<T>(ptr: *const T) -> Self {
        Self::new(u64::from_usize(ptr as usize))
    }

    /// Converts the address to a raw pointer.
    #[cfg(target_pointer_width = "64")]
    pub const fn as_ptr<T>(self) -> *const T {
        self.0 as *const T
    }

    /// Converts the address to a mutable raw pointer.
    #[cfg(target_pointer_width = "64")]
    pub fn as_mut_ptr<T>(self) -> *mut T {
        self.as_ptr::<T>().cast_mut()
    }

    /// Aligns the virtual address upwards to the given alignment.
    ///
    /// See the `align_up` free function for more information.
    #[must_use]
    pub fn aligned_up(self, align: usize) -> Self {
        VirtAddr(
            align::align_up(self.0.try_into().unwrap(), align)
                .try_into()
                .unwrap(),
        )
    }

    /// Aligns the virtual address downwards to the given alignment.
    ///
    /// See the `align_down` free function for more information.
    #[must_use]
    pub fn aligned_down(self, align: usize) -> Self {
        VirtAddr(
            align::align_down(self.0.try_into().unwrap(), align)
                .try_into()
                .unwrap(),
        )
    }

    /// Checks whether the virtual address has the demanded alignment.
    pub fn is_aligned(self, align: usize) -> bool {
        self.aligned_down(align) == self
    }

    // The following index and page fns should be accessible through a PageSize trait or something?

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

    pub const fn is_higher_half(self) -> bool {
        self.0 >= 0xFFFF_0000_0000_0000
    }

    /// Convert kernel-view virtual address of physical memory into a physical memory address.
    pub fn kernel_to_user(&self) -> PhysAddr {
        assert!(self.0 > PHYSICAL_KERNEL_WINDOW);
        PhysAddr::new(self.0 - PHYSICAL_KERNEL_WINDOW)
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
        *self = VirtAddr::new(
            num::traits::CheckedRem::checked_rem(&self.0, &num::cast(rhs).unwrap()).unwrap(),
        );
    }
}
