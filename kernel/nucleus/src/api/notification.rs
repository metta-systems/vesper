//! `Notification` operations: `Signal`/`Wait`/`Poll` (Phase 6 vertical,
//! 2026-09-16).
//!
//! Wire schema (see `doc/nucleus_capabilities.md`):
//! - `Signal` `0`: `x2` bits — used when the capability badge is zero —
//!   `x3..x7` zero. ORs the authorized bits into the bitmap and wakes at
//!   most one waiter (one-consumer delivery, selected 2026-09-16). Success
//!   returns zeros.
//! - `Wait` `1`: `x2` timeout (nanoseconds; `u64::MAX` = infinite; zero and
//!   finite values are invalid/unsupported until the time subsystem exists),
//!   `x3..x7` zero. An already-satisfied wait consumes and returns the
//!   pending bits. A wait that would block reports `InvokeOutcome::Blocked`:
//!   the syscall entry parks the caller and switches, and the caller's
//!   return happens when the record completes (completion foundation,
//!   2026-09-16).
//! - `Poll` `2`: no arguments. Consumes and returns pending bits (zero =
//!   none pending). Never blocks.
//!
//! Authority (per-kind rights reuse): `Signal` requires `SEND`; `Wait` and
//! `Poll` require `RECV`. Signal bits follow the selected hybrid (2026-09-16):
//! the capability's badge when nonzero, else the caller-supplied argument.
//! Retype installs badge zero, so the argument path is the initially
//! reachable one; badge derivation (D4) activates the badge path.

use {
    crate::{
        api::InvokeOutcome,
        objects::{ArchObjects, KeyTable, Notification, Nucleus, access::Access},
    },
    libobject::{CapError, ObjectType, RawKey, Rights, notification::NotificationOp},
};

/// Handle a `Notification` capability invocation.
///
/// `caller_table_addr` is the caller's own table, through which the invoked
/// `key` is resolved. The notification object is a checked pool identity
/// resolved through the guarded `Access` context.
pub fn invoke<A: ArchObjects>(
    access: &Access,
    caller_table_addr: u64,
    key: RawKey,
    op: u64,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<InvokeOutcome, CapError> {
    let op = NotificationOp::try_from(op)?;

    // Resolve the invoked Notification capability through the caller's own
    // table, copying out the checked identity and authority.
    let (id, rights, badge) = {
        let caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
        let entry = caller_table
            .lookup(key)
            .map_err(|e| e.with_key_operand(0))?;
        if entry.object_type() != ObjectType::NOTIFICATION {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::NOTIFICATION,
                found: entry.object_type(),
            });
        }
        (
            entry.object_id().map_err(|e| e.with_key_operand(0))?,
            entry.rights(),
            entry.badge(),
        )
    };

    match op {
        NotificationOp::Signal => {
            if !rights.has(Rights::SEND) {
                return Err(CapError::InsufficientRights);
            }
            if args[1..].iter().any(|&arg| arg != 0) {
                return Err(CapError::InvalidOperation);
            }
            // Selected hybrid (2026-09-16): a badged capability signals its
            // badge; an unbadged one (badge zero — what Retype installs)
            // signals the caller-supplied argument.
            let bits = if badge != 0 {
                u64::from(badge)
            } else {
                args[0]
            };
            let mut notification =
                access.resolve_mut::<Notification>(&mut nucleus.pools.notifications, id)?;
            // One-consumer delivery: the front waiter's record completes
            // with the delivered bitmap and its domain becomes runnable.
            if let Some(record) = notification.signal(bits, &mut nucleus.pending)? {
                wake_waiter(nucleus, record)?;
            }
            Ok(InvokeOutcome::Complete((0, 0)))
        }

        NotificationOp::Wait => {
            if !rights.has(Rights::RECV) {
                return Err(CapError::InsufficientRights);
            }
            if args[1..].iter().any(|&arg| arg != 0) {
                return Err(CapError::InvalidOperation);
            }
            // Timeout model (selected 2026-09-16): `u64::MAX` = infinite.
            // Zero is invalid and finite values are unsupported until the
            // time subsystem exists — both rejected with a defined error
            // rather than a pretended timeout.
            if args[0] != u64::MAX {
                return Err(CapError::InvalidOperation);
            }
            // The already-satisfied path is real behavior: consume and
            // return pending bits. The blocking path reports Blocked: the
            // syscall entry parks the caller and switches; the caller's
            // return happens when the record completes.
            let waiter = current_waiter(nucleus)?;
            let mut notification =
                access.resolve_mut::<Notification>(&mut nucleus.pools.notifications, id)?;
            match notification.wait(waiter, &mut nucleus.pending)? {
                crate::objects::notification::WaitOutcome::Ready(bits) => {
                    Ok(InvokeOutcome::Complete((bits, 0)))
                }
                crate::objects::notification::WaitOutcome::Blocked(record) => {
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
            let mut notification =
                access.resolve_mut::<Notification>(&mut nucleus.pools.notifications, id)?;
            Ok(InvokeOutcome::Complete((notification.poll(), 0)))
        }
    }
}

/// Mark the domain of a completed record runnable.
///
/// The waiter identity is incarnation-checked against the domains pool
/// before enqueueing; a stale identity (domain torn down) releases the
/// terminal record instead — its waiter will never resume.
fn wake_waiter<A: ArchObjects>(
    nucleus: &mut Nucleus<A>,
    record: crate::objects::access::ObjectId,
) -> Result<(), CapError> {
    let waiter = nucleus.pending.waiter(record)?;
    if nucleus.pools.domains.validate(waiter).is_ok() {
        if !nucleus.scheduler.push(waiter.index) {
            // The queue is sized to hold every domain-pool slot; a full
            // queue is a kernel bookkeeping bug, not an expected condition.
            panic!("runnable queue overflow");
        }
    } else if nucleus.pending.release(record).is_err() {
        panic!("failed to release a completed record with a stale waiter");
    }
    Ok(())
}

/// The current domain's incarnation-checked identity, for wait
/// registration.
///
/// The current-domain tracking is index-only today; the generation is read
/// from the authoritative domains-pool metadata so a stale identity can
/// never be registered. Coherent current-domain identity carrying its own
/// generation remains Phase 4 work.
fn current_waiter<A: ArchObjects>(
    nucleus: &Nucleus<A>,
) -> Result<crate::objects::access::ObjectId, CapError> {
    let index = nucleus.current_domain.ok_or(CapError::InvalidDomain)?;
    let index = usize::try_from(index).ok().ok_or(CapError::InvalidDomain)?;
    let generation = nucleus
        .pools
        .domains
        .generation_of(index)
        .ok_or(CapError::InvalidDomain)?;
    Ok(crate::objects::access::ObjectId {
        pool: crate::objects::access::PoolTag::Domain,
        index: u16::try_from(index).map_err(|_too_wide| CapError::InvalidDomain)?,
        generation,
    })
}
