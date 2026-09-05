use {
    crate::key::Key,
    libsyscall::{protected_call0, protected_call1},
};

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Notification capability - bitmap-based async signaling.
/// Best for: IRQs, completion events, wakeups.
pub struct NotificationKey {
    key: Key<NotificationType>,
}

enum NotificationType {}

#[repr(u8)]
pub enum NotifyOp {
    Signal = 0,
    Wait = 1,
    Poll = 2,
}

impl NotificationKey {
    /// Signal: atomic OR into bitmap (always non-blocking)
    /// Multiple signals to same bit coalesce.
    #[inline]
    pub fn signal(&self, bits: u64) -> Result<()> {
        protected_call1(self.key.slot(), NotifyOp::Signal as u32, bits)?;
        Ok(())
    }

    /// Wait: block until any bit set, returns + clears ALL bits
    #[inline]
    pub fn wait(&self) -> Result<u64> {
        protected_call0(self.key.slot(), NotifyOp::Wait as u32)
    }

    /// Poll: non-blocking check, returns + clears bits
    #[inline]
    pub fn poll(&self) -> u64 {
        protected_call0(self.key.slot(), NotifyOp::Poll as u32)
    }
}
