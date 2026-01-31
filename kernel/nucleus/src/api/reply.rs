//! Endpoint IPC Reply object

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Reply capability - one-shot reply to a blocked caller
///
/// Created by kernel when a Call arrives, consumed when reply is sent.
/// This is a LINEAR type - must be used exactly once (or explicitly dropped).
///
/// Key insight from seL4 MCS: making reply explicit enables:
/// - Async reply (store reply cap, reply later)
/// - Delegation (pass reply cap to helper domain)
/// - Multiple outstanding calls (each has own reply cap)
pub struct ReplyKey {
    key: Key<Reply>,
}

impl ReplyKey {
    /// Send reply to the blocked caller
    ///
    /// Consumes self - reply cap is one-shot!
    /// After this, the caller is unblocked with the response.
    pub fn send(self, msg: &Message) -> Result<(), IpcError> {
        let ret = unsafe {
            protected_call4(
                self.key.slot as u64,
                ReplyOp::Send as u64,
                msg.label,
                msg.data[0],
                msg.data[1],
                msg.data[2],
            )
        };

        // Consumed - don't run Drop (kernel already invalidated)
        core::mem::forget(self);

        IpcError::from_code(ret.0)
    }

    /// Send reply with capability transfer
    pub fn send_with_key(self, msg: &Message, key: KeySlot) -> Result<(), IpcError> {
        let ret = unsafe {
            syscall_ipc_reply(
                self.cap.slot as u64,
                ReplyOp::SendWithCap as u64,
                msg.label,
                msg.data[0],
                msg.data[1],
                msg.data[2],
                msg.data[3],
                msg.data[4],
                cap as u64,
            )
        };

        // Consumed - don't run Drop (kernel already invalidated)
        core::mem::forget(self);

        IpcError::from_code(ret.0)
    }
}

/// Dropping a ReplyKey without sending is an ERROR for the caller.
/// The caller remains blocked forever (or until timeout/cancellation).
///
/// In debug builds, we panic. In release, we send an error reply.
impl Drop for ReplyKey {
    fn drop(&mut self) {
        // Reply cap dropped without sending - this is usually a bug!
        // Send error reply to unblock caller
        #[cfg(debug_assertions)]
        panic!("ReplyKey dropped without sending reply!");

        #[cfg(not(debug_assertions))]
        unsafe {
            protected_call1(
                self.cap.slot as u64,
                ReplyOp::SendError as u64,
                IpcError::ReplyDropped as u64,
            );
        }
    }
}

// ==============================================
// == Kernel space object and syscall handling ==
// ==============================================

/// Reply kernel object
struct Reply {
    /// Domain waiting for this reply
    caller: DomainId,

    /// State of this reply object
    state: ReplyState,
}

#[derive(Clone, Copy, PartialEq)]
enum ReplyState {
    /// Caller is blocked waiting
    Pending,
    /// Reply was sent, object is consumed
    Used,
    /// Caller cancelled or timed out
    Cancelled,
}

#[repr(u8)]
enum ReplyOp {
    Send = 0,        // Send reply message
    SendWithCap = 1, // Send reply with cap transfer
    SendError = 2,   // Send error (used by Drop)
}

impl Reply {
    fn handle_send(&mut self, msg: &[u64; 6], cap_slot: Option<CapSlot>) -> SyscallResult {
        match self.state {
            ReplyState::Pending => {
                let caller = get_domain_mut(self.caller);

                // Copy reply to caller's registers
                caller.regs.x0 = 0; // Success
                caller.regs.x1 = msg[0]; // label
                caller.regs.x2 = msg[1];
                caller.regs.x3 = msg[2];
                caller.regs.x4 = msg[3];
                caller.regs.x5 = msg[4];
                caller.regs.x6 = msg[5];

                // Transfer cap if present
                if let Some(slot) = cap_slot {
                    let cap = current_domain().cspace.take(slot)?;
                    caller.cspace.insert(RECEIVED_CAP_SLOT, cap)?;
                    caller.regs.x7 = RECEIVED_CAP_SLOT as u64;
                } else {
                    caller.regs.x7 = u64::MAX;
                }

                // Wake caller
                caller.state = DomainState::Runnable;

                // Mark reply object as used (one-shot)
                self.state = ReplyState::Used;

                Ok(0)
            }

            ReplyState::Used => Err(SyscallError::ReplyAlreadyUsed),

            ReplyState::Cancelled => {
                // Caller gave up - just consume the reply cap
                self.state = ReplyState::Used;
                Ok(0) // Not an error, just no-op
            }
        }
    }
}

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

impl NucleusObject for Reply {
    const TYPE: ObjectType = ObjectType::Reply;
}
