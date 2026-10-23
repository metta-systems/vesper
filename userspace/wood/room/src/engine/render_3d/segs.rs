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
//! The C `rw_*` wall globals live in [`WallState`], held in [`WALL`].  Both
//! entry points borrow the renderer state once per wall and pass it down, so
//! the per-column loops take no borrows.
//!
//! # Coordinate system
//!
//! `fixed_t = i32`, 16.16 fixed point.  `angle_t = u32`, full circle =
//! `0x1_0000_0000`.  "Scale" in this module means the reciprocal distance from
//! the view plane to a wall column (larger = closer).

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::{ffi::c_void, ptr};

use crate::engine::{
    global::Global,
    map::{SectorId, SegId},
    math::{
        fixed::{FRACBITS, FixedMul, angle_t, fixed_t},
        tables::{self, ANG90, ANG180, ANGLETOFINESHIFT},
    },
    play::setup::{LEVEL, LevelState},
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

/// Maximum number of drawsegs that can be stored per frame.
const MAXDRAWSEGS: usize = 256;

// ---------------------------------------------------------------------------
// Externs from other modules
// ---------------------------------------------------------------------------

use crate::engine::render_3d::{
    bsp::BSP,
    data::{R_GetColumn, RENDER_DATA, RenderDataState},
    draw::{ColumnParams, DRAW, DrawState},
    main::{R_PointToDist, R_ScaleFromGlobalAngle, VIEW, ViewState, scalelight_row},
    plane::{PLANES, PlaneState, R_CheckPlane},
    sky::SKY,
    things::{MaskedColumnParams, R_DrawMaskedColumn, negonearray_ptr, screenheightarray_ptr},
};

// ---------------------------------------------------------------------------
// State defined by this module
// ---------------------------------------------------------------------------

/// Wall-segment state (C `r_segs.c` globals).
pub struct WallState {
    /// Non-zero when the current seg has at least one visible texture.
    ///
    /// Set by [`R_StoreWallRange`] as the OR of
    /// `midtexture | toptexture | bottomtexture | maskedtexture`.
    /// Controls whether `R_RenderSegLoop` computes texture-U and lighting.
    pub segtextured: i32,
    /// Non-zero when the floor plane must be extended for this seg.
    ///
    /// Set by [`R_StoreWallRange`].  A floor mark is needed when the back
    /// sector has a different floor flat, height, or light level, or when the
    /// seg is single-sided.
    pub markfloor: i32,
    /// Non-zero when the ceiling plane must be extended for this seg.
    ///
    /// Set by [`R_StoreWallRange`].  Analogous to `markfloor` for the ceiling.
    pub markceiling: i32,
    /// Non-zero when the seg has a transparent mid-texture on a two-sided line.
    ///
    /// When set, [`R_StoreWallRange`] reserves a column slice in the `openings`
    /// scratch buffer and [`R_RenderMaskedSegRange`] later draws it after all
    /// opaque geometry.
    pub maskedtexture: i32,
    /// Translated texture number for the upper wall texture (0 = none).
    ///
    /// Set by [`R_StoreWallRange`] from `sidedef->toptexture` via
    /// `texturetranslation`.
    pub toptexture: i32,
    /// Translated texture number for the lower wall texture (0 = none).
    ///
    /// Set by [`R_StoreWallRange`] from `sidedef->bottomtexture` via
    /// `texturetranslation`.
    pub bottomtexture: i32,
    /// Translated texture number for the middle wall texture (0 = none).
    ///
    /// Non-zero only for single-sided lines (or masked mid-textures, tracked
    /// separately via `maskedtexture`).
    pub midtexture: i32,
    /// Normal angle of the current seg's linedef (perpendicular to the wall).
    ///
    /// Computed as `curline->angle + ANG90` by [`R_StoreWallRange`].
    pub rw_normalangle: angle_t,
    /// Angle from the viewpoint to the left endpoint of the current seg.
    ///
    /// Written by the BSP clipper (`R_AddLine` in `r_bsp`) before calling
    /// [`R_StoreWallRange`].
    pub rw_angle1: angle_t,
    /// Screen column where rendering of the current seg starts (inclusive).
    ///
    /// Also used as the loop variable inside `R_RenderSegLoop`.
    pub rw_x: i32,
    /// Screen column where rendering of the current seg stops (exclusive).
    ///
    /// Set to `stop + 1` in [`R_StoreWallRange`].
    pub rw_stopx: i32,
    /// Angle used to compute the texture-U coordinate for each column.
    ///
    /// Set to `ANG90 + viewangle - rw_normalangle` by [`R_StoreWallRange`].
    pub rw_centerangle: angle_t,
    /// Texture horizontal offset for the current seg (fixed-point pixels).
    ///
    /// Combines the linedef's `textureoffset`, the seg's own `offset`, and a
    /// view-angle correction.
    pub rw_offset: fixed_t,
    /// Perpendicular distance from the viewpoint to the wall (fixed-point).
    ///
    /// Used with `finesine` to derive the scale at each column.
    pub rw_distance: fixed_t,
    /// Projection scale at the current column.
    ///
    /// Larger values mean the wall is closer.  Stepped by `rw_scalestep`
    /// across the seg.
    pub rw_scale: fixed_t,
    /// Per-column increment for `rw_scale`.
    ///
    /// Computed as `(scale2 - scale1) / (stop - start)`.
    pub rw_scalestep: fixed_t,
    /// Texture vertical midpoint for the middle wall texture (fixed-point).
    ///
    /// Controls where the texture origin sits relative to the column;
    /// affected by `DONTPEGBOTTOM`.
    pub rw_midtexturemid: fixed_t,
    /// Texture vertical midpoint for the upper wall texture (fixed-point).
    ///
    /// Affected by `DONTPEGTOP`.
    pub rw_toptexturemid: fixed_t,
    /// Texture vertical midpoint for the lower wall texture (fixed-point).
    ///
    /// Affected by `DONTPEGBOTTOM`.
    pub rw_bottomtexturemid: fixed_t,
    /// Front sector ceiling height minus `viewz`, in world units (not fixed-point).
    ///
    /// Used as the top boundary for wall-texture rendering.  Shifted right by
    /// 4 before the seg loop.
    pub worldtop: i32,
    /// Front sector floor height minus `viewz`, in world units (not fixed-point).
    ///
    /// Used as the bottom boundary for wall-texture rendering.  Shifted right
    /// by 4 before the seg loop.
    pub worldbottom: i32,
    /// Back sector ceiling height minus `viewz`, in world units (not fixed-point).
    ///
    /// Meaningful only for two-sided lines; used to determine if an upper
    /// texture is needed.
    pub worldhigh: i32,
    /// Back sector floor height minus `viewz`, in world units (not fixed-point).
    ///
    /// Meaningful only for two-sided lines; used to determine if a lower
    /// texture is needed.
    pub worldlow: i32,
    /// Screen-y of the top of the upper texture at the current column (scaled).
    ///
    /// Stepped by `pixhighstep` each column in `R_RenderSegLoop`.
    pub pixhigh: fixed_t,
    /// Screen-y of the bottom of the lower texture at the current column (scaled).
    ///
    /// Stepped by `pixlowstep` each column in `R_RenderSegLoop`.
    pub pixlow: fixed_t,
    /// Per-column step for `pixhigh`.
    pub pixhighstep: fixed_t,
    /// Per-column step for `pixlow`.
    pub pixlowstep: fixed_t,
    /// Fractional screen-y of the top of the wall at the current column.
    ///
    /// Maintained in 12.20 format; `>> HEIGHTBITS` yields the integer screen row.
    pub topfrac: fixed_t,
    /// Per-column step for `topfrac`.
    pub topstep: fixed_t,
    /// Fractional screen-y of the bottom of the wall at the current column.
    ///
    /// Maintained in 12.20 format; `>> HEIGHTBITS` yields the integer screen row.
    pub bottomfrac: fixed_t,
    /// Per-column step for `bottomfrac`.
    pub bottomstep: fixed_t,
    /// Pointer to the scale-based light table row for the current seg.
    ///
    /// Points into `scalelight[lightnum]` (or `scalelightfixed`); indexed by
    /// `rw_scale >> LIGHTSCALESHIFT` to pick the per-column colormap.
    pub walllights: *const *mut u8,
    /// Per-column texture-U coordinate buffer for masked mid-textures.
    ///
    /// Points into the `openings` scratch buffer (in `r_plane`); written by
    /// `R_RenderSegLoop` and consumed by [`R_RenderMaskedSegRange`].  A value
    /// of `i16::MAX` (SHRT_MAX) means the column has already been drawn or is
    /// not present.
    pub maskedtexturecol: *mut i16,
}

impl WallState {
    /// The state before the first wall: all zero, null pointers.
    pub const fn new() -> Self {
        Self {
            segtextured: 0,
            markfloor: 0,
            markceiling: 0,
            maskedtexture: 0,
            toptexture: 0,
            bottomtexture: 0,
            midtexture: 0,
            rw_normalangle: 0,
            rw_angle1: 0,
            rw_x: 0,
            rw_stopx: 0,
            rw_centerangle: 0,
            rw_offset: 0,
            rw_distance: 0,
            rw_scale: 0,
            rw_scalestep: 0,
            rw_midtexturemid: 0,
            rw_toptexturemid: 0,
            rw_bottomtexturemid: 0,
            worldtop: 0,
            worldbottom: 0,
            worldhigh: 0,
            worldlow: 0,
            pixhigh: 0,
            pixlow: 0,
            pixhighstep: 0,
            pixlowstep: 0,
            topfrac: 0,
            topstep: 0,
            bottomfrac: 0,
            bottomstep: 0,
            walllights: ptr::null(),
            maskedtexturecol: ptr::null_mut(),
        }
    }
}

impl Default for WallState {
    fn default() -> Self {
        Self::new()
    }
}

/// The wall-segment state.
pub static WALL: Global<WallState> = Global::new(WallState::new());

/// The `scalelight` row for seg `curline` of `level` in `frontsector`,
/// adjusted for the seg's orientation (fake contrast) and clamped to the
/// table.
fn wall_light_row(
    level: &LevelState,
    frontsector: SectorId,
    curline: SegId,
    extralight: i32,
) -> *const *mut u8 {
    let mut lightnum: i32 =
        ((level.sectors[frontsector].lightlevel as u32) >> LIGHTSEGSHIFT) as i32 + extralight;

    let seg = &level.segs[curline];
    let (v1, v2) = (level.vertexes[seg.v1], level.vertexes[seg.v2]);
    if v1.y == v2.y {
        lightnum -= 1;
    } else if v1.x == v2.x {
        lightnum += 1;
    }

    if lightnum < 0 {
        scalelight_row(0)
    } else if lightnum >= LIGHTLEVELS as i32 {
        scalelight_row(LIGHTLEVELS - 1)
    } else {
        scalelight_row(lightnum as usize)
    }
}

// ---------------------------------------------------------------------------
// R_RenderMaskedSegRange
// ---------------------------------------------------------------------------

/// Draws the transparent mid-texture for a two-sided seg.
///
/// Called during the sprite-rendering pass (after all opaque walls are drawn)
/// for each [`drawseg_t`] that has a masked mid-texture.  Iterates columns
/// `[x1, x2]`; for each column where `maskedtexturecol` is not `SHRT_MAX`,
/// fetches the correct texture column, sets up lighting, and calls
/// [`R_DrawMaskedColumn`].
///
/// # Safety
///
/// `ds` must have been filled in by [`R_StoreWallRange`] during the current
/// frame.  All pointer fields within `ds` (`maskedtexturecol`,
/// `sprbottomclip`, `sprtopclip`) must still be valid, and `ds.curline` must
/// be a two-sided seg of the loaded level.
pub unsafe fn R_RenderMaskedSegRange(ds: &drawseg_t, x1: i32, x2: i32) {
    let level = LEVEL.borrow();
    let data = RENDER_DATA.borrow();
    let curline = ds.curline;
    let seg = &level.segs[curline];
    let sidedef = &level.sides[seg.sidedef];
    let texnum: i32 = *data.texturetranslation.add(sidedef.midtexture as usize);

    {
        let mut bsp = BSP.borrow_mut();
        bsp.curline = curline;
        bsp.frontsector = seg.frontsector;
        bsp.backsector = seg.backsector;
    }
    let frontsector = &level.sectors[seg.frontsector];
    let backsector = &level.sectors[seg.backsector.expect("masked seg without back sector")];

    let view = VIEW.borrow();
    let mut wall = WALL.borrow_mut();
    let mut draw = DRAW.borrow_mut();

    wall.walllights = wall_light_row(&level, seg.frontsector, curline, view.extralight);

    wall.maskedtexturecol = ds.maskedtexturecol;
    wall.rw_scalestep = ds.scalestep;
    let mut masked = MaskedColumnParams {
        mfloorclip: ds.sprbottomclip,
        mceilingclip: ds.sprtopclip,
        spryscale: ds.scale1 + (x1 - ds.x1) * wall.rw_scalestep,
        sprtopscreen: 0,
    };

    let mut column = ColumnParams::new(view.centery, data.colormaps);
    if level.lines[seg.linedef].flags & (LinedefFlag::DONTPEGBOTTOM as i16) != 0 {
        column.dc_texturemid = if frontsector.floorheight > backsector.floorheight {
            frontsector.floorheight
        } else {
            backsector.floorheight
        };
        column.dc_texturemid =
            column.dc_texturemid + *data.textureheight.add(texnum as usize) - view.viewz;
    } else {
        column.dc_texturemid = if frontsector.ceilingheight < backsector.ceilingheight {
            frontsector.ceilingheight
        } else {
            backsector.ceilingheight
        };
        column.dc_texturemid -= view.viewz;
    }
    column.dc_texturemid += sidedef.rowoffset;

    if !view.fixedcolormap.is_null() {
        column.dc_colormap = view.fixedcolormap;
    }

    let maskedtexturecol = wall.maskedtexturecol;
    column.dc_x = x1;
    while column.dc_x <= x2 {
        if *maskedtexturecol.add(column.dc_x as usize) != i16::MAX {
            if view.fixedcolormap.is_null() {
                let mut index = (masked.spryscale as u32) >> LIGHTSCALESHIFT;
                if index >= MAXLIGHTSCALE as u32 {
                    index = (MAXLIGHTSCALE - 1) as u32;
                }
                column.dc_colormap = *wall.walllights.add(index as usize);
            }

            masked.sprtopscreen =
                view.centeryfrac - FixedMul(column.dc_texturemid, masked.spryscale);
            column.dc_iscale = (0xffffffff_u32 / (masked.spryscale as u32)) as i32;

            let raw = R_GetColumn(
                &data,
                texnum,
                *maskedtexturecol.add(column.dc_x as usize) as i32,
            );
            let col = raw.sub(3);
            R_DrawMaskedColumn(
                col as *mut c_void,
                &masked,
                &mut column,
                &mut draw,
                view.colfunc,
            );

            *maskedtexturecol.add(column.dc_x as usize) = i16::MAX;
        }
        masked.spryscale += wall.rw_scalestep;
        column.dc_x += 1;
    }
}

// ---------------------------------------------------------------------------
// R_RenderSegLoop
// ---------------------------------------------------------------------------

/// Core per-column drawing loop for one wall segment.
///
/// Iterates from `rw_x` to `rw_stopx` (exclusive), performing the
/// following work for each column:
///
/// 1. Computes `yl` (top of wall) and `yh` (bottom of wall) from `topfrac`
///    and `bottomfrac`, clamped to the current clip boundaries.
/// 2. If `markceiling` is set, records the ceiling span extent into
///    `ceilingplane`.
/// 3. If `markfloor` is set, records the floor span extent into
///    `floorplane`.
/// 4. If `segtextured` is set, computes the texture-U column and the
///    per-column colormap index.
/// 5. Draws the middle, upper, or lower texture tier as appropriate, and
///    updates `ceilingclip` / `floorclip`.
/// 6. Records the masked texture column offset if `maskedtexture` is set.
///
/// # Safety
///
/// `ceilingplane`/`floorplane` must be valid whenever the matching mark is
/// set, and `walllights` and `maskedtexturecol` must be valid for the current
/// frame.
unsafe fn R_RenderSegLoop(
    wall: &mut WallState,
    planes: &mut PlaneState,
    view: &ViewState,
    draw: &mut DrawState,
    data: &RenderDataState,
) {
    let mut column = ColumnParams::new(view.centery, data.colormaps);
    let viewheight = draw.viewheight;

    while wall.rw_x < wall.rw_stopx {
        let rw_x = wall.rw_x as usize;
        let clip_ceil = planes.ceilingclip[rw_x] as i32;
        let clip_floor = planes.floorclip[rw_x] as i32;

        let mut yl = (wall.topfrac + HEIGHTUNIT - 1) >> HEIGHTBITS;
        if yl < clip_ceil + 1 {
            yl = clip_ceil + 1;
        }

        if wall.markceiling != 0 {
            let top = clip_ceil + 1;
            let mut bottom = yl - 1;
            if bottom >= clip_floor {
                bottom = clip_floor - 1;
            }
            if top <= bottom {
                let ceilingplane = &mut planes.visplanes[planes.ceilingplane.unwrap()];
                ceilingplane.top[rw_x] = top as u8;
                ceilingplane.bottom[rw_x] = bottom as u8;
            }
        }

        let mut yh = wall.bottomfrac >> HEIGHTBITS;
        if yh >= clip_floor {
            yh = clip_floor - 1;
        }

        if wall.markfloor != 0 {
            let mut top = yh + 1;
            let bottom = clip_floor - 1;
            if top <= clip_ceil {
                top = clip_ceil + 1;
            }
            if top <= bottom {
                let floorplane = &mut planes.visplanes[planes.floorplane.unwrap()];
                floorplane.top[rw_x] = top as u8;
                floorplane.bottom[rw_x] = bottom as u8;
            }
        }

        let texturecolumn: fixed_t;
        if wall.segtextured != 0 {
            let angle =
                (wall.rw_centerangle.wrapping_add(view.xtoviewangle[rw_x])) >> ANGLETOFINESHIFT;
            texturecolumn = wall.rw_offset
                - FixedMul(
                    tables::finetangent[angle as usize & FINETANGENTMASK],
                    wall.rw_distance,
                );

            let mut index = (wall.rw_scale as u32) >> LIGHTSCALESHIFT;
            if index >= MAXLIGHTSCALE as u32 {
                index = (MAXLIGHTSCALE - 1) as u32;
            }
            column.dc_colormap = *wall.walllights.add(index as usize);
            column.dc_x = wall.rw_x;
            column.dc_iscale = (0xffffffff_u32 / (wall.rw_scale as u32)) as i32;
        } else {
            texturecolumn = 0;
        }

        if wall.midtexture != 0 {
            column.dc_yl = yl;
            column.dc_yh = yh;
            column.dc_texturemid = wall.rw_midtexturemid;
            column.dc_source = R_GetColumn(data, wall.midtexture, texturecolumn >> FRACBITS);
            (view.colfunc)(&mut column, draw);
            planes.ceilingclip[rw_x] = viewheight as i16;
            planes.floorclip[rw_x] = -1;
        } else {
            if wall.toptexture != 0 {
                let mut mid = wall.pixhigh >> HEIGHTBITS;
                wall.pixhigh += wall.pixhighstep;

                let clip_floor = planes.floorclip[rw_x] as i32;
                if mid >= clip_floor {
                    mid = clip_floor - 1;
                }

                if mid >= yl {
                    column.dc_yl = yl;
                    column.dc_yh = mid;
                    column.dc_texturemid = wall.rw_toptexturemid;
                    column.dc_source =
                        R_GetColumn(data, wall.toptexture, texturecolumn >> FRACBITS);
                    (view.colfunc)(&mut column, draw);
                    planes.ceilingclip[rw_x] = mid as i16;
                } else {
                    planes.ceilingclip[rw_x] = (yl - 1) as i16;
                }
            } else {
                if wall.markceiling != 0 {
                    planes.ceilingclip[rw_x] = (yl - 1) as i16;
                }
            }

            if wall.bottomtexture != 0 {
                let mut mid = (wall.pixlow + HEIGHTUNIT - 1) >> HEIGHTBITS;
                wall.pixlow += wall.pixlowstep;

                let clip_ceil = planes.ceilingclip[rw_x] as i32;
                if mid <= clip_ceil {
                    mid = clip_ceil + 1;
                }

                if mid <= yh {
                    column.dc_yl = mid;
                    column.dc_yh = yh;
                    column.dc_texturemid = wall.rw_bottomtexturemid;
                    column.dc_source =
                        R_GetColumn(data, wall.bottomtexture, texturecolumn >> FRACBITS);
                    (view.colfunc)(&mut column, draw);
                    planes.floorclip[rw_x] = mid as i16;
                } else {
                    planes.floorclip[rw_x] = (yh + 1) as i16;
                }
            } else {
                if wall.markfloor != 0 {
                    planes.floorclip[rw_x] = (yh + 1) as i16;
                }
            }

            if wall.maskedtexture != 0 {
                *wall.maskedtexturecol.add(rw_x) = (texturecolumn >> FRACBITS) as i16;
            }
        }

        wall.rw_scale += wall.rw_scalestep;
        wall.topfrac += wall.topstep;
        wall.bottomfrac += wall.bottomstep;
        wall.rw_x += 1;
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
/// the `openings` scratch buffer and advances `ds_p` to the next free
/// [`drawseg_t`] slot.  Borrows the renderer state once for the whole wall.
///
/// A wall range is silently ignored if the [`drawseg_t`] pool
/// (`drawsegs[MAXDRAWSEGS]`) is full; the C source does the same.
///
/// The BSP traversal passes its borrow of the level (`level`); the wall
/// marks its linedef as seen on the automap there.
///
/// # Safety
///
/// `curline`, `frontsector`, and (for two-sided lines) `backsector` must be
/// set by the BSP traversal before this call, and the renderer's plane and
/// opening buffers must be set up for the frame.  The caller must hold no
/// borrow of the renderer state.
// FIXME: In R_StoreWallRange the Rust code computes `rw_scalestep` only when
//        `stop > start`, leaving `rw_scalestep` at its value from the
//        *previous* seg when `stop == start`.  The C source has the same
//        behaviour, so this is intentional, but it means `ds_p->scalestep`
//        may hold a stale value for single-column segs.
pub unsafe fn R_StoreWallRange(level: &mut LevelState, start: i32, stop: i32) {
    let mut bsp = BSP.borrow_mut();
    let bsp = &mut *bsp;
    if bsp.ds_p == MAXDRAWSEGS {
        return;
    }

    let data = RENDER_DATA.borrow();
    let view = VIEW.borrow();
    let skyflatnum = SKY.borrow().skyflatnum;
    let mut wall = WALL.borrow_mut();
    let wall = &mut *wall;
    let mut planes = PLANES.borrow_mut();
    let planes = &mut *planes;
    let mut draw = DRAW.borrow_mut();

    #[cfg(feature = "rangecheck")]
    {
        if start >= draw.viewwidth || start > stop {
            // Would call I_Error — skip in Rust for now
            return;
        }
    }

    let curline = bsp.curline;
    let seg = level.segs[curline];
    bsp.sidedef = seg.sidedef;
    bsp.linedef = seg.linedef;
    let ds = &mut bsp.drawsegs[bsp.ds_p];

    level.lines[seg.linedef].flags |= LinedefFlag::MAPPED as i16;

    let level = &*level;
    let linedef = &level.lines[seg.linedef];
    let sidedef = &level.sides[seg.sidedef];
    let frontsector = &level.sectors[bsp.frontsector];
    let backsector = bsp.backsector.map(|backsector| &level.sectors[backsector]);

    wall.rw_normalangle = seg.angle.wrapping_add(ANG90);
    let mut offsetangle = (wall.rw_normalangle.wrapping_sub(wall.rw_angle1) as i32).unsigned_abs();

    if offsetangle > ANG90 {
        offsetangle = ANG90;
    }

    let distangle = ANG90 - offsetangle;
    let v1 = level.vertexes[seg.v1];
    let hyp = R_PointToDist(v1.x, v1.y);
    let sineval = tables::finesine[(distangle >> ANGLETOFINESHIFT) as usize];
    wall.rw_distance = FixedMul(hyp, sineval);

    ds.x1 = start;
    wall.rw_x = start;
    ds.x2 = stop;
    ds.curline = curline;
    wall.rw_stopx = stop + 1;

    ds.scale1 = wall.rw_scale;
    wall.rw_scale = R_ScaleFromGlobalAngle(
        &view,
        wall,
        view.viewangle
            .wrapping_add(view.xtoviewangle[start as usize]),
    );
    ds.scale1 = wall.rw_scale;

    if stop > start {
        ds.scale2 = R_ScaleFromGlobalAngle(
            &view,
            wall,
            view.viewangle
                .wrapping_add(view.xtoviewangle[stop as usize]),
        );
        ds.scalestep = wall.rw_scalestep;
        wall.rw_scalestep = (ds.scale2 - wall.rw_scale) / (stop - start);
        ds.scalestep = wall.rw_scalestep;
    } else {
        ds.scale2 = ds.scale1;
    }

    let viewz = view.viewz;
    wall.worldtop = frontsector.ceilingheight - viewz;
    wall.worldbottom = frontsector.floorheight - viewz;

    wall.midtexture = 0;
    wall.toptexture = 0;
    wall.bottomtexture = 0;
    wall.maskedtexture = 0;
    ds.maskedtexturecol = ptr::null_mut();

    if let Some(backsector) = backsector {
        // two sided line
        ds.sprtopclip = ptr::null_mut();
        ds.sprbottomclip = ptr::null_mut();
        ds.silhouette = 0;

        if frontsector.floorheight > backsector.floorheight {
            ds.silhouette = SIL_BOTTOM;
            ds.bsilheight = frontsector.floorheight;
        } else if backsector.floorheight > viewz {
            ds.silhouette = SIL_BOTTOM;
            ds.bsilheight = i32::MAX;
        }

        if frontsector.ceilingheight < backsector.ceilingheight {
            ds.silhouette |= SIL_TOP;
            ds.tsilheight = frontsector.ceilingheight;
        } else if backsector.ceilingheight < viewz {
            ds.silhouette |= SIL_TOP;
            ds.tsilheight = i32::MIN;
        }

        if backsector.ceilingheight <= frontsector.floorheight {
            ds.sprbottomclip = negonearray_ptr();
            ds.bsilheight = i32::MAX;
            ds.silhouette |= SIL_BOTTOM;
        }

        if backsector.floorheight >= frontsector.ceilingheight {
            ds.sprtopclip = screenheightarray_ptr();
            ds.tsilheight = i32::MIN;
            ds.silhouette |= SIL_TOP;
        }

        wall.worldhigh = backsector.ceilingheight - viewz;
        wall.worldlow = backsector.floorheight - viewz;

        if frontsector.ceilingpic as i32 == skyflatnum && backsector.ceilingpic as i32 == skyflatnum
        {
            wall.worldtop = wall.worldhigh;
        }

        if wall.worldlow != wall.worldbottom
            || backsector.floorpic != frontsector.floorpic
            || backsector.lightlevel != frontsector.lightlevel
        {
            wall.markfloor = 1;
        } else {
            wall.markfloor = 0;
        }

        if wall.worldhigh != wall.worldtop
            || backsector.ceilingpic != frontsector.ceilingpic
            || backsector.lightlevel != frontsector.lightlevel
        {
            wall.markceiling = 1;
        } else {
            wall.markceiling = 0;
        }

        if backsector.ceilingheight <= frontsector.floorheight
            || backsector.floorheight >= frontsector.ceilingheight
        {
            wall.markceiling = 1;
            wall.markfloor = 1;
        }

        if wall.worldhigh < wall.worldtop {
            wall.toptexture = *data.texturetranslation.add(sidedef.toptexture as usize);
            if linedef.flags & (LinedefFlag::DONTPEGTOP as i16) != 0 {
                wall.rw_toptexturemid = wall.worldtop;
            } else {
                let vtop =
                    backsector.ceilingheight + *data.textureheight.add(sidedef.toptexture as usize);
                wall.rw_toptexturemid = vtop - viewz;
            }
        }

        if wall.worldlow > wall.worldbottom {
            wall.bottomtexture = *data.texturetranslation.add(sidedef.bottomtexture as usize);
            if linedef.flags & (LinedefFlag::DONTPEGBOTTOM as i16) != 0 {
                wall.rw_bottomtexturemid = wall.worldtop;
            } else {
                wall.rw_bottomtexturemid = wall.worldlow;
            }
        }

        wall.rw_toptexturemid += sidedef.rowoffset;
        wall.rw_bottomtexturemid += sidedef.rowoffset;

        if sidedef.midtexture != 0 {
            wall.maskedtexture = 1;
            ds.maskedtexturecol = planes.lastopening.sub(wall.rw_x as usize);
            wall.maskedtexturecol = ds.maskedtexturecol;
            planes.lastopening = planes.lastopening.add((wall.rw_stopx - wall.rw_x) as usize);
        }
    } else {
        // single sided line
        wall.midtexture = *data.texturetranslation.add(sidedef.midtexture as usize);
        wall.markfloor = 1;
        wall.markceiling = 1;

        if linedef.flags & (LinedefFlag::DONTPEGBOTTOM as i16) != 0 {
            let vtop =
                frontsector.floorheight + *data.textureheight.add(sidedef.midtexture as usize);
            wall.rw_midtexturemid = vtop - viewz;
        } else {
            wall.rw_midtexturemid = wall.worldtop;
        }
        wall.rw_midtexturemid += sidedef.rowoffset;

        ds.silhouette = SIL_BOTH;
        ds.sprtopclip = screenheightarray_ptr();
        ds.sprbottomclip = negonearray_ptr();
        ds.bsilheight = i32::MAX;
        ds.tsilheight = i32::MIN;
    }

    wall.segtextured = wall.midtexture | wall.toptexture | wall.bottomtexture | wall.maskedtexture;

    if wall.segtextured != 0 {
        let mut offsetangle = wall.rw_normalangle.wrapping_sub(wall.rw_angle1);
        if offsetangle > ANG180 {
            offsetangle = offsetangle.wrapping_neg();
        }
        if offsetangle > ANG90 {
            offsetangle = ANG90;
        }

        let sineval = tables::finesine[(offsetangle >> ANGLETOFINESHIFT) as usize];
        wall.rw_offset = FixedMul(hyp, sineval);

        if wall.rw_normalangle.wrapping_sub(wall.rw_angle1) < ANG180 {
            wall.rw_offset = -wall.rw_offset;
        }

        wall.rw_offset += sidedef.textureoffset + seg.offset;
        wall.rw_centerangle = ANG90
            .wrapping_add(view.viewangle)
            .wrapping_sub(wall.rw_normalangle);

        if view.fixedcolormap.is_null() {
            wall.walllights = wall_light_row(level, bsp.frontsector, curline, view.extralight);
        }
    }

    if frontsector.floorheight >= viewz {
        wall.markfloor = 0;
    }

    if frontsector.ceilingheight <= viewz && frontsector.ceilingpic as i32 != skyflatnum {
        wall.markceiling = 0;
    }

    wall.worldtop >>= 4;
    wall.worldbottom >>= 4;

    let centeryfrac = view.centeryfrac;
    wall.topstep = -FixedMul(wall.rw_scalestep, wall.worldtop);
    wall.topfrac = (centeryfrac >> 4) - FixedMul(wall.worldtop, wall.rw_scale);

    wall.bottomstep = -FixedMul(wall.rw_scalestep, wall.worldbottom);
    wall.bottomfrac = (centeryfrac >> 4) - FixedMul(wall.worldbottom, wall.rw_scale);

    if backsector.is_some() {
        wall.worldhigh >>= 4;
        wall.worldlow >>= 4;

        if wall.worldhigh < wall.worldtop {
            wall.pixhigh = (centeryfrac >> 4) - FixedMul(wall.worldhigh, wall.rw_scale);
            wall.pixhighstep = -FixedMul(wall.rw_scalestep, wall.worldhigh);
        }

        if wall.worldlow > wall.worldbottom {
            wall.pixlow = (centeryfrac >> 4) - FixedMul(wall.worldlow, wall.rw_scale);
            wall.pixlowstep = -FixedMul(wall.rw_scalestep, wall.worldlow);
        }
    }

    if wall.markceiling != 0 {
        let ceilingplane = planes.ceilingplane.unwrap();
        planes.ceilingplane = Some(R_CheckPlane(
            planes,
            ceilingplane,
            wall.rw_x,
            wall.rw_stopx - 1,
        ));
    }
    if wall.markfloor != 0 {
        let floorplane = planes.floorplane.unwrap();
        planes.floorplane = Some(R_CheckPlane(
            planes,
            floorplane,
            wall.rw_x,
            wall.rw_stopx - 1,
        ));
    }

    R_RenderSegLoop(wall, planes, &view, &mut draw, &data);

    let rw_stopx = wall.rw_stopx;
    if ((ds.silhouette & SIL_TOP) != 0 || wall.maskedtexture != 0) && ds.sprtopclip.is_null() {
        for i in 0..(rw_stopx - start) {
            *planes.lastopening.add(i as usize) = planes.ceilingclip[(start + i) as usize];
        }
        ds.sprtopclip = planes.lastopening.sub(start as usize);
        planes.lastopening = planes.lastopening.add((rw_stopx - start) as usize);
    }

    if ((ds.silhouette & SIL_BOTTOM) != 0 || wall.maskedtexture != 0) && ds.sprbottomclip.is_null()
    {
        for i in 0..(rw_stopx - start) {
            *planes.lastopening.add(i as usize) = planes.floorclip[(start + i) as usize];
        }
        ds.sprbottomclip = planes.lastopening.sub(start as usize);
        planes.lastopening = planes.lastopening.add((rw_stopx - start) as usize);
    }

    if wall.maskedtexture != 0 && (ds.silhouette & SIL_TOP) == 0 {
        ds.silhouette |= SIL_TOP;
        ds.tsilheight = i32::MIN;
    }
    if wall.maskedtexture != 0 && (ds.silhouette & SIL_BOTTOM) == 0 {
        ds.silhouette |= SIL_BOTTOM;
        ds.bsilheight = i32::MAX;
    }

    bsp.ds_p += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wall state starts zeroed with null light and masked-column pointers.
    #[test]
    fn state_defaults_to_zero() {
        let wall = WallState::new();
        assert_eq!(
            (wall.segtextured, wall.markfloor, wall.markceiling),
            (0, 0, 0)
        );
        assert_eq!((wall.rw_x, wall.rw_stopx, wall.rw_scale), (0, 0, 0));
        assert!(wall.walllights.is_null());
        assert!(wall.maskedtexturecol.is_null());
    }
}
