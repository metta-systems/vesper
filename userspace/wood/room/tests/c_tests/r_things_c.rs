//! Tests for `r_things.c` — sprite-projection and rendering state.
//!
//! `r_things.c` manages sprite projection (thing → screen), clipping, and
//! drawing.  The module also initialises the sprite definition table during
//! `R_InitSprites`, which requires the WAD to be loaded.  Its globals live in
//! `SpriteState`.
//!
//! These tests cover:
//!   * Pre-init default values (zero / NULL before `R_InitSprites`)
//!   * Constants derived from the C source (`MINZ`, `BASEYCENTER`)
//!   * Array sizes for `negonearray` and `screenheightarray` (= SCREENWIDTH)

#![allow(non_snake_case)]

use room::engine::{
    interface::video::SCREENWIDTH,
    math::fixed::FRACUNIT,
    render_3d::things::{self, SpriteState},
};

// ---------------------------------------------------------------------------
// Constants derived from r_things.c / r_defs.h
// ---------------------------------------------------------------------------

/// `MINZ` is the nearest depth at which a sprite is projected.
/// C source: `#define MINZ (FRACUNIT*4)`.
#[test]
fn minz_is_four_fracunits() {
    assert_eq!(things::MINZ, FRACUNIT * 4);
    assert_eq!(things::MINZ, 4 * 65536);
}

/// `BASEYCENTER` is the pixel row used as the vertical origin for sprite
/// projection.  C source: `#define BASEYCENTER 100`.
#[test]
fn baseycenter_is_100() {
    assert_eq!(things::BASEYCENTER, 100);
}

// ---------------------------------------------------------------------------
// Sprite state – default zero / NULL before R_InitSprites
// ---------------------------------------------------------------------------

/// `pspritescale` and `pspriteiscale` are computed per-frame for the player
/// weapon sprites; they start at zero before the first rendered frame.
#[test]
fn pspritescale_default_zero() {
    assert_eq!(SpriteState::new().pspritescale, 0);
}

/// `pspriteiscale` (inverse scale) starts at zero before the first rendered frame.
#[test]
fn pspriteiscale_default_zero() {
    assert_eq!(SpriteState::new().pspriteiscale, 0);
}

/// `spritelights` is set each frame to point to the zone-light table for the
/// current light level; NULL before the first `R_DrawMasked` call.
#[test]
fn spritelights_initially_null() {
    assert!(
        SpriteState::new().spritelights.is_null(),
        "spritelights should be NULL before rendering"
    );
}

/// `sprites` (the sprite-definition table pointer) is NULL until `R_InitSprites`
/// allocates and fills it from the WAD.
#[test]
fn sprites_pointer_initially_null() {
    assert!(
        SpriteState::new().sprites.is_null(),
        "sprites should be NULL before R_InitSprites"
    );
}

/// `numsprites` is 0 until `R_InitSprites` counts the sprite lumps.
#[test]
fn numsprites_default_zero() {
    assert_eq!(SpriteState::new().numsprites, 0);
}

/// `maxframe` is used as a high-water mark during sprite initialisation;
/// starts at zero.
#[test]
fn maxframe_default_zero() {
    assert_eq!(SpriteState::new().maxframe, 0);
}

/// `spritename` is the name of the sprite being processed; empty until
/// `R_InitSprites` sets it for the first sprite.
#[test]
fn spritename_initially_empty() {
    assert!(
        SpriteState::new().spritename.is_empty(),
        "spritename should be empty before R_InitSprites"
    );
}

// ---------------------------------------------------------------------------
// Clipping arrays – length == SCREENWIDTH
// ---------------------------------------------------------------------------

/// `negonearray` is `short negonearray[SCREENWIDTH]`; used to initialise the
/// bottom-clip array for psprite rendering.
#[test]
fn negonearray_length_is_screenwidth() {
    assert_eq!(SpriteState::new().negonearray.len(), SCREENWIDTH as usize);
}

/// `screenheightarray` is `short screenheightarray[SCREENWIDTH]`; used to
/// initialise the top-clip array for psprite rendering.
#[test]
fn screenheightarray_length_is_screenwidth() {
    assert_eq!(
        SpriteState::new().screenheightarray.len(),
        SCREENWIDTH as usize
    );
}

/// Both clipping arrays start zeroed before any rendering takes place.
/// (They are filled by `R_InitSprites` and `R_ExecuteSetViewSize`.)
#[test]
fn clipping_arrays_start_zeroed() {
    let sprite_state = SpriteState::new();
    for (i, &v) in sprite_state.negonearray.iter().enumerate() {
        assert_eq!(v, 0, "negonearray[{i}] should be 0 before rendering");
    }
    for (i, &v) in sprite_state.screenheightarray.iter().enumerate() {
        assert_eq!(v, 0, "screenheightarray[{i}] should be 0 before rendering");
    }
}

// ---------------------------------------------------------------------------
// sprtemp – temporary sprite build buffer
// ---------------------------------------------------------------------------

/// `sprtemp[29]` holds per-frame data while `R_InitSprites` builds sprite
/// definitions.  It must have exactly 29 elements (hard-coded in the C source
/// for the maximum frame count of any sprite, '0' through '\\').
#[test]
fn sprtemp_has_29_elements() {
    assert_eq!(SpriteState::new().sprtemp.len(), 29);
}

/// `sprtemp` elements are zeroed before `R_InitSprites` writes to them.
#[test]
fn sprtemp_starts_zeroed() {
    for (i, frame) in SpriteState::new().sprtemp.iter().enumerate() {
        assert_eq!(frame.rotate, 0, "sprtemp[{i}].rotate");
        for (r, &l) in frame.lump.iter().enumerate() {
            assert_eq!(l, 0, "sprtemp[{i}].lump[{r}]");
        }
        for (r, &f) in frame.flip.iter().enumerate() {
            assert_eq!(f, 0, "sprtemp[{i}].flip[{r}]");
        }
    }
}
