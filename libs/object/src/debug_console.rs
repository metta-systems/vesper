use {
    crate::{CapError, Key, RawKey, decode_syscall_result},
    core::sync::atomic::{AtomicU64, Ordering},
};

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Debug-only prototype console wrapper.
///
/// Not a general service or a safety/isolation guarantee: only a kernel built
/// with `debug_kernel` installs a console key, so in any other kernel no
/// component holds one and `write` fails the key lookup. `write` decodes and
/// propagates kernel errors through `decode_syscall_result`.
pub struct DebugConsoleKey {
    key: Key<DebugConsoleType>,
}

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

/// Most bytes one `Write` prints; the nucleus prints the first this many of
/// a longer message.
pub const WRITE_LIMIT: usize = 1024;

// The root component gets a DebugConsoleCap, can delegate to others
impl DebugConsoleKey {
    /// Construct a non-owning handle from an issued key, not a slot convention.
    /// This does not install authority or validate the kernel capability.
    pub const fn from_key(key: RawKey) -> Self {
        Self { key: Key::new(key) }
    }

    pub const fn raw(&self) -> RawKey {
        self.key.raw()
    }

    /// Print `bytes` on the console, as they are: no newline is added. The
    /// nucleus reads them from the caller's memory; it prints at most
    /// [`WRITE_LIMIT`] of them.
    pub fn write(&self, bytes: &[u8]) -> Result<(), CapError> {
        // SAFETY: Write only reads `bytes.len()` bytes at `bytes`, which the
        // caller can read.
        let (status, result0, result1) = unsafe {
            libsyscall::protected_call2(
                self.key.to_wire(),
                DebugConsoleOp::Write as u64,
                bytes.as_ptr() as u64,
                bytes.len() as u64,
            )
        };
        decode_syscall_result((status, result0, result1)).map(|_| ())
    }
}

/// The console key a panic is printed with, as its wire form; zero for none.
static PANIC_CONSOLE: AtomicU64 = AtomicU64::new(0);

/// Print a panic on the console `key` names (the runtime's panic handler
/// reads it back with [`panic_console`]).
pub fn set_panic_console(key: RawKey) {
    PANIC_CONSOLE.store(key.to_wire(), Ordering::Relaxed);
}

/// The console a panic is printed with, if [`set_panic_console`] named one.
pub fn panic_console() -> Option<DebugConsoleKey> {
    match PANIC_CONSOLE.load(Ordering::Relaxed) {
        0 => None,
        wire => Some(DebugConsoleKey::from_key(RawKey::from_wire(wire))),
    }
}
