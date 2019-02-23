/*
 * MIT License
 *
 * Copyright (c) 2018 Jorge Aparicio
 * Copyright (c) 2018-2019 Andre Richter <andre.o.richter@gmail.com>
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */

#![deny(missing_docs)]
#![deny(warnings)]

//! Low-level boot of the Raspberry's processor
//! http://infocenter.arm.com/help/topic/com.arm.doc.dai0527a/DAI0527A_baremetal_boot_code_for_ARMv8_A_processors.pdf

extern crate panic_abort;

/// Type check the user-supplied entry function.
#[macro_export]
macro_rules! entry {
    ($path:path) => {
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
unsafe fn reset() -> ! {
    extern "C" {
        // Boundaries of the .bss section, provided by the linker script
        static mut __bss_start: u64;
        static mut __bss_end: u64;
    }

    use cortex_a::regs::*;
    const STACK_START: u64 = 0x80_000;
    SP.set(STACK_START);

    // Zeroes the .bss section
    r0::zero_bss(&mut __bss_start, &mut __bss_end);

    extern "Rust" {
        fn main() -> !;
    }

    main()
}

/// Prepare and execute transition from EL2 to EL1.
#[inline]
fn setup_and_enter_el1_from_el2() -> ! {
    use cortex_a::{asm, regs::*};

    const STACK_START: u64 = 0x80_000;

    // Enable timer counter registers for EL1
    CNTHCTL_EL2.write(CNTHCTL_EL2::EL1PCEN::SET + CNTHCTL_EL2::EL1PCTEN::SET);

    // No virtual offset for reading the counters
    CNTVOFF_EL2.set(0);

    // Set EL1 execution state to AArch64
    // TODO: Explain the SWIO bit (SWIO hardwired on Pi3)
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
    SP_EL1.set(STACK_START);

    // Use `eret` to "return" to EL1. This will result in execution of
    // `reset()` in EL1.
    asm::eret()
}

// Processors enter EL3 after reset.
// ref: http://infocenter.arm.com/help/topic/com.arm.doc.dai0527a/DAI0527A_baremetal_boot_code_for_ARMv8_A_processors.pdf
// section: 5.5.1
// However, GPU init code must be switching it down to EL2?

/// Entrypoint of the processor.
///
/// Parks all cores except core0 and checks if we started in EL2. If
/// so, proceeds with setting up EL1.
///
/// This is invoked from the linker script, does arch-specific init
/// and passes control to the kernel boot function reset().
#[link_section = ".text.boot"]
#[no_mangle]
pub unsafe extern "C" fn _boot_cores() -> ! {
    use cortex_a::{asm, regs::*};

    // crate::arch::aarch64::jtag_dbg_wait();

    const CORE_0: u64 = 0;
    const CORE_MASK: u64 = 0x3;
    const EL1: u32 = CurrentEL::EL::EL1.value;
    const EL2: u32 = CurrentEL::EL::EL2.value;

    if CORE_0 == MPIDR_EL1.get() & CORE_MASK {
        if EL2 == CurrentEL.get() {
            setup_and_enter_el1_from_el2()
        } else if EL1 == CurrentEL.get() {
            reset()
        }
    }

    // if not core0 or EL2/EL1, infinitely wait for events
    loop {
        asm::wfe();
    }
}
