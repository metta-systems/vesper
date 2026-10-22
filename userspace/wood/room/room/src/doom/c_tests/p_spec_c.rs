//! Tests for `p_spec.c` — sector specials, animation constants, and globals.
//!
//! `p_spec.c` is the dispatcher for all special sector and line actions:
//! doors, floors, ceilings, lights, and platform effects.  The constants that
//! govern their speeds and timing (`VDOORSPEED`, `PLATSPEED`, etc.) must match
//! exactly in any Rust port, or all door/floor/ceiling animations will run at
//! the wrong speed.
//!
//! These tests verify:
//!   * `leveltime` is zero before any level has run
//!   * Animation count limits (`MAXANIMS`, `MAXLINEANIMS`)
//!   * Door, ceiling, platform, and floor speed/wait constants
//!   * Light animation constants (`GLOWSPEED`, `STROBEBRIGHT`, etc.)

#![allow(non_snake_case)]

use crate::doom::c_ffi;
use crate::doom::i_timer::TICRATE;
use crate::doom::m_fixed::FRACUNIT;

// ---------------------------------------------------------------------------
// leveltime — current game tic within the level
// ---------------------------------------------------------------------------

/// `leveltime` is the tic counter for the current level; zero before any
/// level has been loaded.
#[test]
fn leveltime_default_zero() {
    unsafe {
        assert_eq!(
            c_ffi::leveltime,
            0,
            "leveltime should be 0 before level load"
        );
    }
}

// ---------------------------------------------------------------------------
// Animation count limits
// ---------------------------------------------------------------------------

/// `MAXANIMS = 32`: no more than 32 simultaneous floor/ceiling/door animations.
#[test]
fn maxanims_is_32() {
    assert_eq!(c_ffi::MAXANIMS, 32);
}

/// `MAXLINEANIMS = 64`: the line-special list can hold up to 64 active entries.
#[test]
fn maxlineanims_is_64() {
    assert_eq!(c_ffi::MAXLINEANIMS, 64);
}

// ---------------------------------------------------------------------------
// Door speed / wait constants
// ---------------------------------------------------------------------------

/// `VDOORSPEED = FRACUNIT × 2`.  All standard vertical doors move at this
/// speed.  Any deviation changes the tempo of every door in the game.
#[test]
fn vdoorspeed_is_2_fracunits() {
    assert_eq!(c_ffi::VDOORSPEED, FRACUNIT * 2);
    assert_eq!(c_ffi::VDOORSPEED, 2 * 65536);
}

/// `VDOORWAIT = 150` tics ≈ 4.3 seconds before a door auto-closes.
#[test]
fn vdoorwait_is_150() {
    assert_eq!(c_ffi::VDOORWAIT, 150);
}

// ---------------------------------------------------------------------------
// Ceiling speed / wait constants
// ---------------------------------------------------------------------------

/// `CEILSPEED = FRACUNIT`.  All standard ceiling crushers move 1 map unit/tic.
#[test]
fn ceilspeed_is_one_fracunit() {
    assert_eq!(c_ffi::CEILSPEED, FRACUNIT);
    assert_eq!(c_ffi::CEILSPEED, 65536);
}

/// `CEILWAIT = 150` tics: the pause between crusher strokes.
#[test]
fn ceilwait_is_150() {
    assert_eq!(c_ffi::CEILWAIT, 150);
}

/// `MAXCEILINGS = 30`: at most 30 simultaneously active ceiling effects.
#[test]
fn maxceilings_is_30() {
    assert_eq!(c_ffi::MAXCEILINGS, 30);
}

// ---------------------------------------------------------------------------
// Platform speed / wait constants
// ---------------------------------------------------------------------------

/// `PLATSPEED = FRACUNIT`.  All standard platforms move 1 map unit/tic.
#[test]
fn platspeed_is_one_fracunit() {
    assert_eq!(c_ffi::PLATSPEED, FRACUNIT);
    assert_eq!(c_ffi::PLATSPEED, 65536);
}

/// `PLATWAIT = 3` seconds (at TICRATE tics/sec) before a platform returns.
#[test]
fn platwait_is_3() {
    assert_eq!(c_ffi::PLATWAIT, 3);
}

/// `MAXPLATS = 30`: at most 30 simultaneously active platforms.
#[test]
fn maxplats_is_30() {
    assert_eq!(c_ffi::MAXPLATS, 30);
}

// ---------------------------------------------------------------------------
// Floor speed constants
// ---------------------------------------------------------------------------

/// `FLOORSPEED = FRACUNIT`.  Standard floor movements advance 1 map unit/tic.
#[test]
fn floorspeed_is_one_fracunit() {
    assert_eq!(c_ffi::FLOORSPEED, FRACUNIT);
    assert_eq!(c_ffi::FLOORSPEED, 65536);
}

// ---------------------------------------------------------------------------
// Light animation constants
// ---------------------------------------------------------------------------

/// `GLOWSPEED = 8` fixed-point light units/tic for glow effects.
#[test]
fn glowspeed_is_8() {
    assert_eq!(c_ffi::GLOWSPEED, 8);
}

/// `STROBEBRIGHT = 5` tics for the bright phase of strobe lights.
#[test]
fn strobebright_is_5() {
    assert_eq!(c_ffi::STROBEBRIGHT, 5);
}

/// `FASTDARK = 15` tics for the dark phase of fast-strobe lights.
#[test]
fn fastdark_is_15() {
    assert_eq!(c_ffi::FASTDARK, 15);
}

/// `SLOWDARK = 35` tics for the dark phase of slow-strobe lights.
/// Equals TICRATE (1 second), making slow strobes pulse once per second.
#[test]
fn slowdark_is_35() {
    assert_eq!(c_ffi::SLOWDARK, 35);
    assert_eq!(c_ffi::SLOWDARK, TICRATE);
}

// ---------------------------------------------------------------------------
// Speed relationships
// ---------------------------------------------------------------------------

/// Door, ceiling, platform, and floor speeds are all integer multiples of
/// FRACUNIT (no fractional component), ensuring whole-unit movement per tic.
#[test]
fn movement_speeds_are_fracunit_multiples() {
    assert_eq!(c_ffi::VDOORSPEED % FRACUNIT, 0, "VDOORSPEED");
    assert_eq!(c_ffi::CEILSPEED % FRACUNIT, 0, "CEILSPEED");
    assert_eq!(c_ffi::PLATSPEED % FRACUNIT, 0, "PLATSPEED");
    assert_eq!(c_ffi::FLOORSPEED % FRACUNIT, 0, "FLOORSPEED");
}

/// Door speed is exactly 2× platform/floor/ceiling speed.
#[test]
fn vdoorspeed_is_double_platspeed() {
    assert_eq!(c_ffi::VDOORSPEED, 2 * c_ffi::PLATSPEED);
}

/// Both wait constants are equal (150 tics = same pause for doors and crushers).
#[test]
fn door_and_ceil_wait_are_equal() {
    assert_eq!(c_ffi::VDOORWAIT, c_ffi::CEILWAIT);
}
