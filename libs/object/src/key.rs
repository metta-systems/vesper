use core::marker::PhantomData;

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Capability slot index - typed for safety
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Key<T> {
    slot: u32,
    _marker: PhantomData<T>,
}

impl<T> Key<T> {
    pub const fn new(slot: u32) -> Self {
        Self {
            slot,
            _marker: PhantomData,
        }
    }

    pub const fn slot(&self) -> u32 {
        self.slot
    }
}
