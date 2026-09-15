use {
    crate::objects::{ArchObjects, KeyTable, Nucleus, access::Access},
    libobject::{ArchType, CapError, CoreType, ObjectType, RawKey},
    libqemu::semihosting as semi,
};

pub mod arch;
#[cfg(feature = "debug_kernel")]
pub mod debug_console;
pub mod domain;
pub mod key_entry;
pub mod key_table;
pub mod untyped;

pub use key_entry::KeyEntry;

// ═════════════════
// SYSCALL DISPATCH
// ═════════════════

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
) -> Result<(u64, u64), CapError> {
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
    } else {
        // Core dispatch (common path)
        core_invoke::<A>(nucleus, &access, caller_table_addr, key, obj_type, op, args)
    }
}

/// Address of the current domain's capability table (a carved `KeyTable`).
///
/// Resolved as an owned value (not a borrowed reference) so the caller can
/// also borrow the domain pool; the domain and `KeyTable` storage are disjoint.
fn caller_table_addr<A: ArchObjects>(nucleus: &Nucleus<A>) -> Result<u64, CapError> {
    let index = nucleus.current_domain.ok_or(CapError::InvalidDomain)?;
    nucleus
        .pools
        .domains
        .get_live(usize::try_from(index).ok().ok_or(CapError::InvalidDomain)?)
        .ok_or(CapError::InvalidDomain)
        .map(|domain| domain.keytable_addr)
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
) -> Result<(u64, u64), CapError> {
    let core_type = CoreType::try_from(obj_type)?;

    semi::println!("🔄 core_invoke {key:?} / {core_type}:{op}");

    match core_type {
        CoreType::Null => Err(CapError::NullCapability),

        CoreType::Untyped => {
            crate::api::untyped::invoke::<A>(access, caller_table_addr, key, op, args, nucleus)
        }
        #[cfg(feature = "debug_kernel")]
        CoreType::DebugConsole => {
            semi::println!("core_invoke: DebugConsole");
            let caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
            let entry = caller_table.lookup(key)?;
            crate::api::debug_console::invoke(entry, op, args[0], args[1])
        }
        CoreType::Domain => {
            crate::api::domain::invoke(access, caller_table_addr, key, op, args, nucleus)
        }

        CoreType::KeyTable => {
            crate::api::key_table::invoke(access, caller_table_addr, key, op, args)
        }

        // CoreType::Notification => {
        //     let notify = entry.as_object_mut::<Notification>()?;
        //     api::notification::invoke(notify, entry.rights(), entry.badge(), op, args)
        // }

        // CoreType::EventCount => {
        //     let ec = entry.as_object_mut::<EventCount>()?;
        //     api::event_count::invoke(ec, entry.rights(), op, args)
        // }

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

        ArchType::ASIDPool => crate::api::arch::asid_pool::invoke::<A>(
            access,
            caller_table_addr,
            key,
            op,
            args,
            nucleus,
        ),

        // VSpace translation and I/O/IRQ control remain deferred with their
        // kinds: no creatable arch kind other than Frame and PageTable is
        // allowlisted, and their draft handlers stay inactive. The registered
        // ASID kind stays reserved (ASIDs bind through ASIDPool.Assign).
        x => Err(CapError::UnsupportedArchType(x)),
    }
}
