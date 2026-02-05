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
    crate::objects::{Nucleus, ObjectPool, arch::ArchPools, domain::DcbPages},
    cfg_if::cfg_if,
    core::{
        arch::asm,
        cell::{LazyCell, UnsafeCell},
        panic::PanicInfo,
        time::Duration,
    },
    libcpu::endless_sleep,
    libexception::arch::aarch64::ExceptionContext,
    liblocking::{IRQSafeNullLock, interface::Mutex},
    liblog::{info, println, warn},
    libmemory::mmu::AccessPermissions,
    libobject::{ArchType, CapError, KeySlot},
    libqemu::{semi_print, semi_println},
};

/// Syscall API - capability invocation handlers
mod api;
/// Nucleus objects implementations
mod objects;
/// Exception vectors triggering syscall handing and general IRQ routing
mod vectors;

// TODO: Split this into read-only part, that does not need locks, per-cpu mutable part that does not need locks,
// TODO: Shared atomic counters that do not need locks and shared mutable collections that DO need locks (but should be minority)
/// Global kernel state, protected by The Great Kernel Lock
static mut NUCLEUS: IRQSafeNullLock<LazyCell<Nucleus<objects::ArchObjectsImpl>>> =
    IRQSafeNullLock::new(LazyCell::new(|| {
        let mut n = Nucleus::<objects::ArchObjectsImpl> {
            current_domain: None,
            dcb_pages: DcbPages::new(),
            pools: objects::nucleus::NucleusPools {
                domains: unsafe { ObjectPool::new(0x1000 as *mut u8, 16384) }, // TODO: proper alloc...
                arch: unsafe { ArchPools::new() },
            },
        };
        n.create_domain();
        n
    }));

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
        "sub    sp,  sp,  #16 * 17",
        "",
        "stp    x0,  x1,  [sp, #16 * 0]",
        "stp    x2,  x3,  [sp, #16 * 1]",
        "stp    x4,  x5,  [sp, #16 * 2]",
        "stp    x6,  x7,  [sp, #16 * 3]",
        "stp    x8,  x9,  [sp, #16 * 4]",
        "stp    x10, x11, [sp, #16 * 5]",
        "stp    x12, x13, [sp, #16 * 6]",
        "stp    x14, x15, [sp, #16 * 7]",
        "stp    x16, x17, [sp, #16 * 8]",
        "stp    x18, x19, [sp, #16 * 9]",
        "stp    x20, x21, [sp, #16 * 10]",
        "stp    x22, x23, [sp, #16 * 11]",
        "stp    x24, x25, [sp, #16 * 12]",
        "stp    x26, x27, [sp, #16 * 13]",
        "stp    x28, x29, [sp, #16 * 14]",
        "",
        "mrs    x10, SPSR_EL1",
        "mrs    x11, ELR_EL1",
        "",
        "stp    x30, x10, [sp, #16 * 15]",
        "str    x11,      [sp, #16 * 16]",
        // x0-x7 already in place for Rust function call
        "mov    x0, sp", // register frame pointer for handler
        "bl cap_invoke_handler",
        "",
        // Return values in x0, x1, x2 are set in the trap frame
        "ldr    x19,      [sp, #16 * 16]",
        "ldp    x30, x20, [sp, #16 * 15]",
        "",
        "msr    ELR_EL1, x19",
        "msr    SPSR_EL1, x20",
        "",
        "ldp    x0,  x1,  [sp, #16 * 0]",
        "ldp    x2,  x3,  [sp, #16 * 1]",
        "ldp    x4,  x5,  [sp, #16 * 2]",
        "ldp    x6,  x7,  [sp, #16 * 3]",
        "ldp    x8,  x9,  [sp, #16 * 4]",
        "ldp    x10, x11, [sp, #16 * 5]",
        "ldp    x12, x13, [sp, #16 * 6]",
        "ldp    x14, x15, [sp, #16 * 7]",
        "ldp    x16, x17, [sp, #16 * 8]",
        "ldp    x18, x19, [sp, #16 * 9]",
        "ldp    x20, x21, [sp, #16 * 10]",
        "ldp    x22, x23, [sp, #16 * 11]",
        "ldp    x24, x25, [sp, #16 * 12]",
        "ldp    x26, x27, [sp, #16 * 13]",
        "ldp    x28, x29, [sp, #16 * 14]",
        "",
        "add    sp,  sp,  #16 * 17",
        "",
        "eret",
    );
}

/// Kernel entry point
#[unsafe(no_mangle)]
fn cap_invoke_handler(
    // cap_slot: u32,
    // op: u32,
    // arg0: u64,
    // arg1: u64,
    // arg2: u64,
    // arg3: u64,
    // arg4: u64,
    // arg5: u64,
    frame: &mut ExceptionContext,
) {
    let cap_slot = frame.gpr[0] as u32;
    let op = frame.gpr[1] as u32;
    semi_println!(
        "CapInvoke SYSCALL(cap: {cap_slot}, op: {op}) happened, we're at PC {:#016X}, SP {:#016X}, exception frame @ {:#016X}",
        get_pc(),
        get_sp(),
        frame as *mut _ as u64,
    );

    // semi_println!("{}", frame);

    let args: &[u64; 6] = &frame.gpr[2..=7].try_into().unwrap();

    let result = unsafe {
        #[allow(static_mut_refs)]
        NUCLEUS.lock(|nucleus| api::handle_cap_invoke(nucleus, cap_slot, op, args))
    };

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

    let (x0, x1, x2) = match result {
        Ok((v0, v1)) => (0, v0, v1),
        Err(e) => e.code(),
    };
    // Return values
    semi_println!("CapInvoke SYSCALL(Return {x0:#x}, {x1:#x}, {x2:#x})",);
    unsafe {
        frame.gpr[0] = x0;
        frame.gpr[1] = x1;
        frame.gpr[2] = x2;
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

fn get_sp() -> u64 {
    use aarch64_cpu::registers::Readable;
    aarch64_cpu::registers::SP.get()
}
