/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Interrupt handling
//!
//! The base address is given by `VBAR_ELn` and each entry has a defined offset from this
//! base address. Each table has 16 entries, with each entry being 128 bytes (32 instructions)
//! in size. The table effectively consists of 4 sets of 4 entries.
//!
//! Minimal implementation to help catch MMU traps.
//! Reads `ESR_ELx` to understand why trap was taken.
//!
//! `VBAR_EL1`, `VBAR_EL2`, `VBAR_EL3`
//!
//! `CurrentEL` with `SP0`: +0x0
//!
//! * Synchronous
//! * IRQ/vIRQ
//! * FIQ
//! * SError/vSError
//!
//! `CurrentEL` with `SPx`: +0x200
//!
//! * Synchronous
//! * IRQ/vIRQ
//! * FIQ
//! * SError/vSError
//!
//! Lower EL using `AArch64`: +0x400
//!
//! * Synchronous
//! * IRQ/vIRQ
//! * FIQ
//! * SError/vSError
//!
//! Lower EL using `AArch32`: +0x600
//!
//! * Synchronous
//! * IRQ/vIRQ
//! * FIQ
//! * SError/vSError
//!
//! When the processor takes an exception to `AArch64` execution state,
//! all of the PSTATE interrupt masks is set automatically. This means
//! that further exceptions are disabled. If software is to support
//! nested exceptions, for example, to allow a higher priority interrupt
//! to interrupt the handling of a lower priority source, then software needs
//! to explicitly re-enable interrupts

use {
    crate::PrivilegeLevel,
    aarch64_cpu::{asm::barrier, registers::*},
    core::cell::UnsafeCell,
    liblog::info,
    snafu::Snafu,
    tock_registers::interfaces::{Readable, Writeable},
};

mod esr_el1;
mod exception_context;
mod spsr_el1;
mod traps;

pub use exception_context::ExceptionContext;

core::arch::global_asm!(include_str!("vectors.S"));

//--------------------------------------------------------------------------------------------------
// Private Definitions
//--------------------------------------------------------------------------------------------------

//--------------------------------------------------------------------------------------------------
// Private Code
//--------------------------------------------------------------------------------------------------

/// The default exception, invoked for every exception type unless the handler
/// is overridden.
/// Prints verbose information about the exception and then panics.
///
/// Default pointer is configured in the linker script.
#[unsafe(no_mangle)]
extern "C" fn default_exception_handler(exc: &ExceptionContext) {
    panic!(
        "Unexpected CPU Exception!\n\n\
        {}",
        exc
    );
}

//------------------------------------------------------------------------------
// Current, EL0
//------------------------------------------------------------------------------

#[unsafe(no_mangle)]
extern "C" fn current_el0_synchronous(_e: &mut ExceptionContext) {
    panic!("Should not be here. Use of SP_EL0 in EL1 is not supported.")
}

#[unsafe(no_mangle)]
extern "C" fn current_el0_irq(_e: &mut ExceptionContext) {
    panic!("Should not be here. Use of SP_EL0 in EL1 is not supported.")
}

#[unsafe(no_mangle)]
extern "C" fn current_el0_serror(_e: &mut ExceptionContext) {
    panic!("Should not be here. Use of SP_EL0 in EL1 is not supported.")
}

//------------------------------------------------------------------------------
// Current, ELx
//------------------------------------------------------------------------------

#[unsafe(no_mangle)]
extern "C" fn current_elx_synchronous(e: &mut ExceptionContext) {
    #[cfg(any(test, feature = "test_build"))]
    {
        const TEST_SVC_ID: u64 = 0x1337;

        let esr_el1 = esr_el1::EsrEL1(LocalRegisterCopy::new(ESR_EL1.get()));

        if let Some(ESR_EL1::EC::Value::SVC64) = esr_el1.exception_class()
            && esr_el1.iss() == TEST_SVC_ID
        {
            liblog::println!("Serving syscall {TEST_SVC_ID}");
            return;
        }
    }

    if traps::synchronous_common(e) {
        return;
    }

    default_exception_handler(e);
}

#[unsafe(no_mangle)]
extern "C" fn current_elx_irq(_e: &mut ExceptionContext) {
    // -- @todo
    // let token = unsafe { &exception::asynchronous::IRQContext::new() };
    // exception::asynchronous::irq_manager().handle_pending_irqs(token);
}

#[unsafe(no_mangle)]
extern "C" fn current_elx_serror(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

//------------------------------------------------------------------------------
// Lower, AArch64
//------------------------------------------------------------------------------

#[unsafe(no_mangle)]
extern "C" fn lower_aarch64_synchronous(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

#[unsafe(no_mangle)]
extern "C" fn lower_aarch64_irq(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

#[unsafe(no_mangle)]
extern "C" fn lower_aarch64_serror(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

//------------------------------------------------------------------------------
// Lower, AArch32
//------------------------------------------------------------------------------

#[unsafe(no_mangle)]
extern "C" fn lower_aarch32_synchronous(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

#[unsafe(no_mangle)]
extern "C" fn lower_aarch32_irq(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

#[unsafe(no_mangle)]
extern "C" fn lower_aarch32_serror(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

/// The processor's current privilege level.
pub fn current_privilege_level() -> (PrivilegeLevel, &'static str) {
    let el = CurrentEL.read_as_enum(CurrentEL::EL);
    match el {
        Some(CurrentEL::EL::Value::EL3) => (PrivilegeLevel::Unknown, "EL3"),
        Some(CurrentEL::EL::Value::EL2) => (PrivilegeLevel::Hypervisor, "EL2"),
        Some(CurrentEL::EL::Value::EL1) => (PrivilegeLevel::Kernel, "EL1"),
        Some(CurrentEL::EL::Value::EL0) => (PrivilegeLevel::User, "EL0"),
        _ => (PrivilegeLevel::Unknown, "Unknown"),
    }
}

/// Init exception handling by setting the exception vector base address register.
///
/// - Changes the HW state of the executing core.
/// - The vector table and the symbol `__EXCEPTION_VECTORS_START` from the linker script must
///   adhere to the alignment and size constraints demanded by the ARMv8-A Architecture Reference
///   Manual.
pub fn handling_init() {
    unsafe extern "Rust" {
        static __EXCEPTION_VECTORS_START: UnsafeCell<()>;
    }

    // Safety: __EXCEPTION_VECTORS_START is a valid pointer to exception vector table
    // that is properly aligned and accessible during system initialization
    unsafe {
        set_vbar_el1_checked(__EXCEPTION_VECTORS_START.get() as u64)
            .expect("Vector table properly aligned!");
    }
    info!("[!] Exception traps set up");
}

/// Errors possibly returned from the traps module.
/// @todo a big over-engineered here.
#[derive(Debug, Snafu)]
enum Error {
    /// IVT address is unaligned.
    #[snafu(display("Unaligned base address for interrupt vector table"))]
    Unaligned,
}

/// Configure base address of interrupt vectors table.
/// Checks that address is properly 2KiB aligned.
///
/// # Safety
///
/// Totally unsafe in the land of the hardware.
unsafe fn set_vbar_el1_checked(vec_base_addr: u64) -> Result<(), Error> {
    if vec_base_addr.trailing_zeros() < 11 {
        return Err(Error::Unaligned);
    }

    VBAR_EL1.set(vec_base_addr);

    // Force VBAR update to complete before next instruction.
    barrier::isb(barrier::SY);

    Ok(())
}
