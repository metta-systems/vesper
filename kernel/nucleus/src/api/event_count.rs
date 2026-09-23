//! `EventCount` operations: `Advance`/`Await`/`Read` (Phase 6 vertical,
//! 2026-09-18).
//!
//! Wire schema (see `doc/nucleus_capabilities.md`):
//! - `Advance` `0`: `x2` delta (nonzero), `x3..x7` zero. Adds the delta and
//!   completes every queued `Await` whose target the new value satisfies
//!   (broadcast wakeups; each is resumed with the new value). Success
//!   returns the new value. An advance that would exceed `u64::MAX`
//!   returns `CounterOverflow`, leaves the counter unchanged, and completes
//!   every queued `Await` with the same error so waiters observe the
//!   producer's failure instead of blocking indefinitely (selected
//!   2026-09-18).
//! - `Await` `1`: `x2` target, `x3` timeout (nanoseconds; `u64::MAX` =
//!   infinite; zero and finite values are invalid/unsupported until the
//!   time subsystem exists), `x4..x7` zero. An already-satisfied await
//!   returns the current value. An await that would block reports
//!   `InvokeOutcome::Blocked`: the syscall entry parks the caller and
//!   switches, and the caller's return happens when the record completes
//!   (completion foundation, 2026-09-16).
//! - `Read` `2`: no arguments. Returns the current value. Never blocks.
//!
//! Authority (per-kind rights reuse): `Advance` requires `SEND`; `Await`
//! and `Read` require `RECV`.

use {
    crate::{
        api::{InvokeOutcome, current_waiter, wake_waiter},
        objects::{
            ArchObjects, EventCount, KeyTable, Nucleus,
            access::Access,
            event_count::{AdvanceOutcome, AwaitOutcome},
            key_table::CallerTable,
        },
    },
    libobject::{CapError, ObjectType, RawKey, Rights, event_count::EventCountOp},
    libqemu::semihosting as semi,
};

/// Handle an `EventCount` capability invocation.
///
/// `caller` is the caller's own table context, through which the invoked
/// `key` is resolved. The event-count object is a checked pool identity
/// resolved through the guarded `Access` context.
pub fn invoke<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    key: RawKey,
    op: u64,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<InvokeOutcome, CapError> {
    let op = EventCountOp::try_from(op)?;

    // Resolve the invoked EventCount capability through the caller's own
    // table, copying out the checked identity and authority.
    let (id, rights) = {
        let caller_table = access.resolve_carved_mut::<KeyTable>(caller.addr)?;
        let entry = caller_table
            .lookup(key, caller.guard)
            .map_err(|e| e.with_key_operand(0))?;
        if entry.object_type() != ObjectType::EVENT_COUNT {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::EVENT_COUNT,
                found: entry.object_type(),
            });
        }
        (
            entry.object_id().map_err(|e| e.with_key_operand(0))?,
            entry.rights(),
        )
    };

    match op {
        EventCountOp::Advance => {
            if !rights.has(Rights::SEND) {
                return Err(CapError::InsufficientRights);
            }
            if args[1..].iter().any(|&arg| arg != 0) {
                return Err(CapError::InvalidOperation);
            }
            let mut event_count =
                access.resolve_mut::<EventCount>(&mut nucleus.pools.event_counts, id)?;
            match event_count.advance(args[0], &mut nucleus.pending)? {
                AdvanceOutcome::Advanced { new_value, woken } => {
                    // Broadcast wakeups: every satisfied waiter's thread
                    // becomes runnable. The guard's borrow ended at the
                    // advance call, so the scheduler can be touched here.
                    for record in woken.iter() {
                        wake_waiter(nucleus, record)?;
                    }
                    semi::println!("✅ EventCount::Advance(0x{new_value:x})");
                    Ok(InvokeOutcome::Complete((new_value, 0)))
                }
                AdvanceOutcome::Overflow { woken } => {
                    // The counter is unchanged; every queued waiter was
                    // completed with the same error the advancer sees.
                    for record in woken.iter() {
                        wake_waiter(nucleus, record)?;
                    }
                    semi::println!("⬅️ EventCount::Advance overflowed");
                    Err(CapError::CounterOverflow)
                }
            }
        }

        EventCountOp::Await => {
            if !rights.has(Rights::RECV) {
                return Err(CapError::InsufficientRights);
            }
            if args[2..].iter().any(|&arg| arg != 0) {
                return Err(CapError::InvalidOperation);
            }
            // Timeout model (selected 2026-09-16): `u64::MAX` = infinite.
            // Zero is invalid and finite values are unsupported until the
            // time subsystem exists — both rejected with a defined error
            // rather than pretending to time out.
            if args[1] != u64::MAX {
                return Err(CapError::InvalidOperation);
            }
            // The already-satisfied path is real behavior: return the
            // current value. The blocking path reports Blocked: the syscall
            // entry parks the caller and switches; the caller's return
            // happens when the record completes.
            let waiter = current_waiter(nucleus)?;
            let mut event_count =
                access.resolve_mut::<EventCount>(&mut nucleus.pools.event_counts, id)?;
            match event_count.await_ge(args[0], waiter, &mut nucleus.pending)? {
                AwaitOutcome::Ready(value) => {
                    semi::println!("✅ EventCount::Await(0x{value:x})");
                    Ok(InvokeOutcome::Complete((value, 0)))
                }
                // The blocking path's success line prints at the resume that
                // delivers the completed result (see `park_and_switch`).
                AwaitOutcome::Blocked(record) => Ok(InvokeOutcome::Blocked(record)),
            }
        }

        EventCountOp::Read => {
            if !rights.has(Rights::RECV) {
                return Err(CapError::InsufficientRights);
            }
            if args.iter().any(|&arg| arg != 0) {
                return Err(CapError::InvalidOperation);
            }
            let mut event_count =
                access.resolve_mut::<EventCount>(&mut nucleus.pools.event_counts, id)?;
            let value = event_count.read();
            semi::println!("✅ EventCount::Read(0x{value:x})");
            Ok(InvokeOutcome::Complete((value, 0)))
        }
    }
}
