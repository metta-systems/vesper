//! BSP traversal and line-seg clipping for the Doom renderer.
//!
//! Corresponds to `vendor/doomgeneric/r_bsp.c`. Walks the BSP tree
//! front-to-back from the player's viewpoint, renders each subsector leaf,
//! and maintains the solid-column occlusion list (`solidsegs`) so that
//! already-covered screen columns are never redrawn.
//!
//! The C globals live in [`BspState`] ([`BSP`]) and, for the occlusion list
//! that the clippers hold across their `R_StoreWallRange` calls,
//! [`ClipSegsState`] ([`CLIP_SEGS`]). The recursion never holds a borrow
//! across a call into the wall, plane or sprite renderers.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ptr;

use crate::engine::{
    global::Global,
    map::{line_t, node_t, sector_t, seg_t, side_t},
    math::{
        bbox::BBox,
        fixed::fixed_t,
        tables::{ANG90, ANGLETOFINESHIFT},
    },
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of draw-segs that can be prepared in a single frame.
const MAXDRAWSEGS: usize = 256;

/// Maximum number of solid clip-ranges tracked by the occlusion list.
const MAXSEGS: usize = 32;

/// Flag bit in a BSP child index indicating the child is a subsector leaf,
/// not an interior node. Matches `NF_SUBSECTOR` in `doomdef.h`.
const NF_SUBSECTOR: u32 = 0x8000;

// ---------------------------------------------------------------------------
// Diagnostic probes for the "walls disappear / different room appears" bug
// ---------------------------------------------------------------------------
//
// Enable with `RUST_LOG=room::engine::render_3d::bsp=trace`. These are gated at TRACE so
// the existing debug log stays usable; terminate the app the moment the
// glitch appears and we inspect the tail of the trace.

/// Returns the index of `line` within the global `segs` array, or -1 if
/// either pointer is null. Used only for diagnostic trace logging.
#[inline]
unsafe fn seg_index(line: *const seg_t) -> isize {
    if segs.is_null() || line.is_null() {
        -1
    } else {
        (line as isize - segs as isize) / std::mem::size_of::<seg_t>() as isize
    }
}

// ---------------------------------------------------------------------------
// drawseg_t
// ---------------------------------------------------------------------------

/// A seg prepared for the column renderer, holding scale, silhouette, and
/// sprite-clip arrays (`drawseg_t` in `r_defs.h`). Written by
/// `R_StoreWallRange` and read back during sprite clipping in
/// `R_DrawSprite`.
#[derive(Clone, Copy)]
pub struct drawseg_t {
    /// The seg that generated this draw-seg.
    pub curline: *mut seg_t,
    /// Leftmost screen column of this draw-seg (inclusive).
    pub x1: i32,
    /// Rightmost screen column of this draw-seg (inclusive).
    pub x2: i32,
    /// Projection scale at column `x1`.
    pub scale1: fixed_t,
    /// Projection scale at column `x2`.
    pub scale2: fixed_t,
    /// Per-column scale increment: `(scale2 - scale1) / (x2 - x1)`.
    pub scalestep: fixed_t,
    /// Silhouette flags (`SIL_*`) indicating which edges clip sprites.
    pub silhouette: i32,
    /// Bottom silhouette height in fixed-point units (for lower-unpegged walls).
    pub bsilheight: fixed_t,
    /// Top silhouette height in fixed-point units (for upper-unpegged walls).
    pub tsilheight: fixed_t,
    /// Pointer into the sprite-top clip column array; null if not needed.
    pub sprtopclip: *mut i16,
    /// Pointer into the sprite-bottom clip column array; null if not needed.
    pub sprbottomclip: *mut i16,
    /// Pointer into the masked-texture column array; null for solid walls.
    pub maskedtexturecol: *mut i16,
}

/// Compile-time zero initializer for [`drawseg_t`], used to fill the
/// `drawsegs` array before any rendering occurs.
const ZERO_DRAWSEG: drawseg_t = drawseg_t {
    curline: ptr::null_mut(),
    x1: 0,
    x2: 0,
    scale1: 0,
    scale2: 0,
    scalestep: 0,
    silhouette: 0,
    bsilheight: 0,
    tsilheight: 0,
    sprtopclip: ptr::null_mut(),
    sprbottomclip: ptr::null_mut(),
    maskedtexturecol: ptr::null_mut(),
};

// ---------------------------------------------------------------------------
// cliprange_t (internal)
// ---------------------------------------------------------------------------

/// A solid horizontal screen-column range `[first, last]` that has been fully
/// covered by a previously drawn wall. The `solidsegs` array is a sorted,
/// non-overlapping list of these ranges, bounded by two sentinel entries.
/// Mirrors the `cliprange_t` struct defined locally in `r_bsp.c`.
#[derive(Clone, Copy)]
struct cliprange_t {
    /// First (leftmost) solid column in this range, inclusive.
    first: i32,
    /// Last (rightmost) solid column in this range, inclusive.
    last: i32,
}

// ---------------------------------------------------------------------------
// State shared with r_segs, r_plane and r_things
// ---------------------------------------------------------------------------

/// BSP traversal state (C `r_bsp.c` globals other than the clip list).
pub struct BspState {
    /// The seg currently being processed by `R_AddLine` and passed down to
    /// [`R_StoreWallRange`].
    pub curline: *mut seg_t,
    /// The sidedef of the current seg (`curline->sidedef`). Set by `R_StoreWallRange`
    /// in `r_segs.c`.
    pub sidedef: *mut side_t,
    /// The linedef of the current seg (`curline->linedef`). Set by `R_StoreWallRange`
    /// in `r_segs.c`.
    pub linedef: *mut line_t,
    /// The sector on the front side of the current seg. Set by [`R_Subsector`]
    /// and read by `r_segs.c`.
    pub frontsector: *mut sector_t,
    /// The sector on the back side of the current seg, or null for single-sided
    /// lines. Set by `R_AddLine` and read by `r_segs.c`.
    pub backsector: *mut sector_t,
    /// Fixed-size array of prepared draw-segs for the current frame. Filled
    /// sequentially through `ds_p` (read during sprite clipping in `r_things.c`).
    pub drawsegs: [drawseg_t; MAXDRAWSEGS],
    /// Index of the next free slot in `drawsegs` (the C pointer `ds_p`).
    /// Advanced by `R_StoreWallRange` each time a new draw-seg is committed.
    pub ds_p: usize,
    /// Monotonically increasing frame counter used by diagnostic trace logging.
    /// Incremented once per frame in [`R_ClearClipSegs`].
    probe_frame: u64,
}

impl BspState {
    /// The state before the first frame: no current seg, no draw-segs.
    pub const fn new() -> Self {
        Self {
            curline: ptr::null_mut(),
            sidedef: ptr::null_mut(),
            linedef: ptr::null_mut(),
            frontsector: ptr::null_mut(),
            backsector: ptr::null_mut(),
            drawsegs: [ZERO_DRAWSEG; MAXDRAWSEGS],
            ds_p: 0,
            probe_frame: 0,
        }
    }
}

impl Default for BspState {
    fn default() -> Self {
        Self::new()
    }
}

/// The BSP traversal state.
pub static BSP: Global<BspState> = Global::new(BspState::new());

// ---------------------------------------------------------------------------
// Module-local state
// ---------------------------------------------------------------------------

/// The solid-column occlusion list (C `solidsegs` and `newend`).
pub struct ClipSegsState {
    /// Sorted array of solid screen-column ranges (clip list). Entries
    /// `[0..newend)` are valid; the first and last entries are permanent
    /// sentinels initialized by [`R_ClearClipSegs`].
    solidsegs: [cliprange_t; MAXSEGS],
    /// One-past-the-end index into `solidsegs`, tracking how many ranges are
    /// currently active. Maintained by [`R_ClipSolidWallSegment`] and reset by
    /// [`R_ClearClipSegs`].
    newend: usize,
}

impl ClipSegsState {
    /// An empty clip list.
    pub const fn new() -> Self {
        Self {
            solidsegs: [cliprange_t { first: 0, last: 0 }; MAXSEGS],
            newend: 0,
        }
    }
}

impl Default for ClipSegsState {
    fn default() -> Self {
        Self::new()
    }
}

/// The solid-column occlusion list.
static CLIP_SEGS: Global<ClipSegsState> = Global::new(ClipSegsState::new());

// checkcoord — 12 rows, only 11 initialised in C (rows 3 and 7 are {0})
/// Lookup table that selects the two diagonal corners of a bounding box to
/// use for angle computation in [`R_CheckBBox`], indexed by the 4-bit
/// combined viewer-position code `(boxy << 2) | boxx`. Four rows are
/// zero-filled and never meaningfully indexed: row 5 is the `boxpos == 5`
/// case (viewer inside the box) where `R_CheckBBox` returns early before
/// reaching the table; rows 3 and 7 correspond to `boxx == 3`, which is
/// impossible because `boxx` can only be 0, 1, or 2; row 11 is padding
/// added by Rust to complete the array (the C original has only 11 entries,
/// indices 0-10). Each entry is four `BBox::{TOP,BOTTOM,LEFT,RIGHT}` indices.
/// Mirrors the `checkcoord` table from `r_bsp.c`.
static CHECKCOORD: [[i32; 4]; 12] = [
    [3, 0, 2, 1],
    [3, 0, 2, 0],
    [3, 1, 2, 0],
    [0, 0, 0, 0],
    [2, 0, 2, 1],
    [0, 0, 0, 0],
    [3, 1, 3, 0],
    [0, 0, 0, 0],
    [2, 0, 3, 1],
    [2, 1, 3, 1],
    [2, 1, 3, 0],
    [0, 0, 0, 0], // C has no 12th row; zero-padded
];

// ---------------------------------------------------------------------------
// Imports from other modules
// ---------------------------------------------------------------------------

use crate::engine::{
    play::setup::{nodes, segs, subsectors},
    render_3d::{
        draw::DRAW,
        main::{R_PointOnSide, R_PointToAngle, VIEW},
        plane::{R_FindPlane, PLANES},
        segs::{R_StoreWallRange, WALL},
        sky::SKY,
        things::R_AddSprites,
    },
};

// ---------------------------------------------------------------------------
// R_ClearDrawSegs
// ---------------------------------------------------------------------------

/// Resets the draw-seg write index to the beginning of `drawsegs`,
/// discarding all draw-segs accumulated during the previous frame.
/// Called once per frame by `R_RenderPlayerView` before BSP traversal begins.
///
/// # Safety
/// Must be called from the render thread. Mutates `ds_p`.
pub unsafe fn R_ClearDrawSegs() {
    BSP.borrow_mut().ds_p = 0;
}

// ---------------------------------------------------------------------------
// R_ClearClipSegs
// ---------------------------------------------------------------------------

/// Resets the solid-column occlusion list to the two permanent sentinel
/// entries that cover the off-screen left (`-0x7fffffff..-1`) and right
/// (`viewwidth..0x7fffffff`) regions. Also advances the per-frame diagnostic
/// counter used by trace logging.
/// Called once per frame by `R_RenderPlayerView` before BSP traversal begins.
///
/// # Safety
/// Must be called from the render thread. Mutates `solidsegs`, `newend`,
/// and `probe_frame`. Reads `viewwidth` and `viewangle`.
pub unsafe fn R_ClearClipSegs() {
    let viewwidth = DRAW.borrow().viewwidth;
    {
        let mut clip = CLIP_SEGS.borrow_mut();
        clip.solidsegs[0].first = -0x7fffffff;
        clip.solidsegs[0].last = -1;
        clip.solidsegs[1].first = viewwidth;
        clip.solidsegs[1].last = 0x7fffffff;
        clip.newend = 2;
    }

    // Diagnostic: frame marker. R_ClearClipSegs is called once per frame by
    // R_RenderPlayerView before descending the BSP.
    let probe_frame = {
        let mut bsp = BSP.borrow_mut();
        bsp.probe_frame = bsp.probe_frame.wrapping_add(1);
        bsp.probe_frame
    };
    let view = VIEW.borrow();
    log::trace!(
        "=== FRAME {} === viewx={:#x} viewy={:#x} viewangle={:#x} viewwidth={}",
        probe_frame,
        view.viewx,
        view.viewy,
        view.viewangle,
        viewwidth
    );
}

// ---------------------------------------------------------------------------
// R_ClipSolidWallSegment
// ---------------------------------------------------------------------------

/// Clips the screen-column range `[first, last]` against the solid occlusion
/// list and renders every visible sub-range by calling [`R_StoreWallRange`].
/// Inserts a new solid entry for any newly covered columns, merging or
/// compacting adjacent/overlapping ranges in `solidsegs`.
///
/// Used for fully opaque walls (single-sided linedefs and closed doors) that
/// completely block everything behind them.
///
/// Corresponds to `R_ClipSolidWallSegment` in `r_bsp.c`; the C pointers
/// `start`, `next` and `newend` are indices into `solidsegs`.
///
/// # Safety
/// Caller must ensure `solidsegs` and `newend` have been initialized by
/// [`R_ClearClipSegs`] before this frame. `first` must be <= `last` for a
/// valid seg (a debug log is emitted if violated, but the function still runs
/// to match C behavior). Mutates the clip list.
pub unsafe fn R_ClipSolidWallSegment(first: i32, last: i32) {
    if first > last {
        log::debug!(
            "R_ClipSolidWallSegment INVALID: first={} > last={}",
            first,
            last
        );
    }
    // R_StoreWallRange does not touch the clip list, so the borrow is held
    // across its calls.
    let mut clip = CLIP_SEGS.borrow_mut();
    let clip = &mut *clip;
    let solidsegs = &mut clip.solidsegs;

    let mut start = 0;
    while solidsegs[start].last < first - 1 {
        start += 1;
    }

    if first < solidsegs[start].first {
        if last < solidsegs[start].first - 1 {
            R_StoreWallRange(first, last);
            let mut next = clip.newend;
            clip.newend = next + 1;

            while next != start {
                solidsegs[next] = solidsegs[next - 1];
                next -= 1;
            }
            solidsegs[next].first = first;
            solidsegs[next].last = last;
            return;
        }

        R_StoreWallRange(first, solidsegs[start].first - 1);
        solidsegs[start].first = first;
    }

    if last <= solidsegs[start].last {
        return;
    }

    let mut next = start;
    loop {
        let next_plus_1 = next + 1;
        if last < solidsegs[next_plus_1].first - 1 {
            break;
        }

        R_StoreWallRange(solidsegs[next].last + 1, solidsegs[next_plus_1].first - 1);
        next += 1;

        if last <= solidsegs[next].last {
            solidsegs[start].last = solidsegs[next].last;
            // "goto crunch"
            if next == start {
                return;
            }
            // C: while (next++ != newend) { *++start = *next; }
            loop {
                let old_next = next;
                next += 1;
                if old_next == clip.newend {
                    break;
                }
                start += 1;
                solidsegs[start] = solidsegs[next];
            }
            clip.newend = start + 1;
            return;
        }
    }

    R_StoreWallRange(solidsegs[next].last + 1, last);
    solidsegs[start].last = last;

    // crunch:
    if next == start {
        return;
    }

    // C: while (next++ != newend) { *++start = *next; }
    loop {
        let old_next = next;
        next += 1;
        if old_next == clip.newend {
            break;
        }
        start += 1;
        solidsegs[start] = solidsegs[next];
    }
    clip.newend = start + 1;
}

// ---------------------------------------------------------------------------
// R_ClipPassWallSegment
// ---------------------------------------------------------------------------

/// Clips the screen-column range `[first, last]` against the solid occlusion
/// list and renders every visible sub-range by calling [`R_StoreWallRange`],
/// but does **not** add any new solid entries to the occlusion list.
///
/// Used for transparent or partial walls (two-sided linedefs with height
/// differences) that let the player see through to the sector behind.
///
/// Corresponds to `R_ClipPassWallSegment` in `r_bsp.c`.
///
/// # Safety
/// Caller must ensure `solidsegs` and `newend` have been initialized by
/// [`R_ClearClipSegs`] before this frame. `first` must be <= `last` for a
/// valid seg (a debug log is emitted if violated, but the function still runs
/// to match C behavior). Reads the clip list without modifying it.
pub unsafe fn R_ClipPassWallSegment(first: i32, last: i32) {
    if first > last {
        log::debug!(
            "R_ClipPassWallSegment INVALID: first={} > last={}",
            first,
            last
        );
    }
    // R_StoreWallRange does not touch the clip list, so the borrow is held
    // across its calls.
    let clip = CLIP_SEGS.borrow();
    let solidsegs = &clip.solidsegs;

    let mut start = 0;
    while solidsegs[start].last < first - 1 {
        start += 1;
    }

    if first < solidsegs[start].first {
        if last < solidsegs[start].first - 1 {
            R_StoreWallRange(first, last);
            return;
        }

        R_StoreWallRange(first, solidsegs[start].first - 1);
    }

    if last <= solidsegs[start].last {
        return;
    }

    loop {
        let start_plus_1 = start + 1;
        if last < solidsegs[start_plus_1].first - 1 {
            break;
        }

        R_StoreWallRange(solidsegs[start].last + 1, solidsegs[start_plus_1].first - 1);
        start += 1;

        if last <= solidsegs[start].last {
            return;
        }
    }

    R_StoreWallRange(solidsegs[start].last + 1, last);
}

// ---------------------------------------------------------------------------
// R_AddLine
// ---------------------------------------------------------------------------

/// Clips and conditionally renders one seg from the current subsector.
///
/// 1. Computes view-relative angles for both seg endpoints.
/// 2. Back-face culls if the angular span is >= 180 degrees (ANG180).
/// 3. Clips the angular range to the view frustum (`±clipangle`).
/// 4. Projects the clipped angles to screen columns via `viewangletox`.
/// 5. Rejects degenerate single-column segs (`x1 == x2`).
/// 6. Classifies the seg as solid (single-sided line or closed door) or
///    pass-through (window, height delta, or mid-texture), and dispatches
///    to [`R_ClipSolidWallSegment`] or [`R_ClipPassWallSegment`] accordingly.
///
/// Sets `curline` to `line` and `backsector` to the back sector pointer
/// before calling the clip functions.
///
/// Corresponds to `R_AddLine` in `r_bsp.c`.
///
/// # Safety
/// `line` must point to a valid, initialized [`seg_t`] whose `v1`, `v2`,
/// `sidedef`, `linedef`, `frontsector`, and (if non-null) `backsector`
/// pointers are all valid. The view state (`viewangle`, `clipangle`,
/// `viewangletox`) and `frontsector` must have been initialized before the
/// current frame. Must be called only during BSP traversal (i.e. within
/// [`R_Subsector`]).
unsafe fn R_AddLine(line: *mut seg_t) {
    let (probe_frame, frontsector) = {
        let mut bsp = BSP.borrow_mut();
        bsp.curline = line;
        (bsp.probe_frame, bsp.frontsector)
    };
    let curline = line;

    let orig_angle1 = R_PointToAngle((*(*line).v1).x, (*(*line).v1).y);
    let orig_angle2 = R_PointToAngle((*(*line).v2).x, (*(*line).v2).y);

    let (viewangle, clipangle) = {
        let view = VIEW.borrow();
        (view.viewangle, view.clipangle)
    };

    let span = orig_angle1.wrapping_sub(orig_angle2);

    // Back side?
    if span >= 0x8000_0000 {
        log::trace!(
            "R_AddLine SKIP backface: orig_a1={:#x} orig_a2={:#x} va={:#x}",
            orig_angle1,
            orig_angle2,
            viewangle
        );
        return;
    }

    WALL.borrow_mut().rw_angle1 = orig_angle1;
    let mut angle1 = orig_angle1.wrapping_sub(viewangle);
    let mut angle2 = orig_angle2.wrapping_sub(viewangle);

    let clipangle_d2 = clipangle.wrapping_mul(2);

    let mut tspan = angle1.wrapping_add(clipangle);
    if tspan > clipangle_d2 {
        tspan = tspan.wrapping_sub(clipangle_d2);
        if tspan >= span {
            log::trace!("R_AddLine SKIP off-left: orig_a1={:#x} orig_a2={:#x} va={:#x} a1={:#x} a2={:#x} span={:#x} tspan={:#x}", orig_angle1, orig_angle2, viewangle, angle1, angle2, span, tspan);
            return;
        }
        angle1 = clipangle;
    }

    tspan = clipangle.wrapping_sub(angle2);
    if tspan > clipangle_d2 {
        tspan = tspan.wrapping_sub(clipangle_d2);
        if tspan >= span {
            log::trace!("R_AddLine SKIP off-right: orig_a1={:#x} orig_a2={:#x} va={:#x} a1={:#x} a2={:#x} span={:#x} tspan={:#x}", orig_angle1, orig_angle2, viewangle, angle1, angle2, span, tspan);
            return;
        }
        angle2 = 0u32.wrapping_sub(clipangle);
    }

    let idx1 = ((angle1.wrapping_add(ANG90)) >> ANGLETOFINESHIFT) as usize;
    let idx2 = ((angle2.wrapping_add(ANG90)) >> ANGLETOFINESHIFT) as usize;
    let (x1, x2) = {
        let view = VIEW.borrow();
        (view.viewangletox[idx1], view.viewangletox[idx2])
    };

    // Log walls that land in the right portion of the screen
    if x2 >= 200 || x1 >= 200 {
        log::trace!(
            "R_AddLine wall: orig_a1={:#x} orig_a2={:#x} va={:#x} clip={:#x} a1={:#x} a2={:#x} idx1={} idx2={} x1={} x2={}",
            orig_angle1, orig_angle2, viewangle, clipangle, angle1, angle2, idx1, idx2, x1, x2
        );
    }
    // Diagnostic: log every surviving wall (full screen) at TRACE.
    log::trace!(
        "R_AddLine projected: frame={} seg={} x1={} x2={} a1={:#x} a2={:#x}",
        probe_frame,
        seg_index(line),
        x1,
        x2,
        angle1,
        angle2
    );

    if x1 == x2 {
        log::trace!(
            "R_AddLine SKIP x1==x2: orig_a1={:#x} orig_a2={:#x} x1={} x2={}",
            orig_angle1,
            orig_angle2,
            x1,
            x2
        );
        return;
    }

    let backsector = (*line).backsector;
    BSP.borrow_mut().backsector = backsector;

    // Single sided line?
    if backsector.is_null() {
        log::trace!(
            "R_AddLine classify: frame={} seg={} x1={} x2={} decision=SOLID(1-sided) back=null front={:p}",
            probe_frame, seg_index(line), x1, x2, frontsector as *const _
        );
        R_ClipSolidWallSegment(x1, x2 - 1);
        return;
    }

    // Closed door
    if (*backsector).ceilingheight <= (*frontsector).floorheight
        || (*backsector).floorheight >= (*frontsector).ceilingheight
    {
        log::trace!(
            "R_AddLine classify: frame={} seg={} x1={} x2={} decision=SOLID(closed-door) back={:p} f.ch={} f.fh={} b.ch={} b.fh={}",
            probe_frame, seg_index(line), x1, x2, backsector as *const _,
            (*frontsector).ceilingheight, (*frontsector).floorheight,
            (*backsector).ceilingheight, (*backsector).floorheight
        );
        R_ClipSolidWallSegment(x1, x2 - 1);
        return;
    }

    // Window
    if (*backsector).ceilingheight != (*frontsector).ceilingheight
        || (*backsector).floorheight != (*frontsector).floorheight
    {
        log::trace!(
            "R_AddLine classify: frame={} seg={} x1={} x2={} decision=PASS(window) back={:p} f.ch={} f.fh={} b.ch={} b.fh={}",
            probe_frame, seg_index(line), x1, x2, backsector as *const _,
            (*frontsector).ceilingheight, (*frontsector).floorheight,
            (*backsector).ceilingheight, (*backsector).floorheight
        );
        R_ClipPassWallSegment(x1, x2 - 1);
        return;
    }

    // Reject empty lines
    if (*backsector).ceilingpic == (*frontsector).ceilingpic
        && (*backsector).floorpic == (*frontsector).floorpic
        && (*backsector).lightlevel == (*frontsector).lightlevel
        && (*(*curline).sidedef).midtexture == 0
    {
        log::trace!(
            "R_AddLine classify: frame={} seg={} x1={} x2={} decision=REJECT(empty)",
            probe_frame,
            seg_index(line),
            x1,
            x2
        );
        return;
    }

    log::trace!(
        "R_AddLine classify: frame={} seg={} x1={} x2={} decision=PASS(midtex/lighting) midtex={}",
        probe_frame,
        seg_index(line),
        x1,
        x2,
        (*(*curline).sidedef).midtexture
    );
    R_ClipPassWallSegment(x1, x2 - 1);
}

// ---------------------------------------------------------------------------
// R_CheckBBox
// ---------------------------------------------------------------------------

// NOTE: These indices MUST match the C enum in vendor/doomgeneric/m_bbox.h
// (and the matching Rust constants in m_bbox.rs) which stores bbox fields as
// [TOP, BOTTOM, LEFT, RIGHT]. An earlier version had LEFT/RIGHT/TOP/BOTTOM
// ordering here which made R_CheckBBox compare viewx against y-coordinates
// (and viewy against x-coordinates), producing axis-scrambled visibility
// tests — far BSP subtrees were not pruned and rendering descended into the
// wrong parts of the map, causing walls to "disappear" and a different room
// to show through (classic Doom HOM variant).

/// Tests whether a BSP node bounding box might contain any visible geometry
/// from the player's current viewpoint.
///
/// Returns 1 if any part of the box could be visible, 0 if it is entirely
/// hidden behind already-drawn solid walls.
///
/// The test works in three stages:
/// 1. Classify the viewer's position relative to the box (left/inside/right
///    on each axis) and look up the two "most extreme" diagonal corners in
///    `CHECKCOORD`.
/// 2. Compute view-relative BAM angles to those corners and clip against
///    the horizontal frustum (`±clipangle`).
/// 3. Project to screen columns and check whether the column range is fully
///    covered by a single entry in `solidsegs`.
///
/// Corresponds to `R_CheckBBox` in `r_bsp.c`.
///
/// # Safety
/// `bspcoord` must point to a valid 4-element `fixed_t` array laid out as
/// `[TOP, BOTTOM, LEFT, RIGHT]` (matching `BBox::TOP` etc. from `m_bbox.rs`).
/// The view state (`viewx`, `viewy`, `viewangle`, `clipangle`,
/// `viewangletox`) and `solidsegs` must have been initialized for the
/// current frame.
pub unsafe fn R_CheckBBox(bspcoord: *mut fixed_t) -> i32 {
    let view = VIEW.borrow();
    let (viewx, viewy, viewangle, clipangle) =
        (view.viewx, view.viewy, view.viewangle, view.clipangle);

    let boxx = if viewx <= *bspcoord.add(BBox::LEFT) {
        0
    } else if viewx < *bspcoord.add(BBox::RIGHT) {
        1
    } else {
        2
    };

    let boxy = if viewy >= *bspcoord.add(BBox::TOP) {
        0
    } else if viewy > *bspcoord.add(BBox::BOTTOM) {
        1
    } else {
        2
    };

    let boxpos = (boxy << 2) + boxx;
    if boxpos == 5 {
        return 1;
    }

    let cc = CHECKCOORD[boxpos as usize];
    let x1 = *bspcoord.add(cc[0] as usize);
    let y1 = *bspcoord.add(cc[1] as usize);
    let x2 = *bspcoord.add(cc[2] as usize);
    let y2 = *bspcoord.add(cc[3] as usize);

    let mut angle1 = R_PointToAngle(x1, y1).wrapping_sub(viewangle);
    let mut angle2 = R_PointToAngle(x2, y2).wrapping_sub(viewangle);

    let span = angle1.wrapping_sub(angle2);

    if span >= 0x8000_0000 {
        return 1;
    }

    let clipangle_d2 = clipangle.wrapping_mul(2);

    let mut tspan = angle1.wrapping_add(clipangle);
    if tspan > clipangle_d2 {
        tspan = tspan.wrapping_sub(clipangle_d2);
        if tspan >= span {
            return 0;
        }
        angle1 = clipangle;
    }

    tspan = clipangle.wrapping_sub(angle2);
    if tspan > clipangle_d2 {
        tspan = tspan.wrapping_sub(clipangle_d2);
        if tspan >= span {
            return 0;
        }
        angle2 = 0u32.wrapping_sub(clipangle);
    }

    let sx1 = view.viewangletox[((angle1.wrapping_add(ANG90)) >> ANGLETOFINESHIFT) as usize];
    let sx2 = view.viewangletox[((angle2.wrapping_add(ANG90)) >> ANGLETOFINESHIFT) as usize];

    if sx1 == sx2 {
        return 0;
    }
    let sx2 = sx2 - 1;

    let clip = CLIP_SEGS.borrow();
    let mut start = 0;
    while clip.solidsegs[start].last < sx2 {
        start += 1;
    }

    if sx1 >= clip.solidsegs[start].first && sx2 <= clip.solidsegs[start].last {
        return 0;
    }

    1
}

// ---------------------------------------------------------------------------
// R_Subsector
// ---------------------------------------------------------------------------

/// Renders one BSP leaf subsector.
///
/// 1. Increments the subsector counter `sscount`.
/// 2. Sets `frontsector` from the subsector's sector pointer.
/// 3. Registers floor and ceiling visplanes with [`R_FindPlane`] if the
///    viewer can see them (floor below eye, ceiling above eye or sky flat).
/// 4. Adds sprites for all things in the sector via [`R_AddSprites`].
/// 5. Iterates over all segs in the subsector and calls `R_AddLine` for
///    each, which performs frustum clipping and dispatches wall rendering.
///
/// Corresponds to `R_Subsector` in `r_bsp.c`.
///
/// # Safety
/// `num` must be a valid index into the global `subsectors` array (bounds are
/// not checked at runtime to match C behavior). The subsector's `sector`
/// pointer and all `segs` it references must be valid.
pub unsafe fn R_Subsector(num: i32) {
    let num_usize = num as usize;

    let viewz = {
        let mut view = VIEW.borrow_mut();
        view.sscount += 1;
        view.viewz
    };

    let sub = &*subsectors.add(num_usize);
    let frontsector = sub.sector;
    BSP.borrow_mut().frontsector = frontsector;
    let mut count = sub.numlines as i32;
    let mut line = segs.add(sub.firstline as usize);

    let floorplane = if (*frontsector).floorheight < viewz {
        Some(R_FindPlane(
            (*frontsector).floorheight,
            (*frontsector).floorpic as i32,
            (*frontsector).lightlevel as i32,
        ))
    } else {
        None
    };
    PLANES.borrow_mut().floorplane = floorplane;

    let skyflatnum = SKY.borrow().skyflatnum;
    let ceilingplane =
        if (*frontsector).ceilingheight > viewz || (*frontsector).ceilingpic as i32 == skyflatnum {
            Some(R_FindPlane(
                (*frontsector).ceilingheight,
                (*frontsector).ceilingpic as i32,
                (*frontsector).lightlevel as i32,
            ))
        } else {
            None
        };
    PLANES.borrow_mut().ceilingplane = ceilingplane;

    R_AddSprites(frontsector);

    while count > 0 {
        R_AddLine(line);
        line = line.add(1);
        count -= 1;
    }
}

// ---------------------------------------------------------------------------
// R_RenderBSPNode
// ---------------------------------------------------------------------------

/// Recursively traverses the BSP tree and renders all visible subsectors.
///
/// If `bspnum` has the `NF_SUBSECTOR` flag set it is a leaf: calls
/// [`R_Subsector`] with the subsector index (treating -1 as subsector 0).
///
/// Otherwise loads the [`node_t`] at index `bspnum`, determines which side
/// the viewpoint is on via [`R_PointOnSide`], recurses into the near (front)
/// child first, then checks the far (back) child's bounding box with
/// [`R_CheckBBox`] and recurses into it only if it might be visible.
///
/// This front-to-back ordering ensures that solid walls encountered first
/// (nearer to the player) fill the `solidsegs` occlusion list, pruning
/// distant subtrees early.
///
/// Corresponds to `R_RenderBSPNode` in `r_bsp.c`.
///
/// # Safety
/// `bspnum` must be either a valid node index into the global `nodes` array
/// or a value with the `NF_SUBSECTOR` flag set whose lower bits are a valid
/// subsector index. All node and subsector data must have been loaded by
/// `P_SetupLevel`. The view position and the occlusion list must be
/// initialized for the current frame.
pub unsafe fn R_RenderBSPNode(bspnum: i32) {
    // Found a subsector?
    if (bspnum as u32) & NF_SUBSECTOR != 0 {
        let sub_num = if bspnum == -1 {
            0
        } else {
            (bspnum as u32 & !NF_SUBSECTOR) as i32
        };
        log::trace!(
            "R_RenderBSPNode: frame={} leaf subsector={} (bspnum={:#x})",
            BSP.borrow().probe_frame,
            sub_num,
            bspnum as u32
        );
        R_Subsector(sub_num);
        return;
    }

    let bsp = &*(nodes.add(bspnum as usize) as *const node_t);

    let (viewx, viewy) = {
        let view = VIEW.borrow();
        (view.viewx, view.viewy)
    };
    let side = R_PointOnSide(viewx, viewy, bsp);

    log::trace!(
        "R_RenderBSPNode: frame={} node={} side={} front_child={:#x} back_child={:#x}",
        BSP.borrow().probe_frame,
        bspnum,
        side,
        bsp.children[side as usize] as u32,
        bsp.children[(side ^ 1) as usize] as u32,
    );

    R_RenderBSPNode(bsp.children[side as usize] as i32);

    let back_visible = R_CheckBBox(bsp.bbox[(side ^ 1) as usize].as_ptr() as *mut fixed_t) != 0;
    let back_box = bsp.bbox[(side ^ 1) as usize];
    log::trace!(
        "R_RenderBSPNode: frame={} node={} back_visible={} back_bbox=[L={} R={} T={} B={}]",
        BSP.borrow().probe_frame,
        bspnum,
        back_visible,
        back_box[BBox::LEFT],
        back_box[BBox::RIGHT],
        back_box[BBox::TOP],
        back_box[BBox::BOTTOM],
    );
    if back_visible {
        R_RenderBSPNode(bsp.children[(side ^ 1) as usize] as i32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The BSP state starts with no current seg and an empty draw-seg pool.
    #[test]
    fn state_starts_empty() {
        let bsp = BspState::new();
        assert!(bsp.curline.is_null() && bsp.frontsector.is_null());
        assert_eq!(bsp.ds_p, 0);
        assert_eq!(ClipSegsState::new().newend, 0);
    }
}
