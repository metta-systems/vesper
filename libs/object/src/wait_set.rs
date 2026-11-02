use {
    crate::{CapError, Key, RawKey, decode_syscall_result},
    core::fmt,
};

#[cfg(not(test))]
use libsyscall::{protected_call1, protected_call2, protected_call3};
#[cfg(test)]
use tests::{protected_call1, protected_call2, protected_call3};

#[cfg(test)]
#[path = "../tests/support/wait_set.rs"]
mod tests;

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Userspace handle to a `WaitSet` capability: a Thread's multiplexer over
/// up to [`WaitSetKey::MAX_MEMBERS`] Notifications and `EventCount`s.
///
/// Each member sits in a numbered slot with one condition: a Notification
/// member is ready while any bit of its mask is pending, an `EventCount` member
/// while its value has reached its target. `Await` returns the bitmask of
/// ready slots (bit *n* for slot *n*) and consumes nothing: the caller then
/// reads or awaits the ready members themselves. Readiness is level-triggered.
pub struct WaitSetKey {
    key: Key<WaitSetType>,
}

enum WaitSetType {}

/// Operation vocabulary.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WaitSetOp {
    /// Put a Notification or `EventCount` in a vacant slot. Requires `WRITE` on
    /// the set and `RECV` on the member.
    AddMember = 0,
    /// Change the condition of the member in a slot. Requires `WRITE`.
    UpdateMember = 1,
    /// Empty a slot. Requires `WRITE`.
    RemoveMember = 2,
    /// Block until at least one member is ready and return the mask of ready
    /// slots. Requires `RECV`. Takes one relative timeout argument
    /// (nanoseconds; `WAIT_INFINITE` blocks forever).
    Await = 3,
}

impl TryFrom<u64> for WaitSetOp {
    type Error = CapError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::AddMember),
            1 => Ok(Self::UpdateMember),
            2 => Ok(Self::RemoveMember),
            3 => Ok(Self::Await),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

impl TryFrom<u32> for WaitSetOp {
    type Error = CapError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::try_from(u64::from(value))
    }
}

impl WaitSetKey {
    /// Infinite-timeout encoding for [`Self::await_ready`].
    pub const WAIT_INFINITE: u64 = u64::MAX;

    /// Members a set holds; slot numbers run from 0 to this minus one.
    pub const MAX_MEMBERS: usize = 8;

    /// Construct a non-owning handle without installing or validating
    /// authority.
    pub const fn from_key(key: RawKey) -> Self {
        Self { key: Key::new(key) }
    }

    /// The wire encoding of the underlying key.
    pub const fn to_wire(&self) -> u64 {
        self.key.to_wire()
    }

    /// Put `member`, a Notification or `EventCount` key, in the vacant `slot`.
    /// `condition` is the Notification bits to watch (nonzero) or the
    /// `EventCount` target to reach.
    ///
    /// Wire schema: `x2` the member's key, `x3` slot, `x4` condition, `x5..x7`
    /// zero. Authority: `WRITE` on the set, `RECV` on the member. A Notification a
    /// Thread is already waiting on fails with `Watched`; once listed, a
    /// Notification refuses direct `Await`s.
    pub fn add_member(&self, member: RawKey, slot: u64, condition: u64) -> Result<(), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe {
            protected_call3(
                self.key.to_wire(),
                WaitSetOp::AddMember as u64,
                member.to_wire(),
                slot,
                condition,
            )
        };
        decode_syscall_result(result).map(|_| ())
    }

    /// Change the condition of the member in `slot`, while it stays in the set.
    ///
    /// Wire schema: `x2` slot, `x3` condition, `x4..x7` zero. Authority:
    /// `WRITE`.
    pub fn update_member(&self, slot: u64, condition: u64) -> Result<(), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe {
            protected_call2(
                self.key.to_wire(),
                WaitSetOp::UpdateMember as u64,
                slot,
                condition,
            )
        };
        decode_syscall_result(result).map(|_| ())
    }

    /// Empty `slot`.
    ///
    /// Wire schema: `x2` slot, `x3..x7` zero. Authority: `WRITE`.
    pub fn remove_member(&self, slot: u64) -> Result<(), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result =
            unsafe { protected_call1(self.key.to_wire(), WaitSetOp::RemoveMember as u64, slot) };
        decode_syscall_result(result).map(|_| ())
    }

    /// Await: return the mask of ready slots, blocking until one is ready or
    /// the timeout ends. Waiting consumes nothing.
    ///
    /// Wire schema: `x2` timeout (nanoseconds; `WAIT_INFINITE` = forever, zero
    /// invalid), `x3..x7` zero. Authority: `RECV`. A set has at most one waiter.
    pub fn await_ready(&self, timeout_ns: u64) -> Result<u64, CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result =
            unsafe { protected_call1(self.key.to_wire(), WaitSetOp::Await as u64, timeout_ns) };
        decode_syscall_result(result).map(|(mask, _)| mask)
    }
}

impl fmt::Debug for WaitSetKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WaitSetKey")
            .field("key", &self.key)
            .finish()
    }
}
