//! `Thread.Retire`: teardown of a non-current Thread (selected 2026-09-19 as
//! `Domain.Retire`; moved to the Thread kind 2026-09-21).
//!
//! Wire schema (see `doc/nucleus_capabilities.md`):
//! - `Retire` `4`: no arguments (`x2..x7` zero). Tears down the invoked
//!   Thread: cancels every pending record naming it as waiter, purges its
//!   queued wakeup, and reclaims its Thread-pool slot. Success returns
//!   zeros.
//!
//! Authority: `RETIRE` (selected 2026-09-19, D4): delegable lifecycle control —
//! retirement authorization follows capability permissions, not a privileged
//! owner identity.
//!
//! The current Thread may not retire itself (`InvalidOperation`): the
//! invocation must return to a surviving caller. Never-returns
//! self-retirement is wanted as soon as feasible (recorded in the contract)
//! but needs terminal entry-path work. Full Thread Start/Suspend/Resume —
//! initialized execution contexts, execution budget, legal state transitions,
//! and EL0 entry — remains Phase 7 work (D8). `Grant` `1`, `Suspend` `2`, and
//! `Resume` `3` remain unsupported operations and fail with
//! `InvalidOperation`; their design intent is recorded in the contract, not
//! implemented here. The invoked Thread's `AddressSpace` is untouched by
//! Thread.Retire — address-space teardown is the separate
//! `AddressSpace.Retire`.

use {
    crate::objects::{ArchObjects, KeyTable, Nucleus, access::Access},
    libobject::{CapError, ObjectType, RawKey, Rights},
    libqemu::semihosting as semi,
};

/// Handle a `Thread` capability invocation.
///
/// `caller_table_addr` is the caller's own table, through which the invoked
/// `thread_key` is resolved.
pub fn invoke<A: ArchObjects>(
    access: &Access,
    caller_table_addr: u64,
    thread_key: RawKey,
    op: u64,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    match op {
        4 => retire::<A>(access, caller_table_addr, thread_key, args, nucleus),
        _ => Err(CapError::InvalidOperation),
    }
}

/// `Retire` `4` (selected 2026-09-19; moved to the Thread kind 2026-09-21):
/// tear down the invoked Thread.
///
/// Teardown scope: cancel every pending record naming the Thread as waiter
/// and purge its queued wakeup (`Nucleus::cancel_thread_pending`), then
/// deallocate the Thread-pool slot — the contract's teardown-before-reuse
/// rule. Carved backing (keytable, kernel stack) stays leaked per
/// accepted-leak; the `AddressSpace` is untouched (its teardown is the
/// separate `AddressSpace.Retire`); the DCB is untouched (D5). Subsequent
/// invocations of the retired Thread's capabilities fail pool validation
/// with a defined error.
fn retire<A: ArchObjects>(
    access: &Access,
    caller_table_addr: u64,
    thread_key: RawKey,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    if args.iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }

    // Resolve the invoked Thread capability through the caller's own table,
    // copying out the checked identity.
    let thread_id = {
        let caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
        let entry = caller_table
            .lookup(thread_key)
            .map_err(|e| e.with_key_operand(0))?;
        if entry.object_type() != ObjectType::THREAD {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::THREAD,
                found: entry.object_type(),
            });
        }
        // Delegable lifecycle-control authority (selected 2026-09-19, D4).
        if !entry.rights().has(Rights::RETIRE) {
            return Err(CapError::InsufficientRights);
        }
        entry.object_id().map_err(|e| e.with_key_operand(0))?
    };

    // The caller must be a surviving Thread: retiring the current Thread
    // from inside its own invocation has no sound return path yet
    // (never-returns self-retirement is contract-recorded follow-up).
    let current = nucleus.current_thread.ok_or(CapError::InvalidDomain)?;
    if u32::from(thread_id.index) == current {
        return Err(CapError::InvalidOperation);
    }

    // Cancel-then-deallocate: the pending records and queued wakeups go
    // first, then the pool slot is reclaimed. A stale identity (already
    // retired) fails the cancellation's pool validation with a defined
    // error.
    nucleus.cancel_thread_pending(thread_id)?;
    nucleus
        .pools
        .threads
        .deallocate(thread_id)
        .map_err(|e| e.with_key_operand(0))?;
    semi::println!("✅ Thread::Retire()");
    Ok((0, 0))
}
