use {
    crate::api::key::Key,
    core::slice,
    libsyscall::{SyscallError, SyscallResult, protected_call2},
};

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
            self.key.slot,
            DebugConsoleOp::Write as u32,
            s.as_ptr() as u64,
            s.len() as u64,
        )?;
        Ok(())
    }
}

// ==============================================
// == Kernel space object and syscall handling ==
// ==============================================

struct DebugConsole;

impl DebugConsole {
    fn handle_write(ptr: u64, len: u64) {
        let slice = slice::from_raw_parts(ptr, len as usize);
        let buf = [0u8; 4096];
        buf.copy_from_slice(slice);
        buf[slice.len()] = 0;
        let cstr = unsafe { core::ffi::CStr::from_bytes_with_nul(&buf[..=slice.len() + 1]) };
        libqemu::semihosting::sys_write0_call(cstr);
    }
}

#[repr(u8)]
pub enum DebugConsoleOp {
    Write = 0,
}

// =====================
// == Syscall handler ==
// =====================

pub fn invoke(cap: &KeyEntry, op: u32, arg0: u64, arg1: u64) -> SyscallResult {
    let console = cap.as_debug_console()?;
    match op {
        DebugConsoleOp::Write => console.handle_write(arg0, arg1),
        _ => Err(SyscallError::InvalidOp),
    }
    Ok((0, 0))
}
