use {
    crate::{CapError, Key, RawKey, decode_syscall_result},
    core::fmt,
};

#[cfg(not(test))]
use libsyscall::{protected_call0, protected_call1};
#[cfg(test)]
use tests::{protected_call0, protected_call1};

#[cfg(test)]
#[path = "../tests/support/notification.rs"]
mod tests;

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Userspace handle to a `Notification` capability: bitmap-based async
/// signaling. Best for IRQs, completion events, wakeups.
///
/// Nucleus dispatch supports `Signal`, `Wait`, and `Poll` (2026-09-16;
/// `Wait`'s blocking path through the park/resume entry handling landed
/// 2026-09-18). Signal bits come from the capability badge when
/// nonzero, else the caller-supplied argument (selected 2026-09-16).
pub struct NotificationKey {
    key: Key<NotificationType>,
}

enum NotificationType {}

/// Existing operation vocabulary; decoding an ID does not imply kernel
/// support.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum NotificationOp {
    /// OR the authorized bits into the bitmap and wake at most one waiter
    /// (one-consumer delivery). Requires `SEND`. Bits: the capability
    /// badge when nonzero, else the argument word.
    Signal = 0,
    /// Block until bits are pending and consume them. Requires `RECV`.
    /// Takes one relative timeout argument (nanoseconds; `WAIT_INFINITE`
    /// blocks forever; finite values are unsupported until the time
    /// subsystem exists).
    Wait = 1,
    /// Consume pending bits immediately or return zero (none pending).
    /// Requires `RECV`. Never blocks.
    Poll = 2,
}

impl TryFrom<u64> for NotificationOp {
    type Error = CapError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Signal),
            1 => Ok(Self::Wait),
            2 => Ok(Self::Poll),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

impl TryFrom<u32> for NotificationOp {
    type Error = CapError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::try_from(u64::from(value))
    }
}

impl NotificationKey {
    /// Infinite-timeout encoding for [`Self::wait`]: block until completion
    /// or teardown cancellation (selected 2026-09-16).
    pub const WAIT_INFINITE: u64 = u64::MAX;

    /// Construct a non-owning handle without installing or validating
    /// authority.
    pub const fn from_key(key: RawKey) -> Self {
        Self { key: Key::new(key) }
    }

    /// The wire encoding of the underlying key.
    pub const fn to_wire(&self) -> u64 {
        self.key.to_wire()
    }

    /// Signal: OR `bits` into the bitmap (always non-blocking). Multiple
    /// signals to the same bit coalesce; a blocked waiter, if any, is
    /// woken with the delivered bitmap.
    ///
    /// Wire schema: `x2` bits (used when the capability badge is zero),
    /// `x3..x7` zero. Authority: `SEND`.
    pub fn signal(&self, bits: u64) -> Result<(), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result =
            unsafe { protected_call1(self.key.to_wire(), NotificationOp::Signal as u64, bits) };
        decode_syscall_result(result).map(|_| ())
    }

    /// Wait: block until bits are pending, then consume and return them.
    ///
    /// Wire schema: `x2` timeout (nanoseconds; `WAIT_INFINITE` = forever),
    /// `x3..x7` zero. Authority: `RECV`. An already-satisfied wait consumes
    /// and returns the bits immediately; a wait with no pending bits parks
    /// the caller and returns when a signal completes it (finite timeouts
    /// are unsupported until the time subsystem exists).
    pub fn wait(&self, timeout_ns: u64) -> Result<u64, CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result =
            unsafe { protected_call1(self.key.to_wire(), NotificationOp::Wait as u64, timeout_ns) };
        decode_syscall_result(result).map(|(bits, _)| bits)
    }

    /// Poll: consume and return pending bits, or zero when none are
    /// pending. Never blocks.
    ///
    /// Wire schema: no arguments. Authority: `RECV`.
    pub fn poll(&self) -> Result<u64, CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe { protected_call0(self.key.to_wire(), NotificationOp::Poll as u64) };
        decode_syscall_result(result).map(|(bits, _)| bits)
    }
}

impl fmt::Debug for NotificationKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NotificationKey")
            .field("key", &self.key)
            .finish()
    }
}
