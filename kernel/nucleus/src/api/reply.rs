// CLIENT DOMAIN                KERNEL                   SERVER DOMAIN
// ─────────────                ──────                   ─────────────
//
// CSpace:                      Reply Pool:              CSpace:
// ┌──────────────┐             ┌──────────────┐         ┌──────────────┐
// │ ...          │             │ Reply #0     │         │ ...          │
// │ ep_cap ──────┼──┐          │ Reply #1 ◄───┼─────────┼─ reply_slot  │
// │ ...          │  │          │ Reply #2     │         │ ...          │
// └──────────────┘  │          │ ...          │         └──────────────┘
//                   │          └──────────────┘
//                   │                 ▲
//                   │                 │
//                   ▼                 │
//              ┌────────────┐         │
//              │  Endpoint  │         │
//              │            │    kernel creates
//              │  state:    │    Reply object
//              │  Idle      │    on Call arrival
//              │            │
//              │  waiters:  │
//              │  (empty)   │
//              └────────────┘

// =====================
// == Syscall handler ==
// =====================

pub fn invoke(cap: &CapEntry, op: u32, arg0: u64, arg1: u64) -> SyscallResult {
    match op {
        ReplyOp::Send => handle_send(),
        _ => Err(SyscallError::InvalidOp),
    }
}
