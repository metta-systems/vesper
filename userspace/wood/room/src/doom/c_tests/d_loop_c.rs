//! Tests for `d_loop.c` — main game-loop state globals and constants.
//!
//! `d_loop.c` implements the core timing/synchronization loop that paces game
//! tics and handles the (stub) network layer.  The globals `gametic` and
//! `ticdup` are central to frame pacing; `singletics` selects the demo-playback
//! timing mode.  `BACKUPTICS` is the ring-buffer size for tic commands.
//!
//! These tests verify:
//!   * `BACKUPTICS` constant value
//!   * `gametic`, `ticdup`, and `singletics` default to zero/false at start

#![allow(non_snake_case)]

use crate::doom::c_ffi;

// ---------------------------------------------------------------------------
// BACKUPTICS constant
// ---------------------------------------------------------------------------

/// `BACKUPTICS = 128`: the tic-command ring buffer holds 128 slots.
/// Increasing this would change memory layout for the tic-command arrays.
#[test]
fn backuptics_is_128() {
    assert_eq!(c_ffi::BACKUPTICS, 128);
}

/// BACKUPTICS is a power of two, enabling efficient modular indexing.
#[test]
fn backuptics_is_power_of_two() {
    let n = c_ffi::BACKUPTICS;
    assert!(
        n > 0 && (n & (n - 1)) == 0,
        "BACKUPTICS={n} should be a power of two"
    );
}

// ---------------------------------------------------------------------------
// gametic — current game tic
// ---------------------------------------------------------------------------

/// `gametic` is zero at program start; it increments once per rendered frame.
#[test]
fn gametic_default_zero() {
    unsafe {
        assert_eq!(c_ffi::gametic, 0, "gametic should be 0 at startup");
    }
}

// ---------------------------------------------------------------------------
// ticdup — tic duplication factor
// ---------------------------------------------------------------------------

/// `ticdup` is zero before the game loop initialises it.
/// After init it is normally 1 (single-player) or higher for net games.
#[test]
fn ticdup_default_zero() {
    unsafe {
        assert_eq!(c_ffi::ticdup, 0, "ticdup should be 0 before D_InitNetGame");
    }
}

// ---------------------------------------------------------------------------
// singletics — demo-timing mode flag
// ---------------------------------------------------------------------------

/// `singletics` is false (0) at program start; set true only by `-timedemo`.
#[test]
fn singletics_default_false() {
    unsafe {
        assert_eq!(
            c_ffi::singletics,
            0,
            "singletics should be false at startup"
        );
    }
}

// ---------------------------------------------------------------------------
// Type-width checks
// ---------------------------------------------------------------------------

/// `gametic` and `ticdup` are C `int` (4 bytes).
#[test]
fn loop_globals_are_c_int_width() {
    use std::ffi::c_int;
    const _: () = assert!(std::mem::size_of::<c_int>() == 4);
    unsafe {
        let _: c_int = c_ffi::gametic;
        let _: c_int = c_ffi::ticdup;
        let _: c_int = c_ffi::singletics;
    }
}
