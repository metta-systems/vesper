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
    crate::PrivilegeLevel, aarch64_cpu::registers::CurrentEL, tock_registers::interfaces::Readable,
};

mod debug;
mod esr_el1;
mod exception_context;
mod spsr_el1;

pub use exception_context::ExceptionContext;

// Helpers for exception debugging
pub use debug::{IssForDataAbort, cause_to_string, exception_dump, iss_dfsc_to_string};

// Exception vectors for syscall handing and general IRQ routing
core::arch::global_asm!(include_str!("vectors.S"));

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
