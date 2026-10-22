//! Tests for `r_draw.c` — fuzz effect table, viewport initialization, and globals.
//!
//! `r_draw.c` is one of the hottest code paths in the renderer (column/span drawing).
//! The fuzz/spectre effect depends on the exact `fuzzoffset[FUZZTABLE]` values;
//! any deviation would produce a different visual.  `R_InitBuffer` is pure arithmetic
//! over screen constants, so its behavior can be fully verified with unit tests.

#![allow(non_snake_case)]

use std::ffi::c_int;

use crate::doom::c_ffi;
use crate::doom::c_tests::harness::C_GLOBAL_LOCK;
use crate::doom::i_video::{SCREENHEIGHT, SCREENWIDTH};

// ---------------------------------------------------------------------------
// Screen / renderer constants
// ---------------------------------------------------------------------------

/// `SCREENWIDTH` is the vanilla Doom horizontal resolution (320 pixels).
#[test]
fn screenwidth_is_320() {
    assert_eq!(SCREENWIDTH, 320);
}

/// `SCREENHEIGHT` is the vanilla Doom vertical resolution (200 pixels).
#[test]
fn screenheight_is_200() {
    assert_eq!(SCREENHEIGHT, 200);
}

/// `SBARHEIGHT` is the status-bar pixel height (32 rows reserved at the bottom).
#[test]
fn sbarheight_is_32() {
    assert_eq!(c_ffi::SBARHEIGHT, 32);
}

// ---------------------------------------------------------------------------
// Fuzz / spectre effect constants
// ---------------------------------------------------------------------------

/// `FUZZTABLE = 50`: the fuzz/spectre effect cycles through 50 row offsets.
#[test]
fn fuzztable_size() {
    assert_eq!(c_ffi::FUZZTABLE, 50);
}

/// `FUZZOFF` equals `SCREENWIDTH`: one screen row's worth of column stride.
#[test]
fn fuzzoff_equals_screenwidth() {
    assert_eq!(c_ffi::FUZZOFF, SCREENWIDTH);
}

// ---------------------------------------------------------------------------
// fuzzoffset table — exact values from r_draw.c
//
// The 50-entry table determines which adjacent column is sampled for each
// fuzzy pixel row.  Positive means one column to the right, negative one
// to the left.  Every value must be either +SCREENWIDTH or −SCREENWIDTH.
// ---------------------------------------------------------------------------

/// Expected `fuzzoffset` values, verbatim from `r_draw.c`.
const EXPECTED_FUZZ: [c_int; 50] = [
    320, -320, 320, -320, 320, 320, -320, 320, 320, -320, 320, 320, 320, -320, 320, 320, 320, -320,
    -320, -320, -320, 320, -320, -320, 320, 320, 320, 320, -320, 320, -320, 320, 320, -320, -320,
    320, 320, -320, -320, -320, -320, 320, 320, 320, 320, -320, 320, 320, -320, 320,
];

/// `fuzzoffset[]` must contain exactly `FUZZTABLE` (50) entries.
#[test]
fn fuzzoffset_length() {
    unsafe {
        assert_eq!(c_ffi::fuzzoffset.len(), c_ffi::FUZZTABLE);
    }
}

/// Every `fuzzoffset` entry is either +FUZZOFF or -FUZZOFF (one column to
/// the right or left).
#[test]
fn fuzzoffset_all_values_are_plus_or_minus_fuzzoff() {
    unsafe {
        for (i, &v) in c_ffi::fuzzoffset.iter().enumerate() {
            assert!(
                v == c_ffi::FUZZOFF || v == -c_ffi::FUZZOFF,
                "fuzzoffset[{i}] = {v}; expected ±{foff}",
                foff = c_ffi::FUZZOFF,
            );
        }
    }
}

/// Each `fuzzoffset` entry matches the verbatim value from r_draw.c, in order.
#[test]
fn fuzzoffset_exact_values() {
    unsafe {
        for (i, (&got, &want)) in c_ffi::fuzzoffset
            .iter()
            .zip(EXPECTED_FUZZ.iter())
            .enumerate()
        {
            assert_eq!(
                got, want,
                "fuzzoffset[{i}] mismatch: got {got}, want {want}"
            );
        }
    }
}

/// The fuzz table has exactly 29 positive (+SCREENWIDTH) and 21 negative
/// (-SCREENWIDTH) entries; this asymmetry is part of the vanilla effect.
#[test]
fn fuzzoffset_positive_count() {
    // There are exactly 29 positive (+320) entries and 21 negative (−320).
    unsafe {
        let pos = c_ffi::fuzzoffset.iter().filter(|&&v| v > 0).count();
        let neg = c_ffi::fuzzoffset.iter().filter(|&&v| v < 0).count();
        assert_eq!(
            pos, 29,
            "expected 29 positive fuzzoffset entries, got {pos}"
        );
        assert_eq!(
            neg, 21,
            "expected 21 negative fuzzoffset entries, got {neg}"
        );
    }
}

// ---------------------------------------------------------------------------
// R_InitBuffer — viewport geometry arithmetic
//
// Given (width, height), R_InitBuffer sets:
//   viewwindowx = (SCREENWIDTH − width) >> 1
//   viewwindowy = 0                            if width == SCREENWIDTH
//   viewwindowy = (SCREENHEIGHT − SBARHEIGHT − height) >> 1   otherwise
// ---------------------------------------------------------------------------

/// When `R_InitBuffer` is called with a full-width view (width == SCREENWIDTH),
/// both `viewwindowx` and `viewwindowy` must be 0.
#[test]
fn r_init_buffer_fullscreen_sets_zero_offsets() {
    let _guard = C_GLOBAL_LOCK.lock().unwrap();
    // Full-screen view: width == SCREENWIDTH → both offsets must be 0.
    unsafe {
        c_ffi::R_InitBuffer(SCREENWIDTH, 168);
        assert_eq!(
            c_ffi::viewwindowx,
            0,
            "viewwindowx should be 0 for full-width view"
        );
        assert_eq!(
            c_ffi::viewwindowy,
            0,
            "viewwindowy should be 0 for full-width view"
        );
    }
}

/// For a 256-pixel-wide window, `viewwindowx` must equal `(320-256) >> 1 = 32`.
#[test]
fn r_init_buffer_windowed_sets_correct_x_offset() {
    let _guard = C_GLOBAL_LOCK.lock().unwrap();
    // Windowed view 256 wide: viewwindowx = (320 − 256) >> 1 = 32.
    unsafe {
        c_ffi::R_InitBuffer(256, 168);
        assert_eq!(
            c_ffi::viewwindowx,
            (SCREENWIDTH - 256) >> 1,
            "viewwindowx mismatch for width=256"
        );
    }
}

/// For a 200x100 windowed view, `viewwindowy = (200 - 32 - 100) >> 1 = 34`.
#[test]
fn r_init_buffer_windowed_height_sets_correct_y_offset() {
    let _guard = C_GLOBAL_LOCK.lock().unwrap();
    // Small window (200×100): viewwindowy = (200 − 32 − 100) >> 1 = 34.
    unsafe {
        c_ffi::R_InitBuffer(200, 100);
        let want_y = (SCREENHEIGHT - c_ffi::SBARHEIGHT - 100) >> 1;
        assert_eq!(
            c_ffi::viewwindowy,
            want_y,
            "viewwindowy mismatch for height=100"
        );
    }
}

/// Across a range of widths, `viewwindowx` follows `(SCREENWIDTH - w) >> 1`.
#[test]
fn r_init_buffer_various_widths_x_formula() {
    let _guard = C_GLOBAL_LOCK.lock().unwrap();
    for &w in &[160_i32, 200, 256, 280, 304, 312] {
        unsafe {
            c_ffi::R_InitBuffer(w, 100);
            let want = (SCREENWIDTH - w) >> 1;
            assert_eq!(
                c_ffi::viewwindowx,
                want,
                "viewwindowx wrong for width={w}: got {}, want {want}",
                c_ffi::viewwindowx
            );
        }
    }
}

/// Across a range of heights, `viewwindowy` follows
/// `(SCREENHEIGHT - SBARHEIGHT - h) >> 1` when width is below full screen.
#[test]
fn r_init_buffer_various_heights_y_formula() {
    let _guard = C_GLOBAL_LOCK.lock().unwrap();
    // For all heights with a sub-SCREENWIDTH width, verify the Y formula.
    let w = 256;
    let playfield = SCREENHEIGHT - c_ffi::SBARHEIGHT; // 168
    for &h in &[50_i32, 80, 100, 120, 140, 160, 168] {
        unsafe {
            c_ffi::R_InitBuffer(w, h);
            let want = (playfield - h) >> 1;
            assert_eq!(
                c_ffi::viewwindowy,
                want,
                "viewwindowy wrong for height={h}: got {}, want {want}",
                c_ffi::viewwindowy
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Draw-parameter globals — verify the C types are the right width.
// These globals are set by the BSP/clipper before each draw call.
// ---------------------------------------------------------------------------

/// All column/span draw-parameter globals (`dc_*`, `ds_*`, `fuzzpos`) are
/// `c_int` (4 bytes); verifies FFI linkage and read-without-UB.
#[test]
fn draw_globals_are_c_int_width() {
    // Compile-time: dc_x, dc_yl, dc_yh, dc_iscale, dc_texturemid, fuzzpos must
    // all be representable as i32 (= c_int on every supported target).
    const _: () = assert!(std::mem::size_of::<c_int>() == 4);
    unsafe {
        // Just read them — the goal is to ensure the FFI linkage compiles and
        // the values are valid (no UB on read).
        let _: c_int = c_ffi::dc_x;
        let _: c_int = c_ffi::dc_yl;
        let _: c_int = c_ffi::dc_yh;
        let _: c_int = c_ffi::dc_iscale;
        let _: c_int = c_ffi::dc_texturemid;
        let _: c_int = c_ffi::fuzzpos;
        let _: c_int = c_ffi::ds_y;
        let _: c_int = c_ffi::ds_x1;
        let _: c_int = c_ffi::ds_x2;
        let _: c_int = c_ffi::ds_xfrac;
        let _: c_int = c_ffi::ds_yfrac;
        let _: c_int = c_ffi::ds_xstep;
        let _: c_int = c_ffi::ds_ystep;
    }
}
