//! `Scheduler` operations: `ConfigureThread`/`StartThread` on the root
//! Scheduler.
//!
//! Wire schema (see `doc/capabilities-contract.md`):
//! - `ShareRegion` `0`: not defined yet; rejected with `InvalidOperation`.
//! - `ConfigureThread` `1`: `x2` Thread key (`WRITE`), `x3` `AddressSpace` key
//!   (`MAP`), `x4` entry PC, `x5` stack SP, `x6` the Thread's own Time key
//!   (`WRITE`), `x7` the argument delivered in `x0`. The Thread must be inert;
//!   the `AddressSpace` must be bound (`NotBound`) and needs a translation
//!   root (`NotMapped`) and an ASID
//!   (`NoASIDAssigned`); PC and SP must lie below the end of the user range,
//!   PC 4-byte and SP 16-byte aligned; the Time must be on this core. The
//!   Thread records the `AddressSpace`, its Time and this Scheduler and will
//!   start at EL0.
//! - `StartThread` `2`: `x2` Thread key (`WRITE`), `x3..x7` zero. The Thread
//!   must be configured and not started; it becomes runnable.
//!
//! Both need `WRITE` on the invoked Scheduler, which must be the root.
//! Every check precedes the commit, so a failure changes nothing.

use {
    crate::{
        api::KeyEntry,
        objects::{
            ArchObjects, ExecutionContext, KeyTable, Nucleus, Scheduler, Thread, Time,
            access::Access,
            arch_objects::AddressSpaceObject,
            carved::{Carved, CarvedId},
            key_table::CallerTable,
            thread::ThreadId,
        },
    },
    libexception::arch::aarch64::SavedContext,
    libobject::{CapError, ObjectType, RawKey, Rights, scheduler::SchedulerOp},
    libqemu::semihosting as semi,
};

/// Handle a `Scheduler` capability invocation.
pub fn invoke<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    key: RawKey,
    op: u64,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    let op = SchedulerOp::try_from(op)?;
    let scheduler = resolve_root_scheduler(access, caller, key)?;
    match op {
        SchedulerOp::ShareRegion => Err(CapError::InvalidOperation),
        SchedulerOp::ConfigureThread => configure_thread(access, caller, scheduler, args, nucleus),
        SchedulerOp::StartThread => start_thread(access, caller, args, nucleus),
    }
}

/// `ConfigureThread` `1`.
fn configure_thread<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    scheduler: CarvedId,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    let [thread_key, as_key, entry, stack_pointer, time_key, argument] = *args;
    let (thread_key, as_key, time_key) = (
        RawKey::from_wire(thread_key),
        RawKey::from_wire(as_key),
        RawKey::from_wire(time_key),
    );
    let thread_id = resolve_thread(access, caller, thread_key)?;
    let as_id = lookup(
        access,
        caller,
        as_key,
        3,
        ObjectType::ADDRESS_SPACE,
        Rights::MAP,
    )
    .and_then(|entry| entry.address_space_id().map_err(|e| e.with_key_operand(3)))?;
    let time_id = lookup(access, caller, time_key, 6, ObjectType::TIME, Rights::WRITE)
        .and_then(|entry| entry.carved_id().map_err(|e| e.with_key_operand(6)))?;

    let thread = nucleus
        .thread(thread_id)
        .ok_or(CapError::InvalidOperation)?;
    if thread.context != ExecutionContext::Inactive {
        return Err(CapError::InvalidOperation);
    }
    {
        let address_space = access
            .resolve_object::<A::AddressSpace>(as_id)
            .map_err(|error| error.for_key(as_key, 3))?;
        if address_space.keytable().is_none() {
            return Err(CapError::NotBound);
        }
        if address_space.translation_root().is_none() {
            return Err(CapError::NotMapped);
        }
        if address_space.asid().is_none() {
            return Err(CapError::NoASIDAssigned);
        }
    }
    if entry >= A::USER_VA_END
        || stack_pointer > A::USER_VA_END
        || !entry.is_multiple_of(4)
        || !stack_pointer.is_multiple_of(16)
    {
        return Err(CapError::InvalidOperation);
    }
    {
        let time = access
            .resolve_object::<Time>(time_id)
            .map_err(|error| error.for_key(time_key, 6))?;
        // A Time is one Thread's own: one already bound to a live Thread is
        // refused.
        if time.core() != nucleus.current_cpu() || time.is_bound() {
            return Err(CapError::InvalidOperation);
        }
    }

    // Commit: every check above passed. Resolve both objects before writing
    // either, so the Thread and its Time change together or not at all.
    let mut time = access
        .resolve_object::<Time>(time_id)
        .map_err(|error| error.for_key(time_key, 6))?;
    let thread = nucleus
        .thread_mut(thread_id)
        .ok_or(CapError::InvalidOperation)?;
    thread.address_space = as_id;
    thread.time = Some(time_id);
    thread.scheduler = Some(scheduler);
    thread.context = ExecutionContext::Configured {
        saved: SavedContext::el0(entry, stack_pointer, argument),
    };
    time.bind(thread_id);
    semi::println!("✅ Scheduler::ConfigureThread(entry {entry:#x}, sp {stack_pointer:#x})");
    Ok((0, 0))
}

/// `StartThread` `2`.
fn start_thread<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    if args[1..].iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }
    let thread_key = RawKey::from_wire(args[0]);
    let thread_id = resolve_thread(access, caller, thread_key)?;
    let thread = nucleus
        .thread_mut(thread_id)
        .ok_or(CapError::InvalidOperation)?;
    let ExecutionContext::Configured { saved } = thread.context else {
        return Err(CapError::InvalidOperation);
    };
    thread.context = ExecutionContext::NotStarted { saved };
    // A configured Thread was never queued, so the push cannot be refused.
    assert!(
        nucleus.run_queue.push(thread_id),
        "configured Thread did not queue"
    );
    semi::println!("✅ Scheduler::StartThread()");
    Ok((0, 0))
}

/// The invoked Scheduler: a live root Scheduler capability with `WRITE`.
fn resolve_root_scheduler(
    access: &Access,
    caller: CallerTable,
    key: RawKey,
) -> Result<CarvedId, CapError> {
    let id = lookup(access, caller, key, 0, ObjectType::SCHEDULER, Rights::WRITE)
        .and_then(|entry| entry.carved_id().map_err(|e| e.with_key_operand(0)))?;
    let scheduler = access
        .resolve_object::<Scheduler>(id)
        .map_err(|error| error.for_key(key, 0))?;
    if !scheduler.is_root() {
        return Err(CapError::InsufficientRights);
    }
    Ok(id)
}

/// A named, live Thread capability with `WRITE` at input register 2.
fn resolve_thread(access: &Access, caller: CallerTable, key: RawKey) -> Result<ThreadId, CapError> {
    let id = lookup(access, caller, key, 2, ObjectType::THREAD, Rights::WRITE)
        .and_then(|entry| entry.thread_id().map_err(|e| e.with_key_operand(2)))?;
    // SAFETY: Thread identities name carved records that are never freed.
    unsafe { Carved::<Thread>::validate(id) }.map_err(|error| error.for_key(key, 2))?;
    Ok(id)
}

/// Copy out the caller's entry for `key` after checking its kind and rights;
/// `operand` is the input register reported in key errors.
fn lookup(
    access: &Access,
    caller: CallerTable,
    key: RawKey,
    operand: u8,
    kind: ObjectType,
    required: u8,
) -> Result<KeyEntry, CapError> {
    let caller_table = access.resolve_carved_mut::<KeyTable>(caller.addr)?;
    let entry = *caller_table
        .lookup(key, caller.guard)
        .map_err(|error| error.with_key_operand(operand))?;
    if entry.object_type() != kind {
        return Err(CapError::TypeMismatch {
            expected: kind,
            found: entry.object_type(),
        });
    }
    if !entry.rights().has(required) {
        return Err(CapError::InsufficientRights);
    }
    Ok(entry)
}
