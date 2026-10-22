//! Tests for `p_pspr.c` — weapon-sprite globals and constants.
//!
//! `p_pspr.c` handles player weapon animations (raising, lowering, firing)
//! and the weapon-bob oscillation.  The module has a small number of globals
//! and several important `#define` constants that govern weapon movement speed
//! and position limits.
//!
//! These tests verify:
//!   * `LOWERSPEED` and `RAISESPEED` constants (= FRACUNIT × 6)
//!   * `WEAPONBOTTOM` and `WEAPONTOP` position constants
//!   * Relationships between the position constants
//!   * `swingx` and `swingy` default to zero before any tic runs

#![allow(non_snake_case)]

use crate::doom::c_ffi;
use crate::doom::m_fixed::FRACUNIT;

// ---------------------------------------------------------------------------
// Weapon speed constants
// ---------------------------------------------------------------------------

/// `LOWERSPEED = FRACUNIT × 6`.  The weapon descends this many fixed-point
/// units per tic when being lowered (e.g. after firing the BFG or swapping
/// weapons).
#[test]
fn lowerspeed_is_6_fracunits() {
    assert_eq!(c_ffi::LOWERSPEED, FRACUNIT * 6);
    assert_eq!(c_ffi::LOWERSPEED, 6 * 65536);
    assert_eq!(c_ffi::LOWERSPEED, 393216);
}

/// `RAISESPEED = FRACUNIT × 6`.  The weapon rises at the same speed it
/// descends, producing symmetric raise/lower animations.
#[test]
fn raisespeed_is_6_fracunits() {
    assert_eq!(c_ffi::RAISESPEED, FRACUNIT * 6);
    assert_eq!(c_ffi::RAISESPEED, 6 * 65536);
    assert_eq!(c_ffi::RAISESPEED, 393216);
}

/// Lower and raise speeds are identical in vanilla Doom.
#[test]
fn lowerspeed_equals_raisespeed() {
    assert_eq!(c_ffi::LOWERSPEED, c_ffi::RAISESPEED);
}

// ---------------------------------------------------------------------------
// Weapon position constants
// ---------------------------------------------------------------------------

/// `WEAPONBOTTOM = 128 × FRACUNIT`.  The Y-position at which the weapon
/// sprite is considered fully off-screen (below the status bar).  During
/// weapon-switch animations, `psy` starts at `WEAPONBOTTOM` and moves up.
#[test]
fn weaponbottom_is_128_fracunits() {
    assert_eq!(c_ffi::WEAPONBOTTOM, 128 * FRACUNIT);
    assert_eq!(c_ffi::WEAPONBOTTOM, 128 * 65536);
    assert_eq!(c_ffi::WEAPONBOTTOM, 8388608);
}

/// `WEAPONTOP = 32 × FRACUNIT`.  The Y-position at which the weapon sprite is
/// fully raised and ready to fire.  The weapon rests at this position during
/// normal gameplay.
#[test]
fn weapontop_is_32_fracunits() {
    assert_eq!(c_ffi::WEAPONTOP, 32 * FRACUNIT);
    assert_eq!(c_ffi::WEAPONTOP, 32 * 65536);
    assert_eq!(c_ffi::WEAPONTOP, 2097152);
}

/// `WEAPONBOTTOM` is exactly 4× `WEAPONTOP`.  This relationship determines
/// how far off-screen the weapon travels: 128 / 32 = 4.
#[test]
fn weaponbottom_is_4x_weapontop() {
    assert_eq!(c_ffi::WEAPONBOTTOM, 4 * c_ffi::WEAPONTOP);
}

/// `WEAPONBOTTOM − WEAPONTOP` is the total travel distance.
/// = (128 − 32) × FRACUNIT = 96 × FRACUNIT.
#[test]
fn weapon_travel_range_is_96_fracunits() {
    let travel = c_ffi::WEAPONBOTTOM - c_ffi::WEAPONTOP;
    assert_eq!(travel, 96 * FRACUNIT);
}

// ---------------------------------------------------------------------------
// Bob / swing globals – default zero before any tic
// ---------------------------------------------------------------------------

/// `swingx` is the horizontal component of the weapon-bob offset; it is zero
/// before `P_CalcSwing` has been called (i.e. before the first game tic).
#[test]
fn swingx_default_zero() {
    unsafe {
        assert_eq!(c_ffi::swingx, 0, "swingx should be 0 before any tic");
    }
}

/// `swingy` is the vertical component of the weapon-bob offset.  Like
/// `swingx`, it starts at zero and is only updated by `P_CalcSwing`.
#[test]
fn swingy_default_zero() {
    unsafe {
        assert_eq!(c_ffi::swingy, 0, "swingy should be 0 before any tic");
    }
}

/// Both bob components are `fixed_t` (C `int`, 4 bytes).
#[test]
fn swing_globals_are_c_int_width() {
    use std::ffi::c_int;
    const _: () = assert!(std::mem::size_of::<c_int>() == 4);
    unsafe {
        let _: c_int = c_ffi::swingx;
        let _: c_int = c_ffi::swingy;
    }
}

// ---------------------------------------------------------------------------
// Relationship with FRACUNIT / FRACBITS
// ---------------------------------------------------------------------------

/// All p_pspr.c positional constants are multiples of `FRACUNIT = 1 << 16`.
/// If `FRACBITS` or `FRACUNIT` ever changes, all these constants break.
#[test]
fn pspr_constants_are_whole_fracunit_multiples() {
    assert_eq!(c_ffi::LOWERSPEED % FRACUNIT, 0, "LOWERSPEED");
    assert_eq!(c_ffi::RAISESPEED % FRACUNIT, 0, "RAISESPEED");
    assert_eq!(c_ffi::WEAPONBOTTOM % FRACUNIT, 0, "WEAPONBOTTOM");
    assert_eq!(c_ffi::WEAPONTOP % FRACUNIT, 0, "WEAPONTOP");
}
