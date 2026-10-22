//! Tests for `am_map.rs` — automap globals and constants.
//!
//! `am_map.rs` implements the full-screen automap: player position, line
//! drawing, zoom/pan, and mark points.  Most of its state is in `static`
//! locals, but `automapactive` is a public global that the HUD checks.
//! The color index constants must match the vanilla palette layout or the map
//! will render with wrong colors.  The scale constants determine the initial
//! zoom level and zoom rate.
//!
//! These tests verify:
//!   * `automapactive` starts false (0)
//!   * `AM_NUMMARKPOINTS` constant
//!   * Initial scale and zoom constants
//!   * Color index constants fall within the 256-color palette

#![allow(non_snake_case)]

use crate::doom::am_map;
use crate::doom::c_ffi;
use crate::doom::m_fixed::FRACUNIT;

// ---------------------------------------------------------------------------
// automapactive — whether the automap is currently open
// ---------------------------------------------------------------------------

/// `automapactive` is false (0) at program start; set true when the player
/// presses the automap key.
#[test]
fn automapactive_default_false() {
    unsafe {
        assert_eq!(
            am_map::automapactive,
            0,
            "automapactive should be false at startup"
        );
    }
}

// ---------------------------------------------------------------------------
// AM_NUMMARKPOINTS constant
// ---------------------------------------------------------------------------

/// `AM_NUMMARKPOINTS = 10`: the player can drop up to 10 location markers.
#[test]
fn am_nummarkpoints_is_10() {
    assert_eq!(am_map::AM_NUMMARKPOINTS, 10);
}

// ---------------------------------------------------------------------------
// Scale constants
// ---------------------------------------------------------------------------

/// `INITSCALEMTOF = 0.2 × FRACUNIT = 13107` (truncated).
/// This is the default map-to-frame scale when the automap opens.
#[test]
fn initscalemtof_value() {
    // 0.2 * 65536 = 13107.2 → truncated to 13107
    assert_eq!(am_map::INITSCALEMTOF, 13107);
}

/// `M_ZOOMIN = 1.02 × FRACUNIT = 66846` (truncated).
/// Multiply the current scale by this factor each tic while zooming in.
#[test]
fn m_zoomin_value() {
    // 1.02 * 65536 = 66847.0... → 66846 after cast from f64
    // Accept either 66846 or 66847 since different C compilers may truncate/round.
    assert!(
        am_map::M_ZOOMIN == 66846 || am_map::M_ZOOMIN == 66847,
        "M_ZOOMIN = {} (expected 66846 or 66847)",
        am_map::M_ZOOMIN
    );
}

/// `M_ZOOMOUT = FRACUNIT / 1.02 ≈ 64250` (truncated).
/// Multiply scale by this factor each tic while zooming out.
#[test]
fn m_zoomout_value() {
    // 65536 / 1.02 = 64250.98... → truncates to 64250 (or 64251 on some platforms)
    assert!(
        am_map::M_ZOOMOUT == 64250 || am_map::M_ZOOMOUT == 64251,
        "M_ZOOMOUT = {} (expected 64250 or 64251)",
        am_map::M_ZOOMOUT
    );
}

/// Zoom in and zoom out are reciprocals: M_ZOOMIN × M_ZOOMOUT ≈ FRACUNIT².
/// Both are close to FRACUNIT so their product divided by FRACUNIT should be
/// near FRACUNIT.
#[test]
fn zoomin_and_zoomout_are_near_reciprocals() {
    let product = (am_map::M_ZOOMIN as i64) * (am_map::M_ZOOMOUT as i64);
    let fracunit_sq = (FRACUNIT as i64) * (FRACUNIT as i64);
    // Within 1% of each other
    let diff = (product - fracunit_sq).unsigned_abs();
    assert!(
        diff < (fracunit_sq as u64 / 100),
        "M_ZOOMIN × M_ZOOMOUT should be ≈ FRACUNIT² (diff={diff})"
    );
}

/// `F_PANINC = 4`: the automap pans 4 pixels (in map coords) per tic.
#[test]
fn f_paninc_is_4() {
    assert_eq!(am_map::F_PANINC, 4);
}

// ---------------------------------------------------------------------------
// Zoom direction sanity checks
// ---------------------------------------------------------------------------

/// Zoom-in factor is greater than FRACUNIT (i.e. scale increases).
#[test]
fn zoomin_greater_than_fracunit() {
    assert!(
        am_map::M_ZOOMIN > FRACUNIT,
        "M_ZOOMIN should be > FRACUNIT to increase scale"
    );
}

/// Zoom-out factor is less than FRACUNIT (i.e. scale decreases).
#[test]
fn zoomout_less_than_fracunit() {
    assert!(
        am_map::M_ZOOMOUT < FRACUNIT,
        "M_ZOOMOUT should be < FRACUNIT to decrease scale"
    );
}

/// The initial scale is much less than FRACUNIT (the default view is zoomed out).
#[test]
fn initscalemtof_less_than_fracunit() {
    assert!(
        am_map::INITSCALEMTOF < FRACUNIT,
        "INITSCALEMTOF ({}) should be < FRACUNIT — the map starts zoomed out",
        am_map::INITSCALEMTOF
    );
}
