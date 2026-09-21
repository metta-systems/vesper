use {
    crate::objects::{ArchObjects, KeyTable, Nucleus, access::Access},
    libobject::{ArchType, CapError, CoreType, ObjectType, RawKey},
    libqemu::semihosting as semi,
};

pub mod arch;
#[cfg(feature = "debug_kernel")]
pub mod debug_console;
pub mod event_count;
pub mod key_entry;
pub mod key_table;
pub mod notification;
pub mod thread;
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
}

// ═════════════════════════════
// SYSCALL DISPATCH
// ═════════════════════════════

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
    key: RawKey,
    op: u64,
    args: &[u64; 6],
) -> Result<InvokeOutcome, CapError> {
    semi::println!(
        "🔄 handle_cap_invoke(key {key:?},op {op},args[{:x},{:x},{:x},{:x},{:x},{:x}])",
        args[0],
        args[1],
        args[2],
        args[3],
        args[4],
        args[5]
    );
    // SAFETY: the caller holds the kernel lock for the whole invocation and
    // constructs no overlapping access context.
    let access = unsafe { Access::new() };
    let caller_table_addr = caller_table_addr(nucleus)?;
    let obj_type = {
        let caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
        semi::println!("handle_cap_invoke(got entry)");
        caller_table.lookup(key)?.object_type()
    };

    semi::println!("handle_cap_invoke(resolved obj_type {})", obj_type.as_u8());

    if core::hint::unlikely(obj_type.is_arch()) {
        // Architecture-specific dispatch (less common path)
        arch_invoke::<A>(nucleus, &access, caller_table_addr, key, obj_type, op, args)
            .map(InvokeOutcome::Complete)
    } else {
        // Core dispatch (common path)
        core_invoke::<A>(nucleus, &access, caller_table_addr, key, obj_type, op, args)
    }
}

/// Address of the current thread's capability table (a carved `KeyTable`).
///
/// Resolved as an owned value (not a borrowed reference) so the caller can
/// also borrow the thread pool; the thread and `KeyTable` storage are disjoint.
fn caller_table_addr<A: ArchObjects>(nucleus: &Nucleus<A>) -> Result<u64, CapError> {
    let index = nucleus.current_thread.ok_or(CapError::InvalidDomain)?;
    nucleus
        .pools
        .threads
        .get_live(usize::try_from(index).ok().ok_or(CapError::InvalidDomain)?)
        .ok_or(CapError::InvalidDomain)
        .map(|thread| thread.keytable_addr)
}

/// Core object dispatch
#[inline(always)]
fn core_invoke<A: ArchObjects>(
    nucleus: &mut Nucleus<A>,
    access: &Access,
    caller_table_addr: u64,
    key: RawKey,
    obj_type: ObjectType,
    op: u64,
    args: &[u64; 6],
) -> Result<InvokeOutcome, CapError> {
    let core_type = CoreType::try_from(obj_type)?;

    semi::println!("🔄 core_invoke {key:?} / {core_type}:{op}");

    match core_type {
        CoreType::Null => Err(CapError::NullCapability),

        CoreType::Untyped => {
            crate::api::untyped::invoke::<A>(access, caller_table_addr, key, op, args, nucleus)
                .map(InvokeOutcome::Complete)
        }
        #[cfg(feature = "debug_kernel")]
        CoreType::DebugConsole => {
            semi::println!("core_invoke: DebugConsole");
            let caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
            let entry = caller_table.lookup(key)?;
            crate::api::debug_console::invoke(entry, op, args[0], args[1])
                .map(InvokeOutcome::Complete)
        }
        CoreType::Thread => {
            crate::api::thread::invoke(access, caller_table_addr, key, op, args, nucleus)
                .map(InvokeOutcome::Complete)
        }

        CoreType::KeyTable => {
            crate::api::key_table::invoke(access, caller_table_addr, key, op, args)
                .map(InvokeOutcome::Complete)
        }

        CoreType::Notification => {
            crate::api::notification::invoke::<A>(access, caller_table_addr, key, op, args, nucleus)
        }

        CoreType::EventCount => {
            crate::api::event_count::invoke::<A>(access, caller_table_addr, key, op, args, nucleus)
        }

        // CoreType::Endpoint => {
        //     let ep = entry.as_object_mut::<Endpoint>()?;
        //     api::endpoint::invoke(ep, entry.rights(), entry.badge(), op, args, nucleus)
        // }

        // CoreType::Time => {
        //     let time = entry.as_object_mut::<TimeSlice>()?;
        //     api::time::invoke(time, entry.rights(), op, args, nucleus)
        // }

        // CoreType::Reply => {
        //     let reply = entry.as_object_mut::<Reply>()?;
        //     api::reply::invoke(reply, op, args, nucleus)
        // }
        _ => Err(CapError::UnsupportedCoreType(core_type)),
    }
}

/// Mark the thread of a completed record runnable.
///
/// The waiter identity is incarnation-checked against the threads pool
/// before enqueueing; a stale identity (thread torn down) releases the
/// terminal record instead — its waiter will never resume.
pub(crate) fn wake_waiter<A: ArchObjects>(
    nucleus: &mut Nucleus<A>,
    record: crate::objects::access::ObjectId,
) -> Result<(), CapError> {
    let waiter = nucleus.pending.waiter(record)?;
    if nucleus.pools.threads.validate(waiter).is_ok() {
        // The queue is sized to hold every thread-pool slot; a full queue is
        // a kernel bookkeeping bug, not an expected condition.
        assert!(
            nucleus.scheduler.push(waiter.index),
            "runnable queue overflow"
        );
    } else if nucleus.pending.release(record).is_err() {
        panic!("failed to release a completed record with a stale waiter");
    }
    Ok(())
}

/// The current thread's incarnation-checked identity, for wait
/// registration.
///
/// The current-thread tracking is index-only today; the generation is read
/// from the authoritative threads-pool metadata so a stale identity can
/// never be registered. Coherent current-thread identity carrying its own
/// generation remains Phase 4 work.
pub(crate) fn current_waiter<A: ArchObjects>(
    nucleus: &Nucleus<A>,
) -> Result<crate::objects::access::ObjectId, CapError> {
    let index = nucleus.current_thread.ok_or(CapError::InvalidDomain)?;
    let index = usize::try_from(index).ok().ok_or(CapError::InvalidDomain)?;
    let generation = nucleus
        .pools
        .threads
        .generation_of(index)
        .ok_or(CapError::InvalidDomain)?;
    Ok(crate::objects::access::ObjectId {
        pool: crate::objects::access::PoolTag::Thread,
        index: u16::try_from(index).map_err(|_too_wide| CapError::InvalidDomain)?,
        generation,
    })
}

/// Architecture-specific dispatch - defined per architecture
#[inline(always)]
fn arch_invoke<A: ArchObjects>(
    nucleus: &mut Nucleus<A>,
    access: &Access,
    caller_table_addr: u64,
    key: RawKey,
    obj_type: ObjectType,
    op: u64,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    let arch_type = ArchType::try_from(obj_type)?;

    semi::println!("🔄 arch_invoke {key:?} / {arch_type}:{op}");

    match arch_type {
        ArchType::Frame => {
            crate::api::arch::frame::invoke::<A>(access, caller_table_addr, key, op, args, nucleus)
        }

        ArchType::PageTable => crate::api::arch::page_table::invoke::<A>(
            access,
            caller_table_addr,
            key,
            op,
            args,
            nucleus,
        ),

        ArchType::AddressSpace => crate::api::arch::address_space::invoke::<A>(
            access,
            caller_table_addr,
            key,
            op,
            args,
            nucleus,
        ),

        ArchType::ASIDPool => crate::api::arch::asid_pool::invoke::<A>(
            access,
            caller_table_addr,
            key,
            op,
            args,
            nucleus,
        ),

        // ASIDControl and I/O/IRQ control remain deferred with their kinds:
        // no creatable arch kind other than Frame, PageTable, and the
        // boot-provided AddressSpace/ASIDPool is allowlisted or provided,
        // and their draft handlers stay inactive. The registered ASIDControl
        // kind stays reserved (ASIDs bind through ASIDPool.Assign).
        x => Err(CapError::UnsupportedArchType(x)),
    }
}
