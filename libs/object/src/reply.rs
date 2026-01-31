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
    key: Key<ReplyType>,
}

enum ReplyType {};

#[repr(u8)]
pub enum ReplyOp {
    Send = 0,        // Send reply message
    SendWithCap = 1, // Send reply with cap transfer
    SendError = 2,   // Send error (used by Drop)
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
        unsafe {
            protected_call1(
                self.cap.slot as u64,
                ReplyOp::SendError as u64,
                IpcError::ReplyDropped as u64,
            );
        }

        // Panic in debug mode to detect misuse.
        #[cfg(debug_assertions)]
        panic!("ReplyKey dropped without sending reply!");
    }
}
