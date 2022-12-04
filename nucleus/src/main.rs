/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Vesper single-address-space nanokernel.
//!
//! This crate implements the kernel binary proper.

#![no_std]
#![no_main]
#![feature(ptr_internals)]
#![feature(format_args_nl)]
#![feature(strict_provenance)]
#![feature(custom_test_frameworks)]
#![test_runner(machine::tests::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![allow(missing_docs)]
#![deny(warnings)]
#![deny(unused)]
#![feature(allocator_api)]

use armv8a_panic_semihosting as _;

use machine::{entry, DMA_ALLOCATOR};

entry!(kernel_main);

/// Kernel entry point.
/// `arch` crate is responsible for calling it.
// #[inline]
pub fn kernel_main() -> ! {
    if armv8a_semihosting::hprintln!("Lets go!").is_err() {
        // opening semihosting stdout fails!
        armv8a_semihosting::debug::exit(armv8a_semihosting::debug::EXIT_FAILURE);
    }

    use core::alloc::{Allocator, Layout};

    // extrcat allocator to machine crate now
    DMA_ALLOCATOR
        .lock(|a| unsafe { a.allocate(Layout::from_size_align(1024, 16).unwrap()) })
        .unwrap();

    panic!("Off you go!");
}
