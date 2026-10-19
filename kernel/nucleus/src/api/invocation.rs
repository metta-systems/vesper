//! `Invocation.Call` and `Invocation.CallWithKey` admission and preparation
//! (PPC).
//!
//! Wire schema (see `doc/capabilities-contract.md`): `x0` Invocation key,
//! `x1` operation (`0` Call, `1` `CallWithKey`), `x2..x7` six `u64` Call
//! inputs, the provisional target-SP transport in `x9`, and for `CallWithKey`
//! the caller-local key to lend in `x8`. Every input is read from the caller's
//! saved exception frame, never from live registers.
//!
//! Implementation status: [`prepare_call`] is the non-committing admission
//! stage; [`call`] adds the stage-5 commit (borrowed-entry installation,
//! continuation push and Thread migration), which `core_invoke` dispatches for
//! the Invocation kind.

use {
    crate::objects::{
        ArchObjects, KeyTable, Nucleus,
        access::Access,
        invocation::{CallTarget, CommittedCall, LendRequest, PreparedCall},
        key_table::CallerTable,
    },
    libexception::arch::aarch64::SavedContext,
    libobject::{CapError, InvocationOp, ObjectType, RawKey, Rights},
};

/// Admit `Invocation.Call` or `Invocation.CallWithKey` from the saved frame
/// and prepare its migration.
///
/// Stage 1 (operation, key, Invocation kind, `CALL` authority) runs here;
/// the object primitive continues with live target identity, the `CallWithKey`
/// stages, SP, target translation readiness, depth and the placement slot, in
/// that order.
pub fn prepare_call<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    saved: &SavedContext,
    nucleus: &Nucleus<A>,
) -> Result<PreparedCall, CapError> {
    let invocation_key = RawKey::from_wire(saved.gpr[0]);
    let operation = InvocationOp::try_from(saved.gpr[1])?;

    // Copy the checked target out of the entry; no table guard survives.
    let target = {
        let caller_table = access.resolve_caller_table(caller)?;
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
            stack_pool: entry.invocation_stack_pool()?,
            placement_window: entry.invocation_placement_window()?,
            badge: entry.badge(),
        }
    };

    let lend = match operation {
        InvocationOp::Call => None,
        InvocationOp::CallWithKey => Some(LendRequest {
            key: RawKey::from_wire(saved.gpr[8]),
            caller,
        }),
    };
    nucleus.prepare_call(access, saved, target, lend)
}

/// Admit, prepare and commit `Invocation.Call` or `Invocation.CallWithKey` in
/// one serialized interval.
///
/// Every rejection precedes the commit and preserves all state. On success
/// (for `CallWithKey`, the borrowed entry is installed in the target's table
/// and) the continuation is pushed and the current Thread has migrated into the
/// target `AddressSpace`; the returned [`CommittedCall`] still owes the
/// translation install, frame rewrite and success trace at entry, after all
/// guards and the kernel lock end.
///
/// Dispatched by `core_invoke` for the Invocation kind.
pub fn call<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    saved: &SavedContext,
    nucleus: &mut Nucleus<A>,
) -> Result<CommittedCall, CapError> {
    let prepared = prepare_call(access, caller, saved, nucleus)?;
    nucleus.commit_call(access, prepared)
}
