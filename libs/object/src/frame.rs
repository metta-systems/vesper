use {
    crate::{CapError, Key, RawKey, Rights, decode_syscall_result},
    core::fmt,
};

#[cfg(not(test))]
use libsyscall::protected_call6;
#[cfg(test)]
use tests::protected_call6;

#[cfg(test)]
#[path = "../tests/support/frame.rs"]
mod tests;

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Userspace handle to a Frame capability.
///
/// Nucleus dispatch supports `Map`, `Unmap`, and `GetAddress`; the wrapper
/// preserves kernel errors. `Remap` remains unsupported (origin-only remap
/// authority is open, D4/D6).
pub struct FrameKey {
    key: Key<FrameType>,
}

enum FrameType {}

/// Existing operation vocabulary; decoding an ID does not imply kernel support.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameOp {
    /// Map the frame into a target Domain's translation context at a virtual
    /// address. Bootstrap-era schema: the explicit target Domain capability
    /// exists so an authorized builder can populate a Domain's address space
    /// before it runs; self-context mapping is the intended ordinary path once
    /// syscall caller identity exists.
    Map = 0,
    /// Unmap the frame through its recorded mapping identity.
    Unmap = 1,
    /// Query the physical extent (requires `GRANT`).
    GetAddress = 2,
    /// Change attributes on an existing mapping; unsupported.
    Remap = 3,
}

impl TryFrom<u64> for FrameOp {
    type Error = CapError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Map),
            1 => Ok(Self::Unmap),
            2 => Ok(Self::GetAddress),
            3 => Ok(Self::Remap),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

impl TryFrom<u32> for FrameOp {
    type Error = CapError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::try_from(u64::from(value))
    }
}

impl FrameKey {
    /// Construct a non-owning handle without installing or validating authority.
    pub const fn from_key(key: RawKey) -> Self {
        Self { key: Key::new(key) }
    }

    /// The wire encoding of the underlying key.
    pub const fn to_wire(&self) -> u64 {
        self.key.to_wire()
    }

    /// Map this frame into the target Domain's translation context at `vaddr`.
    ///
    /// Wire schema (selected 2026-09-15): `x2` target `Domain` key, `x3` virtual
    /// address, `x4` requested rights, `x5` attributes (zero = normal
    /// write-back cacheable; other values rejected), `x6..x7` zero. The
    /// requested rights must be a subset of the frame capability's rights;
    /// execute is not grantable yet. The walk requires every intermediate page
    /// table to be present.
    pub fn map(
        &self,
        domain: RawKey,
        vaddr: u64,
        rights: Rights,
        attrs: u64,
    ) -> Result<(), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe {
            protected_call6(
                self.key.to_wire(),
                FrameOp::Map as u64,
                domain.to_wire(),
                vaddr,
                u64::from(rights.bits()),
                attrs,
                0,
                0,
            )
        };
        decode_syscall_result(result).map(|_| ())
    }

    /// Unmap this frame through its recorded mapping identity.
    ///
    /// Wire schema: no arguments (`x2..x7` zero).
    pub fn unmap(&self) -> Result<(), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result =
            unsafe { protected_call6(self.key.to_wire(), FrameOp::Unmap as u64, 0, 0, 0, 0, 0, 0) };
        decode_syscall_result(result).map(|_| ())
    }

    /// Query the frame's physical extent (requires `GRANT`).
    ///
    /// Returns `(physical base address, size in bytes)`.
    pub fn get_extent(&self) -> Result<(u64, u64), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe {
            protected_call6(
                self.key.to_wire(),
                FrameOp::GetAddress as u64,
                0,
                0,
                0,
                0,
                0,
                0,
            )
        };
        decode_syscall_result(result)
    }
}

impl fmt::Debug for FrameKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FrameKey").field("key", &self.key).finish()
    }
}
