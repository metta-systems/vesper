pub mod arch;

pub use arch::syscall;

pub enum SysCall {
    Send,
    NBSend,
    Call,
    Recv,
    Reply,
    ReplyRecv,
    NBRecv,
    Yield,
}

#[cfg(test)]
mod tests {
    #[test_case]
    fn test_debug_output_syscall() {}
}
