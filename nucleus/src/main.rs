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
#![feature(decl_macro)]
#![feature(allocator_api)]
#![feature(ptr_internals)]
#![feature(format_args_nl)]
#![feature(nonnull_slice_from_raw_parts)]
#![feature(custom_test_frameworks)]
#![test_runner(crate::tests::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![deny(missing_docs)]
#![deny(warnings)]

#[cfg(not(target_arch = "aarch64"))]
use architecture_not_supported_sorry;

/// Architecture-specific code.
#[macro_use]
pub mod arch;
pub use arch::*;
mod devices;
mod macros;
mod mm;
mod panic;
mod platform;
#[cfg(feature = "qemu")]
mod qemu;
mod sync;
#[cfg(test)]
mod tests;
mod write_to;

entry!(kmain);

/// The global console. Output of the kernel print! and println! macros goes here.
static CONSOLE: sync::NullLock<devices::Console> = sync::NullLock::new(devices::Console::new());

/// The global allocator for DMA-able memory. That is, memory which is tagged
/// non-cacheable in the page tables.
static DMA_ALLOCATOR: sync::NullLock<mm::BumpAllocator> =
    sync::NullLock::new(mm::BumpAllocator::new(
        // @todo Init this after we loaded boot memory map
        memory::map::virt::DMA_HEAP_START as usize,
        memory::map::virt::DMA_HEAP_END as usize,
        "Global DMA Allocator",
        // Try the following arguments instead to see all mailbox operations
        // fail. It will cause the allocator to use memory that are marked
        // cacheable and therefore not DMA-safe. The answer from the VideoCore
        // won't be received by the CPU because it reads an old cached value
        // that resembles an error case instead.

        // 0x00600000 as usize,
        // 0x007FFFFF as usize,
        // "Global Non-DMA Allocator",
    ));

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

#[cfg(not(feature = "noserial"))]
fn init_uart_serial() {
    use crate::platform::rpi3::{gpio::GPIO, mini_uart::MiniUart};
    let gpio = GPIO::default();
    let uart = MiniUart::default();
    let uart = uart.prepare(&gpio);
    CONSOLE.lock(|c| {
        // Move uart into the global CONSOLE.
        c.replace_with(uart.into());
    });

    println!("[0] MiniUART is live!");
}

/// Kernel entry point.
/// `arch` crate is responsible for calling it.
#[inline]
pub fn kmain() -> ! {
    #[cfg(feature = "jtag")]
    jtag::wait_debugger();

    #[cfg(not(feature = "noserial"))]
    init_uart_serial();

    init_exception_traps();
    init_mmu();

    #[cfg(test)]
    test_main();

    println!("Bye, hanging forever...");
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
