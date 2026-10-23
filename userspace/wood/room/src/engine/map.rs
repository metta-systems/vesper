//! Shared map geometry types from C's `r_defs.h`.
//!
//! The level loader (`play::setup`) fills arrays of these records from the
//! WAD lumps; the play simulation and the 3D renderer both walk them. Each
//! type has exactly one definition here.
//!
//! The arrays are owned by the level state (`play::setup::LevelState`).
//! Records refer to each other by typed index ([`SectorId`], [`LineId`],
//! [`SideId`], [`VertexId`], [`SegId`], [`SubsectorId`], [`NodeId`]) where C
//! keeps pointers into those arrays; a nullable C pointer is an `Option` of
//! the id. A level array (a `Vec` or a slice of it) is indexed directly by
//! its id type.

#![allow(non_camel_case_types)]

use std::ops::{Index, IndexMut};

use crate::engine::{
    math::fixed::{angle_t, fixed_t},
    play::{mobj::MobjId, tick::ThinkerId},
};

/// Define a typed index into one of the level's geometry arrays, and let
/// that array (a slice of `$element`) be indexed by it.
macro_rules! map_id {
    ($(#[$doc:meta])* $name:ident => $element:ty) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
        pub struct $name(u32);

        impl $name {
            /// The id of the element at `index` in its level array.
            #[inline]
            pub const fn new(index: usize) -> Self {
                debug_assert!(index <= u32::MAX as usize);
                Self(index as u32)
            }

            /// Position of the element in its level array.
            #[inline]
            pub const fn index(self) -> usize {
                self.0 as usize
            }
        }

        impl Index<$name> for [$element] {
            type Output = $element;

            #[inline]
            fn index(&self, id: $name) -> &$element {
                &self[id.index()]
            }
        }

        impl IndexMut<$name> for [$element] {
            #[inline]
            fn index_mut(&mut self, id: $name) -> &mut $element {
                &mut self[id.index()]
            }
        }

        impl Index<$name> for Vec<$element> {
            type Output = $element;

            #[inline]
            fn index(&self, id: $name) -> &$element {
                &self[id.index()]
            }
        }

        impl IndexMut<$name> for Vec<$element> {
            #[inline]
            fn index_mut(&mut self, id: $name) -> &mut $element {
                &mut self[id.index()]
            }
        }
    };
}

map_id! {
    /// Index of a [`vertex_t`] in the level's `vertexes`.
    VertexId => vertex_t
}
map_id! {
    /// Index of a [`sector_t`] in the level's `sectors`.
    SectorId => sector_t
}
map_id! {
    /// Index of a [`side_t`] in the level's `sides`.
    SideId => side_t
}
map_id! {
    /// Index of a [`line_t`] in the level's `lines`.
    LineId => line_t
}
map_id! {
    /// Index of a [`subsector_t`] in the level's `subsectors`.
    SubsectorId => subsector_t
}
map_id! {
    /// Index of a [`seg_t`] in the level's `segs`.
    SegId => seg_t
}
map_id! {
    /// Index of a [`node_t`] in the level's `nodes`.
    NodeId => node_t
}

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
    pub soundtarget: Option<MobjId>,
    /// Bounding box used for blockmap queries (`BOXTOP/BOXBOTTOM/BOXLEFT/BOXRIGHT`).
    pub blockbox: [i32; 4],
    /// Degenerate mobj used as the sector's spatial sound origin.
    pub soundorg: degenmobj_t,
    /// Validity counter for single-pass linedef and thing traversal.
    pub validcount: i32,
    /// Head of the linked list of things (mobjs) in this sector.
    pub thinglist: Option<MobjId>,
    /// The active special thinker for this sector (a door, a moving floor,
    /// ceiling or platform), or `None` if none is active.
    pub specialdata: Option<ThinkerId>,
    /// Number of linedefs bounding this sector.
    pub linecount: i32,
    /// Start of this sector's linedefs in the level's `linebuffer` (C keeps
    /// a pointer into that buffer); [`lines_in`](Self::lines_in) gives the
    /// `linecount` entries from there.
    pub lines: usize,
}

impl sector_t {
    /// An empty sector: every number zero, no things and no special (C's
    /// zeroed `sector_t`; the handle fields are `None`, which is not all
    /// zero bytes for `specialdata`, so the value is spelled out).
    pub const ZERO: Self = Self {
        floorheight: 0,
        ceilingheight: 0,
        floorpic: 0,
        ceilingpic: 0,
        lightlevel: 0,
        special: 0,
        tag: 0,
        soundtraversed: 0,
        soundtarget: None,
        blockbox: [0; 4],
        soundorg: degenmobj_t { x: 0, y: 0, z: 0 },
        validcount: 0,
        thinglist: None,
        specialdata: None,
        linecount: 0,
        lines: 0,
    };

    /// This sector's linedefs within the level's `linebuffer`.
    #[inline]
    pub fn lines_in<'a>(&self, linebuffer: &'a [LineId]) -> &'a [LineId] {
        &linebuffer[self.lines..self.lines + self.linecount as usize]
    }
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
    pub sector: SectorId,
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
    pub v1: VertexId,
    /// Second vertex (end of the line).
    pub v2: VertexId,
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
    /// Sector on the front (right) side of the linedef; `None` when the
    /// line has no front side (`sidenum[0] == -1`).
    pub frontsector: Option<SectorId>,
    /// Sector on the back (left) side; `None` for single-sided lines.
    pub backsector: Option<SectorId>,
    /// Validity counter to avoid processing the same linedef twice per query.
    pub validcount: i32,
    /// Active special thinker attached to this line, or `None` (C declares
    /// it; nothing sets it).
    pub specialdata: Option<ThinkerId>,
}

/// A BSP leaf convex region made up of one or more segs from the same sector
/// (`subsector_t` in `r_defs.h`).
#[derive(Clone, Copy)]
pub struct subsector_t {
    /// The sector this subsector belongs to.
    pub sector: SectorId,
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
    pub v1: VertexId,
    /// End vertex of this seg.
    pub v2: VertexId,
    /// Distance along the linedef to `v1`, in fixed-point units.
    pub offset: fixed_t,
    /// Absolute BAM angle of this seg (v1 to v2).
    pub angle: angle_t,
    /// The sidedef that provides texture information for this seg.
    pub sidedef: SideId,
    /// The linedef this seg belongs to.
    pub linedef: LineId,
    /// The sector on the front side of this seg.
    pub frontsector: SectorId,
    /// The sector on the back side of this seg; `None` for single-sided
    /// lines.
    pub backsector: Option<SectorId>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_index_their_arrays() {
        let vertexes = Vec::from([vertex_t { x: 1, y: 2 }, vertex_t { x: 3, y: 4 }]);
        let id = VertexId::new(1);
        assert_eq!(id.index(), 1);
        assert_eq!(vertexes[id].x, 3);
        assert_eq!(vertexes[..][id].y, 4);
        assert_ne!(VertexId::new(0), id);
    }

    #[test]
    fn sector_lines_are_a_window_of_the_line_buffer() {
        let linebuffer: Vec<LineId> = (0..6).map(LineId::new).collect();
        let mut sector = sector_t::ZERO;
        sector.lines = 2;
        sector.linecount = 3;
        assert_eq!(
            sector.lines_in(&linebuffer),
            &[LineId::new(2), LineId::new(3), LineId::new(4)]
        );
    }
}
