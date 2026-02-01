use {crate::KeySlot, core::marker::PhantomData};

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Capability slot index - strongly typed
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Key<T> {
    slot: KeySlot,
    _marker: PhantomData<T>,
}

impl<T> Key<T> {
    pub const fn new(slot: KeySlot) -> Self {
        Self {
            slot,
            _marker: PhantomData,
        }
    }

    pub const fn slot(&self) -> u32 {
        self.slot.0
    }
}
