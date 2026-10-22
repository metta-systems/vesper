//! BSP traversal and line-seg clipping for the Doom renderer.
//!
//! Corresponds to `vendor/doomgeneric/r_bsp.c`. Walks the BSP tree
//! front-to-back from the player's viewpoint, renders each subsector leaf,
//! and maintains the solid-column occlusion list (`solidsegs`) so that
//! already-covered screen columns are never redrawn.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::{c_int, c_short, c_void};
use std::ptr;

use super::m_bbox::BBox;
use super::m_fixed::{angle_t, fixed_t};
use super::tables::{ANG90, ANGLETOFINESHIFT};

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
// Enable with `RUST_LOG=room::doom::r_bsp=trace`. These are gated at TRACE so
// the existing debug log stays usable; terminate the app the moment the
// glitch appears and we inspect the tail of the trace.

/// Monotonically increasing frame counter used by diagnostic trace logging.
/// Incremented once per frame in [`R_ClearClipSegs`].
static mut PROBE_FRAME: u64 = 0;

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
// Mirrored C structs (from r_defs.h) — only fields read by r_bsp.c
// ---------------------------------------------------------------------------

/// A map vertex holding a 2-D fixed-point position.
/// Mirrors `vertex_t` from `r_defs.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct vertex_t {
    /// X coordinate in fixed-point map units.
    pub x: fixed_t,
    /// Y coordinate in fixed-point map units.
    pub y: fixed_t,
}

/// Opaque forward-declaration of a map object (monster, item, player, etc.).
/// `r_bsp.rs` holds pointers to these but never dereferences them directly.
/// Mirrors `mobj_t` / `mobj_s` from `p_mobj.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mobj_s {
    _opaque: [u8; 0],
}
/// Type alias for [`mobj_s`], matching the C `mobj_t` typedef.
pub type mobj_t = mobj_s;

// thinker_t is 24 bytes in C (prev/next/function pointers).
// r_bsp.rs never dereferences it, but it must have the correct size
// so that sector_t (which contains degenmobj_t, which contains thinker_t)
// matches the C layout of 128 bytes.

/// Thinker linked-list node. Never dereferenced by this module; included
/// only to maintain the correct `sector_t` layout (128 bytes on x86-64).
/// Mirrors `thinker_t` / `thinker_s` from `p_tick.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct thinker_s {
    _prev: *mut c_void,
    _next: *mut c_void,
    _function: *mut c_void,
}
/// Type alias for [`thinker_s`], matching the C `thinker_t` typedef.
pub type thinker_t = thinker_s;

/// A degenerate map object used as a sound origin embedded inside `sector_t`.
/// Contains a [`thinker_t`] prefix followed by map-unit coordinates.
/// Mirrors `degenmobj_t` / `degenmobj_s` from `p_mobj.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct degenmobj_s {
    /// Thinker header (required for correct sector_t layout).
    pub thinker: thinker_t,
    /// X position of the sound origin in map units.
    pub x: fixed_t,
    /// Y position of the sound origin in map units.
    pub y: fixed_t,
    /// Z position of the sound origin in map units.
    pub z: fixed_t,
}

/// A map sector describing the floor/ceiling geometry and lighting of a
/// convex region. Mirrors `sector_t` from `r_defs.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct sector_t {
    /// Floor height in fixed-point map units.
    pub floorheight: fixed_t,
    /// Ceiling height in fixed-point map units.
    pub ceilingheight: fixed_t,
    /// Flat lump index for the floor texture.
    pub floorpic: c_short,
    /// Flat lump index for the ceiling texture.
    pub ceilingpic: c_short,
    /// Ambient light level (0-255).
    pub lightlevel: c_short,
    /// Special effect number (damage, secret, etc.).
    pub special: c_short,
    /// Sector tag used to link with linedef specials.
    pub tag: c_short,
    /// Sound traversal counter (set during sound propagation).
    pub soundtraversed: c_int,
    /// Last thing to make a sound in this sector.
    pub soundtarget: *mut mobj_t,
    /// Bounding box used for blockmap queries (`BOXLEFT/BOXRIGHT/BOXTOP/BOXBOTTOM`).
    pub blockbox: [c_int; 4],
    /// Degenerate mobj used as the sector's spatial sound origin.
    pub soundorg: degenmobj_s,
    /// Validity counter for single-pass linedef and thing traversal.
    pub validcount: c_int,
    /// Head of the linked list of things (mobjs) in this sector.
    pub thinglist: *mut mobj_t,
    /// Pointer to sector-specific special state (e.g. moving floor/ceiling).
    pub specialdata: *mut c_void,
    /// Number of linedefs bounding this sector.
    pub linecount: c_int,
    /// Pointer to the array of linedef pointers bounding this sector.
    pub lines: *mut *mut line_s,
}

/// One side of a two-sided linedef, carrying texture and sector references.
/// Mirrors `side_t` from `r_defs.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct side_t {
    /// Horizontal texture offset in fixed-point units.
    pub textureoffset: fixed_t,
    /// Vertical texture offset in fixed-point units.
    pub rowoffset: fixed_t,
    /// Upper (above back sector ceiling) texture number; 0 = none.
    pub toptexture: c_short,
    /// Lower (below back sector floor) texture number; 0 = none.
    pub bottomtexture: c_short,
    /// Middle texture number; 0 = none.
    pub midtexture: c_short,
    /// The sector this sidedef faces.
    pub sector: *mut sector_t,
}

/// Line slope type, used to select the correct axis-aligned bounding-box
/// intersection test for a linedef. Mirrors `slopetype_t` from `r_defs.h`.
#[repr(C)]
pub enum slopetype_t {
    /// Line is perfectly horizontal (dy == 0).
    ST_HORIZONTAL,
    /// Line is perfectly vertical (dx == 0).
    ST_VERTICAL,
    /// Line has positive slope (dy/dx > 0).
    ST_POSITIVE,
    /// Line has negative slope (dy/dx < 0).
    ST_NEGATIVE,
}

/// A map linedef connecting two vertices and potentially separating two
/// sectors. Mirrors `line_t` / `line_s` from `r_defs.h`.
#[repr(C)]
pub struct line_s {
    /// First vertex (start of the line).
    pub v1: *mut vertex_t,
    /// Second vertex (end of the line).
    pub v2: *mut vertex_t,
    /// Horizontal delta `v2.x - v1.x` in fixed-point units.
    pub dx: fixed_t,
    /// Vertical delta `v2.y - v1.y` in fixed-point units.
    pub dy: fixed_t,
    /// Linedef flags (ML_* constants from `doomdef.h`).
    pub flags: c_short,
    /// Special action number (door, platform, exit, etc.).
    pub special: c_short,
    /// Tag linking this linedef to a sector for special activations.
    pub tag: c_short,
    /// Side numbers: `sidenum[0]` = front, `sidenum[1]` = back (-1 = none).
    pub sidenum: [c_short; 2],
    /// Axis-aligned bounding box of the linedef.
    pub bbox: [fixed_t; 4],
    /// Pre-computed slope type for fast bbox intersection.
    pub slopetype: slopetype_t,
    /// Sector on the front (right) side of the linedef.
    pub frontsector: *mut sector_t,
    /// Sector on the back (left) side; null for single-sided lines.
    pub backsector: *mut sector_t,
    /// Validity counter to avoid processing the same linedef twice per query.
    pub validcount: c_int,
    /// Pointer to active special state (e.g. a triggered door thinker).
    pub specialdata: *mut c_void,
}
/// Type alias for [`line_s`], matching the C `line_t` typedef.
pub type line_t = line_s;

/// A BSP leaf convex region made up of one or more segs from the same sector.
/// Mirrors `subsector_t` / `subsector_s` from `r_defs.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct subsector_s {
    /// The sector this subsector belongs to.
    pub sector: *mut sector_t,
    /// Number of segs in this subsector.
    pub numlines: c_short,
    /// Index of the first seg in the global `segs` array.
    pub firstline: c_short,
}
/// Type alias for [`subsector_s`], matching the C `subsector_t` typedef.
pub type subsector_t = subsector_s;

/// A wall segment (part of a linedef's front side) used during rendering.
/// Each seg carries pre-computed angle and texture-offset data.
/// Mirrors `seg_t` from `r_defs.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct seg_t {
    /// Start vertex of this seg.
    pub v1: *mut vertex_t,
    /// End vertex of this seg.
    pub v2: *mut vertex_t,
    /// Horizontal texture offset along the linedef in fixed-point units.
    pub offset: fixed_t,
    /// Absolute BAM angle of this seg (v1 to v2).
    pub angle: angle_t,
    /// The sidedef that provides texture information for this seg.
    pub sidedef: *mut side_t,
    /// The linedef this seg belongs to.
    pub linedef: *mut line_t,
    /// The sector on the front side of this seg.
    pub frontsector: *mut sector_t,
    /// The sector on the back side of this seg; null for single-sided lines.
    pub backsector: *mut sector_t,
}

/// An interior BSP tree node with a splitting line and two child references.
/// Children are either node indices or, when `NF_SUBSECTOR` is set, subsector
/// leaf indices. Mirrors `node_t` from `r_defs.h`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct node_t {
    /// X coordinate of the splitting line's origin in fixed-point map units.
    pub x: fixed_t,
    /// Y coordinate of the splitting line's origin in fixed-point map units.
    pub y: fixed_t,
    /// Horizontal delta of the splitting line vector.
    pub dx: fixed_t,
    /// Vertical delta of the splitting line vector.
    pub dy: fixed_t,
    /// Axis-aligned bounding boxes for the two child subtrees:
    /// `bbox[0]` = right child, `bbox[1]` = left child, each encoded as
    /// `[TOP, BOTTOM, LEFT, RIGHT]` in fixed-point map units.
    pub bbox: [[fixed_t; 4]; 2],
    /// Child references: `children[0]` = right, `children[1]` = left.
    /// If `NF_SUBSECTOR` is set in the value, the lower bits are a subsector
    /// index; otherwise the value is a node index.
    pub children: [u16; 2],
}

// ---------------------------------------------------------------------------
// visplane_t — already exported from r_plane.rs, mirror locally for field access
// ---------------------------------------------------------------------------

/// Screen width constant used for the local `visplane_t` mirror.
/// Must match the value in `r_plane.rs`.
const SCREENWIDTH_RP: usize = 320;

/// A horizontal floor/ceiling span to be drawn at a fixed height and texture.
/// Mirrored locally from `r_plane.rs` so that `r_plane` pointer fields can
/// be written from this module. The struct must match the C `visplane_t` layout
/// from `r_plane.h` exactly.
#[repr(C)]
#[derive(Clone, Copy)]
struct visplane_t {
    /// Height of this plane in fixed-point map units.
    pub height: fixed_t,
    /// Flat lump index for this plane's texture.
    pub picnum: c_int,
    /// Light level for this plane (0-255).
    pub lightlevel: c_int,
    /// Leftmost screen column covered by this plane.
    pub minx: c_int,
    /// Rightmost screen column covered by this plane.
    pub maxx: c_int,
    /// Padding byte before the `top` array (matches C struct layout).
    pub pad1: u8,
    /// Per-column top clip (y coordinate); `0xff` means unset.
    pub top: [u8; SCREENWIDTH_RP],
    /// Padding byte between `top` and `bottom` arrays.
    pub pad2: u8,
    /// Padding byte before the `bottom` array.
    pub pad3: u8,
    /// Per-column bottom clip (y coordinate); `0xff` means unset.
    pub bottom: [u8; SCREENWIDTH_RP],
    /// Padding byte after the `bottom` array (matches C struct layout).
    pub pad4: u8,
}

// ---------------------------------------------------------------------------
// drawseg_t — MUST match C layout exactly (r_segs.c writes every field)
// ---------------------------------------------------------------------------

/// A seg prepared for the column renderer, holding scale, silhouette, and
/// sprite-clip arrays. Written by `R_StoreWallRange` and read back during
/// sprite clipping in `R_DrawSprite`. Must match the C `drawseg_t` layout
/// from `r_segs.h` exactly (verified by compile-time assertions in
/// `layout_checks`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct drawseg_t {
    /// The seg that generated this draw-seg.
    pub curline: *mut seg_t,
    /// Leftmost screen column of this draw-seg (inclusive).
    pub x1: c_int,
    /// Rightmost screen column of this draw-seg (inclusive).
    pub x2: c_int,
    /// Projection scale at column `x1`.
    pub scale1: fixed_t,
    /// Projection scale at column `x2`.
    pub scale2: fixed_t,
    /// Per-column scale increment: `(scale2 - scale1) / (x2 - x1)`.
    pub scalestep: fixed_t,
    /// Silhouette flags (`SIL_*`) indicating which edges clip sprites.
    pub silhouette: c_int,
    /// Bottom silhouette height in fixed-point units (for lower-unpegged walls).
    pub bsilheight: fixed_t,
    /// Top silhouette height in fixed-point units (for upper-unpegged walls).
    pub tsilheight: fixed_t,
    /// Pointer into the sprite-top clip column array; null if not needed.
    pub sprtopclip: *mut c_short,
    /// Pointer into the sprite-bottom clip column array; null if not needed.
    pub sprbottomclip: *mut c_short,
    /// Pointer into the masked-texture column array; null for solid walls.
    pub maskedtexturecol: *mut c_short,
}

/// Compile-time zero initializer for [`drawseg_t`], used to fill the static
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

// Compile-time size and offset checks for structs that must match C layout
// (values verified against vendor/doomgeneric C structs on x86_64 Linux)
/// Compile-time layout assertions ensuring all mirrored C structs have the
/// correct size and field offsets on x86-64 Linux. A compile error here means
/// the Rust struct has diverged from the C layout and ABI compatibility is
/// broken.
#[cfg(target_pointer_width = "64")]
mod layout_checks {
    // This module is intentionally private; it exists only for compile-time assertions.
    use super::*;
    const _: () = assert!(std::mem::size_of::<vertex_t>() == 8);
    const _: () = assert!(std::mem::size_of::<sector_t>() == 128);
    const _: () = assert!(std::mem::offset_of!(sector_t, floorheight) == 0);
    const _: () = assert!(std::mem::offset_of!(sector_t, ceilingheight) == 4);
    const _: () = assert!(std::mem::offset_of!(sector_t, floorpic) == 8);
    const _: () = assert!(std::mem::offset_of!(sector_t, ceilingpic) == 10);
    const _: () = assert!(std::mem::offset_of!(sector_t, lightlevel) == 12);
    const _: () = assert!(std::mem::offset_of!(sector_t, special) == 14);
    const _: () = assert!(std::mem::offset_of!(sector_t, tag) == 16);
    const _: () = assert!(std::mem::offset_of!(sector_t, soundtraversed) == 20);
    const _: () = assert!(std::mem::offset_of!(sector_t, soundtarget) == 24);
    const _: () = assert!(std::mem::offset_of!(sector_t, blockbox) == 32);
    const _: () = assert!(std::mem::offset_of!(sector_t, soundorg) == 48);
    const _: () = assert!(std::mem::offset_of!(sector_t, validcount) == 88);
    const _: () = assert!(std::mem::offset_of!(sector_t, thinglist) == 96);
    const _: () = assert!(std::mem::offset_of!(sector_t, specialdata) == 104);
    const _: () = assert!(std::mem::offset_of!(sector_t, linecount) == 112);
    const _: () = assert!(std::mem::offset_of!(sector_t, lines) == 120);

    const _: () = assert!(std::mem::size_of::<side_t>() == 24);
    const _: () = assert!(std::mem::offset_of!(side_t, textureoffset) == 0);
    const _: () = assert!(std::mem::offset_of!(side_t, rowoffset) == 4);
    const _: () = assert!(std::mem::offset_of!(side_t, toptexture) == 8);
    const _: () = assert!(std::mem::offset_of!(side_t, bottomtexture) == 10);
    const _: () = assert!(std::mem::offset_of!(side_t, midtexture) == 12);
    const _: () = assert!(std::mem::offset_of!(side_t, sector) == 16);

    const _: () = assert!(std::mem::size_of::<slopetype_t>() == 4);

    const _: () = assert!(std::mem::size_of::<line_t>() == 88);
    const _: () = assert!(std::mem::offset_of!(line_t, v1) == 0);
    const _: () = assert!(std::mem::offset_of!(line_t, v2) == 8);
    const _: () = assert!(std::mem::offset_of!(line_t, dx) == 16);
    const _: () = assert!(std::mem::offset_of!(line_t, dy) == 20);
    const _: () = assert!(std::mem::offset_of!(line_t, flags) == 24);
    const _: () = assert!(std::mem::offset_of!(line_t, special) == 26);
    const _: () = assert!(std::mem::offset_of!(line_t, tag) == 28);
    const _: () = assert!(std::mem::offset_of!(line_t, sidenum) == 30);
    const _: () = assert!(std::mem::offset_of!(line_t, bbox) == 36);
    const _: () = assert!(std::mem::offset_of!(line_t, slopetype) == 52);
    const _: () = assert!(std::mem::offset_of!(line_t, frontsector) == 56);
    const _: () = assert!(std::mem::offset_of!(line_t, backsector) == 64);
    const _: () = assert!(std::mem::offset_of!(line_t, validcount) == 72);
    const _: () = assert!(std::mem::offset_of!(line_t, specialdata) == 80);

    const _: () = assert!(std::mem::size_of::<subsector_t>() == 16);
    const _: () = assert!(std::mem::offset_of!(subsector_t, sector) == 0);
    const _: () = assert!(std::mem::offset_of!(subsector_t, numlines) == 8);
    const _: () = assert!(std::mem::offset_of!(subsector_t, firstline) == 10);

    const _: () = assert!(std::mem::size_of::<seg_t>() == 56);
    const _: () = assert!(std::mem::offset_of!(seg_t, v1) == 0);
    const _: () = assert!(std::mem::offset_of!(seg_t, v2) == 8);
    const _: () = assert!(std::mem::offset_of!(seg_t, offset) == 16);
    const _: () = assert!(std::mem::offset_of!(seg_t, angle) == 20);
    const _: () = assert!(std::mem::offset_of!(seg_t, sidedef) == 24);
    const _: () = assert!(std::mem::offset_of!(seg_t, linedef) == 32);
    const _: () = assert!(std::mem::offset_of!(seg_t, frontsector) == 40);
    const _: () = assert!(std::mem::offset_of!(seg_t, backsector) == 48);

    const _: () = assert!(std::mem::size_of::<node_t>() == 52);
    const _: () = assert!(std::mem::offset_of!(node_t, x) == 0);
    const _: () = assert!(std::mem::offset_of!(node_t, y) == 4);
    const _: () = assert!(std::mem::offset_of!(node_t, dx) == 8);
    const _: () = assert!(std::mem::offset_of!(node_t, dy) == 12);
    const _: () = assert!(std::mem::offset_of!(node_t, bbox) == 16);
    const _: () = assert!(std::mem::offset_of!(node_t, children) == 48);

    const _: () = assert!(std::mem::size_of::<drawseg_t>() == 64);
    const _: () = assert!(std::mem::offset_of!(drawseg_t, curline) == 0);
    const _: () = assert!(std::mem::offset_of!(drawseg_t, x1) == 8);
    const _: () = assert!(std::mem::offset_of!(drawseg_t, x2) == 12);
    const _: () = assert!(std::mem::offset_of!(drawseg_t, scale1) == 16);
    const _: () = assert!(std::mem::offset_of!(drawseg_t, scale2) == 20);
    const _: () = assert!(std::mem::offset_of!(drawseg_t, scalestep) == 24);
    const _: () = assert!(std::mem::offset_of!(drawseg_t, silhouette) == 28);
    const _: () = assert!(std::mem::offset_of!(drawseg_t, bsilheight) == 32);
    const _: () = assert!(std::mem::offset_of!(drawseg_t, tsilheight) == 36);
    const _: () = assert!(std::mem::offset_of!(drawseg_t, sprtopclip) == 40);
    const _: () = assert!(std::mem::offset_of!(drawseg_t, sprbottomclip) == 48);
    const _: () = assert!(std::mem::offset_of!(drawseg_t, maskedtexturecol) == 56);

    const _: () = assert!(std::mem::size_of::<thinker_t>() == 24);
    const _: () = assert!(std::mem::size_of::<degenmobj_s>() == 40);
}

// ---------------------------------------------------------------------------
// cliprange_t (internal)
// ---------------------------------------------------------------------------

/// A solid horizontal screen-column range `[first, last]` that has been fully
/// covered by a previously drawn wall. The `solidsegs` array is a sorted,
/// non-overlapping list of these ranges, bounded by two sentinel entries.
/// Mirrors the `cliprange_t` struct defined locally in `r_bsp.c`.
#[repr(C)]
#[derive(Clone, Copy)]
struct cliprange_t {
    /// First (leftmost) solid column in this range, inclusive.
    first: c_int,
    /// Last (rightmost) solid column in this range, inclusive.
    last: c_int,
}

// ---------------------------------------------------------------------------
// Globals exported with #[no_mangle]
// ---------------------------------------------------------------------------

/// The seg currently being processed by `R_AddLine` and passed down to
/// [`R_StoreWallRange`]. Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut curline: *mut seg_t = ptr::null_mut();

/// The sidedef of the current seg (`curline->sidedef`). Set by `R_StoreWallRange`
/// in `r_segs.c`. Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut sidedef: *mut side_t = ptr::null_mut();

/// The linedef of the current seg (`curline->linedef`). Set by `R_StoreWallRange`
/// in `r_segs.c`. Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut linedef: *mut line_t = ptr::null_mut();

/// The sector on the front side of the current seg. Set by [`R_Subsector`]
/// and read by `r_segs.c` and `r_plane.rs`. Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut frontsector: *mut sector_t = ptr::null_mut();

/// The sector on the back side of the current seg, or null for single-sided
/// lines. Set by `R_AddLine` and read by `r_segs.c`.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut backsector: *mut sector_t = ptr::null_mut();

/// Fixed-size array of prepared draw-segs for the current frame. Filled
/// sequentially via [`ds_p`]. Exported as `#[no_mangle]` for C callers
/// (read during sprite clipping in `r_things.c`).
#[no_mangle]
pub static mut drawsegs: [drawseg_t; MAXDRAWSEGS] = [ZERO_DRAWSEG; MAXDRAWSEGS];

/// Pointer to the next free slot in [`drawsegs`]. Advanced by
/// `R_StoreWallRange` each time a new draw-seg is committed.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut ds_p: *mut drawseg_t = ptr::null_mut();

// ---------------------------------------------------------------------------
// Module-local state
// ---------------------------------------------------------------------------

/// Sorted array of solid screen-column ranges (clip list). Entries
/// `[0..newend)` are valid; the first and last entries are permanent
/// sentinels initialized by [`R_ClearClipSegs`].
static mut solidsegs: [cliprange_t; MAXSEGS] = [cliprange_t { first: 0, last: 0 }; MAXSEGS];

/// One-past-the-end pointer into [`solidsegs`], tracking how many ranges are
/// currently active. Maintained by [`R_ClipSolidWallSegment`] and reset by
/// [`R_ClearClipSegs`].
static mut newend: *mut cliprange_t = ptr::null_mut();

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
static CHECKCOORD: [[c_int; 4]; 12] = [
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

use crate::doom::p_setup::{nodes, segs, subsectors};
use crate::doom::r_draw::viewwidth;
use crate::doom::r_main::{
    clipangle, sscount, viewangle, viewangletox, viewx, viewy, viewz, R_PointOnSide, R_PointToAngle,
};
use crate::doom::r_plane::{ceilingplane, floorplane, R_FindPlane};
use crate::doom::r_segs::{rw_angle1, R_StoreWallRange};
use crate::doom::r_sky::skyflatnum;
use crate::doom::r_things::R_AddSprites;

// ---------------------------------------------------------------------------
// R_ClearDrawSegs
// ---------------------------------------------------------------------------

/// Resets the draw-seg write pointer to the beginning of [`drawsegs`],
/// discarding all draw-segs accumulated during the previous frame.
/// Called once per frame by `R_RenderPlayerView` before BSP traversal begins.
///
/// # Safety
/// Must be called from the render thread. Mutates the global [`ds_p`].
#[no_mangle]
pub unsafe extern "C" fn R_ClearDrawSegs() {
    ds_p = std::ptr::addr_of_mut!(drawsegs[0]);
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
/// and `PROBE_FRAME`. Reads [`viewwidth`] and [`viewangle`].
#[no_mangle]
pub unsafe extern "C" fn R_ClearClipSegs() {
    solidsegs[0].first = -0x7fffffff;
    solidsegs[0].last = -1;
    solidsegs[1].first = viewwidth;
    solidsegs[1].last = 0x7fffffff;
    newend = std::ptr::addr_of_mut!(solidsegs[0]).add(2);

    // Diagnostic: frame marker. R_ClearClipSegs is called once per frame by
    // R_RenderPlayerView before descending the BSP.
    PROBE_FRAME = PROBE_FRAME.wrapping_add(1);
    log::trace!(
        "=== FRAME {} === viewx={:#x} viewy={:#x} viewangle={:#x} viewwidth={}",
        { PROBE_FRAME },
        { viewx },
        { viewy },
        { viewangle },
        { viewwidth }
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
/// Corresponds to `R_ClipSolidWallSegment` in `r_bsp.c`.
///
/// # Safety
/// Caller must ensure `solidsegs` and `newend` have been initialized by
/// [`R_ClearClipSegs`] before this frame. `first` must be <= `last` for a
/// valid seg (a debug log is emitted if violated, but the function still runs
/// to match C behavior). Mutates the global `solidsegs` array and `newend`.
#[no_mangle]
pub unsafe extern "C" fn R_ClipSolidWallSegment(first: c_int, last: c_int) {
    if first > last {
        log::debug!(
            "R_ClipSolidWallSegment INVALID: first={} > last={}",
            first,
            last
        );
    }
    let solidsegs_base = std::ptr::addr_of_mut!(solidsegs[0]);

    let mut start = solidsegs_base;
    while (*start).last < first - 1 {
        start = start.add(1);
    }

    if first < (*start).first {
        if last < (*start).first - 1 {
            R_StoreWallRange(first, last);
            let mut next = newend;
            newend = next.add(1);

            while next != start {
                *next = *next.sub(1);
                next = next.sub(1);
            }
            (*next).first = first;
            (*next).last = last;
            return;
        }

        R_StoreWallRange(first, (*start).first - 1);
        (*start).first = first;
    }

    if last <= (*start).last {
        return;
    }

    let mut next = start;
    loop {
        let next_plus_1 = next.add(1);
        if last < (*next_plus_1).first - 1 {
            break;
        }

        R_StoreWallRange((*next).last + 1, (*next_plus_1).first - 1);
        next = next.add(1);

        if last <= (*next).last {
            (*start).last = (*next).last;
            // "goto crunch"
            if next == start {
                return;
            }
            // C: while (next++ != newend) { *++start = *next; }
            loop {
                let old_next = next;
                next = next.add(1);
                if old_next == newend {
                    break;
                }
                start = start.add(1);
                *start = *next;
            }
            newend = start.add(1);
            return;
        }
    }

    R_StoreWallRange((*next).last + 1, last);
    (*start).last = last;

    // crunch:
    if next == start {
        return;
    }

    // C: while (next++ != newend) { *++start = *next; }
    loop {
        let old_next = next;
        next = next.add(1);
        if old_next == newend {
            break;
        }
        start = start.add(1);
        *start = *next;
    }
    newend = start.add(1);
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
/// to match C behavior). Reads the global `solidsegs` array without
/// modifying it.
#[no_mangle]
pub unsafe extern "C" fn R_ClipPassWallSegment(first: c_int, last: c_int) {
    if first > last {
        log::debug!(
            "R_ClipPassWallSegment INVALID: first={} > last={}",
            first,
            last
        );
    }
    let solidsegs_base = std::ptr::addr_of_mut!(solidsegs[0]);

    let mut start = solidsegs_base;
    while (*start).last < first - 1 {
        start = start.add(1);
    }

    if first < (*start).first {
        if last < (*start).first - 1 {
            R_StoreWallRange(first, last);
            return;
        }

        R_StoreWallRange(first, (*start).first - 1);
    }

    if last <= (*start).last {
        return;
    }

    loop {
        let start_plus_1 = start.add(1);
        if last < (*start_plus_1).first - 1 {
            break;
        }

        R_StoreWallRange((*start).last + 1, (*start_plus_1).first - 1);
        start = start.add(1);

        if last <= (*start).last {
            return;
        }
    }

    R_StoreWallRange((*start).last + 1, last);
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
/// Sets the global [`curline`] to `line` and [`backsector`] to the back
/// sector pointer before calling the clip functions.
///
/// Corresponds to `R_AddLine` in `r_bsp.c`.
///
/// # Safety
/// `line` must point to a valid, initialized [`seg_t`] whose `v1`, `v2`,
/// `sidedef`, `linedef`, `frontsector`, and (if non-null) `backsector`
/// pointers are all valid. Globals [`viewangle`], [`clipangle`],
/// [`viewangletox`], [`frontsector`], and [`rw_angle1`] must have been
/// initialized before the current frame. Must be called only during BSP
/// traversal (i.e. within [`R_Subsector`]).
unsafe fn R_AddLine(line: *mut seg_t) {
    curline = line;

    let orig_angle1 = R_PointToAngle((*(*line).v1).x, (*(*line).v1).y);
    let orig_angle2 = R_PointToAngle((*(*line).v2).x, (*(*line).v2).y);

    let span = orig_angle1.wrapping_sub(orig_angle2);

    // Back side?
    if span >= 0x8000_0000 {
        log::trace!(
            "R_AddLine SKIP backface: orig_a1={:#x} orig_a2={:#x} va={:#x}",
            orig_angle1,
            orig_angle2,
            { viewangle }
        );
        return;
    }

    rw_angle1 = orig_angle1;
    let mut angle1 = orig_angle1.wrapping_sub(viewangle);
    let mut angle2 = orig_angle2.wrapping_sub(viewangle);

    let clipangle_d2 = clipangle.wrapping_mul(2);

    let mut tspan = angle1.wrapping_add(clipangle);
    if tspan > clipangle_d2 {
        tspan = tspan.wrapping_sub(clipangle_d2);
        if tspan >= span {
            log::trace!("R_AddLine SKIP off-left: orig_a1={:#x} orig_a2={:#x} va={:#x} a1={:#x} a2={:#x} span={:#x} tspan={:#x}", orig_angle1, orig_angle2, { viewangle }, angle1, angle2, span, tspan);
            return;
        }
        angle1 = clipangle;
    }

    tspan = clipangle.wrapping_sub(angle2);
    if tspan > clipangle_d2 {
        tspan = tspan.wrapping_sub(clipangle_d2);
        if tspan >= span {
            log::trace!("R_AddLine SKIP off-right: orig_a1={:#x} orig_a2={:#x} va={:#x} a1={:#x} a2={:#x} span={:#x} tspan={:#x}", orig_angle1, orig_angle2, { viewangle }, angle1, angle2, span, tspan);
            return;
        }
        angle2 = 0u32.wrapping_sub(clipangle);
    }

    let idx1 = ((angle1.wrapping_add(ANG90)) >> ANGLETOFINESHIFT) as usize;
    let idx2 = ((angle2.wrapping_add(ANG90)) >> ANGLETOFINESHIFT) as usize;
    let x1 = viewangletox[idx1];
    let x2 = viewangletox[idx2];

    // Log walls that land in the right portion of the screen
    if x2 >= 200 || x1 >= 200 {
        log::trace!(
            "R_AddLine wall: orig_a1={:#x} orig_a2={:#x} va={:#x} clip={:#x} a1={:#x} a2={:#x} idx1={} idx2={} x1={} x2={}",
            orig_angle1, orig_angle2, { viewangle }, { clipangle }, angle1, angle2, idx1, idx2, x1, x2
        );
    }
    // Diagnostic: log every surviving wall (full screen) at TRACE.
    log::trace!(
        "R_AddLine projected: frame={} seg={} x1={} x2={} a1={:#x} a2={:#x}",
        { PROBE_FRAME },
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

    backsector = (*line).backsector;

    // Single sided line?
    if backsector.is_null() {
        log::trace!(
            "R_AddLine classify: frame={} seg={} x1={} x2={} decision=SOLID(1-sided) back=null front={:p}",
            { PROBE_FRAME }, seg_index(line), x1, x2, frontsector as *const _
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
            { PROBE_FRAME }, seg_index(line), x1, x2, backsector as *const _,
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
            { PROBE_FRAME }, seg_index(line), x1, x2, backsector as *const _,
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
            { PROBE_FRAME },
            seg_index(line),
            x1,
            x2
        );
        return;
    }

    log::trace!(
        "R_AddLine classify: frame={} seg={} x1={} x2={} decision=PASS(midtex/lighting) midtex={}",
        { PROBE_FRAME },
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
/// Globals [`viewx`], [`viewy`], [`viewangle`], [`clipangle`],
/// [`viewangletox`], and `solidsegs` must have been initialized for the
/// current frame.
#[no_mangle]
pub unsafe extern "C" fn R_CheckBBox(bspcoord: *mut fixed_t) -> c_int {
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

    let sx1 = viewangletox[((angle1.wrapping_add(ANG90)) >> ANGLETOFINESHIFT) as usize];
    let sx2 = viewangletox[((angle2.wrapping_add(ANG90)) >> ANGLETOFINESHIFT) as usize];

    if sx1 == sx2 {
        return 0;
    }
    let sx2 = sx2 - 1;

    let solidsegs_base = std::ptr::addr_of!(solidsegs[0]);
    let mut start = solidsegs_base;
    while (*start).last < sx2 {
        start = start.add(1);
    }

    if sx1 >= (*start).first && sx2 <= (*start).last {
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
/// 2. Sets [`frontsector`] from the subsector's sector pointer.
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
/// pointer and all `segs` it references must be valid. Globals [`viewz`],
/// [`skyflatnum`], [`floorplane`], and [`ceilingplane`] must be accessible.
#[no_mangle]
pub unsafe extern "C" fn R_Subsector(num: c_int) {
    let num_usize = num as usize;

    sscount += 1;

    let sub = &*(subsectors.add(num_usize) as *mut subsector_t);
    frontsector = sub.sector;
    let mut count = sub.numlines as c_int;
    let mut line = segs.add(sub.firstline as usize) as *mut seg_t;

    if (*frontsector).floorheight < viewz {
        floorplane = R_FindPlane(
            (*frontsector).floorheight,
            (*frontsector).floorpic as c_int,
            (*frontsector).lightlevel as c_int,
        );
    } else {
        floorplane = ptr::null_mut();
    }

    if (*frontsector).ceilingheight > viewz || (*frontsector).ceilingpic as c_int == skyflatnum {
        ceilingplane = R_FindPlane(
            (*frontsector).ceilingheight,
            (*frontsector).ceilingpic as c_int,
            (*frontsector).lightlevel as c_int,
        );
    } else {
        ceilingplane = ptr::null_mut();
    }

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
/// `P_SetupLevel`. Globals [`viewx`], [`viewy`], and the occlusion list must
/// be initialized for the current frame.
#[no_mangle]
pub unsafe extern "C" fn R_RenderBSPNode(bspnum: c_int) {
    // Found a subsector?
    if (bspnum as u32) & NF_SUBSECTOR != 0 {
        let sub_num = if bspnum == -1 {
            0
        } else {
            (bspnum as u32 & !NF_SUBSECTOR) as c_int
        };
        log::trace!(
            "R_RenderBSPNode: frame={} leaf subsector={} (bspnum={:#x})",
            { PROBE_FRAME },
            sub_num,
            bspnum as u32
        );
        R_Subsector(sub_num);
        return;
    }

    let bsp = &*(nodes.add(bspnum as usize) as *const node_t);

    let side = R_PointOnSide(viewx, viewy, bsp);

    log::trace!(
        "R_RenderBSPNode: frame={} node={} side={} front_child={:#x} back_child={:#x}",
        { PROBE_FRAME },
        bspnum,
        side,
        bsp.children[side as usize] as u32,
        bsp.children[(side ^ 1) as usize] as u32,
    );

    R_RenderBSPNode(bsp.children[side as usize] as c_int);

    let back_visible = R_CheckBBox(bsp.bbox[(side ^ 1) as usize].as_ptr() as *mut fixed_t) != 0;
    let back_box = bsp.bbox[(side ^ 1) as usize];
    log::trace!(
        "R_RenderBSPNode: frame={} node={} back_visible={} back_bbox=[L={} R={} T={} B={}]",
        { PROBE_FRAME },
        bspnum,
        back_visible,
        back_box[BBox::LEFT],
        back_box[BBox::RIGHT],
        back_box[BBox::TOP],
        back_box[BBox::BOTTOM],
    );
    if back_visible {
        R_RenderBSPNode(bsp.children[(side ^ 1) as usize] as c_int);
    }
}

// ---------------------------------------------------------------------------
// Anchor so linker doesn't discard
// ---------------------------------------------------------------------------

/// Linker anchor: references every public `#[no_mangle]` function in this
/// module so the linker does not dead-strip them when building as a library.
/// Not intended to be called at runtime.
///
/// # Safety
/// Calls all exported functions with zero/null arguments purely to create
/// symbol references. Behavior is undefined if called during normal
/// execution; this function exists only to prevent linker GC.
#[no_mangle]
pub unsafe extern "C" fn R_Bsp_Link_Anchor() {
    R_ClearDrawSegs();
    R_ClearClipSegs();
    R_RenderBSPNode(0);
    R_Subsector(0);
    R_ClipSolidWallSegment(0, 0);
    R_ClipPassWallSegment(0, 0);
    R_CheckBBox(ptr::null_mut());
}
