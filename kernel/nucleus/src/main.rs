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

use core::panic::PanicInfo;
#[allow(unused_imports)]
use libconsole::{SerialOps, console::console};
use {
    cfg_if::cfg_if,
    core::{cell::UnsafeCell, time::Duration},
    liblog::{info, println, warn},
    // machine::{arch, entry, memory},
    // exception,
    // , time
};

// kernel/src/main.rs - Kernel entry points are exception handlers in mod vectors

mod vectors;

#[panic_handler]
fn panicked(info: &PanicInfo) -> ! {
    libmachine::panic::handler(info)
}
