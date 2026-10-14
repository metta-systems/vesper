use {
    crate::objects::{
        ArchObjects, KeyTable, Nucleus,
        access::Access,
        arch_objects::AddressSpaceObject,
        invocation::{CommittedCall, CommittedReturn, ReturnFault, ReturnRejection},
        key_table::CallerTable,
        resume::PreparedTranslationContext,
    },
    libexception::arch::aarch64::SavedContext,
    libobject::{
        ArchType, CapError, CoreType, InconsistencyReason, ObjectType, RawKey, fault::FaultKind,
        thread::ThreadOp,
    },
    libqemu::semihosting as semi,
};

pub mod arch;
#[cfg(feature = "debug_kernel")]
pub mod debug_console;
pub mod event_count;
pub mod invocation;
pub mod irq;
pub mod key_entry;
pub mod key_table;
pub mod notification;
pub mod scheduler;
pub mod thread;
pub mod time;
pub mod untyped;

pub use key_entry::KeyEntry;

// ═════════════════════════════
// INVOCATION OUTCOME
// ═════════════════════════════

/// The outcome of one capability invocation (completion foundation,
/// 2026-09-16).
///
/// A blocking operation does not return until completion or cancellation
/// (selected D7 model): `Blocked` tells the syscall entry that the caller's
/// return happens later, when the pending-invocation record `record` reaches
/// its terminal transition. The entry parks the caller and switches; it must
/// not write a result and return.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvokeOutcome {
    /// The invocation completed (or failed) now; write the result and return.
    Complete((u64, u64)),
    /// The invocation blocked; `record` names its pending-invocation record.
    Blocked(crate::objects::access::ObjectId),
    /// Checked own-AS activation; install at entry after all guards/lock end,
    /// then trace and return zeros. This is an internal outcome, not wire ABI.
    Activate(PreparedTranslationContext),
    /// Committed PPC Call: the Thread migrated and its continuation is
    /// pushed. Entry installs the target translation after all guards/lock
    /// end, restores the scrubbed target-entry frame, then traces success.
    Call(CommittedCall),
    /// Committed `Thread.Return`: the top continuation is popped. Entry
    /// installs the source translation after all guards/lock end, restores
    /// the resumed source frame, then traces success.
    Return(CommittedReturn),
    /// `Thread.Return` hit a protocol fault (nothing popped). Entry delivers
    /// it to the fault handler as a fault at the Return's `svc`.
    Fault(libobject::fault::FaultKind),
}

// ═════════════════════════════
// SYSCALL DISPATCH
// ═════════════════════════════

/// Every input comes from the caller's saved frame, never live registers:
/// x0 key, x1 operation, x2..x7 operands; PPC Call also reads x9 (target SP).
fn decode_invocation(saved: &SavedContext) -> (RawKey, u64, [u64; 6]) {
    let mut operands = [0; 6];
    operands.copy_from_slice(&saved.gpr[2..8]);
    (RawKey::from_wire(saved.gpr[0]), saved.gpr[1], operands)
}

/// Main capability invocation handler with two-level dispatch.
///
/// First: single bit test to separate arch vs core
/// Then: smaller match within each category
///
/// This is more branch-predictor friendly because:
/// 1. The arch bit test is highly predictable (most calls are core)
/// 2. Each sub-match has fewer cases
#[inline]
pub fn handle_cap_invoke<A: ArchObjects>(
    nucleus: &mut Nucleus<A>,
    saved: &SavedContext,
) -> Result<InvokeOutcome, CapError> {
    let (key, op, operands) = decode_invocation(saved);
    let args = &operands;
    // semi::println!(
    //     "🔄 handle_cap_invoke(key {key:?},op {op},args[{:x},{:x},{:x},{:x},{:x},{:x}])",
    //     args[0],
    //     args[1],
    //     args[2],
    //     args[3],
    //     args[4],
    //     args[5]
    // );
    // SAFETY: the caller holds the kernel lock for the whole invocation and
    // constructs no overlapping access context.
    let access = unsafe { Access::new() };
    let caller = caller_table(nucleus, &access, key)?;
    let obj_type = {
        let caller_table = access.resolve_caller_table(caller)?;
        // semi::println!("handle_cap_invoke(got entry)");
        caller_table.lookup(key, caller.guard)?.object_type()
    };

    // semi::println!("handle_cap_invoke(resolved obj_type {})", obj_type.as_u8());

    if core::hint::unlikely(obj_type.is_arch()) {
        // Architecture-specific dispatch (less common path)
        arch_invoke::<A>(nucleus, &access, caller, obj_type, saved)
    } else {
        // Core dispatch (common path)
        core_invoke::<A>(nucleus, &access, caller, obj_type, saved)
    }
}

/// The caller's own table for this invocation: its carved address (from the
/// current Thread's live `AddressSpace` binding) plus its guard, sourced from
/// the `SELF_KEYTABLE`
/// capability and validated to name this very table (guarded key-space
/// package, selected 2026-09-23).
///
/// An invocation whose `SELF_KEYTABLE` entry is missing, is not a `KeyTable`
/// capability, or names a different table cannot establish its key-resolution
/// context and is rejected before any key validation with
/// `InvalidSelfReference::KeyTableKey(SELF_KEYTABLE)` and the invoked key.
fn caller_table<A: ArchObjects>(
    nucleus: &Nucleus<A>,
    access: &Access,
    invoked: RawKey,
) -> Result<CallerTable, CapError> {
    let thread = nucleus.current_thread_ref()?;
    Nucleus::<A>::address_space_table(access, thread.address_space, invoked)
}

/// Core object dispatch
#[inline(always)]
fn core_invoke<A: ArchObjects>(
    nucleus: &mut Nucleus<A>,
    access: &Access,
    caller: CallerTable,
    obj_type: ObjectType,
    saved: &SavedContext,
) -> Result<InvokeOutcome, CapError> {
    let (key, op, operands) = decode_invocation(saved);
    let args = &operands;
    let core_type = CoreType::try_from(obj_type)?;

    // semi::println!("🔄 core_invoke {key:?} / {core_type}:{op}");

    match core_type {
        CoreType::Null => Err(CapError::NullCapability),

        CoreType::Untyped => {
            crate::api::untyped::invoke::<A>(access, caller, key, op, args, nucleus)
                .map(InvokeOutcome::Complete)
        }
        #[cfg(feature = "debug_kernel")]
        CoreType::DebugConsole => {
            semi::println!("core_invoke: DebugConsole");
            let caller_table = access.resolve_caller_table(caller)?;
            let entry = caller_table.lookup(key, caller.guard)?;
            crate::api::debug_console::invoke(entry, op, args[0], args[1])
                .map(InvokeOutcome::Complete)
        }
        // Opcode meaning follows the looked-up kind: op 0 on a Thread entry is
        // Return (a named Thread is rejected there), never wrong-form Call.
        CoreType::Thread if op == ThreadOp::Return as u64 => {
            match crate::api::thread::return_from_call(access, caller, saved, nucleus) {
                Ok(committed) => Ok(InvokeOutcome::Return(committed)),
                Err(ReturnRejection::Error(error)) => Err(error),
                // Protocol faults go to the fault handler; nothing was popped.
                Err(ReturnRejection::Fault(fault)) => Ok(InvokeOutcome::Fault(match fault {
                    ReturnFault::IllegalReturn => FaultKind::IllegalReturn,
                    ReturnFault::ReturnTargetRetired => FaultKind::ReturnTargetRetired,
                })),
            }
        }
        CoreType::Thread => crate::api::thread::invoke(access, caller, key, op, args, nucleus)
            .map(InvokeOutcome::Complete),

        CoreType::Invocation => {
            crate::api::invocation::call(access, caller, saved, nucleus).map(InvokeOutcome::Call)
        }

        CoreType::KeyTable => crate::api::key_table::invoke(access, caller, key, op, args)
            .map(InvokeOutcome::Complete),

        CoreType::Notification => {
            crate::api::notification::invoke::<A>(access, caller, key, op, args, nucleus)
        }

        CoreType::EventCount => {
            crate::api::event_count::invoke::<A>(access, caller, key, op, args, nucleus)
        }

        CoreType::Time => {
            crate::api::time::invoke(access, caller, key, op, args).map(InvokeOutcome::Complete)
        }

        CoreType::Scheduler => {
            crate::api::scheduler::invoke::<A>(access, caller, key, op, args, nucleus)
                .map(InvokeOutcome::Complete)
        }

        CoreType::IRQControl => {
            crate::api::irq::invoke_control::<A>(access, caller, key, op, args, nucleus)
                .map(InvokeOutcome::Complete)
        }

        CoreType::IRQHandler => {
            crate::api::irq::invoke_handler::<A>(access, caller, key, op, args, nucleus)
                .map(InvokeOutcome::Complete)
        }

        _ => Err(CapError::UnsupportedCoreType(core_type)),
    }
}

/// Mark the thread of a completed record runnable.
///
/// The waiter identity is checked against the Thread's header before
/// enqueueing; a stale identity (thread torn down) releases the terminal
/// record instead — its waiter will never resume.
pub(crate) fn wake_waiter<A: ArchObjects>(
    nucleus: &mut Nucleus<A>,
    record: crate::objects::access::ObjectId,
) -> Result<(), CapError> {
    let waiter = nucleus.pending.waiter(record)?;
    if nucleus.thread(waiter).is_some() {
        // A completed record's waiter is parked, so it is not already queued;
        // a refused push is a kernel bookkeeping bug.
        assert!(nucleus.run_queue.push(waiter), "woken Thread did not queue");
    } else if nucleus.pending.release(record).is_err() {
        panic!("failed to release a completed record with a stale waiter");
    }
    Ok(())
}

/// The current thread's checked identity, for wait
/// registration.
pub(crate) fn current_waiter<A: ArchObjects>(
    nucleus: &Nucleus<A>,
) -> Result<crate::objects::thread::ThreadId, CapError> {
    nucleus.current_thread_id()
}

/// Architecture-specific dispatch - defined per architecture
#[inline(always)]
fn arch_invoke<A: ArchObjects>(
    nucleus: &mut Nucleus<A>,
    access: &Access,
    caller: CallerTable,
    obj_type: ObjectType,
    saved: &SavedContext,
) -> Result<InvokeOutcome, CapError> {
    // `x8` is read only by AddressSpace.CreateInvocation (its placement window).
    let (key, op, operands) = decode_invocation(saved);
    let args = &operands;
    let x8 = saved.gpr[8];
    let arch_type = ArchType::try_from(obj_type)?;

    // semi::println!("🔄 arch_invoke {key:?} / {arch_type}:{op}");

    match arch_type {
        ArchType::Frame => {
            crate::api::arch::frame::invoke::<A>(access, caller, key, op, args, nucleus)
                .map(InvokeOutcome::Complete)
        }

        ArchType::PageTable => {
            crate::api::arch::page_table::invoke::<A>(access, caller, key, op, args)
                .map(InvokeOutcome::Complete)
        }

        ArchType::AddressSpace => {
            crate::api::arch::address_space::invoke::<A>(access, caller, key, op, args, x8, nucleus)
        }

        ArchType::ASIDPool => {
            crate::api::arch::asid_pool::invoke::<A>(access, caller, key, op, args, nucleus)
                .map(InvokeOutcome::Complete)
        }

        // ASIDControl and I/O control remain deferred with their kinds:
        // no creatable arch kind other than Frame, PageTable, and the
        // boot-provided AddressSpace/ASIDPool is allowlisted or provided,
        // and their draft handlers stay inactive. The registered ASIDControl
        // kind stays reserved (ASIDs bind through ASIDPool.Assign).
        x => Err(CapError::UnsupportedArchType(x)),
    }
}
