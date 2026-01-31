use libsyscall::protected_call2;

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

pub struct DebugConsoleKey {
    key: Key<DebugConsole>,
}

#[repr(u8)]
pub enum DebugConsoleOp {
    /// Capability invocation to write a message to a debug console.
    Write = 0,
}

// Root domain gets a DebugConsoleCap, can delegate to others
impl DebugConsoleKey {
    pub fn write(&self, s: &str) -> Result<(), SyscallError> {
        protected_call2(
            self.key.slot(),
            DebugConsoleOp::Write as u32,
            s.as_ptr() as u64,
            s.len() as u64,
        )?;
        Ok(())
    }
}
