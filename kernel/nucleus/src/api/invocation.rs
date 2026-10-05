//! `Invocation.Call` admission and preparation (PPC).
//!
//! Wire schema (see `doc/nucleus_capabilities.md`): `x0` Invocation key,
//! `x1` operation `0` (Call), `x2..x7` six `u64` Call inputs, and the
//! provisional target-SP transport in `x9`. Every input is read from the
//! caller's saved exception frame, never from live registers.
//!
//! Implementation status: [`prepare_call`] is the non-committing admission
//! stage; [`call`] adds the stage-5 commit (continuation push and Thread
//! migration). Neither is dispatched: `core_invoke` keeps the Invocation kind
//! unsupported until `Thread.Return` exists, so no Call strands its Thread.

use {
    crate::objects::{
        ArchObjects, KeyTable, Nucleus,
        access::Access,
        invocation::{CallTarget, CommittedCall, PreparedCall},
        key_table::CallerTable,
    },
    libexception::arch::aarch64::SavedContext,
    libobject::{CapError, InvocationOp, ObjectType, RawKey, Rights},
};

/// Admit `Invocation.Call` from the saved frame and prepare its migration.
///
/// Stage 1 (operation, key, Call-only kind, `CALL` authority) runs here;
/// the object primitive continues with live target identity, SP, target
/// translation readiness and depth, in that order.
pub fn prepare_call<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    saved: &SavedContext,
    nucleus: &Nucleus<A>,
) -> Result<PreparedCall, CapError> {
    let invocation_key = RawKey::from_wire(saved.gpr[0]);
    match InvocationOp::try_from(saved.gpr[1])? {
        InvocationOp::Call => {}
    }

    // Copy the checked target out of the entry; no table guard survives.
    let target = {
        let caller_table = access.resolve_carved_mut::<KeyTable>(caller.addr)?;
        let entry = caller_table
            .lookup(invocation_key, caller.guard)
            .map_err(|e| e.with_key_operand(0))?;
        if entry.object_type() != ObjectType::INVOCATION {
            return Err(CapError::TypeMismatch {
                expected: ObjectType::INVOCATION,
                found: entry.object_type(),
            });
        }
        if !entry.rights().has(Rights::CALL) {
            return Err(CapError::InsufficientRights);
        }
        let (address_space, function_address) = entry.invocation_target()?;
        CallTarget {
            address_space,
            function_address,
            stack_extent: entry.invocation_stack_extent()?,
        }
    };

    nucleus.prepare_call(access, saved, target)
}

/// Admit, prepare and commit `Invocation.Call` in one serialized interval.
///
/// Every rejection precedes the commit and preserves all state. On success
/// the continuation is pushed and the current Thread has migrated into the
/// target `AddressSpace`; the returned [`CommittedCall`] still owes the
/// translation install, frame rewrite and success trace at entry, after all
/// guards and the kernel lock end.
///
/// Implementation status: not dispatched. Enabling Call through `core_invoke`
/// waits for `Thread.Return`, so a migrated Thread is never stranded.
pub fn call<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    saved: &SavedContext,
    nucleus: &mut Nucleus<A>,
) -> Result<CommittedCall, CapError> {
    let prepared = prepare_call(access, caller, saved, nucleus)?;
    nucleus.commit_call(prepared)
}
