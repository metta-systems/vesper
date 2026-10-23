//! Tests for `f_finale.c` — end-of-episode cast-roll globals and constants.
//!
//! `f_finale.c` handles the text crawl shown at the end of each episode and
//! the cast-of-characters roll that follows.  The cast fields (`castnum`,
//! `casttics`, etc.) are only meaningful after `F_StartCast` is called; before
//! that they are all zero.  The timing constants (`TEXTSPEED`, `TEXTWAIT`)
//! must be preserved exactly, or the text animation will advance at the wrong
//! rate.
//!
//! These tests verify:
//!   * `TEXTSPEED` and `TEXTWAIT` constant values
//!   * All cast fields of the initial `FinaleState` are zero
//!   * `finaletext` and `finaleflat` of the initial `FinaleState` are empty

#![allow(non_snake_case)]

use room::engine::finale::{self, FinaleState};

// ---------------------------------------------------------------------------
// Text-crawl timing constants
// ---------------------------------------------------------------------------

/// `TEXTSPEED = 3` tics per character.
/// The finale text advances one character every 3 game tics.
#[test]
fn textspeed_is_3() {
    assert_eq!(finale::TEXTSPEED, 3);
}

/// `TEXTWAIT = 250` tics ≈ 7.1 seconds of idle time after the text is done.
#[test]
fn textwait_is_250() {
    assert_eq!(finale::TEXTWAIT, 250);
}

/// TEXTWAIT is substantially longer than TEXTSPEED.
#[test]
fn textwait_longer_than_textspeed() {
    assert!(finale::TEXTWAIT > finale::TEXTSPEED);
}

// ---------------------------------------------------------------------------
// finaletext / finaleflat — set by F_StartFinale
// ---------------------------------------------------------------------------

/// Before `F_StartFinale` is called, `finaletext` is empty.
#[test]
fn finaletext_initially_empty() {
    assert!(
        FinaleState::new().finaletext.is_empty(),
        "finaletext should be empty before F_StartFinale"
    );
}

/// Before `F_StartFinale` is called, `finaleflat` is empty.
#[test]
fn finaleflat_initially_empty() {
    assert!(
        FinaleState::new().finaleflat.is_empty(),
        "finaleflat should be empty before F_StartFinale"
    );
}

// ---------------------------------------------------------------------------
// Cast fields — default zero before F_StartCast
// ---------------------------------------------------------------------------

/// `castnum` is the index of the current monster in the cast roll; starts at 0.
#[test]
fn castnum_default_zero() {
    assert_eq!(
        FinaleState::new().castnum,
        0,
        "castnum should be 0 before F_StartCast"
    );
}

/// `casttics` counts down tics remaining in the current cast frame; starts 0.
#[test]
fn casttics_default_zero() {
    assert_eq!(
        FinaleState::new().casttics,
        0,
        "casttics should be 0 before F_StartCast"
    );
}

/// `castdeath` is true when the cast monster is dying; false (0) at program start.
#[test]
fn castdeath_default_zero() {
    assert_eq!(
        FinaleState::new().castdeath,
        0,
        "castdeath should be false before F_StartCast"
    );
}

/// `castframes` counts animation frames played; zero before the cast starts.
#[test]
fn castframes_default_zero() {
    assert_eq!(
        FinaleState::new().castframes,
        0,
        "castframes should be 0 before F_StartCast"
    );
}

/// `castonmelee` is true when the cast monster is using a melee attack;
/// false (0) at program start.
#[test]
fn castonmelee_default_zero() {
    assert_eq!(
        FinaleState::new().castonmelee,
        0,
        "castonmelee should be false before F_StartCast"
    );
}

/// `castattacking` is true during an attack animation; false (0) at start.
#[test]
fn castattacking_default_zero() {
    assert_eq!(
        FinaleState::new().castattacking,
        0,
        "castattacking should be false before F_StartCast"
    );
}

/// `caststate` points at no state until the cast roll starts.
#[test]
fn caststate_default_null() {
    assert!(
        FinaleState::new().caststate.is_null(),
        "caststate should be null before F_StartCast"
    );
}
