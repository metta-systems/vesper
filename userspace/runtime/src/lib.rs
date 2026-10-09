#![no_std]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! Runtime for EL0 userspace components.
//!
//! A component is a `no_std`/`no_main` binary linked with `user.ld`. It names
//! its entry function with [`entry!`]; the loader starts its first Thread
//! there at EL0, with one initial argument word. This crate also supplies the
//! panic handler and the default `vesper_thread_return_fault` handler that
//! `ppc_export!` adapters call when a Return is rejected.

use {core::panic::PanicInfo, libqemu::semihosting as semi};

pub use libqemu::semihosting;

/// Define the component entry `_start(argument: u64) -> !`, which calls
/// `$main(argument)`. The argument is whatever the loader passes in `x0`.
#[macro_export]
macro_rules! entry {
    ($main:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn _start(argument: u64) -> ! {
            $main(argument)
        }
    };
}

/// Spin forever. Components normally block forever on a capability
/// instead; this is the last resort when nothing else can stop the Thread.
pub fn halt() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
// Without `qemu`, semihosting printing compiles away and `info` is unused.
#[cfg_attr(not(feature = "qemu"), allow(unused_variables))]
fn panic(info: &PanicInfo) -> ! {
    semi::println!("EL0 PANICKED: {info}");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            semi::exit_failure()
        } else {
            halt()
        }
    }
}

/// Default handler for a rejected PPC Return: report and stop.
#[unsafe(no_mangle)]
pub extern "C" fn vesper_thread_return_fault(
    status: u64,
    detail1: u64,
    detail2: u64,
    original_r0: u64,
    original_r1: u64,
) -> ! {
    panic!(
        "PPC Return rejected: ({status}, {detail1:#x}, {detail2:#x}), results ({original_r0:#x}, {original_r1:#x})"
    );
}

/// Define `_start` for a passive component — one that only exports
/// procedures and never has a Thread of its own. Starting a Thread there is a
/// loader error. Leave `active` unset in its manifest (passive is the default).
#[macro_export]
macro_rules! passive {
    () => {
        #[unsafe(no_mangle)]
        pub extern "C" fn _start(_argument: u64) -> ! {
            panic!("a Thread was started in a passive component")
        }
    };
}
