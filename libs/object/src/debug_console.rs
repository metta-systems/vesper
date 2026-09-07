use crate::CapError;
#[cfg(feature = "debug_kernel")]
use crate::{Key, RawKey, decode_syscall_result};

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Debug-only prototype console wrapper, available with opt-in `debug_kernel`.
///
/// Not a general service or a safety/isolation guarantee: retains unchecked
/// pointer-based writes and discarded kernel errors for trusted debugging only.
///
/// Implementation status: the discarded-error description above is historical;
/// `write` now decodes and propagates kernel errors through `decode_syscall_result`.
/// The pointer-based debug mechanism and semihosting diagnostic remain unchanged.
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

impl TryFrom<u64> for DebugConsoleOp {
    type Error = CapError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(DebugConsoleOp::Write),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

impl TryFrom<u32> for DebugConsoleOp {
    type Error = CapError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::try_from(u64::from(value))
    }
}

// Root domain gets a DebugConsoleCap, can delegate to others
#[cfg(feature = "debug_kernel")]
impl DebugConsoleKey {
    /// Construct a non-owning handle from an issued key, not a slot convention.
    /// This does not install authority or validate the kernel capability.
    pub const fn from_key(key: RawKey) -> Self {
        Self { key: Key::new(key) }
    }

    pub const fn raw(&self) -> RawKey {
        self.key.raw()
    }

    pub fn write(&self, s: &str) -> Result<(), CapError> {
        // SAFETY: Unsafe call.
        let (status, result0, result1) = unsafe {
            libsyscall::protected_call2(
                self.key.to_wire(),
                DebugConsoleOp::Write as u64,
                s.as_ptr() as u64,
                s.len() as u64,
            )
        };
        libqemu::semihosting::println!(
            "Userspace return from DebugConsoleOp::Write with result ({}, {}, {})",
            status,
            result0,
            result1
        );
        decode_syscall_result((status, result0, result1)).map(|_| ())
    }
}
