use {
    crate::{CapError, Key, RawKey, decode_syscall_result},
    core::fmt,
};

#[cfg(not(test))]
use libsyscall::{protected_call1, protected_call6};
#[cfg(test)]
use tests::{protected_call1, protected_call6};

#[cfg(test)]
#[path = "../tests/support/scheduler.rs"]
mod tests;

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Userspace handle to a `Scheduler` capability.
///
/// The root Scheduler (at `KeySlot::ROOT_SCHEDULER`) is the only authority
/// that turns a Thread created by `Untyped.Retype`, which is inert, into one
/// that runs: [`Self::configure_thread`] binds it to an `AddressSpace`, an
/// entry point, a stack and its own Time, and [`Self::start_thread`] makes it
/// runnable.
pub struct SchedulerKey {
    key: Key<SchedulerType>,
}

enum SchedulerType {}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SchedulerOp {
    /// Share a scheduler-mapped Frame holding scheduling records. Its schema
    /// is not defined yet; the kernel rejects it.
    ShareRegion = 0,
    /// Bind an inert Thread to an `AddressSpace`, entry, stack and Time.
    ConfigureThread = 1,
    /// Make a configured Thread runnable.
    StartThread = 2,
}

impl TryFrom<u64> for SchedulerOp {
    type Error = CapError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::ShareRegion),
            1 => Ok(Self::ConfigureThread),
            2 => Ok(Self::StartThread),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

/// Where a configured Thread starts.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ThreadStart {
    /// Entry point; 4-byte aligned, below the end of the user address range.
    pub entry: u64,
    /// Initial stack pointer; 16-byte aligned, below the end of the user
    /// address range.
    pub stack_pointer: u64,
    /// Delivered in `x0` on first entry.
    pub argument: u64,
}

impl SchedulerKey {
    /// Construct a non-owning handle without installing or validating
    /// authority.
    pub const fn from_key(key: RawKey) -> Self {
        Self { key: Key::new(key) }
    }

    /// The wire encoding of the underlying key.
    pub const fn to_wire(&self) -> u64 {
        self.key.to_wire()
    }

    /// `ConfigureThread`: bind the inert Thread `thread` to `address_space`,
    /// the start state `start` (EL0) and its own Time `time`.
    ///
    /// Wire schema: `x2` Thread key, `x3` `AddressSpace` key, `x4` entry,
    /// `x5` stack pointer, `x6` Time key, `x7` argument. Authority: `WRITE`
    /// on the Scheduler, the Thread and the Time; `MAP` on the
    /// `AddressSpace`.
    pub fn configure_thread(
        &self,
        thread: RawKey,
        address_space: RawKey,
        start: ThreadStart,
        time: RawKey,
    ) -> Result<(), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe {
            protected_call6(
                self.key.to_wire(),
                SchedulerOp::ConfigureThread as u64,
                thread.to_wire(),
                address_space.to_wire(),
                start.entry,
                start.stack_pointer,
                time.to_wire(),
                start.argument,
            )
        };
        decode_syscall_result(result).map(|_| ())
    }

    /// `StartThread`: make the configured Thread `thread` runnable.
    ///
    /// Wire schema: `x2` Thread key, `x3..x7` zero. Authority: `WRITE` on
    /// the Scheduler and the Thread.
    pub fn start_thread(&self, thread: RawKey) -> Result<(), CapError> {
        // SAFETY: the syscall transport is the encapsulated unsafe boundary.
        let result = unsafe {
            protected_call1(
                self.key.to_wire(),
                SchedulerOp::StartThread as u64,
                thread.to_wire(),
            )
        };
        decode_syscall_result(result).map(|_| ())
    }
}

impl fmt::Debug for SchedulerKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SchedulerKey")
            .field("key", &self.key)
            .finish()
    }
}
