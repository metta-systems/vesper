//! Wall-segment rasterizer.
//!
//! Rust port of `vendor/doomgeneric/r_segs.c`.
//!
//! # Overview
//!
//! Each BSP leaf that the traversal visits submits one wall segment to
//! [`R_StoreWallRange`], which:
//!
//! 1. Computes the perpendicular distance from the viewpoint to the seg and
//!    derives scale values at both screen endpoints.
//! 2. Decides which wall textures (upper, middle, lower) are visible.
//! 3. Calls [`R_CheckPlane`] to extend the current floor/ceiling visplanes.
//! 4. Runs `R_RenderSegLoop` to draw textured columns and update the
//!    per-column clip arrays for later sprite rendering.
//! 5. Saves sprite-clipping info into the `openings` scratch buffer.
//!
//! Transparent mid-textures (fences, windows) on two-sided lines are deferred
//! and drawn by `R_RenderMaskedSegRange` after all opaque geometry is done.
//!
//! # Coordinate system
//!
//! `fixed_t = i32`, 16.16 fixed point.  `angle_t = u32`, full circle =
//! `0x1_0000_0000`.  "Scale" in this module means the reciprocal distance from
//! the view plane to a wall column (larger = closer).

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::{ffi::c_void, ptr};

use crate::engine::{
    math::{
        fixed::{angle_t, fixed_t, FixedMul, FRACBITS},
        tables::{self, ANG180, ANG90, ANGLETOFINESHIFT},
    },
    render_3d::bsp::drawseg_t,
    wad::format::LinedefFlag,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Number of fractional bits used for the pixel-height accumulator.
///
/// The top/bottom screen-y values are maintained as 12.20 fixed-point
/// values internally; `>> HEIGHTBITS` converts them to integer screen rows.
const HEIGHTBITS: u32 = 12;

/// One unit in the pixel-height accumulator (`1 << HEIGHTBITS`).
///
/// Added to `topfrac` before the right-shift to achieve ceiling (upward)
/// rounding when computing `yl`.
const HEIGHTUNIT: i32 = 1 << HEIGHTBITS;

/// Silhouette flag: this drawseg occludes sprites below its bottom edge.
const SIL_BOTTOM: i32 = 1;

/// Silhouette flag: this drawseg occludes sprites above its top edge.
const SIL_TOP: i32 = 2;

/// Silhouette flag: this drawseg occludes sprites on both sides.
const SIL_BOTH: i32 = 3;

/// Number of distinct light levels used by the colormap tables.
const LIGHTLEVELS: usize = 16;

/// Shift applied to a sector's `lightlevel` to derive a scale-light table index.
const LIGHTSEGSHIFT: u32 = 4;

/// Number of scale-based light entries per light level in `scalelight`.
const MAXLIGHTSCALE: usize = 48;

/// Shift applied to a column scale value to derive an index into `scalelight[n]`.
const LIGHTSCALESHIFT: u32 = 12;

/// Mask that wraps a fine angle into `finetangent` (4096 entries, half a
/// turn). Tangent repeats every half turn, so the wrapped index is the
/// correct tangent; vanilla instead reads past the table into `finesine`
/// for a wall seen edge-on at the screen edge.
const FINETANGENTMASK: usize = 0x0FFF;

/// Screen width in pixels, re-exported from [`i_video`] for local use.
const SCREENWIDTH: usize = crate::engine::interface::video::SCREENWIDTH as usize;

/// Maximum number of drawsegs that can be stored per frame.
const MAXDRAWSEGS: usize = 256;

// ---------------------------------------------------------------------------
// Externs from other modules
// ---------------------------------------------------------------------------

use crate::engine::render_3d::{
    bsp::{backsector, curline, drawsegs, ds_p, frontsector, linedef, sidedef},
    data::{textureheight, texturetranslation, R_GetColumn},
    draw::{dc_colormap, dc_iscale, dc_source, dc_texturemid, dc_x, dc_yh, dc_yl, viewheight},
    main::{
        centeryfrac, colfunc, extralight, fixedcolormap, scalelight, viewangle, viewz,
        xtoviewangle, R_PointToDist, R_ScaleFromGlobalAngle,
    },
    plane::{ceilingclip, ceilingplane, floorclip, floorplane, lastopening, R_CheckPlane},
    sky::skyflatnum,
    things::{negonearray, screenheightarray, R_DrawMaskedColumn},
};

// ---------------------------------------------------------------------------
// Globals defined by this module
// ---------------------------------------------------------------------------

/// Non-zero when the current seg has at least one visible texture.
///
///Set by [`R_StoreWallRange`]
/// as the OR of `midtexture | toptexture | bottomtexture | maskedtexture`.
/// Controls whether `R_RenderSegLoop` computes texture-U and lighting.
pub static mut segtextured: i32 = 0;

/// Non-zero when the floor plane must be extended for this seg.
///
///Set by [`R_StoreWallRange`].
/// A floor mark is needed when the back sector has a different floor flat,
/// height, or light level, or when the seg is single-sided.
pub static mut markfloor: i32 = 0;

/// Non-zero when the ceiling plane must be extended for this seg.
///
///Set by [`R_StoreWallRange`].
/// Analogous to [`markfloor`] for the ceiling.
pub static mut markceiling: i32 = 0;

/// Non-zero when the seg has a transparent mid-texture on a two-sided line.
///
///When set, [`R_StoreWallRange`]
/// reserves a column slice in the `openings` scratch buffer and
/// [`R_RenderMaskedSegRange`] later draws it after all opaque geometry.
pub static mut maskedtexture: i32 = 0;

/// Translated texture number for the upper wall texture (0 = none).
///
///Set by [`R_StoreWallRange`]
/// from `sidedef->toptexture` via `texturetranslation`.
pub static mut toptexture: i32 = 0;

/// Translated texture number for the lower wall texture (0 = none).
///
///Set by [`R_StoreWallRange`]
/// from `sidedef->bottomtexture` via `texturetranslation`.
pub static mut bottomtexture: i32 = 0;

/// Translated texture number for the middle wall texture (0 = none).
///
///Non-zero only for single-sided
/// lines (or masked mid-textures, tracked separately via [`maskedtexture`]).
pub static mut midtexture: i32 = 0;

/// Normal angle of the current seg's linedef (perpendicular to the wall).
///
///Computed as
/// `curline->angle + ANG90` by [`R_StoreWallRange`].
pub static mut rw_normalangle: angle_t = 0;

/// Angle from the viewpoint to the left endpoint of the current seg.
///
///Written by the BSP clipper
/// (`R_ClipPassWallSegment` / `R_ClipSolidWallSegment` in `r_bsp`) before
/// calling [`R_StoreWallRange`].
pub static mut rw_angle1: angle_t = 0;

/// Screen column where rendering of the current seg starts (inclusive).
///
///Also used as the loop variable
/// inside `R_RenderSegLoop`.
pub static mut rw_x: i32 = 0;

/// Screen column where rendering of the current seg stops (exclusive).
///
///Set to `stop + 1` in
/// [`R_StoreWallRange`].
pub static mut rw_stopx: i32 = 0;

/// Angle used to compute the texture-U coordinate for each column.
///
///Set to
/// `ANG90 + viewangle - rw_normalangle` by [`R_StoreWallRange`].
pub static mut rw_centerangle: angle_t = 0;

/// Texture horizontal offset for the current seg (fixed-point pixels).
///
///Combines the linedef's
/// `textureoffset`, the seg's own `offset`, and a view-angle correction.
pub static mut rw_offset: fixed_t = 0;

/// Perpendicular distance from the viewpoint to the wall (fixed-point).
///
///Used with `finesine` to derive
/// the scale at each column.
pub static mut rw_distance: fixed_t = 0;

/// Projection scale at the current column.
///
///Larger values mean the wall is
/// closer.  Stepped by [`rw_scalestep`] across the seg.
pub static mut rw_scale: fixed_t = 0;

/// Per-column increment for [`rw_scale`].
///
///Computed as
/// `(scale2 - scale1) / (stop - start)`.
pub static mut rw_scalestep: fixed_t = 0;

/// Texture vertical midpoint for the middle wall texture (fixed-point).
///
///Controls where the texture
/// origin sits relative to the column; affected by `DONTPEGBOTTOM`.
pub static mut rw_midtexturemid: fixed_t = 0;

/// Texture vertical midpoint for the upper wall texture (fixed-point).
///
///Affected by `DONTPEGTOP`.
pub static mut rw_toptexturemid: fixed_t = 0;

/// Texture vertical midpoint for the lower wall texture (fixed-point).
///
///Affected by `DONTPEGBOTTOM`.
pub static mut rw_bottomtexturemid: fixed_t = 0;

/// Front sector ceiling height minus `viewz`, in world units (not fixed-point).
///
///Used as the top boundary for
/// wall-texture rendering.  Shifted right by 4 before the seg loop.
pub static mut worldtop: i32 = 0;

/// Front sector floor height minus `viewz`, in world units (not fixed-point).
///
///Used as the bottom boundary for
/// wall-texture rendering.  Shifted right by 4 before the seg loop.
pub static mut worldbottom: i32 = 0;

/// Back sector ceiling height minus `viewz`, in world units (not fixed-point).
///
///Meaningful only for two-sided
/// lines; used to determine if an upper texture is needed.
pub static mut worldhigh: i32 = 0;

/// Back sector floor height minus `viewz`, in world units (not fixed-point).
///
///Meaningful only for two-sided
/// lines; used to determine if a lower texture is needed.
pub static mut worldlow: i32 = 0;

/// Screen-y of the top of the upper texture at the current column (scaled).
///
///Stepped by [`pixhighstep`]
/// each column in `R_RenderSegLoop`.
pub static mut pixhigh: fixed_t = 0;

/// Screen-y of the bottom of the lower texture at the current column (scaled).
///
///Stepped by [`pixlowstep`] each
/// column in `R_RenderSegLoop`.
pub static mut pixlow: fixed_t = 0;

/// Per-column step for [`pixhigh`].
pub static mut pixhighstep: fixed_t = 0;

/// Per-column step for [`pixlow`].
pub static mut pixlowstep: fixed_t = 0;

/// Fractional screen-y of the top of the wall at the current column.
///
///Maintained in 12.20 format;
/// `>> HEIGHTBITS` yields the integer screen row.
pub static mut topfrac: fixed_t = 0;

/// Per-column step for [`topfrac`].
pub static mut topstep: fixed_t = 0;

/// Fractional screen-y of the bottom of the wall at the current column.
///
///Maintained in 12.20 format;
/// `>> HEIGHTBITS` yields the integer screen row.
pub static mut bottomfrac: fixed_t = 0;

/// Per-column step for [`bottomfrac`].
pub static mut bottomstep: fixed_t = 0;

/// Pointer to the scale-based light table row for the current seg.
///
///Points into
/// `scalelight[lightnum]`; indexed by `rw_scale >> LIGHTSCALESHIFT` to pick
/// the per-column colormap.
pub static mut walllights: *mut *mut u8 = ptr::null_mut();

/// Per-column texture-U coordinate buffer for masked mid-textures.
///
///Points into the `openings`
/// scratch buffer (in `r_plane`); written by `R_RenderSegLoop` and consumed by
/// [`R_RenderMaskedSegRange`].  A value of `i16::MAX` (SHRT_MAX) means the
/// column has already been drawn or is not present.
pub static mut maskedtexturecol: *mut i16 = ptr::null_mut();

// ---------------------------------------------------------------------------
// R_RenderMaskedSegRange
// ---------------------------------------------------------------------------

/// Draws the transparent mid-texture for a two-sided seg.
///
/// Called during the sprite-rendering pass (after all opaque walls are drawn)
/// for each [`drawseg_t`] that has a masked mid-texture.  Iterates columns
/// `[x1, x2]`; for each column where [`maskedtexturecol`] is not `SHRT_MAX`,
/// fetches the correct texture column, sets up lighting, and calls
/// [`R_DrawMaskedColumn`].
///
/// # Safety
///
/// `ds` must be a valid, non-null pointer to a [`drawseg_t`] that was filled
/// in by [`R_StoreWallRange`] during the current frame.  All pointer fields
/// within `ds` (`curline`, `maskedtexturecol`, `sprbottomclip`,
/// `sprtopclip`) must still be valid.  Reads and writes numerous
/// `static mut` renderer globals.
pub unsafe fn R_RenderMaskedSegRange(ds: *mut drawseg_t, x1: i32, x2: i32) {
    let texnum: i32 = *texturetranslation.add((*(*(*ds).curline).sidedef).midtexture as usize);

    curline = (*ds).curline;
    frontsector = (*curline).frontsector;
    backsector = (*curline).backsector;

    let mut lightnum: i32 =
        (((*frontsector).lightlevel as u32) >> LIGHTSEGSHIFT) as i32 + extralight;

    if (*(*curline).v1).y == (*(*curline).v2).y {
        lightnum -= 1;
    } else if (*(*curline).v1).x == (*(*curline).v2).x {
        lightnum += 1;
    }

    if lightnum < 0 {
        walllights = scalelight[0].as_mut_ptr();
    } else if lightnum >= LIGHTLEVELS as i32 {
        walllights = scalelight[LIGHTLEVELS - 1].as_mut_ptr();
    } else {
        walllights = scalelight[lightnum as usize].as_mut_ptr();
    }

    maskedtexturecol = (*ds).maskedtexturecol;
    rw_scalestep = (*ds).scalestep;
    crate::engine::render_3d::things::spryscale = (*ds).scale1 + (x1 - (*ds).x1) * rw_scalestep;
    crate::engine::render_3d::things::mfloorclip = (*ds).sprbottomclip;
    crate::engine::render_3d::things::mceilingclip = (*ds).sprtopclip;

    if (*(*curline).linedef).flags & (LinedefFlag::DONTPEGBOTTOM as i16) != 0 {
        dc_texturemid = if (*frontsector).floorheight > (*backsector).floorheight {
            (*frontsector).floorheight
        } else {
            (*backsector).floorheight
        };
        dc_texturemid = dc_texturemid + *textureheight.add(texnum as usize) - viewz;
    } else {
        dc_texturemid = if (*frontsector).ceilingheight < (*backsector).ceilingheight {
            (*frontsector).ceilingheight
        } else {
            (*backsector).ceilingheight
        };
        dc_texturemid -= viewz;
    }
    dc_texturemid += (*(*curline).sidedef).rowoffset;

    if !fixedcolormap.is_null() {
        dc_colormap = fixedcolormap;
    }

    dc_x = x1;
    while dc_x <= x2 {
        if *maskedtexturecol.add(dc_x as usize) != i16::MAX {
            if fixedcolormap.is_null() {
                let mut index =
                    (crate::engine::render_3d::things::spryscale as u32) >> LIGHTSCALESHIFT;
                if index >= MAXLIGHTSCALE as u32 {
                    index = (MAXLIGHTSCALE - 1) as u32;
                }
                dc_colormap = *walllights.add(index as usize);
            }

            crate::engine::render_3d::things::sprtopscreen =
                centeryfrac - FixedMul(dc_texturemid, crate::engine::render_3d::things::spryscale);
            dc_iscale =
                (0xffffffff_u32 / (crate::engine::render_3d::things::spryscale as u32)) as i32;

            let raw = R_GetColumn(texnum, *maskedtexturecol.add(dc_x as usize) as i32);
            let col = raw.sub(3);
            R_DrawMaskedColumn(col as *mut c_void);

            *maskedtexturecol.add(dc_x as usize) = i16::MAX;
        }
        crate::engine::render_3d::things::spryscale += rw_scalestep;
        dc_x += 1;
    }
}

// ---------------------------------------------------------------------------
// R_RenderSegLoop
// ---------------------------------------------------------------------------

/// Core per-column drawing loop for one wall segment.
///
/// Iterates from [`rw_x`] to [`rw_stopx`] (exclusive), performing the
/// following work for each column:
///
/// 1. Computes `yl` (top of wall) and `yh` (bottom of wall) from [`topfrac`]
///    and [`bottomfrac`], clamped to the current clip boundaries.
/// 2. If [`markceiling`] is set, records the ceiling span extent into
///    [`ceilingplane`].
/// 3. If [`markfloor`] is set, records the floor span extent into
///    [`floorplane`].
/// 4. If [`segtextured`] is set, computes the texture-U column and the
///    per-column colormap index.
/// 5. Draws the middle, upper, or lower texture tier as appropriate, and
///    updates [`ceilingclip`] / [`floorclip`].
/// 6. Records the masked texture column offset if [`maskedtexture`] is set.
///
/// # Safety
///
/// Reads and writes numerous `static mut` renderer globals.  All pointer
/// globals (`ceilingplane`, `floorplane`, `walllights`, `maskedtexturecol`,
/// `colfunc`) must be valid for the current frame.
unsafe fn R_RenderSegLoop() {
    while rw_x < rw_stopx {
        let clip_ceil = ceilingclip[rw_x as usize] as i32;
        let clip_floor = floorclip[rw_x as usize] as i32;

        let mut yl = (topfrac + HEIGHTUNIT - 1) >> HEIGHTBITS;
        if yl < clip_ceil + 1 {
            yl = clip_ceil + 1;
        }

        if markceiling != 0 {
            let top = clip_ceil + 1;
            let mut bottom = yl - 1;
            if bottom >= clip_floor {
                bottom = clip_floor - 1;
            }
            if top <= bottom {
                (*ceilingplane).top[rw_x as usize] = top as u8;
                (*ceilingplane).bottom[rw_x as usize] = bottom as u8;
            }
        }

        let mut yh = bottomfrac >> HEIGHTBITS;
        if yh >= clip_floor {
            yh = clip_floor - 1;
        }

        if markfloor != 0 {
            let mut top = yh + 1;
            let bottom = clip_floor - 1;
            if top <= clip_ceil {
                top = clip_ceil + 1;
            }
            if top <= bottom {
                (*floorplane).top[rw_x as usize] = top as u8;
                (*floorplane).bottom[rw_x as usize] = bottom as u8;
            }
        }

        let texturecolumn: fixed_t;
        if segtextured != 0 {
            let angle =
                (rw_centerangle.wrapping_add(xtoviewangle[rw_x as usize])) >> ANGLETOFINESHIFT;
            texturecolumn = rw_offset
                - FixedMul(
                    tables::finetangent[angle as usize & FINETANGENTMASK],
                    rw_distance,
                );

            let mut index = (rw_scale as u32) >> LIGHTSCALESHIFT;
            if index >= MAXLIGHTSCALE as u32 {
                index = (MAXLIGHTSCALE - 1) as u32;
            }
            dc_colormap = *walllights.add(index as usize);
            dc_x = rw_x;
            dc_iscale = (0xffffffff_u32 / (rw_scale as u32)) as i32;
        } else {
            texturecolumn = 0;
        }

        if midtexture != 0 {
            dc_yl = yl;
            dc_yh = yh;
            dc_texturemid = rw_midtexturemid;
            dc_source = R_GetColumn(midtexture, texturecolumn >> FRACBITS);
            if let Some(func) = colfunc {
                func();
            }
            ceilingclip[rw_x as usize] = viewheight as i16;
            floorclip[rw_x as usize] = -1;
        } else {
            if toptexture != 0 {
                let mut mid = pixhigh >> HEIGHTBITS;
                pixhigh += pixhighstep;

                let clip_floor = floorclip[rw_x as usize] as i32;
                if mid >= clip_floor {
                    mid = clip_floor - 1;
                }

                if mid >= yl {
                    dc_yl = yl;
                    dc_yh = mid;
                    dc_texturemid = rw_toptexturemid;
                    dc_source = R_GetColumn(toptexture, texturecolumn >> FRACBITS);
                    if let Some(func) = colfunc {
                        func();
                    }
                    ceilingclip[rw_x as usize] = mid as i16;
                } else {
                    ceilingclip[rw_x as usize] = (yl - 1) as i16;
                }
            } else {
                if markceiling != 0 {
                    ceilingclip[rw_x as usize] = (yl - 1) as i16;
                }
            }

            if bottomtexture != 0 {
                let mut mid = (pixlow + HEIGHTUNIT - 1) >> HEIGHTBITS;
                pixlow += pixlowstep;

                let clip_ceil = ceilingclip[rw_x as usize] as i32;
                if mid <= clip_ceil {
                    mid = clip_ceil + 1;
                }

                if mid <= yh {
                    dc_yl = mid;
                    dc_yh = yh;
                    dc_texturemid = rw_bottomtexturemid;
                    dc_source = R_GetColumn(bottomtexture, texturecolumn >> FRACBITS);
                    if let Some(func) = colfunc {
                        func();
                    }
                    floorclip[rw_x as usize] = mid as i16;
                } else {
                    floorclip[rw_x as usize] = (yh + 1) as i16;
                }
            } else {
                if markfloor != 0 {
                    floorclip[rw_x as usize] = (yh + 1) as i16;
                }
            }

            if maskedtexture != 0 {
                *maskedtexturecol.add(rw_x as usize) = (texturecolumn >> FRACBITS) as i16;
            }
        }

        rw_scale += rw_scalestep;
        topfrac += topstep;
        bottomfrac += bottomstep;
        rw_x += 1;
    }
}

// ---------------------------------------------------------------------------
// R_StoreWallRange
// ---------------------------------------------------------------------------

/// Clips and draws the wall segment between screen columns `start` and `stop`.
///
/// This is the main entry point called by the BSP traversal for every visible
/// seg.  It performs all setup for the segment (distance, scale, texture
/// selection, floor/ceiling mark decisions) and then delegates pixel output to
/// `R_RenderSegLoop`.  After the loop it saves sprite-clipping arrays into
/// the `openings` scratch buffer and advances [`ds_p`] to the next free
/// [`drawseg_t`] slot.
///
/// A wall range is silently ignored if the [`drawseg_t`] pool
/// (`drawsegs[MAXDRAWSEGS]`) is full; the C source does the same.
///
/// # Safety
///
/// `curline`, `frontsector`, and (for two-sided lines) `backsector` must be
/// valid pointers set by the BSP traversal before this call.  `ds_p` must be
/// a valid pointer into `drawsegs` or one-past-end (`&drawsegs[MAXDRAWSEGS]`);
/// the function checks for the one-past-end case internally and returns
/// silently, so callers need not guard that boundary themselves.  Reads and
/// writes numerous `static mut` renderer globals; calls [`R_CheckPlane`] and
/// `R_RenderSegLoop`.
// FIXME: In R_StoreWallRange the Rust code computes `rw_scalestep` only when
//        `stop > start`, leaving `rw_scalestep` at its value from the
//        *previous* seg when `stop == start`.  The C source has the same
//        behaviour, so this is intentional, but it means `ds_p->scalestep`
//        may hold a stale value for single-column segs.
pub unsafe fn R_StoreWallRange(start: i32, stop: i32) {
    if ds_p == std::ptr::addr_of_mut!(drawsegs[0]).add(MAXDRAWSEGS) {
        return;
    }

    #[cfg(feature = "rangecheck")]
    {
        if start >= viewwidth || start > stop {
            // Would call I_Error — skip in Rust for now
            return;
        }
    }

    sidedef = (*curline).sidedef;
    linedef = (*curline).linedef;

    (*linedef).flags |= LinedefFlag::MAPPED as i16;

    rw_normalangle = (*curline).angle.wrapping_add(ANG90);
    let mut offsetangle = (rw_normalangle.wrapping_sub(rw_angle1) as i32).unsigned_abs();

    if offsetangle > ANG90 {
        offsetangle = ANG90;
    }

    let distangle = ANG90 - offsetangle;
    let hyp = R_PointToDist((*(*curline).v1).x, (*(*curline).v1).y);
    let sineval = tables::finesine[(distangle >> ANGLETOFINESHIFT) as usize];
    rw_distance = FixedMul(hyp, sineval);

    (*ds_p).x1 = start;
    rw_x = start;
    (*ds_p).x2 = stop;
    (*ds_p).curline = curline;
    rw_stopx = stop + 1;

    (*ds_p).scale1 = rw_scale;
    rw_scale = R_ScaleFromGlobalAngle(viewangle.wrapping_add(xtoviewangle[start as usize]));
    (*ds_p).scale1 = rw_scale;

    if stop > start {
        (*ds_p).scale2 =
            R_ScaleFromGlobalAngle(viewangle.wrapping_add(xtoviewangle[stop as usize]));
        (*ds_p).scalestep = rw_scalestep;
        rw_scalestep = ((*ds_p).scale2 - rw_scale) / (stop - start);
        (*ds_p).scalestep = rw_scalestep;
    } else {
        (*ds_p).scale2 = (*ds_p).scale1;
    }

    worldtop = (*frontsector).ceilingheight - viewz;
    worldbottom = (*frontsector).floorheight - viewz;

    midtexture = 0;
    toptexture = 0;
    bottomtexture = 0;
    maskedtexture = 0;
    (*ds_p).maskedtexturecol = ptr::null_mut();

    if backsector.is_null() {
        // single sided line
        midtexture = *texturetranslation.add((*sidedef).midtexture as usize);
        markfloor = 1;
        markceiling = 1;

        if (*linedef).flags & (LinedefFlag::DONTPEGBOTTOM as i16) != 0 {
            let vtop =
                (*frontsector).floorheight + *textureheight.add((*sidedef).midtexture as usize);
            rw_midtexturemid = vtop - viewz;
        } else {
            rw_midtexturemid = worldtop;
        }
        rw_midtexturemid += (*sidedef).rowoffset;

        (*ds_p).silhouette = SIL_BOTH;
        (*ds_p).sprtopclip = std::ptr::addr_of_mut!(screenheightarray[0]);
        (*ds_p).sprbottomclip = std::ptr::addr_of_mut!(negonearray[0]);
        (*ds_p).bsilheight = i32::MAX;
        (*ds_p).tsilheight = i32::MIN;
    } else {
        // two sided line
        (*ds_p).sprtopclip = ptr::null_mut();
        (*ds_p).sprbottomclip = ptr::null_mut();
        (*ds_p).silhouette = 0;

        if (*frontsector).floorheight > (*backsector).floorheight {
            (*ds_p).silhouette = SIL_BOTTOM;
            (*ds_p).bsilheight = (*frontsector).floorheight;
        } else if (*backsector).floorheight > viewz {
            (*ds_p).silhouette = SIL_BOTTOM;
            (*ds_p).bsilheight = i32::MAX;
        }

        if (*frontsector).ceilingheight < (*backsector).ceilingheight {
            (*ds_p).silhouette |= SIL_TOP;
            (*ds_p).tsilheight = (*frontsector).ceilingheight;
        } else if (*backsector).ceilingheight < viewz {
            (*ds_p).silhouette |= SIL_TOP;
            (*ds_p).tsilheight = i32::MIN;
        }

        if (*backsector).ceilingheight <= (*frontsector).floorheight {
            (*ds_p).sprbottomclip = std::ptr::addr_of_mut!(negonearray[0]);
            (*ds_p).bsilheight = i32::MAX;
            (*ds_p).silhouette |= SIL_BOTTOM;
        }

        if (*backsector).floorheight >= (*frontsector).ceilingheight {
            (*ds_p).sprtopclip = std::ptr::addr_of_mut!(screenheightarray[0]);
            (*ds_p).tsilheight = i32::MIN;
            (*ds_p).silhouette |= SIL_TOP;
        }

        worldhigh = (*backsector).ceilingheight - viewz;
        worldlow = (*backsector).floorheight - viewz;

        if (*frontsector).ceilingpic as i32 == skyflatnum
            && (*backsector).ceilingpic as i32 == skyflatnum
        {
            worldtop = worldhigh;
        }

        if worldlow != worldbottom
            || (*backsector).floorpic != (*frontsector).floorpic
            || (*backsector).lightlevel != (*frontsector).lightlevel
        {
            markfloor = 1;
        } else {
            markfloor = 0;
        }

        if worldhigh != worldtop
            || (*backsector).ceilingpic != (*frontsector).ceilingpic
            || (*backsector).lightlevel != (*frontsector).lightlevel
        {
            markceiling = 1;
        } else {
            markceiling = 0;
        }

        if (*backsector).ceilingheight <= (*frontsector).floorheight
            || (*backsector).floorheight >= (*frontsector).ceilingheight
        {
            markceiling = 1;
            markfloor = 1;
        }

        if worldhigh < worldtop {
            toptexture = *texturetranslation.add((*sidedef).toptexture as usize);
            if (*linedef).flags & (LinedefFlag::DONTPEGTOP as i16) != 0 {
                rw_toptexturemid = worldtop;
            } else {
                let vtop = (*backsector).ceilingheight
                    + *textureheight.add((*sidedef).toptexture as usize);
                rw_toptexturemid = vtop - viewz;
            }
        }

        if worldlow > worldbottom {
            bottomtexture = *texturetranslation.add((*sidedef).bottomtexture as usize);
            if (*linedef).flags & (LinedefFlag::DONTPEGBOTTOM as i16) != 0 {
                rw_bottomtexturemid = worldtop;
            } else {
                rw_bottomtexturemid = worldlow;
            }
        }

        rw_toptexturemid += (*sidedef).rowoffset;
        rw_bottomtexturemid += (*sidedef).rowoffset;

        if (*sidedef).midtexture != 0 {
            maskedtexture = 1;
            (*ds_p).maskedtexturecol = lastopening.sub(rw_x as usize);
            maskedtexturecol = (*ds_p).maskedtexturecol;
            lastopening = lastopening.add((rw_stopx - rw_x) as usize);
        }
    }

    segtextured = midtexture | toptexture | bottomtexture | maskedtexture;

    if segtextured != 0 {
        let mut offsetangle = rw_normalangle.wrapping_sub(rw_angle1);
        if offsetangle > ANG180 {
            offsetangle = offsetangle.wrapping_neg();
        }
        if offsetangle > ANG90 {
            offsetangle = ANG90;
        }

        let sineval = tables::finesine[(offsetangle >> ANGLETOFINESHIFT) as usize];
        rw_offset = FixedMul(hyp, sineval);

        if rw_normalangle.wrapping_sub(rw_angle1) < ANG180 {
            rw_offset = -rw_offset;
        }

        rw_offset += (*sidedef).textureoffset + (*curline).offset;
        rw_centerangle = ANG90.wrapping_add(viewangle).wrapping_sub(rw_normalangle);

        if fixedcolormap.is_null() {
            let mut lightnum =
                (((*frontsector).lightlevel as u32) >> LIGHTSEGSHIFT) as i32 + extralight;

            if (*(*curline).v1).y == (*(*curline).v2).y {
                lightnum -= 1;
            } else if (*(*curline).v1).x == (*(*curline).v2).x {
                lightnum += 1;
            }

            if lightnum < 0 {
                walllights = scalelight[0].as_mut_ptr();
            } else if lightnum >= LIGHTLEVELS as i32 {
                walllights = scalelight[LIGHTLEVELS - 1].as_mut_ptr();
            } else {
                walllights = scalelight[lightnum as usize].as_mut_ptr();
            }
        }
    }

    if (*frontsector).floorheight >= viewz {
        markfloor = 0;
    }

    if (*frontsector).ceilingheight <= viewz && (*frontsector).ceilingpic as i32 != skyflatnum {
        markceiling = 0;
    }

    worldtop >>= 4;
    worldbottom >>= 4;

    topstep = -FixedMul(rw_scalestep, worldtop);
    topfrac = (centeryfrac >> 4) - FixedMul(worldtop, rw_scale);

    bottomstep = -FixedMul(rw_scalestep, worldbottom);
    bottomfrac = (centeryfrac >> 4) - FixedMul(worldbottom, rw_scale);

    if !backsector.is_null() {
        worldhigh >>= 4;
        worldlow >>= 4;

        if worldhigh < worldtop {
            pixhigh = (centeryfrac >> 4) - FixedMul(worldhigh, rw_scale);
            pixhighstep = -FixedMul(rw_scalestep, worldhigh);
        }

        if worldlow > worldbottom {
            pixlow = (centeryfrac >> 4) - FixedMul(worldlow, rw_scale);
            pixlowstep = -FixedMul(rw_scalestep, worldlow);
        }
    }

    if markceiling != 0 {
        ceilingplane = R_CheckPlane(ceilingplane, rw_x, rw_stopx - 1);
    }
    if markfloor != 0 {
        floorplane = R_CheckPlane(floorplane, rw_x, rw_stopx - 1);
    }

    R_RenderSegLoop();

    if (((*ds_p).silhouette & SIL_TOP) != 0 || maskedtexture != 0) && (*ds_p).sprtopclip.is_null() {
        for i in 0..(rw_stopx - start) {
            *lastopening.add(i as usize) = ceilingclip[(start + i) as usize];
        }
        (*ds_p).sprtopclip = lastopening.sub(start as usize);
        lastopening = lastopening.add((rw_stopx - start) as usize);
    }

    if (((*ds_p).silhouette & SIL_BOTTOM) != 0 || maskedtexture != 0)
        && (*ds_p).sprbottomclip.is_null()
    {
        for i in 0..(rw_stopx - start) {
            *lastopening.add(i as usize) = floorclip[(start + i) as usize];
        }
        (*ds_p).sprbottomclip = lastopening.sub(start as usize);
        lastopening = lastopening.add((rw_stopx - start) as usize);
    }

    if maskedtexture != 0 && ((*ds_p).silhouette & SIL_TOP) == 0 {
        (*ds_p).silhouette |= SIL_TOP;
        (*ds_p).tsilheight = i32::MIN;
    }
    if maskedtexture != 0 && ((*ds_p).silhouette & SIL_BOTTOM) == 0 {
        (*ds_p).silhouette |= SIL_BOTTOM;
        (*ds_p).bsilheight = i32::MAX;
    }

    ds_p = ds_p.add(1);
}
