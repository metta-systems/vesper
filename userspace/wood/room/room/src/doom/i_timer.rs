//! Rust port of vendor/doomgeneric/i_timer.c.
//!
//! Game timer functions built on top of the doomgeneric host hooks
//! `DG_GetTicksMs` and `DG_SleepMs`. Doom counts time in 35 Hz tics; these
//! helpers convert between wall-clock milliseconds and tics, and provide a
//! sleep primitive used by the main loop. `BASETIME` is captured on the
//! first time query so that `I_GetTime` / `I_GetTimeMS` return values
//! relative to game start rather than the host's epoch.

#![allow(non_upper_case_globals, non_snake_case)]

use std::ffi::c_int;

/// Game tick rate in Hz. Doom updates state at 35 tics per second; all
/// gameplay code expresses durations in multiples of this constant.
pub const TICRATE: c_int = 35;

/// Wall-clock millisecond timestamp captured on the first call to either
/// `I_GetTime` or `I_GetTimeMS`. All subsequent results are deltas relative
/// to this baseline so the game clock starts at zero. Mirrors the C
/// `basetime` static in `i_timer.c`.
static mut BASETIME: u32 = 0;

extern "C" {
    /// Host-provided clock: returns wall-clock milliseconds. Implemented per
    /// platform in the doomgeneric backend (e.g. SDL, raw POSIX).
    fn DG_GetTicksMs() -> u32;
    /// Host-provided sleep: blocks for approximately `ms` milliseconds.
    fn DG_SleepMs(ms: u32);
}

/// Return the host's raw millisecond tick counter (not relative to game
/// start). Thin wrapper around `DG_GetTicksMs`.
///
/// Called from a few places that need wall-clock deltas independent of the
/// game tic clock.
#[no_mangle]
pub extern "C" fn I_GetTicks() -> c_int {
    unsafe { DG_GetTicksMs() as c_int }
}

/// Return the time since game start measured in 1/35-second game tics.
///
/// On the first call, the current millisecond counter is recorded in
/// `BASETIME` so all subsequent calls return `(now - BASETIME) * TICRATE /
/// 1000`. Used throughout the game loop to drive tic-based timing.
///
/// Note: the C original uses `basetime == 0` as a not-yet-initialised
/// sentinel, which is preserved here. If the host's first tick value
/// happens to be exactly 0 the baseline is captured on the next call;
/// chocolate-doom relies on the same behaviour.
#[no_mangle]
pub extern "C" fn I_GetTime() -> c_int {
    unsafe {
        let ticks = DG_GetTicksMs();
        if BASETIME == 0 {
            BASETIME = ticks;
        }
        ((ticks - BASETIME) * TICRATE as u32 / 1000) as c_int
    }
}

/// Return the time since game start in milliseconds.
///
/// Same baseline logic as `I_GetTime` but without converting to tics. Used
/// by code paths that need sub-tic precision (e.g. mouse polling).
#[no_mangle]
pub extern "C" fn I_GetTimeMS() -> c_int {
    unsafe {
        let ticks = DG_GetTicksMs();
        if BASETIME == 0 {
            BASETIME = ticks;
        }
        (ticks - BASETIME) as c_int
    }
}

/// Sleep for approximately `ms` milliseconds by delegating to the host
/// `DG_SleepMs` hook. The actual resolution depends on the backend.
#[no_mangle]
pub extern "C" fn I_Sleep(ms: c_int) {
    unsafe { DG_SleepMs(ms as u32) }
}

/// No-op port of the original vertical-blank wait. The chocolate-doom and
/// doomgeneric C versions are both empty (the original would have called
/// `I_Sleep((count * 1000) / 70)`); the Rust port preserves the no-op so
/// timing matches.
#[no_mangle]
pub extern "C" fn I_WaitVBL(_count: c_int) {}

/// Initialise the timer subsystem. The C version originally called
/// `SDL_Init(SDL_INIT_TIMER)`; doomgeneric drops that and the Rust port
/// follows suit, leaving the function as a no-op kept for ABI parity.
#[no_mangle]
pub extern "C" fn I_InitTimer() {}
