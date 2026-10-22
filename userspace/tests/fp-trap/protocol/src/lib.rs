#![no_std]

//! The FP/SIMD trap probe shared by `fp-trap-test`'s `EL1t` boot Thread and
//! its EL0 `fp-probe` component, so both levels run the same instruction and
//! judge the outcome the same way.
//!
//! The probe loads a sentinel into `x0` and executes `fmov x0, d0`. Under the
//! integer-only policy the instruction traps (`ESR_EL1.EC` 0x07):
//!
//! - at `EL1t`, where faults halt the kernel, the nucleus's test-only
//!   `fp_trap_test` hook hands the syndrome back in `x0` and resumes after the
//!   instruction;
//! - at EL0 the fault is delivered to the probe component's own fault handler,
//!   which records the syndrome and resumes after the instruction (skip).
//!
//! Either way the recorded syndrome must be an FP/SIMD access ([`trapped`]);
//! anything else means the instruction executed (FP/SIMD was enabled) or the
//! trap was misclassified.

use {aarch64_cpu::registers::ESR_EL1, tock_registers::LocalRegisterCopy};

/// The EL0 probe's init page (written by the builder before it runs).
pub const INIT_VA: u64 = 0x2000_0000;
/// The 2 MiB span holding the probe's guarded stacks (main and fault handler).
pub const STACK_REGION: u64 = 0x3000_0000;

/// What the builder hands the EL0 probe.
#[repr(C)]
pub struct ProbeInit {
    /// Notification the probe signals with [`TRAPPED_BIT`] or
    /// [`NOT_TRAPPED_BIT`].
    pub result: u64,
    /// The probe table's guard and capacity exponent, for its export adapter's
    /// `Thread.Return` key.
    pub guard: u64,
    pub size_bits: u64,
}

/// The EL0 probe's init page.
///
/// # Safety
///
/// Only in the EL0 probe, whose builder maps and fills a [`ProbeInit`] at
/// [`INIT_VA`] before it runs and never changes it afterwards.
pub unsafe fn probe_init() -> &'static ProbeInit {
    // SAFETY: forwarded caller contract.
    unsafe { &*(INIT_VA as *const ProbeInit) }
}

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

/// Whether `syndrome` (an `ESR_EL1` value) is an FP/SIMD access trap.
pub fn trapped(syndrome: u64) -> bool {
    LocalRegisterCopy::<u64, ESR_EL1::Register>::new(syndrome).matches_all(ESR_EL1::EC::TrappedFP)
}
