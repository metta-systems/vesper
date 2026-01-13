// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

pub struct DebugConsoleKey {
    key: Key<DebugConsole>,
}

// Root domain gets a DebugConsoleCap, can delegate to others
impl DebugConsoleKey {
    pub fn write(&self, s: &str) -> Result<()> {
        protected_call2(
            self.slot,
            DebugConsoleOp::Write as u32,
            s.as_ptr() as u64,
            s.len() as u64,
        )
    }
}

// ==============================================
// == Kernel space object and syscall handling ==
// ==============================================

struct DebugConsole;

impl DebugConsole {
    fn handle_write() {}
}

#[repr(u8)]
pub enum DebugConsoleOp {
    Write = 0,
}

// =====================
// == Syscall handler ==
// =====================

pub fn invoke(cap: &CapEntry, op: u32, arg0: u64, arg1: u64) -> SyscallResult {
    match op {
        _ => Err(SyscallError::InvalidOp),
    }
}
