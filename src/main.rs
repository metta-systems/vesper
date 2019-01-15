#![no_std]
#![no_main]
#![feature(asm)]
#![feature(const_fn)]
#![feature(lang_items)]
#![feature(ptr_internals)] // until we mark with PhantomData instead?
#![feature(core_intrinsics)]
#![doc(html_root_url = "https://docs.metta.systems/")]
#![allow(dead_code)]
#![allow(unused_assignments)]
#![allow(unused_must_use)]

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
use architecture_not_supported_sorry;

// use core::intrinsics::abort;

#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate register;
extern crate cortex_a;
extern crate rlibc;

use core::panic::PanicInfo;
#[macro_use]
pub mod arch;
pub use arch::*;
pub mod platform;

use core::fmt::Write;
use platform::{
    display::{Color, Size2d},
    uart::MiniUart,
    vc::VC,
};

// User-facing kernel parts - syscalls and capability invocations.
// pub mod vesper; -- no mod exported, because available through syscall interface

// Actual interfaces to call these syscalls are in vesper-user (similar to libsel4)
// pub mod vesper; -- exported from vesper-user

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // @todo rect() + drawtext("PANIC")?
    endless_sleep()
}

// Kernel entry point
// arch crate is responsible for calling this
pub fn kmain() -> ! {
    let mut uart = MiniUart::new();
    uart.init();
    writeln!(uart, "Hey there, mini uart talking!");

    if let Some(mut display) = VC::init_fb(Size2d { x: 800, y: 600 }, &mut uart) {
        display.rect(10, 10, 250, 250, Color::rgb(32, 96, 64).0);
        display.draw_text(50, 50, "Hello there!", Color::rgb(128, 192, 255).0);
        // display.draw_text(50, 150, core::fmt("Display width {}", display.width), Color::rgb(255,0,0).0);

        display.draw_text(150, 50, "RED", Color::rgb(255, 0, 0).0);
        display.draw_text(160, 60, "GREEN", Color::rgb(0, 255, 0).0);
        display.draw_text(170, 70, "BLUE", Color::rgb(0, 0, 255).0);
    }

    writeln!(uart, "Bye, going to sleep now");
    qemu_aarch64_exit(); //endless_sleep()
}

// From https://stackoverflow.com/a/49930361/895245
// @todo specify exit value depending on tests result?
fn qemu_aarch64_exit() -> ! {
    unsafe {
        asm!("
            /* 0x20026 == ADP_Stopped_ApplicationExit */
            mov x1, #0x26
            movk x1, #2, lsl #16
            str x1, [sp,#0]

            /* Exit status code. Host QEMU process exits with that status. */
            mov x0, #0
            str x0, [sp,#8]

            /* x1 contains the address of parameter block.
             * Any memory address could be used. */
            mov x1, sp

            /* SYS_EXIT */
            mov w0, #0x18

            /* Do the semihosting call on A64. */
            hlt 0xf000"
        :::: "volatile");
    }
    unreachable!();
}
