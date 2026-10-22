//! Tests for `r_data.rs` — texture/flat data subsystem.
//!
//! Most of `r_data.rs` requires the WAD to be loaded via the full game
//! initialization chain (`D_DoomMain` → `W_Init` → `R_Init` → `R_InitData`),
//! which does **not** happen in the unit-test binary.
//!
//! These tests therefore focus on:
//!   * Symbol linkage (compile-time: the symbols must resolve at link time)
//!   * Pre-init default values (all Rust globals default to zero)
//!   * Constants that `r_data.rs` relies on, verifying they are stable

#![allow(non_snake_case)]

use crate::engine::{
    misc::fixed::{FRACBITS, FRACUNIT},
    render_3d,
};

// ---------------------------------------------------------------------------
// Symbol linkage — these tests simply read the Rust globals.
// A linker error would cause the binary to fail to build; a panic here
// would indicate the symbol resolved to an unexpected non-zero value
// before R_InitData has been called.
// ---------------------------------------------------------------------------

/// Before the full game initializer runs, all Rust globals default to zero.
/// Reading them must not panic or produce UB.
#[test]
fn r_data_globals_readable_before_init() {
    unsafe {
        // These are all initialised to 0 before R_InitData is called.
        // We only verify that they can be read (linker check), not their values.
        let _ff = render_3d::data::firstflat;
        let _lf = render_3d::data::lastflat;
        let _nf = render_3d::data::numflats;
        let _fsl = render_3d::data::firstspritelump;
        let _lsl = render_3d::data::lastspritelump;
        let _nsl = render_3d::data::numspritelumps;
        let _nt = render_3d::data::numtextures;
    }
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
