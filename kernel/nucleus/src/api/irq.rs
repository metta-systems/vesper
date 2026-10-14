//! `IRQControl` and `IRQHandler` operations.
//!
//! Wire schema (see `doc/capabilities-contract.md`, IRQ delivery):
//! - `IRQControl.Issue` `0`: `x2` destination-table key (`INSTALL`), `x3`
//!   destination slot, `x4` specifier cell count (1–3) in bits 7:0 and the
//!   controller index in bits 15:8 (higher bits zero), `x5..x7` the cells
//!   (unused ones zero). Needs `GRANT` on the `IRQControl`. The interrupt
//!   controller component decodes the specifier into a line and its
//!   trigger; the line is configured, kept masked, and an `IRQHandler` for
//!   it is installed. Returns the handler key in `x1` and the line in `x2`.
//!   A line with a live handler, and the nucleus timer line, fail with
//!   `IrqLineActive`; an undecodable specifier with `InvalidOperation`.
//! - `IRQHandler.SetNotification` `0`: `x2` Notification key (`SEND`,
//!   nonzero badge), `x3` receiver Thread key (`WRITE`), which must own a
//!   Time; `x4..x7` zero. Replaces any earlier binding.
//! - `IRQHandler.Clear` `1`: removes the binding.
//! - `IRQHandler.Ack` `2`: unmasks the line.
//! - `IRQHandler.Retire` `3` (`RETIRE`): clears the binding, masks the line
//!   and frees it; every copy of the handler key fails afterwards.
//!
//! The first three handler operations need `WRITE`. Every check precedes the
//! commit, so a failure changes nothing.

use {
    crate::{
        api::{KeyEntry, key_table::resolve_table_cap},
        objects::{
            ArchObjects, KeyTable, Notification, Nucleus, Thread, Time,
            access::Access,
            carved::Carved,
            irq::{IrqBinding, IrqHandlerId},
            key_table::CallerTable,
        },
        platform,
    },
    libobject::{
        CapError, KeySlot, ObjectType, RawKey, Rights,
        irq::{IrqControlOp, IrqHandlerOp, MAX_ISSUE_CELLS},
    },
    libqemu::semihosting as semi,
};

/// Handle an `IRQControl` capability invocation.
pub fn invoke_control<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    key: RawKey,
    op: u64,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    match IrqControlOp::try_from(op)? {
        IrqControlOp::Issue => issue(access, caller, key, args, nucleus),
    }
}

/// `IRQControl.Issue` `0`.
fn issue<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    key: RawKey,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    let [dst_table_key, dst_slot, shape, cell_words @ ..] = *args;
    let dst_table_key = RawKey::from_wire(dst_table_key);
    lookup(
        access,
        caller,
        key,
        0,
        ObjectType::IRQ_CONTROL,
        Rights::GRANT,
    )?;

    // Decode the specifier shape: cell count in bits 7:0, controller in 15:8.
    if shape >> 16 != 0 {
        return Err(CapError::InvalidOperation);
    }
    let count = usize::try_from(shape & 0xff).map_err(|_too_wide| CapError::InvalidOperation)?;
    let controller =
        usize::try_from((shape >> 8) & 0xff).map_err(|_too_wide| CapError::InvalidOperation)?;
    if count == 0 || count > MAX_ISSUE_CELLS || cell_words[count..].iter().any(|&word| word != 0) {
        return Err(CapError::InvalidOperation);
    }
    let mut cells = [0_u32; MAX_ISSUE_CELLS];
    for (cell, &word) in cells.iter_mut().zip(&cell_words[..count]) {
        *cell = u32::try_from(word).map_err(|_too_wide| CapError::InvalidOperation)?;
    }
    let dst_slot =
        KeySlot(u32::try_from(dst_slot).map_err(|_too_wide| CapError::InvalidOperation)?);

    let dst_cap = {
        let caller_table = access.resolve_caller_table(caller)?;
        resolve_table_cap(&caller_table, dst_table_key, caller.guard, 2)?
    };
    if !dst_cap.rights.has(Rights::INSTALL) {
        return Err(CapError::InsufficientRights);
    }

    let (line, trigger) =
        platform::xlate(controller, &cells[..count]).ok_or(CapError::InvalidOperation)?;
    // The nucleus keeps its timer line; it reads as any other line in use.
    if platform::physical_timer_line() == Some(line) {
        return Err(CapError::IrqLineActive { line });
    }
    nucleus.irq_table.check_issue(line)?;
    let mut dst_table = dst_cap.resolve(access)?;
    dst_table
        .check_insert(dst_slot)
        .map_err(|error| error.with_key_operand(3))?;
    if !platform::set_trigger(line, trigger) {
        return Err(CapError::InvalidOperation);
    }

    // Commit: the line stays masked until the handler's first Ack.
    platform::mask(line);
    let id = nucleus.irq_table.issue(line)?;
    let handler = match dst_table.insert(
        dst_slot,
        KeyEntry::new_irq_handler(id, Rights::all()),
        dst_cap.guard,
    ) {
        Ok(handler) => handler,
        Err(failure) => {
            // `check_insert` passed, so this is defensive: undo the issue.
            let _undone = nucleus.irq_table.retire(id);
            return Err(failure.error.with_key_operand(3));
        }
    };
    semi::println!("✅ IRQControl::Issue(line {line})");
    Ok((handler.to_wire(), u64::from(line)))
}

/// Handle an `IRQHandler` capability invocation.
pub fn invoke_handler<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    key: RawKey,
    op: u64,
    args: &[u64; 6],
    nucleus: &mut Nucleus<A>,
) -> Result<(u64, u64), CapError> {
    let op = IrqHandlerOp::try_from(op)?;
    let required = match op {
        IrqHandlerOp::Retire => Rights::RETIRE,
        IrqHandlerOp::SetNotification | IrqHandlerOp::Clear | IrqHandlerOp::Ack => Rights::WRITE,
    };
    let id = lookup(access, caller, key, 0, ObjectType::IRQ_HANDLER, required)?
        .irq_handler_id()
        .map_err(|error| error.with_key_operand(0))?;
    let operands = match op {
        IrqHandlerOp::SetNotification => 2,
        IrqHandlerOp::Clear | IrqHandlerOp::Ack | IrqHandlerOp::Retire => 0,
    };
    if args[operands..].iter().any(|&arg| arg != 0) {
        return Err(CapError::InvalidOperation);
    }
    nucleus
        .irq_table
        .validate(id)
        .map_err(|error| error.for_key(key, 0))?;

    match op {
        IrqHandlerOp::SetNotification => {
            let binding = binding(access, caller, args, nucleus)?;
            nucleus
                .irq_table
                .bind(id, binding)
                .map_err(|error| error.for_key(key, 0))?;
            semi::println!("✅ IRQHandler::SetNotification(line {})", id.line);
        }
        IrqHandlerOp::Clear => {
            nucleus
                .irq_table
                .clear(id)
                .map_err(|error| error.for_key(key, 0))?;
            semi::println!("✅ IRQHandler::Clear(line {})", id.line);
        }
        IrqHandlerOp::Ack => {
            platform::unmask(id.line);
            semi::println!("✅ IRQHandler::Ack(line {})", id.line);
        }
        IrqHandlerOp::Retire => {
            nucleus
                .irq_table
                .retire(id)
                .map_err(|error| error.for_key(key, 0))?;
            platform::mask(id.line);
            semi::println!("✅ IRQHandler::Retire(line {})", id.line);
        }
    }
    Ok((0, 0))
}

/// The binding `SetNotification` asks for: the Notification at `x2`, whose
/// capability's nonzero badge is the signalled bits, and the receiver Thread
/// at `x3`, which must own a live Time.
fn binding<A: ArchObjects>(
    access: &Access,
    caller: CallerTable,
    args: &[u64; 6],
    nucleus: &Nucleus<A>,
) -> Result<IrqBinding, CapError> {
    let (notification_key, receiver_key) = (RawKey::from_wire(args[0]), RawKey::from_wire(args[1]));
    let entry = lookup(
        access,
        caller,
        notification_key,
        2,
        ObjectType::NOTIFICATION,
        Rights::SEND,
    )?;
    if entry.badge() == 0 {
        return Err(CapError::InvalidOperation);
    }
    let notification = entry
        .carved_id()
        .map_err(|error| error.with_key_operand(2))?;
    access
        .resolve_object::<Notification>(notification)
        .map_err(|error| error.for_key(notification_key, 2))?;

    let receiver = lookup(
        access,
        caller,
        receiver_key,
        3,
        ObjectType::THREAD,
        Rights::WRITE,
    )?
    .thread_id()
    .map_err(|error| error.with_key_operand(3))?;
    // SAFETY: Thread identities name carved records that are never freed.
    unsafe { Carved::<Thread>::validate(receiver) }
        .map_err(|error| error.for_key(receiver_key, 3))?;
    let time = nucleus
        .thread(receiver)
        .and_then(|thread| thread.time)
        .ok_or(CapError::InvalidOperation)?;
    access
        .resolve_object::<Time>(time)
        .map_err(|_retired_time| CapError::InvalidOperation)?;

    Ok(IrqBinding {
        notification,
        badge: u64::from(entry.badge()),
        receiver,
    })
}

/// Deliver a claimed `line`: if it is bound to a live Notification, signal
/// it with the bound badge, waking its front waiter. The caller masks the
/// line and ends the interrupt either way. Whether a Notification was
/// signalled.
pub fn deliver<A: ArchObjects>(access: &Access, line: u32, nucleus: &mut Nucleus<A>) -> bool {
    let Some(binding) = nucleus.irq_table.binding(line) else {
        return false;
    };
    let Ok(mut notification) = access.resolve_object::<Notification>(binding.notification) else {
        // A retired Notification leaves the line effectively unbound.
        return false;
    };
    match notification.signal(binding.badge, &mut nucleus.pending) {
        Ok(Some(record)) => {
            crate::api::wake_waiter(nucleus, record)
                .unwrap_or_else(|error| panic!("IRQ wakeup failed: {:?}", error.code()));
        }
        Ok(None) => {}
        Err(error) => panic!("IRQ signal failed: {:?}", error.code()),
    }
    true
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
