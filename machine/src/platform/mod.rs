/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */
use core::{marker::PhantomData, ops};

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

pub mod rpi3;

#[cfg(any(feature = "rpi3", feature = "rpi4"))]
pub use rpi3::*;

pub struct MMIODerefWrapper<T> {
    base_addr: usize,
    phantom: PhantomData<fn() -> T>,
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

impl<T> MMIODerefWrapper<T> {
    /// Create an instance.
    ///
    /// # Safety
    ///
    /// Unsafe, duh!
    pub const unsafe fn new(start_addr: usize) -> Self {
        Self {
            base_addr: start_addr,
            phantom: PhantomData,
        }
    }
}

/// Deref to RegisterBlock
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
        unsafe { &*(self.base_addr as *const _) }
    }
}
