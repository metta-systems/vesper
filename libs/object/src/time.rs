use {
    crate::{CapError, Key, RawKey, decode_syscall_result},
    core::fmt,
};

#[cfg(not(test))]
use libsyscall::{protected_call0, protected_call1, protected_call3};
#[cfg(test)]
use tests::{protected_call0, protected_call1, protected_call3};

#[cfg(test)]
#[path = "../tests/support/time.rs"]
mod tests;

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Userspace handle to a `Time` capability: a CPU budget on one core plus a
/// priority chain, one (scheduler, priority) entry for each scheduler the
/// budget passed through.
///
/// Budget moves only by [`Self::transfer`] and [`Self::merge`]; nothing
/// returns it automatically. A Time created by `Untyped.Retype` is empty.
pub struct TimeKey {
    key: Key<TimeType>,
}

enum TimeType {}

/// Budget in nanoseconds. [`INFINITE_BUDGET`] never decreases.
pub type Budget = u64;

/// Priority at one scheduler: a lower number is more urgent, `0` is the most
/// urgent, and ordinary components start around [`DEFAULT_PRIORITY`].
pub type Priority = u16;

/// The infinite budget: it never decreases, and transferring this amount
/// makes the destination infinite.
pub const INFINITE_BUDGET: Budget = u64::MAX;

/// The most urgent priority.
pub const HIGHEST_PRIORITY: Priority = 0;

/// The least urgent priority; [`TimeKey::query`] reports it for a Time with
/// an empty chain.
pub const LOWEST_PRIORITY: Priority = Priority::MAX;

/// Where ordinary user components start.
pub const DEFAULT_PRIORITY: Priority = 0x8000;

/// Most entries a priority chain holds.
pub const MAX_PRIORITY_CHAIN: usize = 16;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TimeOp {
    /// Move budget from the invoked Time into a destination Time, recording
    /// the sender's priority in the destination's chain. Requires `WRITE` on
    /// both.
    Transfer = 0,
    /// Move all budget from a source Time into the invoked Time, which keeps
    /// its own chain; the source is left empty. Requires `WRITE` on both.
    Merge = 1,
    /// Report the remaining budget and the priority at the most recent
    /// scheduler. Requires `READ`.
    Query = 2,
}

impl TryFrom<u64> for TimeOp {
    type Error = CapError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Transfer),
            1 => Ok(Self::Merge),
            2 => Ok(Self::Query),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

/// What [`TimeKey::query`] reports.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TimeQuery {
    /// Remaining budget in nanoseconds; [`INFINITE_BUDGET`] when infinite.
    pub budget_ns: Budget,
    /// Priority at the last chain entry, [`LOWEST_PRIORITY`] when the chain
    /// is empty.
    pub priority: Priority,
}

impl TimeKey {
    /// Construct a non-owning handle without installing or validating
    /// authority.
    pub const fn from_key(key: RawKey) -> Self {
        Self { key: Key::new(key) }
    }

    /// The wire encoding of the underlying key.
    pub const fn to_wire(&self) -> u64 {
        self.key.to_wire()
    }

    /// Transfer: move `amount_ns` of budget from this Time into
    /// `destination`, which records `priority` for this Time's scheduler.
    ///
    /// Wire schema: `x2` destination key, `x3` amount, `x4` priority,
    /// `x5..x7` zero. Authority: `WRITE` on both. A finite source holding
    /// less than `amount_ns` fails with `InsufficientBudget`.
    pub fn transfer(
        &self,
        destination: &TimeKey,
        amount_ns: Budget,
        priority: Priority,
    ) -> Result<(), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe {
            protected_call3(
                self.key.to_wire(),
                TimeOp::Transfer as u64,
                destination.to_wire(),
                amount_ns,
                u64::from(priority),
            )
        };
        decode_syscall_result(result).map(|_| ())
    }

    /// Merge: move all of `source`'s budget into this Time, which keeps its
    /// own priority chain (or takes `source`'s when its own is empty), and
    /// leave `source` empty.
    ///
    /// Wire schema: `x2` source key, `x3..x7` zero. Authority: `WRITE` on
    /// both.
    pub fn merge(&self, source: &TimeKey) -> Result<(), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result =
            unsafe { protected_call1(self.key.to_wire(), TimeOp::Merge as u64, source.to_wire()) };
        decode_syscall_result(result).map(|_| ())
    }

    /// Query: the remaining budget and the priority at the most recent
    /// scheduler.
    ///
    /// Wire schema: no arguments. Authority: `READ`. A success whose
    /// priority word is wider than 16 bits fails with `Unknown`.
    pub fn query(&self) -> Result<TimeQuery, CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe { protected_call0(self.key.to_wire(), TimeOp::Query as u64) };
        let (budget_ns, priority) = decode_syscall_result(result)?;
        let priority = Priority::try_from(priority).map_err(|_too_wide| CapError::Unknown)?;
        Ok(TimeQuery {
            budget_ns,
            priority,
        })
    }
}

impl fmt::Debug for TimeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TimeKey").field("key", &self.key).finish()
    }
}
