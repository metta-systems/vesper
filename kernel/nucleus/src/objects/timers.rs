//! Dispatcher timers: the deadlines `Scheduler.ArmTimer` registers.
//!
//! A dispatcher has one timer, so the table holds at most one deadline per
//! Scheduler. The storage is a fixed array: the kernel allocates nothing when
//! a timer is armed, and a full table refuses a further Scheduler's timer.
//! Deadlines are counter ticks on the [`crate::objects::Nucleus::counter_ticks`]
//! timeline. The table is shared by all cores; each entry carries the logical
//! core of its Scheduler, whose timer counts it.

use {crate::objects::carved::CarvedId, libobject::CapError};

/// Schedulers that can hold an armed timer at once.
pub const MAX_ARMED_TIMERS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Entry {
    scheduler: CarvedId,
    core: usize,
    deadline_ticks: u64,
}

/// The armed dispatcher timers.
pub struct SchedulerTimers {
    entries: [Option<Entry>; MAX_ARMED_TIMERS],
}

impl SchedulerTimers {
    pub const fn new() -> Self {
        Self {
            entries: [None; MAX_ARMED_TIMERS],
        }
    }

    /// Arm `scheduler`'s timer for `deadline_ticks` on `core`, replacing an
    /// earlier deadline, which is returned. Fails with `PoolExhausted` when
    /// the table is full of other Schedulers' timers.
    pub fn arm(
        &mut self,
        scheduler: CarvedId,
        core: usize,
        deadline_ticks: u64,
    ) -> Result<Option<u64>, CapError> {
        let entry = Entry {
            scheduler,
            core,
            deadline_ticks,
        };
        if let Some(existing) = self
            .entries
            .iter_mut()
            .flatten()
            .find(|existing| existing.scheduler == scheduler)
        {
            let previous = existing.deadline_ticks;
            *existing = entry;
            return Ok(Some(previous));
        }
        let free = self
            .entries
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or(CapError::PoolExhausted)?;
        *free = Some(entry);
        Ok(None)
    }

    /// Disarm `scheduler`'s timer; its deadline, if it was armed.
    pub fn disarm(&mut self, scheduler: CarvedId) -> Option<u64> {
        let slot = self
            .entries
            .iter_mut()
            .find(|slot| slot.is_some_and(|entry| entry.scheduler == scheduler))?;
        slot.take().map(|entry| entry.deadline_ticks)
    }

    /// The earliest deadline armed on `core`.
    pub fn next_deadline(&self, core: usize) -> Option<u64> {
        self.entries
            .iter()
            .flatten()
            .filter(|entry| entry.core == core)
            .map(|entry| entry.deadline_ticks)
            .min()
    }

    /// Disarm and return the Scheduler of `core`'s earliest timer whose
    /// deadline is at or before `now_ticks`.
    pub fn take_expired(&mut self, core: usize, now_ticks: u64) -> Option<CarvedId> {
        let slot = self
            .entries
            .iter_mut()
            .filter(|slot| {
                slot.is_some_and(|entry| entry.core == core && entry.deadline_ticks <= now_ticks)
            })
            .min_by_key(|slot| slot.map(|entry| entry.deadline_ticks))?;
        slot.take().map(|entry| entry.scheduler)
    }
}

impl Default for SchedulerTimers {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use {
        super::{MAX_ARMED_TIMERS, SchedulerTimers},
        crate::objects::carved::test_id,
        libobject::{CapError, ObjectType},
    };

    fn scheduler(number: u64) -> crate::objects::carved::CarvedId {
        test_id(ObjectType::SCHEDULER, 0x1000 + number * 0x100)
    }

    #[test_case]
    fn arming_replaces_and_reports_the_previous_deadline() {
        let mut timers = SchedulerTimers::new();
        assert_eq!(timers.arm(scheduler(1), 0, 500).ok(), Some(None));
        assert_eq!(timers.next_deadline(0), Some(500));
        assert_eq!(timers.arm(scheduler(1), 0, 900).ok(), Some(Some(500)));
        assert_eq!(timers.next_deadline(0), Some(900));
        assert_eq!(timers.disarm(scheduler(1)), Some(900));
        assert_eq!(timers.disarm(scheduler(1)), None);
        assert_eq!(timers.next_deadline(0), None);
    }

    #[test_case]
    fn the_earliest_expired_timer_fires_once_and_disarms() {
        let mut timers = SchedulerTimers::new();
        timers.arm(scheduler(1), 0, 300).ok();
        timers.arm(scheduler(2), 0, 100).ok();
        timers.arm(scheduler(3), 0, 200).ok();
        assert_eq!(timers.next_deadline(0), Some(100));
        assert_eq!(timers.take_expired(0, 50), None);
        assert_eq!(timers.take_expired(0, 250), Some(scheduler(2)));
        assert_eq!(timers.take_expired(0, 250), Some(scheduler(3)));
        assert_eq!(timers.take_expired(0, 250), None);
        assert_eq!(timers.take_expired(0, 300), Some(scheduler(1)));
        assert_eq!(timers.next_deadline(0), None);
    }

    #[test_case]
    fn each_core_counts_and_fires_only_its_own_timers() {
        let mut timers = SchedulerTimers::new();
        timers.arm(scheduler(1), 0, 300).ok();
        timers.arm(scheduler(2), 1, 100).ok();
        assert_eq!(timers.next_deadline(0), Some(300));
        assert_eq!(timers.next_deadline(1), Some(100));
        assert_eq!(timers.next_deadline(2), None);
        assert_eq!(timers.take_expired(0, 250), None);
        assert_eq!(timers.take_expired(1, 250), Some(scheduler(2)));
        assert_eq!(timers.take_expired(1, 400), None);
        assert_eq!(timers.take_expired(0, 400), Some(scheduler(1)));
    }

    #[test_case]
    fn a_full_table_refuses_another_scheduler_but_still_replaces() {
        let mut timers = SchedulerTimers::new();
        for number in 0..MAX_ARMED_TIMERS as u64 {
            assert!(timers.arm(scheduler(number), 0, 100 + number).is_ok());
        }
        assert!(matches!(
            timers.arm(scheduler(99), 0, 1),
            Err(CapError::PoolExhausted)
        ));
        assert_eq!(timers.arm(scheduler(0), 0, 50).ok(), Some(Some(100)));
    }
}
