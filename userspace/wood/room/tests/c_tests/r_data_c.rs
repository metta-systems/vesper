//! Tests for `r_data.rs` — texture/flat data subsystem.
//!
//! Most of `r_data.rs` requires the WAD to be loaded via the full game
//! initialization chain (`D_DoomMain` → `W_Init` → `R_Init` → `R_InitData`),
//! which does **not** happen in the unit-test binary.
//!
//! These tests therefore focus on:
//!   * Pre-init default values (`RenderDataState` starts at zero)
//!   * Constants that `r_data.rs` relies on, verifying they are stable

#![allow(non_snake_case)]

use room::engine::{
    math::fixed::{FRACBITS, FRACUNIT},
    render_3d::data::RenderDataState,
};

// ---------------------------------------------------------------------------
// Pre-init state
// ---------------------------------------------------------------------------

/// Before `R_InitData` runs, the lump ranges and counts are all zero and the
/// lookup tables are unallocated.
#[test]
fn r_data_state_zero_before_init() {
    let data = RenderDataState::new();
    assert_eq!((data.firstflat, data.lastflat, data.numflats), (0, 0, 0));
    assert_eq!(
        (
            data.firstspritelump,
            data.lastspritelump,
            data.numspritelumps
        ),
        (0, 0, 0)
    );
    assert_eq!(data.numtextures, 0);
    assert!(data.textureheight.is_null() && data.colormaps.is_null());
}

// ---------------------------------------------------------------------------
// Constants used internally by r_data.rs / r_textures
// ---------------------------------------------------------------------------

/// `FRACBITS` is used throughout the texture-coordinate arithmetic in
/// `r_data.rs` as well as many other modules.  It must be exactly 16.
#[test]
fn fracbits_is_16() {
    assert_eq!(FRACBITS, 16);
}

/// `FRACUNIT` = 1 << 16.  Encoded in the WAD and used in offset calculations.
#[test]
fn fracunit_is_65536() {
    assert_eq!(FRACUNIT, 65536);
}
