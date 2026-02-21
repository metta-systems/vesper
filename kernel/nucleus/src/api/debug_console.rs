use {
    crate::{api::KeyEntry, objects::DebugConsole},
    libaddress::PhysAddr,
    libobject::{CapError, Key, SyscallResult, debug_console::DebugConsoleOp},
};

// =====================
// == Syscall handler ==
// =====================

#[inline]
pub fn invoke(cap: &mut KeyEntry, op: u32, arg0: u64, arg1: u64) -> SyscallResult {
    let console = cap.as_object_mut::<DebugConsole>()?;
    let op = DebugConsoleOp::try_from(op).map_err(|_| CapError::InvalidOperation)?;

    #[cfg(qemu)]
    libqemu::semi_println!("DebugConsole:invoke");

    match op {
        // DebugConsoleOp::Write => console.handle_write(arg0, arg1),
        DebugConsoleOp::Write => {
            crate::objects::debug_console::DebugConsole::handle_write(PhysAddr::new(arg0), arg1)?;
            Ok((0, 0))
        }
        _ => Err(CapError::InvalidOperation),
    }
}
