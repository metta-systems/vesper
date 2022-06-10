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
#![feature(try_find)] // For DeviceTree iterators
#![feature(custom_test_frameworks)]
#![test_runner(machine::tests::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![deny(missing_docs)]
#![deny(warnings)]

#[cfg(not(test))]
use core::panic::PanicInfo;
#[allow(unused_imports)]
use machine::devices::SerialOps;
use {
    cfg_if::cfg_if,
    core::{alloc::Allocator, cell::UnsafeCell},
    device_tree::DeviceTree,
    fdt_rs::{
        base::DevTree,
        prelude::{FallibleIterator, PropReader},
    },
    machine::{
        arch, entry, memory,
        platform::rpi3::{
            display::{Color, DrawError},
            mailbox::{channel, Mailbox, MailboxOps},
            vc::VC,
        },
        println, CONSOLE,
    },
};

entry!(kmain);

#[cfg(not(test))]
#[panic_handler]
fn panicked(info: &PanicInfo) -> ! {
    machine::panic::handler(info)
}

fn print_mmu_state_and_features() {
    use machine::memory::mmu::interface::MMU;
    memory::mmu::mmu().print_features();
}

fn init_mmu() {
    unsafe {
        use machine::memory::mmu::interface::MMU;
        if let Err(e) = memory::mmu::mmu().enable_mmu_and_caching() {
            panic!("MMU: {}", e);
        }
    }
    println!("[!] MMU initialised");
    print_mmu_state_and_features();
}

fn init_exception_traps() {
    extern "Rust" {
        static __exception_vectors_start: UnsafeCell<()>;
    }

    unsafe {
        arch::traps::set_vbar_el1_checked(__exception_vectors_start.get() as u64)
            .expect("Vector table properly aligned!");
    }
    println!("[!] Exception traps set up");
}

#[cfg(not(feature = "noserial"))]
fn init_uart_serial() {
    use machine::platform::rpi3::{gpio::GPIO, mini_uart::MiniUart, pl011_uart::PL011Uart};

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
    CONSOLE.lock(|c| c.flush());

    match uart.prepare(&gpio) {
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
    machine::arch::jtag::wait_debugger();

    #[cfg(not(feature = "noserial"))]
    init_uart_serial();

    init_exception_traps();
    init_mmu();

    println!("DTB loaded at {:x}", dtb);

    // Safety: we got the address from the bootloader, if it lied - well, we're screwed!
    let device_tree =
        unsafe { DevTree::from_raw_pointer(dtb as *const _).expect("DeviceTree failed to read") };

    let layout = DeviceTree::layout(device_tree).expect("Couldn't calculate DeviceTree index");

    let mut block = crate::DMA_ALLOCATOR
        .lock(|dma| dma.allocate_zeroed(layout))
        .map(|mut ret| unsafe { ret.as_mut() })
        .map_err(|_| ())
        .expect("Couldn't allocate DeviceTree index");

    let device_tree =
        DeviceTree::new(device_tree, block).expect("Couldn't initialize indexed DeviceTree");

    let model = device_tree
        .get_prop_by_path("/model")
        .unwrap()
        .str()
        .expect("Model must be a string");
    println!("Booting on {}", model);

    let address_cells = device_tree
        .get_prop_by_path("/#address-cells")
        .expect("Unable to figure out #address-cells")
        .u32(0)
        .expect("Invalid format for #address-cells");

    let size_cells = device_tree
        .get_prop_by_path("/#size-cells")
        .expect("Unable to figure out #size-cells")
        .u32(0)
        .expect("Invalid format for #size-cells");

    // @todo boot this on 8Gb RasPi, because I'm not sure how it allocates memory regions there.
    println!(
        "Address cells: {}, size cells {}",
        address_cells, size_cells
    );

    let mem_prop = device_tree
        .props()
        .find(|p| Ok(p.name()? == "device_type" && p.str()? == "memory"))
        .unwrap()
        .expect("Unable to find memory node.");
    let mem_node = mem_prop.node();
    // let parent_node = mem_node.parent_node();

    let reg_prop = device_tree
        .get_prop_by_path("/memory@0/reg")
        .expect("Unable to figure out memory-reg");

    println!(
        "Found memnode with reg prop: name {:?}, size {}",
        reg_prop.name(),
        reg_prop.length()
    );

    let mem_addr = reg_prop.u32(0).expect("Oops");
    let mem_size = reg_prop.u32(1).expect("Oops");

    println!("Memory: {} KiB at offset {}", mem_size / 1024, mem_addr);

    // List unusable memory, and remove it from the memory regions for the allocator.
    let mut iter = device_tree.fdt().reserved_entries();
    while let Some(entry) = iter.next() {
        println!(
            "Reserved memory: {:?} bytes at {:?}",
            entry.size, entry.address
        );
    }

    // Iterate compatible nodes (example):
    // let mut iter = device_tree.compatible_nodes("arm,pl011");
    // while let Some(entry) = iter.next() {
    //     println!("reserved: {:?} (bytes at ?)", entry.name()/*, entry.address*/);
    // }

    // Also, remove the DTB memory region + index
    println!(
        "DTB region: {} bytes at {:x}",
        device_tree.fdt().totalsize(),
        dtb
    );

    // To init memory allocation we need to parse memory regions from dtb and add the regions to
    // available memory regions list. Then initial BootRegionAllocator will get memory from these
    // regions and record their usage into some OTHER structures, removing these allocations from
    // the free regions list.
    // memory allocation is described by reg attribute of /memory block.
    // /#address-cells and /#size-cells specify the sizes of address and size attributes in reg.

    // let address_cells = device_tree.try_struct_u32_value("/#address-cells");
    // let size_cells = device_tree.try_struct_u32_value("/#size-cells");
    // let board = device_tree.try_struct_str_value("/model");

    // if board.is_ok() {
    //     println!("Running on {}", board.unwrap());
    // }

    // println!(
    //     "Memory DTB info: address-cells {:?}, size-cells {:?}",
    //     address_cells, size_cells
    // );

    dump_memory_map();

    #[cfg(test)]
    test_main();

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
            b"map" => machine::platform::memory::mmu::virt_mem_layout().print_layout(),
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
    let mut mbox = Mailbox::<8>::default();
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
            machine::qemu::semihosting::exit_success()
        } else {
            use machine::platform::rpi3::power::Power;

            println!("Bye, going to reset now");
            Power::default().reset()
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
        let s = machine::write_to::show(&mut buf, format_args!("Display width {}", display.width));

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
    use {super::*, core::panic::PanicInfo};

    #[panic_handler]
    fn panicked(info: &PanicInfo) -> ! {
        machine::panic::handler_for_tests(info)
    }

    #[test_case]
    fn test_data_abort_trap() {
        check_data_abort_trap()
    }
}
