#![no_std]
// `semi::println!` expands to `format_args_nl!` only under `qemu`.
#![cfg_attr(feature = "qemu", feature(format_args_nl))]

//! Shared scaffolding for Vesper's end-to-end boot-test kernels.
//!
//! `kicktest` and `endpoint-test` both run on the real Kickstart boot path and
//! then build their fixtures through ordinary capability invocations from the
//! boot Thread. This crate holds what they share:
//!
//! - [`keys`]: boot-table and component-table key/slot composition;
//! - [`paging`]: page-table walking and retained-image geometry;
//! - [`builder`]: bootstrap provisioning — page-table chains, Frames, the
//!   retained init image mapped into several roots, high execution stacks;
//! - [`component`]: a complete component `AddressSpace` with its own table,
//!   root and ASID;
//! - [`threads`]: fixture Threads queued runnable by the bootstrap builder;
//! - the panic handler, reporting through QEMU semihosting.
//!
//! Everything here is trusted `EL1t` fixture code. It uses bootstrap-only
//! kernel-private steps (`AddressSpace` and Thread allocation, grants of kinds
//! outside the `CopyDerive` allowlist) where no public ABI exists yet, and says
//! so at each use.

pub mod builder;
pub mod component;
pub mod keys;
pub mod loader;
pub mod paging;
pub mod threads;

use {core::panic::PanicInfo, libqemu::semihosting as semi};

#[panic_handler]
// Without `qemu`, semihosting printing compiles away and `info` is unused.
#[cfg_attr(not(feature = "qemu"), allow(unused_variables))]
fn panic(info: &PanicInfo) -> ! {
    semi::println!("PANICKED: {info}");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_failure()
        } else {
            libcpu::endless_sleep()
        }
    }
}
