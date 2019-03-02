#![no_std]
#![no_main]
#![feature(asm)]
#![feature(global_asm)]
#![feature(const_fn)]
#![feature(format_args_nl)]
#![feature(lang_items)]
#![feature(ptr_internals)] // until we mark with PhantomData instead?
#![feature(core_intrinsics)]
#![feature(range_contains)]
#![feature(try_from)]
#![doc(html_root_url = "https://docs.metta.systems/")]
#![allow(dead_code)]
#![allow(unused_assignments)]
#![allow(unused_must_use)]

//any(target_arch = "aarch64", target_arch = "x86_64")
#[cfg(not(target_arch = "aarch64"))]
use architecture_not_supported_sorry;

extern crate bitflags;
#[macro_use]
extern crate register;
extern crate cortex_a;
extern crate rlibc;

#[macro_use]
pub mod arch;
pub use arch::*;
mod devices;
mod macros;
pub mod platform;
mod sync;
mod write_to;

use core::fmt::Write;
#[cfg(feature = "jlink")]
use jlink_rtt::Output;
use platform::{
    display::{Color, Size2d},
    gpio::GPIO,
    // uart::MiniUart,
    vc::VC,
};

// User-facing kernel parts - syscalls and capability invocations.
// pub mod vesper; -- no mod exported, because available through syscall interface

// Actual interfaces to call these syscalls are in vesper-user (similar to libsel4)
// pub mod vesper; -- exported from vesper-user

/// The global console. Output of the print! and println! macros.
static CONSOLE: sync::NullLock<devices::Console> = sync::NullLock::new(devices::Console::new());

// Kernel entry point
// arch crate is responsible for calling this
fn kmain() -> ! {
    let gpio = GPIO::new_default();

    let uart = platform::MiniUart::new_default();
    uart.init(&gpio);
    CONSOLE.lock(|c| {
        // Moves uart into the global CONSOLE. It is not accessible
        // anymore for the remaining parts of kernel_entry().
        c.replace_with(uart.into());
    });

    let uart = platform::PL011Uart::new_default();

    let mut mbox = platform::mailbox::Mailbox::new();

    match uart.init(&mut mbox, &gpio) {
        Ok(_) => {
            CONSOLE.lock(|c| {
                // Moves uart into the global CONSOLE. It is not accessible
                // anymore for the remaining parts of kernel_entry().
                c.replace_with(uart.into());
            });
        }
        Err(_) => endless_sleep(),
    }

    let mut out = Output::new();
    writeln!(out, "JLink RTT is working!"); // @todo RttConsole

    println!("\n[0] UART is live!");

    extern "C" {
        static __exception_vectors_start: u64;
    }

    //==============================================
    // Since formatted output doesn't work, lets do some other preparatory steps:
    // 1. Initialize MMU
    // 2. Set up exception handlers
    // Obviously, things should keep working after that...
    //==============================================

    unsafe {
        let exception_vectors_start: u64 = &__exception_vectors_start as *const _ as u64;

        arch::traps::set_vbar_el1_checked(exception_vectors_start);
        println!("Exception traps set up");
    }

    unsafe {
        mmu::init();
    }
    println!("MMU initialised");

    if let Some(mut display) = VC::init_fb(Size2d { x: 800, y: 600 }, 32) {
        println!("Display created");

        display.clear(Color::black()); // Takes A LOONG time, check caching opts?
        println!("Display cleared");

        display.rect(10, 10, 250, 250, Color::rgb(32, 96, 64));
        display.draw_text(50, 50, "Hello there!", Color::rgb(128, 192, 255));

        // let mut buf = [0u8; 64];
        // Crashes if uncommenting next line: vvv
        // let s = write_to::show(&mut buf, format_args!("Display width {}", display.width));
        // So, some rust runtime things are breaking it, why?

        // if s.is_err() {
        //     display.draw_text(50, 150, "Error displaying", Color::red())
        // } else {
        // display.draw_text(50, 150, s.unwrap(), Color::white());
        // }

        display.draw_text(150, 50, "RED", Color::red());
        display.draw_text(160, 60, "GREEN", Color::green());
        display.draw_text(170, 70, "BLUE", Color::blue());
    }

    //------------------------------------------------------------
    // Start a command prompt
    //------------------------------------------------------------
    'cmd_loop: loop {
        let mut buf = [0u8; 64];

        match CONSOLE.lock(|c| c.command_prompt(&mut buf)) {
            b"trap" => check_data_abort_trap(),
            b"map" => arch::memory::print_layout(),
            b"help" => print_help(),
            b"end" => break 'cmd_loop,
            x => println!("Unknown command {:?}, try 'help'", x),
        }
    }

    println!("Bye, going to sleep now");
    // qemu_aarch64_exit()
    endless_sleep()
}

fn print_help() {
    println!("Supported console commands:");
    println!("  trap - cause and recover from a data abort exception");
    println!("  map  - show kernel memory layout");
    println!("  end  - leave console and lock up");
}

fn check_data_abort_trap() {
    // Cause an exception by accessing a virtual address for which no
    // address translations have been set up.
    //
    // This line of code accesses the address 3 GiB, but page tables are
    // only set up for the range [0..1] GiB.
    let big_addr: u64 = 3 * 1024 * 1024 * 1024;
    unsafe { core::ptr::read_volatile(big_addr as *mut u64) };

    println!("[i] Whoa! We recovered from an exception.");
}

entry!(kmain);

// From https://stackoverflow.com/a/49930361/895245
// @todo specify exit value depending on tests result?
// fn qemu_aarch64_exit() -> ! {
//     unsafe {
//         asm!("
//             /* 0x20026 == ADP_Stopped_ApplicationExit */
//             mov x1, #0x26
//             movk x1, #2, lsl #16
//             str x1, [sp,#0]

//             /* Exit status code. Host QEMU process exits with that status. */
//             mov x0, #0
//             str x0, [sp,#8]

//             /* x1 contains the address of parameter block.
//              * Any memory address could be used. */
//             mov x1, sp

//             /* SYS_EXIT */
//             mov w0, #0x18

//             /* Do the semihosting call on A64. */
//             hlt 0xf000"
//         :::: "volatile");
//     }
//     unreachable!();
// }
