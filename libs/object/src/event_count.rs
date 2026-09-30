use {
    crate::{CapError, Key, RawKey, decode_syscall_result},
    core::fmt,
};

#[cfg(not(test))]
use libsyscall::{protected_call0, protected_call1, protected_call2};
#[cfg(test)]
use tests::{protected_call0, protected_call1, protected_call2};

#[cfg(test)]
#[path = "../tests/support/event_count.rs"]
mod tests;

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Event count capability - monotonic Reed-Kanodia counter for exact event
/// tracking. Best for: streaming, flow control, producer-consumer
/// coordination.
///
/// Unlike notifications, every `advance()` is counted - no coalescing. This
/// lets consumers know exactly how far behind they are. Readers maintain
/// independent positions; awaiting or reading does not consume the counter,
/// and an advance completes every queued await whose target it satisfies
/// (broadcast wakeups, unlike Notification's one-consumer delivery).
pub struct EventCountKey {
    key: Key<EventCountType>,
}

enum EventCountType {}

/// Existing operation vocabulary; decoding an ID does not imply kernel
/// support.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EventCountOp {
    /// Add a nonzero delta to the counter and complete every queued `Await`
    /// whose target the new value satisfies. Requires `SEND`. An advance
    /// that would exceed `u64::MAX` completes with the shared
    /// `CounterOverflow` error, leaves the counter unchanged, and completes
    /// every queued `Await` with the same error (selected 2026-09-18).
    Advance = 0,
    /// Block until the counter is `>= target`, then return the observed
    /// value. Requires `RECV`. Takes one relative timeout argument
    /// (nanoseconds; `WAIT_INFINITE` blocks forever; finite values are
    /// unsupported until the time subsystem exists).
    Await = 1,
    /// Observe the counter without blocking. Requires `RECV`.
    Read = 2,
}

impl TryFrom<u64> for EventCountOp {
    type Error = CapError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Advance),
            1 => Ok(Self::Await),
            2 => Ok(Self::Read),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

impl TryFrom<u32> for EventCountOp {
    type Error = CapError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::try_from(u64::from(value))
    }
}

impl EventCountKey {
    /// Infinite-timeout encoding for [`Self::await_ge`]: block until
    /// completion or teardown cancellation (selected 2026-09-16).
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

    /// Advance: add `delta` (nonzero) to the counter and return the new
    /// value. Never blocks; every advance is counted (no coalescing).
    ///
    /// Wire schema: `x2` delta, `x3..x7` zero. Authority: `SEND`. Zero is
    /// invalid; an advance that would exceed `u64::MAX` returns
    /// `CounterOverflow` and leaves the counter unchanged, completing every
    /// queued `Await` with the same error (selected 2026-09-18).
    pub fn advance(&self, delta: u64) -> Result<u64, CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result =
            unsafe { protected_call1(self.key.to_wire(), EventCountOp::Advance as u64, delta) };
        decode_syscall_result(result).map(|(value, _)| value)
    }

    /// Await: block until the counter is `>= target`, then return the
    /// observed value (which may exceed the target if the producer is
    /// fast).
    ///
    /// Wire schema: `x2` target, `x3` timeout (nanoseconds;
    /// `WAIT_INFINITE` = forever), `x4..x7` zero. Authority: `RECV`. An
    /// already-satisfied await returns immediately; a would-block await
    /// parks the caller and resumes when an advance completes it.
    pub fn await_ge(&self, target: u64, timeout_ns: u64) -> Result<u64, CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe {
            protected_call2(
                self.key.to_wire(),
                EventCountOp::Await as u64,
                target,
                timeout_ns,
            )
        };
        decode_syscall_result(result).map(|(value, _)| value)
    }

    /// Read: observe the counter without blocking. Does not consume.
    ///
    /// Wire schema: no arguments. Authority: `RECV`.
    pub fn read(&self) -> Result<u64, CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe { protected_call0(self.key.to_wire(), EventCountOp::Read as u64) };
        decode_syscall_result(result).map(|(value, _)| value)
    }
}

/// Helper: tracks consumer position for a single reader
pub struct EventCountReader {
    ec: EventCountKey,
    last_seen: u64,
}

impl EventCountReader {
    pub fn new(ec: EventCountKey) -> Result<Self, CapError> {
        let initial = ec.read()?;
        Ok(Self {
            ec,
            last_seen: initial,
        })
    }

    /// Wait for next event(s), returns count since last wait
    pub fn wait_next(&mut self) -> Result<u64, CapError> {
        // A reader at the counter's ceiling has no next event to await;
        // the position cannot advance past `u64::MAX`.
        let target = self
            .last_seen
            .checked_add(1)
            .ok_or(CapError::CounterOverflow)?;
        let current = self.ec.await_ge(target, EventCountKey::WAIT_INFINITE)?;
        let delta = current - self.last_seen;
        self.last_seen = current;
        Ok(delta)
    }

    /// Check how many events pending without blocking
    pub fn pending(&self) -> Result<u64, CapError> {
        Ok(self.ec.read()? - self.last_seen)
    }
}

impl fmt::Debug for EventCountKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventCountKey")
            .field("key", &self.key)
            .finish()
    }
}
