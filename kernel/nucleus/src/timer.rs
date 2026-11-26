//! The kernel-owned execution timer: the EL1 non-secure physical timer.
//!
//! The physical counter and timers belong to the nucleus (EL0 sees only the
//! virtual counter, see kickstart's `CNTKCTL_EL1` setup). The timer is
//! one-shot: the nucleus computes the next instant anything needs it, the end
//! of the current slice or the exhaustion of the running Thread's budget, and
//! [`program`] sets it, or switches it off when nothing does.

use {
    crate::objects::core_record::{self, NO_DEADLINE},
    aarch64_cpu::{
        asm::barrier,
        registers::{CNTFRQ_EL0, CNTP_CTL_EL0, CNTP_CVAL_EL0, CNTPCT_EL0, Read, Write},
    },
    core::{sync::atomic::Ordering, time::Duration},
};

/// Length of one time slice.
pub const TICK: Duration = Duration::from_millis(10);

const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// The counter frequency, in Hz.
pub fn frequency() -> u64 {
    CNTFRQ_EL0.get().max(1)
}

/// The physical counter, read in program order.
pub fn counter() -> u64 {
    barrier::isb(barrier::SY);
    CNTPCT_EL0.get()
}

/// Nanoseconds since the counter started.
pub fn now_ns() -> u64 {
    let nanoseconds = u128::from(counter()) * NANOS_PER_SECOND / u128::from(frequency());
    u64::try_from(nanoseconds).unwrap_or(u64::MAX)
}

/// Counter ticks in one time slice.
pub fn slice_ticks() -> u64 {
    let ticks = TICK.as_nanos() * u128::from(frequency()) / NANOS_PER_SECOND;
    u64::try_from(ticks).unwrap_or(u64::MAX).max(1)
}

/// Fire this core's timer once when the counter reaches `deadline` ticks, or
/// switch it off for `None`. A deadline already in the past fires at once.
/// The deadline last written is in the core's record, as the timer registers
/// are the core's own.
pub fn program(deadline: Option<u64>) {
    let wanted = deadline.unwrap_or(NO_DEADLINE);
    if core_record::current()
        .programmed_deadline
        .swap(wanted, Ordering::Relaxed)
        == wanted
    {
        return;
    }
    match deadline {
        Some(ticks) => {
            CNTP_CVAL_EL0.set(ticks);
            CNTP_CTL_EL0.write(CNTP_CTL_EL0::ENABLE::SET + CNTP_CTL_EL0::IMASK::CLEAR);
        }
        None => CNTP_CTL_EL0.write(CNTP_CTL_EL0::ENABLE::CLEAR),
    }
}

/// Switch the timer off and forget the programmed deadline. The interrupt
/// handler calls this to deassert a timer interrupt before ending it.
pub fn disarm() {
    program(None);
}
