//! Tests for `r_segs.c` — wall-segment rendering state.
//!
//! `r_segs.c` holds numerous global variables that are written each frame by
//! `R_StoreWallRange` and read by the low-level column drawers.  They live in
//! `WallState`; none of the functions require level data to be loaded, and
//! the state simply starts at zero (C BSS) before the first frame is rendered.
//!
//! These tests verify:
//!   * Pre-init default values (all C globals zero-initialize)
//!   * Pointer fields are NULL before any rendering has occurred

#![allow(non_snake_case)]

use room::engine::render_3d::segs::WallState;

// ---------------------------------------------------------------------------
// Boolean flags – default zero (false)
// ---------------------------------------------------------------------------

/// `segtextured` is set true when the current seg has at least one visible
/// texture; it starts false before the first `R_StoreWallRange` call.
#[test]
fn segtextured_default_zero() {
    assert_eq!(WallState::new().segtextured, 0);
}

/// `markfloor` is true when the current seg exposes a visible floor; starts
/// false before the first `R_StoreWallRange` call.
#[test]
fn markfloor_default_zero() {
    assert_eq!(WallState::new().markfloor, 0);
}

/// `markceiling` is true when the current seg exposes a visible ceiling;
/// starts false before any rendering.
#[test]
fn markceiling_default_zero() {
    assert_eq!(WallState::new().markceiling, 0);
}

/// `maskedtexture` is true when the seg's mid-texture has transparent gaps
/// requiring deferred rendering; starts false.
#[test]
fn maskedtexture_default_zero() {
    assert_eq!(WallState::new().maskedtexture, 0);
}

// ---------------------------------------------------------------------------
// Texture-number fields – default zero
// ---------------------------------------------------------------------------

/// The three texture slots (top/bottom/mid) are indices into the texture
/// directory; index 0 means "no texture" before R_StoreWallRange fills them.
#[test]
fn texture_indices_default_zero() {
    let wall = WallState::new();
    assert_eq!(wall.toptexture, 0, "toptexture");
    assert_eq!(wall.bottomtexture, 0, "bottomtexture");
    assert_eq!(wall.midtexture, 0, "midtexture");
}

// ---------------------------------------------------------------------------
// Angle fields – default zero
// ---------------------------------------------------------------------------

/// `rw_normalangle` is the seg's surface normal angle; zero before rendering.
#[test]
fn rw_normalangle_default_zero() {
    assert_eq!(WallState::new().rw_normalangle, 0);
}

/// `rw_angle1` is the angle to the seg's first vertex relative to the view;
/// zero before rendering.
#[test]
fn rw_angle1_default_zero() {
    assert_eq!(WallState::new().rw_angle1, 0);
}

/// `rw_centerangle` is the angle between the view direction and the seg
/// normal; zero before rendering.
#[test]
fn rw_centerangle_default_zero() {
    assert_eq!(WallState::new().rw_centerangle, 0);
}

// ---------------------------------------------------------------------------
// Column-range fields – default zero
// ---------------------------------------------------------------------------

/// `rw_x` and `rw_stopx` bound the horizontal pixel range of the current wall
/// strip; both are zero before any BSP/clipping work is done.
#[test]
fn rw_column_range_default_zero() {
    let wall = WallState::new();
    assert_eq!(wall.rw_x, 0, "rw_x");
    assert_eq!(wall.rw_stopx, 0, "rw_stopx");
}

// ---------------------------------------------------------------------------
// Scale / distance / offset fields – default zero
// ---------------------------------------------------------------------------

/// `rw_offset`, `rw_distance`, `rw_scale`, and `rw_scalestep` carry the
/// perspective-scale parameters for the current wall strip; all zero before
/// rendering.
#[test]
fn rw_scale_globals_default_zero() {
    let wall = WallState::new();
    assert_eq!(wall.rw_offset, 0, "rw_offset");
    assert_eq!(wall.rw_distance, 0, "rw_distance");
    assert_eq!(wall.rw_scale, 0, "rw_scale");
    assert_eq!(wall.rw_scalestep, 0, "rw_scalestep");
}

// ---------------------------------------------------------------------------
// Texture-midpoint fields – default zero
// ---------------------------------------------------------------------------

/// The three `rw_*texturemid` fields carry the vertical mid-point within
/// each texture for the current seg; they start at zero.
#[test]
fn rw_texturemid_globals_default_zero() {
    let wall = WallState::new();
    assert_eq!(wall.rw_midtexturemid, 0, "rw_midtexturemid");
    assert_eq!(wall.rw_toptexturemid, 0, "rw_toptexturemid");
    assert_eq!(wall.rw_bottomtexturemid, 0, "rw_bottomtexturemid");
}

// ---------------------------------------------------------------------------
// World-space bounds – default zero
// ---------------------------------------------------------------------------

/// `worldtop`, `worldbottom`, `worldhigh`, `worldlow` encode the sector
/// heights in view space; all start zero before any BSP traversal.
#[test]
fn world_bounds_default_zero() {
    let wall = WallState::new();
    assert_eq!(wall.worldtop, 0, "worldtop");
    assert_eq!(wall.worldbottom, 0, "worldbottom");
    assert_eq!(wall.worldhigh, 0, "worldhigh");
    assert_eq!(wall.worldlow, 0, "worldlow");
}

// ---------------------------------------------------------------------------
// Pixel-position step fields – default zero
// ---------------------------------------------------------------------------

/// `pixhigh`, `pixlow`, `pixhighstep`, and `pixlowstep` hold the high/low
/// pixel positions and per-column steps for the current wall strip; all zero
/// before rendering.
#[test]
fn pix_step_globals_default_zero() {
    let wall = WallState::new();
    assert_eq!(wall.pixhigh, 0, "pixhigh");
    assert_eq!(wall.pixlow, 0, "pixlow");
    assert_eq!(wall.pixhighstep, 0, "pixhighstep");
    assert_eq!(wall.pixlowstep, 0, "pixlowstep");
}

// ---------------------------------------------------------------------------
// Texture-fraction step fields – default zero
// ---------------------------------------------------------------------------

/// `topfrac`, `topstep`, `bottomfrac`, and `bottomstep` are the texture-
/// fraction accumulator and per-column step for the top and bottom textures;
/// all zero before rendering.
#[test]
fn frac_step_globals_default_zero() {
    let wall = WallState::new();
    assert_eq!(wall.topfrac, 0, "topfrac");
    assert_eq!(wall.topstep, 0, "topstep");
    assert_eq!(wall.bottomfrac, 0, "bottomfrac");
    assert_eq!(wall.bottomstep, 0, "bottomstep");
}

// ---------------------------------------------------------------------------
// Pointer fields – initially NULL
// ---------------------------------------------------------------------------

/// `walllights` points to the active light table; NULL before rendering.
#[test]
fn walllights_initially_null() {
    assert!(
        WallState::new().walllights.is_null(),
        "walllights should be NULL before rendering"
    );
}

/// `maskedtexturecol` points into a drawseg's column-offset array; NULL before
/// `R_RenderMaskedSegRange` is called.
#[test]
fn maskedtexturecol_ptr_initially_null() {
    assert!(
        WallState::new().maskedtexturecol.is_null(),
        "maskedtexturecol should be NULL before rendering"
    );
}
