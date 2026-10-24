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
//!   little-endian records in `wad::format`, and the raw BLOCKMAP shorts as
//!   little-endian `I16`s, instead of byte-swapping with the C `SHORT` macro
//!   from `i_swap.h`.
//! - `P_LoadThings` increments the `mt` pointer only after the spawn decision
//!   so that a `break` on a non-commercial monster does not advance past it.
//! - `P_SetupLevel` uses `format!` for lump-name construction instead of
//!   `DEH_snprintf`, so DeHackEd lump-name patches are not applied.
//! - The C map-table globals (`vertexes`, `lines`, etc.) are arrays owned by
//!   the [`LevelState`] held in [`LEVEL`], built from the lumps at level
//!   load instead of `Z_Malloc(PU_LEVEL)`; records refer to each other by
//!   typed id rather than by pointer. The start spots live in
//!   [`SPAWN_SPOTS`] and are reached by pointer.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::{ffi::c_void, ptr};

use {
    crate::engine::{
        doom::{
            self,
            player::{MAXPLAYERS, PlayerId},
        },
        global::Global,
        map::{
            LineId, SectorId, SideId, SubsectorId, VertexId, degenmobj_t, line_t, node_t, sector_t,
            seg_t, side_t,
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
            mobj::MobjId,
            tick::{P_InitThinkers, TICK},
        },
        wad::format::{LinedefFlag, MapLump},
        zone::PU_LEVEL,
    },
    zerocopy::little_endian::I16,
};

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

/// The loaded level's geometry: the map tables, the blockmap and the reject
/// table (`p_setup.c` globals).
///
/// `P_SetupLevel` builds every table from the map lumps and replaces the
/// previous level's, which are dropped. Records refer to each other by typed
/// id (see [`crate::engine::map`]) and each table is indexed by its id type.
///
/// Hot paths (renderer, sight checks) borrow [`LEVEL`] once per call or
/// traversal and index the tables. Code that calls back into play code
/// (blockmap iterators, specials, thing movement) takes short borrows that
/// never span such a call, because the callee may borrow the level mutably.
pub struct LevelState {
    /// Runtime vertexes from the map's VERTEXES lump, in fixed-point (16.16)
    /// x/y coordinates.
    pub vertexes: Vec<vertex_t>,

    /// Runtime segs from the map's SEGS lump. Referenced by `r_bsp.c` and
    /// `p_sight.c`.
    pub segs: Vec<seg_t>,

    /// Number of sectors in the map's SECTORS lump: the leading part of
    /// `sectors`. `sectors` holds one more entry, the glass-hack sector of
    /// [`GetSectorAtNullAddress`], when a seg needed it.
    pub numsectors: i32,
    /// Runtime sectors. Sectors own the lighting, floor/ceiling heights, and
    /// thing lists used by most game logic.
    pub sectors: Vec<sector_t>,

    /// Runtime subsectors from the map's SSECTORS lump. Each subsector is a
    /// convex polygon leaf of the BSP tree.
    pub subsectors: Vec<subsector_t>,

    /// Runtime BSP nodes from the map's NODES lump. The root node is the
    /// last one.
    pub nodes: Vec<node_t>,

    /// Runtime linedefs from the map's LINEDEFS lump. Each linedef connects
    /// two vertexes and references up to two sidedefs.
    pub lines: Vec<line_t>,

    /// Runtime sidedefs from the map's SIDEDEFS lump. Sidedefs hold texture
    /// indices and offsets; each linedef references one or two.
    pub sides: Vec<side_t>,

    /// The linedefs bounding each sector, built by `P_GroupLines`: sector
    /// `s` owns the `s.linecount` entries starting at `s.lines`.
    pub linebuffer: Vec<LineId>,

    /// Width of the blockmap grid in 128-unit cells.
    pub bmapwidth: i32,
    /// Height of the blockmap grid in 128-unit cells.
    pub bmapheight: i32,
    /// The blockmap lump in native byte order. Header layout: `[orgx, orgy,
    /// width, height]` in map units (not fixed-point before shifting), then
    /// one entry per cell (C's `blockmap`, at offset 4): the offset in this
    /// lump of the cell's linedef list, terminated by -1.
    pub blockmaplump: Vec<i16>,
    /// X origin of the blockmap in fixed-point (16.16) map coordinates.
    /// Subtract from a thing's x to get the blockmap column.
    pub bmaporgx: i32,
    /// Y origin of the blockmap in fixed-point (16.16) map coordinates.
    /// Subtract from a thing's y to get the blockmap row.
    pub bmaporgy: i32,
    /// The blockmap thing-chain heads, one per cell (`bmapwidth *
    /// bmapheight` entries). Each entry is the first map object in that
    /// cell's chain, or `None`. Emptied at level load.
    pub blocklinks: Vec<Option<MobjId>>,

    /// The precomputed LOS (line-of-sight) reject table.
    /// A 2-D bit array indexed by `[sector1 * numsectors + sector2]`; a set bit
    /// means the two sectors cannot see each other, so detailed LOS checks are
    /// skipped.
    pub rejectmatrix: Vec<u8>,
}

impl LevelState {
    /// No level loaded: every table empty.
    pub const fn new() -> Self {
        Self {
            vertexes: Vec::new(),
            segs: Vec::new(),
            numsectors: 0,
            sectors: Vec::new(),
            subsectors: Vec::new(),
            nodes: Vec::new(),
            lines: Vec::new(),
            sides: Vec::new(),
            linebuffer: Vec::new(),
            bmapwidth: 0,
            bmapheight: 0,
            blockmaplump: Vec::new(),
            bmaporgx: 0,
            bmaporgy: 0,
            blocklinks: Vec::new(),
            rejectmatrix: Vec::new(),
        }
    }

    /// The linedefs bounding `sector`.
    #[inline]
    pub fn sector_lines(&self, sector: SectorId) -> &[LineId] {
        self.sectors[sector].lines_in(&self.linebuffer)
    }

    /// The sector `subsector` belongs to (C `subsector->sector`).
    #[inline]
    pub fn subsector_sector(&self, subsector: SubsectorId) -> &sector_t {
        &self.sectors[self.subsectors[subsector].sector]
    }
}

impl Default for LevelState {
    fn default() -> Self {
        Self::new()
    }
}

/// The loaded level's geometry tables.
pub static LEVEL: Global<LevelState> = Global::new(LevelState::new());

/// Serializes the unit tests that install a level in [`LEVEL`].
#[cfg(test)]
pub(crate) static LEVEL_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The blockmap origin `(bmaporgx, bmaporgy)` in fixed-point map units.
pub fn blockmap_origin() -> (i32, i32) {
    let level = LEVEL.borrow();
    (level.bmaporgx, level.bmaporgy)
}

/// Map-loading scratch state private to this module.
struct SetupState {
    /// Running total of front+back linedef-sector references, used to size the
    /// per-sector line buffer in `P_GroupLines`.
    totallines: i32,
    /// Whether [`GetSectorAtNullAddress`] has filled in `null_sector`.
    null_sector_is_initialized: bool,
    /// The synthetic "sector at address 0" handed out by
    /// [`GetSectorAtNullAddress`]; each level that needs it gets a copy.
    null_sector: sector_t,
}

/// Map-loading scratch state.
static SETUP: Global<SetupState> = Global::new(SetupState {
    totallines: 0,
    null_sector_is_initialized: false,
    null_sector: sector_t::ZERO,
});

// ---------------------------------------------------------------------------
// Starting spots
// ---------------------------------------------------------------------------

/// Player and deathmatch start spots collected from the THINGS lump.
///
/// Spawning code keeps pointers into both arrays (`P_SpawnPlayer` takes a
/// start spot, `deathmatch_p` is a cursor into `deathmatchstarts`), so the
/// spots are reached through [`SPAWN_SPOTS`]'s stable [`Global::as_ptr`]
/// address via [`playerstart`], [`deathmatchstart`] and [`deathmatch_p`].
pub struct SpawnSpots {
    /// Array of deathmatch start positions collected from the THINGS lump.
    /// Holds up to `MAX_DEATHMATCH_STARTS` entries; the active count is tracked
    /// by `deathmatch_p - deathmatchstarts`.
    /// Referenced by `g_game.c`.
    pub deathmatchstarts: [mapthing_t; MAX_DEATHMATCH_STARTS],
    /// Write cursor into `deathmatchstarts`; points to the next free slot.
    /// Reset to `&deathmatchstarts[0]` at the start of each `P_SetupLevel`.
    /// Advanced by `P_SpawnMapThing` when spawning deathmatch starts.
    pub deathmatch_p: *mut mapthing_t,
    /// Per-player single-player start positions, indexed by player number.
    /// Holds `MAXPLAYERS` entries; populated by `P_SpawnMapThing` from THINGS
    /// with type 1-4 (player starts).
    /// Referenced by `g_game.c` for respawning.
    pub playerstarts: [mapthing_t; MAXPLAYERS],
}

/// Player and deathmatch start spots.
pub static SPAWN_SPOTS: Global<SpawnSpots> = Global::new(SpawnSpots {
    deathmatchstarts: [mapthing_t::ZERO; MAX_DEATHMATCH_STARTS],
    deathmatch_p: ptr::null_mut(),
    playerstarts: [mapthing_t::ZERO; MAXPLAYERS],
});

/// Stable pointer to `playerstarts[player]` (C `&playerstarts[player]`).
pub fn playerstart(player: usize) -> *mut mapthing_t {
    assert!(player < MAXPLAYERS);
    // SAFETY: in-bounds projection on the stable state pointer.
    unsafe {
        (&raw mut (*SPAWN_SPOTS.as_ptr()).playerstarts)
            .cast::<mapthing_t>()
            .add(player)
    }
}

/// Stable pointer to `deathmatchstarts[index]` (C `&deathmatchstarts[index]`).
pub fn deathmatchstart(index: usize) -> *mut mapthing_t {
    assert!(index <= MAX_DEATHMATCH_STARTS);
    // SAFETY: projection on the stable state pointer; one past the end is
    // allowed, as the C cursor reaches it.
    unsafe {
        (&raw mut (*SPAWN_SPOTS.as_ptr()).deathmatchstarts)
            .cast::<mapthing_t>()
            .add(index)
    }
}

/// The deathmatch start cursor (C `deathmatch_p`).
pub fn deathmatch_p() -> *mut mapthing_t {
    SPAWN_SPOTS.borrow().deathmatch_p
}

// ---------------------------------------------------------------------------
// Extern declarations for still-unported C modules
// ---------------------------------------------------------------------------

use crate::engine::{
    doom::stat::gamemode,
    game::{G_DeathMatchSpawnPlayer, GAME, deathmatch, playeringame},
    interface::system::I_GetMemoryValue,
    math::fixed::FixedDiv,
    misc::argv::M_CheckParm,
    play::{
        mobj::{MOBJ, P_SpawnMapThing},
        spec::{P_InitPicAnims, P_SpawnSpecials},
        switch::P_InitSwitchList,
    },
    render_3d::{
        data::{R_FlatNumForName, R_PrecacheLevel, R_TextureNumForName},
        things::R_InitSprites,
    },
    sound::S_Start,
    wad::{
        W_GetNumForName, W_ReleaseLumpNum, cache_lump_bytes, cache_lump_records,
        format::{
            maplinedef_t, mapnode_t, mapsector_t, mapseg_t, mapsidedef_t, mapsubsector_t,
            mapthing_t, mapvertex_t,
        },
    },
    world::World,
    zone::{PU_STATIC, Z_FreeTags},
};

// ---------------------------------------------------------------------------
// GetSectorAtNullAddress
// ---------------------------------------------------------------------------

/// Return the id of a synthetic `sector_t` that represents the sector at
/// address 0, used to handle malformed WAD data (the "glass hack").
///
/// The sector is initialized on first call by reading 4 bytes each from
/// address 0 and 4 via `I_GetMemoryValue`, matching vanilla Doom's behavior
/// of reading the initial heap block header at address 0. Subsequent calls
/// reuse the already-initialized sector without re-reading. C hands out the
/// address of one static sector; here a copy of it follows the map's own
/// sectors in `level.sectors` (index `numsectors`), appended on the level's
/// first call.
///
/// C callers: `P_LoadSegs` (this file).
fn GetSectorAtNullAddress(level: &mut LevelState) -> SectorId {
    let null_sector = {
        let mut setup = SETUP.borrow_mut();
        if !setup.null_sector_is_initialized {
            setup.null_sector = sector_t::ZERO;
            // SAFETY: each read fills one 4-byte height field.
            unsafe {
                I_GetMemoryValue(0, &raw mut setup.null_sector.floorheight as *mut c_void, 4);
                I_GetMemoryValue(
                    4,
                    &raw mut setup.null_sector.ceilingheight as *mut c_void,
                    4,
                );
            }
            setup.null_sector_is_initialized = true;
        }
        setup.null_sector
    };
    let index = level.numsectors as usize;
    if level.sectors.len() == index {
        level.sectors.push(null_sector);
    }
    SectorId::new(index)
}

// ---------------------------------------------------------------------------
// P_LoadVertexes
// ---------------------------------------------------------------------------

/// Load the VERTEXES lump and build the level's `vertexes` array.
///
/// Reads each on-disk `mapvertex_t` (2 × i16 little-endian) and stores the
/// result as fixed-point (16.16) coordinates by shifting left by `FRACBITS`.
/// The raw lump is released after conversion.
///
/// `lump` must be the lump number of the VERTEXES entry for the current map
/// (typically `lumpnum + MapLump::VERTEXES`).
/// C callers: `P_SetupLevel` (this file).
pub fn P_LoadVertexes(lump: i32) {
    // SAFETY: the lump view is used only before the lump is released.
    let map_vertexes = unsafe { cache_lump_records::<mapvertex_t>(lump) };
    let vertexes = map_vertexes
        .iter()
        .map(|ml| vertex_t {
            x: (ml.x.get() as i32) << FRACBITS,
            y: (ml.y.get() as i32) << FRACBITS,
        })
        .collect();
    W_ReleaseLumpNum(lump);
    LEVEL.borrow_mut().vertexes = vertexes;
}

// ---------------------------------------------------------------------------
// P_LoadSegs
// ---------------------------------------------------------------------------

/// Load the SEGS lump and build the level's `segs` array.
///
/// Fills each entry from the corresponding on-disk `mapseg_t`. Key
/// conversions:
/// - Vertex indices become ids into `vertexes`.
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
    // SAFETY: the lump view is used only before the lump is released.
    let map_segs = unsafe { cache_lump_records::<mapseg_t>(lump) };
    let mut level = LEVEL.borrow_mut();
    let mut segs = Vec::with_capacity(map_segs.len());

    for ml in map_segs {
        let ldef = level.lines[ml.linedef.get() as usize];
        let side = ml.side.get() as usize;
        let sidedef = SideId::new(ldef.sidenum[side] as usize);

        let backsector = if ldef.flags & LinedefFlag::TWOSIDED as i16 != 0 {
            let sidenum = ldef.sidenum[side ^ 1];
            if sidenum < 0 || sidenum as usize >= level.sides.len() {
                Some(GetSectorAtNullAddress(&mut level))
            } else {
                Some(level.sides[sidenum as usize].sector)
            }
        } else {
            None
        };

        segs.push(seg_t {
            v1: VertexId::new(ml.v1.get() as usize),
            v2: VertexId::new(ml.v2.get() as usize),
            offset: (ml.offset.get() as i32) << 16,
            angle: ((ml.angle.get() as i32) << 16) as u32,
            sidedef,
            linedef: LineId::new(ml.linedef.get() as usize),
            frontsector: level.sides[sidedef].sector,
            backsector,
        });
    }

    level.segs = segs;
    drop(level);
    W_ReleaseLumpNum(lump);
}

// ---------------------------------------------------------------------------
// P_LoadSubsectors
// ---------------------------------------------------------------------------

/// Load the SSECTORS lump and build the level's `subsectors` array.
///
/// Fills each entry from the corresponding on-disk `mapsubsector_t`. The
/// `numlines` and `firstline` fields (confusingly named in the runtime
/// struct - they actually count/index segs) are read from the little-endian
/// record fields. The `sector` field is set by `P_GroupLines` once the segs
/// are loaded; until then it is sector 0.
///
/// `lump` must be the lump number of the SSECTORS entry for the current map.
/// C callers: `P_SetupLevel` (this file).
pub fn P_LoadSubsectors(lump: i32) {
    // SAFETY: the lump view is used only before the lump is released.
    let map_subsectors = unsafe { cache_lump_records::<mapsubsector_t>(lump) };
    let subsectors = map_subsectors
        .iter()
        .map(|ms| subsector_t {
            sector: SectorId::new(0),
            numlines: ms.numsegs.get(),
            firstline: ms.firstseg.get(),
        })
        .collect();
    W_ReleaseLumpNum(lump);
    LEVEL.borrow_mut().subsectors = subsectors;
}

// ---------------------------------------------------------------------------
// P_LoadSectors
// ---------------------------------------------------------------------------

/// Load the SECTORS lump and build the level's `sectors` array.
///
/// Fills each entry from the corresponding on-disk `mapsector_t`; the
/// remaining fields start zeroed (null thing list, no special thinker).
/// Floor and ceiling heights are shifted left by `FRACBITS` to become
/// fixed-point. Flat names are resolved to runtime indices via
/// `R_FlatNumForName`.
///
/// `lump` must be the lump number of the SECTORS entry for the current map.
/// C callers: `P_SetupLevel` (this file).
pub fn P_LoadSectors(lump: i32) {
    // SAFETY: the lump view is used only before the lump is released.
    let map_sectors = unsafe { cache_lump_records::<mapsector_t>(lump) };
    // SAFETY: `R_Init` has set up the flats at startup, before any level.
    let flat_num = |name: &[u8]| unsafe { R_FlatNumForName(name) } as i16;
    let sectors: Vec<sector_t> = map_sectors
        .iter()
        .map(|ms| sector_t {
            floorheight: (ms.floorheight.get() as i32) << FRACBITS,
            ceilingheight: (ms.ceilingheight.get() as i32) << FRACBITS,
            floorpic: flat_num(&ms.floorpic),
            ceilingpic: flat_num(&ms.ceilingpic),
            lightlevel: ms.lightlevel.get(),
            special: ms.special.get(),
            tag: ms.tag.get(),
            soundtraversed: 0,
            soundtarget: None,
            blockbox: [0; 4],
            soundorg: degenmobj_t { x: 0, y: 0, z: 0 },
            validcount: 0,
            thinglist: None,
            specialdata: None,
            linecount: 0,
            lines: 0,
        })
        .collect();
    W_ReleaseLumpNum(lump);
    let mut level = LEVEL.borrow_mut();
    level.numsectors = sectors.len() as i32;
    level.sectors = sectors;
}

// ---------------------------------------------------------------------------
// P_LoadNodes
// ---------------------------------------------------------------------------

/// Load the NODES lump and build the level's `nodes` array.
///
/// Fills each entry from the corresponding on-disk `mapnode_t`.
/// Partition-line origin (`x`, `y`) and direction (`dx`, `dy`) are shifted
/// left by `FRACBITS` to become fixed-point. Bounding-box values are
/// similarly shifted. Child indices are stored as `u16`; the high bit
/// (0x8000) indicates a subsector leaf rather than an internal node.
///
/// `lump` must be the lump number of the NODES entry for the current map.
/// C callers: `P_SetupLevel` (this file).
pub fn P_LoadNodes(lump: i32) {
    // SAFETY: the lump view is used only before the lump is released.
    let map_nodes = unsafe { cache_lump_records::<mapnode_t>(lump) };
    let nodes = map_nodes
        .iter()
        .map(|mn| node_t {
            x: (mn.x.get() as i32) << FRACBITS,
            y: (mn.y.get() as i32) << FRACBITS,
            dx: (mn.dx.get() as i32) << FRACBITS,
            dy: (mn.dy.get() as i32) << FRACBITS,
            bbox: mn
                .bbox
                .map(|child_box| child_box.map(|coordinate| (coordinate.get() as i32) << FRACBITS)),
            children: mn.children.map(|child| child.get()),
        })
        .collect();
    W_ReleaseLumpNum(lump);
    LEVEL.borrow_mut().nodes = nodes;
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
pub fn P_LoadThings(world: &mut World, lump: i32) {
    unsafe {
        let map_things = cache_lump_records::<mapthing_t>(lump);

        for mt in map_things {
            let mut spawn = true;

            if gamemode() != doom::mode::commercial {
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
            P_SpawnMapThing(world, &raw mut spawnthing);
        }

        W_ReleaseLumpNum(lump);
    }
}

// ---------------------------------------------------------------------------
// P_LoadLineDefs
// ---------------------------------------------------------------------------

/// Load the LINEDEFS lump and build the level's `lines` array.
///
/// Fills each entry from the corresponding on-disk `maplinedef_t`. Notable
/// steps per linedef:
/// - Vertex ids are taken from the record's indices into `vertexes`.
/// - `dx` and `dy` are computed from the vertex coordinates (fixed-point).
/// - `slopetype` is set to `ST_VERTICAL`, `ST_HORIZONTAL`, `ST_POSITIVE`, or
///   `ST_NEGATIVE` based on the sign of `FixedDiv(dy, dx)`.
/// - The linedef's axis-aligned bounding box (`bbox[4]`) is computed from the
///   two vertex coordinates.
/// - Front and back sectors are resolved from `sidenum` indices; -1 means no
///   side (one-sided line), leaving that sector `None`.
///
/// `lump` must be the lump number of the LINEDEFS entry for the current map.
/// Preconditions: `vertexes` and `sides` must already be loaded.
/// C callers: `P_SetupLevel` (this file).
pub fn P_LoadLineDefs(lump: i32) {
    // SAFETY: the lump view is used only before the lump is released.
    let map_linedefs = unsafe { cache_lump_records::<maplinedef_t>(lump) };
    let mut level = LEVEL.borrow_mut();
    let mut lines = Vec::with_capacity(map_linedefs.len());

    for mld in map_linedefs {
        let v1 = VertexId::new(mld.v1.get() as usize);
        let v2 = VertexId::new(mld.v2.get() as usize);
        let (start, end) = (level.vertexes[v1], level.vertexes[v2]);
        let dx = end.x - start.x;
        let dy = end.y - start.y;

        let slopetype = if dx == 0 {
            ST_VERTICAL
        } else if dy == 0 {
            ST_HORIZONTAL
        } else if FixedDiv(dy, dx) > 0 {
            ST_POSITIVE
        } else {
            ST_NEGATIVE
        };

        let mut bbox = [0; 4];
        if start.x < end.x {
            bbox[BBox::LEFT] = start.x;
            bbox[BBox::RIGHT] = end.x;
        } else {
            bbox[BBox::LEFT] = end.x;
            bbox[BBox::RIGHT] = start.x;
        }
        if start.y < end.y {
            bbox[BBox::BOTTOM] = start.y;
            bbox[BBox::TOP] = end.y;
        } else {
            bbox[BBox::BOTTOM] = end.y;
            bbox[BBox::TOP] = start.y;
        }

        let sidenum = [mld.sidenum[0].get(), mld.sidenum[1].get()];
        let side_sector = |side: i16| (side != -1).then(|| level.sides[side as usize].sector);

        lines.push(line_t {
            v1,
            v2,
            dx,
            dy,
            flags: mld.flags.get(),
            special: mld.special.get(),
            tag: mld.tag.get(),
            sidenum,
            bbox,
            slopetype,
            frontsector: side_sector(sidenum[0]),
            backsector: side_sector(sidenum[1]),
            validcount: 0,
            specialdata: None,
        });
    }

    level.lines = lines;
    drop(level);
    W_ReleaseLumpNum(lump);
}

// ---------------------------------------------------------------------------
// P_LoadSideDefs
// ---------------------------------------------------------------------------

/// Load the SIDEDEFS lump and build the level's `sides` array.
///
/// Fills each entry from the corresponding on-disk `mapsidedef_t`. Texture
/// offsets are shifted left by `FRACBITS` to become fixed-point. Texture
/// names (8-byte null-padded ASCII) are resolved to runtime indices via
/// `R_TextureNumForName`. The `sector` id is the on-disk sector index.
///
/// `lump` must be the lump number of the SIDEDEFS entry for the current map.
/// Precondition: `sectors` must already be loaded.
/// C callers: `P_SetupLevel` (this file).
pub fn P_LoadSideDefs(lump: i32) {
    // SAFETY: the lump view is used only before the lump is released.
    let map_sidedefs = unsafe { cache_lump_records::<mapsidedef_t>(lump) };
    // SAFETY: `R_Init` has set up the textures at startup, before any level.
    let texture_num = |name: &[u8]| unsafe { R_TextureNumForName(name) } as i16;
    let sides = map_sidedefs
        .iter()
        .map(|msd| side_t {
            textureoffset: (msd.textureoffset.get() as i32) << FRACBITS,
            rowoffset: (msd.rowoffset.get() as i32) << FRACBITS,
            toptexture: texture_num(&msd.toptexture),
            bottomtexture: texture_num(&msd.bottomtexture),
            midtexture: texture_num(&msd.midtexture),
            sector: SectorId::new(msd.sector.get() as usize),
        })
        .collect();
    W_ReleaseLumpNum(lump);
    LEVEL.borrow_mut().sides = sides;
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
/// 1. Read all `lumplen / 2` shorts of the lump, converting them from
///    little-endian to native byte order, into `blockmaplump`. The per-cell
///    offsets (C's `blockmap`) follow the 4-short header.
/// 2. Extract the header: `bmaporgx`, `bmaporgy` (shifted by `FRACBITS`),
///    `bmapwidth`, `bmapheight`.
/// 3. Build the null `blocklinks` thing-chain heads (`bmapwidth *
///    bmapheight` entries).
///
/// `lump` must be the lump number of the BLOCKMAP entry for the current map.
/// C callers: `P_SetupLevel` (this file).
pub fn P_LoadBlockMap(lump: i32) {
    // SAFETY: the lump view is used only before the lump is released.
    let raw_blockmap = unsafe { cache_lump_records::<I16>(lump) };
    let blockmaplump: Vec<i16> = raw_blockmap.iter().map(|short| short.get()).collect();
    W_ReleaseLumpNum(lump);

    let bmapwidth = blockmaplump[2] as i32;
    let bmapheight = blockmaplump[3] as i32;

    let mut level = LEVEL.borrow_mut();
    level.bmaporgx = (blockmaplump[0] as i32) << FRACBITS;
    level.bmaporgy = (blockmaplump[1] as i32) << FRACBITS;
    level.bmapwidth = bmapwidth;
    level.bmapheight = bmapheight;
    level.blocklinks = vec![None; bmapwidth as usize * bmapheight as usize];
    level.blockmaplump = blockmaplump;
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
/// 1. **Subsector sectors**: for each subsector, resolves its sector by
///    following `segs[ss.firstline].sidedef.sector`. This uses the seg's
///    already-resolved sidedef (set during `P_LoadSegs`) rather than the raw
///    `sidenum[0]`, correctly handling back-side segs.
///
/// 2. **Line count and buffer allocation**: counts how many line references
///    each sector accumulates (`totallines` tracks the total for the shared
///    buffer). Two-sided lines contribute to both front and back sectors, but
///    a line shared between the same front and back sector is counted only once
///    per side.
///
/// 3. **Line table assignment**: builds a single flat buffer of `totallines`
///    line ids, then assigns windows of it to each sector using a cumulative
///    cursor (not per-sector independent offsets). A second pass over lines
///    fills in the ids.
///
/// 4. **Bounding boxes and sound origins**: for each sector, computes an
///    axis-aligned bounding box from its lines' vertices, sets the sound
///    origin (`soundorg`) to the bounding-box center, and records the
///    clamped blockmap cell extents in `sector.blockbox`.
///
/// Preconditions: all lump-load functions must have been called first.
/// C callers: `P_SetupLevel` (this file).
pub fn P_GroupLines() {
    let mut level = LEVEL.borrow_mut();
    let level = &mut *level;

    // Look up sector number for each subsector.
    for subsector in level.subsectors.iter_mut() {
        let seg = &level.segs[subsector.firstline as usize];
        // Must use seg->sidedef->sector, not sidenum[0], because the seg
        // may be on side 1 (back side) of the linedef, in which case
        // sidenum[0] points to the wrong side's sector.
        subsector.sector = level.sides[seg.sidedef].sector;
    }

    // Count number of lines in each sector.
    let mut totallines = 0;
    for line in &level.lines {
        totallines += 1;
        if let Some(frontsector) = line.frontsector {
            level.sectors[frontsector].linecount += 1;
        }
        if let Some(backsector) = line.backsector
            && line.backsector != line.frontsector
        {
            level.sectors[backsector].linecount += 1;
            totallines += 1;
        }
    }
    SETUP.borrow_mut().totallines = totallines;

    // Build line tables for each sector.
    let mut current = 0;
    for sector in &mut level.sectors[..level.numsectors as usize] {
        sector.lines = current;
        current += sector.linecount as usize;
        sector.linecount = 0;
    }

    // Assign lines to sectors.
    let mut linebuffer = vec![LineId::new(0); totallines as usize];
    for (index, line) in level.lines.iter().enumerate() {
        if let Some(frontsector) = line.frontsector {
            let sector = &mut level.sectors[frontsector];
            linebuffer[sector.lines + sector.linecount as usize] = LineId::new(index);
            sector.linecount += 1;
        }
        if let Some(backsector) = line.backsector
            && line.frontsector != line.backsector
        {
            let sector = &mut level.sectors[backsector];
            linebuffer[sector.lines + sector.linecount as usize] = LineId::new(index);
            sector.linecount += 1;
        }
    }
    level.linebuffer = linebuffer;

    // Generate bounding boxes for sectors.
    let (bmaporgx, bmaporgy, bmapwidth, bmapheight) = (
        level.bmaporgx,
        level.bmaporgy,
        level.bmapwidth,
        level.bmapheight,
    );
    for sector in &mut level.sectors[..level.numsectors as usize] {
        let mut bbox: [i32; 4] = [0; 4];
        M_ClearBox(&mut bbox);

        for &line in sector.lines_in(&level.linebuffer) {
            let line = &level.lines[line];
            let (v1, v2) = (level.vertexes[line.v1], level.vertexes[line.v2]);
            M_AddToBox(&mut bbox, v1.x, v1.y);
            M_AddToBox(&mut bbox, v2.x, v2.y);
        }

        // Set the degenmobj_t to the middle of the bounding box.
        sector.soundorg.x = (bbox[BBox::RIGHT] + bbox[BBox::LEFT]) / 2;
        sector.soundorg.y = (bbox[BBox::TOP] + bbox[BBox::BOTTOM]) / 2;

        // Adjust bounding box to map blocks.
        let mut block = (bbox[BBox::TOP] - bmaporgy + MAXRADIUS) >> MAPBLOCKSHIFT;
        block = if block >= bmapheight {
            bmapheight - 1
        } else {
            block
        };
        sector.blockbox[BBox::TOP] = block;

        block = (bbox[BBox::BOTTOM] - bmaporgy - MAXRADIUS) >> MAPBLOCKSHIFT;
        block = if block < 0 { 0 } else { block };
        sector.blockbox[BBox::BOTTOM] = block;

        block = (bbox[BBox::RIGHT] - bmaporgx + MAXRADIUS) >> MAPBLOCKSHIFT;
        block = if block >= bmapwidth {
            bmapwidth - 1
        } else {
            block
        };
        sector.blockbox[BBox::RIGHT] = block;

        block = (bbox[BBox::LEFT] - bmaporgx - MAXRADIUS) >> MAPBLOCKSHIFT;
        block = if block < 0 { 0 } else { block };
        sector.blockbox[BBox::LEFT] = block;
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
/// If `array` is longer than `rejectpad` (i.e. the lump is extremely short),
/// a warning is printed and the remainder is filled with 0x00 or 0xff
/// depending on whether `-reject_pad_with_ff` was passed on the command line.
///
/// `array` is the missing tail of the reject matrix: the caller
/// (`P_LoadReject`) passes `rejectmatrix[lumplen..minlength]`.
fn PadRejectArray(array: &mut [u8]) {
    let totallines = SETUP.borrow().totallines;
    let rejectpad: [u32; 4] = [((totallines * 4 + 3) & !3) as u32 + 24, 0, 50, 0x1d4a11];
    let padsize = std::mem::size_of_val(&rejectpad);

    for (i, dest) in array.iter_mut().take(padsize).enumerate() {
        let byte_num = i % 4;
        *dest = ((rejectpad[i / 4] >> (byte_num * 8)) & 0xff) as u8;
    }

    if array.len() > padsize {
        eprintln!(
            "PadRejectArray: REJECT lump too short to pad! ({} > {})",
            array.len(),
            padsize
        );

        let padvalue = if M_CheckParm("-reject_pad_with_ff") != 0 {
            0xff
        } else {
            0xf00
        };

        array[padsize..].fill(padvalue as u8);
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
/// The lump is copied into the level's `rejectmatrix`. If it is shorter than
/// `minlength`, the remaining bytes are filled by `PadRejectArray` to
/// simulate vanilla Doom's zone-header overflow.
///
/// Must be called after `P_GroupLines` so that `totallines` is valid (needed
/// by `PadRejectArray`).
fn P_LoadReject(lumpnum: i32) {
    let numsectors = LEVEL.borrow().numsectors;
    let minlength = (numsectors * numsectors + 7) / 8;

    // SAFETY: the lump view is used only before the lump is released.
    let mut rejectmatrix = unsafe { cache_lump_bytes(lumpnum, PU_STATIC) }.to_vec();
    W_ReleaseLumpNum(lumpnum);

    let lumplen = rejectmatrix.len();
    if lumplen < minlength as usize {
        rejectmatrix.resize(minlength as usize, 0);
        PadRejectArray(&mut rejectmatrix[lumplen..]);
    }
    LEVEL.borrow_mut().rejectmatrix = rejectmatrix;
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
pub fn P_SetupLevel(world: &mut World, episode: i32, map: i32, _playermask: i32, _skill: i32) {
    unsafe {
        {
            let mut game = GAME.borrow_mut();
            game.totalkills = 0;
            game.totalitems = 0;
            game.totalsecret = 0;
            game.wminfo.maxfrags = 0;
            game.wminfo.partime = 180;
        }

        for id in PlayerId::all() {
            id.write(|player| {
                player.killcount = 0;
                player.secretcount = 0;
                player.itemcount = 0;
            });
        }

        PlayerId::console().write(|console| console.viewz = 1);

        S_Start();
        Z_FreeTags(PU_LEVEL, PU_PURGELEVEL - 1);

        P_InitThinkers();

        // Find map name.
        let mut lumpname = [0u8; 9];
        if gamemode() == doom::mode::commercial {
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

        TICK.borrow_mut().leveltime = 0;

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

        GAME.borrow_mut().bodyqueslot = 0;
        SPAWN_SPOTS.borrow_mut().deathmatch_p = deathmatchstart(0);
        P_LoadThings(world, lumpnum + MapLump::THINGS);

        if deathmatch() != 0 {
            for i in 0..MAXPLAYERS {
                if playeringame(i) != 0 {
                    PlayerId::new(i).write(|player| player.mo = None);
                    G_DeathMatchSpawnPlayer(world, i as i32);
                }
            }
        }

        {
            let mut mobj = MOBJ.borrow_mut();
            mobj.iquehead = 0;
            mobj.iquetail = 0;
        }

        P_SpawnSpecials(world);

        if GAME.borrow().precache != 0 {
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
    use super::*;

    // =========================================================================
    // Existing structural tests
    // =========================================================================

    #[test]
    fn max_deathmatch_starts_is_10() {
        assert_eq!(MAX_DEATHMATCH_STARTS, 10);
    }

    #[test]
    fn setup_globals_default_zero() {
        let level = LevelState::new();
        assert!(level.vertexes.is_empty());
        assert!(level.segs.is_empty());
        assert_eq!(level.numsectors, 0);
        assert!(level.sectors.is_empty());
        assert!(level.subsectors.is_empty());
        assert!(level.nodes.is_empty());
        assert!(level.lines.is_empty());
        assert!(level.sides.is_empty());
        assert!(level.linebuffer.is_empty());
        assert_eq!(level.bmapwidth, 0);
        assert_eq!(level.bmapheight, 0);
        assert_eq!(level.bmaporgx, 0);
        assert_eq!(level.bmaporgy, 0);
        assert!(level.blockmaplump.is_empty() && level.blocklinks.is_empty());
        assert!(level.rejectmatrix.is_empty());
    }

    /// Serializes the tests below that use the [`SETUP`] scratch state.
    static SETUP_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A zeroed sector with only its tag set.
    fn tagged_sector(tag: i16) -> sector_t {
        let mut sector = sector_t::ZERO;
        sector.tag = tag;
        sector
    }

    /// The glass-hack sector is appended after the map's own sectors on first
    /// use and shared by later segs; `numsectors` keeps counting only the
    /// map's sectors.
    #[test]
    fn glass_hack_sector_follows_the_map_sectors_once() {
        let _guard = SETUP_LOCK.lock().unwrap();
        let mut level = LevelState {
            numsectors: 2,
            sectors: vec![tagged_sector(1), tagged_sector(2)],
            ..LevelState::new()
        };
        let first = GetSectorAtNullAddress(&mut level);
        let second = GetSectorAtNullAddress(&mut level);
        assert_eq!(first, SectorId::new(2));
        assert_eq!(second, first);
        assert_eq!(level.sectors.len(), 3);
        assert_eq!(level.numsectors, 2);
        assert_eq!(level.sectors[first].tag, 0);
    }

    /// A short REJECT lump is padded with the zone block header C read past
    /// its end: block size from `totallines`, null user, `PU_LEVEL` tag
    /// (50) and the zone id.
    #[test]
    fn pad_reject_array_writes_the_zone_block_header() {
        let _guard = SETUP_LOCK.lock().unwrap();
        SETUP.borrow_mut().totallines = 3;
        let mut tail = [0xAAu8; 16];
        PadRejectArray(&mut tail);
        let words: Vec<u32> = tail
            .chunks(4)
            .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
            .collect();
        assert_eq!(words, [((3 * 4 + 3) & !3) + 24, 0, 50, 0x1d4a11]);

        let mut short_tail = [0xAAu8; 5];
        PadRejectArray(&mut short_tail);
        assert_eq!(short_tail, [36, 0, 0, 0, 0]);
    }

    #[test]
    fn setup_globals_are_i32() {
        let level = LevelState::new();
        let _: i32 = level.numsectors;
        let _: i32 = level.bmapwidth;
        let _: i32 = level.bmapheight;
        let _: i32 = level.bmaporgx;
        let _: i32 = level.bmaporgy;
    }

    #[test]
    fn spawn_spot_pointers_index_the_arrays() {
        let base = unsafe { &raw mut (*SPAWN_SPOTS.as_ptr()).playerstarts };
        assert_eq!(playerstart(0), base.cast::<mapthing_t>());
        assert_eq!(
            unsafe { playerstart(3).offset_from(playerstart(0)) },
            3,
            "playerstart(i) must step one mapthing_t per player"
        );
        assert_eq!(
            unsafe { deathmatchstart(MAX_DEATHMATCH_STARTS).offset_from(deathmatchstart(0)) },
            MAX_DEATHMATCH_STARTS as isize
        );
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
            if !commercial && non_commercial_types.contains(&thing_type) {
                spawn = false;
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
