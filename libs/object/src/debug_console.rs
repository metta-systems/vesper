use crate::{CapError, Key, KeySlot};

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

pub struct DebugConsoleKey {
    key: Key<DebugConsoleType>,
}

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
impl DebugConsoleKey {
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
        let (_ok, _, _) = unsafe {
            libsyscall::protected_call2(
                self.key.slot(),
                DebugConsoleOp::Write as u32,
                s.as_ptr() as u64,
                s.len() as u64,
            )
        };
        Ok(())
    }
}
