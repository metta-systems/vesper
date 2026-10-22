//! Tests for `wi_stuff.c` — intermission screen constants.
//!
//! `wi_stuff.c` draws the level-complete summary screen between levels.
//! The layout constants (`WI_TITLEY`, `WI_SPACINGY`, `SP_STATSX`, etc.) and
//! the map-count constants (`WI_NUMEPISODES`, `WI_NUMMAPS`) are compile-time
//! values that must be preserved exactly or the screen layout will break.
//!
//! These tests verify:
//!   * Episode / map count constants
//!   * Screen layout constants (positions and spacings)
//!   * Derived relationships between constants

#![allow(non_snake_case)]

use crate::doom::c_ffi;
use crate::doom::i_video::SCREENHEIGHT;

// ---------------------------------------------------------------------------
// Episode / map count constants
// ---------------------------------------------------------------------------

/// `NUMEPISODES = 4`: Doom has four episodes (three for shareware/registered,
/// four for Ultimate Doom).
#[test]
fn wi_numepisodes_is_4() {
    assert_eq!(c_ffi::WI_NUMEPISODES, 4);
}

/// `NUMMAPS = 9`: each episode has nine maps (ExM1 … ExM9).
#[test]
fn wi_nummaps_is_9() {
    assert_eq!(c_ffi::WI_NUMMAPS, 9);
}

/// There are 4 × 9 = 36 unique maps in Doom (ignoring secret exits).
#[test]
fn total_doom_maps_is_36() {
    assert_eq!(c_ffi::WI_NUMEPISODES * c_ffi::WI_NUMMAPS, 36);
}

// ---------------------------------------------------------------------------
// Screen-layout constants
// ---------------------------------------------------------------------------

/// `WI_TITLEY = 2` pixels from the top of the screen for the level title.
#[test]
fn wi_titley_is_2() {
    assert_eq!(c_ffi::WI_TITLEY, 2);
}

/// `WI_SPACINGY = 33` pixels between rows in the multi-player stats grid.
#[test]
fn wi_spacingy_is_33() {
    assert_eq!(c_ffi::WI_SPACINGY, 33);
}

/// `SP_STATSX = 50` pixels: left edge of the single-player stats column.
#[test]
fn sp_statsx_is_50() {
    assert_eq!(c_ffi::SP_STATSX, 50);
}

/// `SP_STATSY = 50` pixels: top edge of the single-player stats area.
#[test]
fn sp_statsy_is_50() {
    assert_eq!(c_ffi::SP_STATSY, 50);
}

/// `SP_TIMEX = 16` pixels: left edge of the time display.
#[test]
fn sp_timex_is_16() {
    assert_eq!(c_ffi::SP_TIMEX, 16);
}

/// `SP_TIMEY = SCREENHEIGHT − 32 = 168` pixels from the top.
#[test]
fn sp_timey_is_screenheight_minus_32() {
    assert_eq!(c_ffi::SP_TIMEY, SCREENHEIGHT - 32);
    assert_eq!(c_ffi::SP_TIMEY, 168);
}

/// `SHOWNEXTLOCDELAY = 4` tics before showing the "next level" map dot.
#[test]
fn shownextlocdelay_is_4() {
    assert_eq!(c_ffi::SHOWNEXTLOCDELAY, 4);
}

/// `DM_SPACINGX = 40` pixels between deathmatch score columns.
#[test]
fn dm_spacingx_is_40() {
    assert_eq!(c_ffi::DM_SPACINGX, 40);
}

// ---------------------------------------------------------------------------
// Derived relationships
// ---------------------------------------------------------------------------

/// The stats area starts above the time display.
#[test]
fn stats_y_above_time_y() {
    assert!(
        c_ffi::SP_STATSY < c_ffi::SP_TIMEY,
        "SP_STATSY ({}) should be above SP_TIMEY ({})",
        c_ffi::SP_STATSY,
        c_ffi::SP_TIMEY
    );
}

/// The title row is at the top of the screen (very small Y value).
#[test]
fn title_y_near_top() {
    assert!(
        c_ffi::WI_TITLEY < 10,
        "WI_TITLEY should be near the top of the screen"
    );
}

// ---------------------------------------------------------------------------
// Animation table constants
// ---------------------------------------------------------------------------

/// Episode 0 (Knee-Deep in the Dead) has 10 animated background elements.
#[test]
fn epsd0_animinfo_count() {
    assert_eq!(c_ffi::WI_EPSD0_NANIM, 10);
}

/// Episode 1 (The Shores of Hell) has 9 animated background elements.
#[test]
fn epsd1_animinfo_count() {
    assert_eq!(c_ffi::WI_EPSD1_NANIM, 9);
}

/// Episode 2 (Inferno) has 6 animated background elements.
#[test]
fn epsd2_animinfo_count() {
    assert_eq!(c_ffi::WI_EPSD2_NANIM, 6);
}
