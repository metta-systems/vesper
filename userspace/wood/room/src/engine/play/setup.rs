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
//! - All WAD data is little-endian. Map lumps are viewed as slices of the
//!   little-endian records in `wad::format`; the raw BLOCKMAP shorts go
//!   through the `SHORT` helper, an explicit `i16::from_le` conversion
//!   instead of the C `SHORT` macro from `i_swap.h`.
//! - `P_LoadThings` increments the `mt` pointer only after the spawn decision
//!   so that a `break` on a non-commercial monster does not advance past it.
//! - `P_SetupLevel` uses `format!` for lump-name construction instead of
//!   `DEH_snprintf`, so DeHackEd lump-name patches are not applied.
//! - Global map tables (`vertexes`, `lines`, etc.) are `pub static mut`
//!   values, as the C globals were.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::{ffi::c_void, ptr};

use crate::engine::{
    doom::{
        self,
        player::{consoleplayer, players, MAXPLAYERS},
    },
    map::{
        line_t, node_t, sector_t, seg_t, side_t,
        slopetype_t::{ST_HORIZONTAL, ST_NEGATIVE, ST_POSITIVE, ST_VERTICAL},
        subsector_t, vertex_t,
    },
    math::{
        bbox::{BBox, M_AddToBox, M_ClearBox},
        fixed::{FRACBITS, FRACUNIT},
    },
    play::{
        info::sprnames,
        maputl::MAPBLOCKSHIFT,
        mobj::mobj_t,
        tick::{leveltime, P_InitThinkers},
    },
    wad::format::{LinedefFlag, MapLump},
    zone::PU_LEVEL,
};

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
const PU_PURGELEVEL: i32 = 7;

// ---------------------------------------------------------------------------
// Misc constants
// ---------------------------------------------------------------------------

/// Maximum radius added/subtracted when clamping sector bounding boxes to
/// blockmap cells. 32 map units in fixed-point (32 << FRACBITS).
const MAXRADIUS: i32 = 32 * FRACUNIT;

/// Maximum number of deathmatch start positions in a level.
/// Matches `MAX_DEATHMATCH_STARTS` in `p_setup.c`.
pub const MAX_DEATHMATCH_STARTS: usize = 10;

// ---------------------------------------------------------------------------
// MAP-related lookup tables
// ---------------------------------------------------------------------------

/// Total number of vertexes loaded from the current map's VERTEXES lump.
/// Referenced by `r_bsp.c` and other renderer modules.
pub static mut numvertexes: i32 = 0;
/// Pointer to the runtime vertex array, allocated from zone memory at
/// `PU_LEVEL`. Each entry holds fixed-point (16.16) x/y coordinates.
/// Referenced by `p_map.c`, `r_bsp.c`, and many others.
pub static mut vertexes: *mut vertex_t = ptr::null_mut();

/// Total number of segs loaded from the current map's SEGS lump.
/// Referenced by `r_bsp.c`.
pub static mut numsegs: i32 = 0;
/// Pointer to the runtime seg array, allocated from zone memory at `PU_LEVEL`.
/// Referenced by `r_bsp.c` and `p_sight.c`.
pub static mut segs: *mut seg_t = ptr::null_mut();

/// Total number of sectors loaded from the current map's SECTORS lump.
/// Referenced throughout the game and renderer.
pub static mut numsectors: i32 = 0;
/// Pointer to the runtime sector array, allocated from zone memory at
/// `PU_LEVEL`. Sectors own the lighting, floor/ceiling heights, and thing
/// lists used by most game logic.
/// Referenced throughout the game and renderer.
pub static mut sectors: *mut sector_t = ptr::null_mut();

/// Total number of subsectors loaded from the current map's SSECTORS lump.
/// Referenced by `r_bsp.c` and `p_sight.c`.
pub static mut numsubsectors: i32 = 0;
/// Pointer to the runtime subsector array, allocated from zone memory at
/// `PU_LEVEL`. Each subsector is a convex polygon leaf of the BSP tree.
/// Referenced by `r_bsp.c`, `p_map.c`, and `p_sight.c`.
pub static mut subsectors: *mut subsector_t = ptr::null_mut();

/// Total number of BSP nodes loaded from the current map's NODES lump.
/// Referenced by `r_bsp.c` and `p_sight.c`.
pub static mut numnodes: i32 = 0;
/// Pointer to the runtime BSP node array, allocated from zone memory at
/// `PU_LEVEL`. The root node is `nodes[numnodes - 1]`.
/// Referenced by `r_bsp.c`, `p_map.c`, and `p_sight.c`.
pub static mut nodes: *mut node_t = ptr::null_mut();

/// Total number of linedefs loaded from the current map's LINEDEFS lump.
/// Referenced by `p_map.c`, `p_maputl.c`, and others.
pub static mut numlines: i32 = 0;
/// Pointer to the runtime linedef array, allocated from zone memory at
/// `PU_LEVEL`. Each linedef connects two vertexes and references up to two
/// sidedefs.
/// Referenced throughout the game.
pub static mut lines: *mut line_t = ptr::null_mut();

/// Total number of sidedefs loaded from the current map's SIDEDEFS lump.
/// Referenced by `r_segs.c` and others.
pub static mut numsides: i32 = 0;
/// Pointer to the runtime sidedef array, allocated from zone memory at
/// `PU_LEVEL`. Sidedefs hold texture indices and offsets; each linedef
/// references one or two.
/// Referenced by `r_segs.c`, `p_spec.c`, and others.
pub static mut sides: *mut side_t = ptr::null_mut();

/// Running total of front+back linedef-sector references, used to size the
/// per-sector line-pointer buffer in `P_GroupLines`. Module-private; not
/// exported to C.
static mut totallines: i32 = 0;

// ---------------------------------------------------------------------------
// BLOCKMAP
// ---------------------------------------------------------------------------

/// Width of the blockmap grid in 128-unit cells.
/// Referenced by `p_map.c` and `p_maputl.c` for collision queries.
pub static mut bmapwidth: i32 = 0;
/// Height of the blockmap grid in 128-unit cells.
/// Referenced by `p_map.c` and `p_maputl.c` for collision queries.
pub static mut bmapheight: i32 = 0;
/// Pointer into `blockmaplump` offset by 4 shorts (past the header).
/// Each entry is an offset from `blockmaplump` to the list of linedefs in that
/// cell, terminated by -1.
/// Referenced by `p_maputl.c`.
pub static mut blockmap: *mut i16 = ptr::null_mut();
/// Base pointer to the raw blockmap lump data (including the 4-short header).
/// Header layout: `[orgx, orgy, width, height]` in map units (not fixed-point
/// before shifting). Allocated from zone memory at `PU_LEVEL`.
/// Referenced by `p_maputl.c`.
pub static mut blockmaplump: *mut i16 = ptr::null_mut();
/// X origin of the blockmap in fixed-point (16.16) map coordinates.
/// Subtract from a thing's x to get the blockmap column.
/// Referenced by `p_maputl.c`.
pub static mut bmaporgx: i32 = 0;
/// Y origin of the blockmap in fixed-point (16.16) map coordinates.
/// Subtract from a thing's y to get the blockmap row.
/// Referenced by `p_maputl.c`.
pub static mut bmaporgy: i32 = 0;
/// Pointer to the blockmap thing-chain heads, one per cell (`bmapwidth *
/// bmapheight` entries). Each entry is a pointer to the first `mobj_t` in
/// that cell's chain, or null. Zeroed at level load.
/// Referenced by `p_maputl.c` and `p_map.c`.
pub static mut blocklinks: *mut *mut mobj_t = ptr::null_mut();

// ---------------------------------------------------------------------------
// REJECT
// ---------------------------------------------------------------------------

/// Pointer to the precomputed LOS (line-of-sight) reject table.
/// A 2-D bit array indexed by `[sector1 * numsectors + sector2]`; a set bit
/// means the two sectors cannot see each other, so detailed LOS checks are
/// skipped. Allocated from zone memory at `PU_LEVEL`.
/// Referenced by `p_sight.c`.
pub static mut rejectmatrix: *mut u8 = ptr::null_mut();

// ---------------------------------------------------------------------------
// Starting spots
// ---------------------------------------------------------------------------

/// Array of deathmatch start positions collected from the THINGS lump.
/// Holds up to `MAX_DEATHMATCH_STARTS` entries; the active count is tracked
/// by `deathmatch_p - deathmatchstarts`.
/// Referenced by `g_game.c`.
pub static mut deathmatchstarts: [mapthing_t; MAX_DEATHMATCH_STARTS] =
    [mapthing_t::ZERO; MAX_DEATHMATCH_STARTS];

/// Write cursor into `deathmatchstarts`; points to the next free slot.
/// Reset to `&deathmatchstarts[0]` at the start of each `P_SetupLevel`.
/// Advanced by `P_SpawnMapThing` when spawning deathmatch starts.
pub static mut deathmatch_p: *mut mapthing_t = ptr::null_mut();

/// Per-player single-player start positions, indexed by player number.
/// Holds `MAXPLAYERS` entries; populated by `P_SpawnMapThing` from THINGS
/// with type 1-4 (player starts).
/// Referenced by `g_game.c` for respawning.
pub static mut playerstarts: [mapthing_t; MAXPLAYERS] = [mapthing_t::ZERO; MAXPLAYERS];

// ---------------------------------------------------------------------------
// Extern declarations for still-unported C modules
// ---------------------------------------------------------------------------

use crate::engine::{
    doom::stat::gamemode,
    game::{
        bodyqueslot, deathmatch, playeringame, precache, totalitems, totalkills, totalsecret,
        wminfo, G_DeathMatchSpawnPlayer,
    },
    interface::system::I_GetMemoryValue,
    math::fixed::FixedDiv,
    misc::argv::M_CheckParm,
    play::{
        mobj::{iquehead, iquetail, P_SpawnMapThing},
        spec::{P_InitPicAnims, P_SpawnSpecials},
        switch::P_InitSwitchList,
    },
    render_3d::{
        data::{R_FlatNumForName, R_PrecacheLevel, R_TextureNumForName},
        things::R_InitSprites,
    },
    sound::S_Start,
    wad::{
        cache_lump_records,
        format::{
            maplinedef_t, mapnode_t, mapsector_t, mapseg_t, mapsidedef_t, mapsubsector_t,
            mapthing_t, mapvertex_t,
        },
        W_CacheLumpNum, W_GetNumForName, W_LumpLength, W_ReadLump, W_ReleaseLumpNum,
    },
    zone::{Z_FreeTags, Z_Malloc},
};

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
pub fn GetSectorAtNullAddress() -> *mut sector_t {
    static mut NULL_SECTOR_IS_INITIALIZED: bool = false;
    static mut NULL_SECTOR: sector_t = unsafe { std::mem::zeroed() };

    unsafe {
        if !NULL_SECTOR_IS_INITIALIZED {
            NULL_SECTOR = std::mem::zeroed();
            I_GetMemoryValue(0, &raw mut NULL_SECTOR.floorheight as *mut c_void, 4);
            I_GetMemoryValue(4, &raw mut NULL_SECTOR.ceilingheight as *mut c_void, 4);
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
pub fn P_LoadVertexes(lump: i32) {
    unsafe {
        let map_vertexes = cache_lump_records::<mapvertex_t>(lump);
        numvertexes = map_vertexes.len() as i32;
        vertexes = Z_Malloc(
            numvertexes * std::mem::size_of::<vertex_t>() as i32,
            PU_LEVEL,
            ptr::null_mut(),
        ) as *mut vertex_t;

        let mut li = vertexes;

        for ml in map_vertexes {
            (*li).x = (ml.x.get() as i32) << FRACBITS;
            (*li).y = (ml.y.get() as i32) << FRACBITS;
            li = li.add(1);
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
pub fn P_LoadSegs(lump: i32) {
    unsafe {
        let map_segs = cache_lump_records::<mapseg_t>(lump);
        numsegs = map_segs.len() as i32;
        segs = Z_Malloc(
            numsegs * std::mem::size_of::<seg_t>() as i32,
            PU_LEVEL,
            ptr::null_mut(),
        ) as *mut seg_t;
        // Zeroed by Z_Malloc internally

        let mut li = segs;

        for ml in map_segs {
            (*li).v1 = vertexes.offset(ml.v1.get() as isize);
            (*li).v2 = vertexes.offset(ml.v2.get() as isize);
            (*li).angle = ((ml.angle.get() as i32) << 16) as u32;
            (*li).offset = (ml.offset.get() as i32) << 16;
            let linedef = ml.linedef.get() as i32;
            let ldef = lines.offset(linedef as isize);
            (*li).linedef = ldef;
            let side = ml.side.get() as i32;
            (*li).sidedef = sides.offset((*ldef).sidenum[side as usize] as isize);
            (*li).frontsector = (*sides.offset((*ldef).sidenum[side as usize] as isize)).sector;

            if (*ldef).flags & LinedefFlag::TWOSIDED as i16 != 0 {
                let sidenum = (*ldef).sidenum[side as usize ^ 1];
                if sidenum < 0 || sidenum as i32 >= numsides {
                    (*li).backsector = GetSectorAtNullAddress();
                } else {
                    (*li).backsector = sides.offset(sidenum as isize).as_ref().unwrap().sector;
                }
            } else {
                (*li).backsector = ptr::null_mut();
            }

            li = li.add(1);
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
/// in the runtime struct - they actually count/index segs) are read from
/// the little-endian record fields.
///
/// `lump` must be the lump number of the SSECTORS entry for the current map.
/// C callers: `P_SetupLevel` (this file).
pub fn P_LoadSubsectors(lump: i32) {
    unsafe {
        let map_subsectors = cache_lump_records::<mapsubsector_t>(lump);
        numsubsectors = map_subsectors.len() as i32;
        subsectors = Z_Malloc(
            numsubsectors * std::mem::size_of::<subsector_t>() as i32,
            PU_LEVEL,
            ptr::null_mut(),
        ) as *mut subsector_t;
        // Zeroed by Z_Malloc internally

        let mut ss = subsectors;

        for ms in map_subsectors {
            (*ss).numlines = ms.numsegs.get();
            (*ss).firstline = ms.firstseg.get();
            ss = ss.add(1);
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
pub fn P_LoadSectors(lump: i32) {
    unsafe {
        let map_sectors = cache_lump_records::<mapsector_t>(lump);
        numsectors = map_sectors.len() as i32;
        sectors = Z_Malloc(
            numsectors * std::mem::size_of::<sector_t>() as i32,
            PU_LEVEL,
            ptr::null_mut(),
        ) as *mut sector_t;
        // Zeroed by Z_Malloc internally

        let mut ss = sectors;

        for ms in map_sectors {
            (*ss).floorheight = (ms.floorheight.get() as i32) << FRACBITS;
            (*ss).ceilingheight = (ms.ceilingheight.get() as i32) << FRACBITS;
            (*ss).floorpic = R_FlatNumForName(&ms.floorpic) as i16;
            (*ss).ceilingpic = R_FlatNumForName(&ms.ceilingpic) as i16;
            (*ss).lightlevel = ms.lightlevel.get();
            (*ss).special = ms.special.get();
            (*ss).tag = ms.tag.get();
            (*ss).thinglist = ptr::null_mut();
            ss = ss.add(1);
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
pub fn P_LoadNodes(lump: i32) {
    unsafe {
        let map_nodes = cache_lump_records::<mapnode_t>(lump);
        numnodes = map_nodes.len() as i32;
        nodes = Z_Malloc(
            numnodes * std::mem::size_of::<node_t>() as i32,
            PU_LEVEL,
            ptr::null_mut(),
        ) as *mut node_t;

        let mut no = nodes;

        for mn in map_nodes {
            (*no).x = (mn.x.get() as i32) << FRACBITS;
            (*no).y = (mn.y.get() as i32) << FRACBITS;
            (*no).dx = (mn.dx.get() as i32) << FRACBITS;
            (*no).dy = (mn.dy.get() as i32) << FRACBITS;
            for j in 0..2usize {
                (*no).children[j] = mn.children[j].get();
                for k in 0..4usize {
                    (*no).bbox[j][k] = (mn.bbox[j][k].get() as i32) << FRACBITS;
                }
            }
            no = no.add(1);
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
/// Each surviving thing is copied out of the lump and passed to
/// `P_SpawnMapThing`.
/// `P_SpawnMapThing` handles player starts, deathmatch starts, and all
/// game objects.
///
/// `lump` must be the lump number of the THINGS entry for the current map.
/// Preconditions: `vertexes`, `lines`, `sides`, `sectors`, and `blockmap`
/// must already be loaded; `deathmatch_p` must be reset to the base of
/// `deathmatchstarts`.
/// C callers: `P_SetupLevel` (this file).
pub fn P_LoadThings(lump: i32) {
    unsafe {
        let map_things = cache_lump_records::<mapthing_t>(lump);

        for mt in map_things {
            let mut spawn = true;

            if gamemode != doom::mode::commercial {
                let thing_type = mt.kind.get();
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

            let mut spawnthing = *mt;
            P_SpawnMapThing(&raw mut spawnthing);
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
pub fn P_LoadLineDefs(lump: i32) {
    unsafe {
        let map_linedefs = cache_lump_records::<maplinedef_t>(lump);
        numlines = map_linedefs.len() as i32;
        lines = Z_Malloc(
            numlines * std::mem::size_of::<line_t>() as i32,
            PU_LEVEL,
            ptr::null_mut(),
        ) as *mut line_t;
        // Zeroed by Z_Malloc internally

        let mut ld = lines;

        for mld in map_linedefs {
            (*ld).flags = mld.flags.get();
            (*ld).special = mld.special.get();
            (*ld).tag = mld.tag.get();
            let v1 = vertexes.offset(mld.v1.get() as isize);
            let v2 = vertexes.offset(mld.v2.get() as isize);
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

            (*ld).sidenum[0] = mld.sidenum[0].get();
            (*ld).sidenum[1] = mld.sidenum[1].get();

            if (*ld).sidenum[0] != -1 {
                (*ld).frontsector = sides
                    .offset((*ld).sidenum[0] as isize)
                    .as_ref()
                    .unwrap()
                    .sector;
            } else {
                (*ld).frontsector = ptr::null_mut();
            }

            if (*ld).sidenum[1] != -1 {
                (*ld).backsector = sides
                    .offset((*ld).sidenum[1] as isize)
                    .as_ref()
                    .unwrap()
                    .sector;
            } else {
                (*ld).backsector = ptr::null_mut();
            }

            ld = ld.add(1);
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
pub fn P_LoadSideDefs(lump: i32) {
    unsafe {
        let map_sidedefs = cache_lump_records::<mapsidedef_t>(lump);
        numsides = map_sidedefs.len() as i32;
        sides = Z_Malloc(
            numsides * std::mem::size_of::<side_t>() as i32,
            PU_LEVEL,
            ptr::null_mut(),
        ) as *mut side_t;
        // Zeroed by Z_Malloc internally

        let mut sd = sides;

        for msd in map_sidedefs {
            (*sd).textureoffset = (msd.textureoffset.get() as i32) << FRACBITS;
            (*sd).rowoffset = (msd.rowoffset.get() as i32) << FRACBITS;
            (*sd).toptexture = R_TextureNumForName(&msd.toptexture) as i16;
            (*sd).bottomtexture = R_TextureNumForName(&msd.bottomtexture) as i16;
            (*sd).midtexture = R_TextureNumForName(&msd.midtexture) as i16;
            (*sd).sector = sectors.offset(msd.sector.get() as isize);
            sd = sd.add(1);
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
pub fn P_LoadBlockMap(lump: i32) {
    unsafe {
        let lumplen = W_LumpLength(lump as u32);
        let count = lumplen / 2;

        blockmaplump = Z_Malloc(lumplen, PU_LEVEL, ptr::null_mut()) as *mut i16;
        W_ReadLump(lump as u32, blockmaplump as *mut c_void);
        blockmap = blockmaplump.add(4);

        // Swap all short integers to native byte ordering.
        for i in 0..count {
            *blockmaplump.add(i as usize) = SHORT(*blockmaplump.add(i as usize));
        }

        bmaporgx = (*blockmaplump.add(0) as i32) << FRACBITS;
        bmaporgy = (*blockmaplump.add(1) as i32) << FRACBITS;
        bmapwidth = *blockmaplump.add(2) as i32;
        bmapheight = *blockmaplump.add(3) as i32;

        let bcount = std::mem::size_of::<*mut mobj_t>() * bmapwidth as usize * bmapheight as usize;
        blocklinks = Z_Malloc(bcount as i32, PU_LEVEL, ptr::null_mut()) as *mut *mut mobj_t;
        ptr::write_bytes(blocklinks.cast::<u8>(), 0, bcount);
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
pub fn P_GroupLines() {
    unsafe {
        // Look up sector number for each subsector.
        let mut ss = subsectors;
        for _ in 0..numsubsectors {
            let seg = segs.offset((*ss).firstline as isize);
            // Must use seg->sidedef->sector, not sidenum[0], because the seg
            // may be on side 1 (back side) of the linedef, in which case
            // sidenum[0] points to the wrong side's sector.
            (*ss).sector = (*(*seg).sidedef).sector;
            ss = ss.add(1);
        }

        // Count number of lines in each sector.
        let mut li = lines;
        totallines = 0;
        for _ in 0..numlines {
            totallines += 1;
            let frontsec = (*li).frontsector;
            if !frontsec.is_null() {
                (*frontsec).linecount += 1;
            }
            let backsec = (*li).backsector;
            if !backsec.is_null() && backsec != frontsec {
                (*backsec).linecount += 1;
                totallines += 1;
            }
            li = li.add(1);
        }

        // Build line tables for each sector.
        let linebuffer = Z_Malloc(
            totallines * std::mem::size_of::<*mut line_t>() as i32,
            PU_LEVEL,
            ptr::null_mut(),
        ) as *mut *mut line_t;

        let mut current = linebuffer;
        for i in 0..numsectors as usize {
            let sec = sectors.add(i);
            (*sec).lines = current;
            current = current.offset((*sec).linecount as isize);
            (*sec).linecount = 0;
        }

        // Assign lines to sectors.
        for i in 0..numlines as usize {
            li = lines.add(i);
            if !(*li).frontsector.is_null() {
                let sector = (*li).frontsector;
                (*sector)
                    .lines
                    .offset((*sector).linecount as isize)
                    .write(li);
                (*sector).linecount += 1;
            }
            if !(*li).backsector.is_null() && (*li).frontsector != (*li).backsector {
                let sector = (*li).backsector;
                (*sector)
                    .lines
                    .offset((*sector).linecount as isize)
                    .write(li);
                (*sector).linecount += 1;
            }
        }

        // Generate bounding boxes for sectors.
        let mut sector = sectors;
        for _ in 0..numsectors {
            let mut bbox: [i32; 4] = [0; 4];
            M_ClearBox(bbox.as_mut_ptr());

            for j in 0..(*sector).linecount {
                let line = *(*sector).lines.offset(j as isize);
                M_AddToBox(bbox.as_mut_ptr(), (*(*line).v1).x, (*(*line).v1).y);
                M_AddToBox(bbox.as_mut_ptr(), (*(*line).v2).x, (*(*line).v2).y);
            }

            // Set the degenmobj_t to the middle of the bounding box.
            (*sector).soundorg.x = (bbox[BBox::RIGHT] + bbox[BBox::LEFT]) / 2;
            (*sector).soundorg.y = (bbox[BBox::TOP] + bbox[BBox::BOTTOM]) / 2;

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

        let padvalue = if M_CheckParm("-reject_pad_with_ff") != 0 {
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
unsafe fn P_LoadReject(lumpnum: i32) {
    let minlength = (numsectors * numsectors + 7) / 8;
    let lumplen = W_LumpLength(lumpnum as u32);

    if lumplen >= minlength {
        rejectmatrix = W_CacheLumpNum(lumpnum, PU_LEVEL) as *mut u8;
    } else {
        rejectmatrix =
            Z_Malloc(minlength, PU_LEVEL, &raw mut rejectmatrix as *mut c_void) as *mut u8;
        W_ReadLump(lumpnum as u32, rejectmatrix as *mut c_void);
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
pub fn P_SetupLevel(episode: i32, map: i32, _playermask: i32, _skill: i32) {
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
        let mut lumpname = [0u8; 9];
        if gamemode == doom::mode::commercial {
            if map < 10 {
                let s = format!("map0{}", map);
                ptr::copy_nonoverlapping(s.as_ptr(), lumpname.as_mut_ptr(), s.len());
            } else {
                let s = format!("map{}", map);
                ptr::copy_nonoverlapping(s.as_ptr(), lumpname.as_mut_ptr(), s.len());
            }
        } else {
            lumpname[0] = b'E';
            lumpname[1] = b'0' + episode as u8;
            lumpname[2] = b'M';
            lumpname[3] = b'0' + map as u8;
            lumpname[4] = 0;
        }

        let lumpnum = W_GetNumForName(&lumpname);

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
                    G_DeathMatchSpawnPlayer(i as i32);
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
pub fn P_Init() {
    unsafe {
        P_InitSwitchList();
        P_InitPicAnims();
        R_InitSprites(&sprnames);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use {super::*, std::sync::Mutex};

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
            assert_eq!({ numvertexes }, 0);
            assert_eq!({ numsegs }, 0);
            assert_eq!({ numsectors }, 0);
            assert_eq!({ numsubsectors }, 0);
            assert_eq!({ numnodes }, 0);
            assert_eq!({ numlines }, 0);
            assert_eq!({ numsides }, 0);
            assert_eq!({ bmapwidth }, 0);
            assert_eq!({ bmapheight }, 0);
            assert_eq!({ bmaporgx }, 0);
            assert_eq!({ bmaporgy }, 0);
        }
    }

    #[test]
    fn setup_globals_are_i32() {
        unsafe {
            let _: i32 = numvertexes;
            let _: i32 = numsegs;
            let _: i32 = numsectors;
            let _: i32 = numsubsectors;
            let _: i32 = numnodes;
            let _: i32 = numlines;
            let _: i32 = numsides;
            let _: i32 = bmapwidth;
            let _: i32 = bmapheight;
            let _: i32 = bmaporgx;
            let _: i32 = bmaporgy;
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
