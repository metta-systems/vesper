#![no_std]

//! What `fp-test` and its `fp-worker` components share: where the init and
//! report pages are, what they hold, the worker roles and the check codes.
//!
//! Every check fills the V registers with sentinels derived from a seed
//! (`v<n>` holds `seed + n` in its low half and the complement in its high
//! half), sets FPCR and FPSR, and later compares. A check returns
//! [`CHECK_PASSED`] or the code of the first thing that differed.

use core::sync::atomic::AtomicU64;

/// A worker's init page (written by the builder before it runs).
pub const INIT_VA: u64 = 0x2000_0000;
/// A worker's report page, right after the init page; the kernel reads it
/// through its direct map.
pub const REPORT_VA: u64 = INIT_VA + 0x1000;
/// The 2 MiB span holding a component's guarded stacks.
pub const STACK_REGION: u64 = 0x3000_0000;

/// A worker's argument: fill the V registers once, then check them in a
/// loop forever, counting passes. It never makes a system call, so it only
/// stops when it is preempted.
pub const ROLE_SPIN: u64 = 0;
/// A worker's argument: run the Call and fault scenarios, report them, then
/// spin.
pub const ROLE_SCENARIOS: u64 = 1;

/// How a role's seed differs from the instance seed, so Threads of one
/// `AddressSpace` hold different sentinels.
pub const ROLE_SEED_STRIDE: u64 = 0x0100_0000_0000_0005;

/// What the builder hands a worker.
#[repr(C)]
pub struct FpInit {
    /// The worker table's guard and capacity exponent, for its export
    /// adapters' `Thread.Return` key.
    pub guard: u64,
    pub size_bits: u64,
    /// This instance's seed.
    pub seed: u64,
    /// The Invocation of the other FP worker's `zero_check_entry` (wire
    /// form; the scenario worker only).
    pub call_fp: u64,
    /// The Invocation of the integer-only `fp-probe`'s `probe_call_entry`
    /// (wire form; the scenario worker only).
    pub call_integer: u64,
}

/// What a worker reports. Spinners write the first two words; the scenario
/// Thread writes the rest, then [`done`](Self::done) last.
#[repr(C)]
pub struct FpReport {
    /// Check passes the spinner completed.
    pub spin_passes: AtomicU64,
    /// The spinner's failing check code, or zero while it passes.
    pub spin_failure: AtomicU64,
    /// The caller's check after a Call into the other FP worker.
    pub call_fp: AtomicU64,
    /// What the FP callee saw at entry: [`CHECK_PASSED`] when everything was
    /// zero.
    pub call_fp_callee: AtomicU64,
    /// The caller's check after a Call into the integer-only `fp-probe`.
    pub call_integer: AtomicU64,
    /// What `fp-probe` reported: its FP/SIMD trap bit.
    pub call_integer_callee: AtomicU64,
    /// The check after a handled fault resumed.
    pub fault: AtomicU64,
    /// What the fault handler saw at entry: [`CHECK_PASSED`] when everything
    /// was zero.
    pub fault_handler: AtomicU64,
    /// The syndrome the fault handler received.
    pub fault_syndrome: AtomicU64,
    /// 1 once the scenario results above are written.
    pub done: AtomicU64,
}

/// A check found everything as expected.
pub const CHECK_PASSED: u64 = 0;
/// Check codes 1 to 32 name `v0` to `v31` (the code is the register number
/// plus one).
pub const CHECK_FAILED_V0: u64 = 1;
/// FPCR differed.
pub const CHECK_FAILED_FPCR: u64 = 33;
/// FPSR differed.
pub const CHECK_FAILED_FPSR: u64 = 34;
/// The Call itself failed.
pub const CHECK_FAILED_CALL: u64 = 35;

/// The worker's init page.
///
/// # Safety
///
/// Only in a worker, whose builder maps and fills an [`FpInit`] at
/// [`INIT_VA`] before it runs and never changes it afterwards.
pub unsafe fn fp_init() -> &'static FpInit {
    // SAFETY: forwarded caller contract.
    unsafe { &*(INIT_VA as *const FpInit) }
}

/// The worker's report page.
///
/// # Safety
///
/// Only in a worker, whose builder maps a zeroed page at [`REPORT_VA`]
/// before it runs.
pub unsafe fn fp_report() -> &'static FpReport {
    // SAFETY: forwarded caller contract; every field is atomic.
    unsafe { &*(REPORT_VA as *const FpReport) }
}
