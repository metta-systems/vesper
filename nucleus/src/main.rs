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
#![feature(const_fn_trait_bound)]
#![feature(nonnull_slice_from_raw_parts)]
#![feature(custom_test_frameworks)]
#![test_runner(crate::tests::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![deny(missing_docs)]
#![deny(warnings)]
#![allow(clippy::nonstandard_macro_braces)] // https://github.com/shepmaster/snafu/issues/296
#![allow(clippy::upper_case_acronyms)]
#![allow(clippy::enum_variant_names)]

#[cfg(not(target_arch = "aarch64"))]
use architecture_not_supported_sorry;

/// Architecture-specific code.
#[macro_use]
pub mod arch;
pub use arch::*;
mod boot_info;
mod device_tree;
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

use {
    crate::platform::rpi3::{
        display::{Color, DrawError},
        mailbox::{channel, Mailbox, MailboxOps},
        vc::VC,
    },
    cfg_if::cfg_if,
};

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
    memory::features::print_features();
}

fn init_mmu() {
    // unsafe {
    //     memory::mmu::init().expect("MMU init failed");
    // }
    println!("[!] MMU initialised");
    print_mmu_state_and_features();
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
    println!("[!] Exception traps set up");
}

#[cfg(not(feature = "noserial"))]
fn init_uart_serial() {
    use crate::platform::rpi3::{gpio::GPIO, mini_uart::MiniUart, pl011_uart::PL011Uart};

    let gpio = GPIO::default();
    let uart = MiniUart::default();
    let uart = uart.prepare(&gpio);
    CONSOLE.lock(|c| {
        // Move uart into the global CONSOLE.
        c.replace_with(uart.into());
    });

    println!("[0] MiniUART is live!");

    // Then immediately switch to PL011 (just as an example)

    let uart = PL011Uart::default();
    let mbox = Mailbox::default();

    // uart.init() will reconfigure the GPIO, which causes a race against
    // the MiniUart that is still putting out characters on the physical
    // line that are already buffered in its TX FIFO.
    //
    // To ensure the CPU doesn't rewire the GPIO before the MiniUart has put
    // its last character, explicitly flush it before rewiring.
    //
    // If you switch to an output that happens to not use the same pair of
    // physical wires (e.g. the Framebuffer), you don't need to do this,
    // because flush() is anyways called implicitly by replace_with(). This
    // is just a special case.
    use crate::devices::console::ConsoleOps;
    CONSOLE.lock(|c| c.flush());

    match uart.prepare(mbox, &gpio) {
        Ok(uart) => {
            CONSOLE.lock(|c| {
                // Move uart into the global CONSOLE.
                c.replace_with(uart.into());
            });
            println!("[0] UART0 is live!");
        }
        Err(_) => println!("[0] Error switching to PL011 UART, continue with MiniUART"),
    }
}

/// Kernel entry point.
/// `arch` crate is responsible for calling it.
#[inline]
pub fn kmain(dtb: u32) -> ! {
    #[cfg(feature = "jtag")]
    jtag::wait_debugger();

    init_mmu();
    init_exception_traps();

    #[cfg(not(feature = "noserial"))]
    init_uart_serial();

    #[cfg(test)]
    test_main();

    println!("DTB loaded at {:x}", dtb);

    // Safety: we got the address from the bootloader, if it lied - well, we're screwed!
    let device_tree = crate::device_tree::DeadTree::new(unsafe {
        dtb::Reader::read_from_address(dtb as usize).expect("DeviceTree not found")
    });

    // List unusable memory, and remove it from the memory regions for the allocator.
    for entry in device_tree.reserved_mem_entries() {
        println!("reserved: {:?} bytes at {:?}", entry.size, entry.address);
    }
    // Also, remove the DTB memory region.

    // To init memory allocation we need to parse memory regions from dtb and add the regions to
    // available memory regions list. Then initial BootRegionAllocator will get memory from these
    // regions and record their usage into some OTHER structures, removing these allocations from
    // the free regions list.
    // memory allocation is described by reg attribute of /memory block.
    // /#address-cells and /#size-cells specify the sizes of address and size attributes in reg.

    let address_cells = device_tree.try_struct_u32_value("/#address-cells");
    let size_cells = device_tree.try_struct_u32_value("/#size-cells");
    let board = device_tree.try_struct_str_value("/model");

    if board.is_ok() {
        println!("Running on {}", board.unwrap());
    }

    println!(
        "Memory DTB info: address-cells {:?}, size-cells {:?}",
        address_cells, size_cells
    );

    dump_memory_map();

    command_prompt();

    reboot()
}

fn dump_memory_map() {
    // Output the memory map as we could derive from FDT and information about our loaded image
    // Use it to imagine how the memmap would look like in the end.
    arch::memory::print_layout();


}

//------------------------------------------------------------
// Start a command prompt
//------------------------------------------------------------
fn command_prompt() {
    'cmd_loop: loop {
        let mut buf = [0u8; 64];

        match CONSOLE.lock(|c| c.command_prompt(&mut buf)) {
            b"mmu" => init_mmu(),
            b"feats" => print_mmu_state_and_features(),
            #[cfg(not(feature = "noserial"))]
            b"uart" => init_uart_serial(),
            b"disp" => check_display_init(),
            b"trap" => check_data_abort_trap(),
            b"map" => arch::memory::print_layout(),
            b"led on" => set_led(true),
            b"led off" => set_led(false),
            b"help" => print_help(),
            b"end" => break 'cmd_loop,
            x => println!("[!] Unknown command {:?}, try 'help'", x),
        }
    }
}

fn print_help() {
    println!("Supported console commands:");
    println!("  mmu  - initialize MMU");
    println!("  feats - print MMU state and supported features");
    #[cfg(not(feature = "noserial"))]
    println!("  uart - try to reinitialize UART serial");
    println!("  disp - try to init VC framebuffer and draw some text");
    println!("  trap - trigger and recover from a data abort exception");
    println!("  map  - show kernel memory layout");
    println!("  led [on|off]  - change RPi LED status");
    println!("  end  - leave console and reset board");
}

fn set_led(enable: bool) {
    let mut mbox = Mailbox::default();
    let index = mbox.request();
    let index = mbox.set_led_on(index, enable);
    let mbox = mbox.end(index);

    mbox.call(channel::PropertyTagsArmToVc)
        .map_err(|e| {
            println!("Mailbox call returned error {}", e);
            println!("Mailbox contents: {:?}", mbox);
        })
        .ok();
}

fn reboot() -> ! {
    cfg_if! {
        if #[cfg(feature = "qemu")] {
            println!("Bye, shutting down QEMU");
            qemu::semihosting::exit_success()
        } else {
            use crate::platform::rpi3::power::Power;

            println!("Bye, going to reset now");
            Power::new().reset()
        }
    }
}

fn check_display_init() {
    display_graphics()
        .map_err(|e| {
            println!("Error in display: {}", e);
        })
        .ok();
}

fn display_graphics() -> Result<(), DrawError> {
    if let Ok(mut display) = VC::init_fb(800, 600, 32) {
        println!("Display created");

        display.clear(Color::black());
        println!("Display cleared");

        display.rect(10, 10, 250, 250, Color::rgb(32, 96, 64));
        display.draw_text(50, 50, "Hello there!", Color::rgb(128, 192, 255))?;

        let mut buf = [0u8; 64];
        let s = write_to::show(&mut buf, format_args!("Display width {}", display.width));

        if s.is_err() {
            display.draw_text(50, 150, "Error displaying", Color::red())?
        } else {
            display.draw_text(50, 150, s.unwrap(), Color::white())?
        }

        display.draw_text(150, 50, "RED", Color::red())?;
        display.draw_text(160, 60, "GREEN", Color::green())?;
        display.draw_text(170, 70, "BLUE", Color::blue())?;
    }
    Ok(())
}

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

#[cfg(test)]
mod main_tests {
    use super::*;

    #[test_case]
    fn test_data_abort_trap() {
        check_data_abort_trap()
    }
}
