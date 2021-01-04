#![no_std]
#![feature(asm)]

pub mod arch;

pub use arch::syscall;

// @todo make this use numeric constants for ABI compat
// but to keep this interface simpler, enum-to-numeric remapping will be implemented inside of the
// syscall() fn.
pub enum SysCall {
    Send,
    NBSend,
    Call,
    Recv,
    Reply,
    ReplyRecv,
    NBRecv,
    Yield,
    #[cfg(debug)]
    DebugPutChar,
    #[cfg(debug)]
    DebugHalt,
    #[cfg(debug)]
    DebugSnapshot,
    #[cfg(debug)]
    DebugCapIdentify,
    #[cfg(debug)]
    DebugNameThread,
}

#[cfg(test)]
mod tests {
    #[test_case]
    fn test_debug_output_syscall() {}
}
