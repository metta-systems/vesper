//! Tests for `p_enemy.c` — monster AI globals and constants.
//!
//! `p_enemy.c` implements all monster AI: target acquisition, pathfinding,
//! attack dispatch, and the brain (Spider Mastermind teleportation) logic.
//! The `xspeed[]` and `yspeed[]` tables map the 8 movement directions to
//! fixed-point velocity components; they must be identical in any Rust port or
//! all monster movement directions will be wrong.
//!
//! These tests verify:
//!   * Exact values of the `xspeed` and `yspeed` direction tables
//!   * `TRACEANGLE`, `FATSPREAD`, and `SKULLSPEED` constant values
//!   * Brain-target globals start at zero / NULL

#![allow(non_snake_case)]

use std::ffi::c_int;

use crate::doom::c_ffi;
use crate::doom::m_fixed::FRACUNIT;
use crate::doom::tables::ANG90;

// ---------------------------------------------------------------------------
// xspeed / yspeed — 8-directional movement velocity tables
// ---------------------------------------------------------------------------

/// `xspeed[8]` contains the X velocity component for each of the 8 movement
/// directions (East=0, NE=1, N=2, NW=3, W=4, SW=5, S=6, SE=7).
/// Values (verbatim from p_enemy.c): {FRACUNIT, 47000, 0, -47000,
///   -FRACUNIT, -47000, 0, 47000}.
#[test]
fn xspeed_exact_values() {
    let expected: [c_int; 8] = [FRACUNIT, 47000, 0, -47000, -FRACUNIT, -47000, 0, 47000];
    unsafe {
        for (i, (&got, &want)) in c_ffi::xspeed.iter().zip(expected.iter()).enumerate() {
            assert_eq!(got, want, "xspeed[{i}]: got {got}, want {want}");
        }
    }
}

/// `yspeed[8]` contains the Y velocity component for each direction.
/// Values: {0, 47000, FRACUNIT, 47000, 0, -47000, -FRACUNIT, -47000}.
#[test]
fn yspeed_exact_values() {
    let expected: [c_int; 8] = [0, 47000, FRACUNIT, 47000, 0, -47000, -FRACUNIT, -47000];
    unsafe {
        for (i, (&got, &want)) in c_ffi::yspeed.iter().zip(expected.iter()).enumerate() {
            assert_eq!(got, want, "yspeed[{i}]: got {got}, want {want}");
        }
    }
}

/// Both speed tables have exactly 8 entries (one per movement direction).
#[test]
fn speed_tables_have_8_entries() {
    unsafe {
        assert_eq!(c_ffi::xspeed.len(), 8, "xspeed");
        assert_eq!(c_ffi::yspeed.len(), 8, "yspeed");
    }
}

/// The cardinal-direction entries (E, N, W, S) are ±FRACUNIT or 0.
/// The diagonal entries use 47000 (slightly less than FRACUNIT/√2 ≈ 46341)
/// so that diagonal movement travels slightly farther than cardinal — a
/// vanilla quirk that must be preserved.
#[test]
fn diagonal_speed_is_47000() {
    unsafe {
        // NE: xspeed[1]=47000, yspeed[1]=47000
        assert_eq!(c_ffi::xspeed[1], 47000);
        assert_eq!(c_ffi::yspeed[1], 47000);
        // NW: xspeed[3]=-47000, yspeed[3]=47000
        assert_eq!(c_ffi::xspeed[3], -47000);
        assert_eq!(c_ffi::yspeed[3], 47000);
    }
}

/// Cardinal-direction Y-speed at due-East (dir 0) is 0.
#[test]
fn east_direction_has_zero_yspeed() {
    unsafe {
        assert_eq!(c_ffi::yspeed[0], 0, "east yspeed should be 0");
    }
}

/// Opposite directions have negated X and Y components.
#[test]
fn opposite_directions_are_negated() {
    unsafe {
        // East (0) vs West (4)
        assert_eq!(c_ffi::xspeed[0], -c_ffi::xspeed[4]);
        assert_eq!(c_ffi::yspeed[0], -c_ffi::yspeed[4]);
        // North (2) vs South (6)
        assert_eq!(c_ffi::xspeed[2], -c_ffi::xspeed[6]);
        assert_eq!(c_ffi::yspeed[2], -c_ffi::yspeed[6]);
    }
}

// ---------------------------------------------------------------------------
// TRACEANGLE constant
// ---------------------------------------------------------------------------

/// `TRACEANGLE = 0xc000000`.  This is the BAM turn-rate applied each tic
/// when a homing missile tries to home in on its target.
#[test]
fn traceangle_value() {
    assert_eq!(c_ffi::TRACEANGLE, 0xc000000);
}

// ---------------------------------------------------------------------------
// FATSPREAD / SKULLSPEED constants
// ---------------------------------------------------------------------------

/// `FATSPREAD = ANG90 / 8`.  The angular spread between Mancubus fireballs.
#[test]
fn fatspread_is_ang90_over_8() {
    assert_eq!(c_ffi::FATSPREAD, ANG90 / 8);
}

/// `SKULLSPEED = 20 × FRACUNIT`.  The Lost Soul charges at 20 map units/tic.
#[test]
fn skullspeed_is_20_fracunits() {
    assert_eq!(c_ffi::SKULLSPEED, 20 * FRACUNIT);
    assert_eq!(c_ffi::SKULLSPEED, 20 * 65536);
}

// ---------------------------------------------------------------------------
// Brain-target globals — default zero / NULL
// ---------------------------------------------------------------------------

/// `braintargets[32]` holds pointers to Boss Brain teleport targets; all NULL
/// before `P_SpawnBrainTargets` runs.
#[test]
fn braintargets_initially_null() {
    unsafe {
        for (i, &ptr) in c_ffi::braintargets.iter().enumerate() {
            assert!(
                ptr.is_null(),
                "braintargets[{i}] should be NULL before spawn"
            );
        }
    }
}

/// `braintargets` has exactly 32 slots.
#[test]
fn braintargets_has_32_slots() {
    unsafe {
        assert_eq!(c_ffi::braintargets.len(), 32);
    }
}

/// `numbraintargets` is zero before `P_SpawnBrainTargets` fills the list.
#[test]
fn numbraintargets_default_zero() {
    unsafe {
        assert_eq!(c_ffi::numbraintargets, 0);
    }
}

/// `braintargeton` is the round-robin index; starts at 0.
#[test]
fn braintargeton_default_zero() {
    unsafe {
        assert_eq!(c_ffi::braintargeton, 0);
    }
}

// ---------------------------------------------------------------------------
// Type-width checks
// ---------------------------------------------------------------------------

/// Both speed tables store `fixed_t` (= C `int`, 4 bytes) elements.
#[test]
fn speed_tables_are_c_int_width() {
    const _: () = assert!(std::mem::size_of::<c_int>() == 4);
    unsafe {
        let _: c_int = c_ffi::xspeed[0];
        let _: c_int = c_ffi::yspeed[0];
    }
}
