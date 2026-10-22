//! Tests for `p_map.c` — collision-detection globals and constants.
//!
//! `p_map.c` contains `P_CheckPosition`, the slide-move resolver, the
//! line-of-sight tracer, radius-attack logic, and the use-line traversal.
//! The movement-test globals (`tmbbox`, `tmflags`, `tmx`, `tmy`, etc.) form
//! the "scratchpad" shared between all collision functions; they must be
//! zero-initialised at program start and match their exact C types.
//!
//! These tests verify:
//!   * `DEFAULT_SPECHIT_MAGIC` constant value
//!   * All collision scratchpad globals default to zero
//!   * Type widths of the globals (all `fixed_t` / `int` = 4 bytes)

#![allow(non_snake_case)]

use std::ffi::c_int;

use crate::doom::c_ffi;
use crate::doom::p_map;

// ---------------------------------------------------------------------------
// DEFAULT_SPECHIT_MAGIC — spechit overflow sentinel
// ---------------------------------------------------------------------------

/// `DEFAULT_SPECHIT_MAGIC = 0x01C09C98`.
///
/// When the number of spechit lines exceeds the array bound in vanilla Doom,
/// the overflow writes into adjacent memory.  Chocolate Doom detects this by
/// checking that the word following the array still contains this sentinel.
#[test]
fn default_spechit_magic_value() {
    assert_eq!(c_ffi::DEFAULT_SPECHIT_MAGIC, 0x01C09C98);
}

// ---------------------------------------------------------------------------
// Movement bounding box — default zero
// ---------------------------------------------------------------------------

/// `tmbbox[4]` stores BOXTOP/BOXBOTTOM/BOXLEFT/BOXRIGHT for the current
/// P_CheckPosition test.  Before any check is run the array is all zero.
#[test]
fn tmbbox_default_zero() {
    unsafe {
        for (i, &v) in p_map::tmbbox.iter().enumerate() {
            assert_eq!(v, 0, "tmbbox[{i}] should be 0 before P_CheckPosition");
        }
    }
}

/// `tmbbox` must contain exactly 4 entries (one per box edge).
#[test]
fn tmbbox_length_is_4() {
    unsafe {
        assert_eq!(p_map::tmbbox.len(), 4);
    }
}

// ---------------------------------------------------------------------------
// Movement test scalars — default zero
// ---------------------------------------------------------------------------

/// `tmflags` holds the MF_* flags of the thing under test; zero before first use.
#[test]
fn tmflags_default_zero() {
    unsafe {
        assert_eq!(
            p_map::tmflags,
            0,
            "tmflags should be 0 before P_CheckPosition"
        );
    }
}

/// `tmx` and `tmy` are the proposed new position; zero before first use.
#[test]
fn tmx_tmy_default_zero() {
    unsafe {
        assert_eq!(p_map::tmx, 0, "tmx");
        assert_eq!(p_map::tmy, 0, "tmy");
    }
}

/// `floatok` is the "move is possible" flag; false (0) before any check.
#[test]
fn floatok_default_zero() {
    unsafe {
        assert_eq!(
            p_map::floatok,
            0,
            "floatok should be false before P_CheckPosition"
        );
    }
}

/// `tmfloorz`, `tmceilingz`, and `tmdropoffz` hold the Z extents at the test
/// position; all start at zero.
#[test]
fn tm_z_globals_default_zero() {
    unsafe {
        assert_eq!(p_map::tmfloorz, 0, "tmfloorz");
        assert_eq!(p_map::tmceilingz, 0, "tmceilingz");
        assert_eq!(p_map::tmdropoffz, 0, "tmdropoffz");
    }
}

/// `numspechit` counts the special lines crossed during P_CheckPosition;
/// starts at zero.
#[test]
fn numspechit_default_zero() {
    unsafe {
        assert_eq!(
            p_map::numspechit,
            0,
            "numspechit should be 0 before P_CheckPosition"
        );
    }
}

// ---------------------------------------------------------------------------
// Ranged-attack globals — default zero
// ---------------------------------------------------------------------------

/// `shootz` is the ray origin Z for P_AimLineAttack / P_ShootLine; zero
/// before any attack traversal.
#[test]
fn shootz_default_zero() {
    unsafe {
        assert_eq!(p_map::shootz, 0, "shootz");
    }
}

/// `la_damage` is the damage for the current line-attack trace; zero before
/// any attack (0 means "aim only" in P_AimLineAttack).
#[test]
fn la_damage_default_zero() {
    unsafe {
        assert_eq!(p_map::la_damage, 0, "la_damage");
    }
}

/// `attackrange` is the maximum range of the current attack; zero before use.
#[test]
fn attackrange_default_zero() {
    unsafe {
        assert_eq!(p_map::attackrange, 0, "attackrange");
    }
}

/// `aimslope` is the computed vertical slope of the aiming trace; zero before
/// P_AimLineAttack has been called.
#[test]
fn aimslope_default_zero() {
    unsafe {
        assert_eq!(p_map::aimslope, 0, "aimslope");
    }
}

// ---------------------------------------------------------------------------
// Slide-move globals — default zero
// ---------------------------------------------------------------------------

/// `bestslidefrac` and `secondslidefrac` record the fractions to the
/// nearest and second-nearest slide walls; zero before P_SlideMove runs.
#[test]
fn slide_fracs_default_zero() {
    unsafe {
        assert_eq!(p_map::bestslidefrac, 0, "bestslidefrac");
        assert_eq!(p_map::secondslidefrac, 0, "secondslidefrac");
    }
}

// ---------------------------------------------------------------------------
// Radius-attack globals — default zero
// ---------------------------------------------------------------------------

/// `bombdamage` is the damage value for the current P_RadiusAttack sweep;
/// zero before the function is called.
#[test]
fn bombdamage_default_zero() {
    unsafe {
        assert_eq!(p_map::bombdamage, 0, "bombdamage");
    }
}

// ---------------------------------------------------------------------------
// Type-width checks
// ---------------------------------------------------------------------------

/// All collision scratchpad globals are `fixed_t` / `int` (4 bytes).
#[test]
fn collision_globals_are_c_int_width() {
    const _: () = assert!(std::mem::size_of::<c_int>() == 4);
    unsafe {
        let _: c_int = p_map::tmflags;
        let _: c_int = p_map::tmx;
        let _: c_int = p_map::tmy;
        let _: c_int = p_map::floatok;
        let _: c_int = p_map::tmfloorz;
        let _: c_int = p_map::tmceilingz;
        let _: c_int = p_map::tmdropoffz;
        let _: c_int = p_map::numspechit;
        let _: c_int = p_map::shootz;
        let _: c_int = p_map::la_damage;
        let _: c_int = p_map::attackrange;
        let _: c_int = p_map::aimslope;
        let _: c_int = p_map::bestslidefrac;
        let _: c_int = p_map::secondslidefrac;
        let _: c_int = p_map::bombdamage;
    }
}
