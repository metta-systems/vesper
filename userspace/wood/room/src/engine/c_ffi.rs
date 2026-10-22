//! Not-yet-placed C types and constants, grouped by the original `.c` or
//! `.h` source file they come from.

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]

use {
    super::{
        interface::{
            timer::TICRATE,
            video::{SCREENHEIGHT, SCREENWIDTH},
        },
        map::line_t,
        math::{
            fixed::{FRACBITS, FRACUNIT},
            tables::ANG90,
        },
    },
    std::ffi::c_void,
};

// ---------------------------------------------------------------------------
// Opaque types — we only need pointers to these for many FFI signatures.
// ---------------------------------------------------------------------------

/// Opaque animation-state record (`state_t` in `info.h`).
///
/// Only pointers to this type appear in cross-module FFI signatures; the
/// fields are not accessed from Rust here.
pub enum state_t {}
/// Opaque map-object class descriptor (`mobjinfo_t` in `info.h`).
///
/// Only pointers to this type appear in cross-module FFI signatures; the
/// fields are not accessed from Rust here.
pub enum mobjinfo_t {}

// ---------------------------------------------------------------------------
// p_maputl.c
// ---------------------------------------------------------------------------

/// Intercept record produced by `P_PathTraverse` (`intercept_t` in
/// `p_local.h`).
///
/// `frac` is the fractional position along the trace (fixed-point in
/// `[0, FRACUNIT]`). `isaline` selects the active arm of [`intercept_t_d`]:
/// non-zero means `d.line`, zero means `d.thing`.
#[derive(Clone, Copy)]
pub struct intercept_t {
    pub frac: i32,
    pub isaline: i32, // boolean
    pub d: intercept_t_d,
}

/// Untagged union holding the intercepted entity (`intercept_t.d` in
/// `p_local.h`).
///
/// Active arm is selected by [`intercept_t::isaline`]. Reading the inactive
/// arm is undefined behaviour; callers must consult `isaline` first.
#[derive(Clone, Copy)]
pub union intercept_t_d {
    pub thing: *mut c_void,
    pub line: *mut line_t,
}

// ---------------------------------------------------------------------------
// Constants from C headers
// ---------------------------------------------------------------------------

/// Width/height of one blockmap cell in map units (`MAPBLOCKUNITS` in
/// `p_local.h`).
pub const MAPBLOCKUNITS: i32 = 128;
/// Width/height of one blockmap cell in fixed-point units (`MAPBLOCKSIZE`
/// in `p_local.h`).
pub const MAPBLOCKSIZE: i32 = MAPBLOCKUNITS * FRACUNIT;
/// Right-shift count converting a fixed-point coordinate to a blockmap
/// cell index (`MAPBLOCKSHIFT` in `p_local.h`).
pub const MAPBLOCKSHIFT: i32 = FRACBITS as i32 + 7;
/// Mask isolating the in-cell offset of a fixed-point coordinate
/// (`MAPBMASK` in `p_local.h`).
pub const MAPBMASK: i32 = MAPBLOCKSIZE - 1;
/// Shift count converting in-cell offsets back to fixed-point fractions
/// (`MAPBTOFRAC` in `p_local.h`).
pub const MAPBTOFRAC: i32 = MAPBLOCKSHIFT - FRACBITS as i32;

/// Capacity of the per-tic event queue (`ITEMQUESIZE` analogue used by the
/// engine's input handling).
pub const ITEMQUESIZE: usize = 128;

// ---------------------------------------------------------------------------
// WAD lump-order indices (doomdata.h anonymous enum).
// These are the fixed positions of each data lump within a map's group of
// WAD lumps.  p_setup.c indexes directly into the lump list using these.
// ---------------------------------------------------------------------------

/// WAD map lump indices (`ML_*` in `doomdata.h`). Used as offsets from the
/// map's header lump to locate each sub-lump.
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

const _: () = assert!(
    std::mem::size_of::<i32>() == std::mem::size_of::<i32>(),
    "MapLump constants are i32; i32 must be 32-bit on this platform"
);

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

// ---------------------------------------------------------------------------
// Screen constants (from i_video.h / st_stuff.h)
// ---------------------------------------------------------------------------

/// Status-bar height (ST_HEIGHT from st_stuff.h).
pub const SBARHEIGHT: i32 = 32;

// ---------------------------------------------------------------------------
// r_draw.c constants
// ---------------------------------------------------------------------------

/// Number of entries in the fuzz offset lookup table (FUZZTABLE in r_draw.c).
pub const FUZZTABLE: usize = 50;
/// Column offset used when rendering the fuzz/spectre effect (FUZZOFF = SCREENWIDTH).
pub const FUZZOFF: i32 = SCREENWIDTH;

// ---------------------------------------------------------------------------
// g_game.c
// ---------------------------------------------------------------------------

/// Version code for cph's longtics hack ("v1.91") from `doomdef.h`.
pub const DOOM_191_VERSION: i32 = 111;

// ---------------------------------------------------------------------------
// i_scale.c
// Screen-scaling mode descriptors.
// ---------------------------------------------------------------------------

/// A screen scaling mode descriptor, matching `screen_mode_t` in `i_video.h`.
pub struct screen_mode_t {
    /// Output buffer width in pixels.
    pub width: i32,
    /// Output buffer height in pixels.
    pub height: i32,
    /// Optional initialiser called once with the game palette.
    pub init_mode: Option<unsafe fn(*mut u8)>,
    /// Draw function: copies src buffer → dest buffer for the given rectangle.
    pub draw_screen: Option<unsafe fn(i32, i32, i32, i32) -> i32>,
    /// True when this mode uses blended interpolation (lower visual quality).
    pub poor_quality: i32,
}

// ---------------------------------------------------------------------------
// Additional constants from C headers
// ---------------------------------------------------------------------------

/// Screen width used by the "squash" scale modes (SCREENWIDTH_4_3 in i_video.h).
pub const SCREENWIDTH_4_3: i32 = 256;
/// Screen height used by the "stretch" scale modes (SCREENHEIGHT_4_3 in i_video.h).
pub const SCREENHEIGHT_4_3: i32 = 240;

/// Size of the body-object circular queue (BODYQUESIZE in g_game.c).
pub const BODYQUESIZE: usize = 32;
/// Number of tics during which slow-turn speed is used before switching to
/// normal turn speed (SLOWTURNTICS in g_game.c).
pub const SLOWTURNTICS: i32 = 6;
/// Speed threshold above which the turbo-cheat detector fires
/// (TURBOTHRESHOLD in g_game.c = 0x32 = 50).
pub const TURBOTHRESHOLD: i32 = 0x32;

/// Weapon-sprite lower speed (LOWERSPEED in p_pspr.c = FRACUNIT × 6).
pub const LOWERSPEED: i32 = FRACUNIT * 6;
/// Weapon-sprite raise speed (RAISESPEED in p_pspr.c = FRACUNIT × 6).
pub const RAISESPEED: i32 = FRACUNIT * 6;
/// Y-position of the weapon at its lowest (off-screen) rest point
/// (WEAPONBOTTOM in p_pspr.c = 128 × FRACUNIT).
pub const WEAPONBOTTOM: i32 = 128 * FRACUNIT;
/// Y-position of the weapon at its highest (ready) position
/// (WEAPONTOP in p_pspr.c = 32 × FRACUNIT).
pub const WEAPONTOP: i32 = 32 * FRACUNIT;

/// Minimum sprite Z distance; sprites closer than this are not projected
/// (MINZ in r_things.c = FRACUNIT × 4).
pub const MINZ: i32 = FRACUNIT * 4;
/// Vertical centre of the screen in pixels, used for sprite projection
/// (BASEYCENTER in r_things.c = 100).
pub const BASEYCENTER: i32 = 100;

// ---------------------------------------------------------------------------
// p_inter.c — ammo and interaction constants
// ---------------------------------------------------------------------------

/// Number of ammo types (NUMAMMO in doomdef.h = 4: clip, shell, cell, misl).
pub const NUMAMMO: usize = 4;
/// Bonus count added per pick-up for the screen flash effect
/// (BONUSADD in p_inter.c = 6).
pub const BONUSADD: i32 = 6;

// ---------------------------------------------------------------------------
// p_mobj.c — map-object physics constants
// ---------------------------------------------------------------------------

/// Velocity below which horizontal momentum is zeroed (STOPSPEED = 0x1000).
pub const STOPSPEED: i32 = 0x1000;
/// Friction factor applied to horizontal velocity each tic (FRICTION = 0xe800).
pub const FRICTION: i32 = 0xe800_u32 as i32;

// ---------------------------------------------------------------------------
// p_spec.c — sector-special and animation constants / globals
// ---------------------------------------------------------------------------

/// Maximum number of running floor/ceiling/door animations at once
/// (MAXANIMS in p_spec.c = 32).
pub const MAXANIMS: i32 = 32;
/// Maximum number of active line specials in a level
/// (MAXLINEANIMS in p_spec.c = 64).
pub const MAXLINEANIMS: i32 = 64;

/// Glow effect speed (GLOWSPEED in p_spec.h = 8).
pub const GLOWSPEED: i32 = 8;
/// Bright strobe level (STROBEBRIGHT in p_spec.h = 5).
pub const STROBEBRIGHT: i32 = 5;
/// Fast strobe dark duration in tics (FASTDARK in p_spec.h = 15).
pub const FASTDARK: i32 = 15;
/// Slow strobe dark duration in tics (SLOWDARK in p_spec.h = 35).
pub const SLOWDARK: i32 = 35;

/// Vertical door speed (VDOORSPEED = FRACUNIT × 2).
pub const VDOORSPEED: i32 = FRACUNIT * 2;
/// Vertical door wait time in tics (VDOORWAIT = 150).
pub const VDOORWAIT: i32 = 150;

/// Ceiling movement speed (CEILSPEED = FRACUNIT).
pub const CEILSPEED: i32 = FRACUNIT;
/// Maximum simultaneously active ceilings (MAXCEILINGS = 30).
pub const MAXCEILINGS: i32 = 30;
/// Ceiling movement wait time in tics (CEILWAIT = 150).
pub const CEILWAIT: i32 = 150;

/// Platform movement speed (PLATSPEED = FRACUNIT).
pub const PLATSPEED: i32 = FRACUNIT;
/// Maximum simultaneously active platforms (MAXPLATS = 30).
pub const MAXPLATS: i32 = 30;
/// Platform wait time in seconds (PLATWAIT = 3).
pub const PLATWAIT: i32 = 3;

/// Floor movement speed (FLOORSPEED = FRACUNIT).
pub const FLOORSPEED: i32 = FRACUNIT;

// ---------------------------------------------------------------------------
// p_map.c — collision detection globals and constants
// ---------------------------------------------------------------------------

/// Sentinel magic value used to detect the vanilla spechit overflow bug
/// (DEFAULT_SPECHIT_MAGIC in p_map.c = 0x01C09C98).
pub const DEFAULT_SPECHIT_MAGIC: u32 = 0x01C09C98;

// All p_map globals are now exported from `room/src/engine/play/map.rs`.

// ---------------------------------------------------------------------------
// p_saveg.c — save-game serialization constants and globals
// ---------------------------------------------------------------------------

/// End-of-file marker byte written at the end of every save game
/// (SAVEGAME_EOF in p_saveg.c = 0x1d).
pub const SAVEGAME_EOF: u8 = 0x1d;
/// Length of the version string embedded in save-game headers
/// (VERSIONSIZE in p_saveg.c = 16).
pub const VERSIONSIZE: usize = 16;

// ---------------------------------------------------------------------------
// p_enemy.c — enemy AI globals and constants
// ---------------------------------------------------------------------------

/// Angle turned per tic when pursuing a lost target
/// (TRACEANGLE in p_enemy.c = 0xc000000).
pub const TRACEANGLE: u32 = 0xc000000;
/// Arc spread angle for the Mancubus fireball pattern
/// (FATSPREAD = ANG90 / 8).
pub const FATSPREAD: u32 = ANG90 / 8;
/// Speed of the Lost Soul charge attack in fixed-point units/tic
/// (SKULLSPEED = 20 × FRACUNIT).
pub const SKULLSPEED: i32 = 20 * FRACUNIT;

// ---------------------------------------------------------------------------
// wi_stuff.c — intermission screen constants
// ---------------------------------------------------------------------------

/// Number of episodes (NUMEPISODES in wi_stuff.c = 4).
pub const WI_NUMEPISODES: usize = 4;
/// Number of maps per episode (NUMMAPS in wi_stuff.c = 9).
pub const WI_NUMMAPS: usize = 9;
/// Y-position of the level-name title on the intermission screen (WI_TITLEY = 2).
pub const WI_TITLEY: i32 = 2;
/// Vertical spacing between player stats rows (WI_SPACINGY = 33).
pub const WI_SPACINGY: i32 = 33;
/// X-position of single-player stats (SP_STATSX = 50).
pub const SP_STATSX: i32 = 50;
/// Y-position of single-player stats (SP_STATSY = 50).
pub const SP_STATSY: i32 = 50;
/// X-position of the time display (SP_TIMEX = 16).
pub const SP_TIMEX: i32 = 16;
/// Y-position of the time display (SP_TIMEY = SCREENHEIGHT − 32).
pub const SP_TIMEY: i32 = SCREENHEIGHT - 32;
/// Delay in tics before showing the "next level" location (SHOWNEXTLOCDELAY = 4).
pub const SHOWNEXTLOCDELAY: i32 = 4;
/// X spacing between deathmatch matrix columns (DM_SPACINGX = 40).
pub const DM_SPACINGX: i32 = 40;
/// Number of animated elements in episode 0 background (EPSD0ANIMINFO length).
pub const WI_EPSD0_NANIM: usize = crate::engine::intermission::EPSD0_NANIM;
/// Number of animated elements in episode 1 background (EPSD1ANIMINFO length).
pub const WI_EPSD1_NANIM: usize = crate::engine::intermission::EPSD1_NANIM;
/// Number of animated elements in episode 2 background (EPSD2ANIMINFO length).
pub const WI_EPSD2_NANIM: usize = crate::engine::intermission::EPSD2_NANIM;

// ---------------------------------------------------------------------------
// st_stuff.c — status bar constants
// ---------------------------------------------------------------------------

/// Palette index at which red (pain) palette rotations begin (STARTREDPALS = 1).
pub const STARTREDPALS: i32 = 1;
/// Palette index at which bonus (item pick-up) rotations begin (STARTBONUSPALS = 9).
pub const STARTBONUSPALS: i32 = 9;
/// Number of red pain palette entries (NUMREDPALS = 8).
pub const NUMREDPALS: i32 = 8;
/// Number of bonus pick-up palette entries (NUMBONUSPALS = 4).
pub const NUMBONUSPALS: i32 = 4;
/// Palette index used for the radiation suit overlay (RADIATIONPAL = 13).
pub const RADIATIONPAL: i32 = 13;
/// 1-in-N probability of a random face change per tic (ST_FACEPROBABILITY = 96).
pub const ST_FACEPROBABILITY: i32 = 96;
/// Number of pain-level face groups (ST_NUMPAINFACES = 5).
pub const ST_NUMPAINFACES: i32 = 5;
/// Number of straight (non-turning) faces per pain group (ST_NUMSTRAIGHTFACES = 3).
pub const ST_NUMSTRAIGHTFACES: i32 = 3;
/// Number of turning face variants per pain group (ST_NUMTURNFACES = 2).
pub const ST_NUMTURNFACES: i32 = 2;
/// Number of special face frames (ST_NUMSPECIALFACES = 3: ouch, evil-grin, rampage).
pub const ST_NUMSPECIALFACES: i32 = 3;
/// Number of extra face frames beyond the pain table (ST_NUMEXTRAFACES = 2: god, dead).
pub const ST_NUMEXTRAFACES: i32 = 2;
/// Total face sprite count = pain × stride + extra.
pub const ST_NUMFACES: i32 = ST_NUMPAINFACES
    * (ST_NUMSTRAIGHTFACES + ST_NUMTURNFACES + ST_NUMSPECIALFACES)
    + ST_NUMEXTRAFACES;
/// Tics of evil-grin display (ST_EVILGRINCOUNT = 2 × TICRATE).
pub const ST_EVILGRINCOUNT: i32 = 2 * TICRATE;
/// Tics of straight-face display (ST_STRAIGHTFACECOUNT = TICRATE / 2).
pub const ST_STRAIGHTFACECOUNT: i32 = TICRATE / 2;
/// Tics before turn-face times out (ST_TURNCOUNT = 1 × TICRATE).
pub const ST_TURNCOUNT: i32 = TICRATE;
/// Tics of ouch-face display (ST_OUCHCOUNT = 1 × TICRATE).
pub const ST_OUCHCOUNT: i32 = TICRATE;
/// Tics of rampage-face display (ST_RAMPAGEDELAY = 2 × TICRATE).
pub const ST_RAMPAGEDELAY: i32 = 2 * TICRATE;
/// Damage threshold for the rampage face (ST_MUCHPAIN = 20).
pub const ST_MUCHPAIN: i32 = 20;
/// X pixel offset of the status bar (ST_X = 0).
pub const ST_X: i32 = 0;
/// X pixel offset of the arms display (ST_X2 = 104).
pub const ST_X2: i32 = 104;
