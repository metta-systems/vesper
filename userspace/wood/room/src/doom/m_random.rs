//! Rust port of vendor/doomgeneric/m_random.c.
//!
//! `rndindex` is declared `extern int` in doomstat.h:276 and read by
//! g_game.c:957 for the net-play consistency check. We must export it
//! with C linkage so the C side resolves against our Rust definition.

#![allow(non_upper_case_globals, non_snake_case)]

use std::ffi::c_int;

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

/// `extern int rndindex` — the game-thinker random cursor.
/// Read by `g_game.c` to build net-play consistency hashes. Must have
/// C linkage so the `extern` declaration in `doomstat.h` resolves here.
#[no_mangle]
pub static mut rndindex: c_int = 0;

/// `int prndindex` — the play-simulation random cursor.
/// Exported with C linkage so the demo-playthrough integration test can
/// read it via `extern "C"` for consistency-check snapshots.
#[no_mangle]
pub static mut prndindex: c_int = 0;

/// `int P_Random(void)` — advance and read the play-simulation random
/// cursor. Returns the next byte from `RNDTABLE` masked to `0..=255`.
/// This is the deterministic generator used by game logic; demos and
/// net-play depend on its exact sequence.
// Which one is deterministic?
#[no_mangle]
pub extern "C" fn P_Random() -> c_int {
    unsafe {
        prndindex = (prndindex + 1) & 0xff;
        RNDTABLE[prndindex as usize] as c_int
    }
}

/// `int M_Random(void)` — advance and read the game-thinker random
/// cursor. Returns the next byte from `RNDTABLE` masked to `0..=255`.
/// Used for non-simulation effects (e.g. menu animation) so its use
/// does not perturb demo determinism.
#[no_mangle]
pub extern "C" fn M_Random() -> c_int {
    unsafe {
        rndindex = (rndindex + 1) & 0xff;
        RNDTABLE[rndindex as usize] as c_int
    }
}

/// `void M_ClearRandom(void)` — reset both `rndindex` and `prndindex` to
/// 0. Called at the start of a new game/demo so the random sequence
/// reproduces.
#[no_mangle]
pub extern "C" fn M_ClearRandom() {
    unsafe {
        rndindex = 0;
        prndindex = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Process-wide guard that serialises tests touching the global
    /// `rndindex`/`prndindex` so parallel `cargo test` invocations do not
    /// interleave.
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

    /// Three consecutive `M_Random` calls should return entries 1, 2, 3
    /// of `RNDTABLE` in order (the pre-increment makes index 0 unused).
    #[test]
    fn m_random_walks_the_table() {
        let _g = LOCK.lock().unwrap();
        M_ClearRandom();
        assert_eq!(M_Random(), RNDTABLE[1] as c_int);
        assert_eq!(M_Random(), RNDTABLE[2] as c_int);
        assert_eq!(M_Random(), RNDTABLE[3] as c_int);
    }

    /// `M_Random` and `P_Random` each advance their own cursor; the
    /// first call to either after `M_ClearRandom` returns `RNDTABLE[1]`.
    #[test]
    fn p_and_m_independent() {
        let _g = LOCK.lock().unwrap();
        M_ClearRandom();
        let a = M_Random();
        let b = P_Random();
        assert_eq!(a, b);
        unsafe {
            assert_eq!(rndindex, 1);
            assert_eq!(prndindex, 1);
        }
    }

    /// `M_ClearRandom` must zero `rndindex` even if both cursors have
    /// been advanced by prior calls.
    #[test]
    fn clear_resets_both() {
        let _g = LOCK.lock().unwrap();
        M_Random();
        M_Random();
        P_Random();
        M_ClearRandom();
        unsafe {
            assert_eq!(rndindex, 0);
        }
    }

    /// Two `P_Random` calls should leave `prndindex` at 2 and return
    /// `RNDTABLE[1]` then `RNDTABLE[2]`.
    #[test]
    fn p_random_increments_prndindex() {
        let _g = LOCK.lock().unwrap();
        M_ClearRandom();
        assert_eq!(P_Random(), RNDTABLE[1] as c_int);
        assert_eq!(P_Random(), RNDTABLE[2] as c_int);
        unsafe {
            assert_eq!(prndindex, 2);
        }
    }

    /// `M_ClearRandom` must zero `prndindex` (in addition to `rndindex`,
    /// covered by `clear_resets_both`).
    #[test]
    fn clear_also_resets_prndindex() {
        let _g = LOCK.lock().unwrap();
        P_Random();
        P_Random();
        M_ClearRandom();
        unsafe {
            assert_eq!(prndindex, 0);
        }
    }

    /// After exactly 256 calls to M_Random the index wraps back to 0,
    /// so the 257th call returns RNDTABLE[1] — identical to the first call.
    #[test]
    fn m_random_wraps_at_256() {
        let _g = LOCK.lock().unwrap();
        M_ClearRandom();
        let first = M_Random();
        for _ in 1..256 {
            M_Random();
        }
        // 256 calls done; rndindex = (0 + 256) & 0xFF = 0
        unsafe {
            assert_eq!(rndindex, 0);
        }
        // 257th call should match the first (RNDTABLE[1])
        assert_eq!(M_Random(), first);
    }

    /// P_Random and M_Random share the same RNDTABLE but use independent cursors.
    #[test]
    fn p_and_m_cursors_are_independent() {
        let _g = LOCK.lock().unwrap();
        M_ClearRandom();
        // advance M five steps
        for _ in 0..5 {
            M_Random();
        }
        // P cursor is still at 0; first P_Random returns RNDTABLE[1]
        assert_eq!(P_Random(), RNDTABLE[1] as c_int);
    }
}
