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
/// Notification bit the EL0 probe signals when the [kernel probe](kernel_probe)
/// trapped at EL1.
pub const KERNEL_TRAPPED_BIT: u64 = 0b100;
/// Notification bit the EL0 probe signals when the kernel probe did not trap.
pub const KERNEL_NOT_TRAPPED_BIT: u64 = 0b1000;

/// The probe component's argument: run in an integer-only `AddressSpace`,
/// where the probe must trap at EL0.
pub const ROLE_INTEGER: u64 = 0;
/// The probe component's argument: run in an FP `AddressSpace`, where the
/// probe must execute at EL0, then run the [kernel probe](kernel_probe).
pub const ROLE_FP: u64 = 1;

/// The `brk` immediate of [`kernel_probe`].
pub const KERNEL_PROBE_BRK: u16 = 0xF9;

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

/// Ask the nucleus to run [`probe`] at EL1 while it serves this EL0 frame,
/// and return the probe's `x0`.
///
/// Only the nucleus built with `fp_trap_test` answers this `brk` (any other
/// nucleus delivers it as a fault). With `fp_entry_trap` the kernel runs with
/// FP/SIMD trapped, so the probe traps at EL1 even when this frame is an FP
/// frame; without it, entered from an FP frame, the probe executes.
#[inline(never)]
pub fn kernel_probe() -> u64 {
    let result: u64;
    // SAFETY: the `brk` traps to the nucleus, which writes only `x0` and
    // resumes after it; no memory or stack is touched here.
    unsafe {
        core::arch::asm!(
            "brk #{immediate}",
            immediate = const KERNEL_PROBE_BRK,
            inout("x0") SENTINEL => result,
            options(nomem, nostack),
        );
    }
    result
}

/// Whether `syndrome` (an `ESR_EL1` value) is a [`kernel_probe`] request.
pub fn is_kernel_probe(syndrome: u64) -> bool {
    let syndrome = LocalRegisterCopy::<u64, ESR_EL1::Register>::new(syndrome);
    syndrome.matches_all(ESR_EL1::EC::Brk64)
        && syndrome.read(ESR_EL1::ISS) == u64::from(KERNEL_PROBE_BRK)
}

/// Whether `syndrome` (an `ESR_EL1` value) is an FP/SIMD access trap.
pub fn trapped(syndrome: u64) -> bool {
    LocalRegisterCopy::<u64, ESR_EL1::Register>::new(syndrome).matches_all(ESR_EL1::EC::TrappedFP)
}
