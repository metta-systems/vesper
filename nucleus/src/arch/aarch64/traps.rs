/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

//! Interrupt handling
//!
//! The base address is given by VBAR_ELn and each entry has a defined offset from this
//! base address. Each table has 16 entries, with each entry being 128 bytes (32 instructions)
//! in size. The table effectively consists of 4 sets of 4 entries.
//!
//! Minimal implementation to help catch MMU traps.
//! Reads ESR_ELx to understand why trap was taken.
//!
//! VBAR_EL1, VBAR_EL2, VBAR_EL3
//!
//! CurrentEL with SP0: +0x0
//!
//! * Synchronous
//! * IRQ/vIRQ
//! * FIQ
//! * SError/vSError
//!
//! CurrentEL with SPx: +0x200
//!
//! * Synchronous
//! * IRQ/vIRQ
//! * FIQ
//! * SError/vSError
//!
//! Lower EL using AArch64: +0x400
//!
//! * Synchronous
//! * IRQ/vIRQ
//! * FIQ
//! * SError/vSError
//!
//! Lower EL using AArch32: +0x600
//!
//! * Synchronous
//! * IRQ/vIRQ
//! * FIQ
//! * SError/vSError
//!
//! When the processor takes an exception to AArch64 execution state,
//! all of the PSTATE interrupt masks is set automatically. This means
//! that further exceptions are disabled. If software is to support
//! nested exceptions, for example, to allow a higher priority interrupt
//! to interrupt the handling of a lower priority source, then software needs
//! to explicitly re-enable interrupts

use {
    crate::{arch::endless_sleep, println},
    cortex_a::{
        barrier,
        regs::{RegisterReadOnly, RegisterReadWrite, ESR_EL1, FAR_EL1, VBAR_EL1},
    },
};

global_asm!(include_str!("vectors.S"));

/// Configure base address of interrupt vectors table.
/// Checks that address is properly 2KiB aligned.
///
/// # Safety
///
/// Totally unsafe in the land of the hardware.
pub unsafe fn set_vbar_el1_checked(vec_base_addr: u64) -> Result<(), ()> {
    if vec_base_addr.trailing_zeros() < 11 {
        return Err(());
    }

    VBAR_EL1.set(vec_base_addr);

    // Force VBAR update to complete before next instruction.
    barrier::isb(barrier::SY);

    Ok(())
}

/// A blob of general-purpose registers.
#[repr(C)]
pub struct GPR {
    x: [u64; 31],
}

/// Saved exception context.
#[repr(C)]
pub struct ExceptionContext {
    // General Purpose Registers
    gpr: GPR,
    spsr_el1: u64,
    elr_el1: u64,
}

/// The default exception, invoked for every exception type unless the handler
/// is overridden.
/// Default pointer is configured in the linker script.
///
/// # Safety
///
/// Totally unsafe in the land of the hardware.
#[no_mangle]
unsafe extern "C" fn default_exception_handler() -> ! {
    println!("Unexpected exception. Halting CPU.");

    endless_sleep()
}

// To implement an exception handler, override it by defining the respective
// function below.
// Don't forget the #[no_mangle] attribute.
//
/// # Safety
///
/// Totally unsafe in the land of the hardware.
#[no_mangle]
unsafe extern "C" fn current_el0_synchronous(e: &mut ExceptionContext) {
    println!("[!] USER synchronous exception happened.");
    synchronous_common(e)
}
// unsafe extern "C" fn current_el0_irq(e: &mut ExceptionContext);
// unsafe extern "C" fn current_el0_serror(e: &mut ExceptionContext);

/// # Safety
///
/// Totally unsafe in the land of the hardware.
#[no_mangle]
unsafe extern "C" fn current_elx_synchronous(e: &mut ExceptionContext) {
    println!("[!] KERNEL synchronous exception happened.");
    synchronous_common(e);
}

// unsafe extern "C" fn current_elx_irq(e: &mut ExceptionContext);
/// # Safety
///
/// Totally unsafe in the land of the hardware.
#[no_mangle]
unsafe extern "C" fn current_elx_serror(e: &mut ExceptionContext) {
    println!("[!] KERNEL serror exception happened.");
    synchronous_common(e);
    endless_sleep()
}

// unsafe extern "C" fn lower_aarch64_synchronous(e: &mut ExceptionContext);
// unsafe extern "C" fn lower_aarch64_irq(e: &mut ExceptionContext);
// unsafe extern "C" fn lower_aarch64_serror(e: &mut ExceptionContext);

// unsafe extern "C" fn lower_aarch32_synchronous(e: &mut ExceptionContext);
// unsafe extern "C" fn lower_aarch32_irq(e: &mut ExceptionContext);
// unsafe extern "C" fn lower_aarch32_serror(e: &mut ExceptionContext);

/// Helper function to 1) display current exception, 2) skip the offending asm instruction.
/// Not for production use!
fn synchronous_common(e: &mut ExceptionContext) {
    println!("      ESR_EL1: {:#010x} (syndrome)", ESR_EL1.get());
    println!("           EC: {:#06b} (cause)", ESR_EL1.read(ESR_EL1::EC));
    println!("      FAR_EL1: {:#016x} (location)", FAR_EL1.get());
    println!("      ELR_EL1: {:#010x}", e.elr_el1);

    println!(
        "      Incrementing ELR_EL1 by 4 now to continue with the first \
         instruction after the exception!"
    );

    e.elr_el1 += 4;

    println!("      ELR_EL1 modified: {:#010x}", e.elr_el1);
    println!("      Returning from exception...\n");
}
