//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

#![no_std]
#![feature(const_trait_impl)]
#![feature(const_ops)]
#![feature(const_convert)]

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

/// A canonical 64-bit virtual memory address.
///
/// This is a wrapper type around an `u64`, so it is always 8 bytes, even when compiled
/// on non 64-bit systems. The `UsizeConversions` trait can be used for performing conversions
/// between `u64` and `usize`.
///
/// On `x86_64`, only the 48 lower bits of a virtual address can be used. The top 16 bits need
/// to be copies of bit 47, i.e. the most significant bit. Addresses that fulfil this criterium
/// are called “canonical”. This type guarantees that it always represents a canonical address.
pub type VirtAddr = Address<Virtual>;

/// Address of all physical memory mapping, so that kernel can operate anywhere.
pub const PHYSICAL_KERNEL_WINDOW: u64 = 0xffff_f000_0000_0000;

/// Metadata trait for marking the type of an address.
pub const trait AddressType: Copy + Clone + PartialOrd + PartialEq + Ord + Eq {
    const NAME: &'static str;
    fn validate(addr: u64) -> Result<u64, (u64, &'static str)>; // Ok(canonical_addr) or Err(raw, reason)
}

pub const trait PageSize {
    fn alignment(&self) -> u64;
    fn mask(&self) -> u64;
}

impl const PageSize for u64 {
    fn alignment(&self) -> u64 {
        *self
    }
    fn mask(&self) -> u64 {
        !(self - 1)
    }
}

impl const PageSize for usize {
    fn alignment(&self) -> u64 {
        *self as u64
    }
    fn mask(&self) -> u64 {
        !(self - 1) as u64
    }
}

/// Zero-sized type to mark a physical address.
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq, Ord, Eq)]
pub enum Physical {}

/// Zero-sized type to mark a virtual address.
#[derive(Copy, Clone, Debug, PartialOrd, PartialEq, Ord, Eq)]
pub enum Virtual {}

/// Generic address type.
///
/// This is a wrapper type around an `u64`, so it is always 8 bytes, even when compiled
/// on non 64-bit systems. The `UsizeConversions` trait can be used for performing conversions
/// between `u64` and `usize`.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Address<ATYPE: AddressType> {
    pub(crate) value: u64,
    pub(crate) _address_type: PhantomData<fn() -> ATYPE>,
}

const _: () = assert!(core::mem::size_of::<Address<Physical>>() == core::mem::size_of::<u64>());
const _: () = assert!(core::mem::size_of::<Address<Virtual>>() == core::mem::size_of::<u64>());

/// A passed `u64` was not a valid address.
///
/// What this means exactly depends on architecture assumptions.
/// Arch crate is expected to implement this trait and provide
/// additional information about why this address is invalid.
#[derive(Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct AddressNotValid<ATYPE: AddressType> {
    pub(crate) value: u64,
    pub(crate) _address_type: PhantomData<fn() -> ATYPE>,
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

//=================================================================================================
// AddressNotValid
//=================================================================================================

impl<T: AddressType> AddressNotValid<T> {
    pub const fn new(value: u64) -> Self {
        Self {
            value,
            _address_type: PhantomData,
        }
    }
}

//=================================================================================================
// Physical/Virtual
//=================================================================================================

#[bitfield(u64)]
struct PhysTopBits {
    #[bits(52)]
    address: i64,
    #[bits(12)] // bits(52..64)
    top_bits: u16,
}

impl const AddressType for Physical {
    const NAME: &'static str = "PhysAddr";

    /// Panics if any bits in the bit position 52 to 64 is set.
    /// TODO: this is arch-dependent!.
    fn validate(addr: u64) -> Result<u64, (u64, &'static str)> {
        match PhysTopBits(addr).top_bits() {
            0 => Ok(addr),
            _ => Err((
                addr,
                "physical addresses must not have any set bits in positions 52 to 64",
            )),
        }
    }
}

#[bitfield(u64)]
struct VirtTopBits {
    #[bits(47)]
    address: i64,
    #[bits(17)] // bits(47..64)
    top_bits: u32,
}

impl const AddressType for Virtual {
    const NAME: &'static str = "VirtAddr";

    /// This function tries to performs sign extension of bit 47 to make the address canonical.
    /// It succeeds if bits 47 to 64 are a correct sign extension or all null (i.e. copies of bit 47).
    ///
    /// An error means that bits 48 to 64 are not a valid sign extension and are not null either.
    /// So automatic sign extension would have overwritten possibly meaningful bits.
    /// This likely indicates a bug, for example an invalid address calculation.
    /// TODO: Support ASID byte in top bits of the address.
    fn validate(addr: u64) -> Result<u64, (u64, &'static str)> {
        match VirtTopBits(addr).top_bits() {
            0 | 0x1ffff => Ok(addr), // address is canonical
            1 => {
                // address needs sign extension
                let mut addr = VirtTopBits(addr);
                addr.set_top_bits(0x1ffff);
                Ok(addr.into_bits())
            }
            _ => Err((
                addr,
                "virtual address must not contain any data in bits 48 to 64",
            )),
        }
    }
}

//=================================================================================================
// Address
//=================================================================================================

impl<ATYPE: const AddressType> Default for Address<ATYPE> {
    fn default() -> Self {
        Self::zero()
    }
}

impl<ATYPE: const AddressType> Address<ATYPE> {
    /// Create an address without checking.
    /// # Safety
    pub const unsafe fn new_unchecked(value: u64) -> Self {
        Self {
            value,
            _address_type: PhantomData,
        }
    }

    /// Creates a new address.
    ///
    /// Panics if address is not representable.
    pub const fn new(addr: u64) -> Self {
        match ATYPE::validate(addr) {
            Ok(addr) => unsafe { Address::<ATYPE>::new_unchecked(addr) },
            Err((_addr, message)) => panic!("{}", message),
        }
    }

    /// Tries to create a new address.
    pub const fn try_new(addr: u64) -> Result<Self, AddressNotValid<ATYPE>> {
        match ATYPE::validate(addr) {
            Ok(addr) => Ok(unsafe { Address::<ATYPE>::new_unchecked(addr) }),
            Err((addr, _message)) => Err(AddressNotValid::<ATYPE>::new(addr)),
        }
    }

    /// Converts the address to an `u64`.
    pub const fn as_u64(&self) -> u64 {
        self.value
    }

    pub fn as_usize(&self) -> usize {
        self.value.try_into().unwrap()
    }

    /// Creates an address that points to `0`.
    pub const fn zero() -> Address<ATYPE> {
        Self {
            value: 0,
            _address_type: PhantomData,
        }
    }

    /// Converts the address to a raw pointer.
    pub const fn into_ptr<T>(self) -> *const T {
        self.value as *const T
    }

    /// Converts the address to a raw pointer.
    pub const fn as_ptr<T>(&self) -> *const T {
        self.value as *const T
    }

    /// Converts the address to a mutable raw pointer.
    pub const fn into_mut_ptr<T>(self) -> *mut T {
        self.value as *mut T
    }

    /// Converts the address to a mutable raw pointer.
    pub const fn as_mut_ptr<T>(&self) -> *mut T {
        self.value as *mut T
    }

    /// Convenience method for checking if a physical address is null.
    pub fn is_null(&self) -> bool {
        self.value == 0
    }

    /// Creates an address from the given pointer
    pub fn from_ptr<T>(ptr: *const T) -> Self {
        Self::new(u64::from_usize(ptr as usize))
    }

    /// Creates a virtual address from the given pointer
    pub fn from_mut_ptr<T>(ptr: *mut T) -> Self {
        Self::new(u64::from_usize(ptr as usize))
    }

    // TODO: With pageSize parameterized we can move it out to platform-independent code.

    /// Align down to page size.
    #[must_use]
    pub const fn align_down_page(&self, page_size: &impl const PageSize) -> Self {
        let aligned = align::align_down(self.value, page_size.alignment());
        Self::new(aligned)
    }

    /// Align up to page size.
    #[must_use]
    pub const fn align_up_page(&self, page_size: &impl const PageSize) -> Self {
        let aligned = align::align_up(self.value, page_size.alignment());
        Self::new(aligned)
    }

    /// Checks if the address is page aligned.
    pub const fn is_page_aligned(&self, page_size: &impl const PageSize) -> bool {
        align::is_aligned(self.value, page_size.alignment())
    }

    /// Return the address' offset into the corresponding page.
    pub const fn offset_into_page(&self, page_size: &impl const PageSize) -> u64 {
        self.value & page_size.mask()
    }

    /// Aligns the address upwards to the given alignment.
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

    /// Aligns the address downwards to the given alignment.
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

    /// Checks whether the address has the required alignment.
    pub fn is_aligned<U: Into<u64>>(self, align: U) -> bool {
        align::is_aligned(self.value, align.into())
    }
}

//=================================================================================================
// Address<Physical> specifics
//=================================================================================================

impl Address<Physical> {
    /// Convert physical memory address into a kernel-view virtual address for physical memory.
    pub fn user_to_kernel(&self) -> Address<Virtual> {
        assert!(self.value < !PHYSICAL_KERNEL_WINDOW);
        Address::<Virtual>::new(self.value + PHYSICAL_KERNEL_WINDOW)
    }
}

//=================================================================================================
// Address<Virtual> specifics
//=================================================================================================

impl Address<Virtual> {
    /// Creates a new canonical virtual address without checks (overwriting top bits).
    ///
    /// This function performs sign extension of bit 47 to make the address canonical, so
    /// bits 48 to 64 are overwritten. If you want to check that these bits contain no data,
    /// use `new` or `try_new`.
    pub const fn new_canonical(addr: u64) -> Self {
        let mut v = VirtTopBits(addr);
        if v.top_bits() & 1 != 0 {
            v.set_top_bits(0x1ffff);
        } else {
            v.set_top_bits(0);
        }
        unsafe { Self::new_unchecked(v.into_bits()) }
    }

    // @todo Support ASID byte in top bits of the address.
    // pub fn with_asid(addr: u64, asid: ASID) -> Address<Virtual> {}

    // TODO: The following index and page fns should be accessible through a PageSize trait or something?

    /// Returns the 12-bit page offset of this virtual address.
    pub fn page_offset(&self) -> u12 {
        u12::new((self.value & 0xfff).try_into().unwrap())
    }
    // ^ @todo this only works for 4KiB pages

    /// Returns the 9-bit level 3 page table index.
    pub fn l3_index(&self) -> u9 {
        u9::new(((self.value >> 12) & 0o777).try_into().unwrap())
    }

    /// Returns the 9-bit level 2 page table index.
    pub fn l2_index(&self) -> u9 {
        u9::new(((self.value >> 12 >> 9) & 0o777).try_into().unwrap())
    }

    /// Returns the 9-bit level 1 page table index.
    pub fn l1_index(&self) -> u9 {
        u9::new(((self.value >> 12 >> 9 >> 9) & 0o777).try_into().unwrap())
    }

    /// Returns the 9-bit level 0 page table index.
    pub fn l0_index(&self) -> u9 {
        u9::new(
            ((self.value >> 12 >> 9 >> 9 >> 9) & 0o777)
                .try_into()
                .unwrap(),
        )
    }

    pub const fn is_higher_half(self) -> bool {
        self.value >= 0xFFFF_0000_0000_0000
    }

    /// Convert kernel-view virtual address of physical memory into a physical memory address.
    pub fn kernel_to_user(&self) -> Address<Physical> {
        assert!(self.value >= PHYSICAL_KERNEL_WINDOW);
        Address::<Physical>::new(self.value - PHYSICAL_KERNEL_WINDOW)
    }
}

//=================================================================================================
// From
//=================================================================================================

impl<ATYPE: const AddressType> From<u64> for Address<ATYPE> {
    fn from(value: u64) -> Self {
        Self::new(value)
    }
}

impl<ATYPE: const AddressType> From<usize> for Address<ATYPE> {
    fn from(value: usize) -> Self {
        Self::new(value as u64)
    }
}

impl<ATYPE: const AddressType> From<Address<ATYPE>> for u64 {
    fn from(value: Address<ATYPE>) -> Self {
        value.as_u64()
    }
}

impl<ATYPE: const AddressType> From<Address<ATYPE>> for u128 {
    fn from(value: Address<ATYPE>) -> Self {
        u128::from(value.as_u64())
    }
}

//=================================================================================================
// Add/AddAssign
//=================================================================================================

impl<ATYPE: const AddressType, T: num::PrimInt + num::ToPrimitive> Add<T> for Address<ATYPE> {
    type Output = Self;

    /// Add a given offset to the current virtual address. Never wraps.
    #[inline(always)]
    fn add(self, rhs: T) -> Self::Output {
        // @todo runtime cost of unwrap() here
        // VirtAddr::new(self.value.saturating_add(num::cast(rhs).unwrap()))
        match self.value.checked_add(num::cast(rhs).unwrap()) {
            None => panic!("Overflow on Address::add"),
            Some(x) => Self::new(x),
        }
    }
}

// FIXME: this is already a default impl?
impl<ATYPE: const AddressType, T: num::PrimInt + num::ToPrimitive> AddAssign<T> for Address<ATYPE> {
    fn add_assign(&mut self, rhs: T) {
        *self = *self + rhs;
    }
}

//=================================================================================================
// Sub/SubAssign
//=================================================================================================

// Difference of two addresses is a size.
impl<ATYPE: const AddressType> Sub<Address<ATYPE>> for Address<ATYPE> {
    type Output = usize;

    fn sub(self, rhs: Address<ATYPE>) -> Self::Output {
        match self.value.checked_sub(rhs.value) {
            None => panic!("Overflow on Address::sub"),
            Some(x) => x as usize,
        }
    }
}

impl<ATYPE: const AddressType, T: num::PrimInt + num::ToPrimitive> Sub<T> for Address<ATYPE> {
    type Output = Self;

    fn sub(self, rhs: T) -> Self::Output {
        Address::<ATYPE>::new(self.value.checked_sub(num::cast(rhs).unwrap()).unwrap())
    }
}

impl<ATYPE: const AddressType, T: num::PrimInt + num::ToPrimitive> SubAssign<T> for Address<ATYPE> {
    fn sub_assign(&mut self, rhs: T) {
        *self = *self - rhs;
    }
}

//=================================================================================================
// Shr/Shl
//=================================================================================================

impl<ATYPE: const AddressType> Shr<usize> for Address<ATYPE> {
    type Output = Self;

    fn shr(self, shift: usize) -> Self::Output {
        Address::<ATYPE>::new(self.value >> shift)
    }
}

impl<ATYPE: const AddressType> Shl<usize> for Address<ATYPE> {
    type Output = Self;

    fn shl(self, shift: usize) -> Self::Output {
        Address::<ATYPE>::new(self.value << shift)
    }
}

//=================================================================================================
// Rem/RemAssign
//=================================================================================================

impl<ATYPE: const AddressType, T: num::PrimInt> Rem<T> for Address<ATYPE> {
    type Output = u64;

    fn rem(self, rhs: T) -> Self::Output {
        num::traits::CheckedRem::checked_rem(&self.value, &num::cast(rhs).unwrap()).unwrap()
    }
}

// @todo this is not very useful...
impl<ATYPE: const AddressType, T: num::PrimInt> RemAssign<T> for Address<ATYPE> {
    fn rem_assign(&mut self, rhs: T) {
        *self = Address::<ATYPE>::new(
            num::traits::CheckedRem::checked_rem(&self.value, &num::cast(rhs).unwrap()).unwrap(),
        );
    }
}

//=================================================================================================
// Display/Debug/fmt
//=================================================================================================

impl<ATYPE: AddressType> fmt::Debug for Address<ATYPE> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}({:#x})", ATYPE::NAME, self.value)
    }
}

impl fmt::Display for Address<Physical> {
    // Don't expect to see physical addresses greater than 40 bit.
    #[allow(clippy::cast_possible_truncation)]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let q3: u8 = ((self.value >> 32) & 0xff) as u8;
        let q2: u16 = ((self.value >> 16) & 0xffff) as u16;
        let q1: u16 = (self.value & 0xffff) as u16;

        write!(f, "pa")?;
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

        write!(f, "va")?;
        write!(f, "{q4:04x}_")?;
        write!(f, "{q3:04x}_")?;
        write!(f, "{q2:04x}_")?;
        write!(f, "{q1:04x}")
    }
}

impl<ATYPE: AddressType> fmt::Binary for Address<ATYPE> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.value.fmt(f)
    }
}

impl<ATYPE: AddressType> fmt::LowerHex for Address<ATYPE> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.value.fmt(f)
    }
}

impl<ATYPE: AddressType> fmt::UpperHex for Address<ATYPE> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.value.fmt(f)
    }
}

impl<ATYPE: AddressType> fmt::Octal for Address<ATYPE> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.value.fmt(f)
    }
}
