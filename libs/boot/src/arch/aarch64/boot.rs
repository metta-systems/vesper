/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 *
 * Based on ideas from Jorge Aparicio, Andre Richter, Phil Oppenheimer, Sergio Benitez.
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Low-level boot of the ARMv8-A processor.
//! <http://infocenter.arm.com/help/topic/com.arm.doc.dai0527a/DAI0527A_baremetal_boot_code_for_ARMv8_A_processors.pdf>

use {
    aarch64_cpu::{asm, registers::*},
    core::{arch::global_asm, cell::UnsafeCell},
    libcpu::endless_sleep,
    libplatform::platform::cpu::BOOT_CORE_ID,
    tock_registers::interfaces::{Readable, Writeable},
};

/// Type check the user-supplied entry function.
#[macro_export]
macro_rules! entry {
    ($path:path) => {
        /// # Safety
        /// Only type-checks!
        #[unsafe(export_name = "main")]
        #[inline(always)]
        pub unsafe fn __main() -> ! {
            // type check the given path
            let f: unsafe fn() -> ! = $path;

            unsafe { f() }
        }
    };
}

global_asm!(
    include_str!("boot.s"),
    CONST_CORE_ID_MASK = const 0b11,
    CONST_BOOT_CORE_ID = const BOOT_CORE_ID,
);

/// Entrypoint of the Rust code.
///
/// Checks if we started in EL2/EL3. If so, proceeds with setting up EL1.
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
/// We assume that no statics are accessed before transition to main from `reset()` function.
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.boot")]
pub unsafe extern "C" fn _startup_in_rust() -> ! {
    // Can't match values with dots in match, so use intermediate consts.
    #[cfg(feature = "qemu")]
    const EL3: u64 = CurrentEL::EL::EL3.value;
    const EL2: u64 = CurrentEL::EL::EL2.value;
    const EL1: u64 = CurrentEL::EL::EL1.value;

    shared_setup_and_enter_pre();

    match CurrentEL.get() {
        #[cfg(feature = "qemu")]
        EL3 => setup_and_enter_el1_from_el3(),
        EL2 => setup_and_enter_el1_from_el2(),
        EL1 => reset(),
        // if not core0 or not EL3/EL2/EL1, infinitely wait for events
        _ => endless_sleep(),
    }
}

#[unsafe(link_section = ".text.boot")]
#[inline(always)]
fn shared_setup_and_enter_pre() {
    // Enable timer counter registers for EL1
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

    // enable_armv6_unaligned_access();

    // Set Hypervisor Configuration Register (EL2)
    // Set EL1 execution state to AArch64
    // @todo Explain the SWIO bit (SWIO hardwired on Pi3)
    HCR_EL2.write(HCR_EL2::RW::EL1IsAarch64 + HCR_EL2::SWIO::SET);
    // @todo disable VM bit to prevent stage 2 MMU translations
}

#[unsafe(link_section = ".text.boot")]
#[inline]
fn shared_setup_and_enter_post() -> ! {
    unsafe extern "Rust" {
        // Stack top
        static __STACK_TOP: UnsafeCell<()>;
    }
    // Set up SP_EL1 (stack pointer), which will be used by EL1 once
    // we "return" to it.
    // SAFETY: Pure asm.
    unsafe {
        SP_EL1.set(__STACK_TOP.get() as u64);
    }

    // Use `eret` to "return" to EL1. This will result in execution of
    // `reset()` in EL1.
    asm::eret()
}

/// Real hardware boot-up sequence.
///
/// Prepare and execute transition from EL2 to EL1.
#[unsafe(link_section = ".text.boot")]
#[inline]
fn setup_and_enter_el1_from_el2() -> ! {
    // Set Saved Program Status Register (EL2)
    // Set up a simulated exception return.
    //
    // Fake a saved program status, where all interrupts were
    // masked and SP_EL1 was used as a stack pointer.
    SPSR_EL2.write(
        SPSR_EL2::D::Masked
            + SPSR_EL2::A::Masked
            + SPSR_EL2::I::Masked
            + SPSR_EL2::F::Masked
            + SPSR_EL2::M::EL1h, // Use SP_EL1
    );

    // Make the Exception Link Register (EL2) point to reset().
    #[allow(clippy::fn_to_numeric_cast_any)]
    ELR_EL2.set(reset as *const () as u64);

    shared_setup_and_enter_post()
}

/// QEMU boot-up sequence.
///
/// Processors enter EL3 after reset.
/// ref: http://infocenter.arm.com/help/topic/com.arm.doc.dai0527a/DAI0527A_baremetal_boot_code_for_ARMv8_A_processors.pdf
/// section: 5.5.1
/// However, GPU init code must be switching it down to EL2.
/// QEMU can't emulate Raspberry Pi properly (no VC boot code), so it starts in EL3.
///
/// Prepare and execute transition from EL3 to EL1.
/// (from https://github.com/s-matyukevich/raspberry-pi-os/blob/master/docs/lesson02/rpi-os.md)
#[cfg(feature = "qemu")]
#[unsafe(link_section = ".text.boot")]
#[inline]
fn setup_and_enter_el1_from_el3() -> ! {
    // Set Secure Configuration Register (EL3)
    SCR_EL3.write(SCR_EL3::RW::NextELIsAarch64 + SCR_EL3::NS::NonSecure);

    // Set Saved Program Status Register (EL3)
    // Set up a simulated exception return.
    //
    // Fake a saved program status, where all interrupts were
    // masked and SP_EL1 was used as a stack pointer.
    SPSR_EL3.write(
        SPSR_EL3::D::Masked
            + SPSR_EL3::A::Masked
            + SPSR_EL3::I::Masked
            + SPSR_EL3::F::Masked
            + SPSR_EL3::M::EL1h, // Use SP_EL1
    );

    // Make the Exception Link Register (EL3) point to reset().
    ELR_EL3.set(reset as *const () as u64);

    shared_setup_and_enter_post()
}

fn reset() -> ! {
    unsafe extern "Rust" {
        fn main() -> !;
    }

    // SAFETY: We're getting to more safety right here!
    unsafe { main() }
}
