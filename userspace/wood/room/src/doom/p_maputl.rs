//! Map geometry utilities, blockmap iterators, and intercept routines.
//!
//! Rust port of `vendor/doomgeneric/p_maputl.c`. Provides the low-level
//! primitives used by `p_map.c` for collision detection, line-of-sight, and
//! hitscan/projectile tracing: approximate distance, point-on-line tests,
//! thing-position management, blockmap traversal, and the intercept pipeline
//! used by `P_PathTraverse`.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::{c_int, c_uint, c_void};
use std::ptr;

use crate::doom::c_ffi::{
    divline_t, intercept_t, intercept_t_d, line_t, mobj_t, sector_t, subsector_t, MAPBLOCKSHIFT,
    MAPBLOCKSIZE, MAPBTOFRAC,
};
use crate::doom::info::*;
use crate::doom::m_fixed::{fixed_t, FixedDiv, FixedMul};
use crate::doom::m_fixed::{FRACBITS, FRACUNIT};

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
pub const PT_ADDLINES: c_int = 1;

/// `P_PathTraverse` flag: collect thing intercepts while traversing.
pub const PT_ADDTHINGS: c_int = 2;

/// `P_PathTraverse` flag: stop early at the first solid (one-sided) line hit.
pub const PT_EARLYOUT: c_int = 4;

// ---------------------------------------------------------------------------
// Globals
// ---------------------------------------------------------------------------

/// Top of the open vertical range through the last line tested by
/// `P_LineOpening` (fixed-point map units). Read by `p_map.c`.
#[no_mangle]
pub static mut opentop: fixed_t = 0;

/// Bottom of the open vertical range through the last line tested by
/// `P_LineOpening` (fixed-point map units). Read by `p_map.c`.
#[no_mangle]
pub static mut openbottom: fixed_t = 0;

/// Height of the open range (`opentop - openbottom`). Zero means the line is
/// closed (impassable). Read by `p_map.c`.
#[no_mangle]
pub static mut openrange: fixed_t = 0;

/// The lower of the two floor heights on either side of the last line tested
/// by `P_LineOpening`. Used for step-up logic in `p_map.c`.
#[no_mangle]
pub static mut lowfloor: fixed_t = 0;

/// Array of intercept records accumulated during a `P_PathTraverse` call.
///
/// Each entry records where the trace ray hit a line or thing along with the
/// parametric fraction `t` along the ray. The array is indexed via
/// `intercept_p`. Entries beyond index `MAXINTERCEPTS_ORIGINAL` trigger
/// `InterceptsOverrun` to emulate vanilla memory layout behaviour.
#[no_mangle]
pub static mut intercepts: [intercept_t; MAXINTERCEPTS] = [intercept_t {
    frac: 0,
    isaline: 0,
    d: intercept_t_d {
        thing: ptr::null_mut(),
    },
}; MAXINTERCEPTS];

/// Write cursor into the `intercepts` array.
///
/// Initialised to `&intercepts[0]` at the start of each `P_PathTraverse`
/// call and advanced by `PIT_AddLineIntercepts` / `PIT_AddThingIntercepts`.
/// `P_TraverseIntercepts` uses `intercept_p - intercepts` to know how many
/// entries were collected.
#[no_mangle]
pub static mut intercept_p: *mut intercept_t = ptr::null_mut();

/// The current path-traverse ray, set by `P_PathTraverse`.
///
/// `PIT_AddLineIntercepts` and `PIT_AddThingIntercepts` read this to
/// determine whether each line/thing crosses the ray.
#[no_mangle]
pub static mut trace: divline_t = divline_t {
    x: 0,
    y: 0,
    dx: 0,
    dy: 0,
};

/// Whether `P_PathTraverse` should exit immediately upon hitting a solid
/// one-sided line. Set from the `PT_EARLYOUT` flag passed to `P_PathTraverse`.
static mut earlyout: c_int = 0;

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
#[no_mangle]
pub extern "C" fn P_AproxDistance(dx: fixed_t, dy: fixed_t) -> fixed_t {
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

/// Return which side of a `line_t` a point lies on.
///
/// Returns 0 if the point is on the front (right-hand) side of the line, or
/// 1 if it is on the back side. Uses the precomputed `line.dx` / `line.dy`
/// and cross-product sign.
///
/// Matches `P_PointOnLineSide` in `p_maputl.c`.
///
/// # Safety
/// `line` must be a valid, non-null pointer to an initialised `line_t` whose
/// `v1` field points to a valid `vertex_t`.
#[no_mangle]
pub extern "C" fn P_PointOnLineSide(x: fixed_t, y: fixed_t, line: *mut line_t) -> c_int {
    unsafe {
        let line = &*line;
        if line.dx == 0 {
            if x <= (*line.v1).x {
                return if line.dy > 0 { 1 } else { 0 };
            }
            return if line.dy < 0 { 1 } else { 0 };
        }
        if line.dy == 0 {
            if y <= (*line.v1).y {
                return if line.dx < 0 { 1 } else { 0 };
            }
            return if line.dx > 0 { 1 } else { 0 };
        }
        let dx = x - (*line.v1).x;
        let dy = y - (*line.v1).y;
        let left = FixedMul(line.dy >> FRACBITS, dx);
        let right = FixedMul(dy, line.dx >> FRACBITS);
        if right < left {
            0
        } else {
            1
        }
    }
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
///
/// # Safety
/// `line` must be a valid, non-null pointer to an initialised `divline_t`.
#[no_mangle]
pub extern "C" fn P_PointOnDivlineSide(x: fixed_t, y: fixed_t, line: *mut divline_t) -> c_int {
    unsafe {
        let line = &*line;
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
        if right < left {
            0
        } else {
            1
        }
    }
}

// ---------------------------------------------------------------------------
// P_MakeDivline
// ---------------------------------------------------------------------------

/// Populate a `divline_t` from the geometry of a `line_t`.
///
/// Sets `dl.x` / `dl.y` to `line.v1`, and `dl.dx` / `dl.dy` to
/// `line.dx` / `line.dy`. Used to convert a map linedef into a ray for
/// intercept testing.
///
/// Matches `P_MakeDivline` in `p_maputl.c`.
///
/// # Safety
/// `li` must be a valid, non-null pointer to an initialised `line_t` whose
/// `v1` field points to a valid `vertex_t`. `dl` must be a valid, non-null,
/// writable pointer to a `divline_t`.
#[no_mangle]
pub extern "C" fn P_MakeDivline(li: *mut line_t, dl: *mut divline_t) {
    unsafe {
        let li = &*li;
        let dl = &mut *dl;
        dl.x = (*li.v1).x;
        dl.y = (*li.v1).y;
        dl.dx = li.dx;
        dl.dy = li.dy;
    }
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
///
/// # Safety
/// Both `v1` and `v2` must be valid, non-null pointers to initialised
/// `divline_t` values.
#[no_mangle]
pub extern "C" fn P_InterceptVector(v2: *mut divline_t, v1: *mut divline_t) -> fixed_t {
    unsafe {
        let v1 = &*v1;
        let v2 = &*v2;
        let den = FixedMul(v1.dy >> 8, v2.dx) - FixedMul(v1.dx >> 8, v2.dy);
        if den == 0 {
            return 0;
        }
        let num = FixedMul((v1.x - v2.x) >> 8, v1.dy) + FixedMul((v2.y - v1.y) >> 8, v1.dx);
        FixedDiv(num, den)
    }
}

// ---------------------------------------------------------------------------
// P_BoxOnLineSide
// ---------------------------------------------------------------------------

/// Determine which side(s) of a linedef an axis-aligned bounding box spans.
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
///
/// # Safety
/// `tmbox` must point to at least 4 consecutive `c_int` values. `ld` must be
/// a valid, non-null pointer to an initialised `line_t` with valid `v1`.
#[no_mangle]
pub extern "C" fn P_BoxOnLineSide(tmbox: *mut c_int, ld: *mut line_t) -> c_int {
    unsafe {
        let tmbox = std::slice::from_raw_parts_mut(tmbox, 4);
        let ld = &*ld;
        let mut p1: c_int;
        let mut p2: c_int;
        match ld.slopetype {
            0 => {
                // ST_HORIZONTAL
                p1 = (tmbox[0] > (*ld.v1).y) as c_int;
                p2 = (tmbox[1] > (*ld.v1).y) as c_int;
                if ld.dx < 0 {
                    p1 ^= 1;
                    p2 ^= 1;
                }
            }
            1 => {
                // ST_VERTICAL
                p1 = (tmbox[3] < (*ld.v1).x) as c_int;
                p2 = (tmbox[2] < (*ld.v1).x) as c_int;
                if ld.dy < 0 {
                    p1 ^= 1;
                    p2 ^= 1;
                }
            }
            2 => {
                // ST_POSITIVE
                p1 = P_PointOnLineSide(tmbox[2], tmbox[0], ld as *const _ as *mut _);
                p2 = P_PointOnLineSide(tmbox[3], tmbox[1], ld as *const _ as *mut _);
            }
            _ => {
                // ST_NEGATIVE
                p1 = P_PointOnLineSide(tmbox[3], tmbox[0], ld as *const _ as *mut _);
                p2 = P_PointOnLineSide(tmbox[2], tmbox[1], ld as *const _ as *mut _);
            }
        }
        if p1 == p2 {
            p1
        } else {
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// P_LineOpening
// ---------------------------------------------------------------------------

/// Compute the open vertical portal range for a two-sided linedef.
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
/// # Safety
/// `linedef` must be a valid, non-null pointer to an initialised `line_t`.
/// If the line is two-sided, `frontsector` and `backsector` must also be
/// valid non-null pointers to initialised `sector_t` values.
#[no_mangle]
pub extern "C" fn P_LineOpening(linedef: *mut line_t) {
    unsafe {
        let linedef = &*linedef;
        if linedef.sidenum[1] == -1 {
            openrange = 0;
            return;
        }
        let front = linedef.frontsector as *mut sector_t;
        let back = linedef.backsector as *mut sector_t;
        if (*front).ceilingheight < (*back).ceilingheight {
            opentop = (*front).ceilingheight;
        } else {
            opentop = (*back).ceilingheight;
        }
        if (*front).floorheight > (*back).floorheight {
            openbottom = (*front).floorheight;
            lowfloor = (*back).floorheight;
        } else {
            openbottom = (*back).floorheight;
            lowfloor = (*front).floorheight;
        }
        openrange = opentop - openbottom;
    }
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
#[no_mangle]
pub extern "C" fn P_UnsetThingPosition(thing: *mut mobj_t) {
    unsafe {
        let thing = &mut *thing;
        if (thing.flags & MF_NOSECTOR) == 0 {
            if !thing.snext.is_null() {
                let snext = thing.snext as *mut mobj_t;
                (*snext).sprev = thing.sprev;
            }
            if !thing.sprev.is_null() {
                let sprev = thing.sprev as *mut mobj_t;
                (*sprev).snext = thing.snext;
            } else {
                let subsector = thing.subsector as *mut subsector_t;
                let sector = (*subsector).sector as *mut sector_t;
                (*sector).thinglist = thing.snext;
            }
        }
        if (thing.flags & MF_NOBLOCKMAP) == 0 {
            if !thing.bnext.is_null() {
                let bnext = thing.bnext as *mut mobj_t;
                (*bnext).bprev = thing.bprev;
            }
            if !thing.bprev.is_null() {
                let bprev = thing.bprev as *mut mobj_t;
                (*bprev).bnext = thing.bnext;
            } else {
                let blockx = (thing.x - crate::doom::p_setup::bmaporgx) >> MAPBLOCKSHIFT;
                let blocky = (thing.y - crate::doom::p_setup::bmaporgy) >> MAPBLOCKSHIFT;
                if blockx >= 0
                    && blockx < crate::doom::p_setup::bmapwidth
                    && blocky >= 0
                    && blocky < crate::doom::p_setup::bmapheight
                {
                    let idx = (blocky * crate::doom::p_setup::bmapwidth + blockx) as isize;
                    *crate::doom::p_setup::blocklinks.offset(idx) = thing.bnext;
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
/// Also sets `thing->subsector` to the computed subsector pointer.
///
/// Matches `P_SetThingPosition` in `p_maputl.c`.
///
/// # Safety
/// `thing` must be a valid, non-null pointer to an initialised `mobj_t`. The
/// global map data (sectors, blockmap) from `p_setup` must be loaded.
#[no_mangle]
pub extern "C" fn P_SetThingPosition(thing: *mut mobj_t) {
    unsafe {
        let thing = &mut *thing;
        let ss = crate::doom::r_main::R_PointInSubsector(thing.x, thing.y) as *mut subsector_t;
        thing.subsector = ss as *mut c_void;
        if (thing.flags & MF_NOSECTOR) == 0 {
            let sec = (*ss).sector as *mut sector_t;
            thing.sprev = ptr::null_mut();
            thing.snext = (*sec).thinglist;
            if !(*sec).thinglist.is_null() {
                let thinglist = (*sec).thinglist as *mut mobj_t;
                (*thinglist).sprev = thing as *mut mobj_t as *mut c_void;
            }
            (*sec).thinglist = thing as *mut mobj_t as *mut c_void;
        }
        if (thing.flags & MF_NOBLOCKMAP) == 0 {
            let blockx = (thing.x - crate::doom::p_setup::bmaporgx) >> MAPBLOCKSHIFT;
            let blocky = (thing.y - crate::doom::p_setup::bmaporgy) >> MAPBLOCKSHIFT;
            if blockx >= 0
                && blockx < crate::doom::p_setup::bmapwidth
                && blocky >= 0
                && blocky < crate::doom::p_setup::bmapheight
            {
                let idx = (blocky * crate::doom::p_setup::bmapwidth + blockx) as isize;
                let link = crate::doom::p_setup::blocklinks.offset(idx);
                thing.bprev = ptr::null_mut();
                thing.bnext = *link;
                if !(*link).is_null() {
                    let l = *link as *mut mobj_t;
                    (*l).bprev = thing as *mut mobj_t as *mut c_void;
                }
                *link = thing as *mut mobj_t as *mut c_void;
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
/// The global blockmap arrays from `p_setup` must be initialised. `func` must
/// be a valid function pointer; it will be called with non-null `line_t`
/// pointers from the loaded map data.
#[no_mangle]
pub extern "C" fn P_BlockLinesIterator(
    x: c_int,
    y: c_int,
    func: Option<unsafe extern "C" fn(*mut line_t) -> c_uint>,
) -> c_uint {
    unsafe {
        if x < 0
            || y < 0
            || x >= crate::doom::p_setup::bmapwidth
            || y >= crate::doom::p_setup::bmapheight
        {
            return 1;
        }
        let offset = (y * crate::doom::p_setup::bmapwidth + x) as isize;
        let offset = *crate::doom::p_setup::blockmap.offset(offset) as isize;
        let mut list = crate::doom::p_setup::blockmaplump.offset(offset);
        while *list != -1 {
            let ld = crate::doom::p_setup::lines.offset(*list as isize);
            if (*ld).validcount == crate::doom::r_main::validcount {
                list = list.offset(1);
                continue;
            }
            (*ld).validcount = crate::doom::r_main::validcount;
            if func.unwrap()(ld) == 0 {
                return 0;
            }
            list = list.offset(1);
        }
        1
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
/// The global `blocklinks` array from `p_setup` must be initialised. `func`
/// must be a valid function pointer; it will be called with non-null `mobj_t`
/// pointers from the live thing-list.
#[no_mangle]
pub extern "C" fn P_BlockThingsIterator(
    x: c_int,
    y: c_int,
    func: Option<unsafe extern "C" fn(*mut mobj_t) -> c_uint>,
) -> c_uint {
    unsafe {
        if x < 0
            || y < 0
            || x >= crate::doom::p_setup::bmapwidth
            || y >= crate::doom::p_setup::bmapheight
        {
            return 1;
        }
        let idx = (y * crate::doom::p_setup::bmapwidth + x) as isize;
        let mut mobj = *crate::doom::p_setup::blocklinks.offset(idx) as *mut mobj_t;
        while !mobj.is_null() {
            if func.unwrap()(mobj) == 0 {
                return 0;
            }
            mobj = (*mobj).bnext as *mut mobj_t;
        }
        1
    }
}

// ---------------------------------------------------------------------------
// Intercept routines
// ---------------------------------------------------------------------------

/// Write a single `c_int` value into a specific adjacent memory location to
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
/// Writes directly through raw pointers to module-level globals. Caller must
/// ensure `location` is within the range covered by the table (0..=~300 bytes).
#[allow(unused_assignments)]
unsafe fn InterceptsMemoryOverrun(location: c_int, value: c_int) {
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
                ($ptr as *mut c_int).add(index).write(value);
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

    skip!(4); // 0
    skip!(4); // 1 earlyout
    skip!(4); // 2 intercept_p
    write_i32!(4, std::ptr::addr_of_mut!(lowfloor)); // 3 lowfloor
    write_i32!(4, std::ptr::addr_of_mut!(openbottom)); // 4 openbottom
    write_i32!(4, std::ptr::addr_of_mut!(opentop)); // 5 opentop
    write_i32!(4, std::ptr::addr_of_mut!(openrange)); // 6 openrange
    skip!(4); // 7
    skip!(120); // 8 activeplats
    skip!(8); // 9
    write_i32!(4, std::ptr::addr_of_mut!(crate::doom::p_pspr::bulletslope)); // 10 bulletslope
    skip!(4); // 11 swingx
    skip!(4); // 12 swingy
    skip!(4); // 13
    write_i16_arr!(
        40,
        std::ptr::addr_of_mut!(crate::doom::p_setup::playerstarts)
    ); // 14 playerstarts
    skip!(4); // 15 blocklinks
    write_i32!(4, std::ptr::addr_of_mut!(crate::doom::p_setup::bmapwidth)); // 16 bmapwidth
    skip!(4); // 17 blockmap
    write_i32!(4, std::ptr::addr_of_mut!(crate::doom::p_setup::bmaporgx)); // 18 bmaporgx
    write_i32!(4, std::ptr::addr_of_mut!(crate::doom::p_setup::bmaporgy)); // 19 bmaporgy
    skip!(4); // 20 blockmaplump
    write_i32!(4, std::ptr::addr_of_mut!(crate::doom::p_setup::bmapheight)); // 21 bmapheight
}

/// Emulate the vanilla Doom `intercepts[]` buffer overrun for demo fidelity.
///
/// When `num_intercepts` exceeds `MAXINTERCEPTS_ORIGINAL` (128), the original
/// engine would write past the array into adjacent globals. This function
/// calls `InterceptsMemoryOverrun` three times (for `frac`, `isaline`, and
/// `d.thing`) to reproduce those writes.
///
/// # Safety
/// `intercept` must be a valid, non-null pointer to an initialised
/// `intercept_t`.
unsafe fn InterceptsOverrun(num_intercepts: c_int, intercept: *mut intercept_t) {
    if num_intercepts <= MAXINTERCEPTS_ORIGINAL as c_int {
        return;
    }
    let location = (num_intercepts - MAXINTERCEPTS_ORIGINAL as c_int - 1) * 12;
    InterceptsMemoryOverrun(location, (*intercept).frac);
    InterceptsMemoryOverrun(location + 4, (*intercept).isaline);
    InterceptsMemoryOverrun(location + 8, (*intercept).d.thing as usize as c_int);
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
/// `ld` must be a valid, non-null pointer to an initialised `line_t`. The
/// global `trace`, `intercepts`, and `intercept_p` must be set up by a
/// preceding `P_PathTraverse` call.
#[no_mangle]
pub extern "C" fn PIT_AddLineIntercepts(ld: *mut line_t) -> c_uint {
    unsafe {
        let ld = &*ld;
        let (s1, s2) = if trace.dx > FRACUNIT * 16
            || trace.dy > FRACUNIT * 16
            || trace.dx < -FRACUNIT * 16
            || trace.dy < -FRACUNIT * 16
        {
            (
                P_PointOnDivlineSide(
                    (*ld.v1).x,
                    (*ld.v1).y,
                    &raw const trace as *const _ as *mut _,
                ),
                P_PointOnDivlineSide(
                    (*ld.v2).x,
                    (*ld.v2).y,
                    &raw const trace as *const _ as *mut _,
                ),
            )
        } else {
            (
                P_PointOnLineSide(trace.x, trace.y, ld as *const _ as *mut _),
                P_PointOnLineSide(
                    trace.x + trace.dx,
                    trace.y + trace.dy,
                    ld as *const _ as *mut _,
                ),
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
        P_MakeDivline(ld as *const _ as *mut _, &mut dl);
        let frac = P_InterceptVector(&raw const trace as *const _ as *mut _, &mut dl);
        if frac < 0 {
            return 1;
        }
        if earlyout != 0 && frac < FRACUNIT && ld.backsector.is_null() {
            return 0;
        }
        (*intercept_p).frac = frac;
        (*intercept_p).isaline = 1;
        (*intercept_p).d.line = ld as *const line_t as *mut line_t;
        InterceptsOverrun(
            intercept_p.offset_from(std::ptr::addr_of_mut!(intercepts[0])) as c_int,
            intercept_p,
        );
        intercept_p = intercept_p.offset(1);
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
#[no_mangle]
pub extern "C" fn PIT_AddThingIntercepts(thing: *mut mobj_t) -> c_uint {
    unsafe {
        let thing = &*thing;
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
        let s1 = P_PointOnDivlineSide(x1, y1, &raw const trace as *const _ as *mut _);
        let s2 = P_PointOnDivlineSide(x2, y2, &raw const trace as *const _ as *mut _);
        if s1 == s2 {
            return 1;
        }
        let mut dl = divline_t {
            x: x1,
            y: y1,
            dx: x2 - x1,
            dy: y2 - y1,
        };
        let frac = P_InterceptVector(&raw const trace as *const _ as *mut _, &mut dl);
        if frac < 0 {
            return 1;
        }
        (*intercept_p).frac = frac;
        (*intercept_p).isaline = 0;
        (*intercept_p).d.thing = thing as *const _ as *mut c_void;
        InterceptsOverrun(
            intercept_p.offset_from(std::ptr::addr_of_mut!(intercepts[0])) as c_int,
            intercept_p,
        );
        intercept_p = intercept_p.offset(1);
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
/// minimum each pass, marks it with `c_int::MAX` after processing). Stops
/// when the nearest remaining intercept has `frac > maxfrac`.
///
/// Returns 1 (true) if `func` accepted every intercept, 0 (false) if `func`
/// returned 0 for any intercept.
///
/// Matches `P_TraverseIntercepts` in `p_maputl.c`.
///
/// # Safety
/// `func` must be a valid function pointer. `intercepts` and `intercept_p`
/// must be in a consistent state as set by `P_PathTraverse` / the `PIT_Add*`
/// callbacks.
#[no_mangle]
pub extern "C" fn P_TraverseIntercepts(
    func: Option<unsafe extern "C" fn(*mut intercept_t) -> c_uint>,
    maxfrac: fixed_t,
) -> c_uint {
    unsafe {
        let count = if intercept_p.is_null() {
            0
        } else {
            intercept_p.offset_from(std::ptr::addr_of_mut!(intercepts[0])) as c_int
        };
        for _ in 0..count {
            let mut dist = c_int::MAX;
            let mut in_ptr: *mut intercept_t = ptr::null_mut();
            let mut scan = std::ptr::addr_of_mut!(intercepts[0]);
            while scan < intercept_p {
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
            (*in_ptr).frac = c_int::MAX;
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
/// The global map data from `p_setup` must be loaded. `trav` must be a valid
/// function pointer or `None`. All coordinates are in fixed-point map units.
#[no_mangle]
pub extern "C" fn P_PathTraverse(
    x1: fixed_t,
    y1: fixed_t,
    x2: fixed_t,
    y2: fixed_t,
    flags: c_int,
    trav: Option<unsafe extern "C" fn(*mut intercept_t) -> c_uint>,
) -> c_uint {
    unsafe {
        earlyout = flags & PT_EARLYOUT;
        crate::doom::r_main::validcount = crate::doom::r_main::validcount.wrapping_add(1);
        intercept_p = std::ptr::addr_of_mut!(intercepts[0]);
        let mut x1 = x1;
        let mut y1 = y1;
        if ((x1 - crate::doom::p_setup::bmaporgx) & (MAPBLOCKSIZE - 1)) == 0 {
            x1 += FRACUNIT;
        }
        if ((y1 - crate::doom::p_setup::bmaporgy) & (MAPBLOCKSIZE - 1)) == 0 {
            y1 += FRACUNIT;
        }
        trace.x = x1;
        trace.y = y1;
        trace.dx = x2 - x1;
        trace.dy = y2 - y1;
        let x1 = x1 - crate::doom::p_setup::bmaporgx;
        let y1 = y1 - crate::doom::p_setup::bmaporgy;
        let xt1 = x1 >> MAPBLOCKSHIFT;
        let yt1 = y1 >> MAPBLOCKSHIFT;
        let x2 = x2 - crate::doom::p_setup::bmaporgx;
        let y2 = y2 - crate::doom::p_setup::bmaporgy;
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
                    Some(PIT_AddLineIntercepts as unsafe extern "C" fn(*mut line_t) -> c_uint),
                ) == 0
            {
                return 0;
            }
            if flags & PT_ADDTHINGS != 0
                && P_BlockThingsIterator(
                    mapx,
                    mapy,
                    Some(PIT_AddThingIntercepts as unsafe extern "C" fn(*mut mobj_t) -> c_uint),
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
}
