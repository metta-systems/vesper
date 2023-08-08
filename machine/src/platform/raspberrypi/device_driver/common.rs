// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2020-2022 Andre Richter <andre.o.richter@gmail.com>

//! Common device driver code.

use {
    crate::memory::{Address, Virtual},
    core::{fmt, marker::PhantomData, ops},
};

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

pub struct MMIODerefWrapper<T> {
    pub base_addr: Address<Virtual>, // @todo unmake public, GPIO::Pin uses it
    phantom: PhantomData<fn() -> T>,
}

/// A wrapper type for usize with integrated range bound check.
#[derive(Copy, Clone)]
pub struct BoundedUsize<const MAX_INCLUSIVE: usize>(usize);

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

impl<T> MMIODerefWrapper<T> {
    /// Create an instance.
    pub const fn new(base_addr: Address<Virtual>) -> Self {
        Self {
            base_addr,
            phantom: PhantomData,
        }
    }
}

// Deref to RegisterBlock
///
/// Allows writing
/// ```
/// self.GPPUD.read()
/// ```
/// instead of something along the lines of
/// ```
/// unsafe { (*GPIO::ptr()).GPPUD.read() }
/// ```
impl<T> ops::Deref for MMIODerefWrapper<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*(self.base_addr.as_usize() as *const _) }
    }
}

impl<const MAX_INCLUSIVE: usize> BoundedUsize<{ MAX_INCLUSIVE }> {
    pub const MAX_INCLUSIVE: usize = MAX_INCLUSIVE;

    /// Creates a new instance if number <= MAX_INCLUSIVE.
    pub const fn new(number: usize) -> Self {
        assert!(number <= MAX_INCLUSIVE);

        Self(number)
    }

    /// Return the wrapped number.
    pub const fn get(self) -> usize {
        self.0
    }
}

impl<const MAX_INCLUSIVE: usize> fmt::Display for BoundedUsize<{ MAX_INCLUSIVE }> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
