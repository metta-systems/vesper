use crate::CapError;
#[cfg(feature = "debug_kernel")]
use crate::{Key, KeySlot};

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Debug-only prototype console wrapper, available with opt-in `debug_kernel`.
///
/// Not a general service or a safety/isolation guarantee: retains unchecked
/// pointer-based writes and discarded kernel errors for trusted debugging only.
#[cfg(feature = "debug_kernel")]
pub struct DebugConsoleKey {
    key: Key<DebugConsoleType>,
}

#[cfg(feature = "debug_kernel")]
enum DebugConsoleType {}

#[repr(u8)]
pub enum DebugConsoleOp {
    /// Capability invocation to write a message to a debug console.
    Write = 0,
}

impl TryFrom<u32> for DebugConsoleOp {
    type Error = CapError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(DebugConsoleOp::Write),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

// Root domain gets a DebugConsoleCap, can delegate to others
#[cfg(feature = "debug_kernel")]
impl DebugConsoleKey {
    #[expect(clippy::new_without_default)]
    pub const fn new() -> Self {
        Self {
            key: Key::new(KeySlot::DEBUG_CONSOLE),
        }
    }

    pub const fn new_slot(slot: KeySlot) -> Self {
        Self {
            key: Key::new(slot),
        }
    }

    pub fn write(&self, s: &str) -> Result<(), CapError> {
        // SAFETY: Unsafe call.
        let (_ok, _r1, _r2) = unsafe {
            libsyscall::protected_call2(
                self.key.slot(),
                DebugConsoleOp::Write as u32,
                s.as_ptr() as u64,
                s.len() as u64,
            )
        };
        libqemu::semihosting::println!(
            "Userspace return from DebugConsoleOp::Write with result ({}, {}, {})",
            _ok,
            _r1,
            _r2
        );
        Ok(())
    }
}
