//! Tests for `p_inter.c` — ammo tables and interaction constants.
//!
//! `p_inter.c` handles all player–item, player–monster, and player–player
//! interactions (pick-ups, damage, kills).  The two ammo arrays (`maxammo`
//! and `clipammo`) are global initialised arrays that must survive unchanged
//! through the port; any mutation would silently break ammo economy.
//!
//! These tests verify:
//!   * Exact values of `maxammo[4]` and `clipammo[4]` (regression baseline)
//!   * The `BONUSADD` and `NUMAMMO` constants remain stable
//!   * Relationships between max and clip amounts

#![allow(non_snake_case)]

use crate::doom::c_ffi;
use crate::doom::p_inter;

// ---------------------------------------------------------------------------
// NUMAMMO / BONUSADD constants
// ---------------------------------------------------------------------------

/// There are exactly 4 ammo types (am_clip, am_shell, am_cell, am_misl).
#[test]
fn numammo_is_4() {
    assert_eq!(c_ffi::NUMAMMO, 4);
}

/// `BONUSADD = 6`: each pick-up adds 6 tics of screen-flash bonus.
#[test]
fn bonusadd_is_6() {
    assert_eq!(c_ffi::BONUSADD, 6);
}

// ---------------------------------------------------------------------------
// maxammo — per-type ammo caps
// ---------------------------------------------------------------------------

/// Bullet (clip) ammo cap is 200.
#[test]
fn maxammo_bullets_is_200() {
    unsafe {
        assert_eq!(p_inter::maxammo[0], 200, "maxammo[am_clip] should be 200");
    }
}

/// Shell ammo cap is 50.
#[test]
fn maxammo_shells_is_50() {
    unsafe {
        assert_eq!(p_inter::maxammo[1], 50, "maxammo[am_shell] should be 50");
    }
}

/// Cell ammo cap is 300.
#[test]
fn maxammo_cells_is_300() {
    unsafe {
        assert_eq!(p_inter::maxammo[2], 300, "maxammo[am_cell] should be 300");
    }
}

/// Missile ammo cap is 50.
#[test]
fn maxammo_missiles_is_50() {
    unsafe {
        assert_eq!(p_inter::maxammo[3], 50, "maxammo[am_misl] should be 50");
    }
}

/// All four maxammo entries match the verbatim C initializer: {200, 50, 300, 50}.
#[test]
fn maxammo_exact_values() {
    unsafe {
        assert_eq!(
            p_inter::maxammo,
            [200, 50, 300, 50],
            "maxammo array mismatch"
        );
    }
}

// ---------------------------------------------------------------------------
// clipammo — per-clip (single pick-up) amounts
// ---------------------------------------------------------------------------

/// A single bullet clip gives 10 rounds.
#[test]
fn clipammo_bullets_is_10() {
    unsafe {
        assert_eq!(p_inter::clipammo[0], 10, "clipammo[am_clip] should be 10");
    }
}

/// A single shell box gives 4 shells.
#[test]
fn clipammo_shells_is_4() {
    unsafe {
        assert_eq!(p_inter::clipammo[1], 4, "clipammo[am_shell] should be 4");
    }
}

/// A single energy cell gives 20 charges.
#[test]
fn clipammo_cells_is_20() {
    unsafe {
        assert_eq!(p_inter::clipammo[2], 20, "clipammo[am_cell] should be 20");
    }
}

/// A single missile gives 1 rocket.
#[test]
fn clipammo_missiles_is_1() {
    unsafe {
        assert_eq!(p_inter::clipammo[3], 1, "clipammo[am_misl] should be 1");
    }
}

/// All four clipammo entries match the verbatim C initializer: {10, 4, 20, 1}.
#[test]
fn clipammo_exact_values() {
    unsafe {
        assert_eq!(p_inter::clipammo, [10, 4, 20, 1], "clipammo array mismatch");
    }
}

// ---------------------------------------------------------------------------
// Relationships between maxammo and clipammo
// ---------------------------------------------------------------------------

/// Bullet max (200) is exactly 20 clips of 10.
#[test]
fn bullets_max_is_20_clips() {
    unsafe {
        assert_eq!(p_inter::maxammo[0], 20 * p_inter::clipammo[0]);
    }
}

/// Shell max (50) is 12.5 boxes — not a whole multiple, but the ratio must
/// remain fixed so ammo economy is not altered.
#[test]
fn shells_max_to_clip_ratio() {
    unsafe {
        // 50 / 4 = 12 with remainder 2.
        assert_eq!(p_inter::maxammo[1] / p_inter::clipammo[1], 12);
        assert_eq!(p_inter::maxammo[1] % p_inter::clipammo[1], 2);
    }
}

/// Cell max (300) is exactly 15 energy cells of 20.
#[test]
fn cells_max_is_15_clips() {
    unsafe {
        assert_eq!(p_inter::maxammo[2], 15 * p_inter::clipammo[2]);
    }
}

/// Missile max (50) is exactly 50 individual rockets.
#[test]
fn missiles_max_is_50_clips() {
    unsafe {
        assert_eq!(p_inter::maxammo[3], 50 * p_inter::clipammo[3]);
    }
}

// ---------------------------------------------------------------------------
// Type-width checks
// ---------------------------------------------------------------------------

/// Both arrays store `int` (c_int, 4 bytes).
#[test]
fn ammo_arrays_are_c_int_width() {
    use std::ffi::c_int;
    const _: () = assert!(std::mem::size_of::<c_int>() == 4);
    unsafe {
        let _: c_int = p_inter::maxammo[0];
        let _: c_int = p_inter::clipammo[0];
    }
}
