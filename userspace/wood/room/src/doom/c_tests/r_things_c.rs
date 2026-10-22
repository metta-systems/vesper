//! Tests for `r_things.c` — sprite-projection and rendering globals.
//!
//! `r_things.c` manages sprite projection (thing → screen), clipping, and
//! drawing.  The module also initialises the sprite definition table during
//! `R_InitSprites`, which requires the WAD to be loaded.
//!
//! These tests cover:
//!   * Pre-init default values (zero / NULL before `R_InitSprites`)
//!   * Struct layout of `spriteframe_t` (matches r_defs.h)
//!   * Constants derived from the C source (`MINZ`, `BASEYCENTER`)
//!   * Array sizes for `negonearray` and `screenheightarray` (= SCREENWIDTH)

#![allow(non_snake_case)]

use std::ffi::{c_int, c_short};

use crate::doom::c_ffi;
use crate::doom::i_video::SCREENWIDTH;
use crate::doom::m_fixed::FRACUNIT;

// ---------------------------------------------------------------------------
// Constants derived from r_things.c / r_defs.h
// ---------------------------------------------------------------------------

/// `MINZ` is the nearest depth at which a sprite is projected.
/// C source: `#define MINZ (FRACUNIT*4)`.
#[test]
fn minz_is_four_fracunits() {
    assert_eq!(c_ffi::MINZ, FRACUNIT * 4);
    assert_eq!(c_ffi::MINZ, 4 * 65536);
}

/// `BASEYCENTER` is the pixel row used as the vertical origin for sprite
/// projection.  C source: `#define BASEYCENTER 100`.
#[test]
fn baseycenter_is_100() {
    assert_eq!(c_ffi::BASEYCENTER, 100);
}

// ---------------------------------------------------------------------------
// spriteframe_t layout
// ---------------------------------------------------------------------------

/// The C layout of `spriteframe_t` (r_defs.h) is:
///   boolean rotate  (int,   4 bytes)  at offset  0
///   short   lump[8] (short[8], 16 bytes) at offset  4
///   byte    flip[8] (u8[8],   8 bytes) at offset 20
/// Total: 28 bytes, alignment 4.
#[test]
fn spriteframe_t_size_is_28() {
    assert_eq!(std::mem::size_of::<c_ffi::spriteframe_t>(), 28);
}

/// `spriteframe_t` alignment is 4 (dictated by the leading `int rotate` field).
#[test]
fn spriteframe_t_align_is_4() {
    assert_eq!(std::mem::align_of::<c_ffi::spriteframe_t>(), 4);
}

/// The `lump` field starts at offset 4 (after the 4-byte `rotate` int).
#[test]
fn spriteframe_t_lump_offset_is_4() {
    let frame = c_ffi::spriteframe_t {
        rotate: 0,
        lump: [0; 8],
        flip: [0; 8],
    };
    let base = &frame as *const _ as usize;
    let lump_addr = frame.lump.as_ptr() as usize;
    assert_eq!(lump_addr - base, 4, "lump[] should be at offset 4");
}

/// The `flip` field starts at offset 20 (= 4 + 16).
#[test]
fn spriteframe_t_flip_offset_is_20() {
    let frame = c_ffi::spriteframe_t {
        rotate: 0,
        lump: [0; 8],
        flip: [0; 8],
    };
    let base = &frame as *const _ as usize;
    let flip_addr = frame.flip.as_ptr() as usize;
    assert_eq!(flip_addr - base, 20, "flip[] should be at offset 20");
}

// ---------------------------------------------------------------------------
// Sprite globals – default zero / NULL before R_InitSprites
// ---------------------------------------------------------------------------

/// `pspritescale` and `pspriteiscale` are computed per-frame for the player
/// weapon sprites; they start at zero before the first rendered frame.
#[test]
fn pspritescale_default_zero() {
    unsafe {
        assert_eq!(c_ffi::pspritescale, 0);
    }
}

/// `pspriteiscale` (inverse scale) starts at zero before the first rendered frame.
#[test]
fn pspriteiscale_default_zero() {
    unsafe {
        assert_eq!(c_ffi::pspriteiscale, 0);
    }
}

/// `spritelights` is set each frame to point to the zone-light table for the
/// current light level; NULL before the first `R_DrawMasked` call.
#[test]
fn spritelights_initially_null() {
    unsafe {
        assert!(
            c_ffi::spritelights.is_null(),
            "spritelights should be NULL before rendering"
        );
    }
}

/// `sprites` (the sprite-definition table pointer) is NULL until `R_InitSprites`
/// allocates and fills it from the WAD.
#[test]
fn sprites_pointer_initially_null() {
    unsafe {
        assert!(
            c_ffi::sprites.is_null(),
            "sprites should be NULL before R_InitSprites"
        );
    }
}

/// `numsprites` is 0 until `R_InitSprites` counts the sprite lumps.
#[test]
fn numsprites_default_zero() {
    unsafe {
        assert_eq!(c_ffi::numsprites, 0);
    }
}

/// `maxframe` is used as a high-water mark during sprite initialisation;
/// starts at zero.
#[test]
fn maxframe_default_zero() {
    unsafe {
        assert_eq!(c_ffi::maxframe, 0);
    }
}

/// `spritename` points to the name of the sprite being processed; NULL until
/// `R_InitSprites` sets it for the first sprite.
#[test]
fn spritename_initially_null() {
    unsafe {
        assert!(
            c_ffi::spritename.is_null(),
            "spritename should be NULL before R_InitSprites"
        );
    }
}

// ---------------------------------------------------------------------------
// Clipping arrays – length == SCREENWIDTH
// ---------------------------------------------------------------------------

/// `negonearray` is `short negonearray[SCREENWIDTH]`; used to initialise the
/// bottom-clip array for psprite rendering.
#[test]
fn negonearray_length_is_screenwidth() {
    unsafe {
        assert_eq!(c_ffi::negonearray.len(), SCREENWIDTH as usize);
    }
}

/// `screenheightarray` is `short screenheightarray[SCREENWIDTH]`; used to
/// initialise the top-clip array for psprite rendering.
#[test]
fn screenheightarray_length_is_screenwidth() {
    unsafe {
        assert_eq!(c_ffi::screenheightarray.len(), SCREENWIDTH as usize);
    }
}

/// Both arrays store `short` (c_short) elements.
#[test]
fn clipping_arrays_element_type_is_c_short() {
    const _: () = assert!(std::mem::size_of::<c_short>() == 2);
    unsafe {
        let _: &[c_short] = &c_ffi::negonearray;
        let _: &[c_short] = &c_ffi::screenheightarray;
    }
}

/// Both clipping arrays start zeroed before any rendering takes place.
/// (They are filled by `R_DrawMasked` at the start of each frame.)
#[test]
fn clipping_arrays_start_zeroed() {
    unsafe {
        for (i, &v) in c_ffi::negonearray.iter().enumerate() {
            assert_eq!(v, 0, "negonearray[{i}] should be 0 before rendering");
        }
        for (i, &v) in c_ffi::screenheightarray.iter().enumerate() {
            assert_eq!(v, 0, "screenheightarray[{i}] should be 0 before rendering");
        }
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
    unsafe {
        assert_eq!(c_ffi::sprtemp.len(), 29);
    }
}

/// `sprtemp` elements are zeroed before `R_InitSprites` writes to them.
#[test]
fn sprtemp_starts_zeroed() {
    unsafe {
        for (i, frame) in c_ffi::sprtemp.iter().enumerate() {
            assert_eq!(frame.rotate, 0, "sprtemp[{i}].rotate");
            for (r, &l) in frame.lump.iter().enumerate() {
                assert_eq!(l, 0, "sprtemp[{i}].lump[{r}]");
            }
            for (r, &f) in frame.flip.iter().enumerate() {
                assert_eq!(f, 0, "sprtemp[{i}].flip[{r}]");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Type-width checks
// ---------------------------------------------------------------------------

/// r_things.c stores scales as `fixed_t` (= `int` = 4 bytes).
#[test]
fn psprite_scale_globals_are_c_int() {
    const _: () = assert!(std::mem::size_of::<c_int>() == 4);
    unsafe {
        let _: c_int = c_ffi::pspritescale;
        let _: c_int = c_ffi::pspriteiscale;
    }
}
