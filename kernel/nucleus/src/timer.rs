//! The kernel-owned preemption timer: the EL1 non-secure physical timer.
//!
//! The physical counter and timers belong to the nucleus (EL0 sees only the
//! virtual counter, see kickstart's `CNTKCTL_EL1` setup). The tick is periodic
//! for now; tickless deadline programming comes with the Time objects.

use {
    aarch64_cpu::{
        asm::barrier,
        registers::{CNTFRQ_EL0, CNTP_CTL_EL0, CNTP_CVAL_EL0, CNTPCT_EL0, Readable, Writeable},
    },
    core::time::Duration,
};

/// Length of one time slice.
pub const TICK: Duration = Duration::from_millis(10);

const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// The counter frequency, in Hz.
fn frequency() -> u64 {
    CNTFRQ_EL0.get().max(1)
}

/// The physical counter, read in program order.
fn counter() -> u64 {
    barrier::isb(barrier::SY);
    CNTPCT_EL0.get()
}

/// Nanoseconds since the counter started.
pub fn now_ns() -> u64 {
    let nanoseconds = u128::from(counter()) * NANOS_PER_SECOND / u128::from(frequency());
    u64::try_from(nanoseconds).unwrap_or(u64::MAX)
}

/// Counter ticks in `duration` (at least 1).
fn counter_ticks(duration: Duration) -> u64 {
    let ticks = duration.as_nanos() * u128::from(frequency()) / NANOS_PER_SECOND;
    u64::try_from(ticks).unwrap_or(u64::MAX).max(1)
}

/// Fire the timer one [`TICK`] from now; this also deasserts its interrupt.
pub fn arm_next_tick() {
    CNTP_CVAL_EL0.set(counter().saturating_add(counter_ticks(TICK)));
    CNTP_CTL_EL0.write(CNTP_CTL_EL0::ENABLE::SET + CNTP_CTL_EL0::IMASK::CLEAR);
}
