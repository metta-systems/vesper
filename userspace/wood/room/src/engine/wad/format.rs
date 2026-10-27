//! On-disk record formats of WAD lumps, plus the PCX screenshot header.
//!
//! Each type here is the single definition of a C struct from
//! `doomdata.h`, `w_wad.c`, `r_data.c`, `r_defs.h` or `v_video.c`. Fields
//! wider than a byte are `zerocopy` little-endian wrappers, which have
//! alignment 1: the structs match the byte layout on disk without
//! `packed`, can be viewed in place over unaligned lump bytes, and make
//! the WAD's little-endian byte order explicit, so readers call `.get()`
//! instead of a `SHORT`/`LONG` byte swap. Every type carries a
//! compile-time assertion of its on-disk size. The `ML_*` map lump
//! indices and linedef flag bits from `doomdata.h` live here as well, as
//! [`MapLump`] and [`LinedefFlag`].
//!
//! Records followed by a variable-length array in the WAD (`patch_t`,
//! `maptexture_t`) are defined as their fixed-size header only; the
//! trailing array is read separately (see each type).

#![allow(non_camel_case_types)]

use {
    core::mem::size_of,
    zerocopy::{
        FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned,
        little_endian::{I16, I32, U16},
    },
};

// ---------------------------------------------------------------------------
// WAD header and directory (w_wad.c)
// ---------------------------------------------------------------------------

/// On-disk WAD header. Read from offset 0 of every `.wad` file
/// loaded by `W_AddFile`. Layout matches `wadinfo_t` in `w_wad.c`.
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
pub struct wadinfo_t {
    /// Magic identifier: `"IWAD"` for the main IWAD, `"PWAD"` for
    /// a patch WAD. Anything else triggers `I_Error`.
    pub identification: [u8; 4],
    /// Number of lumps in the directory.
    pub numlumps: I32,
    /// Byte offset to the lump-directory table.
    pub infotableofs: I32,
}

const _: () = assert!(size_of::<wadinfo_t>() == 12);

/// On-disk directory entry as it appears at `infotableofs`. Mirrors
/// `filelump_t` in `w_wad.c`.
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
pub struct filelump_t {
    /// Byte offset of the lump payload inside the WAD.
    pub filepos: I32,
    /// Payload length in bytes.
    pub size: I32,
    /// 8-char ASCII lump name, NUL-padded.
    pub name: [u8; 8],
}

const _: () = assert!(size_of::<filelump_t>() == 16);

// ---------------------------------------------------------------------------
// Map lumps (doomdata.h)
// ---------------------------------------------------------------------------

/// WAD map lump indices (`ML_*` in `doomdata.h`). Used as offsets from the
/// map's header lump to locate each sub-lump: these are the fixed positions
/// of each data lump within a map's group of WAD lumps, and `p_setup.c`
/// indexes directly into the lump list using them.
pub struct MapLump;
/// `ML_*` lump-offset constants used to index a map's group of WAD lumps.
impl MapLump {
    #[doc(alias = "ML_LABEL")]
    pub const LABEL: i32 = 0; // ExMx / MAPxx separator
    #[doc(alias = "ML_THINGS")]
    pub const THINGS: i32 = 1; // Monster/item placement
    #[doc(alias = "ML_LINEDEFS")]
    pub const LINEDEFS: i32 = 2; // Line definitions
    #[doc(alias = "ML_SIDEDEFS")]
    pub const SIDEDEFS: i32 = 3; // Side (texture) definitions
    #[doc(alias = "ML_VERTEXES")]
    pub const VERTEXES: i32 = 4; // Vertex coordinates
    #[doc(alias = "ML_SEGS")]
    pub const SEGS: i32 = 5; // BSP line segments
    #[doc(alias = "ML_SSECTORS")]
    pub const SSECTORS: i32 = 6; // BSP sub-sectors
    #[doc(alias = "ML_NODES")]
    pub const NODES: i32 = 7; // BSP nodes
    #[doc(alias = "ML_SECTORS")]
    pub const SECTORS: i32 = 8; // Sector definitions
    #[doc(alias = "ML_REJECT")]
    pub const REJECT: i32 = 9; // Sector-to-sector visibility table
    #[doc(alias = "ML_BLOCKMAP")]
    pub const BLOCKMAP: i32 = 10; // Motion-clipping blockmap
}

/// On-disk vertex record from the WAD VERTEXES lump.
/// Maps to `mapvertex_t` in `doomdata.h`. Coordinates are 16-bit integers
/// in map units; they are sign-extended and shifted left by `FRACBITS` when
/// copied into the runtime `vertex_t`.
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
pub struct mapvertex_t {
    pub x: I16,
    pub y: I16,
}

const _: () = assert!(size_of::<mapvertex_t>() == 4);

/// On-disk sidedef record from the WAD SIDEDEFS lump.
/// Maps to `mapsidedef_t` in `doomdata.h`. Texture offsets are in map units
/// and are shifted left by `FRACBITS` when copied to the runtime `side_t`.
/// Texture name fields are 8-byte null-padded ASCII strings resolved to
/// runtime texture indices by `R_TextureNumForName`.
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
pub struct mapsidedef_t {
    pub textureoffset: I16,
    pub rowoffset: I16,
    pub toptexture: [u8; 8],
    pub bottomtexture: [u8; 8],
    pub midtexture: [u8; 8],
    pub sector: I16,
}

const _: () = assert!(size_of::<mapsidedef_t>() == 30);

/// On-disk linedef record from the WAD LINEDEFS lump.
/// Maps to `maplinedef_t` in `doomdata.h`. `sidenum[1]` is -1 for one-sided
/// lines; the runtime `line_t` stores null pointers in that case.
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
pub struct maplinedef_t {
    pub v1: I16,
    pub v2: I16,
    pub flags: I16,
    pub special: I16,
    pub tag: I16,
    pub sidenum: [I16; 2],
}

const _: () = assert!(size_of::<maplinedef_t>() == 14);

/// Linedef flag bits (`ML_*` in `doomdata.h`). Stored in `line_t.flags` as
/// a bitmask; each constant is a single-bit mask.
pub struct LinedefFlag;
/// `ML_*` bit-mask constants stored in `line_t.flags`.
impl LinedefFlag {
    #[doc(alias = "ML_BLOCKING")]
    pub const BLOCKING: u16 = 1; // Solid obstacle
    #[doc(alias = "ML_BLOCKMONSTERS")]
    pub const BLOCKMONSTERS: u16 = 2; // Blocks monsters only
    #[doc(alias = "ML_TWOSIDED")]
    pub const TWOSIDED: u16 = 4; // Has a back sector
    #[doc(alias = "ML_DONTPEGTOP")]
    pub const DONTPEGTOP: u16 = 8; // Upper texture is unpegged
    #[doc(alias = "ML_DONTPEGBOTTOM")]
    pub const DONTPEGBOTTOM: u16 = 16; // Lower texture is unpegged
    #[doc(alias = "ML_SECRET")]
    pub const SECRET: u16 = 32; // Secret on automap
    #[doc(alias = "ML_SOUNDBLOCK")]
    pub const SOUNDBLOCK: u16 = 64; // Sound propagation barrier
    #[doc(alias = "ML_DONTDRAW")]
    pub const DONTDRAW: u16 = 128; // Hidden on automap
    #[doc(alias = "ML_MAPPED")]
    pub const MAPPED: u16 = 256; // Already revealed on automap
}

/// On-disk sector record from the WAD SECTORS lump.
/// Maps to `mapsector_t` in `doomdata.h`. Floor/ceiling heights are in map
/// units and shifted left by `FRACBITS` in the runtime `sector_t`. Flat name
/// fields are 8-byte null-padded ASCII strings resolved via `R_FlatNumForName`.
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
pub struct mapsector_t {
    pub floorheight: I16,
    pub ceilingheight: I16,
    pub floorpic: [u8; 8],
    pub ceilingpic: [u8; 8],
    pub lightlevel: I16,
    pub special: I16,
    pub tag: I16,
}

const _: () = assert!(size_of::<mapsector_t>() == 26);

/// On-disk subsector record from the WAD SSECTORS lump.
/// Maps to `mapsubsector_t` in `doomdata.h`. `firstseg` is an index into the
/// segs array; `numsegs` is the count of consecutive segs forming the convex
/// polygon of this subsector.
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
pub struct mapsubsector_t {
    pub numsegs: I16,
    pub firstseg: I16,
}

const _: () = assert!(size_of::<mapsubsector_t>() == 4);

/// On-disk seg record from the WAD SEGS lump.
/// Maps to `mapseg_t` in `doomdata.h`. `angle` is a BAM (Binary Angle
/// Measurement) stored in the upper 16 bits of a `u32` after shifting.
/// `offset` is the distance along the linedef to the seg's start vertex,
/// in map units.
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
pub struct mapseg_t {
    pub v1: I16,
    pub v2: I16,
    pub angle: I16,
    pub linedef: I16,
    pub side: I16,
    pub offset: I16,
}

const _: () = assert!(size_of::<mapseg_t>() == 12);

/// On-disk BSP node record from the WAD NODES lump.
/// Maps to `mapnode_t` in `doomdata.h`. `x`, `y`, `dx`, `dy` define the
/// partition line in map units. `bbox[2][4]` are the bounding boxes for each
/// child subtree. `children[2]` are child indices; the high bit set indicates
/// a subsector leaf.
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
pub struct mapnode_t {
    pub x: I16,
    pub y: I16,
    pub dx: I16,
    pub dy: I16,
    pub bbox: [[I16; 4]; 2],
    pub children: [U16; 2],
}

const _: () = assert!(size_of::<mapnode_t>() == 28);

/// On-disk thing record from the WAD THINGS lump, and also the runtime spawn
/// descriptor passed to `P_SpawnMapThing`.
///
/// Maps to `mapthing_t` in `doomdata.h`. Besides the THINGS lump it is held
/// in the `deathmatchstarts`, `playerstarts` and `itemrespawnque` arrays and
/// in `mobj_t::spawnpoint`, so runtime code builds and edits it with
/// `I16::new` / `.set()`.
///
/// Fields:
/// - `x`, `y`: map-unit position (not fixed-point)
/// - `angle`: facing direction in degrees (0, 45, 90, ...)
/// - `type`: Doom editor number identifying the thing class
/// - `options`: bit flags (skill levels, deaf, etc.)
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
pub struct mapthing_t {
    pub x: I16,
    pub y: I16,
    pub angle: I16,
    pub kind: I16,
    pub options: I16,
}

const _: () = assert!(size_of::<mapthing_t>() == 10);

impl mapthing_t {
    /// An all-zero record, for initialising the spawn-point arrays.
    pub const ZERO: mapthing_t = mapthing_t {
        x: I16::ZERO,
        y: I16::ZERO,
        angle: I16::ZERO,
        kind: I16::ZERO,
        options: I16::ZERO,
    };
}

// ---------------------------------------------------------------------------
// Texture lumps (r_data.c)
// ---------------------------------------------------------------------------

/// On-disk patch entry inside a `maptexture_t` WAD record.
///
/// Corresponds to `mappatch_t` in `r_data.c`. `stepdir` and `colormap` are
/// present in the WAD format but unused by the renderer.
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
pub struct mappatch_t {
    /// Horizontal origin of the patch within the texture, in pixels.
    pub originx: I16,
    /// Vertical origin of the patch within the texture, in pixels.
    pub originy: I16,
    /// Index into the PNAMES patch directory.
    pub patch: I16,
    /// Unused animation field (always 1 in practice).
    pub stepdir: I16,
    /// Unused colormap field (always 0 in practice).
    pub colormap: I16,
}

const _: () = assert!(size_of::<mappatch_t>() == 10);

/// Header of an on-disk texture definition in the TEXTURE1 / TEXTURE2 lumps.
///
/// Corresponds to `maptexture_t` in `r_data.c`, whose last field is the
/// flexible array `mappatch_t patches[]`. Only the fixed header is defined
/// here: `R_InitTextures` reads the `patchcount` entries that follow it as a
/// `[mappatch_t]` slice from the bytes after the header.
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
pub struct maptexture_t {
    /// Texture name, up to 8 ASCII characters, NUL-padded.
    pub name: [u8; 8],
    /// Non-zero if the texture has transparent holes (unused by the renderer).
    pub masked: I32,
    /// Texture width in pixels.
    pub width: I16,
    /// Texture height in pixels.
    pub height: I16,
    /// Obsolete field present in the WAD format; ignored.
    pub obsolete: I32,
    /// Number of `mappatch_t` entries that follow this header.
    pub patchcount: I16,
}

const _: () = assert!(size_of::<maptexture_t>() == 22);

// ---------------------------------------------------------------------------
// Patch graphics (r_defs.h)
// ---------------------------------------------------------------------------

/// Header of a patch graphic lump (wall patches, sprites, menu and HUD
/// graphics).
///
/// Corresponds to `patch_t` in `r_defs.h`, whose last field is the flexible
/// array `int columnofs[width]`: the byte offset, from the start of the
/// lump, of each column's post list. Only the fixed header is defined here;
/// the renderers read a whole patch lump through the checked [`Patch`]
/// view, which holds the header, the `columnofs` table and the post lists.
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
pub struct patch_t {
    /// Width of the patch in pixels.
    pub width: I16,
    /// Height of the patch in pixels.
    pub height: I16,
    /// Horizontal draw offset from the patch origin, in pixels.
    pub leftoffset: I16,
    /// Vertical draw offset from the patch origin, in pixels.
    pub topoffset: I16,
}

const _: () = assert!(size_of::<patch_t>() == 8);

/// Header of a single run of opaque pixels within a column (a "post").
///
/// Corresponds to `post_t` in `r_defs.h` (`column_t` there is an alias of
/// it). A column is a sequence of posts terminated by a `topdelta` value of
/// [`POST_END`]. The pixels follow the two-byte header after one byte of
/// padding, and one more byte of padding follows them, so the next post
/// starts `length + 4` bytes after this one. [`Column`] walks the posts and
/// yields each as a [`Post`].
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
pub struct post_t {
    /// Row offset from the top of the texture column where this post begins.
    pub topdelta: u8,
    /// Number of pixels in this post.
    pub length: u8,
}

const _: () = assert!(size_of::<post_t>() == 2);

/// `topdelta` value that ends a column's post list.
pub const POST_END: u8 = 0xff;

/// Why a byte slice is not a patch lump; returned by [`Patch::from_bytes`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchError {
    /// The lump is shorter than the 8-byte [`patch_t`] header.
    TruncatedHeader { length: usize },
    /// The header gives a negative width.
    NegativeWidth { width: i16 },
    /// The lump is too short for the `columnofs` table of `width` entries.
    TruncatedColumnTable { width: usize, length: usize },
}

impl core::fmt::Display for PatchError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            PatchError::TruncatedHeader { length } => {
                write!(formatter, "{length} bytes is too short for a patch header")
            }
            PatchError::NegativeWidth { width } => write!(formatter, "negative width {width}"),
            PatchError::TruncatedColumnTable { width, length } => write!(
                formatter,
                "{length} bytes is too short for the offsets of {width} columns"
            ),
        }
    }
}

/// A checked view of a patch graphic lump: the [`patch_t`] header, its
/// `columnofs` table and the post lists the offsets point at.
///
/// [`Patch::from_bytes`] checks the header and the `columnofs` table against
/// the lump length once. Columns are then read with [`Patch::column`], whose
/// posts are bounds-checked against the lump as they are walked (see
/// [`Column`]), so no read leaves the lump bytes.
///
/// The view borrows the lump bytes; where they come from the zone cache, see
/// `wad::cache_patch_num` for how long they stay valid.
#[derive(Clone, Copy)]
pub struct Patch<'a> {
    header: &'a patch_t,
    column_offsets: &'a [I32],
    lump: &'a [u8],
}

impl<'a> Patch<'a> {
    /// View `lump` as a patch. Fails if it is too short for the header or
    /// for the `columnofs` table the header's width calls for, or if the
    /// width is negative.
    pub fn from_bytes(lump: &'a [u8]) -> Result<Self, PatchError> {
        let (header, after_header) = patch_t::ref_from_prefix(lump)
            .map_err(|_| PatchError::TruncatedHeader { length: lump.len() })?;
        let width = header.width.get();
        let column_count =
            usize::try_from(width).map_err(|_| PatchError::NegativeWidth { width })?;
        let (column_offsets, _) = <[I32]>::ref_from_prefix_with_elems(after_header, column_count)
            .map_err(|_| PatchError::TruncatedColumnTable {
            width: column_count,
            length: lump.len(),
        })?;
        Ok(Self {
            header,
            column_offsets,
            lump,
        })
    }

    /// Width of the patch in pixels (never negative).
    pub fn width(&self) -> i32 {
        self.header.width.get() as i32
    }

    /// Height of the patch in pixels.
    pub fn height(&self) -> i32 {
        self.header.height.get() as i32
    }

    /// Horizontal draw offset from the patch origin, in pixels.
    pub fn leftoffset(&self) -> i32 {
        self.header.leftoffset.get() as i32
    }

    /// Vertical draw offset from the patch origin, in pixels.
    pub fn topoffset(&self) -> i32 {
        self.header.topoffset.get() as i32
    }

    /// The whole lump the view was made from.
    pub fn bytes(&self) -> &'a [u8] {
        self.lump
    }

    /// Byte offset of column `x`'s post list from the start of the lump
    /// (`columnofs[x]` in C), as stored.
    ///
    /// Panics if `x` is not below [`Patch::width`], where C would read past
    /// the `columnofs` table.
    pub fn column_offset(&self, x: usize) -> i32 {
        match self.column_offsets.get(x) {
            Some(offset) => offset.get(),
            None => panic!("patch column {x} is outside width {}", self.width()),
        }
    }

    /// The posts of column `x`. Panics like [`Patch::column_offset`] if `x`
    /// is not below the width.
    pub fn column(&self, x: usize) -> Column<'a> {
        // A negative offset becomes one past any lump, so the column is empty.
        Column::at_offset(self.lump, self.column_offset(x) as usize)
    }
}

/// The post list of one patch column, iterated as [`Post`]s.
///
/// Each post is bounds-checked against the lump before it is yielded. A
/// post whose header or pixels would run past the end of the lump ends the
/// column, as does an offset outside the lump: the posts before it are
/// drawn and nothing is read out of bounds where C would. Vanilla lumps
/// always end each column with [`POST_END`] inside the lump, so they render
/// exactly as in C.
#[derive(Clone, Copy)]
pub struct Column<'a> {
    lump: &'a [u8],
    offset: usize,
}

impl<'a> Column<'a> {
    /// The column whose first post header is at byte `offset` of `lump`.
    pub fn at_offset(lump: &'a [u8], offset: usize) -> Self {
        Self { lump, offset }
    }
}

impl<'a> Iterator for Column<'a> {
    type Item = Post<'a>;

    fn next(&mut self) -> Option<Post<'a>> {
        let topdelta = *self.lump.get(self.offset)?;
        if topdelta == POST_END {
            return None;
        }
        let length = *self.lump.get(self.offset + 1)? as usize;
        // Header, then one byte of padding before the pixels.
        let pixels_start = self.offset + 3;
        let pixels = self.lump.get(pixels_start..pixels_start + length)?;
        // One byte of padding after the pixels.
        self.offset = pixels_start + length + 1;
        Some(Post { topdelta, pixels })
    }
}

/// One post of a column: `pixels.len()` opaque pixels starting `topdelta`
/// rows below the top of the patch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Post<'a> {
    /// Row offset from the top of the column where the post begins.
    pub topdelta: u8,
    /// The post's pixels, top to bottom; the length is the C `length`.
    pub pixels: &'a [u8],
}

// ---------------------------------------------------------------------------
// Screenshot output (v_video.c)
// ---------------------------------------------------------------------------

/// Header of a PCX image file, written by `WritePCXfile`.
///
/// Corresponds to `pcx_t` in `v_video.c`, whose last field `data` marks
/// where the RLE-packed pixels begin; the header here stops before it, and
/// the writer appends the pixels after `as_bytes()`.
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
pub struct pcx_t {
    pub manufacturer: u8,
    pub version: u8,
    pub encoding: u8,
    pub bits_per_pixel: u8,
    pub xmin: U16,
    pub ymin: U16,
    pub xmax: U16,
    pub ymax: U16,
    pub hres: U16,
    pub vres: U16,
    pub palette: [u8; 48],
    pub reserved: u8,
    pub color_planes: u8,
    pub bytes_per_line: U16,
    pub palette_type: U16,
    pub filler: [u8; 58],
}

const _: () = assert!(size_of::<pcx_t>() == 128);

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Two VERTEXES records and one LINEDEFS record decode from a byte
    /// buffer that starts at an odd address, with negative values intact.
    #[test]
    fn map_records_parse_from_unaligned_bytes() {
        let mut buffer = vec![0xAA];
        // VERTEXES: (-32, 64), (1000, -1)
        for value in [-32i16, 64, 1000, -1] {
            buffer.extend_from_slice(&value.to_le_bytes());
        }
        // LINEDEFS: v1 0, v2 1, flags 4, special 11, tag 7, sidenum [3, -1]
        for value in [0i16, 1, 4, 11, 7, 3, -1] {
            buffer.extend_from_slice(&value.to_le_bytes());
        }

        let vertex_bytes = &buffer[1..1 + 2 * size_of::<mapvertex_t>()];
        let vertexes = <[mapvertex_t]>::ref_from_bytes(vertex_bytes).unwrap();
        assert_eq!(vertexes.len(), 2);
        assert_eq!((vertexes[0].x.get(), vertexes[0].y.get()), (-32, 64));
        assert_eq!((vertexes[1].x.get(), vertexes[1].y.get()), (1000, -1));

        let linedef_bytes = &buffer[1 + 2 * size_of::<mapvertex_t>()..];
        let linedefs = <[maplinedef_t]>::ref_from_bytes(linedef_bytes).unwrap();
        assert_eq!(linedefs.len(), 1);
        let linedef = &linedefs[0];
        assert_eq!((linedef.v1.get(), linedef.v2.get()), (0, 1));
        assert_eq!(linedef.flags.get(), 4);
        assert_eq!(linedef.special.get(), 11);
        assert_eq!(linedef.tag.get(), 7);
        assert_eq!(linedef.sidenum.map(I16::get), [3, -1]);

        // A lump whose length is not a whole number of records is refused.
        assert!(<[mapvertex_t]>::ref_from_bytes(&buffer[1..4]).is_err());
    }

    /// Build a patch lump: the header, the `columnofs` table, then each
    /// column's posts as `(topdelta, pixels)` with C's padding bytes and a
    /// `POST_END` terminator.
    pub(crate) fn build_patch_lump(
        height: i16,
        leftoffset: i16,
        topoffset: i16,
        columns: &[&[(u8, &[u8])]],
    ) -> Vec<u8> {
        let mut lump = Vec::new();
        for value in [columns.len() as i16, height, leftoffset, topoffset] {
            lump.extend_from_slice(&value.to_le_bytes());
        }
        let table_start = lump.len();
        lump.resize(table_start + 4 * columns.len(), 0);
        for (index, posts) in columns.iter().enumerate() {
            let offset = lump.len() as i32;
            let entry = table_start + 4 * index;
            lump[entry..entry + 4].copy_from_slice(&offset.to_le_bytes());
            for &(topdelta, pixels) in posts.iter() {
                lump.extend_from_slice(&[topdelta, pixels.len() as u8, 0xAA]);
                lump.extend_from_slice(pixels);
                lump.push(0xBB);
            }
            lump.push(POST_END);
        }
        lump
    }

    /// The header fields come back signed and widened, and the bytes are
    /// the whole lump.
    #[test]
    fn patch_header_parses() {
        let lump = build_patch_lump(-3, -7, 120, &[&[], &[]]);
        let patch = Patch::from_bytes(&lump).unwrap();
        assert_eq!(patch.width(), 2);
        assert_eq!(patch.height(), -3);
        assert_eq!(patch.leftoffset(), -7);
        assert_eq!(patch.topoffset(), 120);
        assert_eq!(patch.bytes().len(), lump.len());
        assert_eq!(patch.column_offset(0), 16);
    }

    /// Each column yields its posts in order with their topdelta and
    /// pixels, skipping the padding bytes, and stops at `POST_END`.
    #[test]
    fn patch_columns_yield_posts() {
        let lump = build_patch_lump(
            16,
            0,
            0,
            &[&[(0, &[1, 2, 3]), (5, &[4])], &[], &[(254, &[9, 8])]],
        );
        let patch = Patch::from_bytes(&lump).unwrap();

        let first: Vec<_> = patch.column(0).collect();
        assert_eq!(
            first,
            [
                Post {
                    topdelta: 0,
                    pixels: &[1, 2, 3]
                },
                Post {
                    topdelta: 5,
                    pixels: &[4]
                },
            ]
        );
        assert_eq!(patch.column(1).count(), 0);
        let last: Vec<_> = patch.column(2).collect();
        assert_eq!(
            last,
            [Post {
                topdelta: 254,
                pixels: &[9, 8]
            }]
        );

        // The iterator stays finished after the terminator.
        let mut column = patch.column(0);
        assert_eq!(column.by_ref().count(), 2);
        assert_eq!(column.next(), None);
    }

    /// A lump too short for the header or for the `columnofs` table, or
    /// with a negative width, is not a patch.
    #[test]
    fn patch_rejects_truncated_lumps() {
        let lump = build_patch_lump(8, 0, 0, &[&[(0, &[1])], &[(0, &[2])]]);

        assert_eq!(
            Patch::from_bytes(&lump[..7]).err(),
            Some(PatchError::TruncatedHeader { length: 7 })
        );
        // Header plus one of the two column offsets.
        assert_eq!(
            Patch::from_bytes(&lump[..12]).err(),
            Some(PatchError::TruncatedColumnTable {
                width: 2,
                length: 12
            })
        );
        assert!(Patch::from_bytes(&lump[..16]).is_ok());

        let mut negative = lump.clone();
        negative[..2].copy_from_slice(&(-1i16).to_le_bytes());
        assert_eq!(
            Patch::from_bytes(&negative).err(),
            Some(PatchError::NegativeWidth { width: -1 })
        );
    }

    /// Posts are checked against the lump as they are walked: a post or
    /// offset past the end ends the column instead of reading out of
    /// bounds, and a column index past the width panics.
    #[test]
    fn patch_columns_stop_at_lump_end() {
        let lump = build_patch_lump(8, 0, 0, &[&[(0, &[1, 2]), (4, &[3, 4, 5])]]);

        // Cut inside the second post's pixels: only the first post remains.
        // Header and one column offset (12 bytes), the first post (6), then
        // the second post's header and padding (3).
        let second_post_pixels = 12 + 6 + 3;
        let truncated = Patch::from_bytes(&lump[..second_post_pixels + 2]).unwrap();
        let posts: Vec<_> = truncated.column(0).collect();
        assert_eq!(
            posts,
            [Post {
                topdelta: 0,
                pixels: &[1, 2]
            }]
        );

        // Cut before the terminator: both posts, then the end of the lump.
        let unterminated = Patch::from_bytes(&lump[..lump.len() - 1]).unwrap();
        assert_eq!(unterminated.column(0).count(), 2);

        // A column offset outside the lump (or negative) is an empty column.
        for offset in [lump.len() as i32, -4] {
            let mut moved = lump.clone();
            moved[8..12].copy_from_slice(&offset.to_le_bytes());
            assert_eq!(Patch::from_bytes(&moved).unwrap().column(0).count(), 0);
        }

        let patch = Patch::from_bytes(&lump).unwrap();
        let outside_width = std::panic::catch_unwind(|| patch.column(1));
        assert!(outside_width.is_err());
    }
}
