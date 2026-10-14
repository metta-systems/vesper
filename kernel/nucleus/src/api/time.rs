//! `Time` operations: `Transfer`/`Merge`/`Query`.
//!
//! Wire schema (see `doc/capabilities-contract.md`):
//! - `Transfer` `0`, invoked on the source: `x2` destination Time key, `x3`
//!   amount in nanoseconds (`u64::MAX` = infinite, only from an infinite
//!   source), `x4` the priority the sender assigns (16 bits), `x5..x7` zero.
//!   `WRITE` on both. Success returns zeros.
//! - `Merge` `1`, invoked on the destination: `x2` source Time key, `x3..x7`
//!   zero. `WRITE` on both. Moves all of the source's budget and leaves it
//!   empty. Success returns zeros.
//! - `Query` `2`: no arguments; `READ`. Returns the remaining budget in `x1`
//!   and the priority at the last chain entry in `x2`.
//!
//! The same Time as both operands, Times of different cores, a zero amount, a
//! priority wider than 16 bits, nonzero reserved words, and a source bound
//! to a live Thread (by `Scheduler.ConfigureThread`) fail with
//! `InvalidOperation`. Both new states are computed before either is
//! written, so every failure leaves both Times unchanged.

use {
    crate::objects::{
        KeyTable, Time,
        access::{Access, GuardMut},
        carved::CarvedId,
        key_table::CallerTable,
        object_pool::PairIdentityError,
    },
    libobject::{
        CapError, ObjectType, RawKey, Rights,
        time::{Priority, TimeOp},
    },
    libqemu::semihosting as semi,
};

/// Handle a `Time` capability invocation.
pub fn invoke(
    access: &Access,
    caller: CallerTable,
    key: RawKey,
    op: u64,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    let op = TimeOp::try_from(op)?;
    match op {
        TimeOp::Transfer => {
            if args[3..].iter().any(|&arg| arg != 0) {
                return Err(CapError::InvalidOperation);
            }
            let destination_key = RawKey::from_wire(args[0]);
            let amount = args[1];
            let priority = Priority::try_from(args[2])
                .map_err(|_wider_than_16_bits| CapError::InvalidOperation)?;
            let source_id = resolve_time(access, caller, key, 0, Rights::WRITE)?;
            let destination_id = resolve_time(access, caller, destination_key, 2, Rights::WRITE)?;
            let (mut source, mut destination) = resolve_both(
                access,
                (key, source_id, 0),
                (destination_key, destination_id, 2),
            )?;
            // A Time bound to a live Thread is that Thread's own: nothing
            // drains it.
            if source.is_bound() {
                return Err(CapError::InvalidOperation);
            }
            let (new_source, new_destination) =
                Time::transfer(&source, source_id, &destination, amount, priority)?;
            *source = new_source;
            *destination = new_destination;
            semi::println!("✅ Time::Transfer({amount}ns, priority 0x{priority:x})");
            Ok((0, 0))
        }

        TimeOp::Merge => {
            if args[1..].iter().any(|&arg| arg != 0) {
                return Err(CapError::InvalidOperation);
            }
            let source_key = RawKey::from_wire(args[0]);
            let destination_id = resolve_time(access, caller, key, 0, Rights::WRITE)?;
            let source_id = resolve_time(access, caller, source_key, 2, Rights::WRITE)?;
            let (mut destination, mut source) =
                resolve_both(access, (key, destination_id, 0), (source_key, source_id, 2))?;
            // A Time bound to a live Thread is that Thread's own: nothing
            // drains it.
            if source.is_bound() {
                return Err(CapError::InvalidOperation);
            }
            let (new_destination, new_source) = Time::merge(&destination, &source)?;
            *destination = new_destination;
            *source = new_source;
            semi::println!("✅ Time::Merge({}ns)", destination.budget_ns());
            Ok((0, 0))
        }

        TimeOp::Query => {
            if args.iter().any(|&arg| arg != 0) {
                return Err(CapError::InvalidOperation);
            }
            let id = resolve_time(access, caller, key, 0, Rights::READ)?;
            let time = access
                .resolve_object::<Time>(id)
                .map_err(|error| error.for_key(key, 0))?;
            let (budget_ns, priority) = (time.budget_ns(), time.priority());
            semi::println!("✅ Time::Query({budget_ns}ns, priority 0x{priority:x})");
            Ok((budget_ns, u64::from(priority)))
        }
    }
}

/// Look `key` up in the caller's table, check that it is a Time capability
/// with `required` rights, and copy out its identity. `operand` is the input
/// register reported in key errors.
fn resolve_time(
    access: &Access,
    caller: CallerTable,
    key: RawKey,
    operand: u8,
    required: u8,
) -> Result<CarvedId, CapError> {
    let caller_table = access.resolve_caller_table(caller)?;
    let entry = caller_table
        .lookup(key, caller.guard)
        .map_err(|error| error.with_key_operand(operand))?;
    if entry.object_type() != ObjectType::TIME {
        return Err(CapError::TypeMismatch {
            expected: ObjectType::TIME,
            found: entry.object_type(),
        });
    }
    if !entry.rights().has(required) {
        return Err(CapError::InsufficientRights);
    }
    entry
        .carved_id()
        .map_err(|error| error.with_key_operand(operand))
}

/// Resolve two distinct Times for update. Each operand is (key, identity,
/// input register); naming one Time twice fails with `InvalidOperation`.
fn resolve_both<'ctx>(
    access: &Access<'ctx>,
    first: (RawKey, CarvedId, u8),
    second: (RawKey, CarvedId, u8),
) -> Result<(GuardMut<'ctx, Time>, GuardMut<'ctx, Time>), CapError> {
    access
        .resolve_object_pair_mut::<Time>(first.1, second.1)
        .map_err(|error| match error {
            PairIdentityError::First(error) => error.for_key(first.0, first.2),
            PairIdentityError::Second(error) => error.for_key(second.0, second.2),
            PairIdentityError::Aliased => CapError::InvalidOperation,
        })
}
