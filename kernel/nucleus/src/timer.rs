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

/// Counter value at `deadline_ns` on the [`now_ns`] timeline.
fn counter_at(deadline_ns: u64) -> u64 {
    let ticks = u128::from(deadline_ns) * u128::from(frequency()) / NANOS_PER_SECOND;
    u64::try_from(ticks).unwrap_or(u64::MAX)
}

/// Fire the timer once, at `deadline_ns` on the [`now_ns`] timeline.
///
/// A deadline already in the past fires immediately. Arming also deasserts
/// the interrupt, so the handler only has to call this to acknowledge it.
pub fn arm_at(deadline_ns: u64) {
    CNTP_CVAL_EL0.set(counter_at(deadline_ns));
    CNTP_CTL_EL0.write(CNTP_CTL_EL0::ENABLE::SET + CNTP_CTL_EL0::IMASK::CLEAR);
}

/// Fire the timer one [`TICK`] from now; this also deasserts its interrupt.
pub fn arm_next_tick() {
    let tick_ns = u64::try_from(TICK.as_nanos()).unwrap_or(u64::MAX);
    arm_at(now_ns().saturating_add(tick_ns));
}
