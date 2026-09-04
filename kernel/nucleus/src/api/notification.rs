// =====================
// == Syscall handler ==
// =====================

pub fn invoke(
    notify: &mut Notification,
    rights: Rights,
    badge: u32,
    op: u32,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    let op = NotifyOp::try_from(op as u8).map_err(|_| CapError::InvalidOperation)?;

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

            let bits = notify.wait(current_domain_mut()); // FIXME: can't block syscall
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
