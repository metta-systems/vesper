//! On-disk record formats of WAD lumps, plus the PCX screenshot header.
//!
//! Each type here is the single definition of a C struct from
//! `doomdata.h`, `w_wad.c`, `r_data.c`, `r_defs.h` or `v_video.c`. Fields
//! wider than a byte are `zerocopy` little-endian wrappers, which have
//! alignment 1: the structs match the byte layout on disk without
//! `packed`, can be viewed in place over unaligned lump bytes, and make
//! the WAD's little-endian byte order explicit, so readers call `.get()`
//! instead of a `SHORT`/`LONG` byte swap. Every type carries a
//! compile-time assertion of its on-disk size.
//!
//! Records followed by a variable-length array in the WAD (`patch_t`,
//! `maptexture_t`) are defined as their fixed-size header only; the
//! trailing array is read separately (see each type).

#![allow(non_camel_case_types)]

use {
    std::mem::size_of,
    zerocopy::{
        little_endian::{I16, I32, U16},
        FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned,
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
/// lump, of each column's post list. Only the fixed header is defined here.
/// The renderers hold patches as raw pointers into the lump cache and walk
/// the columns by pointer, so the offsets are read with
/// [`patch_t::column_offset`].
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

impl patch_t {
    /// Byte offset of column `column`'s post list from the start of the
    /// patch lump (`columnofs[column]` in C).
    ///
    /// # Safety
    ///
    /// `patch` must point at a whole patch lump whose `columnofs` array has
    /// more than `column` entries.
    pub unsafe fn column_offset(patch: *const patch_t, column: usize) -> usize {
        let columnofs = patch.add(1) as *const I32;
        (*columnofs.add(column)).get() as usize
    }
}

/// Single run of opaque pixels within a column (a "post").
///
/// Corresponds to `post_t` in `r_defs.h`. A column is a sequence of posts
/// terminated by a `topdelta` value of `0xff`. The actual pixel data follows
/// immediately after the two-byte header (one byte of padding before the
/// pixels and one byte of padding after), so the next post starts
/// `length + 4` bytes after this one.
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned)]
pub struct post_t {
    /// Row offset from the top of the texture column where this post begins.
    pub topdelta: u8,
    /// Number of pixels in this post.
    pub length: u8,
}

const _: () = assert!(size_of::<post_t>() == 2);

/// Alias for `post_t`; used interchangeably in the C source as `column_t`.
pub type column_t = post_t;

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
mod tests {
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
}
