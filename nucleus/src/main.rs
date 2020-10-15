/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Vesper single-address-space exokernel.
//!
//! This crate implements the kernel binary proper.

#![no_std]
#![no_main]
#![feature(asm)]
#![feature(global_asm)]
#![feature(ptr_internals)]
#![feature(format_args_nl)]
#![feature(custom_test_frameworks)]
#![test_runner(crate::tests::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![deny(missing_docs)]
#![deny(warnings)]

#[cfg(not(target_arch = "aarch64"))]
use architecture_not_supported_sorry;

extern crate rlibc; // To enable linking memory intrinsics.

/// Architecture-specific code.
#[macro_use]
pub mod arch;
pub use arch::*;
mod macros;
#[cfg(feature = "qemu")]
mod qemu;
#[cfg(test)]
mod tests;
mod write_to;

entry!(kmain);

fn print_mmu_state_and_features() {
    memory::mmu::print_features();
}

fn init_mmu() {
    print_mmu_state_and_features();
    unsafe {
        memory::mmu::init().unwrap();
    }
    println!("MMU initialised");
}

fn init_exception_traps() {
    extern "C" {
        static __exception_vectors_start: u64;
    }

    unsafe {
        let exception_vectors_start: u64 = &__exception_vectors_start as *const _ as u64;

        arch::traps::set_vbar_el1_checked(exception_vectors_start)
            .expect("Vector table properly aligned!");
    }
    println!("Exception traps set up");
}

/// Kernel entry point.
/// `arch` crate is responsible for calling it.
#[inline]
pub fn kmain() -> ! {
    init_exception_traps();
    init_mmu();

    #[cfg(test)]
    test_main();

    endless_sleep()
}

#[panic_handler]
fn panicked(_info: &core::panic::PanicInfo) -> ! {
    endless_sleep()
}

#[cfg(test)]
mod main_tests {
    use super::*;

    #[test_case]
    fn check_data_abort_trap() {
        // Cause an exception by accessing a virtual address for which no
        // address translations have been set up.
        //
        // This line of code accesses the address 3 GiB, but page tables are
        // only set up for the range [0..1) GiB.
        let big_addr: u64 = 3 * 1024 * 1024 * 1024;
        unsafe { core::ptr::read_volatile(big_addr as *mut u64) };

        println!("[i] Whoa! We recovered from an exception.");
    }
}
