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

libboot::entry!(kernel_init);

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
pub unsafe fn kernel_init() -> ! {
    #[cfg(feature = "jtag")]
    libmachine::debug::jtag::wait_debugger();

    libexception::exception::handling_init();

    let phys_kernel_tables_base_addr = match unsafe { libmemory::mmu::kernel_map_binary() } {
        Err(string) => panic!("Error mapping kernel binary: {}", string),
        Ok(addr) => addr,
    };

    if let Err(e) = unsafe { libmemory::mmu::enable_mmu_and_caching(phys_kernel_tables_base_addr) }
    {
        panic!("Enabling MMU failed: {}", e);
    }

    libmemory::mmu::post_enable_init();

    if let Err(x) = unsafe { libplatform::drivers::init() } {
        panic!("Error initializing platform drivers: {}", x);
    }

    // Initialize all device drivers.
    unsafe { machine::drivers::driver_manager().init_drivers_and_irqs() };

    // Unmask interrupts on the boot CPU core.
    libexception::exception::asynchronous::local_irq_unmask();

    // Announce conclusion of the kernel_init() phase.
    libkernel_state::state_manager().transition_to_single_core_main();

    libconsole::init_logger();

    // Transition from unsafe to safe.
    kernel_main()
}

/// Safe kernel code.
// #[inline]
pub fn kernel_main() -> ! {
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

    let (_, privilege_level) = libexception::exception::current_privilege_level();
    info!("Current privilege level: {}", privilege_level);

    info!("Exception handling state:");
    libexception::exception::asynchronous::print_state();

    info!(
        "Architectural timer resolution: {} ns",
        libtime::time::time_manager().resolution().as_nanos()
    );

    info!("Drivers loaded:");
    machine::drivers::driver_manager().enumerate();

    info!("Registered IRQ handlers:");
    machine::platform::exception::asynchronous::irq_manager().print_handler();

    // Test a failing timer case.
    libtime::time::time_manager().spin_for(Duration::from_nanos(1));

    for _ in 0..3 {
        info!("Spinning for 1 second");
        libtime::time::time_manager().spin_for(Duration::from_secs(1));
    }

    command_prompt();

    reboot()
}

#[panic_handler]
fn panicked(info: &PanicInfo) -> ! {
    libmachine::panic::handler(info)
}

fn print_mmu_state_and_features() {
    // use machine::memory::mmu::interface::MMU;
    // memory::mmu::mmu().print_features();
}

//------------------------------------------------------------
// Start a command prompt
//------------------------------------------------------------
fn command_prompt() {
    'cmd_loop: loop {
        let mut buf = [0u8; 64];

        match libconsole::console::command_prompt(&mut buf) {
            // b"mmu" => init_mmu(),
            b"feats" => print_mmu_state_and_features(),
            // b"disp" => check_display_init(),
            // b"trap" => check_data_abort_trap(),
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
            libqemu::semihosting::exit_success()
        } else {
            // use machine::platform::raspberrypi::power::Power;

            info!("Bye, going to reset now");
            // Power::default().reset()
            libcpu::endless_sleep()
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
