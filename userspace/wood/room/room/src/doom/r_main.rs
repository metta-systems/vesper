//! Renderer main loop, view setup, and BSP/geometry/trigonometry utilities.
//!
//! Rust port of `vendor/doomgeneric/r_main.c`. Hosts all renderer global state
//! (viewport dimensions, view position/angle, light tables, column/span function
//! pointers) and the per-frame entry point [`R_RenderPlayerView`].

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::{c_int, c_short, c_uint};
use std::ptr;

use crate::doom::d_player::PlayerT;
use crate::doom::i_video::{SCREENHEIGHT as SCREENHEIGHT_IV, SCREENWIDTH as SCREENWIDTH_IV};
use crate::doom::m_bbox::BBox;
use crate::doom::m_fixed::{angle_t, fixed_t, FixedDiv, FixedMul};
use crate::doom::m_fixed::{FRACBITS, FRACUNIT};
use crate::doom::p_telept::mobj_t;
use crate::doom::r_bsp::{node_t, seg_t, subsector_t};
use crate::doom::tables::{self, SlopeDiv};
use crate::doom::tables::{ANG180, ANG270, ANG90, ANGLETOFINESHIFT};
use crate::types::Boolean;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Number of fine-angle steps spanning the horizontal field of view (90 degrees
/// expressed in fine-angle units; `FINEANGLES / 4 = 2048`).
const FIELDOFVIEW: c_int = 2048;

/// Screen pixel width, mirrored from `i_video` as a `usize` for array sizing.
const SCREENWIDTH: usize = SCREENWIDTH_IV as usize;

/// Screen pixel height, mirrored from `i_video` as a `usize` for array sizing.
const SCREENHEIGHT: usize = SCREENHEIGHT_IV as usize;

/// Number of distinct light levels used in the `scalelight` / `zlight` tables.
const LIGHTLEVELS: usize = 16;

/// Shift used to convert a wall scale value into a light-level index (unused in
/// this module but exported for sibling renderer modules).
#[allow(dead_code)]
const LIGHTSEGSHIFT: u32 = 4;

/// Maximum number of scale steps in the `scalelight` table (one entry per
/// screen-pixel column-scale bucket).
const MAXLIGHTSCALE: usize = 48;

/// Shift applied to a column scale value before indexing `scalelight`.
const LIGHTSCALESHIFT: u32 = 12;

/// Maximum number of Z-distance steps in the `zlight` table.
const MAXLIGHTZ: usize = 128;

/// Shift applied to a Z-distance value before indexing `zlight`.
const LIGHTZSHIFT: u32 = 20;

/// Number of colormap entries (palette remapping tables); each is 256 bytes.
const NUMCOLORMAPS: usize = 32;

/// Divisor applied to the raw scale/distance value when mapping to a light
/// level, controlling how quickly lighting falls off with distance.
const DISTMAP: usize = 2;

/// `FRACBITS - SLOPEBITS = 16 - 11 = 5`.
///
/// Used to right-shift a fixed-point fraction before indexing `tantoangle[]`
/// in [`R_PointToDist`]. Matches `DBITS` in `vendor/doomgeneric/tables.h`.
// DBITS = FRACBITS - SLOPEBITS = 16 - 11 = 5 (matches vendor/doomgeneric/tables.h).
const DBITS: u32 = 5;

/// Raw colormap byte type; a palette index remap table entry.
type lighttable_t = u8;

// ---------------------------------------------------------------------------
// Globals defined by this module
// ---------------------------------------------------------------------------

/// Additive angle offset applied to the player's map angle before rendering,
/// used by the automap and demo-playback angle overrides. Zero during normal
/// gameplay.
#[no_mangle]
pub static mut viewangleoffset: c_int = 0;

/// Monotonically incrementing counter; bumped once per frame in
/// [`R_SetupFrame`]. Sectors and linedefs tag themselves with `validcount`
/// when first visited in a frame so they are not processed twice.
#[no_mangle]
pub static mut validcount: c_int = 1;

/// Active fixed colormap pointer, or null when no override is in effect.
///
/// Set to a non-null colormap when the player has an invulnerability or
/// light-amp powerup active (`player.fixedcolormap != 0`). When non-null,
/// all walls, floors, and sprites use this single colormap instead of the
/// distance-based `scalelight`/`zlight` entries.
#[no_mangle]
pub static mut fixedcolormap: *mut lighttable_t = ptr::null_mut();

/// X coordinate of the viewport centre in screen pixels.
///
/// Recomputed by [`R_ExecuteSetViewSize`] whenever the view size changes.
#[no_mangle]
pub static mut centerx: c_int = 0;

/// Y coordinate of the viewport centre in screen pixels.
///
/// Recomputed by [`R_ExecuteSetViewSize`] whenever the view size changes.
#[no_mangle]
pub static mut centery: c_int = 0;

/// [`centerx`] expressed as a 16.16 fixed-point value (`centerx << FRACBITS`).
///
/// Used by [`R_InitTextureMapping`] and projection arithmetic in
/// [`R_ExecuteSetViewSize`].
#[no_mangle]
pub static mut centerxfrac: fixed_t = 0;

/// [`centery`] expressed as a 16.16 fixed-point value (`centery << FRACBITS`).
///
/// Precomputed by [`R_ExecuteSetViewSize`]; used by wall and sprite renderers
/// in `r_segs` and `r_things` to project top/bottom screen coordinates.
#[no_mangle]
pub static mut centeryfrac: fixed_t = 0;

/// Fixed-point focal length (horizontal projection constant).
///
/// Equal to `centerxfrac`; used by [`R_ScaleFromGlobalAngle`] and the
/// texture mapping setup in [`R_InitTextureMapping`]. Represents the
/// distance from the eye to the projection plane in fixed-point units.
#[no_mangle]
pub static mut projection: fixed_t = 0;

/// Number of frames rendered since [`R_Init`] was called.
///
/// Incremented once per frame in [`R_SetupFrame`]. Used for profiling.
#[no_mangle]
pub static mut framecount: c_int = 0;

/// Number of subsectors drawn in the current frame.
///
/// Reset to zero in [`R_SetupFrame`]; incremented in `r_bsp`. Used for
/// profiling.
#[no_mangle]
pub static mut sscount: c_int = 0;

/// Number of linedefs processed in the current frame (profiling counter).
#[no_mangle]
pub static mut linecount: c_int = 0;

/// Number of BSP traversal iterations in the current frame (profiling
/// counter).
#[no_mangle]
pub static mut loopcount: c_int = 0;

/// View position X in map fixed-point units. Set each frame by
/// [`R_SetupFrame`] from the player mobj's `x` field.
#[no_mangle]
pub static mut viewx: fixed_t = 0;

/// View position Y in map fixed-point units. Set each frame by
/// [`R_SetupFrame`] from the player mobj's `y` field.
#[no_mangle]
pub static mut viewy: fixed_t = 0;

/// View height Z in map fixed-point units (player eye height). Set each
/// frame by [`R_SetupFrame`] from `player.viewz`.
#[no_mangle]
pub static mut viewz: fixed_t = 0;

/// Current view angle as a Binary Angle Measurement (BAM) `u32`.
///
/// Set each frame in [`R_SetupFrame`] from the player mobj angle plus
/// [`viewangleoffset`]. Used throughout the BSP traversal and texture
/// mapping pipeline.
#[no_mangle]
pub static mut viewangle: angle_t = 0;

/// `cos(viewangle)` in 16.16 fixed-point. Precomputed each frame in
/// [`R_SetupFrame`] for fast world-space projection.
#[no_mangle]
pub static mut viewcos: fixed_t = 0;

/// `sin(viewangle)` in 16.16 fixed-point. Precomputed each frame in
/// [`R_SetupFrame`] for fast world-space projection.
#[no_mangle]
pub static mut viewsin: fixed_t = 0;

/// Pointer to the player struct whose view is currently being rendered.
///
/// Set at the start of each frame by [`R_SetupFrame`]; read by several
/// renderer subsystems that need player-specific state (e.g. weapon sprites).
#[no_mangle]
pub static mut viewplayer: *mut PlayerT = ptr::null_mut();

/// Detail level shift: `0` = high detail, `1` = low detail (half-width
/// columns doubled horizontally). Controls which column/span draw functions
/// are active and affects several scaling calculations.
#[no_mangle]
pub static mut detailshift: c_int = 0;

/// Half the horizontal field of view as a BAM angle.
///
/// Set by [`R_InitTextureMapping`] to `xtoviewangle[0]` - the largest view
/// angle that still maps to screen column 0. Used by the BSP clipper in
/// `r_bsp` to cull out-of-frustum segs.
#[no_mangle]
pub static mut clipangle: angle_t = 0;

/// Maps fine-angle index to screen X column.
///
/// `viewangletox[i]` is the screen column (or sentinel `-1` /
/// `viewwidth+1` for out-of-frustum angles) for fine-angle `i`. Indexed
/// by `(viewangle >> ANGLETOFINESHIFT)`. Sized one element larger than
/// strictly necessary (`FINEANGLES/2 + 1`) to match `r_bsp`'s defensive
/// bounds.
// r_bsp.rs originally declared this as [c_int; FINEANGLES/2 + 1] to guard
// against a potential off-by-one in the original C code. Keep the same
// size so the two modules agree.
#[no_mangle]
pub static mut viewangletox: [c_int; tables::FINEANGLES / 2 + 1] = [0; tables::FINEANGLES / 2 + 1];

/// Maps screen X column to the smallest view angle that projects onto that
/// column.
///
/// `xtoviewangle[x]` gives the left-edge angle of the frustum slice at
/// column `x`. Sized `SCREENWIDTH + 1` to include the right-edge sentinel.
#[no_mangle]
pub static mut xtoviewangle: [angle_t; SCREENWIDTH + 1] = [0; SCREENWIDTH + 1];

/// Distance-to-light lookup table indexed by `[light_level][scale]`.
///
/// `scalelight[i][j]` points into the master `colormaps` array. `i` is
/// derived from the sector light level, `j` from the projected wall/sprite
/// scale. Recomputed by [`R_ExecuteSetViewSize`] because it depends on
/// `viewwidth`.
#[no_mangle]
pub static mut scalelight: [[*mut lighttable_t; MAXLIGHTSCALE]; LIGHTLEVELS] =
    [[ptr::null_mut(); MAXLIGHTSCALE]; LIGHTLEVELS];

/// Fixed-scale colormap array used when [`fixedcolormap`] is active.
///
/// All `MAXLIGHTSCALE` entries are set to [`fixedcolormap`] in
/// [`R_SetupFrame`], so that wall-light lookup code does not need a special
/// case for fixed-colormap mode.
#[no_mangle]
pub static mut scalelightfixed: [*mut lighttable_t; MAXLIGHTSCALE] =
    [ptr::null_mut(); MAXLIGHTSCALE];

/// Z-distance-to-light lookup table indexed by `[light_level][z_bucket]`.
///
/// `zlight[i][j]` points into `colormaps`. Used for flat (floor/ceiling)
/// and sprite lighting. Computed once in [`R_InitLightTables`] (unlike
/// `scalelight`, it does not depend on `viewwidth`).
#[no_mangle]
pub static mut zlight: [[*mut lighttable_t; MAXLIGHTZ]; LIGHTLEVELS] =
    [[ptr::null_mut(); MAXLIGHTZ]; LIGHTLEVELS];

/// Additional light bonus added to the sector's base light level, produced
/// by muzzle flashes and similar effects. Set each frame from
/// `player.extralight` in [`R_SetupFrame`].
#[no_mangle]
pub static mut extralight: c_int = 0;

/// Active column-drawing function pointer.
///
/// Points to either the normal or low-detail column renderer; may be
/// temporarily overridden by `r_things` to a fuzz or translated variant.
/// Reset to `basecolfunc` after each sprite.
#[no_mangle]
pub static mut colfunc: Option<unsafe extern "C" fn()> = None;

/// Base (unmodified) column-drawing function pointer.
///
/// Always points to the standard solid-column renderer for the current
/// detail level (`R_DrawColumn` or `R_DrawColumnLow`). Used to restore
/// `colfunc` after drawing special-effect sprites.
#[no_mangle]
pub static mut basecolfunc: Option<unsafe extern "C" fn()> = None;

/// Fuzz (partial-invisibility) column-drawing function pointer.
///
/// Points to `R_DrawFuzzColumn` or `R_DrawFuzzColumnLow` depending on the
/// active detail level.
#[no_mangle]
pub static mut fuzzcolfunc: Option<unsafe extern "C" fn()> = None;

/// Translated (palette-remapped) column-drawing function pointer.
///
/// Points to `R_DrawTranslatedColumn` or `R_DrawTranslatedColumnLow`.
/// Used for colored player sprites in multiplayer.
#[no_mangle]
pub static mut transcolfunc: Option<unsafe extern "C" fn()> = None;

/// Horizontal span (floor/ceiling) drawing function pointer.
///
/// Points to `R_DrawSpan` or `R_DrawSpanLow` depending on the active
/// detail level.
#[no_mangle]
pub static mut spanfunc: Option<unsafe extern "C" fn()> = None;

/// Flag set by [`R_SetViewSize`] when a view-size change is pending.
///
/// [`R_ExecuteSetViewSize`] checks this at the start of each frame and
/// applies the pending change if set. The deferred approach avoids changing
/// viewport dimensions mid-frame.
#[no_mangle]
pub static mut setsizeneeded: Boolean = Boolean::FALSE;

/// Pending viewport block size (1-11). Set by [`R_SetViewSize`] and
/// consumed by [`R_ExecuteSetViewSize`]. Value 11 selects full-screen
/// rendering.
#[no_mangle]
pub static mut setblocks: c_int = 0;

/// Pending detail level (0 = high, 1 = low). Set by [`R_SetViewSize`] and
/// consumed by [`R_ExecuteSetViewSize`].
#[no_mangle]
pub static mut setdetail: c_int = 0;

// ---------------------------------------------------------------------------
// Imports from other modules
// ---------------------------------------------------------------------------

use crate::doom::d_loop::NetUpdate;
use crate::doom::m_menu::{detailLevel, screenblocks};
use crate::doom::p_setup::{nodes, numnodes, subsectors};
use crate::doom::r_bsp::{R_ClearClipSegs, R_ClearDrawSegs, R_RenderBSPNode};
use crate::doom::r_data::{colormaps, R_InitData};
use crate::doom::r_draw::{
    scaledviewwidth, viewheight, viewwidth, R_DrawColumn, R_DrawColumnLow, R_DrawFuzzColumn,
    R_DrawFuzzColumnLow, R_DrawSpan, R_DrawSpanLow, R_DrawTranslatedColumn,
    R_DrawTranslatedColumnLow, R_InitBuffer, R_InitTranslationTables,
};
use crate::doom::r_plane::{R_ClearPlanes, R_DrawPlanes, R_InitPlanes};
use crate::doom::r_segs::{rw_distance, rw_normalangle, walllights};
use crate::doom::r_things::{
    pspriteiscale, pspritescale, screenheightarray, R_ClearSprites, R_DrawMasked,
};

// ---------------------------------------------------------------------------
// R_AddPointToBox
// ---------------------------------------------------------------------------

/// Expand a bounding box so that it encloses the given map-coordinate point.
///
/// Equivalent to `R_AddPointToBox` in `r_main.c`. Modifies the four
/// fixed-point values at `box_` in the [`BBox`] layout (`LEFT`, `RIGHT`,
/// `BOTTOM`, `TOP`).
///
/// # Safety
/// `box_` must point to a valid, writable array of at least four `fixed_t`
/// values laid out in [`BBox`] index order.
#[no_mangle]
pub unsafe extern "C" fn R_AddPointToBox(x: c_int, y: c_int, box_: *mut fixed_t) {
    if x < *box_.add(BBox::LEFT) {
        *box_.add(BBox::LEFT) = x;
    }
    if x > *box_.add(BBox::RIGHT) {
        *box_.add(BBox::RIGHT) = x;
    }
    if y < *box_.add(BBox::BOTTOM) {
        *box_.add(BBox::BOTTOM) = y;
    }
    if y > *box_.add(BBox::TOP) {
        *box_.add(BBox::TOP) = y;
    }
}

// ---------------------------------------------------------------------------
// R_PointOnSide
// ---------------------------------------------------------------------------

/// Determine which side of a BSP partition plane a map point lies on.
///
/// Returns `0` for the front (right) side and `1` for the back (left) side.
/// Uses fast sign-bit shortcuts for axis-aligned partitions before falling
/// back to a full cross-product test.
///
/// Equivalent to `R_PointOnSide` in `r_main.c`.
///
/// # Safety
/// `node` must be a valid, non-null pointer to a [`node_t`].
#[no_mangle]
pub unsafe extern "C" fn R_PointOnSide(x: fixed_t, y: fixed_t, node: *const node_t) -> c_int {
    if (*node).dx == 0 {
        if x <= (*node).x {
            return ((*node).dy > 0) as c_int;
        }
        return ((*node).dy < 0) as c_int;
    }
    if (*node).dy == 0 {
        if y <= (*node).y {
            return ((*node).dx < 0) as c_int;
        }
        return ((*node).dx > 0) as c_int;
    }

    let dx = x - (*node).x;
    let dy = y - (*node).y;

    // Try to quickly decide by looking at sign bits.
    if (((*node).dy ^ (*node).dx ^ dx ^ dy) as u32) & 0x80000000 != 0 {
        if (((*node).dy ^ dx) as u32) & 0x80000000 != 0 {
            return 1;
        }
        return 0;
    }

    let left = FixedMul((*node).dy >> FRACBITS, dx);
    let right = FixedMul(dy, (*node).dx >> FRACBITS);

    if right < left {
        0
    } else {
        1
    }
}

// ---------------------------------------------------------------------------
// R_PointOnSegSide
// ---------------------------------------------------------------------------

/// Determine which side of a seg (map line segment) a point lies on.
///
/// Returns `0` for the front side and `1` for the back side, using the same
/// sign-bit shortcut as [`R_PointOnSide`].
///
/// Equivalent to `R_PointOnSegSide` in `r_main.c`.
///
/// # Safety
/// `line` must be a valid, non-null pointer to a [`seg_t`] whose `v1` and
/// `v2` vertex pointers are also valid.
#[no_mangle]
pub unsafe extern "C" fn R_PointOnSegSide(x: fixed_t, y: fixed_t, line: *const seg_t) -> c_int {
    let lx = (*(*line).v1).x;
    let ly = (*(*line).v1).y;

    let ldx = (*(*line).v2).x - lx;
    let ldy = (*(*line).v2).y - ly;

    if ldx == 0 {
        if x <= lx {
            return (ldy > 0) as c_int;
        }
        return (ldy < 0) as c_int;
    }
    if ldy == 0 {
        if y <= ly {
            return (ldx < 0) as c_int;
        }
        return (ldx > 0) as c_int;
    }

    let dx = x - lx;
    let dy = y - ly;

    // Try to quickly decide by looking at sign bits.
    if ((ldy ^ ldx ^ dx ^ dy) as u32) & 0x80000000 != 0 {
        if ((ldy ^ dx) as u32) & 0x80000000 != 0 {
            return 1;
        }
        return 0;
    }

    let left = FixedMul(ldy >> FRACBITS, dx);
    let right = FixedMul(dy, ldx >> FRACBITS);

    if right < left {
        0
    } else {
        1
    }
}

// ---------------------------------------------------------------------------
// R_PointToAngle
// ---------------------------------------------------------------------------

/// Classify a relative vector `(dx, dy)` into one of eight octants and look
/// up the corresponding BAM angle from the `tantoangle` table.
///
/// This is the pure octant-classification core shared by [`R_PointToAngle`]
/// and [`R_PointToAngle2`]; it never reads or writes any global state.
/// Returns 0 for the zero vector.
fn point_to_angle_from_delta(mut dx: fixed_t, mut dy: fixed_t) -> angle_t {
    if dx == 0 && dy == 0 {
        return 0;
    }

    if dx >= 0 {
        // dx >= 0
        if dy >= 0 {
            // dy >= 0
            if dx > dy {
                // octant 0
                tables::tantoangle[SlopeDiv(dy as c_uint, dx as c_uint) as usize]
            } else {
                // octant 1
                ANG90 - 1 - tables::tantoangle[SlopeDiv(dx as c_uint, dy as c_uint) as usize]
            }
        } else {
            // dy < 0
            dy = -dy;
            if dx > dy {
                // octant 8
                0u32.wrapping_sub(tables::tantoangle[SlopeDiv(dy as c_uint, dx as c_uint) as usize])
            } else {
                // octant 7
                ANG270 + tables::tantoangle[SlopeDiv(dx as c_uint, dy as c_uint) as usize]
            }
        }
    } else {
        // dx < 0
        dx = -dx;
        if dy >= 0 {
            // dy >= 0
            if dx > dy {
                // octant 3
                ANG180 - 1 - tables::tantoangle[SlopeDiv(dy as c_uint, dx as c_uint) as usize]
            } else {
                // octant 2
                ANG90 + tables::tantoangle[SlopeDiv(dx as c_uint, dy as c_uint) as usize]
            }
        } else {
            // dy < 0
            dy = -dy;
            if dx > dy {
                // octant 4
                ANG180 + tables::tantoangle[SlopeDiv(dy as c_uint, dx as c_uint) as usize]
            } else {
                // octant 5
                ANG270 - 1 - tables::tantoangle[SlopeDiv(dx as c_uint, dy as c_uint) as usize]
            }
        }
    }
}

/// Convert an absolute map coordinate to a view angle (BAM `u32`).
///
/// Subtracts the current [`viewx`]/[`viewy`] to get a relative vector, then
/// classifies the vector into one of eight octants and looks up the angle
/// using the `tantoangle` table. Returns 0 for the view position itself.
///
/// Equivalent to `R_PointToAngle` in `r_main.c`.
///
/// # Safety
/// Reads the global [`viewx`] and [`viewy`]; these must have been
/// initialised by [`R_SetupFrame`] before this function is called.
#[no_mangle]
pub unsafe extern "C" fn R_PointToAngle(x: fixed_t, y: fixed_t) -> angle_t {
    point_to_angle_from_delta(x - viewx, y - viewy)
}

// ---------------------------------------------------------------------------
// R_PointToAngle2
// ---------------------------------------------------------------------------

/// Compute the BAM angle from map point `(x1, y1)` to map point `(x2, y2)`.
///
/// Computes the delta `(x2 - x1, y2 - y1)` and classifies it directly via the
/// shared octant lookup. Unlike the C original (and earlier Rust port), this
/// implementation does not touch the [`viewx`]/[`viewy`] globals, so it is
/// reentrant and safe to call between a read and a use of the view position.
///
/// Equivalent to `R_PointToAngle2` in `r_main.c`, with the global-aliasing
/// trick removed.
///
/// # Safety
/// Pure computation; takes no globals. Marked `unsafe extern "C"` only to
/// keep the C ABI for callers linked against the legacy engine.
#[no_mangle]
pub unsafe extern "C" fn R_PointToAngle2(
    x1: fixed_t,
    y1: fixed_t,
    x2: fixed_t,
    y2: fixed_t,
) -> angle_t {
    point_to_angle_from_delta(x2 - x1, y2 - y1)
}

// ---------------------------------------------------------------------------
// R_PointToDist
// ---------------------------------------------------------------------------

/// Compute the distance from the current view position to a map point.
///
/// Uses the `tantoangle` and `finesine` tables to compute the Euclidean
/// distance via a cosine projection. Handles `dx == 0` to avoid division by
/// zero (matches the udm1.wad crash fix present in the C source).
///
/// Equivalent to `R_PointToDist` in `r_main.c`.
///
/// # Safety
/// Reads [`viewx`] and [`viewy`], which must have been set by
/// [`R_SetupFrame`] before this is called in a rendering context.
#[no_mangle]
pub unsafe extern "C" fn R_PointToDist(x: fixed_t, y: fixed_t) -> fixed_t {
    let mut dx = (x - viewx).wrapping_abs();
    let mut dy = (y - viewy).wrapping_abs();

    if dy > dx {
        std::mem::swap(&mut dx, &mut dy);
    }

    let frac = if dx != 0 { FixedDiv(dy, dx) } else { 0 };

    let angle = (tables::tantoangle[(frac as u32 >> DBITS) as usize] + ANG90) >> ANGLETOFINESHIFT;

    FixedDiv(dx, tables::finesine[angle as usize])
}

// ---------------------------------------------------------------------------
// R_InitPointToAngle
// ---------------------------------------------------------------------------

/// No-op initialiser kept for ABI compatibility.
///
/// In the original Doom source, `R_InitPointToAngle` built the `tantoangle`
/// lookup table at runtime. The table is now precomputed in `tables.c` (and
/// `tables.rs`), so this function has no work to do.
///
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub extern "C" fn R_InitPointToAngle() {
    // UNUSED - now getting from tables.c
}

// ---------------------------------------------------------------------------
// R_ScaleFromGlobalAngle
// ---------------------------------------------------------------------------

/// Compute the texture-mapping scale for a wall column at the given view
/// angle.
///
/// Returns the fixed-point scale factor used to stretch or shrink a wall
/// texture column. Clamps the result to `[256, 64 * FRACUNIT]` to avoid
/// extreme near/far values.
///
/// `rw_distance` (distance from view to the wall normal) and
/// `rw_normalangle` (angle of the wall's outward normal) must be set before
/// calling this function; they are both globals in `r_segs`.
///
/// Equivalent to `R_ScaleFromGlobalAngle` in `r_main.c`.
///
/// # Safety
/// Reads [`viewangle`], [`projection`], [`detailshift`], and the `r_segs`
/// globals [`rw_distance`] and [`rw_normalangle`]. All must be initialised
/// before calling.
#[no_mangle]
pub unsafe extern "C" fn R_ScaleFromGlobalAngle(visangle: angle_t) -> fixed_t {
    let anglea = ANG90.wrapping_add(visangle.wrapping_sub(viewangle));
    let angleb = ANG90.wrapping_add(visangle.wrapping_sub(rw_normalangle));

    let sinea = tables::finesine[(anglea >> ANGLETOFINESHIFT) as usize];
    let sineb = tables::finesine[(angleb >> ANGLETOFINESHIFT) as usize];
    let num = FixedMul(projection, sineb) << detailshift;
    let den = FixedMul(rw_distance, sinea);

    if den > num >> 16 {
        let mut scale = FixedDiv(num, den);
        scale = scale.clamp(256, 64 * FRACUNIT);
        scale
    } else {
        64 * FRACUNIT
    }
}

// ---------------------------------------------------------------------------
// R_InitTables
// ---------------------------------------------------------------------------

/// No-op initialiser kept for ABI compatibility.
///
/// In the original source, `R_InitTables` computed `finetangent` and
/// `finesine` at runtime. Both tables are now precomputed in `tables.c`
/// (and `tables.rs`).
///
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub extern "C" fn R_InitTables() {
    // UNUSED: now getting from tables.c
}

// ---------------------------------------------------------------------------
// R_InitTextureMapping
// ---------------------------------------------------------------------------

/// Build the [`viewangletox`] and [`xtoviewangle`] lookup tables for the
/// current viewport geometry, then set [`clipangle`].
///
/// The focal length is derived from [`centerxfrac`] and the `finetangent`
/// table so that `FIELDOFVIEW` fine-angle steps span exactly [`viewwidth`]
/// pixels. After building both tables the function removes the sentinel
/// `-1`/`viewwidth+1` values from [`viewangletox`] (fencepost cleanup).
///
/// Called by [`R_ExecuteSetViewSize`] whenever the viewport is resized.
///
/// # Safety
/// Reads and writes numerous renderer globals ([`viewwidth`],
/// [`centerxfrac`], [`clipangle`], etc.). Must be called after [`viewwidth`]
/// and [`centerxfrac`] have been set by [`R_ExecuteSetViewSize`].
#[no_mangle]
pub unsafe extern "C" fn R_InitTextureMapping() {
    // Use tangent table to generate viewangletox:
    //  viewangletox will give the next greatest x after the view angle.
    //
    // Calc focallength so FIELDOFVIEW angles covers SCREENWIDTH.
    let focallength = FixedDiv(
        centerxfrac,
        tables::finetangent[tables::FINEANGLES / 4 + FIELDOFVIEW as usize / 2],
    );

    for i in 0..tables::FINEANGLES / 2 {
        let t: c_int;
        if tables::finetangent[i] > FRACUNIT * 2 {
            t = -1;
        } else if tables::finetangent[i] < -FRACUNIT * 2 {
            t = viewwidth + 1;
        } else {
            let mut tt = FixedMul(tables::finetangent[i], focallength);
            tt = (centerxfrac - tt + FRACUNIT - 1) >> FRACBITS;
            if tt < -1 {
                t = -1;
            } else if tt > viewwidth + 1 {
                t = viewwidth + 1;
            } else {
                t = tt;
            }
        }
        viewangletox[i] = t;
    }

    // Scan viewangletox[] to generate xtoviewangle[]:
    //  xtoviewangle will give the smallest view angle that maps to x.
    for x in 0..=viewwidth as usize {
        let mut i = 0usize;
        while viewangletox[i] > x as c_int {
            i += 1;
        }
        xtoviewangle[x] = ((i as u32) << ANGLETOFINESHIFT).wrapping_sub(ANG90);
    }

    // Take out the fencepost cases from viewangletox.
    for i in 0..tables::FINEANGLES / 2 {
        let mut t = FixedMul(tables::finetangent[i], focallength);
        t = centerx - t;

        if viewangletox[i] == -1 {
            viewangletox[i] = 0;
        } else if viewangletox[i] == viewwidth + 1 {
            viewangletox[i] = viewwidth;
        }
    }

    clipangle = xtoviewangle[0];
}

// ---------------------------------------------------------------------------
// R_InitLightTables
// ---------------------------------------------------------------------------

/// Precompute the Z-distance-based light table [`zlight`].
///
/// For each combination of sector light level and Z-distance bucket, computes
/// a pointer into the master `colormaps` array. The Z-distance buckets are
/// shifted by `LIGHTZSHIFT` before lookup. Only the distance-based
/// [`zlight`] table is built here; the scale-based [`scalelight`] table
/// depends on `viewwidth` and is rebuilt in [`R_ExecuteSetViewSize`].
///
/// Equivalent to `R_InitLightTables` in `r_main.c`.
///
/// # Safety
/// Reads `colormaps` from `r_data`; that pointer must be non-null and point
/// to `NUMCOLORMAPS * 256` valid bytes. Called during startup by [`R_Init`].
#[no_mangle]
pub unsafe extern "C" fn R_InitLightTables() {
    for i in 0..LIGHTLEVELS {
        let startmap = (((LIGHTLEVELS - 1 - i) * 2) * NUMCOLORMAPS / LIGHTLEVELS) as c_int;
        for j in 0..MAXLIGHTZ {
            let mut scale = FixedDiv(
                (SCREENWIDTH as c_int / 2) * FRACUNIT,
                ((j + 1) << LIGHTZSHIFT) as c_int,
            );
            scale >>= LIGHTSCALESHIFT;
            let mut level = startmap - scale / DISTMAP as c_int;

            if level < 0 {
                level = 0;
            }
            if level >= NUMCOLORMAPS as c_int {
                level = NUMCOLORMAPS as c_int - 1;
            }

            zlight[i][j] = colormaps.add(level as usize * 256);
        }
    }
}

// ---------------------------------------------------------------------------
// R_SetViewSize
// ---------------------------------------------------------------------------

/// Schedule a viewport size change for the next frame.
///
/// Sets [`setsizeneeded`], [`setblocks`], and [`setdetail`] so that
/// [`R_ExecuteSetViewSize`] will apply them at the start of the next rendered
/// frame. Safe to call mid-frame because the actual resize is deferred.
///
/// Equivalent to `R_SetViewSize` in `r_main.c`.
///
/// # Safety
/// Writes three renderer globals; safe to call from any context as long as
/// no other thread reads those globals concurrently (single-threaded engine).
#[no_mangle]
pub unsafe extern "C" fn R_SetViewSize(blocks: c_int, detail: c_int) {
    setsizeneeded = Boolean::TRUE;
    setblocks = blocks;
    setdetail = detail;
}

// ---------------------------------------------------------------------------
// R_ExecuteSetViewSize
// ---------------------------------------------------------------------------

/// Apply a pending viewport size change.
///
/// Computes all viewport dimension globals ([`viewwidth`], [`viewheight`],
/// [`scaledviewwidth`], [`centerx`], [`centery`], [`centerxfrac`],
/// [`centeryfrac`], [`projection`], [`detailshift`]), selects the appropriate
/// column/span draw function pointers, rebuilds the texture-mapping tables,
/// and recomputes both the `scalelight` and `yslope`/`distscale` plane
/// tables.
///
/// Only called when [`setsizeneeded`] is `TRUE`; normally invoked once per
/// frame from the game loop before rendering begins.
///
/// Equivalent to `R_ExecuteSetViewSize` in `r_main.c`.
///
/// # Safety
/// Writes a large number of renderer globals and calls several initialisation
/// helpers. Must not be called while a frame render is in progress.
#[no_mangle]
pub unsafe extern "C" fn R_ExecuteSetViewSize() {
    setsizeneeded = Boolean::FALSE;

    if setblocks == 11 {
        scaledviewwidth = SCREENWIDTH as c_int;
        viewheight = SCREENHEIGHT as c_int;
    } else {
        scaledviewwidth = setblocks * 32;
        viewheight = (setblocks * 168 / 10) & !7;
    }

    detailshift = setdetail;
    viewwidth = scaledviewwidth >> detailshift;

    centery = viewheight / 2;
    centerx = viewwidth / 2;
    centerxfrac = centerx << FRACBITS;
    centeryfrac = centery << FRACBITS;
    projection = centerxfrac;

    if detailshift == 0 {
        colfunc = Some(R_DrawColumn);
        basecolfunc = Some(R_DrawColumn);
        fuzzcolfunc = Some(R_DrawFuzzColumn);
        transcolfunc = Some(R_DrawTranslatedColumn);
        spanfunc = Some(R_DrawSpan);
    } else {
        colfunc = Some(R_DrawColumnLow);
        basecolfunc = Some(R_DrawColumnLow);
        fuzzcolfunc = Some(R_DrawFuzzColumnLow);
        transcolfunc = Some(R_DrawTranslatedColumnLow);
        spanfunc = Some(R_DrawSpanLow);
    }

    R_InitBuffer(scaledviewwidth, viewheight);
    R_InitTextureMapping();

    // psprite scales
    pspritescale = FRACUNIT * viewwidth / SCREENWIDTH as c_int;
    pspriteiscale = FRACUNIT * SCREENWIDTH as c_int / viewwidth;

    // thing clipping
    for i in 0..viewwidth as usize {
        screenheightarray[i] = viewheight as c_short;
    }

    // planes
    for i in 0..viewheight as usize {
        let mut dy = ((i as c_int - viewheight / 2) << FRACBITS) + FRACUNIT / 2;
        dy = dy.wrapping_abs();
        crate::doom::r_plane::yslope[i] = FixedDiv((viewwidth << detailshift) / 2 * FRACUNIT, dy);
    }

    for i in 0..viewwidth as usize {
        let cosadj = (*tables::finecosine
            .0
            .add((xtoviewangle[i] >> ANGLETOFINESHIFT) as usize))
        .wrapping_abs();
        crate::doom::r_plane::distscale[i] = FixedDiv(FRACUNIT, cosadj);
    }

    // Calculate the light levels to use for each level / scale combination.
    for i in 0..LIGHTLEVELS {
        let startmap = (((LIGHTLEVELS - 1 - i) * 2) * NUMCOLORMAPS / LIGHTLEVELS) as c_int;
        for j in 0..MAXLIGHTSCALE {
            let mut level = startmap
                - (j as c_int * SCREENWIDTH as c_int)
                    / (viewwidth << detailshift)
                    / DISTMAP as c_int;

            if level < 0 {
                level = 0;
            }
            if level >= NUMCOLORMAPS as c_int {
                level = NUMCOLORMAPS as c_int - 1;
            }

            scalelight[i][j] = colormaps.add(level as usize * 256);
        }
    }
}

// ---------------------------------------------------------------------------
// R_Init
// ---------------------------------------------------------------------------

/// One-time renderer initialisation called at engine startup.
///
/// Calls, in order: `R_InitData`, `R_InitPointToAngle`, `R_InitTables`,
/// `R_SetViewSize`, `R_InitPlanes`, `R_InitLightTables`, `R_InitSkyMap`,
/// `R_InitTranslationTables`. Prints a `.` to stdout after each step as a
/// startup progress indicator.
///
/// Equivalent to `R_Init` in `r_main.c`.
///
/// # Safety
/// Initialises global renderer state; must be called exactly once before any
/// frame is rendered. Calls multiple unsafe initialisers internally.
#[no_mangle]
pub unsafe extern "C" fn R_Init() {
    R_InitData();
    libc::printf(c".".as_ptr());
    R_InitPointToAngle();
    libc::printf(c".".as_ptr());
    R_InitTables();
    libc::printf(c".".as_ptr());
    R_SetViewSize(screenblocks, detailLevel);
    R_InitPlanes();
    libc::printf(c".".as_ptr());
    R_InitLightTables();
    libc::printf(c".".as_ptr());
    crate::doom::r_sky::R_InitSkyMap();
    R_InitTranslationTables();
    libc::printf(c".".as_ptr());

    framecount = 0;
}

// ---------------------------------------------------------------------------
// R_PointInSubsector
// ---------------------------------------------------------------------------

/// Node flag indicating the child index refers to a subsector, not another
/// node. Matches `NF_SUBSECTOR` in `r_local.h`.
const NF_SUBSECTOR: u32 = 0x8000;

/// Walk the BSP tree to find the subsector that contains the given map point.
///
/// Starts at the root node (`numnodes - 1`) and descends by calling
/// [`R_PointOnSide`] at each node until reaching a leaf (subsector) indicated
/// by the `NF_SUBSECTOR` flag. Handles the degenerate case of a single
/// subsector (no nodes).
///
/// Equivalent to `R_PointInSubsector` in `r_main.c`.
///
/// # Safety
/// Reads the `nodes` and `subsectors` arrays from `p_setup`; both must be
/// fully populated (i.e. the map must have been loaded) before this function
/// is called.
#[no_mangle]
pub unsafe extern "C" fn R_PointInSubsector(x: fixed_t, y: fixed_t) -> *mut subsector_t {
    // single subsector is a special case
    if numnodes == 0 {
        return subsectors as *mut subsector_t;
    }

    let mut nodenum = numnodes - 1;

    while (nodenum as u32) & NF_SUBSECTOR == 0 {
        let node = nodes.add(nodenum as usize) as *const node_t;
        let side = R_PointOnSide(x, y, node);
        nodenum = (*node).children[side as usize] as c_int;
    }

    subsectors.add((nodenum as u32 & !NF_SUBSECTOR) as usize) as *mut subsector_t
}

// ---------------------------------------------------------------------------
// R_SetupFrame
// ---------------------------------------------------------------------------

/// Prepare all per-frame view globals from the given player's current state.
///
/// Sets [`viewplayer`], [`viewx`], [`viewy`], [`viewz`], [`viewangle`],
/// [`viewsin`], [`viewcos`], [`extralight`], and [`fixedcolormap`]. Also
/// resets [`sscount`] and increments both [`framecount`] and [`validcount`].
///
/// When the player has a fixed colormap powerup, fills [`scalelightfixed`]
/// with the fixed colormap pointer and redirects [`walllights`] to it so
/// that wall-lighting lookup code requires no special-case handling.
///
/// Equivalent to `R_SetupFrame` in `r_main.c`.
///
/// # Safety
/// `player` must be a valid, non-null pointer to a [`PlayerT`] whose `mo`
/// mobj pointer is also valid. Reads and writes numerous renderer globals.
#[no_mangle]
pub unsafe extern "C" fn R_SetupFrame(player: *mut PlayerT) {
    viewplayer = player;
    let mo = (*player).mo as *mut mobj_t;
    viewx = (*mo).x;
    viewy = (*mo).y;
    viewangle = (*mo).angle.wrapping_add(viewangleoffset as u32);
    extralight = (*player).extralight;

    viewz = (*player).viewz;

    viewsin = tables::finesine[(viewangle >> ANGLETOFINESHIFT) as usize];
    viewcos = *tables::finecosine
        .0
        .add((viewangle >> ANGLETOFINESHIFT) as usize);

    sscount = 0;

    if (*player).fixedcolormap != 0 {
        fixedcolormap = colormaps
            .add((*player).fixedcolormap as usize * 256 * std::mem::size_of::<lighttable_t>());

        for i in 0..MAXLIGHTSCALE {
            scalelightfixed[i] = fixedcolormap;
        }
        walllights = std::ptr::addr_of_mut!(scalelightfixed[0]);
    } else {
        fixedcolormap = ptr::null_mut();
    }

    framecount += 1;
    validcount += 1;
}

// ---------------------------------------------------------------------------
// R_RenderPlayerView
// ---------------------------------------------------------------------------

/// Top-level per-frame render entry point.
///
/// Calls [`R_SetupFrame`] to prepare view globals, clears all renderer
/// buffers (`R_ClearClipSegs`, `R_ClearDrawSegs`, `R_ClearPlanes`,
/// `R_ClearSprites`), traverses the BSP tree via `R_RenderBSPNode`, then
/// draws floors/ceilings (`R_DrawPlanes`) and masked objects
/// (`R_DrawMasked`). `NetUpdate` is called between phases to keep network
/// and demo state responsive on slow machines.
///
/// Equivalent to `R_RenderPlayerView` (`R_RenderView`) in `r_main.c`.
///
/// # Safety
/// `player` must be a valid, non-null pointer to a fully-initialised
/// [`PlayerT`]. All renderer globals and map data must have been loaded and
/// initialised prior to this call.
#[no_mangle]
pub unsafe extern "C" fn R_RenderPlayerView(player: *mut PlayerT) {
    R_SetupFrame(player);

    R_ClearClipSegs();
    R_ClearDrawSegs();
    R_ClearPlanes();
    R_ClearSprites();

    NetUpdate();
    R_RenderBSPNode(numnodes - 1);
    NetUpdate();
    R_DrawPlanes();
    NetUpdate();
    R_DrawMasked();
    NetUpdate();
}

// ---------------------------------------------------------------------------
// Anchor
// ---------------------------------------------------------------------------

/// Force-references all exported symbols so the linker does not strip them.
///
/// Each renderer function is referenced as a raw function pointer, preventing
/// dead-code elimination when the crate is compiled as a library linked into
/// the C engine.
///
/// Exported as `#[no_mangle]` for C callers.
///
/// # Safety
/// This function only takes addresses of functions; no actual calls are made.
/// Safe to call at any time.
#[no_mangle]
pub unsafe extern "C" fn R_Main_Link_Anchor() {
    let _ = R_AddPointToBox as *const () as usize;
    let _ = R_PointOnSide as *const () as usize;
    let _ = R_PointOnSegSide as *const () as usize;
    let _ = R_PointToAngle as *const () as usize;
    let _ = R_PointToAngle2 as *const () as usize;
    let _ = R_PointToDist as *const () as usize;
    let _ = R_InitPointToAngle as *const () as usize;
    let _ = R_ScaleFromGlobalAngle as *const () as usize;
    let _ = R_InitTables as *const () as usize;
    let _ = R_InitTextureMapping as *const () as usize;
    let _ = R_InitLightTables as *const () as usize;
    let _ = R_SetViewSize as *const () as usize;
    let _ = R_ExecuteSetViewSize as *const () as usize;
    let _ = R_Init as *const () as usize;
    let _ = R_PointInSubsector as *const () as usize;
    let _ = R_SetupFrame as *const () as usize;
    let _ = R_RenderPlayerView as *const () as usize;
}

// ---------------------------------------------------------------------------
// Regression tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doom::m_fixed::FixedDiv;
    use std::sync::Mutex;

    /// Process-wide guard serialising tests that read or write the renderer
    /// globals (`viewx`, `viewy`, `viewangle`, ...). `cargo test` runs tests
    /// in parallel by default; without this lock, two tests touching these
    /// statics could race and tear each other's state.
    static RENDER_GLOBALS_LOCK: Mutex<()> = Mutex::new(());

    /// RAII guard that snapshots `viewx`/`viewy` on construction and
    /// restores them on drop, so an assertion panic cannot leave the
    /// globals in a perturbed state for any subsequent test that
    /// happens to acquire the lock.
    struct ViewPosGuard {
        saved_x: fixed_t,
        saved_y: fixed_t,
    }

    impl ViewPosGuard {
        fn new() -> Self {
            unsafe {
                Self {
                    saved_x: viewx,
                    saved_y: viewy,
                }
            }
        }
    }

    impl Drop for ViewPosGuard {
        fn drop(&mut self) {
            unsafe {
                viewx = self.saved_x;
                viewy = self.saved_y;
            }
        }
    }

    /// The DBITS constant was once incorrectly set to 15 instead of
    /// FRACBITS - SLOPEBITS = 16 - 11 = 5.  With the wrong value,
    /// R_PointToDist indexes tantoangle with the wrong shift and
    /// computes completely wrong distances, causing wall-offset /
    /// sprite-clipping glitches.
    #[test]
    fn dbits_is_five() {
        assert_eq!(DBITS, 5, "DBITS must be FRACBITS - SLOPEBITS = 5");
    }

    /// For a 45-degree line (dy == dx), FixedDiv(dy, dx) returns FRACUNIT.
    /// With DBITS = 5 the index into tantoangle is FRACUNIT >> 5 == 2048,
    /// the top of the table.  With DBITS = 15 the index would be 2,
    /// which produces a garbage distance.
    #[test]
    fn r_point_to_dist_45_degree_index() {
        let frac = FixedDiv(FRACUNIT, FRACUNIT); // dy == dx
        let index = (frac as u32 >> DBITS) as usize;
        assert_eq!(
            index, 2048,
            "frac>>DBITS for 45-degree case must index tantoangle[2048]"
        );
    }

    #[test]
    fn r_point_to_dist_reasonable_values() {
        let _g = RENDER_GLOBALS_LOCK.lock().unwrap();
        let _vp = ViewPosGuard::new();
        unsafe {
            viewx = 0;
            viewy = 0;

            // Straight ahead: distance should be approximately FRACUNIT.
            let dist_ahead = R_PointToDist(FRACUNIT, 0);
            assert!(dist_ahead > 0, "distance straight ahead must be positive");
            assert!(
                (dist_ahead - FRACUNIT).abs() < FRACUNIT / 4,
                "distance straight ahead should be near FRACUNIT, got {}",
                dist_ahead
            );

            // 45-degree diagonal: distance should be larger than straight ahead.
            let dist_diag = R_PointToDist(FRACUNIT, FRACUNIT);
            assert!(
                dist_diag > dist_ahead,
                "diagonal distance ({}) must exceed straight-ahead distance ({})",
                dist_diag,
                dist_ahead
            );

            // Same point: distance must be exactly zero.
            let dist_zero = R_PointToDist(0, 0);
            assert_eq!(dist_zero, 0, "distance to view position must be zero");
        }
    }

    #[test]
    fn r_point_to_angle2_cardinals() {
        unsafe {
            // Due east is exact.
            assert_eq!(R_PointToAngle2(0, 0, FRACUNIT, 0), 0);

            // Due north quantizes to ANG90 - 1 in the fixed-point LUT.
            assert_eq!(R_PointToAngle2(0, 0, 0, FRACUNIT), ANG90 - 1);

            // Due west quantizes to ANG180 - 1.
            assert_eq!(R_PointToAngle2(0, 0, -FRACUNIT, 0), ANG180 - 1);

            // Due south is exact.
            assert_eq!(R_PointToAngle2(0, 0, 0, -FRACUNIT), ANG270);
        }
    }

    /// R_PointToAngle2 used to set `viewx = x1; viewy = y1` and delegate to
    /// R_PointToAngle, clobbering the view-position globals. The refactored
    /// implementation must compute the angle purely from the delta and leave
    /// `viewx` / `viewy` untouched.
    #[test]
    fn r_point_to_angle2_preserves_view_globals() {
        let _g = RENDER_GLOBALS_LOCK.lock().unwrap();
        let _vp = ViewPosGuard::new();
        unsafe {
            viewx = 12345;
            viewy = -6789;

            let _ = R_PointToAngle2(1000, 2000, 3000, 4000);

            assert_eq!(viewx, 12345, "R_PointToAngle2 must not modify viewx");
            assert_eq!(viewy, -6789, "R_PointToAngle2 must not modify viewy");
        }
    }

    /// R_PointToAngle2 must be translation-invariant: the angle from
    /// `(x1, y1)` to `(x2, y2)` equals the angle of the delta vector from
    /// the origin. Confirms the rewrite preserves the original semantics
    /// for nonzero source points.
    #[test]
    fn r_point_to_angle2_translation_invariant() {
        // R_PointToAngle2 itself does not read viewx/viewy, but acquire
        // the lock and restore guard anyway so this test never observes
        // a torn state if a future change introduces a global read.
        let _g = RENDER_GLOBALS_LOCK.lock().unwrap();
        let _vp = ViewPosGuard::new();

        let cases: [(fixed_t, fixed_t); 8] = [
            (FRACUNIT, 0),
            (FRACUNIT, FRACUNIT),
            (0, FRACUNIT),
            (-FRACUNIT, FRACUNIT),
            (-FRACUNIT, 0),
            (-FRACUNIT, -FRACUNIT),
            (0, -FRACUNIT),
            (FRACUNIT, -FRACUNIT),
        ];

        for (dx, dy) in cases.iter().copied() {
            let from_origin = unsafe { R_PointToAngle2(0, 0, dx, dy) };
            let translated = unsafe {
                R_PointToAngle2(
                    100 * FRACUNIT,
                    -50 * FRACUNIT,
                    100 * FRACUNIT + dx,
                    -50 * FRACUNIT + dy,
                )
            };
            assert_eq!(
                from_origin, translated,
                "R_PointToAngle2 must be translation-invariant (dx={}, dy={})",
                dx, dy
            );
        }
    }

    /// R_ScaleFromGlobalAngle previously used plain `+` and `-` on
    /// angle_t values, which panics in debug builds on wraparound.
    /// It must use wrapping_add / wrapping_sub.
    #[test]
    fn r_scale_from_global_angle_wraparound_no_panic() {
        unsafe {
            viewangle = 0;
            rw_normalangle = 0;
            projection = FRACUNIT;
            rw_distance = FRACUNIT;
            detailshift = 0;

            // Angles near the u32 boundary must not panic.
            let _ = R_ScaleFromGlobalAngle(u32::MAX);
            let _ = R_ScaleFromGlobalAngle(0);
            let _ = R_ScaleFromGlobalAngle(ANG90);
            let _ = R_ScaleFromGlobalAngle(ANG180);
            let _ = R_ScaleFromGlobalAngle(ANG270);
            let _ = R_ScaleFromGlobalAngle(viewangle.wrapping_sub(1));
            let _ = R_ScaleFromGlobalAngle(viewangle.wrapping_add(1));
        }
    }

    #[test]
    fn r_point_on_side_vertical_line() {
        unsafe {
            // Node with dx == 0, dy > 0 (vertical line, northward).
            let node = node_t {
                x: 0,
                y: 0,
                dx: 0,
                dy: FRACUNIT,
                bbox: [[0; 4]; 2],
                children: [0; 2],
            };
            // Point to the left of the line -> side 1 (from C logic).
            assert_eq!(R_PointOnSide(-FRACUNIT, 0, &node), 1);
            // Point to the right of the line -> side 0.
            assert_eq!(R_PointOnSide(FRACUNIT, 0, &node), 0);
            // Point exactly on the line -> x <= node.x, side depends on dy > 0 -> 1.
            assert_eq!(R_PointOnSide(0, 0, &node), 1);
        }
    }
}
