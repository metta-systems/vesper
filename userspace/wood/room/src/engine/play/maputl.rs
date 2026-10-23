//! Map geometry utilities, blockmap iterators, and intercept routines.
//!
//! Rust port of `vendor/doomgeneric/p_maputl.c`. Provides the low-level
//! primitives used by `p_map.c` for collision detection, line-of-sight, and
//! hitscan/projectile tracing: approximate distance, point-on-line tests,
//! thing-position management, blockmap traversal, and the intercept pipeline
//! used by `P_PathTraverse`.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::{ffi::c_void, ptr};

use crate::engine::{
    global::Global,
    map::{LineId, slopetype_t},
    math::fixed::{FRACBITS, FRACUNIT, FixedDiv, FixedMul, fixed_t},
    play::{
        info::*,
        mobj::mobj_t,
        pspr::PSPR,
        setup::{LEVEL, LevelState, SPAWN_SPOTS, blockmap_origin},
    },
    render_3d::main::{R_PointInSubsector, VALIDCOUNT},
};

/// Width/height of one blockmap cell in map units (`MAPBLOCKUNITS` in
/// `p_local.h`).
pub const MAPBLOCKUNITS: i32 = 128;
/// Width/height of one blockmap cell in fixed-point units (`MAPBLOCKSIZE`
/// in `p_local.h`).
pub const MAPBLOCKSIZE: i32 = MAPBLOCKUNITS * FRACUNIT;
/// Right-shift count converting a fixed-point coordinate to a blockmap
/// cell index (`MAPBLOCKSHIFT` in `p_local.h`).
pub const MAPBLOCKSHIFT: i32 = FRACBITS as i32 + 7;
/// Mask isolating the in-cell offset of a fixed-point coordinate
/// (`MAPBMASK` in `p_local.h`).
pub const MAPBMASK: i32 = MAPBLOCKSIZE - 1;
/// Shift count converting in-cell offsets back to fixed-point fractions
/// (`MAPBTOFRAC` in `p_local.h`).
pub const MAPBTOFRAC: i32 = MAPBLOCKSHIFT - FRACBITS as i32;

/// Intercept record produced by `P_PathTraverse` (`intercept_t` in
/// `p_local.h`).
///
/// `frac` is the fractional position along the trace (fixed-point in
/// `[0, FRACUNIT]`). `isaline` selects the active arm of [`intercept_t_d`]:
/// non-zero means `d.line`, zero means `d.thing`.
#[derive(Clone, Copy)]
pub struct intercept_t {
    pub frac: i32,
    pub isaline: i32, // boolean
    pub d: intercept_t_d,
}

/// Untagged union holding the intercepted entity (`intercept_t.d` in
/// `p_local.h`).
///
/// Active arm is selected by [`intercept_t::isaline`]. Reading the inactive
/// arm is undefined behaviour; callers must consult `isaline` first.
#[derive(Clone, Copy)]
pub union intercept_t_d {
    pub thing: *mut c_void,
    pub line: LineId,
}

/// Parametric dividing line (`divline_t` in `p_local.h`).
///
/// Used by `P_PointOnDivlineSide`, the sight check and the
/// intercept-traversal helpers. `(x, y)` is the start point; `(dx, dy)` is
/// the direction vector in fixed-point map units.
#[derive(Clone, Copy)]
pub struct divline_t {
    /// X coordinate of the line origin (fixed-point map units).
    pub x: fixed_t,
    /// Y coordinate of the line origin (fixed-point map units).
    pub y: fixed_t,
    /// X component of the line direction (fixed-point).
    pub dx: fixed_t,
    /// Y component of the line direction (fixed-point).
    pub dy: fixed_t,
}

/// The original Doom intercepts array capacity (128 entries).
///
/// When the number of intercepts exceeds this value the engine emulates the
/// original buffer-overrun behaviour via `InterceptsOverrun`. Matches
/// `MAXINTERCEPTS` in `p_local.h` (Chocolate Doom source).
pub const MAXINTERCEPTS_ORIGINAL: usize = 128;

/// Extended intercepts capacity used by this port.
///
/// 61 extra entries are allocated so that overrun emulation can write into the
/// correct adjacent variables without touching unrelated memory.
pub const MAXINTERCEPTS: usize = MAXINTERCEPTS_ORIGINAL + 61;

/// `P_PathTraverse` flag: collect line intercepts while traversing.
pub const PT_ADDLINES: i32 = 1;

/// `P_PathTraverse` flag: collect thing intercepts while traversing.
pub const PT_ADDTHINGS: i32 = 2;

/// `P_PathTraverse` flag: stop early at the first solid (one-sided) line hit.
pub const PT_EARLYOUT: i32 = 4;

// ---------------------------------------------------------------------------
// Globals
// ---------------------------------------------------------------------------

/// Line-opening results and path-traversal state (`p_maputl.c` globals).
///
/// Path traversals re-enter: an intercept callback can damage or kill a map
/// object, whose actions move other objects and traverse again. Every access
/// is therefore a short borrow that never spans a callback, matching the C
/// globals that each nested call overwrites.
pub struct MapUtlState {
    /// Top of the open vertical range through the last line tested by
    /// `P_LineOpening` (fixed-point map units). Read by `p_map.c`.
    pub opentop: fixed_t,
    /// Bottom of the open vertical range through the last line tested by
    /// `P_LineOpening` (fixed-point map units). Read by `p_map.c`.
    pub openbottom: fixed_t,
    /// Height of the open range (`opentop - openbottom`). Zero means the line is
    /// closed (impassable). Read by `p_map.c`.
    pub openrange: fixed_t,
    /// The lower of the two floor heights on either side of the last line tested
    /// by `P_LineOpening`. Used for step-up logic in `p_map.c`.
    pub lowfloor: fixed_t,
    /// Write cursor into the [`INTERCEPTS`] array.
    ///
    /// Initialised to `&intercepts[0]` at the start of each `P_PathTraverse`
    /// call and advanced by `PIT_AddLineIntercepts` / `PIT_AddThingIntercepts`.
    /// `P_TraverseIntercepts` uses `intercept_p - intercepts` to know how many
    /// entries were collected.
    pub intercept_p: *mut intercept_t,
    /// The current path-traverse ray, set by `P_PathTraverse`.
    ///
    /// `PIT_AddLineIntercepts` and `PIT_AddThingIntercepts` read this to
    /// determine whether each line/thing crosses the ray.
    pub trace: divline_t,
    /// Whether `P_PathTraverse` should exit immediately upon hitting a solid
    /// one-sided line. Set from the `PT_EARLYOUT` flag passed to `P_PathTraverse`.
    earlyout: i32,
}

impl MapUtlState {
    /// The state at program start: no opening measured, no traversal run.
    pub const fn new() -> Self {
        Self {
            opentop: 0,
            openbottom: 0,
            openrange: 0,
            lowfloor: 0,
            intercept_p: ptr::null_mut(),
            trace: divline_t {
                x: 0,
                y: 0,
                dx: 0,
                dy: 0,
            },
            earlyout: 0,
        }
    }
}

impl Default for MapUtlState {
    fn default() -> Self {
        Self::new()
    }
}

/// Line-opening results and path-traversal state.
pub static MAPUTL: Global<MapUtlState> = Global::new(MapUtlState::new());

/// Array of intercept records accumulated during a `P_PathTraverse` call.
///
/// Each entry records where the trace ray hit a line or thing along with the
/// parametric fraction `t` along the ray. The array is indexed via
/// `intercept_p`. Entries beyond index `MAXINTERCEPTS_ORIGINAL` trigger
/// `InterceptsOverrun` to emulate vanilla memory layout behaviour.
///
/// `P_TraverseIntercepts` hands entries to callbacks that may start a nested
/// traversal, so the array is only reached through `intercepts_base`'s
/// stable pointer.
pub static INTERCEPTS: Global<[intercept_t; MAXINTERCEPTS]> = Global::new(
    [intercept_t {
        frac: 0,
        isaline: 0,
        d: intercept_t_d {
            thing: ptr::null_mut(),
        },
    }; MAXINTERCEPTS],
);

/// Stable pointer to `intercepts[0]`.
fn intercepts_base() -> *mut intercept_t {
    INTERCEPTS.as_ptr().cast::<intercept_t>()
}

/// Top of the last line opening (C `opentop`).
pub fn opentop() -> fixed_t {
    MAPUTL.borrow().opentop
}

/// Bottom of the last line opening (C `openbottom`).
pub fn openbottom() -> fixed_t {
    MAPUTL.borrow().openbottom
}

/// Height of the last line opening (C `openrange`).
pub fn openrange() -> fixed_t {
    MAPUTL.borrow().openrange
}

/// Lower floor of the last line opening (C `lowfloor`).
pub fn lowfloor() -> fixed_t {
    MAPUTL.borrow().lowfloor
}

/// The current path-traverse ray (C `trace`).
pub fn trace() -> divline_t {
    MAPUTL.borrow().trace
}

// ---------------------------------------------------------------------------
// P_AproxDistance
// ---------------------------------------------------------------------------

/// Fast approximate Euclidean distance between two points.
///
/// Computes `|dx| + |dy| - min(|dx|, |dy|) / 2`, which over-estimates the
/// true distance by at most ~6 %. Used wherever an exact distance is not
/// required (enemy AI, sound attenuation).
///
/// Matches `P_AproxDistance` in `p_maputl.c`.
pub fn P_AproxDistance(dx: fixed_t, dy: fixed_t) -> fixed_t {
    let dx = dx.wrapping_abs();
    let dy = dy.wrapping_abs();
    if dx < dy {
        dx.wrapping_add(dy).wrapping_sub(dx >> 1)
    } else {
        dx.wrapping_add(dy).wrapping_sub(dy >> 1)
    }
}

// ---------------------------------------------------------------------------
// P_PointOnLineSide
// ---------------------------------------------------------------------------

/// Return which side of linedef `line` of `level` a point lies on.
///
/// Returns 0 if the point is on the front (right-hand) side of the line, or
/// 1 if it is on the back side. Uses the precomputed `line.dx` / `line.dy`
/// and cross-product sign.
///
/// Matches `P_PointOnLineSide` in `p_maputl.c`.
pub fn P_PointOnLineSide(level: &LevelState, x: fixed_t, y: fixed_t, line: LineId) -> i32 {
    let line = &level.lines[line];
    let v1 = level.vertexes[line.v1];
    if line.dx == 0 {
        if x <= v1.x {
            return if line.dy > 0 { 1 } else { 0 };
        }
        return if line.dy < 0 { 1 } else { 0 };
    }
    if line.dy == 0 {
        if y <= v1.y {
            return if line.dx < 0 { 1 } else { 0 };
        }
        return if line.dx > 0 { 1 } else { 0 };
    }
    let dx = x - v1.x;
    let dy = y - v1.y;
    let left = FixedMul(line.dy >> FRACBITS, dx);
    let right = FixedMul(dy, line.dx >> FRACBITS);
    if right < left { 0 } else { 1 }
}

// ---------------------------------------------------------------------------
// P_PointOnDivlineSide
// ---------------------------------------------------------------------------

/// Return which side of a `divline_t` a point lies on.
///
/// Returns 0 (front) or 1 (back). Uses a sign-bit fast path before falling
/// back to a fixed-point cross product. This variant operates on a `divline_t`
/// (ray) rather than a full `line_t`.
///
/// Matches `P_PointOnDivlineSide` in `p_maputl.c`.
pub fn P_PointOnDivlineSide(x: fixed_t, y: fixed_t, line: &divline_t) -> i32 {
    if line.dx == 0 {
        if x <= line.x {
            return if line.dy > 0 { 1 } else { 0 };
        }
        return if line.dy < 0 { 1 } else { 0 };
    }
    if line.dy == 0 {
        if y <= line.y {
            return if line.dx < 0 { 1 } else { 0 };
        }
        return if line.dx > 0 { 1 } else { 0 };
    }
    let dx = x - line.x;
    let dy = y - line.y;
    if ((line.dy ^ line.dx ^ dx ^ dy) & (0x80000000u32 as i32)) != 0 {
        if ((line.dy ^ dx) & (0x80000000u32 as i32)) != 0 {
            return 1;
        }
        return 0;
    }
    let left = FixedMul(line.dy >> 8, dx >> 8);
    let right = FixedMul(dy >> 8, line.dx >> 8);
    if right < left { 0 } else { 1 }
}

// ---------------------------------------------------------------------------
// P_MakeDivline
// ---------------------------------------------------------------------------

/// Populate a `divline_t` from the geometry of linedef `li` of `level`.
///
/// Sets `dl.x` / `dl.y` to `line.v1`, and `dl.dx` / `dl.dy` to
/// `line.dx` / `line.dy`. Used to convert a map linedef into a ray for
/// intercept testing.
///
/// Matches `P_MakeDivline` in `p_maputl.c`.
pub fn P_MakeDivline(level: &LevelState, li: LineId, dl: &mut divline_t) {
    let li = &level.lines[li];
    let v1 = level.vertexes[li.v1];
    dl.x = v1.x;
    dl.y = v1.y;
    dl.dx = li.dx;
    dl.dy = li.dy;
}

// ---------------------------------------------------------------------------
// P_InterceptVector
// ---------------------------------------------------------------------------

/// Compute the fractional intercept `t` along `v2` where it crosses `v1`.
///
/// Returns the fixed-point parameter `t` such that `v2.origin + t * v2.dir`
/// is the intersection point. Returns 0 when the lines are parallel
/// (`den == 0`). Uses an 8-bit pre-shift to avoid overflow with large
/// fixed-point coordinates.
///
/// `v2` is the ray being measured; `v1` is the crossing line.
///
/// Matches `P_InterceptVector` in `p_maputl.c`.
pub fn P_InterceptVector(v2: &divline_t, v1: &divline_t) -> fixed_t {
    let den = FixedMul(v1.dy >> 8, v2.dx) - FixedMul(v1.dx >> 8, v2.dy);
    if den == 0 {
        return 0;
    }
    let num = FixedMul((v1.x - v2.x) >> 8, v1.dy) + FixedMul((v2.y - v1.y) >> 8, v1.dx);
    FixedDiv(num, den)
}

// ---------------------------------------------------------------------------
// P_BoxOnLineSide
// ---------------------------------------------------------------------------

/// Determine which side(s) of linedef `ld` of `level` an axis-aligned
/// bounding box spans.
///
/// `tmbox` is a 4-element array `[top, bottom, left, right]` in fixed-point
/// map units. Returns 0 if the entire box is on the front side, 1 if entirely
/// on the back side, or -1 if the box straddles the line. Treats the line as
/// infinite.
///
/// Uses `line.slopetype` to select the fastest test variant (`ST_HORIZONTAL`,
/// `ST_VERTICAL`, `ST_POSITIVE`, or `ST_NEGATIVE`).
///
/// Matches `P_BoxOnLineSide` in `p_maputl.c`.
pub fn P_BoxOnLineSide(level: &LevelState, tmbox: &[fixed_t; 4], ld: LineId) -> i32 {
    let line = &level.lines[ld];
    let v1 = level.vertexes[line.v1];
    let mut p1: i32;
    let mut p2: i32;
    match line.slopetype {
        slopetype_t::ST_HORIZONTAL => {
            p1 = (tmbox[0] > v1.y) as i32;
            p2 = (tmbox[1] > v1.y) as i32;
            if line.dx < 0 {
                p1 ^= 1;
                p2 ^= 1;
            }
        }
        slopetype_t::ST_VERTICAL => {
            p1 = (tmbox[3] < v1.x) as i32;
            p2 = (tmbox[2] < v1.x) as i32;
            if line.dy < 0 {
                p1 ^= 1;
                p2 ^= 1;
            }
        }
        slopetype_t::ST_POSITIVE => {
            p1 = P_PointOnLineSide(level, tmbox[2], tmbox[0], ld);
            p2 = P_PointOnLineSide(level, tmbox[3], tmbox[1], ld);
        }
        slopetype_t::ST_NEGATIVE => {
            p1 = P_PointOnLineSide(level, tmbox[3], tmbox[0], ld);
            p2 = P_PointOnLineSide(level, tmbox[2], tmbox[1], ld);
        }
    }
    if p1 == p2 { p1 } else { -1 }
}

// ---------------------------------------------------------------------------
// P_LineOpening
// ---------------------------------------------------------------------------

/// Compute the open vertical portal range for two-sided linedef `linedef`
/// of `level`.
///
/// Sets the module globals `opentop`, `openbottom`, `openrange`, and
/// `lowfloor` based on the floor/ceiling heights of the front and back sectors.
/// If the linedef has no back side (`sidenum[1] == -1`) then `openrange` is
/// set to 0 (no passage).
///
/// These globals are read by `p_map.c` after each crossing test to determine
/// whether a moving object fits through the gap.
///
/// Matches `P_LineOpening` in `p_maputl.c`.
///
/// # Panics
/// If the line has a back side but lacks a front or back sector (C
/// dereferences the null sector).
pub fn P_LineOpening(level: &LevelState, linedef: LineId) {
    let linedef = &level.lines[linedef];
    let mut opening = MAPUTL.borrow_mut();
    if linedef.sidenum[1] == -1 {
        opening.openrange = 0;
        return;
    }
    let front = &level.sectors[linedef
        .frontsector
        .expect("two-sided line without front sector")];
    let back = &level.sectors[linedef
        .backsector
        .expect("two-sided line without back sector")];
    if front.ceilingheight < back.ceilingheight {
        opening.opentop = front.ceilingheight;
    } else {
        opening.opentop = back.ceilingheight;
    }
    if front.floorheight > back.floorheight {
        opening.openbottom = front.floorheight;
        opening.lowfloor = back.floorheight;
    } else {
        opening.openbottom = back.floorheight;
        opening.lowfloor = front.floorheight;
    }
    opening.openrange = opening.opentop - opening.openbottom;
}

// ---------------------------------------------------------------------------
// P_UnsetThingPosition
// ---------------------------------------------------------------------------

/// Unlink a map object from the sector thing-list and the blockmap.
///
/// Removes `thing` from:
/// - the doubly-linked sector thing-list (unless `MF_NOSECTOR` is set), and
/// - the blockmap cell's singly-linked thing-list (unless `MF_NOBLOCKMAP` is
///   set).
///
/// Must be called before moving an mobj; `P_SetThingPosition` re-inserts it
/// at the new location.
///
/// Matches `P_UnsetThingPosition` in `p_maputl.c`.
///
/// # Safety
/// `thing` must be a valid, non-null pointer to an initialised `mobj_t`. All
/// linked-list pointers (`snext`, `sprev`, `bnext`, `bprev`) must be either
/// null or valid `mobj_t` pointers. The global blockmap arrays from
/// `p_setup` must be initialised.
pub unsafe fn P_UnsetThingPosition(thing: *mut mobj_t) {
    unsafe {
        let thing = &mut *thing;
        let mut level = LEVEL.borrow_mut();
        if (thing.flags & MF_NOSECTOR) == 0 {
            if !thing.snext.is_null() {
                let snext = thing.snext;
                (*snext).sprev = thing.sprev;
            }
            if !thing.sprev.is_null() {
                let sprev = thing.sprev;
                (*sprev).snext = thing.snext;
            } else {
                let sector = level.subsectors[thing.subsector].sector;
                level.sectors[sector].thinglist = thing.snext;
            }
        }
        if (thing.flags & MF_NOBLOCKMAP) == 0 {
            if !thing.bnext.is_null() {
                let bnext = thing.bnext;
                (*bnext).bprev = thing.bprev;
            }
            if !thing.bprev.is_null() {
                let bprev = thing.bprev;
                (*bprev).bnext = thing.bnext;
            } else {
                let blockx = (thing.x - level.bmaporgx) >> MAPBLOCKSHIFT;
                let blocky = (thing.y - level.bmaporgy) >> MAPBLOCKSHIFT;
                if blockx >= 0
                    && blockx < level.bmapwidth
                    && blocky >= 0
                    && blocky < level.bmapheight
                {
                    let index = (blocky * level.bmapwidth + blockx) as usize;
                    level.blocklinks[index] = thing.bnext;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// P_SetThingPosition
// ---------------------------------------------------------------------------

/// Insert a map object into the sector thing-list and the blockmap.
///
/// Calls `R_PointInSubsector` to determine the correct subsector and sector,
/// then prepends `thing` to:
/// - the sector's doubly-linked thing-list (unless `MF_NOSECTOR` is set), and
/// - the blockmap cell's singly-linked thing-list (unless `MF_NOBLOCKMAP` is
///   set or the thing is outside the map bounds).
///
/// Also sets `thing->subsector` to the computed subsector.
///
/// Matches `P_SetThingPosition` in `p_maputl.c`.
///
/// # Safety
/// `thing` must be a valid, non-null pointer to an initialised `mobj_t`. The
/// global map data (sectors, blockmap) from `p_setup` must be loaded.
pub unsafe fn P_SetThingPosition(thing: *mut mobj_t) {
    unsafe {
        let thing = &mut *thing;
        let ss = R_PointInSubsector(thing.x, thing.y);
        thing.subsector = ss;
        let mut level = LEVEL.borrow_mut();
        if (thing.flags & MF_NOSECTOR) == 0 {
            let sector = level.subsectors[ss].sector;
            let sec = &mut level.sectors[sector];
            thing.sprev = ptr::null_mut();
            thing.snext = sec.thinglist;
            if !sec.thinglist.is_null() {
                let thinglist = sec.thinglist;
                (*thinglist).sprev = thing as *mut mobj_t;
            }
            sec.thinglist = thing as *mut mobj_t;
        }
        if (thing.flags & MF_NOBLOCKMAP) == 0 {
            let blockx = (thing.x - level.bmaporgx) >> MAPBLOCKSHIFT;
            let blocky = (thing.y - level.bmaporgy) >> MAPBLOCKSHIFT;
            if blockx >= 0 && blockx < level.bmapwidth && blocky >= 0 && blocky < level.bmapheight {
                let index = (blocky * level.bmapwidth + blockx) as usize;
                let link = &mut level.blocklinks[index];
                thing.bprev = ptr::null_mut();
                thing.bnext = *link;
                if !link.is_null() {
                    (**link).bprev = thing as *mut mobj_t;
                }
                *link = thing as *mut mobj_t;
            } else {
                thing.bnext = ptr::null_mut();
                thing.bprev = ptr::null_mut();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// P_BlockLinesIterator
// ---------------------------------------------------------------------------

/// Iterate all linedefs in blockmap cell `(x, y)`, calling `func` for each.
///
/// Returns 1 (true) if `func` returned non-zero for every line, 0 (false) if
/// `func` returned 0 for any line (early-out). Lines marked with the current
/// `validcount` are skipped to avoid processing the same line twice when it
/// spans multiple blockmap cells.
///
/// Out-of-bounds cell coordinates return 1 without calling `func`.
///
/// Matches `P_BlockLinesIterator` in `p_maputl.c`.
///
/// # Safety
/// A level must be loaded (blockmap, blockmap lump and lines from `p_setup`).
/// `func` must be `Some`, and calling it with any line of the loaded level
/// must be sound in the caller's current state: it is an `unsafe fn` and
/// this function discharges its contract on the caller's behalf.
pub unsafe fn P_BlockLinesIterator(x: i32, y: i32, func: Option<unsafe fn(LineId) -> u32>) -> u32 {
    // The callback can start a nested traversal whose intercept overrun
    // rewrites the blockmap dimensions; like C, this cell's list was located
    // before any callback ran. The callback may also borrow the level, so
    // each line is marked under a borrow that ends before the call.
    let mut list = {
        let level = LEVEL.borrow();
        if x < 0 || y < 0 || x >= level.bmapwidth || y >= level.bmapheight {
            return 1;
        }
        level.blockmaplump[4 + (y * level.bmapwidth + x) as usize] as usize
    };
    loop {
        let ld = {
            let mut level = LEVEL.borrow_mut();
            let linenum = level.blockmaplump[list];
            if linenum == -1 {
                return 1;
            }
            list += 1;
            let validcount = VALIDCOUNT.get();
            let line = &mut level.lines[linenum as usize];
            if line.validcount == validcount {
                continue;
            }
            line.validcount = validcount;
            LineId::new(linenum as usize)
        };
        if unsafe { func.unwrap()(ld) } == 0 {
            return 0;
        }
    }
}

// ---------------------------------------------------------------------------
// P_BlockThingsIterator
// ---------------------------------------------------------------------------

/// Iterate all map objects in blockmap cell `(x, y)`, calling `func` for each.
///
/// Returns 1 (true) if `func` returned non-zero for every thing, 0 (false)
/// if `func` returned 0 for any thing (early-out). Out-of-bounds cell
/// coordinates return 1 without calling `func`.
///
/// Matches `P_BlockThingsIterator` in `p_maputl.c`.
///
/// # Safety
/// A level must be loaded and its `blocklinks` chains must link only live
/// map objects. `func` must be `Some`, and calling it with any of those map
/// objects must be sound in the caller's current state: it is an
/// `unsafe fn` and this function discharges its contract on the caller's
/// behalf.
pub unsafe fn P_BlockThingsIterator(
    x: i32,
    y: i32,
    func: Option<unsafe fn(*mut mobj_t) -> u32>,
) -> u32 {
    let mut mobj = {
        let level = LEVEL.borrow();
        if x < 0 || y < 0 || x >= level.bmapwidth || y >= level.bmapheight {
            return 1;
        }
        level.blocklinks[(y * level.bmapwidth + x) as usize]
    };
    unsafe {
        while !mobj.is_null() {
            if func.unwrap()(mobj) == 0 {
                return 0;
            }
            mobj = (*mobj).bnext;
        }
        1
    }
}

// ---------------------------------------------------------------------------
// Intercept routines
// ---------------------------------------------------------------------------

/// Write a single `i32` value into a specific adjacent memory location to
/// emulate a vanilla Doom buffer overrun past the `intercepts` array.
///
/// Doom's original engine laid out its BSS segment in a fixed order; when
/// `intercepts[]` overflowed, writes would corrupt the adjacent variables in
/// that order. This function reproduces that behaviour for demo compatibility.
/// Each `skip!` / `write_i32!` / `write_i16_arr!` call accounts for one
/// variable in the original layout (offsets noted in the comments).
///
/// Based on PrBoom-plus research by Andrey Budko (entryway).
///
/// # Safety
/// Writes directly through raw pointers into engine state. Caller must
/// ensure `location` is within the range covered by the table (0..=~300 bytes).
#[allow(unused_assignments)]
unsafe fn InterceptsMemoryOverrun(location: i32, value: i32) {
    let mut offset: usize = 0;
    let loc = location as usize;

    macro_rules! skip {
        ($len:expr) => {
            if offset + $len > loc {
                return;
            }
            offset += $len;
        };
    }
    macro_rules! write_i32 {
        ($len:expr, $ptr:expr) => {
            if offset + $len > loc {
                let index = (loc - offset) / 4;
                ($ptr as *mut i32).add(index).write(value);
                return;
            }
            offset += $len;
        };
    }
    macro_rules! write_i16_arr {
        ($len:expr, $ptr:expr) => {
            if offset + $len > loc {
                let index = (loc - offset) / 2;
                let p = ($ptr as *mut _ as *mut u8) as *mut i16;
                p.add(index).write((value & 0xffff) as i16);
                p.add(index + 1).write(((value >> 16) & 0xffff) as i16);
                return;
            }
            offset += $len;
        };
    }

    // The targets live in other modules' state; they are written through
    // stable field pointers, and no guard on those states is live while a
    // path traversal collects intercepts.
    let maputl = MAPUTL.as_ptr();
    let level = LEVEL.as_ptr();
    skip!(4); // 0
    skip!(4); // 1 earlyout
    skip!(4); // 2 intercept_p
    write_i32!(4, &raw mut (*maputl).lowfloor); // 3 lowfloor
    write_i32!(4, &raw mut (*maputl).openbottom); // 4 openbottom
    write_i32!(4, &raw mut (*maputl).opentop); // 5 opentop
    write_i32!(4, &raw mut (*maputl).openrange); // 6 openrange
    skip!(4); // 7
    skip!(120); // 8 activeplats
    skip!(8); // 9
    write_i32!(4, &raw mut (*PSPR.as_ptr()).bulletslope); // 10 bulletslope
    skip!(4); // 11 swingx
    skip!(4); // 12 swingy
    skip!(4); // 13
    write_i16_arr!(40, &raw mut (*SPAWN_SPOTS.as_ptr()).playerstarts); // 14 playerstarts
    skip!(4); // 15 blocklinks
    write_i32!(4, &raw mut (*level).bmapwidth); // 16 bmapwidth
    skip!(4); // 17 blockmap
    write_i32!(4, &raw mut (*level).bmaporgx); // 18 bmaporgx
    write_i32!(4, &raw mut (*level).bmaporgy); // 19 bmaporgy
    skip!(4); // 20 blockmaplump
    write_i32!(4, &raw mut (*level).bmapheight); // 21 bmapheight
}

/// Emulate the vanilla Doom `intercepts[]` buffer overrun for demo fidelity.
///
/// When `num_intercepts` exceeds `MAXINTERCEPTS_ORIGINAL` (128), the original
/// engine would write past the array into adjacent globals. This function
/// calls `InterceptsMemoryOverrun` three times (for `frac`, `isaline`, and
/// `d`) to reproduce those writes. `d` is the address C stores: the thing's,
/// or for a line the address of its record in the level's `lines`.
///
/// # Safety
/// `intercept` must be a valid, non-null pointer to an initialised
/// `intercept_t`. No guard on the level may be live (the overrun writes the
/// blockmap dimensions).
unsafe fn InterceptsOverrun(num_intercepts: i32, intercept: *mut intercept_t) {
    if num_intercepts <= MAXINTERCEPTS_ORIGINAL as i32 {
        return;
    }
    let location = (num_intercepts - MAXINTERCEPTS_ORIGINAL as i32 - 1) * 12;
    let intercept = *intercept;
    let address = if intercept.isaline != 0 {
        let line = intercept.d.line;
        (&raw const LEVEL.borrow().lines[line]).addr()
    } else {
        intercept.d.thing.addr()
    };
    InterceptsMemoryOverrun(location, intercept.frac);
    InterceptsMemoryOverrun(location + 4, intercept.isaline);
    InterceptsMemoryOverrun(location + 8, address as i32);
}

// ---------------------------------------------------------------------------
// PIT_AddLineIntercepts
// ---------------------------------------------------------------------------

/// Intercept callback: record where the current `trace` ray crosses linedef `ld`.
///
/// Called by `P_BlockLinesIterator` during `P_PathTraverse`. If the ray
/// crosses `ld` (endpoints on opposite sides) and the intersection fraction
/// `t >= 0`, appends an `intercept_t` to `intercepts` and advances
/// `intercept_p`. Uses `P_PointOnDivlineSide` for long traces and
/// `P_PointOnLineSide` for short ones to avoid fixed-point precision issues.
///
/// If `PT_EARLYOUT` is set and the line has no back sector, returns 0 to halt
/// traversal immediately.
///
/// Returns 1 (continue) or 0 (stop).
///
/// Matches `PIT_AddLineIntercepts` in `p_maputl.c`.
///
/// # Safety
/// The global `trace`, `intercepts`, and `intercept_p` must be set up by a
/// preceding `P_PathTraverse` call.
pub unsafe fn PIT_AddLineIntercepts(ld: LineId) -> u32 {
    unsafe {
        let (trace, earlyout) = {
            let maputl = MAPUTL.borrow();
            (maputl.trace, maputl.earlyout)
        };
        let (frac, backsector) = {
            let level = LEVEL.borrow();
            let line = &level.lines[ld];
            let (s1, s2) = if trace.dx > FRACUNIT * 16
                || trace.dy > FRACUNIT * 16
                || trace.dx < -FRACUNIT * 16
                || trace.dy < -FRACUNIT * 16
            {
                let (v1, v2) = (level.vertexes[line.v1], level.vertexes[line.v2]);
                (
                    P_PointOnDivlineSide(v1.x, v1.y, &trace),
                    P_PointOnDivlineSide(v2.x, v2.y, &trace),
                )
            } else {
                (
                    P_PointOnLineSide(&level, trace.x, trace.y, ld),
                    P_PointOnLineSide(&level, trace.x + trace.dx, trace.y + trace.dy, ld),
                )
            };
            if s1 == s2 {
                return 1;
            }
            let mut dl = divline_t {
                x: 0,
                y: 0,
                dx: 0,
                dy: 0,
            };
            P_MakeDivline(&level, ld, &mut dl);
            (P_InterceptVector(&trace, &dl), line.backsector)
        };
        if frac < 0 {
            return 1;
        }
        if earlyout != 0 && frac < FRACUNIT && backsector.is_none() {
            return 0;
        }
        let intercept_p = MAPUTL.borrow().intercept_p;
        (*intercept_p).frac = frac;
        (*intercept_p).isaline = 1;
        (*intercept_p).d.line = ld;
        InterceptsOverrun(
            intercept_p.offset_from(intercepts_base()) as i32,
            intercept_p,
        );
        MAPUTL.borrow_mut().intercept_p = intercept_p.offset(1);
        1
    }
}

// ---------------------------------------------------------------------------
// PIT_AddThingIntercepts
// ---------------------------------------------------------------------------

/// Intercept callback: record where the current `trace` ray crosses thing `thing`.
///
/// Called by `P_BlockThingsIterator` during `P_PathTraverse`. Approximates
/// the thing as a diagonal bounding segment (corner-to-corner across its
/// radius), choosing orientation based on `trace` direction sign. If the ray
/// crosses the segment and `t >= 0`, appends an `intercept_t` (with
/// `isaline = 0`) and advances `intercept_p`.
///
/// Returns 1 (continue) always.
///
/// Matches `PIT_AddThingIntercepts` in `p_maputl.c`.
///
/// # Safety
/// `thing` must be a valid, non-null pointer to an initialised `mobj_t`. The
/// global `trace`, `intercepts`, and `intercept_p` must be set up by a
/// preceding `P_PathTraverse` call.
pub unsafe fn PIT_AddThingIntercepts(thing: *mut mobj_t) -> u32 {
    unsafe {
        let thing = &*thing;
        let trace = trace();
        // C: tracepositive = (trace.dx ^ trace.dy)>0;
        let tracepositive = (trace.dx ^ trace.dy) > 0;
        let (x1, y1, x2, y2) = if tracepositive {
            (
                thing.x - thing.radius,
                thing.y + thing.radius,
                thing.x + thing.radius,
                thing.y - thing.radius,
            )
        } else {
            (
                thing.x - thing.radius,
                thing.y - thing.radius,
                thing.x + thing.radius,
                thing.y + thing.radius,
            )
        };
        let s1 = P_PointOnDivlineSide(x1, y1, &trace);
        let s2 = P_PointOnDivlineSide(x2, y2, &trace);
        if s1 == s2 {
            return 1;
        }
        let dl = divline_t {
            x: x1,
            y: y1,
            dx: x2 - x1,
            dy: y2 - y1,
        };
        let frac = P_InterceptVector(&trace, &dl);
        if frac < 0 {
            return 1;
        }
        let intercept_p = MAPUTL.borrow().intercept_p;
        (*intercept_p).frac = frac;
        (*intercept_p).isaline = 0;
        (*intercept_p).d.thing = thing as *const _ as *mut c_void;
        InterceptsOverrun(
            intercept_p.offset_from(intercepts_base()) as i32,
            intercept_p,
        );
        MAPUTL.borrow_mut().intercept_p = intercept_p.offset(1);
        1
    }
}

// ---------------------------------------------------------------------------
// P_TraverseIntercepts
// ---------------------------------------------------------------------------

/// Walk the collected intercepts in nearest-first order, calling `func` for each.
///
/// After `P_PathTraverse` has filled `intercepts[0..intercept_p]`, this
/// function iterates them in ascending `frac` order (selection sort: picks the
/// minimum each pass, marks it with `i32::MAX` after processing). Stops
/// when the nearest remaining intercept has `frac > maxfrac`.
///
/// Returns 1 (true) if `func` accepted every intercept, 0 (false) if `func`
/// returned 0 for any intercept.
///
/// Matches `P_TraverseIntercepts` in `p_maputl.c`.
///
/// # Safety
/// `func` must be `Some`, and calling it with each collected intercept must
/// be sound in the caller's current state (it is an `unsafe fn`). The
/// intercepts and `intercept_p` must be in a consistent state as set by
/// `P_PathTraverse` / the `PIT_Add*` callbacks, so their lines and things
/// still point into the loaded level.
pub unsafe fn P_TraverseIntercepts(
    func: Option<unsafe fn(*mut intercept_t) -> u32>,
    maxfrac: fixed_t,
) -> u32 {
    // A callback can start a nested traversal that refills the array and
    // moves the cursor; like C, each scan reads the cursor afresh.
    let intercept_p = || MAPUTL.borrow().intercept_p;
    unsafe {
        let count = if intercept_p().is_null() {
            0
        } else {
            intercept_p().offset_from(intercepts_base()) as i32
        };
        for _ in 0..count {
            let mut dist = i32::MAX;
            let mut in_ptr: *mut intercept_t = ptr::null_mut();
            let mut scan = intercepts_base();
            while scan < intercept_p() {
                if (*scan).frac < dist {
                    dist = (*scan).frac;
                    in_ptr = scan;
                }
                scan = scan.offset(1);
            }
            if dist > maxfrac {
                return 1;
            }
            if func.unwrap()(in_ptr) == 0 {
                return 0;
            }
            (*in_ptr).frac = i32::MAX;
        }
        1
    }
}

// ---------------------------------------------------------------------------
// P_PathTraverse
// ---------------------------------------------------------------------------

/// Trace a ray from `(x1, y1)` to `(x2, y2)` through the blockmap.
///
/// Steps through each blockmap cell the ray crosses (up to 64 cells), calling
/// `P_BlockLinesIterator` and/or `P_BlockThingsIterator` (depending on
/// `flags`) to collect intercepts via `PIT_AddLineIntercepts` /
/// `PIT_AddThingIntercepts`. After traversal, calls `P_TraverseIntercepts`
/// with `trav` to process all hits in nearest-first order.
///
/// The origin is nudged by `FRACUNIT` if it falls exactly on a blockmap grid
/// line to avoid ambiguous cell assignments.
///
/// Returns 1 if the traverser accepted all intercepts, 0 if it rejected one
/// (signalling a hit / early stop).
///
/// Matches `P_PathTraverse` in `p_maputl.c`.
///
/// # Safety
/// A level must be loaded (see `P_BlockLinesIterator` and
/// `P_BlockThingsIterator`). `trav` must be `Some` whenever the ray collects
/// an intercept, and calling it with any intercept on the ray must be sound
/// in the caller's current state (it is an `unsafe fn`). All coordinates
/// are in fixed-point map units.
pub unsafe fn P_PathTraverse(
    x1: fixed_t,
    y1: fixed_t,
    x2: fixed_t,
    y2: fixed_t,
    flags: i32,
    trav: Option<unsafe fn(*mut intercept_t) -> u32>,
) -> u32 {
    VALIDCOUNT.set(VALIDCOUNT.get().wrapping_add(1));
    let (bmaporgx, bmaporgy) = blockmap_origin();
    let mut x1 = x1;
    let mut y1 = y1;
    if ((x1 - bmaporgx) & (MAPBLOCKSIZE - 1)) == 0 {
        x1 += FRACUNIT;
    }
    if ((y1 - bmaporgy) & (MAPBLOCKSIZE - 1)) == 0 {
        y1 += FRACUNIT;
    }
    {
        let mut maputl = MAPUTL.borrow_mut();
        maputl.earlyout = flags & PT_EARLYOUT;
        maputl.intercept_p = intercepts_base();
        maputl.trace = divline_t {
            x: x1,
            y: y1,
            dx: x2 - x1,
            dy: y2 - y1,
        };
    }
    let x1 = x1 - bmaporgx;
    let y1 = y1 - bmaporgy;
    let xt1 = x1 >> MAPBLOCKSHIFT;
    let yt1 = y1 >> MAPBLOCKSHIFT;
    let x2 = x2 - bmaporgx;
    let y2 = y2 - bmaporgy;
    let xt2 = x2 >> MAPBLOCKSHIFT;
    let yt2 = y2 >> MAPBLOCKSHIFT;
    let (mapxstep, partial, ystep) = if xt2 > xt1 {
        (
            1,
            FRACUNIT - ((x1 >> MAPBTOFRAC) & (FRACUNIT - 1)),
            FixedDiv(y2 - y1, (x2 - x1).wrapping_abs()),
        )
    } else if xt2 < xt1 {
        (
            -1,
            (x1 >> MAPBTOFRAC) & (FRACUNIT - 1),
            FixedDiv(y2 - y1, (x2 - x1).wrapping_abs()),
        )
    } else {
        (0, FRACUNIT, 256 * FRACUNIT)
    };
    let mut yintercept = (y1 >> MAPBTOFRAC) + FixedMul(partial, ystep);
    let (mapystep, partial, xstep) = if yt2 > yt1 {
        (
            1,
            FRACUNIT - ((y1 >> MAPBTOFRAC) & (FRACUNIT - 1)),
            FixedDiv(x2 - x1, (y2 - y1).wrapping_abs()),
        )
    } else if yt2 < yt1 {
        (
            -1,
            (y1 >> MAPBTOFRAC) & (FRACUNIT - 1),
            FixedDiv(x2 - x1, (y2 - y1).wrapping_abs()),
        )
    } else {
        (0, FRACUNIT, 256 * FRACUNIT)
    };
    let mut xintercept = (x1 >> MAPBTOFRAC) + FixedMul(partial, xstep);
    let mut mapx = xt1;
    let mut mapy = yt1;
    for _ in 0..64 {
        if flags & PT_ADDLINES != 0
            && P_BlockLinesIterator(
                mapx,
                mapy,
                Some(PIT_AddLineIntercepts as unsafe fn(LineId) -> u32),
            ) == 0
        {
            return 0;
        }
        if flags & PT_ADDTHINGS != 0
            && P_BlockThingsIterator(
                mapx,
                mapy,
                Some(PIT_AddThingIntercepts as unsafe fn(*mut mobj_t) -> u32),
            ) == 0
        {
            return 0;
        }
        if mapx == xt2 && mapy == yt2 {
            break;
        }
        if (yintercept >> FRACBITS) == mapy {
            yintercept += ystep;
            mapx += mapxstep;
        } else if (xintercept >> FRACBITS) == mapx {
            xintercept += xstep;
            mapy += mapystep;
        }
    }
    P_TraverseIntercepts(trav, FRACUNIT)
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::engine::{
            map::{VertexId, line_t},
            play::setup::LEVEL_TEST_LOCK,
        },
        std::sync::Mutex,
    };

    /// The lines handed to [`record_line`], in call order.
    static VISITED: Mutex<Vec<LineId>> = Mutex::new(Vec::new());

    /// Blockmap callback that records `line` and continues.
    unsafe fn record_line(line: LineId) -> u32 {
        VISITED.lock().unwrap().push(line);
        1
    }

    /// A one-sided line record; the iterator reads only its `validcount`.
    fn line_record() -> line_t {
        line_t {
            v1: VertexId::new(0),
            v2: VertexId::new(0),
            dx: 0,
            dy: 0,
            flags: 0,
            special: 0,
            tag: 0,
            sidenum: [0, -1],
            bbox: [0; 4],
            slopetype: slopetype_t::ST_HORIZONTAL,
            frontsector: None,
            backsector: None,
            validcount: 0,
            specialdata: ptr::null_mut(),
        }
    }

    /// A line listed in two blockmap cells reaches the callback once per
    /// `validcount`, which it is marked with; cells outside the grid visit
    /// nothing.
    #[test]
    fn block_lines_iterator_visits_each_line_once_per_validcount() {
        let _guard = LEVEL_TEST_LOCK.lock().unwrap();
        // 2x1 grid: header [orgx, orgy, width, height], the two cells' list
        // offsets, then cell 0 = [line 0, line 1] and cell 1 = [line 1].
        let level = LevelState {
            bmapwidth: 2,
            bmapheight: 1,
            blockmaplump: vec![0, 0, 2, 1, 6, 9, 0, 1, -1, 1, -1],
            lines: vec![line_record(); 2],
            ..LevelState::new()
        };
        let saved = std::mem::replace(&mut *LEVEL.borrow_mut(), level);
        VISITED.lock().unwrap().clear();
        VALIDCOUNT.set(VALIDCOUNT.get().wrapping_add(1));

        // SAFETY: the callback only records the line it is given.
        unsafe {
            assert_eq!(P_BlockLinesIterator(0, 0, Some(record_line)), 1);
            assert_eq!(P_BlockLinesIterator(1, 0, Some(record_line)), 1);
            assert_eq!(P_BlockLinesIterator(2, 0, Some(record_line)), 1);
        }

        assert_eq!(*VISITED.lock().unwrap(), [LineId::new(0), LineId::new(1)]);
        let validcount = VALIDCOUNT.get();
        assert!(
            LEVEL
                .borrow()
                .lines
                .iter()
                .all(|line| line.validcount == validcount)
        );
        *LEVEL.borrow_mut() = saved;
    }
}
