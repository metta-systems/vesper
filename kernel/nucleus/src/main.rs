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

mod vectors;

#[panic_handler]
fn panicked(info: &PanicInfo) -> ! {
    libmachine::panic::handler(info)
}
/// Syscall entry point (the only other thing nucleus does)
#[unsafe(no_mangle)]
pub extern "C" fn syscall_handler() -> ! {
    semi_println!("SYSCALL happened, we're at 0x{:016X}", get_pc());
    endless_sleep()
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
