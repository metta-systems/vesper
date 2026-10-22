//! Tests for `st_stuff.c` — status bar constants.
//!
//! `st_stuff.c` draws the status bar at the bottom of the screen: health,
//! ammo, armor, keys, and the player-face widget.  The face widget uses a
//! carefully structured table of sprites: pain levels × face types + extras.
//! The palette animation constants control the red-flash (pain) and
//! yellow-flash (bonus) palette rotations.
//!
//! These tests verify:
//!   * Palette animation start indices and counts
//!   * Face-sprite table structure constants
//!   * Derived `ST_NUMFACES` total
//!   * Timing constants for face-state transitions
//!   * Screen-position constants for status bar elements

#![allow(non_snake_case)]

use crate::doom::c_ffi;
use crate::doom::i_timer::TICRATE;

// ---------------------------------------------------------------------------
// Palette animation constants
// ---------------------------------------------------------------------------

/// Red (pain) palettes start at index 1.
#[test]
fn startredpals_is_1() {
    assert_eq!(c_ffi::STARTREDPALS, 1);
}

/// Bonus (pick-up) palettes start at index 9.
#[test]
fn startbonuspals_is_9() {
    assert_eq!(c_ffi::STARTBONUSPALS, 9);
}

/// There are 8 red pain palette entries.
#[test]
fn numredpals_is_8() {
    assert_eq!(c_ffi::NUMREDPALS, 8);
}

/// There are 4 bonus pick-up palette entries.
#[test]
fn numbonuspals_is_4() {
    assert_eq!(c_ffi::NUMBONUSPALS, 4);
}

/// Radiation-suit palette is at index 13.
#[test]
fn radiationpal_is_13() {
    assert_eq!(c_ffi::RADIATIONPAL, 13);
}

/// Bonus palettes immediately follow the red pain palettes in the WAD.
/// STARTBONUSPALS = STARTREDPALS + NUMREDPALS.
#[test]
fn bonus_pals_follow_red_pals() {
    assert_eq!(
        c_ffi::STARTBONUSPALS,
        c_ffi::STARTREDPALS + c_ffi::NUMREDPALS,
        "bonus pals should immediately follow red pals"
    );
}

/// Radiation palette is at the index after the bonus palette block ends.
/// RADIATIONPAL = STARTBONUSPALS + NUMBONUSPALS.
#[test]
fn radiation_pal_follows_bonus_pals() {
    assert_eq!(
        c_ffi::RADIATIONPAL,
        c_ffi::STARTBONUSPALS + c_ffi::NUMBONUSPALS,
        "RADIATIONPAL should follow the bonus palette block"
    );
}

// ---------------------------------------------------------------------------
// Face-sprite table structure constants
// ---------------------------------------------------------------------------

/// `ST_NUMPAINFACES = 5`: five pain levels (0%–100% health in 20% bands).
#[test]
fn st_numpainfaces_is_5() {
    assert_eq!(c_ffi::ST_NUMPAINFACES, 5);
}

/// `ST_NUMSTRAIGHTFACES = 3`: three forward-looking face variants per pain level.
#[test]
fn st_numstraightfaces_is_3() {
    assert_eq!(c_ffi::ST_NUMSTRAIGHTFACES, 3);
}

/// `ST_NUMTURNFACES = 2`: two turning face variants (left/right).
#[test]
fn st_numturnfaces_is_2() {
    assert_eq!(c_ffi::ST_NUMTURNFACES, 2);
}

/// `ST_NUMSPECIALFACES = 3`: three special face states (ouch, evil-grin, rampage).
#[test]
fn st_numspecialfaces_is_3() {
    assert_eq!(c_ffi::ST_NUMSPECIALFACES, 3);
}

/// `ST_NUMEXTRAFACES = 2`: two extra face states (god mode, dead).
#[test]
fn st_numextrafaces_is_2() {
    assert_eq!(c_ffi::ST_NUMEXTRAFACES, 2);
}

/// `ST_NUMFACES` is derived as pain × stride + extras.
/// = 5 × (3 + 2 + 3) + 2 = 5 × 8 + 2 = 42.
#[test]
fn st_numfaces_is_42() {
    let stride = c_ffi::ST_NUMSTRAIGHTFACES + c_ffi::ST_NUMTURNFACES + c_ffi::ST_NUMSPECIALFACES;
    let expected = c_ffi::ST_NUMPAINFACES * stride + c_ffi::ST_NUMEXTRAFACES;
    assert_eq!(c_ffi::ST_NUMFACES, expected);
    assert_eq!(c_ffi::ST_NUMFACES, 42);
}

// ---------------------------------------------------------------------------
// Face-state timing constants (in tics)
// ---------------------------------------------------------------------------

/// `ST_EVILGRINCOUNT = 2 × TICRATE` = 70 tics (2 seconds).
#[test]
fn st_evilgrincount_is_2_seconds() {
    assert_eq!(c_ffi::ST_EVILGRINCOUNT, 2 * TICRATE);
    assert_eq!(c_ffi::ST_EVILGRINCOUNT, 70);
}

/// `ST_STRAIGHTFACECOUNT = TICRATE / 2` = 17 tics (half second).
#[test]
fn st_straightfacecount_is_half_second() {
    assert_eq!(c_ffi::ST_STRAIGHTFACECOUNT, TICRATE / 2);
    assert_eq!(c_ffi::ST_STRAIGHTFACECOUNT, 17);
}

/// `ST_TURNCOUNT = TICRATE` = 35 tics (1 second).
#[test]
fn st_turncount_is_one_second() {
    assert_eq!(c_ffi::ST_TURNCOUNT, TICRATE);
    assert_eq!(c_ffi::ST_TURNCOUNT, 35);
}

/// `ST_OUCHCOUNT = TICRATE` = 35 tics (1 second).
#[test]
fn st_ouchcount_is_one_second() {
    assert_eq!(c_ffi::ST_OUCHCOUNT, TICRATE);
}

/// `ST_RAMPAGEDELAY = 2 × TICRATE` = 70 tics (2 seconds).
#[test]
fn st_rampagedelay_is_2_seconds() {
    assert_eq!(c_ffi::ST_RAMPAGEDELAY, 2 * TICRATE);
    assert_eq!(c_ffi::ST_RAMPAGEDELAY, 70);
}

/// Evil-grin duration equals rampage delay (both are 2 seconds).
#[test]
fn evilgrin_equals_rampagedelay() {
    assert_eq!(c_ffi::ST_EVILGRINCOUNT, c_ffi::ST_RAMPAGEDELAY);
}

/// `ST_MUCHPAIN = 20`: damage must exceed 20 to trigger the rampage face.
#[test]
fn st_muchpain_is_20() {
    assert_eq!(c_ffi::ST_MUCHPAIN, 20);
}

// ---------------------------------------------------------------------------
// Screen-position constants
// ---------------------------------------------------------------------------

/// `ST_X = 0`: the status bar starts at the left edge of the screen.
#[test]
fn st_x_is_zero() {
    assert_eq!(c_ffi::ST_X, 0);
}

/// `ST_X2 = 104`: the arms-display portion starts at pixel 104.
#[test]
fn st_x2_is_104() {
    assert_eq!(c_ffi::ST_X2, 104);
}

/// `ST_FACEPROBABILITY = 96`: 1/96 chance per tic of a new random face.
#[test]
fn st_faceprobability_is_96() {
    assert_eq!(c_ffi::ST_FACEPROBABILITY, 96);
}
