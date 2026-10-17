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
            ArchObjects, EventCount, ExecutionContext, KeyTable, Nucleus, Scheduler, Thread, Time,
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
        SchedulerOp::ShareRegion => share_region(access, caller, key, scheduler, args, nucleus),
        SchedulerOp::ConfigureThread => configure_thread(access, caller, scheduler, args, nucleus),
        SchedulerOp::StartThread => start_thread(access, caller, args, nucleus),
        SchedulerOp::AssignRecord => assign_record(access, caller, key, scheduler, args, nucleus),
        SchedulerOp::BindEventCount => bind_event_count(access, caller, key, scheduler, args),
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

/// `ShareRegion` `0`: add a mapped RAM Frame to the scheduler's record table.
fn share_region<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    key: RawKey,
    scheduler: CarvedId,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    let [frame_key, first_slot, stride, rest @ ..] = *args;
    if rest.iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }
    let first_slot = u32::try_from(first_slot).map_err(|_too_wide| CapError::InvalidOperation)?;
    let stride = u32::try_from(stride).map_err(|_too_wide| CapError::InvalidOperation)?;
    let entry = lookup(
        access,
        caller,
        RawKey::from_wire(frame_key),
        2,
        ObjectType::FRAME,
        Rights::READ | Rights::WRITE,
    )?;
    let frame = entry.as_frame()?;
    // The nucleus writes the records through its own mapping, so the Frame
    // must be ordinary RAM; it must already be mapped, in the AddressSpace
    // the scheduler reads the records from.
    if frame.is_device() {
        return Err(CapError::InvalidOperation);
    }
    let mapping = frame.mapping().ok_or(CapError::NotMapped)?;
    let region = {
        let mut object = access
            .resolve_object::<Scheduler>(scheduler)
            .map_err(|error| error.for_key(key, 0))?;
        object.add_region(
            mapping.address_space,
            frame.paddr,
            1_u64 << frame.size_bits,
            first_slot,
            stride,
        )?
    };
    nucleus.initialize_region(&region);
    semi::println!(
        "✅ Scheduler::ShareRegion(slots {}..{}, stride {stride})",
        region.first_slot,
        region.first_slot + region.slots
    );
    Ok((0, 0))
}

/// `AssignRecord` `3`: link a Thread to a record slot and publish it.
fn assign_record<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    key: RawKey,
    scheduler: CarvedId,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    if args[2..].iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }
    let thread_id = resolve_thread(access, caller, RawKey::from_wire(args[0]))?;
    let slot = u32::try_from(args[1]).map_err(|_too_wide| CapError::InvalidOperation)?;
    {
        let object = access
            .resolve_object::<Scheduler>(scheduler)
            .map_err(|error| error.for_key(key, 0))?;
        if !object.covers_thread_slot(slot) {
            return Err(CapError::InvalidOperation);
        }
    }
    // Only a Thread this Scheduler runs can have a record in its table.
    let thread = nucleus
        .thread(thread_id)
        .ok_or(CapError::InvalidOperation)?;
    if thread.scheduler != Some(scheduler) {
        return Err(CapError::InvalidOperation);
    }
    nucleus.assign_record(thread_id, scheduler, slot)?;
    semi::println!("✅ Scheduler::AssignRecord(slot {slot})");
    Ok((0, 0))
}

/// `BindEventCount` `4`: bind the `EventCount` advanced on scheduling events.
fn bind_event_count(
    access: &Access,
    caller: CallerTable,
    key: RawKey,
    scheduler: CarvedId,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    if args[1..].iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }
    let event_count_key = RawKey::from_wire(args[0]);
    let event_count = lookup(
        access,
        caller,
        event_count_key,
        2,
        ObjectType::EVENT_COUNT,
        Rights::SEND,
    )
    .and_then(|entry| entry.carved_id().map_err(|e| e.with_key_operand(2)))?;
    // The EventCount must be live.
    access
        .resolve_object::<EventCount>(event_count)
        .map_err(|error| error.for_key(event_count_key, 2))?;
    let mut object = access
        .resolve_object::<Scheduler>(scheduler)
        .map_err(|error| error.for_key(key, 0))?;
    object.bind_event_count(event_count)?;
    semi::println!("✅ Scheduler::BindEventCount()");
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
    // A configured Thread was never queued, so the push cannot be refused. A
    // Thread whose Time holds no budget yet waits for it instead.
    nucleus.make_runnable(thread_id);
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
    let caller_table = access.resolve_caller_table(caller)?;
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
