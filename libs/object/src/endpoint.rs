// Endpoint capability operations
//
// Endpoints enable synchronous call/return IPC with direct domain switch.
// Unlike Notifications (fire-and-forget) or EventCounts (streaming),
// Endpoints are for request/response patterns.

use {crate::key::Key, libsyscall::protected_call4};

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Endpoint capability for synchronous IPC
///
/// Two "views" of the same endpoint:
/// - Client holds EndpointKey (can Call/Send)
/// - Server holds EndpointKey with Recv rights (can Recv/Reply)
///
/// Badges identify which client is calling (granted during cap derivation)
///
/// With explicit Reply objects:
/// - recv() returns (badge, msg, ReplyCap)
/// - reply is done via ReplyCap::send(), not endpoint method
/// - reply_recv() takes a ReplyCap to consume
pub struct EndpointKey {
    key: Key<EndpointType>,
}

enum EndpointType {}

#[repr(u8)]
pub enum EndpointOp {
    /// Send message and wait for reply (client operation)
    /// Blocks until server receives, processes, and replies
    Call = 0,

    /// Send message without waiting (non-blocking)
    /// If no receiver waiting, message is dropped (returns error)
    Send = 1,

    /// Wait for incoming message (server operation)
    /// Blocks until a sender calls
    Recv = 2,

    /// Reply to caller and wait for next message (server fast-path)
    /// Combines Reply + Recv in single syscall (seL4 optimization)
    ReplyRecv = 3,

    /// Reply to caller without waiting for next
    Reply = 4,

    /// Forward call to another endpoint (proxy pattern)
    /// Transfers the reply capability to the new endpoint
    Forward = 5,
}

impl EndpointKey {
    // ═══════════════════════════════════════════════════════════════════
    // CLIENT OPERATIONS
    // ═══════════════════════════════════════════════════════════════════

    /// Call: send message and block waiting for reply
    ///
    /// This is the primary client→server operation.
    /// Performs direct domain switch to receiver (fast path).
    ///
    /// Returns the reply message (label + data + optional cap)
    pub fn call(&self, msg: &Message) -> Result<Message, IpcError> {
        let (ret0, ret1, ret2, ret3, ret4, ret5, ret6) = unsafe {
            syscall_ipc(
                self.cap.slot as u64,
                EndpointOp::Call as u64,
                msg.label,
                msg.data[0],
                msg.data[1],
                msg.data[2],
                msg.data[3],
                msg.data[4],
                msg.cap.map(|s| s as u64).unwrap_or(u64::MAX),
            )
        };

        if ret0 == 0 {
            Ok(Message {
                label: ret1,
                data: [ret2, ret3, ret4, ret5, 0],
                cap: if ret6 != u64::MAX {
                    Some(ret6 as CapSlot)
                } else {
                    None
                },
            })
        } else {
            Err(IpcError::from_code(ret0))
        }
    }

    /// Send: non-blocking send (no reply expected)
    ///
    /// If receiver is waiting → message delivered, returns Ok
    /// If no receiver → message dropped, returns Err(WouldBlock)
    pub fn send(&self, msg: &Message) -> Result<(), IpcError> {
        let ret = unsafe {
            protected_call4(
                self.cap.slot as u64,
                EndpointOp::Send as u64,
                msg.label,
                msg.data[0],
                msg.data[1],
                msg.data[2],
            )
        };
        IpcError::from_code(ret.0)
    }

    // ═══════════════════════════════════════════════════════════════════
    // SERVER OPERATIONS
    // ═══════════════════════════════════════════════════════════════════

    /// Recv: block waiting for incoming Call
    ///
    /// Returns (sender_badge, message, reply_cap)
    /// Badge identifies which client called (set during cap derivation)
    ///
    /// The ReplyCap MUST be used to reply - it's consumed on use.
    /// Dropping it without replying will send an error to the caller.
    pub fn recv(&self, reply_slot: CapSlot) -> Result<(u64, Message, ReplyCap), IpcError> {
        let (ret0, badge, label, d0, d1, d2, d3, d4, cap_slot) = unsafe {
            syscall_ipc_recv(
                self.cap.slot as u64,
                EndpointOp::Recv as u64,
                reply_slot as u64, // Where kernel places ReplyCap
            )
        };

        if ret0 == 0 {
            let msg = Message {
                label,
                data: [d0, d1, d2, d3, d4],
                cap: if cap_slot != u64::MAX {
                    Some(cap_slot as CapSlot)
                } else {
                    None
                },
            };

            // Kernel placed ReplyCap in reply_slot
            let reply_cap = ReplyCap {
                cap: Cap::new(reply_slot),
            };

            Ok((badge, msg, reply_cap))
        } else {
            Err(IpcError::from_code(ret0))
        }
    }

    /// ReplyRecv: consume reply cap, send reply, AND wait for next message
    ///
    /// This is the server fast-path: one syscall does Reply + Recv.
    ///
    /// Takes the ReplyCap to consume (must reply to previous caller)
    /// Returns new (badge, msg, reply_cap) for next caller
    pub fn reply_recv(
        &self,
        reply_cap: ReplyCap,
        reply_msg: &Message,
        next_reply_slot: CapSlot,
    ) -> Result<(u64, Message, ReplyCap), IpcError> {
        let (ret0, badge, label, d0, d1, d2, d3, d4, cap_slot) = unsafe {
            syscall_ipc_reply_recv(
                self.cap.slot as u64,
                EndpointOp::ReplyRecv as u64,
                reply_cap.cap.slot as u64, // Reply cap to consume
                next_reply_slot as u64,    // Where to place next ReplyCap
                reply_msg.label,
                reply_msg.data[0],
                reply_msg.data[1],
                reply_msg.data[2],
                reply_msg.data[3],
            )
        };

        // reply_cap consumed by kernel
        core::mem::forget(reply_cap);

        if ret0 == 0 {
            let msg = Message {
                label,
                data: [d0, d1, d2, d3, d4],
                cap: if cap_slot != u64::MAX {
                    Some(cap_slot as CapSlot)
                } else {
                    None
                },
            };

            let next_reply = ReplyCap {
                cap: Cap::new(next_reply_slot),
            };

            Ok((badge, msg, next_reply))
        } else {
            Err(IpcError::from_code(ret0))
        }
    }

    /// Forward: pass this call to another endpoint (proxy pattern)
    ///
    /// Useful for: capability-filtered proxies, load balancers, etc.
    /// The forwarded call appears to come from us (our badge)
    pub fn forward(&self, target: &EndpointKey, msg: &Message) -> Result<(), IpcError> {
        let ret = unsafe {
            protected_call4(
                self.cap.slot as u64,
                EndpointOp::Forward as u64,
                target.cap.slot as u64,
                msg.label,
                msg.data[0],
                msg.data[1],
            )
        };
        IpcError::from_code(ret.0)
    }
}

impl EndpointKey {
    /// Derive a client capability with a specific badge
    ///
    /// The badge is returned to the server on recv(), identifying the caller.
    /// This is how servers distinguish between clients.
    pub fn derive_client(&self, badge: u64, dest_slot: CapSlot) -> Result<EndpointKey, Error> {
        let (ret, _, _) = unsafe {
            protected_call4(
                CAPTBL_SELF, // Support deriving directly into a client keytable?
                KeyTableOp::CopyDerive,
                self.key.slot() as u64,
                dest_slot as u64,
                Rights::CALL.bits() as u64, // Client can only Call, not Recv
                badge,
            )
        };

        if ret == 0 {
            Ok(EndpointKey {
                cap: Cap::new(dest_slot),
            })
        } else {
            Err(Error::from_code(ret))
        }
    }
}
