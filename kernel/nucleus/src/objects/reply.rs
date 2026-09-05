// ====================
// == Nucleus object ==
// ====================

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

impl NucleusObject for Reply {
    const TYPE: ObjectType = ObjectType::Reply;
}
