// Client Domain                              Server Domain
// ─────────────                              ─────────────
//      │                                          │
//      │ ep.call(msg)                             │ ep.recv()
//      │                                          │
//      ▼                                          ▼
// ┌─────────┐                                ┌─────────┐
// │ BLOCKED │                                │ BLOCKED │
// │ waiting │                                │ waiting │
// │ for     │                                │ for     │
// │ reply   │                                │ sender  │
// └────┬────┘                                └────┬────┘
//      │                                          │
//      │        ┌───────────────────┐             │
//      │        │  KERNEL SWITCH    │             │
//      ├───────►│                   │◄────────────┤
//      │        │ 1. Copy msg regs  │             │
//      │        │ 2. Set badge      │             │
//      │        │ 3. Switch domain  │             │
//      │        └───────────────────┘             │
//      │                                          │
//      │                                          ▼
//      │                                     (running)
//      │                                     process msg
//      │                                          │
//      │        ┌───────────────────┐             │
//      │        │  KERNEL SWITCH    │             │
//      │◄───────│                   │◄────────────┤
//      │        │ 1. Copy reply     │             │ ep.reply(response)
//      │        │ 2. Switch back    │             │
//      │        └───────────────────┘             │
//      ▼
// (running)
// got reply

// =====================
// == Syscall handler ==
// =====================

#[inline]
pub fn invoke(cap: &Cap, op: u32, arg0: u64, arg1: u64) -> SyscallResult {
    let ep = cap.as_endpoint()?;
    match op {
        EndpointOp::Call => ep.handle_call(),
        EndpointOp::Send => ep.handle_send(),
        EndpointOp::Recv => ep.handle_recv(),
        EndpointOp::Reply => { /* invoke the reply_cap */
            // TODO: do this on ReplyCap side, see ReplyOp::Send
        }
        EndpointOp::ReplyRecv => ep.handle_replyrecv(),
        EndpointOp::Forward => ep.handle_forward(),
        _ => Err(SyscallError::InvalidOp),
    }
}
