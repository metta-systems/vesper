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
    liblog::{info, println, warn},
    libqemu::semi_println,
};

// mod api;
mod vectors;

#[panic_handler]
fn panicked(info: &PanicInfo) -> ! {
    libmachine::panic::handler(info)
}

// Kernel API surface:
//
// Wait options (open wait, like servers waiting; closed wait, like waiting one client's response specifically)
// Timeouts for send and recv phases separately

// -----
// -----
// -----
// -----
// -----
// -----
// -----
// -----
// -----
// -----
// -----
// -----
// -----
// -----
// -----

// use crate::{
//     buffer::{BufferInfo, BufferOp},
//     endpoint::EndpointOp,
// };

struct Nucleus;

/// Capability types now include:
pub enum ObjectType {
    /// No capability
    None = 0,
    /// Creates objects (including new key tables)
    Untyped = 1,
    /// Protection domain
    Domain = 2,
    /// capability table itself
    KeyTable = 3,
    /// Time capability
    Time = 4,
    /// Endpoint capability
    Endpoint = 5,
    /// Notification endpoint capability
    Notification = 6,
    /// Event count endpoint capability
    EventCount = 7,
    /// Shareable buffer capability
    Buffer = 8,
}

type SyscallResult = Result<(u64, u64), SyscallError>;

enum SyscallError {
    PermissionDenied,
    InvalidOp,
    SlotOccupied,
    AlreadyMapped,
    NotMapped,
    InvalidPointer,
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
    semi_println!("CapInvoke SYSCALL happened, we're at 0x{:016X}", get_pc());
    return (0, 0, 0);

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
