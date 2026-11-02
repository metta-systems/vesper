use {
    crate::{CapError, Key, RawKey, decode_syscall_result},
    core::fmt,
};

#[cfg(not(test))]
use libsyscall::{protected_call0, protected_call1, protected_call2};
#[cfg(test)]
use tests::{protected_call0, protected_call1, protected_call2};

#[cfg(test)]
#[path = "../tests/support/time.rs"]
mod tests;

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Userspace handle to a `Time` capability: a CPU budget on one core.
///
/// Budget moves only by [`Self::transfer`] and [`Self::merge`]; nothing
/// returns it automatically. A Time created by `Untyped.Retype` is empty.
pub struct TimeKey {
    key: Key<TimeType>,
}

enum TimeType {}

/// Budget in nanoseconds. [`INFINITE_BUDGET`] never decreases.
pub type Budget = u64;

/// The infinite budget: it never decreases, and transferring this amount
/// makes the destination infinite.
pub const INFINITE_BUDGET: Budget = u64::MAX;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TimeOp {
    /// Move budget from the invoked Time into a destination Time. Requires
    /// `WRITE` on both.
    Transfer = 0,
    /// Move all budget from a source Time into the invoked Time; the source
    /// is left empty. Requires `WRITE` on both.
    Merge = 1,
    /// Report the remaining budget. Requires `READ`.
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
    /// `destination`.
    ///
    /// Wire schema: `x2` destination key, `x3` amount, `x4..x7` zero.
    /// Authority: `WRITE` on both. A finite source holding less than
    /// `amount_ns` fails with `InsufficientBudget`.
    pub fn transfer(&self, destination: &TimeKey, amount_ns: Budget) -> Result<(), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe {
            protected_call2(
                self.key.to_wire(),
                TimeOp::Transfer as u64,
                destination.to_wire(),
                amount_ns,
            )
        };
        decode_syscall_result(result).map(|_| ())
    }

    /// Merge: move all of `source`'s budget into this Time and leave `source`
    /// empty.
    ///
    /// Wire schema: `x2` source key, `x3..x7` zero. Authority: `WRITE` on
    /// both.
    pub fn merge(&self, source: &TimeKey) -> Result<(), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result =
            unsafe { protected_call1(self.key.to_wire(), TimeOp::Merge as u64, source.to_wire()) };
        decode_syscall_result(result).map(|_| ())
    }

    /// Query: the remaining budget.
    ///
    /// Wire schema: no arguments. Authority: `READ`. The result is `x1` the
    /// budget and `x2` zero; a nonzero `x2` fails with `Unknown`.
    pub fn query(&self) -> Result<TimeQuery, CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe { protected_call0(self.key.to_wire(), TimeOp::Query as u64) };
        let (budget_ns, reserved) = decode_syscall_result(result)?;
        if reserved != 0 {
            return Err(CapError::Unknown);
        }
        Ok(TimeQuery { budget_ns })
    }
}

impl fmt::Debug for TimeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TimeKey").field("key", &self.key).finish()
    }
}
