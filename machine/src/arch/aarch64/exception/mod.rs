/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
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
    crate::{
        exception::{self, PrivilegeLevel},
        info,
    },
    aarch64_cpu::{asm::barrier, registers::*},
    core::{cell::UnsafeCell, fmt},
    snafu::Snafu,
    tock_registers::{
        interfaces::{Readable, Writeable},
        registers::InMemoryRegister,
    },
};

pub mod asynchronous;

core::arch::global_asm!(include_str!("vectors.S"));

//--------------------------------------------------------------------------------------------------
// Private Definitions
//--------------------------------------------------------------------------------------------------

/// Wrapper structs for memory copies of registers.
#[repr(transparent)]
struct SpsrEL1(InMemoryRegister<u64, SPSR_EL1::Register>);
struct EsrEL1(InMemoryRegister<u64, ESR_EL1::Register>);

/// The exception context as it is stored on the stack on exception entry.
#[repr(C)]
struct ExceptionContext {
    /// General Purpose Registers, x0-x29
    gpr: [u64; 30],

    /// The link register, aka x30.
    lr: u64,

    /// Exception link register. The program counter at the time the exception happened.
    elr_el1: u64,

    /// Saved program status.
    spsr_el1: SpsrEL1,

    /// Exception syndrome register.
    esr_el1: EsrEL1,
}

//--------------------------------------------------------------------------------------------------
// Private Code
//--------------------------------------------------------------------------------------------------

/// The default exception, invoked for every exception type unless the handler
/// is overridden.
/// Prints verbose information about the exception and then panics.
///
/// Default pointer is configured in the linker script.
fn default_exception_handler(exc: &ExceptionContext) {
    panic!(
        "Unexpected CPU Exception!\n\n\
        {}",
        exc
    );
}

//------------------------------------------------------------------------------
// Current, EL0
//------------------------------------------------------------------------------

#[no_mangle]
extern "C" fn current_el0_synchronous(_e: &mut ExceptionContext) {
    panic!("Should not be here. Use of SP_EL0 in EL1 is not supported.")
}

#[no_mangle]
extern "C" fn current_el0_irq(_e: &mut ExceptionContext) {
    panic!("Should not be here. Use of SP_EL0 in EL1 is not supported.")
}

#[no_mangle]
extern "C" fn current_el0_serror(_e: &mut ExceptionContext) {
    panic!("Should not be here. Use of SP_EL0 in EL1 is not supported.")
}

//------------------------------------------------------------------------------
// Current, ELx
//------------------------------------------------------------------------------

#[no_mangle]
extern "C" fn current_elx_synchronous(e: &mut ExceptionContext) {
    #[cfg(feature = "test_build")]
    {
        const TEST_SVC_ID: u64 = 0x1337;

        if let Some(ESR_EL1::EC::Value::SVC64) = e.esr_el1.exception_class() {
            if e.esr_el1.iss() == TEST_SVC_ID {
                return;
            }
        }
    }

    default_exception_handler(e);
}

#[no_mangle]
extern "C" fn current_elx_irq(_e: &mut ExceptionContext) {
    let token = unsafe { &exception::asynchronous::IRQContext::new() };
    exception::asynchronous::irq_manager().handle_pending_irqs(token);
}

#[no_mangle]
extern "C" fn current_elx_serror(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

//------------------------------------------------------------------------------
// Lower, AArch64
//------------------------------------------------------------------------------

#[no_mangle]
extern "C" fn lower_aarch64_synchronous(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

#[no_mangle]
extern "C" fn lower_aarch64_irq(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

#[no_mangle]
extern "C" fn lower_aarch64_serror(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

//------------------------------------------------------------------------------
// Lower, AArch32
//------------------------------------------------------------------------------

#[no_mangle]
extern "C" fn lower_aarch32_synchronous(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

#[no_mangle]
extern "C" fn lower_aarch32_irq(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

#[no_mangle]
extern "C" fn lower_aarch32_serror(e: &mut ExceptionContext) {
    default_exception_handler(e);
}

//------------------------------------------------------------------------------
// Misc
//------------------------------------------------------------------------------

/// Human readable SPSR_EL1.
#[rustfmt::skip]
impl fmt::Display for SpsrEL1 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Raw value.
        writeln!(f, "SPSR_EL1: {:#010x}", self.0.get())?;

        let to_flag_str = |x| -> _ {
            if x { "Set" } else { "Not set" }
        };

        writeln!(f, "      Flags:")?;
        writeln!(f, "            Negative (N): {}", to_flag_str(self.0.is_set(SPSR_EL1::N)))?;
        writeln!(f, "            Zero     (Z): {}", to_flag_str(self.0.is_set(SPSR_EL1::Z)))?;
        writeln!(f, "            Carry    (C): {}", to_flag_str(self.0.is_set(SPSR_EL1::C)))?;
        writeln!(f, "            Overflow (V): {}", to_flag_str(self.0.is_set(SPSR_EL1::V)))?;

        let to_mask_str = |x| -> _ {
            if x { "Masked" } else { "Unmasked" }
        };

        writeln!(f, "      Exception handling state:")?;
        writeln!(f, "            Debug  (D): {}", to_mask_str(self.0.is_set(SPSR_EL1::D)))?;
        writeln!(f, "            SError (A): {}", to_mask_str(self.0.is_set(SPSR_EL1::A)))?;
        writeln!(f, "            IRQ    (I): {}", to_mask_str(self.0.is_set(SPSR_EL1::I)))?;
        writeln!(f, "            FIQ    (F): {}", to_mask_str(self.0.is_set(SPSR_EL1::F)))?;

        write!(f, "      Illegal Execution State (IL): {}",
               to_flag_str(self.0.is_set(SPSR_EL1::IL))
        )
    }
}

impl EsrEL1 {
    #[inline(always)]
    fn exception_class(&self) -> Option<ESR_EL1::EC::Value> {
        self.0.read_as_enum(ESR_EL1::EC)
    }

    #[cfg(feature = "test_build")]
    #[inline(always)]
    fn iss(&self) -> u64 {
        self.0.read(ESR_EL1::ISS)
    }
}

/// Human readable ESR_EL1.
#[rustfmt::skip]
impl fmt::Display for EsrEL1 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Raw print of whole register.
        writeln!(f, "ESR_EL1: {:#010x}", self.0.get())?;

        // Raw print of exception class.
        write!(f, "      Exception Class         (EC) : {:#x}", self.0.read(ESR_EL1::EC))?;

        // Exception class.
        let ec_translation = match self.exception_class() {
            Some(ESR_EL1::EC::Value::DataAbortCurrentEL) => "Data Abort, current EL",
            _ => "N/A",
        };
        writeln!(f, " - {}", ec_translation)?;

        // Raw print of instruction specific syndrome.
        write!(f, "      Instr Specific Syndrome (ISS): {:#x}", self.0.read(ESR_EL1::ISS))
    }
}

impl ExceptionContext {
    #[inline(always)]
    fn exception_class(&self) -> Option<ESR_EL1::EC::Value> {
        self.esr_el1.exception_class()
    }

    #[inline(always)]
    fn fault_address_valid(&self) -> bool {
        use ESR_EL1::EC::Value::*;

        match self.exception_class() {
            None => false,
            Some(ec) => matches!(
                ec,
                InstrAbortLowerEL
                    | InstrAbortCurrentEL
                    | PCAlignmentFault
                    | DataAbortLowerEL
                    | DataAbortCurrentEL
                    | WatchpointLowerEL
                    | WatchpointCurrentEL
            ),
        }
    }
}

/// Human readable print of the exception context.
impl fmt::Display for ExceptionContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{}", self.esr_el1)?;

        if self.fault_address_valid() {
            writeln!(f, "FAR_EL1: {:#018x}", FAR_EL1.get() as usize)?;
        }

        writeln!(f, "{}", self.spsr_el1)?;
        writeln!(f, "ELR_EL1: {:#018x}", self.elr_el1)?;
        writeln!(f)?;
        writeln!(f, "General purpose register:")?;

        let alternating = |x| -> _ {
            if x % 2 == 0 {
                "   "
            } else {
                "\n"
            }
        };

        // Print two registers per line.
        for (i, reg) in self.gpr.iter().enumerate() {
            write!(f, "      x{: <2}: {: >#018x}{}", i, reg, alternating(i))?;
        }
        write!(f, "      lr : {:#018x}", self.lr)
    }
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
/// # Safety
///
/// - Changes the HW state of the executing core.
/// - The vector table and the symbol `__EXCEPTION_VECTORS_START` from the linker script must
///   adhere to the alignment and size constraints demanded by the ARMv8-A Architecture Reference
///   Manual.
pub fn handling_init() {
    // Provided by vectors.S.
    extern "Rust" {
        static __EXCEPTION_VECTORS_START: UnsafeCell<()>;
    }

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
