//! `Thread.Retire`: teardown of a non-current Thread.
//!
//! Wire schema (see `doc/capabilities-contract.md`):
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
//!
//! `Return` `0` is selected only for `CurrentReturnOnly`, not a named Thread.
//! Its admission, fault classification and pop/migration commit are
//! implemented by [`prepare_return`]/[`return_from_call`], which `core_invoke`
//! dispatches for op 0. Its faults halt the kernel under the interim policy
//! until fault delivery (D1/D7) is designed.
//! The current-relative form rejects management regardless of its rights.

use {
    crate::objects::{
        ArchObjects, KeyTable, Nucleus,
        access::Access,
        invocation::{CommittedReturn, PreparedReturn, ReturnRejection},
        key_table::CallerTable,
    },
    libexception::arch::aarch64::SavedContext,
    libobject::{CapError, ObjectType, RawKey, Rights, thread::ThreadOp},
    libqemu::semihosting as semi,
};

/// Handle a `Thread` capability invocation.
///
/// `caller` is the caller's own table context, through which the invoked
/// `thread_key` is resolved.
pub fn invoke<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    thread_key: RawKey,
    op: u64,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    match ThreadOp::try_from(op)? {
        ThreadOp::Retire => retire::<A>(access, caller, thread_key, args, nucleus),
        // Return is current-relative, not named-Thread control. Dispatch routes
        // op 0 to `return_from_call`; this handler rejects it if reached.
        ThreadOp::Return | ThreadOp::Grant | ThreadOp::Suspend | ThreadOp::Resume => {
            Err(CapError::InvalidOperation)
        }
    }
}

/// `Retire` `4`: tear down the invoked Thread.
///
/// Teardown scope: cancel every pending record naming the Thread as waiter
/// and purge its queued wakeup (`Nucleus::cancel_thread_pending`), then
/// deallocate the Thread-pool slot — the contract's teardown-before-reuse
/// rule. Carved backing (keytable, kernel stack) stays leaked per
/// accepted-leak; the `AddressSpace` is untouched (its teardown is the
/// separate `AddressSpace.Retire`); the DCB is untouched (D5). Subsequent
/// invocations of the retired Thread's capabilities fail pool validation
/// with a defined error.
/// Implementation status: the keytable backing belongs to the retained
/// `AddressSpace` association. No per-Thread kernel stack exists; retirement
/// drops its private saved context after cancelling its pending records.
fn retire<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
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
        let caller_table = access.resolve_carved_mut::<KeyTable>(caller.addr)?;
        let entry = caller_table
            .lookup(thread_key, caller.guard)
            .map_err(|e| e.with_key_operand(0))?;
        if entry.object_type() != ObjectType::THREAD {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::THREAD,
                found: entry.object_type(),
            });
        }
        // Selector restrictions are independent of rights: current-relative
        // Return authority must never resolve or manage a named Thread.
        if entry.is_thread_return_key() {
            return Err(CapError::InvalidOperation);
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
    // A Thread retired inside a fault handler frees that handler.
    nucleus.release_thread_fault(usize::from(thread_id.index));
    nucleus
        .pools
        .threads
        .deallocate(thread_id)
        .map_err(|e| e.with_key_operand(0))?;
    semi::println!("✅ Thread::Retire()");
    Ok((0, 0))
}

/// Admit `Thread.Return` from the saved frame and prepare the pop/migration.
///
/// Inputs come only from the saved frame: `x0` target-table-local packed
/// Slot(1) key, `x1` operation `0`, `x2`/`x3` payload; `x4..x7` are ignored.
/// Ordinary lookup (table guard, incarnation, bounds, presence) applies; a
/// named Thread entry is `InvalidOperation`. The sentinel carries no rights,
/// so the `CurrentReturnOnly` selector itself is the authority.
///
/// Fault delivery for the classified underflow and retired-source faults is
/// the open D1/D7 decision; dispatch currently halts on them (interim policy).
pub fn prepare_return<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    saved: &SavedContext,
    nucleus: &Nucleus<A>,
) -> Result<PreparedReturn, ReturnRejection> {
    let return_key = RawKey::from_wire(saved.gpr[0]);
    if !matches!(ThreadOp::try_from(saved.gpr[1])?, ThreadOp::Return) {
        return Err(CapError::InvalidOperation.into());
    }
    {
        let caller_table = access.resolve_carved_mut::<KeyTable>(caller.addr)?;
        let entry = caller_table
            .lookup(return_key, caller.guard)
            .map_err(|e| e.with_key_operand(0))?;
        if entry.object_type() != ObjectType::THREAD {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::THREAD,
                found: entry.object_type(),
            }
            .into());
        }
        if !entry.is_thread_return_key() {
            return Err(CapError::InvalidOperation.into());
        }
    }
    nucleus.prepare_return(access, saved)
}

/// Admit, prepare and commit `Thread.Return` in one serialized interval.
///
/// Every rejection or fault precedes the pop and preserves all state. On
/// success the top record is popped and the Thread has migrated back to its
/// source `AddressSpace`; the returned [`CommittedReturn`] still owes the
/// translation install, frame rewrite and success trace at entry.
pub fn return_from_call<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    saved: &SavedContext,
    nucleus: &mut Nucleus<A>,
) -> Result<CommittedReturn, ReturnRejection> {
    let prepared = prepare_return(access, caller, saved, nucleus)?;
    Ok(nucleus.commit_return(prepared)?)
}
