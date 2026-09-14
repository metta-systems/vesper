use {
    crate::{CapError, Key, KeyTableKey, ObjectType, RawKey, Rights, decode_syscall_result},
    core::fmt,
};

#[cfg(not(test))]
use libsyscall::protected_call6;
#[cfg(test)]
use tests::protected_call6;

#[cfg(test)]
#[path = "../tests/support/untyped.rs"]
mod tests;

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Userspace handle to an Untyped capability.
///
/// Nucleus dispatch supports `Retype`; the wrapper preserves kernel errors and
/// returns the first destination-local key.
pub struct UntypedKey {
    key: Key<UntypedType>,
}

enum UntypedType {}

/// Existing operation vocabulary; decoding an ID does not imply kernel support.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum UntypedOp {
    /// Create one or more objects of one kind from the Untyped's unused
    /// watermark range, installing capabilities into consecutive destination
    /// slots and returning the first destination-local key.
    Retype = 0,
}

impl TryFrom<u64> for UntypedOp {
    type Error = CapError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Retype),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

impl TryFrom<u32> for UntypedOp {
    type Error = CapError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::try_from(u64::from(value))
    }
}

// ┌────────────────────────────────────────────────────────────┐
// │  ALLOWED OPERATIONS ON UNTYPED                             │
// ├────────────────────────────────────────────────────────────┤
// │  ✓ Untyped_Retype  → Create children (objects/sub-untypeds)│
// │  ✓ CNode_Revoke    → Delete all children, reset watermark  │
// │  ✓ CNode_Delete    → Delete this cap (if no children)      │
// │  ✓ CNode_Move      → Move cap to different slot            │
// ├────────────────────────────────────────────────────────────┤
// │  DISALLOWED                                                │
// ├────────────────────────────────────────────────────────────┤
// │  ✗ CNode_Copy      → Cannot duplicate                      │
// │  ✗ CNode_Mint      → Cannot derive with reduced rights     │
// │  ✗ CNode_Mutate    → Cannot modify                        │
// └────────────────────────────────────────────────────────────┘
// Contract status (2026-09-14): Retype is implemented (initial KeyTable
// kind allowlist). Revoke remains unsupported pending its scope/completion
// contract (D2). Delete/Move of the Untyped entry apply through the KeyTable
// management operations like any other entry. Copy/Mint of an Untyped stay
// rejected: one region must not gain independent allocation watermarks.

impl UntypedKey {
    /// Construct a non-owning handle without installing or validating authority.
    pub const fn from_key(key: RawKey) -> Self {
        Self { key: Key::new(key) }
    }

    /// Retype `count` objects of `kind` from this Untyped's unused watermark
    /// range, installing capabilities into `count` consecutive vacant slots of
    /// the destination table.
    ///
    /// Wire schema (approved 2026-09-13): `x2` object kind, `x3` `size_bits`,
    /// `x4` count, `x5` destination-table key, `x6` first destination slot,
    /// `x7` requested rights. Returns the first destination-local key from
    /// the first success word and ignores the second; the remaining keys are
    /// at the consecutive destination slots.
    ///
    /// The initial kind allowlist is `KeyTable`; other kinds are rejected by
    /// the kernel as unsupported rather than created. Device Untypeds cannot
    /// be retyped at all until a device-capable kind is approved (D6).
    pub fn retype(
        &self,
        kind: ObjectType,
        size_bits: u8,
        count: u32,
        dst_table: &KeyTableKey,
        dst_slot: u32,
        rights: Rights,
    ) -> Result<RawKey, CapError> {
        // SAFETY: Unsafe call.
        let result = unsafe {
            protected_call6(
                self.key.to_wire(),
                UntypedOp::Retype as u64,
                u64::from(kind.as_u8()),
                u64::from(size_bits),
                u64::from(count),
                dst_table.to_wire(),
                u64::from(dst_slot),
                u64::from(rights.bits()),
            )
        };
        let (key, _) = decode_syscall_result(result)?;
        Ok(RawKey::from_wire(key))
    }
}

impl fmt::Debug for UntypedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UntypedKey")
            .field("key", &self.key)
            .finish()
    }
}
