//! Map geometry utilities, blockmap iterators, and intercept routines.
//!
//! Rust port of `vendor/doomgeneric/p_maputl.c`. Provides the low-level
//! primitives used by `p_map.c` for collision detection, line-of-sight, and
//! hitscan/projectile tracing: approximate distance, point-on-line tests,
//! thing-position management, blockmap traversal, and the intercept pipeline
//! used by `P_PathTraverse`.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use zerocopy::IntoBytes;

use crate::engine::{
    map::{LineId, slopetype_t},
    math::fixed::{FRACBITS, FRACUNIT, FixedDiv, FixedMul, fixed_t},
    play::{info::*, mobj::MobjId, setup::LevelState, tick::MOBJ_ARENA},
    render_3d::main::R_PointInSubsector,
    world::World,
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
/// `[0, FRACUNIT]`). `hit` is what the trace crossed: C's `isaline` flag and
/// the `d` union of a line and a thing pointer.
#[derive(Clone, Copy)]
pub struct intercept_t {
    pub frac: i32,
    pub hit: InterceptHit,
}

/// What an intercept crossed (C's `intercept_t.isaline` and `d`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterceptHit {
    /// A linedef (`isaline` true, `d.line`).
    Line(LineId),
    /// A map object (`isaline` false, `d.thing`).
    Thing(MobjId),
}

impl intercept_t {
    /// C's `isaline` flag: 1 for a line, 0 for a thing.
    pub fn isaline(&self) -> i32 {
        matches!(self.hit, InterceptHit::Line(_)) as i32
    }
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
// Line-opening and traversal state
// ---------------------------------------------------------------------------

/// Line-opening results and path-traversal state (`p_maputl.c` globals),
/// kept in [`PlayScratch`](crate::engine::world::PlayScratch).
///
/// Path traversals re-enter: an intercept callback can damage or kill a map
/// object, whose actions move other objects and traverse again. Every access
/// is therefore a short one that never spans a callback, matching the C
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
    /// Write cursor into
    /// [`PlayScratch::intercepts`](crate::engine::world::PlayScratch::intercepts): the number of entries
    /// collected (C `intercept_p - intercepts`).
    ///
    /// Reset to 0 at the start of each `P_PathTraverse` call and advanced by
    /// `PIT_AddLineIntercepts` / `PIT_AddThingIntercepts`.
    /// `P_TraverseIntercepts` walks the entries below it.
    pub intercept_count: usize,
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
            intercept_count: 0,
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
/// Sets `opentop`, `openbottom`, `openrange`, and `lowfloor` of `opening`
/// based on the floor/ceiling heights of the front and back sectors.
/// If the linedef has no back side (`sidenum[1] == -1`) then only
/// `openrange` is set, to 0 (no passage).
///
/// These values are read by `p_map.c` after each crossing test to determine
/// whether a moving object fits through the gap.
///
/// Matches `P_LineOpening` in `p_maputl.c`.
///
/// # Panics
/// If the line has a back side but lacks a front or back sector (C
/// dereferences the null sector).
pub fn P_LineOpening(opening: &mut MapUtlState, level: &LevelState, linedef: LineId) {
    let linedef = &level.lines[linedef];
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
/// # Panics
/// If the thing's subsector or its list links lie outside the loaded level
/// and the mobj arena.
pub fn P_UnsetThingPosition(world: &mut World, thing_id: MobjId) {
    // The link surgery touches the thing and its neighbours; the neighbour
    // writes change only the neighbours' links (or rewrite the thing's own
    // link with its value, for a corrupt self-link), so the thing's fields
    // are read once.
    let mut mobjs = MOBJ_ARENA.borrow_mut();
    let level = &mut world.level;
    let thing = &mobjs[thing_id];
    let (flags, x, y, subsector) = (thing.flags, thing.x, thing.y, thing.subsector);
    let (snext, sprev, bnext, bprev) = (thing.snext, thing.sprev, thing.bnext, thing.bprev);
    if (flags & MF_NOSECTOR) == 0 {
        if let Some(snext) = snext {
            mobjs[snext].sprev = sprev;
        }
        if let Some(sprev) = sprev {
            mobjs[sprev].snext = snext;
        } else {
            let sector = level.subsectors[subsector].sector;
            level.sectors[sector].thinglist = snext;
        }
    }
    if (flags & MF_NOBLOCKMAP) == 0 {
        if let Some(bnext) = bnext {
            mobjs[bnext].bprev = bprev;
        }
        if let Some(bprev) = bprev {
            mobjs[bprev].bnext = bnext;
        } else {
            let blockx = (x - level.bmaporgx) >> MAPBLOCKSHIFT;
            let blocky = (y - level.bmaporgy) >> MAPBLOCKSHIFT;
            if blockx >= 0 && blockx < level.bmapwidth && blocky >= 0 && blocky < level.bmapheight {
                let index = (blocky * level.bmapwidth + blockx) as usize;
                level.blocklinks[index] = bnext;
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
/// # Panics
/// If no level is loaded or the heads of the new lists lie outside the mobj
/// arena.
pub fn P_SetThingPosition(world: &mut World, thing_id: MobjId) {
    // The writes go through the arena in C's order, so a thing that already
    // heads its new list links to itself as it does in C.
    let mut mobjs = MOBJ_ARENA.borrow_mut();
    let level = &mut world.level;
    let (x, y, flags) = {
        let thing = &mobjs[thing_id];
        (thing.x, thing.y, thing.flags)
    };
    let ss = R_PointInSubsector(level, x, y);
    mobjs[thing_id].subsector = ss;
    if (flags & MF_NOSECTOR) == 0 {
        let sector = level.subsectors[ss].sector;
        let sec = &mut level.sectors[sector];
        let thing = &mut mobjs[thing_id];
        thing.sprev = None;
        thing.snext = sec.thinglist;
        if let Some(thinglist) = sec.thinglist {
            mobjs[thinglist].sprev = Some(thing_id);
        }
        sec.thinglist = Some(thing_id);
    }
    if (flags & MF_NOBLOCKMAP) == 0 {
        let blockx = (x - level.bmaporgx) >> MAPBLOCKSHIFT;
        let blocky = (y - level.bmaporgy) >> MAPBLOCKSHIFT;
        let thing = &mut mobjs[thing_id];
        if blockx >= 0 && blockx < level.bmapwidth && blocky >= 0 && blocky < level.bmapheight {
            let index = (blocky * level.bmapwidth + blockx) as usize;
            let link = &mut level.blocklinks[index];
            thing.bprev = None;
            thing.bnext = *link;
            if let Some(head) = *link {
                mobjs[head].bprev = Some(thing_id);
            }
            *link = Some(thing_id);
        } else {
            thing.bnext = None;
            thing.bprev = None;
        }
    }
}

// ---------------------------------------------------------------------------
// P_BlockLinesIterator
// ---------------------------------------------------------------------------

/// Iterate all linedefs in blockmap cell `(x, y)`, calling `func` for each.
///
/// Returns `true` if `func` returned `true` for every line, `false` if it
/// returned `false` for any line (early-out). Lines marked with the current
/// `validcount` are skipped to avoid processing the same line twice when it
/// spans multiple blockmap cells.
///
/// Out-of-bounds cell coordinates return `true` without calling `func`.
///
/// Matches `P_BlockLinesIterator` in `p_maputl.c`.
///
/// # Panics
/// If no level is loaded (the cell's list lies outside the blockmap lump).
pub fn P_BlockLinesIterator(
    world: &mut World,
    x: i32,
    y: i32,
    mut func: impl FnMut(&mut World, LineId) -> bool,
) -> bool {
    // The callback can start a nested traversal whose intercept overrun
    // rewrites the blockmap dimensions; like C, this cell's list was located
    // before any callback ran. The callback takes the world, so each line is
    // read and marked under a borrow that ends before the call.
    let mut list = {
        let level = &world.level;
        if x < 0 || y < 0 || x >= level.bmapwidth || y >= level.bmapheight {
            return true;
        }
        level.blockmaplump[4 + (y * level.bmapwidth + x) as usize] as usize
    };
    loop {
        let ld = {
            let World { level, scratch, .. } = &mut *world;
            let linenum = level.blockmaplump[list];
            if linenum == -1 {
                return true;
            }
            list += 1;
            let validcount = scratch.validcount;
            let line = &mut level.lines[linenum as usize];
            if line.validcount == validcount {
                continue;
            }
            line.validcount = validcount;
            LineId::new(linenum as usize)
        };
        if !func(world, ld) {
            return false;
        }
    }
}

// ---------------------------------------------------------------------------
// P_BlockThingsIterator
// ---------------------------------------------------------------------------

/// Iterate all map objects in blockmap cell `(x, y)`, calling `func` for each.
///
/// Returns `true` if `func` returned `true` for every thing, `false` if it
/// returned `false` for any thing (early-out). Out-of-bounds cell
/// coordinates return `true` without calling `func`.
///
/// Matches `P_BlockThingsIterator` in `p_maputl.c`.
///
/// The next link is read from each object after `func` has run on it,
/// freed or not.
///
/// # Panics
/// If the cell's `blocklinks` chain names an object outside the mobj arena.
pub fn P_BlockThingsIterator(
    world: &mut World,
    x: i32,
    y: i32,
    mut func: impl FnMut(&mut World, MobjId) -> bool,
) -> bool {
    let mut mobj = {
        let level = &world.level;
        if x < 0 || y < 0 || x >= level.bmapwidth || y >= level.bmapheight {
            return true;
        }
        level.blocklinks[(y * level.bmapwidth + x) as usize]
    };
    while let Some(current) = mobj {
        if !func(world, current) {
            return false;
        }
        // Read after the callback, as C does: it may have moved or
        // removed the object, whose links still name its old neighbour.
        mobj = MOBJ_ARENA.borrow()[current].bnext;
    }
    true
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
/// Each arm of the match below is one variable of the original layout, by
/// its byte range after the array; variables this port does not emulate
/// (their writes would be lost or harmless) are listed as skipped.
///
/// Based on PrBoom-plus research by Andrey Budko (entryway).
///
/// The overrun writes the play scratch, the blockmap dimensions and the
/// player starts, all through `world`.
fn InterceptsMemoryOverrun(world: &mut World, location: i32, value: i32) {
    match location {
        // 0..12: an unnamed word, `earlyout`, `intercept_p` (skipped).
        12..16 => world.scratch.maputl.lowfloor = value,
        16..20 => world.scratch.maputl.openbottom = value,
        20..24 => world.scratch.maputl.opentop = value,
        24..28 => world.scratch.maputl.openrange = value,
        // 28..160: an unnamed word, `activeplats`, two unnamed words
        // (skipped).
        160..164 => world.scratch.pspr.bulletslope = value,
        // 164..176: `swingx`, `swingy`, an unnamed word (skipped).
        176..216 => {
            // `playerstarts`, an array of `short`: the value lands in two
            // consecutive shorts, low half first.
            let shorts = world.level.playerstarts.as_mut_bytes();
            let offset = (location - 176) as usize / 2 * 2;
            shorts[offset..offset + 2].copy_from_slice(&((value & 0xffff) as i16).to_le_bytes());
            shorts[offset + 2..offset + 4]
                .copy_from_slice(&(((value >> 16) & 0xffff) as i16).to_le_bytes());
        }
        // 216..220: `blocklinks` (skipped).
        220..224 => world.level.bmapwidth = value,
        // 224..228: `blockmap` (skipped).
        228..232 => world.level.bmaporgx = value,
        232..236 => world.level.bmaporgy = value,
        // 236..240: `blockmaplump` (skipped).
        240..244 => world.level.bmapheight = value,
        _ => {}
    }
}

/// Emulate the vanilla Doom `intercepts[]` buffer overrun for demo fidelity.
///
/// When `num_intercepts` exceeds `MAXINTERCEPTS_ORIGINAL` (128), the original
/// engine would write past the array into adjacent globals. This function
/// calls `InterceptsMemoryOverrun` three times (for `frac`, `isaline`, and
/// `d`) to reproduce those writes. `d` is the address C stores: the thing's,
/// or for a line the address of its record in the level's `lines`.
fn InterceptsOverrun(world: &mut World, num_intercepts: usize, intercept: intercept_t) {
    if num_intercepts <= MAXINTERCEPTS_ORIGINAL {
        return;
    }
    let location = (num_intercepts - MAXINTERCEPTS_ORIGINAL - 1) as i32 * 12;
    let address = match intercept.hit {
        InterceptHit::Line(line) => (&raw const world.level.lines[line]).addr(),
        InterceptHit::Thing(thing) => thing.address(),
    };
    InterceptsMemoryOverrun(world, location, intercept.frac);
    InterceptsMemoryOverrun(world, location + 4, intercept.isaline());
    InterceptsMemoryOverrun(world, location + 8, address as i32);
}

/// Append `intercept` to the traversal's intercepts, emulating the vanilla
/// overrun once the original array is full (the `PIT_Add*` epilogue).
///
/// # Panics
/// If the extended array is full too (C writes past it).
fn add_intercept(world: &mut World, intercept: intercept_t) {
    let index = world.scratch.maputl.intercept_count;
    world.scratch.intercepts[index] = intercept;
    InterceptsOverrun(world, index, intercept);
    world.scratch.maputl.intercept_count = index + 1;
}

// ---------------------------------------------------------------------------
// PIT_AddLineIntercepts
// ---------------------------------------------------------------------------

/// Intercept callback: record where the current `trace` ray crosses linedef `ld`.
///
/// Called by `P_BlockLinesIterator` during `P_PathTraverse`. If the ray
/// crosses `ld` (endpoints on opposite sides) and the intersection fraction
/// `t >= 0`, appends an `intercept_t` to `intercepts` and advances
/// `intercept_count`. Uses `P_PointOnDivlineSide` for long traces and
/// `P_PointOnLineSide` for short ones to avoid fixed-point precision issues.
///
/// If `PT_EARLYOUT` is set and the line has no back sector, returns `false`
/// to halt traversal immediately.
///
/// Returns `true` (continue) or `false` (stop).
///
/// `ld` must be a line of the loaded level, and `trace`, the intercepts and
/// their count must be set up by a preceding `P_PathTraverse` call.
///
/// Matches `PIT_AddLineIntercepts` in `p_maputl.c`.
pub fn PIT_AddLineIntercepts(world: &mut World, ld: LineId) -> bool {
    let (trace, earlyout) = (world.scratch.maputl.trace, world.scratch.maputl.earlyout);
    let (frac, backsector) = {
        let level = &world.level;
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
                P_PointOnLineSide(level, trace.x, trace.y, ld),
                P_PointOnLineSide(level, trace.x + trace.dx, trace.y + trace.dy, ld),
            )
        };
        if s1 == s2 {
            return true;
        }
        let mut dl = divline_t {
            x: 0,
            y: 0,
            dx: 0,
            dy: 0,
        };
        P_MakeDivline(level, ld, &mut dl);
        (P_InterceptVector(&trace, &dl), line.backsector)
    };
    if frac < 0 {
        return true;
    }
    if earlyout != 0 && frac < FRACUNIT && backsector.is_none() {
        return false;
    }
    add_intercept(
        world,
        intercept_t {
            frac,
            hit: InterceptHit::Line(ld),
        },
    );
    true
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
/// `isaline = 0`) and advances `intercept_count`.
///
/// Returns `true` (continue) always.
///
/// `trace`, the intercepts and their count must be set up by a preceding
/// `P_PathTraverse` call.
///
/// Matches `PIT_AddThingIntercepts` in `p_maputl.c`.
pub fn PIT_AddThingIntercepts(world: &mut World, thing_id: MobjId) -> bool {
    let (x, y, radius) = {
        let mobjs = MOBJ_ARENA.borrow();
        let thing = &mobjs[thing_id];
        (thing.x, thing.y, thing.radius)
    };
    let trace = world.scratch.maputl.trace;
    // C: tracepositive = (trace.dx ^ trace.dy)>0;
    let tracepositive = (trace.dx ^ trace.dy) > 0;
    let (x1, y1, x2, y2) = if tracepositive {
        (x - radius, y + radius, x + radius, y - radius)
    } else {
        (x - radius, y - radius, x + radius, y + radius)
    };
    let s1 = P_PointOnDivlineSide(x1, y1, &trace);
    let s2 = P_PointOnDivlineSide(x2, y2, &trace);
    if s1 == s2 {
        return true;
    }
    let dl = divline_t {
        x: x1,
        y: y1,
        dx: x2 - x1,
        dy: y2 - y1,
    };
    let frac = P_InterceptVector(&trace, &dl);
    if frac < 0 {
        return true;
    }
    add_intercept(
        world,
        intercept_t {
            frac,
            hit: InterceptHit::Thing(thing_id),
        },
    );
    true
}

// ---------------------------------------------------------------------------
// P_TraverseIntercepts
// ---------------------------------------------------------------------------

/// Walk the collected intercepts in nearest-first order, calling `func` for each.
///
/// After `P_PathTraverse` has filled the first `intercept_count`
/// intercepts, this function iterates them in ascending `frac` order
/// (selection sort: picks the first minimum each pass, marks it with
/// `i32::MAX` after processing). Stops when the nearest remaining intercept
/// has `frac > maxfrac`. `func` gets a copy of the intercept; a nested
/// traversal it starts refills the array, as in C.
///
/// Returns `true` if `func` accepted every intercept, `false` if `func`
/// returned `false` for any intercept.
///
/// Matches `P_TraverseIntercepts` in `p_maputl.c`.
pub fn P_TraverseIntercepts(
    world: &mut World,
    mut func: impl FnMut(&mut World, intercept_t) -> bool,
    maxfrac: fixed_t,
) -> bool {
    // A callback can start a nested traversal that refills the array and
    // moves the cursor; like C, each scan reads the cursor afresh.
    let count = world.scratch.maputl.intercept_count;
    for _ in 0..count {
        let mut dist = i32::MAX;
        let mut nearest = 0;
        let collected = world.scratch.maputl.intercept_count;
        for (index, intercept) in world.scratch.intercepts[..collected].iter().enumerate() {
            if intercept.frac < dist {
                dist = intercept.frac;
                nearest = index;
            }
        }
        if dist > maxfrac {
            return true;
        }
        if !func(world, world.scratch.intercepts[nearest]) {
            return false;
        }
        world.scratch.intercepts[nearest].frac = i32::MAX;
    }
    true
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
/// Returns `true` if the traverser accepted all intercepts, `false` if it
/// rejected one (signalling a hit / early stop).
///
/// All coordinates are in fixed-point map units.
///
/// Matches `P_PathTraverse` in `p_maputl.c`.
///
/// # Panics
/// If no level is loaded (see `P_BlockThingsIterator`).
pub fn P_PathTraverse(
    world: &mut World,
    x1: fixed_t,
    y1: fixed_t,
    x2: fixed_t,
    y2: fixed_t,
    flags: i32,
    trav: impl FnMut(&mut World, intercept_t) -> bool,
) -> bool {
    world.scratch.validcount = world.scratch.validcount.wrapping_add(1);
    let (bmaporgx, bmaporgy) = world.level.blockmap_origin();
    let mut x1 = x1;
    let mut y1 = y1;
    if ((x1 - bmaporgx) & (MAPBLOCKSIZE - 1)) == 0 {
        x1 += FRACUNIT;
    }
    if ((y1 - bmaporgy) & (MAPBLOCKSIZE - 1)) == 0 {
        y1 += FRACUNIT;
    }
    {
        let maputl = &mut world.scratch.maputl;
        maputl.earlyout = flags & PT_EARLYOUT;
        maputl.intercept_count = 0;
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
            && !P_BlockLinesIterator(world, mapx, mapy, PIT_AddLineIntercepts)
        {
            return false;
        }
        if flags & PT_ADDTHINGS != 0
            && !P_BlockThingsIterator(world, mapx, mapy, PIT_AddThingIntercepts)
        {
            return false;
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
    P_TraverseIntercepts(world, trav, FRACUNIT)
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::engine::{
            map::{VertexId, line_t},
            play::{
                mobj::mobj_t,
                tick::{P_InitThinkers, THINKER_TEST_LOCK, new_thinker},
            },
        },
    };

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
            specialdata: None,
        }
    }

    /// A line listed in two blockmap cells reaches the callback once per
    /// `validcount`, which it is marked with; cells outside the grid visit
    /// nothing.
    #[test]
    fn block_lines_iterator_visits_each_line_once_per_validcount() {
        // 2x1 grid: header [orgx, orgy, width, height], the two cells' list
        // offsets, then cell 0 = [line 0, line 1] and cell 1 = [line 1].
        let mut world = World::new();
        world.level = LevelState {
            bmapwidth: 2,
            bmapheight: 1,
            blockmaplump: vec![0, 0, 2, 1, 6, 9, 0, 1, -1, 1, -1],
            lines: vec![line_record(); 2],
            ..LevelState::new()
        };
        world.scratch.validcount += 1;
        let mut visited = Vec::new();
        for x in 0..3 {
            assert!(P_BlockLinesIterator(&mut world, x, 0, |_, line| {
                visited.push(line);
                true
            }));
        }

        assert_eq!(visited, [LineId::new(0), LineId::new(1)]);
        let validcount = world.scratch.validcount;
        assert!(
            world
                .level
                .lines
                .iter()
                .all(|line| line.validcount == validcount)
        );
    }

    /// A thing removed by the callback during a blockmap walk (unlinked and
    /// freed) keeps its `bnext`, so the walk continues with the thing after
    /// it, as C does; the list itself no longer holds the removed thing.
    #[test]
    fn block_things_iterator_continues_after_removal_via_bnext() {
        let _thinker_guard = THINKER_TEST_LOCK.lock().unwrap();
        P_InitThinkers();

        // Three things at (0, 0) chained in the one cell of a 1x1 blockmap,
        // kept out of the sector lists, which the test level does not have.
        let things: Vec<MobjId> = (0..3).map(|_| new_thinker::<mobj_t>()).collect();
        {
            let mut mobjs = MOBJ_ARENA.borrow_mut();
            for (index, &thing) in things.iter().enumerate() {
                let mobj = &mut mobjs[thing];
                mobj.flags = MF_NOSECTOR;
                mobj.bprev = index.checked_sub(1).map(|previous| things[previous]);
                mobj.bnext = things.get(index + 1).copied();
            }
        }
        let mut world = World::new();
        world.level = LevelState {
            bmapwidth: 1,
            bmapheight: 1,
            blocklinks: vec![Some(things[0])],
            ..LevelState::new()
        };
        let mut visited = Vec::new();
        // The removed thing stays readable through its stale handle.
        let completed = P_BlockThingsIterator(&mut world, 0, 0, |world, thing| {
            visited.push(thing);
            if thing == things[1] {
                P_UnsetThingPosition(world, thing);
                MOBJ_ARENA.borrow_mut().free(thing);
            }
            true
        });

        assert!(completed);
        assert_eq!(visited, things);
        assert_eq!(MOBJ_ARENA.borrow()[things[0]].bnext, Some(things[2]));
        assert_eq!(MOBJ_ARENA.borrow()[things[2]].bprev, Some(things[0]));

        // A callback returning `false` stops the walk at that thing.
        let mut visited = Vec::new();
        let completed = P_BlockThingsIterator(&mut world, 0, 0, |_, thing| {
            visited.push(thing);
            false
        });
        assert!(!completed);
        assert_eq!(visited, [things[0]]);

        P_InitThinkers();
    }

    /// The collected intercepts reach the callback nearest first, by value,
    /// with ties in collection order; each is marked done after its call,
    /// and those beyond `maxfrac` are left alone.
    #[test]
    fn traverse_intercepts_keeps_the_nearest_first_order() {
        let mut world = World::new();
        let fracs = [
            FRACUNIT / 2,
            FRACUNIT / 4,
            FRACUNIT / 4,
            2 * FRACUNIT,
            FRACUNIT,
        ];
        for (index, &frac) in fracs.iter().enumerate() {
            world.scratch.intercepts[index] = intercept_t {
                frac,
                hit: InterceptHit::Line(LineId::new(index)),
            };
        }
        world.scratch.maputl.intercept_count = fracs.len();

        let mut received = Vec::new();
        assert!(P_TraverseIntercepts(
            &mut world,
            |_, intercept| {
                received.push((intercept.hit, intercept.frac));
                true
            },
            FRACUNIT,
        ));

        let line = |index| InterceptHit::Line(LineId::new(index));
        assert_eq!(
            received,
            [
                (line(1), FRACUNIT / 4),
                (line(2), FRACUNIT / 4),
                (line(0), FRACUNIT / 2),
                (line(4), FRACUNIT),
            ]
        );
        let fracs_after: Vec<i32> = world.scratch.intercepts[..fracs.len()]
            .iter()
            .map(|intercept| intercept.frac)
            .collect();
        assert_eq!(
            fracs_after,
            [i32::MAX, i32::MAX, i32::MAX, 2 * FRACUNIT, i32::MAX]
        );
    }

    /// Past the original 128 intercepts, each added intercept also writes
    /// its `frac`, `isaline` and address words into the emulated variables
    /// after the vanilla array: the 131st lands on the line opening.
    #[test]
    fn intercepts_overrun_writes_the_emulated_variables() {
        let _thinker_guard = THINKER_TEST_LOCK.lock().unwrap();
        let mut world = World::new();
        world.scratch.maputl.intercept_count = MAXINTERCEPTS_ORIGINAL + 2;
        let thing = new_thinker::<mobj_t>();
        add_intercept(
            &mut world,
            intercept_t {
                frac: 0x1234,
                hit: InterceptHit::Thing(thing),
            },
        );
        // Entry 130 starts at byte 12: `lowfloor`, `openbottom`, `opentop`.
        assert_eq!(world.scratch.maputl.lowfloor, 0x1234);
        assert_eq!(world.scratch.maputl.openbottom, 0);
        assert_eq!(world.scratch.maputl.opentop, thing.address() as i32);
        assert_eq!(
            world.scratch.maputl.intercept_count,
            MAXINTERCEPTS_ORIGINAL + 3
        );
        assert_eq!(
            world.scratch.intercepts[MAXINTERCEPTS_ORIGINAL + 2].frac,
            0x1234
        );

        // Entry 142 starts at byte 156: its `isaline` word is `bulletslope`.
        world.scratch.pspr.bulletslope = 99;
        world.scratch.maputl.intercept_count = MAXINTERCEPTS_ORIGINAL + 14;
        add_intercept(
            &mut world,
            intercept_t {
                frac: 7,
                hit: InterceptHit::Thing(thing),
            },
        );
        assert_eq!(world.scratch.pspr.bulletslope, 0);
    }
}
