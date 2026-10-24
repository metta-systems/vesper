//! Rust port of vendor/doomgeneric/m_random.c.
//!
//! The menu and game-controller cursor `rndindex` lives in [`RandomState`],
//! held in the [`RANDOM`] global; it is declared `extern int` in
//! doomstat.h:276 and read by g_game.c:957 for the net-play consistency
//! check. The play cursor `prndindex` is simulation state and lives in
//! [`World::random`] as a [`PlayRandom`].

#![allow(non_upper_case_globals, non_snake_case)]

use crate::engine::{global::Global, world::World};

/// 256-entry lookup table backing both `M_Random` and `P_Random`.
/// Byte-for-byte identical to `rndtable` in `m_random.c`; the values are
/// the original Doom random sequence and must not be changed.
pub(crate) static RNDTABLE: [u8; 256] = [
    0, 8, 109, 220, 222, 241, 149, 107, 75, 248, 254, 140, 16, 66, 74, 21, 211, 47, 80, 242, 154,
    27, 205, 128, 161, 89, 77, 36, 95, 110, 85, 48, 212, 140, 211, 249, 22, 79, 200, 50, 28, 188,
    52, 140, 202, 120, 68, 145, 62, 70, 184, 190, 91, 197, 152, 224, 149, 104, 25, 178, 252, 182,
    202, 182, 141, 197, 4, 81, 181, 242, 145, 42, 39, 227, 156, 198, 225, 193, 219, 93, 122, 175,
    249, 0, 175, 143, 70, 239, 46, 246, 163, 53, 163, 109, 168, 135, 2, 235, 25, 92, 20, 145, 138,
    77, 69, 166, 78, 176, 173, 212, 166, 113, 94, 161, 41, 50, 239, 49, 111, 164, 70, 60, 2, 37,
    171, 75, 136, 156, 11, 56, 42, 146, 138, 229, 73, 146, 77, 61, 98, 196, 135, 106, 63, 197, 195,
    86, 96, 203, 113, 101, 170, 247, 181, 113, 80, 250, 108, 7, 255, 237, 129, 226, 79, 107, 112,
    166, 103, 241, 24, 223, 239, 120, 198, 58, 60, 82, 128, 3, 184, 66, 143, 224, 145, 224, 81,
    206, 163, 45, 63, 90, 168, 114, 59, 33, 159, 95, 28, 139, 123, 98, 125, 196, 15, 70, 194, 253,
    54, 14, 109, 226, 71, 17, 161, 93, 186, 87, 244, 138, 20, 52, 123, 251, 26, 36, 17, 46, 52,
    231, 232, 76, 31, 221, 84, 37, 216, 165, 212, 106, 197, 242, 98, 43, 39, 175, 254, 145, 190,
    84, 118, 222, 187, 136, 120, 163, 236, 249,
];

/// The game-controller random cursor (C `m_random.c` global `rndindex`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RandomState {
    /// `extern int rndindex` — the game-thinker random cursor.
    /// Read by `g_game` to build net-play consistency hashes.
    pub rndindex: i32,
}

impl RandomState {
    /// The cursor at the start of the table.
    pub const fn new() -> Self {
        Self { rndindex: 0 }
    }
}

impl Default for RandomState {
    fn default() -> Self {
        Self::new()
    }
}

/// The game-controller random cursor.
pub static RANDOM: Global<RandomState> = Global::new(RandomState::new());

/// The play-simulation random cursor (C `m_random.c` global `prndindex`),
/// kept in [`World::random`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayRandom {
    /// `int prndindex`. The demo-playthrough integration test reads it for
    /// consistency-check snapshots.
    pub prndindex: i32,
}

impl PlayRandom {
    /// The cursor at the start of the table.
    pub const fn new() -> Self {
        Self { prndindex: 0 }
    }

    /// Advance the cursor and read the table entry it reaches.
    pub fn next_byte(&mut self) -> i32 {
        self.prndindex = (self.prndindex + 1) & 0xff;
        RNDTABLE[self.prndindex as usize] as i32
    }
}

impl Default for PlayRandom {
    fn default() -> Self {
        Self::new()
    }
}

/// `int P_Random(void)` — advance and read the play-simulation random
/// cursor of `world`. Returns the next byte from `RNDTABLE` masked to
/// `0..=255`. This is the deterministic generator used by game logic;
/// demos and net-play depend on its exact sequence.
pub fn P_Random(world: &mut World) -> i32 {
    world.random.next_byte()
}

/// `int M_Random(void)` — advance and read the game-thinker random
/// cursor. Returns the next byte from `RNDTABLE` masked to `0..=255`.
/// Used for non-simulation effects (e.g. menu animation) so its use
/// does not perturb demo determinism.
pub fn M_Random() -> i32 {
    let mut random = RANDOM.borrow_mut();
    random.rndindex = (random.rndindex + 1) & 0xff;
    RNDTABLE[random.rndindex as usize] as i32
}

/// `void M_ClearRandom(void)` — reset both `rndindex` and the `prndindex`
/// of `world` to 0. Called at the start of a new game/demo so the random
/// sequence reproduces.
pub fn M_ClearRandom(world: &mut World) {
    RANDOM.set(RandomState::new());
    world.random = PlayRandom::new();
}

#[cfg(test)]
mod tests {
    use {super::*, std::sync::Mutex};

    /// Process-wide guard that serialises tests touching the global
    /// `rndindex` so parallel `cargo test` invocations do not interleave.
    // Random globals are process-wide; serialise tests that mutate them.
    static LOCK: Mutex<()> = Mutex::new(());

    /// Spot-check that `RNDTABLE` still contains the canonical Doom
    /// values at a few known positions.
    #[test]
    fn table_sentinels() {
        assert_eq!(RNDTABLE[0], 0);
        assert_eq!(RNDTABLE[1], 8);
        assert_eq!(RNDTABLE[255], 249);
        assert_eq!(RNDTABLE.len(), 256);
    }

    /// Both cursors start at zero.
    #[test]
    fn new_state_starts_at_zero() {
        assert_eq!(RandomState::new(), RandomState { rndindex: 0 });
        assert_eq!(PlayRandom::new(), PlayRandom { prndindex: 0 });
        assert_eq!(World::new().random.prndindex, 0);
    }

    /// Three consecutive `M_Random` calls should return entries 1, 2, 3
    /// of `RNDTABLE` in order (the pre-increment makes index 0 unused).
    #[test]
    fn m_random_walks_the_table() {
        let _guard = LOCK.lock().unwrap();
        M_ClearRandom(&mut World::new());
        assert_eq!(M_Random(), RNDTABLE[1] as i32);
        assert_eq!(M_Random(), RNDTABLE[2] as i32);
        assert_eq!(M_Random(), RNDTABLE[3] as i32);
    }

    /// `M_Random` and `P_Random` each advance their own cursor; the
    /// first call to either after `M_ClearRandom` returns `RNDTABLE[1]`.
    #[test]
    fn p_and_m_independent() {
        let _guard = LOCK.lock().unwrap();
        let mut world = World::new();
        M_ClearRandom(&mut world);
        let a = M_Random();
        let b = P_Random(&mut world);
        assert_eq!(a, b);
        assert_eq!(RANDOM.get().rndindex, 1);
        assert_eq!(world.random.prndindex, 1);
    }

    /// `M_ClearRandom` must zero `rndindex` even if both cursors have
    /// been advanced by prior calls.
    #[test]
    fn clear_resets_both() {
        let _guard = LOCK.lock().unwrap();
        let mut world = World::new();
        M_Random();
        M_Random();
        P_Random(&mut world);
        M_ClearRandom(&mut world);
        assert_eq!(RANDOM.get().rndindex, 0);
    }

    /// Two `P_Random` calls should leave `prndindex` at 2 and return
    /// `RNDTABLE[1]` then `RNDTABLE[2]`.
    #[test]
    fn p_random_increments_prndindex() {
        let mut world = World::new();
        assert_eq!(P_Random(&mut world), RNDTABLE[1] as i32);
        assert_eq!(P_Random(&mut world), RNDTABLE[2] as i32);
        assert_eq!(world.random.prndindex, 2);
    }

    /// `M_ClearRandom` must zero the world's `prndindex` (in addition to
    /// `rndindex`, covered by `clear_resets_both`).
    #[test]
    fn clear_also_resets_prndindex() {
        let _guard = LOCK.lock().unwrap();
        let mut world = World::new();
        P_Random(&mut world);
        P_Random(&mut world);
        M_ClearRandom(&mut world);
        assert_eq!(world.random.prndindex, 0);
    }

    /// After exactly 256 calls to M_Random the index wraps back to 0,
    /// so the 257th call returns RNDTABLE[1] — identical to the first call.
    #[test]
    fn m_random_wraps_at_256() {
        let _guard = LOCK.lock().unwrap();
        M_ClearRandom(&mut World::new());
        let first = M_Random();
        for _ in 1..256 {
            M_Random();
        }
        // 256 calls done; rndindex = (0 + 256) & 0xFF = 0
        assert_eq!(RANDOM.get().rndindex, 0);
        // 257th call should match the first (RNDTABLE[1])
        assert_eq!(M_Random(), first);
    }

    /// P_Random and M_Random share the same RNDTABLE but use independent cursors.
    #[test]
    fn p_and_m_cursors_are_independent() {
        let _guard = LOCK.lock().unwrap();
        let mut world = World::new();
        M_ClearRandom(&mut world);
        // advance M five steps
        for _ in 0..5 {
            M_Random();
        }
        // P cursor is still at 0; first P_Random returns RNDTABLE[1]
        assert_eq!(P_Random(&mut world), RNDTABLE[1] as i32);
    }
}
