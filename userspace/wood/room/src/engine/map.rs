//! Shared map geometry types from C's `r_defs.h`.
//!
//! The level loader (`play::setup`) fills arrays of these records from the
//! WAD lumps; the play simulation and the 3D renderer both walk them. Each
//! type has exactly one definition here.

#![allow(non_camel_case_types)]

use std::ffi::c_void;

use crate::engine::{
    math::fixed::{angle_t, fixed_t},
    play::mobj::mobj_t,
};

/// A map vertex holding a 2-D fixed-point position (`vertex_t` in
/// `r_defs.h`).
#[derive(Clone, Copy)]
pub struct vertex_t {
    /// X coordinate in fixed-point map units.
    pub x: fixed_t,
    /// Y coordinate in fixed-point map units.
    pub y: fixed_t,
}

/// A degenerate map object used as a sound origin embedded inside
/// [`sector_t`] (`degenmobj_t` in `r_defs.h`). C gives it a thinker
/// header so it can pass for a `mobj_t`; sound origins are typed here, so
/// only the position is kept.
#[derive(Clone, Copy)]
pub struct degenmobj_t {
    /// X position of the sound origin in map units.
    pub x: fixed_t,
    /// Y position of the sound origin in map units.
    pub y: fixed_t,
    /// Z position of the sound origin in map units.
    pub z: fixed_t,
}

/// A map sector describing the floor/ceiling geometry and lighting of a
/// convex region (`sector_t` in `r_defs.h`).
#[derive(Clone, Copy)]
pub struct sector_t {
    /// Floor height in fixed-point map units.
    pub floorheight: fixed_t,
    /// Ceiling height in fixed-point map units.
    pub ceilingheight: fixed_t,
    /// Flat lump index for the floor texture.
    pub floorpic: i16,
    /// Flat lump index for the ceiling texture.
    pub ceilingpic: i16,
    /// Ambient light level (0-255).
    pub lightlevel: i16,
    /// Special effect number (damage, secret, etc.).
    pub special: i16,
    /// Sector tag used to link with linedef specials.
    pub tag: i16,
    /// Sound traversal counter (set during sound propagation).
    pub soundtraversed: i32,
    /// Last thing to make a sound in this sector (for monster alerting).
    pub soundtarget: *mut mobj_t,
    /// Bounding box used for blockmap queries (`BOXTOP/BOXBOTTOM/BOXLEFT/BOXRIGHT`).
    pub blockbox: [i32; 4],
    /// Degenerate mobj used as the sector's spatial sound origin.
    pub soundorg: degenmobj_t,
    /// Validity counter for single-pass linedef and thing traversal.
    pub validcount: i32,
    /// Head of the linked list of things (mobjs) in this sector.
    pub thinglist: *mut mobj_t,
    /// Active special thinker for this sector (e.g. a moving floor or
    /// ceiling), or null if none is active.
    pub specialdata: *mut c_void,
    /// Number of linedefs bounding this sector.
    pub linecount: i32,
    /// Pointer to the array of linedef pointers bounding this sector.
    pub lines: *mut *mut line_t,
}

/// One side of a linedef, carrying texture and sector references
/// (`side_t` in `r_defs.h`).
#[derive(Clone, Copy)]
pub struct side_t {
    /// Horizontal texture offset in fixed-point units.
    pub textureoffset: fixed_t,
    /// Vertical texture offset in fixed-point units.
    pub rowoffset: fixed_t,
    /// Upper (above back sector ceiling) texture number; 0 = none.
    pub toptexture: i16,
    /// Lower (below back sector floor) texture number; 0 = none.
    pub bottomtexture: i16,
    /// Middle texture number; 0 = none.
    pub midtexture: i16,
    /// The sector this sidedef faces.
    pub sector: *mut sector_t,
}

/// Line slope type, used to select the correct axis-aligned bounding-box
/// intersection test for a linedef (`slopetype_t` in `r_defs.h`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
/// sectors (`line_t` in `r_defs.h`).
#[derive(Clone, Copy)]
pub struct line_t {
    /// First vertex (start of the line).
    pub v1: *mut vertex_t,
    /// Second vertex (end of the line).
    pub v2: *mut vertex_t,
    /// Horizontal delta `v2.x - v1.x` in fixed-point units.
    pub dx: fixed_t,
    /// Vertical delta `v2.y - v1.y` in fixed-point units.
    pub dy: fixed_t,
    /// Linedef flags, a bit mask of `LinedefFlag` entries.
    pub flags: i16,
    /// Special action number (door, platform, exit, etc.).
    pub special: i16,
    /// Tag linking this linedef to a sector for special activations.
    pub tag: i16,
    /// Side numbers: `sidenum[0]` = front, `sidenum[1]` = back (-1 = none).
    pub sidenum: [i16; 2],
    /// Axis-aligned bounding box of the linedef.
    pub bbox: [fixed_t; 4],
    /// Pre-computed slope type for fast bbox intersection.
    pub slopetype: slopetype_t,
    /// Sector on the front (right) side of the linedef.
    pub frontsector: *mut sector_t,
    /// Sector on the back (left) side; null for single-sided lines.
    pub backsector: *mut sector_t,
    /// Validity counter to avoid processing the same linedef twice per query.
    pub validcount: i32,
    /// Active special thinker attached to this line (e.g. a triggered door),
    /// or null.
    pub specialdata: *mut c_void,
}

/// A BSP leaf convex region made up of one or more segs from the same sector
/// (`subsector_t` in `r_defs.h`).
#[derive(Clone, Copy)]
pub struct subsector_t {
    /// The sector this subsector belongs to.
    pub sector: *mut sector_t,
    /// Number of segs in this subsector.
    pub numlines: i16,
    /// Index of the first seg in the global `segs` array.
    pub firstline: i16,
}

/// A wall segment (part of a linedef's side) produced by the BSP split
/// (`seg_t` in `r_defs.h`).
#[derive(Clone, Copy)]
pub struct seg_t {
    /// Start vertex of this seg.
    pub v1: *mut vertex_t,
    /// End vertex of this seg.
    pub v2: *mut vertex_t,
    /// Distance along the linedef to `v1`, in fixed-point units.
    pub offset: fixed_t,
    /// Absolute BAM angle of this seg (v1 to v2).
    pub angle: angle_t,
    /// The sidedef that provides texture information for this seg.
    pub sidedef: *mut side_t,
    /// The linedef this seg belongs to.
    pub linedef: *mut line_t,
    /// The sector on the front side of this seg; never null.
    pub frontsector: *mut sector_t,
    /// The sector on the back side of this seg; null for single-sided lines.
    pub backsector: *mut sector_t,
}

/// An interior BSP tree node with a splitting line and two child references
/// (`node_t` in `r_defs.h`).
///
/// Children are either node indices or, when `NF_SUBSECTOR` is set,
/// subsector leaf indices.
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
