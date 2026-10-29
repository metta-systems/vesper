#![no_std]

//! The root dispatcher's choice, as a pure function host-tested in
//! `tests/policy.rs`: which record slot runs next.
//!
//! Each managed Thread has a priority level (`libdevice_driver::Priority`,
//! one byte, higher runs first). The dispatcher always runs a runnable Thread
//! of the highest level that has one, and takes turns within a level: each
//! level keeps its own cursor, the slot after the one it last chose, and the
//! search for that level starts there.
//!
//! Starvation guard: a Thread above [`NORMAL`] that has executed for a whole
//! slice since it last blocked is chosen as if it were normal, until it
//! blocks again. Being preempted (by the end of its slice or by a Thread
//! woken at a higher level) or yielding is not blocking, so the time
//! adds up across those runs. A misbehaving raised Thread therefore takes
//! turns with the normal ones instead of keeping them off the CPU, while a
//! raised Thread that blocks before it has used a slice keeps its level.
//! The dispatcher reports each run with [`Rounds::ran`].

/// Priority levels: every value of the one-byte level.
pub const LEVELS: usize = 256;

/// The normal level (`libdevice_driver::Priority::NORMAL`), which a demoted
/// Thread runs at.
pub const NORMAL: u8 = 0;

/// Most record slots the dispatcher manages (slots `1..=MAX_SLOTS`).
pub const MAX_SLOTS: usize = 128;

/// Round-robin within each priority level over the record slots
/// `1..=slot_count`, with the starvation guard.
pub struct Rounds {
    /// For each level, the slot its next search starts at
    next: [usize; LEVELS],
    slot_count: usize,
    /// Execution a Thread may accumulate without blocking before it is
    /// demoted, in nanoseconds: the dispatcher's slice
    slice_ns: u64,
    /// For each slot, the Thread's total execution time when last reported
    run_time_ns: [u64; MAX_SLOTS + 1],
    /// For each slot, the execution time accumulated since the Thread last
    /// blocked
    unblocked_ns: [u64; MAX_SLOTS + 1],
    /// Times a raised Thread was demoted
    demotions: u64,
}

impl Rounds {
    /// Every level's search starting at slot 1; a raised Thread is demoted
    /// after `slice_ns` of execution without blocking.
    ///
    /// # Panics
    ///
    /// If `slot_count` exceeds [`MAX_SLOTS`].
    #[must_use]
    pub const fn new(slot_count: usize, slice_ns: u64) -> Self {
        assert!(slot_count <= MAX_SLOTS, "too many slots for the dispatcher");
        Self {
            next: [1; LEVELS],
            slot_count,
            slice_ns,
            run_time_ns: [0; MAX_SLOTS + 1],
            unblocked_ns: [0; MAX_SLOTS + 1],
            demotions: 0,
        }
    }

    /// Whether the Thread in `slot` is demoted if it is above normal: it has
    /// executed for a whole slice since it last blocked.
    #[must_use]
    pub fn demoted(&self, slot: usize) -> bool {
        self.unblocked_ns
            .get(slot)
            .is_some_and(|&unblocked| unblocked >= self.slice_ns)
    }

    /// Times a raised Thread was demoted so far. The dispatcher is not a log
    /// client, so this count is the guard's only trace.
    #[must_use]
    pub const fn demotions(&self) -> u64 {
        self.demotions
    }

    /// Report a run of the Thread in `slot`, declared at `level`: its
    /// record's total execution time `run_time_ns` and whether it stopped by
    /// blocking (or exiting or faulting). A Thread that blocked starts again
    /// from nothing; one that did not adds the time it ran since the last
    /// report. A total smaller than the last one is a new Thread in the slot,
    /// counted from zero.
    pub fn ran(&mut self, slot: usize, level: u8, run_time_ns: u64, blocked: bool) {
        let (Some(previous_total), Some(unblocked)) = (
            self.run_time_ns.get_mut(slot),
            self.unblocked_ns.get_mut(slot),
        ) else {
            return;
        };
        let ran_ns = run_time_ns
            .checked_sub(*previous_total)
            .unwrap_or(run_time_ns);
        *previous_total = run_time_ns;
        if blocked {
            *unblocked = 0;
            return;
        }
        let was_demoted = *unblocked >= self.slice_ns;
        *unblocked = unblocked.saturating_add(ran_ns);
        if level > NORMAL && !was_demoted && *unblocked >= self.slice_ns {
            self.demotions = self.demotions.wrapping_add(1);
        }
    }

    /// The slot to dispatch, with what `runnable` said of it: among the slots
    /// for which `runnable` returns their priority level, the highest level,
    /// and within it the first slot at or after that level's cursor (wrapping
    /// around). A demoted slot counts at [`NORMAL`]. The cursor of the level
    /// it counted at moves past the chosen slot. `None` when no slot is
    /// runnable.
    pub fn choose<Seen>(
        &mut self,
        mut runnable: impl FnMut(usize) -> Option<(u8, Seen)>,
    ) -> Option<(usize, Seen)> {
        let slot_count = self.slot_count;
        // (level, distance from the level's cursor, slot, seen)
        let mut best: Option<(u8, usize, usize, Seen)> = None;
        for slot in 1..=slot_count {
            let Some((declared, seen)) = runnable(slot) else {
                continue;
            };
            let level = if declared > NORMAL && self.demoted(slot) {
                NORMAL
            } else {
                declared
            };
            let start = self.next[usize::from(level)];
            let distance = (slot + slot_count - start) % slot_count;
            let better = best
                .as_ref()
                .is_none_or(|&(best_level, best_distance, ..)| {
                    level > best_level || (level == best_level && distance < best_distance)
                });
            if better {
                best = Some((level, distance, slot, seen));
            }
        }
        let (level, _distance, slot, seen) = best?;
        self.next[usize::from(level)] = slot % slot_count + 1;
        Some((slot, seen))
    }
}
