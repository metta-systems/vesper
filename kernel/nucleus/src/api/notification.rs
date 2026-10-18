//! `Notification` operations: `Signal`/`Await`/`Poll`
//!
//! Wire schema (see `doc/capabilities-contract.md`):
//! - `Signal` `0`: `x2` bits — used when the capability badge is zero —
//!   `x3..x7` zero. ORs the authorized bits into the bitmap and wakes at
//!   most one waiter. Success returns zeros.
//! - `Await` `1`: `x2` timeout (nanoseconds; `u64::MAX` = infinite; zero and
//!   finite values are invalid/unsupported until the time subsystem exists),
//!   `x3..x7` zero. An already-satisfied wait consumes and returns the
//!   pending bits. A wait that would block reports `InvokeOutcome::Blocked`:
//!   the syscall entry parks the caller and switches, and the caller's
//!   return happens when the record completes
//! - `Poll` `2`: no arguments. Consumes and returns pending bits (zero =
//!   none pending). Never blocks.
//!
//! A Notification listed in a `WaitSet` refuses `Await` with `Watched`.
//!
//! Authority (per-kind rights reuse): `Signal` requires `SEND`; `Await` and
//! `Poll` require `RECV`. Signal bits come from the capability's badge when
//! nonzero, else the caller-supplied argument.
//! Retype installs badge zero, so the argument path is the one reached
//! until badge derivation (D4) exists.

use {
    crate::{
        api::{InvokeOutcome, current_waiter, wake_waiter},
        objects::{
            ArchObjects, KeyTable, Notification, Nucleus, access::Access, key_table::CallerTable,
        },
    },
    libobject::{CapError, ObjectType, RawKey, Rights, notification::NotificationOp},
    libqemu::semihosting as semi,
};

/// Handle a `Notification` capability invocation.
///
/// `caller` is the caller's own table context, through which the invoked
/// `key` is resolved. The notification object is a checked pool identity
/// resolved through the guarded `Access` context.
pub fn invoke<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    key: RawKey,
    op: u64,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<InvokeOutcome, CapError> {
    let op = NotificationOp::try_from(op)?;

    // Resolve the invoked Notification capability through the caller's own
    // table, copying out the checked identity and authority.
    let (id, rights, badge) = {
        let caller_table = access.resolve_caller_table(caller)?;
        let entry = caller_table
            .lookup(key, caller.guard)
            .map_err(|e| e.with_key_operand(0))?;
        if entry.object_type() != ObjectType::NOTIFICATION {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::NOTIFICATION,
                found: entry.object_type(),
            });
        }
        (entry.carved_id()?, entry.rights(), entry.badge())
    };

    match op {
        NotificationOp::Signal => {
            if !rights.has(Rights::SEND) {
                return Err(CapError::InsufficientRights);
            }
            if args[1..].iter().any(|&arg| arg != 0) {
                return Err(CapError::InvalidOperation);
            }
            // Selected hybrid: a badged capability signals its
            // badge; an unbadged one (badge zero — what Retype installs)
            // signals the caller-supplied argument.
            let bits = if badge != 0 {
                u64::from(badge)
            } else {
                args[0]
            };
            let mut notification = access
                .resolve_object::<Notification>(id)
                .map_err(|error| error.for_key(key, 0))?;
            // One-consumer delivery: the front waiter's record completes
            // with the delivered bitmap and its thread becomes runnable.
            if let Some(record) = notification.signal(bits, &mut nucleus.pending)? {
                wake_waiter(nucleus, record)?;
            }
            nucleus.wake_wait_sets();
            semi::println!("✅ Notification::Signal(0x{bits:x})");
            Ok(InvokeOutcome::Complete((0, 0)))
        }

        NotificationOp::Await => {
            if !rights.has(Rights::RECV) {
                return Err(CapError::InsufficientRights);
            }
            if args[1..].iter().any(|&arg| arg != 0) {
                return Err(CapError::InvalidOperation);
            }
            // Timeout model: `u64::MAX` = infinite, zero is invalid, and any
            // other value is a relative deadline in nanoseconds.
            let timeout_ns = args[0];
            if timeout_ns == 0 {
                return Err(CapError::InvalidOperation);
            }
            // The already-satisfied path is real behavior: consume and
            // return pending bits. The blocking path reports Blocked: the
            // syscall entry parks the caller and switches; the caller's
            // return happens when the record completes.
            let waiter = current_waiter(nucleus)?;
            let mut notification = access
                .resolve_object::<Notification>(id)
                .map_err(|error| error.for_key(key, 0))?;
            match notification.await_bits(id, waiter, &mut nucleus.pending)? {
                crate::objects::notification::AwaitOutcome::Ready(bits) => {
                    semi::println!("✅ Notification::Await(0x{bits:x})");
                    Ok(InvokeOutcome::Complete((bits, 0)))
                }
                // The blocking path's success line prints at the resume that
                // delivers the completed result (see `park_and_resume`).
                crate::objects::notification::AwaitOutcome::Blocked(record) => {
                    if timeout_ns != u64::MAX {
                        nucleus
                            .set_wait_timeout(record, timeout_ns)
                            .unwrap_or_else(|_| panic!("a fresh waiting record takes a deadline"));
                    }
                    Ok(InvokeOutcome::Blocked(record))
                }
            }
        }

        NotificationOp::Poll => {
            if !rights.has(Rights::RECV) {
                return Err(CapError::InsufficientRights);
            }
            if args.iter().any(|&arg| arg != 0) {
                return Err(CapError::InvalidOperation);
            }
            let mut notification = access
                .resolve_object::<Notification>(id)
                .map_err(|error| error.for_key(key, 0))?;
            let bits = notification.poll();
            semi::println!("✅ Notification::Poll(0x{bits:x})");
            Ok(InvokeOutcome::Complete((bits, 0)))
        }
    }
}
