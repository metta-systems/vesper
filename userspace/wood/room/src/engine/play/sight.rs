//! Line-of-sight checks between map objects.
//!
//! Rust port of `vendor/doomgeneric/p_sight.c`. Implements `P_CheckSight`
//! using a REJECT table fast-path followed by BSP-tree traversal and
//! per-segment portal testing.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::i_error;

use crate::engine::math::fixed::{FixedDiv, FixedMul};

use crate::engine::math::fixed::FRACBITS;

use crate::engine::{
    map::{NodeId, node_t},
    play::{maputl::divline_t, mobj::MobjId},
    wad::format::LinedefFlag,
    world::World,
};

/// BSP node flag: child index has this bit set when the child is a subsector
/// (leaf) rather than an internal node. Matches `NF_SUBSECTOR` in `p_local.h`.
const NF_SUBSECTOR: u32 = 0x8000;

/// Sight-check state (`p_sight.c` globals).
///
/// A sight check calls nothing that re-enters play code, so `P_CheckSight`
/// borrows this once and hands it down the BSP traversal.
pub struct SightState {
    /// Z-coordinate of the looker's eyes (fixed-point map units).
    ///
    /// Set by `P_CheckSight` to `t1->z + t1->height - (t1->height >> 2)` before
    /// BSP traversal. Read by `P_CrossSubsector` for slope comparisons.
    pub sightzstart: i32,
    /// Upper slope bound for the sight corridor (fixed-point).
    ///
    /// Initialised to the slope from `sightzstart` to the top of the target.
    /// Narrowed downward as occluding portals are encountered. `p_map.c`
    /// shares it as the auto-aim window.
    pub topslope: i32,
    /// Lower slope bound for the sight corridor (fixed-point).
    ///
    /// Initialised to the slope from `sightzstart` to the bottom of the target.
    /// Narrowed upward as occluding portals are encountered. `p_map.c`
    /// shares it as the auto-aim window.
    pub bottomslope: i32,
    /// The sight-trace ray from `t1` to `t2`, stored as a `divline_t`.
    ///
    /// Set by `P_CheckSight` and used throughout the BSP traversal in
    /// `P_CrossBSPNode` and `P_CrossSubsector`.
    pub strace: divline_t,
    /// X coordinate of the sight target (`t2->x`).
    ///
    /// Stored separately from `strace` so that `P_CrossBSPNode` can test which
    /// side of each partition the target falls on without a full ray struct.
    pub t2x: i32,
    /// Y coordinate of the sight target (`t2->y`).
    ///
    /// See `t2x`.
    pub t2y: i32,
    /// Diagnostic counters: `sightcounts[0]` is REJECT table hits (early-out),
    /// `sightcounts[1]` is full BSP traversals attempted.
    pub sightcounts: [i32; 2],
}

impl SightState {
    /// The state at program start: no sight check run.
    pub const fn new() -> Self {
        Self {
            sightzstart: 0,
            topslope: 0,
            bottomslope: 0,
            strace: divline_t {
                x: 0,
                y: 0,
                dx: 0,
                dy: 0,
            },
            t2x: 0,
            t2y: 0,
            sightcounts: [0, 0],
        }
    }
}

impl Default for SightState {
    fn default() -> Self {
        Self::new()
    }
}

/// Sight-check state.
pub static SIGHT: Global<SightState> = Global::new(SightState::new());

use crate::engine::{
    global::Global,
    play::setup::{LEVEL, LevelState},
    render_3d::main::VALIDCOUNT,
};

/// Determine which half-space of a `divline_t` a point lies in.
///
/// Returns 0 (front side), 1 (back side), or 2 (on the line).
///
/// Matches `P_DivlineSide` in `p_sight.c`. The `dy == 0` fast path contains
/// a deliberate quirk inherited from the original Doom source: it tests
/// `x == node.y` rather than `y == node.y`. This is intentional for
/// demo-compatible determinism; do not correct it.
fn P_DivlineSide(x: i32, y: i32, node: &divline_t) -> i32 {
    if node.dx == 0 {
        if x == node.x {
            return 2;
        }
        return if x <= node.x {
            (node.dy > 0) as i32
        } else {
            (node.dy < 0) as i32
        };
    }

    if node.dy == 0 {
        // NOTE: The original C code reads `if (x==node->y)` here — NOT
        // `y==node->y`. This is a long-standing quirk in the Doom source
        // that demos rely on for deterministic playback. DO NOT "fix" it.
        if x == node.y {
            return 2;
        }
        return if y <= node.y {
            (node.dx < 0) as i32
        } else {
            (node.dx > 0) as i32
        };
    }

    let dx = x - node.x;
    let dy = y - node.y;

    let left = (node.dy >> FRACBITS) * (dx >> FRACBITS);
    let right = (dy >> FRACBITS) * (node.dx >> FRACBITS);

    if right < left {
        return 0; // front side
    }
    if left == right {
        return 2;
    }
    1 // back side
}

/// Returns the fractional intercept point along the first divline.
///
/// Computes the `t`-parameter where ray `v2` intersects line `v1` using
/// fixed-point arithmetic (8-bit pre-shift to avoid overflow). Returns 0
/// when the lines are parallel (`den == 0`).
///
/// Matches `P_InterceptVector2` in `p_sight.c` (the sight-local variant; the
/// publicly exported version lives in `p_maputl.rs` as `P_InterceptVector`).
fn P_InterceptVector2(v2: &divline_t, v1: &divline_t) -> i32 {
    let den = FixedMul(v1.dy >> 8, v2.dx) - FixedMul(v1.dx >> 8, v2.dy);

    if den == 0 {
        return 0;
    }

    let num = FixedMul((v1.x - v2.x) >> 8, v1.dy) + FixedMul((v2.y - v1.y) >> 8, v1.dx);

    FixedDiv(num, den)
}

/// Test whether the sight ray (`sight.strace`) passes through subsector `num`
/// without being blocked.
///
/// Iterates every seg in the subsector. For each linedef crossed by the sight
/// ray, checks whether the portal is tall enough to let the ray through given
/// the current `topslope` / `bottomslope` bounds. Narrows those bounds when a
/// two-sided line is partially occluding.
///
/// Returns `true` if the ray is unobstructed through this subsector.
///
/// Matches `P_CrossSubsector` in `p_sight.c`.
fn P_CrossSubsector(sight: &mut SightState, level: &mut LevelState, num: i32) -> bool {
    {
        let strace_copy = sight.strace;
        let (t2x, t2y) = (sight.t2x, sight.t2y);
        if num as usize >= level.subsectors.len() {
            i_error!(
                "P_CrossSubsector: ss {} with numss = {}",
                num,
                level.subsectors.len()
            );
        }

        let sub = level.subsectors[num as usize];
        let firstline = sub.firstline as usize;
        let validcount = VALIDCOUNT.get();

        for seg in &level.segs[firstline..firstline + sub.numlines.max(0) as usize] {
            let line = &mut level.lines[seg.linedef];

            // Already checked other side?
            if line.validcount == validcount {
                continue;
            }

            line.validcount = validcount;

            let v1 = &level.vertexes[line.v1];
            let v2 = &level.vertexes[line.v2];

            let s1 = P_DivlineSide(v1.x, v1.y, &strace_copy);
            let s2 = P_DivlineSide(v2.x, v2.y, &strace_copy);

            // Line isn't crossed?
            if s1 == s2 {
                continue;
            }

            let divl = divline_t {
                x: v1.x,
                y: v1.y,
                dx: v2.x - v1.x,
                dy: v2.y - v1.y,
            };

            let s1 = P_DivlineSide(strace_copy.x, strace_copy.y, &divl);
            let s2 = P_DivlineSide(t2x, t2y, &divl);

            // Line isn't crossed?
            if s1 == s2 {
                continue;
            }

            // Backsector may be NULL if this is an "impassible glass" hack line.
            // IMPORTANT: C uses line->backsector (the original linedef side), NOT
            // seg->backsector (which is the BSP-split sub-side and may differ).
            if line.backsector.is_none() {
                return false;
            }

            // Stop because it is not two sided anyway.
            // Also must use line->flags, not a re-read through seg->linedef.
            if line.flags & LinedefFlag::TWOSIDED as i16 == 0 {
                return false;
            }

            let front = &level.sectors[seg.frontsector];
            let back = &level.sectors[seg.backsector.expect("two-sided seg without back sector")];

            let front_floor = front.floorheight;
            let front_ceil = front.ceilingheight;
            let back_floor = back.floorheight;
            let back_ceil = back.ceilingheight;

            // No wall to block sight with?
            if front_floor == back_floor && front_ceil == back_ceil {
                continue;
            }

            // Possible occluder.
            let opentop = if front_ceil < back_ceil {
                front_ceil
            } else {
                back_ceil
            };

            let openbottom = if front_floor > back_floor {
                front_floor
            } else {
                back_floor
            };

            // Quick test for totally closed doors.
            if openbottom >= opentop {
                return false;
            }

            let frac = P_InterceptVector2(&strace_copy, &divl);

            if front_floor != back_floor {
                let slope = FixedDiv(openbottom - sight.sightzstart, frac);
                if slope > sight.bottomslope {
                    sight.bottomslope = slope;
                }
            }

            if front_ceil != back_ceil {
                let slope = FixedDiv(opentop - sight.sightzstart, frac);
                if slope < sight.topslope {
                    sight.topslope = slope;
                }
            }

            if sight.topslope <= sight.bottomslope {
                return false;
            }
        }

        true
    }
}

/// Extract the first 4 fields of a node_t as a divline_t.
/// The C code casts `(divline_t*)node` for P_DivlineSide calls;
/// since both structs share the same header layout (x, y, dx, dy),
/// this produces identical results.
#[inline]
fn node_as_divline(node: &node_t) -> divline_t {
    divline_t {
        x: node.x,
        y: node.y,
        dx: node.dx,
        dy: node.dy,
    }
}

/// Recursively cross a BSP node (or leaf subsector) to test sight.
///
/// Determines which side of the partition the trace origin lies on, crosses
/// that subtree first, then crosses the other side only if the trace endpoint
/// is on a different side of the partition.
///
/// Returns `true` if the sight ray is unobstructed through `bspnum`.
///
/// Matches `P_CrossBSPNode` in `p_sight.c`.
fn P_CrossBSPNode(sight: &mut SightState, level: &mut LevelState, bspnum: i32) -> bool {
    {
        let strace_copy = sight.strace;
        if bspnum & NF_SUBSECTOR as i32 != 0 {
            if bspnum == -1 {
                return P_CrossSubsector(sight, level, 0);
            } else {
                return P_CrossSubsector(sight, level, bspnum & !(NF_SUBSECTOR as i32));
            }
        }

        let bsp = level.nodes[NodeId::new(bspnum as usize)];
        let bsp = &bsp;
        let bsp_div = node_as_divline(bsp);

        let side = P_DivlineSide(strace_copy.x, strace_copy.y, &bsp_div);
        let side = if side == 2 { 0 } else { side };

        // Cross the starting side.
        if !P_CrossBSPNode(sight, level, bsp.children[side as usize] as i32) {
            return false;
        }

        // The partition plane is crossed here.
        let bsp_div2 = node_as_divline(bsp);
        let t2_side = P_DivlineSide(sight.t2x, sight.t2y, &bsp_div2);
        if side == t2_side {
            return true;
        }

        // Cross the ending side.
        P_CrossBSPNode(sight, level, bsp.children[(side ^ 1) as usize] as i32)
    }
}

/// Test whether there is an unobstructed line of sight between two map objects.
///
/// Performs a two-stage check:
/// 1. REJECT table lookup: if the sector pair is flagged as mutually invisible,
///    return 0 immediately (no BSP traversal needed).
/// 2. Full BSP traversal via `P_CrossBSPNode`, narrowing `topslope` /
///    `bottomslope` at each two-sided portal until the ray is either confirmed
///    clear or blocked.
///
/// Returns a C `boolean` (`i32`, non-zero = visible) rather than Rust `bool`
/// because Doom's `typedef unsigned int boolean` makes the C caller read a
/// 32-bit value. Returning `bool` (1 byte) would leave the upper 24 bits
/// undefined and cause sight checks to randomly succeed or fail, desynchronising
/// demos and RNG.
///
/// Matches `P_CheckSight` in `p_sight.c`.
///
/// The check marks the linedefs it crosses, so it borrows the level mutably
/// for its whole run; it calls no other play code.
///
/// # Safety
/// Both `t1` and `t2` must name initialised map objects whose `subsector`
/// belongs to the loaded level.
pub unsafe fn P_CheckSight(_world: &mut World, t1: MobjId, t2: MobjId) -> i32 {
    let (t1, t2) = (t1.ptr(), t2.ptr());
    let mut level = LEVEL.borrow_mut();
    let mut sight = SIGHT.borrow_mut();
    unsafe {
        let s1 = level.subsectors[(*t1).subsector].sector.index() as i32;
        let s2 = level.subsectors[(*t2).subsector].sector.index() as i32;
        let pnum = s1 * level.numsectors + s2;
        let bytenum = pnum >> 3;
        let bitnum = 1 << (pnum & 7);

        // Check in REJECT table.
        if level.rejectmatrix[bytenum as usize] & bitnum != 0 {
            sight.sightcounts[0] += 1;
            return 0;
        }

        sight.sightcounts[1] += 1;
        VALIDCOUNT.set(VALIDCOUNT.get() + 1);

        sight.sightzstart = (*t1).z + (*t1).height - ((*t1).height >> 2);
        sight.topslope = (*t2).z + (*t2).height - sight.sightzstart;
        sight.bottomslope = (*t2).z - sight.sightzstart;

        sight.strace.x = (*t1).x;
        sight.strace.y = (*t1).y;
        sight.t2x = (*t2).x;
        sight.t2y = (*t2).y;
        sight.strace.dx = (*t2).x - (*t1).x;
        sight.strace.dy = (*t2).y - (*t1).y;

        let root_node = level.nodes.len() as i32 - 1;
        P_CrossBSPNode(&mut sight, &mut level, root_node) as i32
    }
}
