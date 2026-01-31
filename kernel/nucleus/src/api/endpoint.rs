// Endpoint capability operations
//
// Endpoints enable synchronous call/return IPC with direct domain switch.
// Unlike Notifications (fire-and-forget) or EventCounts (streaming),
// Endpoints are for request/response patterns.

use crate::{key::Cap, syscall::protected_call4};

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Endpoint capability for synchronous IPC
///
/// Two "views" of the same endpoint:
/// - Client holds EndpointCap (can Call/Send)
/// - Server holds EndpointCap with Recv rights (can Recv/Reply)
///
/// Badges identify which client is calling (granted during cap derivation)
///
/// With explicit Reply objects:
/// - recv() returns (badge, msg, ReplyCap)
/// - reply is done via ReplyCap::send(), not endpoint method
/// - reply_recv() takes a ReplyCap to consume
pub struct EndpointCap {
    cap: Cap<Endpoint>,
}

impl EndpointCap {
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
    pub fn forward(&self, target: &EndpointCap, msg: &Message) -> Result<(), IpcError> {
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

impl EndpointCap {
    /// Derive a client capability with a specific badge
    ///
    /// The badge is returned to the server on recv(), identifying the caller.
    /// This is how servers distinguish between clients.
    pub fn derive_client(&self, badge: u64, dest_slot: CapSlot) -> Result<EndpointCap, Error> {
        let ret = unsafe {
            syscall4(
                CAPTBL_SELF, // Support deriving directly into a client keytable?
                KeyTableOp::CopyDerive,
                self.cap.slot as u64,
                dest_slot as u64,
                Rights::CALL.bits() as u64, // Client can only Call, not Recv
                badge,
            )
        };

        if ret == 0 {
            Ok(EndpointCap {
                cap: Cap::new(dest_slot),
            })
        } else {
            Err(Error::from_code(ret))
        }
    }
}

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

// ==============================================
// == Kernel space object and syscall handling ==
// ==============================================

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

/// Message passed through endpoints
/// Fits in registers for fast IPC (~200-500 cycles)
#[repr(C)]
pub struct Message {
    /// Message label/opcode - receiver dispatches on this
    pub label: u64,

    /// 5 general-purpose data words
    pub data: [u64; 5],

    /// Optional capability to transfer (None = no transfer)
    /// Sender's cap is moved (not copied) to receiver
    pub cap: Option<CapSlot>,
}

impl Message {
    pub const fn new(label: u64) -> Self {
        Self {
            label,
            data: [0; 5],
            cap: None,
        }
    }

    pub fn with_data(label: u64, d0: u64, d1: u64, d2: u64, d3: u64, d4: u64) -> Self {
        Self {
            label,
            data: [d0, d1, d2, d3, d4],
            cap: None,
        }
    }

    pub fn with_cap(mut self, cap_slot: CapSlot) -> Self {
        self.cap = Some(cap_slot);
        self
    }
}

/// Kernel object
struct Endpoint {
    /// Domain that can receive on this endpoint (None = anyone)
    receiver: Option<DomainId>,

    /// Current state
    state: EndpointState,

    /// Waiting senders (if state == Recving, this is empty)
    /// Waiting receivers (if state == Sending, this is empty)
    wait_queue: WaitQueue,

    /// Message buffer (kernel memory)
    msg_regs: [u64; 6],

    /// Badge of sender (filled when message delivered)
    sender_badge: u64,

    /// Cap transfer slot
    transfer_cap: Option<CapSlot>,
}

enum EndpointState {
    Idle,
    /// Someone is blocked sending
    Sending,
    /// Someone is blocked receiving
    Recving,
}

impl Endpoint {
    fn handle_call(
        &mut self,
        caller: &mut Domain,
        msg: &[u64; 6],
        cap_slot: Option<CapSlot>,
        badge: u64,
    ) -> SyscallResult {
        match self.state {
            EndpointState::Recving => {
                // Receiver waiting! Fast path - direct switch
                let receiver = self.wait_queue.pop_front().unwrap();

                // Copy message to receiver
                self.msg_regs = *msg;
                self.sender_badge = badge;
                self.transfer_cap = cap_slot;

                // Block caller waiting for reply
                caller.state = DomainState::Blocked;
                caller.block_reason = BlockReason::Endpoint;

                // Wake receiver
                receiver.state = DomainState::Runnable;

                // Switch to receiver (donate remaining time)
                switch_to(receiver);

                Ok(0)
            }

            EndpointState::Idle | EndpointState::Sending => {
                // No receiver - block caller
                caller.state = DomainState::Blocked;
                caller.block_reason = BlockReason::Endpoint;

                self.wait_queue.push_back(caller.id);
                self.state = EndpointState::Sending;

                // Store message for when receiver arrives
                self.msg_regs = *msg;
                self.sender_badge = badge;
                self.transfer_cap = cap_slot;

                // Schedule someone else
                schedule_next();

                Ok(0)
            }
        }
    }

    fn handle_recv(&mut self, receiver: &mut Domain, reply_dest_slot: CapSlot) -> SyscallResult {
        match self.state {
            EndpointState::Sending => {
                let sender_info = self.wait_queue.pop_front().unwrap();

                // Create Reply object for this call
                let reply = Reply {
                    caller: sender_info.domain_id,
                    state: ReplyState::Pending,
                };

                // Allocate kernel memory for Reply object - FIXME: kernel memory allocation!
                // and place capability in receiver's cspace
                let reply_cap = kernel_alloc_reply(reply)?;
                receiver.cspace.insert(reply_dest_slot, reply_cap)?;

                // Copy message to receiver
                receiver.regs.x0 = 0; // Success
                receiver.regs.x1 = sender_info.badge;
                receiver.regs.x2 = self.msg_regs[0]; // label
                receiver.regs.x3 = self.msg_regs[1];
                receiver.regs.x4 = self.msg_regs[2];
                receiver.regs.x5 = self.msg_regs[3];
                receiver.regs.x6 = self.msg_regs[4];
                receiver.regs.x7 = self.msg_regs[5];
                // x8 = transferred cap slot (if any)

                if self.wait_queue.is_empty() {
                    self.state = EndpointState::Idle;
                }

                Ok(0)
            }

            EndpointState::Idle | EndpointState::Recving => {
                // Block receiver
                receiver.state = DomainState::Blocked;
                receiver.block_reason = BlockReason::Endpoint;
                receiver.blocked_data = reply_dest_slot as u64; // Remember where to put reply cap

                self.wait_queue.push_back(receiver.id);
                self.state = EndpointState::Recving;

                schedule_next();
                Ok(0)
            }
        }
    }

    fn handle_reply_recv(
        &mut self,
        server: &mut Domain,
        reply_cap_slot: CapSlot,
        next_reply_slot: CapSlot,
        reply_msg: &[u64; 6],
    ) -> SyscallResult {
        // 1. Send reply via the provided reply cap
        let reply_cap = server.cspace.take(reply_cap_slot)?;
        let reply = reply_cap.as_reply()?;
        reply.handle_send(reply_msg, None)?;

        // Reply object is consumed, free kernel memory
        kernel_free_reply(reply);

        // 2. Receive next message
        self.handle_recv(server, next_reply_slot)
    }
}

// =====================
// == Syscall handler ==
// =====================

pub fn invoke(cap: &Cap, op: u32, arg0: u64, arg1: u64) -> SyscallResult {
    let ep = cap.as_endpoint()?;
    match op {
        EndpointOp::Call => ep.handle_call(),
        EndpointOp::Send => ep.handle_send(),
        EndpointOp::Recv => ep.handle_recv(),
        EndpointOp::Reply => { /* invoke the reply_cap */ }
        EndpointOp::ReplyRecv => ep.handle_replyrecv(),
        EndpointOp::Forward => ep.handle_forward(),
        _ => Err(SyscallError::InvalidOp),
    }
}

impl NucleusObject for Endpoint {
    const TYPE: ObjectType = ObjectType::Endpoint;
}
