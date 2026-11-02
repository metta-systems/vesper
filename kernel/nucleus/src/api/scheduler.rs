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
        api::{InvokeOutcome, KeyEntry},
        objects::{
            ArchObjects, EventCount, ExecutionContext, KeyTable, Nucleus, Scheduler, Thread, Time,
            access::Access,
            arch_objects::AddressSpaceObject,
            carved::{Carved, CarvedId},
            completion::{PendingKind, PendingState},
            key_table::CallerTable,
            nucleus::Dispatched,
            thread::ThreadId,
            time::ns_to_ticks_ceil,
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
) -> Result<InvokeOutcome, CapError> {
    let op = SchedulerOp::try_from(op)?;
    let scheduler = resolve_scheduler(access, caller, key)?;
    let complete = |result: Result<(u64, u64), CapError>| result.map(InvokeOutcome::Complete);
    match op {
        SchedulerOp::ShareRegion => {
            complete(share_region(access, caller, key, scheduler, args, nucleus))
        }
        SchedulerOp::ConfigureThread => {
            complete(configure_thread(access, caller, scheduler, args, nucleus))
        }
        SchedulerOp::StartThread => {
            complete(start_thread(access, caller, scheduler, args, nucleus))
        }
        SchedulerOp::Adopt => complete(adopt(access, caller, key, scheduler, args)),
        SchedulerOp::MoveThread => {
            complete(move_thread(access, caller, key, scheduler, args, nucleus))
        }
        SchedulerOp::AssignRecord => {
            complete(assign_record(access, caller, key, scheduler, args, nucleus))
        }
        SchedulerOp::BindEventCount => {
            complete(bind_event_count(access, caller, key, scheduler, args))
        }
        SchedulerOp::BindDispatcher => {
            complete(bind_dispatcher(access, caller, scheduler, args, nucleus))
        }
        SchedulerOp::Dispatch => dispatch(access, caller, key, scheduler, args, nucleus),
        SchedulerOp::ConfigureActivation => {
            complete(configure_activation(access, key, scheduler, args))
        }
        SchedulerOp::ArmTimer => complete(arm_timer(access, key, scheduler, args, nucleus)),
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
    nucleus.join_scheduler(thread_id, scheduler);
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

/// `BindDispatcher` `5`: bind the Thread that makes this scheduler's
/// decisions, and start it.
fn bind_dispatcher<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    scheduler: CarvedId,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    if args[1..].iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }
    let thread_id = resolve_thread(access, caller, RawKey::from_wire(args[0]))?;
    // The root's dispatcher is a Thread the root configured; a child's is a
    // Thread its parent configured (checked in the nucleus).
    nucleus.bind_dispatcher(scheduler, thread_id)?;
    semi::println!("✅ Scheduler::BindDispatcher()");
    Ok((0, 0))
}

/// `ArmTimer` `8`: arm the dispatcher's timer for an absolute deadline on the
/// monotonic clock, in nanoseconds (0 disarms it). Only the bound dispatcher
/// may call it. The result is the time now and the previous deadline.
fn arm_timer<A: ArchObjects>(
    access: &Access,
    key: RawKey,
    scheduler: CarvedId,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    if args[1..].iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }
    let dispatcher = nucleus.current_thread_id()?;
    let bound = access
        .resolve_object::<Scheduler>(scheduler)
        .map_err(|error| error.for_key(key, 0))?
        .dispatcher();
    if bound != Some(dispatcher) {
        return Err(CapError::InvalidOperation);
    }
    let armed = nucleus.arm_dispatcher_timer(scheduler, args[0])?;
    semi::println!("✅ Scheduler::ArmTimer(deadline {} ns)", args[0]);
    Ok(armed)
}

/// `ConfigureActivation` `7`: choose the events that activate the
/// dispatcher besides the ones that always do.
fn configure_activation(
    access: &Access,
    key: RawKey,
    scheduler: CarvedId,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    if args[1..].iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }
    let mut object = access
        .resolve_object::<Scheduler>(scheduler)
        .map_err(|error| error.for_key(key, 0))?;
    object.set_activation_mask(args[0])?;
    semi::println!("✅ Scheduler::ConfigureActivation(mask {:#x})", args[0]);
    Ok((0, 0))
}

/// Whether the Thread `id` can be dispatched now: runnable, off every queue,
/// with budget.
fn dispatchable<A: ArchObjects>(nucleus: &Nucleus<A>, id: ThreadId) -> bool {
    let Some(thread) = nucleus.thread(id) else {
        return false;
    };
    let resumable = match thread.context {
        ExecutionContext::NotStarted { .. } | ExecutionContext::Preempted { .. } => true,
        // A woken waiter: its pending record has completed.
        ExecutionContext::Parked { record, .. } => matches!(
            nucleus.pending.state(record),
            Ok(PendingState::Completed { .. })
        ),
        _ => false,
    };
    resumable && !thread.queued && !thread.awaiting_budget && !thread.suspended
}

/// `Dispatch` `6`: the dispatcher runs `x2` until the nucleus activates it.
///
/// On one CPU the dispatcher cannot run while it dispatches, so the call
/// blocks. It returns at once if an activation is already pending.
fn dispatch<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    key: RawKey,
    scheduler: CarvedId,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<InvokeOutcome, CapError> {
    let [
        thread_key,
        time_key,
        limit_ns,
        expected_generation,
        cpu,
        reserved,
    ] = *args;
    if cpu != 0 || reserved != 0 || limit_ns == 0 {
        return Err(CapError::InvalidOperation);
    }
    let dispatcher = nucleus.current_thread_id()?;
    let (bound, pending, events, own_grant) = {
        let object = access
            .resolve_object::<Scheduler>(scheduler)
            .map_err(|error| error.for_key(key, 0))?;
        (
            object.dispatcher(),
            object.pending(),
            object.events(),
            object.grant(),
        )
    };
    // Only the bound dispatcher dispatches.
    if bound != Some(dispatcher) {
        return Err(CapError::InvalidOperation);
    }
    let target = if thread_key == 0 {
        None
    } else {
        Some(resolve_thread(
            access,
            caller,
            RawKey::from_wire(thread_key),
        )?)
    };
    // An activation raised while the dispatcher ran returns at once.
    if pending != 0 {
        let reasons = access
            .resolve_object::<Scheduler>(scheduler)
            .map_err(|error| error.for_key(key, 0))?
            .take_pending();
        return Ok(InvokeOutcome::Complete((reasons, events)));
    }
    if let Some(thread_id) = target {
        let thread = nucleus
            .thread(thread_id)
            .ok_or(CapError::InvalidOperation)?;
        if thread.scheduler != Some(scheduler) || thread_id == dispatcher {
            return Err(CapError::InvalidOperation);
        }
        if thread.generation != expected_generation {
            return Err(CapError::StaleGeneration {
                current: thread.generation,
            });
        }
        if !dispatchable(nucleus, thread_id) {
            return Err(CapError::InvalidOperation);
        }
        // The Time operand is the Thread's own bound Time, or 0 for it.
        if time_key != 0 {
            let time_id = lookup(
                access,
                caller,
                RawKey::from_wire(time_key),
                3,
                ObjectType::TIME,
                Rights::WRITE,
            )
            .and_then(|entry| entry.carved_id().map_err(|e| e.with_key_operand(3)))?;
            if thread.time != Some(time_id) {
                return Err(CapError::InvalidOperation);
            }
        }
    }
    let frequency = nucleus.counter_frequency();
    let now = nucleus.counter_ticks();
    // The limit, clamped to what is left of the grant a child dispatcher
    // runs in: its Threads never outlast the parent's allowance.
    let limit_deadline =
        (limit_ns != u64::MAX).then(|| now.saturating_add(ns_to_ticks_ceil(limit_ns, frequency)));
    let deadline_ticks = match (
        limit_deadline,
        own_grant.and_then(|grant| grant.deadline_ticks),
    ) {
        (Some(limit), Some(end)) => Some(limit.min(end)),
        (limit, end) => limit.or(end),
    };
    let next = target.map(|thread| Dispatched {
        thread,
        scheduler,
        deadline_ticks,
    });
    // Dispatching a child's dispatcher starts the grant its Threads run in.
    let child_domain = target
        .and_then(|thread| nucleus.thread(thread))
        .and_then(|thread| thread.dispatches);
    // Commit: the dispatcher waits for its activation.
    let record = nucleus
        .pending
        .block(dispatcher, PendingKind::SchedulerDispatch, scheduler)?;
    access
        .resolve_object::<Scheduler>(scheduler)
        .map_err(|error| error.for_key(key, 0))?
        .park_dispatcher(record, target.is_none());
    nucleus.dispatch_next = next;
    if let Some(child) = child_domain {
        nucleus.set_child_grant(child, deadline_ticks);
    }
    semi::println!("✅ Scheduler::Dispatch()");
    Ok(InvokeOutcome::Blocked(record))
}

/// `Adopt` `9`: make the fresh Scheduler at `x2` a child of the invoked one.
///
/// The child must be live, not the invoked Scheduler, not the root, without
/// a parent, and have no children, Threads or dispatcher; the invoked
/// Scheduler must sit above the depth limit. Every check precedes the
/// change.
fn adopt(
    access: &Access,
    caller: CallerTable,
    key: RawKey,
    scheduler: CarvedId,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    if args[1..].iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }
    let child_key = RawKey::from_wire(args[0]);
    let child = lookup(
        access,
        caller,
        child_key,
        2,
        ObjectType::SCHEDULER,
        Rights::WRITE,
    )
    .and_then(|entry| entry.carved_id().map_err(|e| e.with_key_operand(2)))?;
    if child == scheduler {
        return Err(CapError::InvalidOperation);
    }
    let depth = {
        let parent = access
            .resolve_object::<Scheduler>(scheduler)
            .map_err(|error| error.for_key(key, 0))?;
        if parent.depth() >= Scheduler::MAX_DEPTH {
            return Err(CapError::InvalidOperation);
        }
        parent.depth() + 1
    };
    {
        let candidate = access
            .resolve_object::<Scheduler>(child)
            .map_err(|error| error.for_key(child_key, 2))?;
        if !candidate.is_fresh() {
            return Err(CapError::InvalidOperation);
        }
    }
    access
        .resolve_object::<Scheduler>(child)
        .map_err(|error| error.for_key(child_key, 2))?
        .set_parent(scheduler, depth);
    access
        .resolve_object::<Scheduler>(scheduler)
        .map_err(|error| error.for_key(key, 0))?
        .child_adopted();
    semi::println!("✅ Scheduler::Adopt(depth {depth})");
    Ok((0, 0))
}

/// `MoveThread` `10`: move the Thread at `x2` to the Scheduler at `x3`, into
/// slot `x4` of its table.
///
/// Invoked on the Thread's owner, the destination is a direct child of the
/// invoked Scheduler (donation); invoked on the owner's parent, the
/// destination is the invoked Scheduler itself (taking the Thread back).
fn move_thread<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    key: RawKey,
    scheduler: CarvedId,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    let [thread_key, destination_key, slot, rest @ ..] = *args;
    if rest.iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }
    let slot = u32::try_from(slot).map_err(|_too_wide| CapError::InvalidOperation)?;
    let thread_id = resolve_thread(access, caller, RawKey::from_wire(thread_key))?;
    let destination_key = RawKey::from_wire(destination_key);
    let destination = lookup(
        access,
        caller,
        destination_key,
        3,
        ObjectType::SCHEDULER,
        Rights::WRITE,
    )
    .and_then(|entry| entry.carved_id().map_err(|e| e.with_key_operand(3)))?;
    let parent_of = |id: CarvedId, key: RawKey, operand: u8| {
        access
            .resolve_object::<Scheduler>(id)
            .map(|object| object.parent())
            .map_err(|error| error.for_key(key, operand))
    };
    let destination_parent = parent_of(destination, destination_key, 3)?;
    let owner = nucleus
        .thread(thread_id)
        .and_then(|thread| thread.scheduler)
        .ok_or(CapError::InvalidOperation)?;
    let owner_parent = parent_of(owner, key, 0)?;
    let donation = owner == scheduler && destination_parent == Some(scheduler);
    let reclaim = destination == scheduler && owner_parent == Some(scheduler);
    if !(donation || reclaim) {
        return Err(CapError::InvalidOperation);
    }
    nucleus.move_thread(thread_id, destination, slot)?;
    semi::println!("✅ Scheduler::MoveThread(slot {slot})");
    Ok((0, 0))
}

/// `StartThread` `2`.
fn start_thread<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    scheduler: CarvedId,
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
    // Only the Scheduler that owns a Thread starts it.
    if thread.scheduler != Some(scheduler) {
        return Err(CapError::InvalidOperation);
    }
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

/// The invoked Scheduler: a live Scheduler capability with `WRITE`. Every
/// operation works on any Scheduler, root or not (maintainer decision,
/// 2026-10-18); a child has no execution authority until it is adopted and
/// its dispatcher is dispatched.
fn resolve_scheduler(
    access: &Access,
    caller: CallerTable,
    key: RawKey,
) -> Result<CarvedId, CapError> {
    let id = lookup(access, caller, key, 0, ObjectType::SCHEDULER, Rights::WRITE)
        .and_then(|entry| entry.carved_id().map_err(|e| e.with_key_operand(0)))?;
    access
        .resolve_object::<Scheduler>(id)
        .map_err(|error| error.for_key(key, 0))?;
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
