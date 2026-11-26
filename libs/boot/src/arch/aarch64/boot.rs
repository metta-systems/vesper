/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 *
 * Based on ideas from Jorge Aparicio, Andre Richter, Phil Oppenheimer, Sergio Benitez.
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Low-level boot of the ARMv8-A processor.
//! <http://infocenter.arm.com/help/topic/com.arm.doc.dai0527a/DAI0527A_baremetal_boot_code_for_ARMv8_A_processors.pdf>

//! Raspi kernel boot helper: <https://github.com/raspberrypi/tools/blob/master/armstubs/armstub8.S>
//! In particular, see `dtb_ptr32`

use {
    aarch64_cpu::registers::*,
    core::{
        arch::global_asm,
        sync::atomic::{AtomicU64, Ordering},
    },
    libcpu::endless_sleep,
    tock_registers::{Read, Write},
};

/// `x1` and `x2` at the kernel's entry. Written once, by plain stores, before
/// anything reads them: the MMU is off, where an atomic read-modify-write never
/// completes on the board.
static LOADER_REGISTERS: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

/// `x1` and `x2` as the loader left them at the kernel's entry: zero from the
/// firmware's stub and QEMU's boot code (the arm64 boot protocol reserves them),
/// chainboot's handoff from chainboot (`libchainboot_protocol::kept_rate`).
#[must_use]
pub fn loader_registers() -> [u64; 2] {
    LOADER_REGISTERS
        .each_ref()
        .map(|register| register.load(Ordering::Relaxed))
}

/// Type check the user-supplied entry function.
#[macro_export]
macro_rules! entry {
    ($path:path) => {
        /// # Safety
        /// Only type-checks!
        #[unsafe(export_name = "main")]
        #[inline(always)]
        pub unsafe fn __main(dtb: u32) -> ! {
            // type check the given path
            let f: unsafe fn(u32) -> ! = $path;

            unsafe { f(dtb) }
        }
    };
}

global_asm!(
    include_str!("boot.s"),
    CONST_AFFINITY_LOW_MASK = const libcpu::smp::LOW_AFFINITY_MASK,
    CONST_AFF3_SHIFT = const MPIDR_EL1::Aff3.shift,
    CONST_AFF3_BITS = const MPIDR_EL1::Aff3.mask.count_ones(),
    CONST_BOOT_CORE_AFFINITY = const libcpu::smp::BOOT_CORE_AFFINITY,
);

/// Entrypoint of the Rust code.
///
/// Checks that we started in EL2. If so, proceeds with setting up EL1.
///
/// This is invoked from the boot.s asm `_boot_cores` fn, does arch-specific init
/// and passes control to the kernel boot function `reset()`.
///
/// Dissection of various `RPi` core boot stubs is available
/// [here](https://leiradel.github.io/2019/01/20/Raspberry-Pi-Stubs.html).
///
/// # Safety
///
/// Totally unsafe! We're in the hardware land.
/// We assume that no statics are accessed before transition to this fn.
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.boot")]
pub unsafe extern "C" fn _startup_in_rust(dtb: u32, loader_x1: u64, loader_x2: u64) -> ! {
    // On entry, w0 should contain the dtb address.
    // For non-primary cores it contains 0.

    // BSS is zeroed: keep what the loader left in x1 and x2 for later.
    LOADER_REGISTERS[0].store(loader_x1, Ordering::Relaxed);
    LOADER_REGISTERS[1].store(loader_x2, Ordering::Relaxed);

    configure_el2();

    // The firmware and QEMU enter the kernel at EL2. Elsewhere memory mappings cannot be
    // configured: fail instead.
    if CurrentEL.get() == CurrentEL::EL::EL2.value {
        reset(dtb)
    }
    endless_sleep()
}

/// The EL2 state every core sets before it drops to EL1: the boot core from
/// [`_startup_in_rust`], each secondary core from Kickstart's secondary entry. Reset
/// leaves these registers UNKNOWN on every core.
///
/// Touches no memory beyond the stack: a secondary core calls it with its MMU off, before
/// anything else.
#[unsafe(link_section = ".text.boot")]
pub fn configure_el2() {
    // Enable timer counter registers for EL1 - FIXME can be done later after reset
    CNTHCTL_EL2.write(CNTHCTL_EL2::EL1PCEN::SET + CNTHCTL_EL2::EL1PCTEN::SET);

    // No virtual offset for reading the counters
    CNTVOFF_EL2.set(0);

    // Set System Control Register (EL1)
    // Make memory non-cacheable and disable MMU mapping.
    // Disable alignment checks, because Rust fmt module uses a little optimization
    // that happily reads and writes half-words (ldrh/strh) from/to unaligned addresses.
    SCTLR_EL1.write(
        SCTLR_EL1::I::NonCacheable
            + SCTLR_EL1::C::NonCacheable
            + SCTLR_EL1::M::Disable
            + SCTLR_EL1::A::Disable
            + SCTLR_EL1::SA::Disable
            + SCTLR_EL1::SA0::Disable,
    );

    // Same for SCTLR_EL2?
    SCTLR_EL2.write(
        SCTLR_EL2::I::NonCacheable
            + SCTLR_EL2::C::NonCacheable
            + SCTLR_EL2::M::Disable
            + SCTLR_EL2::A::Disable
            + SCTLR_EL2::SA::Disable,
    );

    // Set Hypervisor Configuration Register (EL2)
    // Set EL1 execution state to AArch64
    // @todo Explain the SWIO bit (SWIO is hardwired on RPi3)
    HCR_EL2.write(HCR_EL2::RW::EL1IsAarch64 + HCR_EL2::SWIO::SET);
    // @todo disable VM bit to prevent stage 2 MMU translations

    // Trap nothing to EL2: FP/SIMD trapping is then governed by `CPACR_EL1`
    // alone and traps are taken to EL1, where the kernel handles them; nothing
    // handles a trap taken to EL2. `HCR_EL2.E2H` is 0 (written above), which
    // selects this field layout. `TZ` and `TSM` are RES1 on the supported
    // ARMv8.0 cores (no SVE/SME), so they are written as 1; on a core with
    // SVE/SME they would route those traps to EL2 — selecting them by the
    // core's features belongs to runtime Arm version detection.
    CPTR_EL2.write(
        CPTR_EL2::TCPAC::NoTrap
            + CPTR_EL2::TTA::NoTrap
            + CPTR_EL2::RES1_13::SET
            + CPTR_EL2::TSM::Trap
            + CPTR_EL2::TFP::NoTrap
            + CPTR_EL2::RES1_9::SET
            + CPTR_EL2::TZ::Trap
            + CPTR_EL2::RES1_7_0::SET,
    );
}

// Enter Rust code in EL2.
#[unsafe(link_section = ".text.boot")]
fn reset(dtb: u32) -> ! {
    unsafe extern "Rust" {
        fn main(dtb: u32) -> !;
    }

    unsafe extern "Rust" {
        // Stack top
        static __STACK_TOP: core::cell::UnsafeCell<()>;
    }
    // Set up SP (stack pointer), which will be used by EL2 rust code.
    // SAFETY: Pure asm.
    unsafe {
        SP.set(__STACK_TOP.get() as u64);
    }

    // SAFETY: We're getting to more safety right here!
    unsafe { main(dtb) }
}
