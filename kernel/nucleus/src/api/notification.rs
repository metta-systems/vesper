use crate::syscall::{protected_call0, protected_call1};

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

/// Notification capability - bitmap-based async signaling.
/// Best for: IRQs, completion events, wakeups.
pub struct NotificationKey {
    key: Key<Notification>,
}

impl NotificationKey {
    /// Signal: atomic OR into bitmap (always non-blocking)
    /// Multiple signals to same bit coalesce.
    /// ~30 cycles, no domain switch needed
    #[inline]
    pub fn signal(&self, bits: u64) -> Result<()> {
        protected_call1(self.slot, NotifyOp::Signal as u32, bits)?;
        Ok(())
    }

    /// Wait: block until any bit set, returns + clears ALL bits
    #[inline]
    pub fn wait(&self) -> Result<u64> {
        protected_call0(self.slot, NotifyOp::Wait as u32)
    }

    /// Poll: non-blocking check, returns + clears bits
    #[inline]
    pub fn poll(&self) -> u64 {
        protected_call0(self.cap.slot as u64, NotifyOp::Poll as u32)
    }
}

// ==============================================
// == Kernel space object and syscall handling ==
// ==============================================

struct Notification {
    state: u64,         // Bitmap
    waiters: WaitQueue, // Blocked domains
    bound: Option<Key>, // Optional bound domain for fast wakeup
}

#[repr(u8)]
enum NotifyOp {
    Signal = 0,
    Wait = 1,
    Poll = 2,
}

impl Notification {
    fn signal(&mut self, bits: u64) {}
    fn wait() {
        // if bits are already set, clear and immediately return
        // otherwise block the domain..
    }
    fn poll() {}
}

// =====================
// == Syscall handler ==
// =====================

pub fn invoke(cap: &Cap, op: u32, arg0: u64) -> SyscallResult {
    let notify = cap.as_notification()?;
    match op {
        NotifyOp::Signal => {
            notify.signal(arg0); // atomic OR
            Ok(0)
        }
        NotifyOp::Wait => {
            Ok(notify.wait()) // blocks, returns + clears
        }
        NotifyOp::Poll => {
            Ok(notify.poll()) // non-blocking
        }
        _ => Err(SyscallError::InvalidOp),
    }
}

pub fn invoke(
            notify: &mut Notification,
            rights: Rights,
            badge: u32,
            op: u32,
            args: &[u64; 6],
        ) -> Result<(u64, u64), CapError> {
            let op = NotifyOp::try_from(op as u8)
                .map_err(|_| CapError::InvalidOperation)?;

            match op {
                NotifyOp::Signal => {
                    // Check we have send rights
                    if !rights.contains(Rights::SEND) {
                        return Err(CapError::InsufficientRights);
                    }

                    // Signal using badge (or args[0] if badge is 0)
                    let bits = if badge != 0 { badge as u64 } else { args[0] };
                    notify.signal(bits);
                    Ok((0, 0))
                }

                NotifyOp::Wait => {
                    // Check we have receive rights
                    if !rights.contains(Rights::RECV) {
                        return Err(CapError::InsufficientRights);
                    }

                    let bits = notify.wait(current_domain_mut());
                    Ok((bits, 0))
                }

                NotifyOp::Poll => {
                    if !rights.contains(Rights::RECV) {
                        return Err(CapError::InsufficientRights);
                    }

                    let bits = notify.poll();
                    Ok((bits, 0))
                }
            }
        }
    }
}

impl KernelObject for Notification {
    const TYPE: ObjectType = ObjectType::Notification;
}
