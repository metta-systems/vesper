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
//!
//! The panic handler prints on the semihosting console (under `qemu`), then
//! calls the reporter a linked library registered with
//! [`set_panic_reporter`] (`liblog_client` puts a `PANICKED:` line on the
//! component's log channel), then exits QEMU or halts.

pub mod clock;

use {
    core::{
        panic::PanicInfo,
        sync::atomic::{AtomicBool, AtomicPtr, Ordering},
    },
    libqemu::semihosting as semi,
};

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

/// What reports a panic besides the semihosting console, as a
/// `fn(&PanicInfo)`; null for nothing. A library the component links sets it
/// (`liblog_client` once it finds the component's log channel): this crate
/// cannot depend on it.
static PANIC_REPORTER: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

/// Set by the first panic: a panic while reporting (a message whose
/// formatting panics, a logger left broken) or a second Thread's panic is
/// not reported again.
static PANICKING: AtomicBool = AtomicBool::new(false);

/// Have `reporter` called with the component's first panic, before the Thread
/// stops. It must never wait unbounded; a panic inside it stops the Thread
/// without a report.
pub fn set_panic_reporter(reporter: fn(&PanicInfo<'_>)) {
    PANIC_REPORTER.store(reporter as *mut (), Ordering::Release);
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    if PANICKING.swap(true, Ordering::AcqRel) {
        semi::println!("EL0 PANICKED again, not reported: {info}");
    } else {
        semi::println!("EL0 PANICKED: {info}");
        let reporter = PANIC_REPORTER.load(Ordering::Acquire);
        if !reporter.is_null() {
            // SAFETY: only `set_panic_reporter` stores a non-null value, and
            // it stores a `fn(&PanicInfo)`.
            let reporter = unsafe { core::mem::transmute::<*mut (), fn(&PanicInfo<'_>)>(reporter) };
            reporter(info);
        }
    }
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
