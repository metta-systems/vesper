use {
    crate::objects::{ArchObjects, KeyTable, Nucleus, access::Access},
    libobject::{ArchType, CapError, CoreType, ObjectType, RawKey},
    libqemu::semihosting as semi,
};

// pub mod arch;
#[cfg(feature = "debug_kernel")]
pub mod debug_console;
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
        "handle_cap_invoke(key {key:?},op {op},args[{:x},{:x},{:x},{:x},{:x},{:x}])",
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

    semi::println!("core_invoke");

    match core_type {
        CoreType::Null => Err(CapError::NullCapability),

        CoreType::Untyped => {
            crate::api::untyped::invoke::<A>(access, caller_table_addr, key, op, args)
        }
        #[cfg(feature = "debug_kernel")]
        CoreType::DebugConsole => {
            semi::println!("core_invoke: DebugConsole");
            let caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
            let entry = caller_table.lookup(key)?;
            crate::api::debug_console::invoke(entry, op, args[0], args[1])
        } // CoreType::Domain => {
        //     let domain = entry.as_object_mut::<Domain>()?;
        //     api::domain::invoke(domain, entry.rights(), op, args)
        // }
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

        // CoreType::Buffer => {
        //     let buf = entry.as_object_mut::<Buffer>()?;
        //     api::buffer::invoke(buf, entry.rights(), op, args)
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

    let caller_table = access.resolve_carved_mut::<KeyTable>(caller_table_addr)?;
    let entry = caller_table.lookup(key)?;

    #[expect(
        clippy::match_single_binding,
        reason = "All other arms are commented out"
    )]
    match arch_type {
        // ArchType::Frame => {
        //     A::invoke_frame(entry, op, args, nucleus)
        // }

        // ArchType::PageTable => {
        //     let pt = entry.as_object_mut::<A::PageTable>()?;
        //     A::invoke_page_table(pt, entry.rights(), op, args, nucleus)
        // }

        // ArchType::VSpace => {
        //     let vspace = entry.as_object_mut::<A::VSpace>()?;
        //     A::invoke_vspace(vspace, entry.rights(), op, args, nucleus)
        // }

        // ArchType::ASIDPool => {
        //     let pool = entry.as_object_mut::<A::ASIDPool>()?;
        //     A::invoke_asid_pool(pool, entry.rights(), op, args, nucleus)
        // }

        // ArchType::ASID => {
        //     let asid = entry.as_object_mut::<A::ASID>()?;
        //     A::invoke_asid(asid, entry.rights(), op, args)
        // }

        // ArchType::IOSpace => {
        //     // May not be supported on all architectures
        //     A::invoke_io_space(entry, op, args, nucleus)
        // }

        // ArchType::IOPort => {
        //     // x86 only
        //     #[cfg(target_arch = "x86_64")]
        //     {
        //         let port = entry.as_object_mut::<x86_64::IOPort>()?;
        //         x86_64::invoke_io_port(port, entry.rights(), op, args)
        //     }
        //     #[cfg(not(target_arch = "x86_64"))]
        //     {
        //         Err(CapError::UnsupportedArchType(arch_type))
        //     }
        // }

        // ArchType::IRQHandler => A::invoke_irq_handler(entry, op, args, nucleus),

        // ArchType::IRQControl => A::invoke_irq_control(entry, op, args, nucleus),
        x => Err(CapError::UnsupportedArchType(x)),
    }
}
