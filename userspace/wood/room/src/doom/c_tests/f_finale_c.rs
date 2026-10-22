//! Tests for `f_finale.c` — end-of-episode cast-roll globals and constants.
//!
//! `f_finale.c` handles the text crawl shown at the end of each episode and
//! the cast-of-characters roll that follows.  The cast globals (`castnum`,
//! `casttics`, etc.) are only meaningful after `F_StartCast` is called; before
//! that they are all zero.  The timing constants (`TEXTSPEED`, `TEXTWAIT`)
//! must be preserved exactly, or the text animation will advance at the wrong
//! rate.
//!
//! These tests verify:
//!   * `TEXTSPEED` and `TEXTWAIT` constant values
//!   * All cast globals start at zero before `F_StartCast`
//!   * `finaletext` and `finaleflat` pointers are NULL before `F_StartFinale`

#![allow(non_snake_case)]

use crate::doom::c_ffi;

// ---------------------------------------------------------------------------
// Text-crawl timing constants
// ---------------------------------------------------------------------------

/// `TEXTSPEED = 3` tics per character.
/// The finale text advances one character every 3 game tics.
#[test]
fn textspeed_is_3() {
    assert_eq!(c_ffi::TEXTSPEED, 3);
}

/// `TEXTWAIT = 250` tics ≈ 7.1 seconds of idle time after the text is done.
#[test]
fn textwait_is_250() {
    assert_eq!(c_ffi::TEXTWAIT, 250);
}

/// TEXTWAIT is substantially longer than TEXTSPEED.
#[test]
fn textwait_longer_than_textspeed() {
    assert!(c_ffi::TEXTWAIT > c_ffi::TEXTSPEED);
}

// ---------------------------------------------------------------------------
// finaletext / finaleflat — set by F_StartFinale
// ---------------------------------------------------------------------------

/// Before `F_StartFinale` is called, `finaletext` is NULL.
#[test]
fn finaletext_initially_null() {
    unsafe {
        assert!(
            c_ffi::finaletext.is_null(),
            "finaletext should be NULL before F_StartFinale"
        );
    }
}

/// Before `F_StartFinale` is called, `finaleflat` is NULL.
#[test]
fn finaleflat_initially_null() {
    unsafe {
        assert!(
            c_ffi::finaleflat.is_null(),
            "finaleflat should be NULL before F_StartFinale"
        );
    }
}

// ---------------------------------------------------------------------------
// Cast globals — default zero before F_StartCast
// ---------------------------------------------------------------------------

/// `castnum` is the index of the current monster in the cast roll; starts at 0.
#[test]
fn castnum_default_zero() {
    unsafe {
        assert_eq!(c_ffi::castnum, 0, "castnum should be 0 before F_StartCast");
    }
}

/// `casttics` counts down tics remaining in the current cast frame; starts 0.
#[test]
fn casttics_default_zero() {
    unsafe {
        assert_eq!(
            c_ffi::casttics,
            0,
            "casttics should be 0 before F_StartCast"
        );
    }
}

/// `castdeath` is true when the cast monster is dying; false (0) at program start.
#[test]
fn castdeath_default_zero() {
    unsafe {
        assert_eq!(
            c_ffi::castdeath,
            0,
            "castdeath should be false before F_StartCast"
        );
    }
}

/// `castframes` counts animation frames played; zero before the cast starts.
#[test]
fn castframes_default_zero() {
    unsafe {
        assert_eq!(
            c_ffi::castframes,
            0,
            "castframes should be 0 before F_StartCast"
        );
    }
}

/// `castonmelee` is true when the cast monster is using a melee attack;
/// false (0) at program start.
#[test]
fn castonmelee_default_zero() {
    unsafe {
        assert_eq!(
            c_ffi::castonmelee,
            0,
            "castonmelee should be false before F_StartCast"
        );
    }
}

/// `castattacking` is true during an attack animation; false (0) at start.
#[test]
fn castattacking_default_zero() {
    unsafe {
        assert_eq!(
            c_ffi::castattacking,
            0,
            "castattacking should be false before F_StartCast"
        );
    }
}

// ---------------------------------------------------------------------------
// Type-width checks
// ---------------------------------------------------------------------------

/// All cast integer globals are C `int` (4 bytes).
#[test]
fn cast_globals_are_c_int_width() {
    use std::ffi::c_int;
    const _: () = assert!(std::mem::size_of::<c_int>() == 4);
    unsafe {
        let _: c_int = c_ffi::castnum;
        let _: c_int = c_ffi::casttics;
        let _: c_int = c_ffi::castdeath;
        let _: c_int = c_ffi::castframes;
        let _: c_int = c_ffi::castonmelee;
        let _: c_int = c_ffi::castattacking;
    }
}
