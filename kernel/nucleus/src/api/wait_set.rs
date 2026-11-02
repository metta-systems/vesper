//! `WaitSet` operations: `AddMember`/`UpdateMember`/`RemoveMember`/`Await`
//!
//! Wire schema (see `doc/capabilities-contract.md`):
//! - `AddMember` `0`: `x2` the member's key (a Notification or `EventCount` in
//!   the caller's table, needing `RECV`), `x3` slot (0 to 7, vacant), `x4`
//!   condition, `x5..x7` zero. The condition is the Notification bits to
//!   watch (nonzero) or the `EventCount` target to reach.
//! - `UpdateMember` `1`: `x2` slot (occupied), `x3` condition, `x4..x7` zero.
//! - `RemoveMember` `2`: `x2` slot (occupied), `x3..x7` zero.
//! - `Await` `3`: `x2` timeout (nanoseconds; `u64::MAX` = infinite, zero
//!   invalid), `x3..x7` zero. Returns the mask of ready slots in `x1` (bit
//!   *n* for slot *n*) and consumes nothing. With no ready member the caller
//!   blocks, reported as `InvokeOutcome::Blocked`, until a Signal or Advance
//!   makes a member ready or the timeout ends the wait with `TIMED_OUT` and
//!   an empty mask. A set has at most one waiter (`PoolExhausted`).
//!
//! Authority (per-kind rights reuse): the membership operations require
//! `WRITE` on the set, `Await` requires `RECV`.

use {
    crate::{
        api::{InvokeOutcome, current_waiter},
        objects::{
            ArchObjects, Notification, Nucleus, WaitSet, access::Access, key_table::CallerTable,
            wait_set::WaitSetOutcome,
        },
    },
    libobject::{CapError, ObjectType, RawKey, Rights, wait_set::WaitSetOp},
    libqemu::semihosting as semi,
};

/// Handle a `WaitSet` capability invocation.
///
/// `caller` is the caller's own table context, through which the invoked
/// `key` is resolved.
pub fn invoke<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    key: RawKey,
    op: u64,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<InvokeOutcome, CapError> {
    let op = WaitSetOp::try_from(op)?;

    // Resolve the invoked WaitSet capability through the caller's own table,
    // copying out the checked identity and authority.
    let (id, rights) = {
        let caller_table = access.resolve_caller_table(caller)?;
        let entry = caller_table
            .lookup(key, caller.guard)
            .map_err(|e| e.with_key_operand(0))?;
        if entry.object_type() != ObjectType::WAIT_SET {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::WAIT_SET,
                found: entry.object_type(),
            });
        }
        (entry.carved_id()?, entry.rights())
    };

    match op {
        WaitSetOp::AddMember => {
            if !rights.has(Rights::WRITE) {
                return Err(CapError::InsufficientRights);
            }
            if args[3..].iter().any(|&arg| arg != 0) {
                return Err(CapError::InvalidOperation);
            }
            // The member: a Notification or `EventCount` the caller may wait on.
            let member = {
                let caller_table = access.resolve_caller_table(caller)?;
                let entry = caller_table
                    .lookup(RawKey::from_wire(args[0]), caller.guard)
                    .map_err(|e| e.with_key_operand(2))?;
                if entry.object_type() != ObjectType::NOTIFICATION
                    && entry.object_type() != ObjectType::EVENT_COUNT
                {
                    return Err(CapError::TypeMismatch {
                        expected: ObjectType::NOTIFICATION,
                        found: entry.object_type(),
                    });
                }
                if !entry.rights().has(Rights::RECV) {
                    return Err(CapError::InsufficientRights);
                }
                entry.carved_id()?
            };
            let mut wait_set = access
                .resolve_object::<WaitSet>(id)
                .map_err(|error| error.for_key(key, 0))?;
            wait_set.add_member(args[1], member, args[2])?;
            // A Notification in a set refuses direct waits from now on, and
            // one that already has a direct waiter cannot join: undo the
            // slot if the Notification declines.
            if member.kind == ObjectType::NOTIFICATION {
                let watched = match access.resolve_object::<Notification>(member) {
                    Ok(mut notification) => notification.watch(),
                    Err(error) => Err(error.for_key(RawKey::from_wire(args[0]), 2)),
                };
                if let Err(error) = watched {
                    // The slot was vacant a moment ago, so it can be emptied.
                    wait_set.remove_member(args[1]).ok();
                    return Err(error);
                }
            }
            // The new member may already be ready.
            nucleus.wake_wait_sets();
            semi::println!("✅ WaitSet::AddMember({})", args[1]);
            Ok(InvokeOutcome::Complete((0, 0)))
        }

        WaitSetOp::UpdateMember => {
            if !rights.has(Rights::WRITE) {
                return Err(CapError::InsufficientRights);
            }
            if args[2..].iter().any(|&arg| arg != 0) {
                return Err(CapError::InvalidOperation);
            }
            let mut wait_set = access
                .resolve_object::<WaitSet>(id)
                .map_err(|error| error.for_key(key, 0))?;
            wait_set.update_member(args[0], args[1])?;
            // The new condition may already hold.
            nucleus.wake_wait_sets();
            semi::println!("✅ WaitSet::UpdateMember({})", args[0]);
            Ok(InvokeOutcome::Complete((0, 0)))
        }

        WaitSetOp::RemoveMember => {
            if !rights.has(Rights::WRITE) {
                return Err(CapError::InsufficientRights);
            }
            if args[1..].iter().any(|&arg| arg != 0) {
                return Err(CapError::InvalidOperation);
            }
            let mut wait_set = access
                .resolve_object::<WaitSet>(id)
                .map_err(|error| error.for_key(key, 0))?;
            let removed = wait_set.remove_member(args[0])?;
            // The Notification is no longer watched by this set. One that is
            // gone has nothing to update.
            if removed.object.kind == ObjectType::NOTIFICATION
                && let Ok(mut notification) = access.resolve_object::<Notification>(removed.object)
            {
                notification.unwatch();
            }
            semi::println!("✅ WaitSet::RemoveMember({})", args[0]);
            Ok(InvokeOutcome::Complete((0, 0)))
        }

        WaitSetOp::Await => {
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
            let waiter = current_waiter(nucleus)?;
            let mut wait_set = access
                .resolve_object::<WaitSet>(id)
                .map_err(|error| error.for_key(key, 0))?;
            match wait_set.await_ready(id, waiter, &mut nucleus.pending)? {
                WaitSetOutcome::Ready(mask) => {
                    semi::println!("✅ WaitSet::Await(0x{mask:x})");
                    Ok(InvokeOutcome::Complete((mask, 0)))
                }
                // The blocking path's success line prints at the resume that
                // delivers the completed result (see `park_and_resume`).
                WaitSetOutcome::Blocked(record) => {
                    if timeout_ns != u64::MAX {
                        nucleus
                            .set_wait_timeout(record, timeout_ns)
                            .unwrap_or_else(|_| panic!("a fresh waiting record takes a deadline"));
                    }
                    Ok(InvokeOutcome::Blocked(record))
                }
            }
        }
    }
}
