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
#![allow(unused)]
#![feature(allocator_api)]

use armv8a_panic_semihosting as _;

machine::entry!(kernel_main);

/// Kernel entry point.
/// `arch` crate is responsible for calling it.
// #[inline]
pub fn kernel_main() -> ! {
    armv8a_semihosting::hprintln!("Letsgo!").ok();
    // armv8a_semihosting::hprintln!("Lets {}!", "go").ok();
    // let r = armv8a_semihosting::hprintln!("Lets {}!", "go");
    // armv8a_semihosting::hprintln!("{:?}", r).ok();

    use {
        core::alloc::{Allocator, Layout},
        machine::DMA_ALLOCATOR,
    };

    DMA_ALLOCATOR
        .lock(|a| a.allocate(Layout::from_size_align(1024, 16).unwrap()))
        .unwrap();

    armv8a_semihosting::hprintln!("Lets go 2!").ok();
    panic!("Off you go!");
}
