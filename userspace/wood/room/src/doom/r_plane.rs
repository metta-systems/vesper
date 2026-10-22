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
//! with [`colfunc`] rather than [`spanfunc`].
//!
//! # Coordinate system
//!
//! All heights use the fixed-point `fixed_t` type (`i32`, 16.16 format,
//! `FRACUNIT = 1 << 16`).  Angles are `angle_t = u32` where a full circle is
//! `0x1_0000_0000` (wrapping arithmetic).

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::{c_int, c_short, c_uchar};
use std::ptr;

use crate::i_error;
use super::m_fixed::{fixed_t, FixedDiv, FixedMul};
use super::r_sky;
use super::tables;
use super::tables::{ANG90, ANGLETOFINESHIFT, FINEMASK};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Screen width in pixels, re-exported from [`i_video`] for local use.
const SCREENWIDTH: usize = crate::doom::i_video::SCREENWIDTH as usize;

/// Screen height in pixels, re-exported from [`i_video`] for local use.
const SCREENHEIGHT: usize = crate::doom::i_video::SCREENHEIGHT as usize;

/// Maximum number of simultaneous visplanes per frame.
///
/// Doom aborts with `I_Error` when this limit is exceeded.  The Rust port
/// aborts via [`i_error!`] when exceeded (see [`R_FindPlane`]).
const MAXVISPLANES: usize = 128;

/// Maximum number of `c_short` slots in the [`openings`] array.
///
/// Used as scratch space for sprite clipping arrays stored by
/// [`R_StoreWallRange`] (in `r_segs`).  The C source comments this as `"?"`.
const MAXOPENINGS: usize = SCREENWIDTH * 64;

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
fn finecosine(idx: usize) -> c_int {
    unsafe { *tables::finecosine.0.add(idx & FINEMASK as usize) }
}

// ---------------------------------------------------------------------------
// Type aliases
// ---------------------------------------------------------------------------

/// Colormap entry type: a single palette index byte.
type lighttable_t = c_uchar;

/// Function pointer type for horizontal-span and sky-column draw callbacks.
///
/// The two parameters are the left (`x1`) and right (`x2`) screen columns of
/// the span, matching the C `planefunction_t` signature
/// `void (*)(int top, int bottom)`.
pub type planefunction_t = unsafe extern "C" fn(c_int, c_int);

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
/// land in the padding when the plane spans the full screen width.  The layout
/// is verified at compile time by the `assert!` blocks below.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct visplane_t {
    /// World height of the flat, in fixed-point units.
    pub height: fixed_t,
    /// Flat texture number (lump index relative to `firstflat`).
    pub picnum: c_int,
    /// Sector light level (0-255) for this plane.
    pub lightlevel: c_int,
    /// Leftmost screen column used by this plane (inclusive).
    pub minx: c_int,
    /// Rightmost screen column used by this plane (inclusive).
    pub maxx: c_int,
    /// Padding byte before `top[]`; acts as a sentinel slot for
    /// `top[minx-1]` when `minx == 0`.
    pub pad1: c_uchar,
    /// Per-column top clip (inclusive screen-y).  `0xFF` means unused.
    pub top: [c_uchar; SCREENWIDTH],
    /// Padding byte after `top[]`; sentinel for `top[maxx+1]` when
    /// `maxx == SCREENWIDTH - 1`.
    pub pad2: c_uchar,
    /// Padding byte before `bottom[]`; mirrors the pad3 slot in C.
    pub pad3: c_uchar,
    /// Per-column bottom clip (inclusive screen-y).
    pub bottom: [c_uchar; SCREENWIDTH],
    /// Padding byte after `bottom[]`.
    pub pad4: c_uchar,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(
    std::mem::size_of::<visplane_t>() == 664,
    "visplane_t size mismatch"
);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::offset_of!(visplane_t, height) == 0);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::offset_of!(visplane_t, picnum) == 4);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::offset_of!(visplane_t, lightlevel) == 8);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::offset_of!(visplane_t, minx) == 12);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::offset_of!(visplane_t, maxx) == 16);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::offset_of!(visplane_t, pad1) == 20);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::offset_of!(visplane_t, top) == 21);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::offset_of!(visplane_t, pad2) == 341);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::offset_of!(visplane_t, pad3) == 342);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::offset_of!(visplane_t, bottom) == 343);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::offset_of!(visplane_t, pad4) == 663);

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
// Global mutable state (exported for C consumers)
// ---------------------------------------------------------------------------

/// Legacy floor-span callback pointer, carried over from the C source.
///
/// Not used in this port; `R_MapPlane` does not invoke it.  Retained and
/// exported as `#[no_mangle]` for ABI compatibility with C consumers.
#[no_mangle]
pub static mut floorfunc: Option<planefunction_t> = None;

/// Legacy ceiling-span callback pointer, carried over from the C source.
///
/// Not used in this port; `R_MapPlane` does not invoke it.  Retained and
/// exported as `#[no_mangle]` for ABI compatibility with C consumers.
/// Parallels [`floorfunc`].
#[no_mangle]
pub static mut ceilingfunc: Option<planefunction_t> = None;

/// Fixed-size pool of visplane accumulators for one frame.
///
/// Exported as `#[no_mangle]` for C callers.  [`lastvisplane`] tracks how
/// many are in use.  Initialized to all-zero at program start; each slot is
/// reset via [`R_FindPlane`] before use.
///
/// # Note
///
/// The compile-time initializer leaves `top[]` as `0x00` instead of `0xFF`.
/// [`R_ClearPlanes`] does not reset individual slots; only [`R_FindPlane`]
/// writes the sentinel `0xFF` pattern when it claims a new slot.  This matches
/// the C behavior where `lastvisplane` is rewound to `visplanes[0]` each frame
/// and slots are re-initialized on demand.
#[no_mangle]
pub static mut visplanes: [visplane_t; MAXVISPLANES] = {
    // Can't use Default::default() in const context, so we transmute
    // from zeroed bytes. Each visplane_t is initialized at runtime.
    const ZERO: visplane_t = visplane_t {
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
    let arr: [visplane_t; MAXVISPLANES] = [ZERO; MAXVISPLANES];
    // Initialize top to 0xFF for each visplane at runtime via memset-like
    // approach. For now leave as 0; R_ClearPlanes will reset them.
    arr
};

/// Pointer to the next free slot in [`visplanes`].
///
/// Exported as `#[no_mangle]` for C callers.  Reset to `&visplanes[0]` by
/// [`R_ClearPlanes`] at the start of each frame.
#[no_mangle]
pub static mut lastvisplane: *mut visplane_t = ptr::null_mut();

/// The current floor visplane being built for this BSP subtree.
///
/// Exported as `#[no_mangle]` for C callers.  Updated by
/// `R_StoreWallRange` (in `r_segs`) via [`R_CheckPlane`].
#[no_mangle]
pub static mut floorplane: *mut visplane_t = ptr::null_mut();

/// The current ceiling visplane being built for this BSP subtree.
///
/// Exported as `#[no_mangle]` for C callers.  Updated by
/// `R_StoreWallRange` (in `r_segs`) via [`R_CheckPlane`].
#[no_mangle]
pub static mut ceilingplane: *mut visplane_t = ptr::null_mut();

/// Scratch buffer for sprite clipping arrays.
///
/// Exported as `#[no_mangle]` for C callers.  Slices within this buffer are
/// handed out by `R_StoreWallRange` (in `r_segs`) and later read by the
/// sprite renderer.  [`lastopening`] tracks the allocation watermark.
#[no_mangle]
pub static mut openings: [c_short; MAXOPENINGS] = [0; MAXOPENINGS];

/// Allocation watermark within [`openings`].
///
/// Exported as `#[no_mangle]` for C callers.  Advanced by
/// `R_StoreWallRange` each time it allocates a clipping sub-array.
/// Reset to `&openings[0]` by [`R_ClearPlanes`].
#[no_mangle]
pub static mut lastopening: *mut c_short = ptr::null_mut();

/// Per-column floor clip (highest opaque pixel so far, inclusive).
///
/// Exported as `#[no_mangle]` for C callers.  `floorclip[x]` is the
/// screen-y of the lowest pixel that is still open for floor rendering in
/// column `x`.  Initialized to `viewheight` (fully open) by
/// [`R_ClearPlanes`].
#[no_mangle]
pub static mut floorclip: [c_short; SCREENWIDTH] = [0; SCREENWIDTH];

/// Per-column ceiling clip (lowest opaque pixel so far, inclusive).
///
/// Exported as `#[no_mangle]` for C callers.  `ceilingclip[x]` is the
/// screen-y of the highest pixel that is still open for ceiling rendering in
/// column `x`.  Initialized to `-1` (fully open) by [`R_ClearPlanes`].
#[no_mangle]
pub static mut ceilingclip: [c_short; SCREENWIDTH] = [0; SCREENWIDTH];

/// Per-row span start columns, indexed by screen-y.
///
/// Exported as `#[no_mangle]` for C callers.  [`R_MakeSpans`] writes the
/// left edge of a span in progress here; [`R_MapPlane`] reads it to obtain
/// `x1` when the span ends.
#[no_mangle]
pub static mut spanstart: [c_int; SCREENHEIGHT] = [0; SCREENHEIGHT];

/// Per-row span stop columns (unused in the current implementation).
///
/// Exported as `#[no_mangle]` for C callers.  Present in the C source but
/// never written; kept for ABI compatibility.
#[no_mangle]
pub static mut spanstop: [c_int; SCREENHEIGHT] = [0; SCREENHEIGHT];

/// Pointer into the distance-based light table for the current plane.
///
/// Exported as `#[no_mangle]` for C callers.  Set by [`R_DrawPlanes`] from
/// `zlight[light]` before iterating a visplane's columns.
#[no_mangle]
pub static mut planezlight: *const *const lighttable_t = ptr::null();

/// Absolute height difference between the current flat and the viewpoint.
///
/// Exported as `#[no_mangle]` for C callers.  Written by [`R_DrawPlanes`]
/// before calling [`R_MakeSpans`]; read by [`R_MapPlane`] to derive the
/// per-row texture step.
#[no_mangle]
pub static mut planeheight: fixed_t = 0;

/// Per-row slope factor used to convert plane distance to a screen-y fraction.
///
/// Exported as `#[no_mangle]` for C callers.  Precomputed by `R_ExecuteSetViewSize`
/// in `r_main` for each possible screen row.
#[no_mangle]
pub static mut yslope: [fixed_t; SCREENHEIGHT] = [0; SCREENHEIGHT];

/// Per-column angular distance scale from the screen center.
///
/// Exported as `#[no_mangle]` for C callers.  Precomputed by `R_ExecuteSetViewSize`
/// in `r_main`; used by [`R_MapPlane`] to project a flat texel onto a column.
#[no_mangle]
pub static mut distscale: [fixed_t; SCREENWIDTH] = [0; SCREENWIDTH];

/// Base x-axis texture step per unit of distance, computed from `viewangle`.
///
/// Exported as `#[no_mangle]` for C callers.  Recomputed each frame by
/// [`R_ClearPlanes`].
#[no_mangle]
pub static mut basexscale: fixed_t = 0;

/// Base y-axis texture step per unit of distance, computed from `viewangle`.
///
/// Exported as `#[no_mangle]` for C callers.  Recomputed each frame by
/// [`R_ClearPlanes`].
#[no_mangle]
pub static mut baseyscale: fixed_t = 0;

/// Cache of the last `planeheight` value computed for each screen row.
///
/// Exported as `#[no_mangle]` for C callers.  [`R_MapPlane`] avoids
/// recomputing `distance`, `xstep`, and `ystep` when the plane height has not
/// changed since the previous span on the same row.
#[no_mangle]
pub static mut cachedheight: [fixed_t; SCREENHEIGHT] = [0; SCREENHEIGHT];

/// Cache of the last computed world distance for each screen row.
///
/// Exported as `#[no_mangle]` for C callers.  Paired with [`cachedheight`].
#[no_mangle]
pub static mut cacheddistance: [fixed_t; SCREENHEIGHT] = [0; SCREENHEIGHT];

/// Cache of the last computed flat x-step for each screen row.
///
/// Exported as `#[no_mangle]` for C callers.  Paired with [`cachedheight`].
#[no_mangle]
pub static mut cachedxstep: [fixed_t; SCREENHEIGHT] = [0; SCREENHEIGHT];

/// Cache of the last computed flat y-step for each screen row.
///
/// Exported as `#[no_mangle]` for C callers.  Paired with [`cachedheight`].
#[no_mangle]
pub static mut cachedystep: [fixed_t; SCREENHEIGHT] = [0; SCREENHEIGHT];

// ---------------------------------------------------------------------------
// Imports from other modules
// ---------------------------------------------------------------------------

use crate::doom::r_data::{colormaps, firstflat, flattranslation, R_GetColumn};
use crate::doom::r_draw::{
    dc_colormap, dc_iscale, dc_source, dc_texturemid, dc_x, dc_yh, dc_yl, ds_colormap, ds_source,
    ds_x1, ds_x2, ds_xfrac, ds_xstep, ds_y, ds_yfrac, ds_ystep, viewheight, viewwidth,
};
use crate::doom::r_main::{
    centerxfrac, colfunc, detailshift, extralight, fixedcolormap, spanfunc, viewangle, viewx,
    viewy, viewz, xtoviewangle, zlight,
};
use crate::doom::r_sky::{skytexture, skytexturemid};
use crate::doom::r_things::pspriteiscale;
use crate::doom::w_wad::{W_CacheLumpNum, W_ReleaseLumpNum};

// ---------------------------------------------------------------------------
// R_InitPlanes — called once at game startup
// ---------------------------------------------------------------------------

/// Initialises the plane renderer at game startup.
///
/// The C source contains only a comment `"Doh!"`.  There is nothing to
/// initialise; the function exists so that `R_Init` can call it unconditionally
/// alongside the other renderer subsystems.
///
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub extern "C" fn R_InitPlanes() {
    // Doh! — nothing to do here, matching original C
}

// ---------------------------------------------------------------------------
// R_MapPlane — maps a single plane span
// ---------------------------------------------------------------------------

/// Draws one horizontal span of a floor or ceiling flat.
///
/// Called by [`R_MakeSpans`] with the screen row `y` and the inclusive column
/// range `[x1, x2]`.  Sets up all `ds_*` globals required by [`spanfunc`]
/// and then calls it.
///
/// Uses globals: `planeheight`, `basexscale`, `baseyscale`, `viewx`, `viewy`,
/// `viewangle`, `xtoviewangle`, `distscale`, `yslope`, `planezlight`,
/// `fixedcolormap`, `spanfunc`.
///
/// Exported as `#[no_mangle]` for C callers.
///
/// # Safety
///
/// Reads and writes numerous `static mut` renderer globals.  All pointers
/// (`planezlight`, `spanfunc`, `fixedcolormap`) must be valid when called.
/// The range-check feature gate guards against out-of-bounds `x1`/`x2`/`y`
/// values; without it the caller is responsible for passing valid coordinates.
#[no_mangle]
pub extern "C" fn R_MapPlane(y: c_int, x1: c_int, x2: c_int) {
    unsafe {
        #[cfg(feature = "rangecheck")]
        {
            if x2 < x1 || x1 < 0 || x2 >= viewwidth || y > viewheight {
                // Would call I_Error — skip in Rust for now
                return;
            }
        }

        let y_usize = y as usize;

        let distance = if planeheight != cachedheight[y_usize] {
            cachedheight[y_usize] = planeheight;
            let d = FixedMul(planeheight, yslope[y_usize]);
            cacheddistance[y_usize] = d;
            cachedxstep[y_usize] = FixedMul(d, basexscale);
            cachedystep[y_usize] = FixedMul(d, baseyscale);
            d
        } else {
            cacheddistance[y_usize]
        };

        ds_xstep = cachedxstep[y_usize];
        ds_ystep = cachedystep[y_usize];

        let length = FixedMul(distance, distscale[x1 as usize]);
        let angle = viewangle.wrapping_add(xtoviewangle[x1 as usize]) >> ANGLETOFINESHIFT;
        ds_xfrac = viewx.wrapping_add(FixedMul(finecosine(angle as usize), length));
        ds_yfrac = (-viewy).wrapping_sub(FixedMul(
            tables::finesine[angle as usize & FINEMASK as usize],
            length,
        ));

        if !fixedcolormap.is_null() {
            ds_colormap = fixedcolormap;
        } else {
            let mut index = (distance >> LIGHTZSHIFT) as usize;
            if index >= MAXLIGHTZ {
                index = MAXLIGHTZ - 1;
            }
            ds_colormap = *planezlight.add(index) as *mut u8;
        }

        ds_y = y;
        ds_x1 = x1;
        ds_x2 = x2;

        if let Some(func) = spanfunc {
            func();
        }
    }
}

// ---------------------------------------------------------------------------
// R_ClearPlanes — called at beginning of each frame
// ---------------------------------------------------------------------------

/// Resets all plane state at the start of each frame.
///
/// Initialises clip arrays, rewinds the visplane and opening allocation
/// pointers, clears the per-row height cache, and recomputes the base
/// texture-step scales from the current `viewangle`.
///
/// Exported as `#[no_mangle]` for C callers.
///
/// # Safety
///
/// Writes to several `static mut` renderer globals and reads `viewwidth`,
/// `viewheight`, and `viewangle`.  Must be called exactly once per frame
/// before any BSP traversal begins.
#[no_mangle]
pub extern "C" fn R_ClearPlanes() {
    unsafe {
        let vw = viewwidth as usize;
        let vh = viewheight as c_short;

        for i in 0..vw {
            floorclip[i] = vh;
            ceilingclip[i] = -1;
        }

        lastvisplane = std::ptr::addr_of_mut!(visplanes[0]);
        lastopening = std::ptr::addr_of_mut!(openings[0]);

        // Reset cache heights
        for h in std::slice::from_raw_parts_mut(std::ptr::addr_of_mut!(cachedheight[0]), 200) {
            *h = 0;
        }

        // Left to right mapping
        let angle = (viewangle.wrapping_sub(ANG90)) >> ANGLETOFINESHIFT;
        basexscale = FixedDiv(finecosine(angle as usize), centerxfrac);
        baseyscale = 0i32.wrapping_sub(FixedDiv(
            tables::finesine[angle as usize & FINEMASK as usize],
            centerxfrac,
        ));
    }
}

// ---------------------------------------------------------------------------
// R_FindPlane — find or create a visplane matching the given parameters
// ---------------------------------------------------------------------------

/// Returns a visplane for the given `height`, `picnum`, and `lightlevel`.
///
/// Searches the already-allocated visplanes for an exact match.  If none is
/// found, claims the next free slot from [`visplanes`] and initialises it.
/// All sky flats (`picnum == skyflatnum`) share a single visplane at height 0
/// and light level 0.
///
/// Aborts via [`crate::i_error!`] if the visplane pool (128 entries) is exhausted
/// (matches the C source `I_Error` path).
///
/// Exported as `#[no_mangle]` for C callers.
///
/// # Safety
///
/// Reads and writes the `static mut` globals [`visplanes`], [`lastvisplane`],
/// and [`r_sky::skyflatnum`].  The returned pointer is valid for the lifetime
/// of the current frame (until the next [`R_ClearPlanes`] call).
#[no_mangle]
pub extern "C" fn R_FindPlane(
    height: fixed_t,
    picnum: c_int,
    lightlevel: c_int,
) -> *mut visplane_t {
    unsafe {
        let mut h = height;
        let mut ll = lightlevel;

        // All sky planes map together
        if picnum == r_sky::skyflatnum {
            h = 0;
            ll = 0;
        }

        // Search existing visplanes
        let mut check = std::ptr::addr_of_mut!(visplanes[0]);
        let end = lastvisplane;

        while check < end {
            if h == (*check).height && picnum == (*check).picnum && ll == (*check).lightlevel {
                return check;
            }
            check = check.add(1);
        }

        // Need a new visplane
        if (lastvisplane as usize - std::ptr::addr_of!(visplanes[0]) as usize)
            / std::mem::size_of::<visplane_t>()
            >= MAXVISPLANES
        {
            // Match C doomgeneric: abort rather than hand callers a null
            // visplane pointer (they dereference unconditionally).
            i_error!("R_FindPlane: no more visplanes");
        }

        let new_vp = lastvisplane;
        lastvisplane = lastvisplane.add(1);

        (*new_vp).height = h;
        (*new_vp).picnum = picnum;
        (*new_vp).lightlevel = ll;
        (*new_vp).minx = SCREENWIDTH as c_int;
        (*new_vp).maxx = -1;

        // memset top to 0xFF
        for t in (*new_vp).top.iter_mut() {
            *t = 0xFF;
        }

        new_vp
    }
}

// ---------------------------------------------------------------------------
// R_CheckPlane — check if a visplane can cover [start..stop] without overlap
// ---------------------------------------------------------------------------

/// Ensures the visplane `pl` can accommodate the column range `[start, stop]`.
///
/// If the range does not overlap any already-filled column in `pl`, the
/// plane's `minx`/`maxx` bounds are extended to cover the union and `pl` is
/// returned unchanged.  Otherwise a new visplane with the same flat/height/
/// lightlevel is allocated for `[start, stop]` and returned.
///
/// Exported as `#[no_mangle]` for C callers.
///
/// # Safety
///
/// Reads and writes the `static mut` globals [`lastvisplane`] and the
/// [`visplane_t`] pointed to by `pl`.  `pl` must be a non-null pointer to a
/// valid, frame-lived visplane obtained from [`R_FindPlane`].
#[no_mangle]
pub extern "C" fn R_CheckPlane(pl: *mut visplane_t, start: c_int, stop: c_int) -> *mut visplane_t {
    unsafe {
        let intrl = if start < (*pl).minx {
            (*pl).minx
        } else {
            start
        };
        let unionl = if start < (*pl).minx {
            start
        } else {
            (*pl).minx
        };

        let intrh = if stop > (*pl).maxx { (*pl).maxx } else { stop };
        let unionh = if stop > (*pl).maxx { stop } else { (*pl).maxx };

        // Check for overlap in the intersection range
        let mut x = intrl;
        while x <= intrh {
            if (*pl).top[x as usize] != 0xFF {
                break;
            }
            x += 1;
        }

        if x > intrh {
            // No overlap — extend the visplane
            (*pl).minx = unionl;
            (*pl).maxx = unionh;
            return pl;
        }

        // Make a new visplane
        let new_vp = lastvisplane;
        lastvisplane = lastvisplane.add(1);

        (*new_vp).height = (*pl).height;
        (*new_vp).picnum = (*pl).picnum;
        (*new_vp).lightlevel = (*pl).lightlevel;
        (*new_vp).minx = start;
        (*new_vp).maxx = stop;

        for t in (*new_vp).top.iter_mut() {
            *t = 0xFF;
        }

        new_vp
    }
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
/// start recorded in [`spanstart`].
///
/// Exported as `#[no_mangle]` for C callers.
///
/// # Safety
///
/// Reads and writes the `static mut` globals [`spanstart`].  Calls
/// [`R_MapPlane`], which itself writes additional globals.  All indices
/// derived from `t1`/`b1`/`t2`/`b2` must be valid screen rows
/// (`0 <= row < SCREENHEIGHT`); the caller (the `R_DrawPlanes` loop) is
/// responsible for keeping them in range.
#[no_mangle]
pub extern "C" fn R_MakeSpans(x: c_int, t1: c_int, b1: c_int, t2: c_int, b2: c_int) {
    unsafe {
        let mut t1 = t1;
        let mut b1 = b1;
        let mut t2 = t2;
        let mut b2 = b2;

        while t1 < t2 && t1 <= b1 {
            R_MapPlane(t1, spanstart[t1 as usize], x - 1);
            t1 += 1;
        }
        while b1 > b2 && b1 >= t1 {
            R_MapPlane(b1, spanstart[b1 as usize], x - 1);
            b1 -= 1;
        }

        while t2 < t1 && t2 <= b2 {
            spanstart[t2 as usize] = x;
            t2 += 1;
        }
        while b2 > b1 && b2 >= t2 {
            spanstart[b2 as usize] = x;
            b2 -= 1;
        }
    }
}

// ---------------------------------------------------------------------------
// R_DrawPlanes — draw all visplanes at end of frame
// ---------------------------------------------------------------------------

use crate::doom::z_zone::PU_STATIC;

/// Rasterizes all accumulated visplanes into horizontal pixel spans.
///
/// Called once per frame after BSP traversal is complete.  Iterates over every
/// visplane in the pool up to [`lastvisplane`].  Sky flats are drawn as
/// vertical columns via [`colfunc`]; regular flats are drawn as horizontal
/// spans via [`R_MakeSpans`] / [`R_MapPlane`] / [`spanfunc`].
///
/// Exported as `#[no_mangle]` for C callers.
///
/// # Safety
///
/// Reads and writes numerous `static mut` renderer globals.  All function
/// pointers (`colfunc`, `spanfunc`) and data pointers (`fixedcolormap`,
/// `planezlight`, `zlight`) must be valid.  The `W_CacheLumpNum` /
/// `W_ReleaseLumpNum` calls must have access to a properly initialised WAD
/// lump directory.
#[no_mangle]
pub extern "C" fn R_DrawPlanes() {
    unsafe {
        let mut pl = std::ptr::addr_of_mut!(visplanes[0]);
        let end = lastvisplane;

        while pl < end {
            if (*pl).minx > (*pl).maxx {
                pl = pl.add(1);
                continue;
            }

            // Sky flat
            if (*pl).picnum == r_sky::skyflatnum {
                dc_iscale = pspriteiscale >> detailshift;
                // Sky is always drawn full bright,
                // i.e. colormaps[0] is used.
                // Because of this hack, sky is not affected
                // by INVUL inverse mapping.
                dc_colormap = colormaps;
                dc_texturemid = skytexturemid;

                for x in (*pl).minx..=(*pl).maxx {
                    dc_yl = (*pl).top[x as usize] as c_int;
                    dc_yh = (*pl).bottom[x as usize] as c_int;

                    if dc_yl <= dc_yh {
                        let angle =
                            viewangle.wrapping_add(xtoviewangle[x as usize]) >> ANGLETOSKYSHIFT;
                        dc_x = x;
                        dc_source = R_GetColumn(skytexture, angle as c_int);
                        if let Some(func) = colfunc {
                            func();
                        }
                    }
                }
                pl = pl.add(1);
                continue;
            }

            // Regular flat
            let flat_idx = *flattranslation.offset((*pl).picnum as isize);
            let lumpnum = firstflat + flat_idx;
            ds_source = W_CacheLumpNum(lumpnum, PU_STATIC) as *mut u8;

            planeheight = ((*pl).height.wrapping_sub(viewz)).abs();
            let mut light = ((*pl).lightlevel >> LIGHTSEGSHIFT) as c_int + extralight;

            if light >= LIGHTLEVELS as c_int {
                light = LIGHTLEVELS as c_int - 1;
            }
            if light < 0 {
                light = 0;
            }

            // Set planezlight — in the full implementation this would be
            // a pointer into zlight[light]
            let light_usize = light as usize;
            if light_usize < LIGHTLEVELS {
                planezlight = zlight[light_usize].as_ptr() as *const *const lighttable_t;
            }

            // Set sentinel values at the visplane boundaries. The C code does
            // pl->top[pl->maxx+1] = 0xff and pl->top[pl->minx-1] = 0xff, which
            // relies on padding bytes when at screen edges.
            if (*pl).minx > 0 {
                (*pl).top[((*pl).minx - 1) as usize] = 0xFF;
            } else {
                (*pl).pad1 = 0xFF;
            }
            if (*pl).maxx < SCREENWIDTH as c_int - 1 {
                (*pl).top[((*pl).maxx + 1) as usize] = 0xFF;
            } else {
                (*pl).pad2 = 0xFF;
            }

            let stop = (*pl).maxx + 1;
            for x in (*pl).minx..=stop {
                let prev_x = x - 1;
                let t_top = if prev_x < 0 {
                    (*pl).pad1
                } else {
                    (*pl).top[prev_x as usize]
                };
                let t_bottom = if prev_x < 0 {
                    (*pl).pad3
                } else {
                    (*pl).bottom[prev_x as usize]
                };
                let b_top = if x == SCREENWIDTH as c_int {
                    (*pl).pad2
                } else {
                    (*pl).top[x as usize]
                };
                let b_bottom = if x == SCREENWIDTH as c_int {
                    (*pl).pad4
                } else {
                    (*pl).bottom[x as usize]
                };
                R_MakeSpans(
                    x,
                    t_top as c_int,
                    t_bottom as c_int,
                    b_top as c_int,
                    b_bottom as c_int,
                );
            }

            W_ReleaseLumpNum(lumpnum);
            pl = pl.add(1);
        }
    }
}
