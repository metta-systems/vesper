// =====================
// == Syscall handler ==
// =====================

pub fn invoke(cap: &Cap, op: u32, arg0: u64) -> SyscallResult {
    let ec = cap.as_event_count()?;
    match op {
        EventOp::Advance => {
            Ok(ec.advance(arg0)) // atomic ADD, returns new value
        }
        EventOp::Await => {
            Ok(ec.await_ge(arg0)) // blocks until >= arg0
        }
        EventOp::Read => {
            Ok(ec.read()) // non-blocking read
        }
        _ => Err(SyscallError::InvalidOp),
    }
}
