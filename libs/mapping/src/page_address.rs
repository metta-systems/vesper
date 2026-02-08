use {
    core::iter::Step,
    libaddress::{Address, AddressType},
};

/// A wrapper type around [Address] that ensures page alignment.
#[derive(Copy, Clone, Debug, Eq, PartialOrd, PartialEq)]
pub struct PageAddress<ATYPE: const AddressType, const PAGE_SIZE: usize> {
    inner: Address<ATYPE>,
}

impl<ATYPE: const AddressType, const PAGE_SIZE: usize> PageAddress<ATYPE, PAGE_SIZE> {
    /// Unwraps the value.
    pub fn into_inner(self) -> Address<ATYPE> {
        self.inner
    }

    /// Calculates the offset from the page address.
    ///
    /// `count` is in units of [`PageAddress`]. For example, a count of 2 means `result = self + 2 *
    /// page_size`.
    pub fn checked_page_offset(self, count: isize) -> Option<Self> {
        if count == 0 {
            return Some(self);
        }

        let delta = count.unsigned_abs().checked_mul(PAGE_SIZE)? as u64;
        let result = if count.is_positive() {
            self.inner.as_u64().checked_add(delta)?
        } else {
            self.inner.as_u64().checked_sub(delta)?
        };

        Some(Self {
            inner: Address::<ATYPE>::new(result),
        })
    }
}

impl<ATYPE: const AddressType, const PAGE_SIZE: usize> From<usize>
    for PageAddress<ATYPE, PAGE_SIZE>
{
    fn from(addr: usize) -> Self {
        assert!(
            libaddress::align::is_aligned(addr as u64, PAGE_SIZE as u64),
            "Input usize not page aligned"
        );

        Self {
            inner: Address::<ATYPE>::new(addr as u64),
        }
    }
}

impl<ATYPE: const AddressType, const PAGE_SIZE: usize> From<Address<ATYPE>>
    for PageAddress<ATYPE, PAGE_SIZE>
{
    fn from(addr: Address<ATYPE>) -> Self {
        assert!(
            addr.is_page_aligned(&PAGE_SIZE),
            "Input Address not page aligned"
        );

        Self { inner: addr }
    }
}

impl<ATYPE: const AddressType, const PAGE_SIZE: usize> Step for PageAddress<ATYPE, PAGE_SIZE> {
    fn steps_between(start: &Self, end: &Self) -> (usize, Option<usize>) {
        if start > end {
            return (0, None);
        }

        // Since start <= end, do unchecked arithmetic.
        let steps = (end.inner.as_usize() - start.inner.as_usize()) / PAGE_SIZE;
        (steps, Some(steps))
    }

    fn forward_checked(start: Self, count: usize) -> Option<Self> {
        start.checked_page_offset(count.cast_signed())
    }

    fn backward_checked(start: Self, count: usize) -> Option<Self> {
        start.checked_page_offset(-(count.cast_signed()))
    }
}
