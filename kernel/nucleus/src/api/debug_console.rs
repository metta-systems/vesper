use libobject::{debug_console::DebugConsoleOp, Key};

// =====================
// == Syscall handler ==
// =====================

#[inline]
pub fn invoke(cap: &KeyEntry, op: u32, arg0: u64, arg1: u64) -> SyscallResult {
    let console = cap.as_debug_console()?;
    let op = DebugConsoleOp::try_from(op).map_err(|_| CapError::InvalidOperation)?;

    match op {
        DebugConsoleOp::Write => console.handle_write(arg0, arg1),
        _ => Err(SyscallError::InvalidOp),
    }
}
