//! The monotonic clock, read without a system call.
//!
//! EL0 reads the virtual counter, which the nucleus leaves enabled for EL0
//! (`CNTKCTL_EL1.EL0VCTEN`) with a zero virtual offset, so it equals the
//! physical counter the nucleus keeps time with. A deadline given to
//! `Scheduler.ArmTimer` is in the same nanoseconds.

use core::arch::asm;

const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// The counter frequency, in Hz.
pub fn frequency() -> u64 {
    let frequency: u64;
    // SAFETY: reading `CNTFRQ_EL0` has no side effects and is accessible at
    // EL0.
    unsafe {
        asm!("mrs {}, cntfrq_el0", out(reg) frequency, options(nomem, nostack));
    }
    frequency.max(1)
}

/// The virtual counter, in ticks, read in program order.
pub fn ticks() -> u64 {
    let ticks: u64;
    // SAFETY: the barrier orders the read after earlier instructions;
    // `CNTVCT_EL0` is readable at EL0 (see the module docs) and has no side
    // effects.
    unsafe {
        asm!(
            "isb",
            "mrs {}, cntvct_el0",
            out(reg) ticks,
            options(nomem, nostack, preserves_flags)
        );
    }
    ticks
}

/// Nanoseconds since the counter started.
pub fn now_ns() -> u64 {
    let nanoseconds = u128::from(ticks()) * NANOS_PER_SECOND / u128::from(frequency());
    u64::try_from(nanoseconds).unwrap_or(u64::MAX)
}
