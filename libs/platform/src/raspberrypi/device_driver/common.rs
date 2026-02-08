// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2020-2022 Andre Richter <andre.o.richter@gmail.com>

//! Common device driver code.
// @todo: Move to libprimitive or libdriver or sth?

use {
    core::{marker::PhantomData, ops},
    libaddress::{Address, Virtual},
};

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

#[allow(clippy::partial_pub_fields)]
pub struct MMIODerefWrapper<T> {
    pub base_addr: Address<Virtual>, // @todo unmake public, GPIO::Pin uses it
    phantom: PhantomData<fn() -> T>,
}

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
/// ```ignore
/// self.GPPUD.read()
/// ```
/// instead of something along the lines of
/// ```ignore
/// unsafe { (*GPIO::ptr()).GPPUD.read() }
/// ```
impl<T> ops::Deref for MMIODerefWrapper<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: Not safe.
        unsafe { &*(self.base_addr.as_usize() as *const _) }
    }
}
