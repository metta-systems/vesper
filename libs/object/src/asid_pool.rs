use {
    crate::{CapError, Key, RawKey, decode_syscall_result},
    core::fmt,
};

#[cfg(not(test))]
use libsyscall::protected_call6;
#[cfg(test)]
use tests::protected_call6;

#[cfg(test)]
#[path = "../tests/support/asid_pool.rs"]
mod tests;

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Userspace handle to an `ASIDPool` capability.
///
/// Nucleus dispatch supports `Assign`: binding an ASID from this pool to a
/// Domain's translation root.
pub struct ASIDPoolKey {
    key: Key<ASIDPoolType>,
}

enum ASIDPoolType {}

/// Existing operation vocabulary; decoding an ID does not imply kernel support.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ASIDPoolOp {
    /// Assign an ASID from this pool to the target Domain's translation root.
    /// The Domain must have a root installed and no ASID yet; the pool
    /// capability requires `GRANT` and the Domain capability `MAP`.
    Assign = 0,
}

impl TryFrom<u64> for ASIDPoolOp {
    type Error = CapError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Assign),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

impl TryFrom<u32> for ASIDPoolOp {
    type Error = CapError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::try_from(u64::from(value))
    }
}

impl ASIDPoolKey {
    /// Construct a non-owning handle without installing or validating authority.
    pub const fn from_key(key: RawKey) -> Self {
        Self { key: Key::new(key) }
    }

    /// The wire encoding of the underlying key.
    pub const fn to_wire(&self) -> u64 {
        self.key.to_wire()
    }

    /// Assign an ASID from this pool to `domain`'s translation root.
    ///
    /// Wire schema (selected 2026-09-15): `x2` target `Domain` key,
    /// `x3..x7` zero. Success returns the assigned ASID in `x1` and zero in
    /// `x2`. The Domain must have a translation root installed (`NotMapped`
    /// otherwise) and no ASID yet (`AlreadyMapped` otherwise); pool
    /// exhaustion is `ASIDPoolExhausted`. Authority: `GRANT` on this pool
    /// capability, `MAP` on the Domain capability.
    pub fn assign(&self, domain: RawKey) -> Result<u16, CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe {
            protected_call6(
                self.key.to_wire(),
                ASIDPoolOp::Assign as u64,
                domain.to_wire(),
                0,
                0,
                0,
                0,
                0,
            )
        };
        decode_syscall_result(result).and_then(|(asid, _)| {
            // The wire contract promises a 16-bit ASID in x1; a wider value is
            // a contract violation, reported as a defined error.
            u16::try_from(asid).map_err(|_too_wide| CapError::InvalidOperation)
        })
    }
}

impl fmt::Debug for ASIDPoolKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ASIDPoolKey")
            .field("key", &self.key)
            .finish()
    }
}
