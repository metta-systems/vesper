/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 *
 * Based on ideas from Jorge Aparicio, Andre Richter, Phil Oppenheimer, Sergio Benitez.
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Low-level boot of the Raspberry's processor
//! <http://infocenter.arm.com/help/topic/com.arm.doc.dai0527a/DAI0527A_baremetal_boot_code_for_ARMv8_A_processors.pdf>

use {
    crate::endless_sleep,
    aarch64_cpu::{asm, registers::*},
    core::{
        cell::UnsafeCell,
        sync::atomic::{self, Ordering},
    },
    tock_registers::interfaces::{Readable, Writeable},
};

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
/// # Safety
///
/// Totally unsafe! We're in the hardware land.
/// We assume that no statics are accessed before transition to main from reset() function.
#[no_mangle]
#[link_section = ".text.main.entry"]
pub unsafe extern "C" fn _boot_cores() -> ! {
    const CORE_0: u64 = 0;
    const CORE_MASK: u64 = 0x3;
    // Can't match values with dots in match, so use intermediate consts.
    #[cfg(qemu)]
    const EL3: u64 = CurrentEL::EL::EL3.value;
    const EL2: u64 = CurrentEL::EL::EL2.value;
    const EL1: u64 = CurrentEL::EL::EL1.value;

    extern "Rust" {
        // Stack top
        // Stack placed before first executable instruction
        static __STACK_START: UnsafeCell<()>;
    }
    // Set stack pointer. Used in case we started in EL1.
    SP.set(__STACK_START.get() as u64);

    shared_setup_and_enter_pre();

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

#[link_section = ".text.boot"]
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

#[link_section = ".text.boot"]
#[inline]
fn shared_setup_and_enter_post() -> ! {
    extern "Rust" {
        // Stack top
        static __STACK_START: UnsafeCell<()>;
    }
    // Set up SP_EL1 (stack pointer), which will be used by EL1 once
    // we "return" to it.
    unsafe {
        SP_EL1.set(__STACK_START.get() as u64);
    }

    // Use `eret` to "return" to EL1. This will result in execution of
    // `reset()` in EL1.
    asm::eret()
}

/// Real hardware boot-up sequence.
///
/// Prepare and execute transition from EL2 to EL1.
#[link_section = ".text.boot"]
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
#[cfg(qemu)]
#[link_section = ".text.boot"]
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

/// Reset function.
///
/// Initializes the bss section before calling into the user's `main()`.
///
/// # Safety
///
/// Totally unsafe! We're in the hardware land.
/// We assume that no statics are accessed before transition to main from this function.
///
/// We are guaranteed to be in EL1 non-secure mode here.
#[link_section = ".text.boot"]
unsafe fn reset() -> ! {
    extern "Rust" {
        // Boundaries of the .bss section, provided by the linker script.
        static __BSS_START: UnsafeCell<()>;
        static __BSS_SIZE_U64S: UnsafeCell<()>;
    }

    // Zeroes the .bss section
    // Based on https://gist.github.com/skoe/dbd3add2fc3baa600e9ebc995ddf0302 and discussions
    // on pointer provenance in closing r0 issues (https://github.com/rust-embedded/cortex-m-rt/issues/300)

    // NB: https://doc.rust-lang.org/nightly/core/ptr/index.html#provenance
    // Importing pointers like `__BSS_START` and `__BSS_END` and performing pointer
    // arithmetic on them directly may lead to Undefined Behavior, because the
    // compiler may assume they come from different allocations and thus performing
    // undesirable optimizations on them.
    // So we use a painter-and-a-size as described in provenance section.

    let bss = core::slice::from_raw_parts_mut(
        __BSS_START.get() as *mut u64,
        __BSS_SIZE_U64S.get() as usize,
    );
    for i in bss {
        *i = 0;
    }

    // Don't cross this line with loads and stores. The initializations
    // done above could be "invisible" to the compiler, because we write to the
    // same memory location that is used by statics after this point.
    // Additionally, we assume that no statics are accessed before this point.
    atomic::compiler_fence(Ordering::SeqCst);

    extern "Rust" {
        fn main() -> !;
    }

    main()
}
