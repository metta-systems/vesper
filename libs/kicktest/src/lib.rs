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

use {
    aarch64_cpu::registers::{
        CNTKCTL_EL1, CPACR_EL1, MDSCR_EL1, PMUSERENR_EL0, Readable, TPIDRRO_EL0,
    },
    core::panic::PanicInfo,
    libqemu::semihosting as semi,
};

/// Check the effective FP/SIMD policy before any component runs: Kickstart
/// must have left `CPACR_EL1` trapping FP/SIMD (and SVE) at both EL1 and EL0.
/// (`CPTR_EL2` is not readable from EL1; that FP/SIMD traps reach EL1 is shown
/// by `fp-trap-test`.)
pub fn assert_fp_simd_trapped() {
    assert!(
        CPACR_EL1.matches_all(CPACR_EL1::FPEN::TrapEl0El1 + CPACR_EL1::ZEN::TrapEl0El1),
        "FP/SIMD must trap at EL1 and EL0 (CPACR_EL1 = {:#x})",
        CPACR_EL1.get()
    );
}

/// Check the selected EL0-visible architectural state Kickstart established:
/// read-only TLS zero, EL0 limited to reading the virtual counter, EL0
/// performance-monitor and debug-channel access trapped, software debug off.
pub fn assert_el0_visible_state() {
    assert_eq!(TPIDRRO_EL0.get(), 0, "TPIDRRO_EL0 must be zero");
    assert!(
        CNTKCTL_EL1.matches_all(
            CNTKCTL_EL1::EL0PTEN::TrappedPhysical
                + CNTKCTL_EL1::EL0VTEN::TrappedVirtual
                + CNTKCTL_EL1::EVNTEN::Disable
                + CNTKCTL_EL1::EL0VCTEN::TrappedNone
                + CNTKCTL_EL1::EL0PCTEN::TrappedFreqPct
        ),
        "EL0 may only read the virtual counter (CNTKCTL_EL1 = {:#x})",
        CNTKCTL_EL1.get()
    );
    assert!(
        MDSCR_EL1.matches_all(
            MDSCR_EL1::TDCC::SET
                + MDSCR_EL1::MDE::CLEAR
                + MDSCR_EL1::KDE::CLEAR
                + MDSCR_EL1::SS::CLEAR
        ),
        "EL0 debug-channel access must trap, software debug off (MDSCR_EL1 = {:#x})",
        MDSCR_EL1.get()
    );
    assert!(
        PMUSERENR_EL0.matches_all(
            PMUSERENR_EL0::ER::TrappedUnlessEnabled
                + PMUSERENR_EL0::CR::TrappedUnlessEnabled
                + PMUSERENR_EL0::SW::TrappedUnlessEnabled
                + PMUSERENR_EL0::EN::TrappedUnlessEnabled
        ),
        "EL0 performance-monitor access must trap (PMUSERENR_EL0 = {:#x})",
        PMUSERENR_EL0.get()
    );
}

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
