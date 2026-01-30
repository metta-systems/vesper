/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Vesper single-address-space nanokernel.
//!
//! This crate implements the kernel binary proper.

#![no_std]
#![no_main]
#![feature(decl_macro)]
#![feature(allocator_api)]
#![feature(format_args_nl)]
#![feature(stmt_expr_attributes)]
#![feature(slice_ptr_get)]
#![deny(missing_docs)]
#![deny(warnings)]
#![allow(unused)]
#![allow(internal_features)]
#![allow(linker_messages)]
#![feature(ptr_internals)]
#![feature(core_intrinsics)]

use {
    cfg_if::cfg_if,
    core::{arch::asm, cell::UnsafeCell, panic::PanicInfo, time::Duration},
    libcpu::endless_sleep,
    liblocking::IRQSafeNullLock,
    liblog::{info, println, warn},
    libmemory::mmu::AccessPermissions,
    libqemu::semi_println
};use core::cell::LazyCell;
use crate::nucleus::Nucleus;

mod api;
mod objects;
mod nucleus;
mod vectors;

/// Global kernel state, protected by The Great Kernel Lock
static mut NUCLEUS: IRQSafeNullLock<LazyCell<Nucleus<objects::ArchObjectsImpl>>> = IRQSafeNullLock::new(LazyCell::new(|| { Nucleus<objects::ArchObjectsImpl> {} }));


#[panic_handler]
fn panicked(info: &PanicInfo) -> ! {
    libmachine::panic::handler(info)
}

// Syscall handler - exception vector for EL0 synchronous exceptions
//  (the only other thing nucleus does)
#[unsafe(naked)]
#[unsafe(no_mangle)]
unsafe extern "C" fn syscall_handler() {
    core::arch::naked_asm!(
        // Save user context to kernel stack
        "sub sp, sp, #272",
        "stp x0, x1, [sp, #0]",
        "stp x2, x3, [sp, #16]",
        "stp x4, x5, [sp, #32]",
        "stp x6, x7, [sp, #48]",
        "stp x8, x9, [sp, #64]",
        "stp x10, x11, [sp, #80]",
        "stp x12, x13, [sp, #96]",
        "stp x14, x15, [sp, #112]",
        "stp x16, x17, [sp, #128]",
        "stp x18, x19, [sp, #144]",
        "stp x20, x21, [sp, #160]",
        "stp x22, x23, [sp, #176]",
        "stp x24, x25, [sp, #192]",
        "stp x26, x27, [sp, #208]",
        "stp x28, x29, [sp, #224]",
        "str x30, [sp, #240]", // LR
        "mrs x10, elr_el1",
        "mrs x11, spsr_el1",
        "stp x10, x11, [sp, #248]", // ELR, SPSR
        // x0-x7 already in place for Rust function call
        "mov x8, sp", // frame pointer for handler
        "bl cap_invoke_handler",
        // Return values in x0, x1, x2 are already set by handler
        // Restore context (skip x0, x1, x2 - they hold return values)
        "ldr x3, [sp, #24]",
        "ldp x4, x5, [sp, #32]",
        "ldp x6, x7, [sp, #48]",
        "ldp x8, x9, [sp, #64]",
        "ldp x10, x11, [sp, #248]",
        "msr elr_el1, x10",
        "msr spsr_el1, x11",
        "ldp x10, x11, [sp, #80]",
        "ldp x12, x13, [sp, #96]",
        "ldp x14, x15, [sp, #112]",
        "ldp x16, x17, [sp, #128]",
        "ldp x18, x19, [sp, #144]",
        "ldp x20, x21, [sp, #160]",
        "ldp x22, x23, [sp, #176]",
        "ldp x24, x25, [sp, #192]",
        "ldp x26, x27, [sp, #208]",
        "ldp x28, x29, [sp, #224]",
        "ldr x30, [sp, #240]",
        "add sp, sp, #272",
        "eret",
    );
}

/// Kernel entry point
#[unsafe(no_mangle)]
fn cap_invoke_handler(
    cap_slot: u32,
    op: u32,
    arg0: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    frame: u64, //*mut TrapFrame,
) -> (u64, u64, u64) {
    semi_println!(
        "CapInvoke SYSCALL(cap: {cap_slot}, op: {op}) happened, we're at 0x{:016X}",
        get_pc()
    );

    handle_cap_invoke(NUCLEUS.lock(), cap_slot, op, args)

    // let cap = current_domain().keytable.lookup(cap_slot)?;
    // let args = &[arg0, arg1, arg2, arg3, arg4, arg5]; // FIXME temp

    // let result = match cap.cap_type() {
    //     ObjectType::Untyped => api::untyped::invoke(cap, op, args), // retype, split
    //     ObjectType::Domain => api::domain::invoke(cap, op, args),   // activate, suspend...
    //     ObjectType::KeyTable => api::key_table::invoke(cap, op, args),
    //     ObjectType::Time => api::time::invoke(cap, op, args), // donate, split, merge
    //     ObjectType::Endpoint => api::endpoint::invoke(cap, op, args),
    //     ObjectType::Notification => api::notification::invoke(cap, op, args),
    //     ObjectType::EventCount => api::event_count::invoke(cap, op, args),
    //     ObjectType::Buffer => api::buffer::invoke(cap, op, args), // map, unmap, query
    //     ObjectType::None => Err(SyscallError::InvalidSlot),
    // };

    // match result {
    //     Ok((v0, v1)) => (0, v0, v1),
    //     Err(e) => (e.code(), 0, 0),
    // }
}

// ═══════════════════════════════════════════════════════════════════
// SYSCALL DISPATCH WITH ARCH OBJECTS
// ═══════════════════════════════════════════════════════════════════

/// Main capability invocation handler with two-level dispatch.
///
/// First: single bit test to separate arch vs core
/// Then: smaller match within each category
///
/// This is more branch-predictor friendly because:
/// 1. The arch bit test is highly predictable (most calls are core)
/// 2. Each sub-match has fewer cases
#[inline(never)] // Keep as separate function for branch prediction
pub fn handle_cap_invoke<A: ArchObjects>(
    nucleus: &mut Nucleus<A>,
    cap_slot: u32,
    op: u32,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    let domain = nucleus.current_domain_mut()?;
    let slot = KeySlot(cap_slot as u16);
    let entry = domain.keytable.lookup_mut(slot)?;
    let obj_type = entry.object_type();

    if obj_type.is_arch() {
        // Architecture-specific dispatch (less common path)
        arch_invoke::<A>(nucleus, entry, obj_type, op, args)
    } else {
        // Core dispatch (common path)
        core_invoke::<A>(nucleus, entry, obj_type, op, args)
    }
}

/// Core object dispatch
#[inline(always)]
fn core_invoke<A: ArchObjects>(
    nucleus: &mut Nucleus<A>,
    entry: &mut KeyEntry,
    obj_type: ObjectType,
    op: u32,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    let core_type = CoreType::try_from(obj_type)?;

    match core_type {
        CoreType::Null => Err(CapError::NullCapability),

        // CoreType::Untyped => {
        //     let untyped = entry.as_object_mut::<Untyped>()?;
        //     // Untyped::invoke(untyped, ....)
        //     api::untyped::invoke(untyped, entry.rights(), op, args, &mut nucleus.pools)
        // }
        CoreType::DebugConsole => {
            let debug_console = entry.as_object_mut::<DebugConsole>()?;
            DebugConsole::invoke(debug_console, parampampam)
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
    }
}

/// Architecture-specific dispatch - defined per architecture
#[inline(always)]
fn arch_invoke<A: ArchObjects>(
    nucleus: &mut Nucleus<A>,
    entry: &mut KeyEntry,
    obj_type: ObjectType,
    op: u32,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    let arch_type = ArchType::try_from(obj_type)?;

    match arch_type {
        ArchType::Frame => {
            let frame = entry.as_object_mut::<A::Frame>()?;
            A::invoke_frame(frame, entry.rights(), op, args, nucleus) // or A::Frame::invoke()?
        }

        ArchType::PageTable => {
            let pt = entry.as_object_mut::<A::PageTable>()?;
            A::invoke_page_table(pt, entry.rights(), op, args, nucleus)
        }

        ArchType::VSpace => {
            let vspace = entry.as_object_mut::<A::VSpace>()?;
            A::invoke_vspace(vspace, entry.rights(), op, args, nucleus)
        }

        ArchType::ASIDPool => {
            let pool = entry.as_object_mut::<A::ASIDPool>()?;
            A::invoke_asid_pool(pool, entry.rights(), op, args, nucleus)
        }

        ArchType::ASID => {
            let asid = entry.as_object_mut::<A::ASID>()?;
            A::invoke_asid(asid, entry.rights(), op, args)
        }

        ArchType::IOSpace => {
            // May not be supported on all architectures
            A::invoke_io_space(entry, op, args, nucleus)
        }

        ArchType::IOPort => {
            // x86 only
            #[cfg(target_arch = "x86_64")]
            {
                let port = entry.as_object_mut::<x86_64::IOPort>()?;
                x86_64::invoke_io_port(port, entry.rights(), op, args)
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                Err(CapError::UnsupportedArchType(arch_type))
            }
        }

        ArchType::IRQHandler => A::invoke_irq_handler(entry, op, args, nucleus),

        ArchType::IRQControl => A::invoke_irq_control(entry, op, args, nucleus),
    }
}

fn get_pc() -> u64 {
    let pc: u64;
    unsafe {
        asm!(
            "adr {}, .",
            out(reg) pc,
        );
    }
    pc
}
