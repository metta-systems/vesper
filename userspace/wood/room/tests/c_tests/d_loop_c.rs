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

use room::engine::doom::main_loop;

// ---------------------------------------------------------------------------
// BACKUPTICS constant
// ---------------------------------------------------------------------------

/// `BACKUPTICS = 128`: the tic-command ring buffer holds 128 slots.
/// Increasing this would change memory layout for the tic-command arrays.
#[test]
fn backuptics_is_128() {
    assert_eq!(main_loop::BACKUPTICS, 128);
}

/// BACKUPTICS is a power of two, enabling efficient modular indexing.
#[test]
fn backuptics_is_power_of_two() {
    let n = main_loop::BACKUPTICS;
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
        assert_eq!({ main_loop::gametic }, 0, "gametic should be 0 at startup");
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
        assert_eq!(
            { main_loop::ticdup },
            0,
            "ticdup should be 0 before D_InitNetGame"
        );
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
            { main_loop::singletics },
            0,
            "singletics should be false at startup"
        );
    }
}
