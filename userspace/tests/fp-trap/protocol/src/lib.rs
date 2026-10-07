#![no_std]

//! The FP/SIMD trap probe shared by `fp-trap-test`'s `EL1t` boot Thread and
//! its EL0 `fp-probe` component, so both levels run the same instruction and
//! judge the outcome the same way.
//!
//! The probe loads a sentinel into `x0` and executes `fmov x0, d0`. Under the
//! integer-only policy the instruction traps (`ESR_EL1.EC` 0x07); the
//! nucleus's `fp_trap_test` hook hands the syndrome back in `x0` and resumes
//! after the instruction. So afterwards `x0` holds:
//!
//! - an FP/SIMD-access syndrome — trapped and classified as an execution fault;
//! - anything else — the instruction executed (FP/SIMD was enabled) or the
//!   trap was misclassified.

use {aarch64_cpu::registers::ESR_EL1, tock_registers::LocalRegisterCopy};

/// Notification bit the EL0 probe signals when its instruction trapped.
pub const TRAPPED_BIT: u64 = 0b01;
/// Notification bit the EL0 probe signals when its instruction did not trap.
pub const NOT_TRAPPED_BIT: u64 = 0b10;

/// `x0` before the probe instruction: not an FP/SIMD-access syndrome.
const SENTINEL: u64 = 0x5E17_1E15_0000_0000;

/// Execute one FP/SIMD instruction and return the resulting `x0`.
///
/// Without the nucleus's `fp_trap_test` hook the trap halts the kernel.
#[inline(never)]
pub fn probe() -> u64 {
    let result: u64;
    // SAFETY: `fmov x0, d0` only reads `d0` and writes `x0`; it touches no
    // memory or stack. The `.arch_extension` lets the assembler accept the FP
    // instruction in this integer-only build — executing it is the point.
    unsafe {
        core::arch::asm!(
            ".arch_extension fp",
            "fmov x0, d0",
            inout("x0") SENTINEL => result,
            options(nomem, nostack),
        );
    }
    result
}

/// Whether a [`probe`] result is the syndrome of an FP/SIMD access trap.
pub fn trapped(result: u64) -> bool {
    LocalRegisterCopy::<u64, ESR_EL1::Register>::new(result).matches_all(ESR_EL1::EC::TrappedFP)
}
