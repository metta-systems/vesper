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
//! There is no starvation guard: as long as a Thread of a higher level is
//! runnable, no Thread of a lower level runs. A high-priority Thread is
//! expected to block most of the time.

/// Priority levels: every value of the one-byte level.
pub const LEVELS: usize = 256;

/// Round-robin within each priority level over the record slots `1..=slot_count`.
pub struct Rounds {
    /// For each level, the slot its next search starts at
    next: [usize; LEVELS],
    slot_count: usize,
}

impl Rounds {
    /// Every level's search starting at slot 1.
    #[must_use]
    pub const fn new(slot_count: usize) -> Self {
        Self {
            next: [1; LEVELS],
            slot_count,
        }
    }

    /// The slot to dispatch, with what `runnable` said of it: among the slots
    /// for which `runnable` returns their priority level, the highest level,
    /// and within it the first slot at or after that level's cursor (wrapping
    /// around). The level's cursor moves past the chosen slot. `None` when no
    /// slot is runnable.
    pub fn choose<Seen>(
        &mut self,
        mut runnable: impl FnMut(usize) -> Option<(u8, Seen)>,
    ) -> Option<(usize, Seen)> {
        let slot_count = self.slot_count;
        // (level, distance from the level's cursor, slot, seen)
        let mut best: Option<(u8, usize, usize, Seen)> = None;
        for slot in 1..=slot_count {
            let Some((level, seen)) = runnable(slot) else {
                continue;
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
