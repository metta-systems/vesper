//! `Thread.Suspend`, `Thread.Resume` and `Thread.Retire`: lifecycle control of
//! a named Thread.
//!
//! Wire schema (see `doc/capabilities-contract.md`):
//! - `Retire` `4`: no arguments (`x2..x7` zero). Tears down the invoked
//!   Thread: cancels every pending record naming it as waiter, purges its
//!   queued wakeup, and retires the carved Thread. Success returns
//!   zeros.
//!
//! Authority: `RETIRE`: delegable lifecycle control —
//! retirement authorization follows capability permissions, not a privileged
//! owner identity.
//!
//! The current Thread may retire itself: that call never returns (see
//! [`InvokeOutcome::Exit`]). `Suspend` `2` and `Resume` `3` need `WRITE` and
//! no arguments; their rules are in the contract and on
//! [`Nucleus::suspend_thread`] and [`Nucleus::resume_thread`]. A Thread first
//! starts through `Scheduler.StartThread`, not here. `Grant` `1` remains an
//! unsupported operation and fails with `InvalidOperation`. The invoked
//! Thread's `AddressSpace` is untouched by Thread.Retire — address-space
//! teardown is the separate `AddressSpace.Retire`.
//!
//! `Return` `0` is selected only for `CurrentReturnOnly`, not a named Thread.
//! Its admission, fault classification and pop/migration commit are
//! implemented by [`prepare_return`]/[`return_from_call`], which `core_invoke`
//! dispatches for op 0. Its faults halt the kernel under the interim policy
//! until fault delivery (D1/D7) is designed.
//! The current-relative form rejects management regardless of its rights.

use {
    crate::{
        api::InvokeOutcome,
        objects::{
            ArchObjects, KeyTable, Nucleus, Thread,
            access::Access,
            carved::Carved,
            invocation::{CommittedReturn, PreparedReturn, ReturnRejection},
            key_table::CallerTable,
            thread::ThreadId,
        },
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
) -> Result<InvokeOutcome, CapError> {
    let complete = |result: Result<(u64, u64), CapError>| result.map(InvokeOutcome::Complete);
    match ThreadOp::try_from(op)? {
        ThreadOp::Retire => retire::<A>(access, caller, thread_key, args, nucleus),
        ThreadOp::Suspend => complete(suspend::<A>(access, caller, thread_key, args, nucleus)),
        ThreadOp::Resume => complete(resume::<A>(access, caller, thread_key, args, nucleus)),
        // Return is current-relative, not named-Thread control. Dispatch routes
        // op 0 to `return_from_call`; this handler rejects it if reached.
        ThreadOp::Return | ThreadOp::Grant => Err(CapError::InvalidOperation),
    }
}

/// The Thread a named Thread capability names, after the rights check and
/// the checks every management operation makes: the selector is a named
/// Thread, and the Thread is not retired.
fn named_thread(
    access: &Access,
    caller: CallerTable,
    thread_key: RawKey,
    required: u8,
) -> Result<ThreadId, CapError> {
    let thread_id = {
        let caller_table = access.resolve_caller_table(caller)?;
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
        if !entry.rights().has(required) {
            return Err(CapError::InsufficientRights);
        }
        entry.thread_id().map_err(|e| e.with_key_operand(0))?
    };
    // A retired Thread is reported against its key.
    // SAFETY: Thread identities name carved records that are never freed.
    unsafe { Carved::<Thread>::validate(thread_id) }
        .map_err(|error| error.for_key(thread_key, 0))?;
    Ok(thread_id)
}

/// `Suspend` `2`: take the Thread out of scheduling, keeping its state, its
/// wait and its Time.
fn suspend<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    thread_key: RawKey,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    if args.iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }
    let thread_id = named_thread(access, caller, thread_key, Rights::WRITE)?;
    nucleus.suspend_thread(thread_id)?;
    semi::println!("✅ Thread::Suspend()");
    Ok((0, 0))
}

/// `Resume` `3`: end a suspension.
fn resume<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    thread_key: RawKey,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    if args.iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }
    let thread_id = named_thread(access, caller, thread_key, Rights::WRITE)?;
    nucleus.resume_thread(thread_id)?;
    semi::println!("✅ Thread::Resume()");
    Ok((0, 0))
}

/// `Retire` `4`: tear down the invoked Thread.
///
/// Teardown scope: cancel every pending record naming the Thread as waiter
/// and purge its queued wakeup (`Nucleus::cancel_thread_pending`), then
/// retire the carved Thread, which bumps its generation so every capability
/// still naming it fails with `ObjectRetired` — the contract's
/// teardown-before-reuse rule. Its memory stays with the Untyped it was
/// carved from; the `AddressSpace` is untouched (its teardown is the separate
/// `AddressSpace.Retire`); the DCB is untouched (D5).
/// Implementation status: the keytable backing belongs to the retained
/// `AddressSpace` association. No per-Thread kernel stack exists; retirement
/// drops its private saved context after cancelling its pending records.
fn retire<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    thread_key: RawKey,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<InvokeOutcome, CapError> {
    if args.iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }

    // Delegable lifecycle-control authority.
    let thread_id = named_thread(access, caller, thread_key, Rights::RETIRE)?;

    // The current Thread retiring itself never returns: the entry selects
    // the next Thread (or idles) first, so a failure there still returns an
    // error to the caller, and only then tears the Thread down.
    if thread_id == nucleus.current_thread_id()? {
        return Ok(InvokeOutcome::Exit);
    }

    // Cancel-then-retire: the pending records and queued wakeups go first,
    // then the Thread is retired.
    nucleus.cancel_thread_pending(thread_id)?;
    // A Thread retired inside a fault handler frees that handler.
    nucleus.release_thread_fault(thread_id);
    // Keys its outstanding calls borrowed leave the callees' tables.
    nucleus.release_thread_placements(access, thread_id);
    // The Thread's record, if it has one, ends as `Dying`.
    nucleus.leave_scheduler(thread_id);
    nucleus.publish_thread(thread_id, true);
    // SAFETY: as above; no reference to the Thread is live here.
    unsafe { Carved::<Thread>::retire(thread_id) }.map_err(|error| error.for_key(thread_key, 0))?;
    semi::println!("✅ Thread::Retire()");
    Ok(InvokeOutcome::Complete((0, 0)))
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
        let caller_table = access.resolve_caller_table(caller)?;
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
    let committed = nucleus.commit_return(prepared)?;
    // The call returned: a key it borrowed (CallWithKey) leaves the callee's
    // table, which is the table this Return ran in. A callee that already
    // deleted it leaves nothing to remove.
    Nucleus::<A>::release_placement(access, caller.table, committed.placement);
    Ok(committed)
}
