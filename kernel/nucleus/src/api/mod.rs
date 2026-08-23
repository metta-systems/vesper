use {
    crate::objects::{ArchObjects, DebugConsole, Nucleus},
    libobject::{ArchType, CapError, CoreType, KeySlot, ObjectType},
    libqemu::semihosting as semi,
};

// pub mod arch;
pub mod debug_console;
pub mod key_entry;
// pub mod key_table;

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
    cap_slot: u32,
    op: u32,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    let slot = KeySlot(cap_slot);
    semi::println!(
        "handle_cap_invoke(slot {cap_slot}:op {op}:args[{},{},{},{},{},{}])",
        args[0],
        args[1],
        args[2],
        args[3],
        args[4],
        args[5]
    );
    let obj_type = {
        let domain = nucleus
            .current_domain_mut()
            .ok_or(CapError::InvalidDomain)?;
        semi::println!("handle_cap_invoke(got domain)");
        let entry = domain.keytable.lookup_mut(slot)?;
        semi::println!("handle_cap_invoke(got entry)");
        entry.object_type()
    };

    semi::println!("handle_cap_invoke(resolved obj_type {})", obj_type.as_u8());

    if obj_type.is_arch() {
        // Architecture-specific dispatch (less common path)
        arch_invoke::<A>(nucleus, slot, obj_type, op, args)
    } else {
        // Core dispatch (common path)
        core_invoke::<A>(nucleus, slot, obj_type, op, args)
    }
}

/// Core object dispatch
#[inline(always)]
fn core_invoke<A: ArchObjects>(
    nucleus: &mut Nucleus<A>,
    entry_slot: KeySlot,
    obj_type: ObjectType,
    op: u32,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    let core_type = CoreType::try_from(obj_type)?;

    let domain = nucleus
        .current_domain_mut()
        .ok_or(CapError::InvalidDomain)?;
    let entry = domain.keytable.lookup_mut(entry_slot)?;

    semi::println!("core_invoke");

    match core_type {
        CoreType::Null => Err(CapError::NullCapability),

        // CoreType::Untyped => {
        //     let untyped = entry.as_object_mut::<Untyped>()?;
        //     // Untyped::invoke(untyped, ....)
        //     api::untyped::invoke(untyped, entry.rights(), op, args, &mut nucleus.pools)
        // }
        CoreType::DebugConsole => {
            semi::println!("core_invoke: DebugConsole");
            let debug_console = entry.as_object_mut::<DebugConsole>()?;
            // DebugConsole::invoke(debug_console, entry.rights(), op, args, nucleus)
            crate::api::debug_console::invoke(entry, op, args[0], args[1])
        } // CoreType::Domain => {
        //     let domain = entry.as_object_mut::<Domain>()?;
        //     api::domain::invoke(domain, entry.rights(), op, args)
        // }

        // CoreType::KeyTable => {
        //     let kt = entry.as_object_mut::<KeyTable>()?;
        //     api::keytable::invoke(kt, entry.rights(), op, args)
        // }

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
    entry_slot: KeySlot,
    obj_type: ObjectType,
    op: u32,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    let arch_type = ArchType::try_from(obj_type)?;

    let domain = nucleus
        .current_domain_mut()
        .ok_or(CapError::InvalidDomain)?;
    let entry = domain.keytable.lookup_mut(entry_slot)?;

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
