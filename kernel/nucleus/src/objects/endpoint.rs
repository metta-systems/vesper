// ====================
// == Nucleus object ==
// ====================

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

impl NucleusObject for Endpoint {
    const TYPE: ObjectType = ObjectType::Endpoint;
}
