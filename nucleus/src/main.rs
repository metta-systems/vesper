/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */
#![no_std]
#![no_main]
#![feature(asm)]
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

// Kernel entry point
// arch crate is responsible for calling this
#[inline]
pub fn kmain() -> ! {
    init_mmu();

    #[cfg(test)]
    test_main();

    endless_sleep()
}

#[panic_handler]
fn panicked(_info: &core::panic::PanicInfo) -> ! {
    endless_sleep()
}
