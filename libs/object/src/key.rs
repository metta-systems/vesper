use {
    crate::KeySlot,
    core::{fmt, marker::PhantomData},
};

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Caller-local capability identity, without a claim that the key is valid.
///
/// Wire encoding is explicit: incarnation in bits 63–32, slot in bits 31–0.
/// Every bit pattern is representable; only the kernel validates authority,
/// slot bounds, incarnation and object lifetime.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RawKey {
    slot: u32,
    incarnation: u32,
}

impl RawKey {
    pub const fn new(slot: KeySlot, incarnation: u32) -> Self {
        Self {
            slot: slot.0,
            incarnation,
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "explicitly split the two wire halves"
    )]
    pub const fn from_wire(wire: u64) -> Self {
        Self {
            slot: wire as u32,
            incarnation: (wire >> 32) as u32,
        }
    }

    #[allow(
        clippy::cast_lossless,
        reason = "From is not const without a feature gate"
    )]
    pub const fn to_wire(&self) -> u64 {
        ((self.incarnation as u64) << 32) | (self.slot as u64)
    }

    pub const fn slot(&self) -> KeySlot {
        KeySlot(self.slot)
    }

    pub const fn incarnation(&self) -> u32 {
        self.incarnation
    }
}

/// Capability slot index - strongly typed
///
/// Contract update: this names a particular incarnation, not a slot's future
/// occupant. It is a non-owning handle: copying does not derive authority and
/// dropping does not delete the capability. The phantom type is only ergonomic.
#[repr(transparent)]
pub struct Key<T> {
    raw: RawKey,
    _marker: PhantomData<T>,
}

impl<T> Key<T> {
    pub const fn new(raw: RawKey) -> Self {
        Self {
            raw,
            _marker: PhantomData,
        }
    }

    pub const fn raw(&self) -> RawKey {
        self.raw
    }

    pub const fn to_wire(&self) -> u64 {
        self.raw.to_wire()
    }

    /// Descriptive slot access only; invocation must submit the complete key.
    pub const fn slot(&self) -> u32 {
        self.raw.slot().0
    }
}

impl<T> Copy for Key<T> {}

#[allow(
    clippy::expl_impl_clone_on_copy,
    reason = "derive would require T: Clone"
)]
impl<T> Clone for Key<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> PartialEq for Key<T> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl<T> Eq for Key<T> {}

impl<T> fmt::Debug for Key<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Key")
            .field("raw", &self.raw)
            .field("_marker", &self._marker)
            .finish()
    }
}

/// Invalid submitted key input, checked before capability consistency.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum InvalidKeyReason {
    ZeroIncarnation = 1,
    SlotOutOfRange = 2,
    NeverIssued = 3,
}

impl TryFrom<u8> for InvalidKeyReason {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::ZeroIncarnation),
            2 => Ok(Self::SlotOutOfRange),
            3 => Ok(Self::NeverIssued),
            _ => Err(()),
        }
    }
}

/// Changed capability or object identity; no replacement key is implied.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum InconsistencyReason {
    SlotIncarnationMismatch = 1,
    CapabilityInvalidated = 2,
    ObjectRetired = 3,
}

impl TryFrom<u8> for InconsistencyReason {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::SlotIncarnationMismatch),
            2 => Ok(Self::CapabilityInvalidated),
            3 => Ok(Self::ObjectRetired),
            _ => Err(()),
        }
    }
}
