/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 *
 * Based on ideas from Jorge Aparicio, Andre Richter, Phil Oppenheimer, Sergio Benitez.
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Low-level boot of the Raspberry's processor
//! <http://infocenter.arm.com/help/topic/com.arm.doc.dai0527a/DAI0527A_baremetal_boot_code_for_ARMv8_A_processors.pdf>

//! Raspi kernel boot helper: https://github.com/raspberrypi/tools/blob/master/armstubs/armstub8.S
//! In particular, see dtb_ptr32

//! To get memory size from DTB:
//! 1. Find nodes with unit-names `/memory`
//! 2. From those read reg entries, using `/#address-cells` and `/#size-cells` as units
//! 3. Union of all these reg entries will be the available memory. Enter it as mem-regions.

use {
    crate::endless_sleep,
    cortex_a::{asm, registers::*},
    tock_registers::interfaces::{Readable, Writeable},
};

// Stack placed before first executable instruction
const STACK_START: u64 = 0x0008_0000; // Keep in sync with linker script

/// Type check the user-supplied entry function.
#[macro_export]
macro_rules! entry {
    ($path:path) => {
        /// # Safety
        /// Only type-checks!
        #[export_name = "main"]
        pub unsafe fn __main(dtb: u32) -> ! {
            // type check the given path
            let f: fn(u32) -> ! = $path;

            f(dtb)
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
unsafe fn reset(dtb: u32) -> ! {
    extern "C" {
        // Boundaries of the .bss section, provided by the linker script
        static mut __BSS_START: u64;
        static mut __BSS_END: u64;
    }

    // Zeroes the .bss section
    r0::zero_bss(&mut __BSS_START, &mut __BSS_END);

    extern "Rust" {
        fn main(dtb: u32) -> !;
    }

    main(dtb)
}

// [ARMv6 unaligned data access restrictions](https://developer.arm.com/documentation/ddi0333/h/unaligned-and-mixed-endian-data-access-support/unaligned-access-support/armv6-unaligned-data-access-restrictions?lang=en)
// dictates that compatibility bit U in CP15 must be set to 1 to allow Unaligned accesses while MMU is off.
// (In addition to SCTLR_EL1.A being 0)
// See also [CP15 C1 docs](https://developer.arm.com/documentation/ddi0290/g/system-control-coprocessor/system-control-processor-registers/c1--control-register).
// #[link_section = ".text.boot"]
// #[inline]
// fn enable_armv6_unaligned_access() {
//     unsafe {
//         asm!(
//             "mrc p15, 0, {u}, c1, c0, 0",
//             "or {u}, {u}, {CR_U}",
//             "mcr p15, 0, {u}, c1, c0, 0",
//             u = out(reg) _,
//             CR_U = const 1 << 22
//         );
//     }
// }

#[link_section = ".text.boot"]
#[inline]
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
}

#[link_section = ".text.boot"]
#[inline]
fn shared_setup_and_enter_post(dtb: u32) -> ! {
    // Set up SP_EL1 (stack pointer), which will be used by EL1 once
    // we "return" to it.
    SP_EL1.set(STACK_START);

    unsafe {
        asm!("mov {dtb:w}, w0", dtb = in(reg) dtb);
        // @todo How to enforce dtb being in w0 at this point? -- must be an arg to eret()
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
fn setup_and_enter_el1_from_el2(dtb: u32) -> ! {
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

    shared_setup_and_enter_post(dtb)
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
fn setup_and_enter_el1_from_el3(dtb: u32) -> ! {
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

    shared_setup_and_enter_post(dtb)
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
pub unsafe extern "C" fn _boot_cores(dtb: u32) -> ! {
    const CORE_0: u64 = 0;
    const CORE_MASK: u64 = 0x3;
    // Can't match values with dots in match, so use intermediate consts.
    #[cfg(qemu)]
    const EL3: u64 = CurrentEL::EL::EL3.value;
    const EL2: u64 = CurrentEL::EL::EL2.value;
    const EL1: u64 = CurrentEL::EL::EL1.value;

    // Set stack pointer. Used in case we started in EL1.
    SP.set(STACK_START);

    shared_setup_and_enter_pre();

    if CORE_0 == MPIDR_EL1.get() & CORE_MASK {
        // @todo On entry, w0 should contain the dtb address.
        // For non-primary cores it however contains 0.

        match CurrentEL.get() {
            #[cfg(qemu)]
            EL3 => setup_and_enter_el1_from_el3(dtb),
            EL2 => setup_and_enter_el1_from_el2(dtb),
            EL1 => reset(dtb),
            _ => endless_sleep(),
        }
    }

    // if not core0 or not EL3/EL2/EL1, infinitely wait for events
    endless_sleep()
}
