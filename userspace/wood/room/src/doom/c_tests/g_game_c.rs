//! Tests for `g_game.c` — movement speed tables and game-state defaults.
//!
//! `g_game.c` owns the core game loop, the player-input movement tables, and
//! miscellaneous game-state globals.  The movement tables (`forwardmove`,
//! `sidemove`, `angleturn`) are static initialisers — they are **non-zero**
//! at program start and must not change between runs.
//!
//! These tests verify:
//!   * Exact values of the movement speed tables (regression baseline)
//!   * Relationships between slow/fast/normal speeds
//!   * Default values of game-state flags that are set at startup
//!   * Constants (`BODYQUESIZE`, `TURBOTHRESHOLD`, `SLOWTURNTICS`) remain stable

#![allow(non_snake_case)]

use crate::doom::c_ffi;

// ---------------------------------------------------------------------------
// Forward-movement speed table
// ---------------------------------------------------------------------------

/// Slow-walk forward speed must be 0x19 = 25 fixed-point units per tic.
/// This value has been stable since the original Doom 1.9 release.
#[test]
fn forwardmove_slow_is_0x19() {
    unsafe {
        assert_eq!(
            c_ffi::forwardmove[0],
            0x19,
            "slow forwardmove should be 0x19 (25)"
        );
    }
}

/// Fast-run forward speed must be 0x32 = 50 fixed-point units per tic.
#[test]
fn forwardmove_fast_is_0x32() {
    unsafe {
        assert_eq!(
            c_ffi::forwardmove[1],
            0x32,
            "fast forwardmove should be 0x32 (50)"
        );
    }
}

/// The fast forward speed is exactly double the slow speed.
/// Vanilla Doom: `{0x19, 0x32}` = `{25, 50}`.
#[test]
fn forwardmove_fast_is_double_slow() {
    unsafe {
        assert_eq!(
            c_ffi::forwardmove[1],
            c_ffi::forwardmove[0] * 2,
            "fast forwardmove should be 2× slow"
        );
    }
}

// ---------------------------------------------------------------------------
// Lateral (strafe) movement speed table
// ---------------------------------------------------------------------------

/// Slow strafe speed is 0x18 = 24 units/tic.
#[test]
fn sidemove_slow_is_0x18() {
    unsafe {
        assert_eq!(
            c_ffi::sidemove[0],
            0x18,
            "slow sidemove should be 0x18 (24)"
        );
    }
}

/// Fast strafe speed is 0x28 = 40 units/tic.
#[test]
fn sidemove_fast_is_0x28() {
    unsafe {
        assert_eq!(
            c_ffi::sidemove[1],
            0x28,
            "fast sidemove should be 0x28 (40)"
        );
    }
}

// ---------------------------------------------------------------------------
// Turn-speed table
// ---------------------------------------------------------------------------

/// Normal turn speed is 640 BAM (binary-angle-measurement) units per tic.
#[test]
fn angleturn_normal_is_640() {
    unsafe {
        assert_eq!(c_ffi::angleturn[0], 640, "normal angleturn should be 640");
    }
}

/// Fast turn speed (with the run key) is 1280 BAM units per tic.
#[test]
fn angleturn_fast_is_1280() {
    unsafe {
        assert_eq!(c_ffi::angleturn[1], 1280, "fast angleturn should be 1280");
    }
}

/// Slow turn speed is 320 BAM units per tic.  This is used for the first
/// `SLOWTURNTICS` (6) tics after a turn key is pressed; it prevents the
/// player from accidentally over-rotating when they just tap the key.
#[test]
fn angleturn_slow_is_320() {
    unsafe {
        assert_eq!(c_ffi::angleturn[2], 320, "slow angleturn should be 320");
    }
}

/// Fast turn speed is exactly 2× normal turn speed.
#[test]
fn angleturn_fast_is_double_normal() {
    unsafe {
        assert_eq!(
            c_ffi::angleturn[1],
            c_ffi::angleturn[0] * 2,
            "fast angleturn should be 2× normal"
        );
    }
}

/// Slow turn speed is exactly half the normal turn speed.
/// This is the quirk that makes the player momentarily turn slowly when a
/// turn key is first pressed; important to preserve in any Rust port.
#[test]
fn angleturn_slow_is_half_normal() {
    unsafe {
        assert_eq!(
            c_ffi::angleturn[2],
            c_ffi::angleturn[0] / 2,
            "slow angleturn should be angleturn[0]/2"
        );
    }
}

// ---------------------------------------------------------------------------
// MAXPLMOVE / TURBOTHRESHOLD relationship
// ---------------------------------------------------------------------------

/// `MAXPLMOVE` is `#define MAXPLMOVE (forwardmove[1])` in g_game.c.
/// The turbo-cheat detector compares `mom` > `TURBOTHRESHOLD * MAXPLMOVE`
/// per axis; if `forwardmove[1]` ever changes, the turbo logic breaks.
#[test]
fn forwardmove_fast_equals_turbothreshold() {
    unsafe {
        // TURBOTHRESHOLD = 0x32 = 50 = forwardmove[1]
        assert_eq!(
            c_ffi::forwardmove[1],
            c_ffi::TURBOTHRESHOLD,
            "forwardmove[1] (MAXPLMOVE) should equal TURBOTHRESHOLD (0x32)"
        );
    }
}

// ---------------------------------------------------------------------------
// SLOWTURNTICS constant
// ---------------------------------------------------------------------------

/// `SLOWTURNTICS` is 6: the first 6 tics of a turn use `angleturn[2]`;
/// after that, the normal or fast speed is used.
#[test]
fn slowturntics_is_6() {
    assert_eq!(c_ffi::SLOWTURNTICS, 6);
}

// ---------------------------------------------------------------------------
// BODYQUESIZE constant
// ---------------------------------------------------------------------------

/// `BODYQUESIZE` is 32: the circular body-queue has 32 slots.
/// Player corpses cycle through this ring; if the constant changes,
/// corpse-disappearance timing changes.
#[test]
fn bodyquesize_is_32() {
    assert_eq!(c_ffi::BODYQUESIZE, 32);
}

// ---------------------------------------------------------------------------
// Game-state globals – default values set at program start
// ---------------------------------------------------------------------------

/// `vanilla_savegame_limit = 1` by default: saves are limited to the vanilla
/// 0x2c000-byte maximum unless `-nosavelimit` is used.
#[test]
fn vanilla_savegame_limit_is_one() {
    unsafe {
        assert_eq!(c_ffi::vanilla_savegame_limit, 1);
    }
}

/// `vanilla_demo_limit = 1` by default: demos respect the vanilla size cap.
#[test]
fn vanilla_demo_limit_is_one() {
    unsafe {
        assert_eq!(c_ffi::vanilla_demo_limit, 1);
    }
}

/// `precache = true` by default: all graphics are preloaded at level start
/// unless `-noprecache` is passed on the command line.
#[test]
fn precache_is_true_by_default() {
    unsafe {
        assert_ne!(
            c_ffi::precache,
            0,
            "precache should be true (non-zero) by default"
        );
    }
}

/// `testcontrols` starts false; it is set true only by `-testcontrols`.
#[test]
fn testcontrols_is_false_by_default() {
    unsafe {
        assert_eq!(c_ffi::testcontrols, 0);
    }
}

/// `bodyqueslot` is the write index into the ring buffer; starts at zero.
#[test]
fn bodyqueslot_default_zero() {
    unsafe {
        assert_eq!(c_ffi::bodyqueslot, 0);
    }
}

/// Level-start tic and kill/item/secret totals are all zero before a level
/// has been loaded.
#[test]
fn level_counters_default_zero() {
    unsafe {
        assert_eq!(c_ffi::levelstarttic, 0, "levelstarttic");
        assert_eq!(c_ffi::totalkills, 0, "totalkills");
        assert_eq!(c_ffi::totalitems, 0, "totalitems");
        assert_eq!(c_ffi::totalsecret, 0, "totalsecret");
    }
}

// ---------------------------------------------------------------------------
// Type-width checks
// ---------------------------------------------------------------------------

/// All movement-table entries are `fixed_t` (= C `int` = 4 bytes).
#[test]
fn movement_tables_are_c_int_width() {
    use std::ffi::c_int;
    const _: () = assert!(std::mem::size_of::<c_int>() == 4);
    unsafe {
        let _: c_int = c_ffi::forwardmove[0];
        let _: c_int = c_ffi::forwardmove[1];
        let _: c_int = c_ffi::sidemove[0];
        let _: c_int = c_ffi::sidemove[1];
        let _: c_int = c_ffi::angleturn[0];
        let _: c_int = c_ffi::angleturn[1];
        let _: c_int = c_ffi::angleturn[2];
    }
}
