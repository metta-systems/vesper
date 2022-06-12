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
#![feature(try_find)] // For DeviceTree iterators
#![feature(allocator_api)]
#![feature(ptr_internals)]
#![feature(format_args_nl)]
#![feature(stmt_expr_attributes)]
#![feature(slice_ptr_get)]
#![feature(custom_test_frameworks)]
#![test_runner(machine::tests::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![deny(missing_docs)]
#![deny(warnings)]
#![allow(unused)]
#![allow(internal_features)]
#![allow(linker_messages)]
#![feature(ptr_internals)]
#![feature(core_intrinsics)]

#[cfg(not(test))]
use core::panic::PanicInfo;
#[allow(unused_imports)]
use machine::devices::serial::SerialOps;
use {
    cfg_if::cfg_if,
    core::{cell::UnsafeCell, time::Duration},
    fdt_rs::{base::DevTree, error::DevTreeError, prelude::PropReader},
    machine::{
        console::console,
        device_tree::{DeviceTree, DeviceTreeProp},
        entry, exception, info, memory,
        platform::memory::mmu::virt_mem_layout,
        println, time, warn,
    },
};

entry!(kernel_init);

/// Kernel early init code.
/// `arch` crate is responsible for calling it.
///
/// # Safety
///
/// - Only a single core must be active and running this function.
/// - The init calls in this function must appear in the correct order:
///     - MMU + Data caching must be activated at the earliest. Without it, any atomic operations,
///       e.g. the yet-to-be-introduced spinlocks in the device drivers (which currently employ
///       IRQSafeNullLocks instead of spinlocks), will fail to work (properly) on the RPi SoCs.
pub unsafe fn kernel_init(dtb: u32) -> ! {
    #[cfg(feature = "jtag")]
    machine::debug::jtag::wait_debugger();

    exception::handling_init();

    let phys_kernel_tables_base_addr = match unsafe { memory::mmu::kernel_map_binary() } {
        Err(string) => panic!("Error mapping kernel binary: {}", string),
        Ok(addr) => addr,
    };

    if let Err(e) = unsafe { memory::mmu::enable_mmu_and_caching(phys_kernel_tables_base_addr) } {
        panic!("Enabling MMU failed: {}", e);
    }

    memory::mmu::post_enable_init();

    if let Err(x) = unsafe { machine::platform::drivers::init() } {
        panic!("Error initializing platform drivers: {}", x);
    }

    // Initialize all device drivers.
    unsafe { machine::drivers::driver_manager().init_drivers_and_irqs() };

    // Unmask interrupts on the boot CPU core.
    machine::exception::asynchronous::local_irq_unmask();

    // Announce conclusion of the kernel_init() phase.
    machine::state::state_manager().transition_to_single_core_main();

    // Transition from unsafe to safe.
    kernel_main(dtb)
}

/// Safe kernel code.
// #[inline]
#[cfg(not(test))]
pub fn kernel_main(dtb: u32) -> ! {
    // info!("{}", libkernel::version());
    // info!("Booting on: {}", bsp::board_name());

    info!(
        "{} version {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );
    info!("Booting on: {}", machine::platform::BcmHost::board_name());

    // info!("MMU online. Special regions:");
    // machine::platform::memory::mmu::virt_mem_layout().print_layout();

    let (_, privilege_level) = exception::current_privilege_level();
    info!("Current privilege level: {}", privilege_level);

    info!("Exception handling state:");
    exception::asynchronous::print_state();

    info!(
        "Architectural timer resolution: {} ns",
        time::time_manager().resolution().as_nanos()
    );

    info!("Drivers loaded:");
    machine::drivers::driver_manager().enumerate();

    info!("Registered IRQ handlers:");
    exception::asynchronous::irq_manager().print_handler();

    #[cfg(test)]
    test_main();

    // Test a failing timer case.
    time::time_manager().spin_for(Duration::from_nanos(1));

    for _ in 0..3 {
        info!("Spinning for 1 second");
        time::time_manager().spin_for(Duration::from_secs(1));
    }

    println!("DTB loaded at {:x}", dtb);

    // Safety: we got the address from the bootloader, if it lied - well, we're screwed!
    let device_tree =
        unsafe { DevTree::from_raw_pointer(dtb as *const _).expect("DeviceTree failed to read") };

    let layout = DeviceTree::layout(device_tree).expect("Couldn't calculate DeviceTree index");

    let block = machine::allocate_zeroed(layout)
        .map(|mut ret| unsafe { ret.as_mut() })
        // .map_err(|_| ())
        .expect("Couldn't allocate DeviceTree index");

    let device_tree =
        DeviceTree::new(device_tree, block).expect("Couldn't initialize indexed DeviceTree");

    let board = device_tree.get_prop_by_path("/model").unwrap().str();
    if board.is_ok() {
        println!("Running on {}", board.unwrap());
    }

    // To init memory allocation we need to parse memory regions from dtb and add the regions to
    // available memory regions list. Then initial BootRegionAllocator will get memory from these
    // regions and record their usage into some OTHER structures, removing these allocations from
    // the free regions list.
    // memory allocation is described by reg attribute of /memory block.
    // /#address-cells and /#size-cells specify the sizes of address and size attributes in reg.
    // To get memory size from DTB:
    // 1. Find nodes with unit-names `/memory`
    // 2. From those read reg entries, using `/#address-cells` and `/#size-cells` as units
    // 3. Union of all these reg entries will be the available memory. Enter it as mem-regions.

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

    let res: Result<_, DevTreeError> = device_tree
        .props()
        .try_find(|p| Ok(p.name()? == "device_type" && p.str()? == "memory"));
    let mem_prop = res.unwrap().expect("Unable to find memory node.");
    let _mem_node = mem_prop.node();
    // let parent_node = mem_node.parent_node();

    let reg_prop = device_tree
        .get_prop_by_path("/memory@0/reg")
        .expect("Unable to figure out memory-reg");

    println!(
        "Found memnode with reg prop: name {:?}, size {}",
        reg_prop.name(),
        reg_prop.length()
    );

    let reg_prop = DeviceTreeProp::new(reg_prop);
    let mut mem_iter = reg_prop.payload_pairs_iter(address_cells, size_cells);

    while let Some((mem_addr, mem_size)) = mem_iter.next() {
        println!("Memory: {} KiB at offset {}", mem_size / 1024, mem_addr);
    }

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

    // let address_cells = device_tree.try_struct_u32_value("/#address-cells");
    // let size_cells = device_tree.try_struct_u32_value("/#size-cells");

    // println!(
    //     "Memory DTB info: address-cells {:?}, size-cells {:?}",
    //     address_cells, size_cells
    // );

    dump_memory_map();

    command_prompt();

    reboot()
}

#[cfg(not(test))]
#[panic_handler]
fn panicked(info: &PanicInfo) -> ! {
    machine::panic::handler(info)
}

fn print_mmu_state_and_features() {
    // use machine::memory::mmu::interface::MMU;
    // memory::mmu::mmu().print_features();
}

fn dump_memory_map() {
    // Output the memory map as we could derive from FDT and information about our loaded image
    // Use it to imagine how the memmap would look like in the end.
    virt_mem_layout().print_layout();
}

//------------------------------------------------------------
// Start a command prompt
//------------------------------------------------------------
fn command_prompt() {
    'cmd_loop: loop {
        let mut buf = [0u8; 64];

        match machine::console::command_prompt(&mut buf) {
            // b"mmu" => init_mmu(),
            b"feats" => print_mmu_state_and_features(),
            // b"disp" => check_display_init(),
            b"trap" => check_data_abort_trap(),
            // b"map" => machine::platform::memory::mmu::virt_mem_layout().print_layout(),
            // b"led on" => set_led(true),
            // b"led off" => set_led(false),
            b"help" => print_help(),
            b"end" => break 'cmd_loop,
            x => warn!("[!] Unknown command {:?}, try 'help'", x),
        }
    }
}

fn print_help() {
    println!("Supported console commands:");
    println!("  mmu  - initialize MMU");
    println!("  feats - print MMU state and supported features");
    #[cfg(not(feature = "noserial"))]
    println!("  uart - try to reinitialize UART serial");
    // println!("  disp - try to init VC framebuffer and draw some text");
    println!("  trap - trigger and recover from a data abort exception");
    println!("  map  - show kernel memory layout");
    // println!("  led [on|off]  - change RPi LED status");
    println!("  end  - leave console and reset board");
}

// fn set_led(enable: bool) {
//     let mut mbox = Mailbox::<8>::default();
//     let index = mbox.request();
//     let index = mbox.set_led_on(index, enable);
//     let mbox = mbox.end(index);
//
//     mbox.call(channel::PropertyTagsArmToVc)
//         .map_err(|e| {
//             warn!("Mailbox call returned error {}", e);
//             warn!("Mailbox contents: {:?}", mbox);
//         })
//         .ok();
// }

fn reboot() -> ! {
    cfg_if! {
        if #[cfg(feature = "qemu")] {
            info!("Bye, shutting down QEMU");
            machine::qemu::semihosting::exit_success()
        } else {
            // use machine::platform::raspberrypi::power::Power;

            info!("Bye, going to reset now");
            // Power::default().reset()
            machine::cpu::endless_sleep()
        }
    }
}

// fn check_display_init() {
//     display_graphics()
//         .map_err(|e| {
//             warn!("Error in display: {}", e);
//         })
//         .ok();
// }
//
// fn display_graphics() -> Result<(), DrawError> {
//     if let Ok(mut display) = VC::init_fb(800, 600, 32) {
//         info!("Display created");
//
//         display.clear(Color::black());
//         info!("Display cleared");
//
//         display.rect(10, 10, 250, 250, Color::rgb(32, 96, 64));
//         display.draw_text(50, 50, "Hello there!", Color::rgb(128, 192, 255))?;
//
//         let mut buf = [0u8; 64];
//         let s = machine::write_to::show(&mut buf, format_args!("Display width {}", display.width));
//
//         if s.is_err() {
//             display.draw_text(50, 150, "Error displaying", Color::red())?
//         } else {
//             display.draw_text(50, 150, s.unwrap(), Color::white())?
//         }
//
//         display.draw_text(150, 50, "RED", Color::red())?;
//         display.draw_text(160, 60, "GREEN", Color::green())?;
//         display.draw_text(170, 70, "BLUE", Color::blue())?;
//     }
//     Ok(())
// }

fn check_data_abort_trap() {
    // Cause an exception by accessing a virtual address for which no
    // address translations have been set up.
    //
    // This line of code accesses the address 3 GiB, but page tables are
    // only set up for the range [0..1) GiB.
    let big_addr: u64 = 3 * 1024 * 1024 * 1024;
    unsafe { core::ptr::read_volatile(big_addr as *mut u64) };

    info!("[i] Whoa! We recovered from an exception.");
}

#[cfg(test)]
pub fn kernel_main(_dtb: u32) -> ! {
    test_main()
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
