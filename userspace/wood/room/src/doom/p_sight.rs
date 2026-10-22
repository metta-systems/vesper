//! Line-of-sight checks between map objects.
//!
//! Rust port of `vendor/doomgeneric/p_sight.c`. Implements `P_CheckSight`
//! using a REJECT table fast-path followed by BSP-tree traversal and
//! per-segment portal testing.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::i_error;
use std::os::raw::c_int;

use crate::doom::m_fixed::{FixedDiv, FixedMul};

use crate::doom::m_fixed::FRACBITS;

use crate::doom::c_ffi::LinedefFlag;

/// BSP node flag: child index has this bit set when the child is a subsector
/// (leaf) rather than an internal node. Matches `NF_SUBSECTOR` in `p_local.h`.
const NF_SUBSECTOR: u32 = 0x8000;

/// A ray origin and direction in fixed-point map coordinates.
///
/// Used as the primitive for BSP-side tests and intercept computations.
/// The first four fields (`x`, `y`, `dx`, `dy`) share the same layout as the
/// first four fields of `node_t`, allowing C code to cast `node_t*` to
/// `divline_t*` for `P_DivlineSide` calls.
///
/// Corresponds to `divline_t` in `p_local.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct divline_t {
    /// X coordinate of the ray origin (fixed-point map units).
    pub x: c_int,
    /// Y coordinate of the ray origin (fixed-point map units).
    pub y: c_int,
    /// X component of the ray direction (fixed-point).
    pub dx: c_int,
    /// Y component of the ray direction (fixed-point).
    pub dy: c_int,
}

/// A two-dimensional vertex in fixed-point map coordinates.
///
/// Corresponds to `vertex_t` in `r_defs.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct vertex_t {
    /// X coordinate (fixed-point map units).
    pub x: c_int,
    /// Y coordinate (fixed-point map units).
    pub y: c_int,
}

/// A BSP leaf node grouping a contiguous run of segs.
///
/// Corresponds to `subsector_t` in `r_defs.h`. The 4-byte pad after
/// `firstline` matches the C struct's natural alignment on 64-bit targets.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct subsector_t {
    /// The sector this subsector belongs to.
    pub sector: *mut sector_t,
    /// Number of segs in this subsector.
    pub numlines: i16,
    /// Index of the first seg in the global `segs` array.
    pub firstline: i16,
    _pad: [u8; 4],
}

/// A BSP line segment (half-edge) within a subsector.
///
/// Corresponds to `seg_t` in `r_defs.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct seg_t {
    /// Start vertex of this segment.
    pub v1: *mut vertex_t,
    /// End vertex of this segment.
    pub v2: *mut vertex_t,
    /// Texture offset along the linedef (fixed-point).
    pub offset: c_int,
    /// Angle of this segment (binary angle measurement).
    pub angle: u32,
    /// Sidedef that provides textures for this seg.
    pub sidedef: *mut c_void,
    /// The linedef this seg was split from.
    pub linedef: *mut line_t,
    /// Sector on the front side of this seg.
    pub frontsector: *mut sector_t,
    /// Sector on the back side of this seg (`NULL` for one-sided lines).
    pub backsector: *mut sector_t,
}

// NOTE: The first 4 fields of node_t have the same layout as divline_t.
// C code casts (divline_t*)node for P_DivlineSide calls. We replicate
// the divline_t header here so we can safely transmute.
/// A BSP internal node dividing the map into front and back half-spaces.
///
/// The first four fields (`x`, `y`, `dx`, `dy`) share the same memory layout
/// as `divline_t` so that the C idiom `P_DivlineSide(x, y, (divline_t*)node)`
/// is reproduced safely via `node_as_divline`.
///
/// Corresponds to `node_t` in `r_defs.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct node_t {
    /// X coordinate of the partition line origin.
    pub x: c_int,
    /// Y coordinate of the partition line origin.
    pub y: c_int,
    /// X component of the partition line direction.
    pub dx: c_int,
    /// Y component of the partition line direction.
    pub dy: c_int,
    /// Bounding boxes for each child: `bbox[0]` is front, `bbox[1]` is back.
    /// Each box is `[top, bottom, left, right]` in fixed-point map units.
    pub bbox: [[c_int; 4]; 2],
    /// Child indices: front (`children[0]`) and back (`children[1]`).
    /// If bit 15 (`NF_SUBSECTOR`) is set the index refers to a subsector leaf.
    pub children: [u16; 2],
}

use std::ffi::c_void;

/// A map sector defining floor/ceiling geometry and ambient properties.
///
/// Corresponds to `sector_t` in `r_defs.h`. Layout is verified by the
/// `sector_t_layout` unit test.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct sector_t {
    /// Floor height (fixed-point map units).
    pub floorheight: c_int,
    /// Ceiling height (fixed-point map units).
    pub ceilingheight: c_int,
    /// Floor flat texture number.
    pub floorpic: i16,
    /// Ceiling flat texture number.
    pub ceilingpic: i16,
    /// Ambient light level (0-255).
    pub lightlevel: i16,
    /// Special behaviour type (e.g. damaging floor, secret).
    pub special: i16,
    /// Sector tag used to link linedefs to sectors.
    pub tag: i16,
    _pad0: [u8; 2],
    /// Sound traversal counter (used by sound propagation).
    pub soundtraversed: c_int,
    /// Last sound-emitting mobj in this sector.
    pub soundtarget: *mut c_void,
    /// Bounding box of the sector in blockmap units.
    pub blockbox: [c_int; 4],
    /// Origin point for sector sounds (opaque 40-byte `mobj_t`-like struct).
    pub soundorg: [u8; 40],
    /// Validity stamp; updated each traversal to avoid duplicate processing.
    pub validcount: c_int,
    /// Head of the linked list of things in this sector.
    pub thinglist: *mut c_void,
    /// In-progress thinker data (used by door/floor/ceiling specials).
    pub specialdata: *mut c_void,
    /// Number of linedefs bounding this sector.
    pub linecount: c_int,
    /// Array of pointers to the linedefs bounding this sector.
    pub lines: *mut *mut c_void,
}

/// A map linedef connecting two vertices and separating two sectors.
///
/// Corresponds to `line_t` in `r_defs.h`. Layout is verified by the
/// `line_t_layout` unit test.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct line_t {
    /// Start vertex.
    pub v1: *mut vertex_t,
    /// End vertex.
    pub v2: *mut vertex_t,
    /// Precomputed `v2.x - v1.x` (fixed-point).
    pub dx: c_int,
    /// Precomputed `v2.y - v1.y` (fixed-point).
    pub dy: c_int,
    /// Linedef flags bitmask (`ML_*` constants from `doomdata.h`).
    pub flags: i16,
    /// Special action type triggered by this line.
    pub special: i16,
    /// Sector tag this line targets.
    pub tag: i16,
    /// Sidedef indices: `sidenum[0]` is front, `sidenum[1]` is back
    /// (`-1` means no back side).
    pub sidenum: [i16; 2],
    /// Axis-aligned bounding box of the line in map units.
    pub bbox: [c_int; 4],
    /// Slope type used for fast side-of-line tests (`ST_HORIZONTAL` etc.).
    pub slopetype: c_int,
    /// Sector on the front side.
    pub frontsector: *mut sector_t,
    /// Sector on the back side (`NULL` for one-sided lines).
    pub backsector: *mut sector_t,
    /// Validity stamp for blockmap/BSP traversal deduplication.
    pub validcount: c_int,
    /// In-progress special data (e.g. active door thinker pointer).
    pub specialdata: *mut c_void,
}

/// Z-coordinate of the looker's eyes (fixed-point map units).
///
/// Set by `P_CheckSight` to `t1->z + t1->height - (t1->height >> 2)` before
/// BSP traversal. Read by `P_CrossSubsector` for slope comparisons.
#[no_mangle]
pub static mut sightzstart: c_int = 0;

/// Upper slope bound for the sight corridor (fixed-point).
///
/// Initialised to the slope from `sightzstart` to the top of the target.
/// Narrowed downward as occluding portals are encountered.
#[no_mangle]
pub static mut topslope: c_int = 0;

/// Lower slope bound for the sight corridor (fixed-point).
///
/// Initialised to the slope from `sightzstart` to the bottom of the target.
/// Narrowed upward as occluding portals are encountered.
#[no_mangle]
pub static mut bottomslope: c_int = 0;

/// The sight-trace ray from `t1` to `t2`, stored as a `divline_t`.
///
/// Set by `P_CheckSight` and used throughout the BSP traversal in
/// `P_CrossBSPNode` and `P_CrossSubsector`.
#[no_mangle]
pub static mut strace: divline_t = divline_t {
    x: 0,
    y: 0,
    dx: 0,
    dy: 0,
};

/// X coordinate of the sight target (`t2->x`).
///
/// Stored separately from `strace` so that `P_CrossBSPNode` can test which
/// side of each partition the target falls on without a full ray struct.
#[no_mangle]
pub static mut t2x: c_int = 0;

/// Y coordinate of the sight target (`t2->y`).
///
/// See `t2x`.
#[no_mangle]
pub static mut t2y: c_int = 0;

/// Diagnostic counters: `sightcounts[0]` is REJECT table hits (early-out),
/// `sightcounts[1]` is full BSP traversals attempted.
#[no_mangle]
pub static mut sightcounts: [c_int; 2] = [0, 0];

use crate::doom::p_setup::{
    nodes as p_setup_nodes, numnodes, numsectors, numsubsectors, rejectmatrix,
    sectors as p_setup_sectors, segs as p_setup_segs, subsectors as p_setup_subsectors,
};
use crate::doom::r_main::validcount;
// Reuse the authoritative mobj_t mirror from p_telept.rs to guarantee
// field offsets match the C layout (x=24, subsector=88, height=108).
pub use crate::doom::p_telept::mobj_t;

/// Determine which half-space of a `divline_t` a point lies in.
///
/// Returns 0 (front side), 1 (back side), or 2 (on the line).
///
/// Matches `P_DivlineSide` in `p_sight.c`. The `dy == 0` fast path contains
/// a deliberate quirk inherited from the original Doom source: it tests
/// `x == node.y` rather than `y == node.y`. This is intentional for
/// demo-compatible determinism; do not correct it.
fn P_DivlineSide(x: c_int, y: c_int, node: &divline_t) -> c_int {
    if node.dx == 0 {
        if x == node.x {
            return 2;
        }
        return if x <= node.x {
            (node.dy > 0) as c_int
        } else {
            (node.dy < 0) as c_int
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
            (node.dx < 0) as c_int
        } else {
            (node.dx > 0) as c_int
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
fn P_InterceptVector2(v2: &divline_t, v1: &divline_t) -> c_int {
    let den = FixedMul(v1.dy >> 8, v2.dx) - FixedMul(v1.dx >> 8, v2.dy);

    if den == 0 {
        return 0;
    }

    let num = FixedMul((v1.x - v2.x) >> 8, v1.dy) + FixedMul((v2.y - v1.y) >> 8, v1.dx);

    FixedDiv(num, den)
}

/// Test whether the global sight ray (`strace`) passes through subsector `num`
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
fn P_CrossSubsector(num: c_int) -> bool {
    unsafe {
        let strace_copy = strace;
        let numsubsectors_val = *std::ptr::addr_of!(numsubsectors);
        if num >= numsubsectors_val {
            i_error!(
                "P_CrossSubsector: ss {} with numss = {}",
                num,
                numsubsectors_val
            );
        }

        let sub = &*(p_setup_subsectors as *mut subsector_t).add(num as usize);

        let mut count = sub.numlines as c_int;
        let mut seg_ptr = (p_setup_segs as *mut seg_t).add(sub.firstline as usize);

        while count > 0 {
            let seg = &*seg_ptr;
            seg_ptr = seg_ptr.add(1);
            count -= 1;

            let line = &mut *seg.linedef;

            // Already checked other side?
            if line.validcount == validcount {
                continue;
            }

            line.validcount = validcount;

            let v1 = &*line.v1;
            let v2 = &*line.v2;

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
            if line.backsector.is_null() {
                return false;
            }

            // Stop because it is not two sided anyway.
            // Also must use line->flags, not a re-read through seg->linedef.
            if line.flags & LinedefFlag::TWOSIDED as i16 == 0 {
                return false;
            }

            let front = seg.frontsector;
            let back = seg.backsector;

            let front_floor = (*front).floorheight;
            let front_ceil = (*front).ceilingheight;
            let back_floor = (*back).floorheight;
            let back_ceil = (*back).ceilingheight;

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
                let slope = FixedDiv(openbottom - sightzstart, frac);
                if slope > bottomslope {
                    bottomslope = slope;
                }
            }

            if front_ceil != back_ceil {
                let slope = FixedDiv(opentop - sightzstart, frac);
                if slope < topslope {
                    topslope = slope;
                }
            }

            if topslope <= bottomslope {
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
fn P_CrossBSPNode(bspnum: c_int) -> bool {
    unsafe {
        let strace_copy = strace;
        if bspnum & NF_SUBSECTOR as c_int != 0 {
            if bspnum == -1 {
                return P_CrossSubsector(0);
            } else {
                return P_CrossSubsector(bspnum & !(NF_SUBSECTOR as c_int));
            }
        }

        let bsp = &*(p_setup_nodes as *mut node_t).add(bspnum as usize);
        let bsp_div = node_as_divline(bsp);

        let side = P_DivlineSide(strace_copy.x, strace_copy.y, &bsp_div);
        let side = if side == 2 { 0 } else { side };

        // Cross the starting side.
        if !P_CrossBSPNode(bsp.children[side as usize] as c_int) {
            return false;
        }

        // The partition plane is crossed here.
        let bsp_div2 = node_as_divline(bsp);
        let t2_side = P_DivlineSide(t2x, t2y, &bsp_div2);
        if side == t2_side {
            return true;
        }

        // Cross the ending side.
        P_CrossBSPNode(bsp.children[(side ^ 1) as usize] as c_int)
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
/// Returns a C `boolean` (`c_int`, non-zero = visible) rather than Rust `bool`
/// because Doom's `typedef unsigned int boolean` makes the C caller read a
/// 32-bit value. Returning `bool` (1 byte) would leave the upper 24 bits
/// undefined and cause sight checks to randomly succeed or fail, desynchronising
/// demos and RNG.
///
/// Matches `P_CheckSight` in `p_sight.c`.
///
/// # Safety
/// Both `t1` and `t2` must be valid, non-null pointers to initialised
/// `mobj_t` instances. Their `subsector` fields must point into the live
/// subsector/sector arrays loaded by `p_setup`.
#[no_mangle]
pub extern "C" fn P_CheckSight(t1: *mut mobj_t, t2: *mut mobj_t) -> c_int {
    unsafe {
        // Cast both pointers through *const u8 to sidestep the fact that
        // mobj_t (from p_telept) references a different sector_t type than
        // the one declared here. Sector size is the same in both.
        let sec_size = std::mem::size_of::<sector_t>() as isize;
        let s1 = (((*(*t1).subsector).sector as *const u8)
            .offset_from(p_setup_sectors as *const u8)
            / sec_size) as c_int;
        let s2 = (((*(*t2).subsector).sector as *const u8)
            .offset_from(p_setup_sectors as *const u8)
            / sec_size) as c_int;
        let pnum = s1 * numsectors + s2;
        let bytenum = pnum >> 3;
        let bitnum = 1 << (pnum & 7);

        // Check in REJECT table.
        if *rejectmatrix.add(bytenum as usize) & bitnum != 0 {
            sightcounts[0] += 1;
            return 0;
        }

        sightcounts[1] += 1;
        validcount += 1;

        sightzstart = (*t1).z + (*t1).height - ((*t1).height >> 2);
        topslope = (*t2).z + (*t2).height - sightzstart;
        bottomslope = (*t2).z - sightzstart;

        strace.x = (*t1).x;
        strace.y = (*t1).y;
        t2x = (*t2).x;
        t2y = (*t2).y;
        strace.dx = (*t2).x - (*t1).x;
        strace.dy = (*t2).y - (*t1).y;

        P_CrossBSPNode(numnodes - 1) as c_int
    }
}

/// Anchor function to ensure exports survive linker dead-code elimination.
#[no_mangle]
pub extern "C" fn P_Sight_Link_Anchor() {
    let _ = P_CheckSight as *const () as usize;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    const DIVLINE_T_SIZEOF: usize = 16;
    const VERTEX_T_SIZEOF: usize = 8;
    const SUBSECTOR_T_SIZEOF: usize = 16;
    const SEG_T_SIZEOF: usize = 56;
    const NODE_T_SIZEOF: usize = 52;
    const SECTOR_T_SIZEOF: usize = 128;
    const LINE_T_SIZEOF: usize = 88;

    #[test]
    fn divline_t_size() {
        assert_eq!(std::mem::size_of::<divline_t>(), DIVLINE_T_SIZEOF);
    }

    #[test]
    fn vertex_t_size() {
        assert_eq!(std::mem::size_of::<vertex_t>(), VERTEX_T_SIZEOF);
    }

    #[test]
    fn subsector_t_size() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(std::mem::size_of::<subsector_t>(), SUBSECTOR_T_SIZEOF,);
    }

    #[test]
    fn seg_t_size() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(std::mem::size_of::<seg_t>(), SEG_T_SIZEOF,);
    }

    #[test]
    fn node_t_size() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(std::mem::size_of::<node_t>(), NODE_T_SIZEOF,);
    }

    #[test]
    fn sector_t_layout() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(std::mem::size_of::<sector_t>(), SECTOR_T_SIZEOF);
        assert_eq!(std::mem::offset_of!(sector_t, floorheight), 0);
        assert_eq!(std::mem::offset_of!(sector_t, ceilingheight), 4);
        assert_eq!(std::mem::offset_of!(sector_t, floorpic), 8);
        assert_eq!(std::mem::offset_of!(sector_t, ceilingpic), 10);
        assert_eq!(std::mem::offset_of!(sector_t, lightlevel), 12);
        assert_eq!(std::mem::offset_of!(sector_t, special), 14);
        assert_eq!(std::mem::offset_of!(sector_t, tag), 16);
        assert_eq!(std::mem::offset_of!(sector_t, soundtraversed), 20);
        assert_eq!(std::mem::offset_of!(sector_t, soundtarget), 24);
        assert_eq!(std::mem::offset_of!(sector_t, blockbox), 32);
        assert_eq!(std::mem::offset_of!(sector_t, soundorg), 48);
        assert_eq!(std::mem::offset_of!(sector_t, validcount), 88);
        assert_eq!(std::mem::offset_of!(sector_t, thinglist), 96);
        assert_eq!(std::mem::offset_of!(sector_t, specialdata), 104);
        assert_eq!(std::mem::offset_of!(sector_t, linecount), 112);
        assert_eq!(std::mem::offset_of!(sector_t, lines), 120);
    }

    #[test]
    fn line_t_layout() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(std::mem::size_of::<line_t>(), LINE_T_SIZEOF);
        assert_eq!(std::mem::offset_of!(line_t, v1), 0);
        assert_eq!(std::mem::offset_of!(line_t, v2), 8);
        assert_eq!(std::mem::offset_of!(line_t, dx), 16);
        assert_eq!(std::mem::offset_of!(line_t, dy), 20);
        assert_eq!(std::mem::offset_of!(line_t, flags), 24);
        assert_eq!(std::mem::offset_of!(line_t, special), 26);
        assert_eq!(std::mem::offset_of!(line_t, tag), 28);
        assert_eq!(std::mem::offset_of!(line_t, sidenum), 30);
        assert_eq!(std::mem::offset_of!(line_t, bbox), 36);
        assert_eq!(std::mem::offset_of!(line_t, slopetype), 52);
        assert_eq!(std::mem::offset_of!(line_t, frontsector), 56);
        assert_eq!(std::mem::offset_of!(line_t, backsector), 64);
        assert_eq!(std::mem::offset_of!(line_t, validcount), 72);
        assert_eq!(std::mem::offset_of!(line_t, specialdata), 80);
    }
}
