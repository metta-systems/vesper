//! Visplane (floor and ceiling) renderer.
//!
//! Rust port of `vendor/doomgeneric/r_plane.c`.
//!
//! # Overview
//!
//! Doom renders floors and ceilings as horizontal pixel spans.  During BSP
//! traversal each wall segment calls [`R_CheckPlane`] / [`R_FindPlane`] to
//! record the per-column screen-y extents into a [`visplane_t`].  After the
//! full BSP walk, [`R_DrawPlanes`] iterates over every accumulated visplane
//! and rasterizes it into horizontal spans via [`R_MakeSpans`] /
//! [`R_MapPlane`].  Sky columns are treated as a special case: they are drawn
//! with `colfunc` rather than `spanfunc`.
//!
//! The C globals live in [`PlaneState`], the renderer's `planes`; visplanes
//! are addressed by index into its pool.  The `openings` scratch buffer is
//! the renderer's `openings`, which the drawsegs keep raw pointers into.
//!
//! # Coordinate system
//!
//! All heights use the fixed-point `fixed_t` type (`i32`, 16.16 format,
//! `FRACUNIT = 1 << 16`).  Angles are `angle_t = u32` where a full circle is
//! `0x1_0000_0000` (wrapping arithmetic).

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use core::ptr;

use crate::{
    engine::math::{
        fixed::{FixedDiv, FixedMul, fixed_t},
        tables::{self, ANG90, ANGLETOFINESHIFT, FINEMASK},
    },
    i_error,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Screen width in pixels, re-exported from [`i_video`] for local use.
const SCREENWIDTH: usize = crate::engine::interface::video::SCREENWIDTH as usize;

/// Screen height in pixels, re-exported from [`i_video`] for local use.
const SCREENHEIGHT: usize = crate::engine::interface::video::SCREENHEIGHT as usize;

/// Maximum number of simultaneous visplanes per frame.
///
/// Doom aborts with `I_Error` when this limit is exceeded.  The Rust port
/// aborts via [`i_error!`] when exceeded (see [`R_FindPlane`]).
const MAXVISPLANES: usize = 128;

/// Maximum number of `i16` slots in the renderer's `openings` array.
///
/// Used as scratch space for sprite clipping arrays stored by
/// `R_StoreWallRange` (in `r_segs`).  The C source comments this as `"?"`.
pub const MAXOPENINGS: usize = SCREENWIDTH * 64;

/// Number of distinct light levels used by the colormap tables.
const LIGHTLEVELS: usize = 16;

/// Shift applied to a sector's `lightlevel` to derive a colormap-table index.
const LIGHTSEGSHIFT: u32 = 4;

/// Number of distance-based light entries per light level in `zlight`.
const MAXLIGHTZ: usize = 128;

/// Shift applied to a world distance to derive an index into `zlight[n]`.
const LIGHTZSHIFT: u32 = 20;

/// Shift applied to a view angle to derive a sky-texture column index.
///
/// Produces a coarser (less precise) mapping than `ANGLETOFINESHIFT` so that
/// the sky texture wraps once around the full horizontal field of view.
const ANGLETOSKYSHIFT: u32 = 22;

/// Safe accessor for finecosine table (it's a pointer into finesine).
///
/// # Safety
///
/// The index is masked to `FINEMASK` before dereferencing, so the access is
/// always within the bounds of the static sine table exported by
/// [`tables`].
#[inline]
fn finecosine(idx: usize) -> i32 {
    unsafe { *tables::finecosine.0.add(idx & FINEMASK as usize) }
}

// ---------------------------------------------------------------------------
// Type aliases
// ---------------------------------------------------------------------------

/// Colormap entry type: a single palette index byte.
type lighttable_t = u8;

/// Function pointer type for horizontal-span and sky-column draw callbacks.
///
/// The two parameters are the left (`x1`) and right (`x2`) screen columns of
/// the span, matching the C `planefunction_t` signature
/// `void (*)(int top, int bottom)`.
pub type planefunction_t = unsafe fn(i32, i32);

// ---------------------------------------------------------------------------
// visplane_t — mirrors the C struct including pad bytes
// ---------------------------------------------------------------------------

/// One floor or ceiling plane accumulator.
///
/// A visplane records which columns (screen x) are covered by a particular
/// flat/height/lightlevel combination.  For each covered column, `top` and
/// `bottom` store the inclusive screen-y range that the span rasterizer must
/// fill.  A value of `0xFF` in `top[x]` means column `x` is not yet used.
///
/// The `pad1`/`pad2`/`pad3`/`pad4` bytes are intentional: the C renderer uses
/// `pl->top[pl->minx-1]` and `pl->top[pl->maxx+1]` as sentinel writes, which
/// land in the padding when the plane spans the full screen width.
/// `R_DrawPlanes` reads and writes these bytes by name instead of indexing
/// out of bounds, so the field order does not matter.
#[derive(Clone, Copy)]
pub struct visplane_t {
    /// World height of the flat, in fixed-point units.
    pub height: fixed_t,
    /// Flat texture number (lump index relative to `firstflat`).
    pub picnum: i32,
    /// Sector light level (0-255) for this plane.
    pub lightlevel: i32,
    /// Leftmost screen column used by this plane (inclusive).
    pub minx: i32,
    /// Rightmost screen column used by this plane (inclusive).
    pub maxx: i32,
    /// Padding byte before `top[]`; acts as a sentinel slot for
    /// `top[minx-1]` when `minx == 0`.
    pub pad1: u8,
    /// Per-column top clip (inclusive screen-y).  `0xFF` means unused.
    pub top: [u8; SCREENWIDTH],
    /// Padding byte after `top[]`; sentinel for `top[maxx+1]` when
    /// `maxx == SCREENWIDTH - 1`.
    pub pad2: u8,
    /// Padding byte before `bottom[]`; mirrors the pad3 slot in C.
    pub pad3: u8,
    /// Per-column bottom clip (inclusive screen-y).
    pub bottom: [u8; SCREENWIDTH],
    /// Padding byte after `bottom[]`.
    pub pad4: u8,
}

/// Compile-time zero initializer for the visplane pool entries.
const ZERO_VISPLANE: visplane_t = visplane_t {
    height: 0,
    picnum: 0,
    lightlevel: 0,
    minx: 0,
    maxx: 0,
    pad1: 0,
    top: [0; SCREENWIDTH],
    pad2: 0,
    pad3: 0,
    bottom: [0; SCREENWIDTH],
    pad4: 0,
};

/// Default initialization for visplane pool entries.
impl Default for visplane_t {
    /// Returns a zeroed `visplane_t`, used to initialize the visplane pool.
    fn default() -> Self {
        Self {
            height: 0,
            picnum: 0,
            lightlevel: 0,
            minx: 0,
            maxx: 0,
            pad1: 0,
            top: [0xFF; SCREENWIDTH],
            pad2: 0,
            pad3: 0,
            bottom: [0; SCREENWIDTH],
            pad4: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Plane state
// ---------------------------------------------------------------------------

/// Plane renderer state (C `r_plane.c` globals other than `openings`), the
/// renderer's `planes`.
pub struct PlaneState {
    /// Fixed-size pool of visplane accumulators for one frame.
    ///
    /// `lastvisplane` tracks how many are in use.  Initialized to all-zero at
    /// program start; each slot is reset via [`R_FindPlane`] before use.
    ///
    /// # Note
    ///
    /// The compile-time initializer leaves `top[]` as `0x00` instead of `0xFF`.
    /// [`R_ClearPlanes`] does not reset individual slots; only [`R_FindPlane`]
    /// writes the sentinel `0xFF` pattern when it claims a new slot.  This matches
    /// the C behavior where `lastvisplane` is rewound to `visplanes[0]` each frame
    /// and slots are re-initialized on demand.
    pub visplanes: [visplane_t; MAXVISPLANES],
    /// Index of the next free slot in `visplanes` (the C pointer `lastvisplane`).
    ///
    /// Reset to 0 by [`R_ClearPlanes`] at the start of each frame.
    pub lastvisplane: usize,
    /// Index of the current floor visplane being built for this BSP subtree,
    /// or `None` when the floor is not visible (the C null `floorplane`).
    ///
    /// Updated by `R_StoreWallRange` (in `r_segs`) via [`R_CheckPlane`].
    pub floorplane: Option<usize>,
    /// Index of the current ceiling visplane being built for this BSP subtree,
    /// or `None` when the ceiling is not visible (the C null `ceilingplane`).
    ///
    /// Updated by `R_StoreWallRange` (in `r_segs`) via [`R_CheckPlane`].
    pub ceilingplane: Option<usize>,
    /// Allocation watermark within the renderer's `openings`.
    ///
    /// Advanced by `R_StoreWallRange` each time it allocates a clipping
    /// sub-array.  Reset to the start of `openings` by [`R_ClearPlanes`].
    pub lastopening: *mut i16,
    /// Per-column floor clip (highest opaque pixel so far, inclusive).
    ///
    /// `floorclip[x]` is the screen-y of the lowest pixel that is still open
    /// for floor rendering in column `x`.  Initialized to `viewheight` (fully
    /// open) by [`R_ClearPlanes`].
    pub floorclip: [i16; SCREENWIDTH],
    /// Per-column ceiling clip (lowest opaque pixel so far, inclusive).
    ///
    /// `ceilingclip[x]` is the screen-y of the highest pixel that is still
    /// open for ceiling rendering in column `x`.  Initialized to `-1` (fully
    /// open) by [`R_ClearPlanes`].
    pub ceilingclip: [i16; SCREENWIDTH],
    /// Per-row span start columns, indexed by screen-y.
    ///
    /// [`R_MakeSpans`] writes the left edge of a span in progress here;
    /// [`R_MapPlane`] reads it to obtain `x1` when the span ends.
    pub spanstart: [i32; SCREENHEIGHT],
    /// Pointer into the distance-based light table for the current plane.
    ///
    /// Set by [`R_DrawPlanes`] from `zlight[light]` before iterating a
    /// visplane's columns.
    pub planezlight: *const *const lighttable_t,
    /// Absolute height difference between the current flat and the viewpoint.
    ///
    /// Written by [`R_DrawPlanes`] before calling [`R_MakeSpans`]; read by
    /// [`R_MapPlane`] to derive the per-row texture step.
    pub planeheight: fixed_t,
    /// Per-row slope factor used to convert plane distance to a screen-y fraction.
    ///
    /// Precomputed by `R_ExecuteSetViewSize` in `r_main` for each possible
    /// screen row.
    pub yslope: [fixed_t; SCREENHEIGHT],
    /// Per-column angular distance scale from the screen center.
    ///
    /// Precomputed by `R_ExecuteSetViewSize` in `r_main`; used by
    /// [`R_MapPlane`] to project a flat texel onto a column.
    pub distscale: [fixed_t; SCREENWIDTH],
    /// Base x-axis texture step per unit of distance, computed from `viewangle`.
    ///
    /// Recomputed each frame by [`R_ClearPlanes`].
    pub basexscale: fixed_t,
    /// Base y-axis texture step per unit of distance, computed from `viewangle`.
    ///
    /// Recomputed each frame by [`R_ClearPlanes`].
    pub baseyscale: fixed_t,
    /// Cache of the last `planeheight` value computed for each screen row.
    ///
    /// [`R_MapPlane`] avoids recomputing `distance`, `xstep`, and `ystep` when
    /// the plane height has not changed since the previous span on the same row.
    pub cachedheight: [fixed_t; SCREENHEIGHT],
    /// Cache of the last computed world distance for each screen row.
    ///
    /// Paired with `cachedheight`.
    pub cacheddistance: [fixed_t; SCREENHEIGHT],
    /// Cache of the last computed flat x-step for each screen row.
    ///
    /// Paired with `cachedheight`.
    pub cachedxstep: [fixed_t; SCREENHEIGHT],
    /// Cache of the last computed flat y-step for each screen row.
    ///
    /// Paired with `cachedheight`.
    pub cachedystep: [fixed_t; SCREENHEIGHT],
}

impl PlaneState {
    /// The state before the first frame: an empty pool, zeroed tables.
    pub const fn new() -> Self {
        Self {
            visplanes: [ZERO_VISPLANE; MAXVISPLANES],
            lastvisplane: 0,
            floorplane: None,
            ceilingplane: None,
            lastopening: ptr::null_mut(),
            floorclip: [0; SCREENWIDTH],
            ceilingclip: [0; SCREENWIDTH],
            spanstart: [0; SCREENHEIGHT],
            planezlight: ptr::null(),
            planeheight: 0,
            yslope: [0; SCREENHEIGHT],
            distscale: [0; SCREENWIDTH],
            basexscale: 0,
            baseyscale: 0,
            cachedheight: [0; SCREENHEIGHT],
            cacheddistance: [0; SCREENHEIGHT],
            cachedxstep: [0; SCREENHEIGHT],
            cachedystep: [0; SCREENHEIGHT],
        }
    }
}

impl Default for PlaneState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Imports from other modules
// ---------------------------------------------------------------------------

use crate::engine::{
    render_3d::{
        Renderer,
        data::R_GetColumn,
        draw::{ColumnParams, DrawState, SpanParams},
        main::{ViewState, zlight_row},
    },
    wad::{W_CacheLumpNum, W_ReleaseLumpNum},
};

// ---------------------------------------------------------------------------
// R_InitPlanes — called once at game startup
// ---------------------------------------------------------------------------

/// Initialises the plane renderer at game startup.
///
/// The C source contains only a comment `"Doh!"`.  There is nothing to
/// initialise; the function exists so that `R_Init` can call it unconditionally
/// alongside the other renderer subsystems.
pub fn R_InitPlanes() {
    // Doh! — nothing to do here, matching original C
}

// ---------------------------------------------------------------------------
// R_MapPlane — maps a single plane span
// ---------------------------------------------------------------------------

/// Draws one horizontal span of a floor or ceiling flat.
///
/// Called by [`R_MakeSpans`] with the screen row `y` and the inclusive column
/// range `[x1, x2]`.  Sets up the `ds_*` fields of `span` required by
/// `spanfunc` (all but `ds_source`, which the caller set for the plane) and
/// then calls it.
///
/// Uses `planeheight`, `basexscale`, `baseyscale`, `distscale`, `yslope` and
/// `planezlight` from `planes` and `viewx`, `viewy`, `viewangle`,
/// `xtoviewangle`, `fixedcolormap` and `spanfunc` from `view`.
///
/// # Safety
///
/// `planezlight` and `fixedcolormap` must be valid when called.  The
/// range-check feature gate guards against out-of-bounds `x1`/`x2`/`y`
/// values; without it the caller is responsible for passing valid coordinates.
pub fn R_MapPlane(
    planes: &mut PlaneState,
    view: &ViewState,
    draw: &DrawState,
    span: &mut SpanParams,
    y: i32,
    x1: i32,
    x2: i32,
) {
    unsafe {
        #[cfg(feature = "rangecheck")]
        {
            if x2 < x1 || x1 < 0 || x2 >= draw.viewwidth || y > draw.viewheight {
                // Would call I_Error — skip in Rust for now
                return;
            }
        }

        let y_usize = y as usize;

        let distance = if planes.planeheight != planes.cachedheight[y_usize] {
            planes.cachedheight[y_usize] = planes.planeheight;
            let d = FixedMul(planes.planeheight, planes.yslope[y_usize]);
            planes.cacheddistance[y_usize] = d;
            planes.cachedxstep[y_usize] = FixedMul(d, planes.basexscale);
            planes.cachedystep[y_usize] = FixedMul(d, planes.baseyscale);
            d
        } else {
            planes.cacheddistance[y_usize]
        };

        span.ds_xstep = planes.cachedxstep[y_usize];
        span.ds_ystep = planes.cachedystep[y_usize];

        let length = FixedMul(distance, planes.distscale[x1 as usize]);
        let angle = view.viewangle.wrapping_add(view.xtoviewangle[x1 as usize]) >> ANGLETOFINESHIFT;
        span.ds_xfrac = view
            .viewx
            .wrapping_add(FixedMul(finecosine(angle as usize), length));
        span.ds_yfrac = (-view.viewy).wrapping_sub(FixedMul(
            tables::finesine[angle as usize & FINEMASK as usize],
            length,
        ));

        if !view.fixedcolormap.is_null() {
            span.ds_colormap = view.fixedcolormap;
        } else {
            let mut index = (distance >> LIGHTZSHIFT) as usize;
            if index >= MAXLIGHTZ {
                index = MAXLIGHTZ - 1;
            }
            span.ds_colormap = *planes.planezlight.add(index) as *mut u8;
        }

        span.ds_y = y;
        span.ds_x1 = x1;
        span.ds_x2 = x2;

        (view.spanfunc)(span, draw);
    }
}

// ---------------------------------------------------------------------------
// R_ClearPlanes — called at beginning of each frame
// ---------------------------------------------------------------------------

/// Resets all plane state at the start of each frame.
///
/// Initialises clip arrays, rewinds the visplane and opening allocation
/// cursors, clears the per-row height cache, and recomputes the base
/// texture-step scales from the current `viewangle`.
///
/// Must be called exactly once per frame before any BSP traversal begins.
pub fn R_ClearPlanes(renderer: &mut Renderer) {
    let Renderer {
        draw,
        view,
        planes,
        openings,
        ..
    } = renderer;
    let (viewangle, centerxfrac) = (view.viewangle, view.centerxfrac);

    let vw = draw.viewwidth as usize;
    let vh = draw.viewheight as i16;

    for i in 0..vw {
        planes.floorclip[i] = vh;
        planes.ceilingclip[i] = -1;
    }

    planes.lastvisplane = 0;
    // The drawsegs keep slices of the openings until the frame's masked
    // phase.
    planes.lastopening = openings.as_mut_ptr();

    // Reset cache heights
    for h in &mut planes.cachedheight[..200] {
        *h = 0;
    }

    // Left to right mapping
    let angle = (viewangle.wrapping_sub(ANG90)) >> ANGLETOFINESHIFT;
    planes.basexscale = FixedDiv(finecosine(angle as usize), centerxfrac);
    planes.baseyscale = 0i32.wrapping_sub(FixedDiv(
        tables::finesine[angle as usize & FINEMASK as usize],
        centerxfrac,
    ));
}

// ---------------------------------------------------------------------------
// R_FindPlane — find or create a visplane matching the given parameters
// ---------------------------------------------------------------------------

/// Returns the index of a visplane for the given `height`, `picnum`, and
/// `lightlevel`.
///
/// Searches the already-allocated visplanes for an exact match.  If none is
/// found, claims the next free slot from `visplanes` and initialises it.
/// All sky flats (`picnum == skyflatnum`) share a single visplane at height 0
/// and light level 0.
///
/// Aborts via [`crate::i_error!`] if the visplane pool (128 entries) is exhausted
/// (matches the C source `I_Error` path).
///
/// The returned index is valid for the current frame (until the next
/// [`R_ClearPlanes`] call).
pub fn R_FindPlane(
    planes: &mut PlaneState,
    skyflatnum: i32,
    height: fixed_t,
    picnum: i32,
    lightlevel: i32,
) -> usize {
    let mut h = height;
    let mut ll = lightlevel;

    // All sky planes map together
    if picnum == skyflatnum {
        h = 0;
        ll = 0;
    }

    // Search existing visplanes
    for check in 0..planes.lastvisplane {
        let plane = &planes.visplanes[check];
        if h == plane.height && picnum == plane.picnum && ll == plane.lightlevel {
            return check;
        }
    }

    // Need a new visplane
    if planes.lastvisplane >= MAXVISPLANES {
        // Match C doomgeneric: abort rather than hand callers a null
        // visplane pointer (they dereference unconditionally).
        i_error!("R_FindPlane: no more visplanes");
    }

    let new_index = planes.lastvisplane;
    planes.lastvisplane += 1;

    let new_vp = &mut planes.visplanes[new_index];
    new_vp.height = h;
    new_vp.picnum = picnum;
    new_vp.lightlevel = ll;
    new_vp.minx = SCREENWIDTH as i32;
    new_vp.maxx = -1;

    // memset top to 0xFF
    for t in new_vp.top.iter_mut() {
        *t = 0xFF;
    }

    new_index
}

// ---------------------------------------------------------------------------
// R_CheckPlane — check if a visplane can cover [start..stop] without overlap
// ---------------------------------------------------------------------------

/// Ensures the visplane at index `pl` can accommodate the column range
/// `[start, stop]`.
///
/// If the range does not overlap any already-filled column in `pl`, the
/// plane's `minx`/`maxx` bounds are extended to cover the union and `pl` is
/// returned unchanged.  Otherwise a new visplane with the same flat/height/
/// lightlevel is allocated for `[start, stop]` and its index returned.
///
/// `pl` must be an index obtained from [`R_FindPlane`] this frame.
pub fn R_CheckPlane(planes: &mut PlaneState, pl: usize, start: i32, stop: i32) -> usize {
    let plane = &mut planes.visplanes[pl];
    let intrl = if start < plane.minx {
        plane.minx
    } else {
        start
    };
    let unionl = if start < plane.minx {
        start
    } else {
        plane.minx
    };

    let intrh = if stop > plane.maxx { plane.maxx } else { stop };
    let unionh = if stop > plane.maxx { stop } else { plane.maxx };

    // Check for overlap in the intersection range
    let mut x = intrl;
    while x <= intrh {
        if plane.top[x as usize] != 0xFF {
            break;
        }
        x += 1;
    }

    if x > intrh {
        // No overlap — extend the visplane
        plane.minx = unionl;
        plane.maxx = unionh;
        return pl;
    }

    let (height, picnum, lightlevel) = (plane.height, plane.picnum, plane.lightlevel);

    // Make a new visplane
    let new_index = planes.lastvisplane;
    planes.lastvisplane += 1;

    let new_vp = &mut planes.visplanes[new_index];
    new_vp.height = height;
    new_vp.picnum = picnum;
    new_vp.lightlevel = lightlevel;
    new_vp.minx = start;
    new_vp.maxx = stop;

    for t in new_vp.top.iter_mut() {
        *t = 0xFF;
    }

    new_index
}

// ---------------------------------------------------------------------------
// R_MakeSpans — generate spans from ceiling/floor clip differences
// ---------------------------------------------------------------------------

/// Closes and opens horizontal spans as the per-column clip bounds change.
///
/// Called once per column `x` during [`R_DrawPlanes`].  `t1`/`b1` are the
/// top/bottom clip values for the previous column; `t2`/`b2` are those for
/// the current column.  Any row that was open in the previous column but
/// closed in the current one is flushed to [`R_MapPlane`].  Any row that
/// opens in the current column but was closed in the previous one has its
/// start recorded in `spanstart`.
///
/// All indices derived from `t1`/`b1`/`t2`/`b2` must be valid screen rows
/// (`0 <= row < SCREENHEIGHT`); the caller (the `R_DrawPlanes` loop) is
/// responsible for keeping them in range.
#[allow(clippy::too_many_arguments)]
pub fn R_MakeSpans(
    planes: &mut PlaneState,
    view: &ViewState,
    draw: &DrawState,
    span: &mut SpanParams,
    x: i32,
    t1: i32,
    b1: i32,
    t2: i32,
    b2: i32,
) {
    let mut t1 = t1;
    let mut b1 = b1;
    let mut t2 = t2;
    let mut b2 = b2;

    while t1 < t2 && t1 <= b1 {
        let x1 = planes.spanstart[t1 as usize];
        R_MapPlane(planes, view, draw, span, t1, x1, x - 1);
        t1 += 1;
    }
    while b1 > b2 && b1 >= t1 {
        let x1 = planes.spanstart[b1 as usize];
        R_MapPlane(planes, view, draw, span, b1, x1, x - 1);
        b1 -= 1;
    }

    while t2 < t1 && t2 <= b2 {
        planes.spanstart[t2 as usize] = x;
        t2 += 1;
    }
    while b2 > b1 && b2 >= t2 {
        planes.spanstart[b2 as usize] = x;
        b2 -= 1;
    }
}

// ---------------------------------------------------------------------------
// R_DrawPlanes — draw all visplanes at end of frame
// ---------------------------------------------------------------------------

use crate::engine::zone::PU_STATIC;

/// Rasterizes all accumulated visplanes into horizontal pixel spans.
///
/// Called once per frame after BSP traversal is complete, as the frame's
/// plane phase.  Iterates over every visplane in the pool up to
/// `lastvisplane`.  Sky flats are drawn as vertical columns via `colfunc`;
/// regular flats are drawn as horizontal spans via [`R_MakeSpans`] /
/// [`R_MapPlane`] / `spanfunc`.
///
/// The `W_CacheLumpNum` / `W_ReleaseLumpNum` calls need the WAD lump
/// directory, which is set up before any frame.
pub fn R_DrawPlanes(renderer: &mut Renderer) {
    unsafe {
        let Renderer {
            sky,
            sprites,
            data,
            view,
            draw,
            planes,
            ..
        } = renderer;
        let (skyflatnum, skytexture, skytexturemid) =
            (sky.skyflatnum, sky.skytexture, sky.skytexturemid);
        let pspriteiscale = sprites.pspriteiscale;

        let mut column = ColumnParams::new(view.centery, data.colormaps);
        let mut span = SpanParams::new();

        for pl in 0..planes.lastvisplane {
            if planes.visplanes[pl].minx > planes.visplanes[pl].maxx {
                continue;
            }

            // Sky flat
            if planes.visplanes[pl].picnum == skyflatnum {
                let plane = &planes.visplanes[pl];
                column.dc_iscale = pspriteiscale >> view.detailshift;
                // Sky is always drawn full bright,
                // i.e. colormaps[0] is used.
                // Because of this hack, sky is not affected
                // by INVUL inverse mapping.
                column.dc_colormap = data.colormaps;
                column.dc_texturemid = skytexturemid;

                for x in plane.minx..=plane.maxx {
                    column.dc_yl = plane.top[x as usize] as i32;
                    column.dc_yh = plane.bottom[x as usize] as i32;

                    if column.dc_yl <= column.dc_yh {
                        let angle = view.viewangle.wrapping_add(view.xtoviewangle[x as usize])
                            >> ANGLETOSKYSHIFT;
                        column.dc_x = x;
                        column.dc_source = R_GetColumn(data, skytexture, angle as i32);
                        (view.colfunc)(&mut column, draw);
                    }
                }
                continue;
            }

            // Regular flat
            let flat_idx = *data
                .flattranslation
                .offset(planes.visplanes[pl].picnum as isize);
            let lumpnum = data.firstflat + flat_idx;
            span.ds_source = W_CacheLumpNum(lumpnum, PU_STATIC) as *mut u8;

            planes.planeheight = (planes.visplanes[pl].height.wrapping_sub(view.viewz)).abs();
            let mut light = (planes.visplanes[pl].lightlevel >> LIGHTSEGSHIFT) + view.extralight;

            if light >= LIGHTLEVELS as i32 {
                light = LIGHTLEVELS as i32 - 1;
            }
            if light < 0 {
                light = 0;
            }

            // Set planezlight — in the full implementation this would be
            // a pointer into zlight[light]
            let light_usize = light as usize;
            if light_usize < LIGHTLEVELS {
                planes.planezlight = zlight_row(view, light_usize) as *const *const lighttable_t;
            }

            // Set sentinel values at the visplane boundaries. The C code does
            // pl->top[pl->maxx+1] = 0xff and pl->top[pl->minx-1] = 0xff, which
            // relies on padding bytes when at screen edges.
            let plane = &mut planes.visplanes[pl];
            if plane.minx > 0 {
                plane.top[(plane.minx - 1) as usize] = 0xFF;
            } else {
                plane.pad1 = 0xFF;
            }
            if plane.maxx < SCREENWIDTH as i32 - 1 {
                plane.top[(plane.maxx + 1) as usize] = 0xFF;
            } else {
                plane.pad2 = 0xFF;
            }

            // R_MakeSpans needs the plane state mutably; work from a copy of
            // the finished visplane.
            let plane = planes.visplanes[pl];
            let stop = plane.maxx + 1;
            for x in plane.minx..=stop {
                let prev_x = x - 1;
                let t_top = if prev_x < 0 {
                    plane.pad1
                } else {
                    plane.top[prev_x as usize]
                };
                let t_bottom = if prev_x < 0 {
                    plane.pad3
                } else {
                    plane.bottom[prev_x as usize]
                };
                let b_top = if x == SCREENWIDTH as i32 {
                    plane.pad2
                } else {
                    plane.top[x as usize]
                };
                let b_bottom = if x == SCREENWIDTH as i32 {
                    plane.pad4
                } else {
                    plane.bottom[x as usize]
                };
                R_MakeSpans(
                    planes,
                    view,
                    draw,
                    &mut span,
                    x,
                    t_top as i32,
                    t_bottom as i32,
                    b_top as i32,
                    b_bottom as i32,
                );
            }

            W_ReleaseLumpNum(lumpnum);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The plane state starts with an empty pool and no current planes.
    #[test]
    fn state_starts_empty() {
        let planes = PlaneState::new();
        assert_eq!(planes.lastvisplane, 0);
        assert_eq!((planes.floorplane, planes.ceilingplane), (None, None));
        assert!(planes.lastopening.is_null());
    }

    /// `R_CheckPlane` extends a plane whose columns do not overlap the new
    /// range and splits off a new plane when they do.
    #[test]
    fn check_plane_extends_or_splits() {
        let mut planes = Box::new(PlaneState::new());
        planes.lastvisplane = 1;
        let first = &mut planes.visplanes[0];
        first.top = [0xFF; SCREENWIDTH];
        first.height = 7;
        first.minx = 10;
        first.maxx = 20;

        assert_eq!(R_CheckPlane(&mut planes, 0, 21, 30), 0);
        assert_eq!(
            (planes.visplanes[0].minx, planes.visplanes[0].maxx),
            (10, 30)
        );

        planes.visplanes[0].top[25] = 5;
        assert_eq!(R_CheckPlane(&mut planes, 0, 25, 40), 1);
        assert_eq!(planes.lastvisplane, 2);
        let split = &planes.visplanes[1];
        assert_eq!((split.height, split.minx, split.maxx), (7, 25, 40));
        assert!(split.top.iter().all(|&top| top == 0xFF));
    }
}
