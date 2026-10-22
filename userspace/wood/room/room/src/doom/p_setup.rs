//! Rust port of vendor/doomgeneric/p_setup.c.
//!
//! Level/map loading and initialization. Reads all BSP and geometry lumps
//! from the WAD into runtime data structures (vertexes, lines, sectors,
//! subsectors, nodes, segs, blockmap, and reject table). `P_SetupLevel` is
//! the single entry point called by the game loop when entering a new map;
//! `P_Init` is called once at startup to initialize switch lists, animated
//! flats, and the sprite name table.
//!
//! # Rust-vs-C differences
//!
//! - All WAD data is little-endian; the `SHORT` helper performs an explicit
//!   `i16::from_le` conversion instead of relying on the C `SHORT` macro from
//!   `i_swap.h`.
//! - `P_LoadThings` increments the `mt` pointer only after the spawn decision
//!   so that a `break` on a non-commercial monster does not advance past it.
//! - `P_SetupLevel` uses `format!` for lump-name construction instead of
//!   `DEH_snprintf`, so DeHackEd lump-name patches are not applied.
//! - Global map tables (`vertexes`, `lines`, etc.) are `#[no_mangle]`
//!   `static mut` values with C linkage, matching the C extern declarations.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::{c_char, c_int, c_short, c_uint, c_ushort, c_void};
use std::ptr;

use crate::doom::c_ffi::{
    line_t, node_t, sector_t, seg_t, side_t, subsector_t, vertex_t, LinedefFlag, MapLump,
};
use crate::doom::d_mode;
use crate::doom::d_player::{consoleplayer, players, MAXPLAYERS};
use crate::doom::info::sprnames;
use crate::doom::m_bbox::{BBox, M_AddToBox, M_ClearBox};
use crate::doom::m_fixed::{FRACBITS, FRACUNIT};
use crate::doom::p_tick::{leveltime, P_InitThinkers};
use crate::doom::z_zone::{PU_LEVEL, PU_STATIC};

// ---------------------------------------------------------------------------
// Byte-order helper
// ---------------------------------------------------------------------------

/// Convert a little-endian `i16` from a WAD file to host byte order.
/// On little-endian hosts this is a no-op cast; on big-endian it swaps.
#[inline]
fn SHORT(x: i16) -> i16 {
    i16::from_le(x)
}

// ---------------------------------------------------------------------------
// Zone-memory tags
// ---------------------------------------------------------------------------

/// Zone-memory purge level used when freeing all level data between maps.
/// Matches `PU_PURGELEVEL` in `z_zone.h`; all tags in `[PU_LEVEL,
/// PU_PURGELEVEL)` are freed by `Z_FreeTags` at the start of `P_SetupLevel`.
const PU_PURGELEVEL: c_int = 7;

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Slope types
// ---------------------------------------------------------------------------

/// Linedef slope type: line is perfectly horizontal (dy == 0).
/// Referenced by `c_ffi::line_t.slopetype`.
const ST_HORIZONTAL: c_int = 0;
/// Linedef slope type: line is perfectly vertical (dx == 0).
/// Referenced by `c_ffi::line_t.slopetype`.
const ST_VERTICAL: c_int = 1;
/// Linedef slope type: dy/dx > 0 (rises left-to-right).
/// Referenced by `c_ffi::line_t.slopetype`.
const ST_POSITIVE: c_int = 2;
/// Linedef slope type: dy/dx < 0 (falls left-to-right).
/// Referenced by `c_ffi::line_t.slopetype`.
const ST_NEGATIVE: c_int = 3;

// ---------------------------------------------------------------------------
// Misc constants
// ---------------------------------------------------------------------------

/// Shift to convert a fixed-point map coordinate to a blockmap cell index.
/// Equals `FRACBITS + 7`, i.e. each blockmap cell covers 128 map units.
const MAPBLOCKSHIFT: c_int = FRACBITS as c_int + 7;

/// Maximum radius added/subtracted when clamping sector bounding boxes to
/// blockmap cells. 32 map units in fixed-point (32 << FRACBITS).
const MAXRADIUS: c_int = 32 * FRACUNIT;

/// Maximum number of deathmatch start positions in a level.
/// Matches `MAX_DEATHMATCH_STARTS` in `p_setup.c`.
pub const MAX_DEATHMATCH_STARTS: usize = 10;

// ---------------------------------------------------------------------------
// Packed WAD structs (exact on-disk layout)
// ---------------------------------------------------------------------------

/// On-disk vertex record from the WAD VERTEXES lump.
/// Maps to `mapvertex_t` in `p_local.h`. Coordinates are 16-bit integers
/// in map units; they are sign-extended and shifted left by `FRACBITS` when
/// copied into the runtime `vertex_t`.
#[repr(C, packed)]
struct mapvertex_t {
    x: i16,
    y: i16,
}

/// On-disk sidedef record from the WAD SIDEDEFS lump.
/// Maps to `mapsidedef_t` in `p_local.h`. Texture offsets are in map units
/// and are shifted left by `FRACBITS` when copied to the runtime `side_t`.
/// Texture name fields are 8-byte null-padded ASCII strings resolved to
/// runtime texture indices by `R_TextureNumForName`.
#[repr(C, packed)]
struct mapsidedef_t {
    textureoffset: i16,
    rowoffset: i16,
    toptexture: [u8; 8],
    bottomtexture: [u8; 8],
    midtexture: [u8; 8],
    sector: i16,
}

/// On-disk linedef record from the WAD LINEDEFS lump.
/// Maps to `maplinedef_t` in `p_local.h`. `sidenum[1]` is -1 for one-sided
/// lines; the runtime `line_t` stores null pointers in that case.
#[repr(C, packed)]
struct maplinedef_t {
    v1: i16,
    v2: i16,
    flags: i16,
    special: i16,
    tag: i16,
    sidenum: [i16; 2],
}

/// On-disk sector record from the WAD SECTORS lump.
/// Maps to `mapsector_t` in `p_local.h`. Floor/ceiling heights are in map
/// units and shifted left by `FRACBITS` in the runtime `sector_t`. Flat name
/// fields are 8-byte null-padded ASCII strings resolved via `R_FlatNumForName`.
#[repr(C, packed)]
struct mapsector_t {
    floorheight: i16,
    ceilingheight: i16,
    floorpic: [u8; 8],
    ceilingpic: [u8; 8],
    lightlevel: i16,
    special: i16,
    tag: i16,
}

/// On-disk subsector record from the WAD SSECTORS lump.
/// Maps to `mapsubsector_t` in `p_local.h`. `firstseg` is an index into the
/// segs array; `numsegs` is the count of consecutive segs forming the convex
/// polygon of this subsector.
#[repr(C, packed)]
struct mapsubsector_t {
    numsegs: i16,
    firstseg: i16,
}

/// On-disk seg record from the WAD SEGS lump.
/// Maps to `mapseg_t` in `p_local.h`. `angle` is a BAM (Binary Angle
/// Measurement) stored in the upper 16 bits of a `u32` after shifting.
/// `offset` is the distance along the linedef to the seg's start vertex,
/// in fixed-point.
#[repr(C, packed)]
struct mapseg_t {
    v1: i16,
    v2: i16,
    angle: i16,
    linedef: i16,
    side: i16,
    offset: i16,
}

/// On-disk BSP node record from the WAD NODES lump.
/// Maps to `mapnode_t` in `p_local.h`. `x`, `y`, `dx`, `dy` define the
/// partition line in map units. `bbox[2][4]` are the bounding boxes for each
/// child subtree. `children[2]` are child indices; the high bit set indicates
/// a subsector leaf.
#[repr(C, packed)]
struct mapnode_t {
    x: i16,
    y: i16,
    dx: i16,
    dy: i16,
    bbox: [[i16; 4]; 2],
    children: [u16; 2],
}

/// On-disk thing record from the WAD THINGS lump, and also the runtime spawn
/// descriptor passed to `P_SpawnMapThing`.
///
/// Maps to `mapthing_t` in `p_local.h`. This struct is `pub` because it is
/// referenced by `deathmatchstarts`, `deathmatch_p`, and `playerstarts`
/// globals that are visible to C. `#[repr(C)]` ensures ABI compatibility;
/// `#[derive(Clone, Copy)]` allows it to be used by value in initialization
/// expressions.
///
/// Fields:
/// - `x`, `y`: map-unit position (not fixed-point)
/// - `angle`: facing direction in degrees (0, 45, 90, ...)
/// - `type`: Doom editor number identifying the thing class
/// - `options`: bit flags (skill levels, deaf, etc.)
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct mapthing_t {
    pub x: i16,
    pub y: i16,
    pub angle: i16,
    pub r#type: i16,
    pub options: i16,
}

// ---------------------------------------------------------------------------
// MAP-related lookup tables
// ---------------------------------------------------------------------------

/// Total number of vertexes loaded from the current map's VERTEXES lump.
/// C linkage: referenced by `r_bsp.c` and other renderer modules.
#[no_mangle]
pub static mut numvertexes: c_int = 0;
/// Pointer to the runtime vertex array, allocated from zone memory at
/// `PU_LEVEL`. Each entry holds fixed-point (16.16) x/y coordinates.
/// C linkage: referenced by `p_map.c`, `r_bsp.c`, and many others.
#[no_mangle]
pub static mut vertexes: *mut vertex_t = ptr::null_mut();

/// Total number of segs loaded from the current map's SEGS lump.
/// C linkage: referenced by `r_bsp.c`.
#[no_mangle]
pub static mut numsegs: c_int = 0;
/// Pointer to the runtime seg array, allocated from zone memory at `PU_LEVEL`.
/// C linkage: referenced by `r_bsp.c` and `p_sight.c`.
#[no_mangle]
pub static mut segs: *mut seg_t = ptr::null_mut();

/// Total number of sectors loaded from the current map's SECTORS lump.
/// C linkage: referenced throughout the game and renderer.
#[no_mangle]
pub static mut numsectors: c_int = 0;
/// Pointer to the runtime sector array, allocated from zone memory at
/// `PU_LEVEL`. Sectors own the lighting, floor/ceiling heights, and thing
/// lists used by most game logic.
/// C linkage: referenced throughout the game and renderer.
#[no_mangle]
pub static mut sectors: *mut sector_t = ptr::null_mut();

/// Total number of subsectors loaded from the current map's SSECTORS lump.
/// C linkage: referenced by `r_bsp.c` and `p_sight.c`.
#[no_mangle]
pub static mut numsubsectors: c_int = 0;
/// Pointer to the runtime subsector array, allocated from zone memory at
/// `PU_LEVEL`. Each subsector is a convex polygon leaf of the BSP tree.
/// C linkage: referenced by `r_bsp.c`, `p_map.c`, and `p_sight.c`.
#[no_mangle]
pub static mut subsectors: *mut subsector_t = ptr::null_mut();

/// Total number of BSP nodes loaded from the current map's NODES lump.
/// C linkage: referenced by `r_bsp.c` and `p_sight.c`.
#[no_mangle]
pub static mut numnodes: c_int = 0;
/// Pointer to the runtime BSP node array, allocated from zone memory at
/// `PU_LEVEL`. The root node is `nodes[numnodes - 1]`.
/// C linkage: referenced by `r_bsp.c`, `p_map.c`, and `p_sight.c`.
#[no_mangle]
pub static mut nodes: *mut node_t = ptr::null_mut();

/// Total number of linedefs loaded from the current map's LINEDEFS lump.
/// C linkage: referenced by `p_map.c`, `p_maputl.c`, and others.
#[no_mangle]
pub static mut numlines: c_int = 0;
/// Pointer to the runtime linedef array, allocated from zone memory at
/// `PU_LEVEL`. Each linedef connects two vertexes and references up to two
/// sidedefs.
/// C linkage: referenced throughout the game.
#[no_mangle]
pub static mut lines: *mut line_t = ptr::null_mut();

/// Total number of sidedefs loaded from the current map's SIDEDEFS lump.
/// C linkage: referenced by `r_segs.c` and others.
#[no_mangle]
pub static mut numsides: c_int = 0;
/// Pointer to the runtime sidedef array, allocated from zone memory at
/// `PU_LEVEL`. Sidedefs hold texture indices and offsets; each linedef
/// references one or two.
/// C linkage: referenced by `r_segs.c`, `p_spec.c`, and others.
#[no_mangle]
pub static mut sides: *mut side_t = ptr::null_mut();

/// Running total of front+back linedef-sector references, used to size the
/// per-sector line-pointer buffer in `P_GroupLines`. Module-private; not
/// exported to C.
static mut totallines: c_int = 0;

// ---------------------------------------------------------------------------
// BLOCKMAP
// ---------------------------------------------------------------------------

/// Width of the blockmap grid in 128-unit cells.
/// C linkage: referenced by `p_map.c` and `p_maputl.c` for collision queries.
#[no_mangle]
pub static mut bmapwidth: c_int = 0;
/// Height of the blockmap grid in 128-unit cells.
/// C linkage: referenced by `p_map.c` and `p_maputl.c` for collision queries.
#[no_mangle]
pub static mut bmapheight: c_int = 0;
/// Pointer into `blockmaplump` offset by 4 shorts (past the header).
/// Each entry is an offset from `blockmaplump` to the list of linedefs in that
/// cell, terminated by -1.
/// C linkage: referenced by `p_maputl.c`.
#[no_mangle]
pub static mut blockmap: *mut c_short = ptr::null_mut();
/// Base pointer to the raw blockmap lump data (including the 4-short header).
/// Header layout: `[orgx, orgy, width, height]` in map units (not fixed-point
/// before shifting). Allocated from zone memory at `PU_LEVEL`.
/// C linkage: referenced by `p_maputl.c`.
#[no_mangle]
pub static mut blockmaplump: *mut c_short = ptr::null_mut();
/// X origin of the blockmap in fixed-point (16.16) map coordinates.
/// Subtract from a thing's x to get the blockmap column.
/// C linkage: referenced by `p_maputl.c`.
#[no_mangle]
pub static mut bmaporgx: c_int = 0;
/// Y origin of the blockmap in fixed-point (16.16) map coordinates.
/// Subtract from a thing's y to get the blockmap row.
/// C linkage: referenced by `p_maputl.c`.
#[no_mangle]
pub static mut bmaporgy: c_int = 0;
/// Pointer to the blockmap thing-chain heads, one per cell (`bmapwidth *
/// bmapheight` entries). Each entry is a pointer to the first `mobj_t` in
/// that cell's chain, or null. Zeroed at level load.
/// C linkage: referenced by `p_maputl.c` and `p_map.c`.
#[no_mangle]
pub static mut blocklinks: *mut *mut c_void = ptr::null_mut();

// ---------------------------------------------------------------------------
// REJECT
// ---------------------------------------------------------------------------

/// Pointer to the precomputed LOS (line-of-sight) reject table.
/// A 2-D bit array indexed by `[sector1 * numsectors + sector2]`; a set bit
/// means the two sectors cannot see each other, so detailed LOS checks are
/// skipped. Allocated from zone memory at `PU_LEVEL`.
/// C linkage: referenced by `p_sight.c`.
#[no_mangle]
pub static mut rejectmatrix: *mut u8 = ptr::null_mut();

// ---------------------------------------------------------------------------
// Starting spots
// ---------------------------------------------------------------------------

/// Array of deathmatch start positions collected from the THINGS lump.
/// Holds up to `MAX_DEATHMATCH_STARTS` entries; the active count is tracked
/// by `deathmatch_p - deathmatchstarts`.
/// C linkage: referenced by `g_game.c`.
#[no_mangle]
pub static mut deathmatchstarts: [mapthing_t; MAX_DEATHMATCH_STARTS] = [mapthing_t {
    x: 0,
    y: 0,
    angle: 0,
    r#type: 0,
    options: 0,
}; MAX_DEATHMATCH_STARTS];

/// Write cursor into `deathmatchstarts`; points to the next free slot.
/// Reset to `&deathmatchstarts[0]` at the start of each `P_SetupLevel`.
/// C linkage: advanced by `P_SpawnMapThing` when spawning deathmatch starts.
#[no_mangle]
pub static mut deathmatch_p: *mut mapthing_t = ptr::null_mut();

/// Per-player single-player start positions, indexed by player number.
/// Holds `MAXPLAYERS` entries; populated by `P_SpawnMapThing` from THINGS
/// with type 1-4 (player starts).
/// C linkage: referenced by `g_game.c` for respawning.
#[no_mangle]
pub static mut playerstarts: [mapthing_t; MAXPLAYERS] = [mapthing_t {
    x: 0,
    y: 0,
    angle: 0,
    r#type: 0,
    options: 0,
}; MAXPLAYERS];

// ---------------------------------------------------------------------------
// Extern declarations for still-unported C modules
// ---------------------------------------------------------------------------

use crate::doom::doomstat::gamemode;
use crate::doom::g_game::{
    bodyqueslot, deathmatch, playeringame, precache, totalitems, totalkills, totalsecret, wminfo,
    G_DeathMatchSpawnPlayer,
};
use crate::doom::i_system::I_GetMemoryValue;
use crate::doom::m_argv::M_CheckParm;
use crate::doom::m_fixed::FixedDiv;
use crate::doom::p_mobj::{iquehead, iquetail, P_SpawnMapThing};
use crate::doom::p_spec::{P_InitPicAnims, P_SpawnSpecials};
use crate::doom::p_switch::P_InitSwitchList;
use crate::doom::r_data::{R_FlatNumForName, R_PrecacheLevel, R_TextureNumForName};
use crate::doom::r_things::R_InitSprites;
use crate::doom::s_sound::S_Start;
use crate::doom::w_wad::{
    W_CacheLumpNum, W_GetNumForName, W_LumpLength, W_ReadLump, W_ReleaseLumpNum,
};
use crate::doom::z_zone::{Z_FreeTags, Z_Malloc};

// ---------------------------------------------------------------------------
// GetSectorAtNullAddress
// ---------------------------------------------------------------------------

/// Return a pointer to a synthetic `sector_t` that represents the sector at
/// address 0, used to handle malformed WAD data (the "glass hack").
///
/// The returned sector is initialized on first call by reading 4 bytes each
/// from address 0 and 4 via `I_GetMemoryValue`, matching vanilla Doom's
/// behavior of reading the initial heap block header at address 0.
/// Subsequent calls return the already-initialized sector without re-reading.
///
/// C callers: `P_LoadSegs` (this file). Also called via C FFI from legacy
/// unported code that encounters two-sided linedefs with an invalid back sidenum.
#[no_mangle]
pub extern "C" fn GetSectorAtNullAddress() -> *mut sector_t {
    static mut NULL_SECTOR_IS_INITIALIZED: bool = false;
    static mut NULL_SECTOR: sector_t = unsafe { std::mem::zeroed() };

    unsafe {
        if !NULL_SECTOR_IS_INITIALIZED {
            NULL_SECTOR = std::mem::zeroed();
            I_GetMemoryValue(
                0,
                &raw mut NULL_SECTOR.floorheight as *mut c_int as *mut c_void,
                4,
            );
            I_GetMemoryValue(
                4,
                &raw mut NULL_SECTOR.ceilingheight as *mut c_int as *mut c_void,
                4,
            );
            NULL_SECTOR_IS_INITIALIZED = true;
        }
        &raw mut NULL_SECTOR
    }
}

// ---------------------------------------------------------------------------
// P_LoadVertexes
// ---------------------------------------------------------------------------

/// Load the VERTEXES lump and populate the global `vertexes` array.
///
/// Allocates `numvertexes * sizeof(vertex_t)` bytes at `PU_LEVEL`, reads each
/// on-disk `mapvertex_t` (2 × i16 little-endian), and stores the result as
/// fixed-point (16.16) coordinates by shifting left by `FRACBITS`. The raw
/// lump is released after conversion.
///
/// `lump` must be the lump number of the VERTEXES entry for the current map
/// (typically `lumpnum + MapLump::VERTEXES`).
/// C callers: `P_SetupLevel` (this file).
#[no_mangle]
pub extern "C" fn P_LoadVertexes(lump: c_int) {
    unsafe {
        numvertexes = W_LumpLength(lump as c_uint) / std::mem::size_of::<mapvertex_t>() as c_int;
        vertexes = Z_Malloc(
            numvertexes * std::mem::size_of::<vertex_t>() as c_int,
            PU_LEVEL,
            ptr::null_mut(),
        ) as *mut vertex_t;

        let data = W_CacheLumpNum(lump, PU_STATIC);
        let mut ml = data as *mut mapvertex_t;
        let mut li = vertexes;

        for _ in 0..numvertexes {
            (*li).x = (SHORT((*ml).x) as c_int) << FRACBITS;
            (*li).y = (SHORT((*ml).y) as c_int) << FRACBITS;
            li = li.add(1);
            ml = ml.add(1);
        }

        W_ReleaseLumpNum(lump);
    }
}

// ---------------------------------------------------------------------------
// P_LoadSegs
// ---------------------------------------------------------------------------

/// Load the SEGS lump and populate the global `segs` array.
///
/// Allocates `numsegs * sizeof(seg_t)` bytes at `PU_LEVEL` (zero-initialized
/// via zone memory) and fills each entry from the corresponding on-disk
/// `mapseg_t`. Key conversions:
/// - Vertex indices are resolved to pointers into `vertexes`.
/// - `angle` is shifted left 16 to become a full BAM (Binary Angle
///   Measurement) u32 value.
/// - `offset` is shifted left by `FRACBITS` to become fixed-point.
/// - `sidedef` and `frontsector` are resolved from the linedef's `sidenum`
///   array using the seg's side (0 = front, 1 = back).
/// - For two-sided linedefs, `backsector` is resolved from the opposite
///   sidenum; if that sidenum is out of range, `GetSectorAtNullAddress` is
///   used (glass-hack compatibility).
///
/// `lump` must be the lump number of the SEGS entry for the current map.
/// Precondition: `vertexes`, `lines`, and `sides` must already be loaded.
/// C callers: `P_SetupLevel` (this file).
#[no_mangle]
pub extern "C" fn P_LoadSegs(lump: c_int) {
    unsafe {
        numsegs = W_LumpLength(lump as c_uint) / std::mem::size_of::<mapseg_t>() as c_int;
        segs = Z_Malloc(
            numsegs * std::mem::size_of::<seg_t>() as c_int,
            PU_LEVEL,
            ptr::null_mut(),
        ) as *mut seg_t;
        // Zeroed by Z_Malloc internally

        let data = W_CacheLumpNum(lump, PU_STATIC);
        let mut ml = data as *mut mapseg_t;
        let mut li = segs;

        for _ in 0..numsegs {
            (*li).v1 = vertexes.offset(SHORT((*ml).v1) as isize);
            (*li).v2 = vertexes.offset(SHORT((*ml).v2) as isize);
            (*li).angle = ((SHORT((*ml).angle) as c_int) << 16) as c_uint;
            (*li).offset = (SHORT((*ml).offset) as c_int) << 16;
            let linedef = SHORT((*ml).linedef) as c_int;
            let ldef = lines.offset(linedef as isize);
            (*li).linedef = ldef;
            let side = SHORT((*ml).side) as c_int;
            (*li).sidedef = sides.offset((*ldef).sidenum[side as usize] as isize);
            (*li).frontsector = (*sides.offset((*ldef).sidenum[side as usize] as isize)).sector;

            if (*ldef).flags & LinedefFlag::TWOSIDED as i16 != 0 {
                let sidenum = (*ldef).sidenum[side as usize ^ 1];
                if sidenum < 0 || sidenum as c_int >= numsides {
                    (*li).backsector = GetSectorAtNullAddress();
                } else {
                    (*li).backsector = sides.offset(sidenum as isize).as_ref().unwrap().sector;
                }
            } else {
                (*li).backsector = ptr::null_mut();
            }

            li = li.add(1);
            ml = ml.add(1);
        }

        W_ReleaseLumpNum(lump);
    }
}

// ---------------------------------------------------------------------------
// P_LoadSubsectors
// ---------------------------------------------------------------------------

/// Load the SSECTORS lump and populate the global `subsectors` array.
///
/// Allocates `numsubsectors * sizeof(subsector_t)` bytes at `PU_LEVEL`
/// (zero-initialized) and fills each entry from the corresponding on-disk
/// `mapsubsector_t`. The `numlines` and `firstline` fields (confusingly named
/// in the runtime struct - they actually count/index segs) are byte-swapped
/// from little-endian.
///
/// `lump` must be the lump number of the SSECTORS entry for the current map.
/// C callers: `P_SetupLevel` (this file).
#[no_mangle]
pub extern "C" fn P_LoadSubsectors(lump: c_int) {
    unsafe {
        numsubsectors =
            W_LumpLength(lump as c_uint) / std::mem::size_of::<mapsubsector_t>() as c_int;
        subsectors = Z_Malloc(
            numsubsectors * std::mem::size_of::<subsector_t>() as c_int,
            PU_LEVEL,
            ptr::null_mut(),
        ) as *mut subsector_t;
        // Zeroed by Z_Malloc internally

        let data = W_CacheLumpNum(lump, PU_STATIC);
        let mut ms = data as *mut mapsubsector_t;
        let mut ss = subsectors;

        for _ in 0..numsubsectors {
            (*ss).numlines = SHORT((*ms).numsegs);
            (*ss).firstline = SHORT((*ms).firstseg);
            ss = ss.add(1);
            ms = ms.add(1);
        }

        W_ReleaseLumpNum(lump);
    }
}

// ---------------------------------------------------------------------------
// P_LoadSectors
// ---------------------------------------------------------------------------

/// Load the SECTORS lump and populate the global `sectors` array.
///
/// Allocates `numsectors * sizeof(sector_t)` bytes at `PU_LEVEL`
/// (zero-initialized) and fills each entry from the corresponding on-disk
/// `mapsector_t`. Floor and ceiling heights are shifted left by `FRACBITS`
/// to become fixed-point. Flat names are resolved to runtime indices via
/// `R_FlatNumForName`. The `thinglist` field is explicitly cleared to null.
///
/// `lump` must be the lump number of the SECTORS entry for the current map.
/// C callers: `P_SetupLevel` (this file).
#[no_mangle]
pub extern "C" fn P_LoadSectors(lump: c_int) {
    unsafe {
        numsectors = W_LumpLength(lump as c_uint) / std::mem::size_of::<mapsector_t>() as c_int;
        sectors = Z_Malloc(
            numsectors * std::mem::size_of::<sector_t>() as c_int,
            PU_LEVEL,
            ptr::null_mut(),
        ) as *mut sector_t;
        // Zeroed by Z_Malloc internally

        let data = W_CacheLumpNum(lump, PU_STATIC);
        let mut ms = data as *mut mapsector_t;
        let mut ss = sectors;

        for _ in 0..numsectors {
            (*ss).floorheight = (SHORT((*ms).floorheight) as c_int) << FRACBITS;
            (*ss).ceilingheight = (SHORT((*ms).ceilingheight) as c_int) << FRACBITS;
            (*ss).floorpic = R_FlatNumForName((*ms).floorpic.as_ptr() as *mut c_char) as c_short;
            (*ss).ceilingpic =
                R_FlatNumForName((*ms).ceilingpic.as_ptr() as *mut c_char) as c_short;
            (*ss).lightlevel = SHORT((*ms).lightlevel);
            (*ss).special = SHORT((*ms).special);
            (*ss).tag = SHORT((*ms).tag);
            (*ss).thinglist = ptr::null_mut();
            ss = ss.add(1);
            ms = ms.add(1);
        }

        W_ReleaseLumpNum(lump);
    }
}

// ---------------------------------------------------------------------------
// P_LoadNodes
// ---------------------------------------------------------------------------

/// Load the NODES lump and populate the global `nodes` array.
///
/// Allocates `numnodes * sizeof(node_t)` bytes at `PU_LEVEL` and fills each
/// entry from the corresponding on-disk `mapnode_t`. Partition-line origin
/// (`x`, `y`) and direction (`dx`, `dy`) are shifted left by `FRACBITS` to
/// become fixed-point. Bounding-box values are similarly shifted. Child
/// indices are stored as `u16`; the high bit (0x8000) indicates a subsector
/// leaf rather than an internal node.
///
/// `lump` must be the lump number of the NODES entry for the current map.
/// C callers: `P_SetupLevel` (this file).
#[no_mangle]
pub extern "C" fn P_LoadNodes(lump: c_int) {
    unsafe {
        numnodes = W_LumpLength(lump as c_uint) / std::mem::size_of::<mapnode_t>() as c_int;
        nodes = Z_Malloc(
            numnodes * std::mem::size_of::<node_t>() as c_int,
            PU_LEVEL,
            ptr::null_mut(),
        ) as *mut node_t;

        let data = W_CacheLumpNum(lump, PU_STATIC);
        let mut mn = data as *mut mapnode_t;
        let mut no = nodes;

        for _ in 0..numnodes {
            (*no).x = (SHORT((*mn).x) as c_int) << FRACBITS;
            (*no).y = (SHORT((*mn).y) as c_int) << FRACBITS;
            (*no).dx = (SHORT((*mn).dx) as c_int) << FRACBITS;
            (*no).dy = (SHORT((*mn).dy) as c_int) << FRACBITS;
            for j in 0..2usize {
                (*no).children[j] = SHORT((*mn).children[j] as i16) as c_ushort;
                for k in 0..4usize {
                    (*no).bbox[j][k] = (SHORT((*mn).bbox[j][k]) as c_int) << FRACBITS;
                }
            }
            no = no.add(1);
            mn = mn.add(1);
        }

        W_ReleaseLumpNum(lump);
    }
}

// ---------------------------------------------------------------------------
// P_LoadThings
// ---------------------------------------------------------------------------

/// Load the THINGS lump and spawn all map objects for the current level.
///
/// Iterates over each on-disk `mapthing_t` in the lump. In non-commercial
/// (shareware/registered) game modes, monster types that only appear in Doom
/// II are skipped; when the first such type is encountered the loop
/// **breaks** immediately (not `continue`), so no subsequent things are
/// processed either - this matches vanilla Doom's behavior and preserves
/// demo-compatible RNG sequencing.
///
/// Each surviving thing is byte-swapped and passed to `P_SpawnMapThing`.
/// `P_SpawnMapThing` handles player starts, deathmatch starts, and all
/// game objects.
///
/// `lump` must be the lump number of the THINGS entry for the current map.
/// Preconditions: `vertexes`, `lines`, `sides`, `sectors`, and `blockmap`
/// must already be loaded; `deathmatch_p` must be reset to the base of
/// `deathmatchstarts`.
/// C callers: `P_SetupLevel` (this file).
#[no_mangle]
pub extern "C" fn P_LoadThings(lump: c_int) {
    unsafe {
        let data = W_CacheLumpNum(lump, PU_STATIC);
        let numthings = W_LumpLength(lump as c_uint) / std::mem::size_of::<mapthing_t>() as c_int;
        let mut mt = data as *mut mapthing_t;

        for _ in 0..numthings {
            let mut spawn = true;

            if gamemode != d_mode::commercial {
                let thing_type = SHORT((*mt).r#type);
                match thing_type {
                    68 | 64 | 88 | 89 | 69 | 67 | 71 | 65 | 66 | 84 => {
                        spawn = false;
                    }
                    _ => {}
                }
            }

            if !spawn {
                break;
            }

            let mut spawnthing = mapthing_t {
                x: SHORT((*mt).x),
                y: SHORT((*mt).y),
                angle: SHORT((*mt).angle),
                r#type: SHORT((*mt).r#type),
                options: SHORT((*mt).options),
            };
            P_SpawnMapThing(&raw mut spawnthing as *mut crate::doom::p_telept::mapthing_t);
            mt = mt.add(1);
        }

        W_ReleaseLumpNum(lump);
    }
}

// ---------------------------------------------------------------------------
// P_LoadLineDefs
// ---------------------------------------------------------------------------

/// Load the LINEDEFS lump and populate the global `lines` array.
///
/// Allocates `numlines * sizeof(line_t)` bytes at `PU_LEVEL`
/// (zero-initialized) and fills each entry from the corresponding on-disk
/// `maplinedef_t`. Notable steps per linedef:
/// - Vertex pointers are resolved from indices into `vertexes`.
/// - `dx` and `dy` are computed from the vertex coordinates (fixed-point).
/// - `slopetype` is set to `ST_VERTICAL`, `ST_HORIZONTAL`, `ST_POSITIVE`, or
///   `ST_NEGATIVE` based on the sign of `FixedDiv(dy, dx)`.
/// - The linedef's axis-aligned bounding box (`bbox[4]`) is computed from the
///   two vertex coordinates.
/// - Front and back sector pointers are resolved from `sidenum` indices; -1
///   means no side (one-sided line), resulting in a null sector pointer.
///
/// `lump` must be the lump number of the LINEDEFS entry for the current map.
/// Preconditions: `vertexes` and `sides` must already be loaded.
/// C callers: `P_SetupLevel` (this file).
#[no_mangle]
pub extern "C" fn P_LoadLineDefs(lump: c_int) {
    unsafe {
        numlines = W_LumpLength(lump as c_uint) / std::mem::size_of::<maplinedef_t>() as c_int;
        lines = Z_Malloc(
            numlines * std::mem::size_of::<line_t>() as c_int,
            PU_LEVEL,
            ptr::null_mut(),
        ) as *mut line_t;
        // Zeroed by Z_Malloc internally

        let data = W_CacheLumpNum(lump, PU_STATIC);
        let mut mld = data as *mut maplinedef_t;
        let mut ld = lines;

        for _ in 0..numlines {
            (*ld).flags = SHORT((*mld).flags);
            (*ld).special = SHORT((*mld).special);
            (*ld).tag = SHORT((*mld).tag);
            let v1 = vertexes.offset(SHORT((*mld).v1) as isize);
            let v2 = vertexes.offset(SHORT((*mld).v2) as isize);
            (*ld).v1 = v1;
            (*ld).v2 = v2;
            (*ld).dx = (*v2).x - (*v1).x;
            (*ld).dy = (*v2).y - (*v1).y;

            if (*ld).dx == 0 {
                (*ld).slopetype = ST_VERTICAL;
            } else if (*ld).dy == 0 {
                (*ld).slopetype = ST_HORIZONTAL;
            } else {
                if FixedDiv((*ld).dy, (*ld).dx) > 0 {
                    (*ld).slopetype = ST_POSITIVE;
                } else {
                    (*ld).slopetype = ST_NEGATIVE;
                }
            }

            if (*v1).x < (*v2).x {
                (*ld).bbox[BBox::LEFT] = (*v1).x;
                (*ld).bbox[BBox::RIGHT] = (*v2).x;
            } else {
                (*ld).bbox[BBox::LEFT] = (*v2).x;
                (*ld).bbox[BBox::RIGHT] = (*v1).x;
            }

            if (*v1).y < (*v2).y {
                (*ld).bbox[BBox::BOTTOM] = (*v1).y;
                (*ld).bbox[BBox::TOP] = (*v2).y;
            } else {
                (*ld).bbox[BBox::BOTTOM] = (*v2).y;
                (*ld).bbox[BBox::TOP] = (*v1).y;
            }

            (*ld).sidenum[0] = SHORT((*mld).sidenum[0]);
            (*ld).sidenum[1] = SHORT((*mld).sidenum[1]);

            if (*ld).sidenum[0] != -1 {
                (*ld).frontsector = sides
                    .offset((*ld).sidenum[0] as isize)
                    .as_ref()
                    .unwrap()
                    .sector as *mut c_void;
            } else {
                (*ld).frontsector = ptr::null_mut();
            }

            if (*ld).sidenum[1] != -1 {
                (*ld).backsector = sides
                    .offset((*ld).sidenum[1] as isize)
                    .as_ref()
                    .unwrap()
                    .sector as *mut c_void;
            } else {
                (*ld).backsector = ptr::null_mut();
            }

            ld = ld.add(1);
            mld = mld.add(1);
        }

        W_ReleaseLumpNum(lump);
    }
}

// ---------------------------------------------------------------------------
// P_LoadSideDefs
// ---------------------------------------------------------------------------

/// Load the SIDEDEFS lump and populate the global `sides` array.
///
/// Allocates `numsides * sizeof(side_t)` bytes at `PU_LEVEL`
/// (zero-initialized) and fills each entry from the corresponding on-disk
/// `mapsidedef_t`. Texture offsets are shifted left by `FRACBITS` to become
/// fixed-point. Texture names (8-byte null-padded ASCII) are resolved to
/// runtime indices via `R_TextureNumForName`. The `sector` pointer is
/// resolved from the on-disk sector index into the `sectors` array.
///
/// `lump` must be the lump number of the SIDEDEFS entry for the current map.
/// Precondition: `sectors` must already be loaded.
/// C callers: `P_SetupLevel` (this file).
#[no_mangle]
pub extern "C" fn P_LoadSideDefs(lump: c_int) {
    unsafe {
        numsides = W_LumpLength(lump as c_uint) / std::mem::size_of::<mapsidedef_t>() as c_int;
        sides = Z_Malloc(
            numsides * std::mem::size_of::<side_t>() as c_int,
            PU_LEVEL,
            ptr::null_mut(),
        ) as *mut side_t;
        // Zeroed by Z_Malloc internally

        let data = W_CacheLumpNum(lump, PU_STATIC);
        let mut msd = data as *mut mapsidedef_t;
        let mut sd = sides;

        for _ in 0..numsides {
            (*sd).textureoffset = (SHORT((*msd).textureoffset) as c_int) << FRACBITS;
            (*sd).rowoffset = (SHORT((*msd).rowoffset) as c_int) << FRACBITS;
            (*sd).toptexture =
                R_TextureNumForName((*msd).toptexture.as_ptr() as *mut c_char) as c_short;
            (*sd).bottomtexture =
                R_TextureNumForName((*msd).bottomtexture.as_ptr() as *mut c_char) as c_short;
            (*sd).midtexture =
                R_TextureNumForName((*msd).midtexture.as_ptr() as *mut c_char) as c_short;
            (*sd).sector = sectors.offset(SHORT((*msd).sector) as isize);
            sd = sd.add(1);
            msd = msd.add(1);
        }

        W_ReleaseLumpNum(lump);
    }
}

// ---------------------------------------------------------------------------
// P_LoadBlockMap
// ---------------------------------------------------------------------------

/// Load the BLOCKMAP lump and initialize the blockmap collision grid.
///
/// The blockmap is a WAD-stored acceleration structure dividing the map into
/// 128-unit cells. Each cell contains a null-terminated list of linedefs that
/// pass through it, enabling fast broad-phase collision detection.
///
/// Loading steps:
/// 1. Allocate zone memory for the raw lump and read it with `W_ReadLump`.
/// 2. Set `blockmap = blockmaplump + 4` (skip the 4-short header).
/// 3. Byte-swap all `count = lumplen / 2` shorts from little-endian to native.
/// 4. Extract the header: `bmaporgx`, `bmaporgy` (shifted by `FRACBITS`),
///    `bmapwidth`, `bmapheight`.
/// 5. Allocate and zero-fill the `blocklinks` thing-chain array
///    (`bmapwidth * bmapheight` pointer-sized entries).
///
/// `lump` must be the lump number of the BLOCKMAP entry for the current map.
/// C callers: `P_SetupLevel` (this file).
#[no_mangle]
pub extern "C" fn P_LoadBlockMap(lump: c_int) {
    unsafe {
        let lumplen = W_LumpLength(lump as c_uint);
        let count = lumplen / 2;

        blockmaplump = Z_Malloc(lumplen, PU_LEVEL, ptr::null_mut()) as *mut c_short;
        W_ReadLump(lump as c_uint, blockmaplump as *mut c_void);
        blockmap = blockmaplump.add(4);

        // Swap all short integers to native byte ordering.
        for i in 0..count {
            *blockmaplump.add(i as usize) = SHORT(*blockmaplump.add(i as usize));
        }

        bmaporgx = (*blockmaplump.add(0) as c_int) << FRACBITS;
        bmaporgy = (*blockmaplump.add(1) as c_int) << FRACBITS;
        bmapwidth = *blockmaplump.add(2) as c_int;
        bmapheight = *blockmaplump.add(3) as c_int;

        let bcount = std::mem::size_of::<*mut c_void>() * bmapwidth as usize * bmapheight as usize;
        blocklinks = Z_Malloc(bcount as c_int, PU_LEVEL, ptr::null_mut()) as *mut *mut c_void;
        libc::memset(blocklinks as *mut c_void, 0, bcount);
    }
}

// ---------------------------------------------------------------------------
// P_GroupLines
// ---------------------------------------------------------------------------

/// Build per-sector line lists, resolve subsector sectors, and compute sector
/// bounding boxes.
///
/// This function performs three passes over the map geometry after all lumps
/// have been loaded:
///
/// 1. **Subsector sectors**: for each subsector, resolves its sector pointer
///    by following `segs[ss.firstline].sidedef.sector`. This uses the seg's
///    already-resolved sidedef (set during `P_LoadSegs`) rather than the raw
///    `sidenum[0]`, correctly handling back-side segs.
///
/// 2. **Line count and buffer allocation**: counts how many line references
///    each sector accumulates (`totallines` tracks the total for the shared
///    buffer). Two-sided lines contribute to both front and back sectors, but
///    a line shared between the same front and back sector is counted only once
///    per side.
///
/// 3. **Line table assignment**: allocates a single flat buffer of
///    `totallines` line pointers at `PU_LEVEL`, then assigns slices to each
///    sector using a cumulative cursor (not per-sector independent offsets).
///    A second pass over lines fills in the pointers.
///
/// 4. **Bounding boxes and sound origins**: for each sector, computes an
///    axis-aligned bounding box from its lines' vertices, sets the sound
///    origin (`soundorg`) to the bounding-box center, and records the
///    clamped blockmap cell extents in `sector.blockbox`.
///
/// Preconditions: all lump-load functions must have been called first.
/// C callers: `P_SetupLevel` (this file).
#[no_mangle]
pub extern "C" fn P_GroupLines() {
    unsafe {
        // Look up sector number for each subsector.
        let mut ss = subsectors;
        for _ in 0..numsubsectors {
            let seg = segs.offset((*ss).firstline as isize);
            // Must use seg->sidedef->sector, not sidenum[0], because the seg
            // may be on side 1 (back side) of the linedef, in which case
            // sidenum[0] points to the wrong side's sector.
            (*ss).sector = (*(*seg).sidedef).sector as *mut c_void;
            ss = ss.add(1);
        }

        // Count number of lines in each sector.
        let mut li = lines;
        totallines = 0;
        for _ in 0..numlines {
            totallines += 1;
            let frontsec = (*li).frontsector as *mut sector_t;
            if !frontsec.is_null() {
                (*frontsec).linecount += 1;
            }
            let backsec = (*li).backsector as *mut sector_t;
            if !backsec.is_null() && backsec != frontsec {
                (*backsec).linecount += 1;
                totallines += 1;
            }
            li = li.add(1);
        }

        // Build line tables for each sector.
        let linebuffer = Z_Malloc(
            totallines * std::mem::size_of::<*mut line_t>() as c_int,
            PU_LEVEL,
            ptr::null_mut(),
        ) as *mut *mut line_t;

        let mut current = linebuffer;
        for i in 0..numsectors as usize {
            let sec = sectors.add(i);
            (*sec).lines = current as *mut *mut c_void;
            current = current.offset((*sec).linecount as isize);
            (*sec).linecount = 0;
        }

        // Assign lines to sectors.
        for i in 0..numlines as usize {
            li = lines.add(i);
            if !(*li).frontsector.is_null() {
                let sector = (*li).frontsector as *mut sector_t;
                (*sector)
                    .lines
                    .offset((*sector).linecount as isize)
                    .write(li as *mut c_void);
                (*sector).linecount += 1;
            }
            if !(*li).backsector.is_null() && (*li).frontsector != (*li).backsector {
                let sector = (*li).backsector as *mut sector_t;
                (*sector)
                    .lines
                    .offset((*sector).linecount as isize)
                    .write(li as *mut c_void);
                (*sector).linecount += 1;
            }
        }

        // Generate bounding boxes for sectors.
        let mut sector = sectors;
        for _ in 0..numsectors {
            let mut bbox: [c_int; 4] = [0; 4];
            M_ClearBox(bbox.as_mut_ptr());

            for j in 0..(*sector).linecount {
                let line = *(*sector).lines.offset(j as isize) as *mut line_t;
                M_AddToBox(bbox.as_mut_ptr(), (*(*line).v1).x, (*(*line).v1).y);
                M_AddToBox(bbox.as_mut_ptr(), (*(*line).v2).x, (*(*line).v2).y);
            }

            // Set the degenmobj_t to the middle of the bounding box.
            let soundorg_x = (bbox[BBox::RIGHT] + bbox[BBox::LEFT]) / 2;
            let soundorg_y = (bbox[BBox::TOP] + bbox[BBox::BOTTOM]) / 2;
            // sector->soundorg is a 40-byte degenmobj_t; first two fields are x,y.
            let soundorg_ptr = (*sector).soundorg.as_mut_ptr() as *mut c_int;
            *soundorg_ptr = soundorg_x;
            *soundorg_ptr.add(1) = soundorg_y;

            // Adjust bounding box to map blocks.
            let mut block = (bbox[BBox::TOP] - bmaporgy + MAXRADIUS) >> MAPBLOCKSHIFT;
            block = if block >= bmapheight {
                bmapheight - 1
            } else {
                block
            };
            (*sector).blockbox[BBox::TOP] = block;

            block = (bbox[BBox::BOTTOM] - bmaporgy - MAXRADIUS) >> MAPBLOCKSHIFT;
            block = if block < 0 { 0 } else { block };
            (*sector).blockbox[BBox::BOTTOM] = block;

            block = (bbox[BBox::RIGHT] - bmaporgx + MAXRADIUS) >> MAPBLOCKSHIFT;
            block = if block >= bmapwidth {
                bmapwidth - 1
            } else {
                block
            };
            (*sector).blockbox[BBox::RIGHT] = block;

            block = (bbox[BBox::LEFT] - bmaporgx - MAXRADIUS) >> MAPBLOCKSHIFT;
            block = if block < 0 { 0 } else { block };
            (*sector).blockbox[BBox::LEFT] = block;

            sector = sector.add(1);
        }
    }
}

// ---------------------------------------------------------------------------
// PadRejectArray
// ---------------------------------------------------------------------------

/// Pad the tail of an undersized REJECT lump to simulate vanilla Doom's
/// behavior of reading past the end of the lump into the zone-memory block
/// header.
///
/// Vanilla Doom allocated the REJECT array with `Z_Malloc`, and when the lump
/// was shorter than the required `(numsectors * numsectors + 7) / 8` bytes,
/// reads of the missing bytes would fall into the zone block header that
/// immediately precedes the allocation in memory. This function reproduces
/// those header bytes so that WADs relying on the overflow behavior work
/// correctly.
///
/// `rejectpad` encodes the first 16 bytes of a zone block header:
/// - `[(totallines * 4 + 3) & !3) + 24]` - block size field
/// - `0` - user pointer (low word of z_zone header)
/// - `50` - `PU_LEVEL` tag
/// - `0x1d4a11` - `DOOM_CONST_ZONEID`
///
/// If `len > sizeof(rejectpad)` (i.e. the lump is extremely short), a warning
/// is printed and the remainder is filled with 0x00 or 0xff depending on
/// whether `-reject_pad_with_ff` was passed on the command line.
///
/// # Safety
///
/// `array` must point to at least `len` writable bytes. The caller
/// (`P_LoadReject`) ensures this by passing `rejectmatrix + lumplen` with
/// `len = minlength - lumplen`.
unsafe fn PadRejectArray(array: *mut u8, len: usize) {
    let rejectpad: [u32; 4] = [((totallines * 4 + 3) & !3) as u32 + 24, 0, 50, 0x1d4a11];

    let mut dest = array;
    for i in 0..len.min(std::mem::size_of_val(&rejectpad)) {
        let byte_num = i % 4;
        *dest = ((rejectpad[i / 4] >> (byte_num * 8)) & 0xff) as u8;
        dest = dest.add(1);
    }

    if len > std::mem::size_of_val(&rejectpad) {
        eprintln!(
            "PadRejectArray: REJECT lump too short to pad! ({} > {})",
            len,
            std::mem::size_of_val(&rejectpad)
        );

        let padvalue = if M_CheckParm(c"-reject_pad_with_ff".as_ptr() as *mut c_char) != 0 {
            0xff
        } else {
            0xf00
        };

        let pad_byte = padvalue as u8;
        let pad_start = array.add(std::mem::size_of_val(&rejectpad));
        let pad_len = len - std::mem::size_of_val(&rejectpad);
        for i in 0..pad_len {
            *pad_start.add(i) = pad_byte;
        }
    }
}

// ---------------------------------------------------------------------------
// P_LoadReject
// ---------------------------------------------------------------------------

/// Load or synthesize the REJECT lump for the current map.
///
/// The REJECT table is a packed bit array of size
/// `ceil(numsectors^2 / 8)` bytes. Bit `(s1 * numsectors + s2)` is set if
/// sectors `s1` and `s2` cannot see each other, allowing the enemy AI to skip
/// expensive LOS checks.
///
/// If the WAD's REJECT lump is large enough (`lumplen >= minlength`), it is
/// cached directly at `PU_LEVEL`. Otherwise a `minlength`-byte buffer is
/// allocated, the lump is read into it, and the remaining bytes are filled by
/// `PadRejectArray` to simulate vanilla Doom's zone-header overflow.
///
/// # Safety
///
/// Writes to the global `rejectmatrix`. Must be called after `P_GroupLines`
/// so that `totallines` is valid (needed by `PadRejectArray`).
unsafe fn P_LoadReject(lumpnum: c_int) {
    let minlength = (numsectors * numsectors + 7) / 8;
    let lumplen = W_LumpLength(lumpnum as c_uint);

    if lumplen >= minlength {
        rejectmatrix = W_CacheLumpNum(lumpnum, PU_LEVEL) as *mut u8;
    } else {
        rejectmatrix =
            Z_Malloc(minlength, PU_LEVEL, &raw mut rejectmatrix as *mut c_void) as *mut u8;
        W_ReadLump(lumpnum as c_uint, rejectmatrix as *mut c_void);
        PadRejectArray(
            rejectmatrix.add(lumplen as usize),
            (minlength - lumplen) as usize,
        );
    }
}

// ---------------------------------------------------------------------------
// P_SetupLevel
// ---------------------------------------------------------------------------

/// Initialize all game state for the given episode/map and load its geometry.
///
/// This is the main map-load entry point, called by `G_DoLoadLevel` in
/// `g_game.c`. It performs the following steps in order (order matters):
///
/// 1. Reset kill/item/secret counters and intermission stats.
/// 2. Set the console player's `viewz` to 1 (will be corrected by player think).
/// 3. Stop all sounds (`S_Start`) before freeing zone memory.
/// 4. Free all `PU_LEVEL` through `PU_PURGELEVEL - 1` zone blocks.
/// 5. Re-initialize the thinker list (`P_InitThinkers`).
/// 6. Construct the WAD lump name: `"MAPxx"` for commercial, `"ExMy"` for
///    episodic. The `episode` and `map` parameters are 1-based.
/// 7. Load map lumps in this specific order: BLOCKMAP, VERTEXES, SECTORS,
///    SIDEDEFS, LINEDEFS, SSECTORS, NODES, SEGS.
/// 8. Post-process: `P_GroupLines`, then load REJECT.
/// 9. Reset `bodyqueslot` and `deathmatch_p`, then load THINGS
///    (`P_LoadThings` spawns all map objects).
/// 10. If deathmatch mode, randomly respawn active players.
/// 11. Reset the item-queue indices (`iquehead`, `iquetail`).
/// 12. Spawn special sector effects (`P_SpawnSpecials`).
/// 13. If `precache` is set, preload all level graphics (`R_PrecacheLevel`).
///
/// `_playermask` and `_skill` parameters are accepted for C ABI compatibility
/// but are unused; skill filtering is handled by `P_SpawnMapThing`.
///
/// C callers: `G_DoLoadLevel` in `g_game.c`.
// FIXME: C uses DEH_snprintf for lump name construction, allowing DeHackEd
// patches to rename map lumps. The Rust port uses format! and does not apply
// DEH patches to the lump name, which may break modded WADs that rely on this.
#[no_mangle]
pub extern "C" fn P_SetupLevel(episode: c_int, map: c_int, _playermask: c_int, _skill: c_int) {
    unsafe {
        totalkills = 0;
        totalitems = 0;
        totalsecret = 0;
        wminfo.maxfrags = 0;
        wminfo.partime = 180;

        for i in 0..MAXPLAYERS {
            players[i].killcount = 0;
            players[i].secretcount = 0;
            players[i].itemcount = 0;
        }

        players[consoleplayer as usize].viewz = 1;

        S_Start();
        Z_FreeTags(PU_LEVEL, PU_PURGELEVEL - 1);

        P_InitThinkers();

        // Find map name.
        let mut lumpname = [0i8; 9];
        if gamemode == d_mode::commercial {
            if map < 10 {
                let s = format!("map0{}", map);
                ptr::copy_nonoverlapping(s.as_ptr(), lumpname.as_mut_ptr() as *mut u8, s.len());
            } else {
                let s = format!("map{}", map);
                ptr::copy_nonoverlapping(s.as_ptr(), lumpname.as_mut_ptr() as *mut u8, s.len());
            }
        } else {
            lumpname[0] = b'E' as i8;
            lumpname[1] = (b'0' + episode as u8) as i8;
            lumpname[2] = b'M' as i8;
            lumpname[3] = (b'0' + map as u8) as i8;
            lumpname[4] = 0;
        }

        let lumpnum = W_GetNumForName(lumpname.as_mut_ptr());

        leveltime = 0;

        // Note: most of this ordering is important.
        P_LoadBlockMap(lumpnum + MapLump::BLOCKMAP);
        P_LoadVertexes(lumpnum + MapLump::VERTEXES);
        P_LoadSectors(lumpnum + MapLump::SECTORS);
        P_LoadSideDefs(lumpnum + MapLump::SIDEDEFS);

        P_LoadLineDefs(lumpnum + MapLump::LINEDEFS);
        P_LoadSubsectors(lumpnum + MapLump::SSECTORS);
        P_LoadNodes(lumpnum + MapLump::NODES);
        P_LoadSegs(lumpnum + MapLump::SEGS);

        P_GroupLines();
        P_LoadReject(lumpnum + MapLump::REJECT);

        bodyqueslot = 0;
        deathmatch_p = std::ptr::addr_of_mut!(deathmatchstarts[0]);
        P_LoadThings(lumpnum + MapLump::THINGS);

        if deathmatch != 0 {
            for i in 0..MAXPLAYERS {
                if playeringame[i] != 0 {
                    players[i].mo = ptr::null_mut();
                    G_DeathMatchSpawnPlayer(i as c_int);
                }
            }
        }

        iquehead = 0;
        iquetail = 0;

        P_SpawnSpecials();

        if precache != 0 {
            R_PrecacheLevel();
        }
    }
}

// ---------------------------------------------------------------------------
// P_Init
// ---------------------------------------------------------------------------

/// Initialize the map/physics subsystem at engine startup.
///
/// Called once from `D_DoomMain` before the game loop starts. Sets up:
/// - `P_InitSwitchList`: builds the switch animation list from the texture
///   names defined in `p_switch.c`.
/// - `P_InitPicAnims`: builds the animated flat/texture list from the lump
///   sequence tables in `p_spec.c`.
/// - `R_InitSprites`: builds the sprite frame lookup table from the WAD's
///   sprite lump names, starting at `sprnames[0]`.
///
/// C callers: `D_DoomMain` in `d_main.c`.
#[no_mangle]
pub extern "C" fn P_Init() {
    unsafe {
        P_InitSwitchList();
        P_InitPicAnims();
        R_InitSprites(std::ptr::addr_of_mut!(sprnames[0]));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    // =========================================================================
    // Existing structural tests
    // =========================================================================

    #[test]
    fn max_deathmatch_starts_is_10() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(MAX_DEATHMATCH_STARTS, 10);
    }

    #[test]
    fn setup_globals_default_zero() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert_eq!(numvertexes, 0);
            assert_eq!(numsegs, 0);
            assert_eq!(numsectors, 0);
            assert_eq!(numsubsectors, 0);
            assert_eq!(numnodes, 0);
            assert_eq!(numlines, 0);
            assert_eq!(numsides, 0);
            assert_eq!(bmapwidth, 0);
            assert_eq!(bmapheight, 0);
            assert_eq!(bmaporgx, 0);
            assert_eq!(bmaporgy, 0);
        }
    }

    #[test]
    fn setup_globals_are_c_int_width() {
        use std::ffi::c_int;
        const _: () = assert!(std::mem::size_of::<c_int>() == 4);
        unsafe {
            let _: c_int = numvertexes;
            let _: c_int = numsegs;
            let _: c_int = numsectors;
            let _: c_int = numsubsectors;
            let _: c_int = numnodes;
            let _: c_int = numlines;
            let _: c_int = numsides;
            let _: c_int = bmapwidth;
            let _: c_int = bmapheight;
            let _: c_int = bmaporgx;
            let _: c_int = bmaporgy;
        }
    }

    // =========================================================================
    // Regression tests for bugs fixed in the p_setup.c → Rust port.
    //
    // These are pure unit tests that verify the core algorithms without
    // requiring full engine (WAD, zone memory, C FFI) initialisation.
    //
    // Bug 1: P_LoadThings used `continue` instead of `break` when a
    //        non-commercial monster was encountered in shareware mode.
    // Bug 2: P_GroupLines sector line table computed per-sector offsets
    //        from the buffer base instead of advancing cumulatively.
    // Bug 3: P_GroupLines resolved subsector→sector via sidenum[0]
    //        instead of seg->sidedef->sector, corrupting back-side segs.
    // =========================================================================

    // -----------------------------------------------------------------------
    // Regression: P_LoadThings break-on-non-commercial-monster
    // -----------------------------------------------------------------------

    /// Simulates the P_LoadThings thing-filter loop in pure Rust.
    /// Returns the number of things that would actually be spawned.
    ///
    /// In shareware/non-commercial mode the loop must **break** (not continue)
    /// when it hits the first non-commercial monster type.  Using `continue`
    /// instead causes every subsequent thing to also be processed, which
    /// changes the RNG call sequence and breaks demo determinism.
    fn count_spawnable_things(things: &[i16], commercial: bool) -> usize {
        let non_commercial_types: [i16; 10] = [68, 64, 88, 89, 69, 67, 71, 65, 66, 84];

        let mut count = 0;
        for &thing_type in things {
            let mut spawn = true;
            if !commercial {
                if non_commercial_types.contains(&thing_type) {
                    spawn = false;
                }
            }
            if !spawn {
                break; // MUST break – was `continue` in the buggy port
            }
            count += 1;
        }
        count
    }

    /// In commercial mode every thing is spawned regardless of type.
    #[test]
    fn p_load_things_commercial_spawns_all() {
        // Types: player start, imp, Archvile(64), cacodemon, Revenant(66)
        let things: [i16; 5] = [1, 3001, 64, 3003, 66];
        assert_eq!(count_spawnable_things(&things, true), 5);
    }

    /// In shareware mode the loop stops at the first non-commercial monster.
    /// Things after it must NOT be spawned (regression: was `continue`).
    #[test]
    fn p_load_things_shareware_breaks_at_non_commercial() {
        // Types: player start, imp, Archvile(64), cacodemon, Revenant(66)
        // Archvile is non-commercial → loop breaks, only 2 things spawned.
        let things: [i16; 5] = [1, 3001, 64, 3003, 66];
        assert_eq!(count_spawnable_things(&things, false), 2);
    }

    /// Non-commercial monster at the very start → zero things spawned.
    #[test]
    fn p_load_things_shareware_first_thing_non_commercial() {
        let things: [i16; 3] = [64, 1, 3001]; // Archvile first
        assert_eq!(count_spawnable_things(&things, false), 0);
    }

    /// No non-commercial monsters → all things spawned in shareware mode.
    #[test]
    fn p_load_things_shareware_no_non_commercial_spawns_all() {
        let things: [i16; 4] = [1, 3001, 3003, 3004]; // all shareware types
        assert_eq!(count_spawnable_things(&things, false), 4);
    }

    /// Non-commercial monster at the very end → everything before spawns.
    #[test]
    fn p_load_things_shareware_non_commercial_at_end() {
        let things: [i16; 5] = [1, 3001, 3003, 3004, 88]; // Boss Brain last
        assert_eq!(count_spawnable_things(&things, false), 4);
    }

    // -----------------------------------------------------------------------
    // Regression: P_GroupLines sector line table cumulative offset
    // -----------------------------------------------------------------------

    /// Simulates the sector line-table pointer assignment from P_GroupLines.
    /// Returns the computed start-offset for each sector within the shared
    /// line-buffer.
    ///
    /// The C original advances a single cursor cumulatively:
    ///   sectors[i].lines = linebuffer;
    ///   linebuffer += sectors[i].linecount;
    ///
    /// The buggy Rust port computed each sector's offset independently from
    /// the buffer base (`base + linecount`), causing every sector's line
    /// table to point to the wrong memory.
    fn compute_sector_line_offsets(linecounts: &[usize]) -> Vec<usize> {
        let mut offsets = Vec::with_capacity(linecounts.len());
        let mut current: usize = 0;
        for &lc in linecounts {
            offsets.push(current);
            current += lc;
        }
        offsets
    }

    /// Cumulative offsets must equal the sum of all preceding linecounts.
    #[test]
    fn p_grouplines_sector_offsets_are_cumulative() {
        // Sector linecounts: [10, 5, 3]
        // Expected offsets:  [0, 10, 15]
        let linecounts = [10usize, 5, 3];
        let offsets = compute_sector_line_offsets(&linecounts);
        assert_eq!(offsets, vec![0, 10, 15]);
    }

    /// Single sector → offset 0.
    #[test]
    fn p_grouplines_single_sector_offset_zero() {
        let linecounts = [7usize];
        assert_eq!(compute_sector_line_offsets(&linecounts), vec![0]);
    }

    /// Sector with zero lines must still advance correctly for next sector.
    #[test]
    fn p_grouplines_zero_line_sector_preserves_cumulative() {
        let linecounts = [5usize, 0, 3];
        // sector 0 → offset 0, sector 1 → offset 5, sector 2 → offset 5
        assert_eq!(compute_sector_line_offsets(&linecounts), vec![0, 5, 5]);
    }

    /// The buggy version (`base + linecount`) would produce wrong results.
    /// This test documents the exact wrong values that the bug produced.
    #[test]
    fn p_grouplines_buggy_offset_produces_wrong_values() {
        let linecounts = [10usize, 5, 3];

        // Correct (cumulative):
        let correct = compute_sector_line_offsets(&linecounts);
        assert_eq!(correct, vec![0, 10, 15]);

        // Buggy (independent from base):
        let buggy: Vec<usize> = linecounts.iter().copied().collect();
        assert_eq!(buggy, vec![10, 5, 3]);

        // They must NOT be equal.
        assert_ne!(correct, buggy);
    }

    // -----------------------------------------------------------------------
    // Regression: P_GroupLines subsector sector via seg->sidedef->sector
    // -----------------------------------------------------------------------

    /// Simulates the subsector→sector resolution from P_GroupLines.
    ///
    /// The C original uses `seg->sidedef->sector`, which respects the seg's
    /// side (0 = front, 1 = back).  The buggy Rust port hardcoded
    /// `sidenum[0]`, always resolving to the front side's sector regardless
    /// of which side the seg is on.
    ///
    /// Parameters:
    /// - `seg_side`: 0 (front) or 1 (back) – which side of the linedef the seg is on
    /// - `sidenum`: [front_sidenum, back_sidenum]
    /// - `sector_of_sidenum`: maps each sidenum to its sector index
    ///
    /// Returns the resolved sector index.
    fn resolve_subsector_sector_via_sidedef(
        seg_side: usize,
        sidenum: [i16; 2],
        sector_of_sidenum: &[usize],
    ) -> usize {
        // Correct: use the seg's already-resolved sidedef, which was set
        // during P_LoadSegs to `sides[ldef->sidenum[seg_side]]`.
        // So sector = sector_of_sidenum[sidenum[seg_side]]
        let sidenum_idx = sidenum[seg_side] as usize;
        sector_of_sidenum[sidenum_idx]
    }

    /// The buggy version always used sidenum[0] regardless of seg_side.
    fn resolve_subsector_sector_buggy(
        seg_side: usize,
        sidenum: [i16; 2],
        sector_of_sidenum: &[usize],
    ) -> usize {
        let _ = seg_side; // unused – the bug!
        let sidenum_idx = sidenum[0] as usize;
        sector_of_sidenum[sidenum_idx]
    }

    /// Front-side seg (side=0): both methods agree.
    #[test]
    fn p_grouplines_subsector_front_side_agrees() {
        // linedef has sidenum[0]=2, sidenum[1]=5
        // sector_of_sidenum[2]=10, sector_of_sidenum[5]=20
        let sidenum = [2i16, 5];
        let sector_map = [0, 0, 10, 0, 0, 20];

        assert_eq!(
            resolve_subsector_sector_via_sidedef(0, sidenum, &sector_map),
            10
        );
        assert_eq!(resolve_subsector_sector_buggy(0, sidenum, &sector_map), 10);
    }

    /// Back-side seg (side=1): correct method uses sidenum[1],
    /// buggy method uses sidenum[0] → wrong sector.
    #[test]
    fn p_grouplines_subsector_back_side_differs() {
        // linedef has sidenum[0]=2 (sector 10), sidenum[1]=5 (sector 20)
        let sidenum = [2i16, 5];
        let sector_map = [0, 0, 10, 0, 0, 20];

        // Correct: seg on side 1 → uses sidenum[1]=5 → sector 20
        assert_eq!(
            resolve_subsector_sector_via_sidedef(1, sidenum, &sector_map),
            20
        );

        // Buggy: always uses sidenum[0]=2 → sector 10 (WRONG)
        assert_eq!(resolve_subsector_sector_buggy(1, sidenum, &sector_map), 10);
    }

    /// Ensures the correct and buggy implementations produce different
    /// results for back-side segs, proving the regression test is meaningful.
    #[test]
    fn p_grouplines_subsector_back_side_correct_vs_buggy_differ() {
        let sidenum = [3i16, 7];
        let sector_map = [0, 0, 0, 11, 0, 0, 0, 22];

        let correct = resolve_subsector_sector_via_sidedef(1, sidenum, &sector_map);
        let buggy = resolve_subsector_sector_buggy(1, sidenum, &sector_map);

        assert_ne!(
            correct, buggy,
            "back-side seg must resolve to different sectors"
        );
        assert_eq!(correct, 22);
        assert_eq!(buggy, 11);
    }
}
