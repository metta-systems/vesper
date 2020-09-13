/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 *
 * Based on ideas from Jorge Aparicio, Andre Richter, Phil Oppenheimer.
 * Copyright (c) 2019 Berkus Decker <berkus+vesper@metta.systems>
 */

#![deny(missing_docs)]
#![deny(warnings)]

use {
    crate::endless_sleep,
    cortex_a::{asm, regs::*},
};

/// Low-level boot of the Raspberry's processor
/// http://infocenter.arm.com/help/topic/com.arm.doc.dai0527a/DAI0527A_baremetal_boot_code_for_ARMv8_A_processors.pdf

/// Type check the user-supplied entry function.
#[macro_export]
macro_rules! entry {
    ($path:path) => {
        /// # Safety
        /// Only type-checks!
        #[export_name = "main"]
        pub unsafe fn __main() -> ! {
            // type check the given path
            let f: fn() -> ! = $path;

            f()
        }
    };
}

/// Reset function.
///
/// Initializes the bss section before calling into the user's `main()`.
///
/// # Safety
///
/// Totally unsafe! We're in the hardware land.
#[link_section = ".text.boot"]
unsafe fn reset() -> ! {
    extern "C" {
        // Boundaries of the .bss section, provided by the linker script
        static mut __BSS_START: u64;
        static mut __BSS_END: u64;
    }

    // Set stack pointer. Used in case we started in EL1.
    const STACK_START: u64 = 0x80_000;
    SP.set(STACK_START);

    // Zeroes the .bss section
    r0::zero_bss(&mut __BSS_START, &mut __BSS_END);

    extern "Rust" {
        fn main() -> !;
    }

    main()
}

/// Real hardware boot-up sequence.
///
/// Prepare and execute transition from EL2 to EL1.
#[link_section = ".text.boot"]
#[inline]
fn setup_and_enter_el1_from_el2() -> ! {
    // Enable timer counter registers for EL1
    CNTHCTL_EL2.write(CNTHCTL_EL2::EL1PCEN::SET + CNTHCTL_EL2::EL1PCTEN::SET);

    // No virtual offset for reading the counters
    CNTVOFF_EL2.set(0);

    // Set EL1 execution state to AArch64
    // @todo Explain the SWIO bit (SWIO hardwired on Pi3)
    HCR_EL2.write(HCR_EL2::RW::EL1IsAarch64 + HCR_EL2::SWIO::SET);

    // Set up a simulated exception return.
    //
    // First, fake a saved program status, where all interrupts were
    // masked and SP_EL1 was used as a stack pointer.
    SPSR_EL2.write(
        SPSR_EL2::D::Masked
            + SPSR_EL2::A::Masked
            + SPSR_EL2::I::Masked
            + SPSR_EL2::F::Masked
            + SPSR_EL2::M::EL1h, // Use SP_EL1
    );

    // Second, let the link register point to reset().
    ELR_EL2.set(reset as *const () as u64);

    // Set up SP_EL1 (stack pointer), which will be used by EL1 once
    // we "return" to it.
    const STACK_START: u64 = 0x80_000;
    SP_EL1.set(STACK_START);

    // Use `eret` to "return" to EL1. This will result in execution of
    // `reset()` in EL1.
    asm::eret()
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
#[cfg(qemu)]
#[link_section = ".text.boot"]
#[inline]
fn setup_and_enter_el1_from_el3() -> ! {
    use crate::arch::aarch64::regs::{ELR_EL3, SCR_EL3, SPSR_EL3};

    // Enable timer counter registers for EL1
    CNTHCTL_EL2.write(CNTHCTL_EL2::EL1PCEN::SET + CNTHCTL_EL2::EL1PCTEN::SET);

    // No virtual offset for reading the counters
    CNTVOFF_EL2.set(0);

    // Set System Control Register (EL1)
    // Make memory non-cacheable and disable MMU mapping.
    SCTLR_EL1
        .write(SCTLR_EL1::I::NonCacheable + SCTLR_EL1::C::NonCacheable + SCTLR_EL1::M::Disable);

    // Set Hypervisor Configuration Register (EL2)
    // Set EL1 execution state to AArch64
    // TODO: Explain the SWIO bit (SWIO hardwired on Pi3)
    HCR_EL2.write(HCR_EL2::RW::EL1IsAarch64 + HCR_EL2::SWIO::SET);

    {
        use crate::register::cpu::RegisterReadWrite;

        // Set Secure Configuration Register (EL3)
        SCR_EL3.write(SCR_EL3::RW::NextELIsAarch64 + SCR_EL3::NS::NonSecure);

        // Set Saved Program Status Register (EL3)
        // Set up a simulated exception return.
        //
        // First, fake a saved program status, where all interrupts were
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
    }

    // Set up SP_EL1 (stack pointer), which will be used by EL1 once
    // we "return" to it.
    const STACK_START: u64 = 0x80_000;
    SP_EL1.set(STACK_START);

    // Use `eret` to "return" to EL1. This will result in execution of
    // `reset()` in EL1.
    asm::eret()
}

/// Entrypoint of the processor.
///
/// Parks all cores except core0 and checks if we started in EL2/EL3. If
/// so, proceeds with setting up EL1.
///
/// This is invoked from the linker script, does arch-specific init
/// and passes control to the kernel boot function reset().
///
/// Dissection of various RPi core boot stubs is available
/// [here](https://leiradel.github.io/2019/01/20/Raspberry-Pi-Stubs.html).
///
#[no_mangle]
#[link_section = ".text.boot.entry"]
pub unsafe extern "C" fn _boot_cores() -> ! {
    const CORE_0: u64 = 0;
    const CORE_MASK: u64 = 0x3;
    // Can't match values with dots in match, so use intermediate consts.
    #[cfg(qemu)]
    const EL3: u32 = CurrentEL::EL::EL3.value;
    const EL2: u32 = CurrentEL::EL::EL2.value;
    const EL1: u32 = CurrentEL::EL::EL1.value;

    if CORE_0 == MPIDR_EL1.get() & CORE_MASK {
        match CurrentEL.get() {
            #[cfg(qemu)]
            EL3 => setup_and_enter_el1_from_el3(),
            EL2 => setup_and_enter_el1_from_el2(),
            EL1 => reset(),
            _ => endless_sleep(),
        }
    }

    // if not core0 or not EL3/EL2/EL1, infinitely wait for events
    endless_sleep()
}
