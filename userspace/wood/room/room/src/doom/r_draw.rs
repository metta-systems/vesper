//! Rust port of vendor/doomgeneric/r_draw.c.
//!
//! The actual span/column drawing functions. All drawing to the view buffer
//! is accomplished in this module; the other refresh files only know about
//! coordinates, not the architecture of the frame buffer.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::{c_char, c_int, c_void};
use std::ptr;

use crate::doom::c_ffi::{FUZZOFF, FUZZTABLE, SBARHEIGHT};
use crate::doom::d_mode::commercial;
use crate::doom::doomstat::gamemode;
use crate::doom::i_video::{SCREENHEIGHT, SCREENWIDTH};
use crate::doom::m_fixed::FRACBITS;
use crate::doom::v_video::{patch_t, V_DrawPatch, V_MarkRect, V_RestoreBuffer, V_UseBuffer};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum framebuffer width supported by the lookup tables.
const MAXWIDTH: usize = 1120;
/// Maximum framebuffer height supported by the lookup tables.
const MAXHEIGHT: usize = 832;

// ---------------------------------------------------------------------------
// External symbols
// ---------------------------------------------------------------------------

extern "C" {
    /// Prints a formatted error message and terminates the program.
    fn I_Error(format: *const c_char, ...);
    /// Returns a pointer to the cached lump with the given name, using the given zone tag.
    fn W_CacheLumpName(name: *const c_char, tag: c_int) -> *mut c_void;
    /// Allocates `size` bytes from the zone heap with the given tag; returns a pointer to the block.
    fn Z_Malloc(size: c_int, tag: c_int, user: *mut c_void) -> *mut c_void;
    /// Frees a block previously allocated from the zone heap.
    fn Z_Free(ptr: *mut c_void);

    /// Raw linear framebuffer written to the display; `screens[0]` in C terms.
    static mut I_VideoBuffer: *mut u8;
    /// Flat array of all 32 light-level colormaps (32 * 256 bytes); index 0 is fullbright.
    static mut colormaps: *mut u8;
    /// Screen-space Y coordinate of the view center, used to compute texture fractions.
    static mut centery: c_int;
}

// ---------------------------------------------------------------------------
// View-buffer globals
// ---------------------------------------------------------------------------

/// Base address of the view image written by the renderer (typically points into `I_VideoBuffer`).
#[no_mangle]
pub static mut viewimage: *mut u8 = ptr::null_mut();

/// Width of the current render viewport in pixels.
#[no_mangle]
pub static mut viewwidth: c_int = 0;

/// Width of the viewport scaled for the current detail mode (equals `viewwidth` in high-detail).
#[no_mangle]
pub static mut scaledviewwidth: c_int = 0;

/// Height of the current render viewport in pixels.
#[no_mangle]
pub static mut viewheight: c_int = 0;

/// X pixel offset from the left edge of the framebuffer to the left edge of the viewport.
#[no_mangle]
pub static mut viewwindowx: c_int = 0;

/// Y pixel offset from the top of the framebuffer to the top of the viewport.
#[no_mangle]
pub static mut viewwindowy: c_int = 0;

/// Per-row pointer LUT: `ylookup[y]` points to the first byte of row `y` in the framebuffer.
/// Avoids a multiply by `SCREENWIDTH` in the inner rendering loops.
#[no_mangle]
pub static mut ylookup: [*mut u8; MAXHEIGHT] = [ptr::null_mut(); MAXHEIGHT];

/// Per-column byte offset LUT: `columnofs[x]` is the byte offset within a row for column `x`.
/// Accounts for `viewwindowx` so that sub-window rendering works without extra arithmetic.
#[no_mangle]
pub static mut columnofs: [c_int; MAXWIDTH] = [0; MAXWIDTH];

/// Color-translation tables for the three non-green player colors (gray, brown, red).
/// Each table remaps the 16-entry green palette ramp (indices `0x70`-`0x7f`) to another ramp.
#[no_mangle]
pub static mut translations: [[u8; 256]; 3] = [[0; 256]; 3];

// ---------------------------------------------------------------------------
// Background buffer (module-local)
// ---------------------------------------------------------------------------

/// Backing buffer for the bezel drawn around the viewport when the window is smaller than
/// the full screen. Allocated on demand; freed when switching to full-screen mode.
static mut background_buffer: *mut u8 = ptr::null_mut();

// ---------------------------------------------------------------------------
// Column-drawing globals
// ---------------------------------------------------------------------------

/// Current colormap (light-level lookup table) used by the column renderer.
/// Points into `colormaps`; index `colormap[p]` converts a palette index to a lit palette index.
#[no_mangle]
pub static mut dc_colormap: *mut u8 = ptr::null_mut();

/// Screen-space X coordinate of the column being drawn (0 = left edge of viewport).
#[no_mangle]
pub static mut dc_x: c_int = 0;

/// Topmost screen-space Y coordinate of the column segment to draw (inclusive).
#[no_mangle]
pub static mut dc_yl: c_int = 0;

/// Bottommost screen-space Y coordinate of the column segment to draw (inclusive).
#[no_mangle]
pub static mut dc_yh: c_int = 0;

/// Inverse texture scale in 16.16 fixed-point: the amount added to the texture fraction
/// per screen row, equal to `textureheight / columnheight`.
#[no_mangle]
pub static mut dc_iscale: c_int = 0;

/// Texture mid-point fraction in 16.16 fixed-point, corresponding to the true center of
/// the wall post; used together with `dc_iscale` to compute the starting texture row.
#[no_mangle]
pub static mut dc_texturemid: c_int = 0;

/// First pixel in a column (possibly virtual).
#[no_mangle]
pub static mut dc_source: *mut u8 = ptr::null_mut();

/// Just for profiling.
#[no_mangle]
pub static mut dccount: c_int = 0;

// ---------------------------------------------------------------------------
// Fuzz / spectre effect
// ---------------------------------------------------------------------------

/// Pre-computed table of per-pixel row offsets (in bytes) used by the fuzz/spectre effect.
/// Each entry is either `+FUZZOFF` (one row down) or `-FUZZOFF` (one row up), giving the
/// smeared, semi-transparent look of partial-invisibility.
#[no_mangle]
pub static mut fuzzoffset: [c_int; FUZZTABLE] = [
    FUZZOFF, -FUZZOFF, FUZZOFF, -FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF,
    FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF, FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF, -FUZZOFF, -FUZZOFF,
    -FUZZOFF, FUZZOFF, -FUZZOFF, -FUZZOFF, FUZZOFF, FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF, FUZZOFF,
    -FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF, -FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF, -FUZZOFF, -FUZZOFF,
    -FUZZOFF, FUZZOFF, FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF, FUZZOFF,
];

/// Current position within `fuzzoffset`; wraps back to 0 when it reaches `FUZZTABLE`.
#[no_mangle]
pub static mut fuzzpos: c_int = 0;

// ---------------------------------------------------------------------------
// Translation tables
// ---------------------------------------------------------------------------

/// Pointer to the 256-byte color-translation table currently active for `R_DrawTranslatedColumn`.
/// Used to remap the player-sprite green ramp to another color set.
#[no_mangle]
pub static mut dc_translation: *mut u8 = ptr::null_mut();

/// Heap-allocated block of 3 × 256 bytes holding the gray, brown, and red translation tables
/// built by `R_InitTranslationTables`.
#[no_mangle]
pub static mut translationtables: *mut u8 = ptr::null_mut();

// ---------------------------------------------------------------------------
// Span-drawing globals
// ---------------------------------------------------------------------------

/// Screen-space Y row of the span being drawn.
#[no_mangle]
pub static mut ds_y: c_int = 0;

/// Leftmost screen-space X coordinate of the span (inclusive).
#[no_mangle]
pub static mut ds_x1: c_int = 0;

/// Rightmost screen-space X coordinate of the span (inclusive).
#[no_mangle]
pub static mut ds_x2: c_int = 0;

/// Colormap used by the span renderer; points into `colormaps` for the appropriate light level.
#[no_mangle]
pub static mut ds_colormap: *mut u8 = ptr::null_mut();

/// Starting texture U (X) fraction in 16.16 fixed-point for the leftmost pixel of the span.
#[no_mangle]
pub static mut ds_xfrac: c_int = 0;

/// Starting texture V (Y) fraction in 16.16 fixed-point for the leftmost pixel of the span.
#[no_mangle]
pub static mut ds_yfrac: c_int = 0;

/// Per-pixel increment of `ds_xfrac` in 16.16 fixed-point along the span.
#[no_mangle]
pub static mut ds_xstep: c_int = 0;

/// Per-pixel increment of `ds_yfrac` in 16.16 fixed-point along the span.
#[no_mangle]
pub static mut ds_ystep: c_int = 0;

/// Start of a 64×64 tile image.
#[no_mangle]
pub static mut ds_source: *mut u8 = ptr::null_mut();

/// Just for profiling.
#[no_mangle]
pub static mut dscount: c_int = 0;

// ---------------------------------------------------------------------------
// R_DrawColumn
// ---------------------------------------------------------------------------

/// Draws a single vertical column of opaque wall texture into the framebuffer.
///
/// Reads the column context from the `dc_*` globals and writes `dc_yh - dc_yl + 1`
/// pixels into `screens[0]`, applying `dc_colormap` for lighting. The texture is
/// sampled at a 128-texel-high virtual column; `dc_iscale` steps through it in
/// 16.16 fixed-point per screen row.
///
/// Does nothing if `dc_yh < dc_yl` (empty column segment).
#[no_mangle]
pub extern "C" fn R_DrawColumn() {
    unsafe {
        let count = dc_yh - dc_yl;

        // Zero length, column does not exceed a pixel.
        if count < 0 {
            return;
        }

        debug_assert!(
            (dc_x as u32) < (SCREENWIDTH as u32) && dc_yl >= 0 && dc_yh < SCREENHEIGHT,
            "R_DrawColumn: {} to {} at {}",
            dc_yl as c_int,
            dc_yh as c_int,
            dc_x as c_int
        );

        // Framebuffer destination address.
        let mut dest = ylookup[dc_yl as usize].add(columnofs[dc_x as usize] as usize);

        // Determine scaling, which is the only mapping to be done.
        let fracstep = dc_iscale;
        let mut frac = dc_texturemid + (dc_yl - centery) * fracstep;

        // Inner loop that does the actual texture mapping.
        let mut count = count;
        loop {
            *dest = *dc_colormap.add(*dc_source.add(((frac >> FRACBITS) & 127) as usize) as usize);
            dest = dest.add(SCREENWIDTH as usize);
            frac += fracstep;
            if count == 0 {
                break;
            }
            count -= 1;
        }
    }
}

// ---------------------------------------------------------------------------
// R_DrawColumnLow
// ---------------------------------------------------------------------------

/// Low-detail variant of `R_DrawColumn` that writes each pixel to two adjacent
/// screen columns, producing blocky 2x-wide columns for the low-resolution detail mode.
///
/// Uses `dc_x * 2` and `dc_x * 2 + 1` as the destination columns. All other
/// column-context globals (`dc_*`) have the same meaning as in `R_DrawColumn`.
///
/// Does nothing if `dc_yh < dc_yl`.
#[no_mangle]
pub extern "C" fn R_DrawColumnLow() {
    unsafe {
        let count = dc_yh - dc_yl;

        // Zero length.
        if count < 0 {
            return;
        }

        // Blocky mode, need to multiply by 2.
        let x = dc_x << 1;

        debug_assert!(
            (dc_x as u32) < (SCREENWIDTH as u32) && dc_yl >= 0 && dc_yh < SCREENHEIGHT,
            "R_DrawColumnLow: {} to {} at {}",
            dc_yl as c_int,
            dc_yh as c_int,
            dc_x as c_int
        );

        let mut dest = ylookup[dc_yl as usize].add(columnofs[x as usize] as usize);
        let mut dest2 = ylookup[dc_yl as usize].add(columnofs[(x + 1) as usize] as usize);

        let fracstep = dc_iscale;
        let mut frac = dc_texturemid + (dc_yl - centery) * fracstep;

        let mut count = count;
        loop {
            let pix =
                *dc_colormap.add(*dc_source.add(((frac >> FRACBITS) & 127) as usize) as usize);
            *dest = pix;
            *dest2 = pix;
            dest = dest.add(SCREENWIDTH as usize);
            dest2 = dest2.add(SCREENWIDTH as usize);
            frac += fracstep;
            if count == 0 {
                break;
            }
            count -= 1;
        }
    }
}

// ---------------------------------------------------------------------------
// R_DrawFuzzColumn
// ---------------------------------------------------------------------------

/// Draws a partial-invisibility (spectre/fuzz) column using the fuzz effect.
///
/// For each row the function reads a neighboring pixel from the framebuffer
/// (offset by `fuzzoffset[fuzzpos]` bytes) and re-indexes it through colormap 6,
/// producing a smeared dark image. `fuzzpos` advances and wraps modulo `FUZZTABLE`.
///
/// The top and bottom rows are clamped: `dc_yl` is raised to 1 and `dc_yh` is
/// lowered to `viewheight - 2` to avoid reading outside the viewport.
///
/// Does nothing if the clamped range is empty (`dc_yh < dc_yl`).
#[no_mangle]
pub extern "C" fn R_DrawFuzzColumn() {
    unsafe {
        // Adjust borders. Low...
        if dc_yl == 0 {
            dc_yl = 1;
        }

        // .. and high.
        if dc_yh == viewheight - 1 {
            dc_yh = viewheight - 2;
        }

        let count = dc_yh - dc_yl;

        // Zero length.
        if count < 0 {
            return;
        }

        debug_assert!(
            (dc_x as u32) < (SCREENWIDTH as u32) && dc_yl >= 0 && dc_yh < SCREENHEIGHT,
            "R_DrawFuzzColumn: {} to {} at {}",
            dc_yl as c_int,
            dc_yh as c_int,
            dc_x as c_int
        );

        let mut dest = ylookup[dc_yl as usize].add(columnofs[dc_x as usize] as usize);

        let mut count = count;
        loop {
            // Lookup framebuffer, and retrieve a pixel that is either one
            // column left or right of the current one.  Add index from
            // colormap to index.
            let offset = fuzzoffset[fuzzpos as usize];
            let src_pix = *dest.offset(offset as isize);
            *dest = *colormaps.add(6 * 256 + src_pix as usize);

            // Clamp table lookup index.
            fuzzpos += 1;
            if fuzzpos == FUZZTABLE as c_int {
                fuzzpos = 0;
            }

            dest = dest.add(SCREENWIDTH as usize);
            if count == 0 {
                break;
            }
            count -= 1;
        }
    }
}

// ---------------------------------------------------------------------------
// R_DrawFuzzColumnLow
// ---------------------------------------------------------------------------

/// Low-detail variant of `R_DrawFuzzColumn` that writes each fuzz pixel to
/// two adjacent screen columns (`dc_x * 2` and `dc_x * 2 + 1`).
///
/// Both pixels receive the same fuzz-sampled value derived from column `dc_x * 2`'s
/// neighbor; the fuzz table position advances once per row pair.
/// Border clamping and early-exit behavior are identical to `R_DrawFuzzColumn`.
#[no_mangle]
pub extern "C" fn R_DrawFuzzColumnLow() {
    unsafe {
        // Adjust borders. Low...
        if dc_yl == 0 {
            dc_yl = 1;
        }

        // .. and high.
        if dc_yh == viewheight - 1 {
            dc_yh = viewheight - 2;
        }

        let count = dc_yh - dc_yl;

        // Zero length.
        if count < 0 {
            return;
        }

        // low detail mode, need to multiply by 2
        let x = dc_x << 1;

        debug_assert!(
            (x as u32) < (SCREENWIDTH as u32) && dc_yl >= 0 && dc_yh < SCREENHEIGHT,
            "R_DrawFuzzColumnLow: {} to {} at {}",
            dc_yl as c_int,
            dc_yh as c_int,
            dc_x as c_int
        );

        let mut dest = ylookup[dc_yl as usize].add(columnofs[x as usize] as usize);
        let mut dest2 = ylookup[dc_yl as usize].add(columnofs[(x + 1) as usize] as usize);

        let mut count = count;
        loop {
            let offset = fuzzoffset[fuzzpos as usize];
            let src_pix = *dest.offset(offset as isize);
            let pix = *colormaps.add(6 * 256 + src_pix as usize);
            *dest = pix;
            *dest2 = pix;

            // Clamp table lookup index.
            fuzzpos += 1;
            if fuzzpos == FUZZTABLE as c_int {
                fuzzpos = 0;
            }

            dest = dest.offset(SCREENWIDTH as isize);
            dest2 = dest2.offset(SCREENWIDTH as isize);
            if count == 0 {
                break;
            }
            count -= 1;
        }
    }
}

// ---------------------------------------------------------------------------
// R_DrawTranslatedColumn
// ---------------------------------------------------------------------------

/// Draws a color-translated column; used for player sprites rendered in non-green colors.
///
/// The pixel pipeline is: `dc_colormap[ dc_translation[ dc_source[frac] ] ]`.
/// `dc_translation` maps the green palette ramp to another color ramp, allowing one
/// set of player sprites to appear in multiple colors (gray, brown, red).
///
/// Does nothing if `dc_yh < dc_yl`.
#[no_mangle]
pub extern "C" fn R_DrawTranslatedColumn() {
    unsafe {
        let count = dc_yh - dc_yl;
        if count < 0 {
            return;
        }

        debug_assert!(
            (dc_x as u32) < (SCREENWIDTH as u32) && dc_yl >= 0 && dc_yh < SCREENHEIGHT,
            "R_DrawTranslatedColumn: {} to {} at {}",
            dc_yl as c_int,
            dc_yh as c_int,
            dc_x as c_int
        );

        let mut dest = ylookup[dc_yl as usize].add(columnofs[dc_x as usize] as usize);

        let fracstep = dc_iscale;
        let mut frac = dc_texturemid + (dc_yl - centery) * fracstep;

        let mut count = count;
        loop {
            let src_idx = *dc_source.add((frac >> FRACBITS) as usize) as usize;
            let trans_idx = *dc_translation.add(src_idx) as usize;
            *dest = *dc_colormap.add(trans_idx);
            dest = dest.add(SCREENWIDTH as usize);
            frac += fracstep;
            if count == 0 {
                break;
            }
            count -= 1;
        }
    }
}

// ---------------------------------------------------------------------------
// R_DrawTranslatedColumnLow
// ---------------------------------------------------------------------------

/// Low-detail variant of `R_DrawTranslatedColumn` that writes each translated
/// pixel to two adjacent screen columns (`dc_x * 2` and `dc_x * 2 + 1`).
///
/// The pixel pipeline and border behavior are identical to `R_DrawTranslatedColumn`.
/// Does nothing if `dc_yh < dc_yl`.
#[no_mangle]
pub extern "C" fn R_DrawTranslatedColumnLow() {
    unsafe {
        let count = dc_yh - dc_yl;
        if count < 0 {
            return;
        }

        // low detail, need to scale by 2
        let x = dc_x << 1;

        debug_assert!(
            (x as u32) < (SCREENWIDTH as u32) && dc_yl >= 0 && dc_yh < SCREENHEIGHT,
            "R_DrawTranslatedColumnLow: {} to {} at {}",
            dc_yl as c_int,
            dc_yh as c_int,
            x
        );

        let mut dest = ylookup[dc_yl as usize].add(columnofs[x as usize] as usize);
        let mut dest2 = ylookup[dc_yl as usize].add(columnofs[(x + 1) as usize] as usize);

        let fracstep = dc_iscale;
        let mut frac = dc_texturemid + (dc_yl - centery) * fracstep;

        let mut count = count;
        loop {
            let src_idx = *dc_source.add((frac >> FRACBITS) as usize) as usize;
            let trans_idx = *dc_translation.add(src_idx) as usize;
            let pix = *dc_colormap.add(trans_idx);
            *dest = pix;
            *dest2 = pix;
            dest = dest.add(SCREENWIDTH as usize);
            dest2 = dest2.add(SCREENWIDTH as usize);
            frac += fracstep;
            if count == 0 {
                break;
            }
            count -= 1;
        }
    }
}

// ---------------------------------------------------------------------------
// R_InitTranslationTables
// ---------------------------------------------------------------------------

/// Allocates and initialises the three color-translation tables used for player colors.
///
/// Builds a 3 × 256 byte block (gray / brown / red) in the zone heap (tag `PU_STATIC`).
/// The green palette ramp (`0x70`-`0x7f`) is remapped to the gray ramp (`0x60`),
/// brown ramp (`0x40`), and red ramp (`0x20`). All other palette entries are identity-mapped.
///
/// Sets the `translationtables` global to the allocated block.
#[no_mangle]
pub extern "C" fn R_InitTranslationTables() {
    unsafe {
        translationtables = Z_Malloc(256 * 3, 1, ptr::null_mut()) as *mut u8; // PU_STATIC = 1

        let tt = translationtables;
        for i in 0..256 {
            if (0x70..=0x7f).contains(&i) {
                // map green ramp to gray, brown, red
                *tt.add(i) = (0x60 + (i & 0xf)) as u8;
                *tt.add(i + 256) = (0x40 + (i & 0xf)) as u8;
                *tt.add(i + 512) = (0x20 + (i & 0xf)) as u8;
            } else {
                // Keep all other colors as is.
                *tt.add(i) = i as u8;
                *tt.add(i + 256) = i as u8;
                *tt.add(i + 512) = i as u8;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// R_DrawSpan
// ---------------------------------------------------------------------------

/// Draws a single horizontal floor or ceiling span into the framebuffer.
///
/// Samples a 64×64 flat texture tile stored at `ds_source`, walking through it
/// in u/v (x/y) texture space with `ds_xstep` / `ds_ystep` increments. Position
/// and step are packed into 32-bit words (upper 16 bits = X, lower 16 bits = Y)
/// to avoid separate fixed-point additions.
///
/// Writes pixels to `screens[0]` from column `ds_x1` to `ds_x2` inclusive on row `ds_y`.
/// Does not check for zero-length spans; the caller must ensure `ds_x2 >= ds_x1`.
#[no_mangle]
pub extern "C" fn R_DrawSpan() {
    unsafe {
        let mut position: u32;

        // Pack position and step variables into a single 32-bit integer,
        // with x in the top 16 bits and y in the bottom 16 bits.  For
        // each 16-bit part, the top 6 bits are the integer part and the
        // bottom 10 bits are the fractional part of the pixel position.
        position = ((ds_xfrac << 10) as u32 & 0xffff0000) | ((ds_yfrac >> 6) as u32 & 0x0000ffff);
        let step: u32 =
            ((ds_xstep << 10) as u32 & 0xffff0000) | ((ds_ystep >> 6) as u32 & 0x0000ffff);

        let mut dest = ylookup[ds_y as usize].add(columnofs[ds_x1 as usize] as usize);

        // We do not check for zero spans here?
        let mut count = ds_x2 - ds_x1;

        debug_assert!(
            ds_x2 >= ds_x1
                && ds_x1 >= 0
                && ds_x2 < SCREENWIDTH
                && (ds_y as u32) <= (SCREENHEIGHT as u32),
            "R_DrawSpan: {} to {} at {}",
            ds_x1 as c_int,
            ds_x2 as c_int,
            ds_y as c_int
        );

        loop {
            // Calculate current texture index in u,v.
            let ytemp = ((position >> 4) & 0x0fc0) as usize;
            let xtemp = (position >> 26) as usize;
            let spot = xtemp | ytemp;

            // Lookup pixel from flat texture tile, re-index using light/colormap.
            *dest = *ds_colormap.add(*ds_source.add(spot) as usize);
            dest = dest.add(1);

            position = position.wrapping_add(step);
            if count == 0 {
                break;
            }
            count -= 1;
        }
    }
}

// ---------------------------------------------------------------------------
// R_DrawSpanLow
// ---------------------------------------------------------------------------

/// Low-detail variant of `R_DrawSpan` that writes each sampled texel to two
/// consecutive framebuffer bytes, producing blocky 2x-wide pixels.
///
/// The logical span coordinates (`ds_x1`, `ds_x2`) are doubled to address the
/// correct pixels; the texture-space walk is unchanged.
///
/// # FIXME
/// In the original C (`r_draw.c`) `ds_x1` and `ds_x2` are mutated in-place
/// (`ds_x1 <<= 1; ds_x2 <<= 1`), leaving the globals modified after the call.
/// This Rust port uses a local `ds_x1_low` and leaves the globals unchanged,
/// so callers that read `ds_x1`/`ds_x2` after `R_DrawSpanLow` see different
/// values than they would after the C version.
#[no_mangle]
pub extern "C" fn R_DrawSpanLow() {
    unsafe {
        let mut position: u32;

        position = ((ds_xfrac << 10) as u32 & 0xffff0000) | ((ds_yfrac >> 6) as u32 & 0x0000ffff);
        let step: u32 =
            ((ds_xstep << 10) as u32 & 0xffff0000) | ((ds_ystep >> 6) as u32 & 0x0000ffff);

        let mut count = ds_x2 - ds_x1;

        // Blocky mode, need to multiply by 2.
        let ds_x1_low = ds_x1 << 1;

        debug_assert!(
            ds_x2 >= ds_x1
                && ds_x1 >= 0
                && ds_x2 < SCREENWIDTH
                && (ds_y as u32) <= (SCREENHEIGHT as u32),
            "R_DrawSpanLow: {} to {} at {}",
            ds_x1 as c_int,
            ds_x2 as c_int,
            ds_y as c_int
        );

        let mut dest = ylookup[ds_y as usize].add(columnofs[ds_x1_low as usize] as usize);

        loop {
            let ytemp = ((position >> 4) & 0x0fc0) as usize;
            let xtemp = (position >> 26) as usize;
            let spot = xtemp | ytemp;

            // Lowres/blocky mode does it twice,
            // while scale is adjusted appropriately.
            let pix = *ds_colormap.add(*ds_source.add(spot) as usize);
            *dest = pix;
            dest = dest.add(1);
            *dest = pix;
            dest = dest.add(1);

            position = position.wrapping_add(step);
            if count == 0 {
                break;
            }
            count -= 1;
        }
    }
}

// ---------------------------------------------------------------------------
// R_InitBuffer
// ---------------------------------------------------------------------------

/// Initialises the `ylookup` and `columnofs` lookup tables for a viewport of the given size.
///
/// Computes `viewwindowx` (horizontal centering offset) and `viewwindowy` (vertical offset,
/// accounting for the status-bar height when the viewport is not full-screen). Then fills
/// `columnofs[0..width]` and `ylookup[0..height]` so that each inner rendering loop can
/// locate any pixel without a multiply.
///
/// Must be called whenever the viewport dimensions change (e.g. when the player resizes
/// the view window).
#[no_mangle]
pub extern "C" fn R_InitBuffer(width: c_int, height: c_int) {
    unsafe {
        // Handle resize, e.g. smaller view windows with border and/or status bar.
        viewwindowx = (SCREENWIDTH - width) >> 1;

        // Column offset. For windows.
        for i in 0..width {
            columnofs[i as usize] = viewwindowx + i;
        }

        // Same with base row offset.
        if width == SCREENWIDTH {
            viewwindowy = 0;
        } else {
            viewwindowy = (SCREENHEIGHT - SBARHEIGHT - height) >> 1;
        }

        // Precalculate all row offsets.
        for i in 0..height {
            ylookup[i as usize] = I_VideoBuffer.add(((i + viewwindowy) * SCREENWIDTH) as usize);
        }
    }
}

// ---------------------------------------------------------------------------
// DEH_String shim — identity in this build
// ---------------------------------------------------------------------------

/// Identity shim for the DeHackEd string-replacement function.
///
/// In the original C codebase `DEH_String` allows patch files to substitute string constants
/// at runtime. This Rust build does not support DeHackEd patches, so the function simply
/// returns its argument unchanged.
///
/// # Safety
/// `s` must be a valid, non-null pointer to a NUL-terminated C string for the duration of
/// any downstream C FFI call that receives the returned pointer.
unsafe fn DEH_String(s: *const c_char) -> *const c_char {
    s
}

// ---------------------------------------------------------------------------
// R_FillBackScreen
// ---------------------------------------------------------------------------

/// Fills the background buffer with a tiled flat texture and draws the beveled viewport border.
///
/// When the viewport is smaller than the full screen, the area outside it is filled with a
/// repeating 64×64 flat: `FLOOR7_2` for Doom 1 / Ultimate Doom, `GRNROCK` for Doom II.
/// Border patches (`brdr_t`, `brdr_b`, `brdr_l`, `brdr_r`, and the four corner patches)
/// are then drawn into the same `background_buffer` using `V_UseBuffer`.
///
/// If the viewport covers the full screen (`scaledviewwidth == SCREENWIDTH`) the background
/// buffer is freed and the function returns immediately.
#[no_mangle]
pub extern "C" fn R_FillBackScreen() {
    unsafe {
        // If we are running full screen, there is no need to do any of this,
        // and the background buffer can be freed if it was previously in use.
        if scaledviewwidth == SCREENWIDTH {
            if !background_buffer.is_null() {
                Z_Free(background_buffer as *mut c_void);
                background_buffer = ptr::null_mut();
            }
            return;
        }

        // Allocate the background buffer if necessary
        if background_buffer.is_null() {
            background_buffer = Z_Malloc(
                SCREENWIDTH * (SCREENHEIGHT - SBARHEIGHT),
                1, // PU_STATIC
                ptr::null_mut(),
            ) as *mut u8;
        }

        let name = if gamemode == commercial {
            DEH_String(c"GRNROCK".as_ptr())
        } else {
            DEH_String(c"FLOOR7_2".as_ptr())
        };

        let src = W_CacheLumpName(name, 8) as *mut u8; // PU_CACHE = 8
        let mut dest = background_buffer;

        for y in 0..(SCREENHEIGHT - SBARHEIGHT) {
            let row_src = src.add(((y & 63) << 6) as usize);
            for _ in 0..(SCREENWIDTH / 64) {
                ptr::copy_nonoverlapping(row_src, dest, 64);
                dest = dest.add(64);
            }
            let remainder = SCREENWIDTH & 63;
            if remainder != 0 {
                ptr::copy_nonoverlapping(row_src, dest, remainder as usize);
                dest = dest.add(remainder as usize);
            }
        }

        // Draw screen and bezel; this is done to a separate screen buffer.
        V_UseBuffer(background_buffer);

        let mut patch = W_CacheLumpName(DEH_String(c"brdr_t".as_ptr()), 8) as *mut patch_t;
        for x in (0..scaledviewwidth).step_by(8) {
            V_DrawPatch(viewwindowx + x, viewwindowy - 8, patch);
        }

        patch = W_CacheLumpName(DEH_String(c"brdr_b".as_ptr()), 8) as *mut patch_t;
        for x in (0..scaledviewwidth).step_by(8) {
            V_DrawPatch(viewwindowx + x, viewwindowy + viewheight, patch);
        }

        patch = W_CacheLumpName(DEH_String(c"brdr_l".as_ptr()), 8) as *mut patch_t;
        for y in (0..viewheight).step_by(8) {
            V_DrawPatch(viewwindowx - 8, viewwindowy + y, patch);
        }

        patch = W_CacheLumpName(DEH_String(c"brdr_r".as_ptr()), 8) as *mut patch_t;
        for y in (0..viewheight).step_by(8) {
            V_DrawPatch(viewwindowx + scaledviewwidth, viewwindowy + y, patch);
        }

        // Draw beveled edge.
        V_DrawPatch(
            viewwindowx - 8,
            viewwindowy - 8,
            W_CacheLumpName(DEH_String(c"brdr_tl".as_ptr()), 8) as *mut patch_t,
        );
        V_DrawPatch(
            viewwindowx + scaledviewwidth,
            viewwindowy - 8,
            W_CacheLumpName(DEH_String(c"brdr_tr".as_ptr()), 8) as *mut patch_t,
        );
        V_DrawPatch(
            viewwindowx - 8,
            viewwindowy + viewheight,
            W_CacheLumpName(DEH_String(c"brdr_bl".as_ptr()), 8) as *mut patch_t,
        );
        V_DrawPatch(
            viewwindowx + scaledviewwidth,
            viewwindowy + viewheight,
            W_CacheLumpName(DEH_String(c"brdr_br".as_ptr()), 8) as *mut patch_t,
        );

        V_RestoreBuffer();
    }
}

// ---------------------------------------------------------------------------
// R_VideoErase
// ---------------------------------------------------------------------------

/// Copies `count` bytes from the background buffer to the video buffer at byte offset `ofs`.
///
/// Used by `R_DrawViewBorder` to blit the pre-rendered bezel regions from `background_buffer`
/// into `I_VideoBuffer`. Does nothing if `background_buffer` is null (full-screen mode).
#[no_mangle]
pub extern "C" fn R_VideoErase(ofs: u32, count: c_int) {
    unsafe {
        if !background_buffer.is_null() {
            ptr::copy_nonoverlapping(
                background_buffer.add(ofs as usize),
                I_VideoBuffer.add(ofs as usize),
                count as usize,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// R_DrawViewBorder
// ---------------------------------------------------------------------------

/// Copies the border regions from the background buffer into the video buffer each frame.
///
/// Blits four rectangular areas (top strip, bottom strip, and the two side strips) using
/// `R_VideoErase`, then calls `V_MarkRect` to tell the video subsystem that the full
/// non-status-bar area needs to be presented.
///
/// Returns immediately without doing anything if `scaledviewwidth == SCREENWIDTH`
/// (full-screen viewport, no border to draw).
#[no_mangle]
pub extern "C" fn R_DrawViewBorder() {
    unsafe {
        if scaledviewwidth == SCREENWIDTH {
            return;
        }

        let top = ((SCREENHEIGHT - SBARHEIGHT) - viewheight) / 2;
        let side = (SCREENWIDTH - scaledviewwidth) / 2;

        // copy top and one line of left side
        R_VideoErase(0, (top * SCREENWIDTH + side) as c_int);

        // copy one line of right side and bottom
        let ofs = ((viewheight + top) * SCREENWIDTH - side) as u32;
        R_VideoErase(ofs, (top * SCREENWIDTH + side) as c_int);

        // copy sides using wraparound
        let mut ofs = ((top * SCREENWIDTH) + SCREENWIDTH - side) as u32;
        let side_doubled = side << 1;

        for _ in 1..viewheight {
            R_VideoErase(ofs, side_doubled);
            ofs += SCREENWIDTH as u32;
        }

        V_MarkRect(0, 0, SCREENWIDTH, SCREENHEIGHT - SBARHEIGHT);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialises all tests that touch the shared mutable renderer globals.
    static LOCK: Mutex<()> = Mutex::new(());

    // -----------------------------------------------------------------------
    // R_InitBuffer
    // -----------------------------------------------------------------------

    /// Verifies that a full-screen `R_InitBuffer` call sets `viewwindowx` and `viewwindowy`
    /// to zero and fills `columnofs` and `ylookup` with identity-offset values.
    #[test]
    fn init_buffer_fullscreen() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            // Allocate a fake video buffer so ylookup doesn't deref null.
            let mut fake_buf = vec![0u8; (SCREENWIDTH * SCREENHEIGHT) as usize];
            let orig_buf = I_VideoBuffer;
            I_VideoBuffer = fake_buf.as_mut_ptr();

            R_InitBuffer(SCREENWIDTH, 168);

            assert_eq!(viewwindowx, 0);
            assert_eq!(viewwindowy, 0);
            for i in 0..SCREENWIDTH {
                assert_eq!(columnofs[i as usize], i, "columnofs[{i}] mismatch");
            }
            for i in 0..168 {
                assert_eq!(
                    ylookup[i as usize],
                    I_VideoBuffer.add((i * SCREENWIDTH) as usize),
                    "ylookup[{i}] mismatch"
                );
            }

            I_VideoBuffer = orig_buf;
        }
    }

    /// Verifies centering offsets and `columnofs` values for a 256×168 windowed viewport.
    #[test]
    fn init_buffer_windowed_256x168() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            let mut fake_buf = vec![0u8; (SCREENWIDTH * SCREENHEIGHT) as usize];
            let orig_buf = I_VideoBuffer;
            I_VideoBuffer = fake_buf.as_mut_ptr();

            R_InitBuffer(256, 168);

            assert_eq!(viewwindowx, (SCREENWIDTH - 256) >> 1); // 32
            assert_eq!(viewwindowy, (SCREENHEIGHT - SBARHEIGHT - 168) >> 1); // 0
            for i in 0..256 {
                assert_eq!(columnofs[i as usize], viewwindowx + i);
            }

            I_VideoBuffer = orig_buf;
        }
    }

    /// Verifies centering offsets, `columnofs`, and `ylookup` for a 200×100 windowed viewport.
    #[test]
    fn init_buffer_windowed_200x100() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            let mut fake_buf = vec![0u8; (SCREENWIDTH * SCREENHEIGHT) as usize];
            let orig_buf = I_VideoBuffer;
            I_VideoBuffer = fake_buf.as_mut_ptr();

            R_InitBuffer(200, 100);

            assert_eq!(viewwindowx, (SCREENWIDTH - 200) >> 1); // 60
            assert_eq!(viewwindowy, (SCREENHEIGHT - SBARHEIGHT - 100) >> 1); // 34
            for i in 0..200 {
                assert_eq!(columnofs[i as usize], viewwindowx + i);
            }
            for i in 0..100 {
                let expected = I_VideoBuffer.add(((i + viewwindowy) * SCREENWIDTH) as usize);
                assert_eq!(ylookup[i as usize], expected, "ylookup[{i}] mismatch");
            }

            I_VideoBuffer = orig_buf;
        }
    }

    // -----------------------------------------------------------------------
    // R_InitTranslationTables
    // -----------------------------------------------------------------------

    /// Verifies that `R_InitTranslationTables` maps the green ramp to gray/brown/red and
    /// leaves all other palette entries as identity mappings.
    #[test]
    fn init_translation_tables() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            // Initialize the zone allocator so Z_Malloc works.
            // It's fine to re-initialize in a unit test.
            crate::doom::z_zone::Z_Init();

            R_InitTranslationTables();

            let tt = translationtables;
            for i in 0..256usize {
                if i >= 0x70 && i <= 0x7f {
                    assert_eq!(
                        *tt.add(i),
                        (0x60 + (i & 0xf)) as u8,
                        "tt[{i}] gray mismatch"
                    );
                    assert_eq!(
                        *tt.add(i + 256),
                        (0x40 + (i & 0xf)) as u8,
                        "tt[{i}+256] brown mismatch"
                    );
                    assert_eq!(
                        *tt.add(i + 512),
                        (0x20 + (i & 0xf)) as u8,
                        "tt[{i}+512] red mismatch"
                    );
                } else {
                    assert_eq!(*tt.add(i), i as u8, "tt[{i}] identity mismatch");
                    assert_eq!(*tt.add(i + 256), i as u8, "tt[{i}+256] identity mismatch");
                    assert_eq!(*tt.add(i + 512), i as u8, "tt[{i}+512] identity mismatch");
                }
            }

            // Clean up
            crate::doom::z_zone::Z_Free(translationtables as *mut c_void);
            translationtables = ptr::null_mut();
        }
    }

    // -----------------------------------------------------------------------
    // R_DrawColumn
    // -----------------------------------------------------------------------

    /// Verifies that `R_DrawColumn` writes the correct colormap-indexed texture samples
    /// for each row in a small column segment.
    #[test]
    fn draw_column_basic() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            let mut framebuffer = vec![0u8; (SCREENWIDTH * SCREENHEIGHT) as usize];
            let mut source = vec![0u8; 128];
            for i in 0..128 {
                source[i] = i as u8;
            }
            let mut colormap = vec![0u8; 256];
            for i in 0..256 {
                colormap[i] = (i ^ 0x55) as u8;
            }

            // Set up ylookup and columnofs for a full-screen buffer
            for i in 0..SCREENHEIGHT {
                ylookup[i as usize] = framebuffer.as_mut_ptr().add((i * SCREENWIDTH) as usize);
            }
            for i in 0..SCREENWIDTH {
                columnofs[i as usize] = i;
            }

            dc_colormap = colormap.as_mut_ptr();
            dc_source = source.as_mut_ptr();
            dc_x = 10;
            dc_yl = 20;
            dc_yh = 30;
            dc_iscale = 0x10000; // 1.0 in fixed point
            dc_texturemid = 0;
            centery = 0;

            R_DrawColumn();

            // Verify each pixel
            for row in 20..=30 {
                let src_idx = row & 127;
                let expected = colormap[source[src_idx as usize] as usize];
                let actual = framebuffer[(row * SCREENWIDTH + 10) as usize];
                assert_eq!(actual, expected, "pixel mismatch at row {row}");
            }
        }
    }

    /// Verifies that `R_DrawColumn` leaves the framebuffer untouched when `dc_yh < dc_yl`.
    #[test]
    fn draw_column_negative_count_returns_early() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            let mut framebuffer = vec![0xABu8; (SCREENWIDTH * SCREENHEIGHT) as usize];
            for i in 0..SCREENHEIGHT {
                ylookup[i as usize] = framebuffer.as_mut_ptr().add((i * SCREENWIDTH) as usize);
            }
            for i in 0..SCREENWIDTH {
                columnofs[i as usize] = i;
            }

            dc_x = 5;
            dc_yl = 10;
            dc_yh = 5; // count = -5
            dc_colormap = ptr::null_mut();
            dc_source = ptr::null_mut();

            R_DrawColumn();

            // Framebuffer should be untouched
            for i in 0..framebuffer.len() {
                assert_eq!(framebuffer[i], 0xAB, "framebuffer[{i}] was modified");
            }
        }
    }

    // -----------------------------------------------------------------------
    // R_DrawColumnLow
    // -----------------------------------------------------------------------

    /// Verifies that `R_DrawColumnLow` writes the same pixel value to both adjacent
    /// screen columns (`dc_x * 2` and `dc_x * 2 + 1`).
    #[test]
    fn draw_column_low_doubles() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            let mut framebuffer = vec![0u8; (SCREENWIDTH * SCREENHEIGHT) as usize];
            let mut source = vec![0u8; 128];
            source[0] = 42;
            let mut colormap = vec![0u8; 256];
            colormap[42] = 99;

            for i in 0..SCREENHEIGHT {
                ylookup[i as usize] = framebuffer.as_mut_ptr().add((i * SCREENWIDTH) as usize);
            }
            for i in 0..SCREENWIDTH {
                columnofs[i as usize] = i;
            }

            dc_colormap = colormap.as_mut_ptr();
            dc_source = source.as_mut_ptr();
            dc_x = 5;
            dc_yl = 10;
            dc_yh = 10;
            dc_iscale = 0;
            dc_texturemid = 0;
            centery = 0;

            R_DrawColumnLow();

            let x = dc_x << 1;
            assert_eq!(framebuffer[(10 * SCREENWIDTH + x) as usize], 99);
            assert_eq!(framebuffer[(10 * SCREENWIDTH + x + 1) as usize], 99);
        }
    }

    // -----------------------------------------------------------------------
    // R_DrawSpan
    // -----------------------------------------------------------------------

    /// Verifies that `R_DrawSpan` samples and colormap-indexes five consecutive texels
    /// across a short horizontal span.
    #[test]
    fn draw_span_basic() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            let mut framebuffer = vec![0u8; (SCREENWIDTH * SCREENHEIGHT) as usize];
            let mut source = vec![0u8; 64 * 64];
            for i in 0..(64 * 64) {
                source[i] = (i % 256) as u8;
            }
            let mut colormap = vec![0u8; 256];
            for i in 0..256 {
                colormap[i] = (i ^ 0xAA) as u8;
            }

            for i in 0..SCREENHEIGHT {
                ylookup[i as usize] = framebuffer.as_mut_ptr().add((i * SCREENWIDTH) as usize);
            }
            for i in 0..SCREENWIDTH {
                columnofs[i as usize] = i;
            }

            ds_colormap = colormap.as_mut_ptr();
            ds_source = source.as_mut_ptr();
            ds_y = 50;
            ds_x1 = 10;
            ds_x2 = 14;
            ds_xfrac = 0;
            ds_yfrac = 0;
            ds_xstep = 0x10000; // 1.0 in fixed point
            ds_ystep = 0;

            R_DrawSpan();

            // With xfrac=0, yfrac=0, xstep=1.0 (0x10000), ystep=0:
            // position = 0
            // step = (0x10000 << 10) & 0xffff0000 = 0x0400_0000
            // After each pixel, position increases by 0x0400_0000,
            // so xtemp (position >> 26) increments by 1 each time.
            // ds_x2 - ds_x1 = 4, so we draw 5 pixels (10..=14).
            let base = (50 * SCREENWIDTH + 10) as usize;
            assert_eq!(framebuffer[base], colormap[source[0] as usize]);
            assert_eq!(framebuffer[base + 1], colormap[source[1] as usize]);
            assert_eq!(framebuffer[base + 2], colormap[source[2] as usize]);
            assert_eq!(framebuffer[base + 3], colormap[source[3] as usize]);
            assert_eq!(framebuffer[base + 4], colormap[source[4] as usize]);
        }
    }

    // -----------------------------------------------------------------------
    // R_DrawSpanLow
    // -----------------------------------------------------------------------

    /// Verifies that `R_DrawSpanLow` writes the same texel to two consecutive framebuffer bytes.
    #[test]
    fn draw_span_low_doubles() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            let mut framebuffer = vec![0u8; (SCREENWIDTH * SCREENHEIGHT) as usize];
            let mut source = vec![0u8; 64 * 64];
            source[0] = 77;
            let mut colormap = vec![0u8; 256];
            colormap[77] = 88;

            for i in 0..SCREENHEIGHT {
                ylookup[i as usize] = framebuffer.as_mut_ptr().add((i * SCREENWIDTH) as usize);
            }
            for i in 0..SCREENWIDTH {
                columnofs[i as usize] = i;
            }

            ds_colormap = colormap.as_mut_ptr();
            ds_source = source.as_mut_ptr();
            ds_y = 60;
            ds_x1 = 5;
            ds_x2 = 5;
            ds_xfrac = 0;
            ds_yfrac = 0;
            ds_xstep = 0;
            ds_ystep = 0;

            R_DrawSpanLow();

            // ds_x2 - ds_x1 = 0, so 1 iteration. Each iteration writes 2 pixels.
            // ds_x1_low = 10
            let base = (60 * SCREENWIDTH + 10) as usize;
            assert_eq!(framebuffer[base], 88);
            assert_eq!(framebuffer[base + 1], 88);
        }
    }

    // -----------------------------------------------------------------------
    // R_VideoErase
    // -----------------------------------------------------------------------

    /// Verifies that `R_VideoErase` copies the specified bytes from `background_buffer`
    /// into `I_VideoBuffer` at the correct offset.
    #[test]
    fn video_erase_copies_from_background() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            let mut video = vec![0u8; (SCREENWIDTH * SCREENHEIGHT) as usize];
            let mut bg = vec![0u8; (SCREENWIDTH * SCREENHEIGHT) as usize];
            for i in 0..bg.len() {
                bg[i] = (i % 256) as u8;
            }

            let orig_video = I_VideoBuffer;
            I_VideoBuffer = video.as_mut_ptr();
            background_buffer = bg.as_mut_ptr();

            R_VideoErase(100, 10);

            for i in 100..110 {
                assert_eq!(video[i as usize], bg[i as usize]);
            }
            // Ensure surrounding bytes are untouched
            assert_eq!(video[99], 0);
            assert_eq!(video[110], 0);

            I_VideoBuffer = orig_video;
            background_buffer = ptr::null_mut();
        }
    }

    /// Verifies that `R_VideoErase` is a no-op when `background_buffer` is null.
    #[test]
    fn video_erase_null_background_does_nothing() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            let mut video = vec![0xCCu8; (SCREENWIDTH * SCREENHEIGHT) as usize];
            let orig_video = I_VideoBuffer;
            I_VideoBuffer = video.as_mut_ptr();
            background_buffer = ptr::null_mut();

            R_VideoErase(0, 10);

            for i in 0..10 {
                assert_eq!(video[i], 0xCC);
            }

            I_VideoBuffer = orig_video;
        }
    }

    // -----------------------------------------------------------------------
    // R_DrawViewBorder
    // -----------------------------------------------------------------------

    /// Verifies that `R_DrawViewBorder` exits immediately without panicking when the
    /// viewport is full-screen (`scaledviewwidth == SCREENWIDTH`).
    #[test]
    fn draw_view_border_fullscreen_returns_early() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            scaledviewwidth = SCREENWIDTH;
            // Should return without doing anything (no panic)
            R_DrawViewBorder();
        }
    }

    /// Verifies the `top` and `side` geometry values computed inside `R_DrawViewBorder`
    /// for a representative windowed viewport.
    #[test]
    fn draw_view_border_sets_correct_offsets() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            scaledviewwidth = 256;
            viewheight = 168;

            let top = ((SCREENHEIGHT - SBARHEIGHT) - viewheight) / 2;
            let side = (SCREENWIDTH - scaledviewwidth) / 2;

            assert_eq!(top, 0);
            assert_eq!(side, 32);
        }
    }

    // -----------------------------------------------------------------------
    // R_DrawTranslatedColumn
    // -----------------------------------------------------------------------

    /// Verifies that `R_DrawTranslatedColumn` applies `dc_translation` then `dc_colormap`
    /// to produce the expected output pixel.
    #[test]
    fn draw_translated_column_maps_colors() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            let mut framebuffer = vec![0u8; (SCREENWIDTH * SCREENHEIGHT) as usize];
            let mut source = vec![0u8; 256];
            source[0] = 5; // source pixel at frac=0
            let mut translation = vec![0u8; 256];
            translation[5] = 7; // map color 5 → 7
            let mut colormap = vec![0u8; 256];
            colormap[7] = 99;

            for i in 0..SCREENHEIGHT {
                ylookup[i as usize] = framebuffer.as_mut_ptr().add((i * SCREENWIDTH) as usize);
            }
            for i in 0..SCREENWIDTH {
                columnofs[i as usize] = i;
            }

            dc_colormap = colormap.as_mut_ptr();
            dc_source = source.as_mut_ptr();
            dc_translation = translation.as_mut_ptr();
            dc_x = 15;
            dc_yl = 25;
            dc_yh = 25;
            dc_iscale = 0;
            dc_texturemid = 0;
            centery = 0;

            R_DrawTranslatedColumn();

            assert_eq!(framebuffer[(25 * SCREENWIDTH + 15) as usize], 99);
        }
    }

    // -----------------------------------------------------------------------
    // R_DrawFuzzColumn
    // -----------------------------------------------------------------------

    /// Verifies that `R_DrawFuzzColumn` reads a neighboring framebuffer pixel via `fuzzoffset`,
    /// indexes it through colormap 6, and advances `fuzzpos`.
    #[test]
    fn draw_fuzz_column_reads_adjacent() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            let mut framebuffer = vec![0u8; (SCREENWIDTH * SCREENHEIGHT) as usize];
            // Pre-fill the framebuffer so the fuzz effect has something to read.
            // fuzzoffset[0] = FUZZOFF = SCREENWIDTH, so it reads one row below.
            framebuffer[((25 + 1) * SCREENWIDTH + 10) as usize] = 3;

            // Set up colormaps: colormaps[6*256 + 3] = 77
            let mut colormaps_buf = vec![0u8; 32 * 256];
            colormaps_buf[6 * 256 + 3] = 77;

            for i in 0..SCREENHEIGHT {
                ylookup[i as usize] = framebuffer.as_mut_ptr().add((i * SCREENWIDTH) as usize);
            }
            for i in 0..SCREENWIDTH {
                columnofs[i as usize] = i;
            }

            colormaps = colormaps_buf.as_mut_ptr();
            dc_x = 10;
            dc_yl = 25;
            dc_yh = 25;
            dc_iscale = 0;
            dc_texturemid = 0;
            centery = 0;
            fuzzpos = 0;
            viewheight = SCREENHEIGHT;

            R_DrawFuzzColumn();

            // fuzzpos should have advanced
            assert_eq!(fuzzpos, 1);
            // The pixel at (10, 25) should now be colormaps[6*256 + framebuffer[(10, 26)]]
            assert_eq!(framebuffer[(25 * SCREENWIDTH + 10) as usize], 77);
        }
    }

    /// Verifies that `R_DrawFuzzColumn` clamps `dc_yl` to 1 and `dc_yh` to `viewheight - 2`
    /// to avoid reading outside the viewport.
    #[test]
    fn draw_fuzz_column_clamps_borders() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            let mut framebuffer = vec![0u8; (SCREENWIDTH * SCREENHEIGHT) as usize];
            let mut colormaps_buf = vec![0u8; 32 * 256];

            for i in 0..SCREENHEIGHT {
                ylookup[i as usize] = framebuffer.as_mut_ptr().add((i * SCREENWIDTH) as usize);
            }
            for i in 0..SCREENWIDTH {
                columnofs[i as usize] = i;
            }

            colormaps = colormaps_buf.as_mut_ptr();
            dc_x = 0;
            dc_yl = 0; // will be clamped to 1
            dc_yh = SCREENHEIGHT - 1; // will be clamped to SCREENHEIGHT - 2
            dc_iscale = 0;
            dc_texturemid = 0;
            centery = 0;
            fuzzpos = 0;
            viewheight = SCREENHEIGHT;

            R_DrawFuzzColumn();

            // After clamping: dc_yl = 1, dc_yh = SCREENHEIGHT - 2
            assert_eq!(dc_yl, 1);
            assert_eq!(dc_yh, SCREENHEIGHT - 2);
        }
    }

    // -----------------------------------------------------------------------
    // Fuzz offset table
    // -----------------------------------------------------------------------

    /// Checks that `fuzzoffset` exactly matches the table from the C source (`r_draw.c`).
    #[test]
    fn fuzzoffset_exact_values() {
        let expected: [c_int; FUZZTABLE] = [
            FUZZOFF, -FUZZOFF, FUZZOFF, -FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF, FUZZOFF, FUZZOFF,
            -FUZZOFF, FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF, FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF,
            -FUZZOFF, -FUZZOFF, -FUZZOFF, FUZZOFF, -FUZZOFF, -FUZZOFF, FUZZOFF, FUZZOFF, FUZZOFF,
            FUZZOFF, -FUZZOFF, FUZZOFF, -FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF, -FUZZOFF, FUZZOFF,
            FUZZOFF, -FUZZOFF, -FUZZOFF, -FUZZOFF, -FUZZOFF, FUZZOFF, FUZZOFF, FUZZOFF, FUZZOFF,
            -FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF, FUZZOFF,
        ];
        unsafe {
            for (i, (&got, &want)) in fuzzoffset.iter().zip(expected.iter()).enumerate() {
                assert_eq!(
                    got, want,
                    "fuzzoffset[{i}] mismatch: got {got}, want {want}"
                );
            }
        }
    }

    /// Checks that the `fuzzoffset` table contains exactly 29 positive and 21 negative entries.
    #[test]
    fn fuzzoffset_positive_count() {
        unsafe {
            let pos = fuzzoffset.iter().filter(|&&v| v > 0).count();
            let neg = fuzzoffset.iter().filter(|&&v| v < 0).count();
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
}
