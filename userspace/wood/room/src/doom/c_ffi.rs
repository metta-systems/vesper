//! Centralized FFI declarations for unported C modules.
//!
//! Each `extern "C"` block is organized by the original `.c` source file.
//! These declarations allow Rust unit tests to call the original C functions
//! directly and read C global variables, establishing behavioral baselines
//! before porting each module.

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]

use std::ffi::{c_int, c_short, c_uint, c_ushort, c_void};

pub use super::i_timer::TICRATE;
pub use super::i_video::{SCREENHEIGHT, SCREENWIDTH};
pub use super::m_bbox::BBox;
pub use super::m_fixed::{FRACBITS, FRACUNIT};
pub use super::tables::{ANG180, ANG270, ANG45, ANG90, ANGLETOFINESHIFT, FINEMASK};

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
// Minimal repr(C) types needed for p_maputl.c tests.
//
// These definitions match the layouts in p_telept.rs / p_sight.rs so that
// pointer casts between the two are safe.
// ---------------------------------------------------------------------------

/// Runtime vertex record (`vertex_t` in `r_defs.h`).
///
/// Fields `x` and `y` are fixed-point map coordinates.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct vertex_t {
    pub x: c_int,
    pub y: c_int,
}

/// Parametric dividing line (`divline_t` in `p_local.h`).
///
/// Used by `P_PointOnDivlineSide` and the intercept-traversal helpers.
/// `(x, y)` is the start point; `(dx, dy)` is the direction vector in
/// fixed-point map units.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct divline_t {
    pub x: c_int,
    pub y: c_int,
    pub dx: c_int,
    pub dy: c_int,
}

/// Runtime BSP subsector (`subsector_t` in `r_defs.h`).
///
/// `sector` points to the owning `sector_t`; `firstline` indexes into the
/// segs array and `numlines` is the count of segs forming the convex
/// polygon for this subsector. Trailing padding aligns to 8 bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct subsector_t {
    pub sector: *mut c_void,
    pub numlines: i16,
    pub firstline: i16,
    _pad: [u8; 4],
}

/// On-disk thing record (`mapthing_t` in `doomdata.h`).
///
/// 10-byte packed record from the WAD THINGS lump describing a spawn point.
/// `x` and `y` are in map units (not fixed-point), `angle` is degrees,
/// `type` is the Doom editor number, and `options` is a bit mask of skill
/// and multiplayer flags.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct mapthing_t {
    pub x: i16,
    pub y: i16,
    pub angle: i16,
    pub r#type: i16,
    pub options: i16,
}

/// Runtime sector record (`sector_t` in `r_defs.h`).
///
/// Layout mirrors the C definition exactly so pointer casts between the
/// Rust and C halves of the engine remain sound. `soundorg` is opaque
/// `degenmobj_t` storage (40 bytes); explicit `_pad0` reflects the C
/// compiler's natural padding after the four packed `c_short` fields.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct sector_t {
    pub floorheight: c_int,
    pub ceilingheight: c_int,
    pub floorpic: c_short,
    pub ceilingpic: c_short,
    pub lightlevel: c_short,
    pub special: c_short,
    pub tag: c_short,
    _pad0: [u8; 2],
    pub soundtraversed: c_int,
    pub soundtarget: *mut c_void,
    pub blockbox: [c_int; 4],
    pub soundorg: [u8; 40],
    pub validcount: c_int,
    pub thinglist: *mut c_void,
    pub specialdata: *mut c_void,
    pub linecount: c_int,
    pub lines: *mut *mut c_void,
}

/// Runtime line definition (`line_t` in `r_defs.h`).
///
/// A line is bounded by two vertices, has a direction vector `(dx, dy)`,
/// a `flags` bit mask of [`LinedefFlag`] entries, an optional `special`
/// trigger, sector tags, and `sidenum[2]` indices (-1 for one-sided lines).
/// `frontsector` and `backsector` are opaque `sector_t*` pointers; cast
/// them to [`sector_t`] when access is required.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct line_t {
    pub v1: *mut vertex_t,
    pub v2: *mut vertex_t,
    pub dx: c_int,
    pub dy: c_int,
    pub flags: c_short,
    pub special: c_short,
    pub tag: c_short,
    pub sidenum: [c_short; 2],
    pub bbox: [c_int; 4],
    pub slopetype: c_int,
    pub frontsector: *mut c_void,
    pub backsector: *mut c_void,
    pub validcount: c_int,
    pub specialdata: *mut c_void,
}

/// Intercept record produced by `P_PathTraverse` (`intercept_t` in
/// `p_local.h`).
///
/// `frac` is the fractional position along the trace (fixed-point in
/// `[0, FRACUNIT]`). `isaline` selects the active arm of [`intercept_t_d`]:
/// non-zero means `d.line`, zero means `d.thing`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct intercept_t {
    pub frac: c_int,
    pub isaline: c_int, // boolean
    pub d: intercept_t_d,
}

/// Untagged union holding the intercepted entity (`intercept_t.d` in
/// `p_local.h`).
///
/// Active arm is selected by [`intercept_t::isaline`]. Reading the inactive
/// arm is undefined behaviour; callers must consult `isaline` first.
#[repr(C)]
#[derive(Clone, Copy)]
pub union intercept_t_d {
    pub thing: *mut c_void,
    pub line: *mut line_t,
}

/// Runtime map-object record (`mobj_t` in `p_mobj.h`).
///
/// Layout matches the C definition exactly, including the 24-byte
/// `thinker_t` prefix (`thinker_prev`, `thinker_next`, `thinker_fn`) and
/// the explicit `_padN` fillers required by the C compiler's alignment
/// rules on 64-bit platforms. See the project memory note on `MobjStub` for
/// the consequences of getting this layout wrong.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mobj_t {
    pub thinker_prev: *mut c_void,
    pub thinker_next: *mut c_void,
    pub thinker_fn: *mut c_void,
    pub x: c_int,
    pub y: c_int,
    pub z: c_int,
    _pad0: [u8; 4],
    pub snext: *mut c_void,
    pub sprev: *mut c_void,
    pub angle: c_uint,
    pub sprite: c_int,
    pub frame: c_int,
    _pad1: [u8; 4],
    pub bnext: *mut c_void,
    pub bprev: *mut c_void,
    pub subsector: *mut c_void,
    pub floorz: c_int,
    pub ceilingz: c_int,
    pub radius: c_int,
    pub height: c_int,
    pub momx: c_int,
    pub momy: c_int,
    pub momz: c_int,
    pub validcount: c_int,
    pub type_: c_int,
    _pad2: [u8; 4],
    pub info: *mut mobjinfo_t,
    pub tics: c_int,
    _pad3: [u8; 4],
    pub state: *mut state_t,
    pub flags: c_int,
    pub health: c_int,
    pub movedir: c_int,
    pub movecount: c_int,
    pub target: *mut c_void,
    pub reactiontime: c_int,
    pub threshold: c_int,
    pub player: *mut c_void,
    pub lastlook: c_int,
    pub spawnpoint: mapthing_t,
    _pad4: [u8; 2],
    pub tracer: *mut c_void,
}

// ---------------------------------------------------------------------------
// p_maputl.c
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// r_draw.c — remaining C functions
// ---------------------------------------------------------------------------

extern "C" {
    /// Initialise the view-window buffer for the given dimensions
    /// (`R_InitBuffer` in `r_draw.c`).
    pub fn R_InitBuffer(width: c_int, height: c_int);
    /// Build per-player palette-translation tables
    /// (`R_InitTranslationTables` in `r_draw.c`).
    pub fn R_InitTranslationTables();
    /// Fill the area outside the 3D view window with the back screen
    /// (`R_FillBackScreen` in `r_draw.c`).
    pub fn R_FillBackScreen();
    /// Erase a horizontal run of pixels from the back-screen buffer
    /// (`R_VideoErase` in `r_draw.c`).
    pub fn R_VideoErase(ofs: c_uint, count: c_int);
    /// Redraw the view-window border after a console or menu close
    /// (`R_DrawViewBorder` in `r_draw.c`).
    pub fn R_DrawViewBorder();
}

// ---------------------------------------------------------------------------
// Constants from C headers
// ---------------------------------------------------------------------------

/// Width/height of one blockmap cell in map units (`MAPBLOCKUNITS` in
/// `p_local.h`).
pub const MAPBLOCKUNITS: c_int = 128;
/// Width/height of one blockmap cell in fixed-point units (`MAPBLOCKSIZE`
/// in `p_local.h`).
pub const MAPBLOCKSIZE: c_int = MAPBLOCKUNITS * FRACUNIT;
/// Right-shift count converting a fixed-point coordinate to a blockmap
/// cell index (`MAPBLOCKSHIFT` in `p_local.h`).
pub const MAPBLOCKSHIFT: c_int = FRACBITS as c_int + 7;
/// Mask isolating the in-cell offset of a fixed-point coordinate
/// (`MAPBMASK` in `p_local.h`).
pub const MAPBMASK: c_int = MAPBLOCKSIZE - 1;
/// Shift count converting in-cell offsets back to fixed-point fractions
/// (`MAPBTOFRAC` in `p_local.h`).
pub const MAPBTOFRAC: c_int = MAPBLOCKSHIFT - FRACBITS as c_int;

/// Capacity of the per-tic event queue (`ITEMQUESIZE` analogue used by the
/// engine's input handling).
pub const ITEMQUESIZE: usize = 128;

/// Slope type for a horizontal line segment (`ST_HORIZONTAL` in `r_defs.h`).
pub const ST_HORIZONTAL: c_int = 0;
/// Slope type for a vertical line segment (`ST_VERTICAL` in `r_defs.h`).
pub const ST_VERTICAL: c_int = 1;
/// Slope type for a line with positive gradient (`ST_POSITIVE` in `r_defs.h`).
pub const ST_POSITIVE: c_int = 2;
/// Slope type for a line with negative gradient (`ST_NEGATIVE` in `r_defs.h`).
pub const ST_NEGATIVE: c_int = 3;

// ---------------------------------------------------------------------------
// WAD lump-order indices (doomdata.h anonymous enum).
// These are the fixed positions of each data lump within a map's group of
// WAD lumps.  p_setup.c indexes directly into the lump list using these.
// ---------------------------------------------------------------------------

/// WAD map lump indices (`ML_*` in `doomdata.h`). Used as offsets from the
/// map's header lump to locate each sub-lump.
#[repr(C)]
pub struct MapLump;
/// `ML_*` lump-offset constants used to index a map's group of WAD lumps.
impl MapLump {
    #[doc(alias = "ML_LABEL")]
    pub const LABEL: c_int = 0; // ExMx / MAPxx separator
    #[doc(alias = "ML_THINGS")]
    pub const THINGS: c_int = 1; // Monster/item placement
    #[doc(alias = "ML_LINEDEFS")]
    pub const LINEDEFS: c_int = 2; // Line definitions
    #[doc(alias = "ML_SIDEDEFS")]
    pub const SIDEDEFS: c_int = 3; // Side (texture) definitions
    #[doc(alias = "ML_VERTEXES")]
    pub const VERTEXES: c_int = 4; // Vertex coordinates
    #[doc(alias = "ML_SEGS")]
    pub const SEGS: c_int = 5; // BSP line segments
    #[doc(alias = "ML_SSECTORS")]
    pub const SSECTORS: c_int = 6; // BSP sub-sectors
    #[doc(alias = "ML_NODES")]
    pub const NODES: c_int = 7; // BSP nodes
    #[doc(alias = "ML_SECTORS")]
    pub const SECTORS: c_int = 8; // Sector definitions
    #[doc(alias = "ML_REJECT")]
    pub const REJECT: c_int = 9; // Sector-to-sector visibility table
    #[doc(alias = "ML_BLOCKMAP")]
    pub const BLOCKMAP: c_int = 10; // Motion-clipping blockmap
}

const _: () = assert!(
    std::mem::size_of::<c_int>() == std::mem::size_of::<i32>(),
    "MapLump constants are c_int; c_int must be 32-bit on this platform"
);

/// Linedef flag bits (`ML_*` in `doomdata.h`). Stored in `line_t.flags` as
/// a bitmask; each constant is a single-bit mask.
#[repr(C)]
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

const _: () = assert!(
    std::mem::size_of::<u16>() == std::mem::size_of::<std::os::raw::c_short>(),
    "LinedefFlag masks are u16; u16 must match c_short (line_t.flags)"
);

// ---------------------------------------------------------------------------
// Screen constants (from i_video.h / st_stuff.h)
// ---------------------------------------------------------------------------

/// Status-bar height (ST_HEIGHT from st_stuff.h).
pub const SBARHEIGHT: c_int = 32;

// ---------------------------------------------------------------------------
// r_draw.c constants
// ---------------------------------------------------------------------------

/// Number of entries in the fuzz offset lookup table (FUZZTABLE in r_draw.c).
pub const FUZZTABLE: usize = 50;
/// Column offset used when rendering the fuzz/spectre effect (FUZZOFF = SCREENWIDTH).
pub const FUZZOFF: c_int = SCREENWIDTH;

// ---------------------------------------------------------------------------
// Runtime renderer types (r_defs.h) — used by unported r_segs.c / r_things.c
// ---------------------------------------------------------------------------

/// SideDef: visual appearance of a wall segment (`side_t` in `r_defs.h`).
///
/// Two-byte `_pad` matches the C compiler's natural alignment after the
/// three packed `c_short` texture indices and before the pointer field.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct side_t {
    pub textureoffset: c_int,
    pub rowoffset: c_int,
    pub toptexture: c_short,
    pub bottomtexture: c_short,
    pub midtexture: c_short,
    _pad: [u8; 2],
    pub sector: *mut sector_t,
}

/// LineSeg: a BSP-split segment of a line definition (`seg_t` in
/// `r_defs.h`).
///
/// `offset` is the distance along the parent linedef to `v1` in
/// fixed-point. `angle` is a BAM (Binary Angle Measurement) facing
/// direction. `frontsector` is always non-null; `backsector` is null
/// for one-sided segments (per the C comment in `r_defs.h`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct seg_t {
    pub v1: *mut vertex_t,
    pub v2: *mut vertex_t,
    pub offset: c_int,
    pub angle: c_uint,
    pub sidedef: *mut side_t,
    pub linedef: *mut line_t,
    pub frontsector: *mut sector_t,
    pub backsector: *mut sector_t,
}

/// BSP node: partitions space into two sub-trees (`node_t` in `r_defs.h`).
///
/// `(x, y)` is a point on the partition line; `(dx, dy)` is the direction
/// vector. `bbox[side]` is the bounding box of each child subtree;
/// `children[side]` is a node or subsector index, with the high bit set
/// to indicate a subsector leaf.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct node_t {
    pub x: c_int,
    pub y: c_int,
    pub dx: c_int,
    pub dy: c_int,
    pub bbox: [[c_int; 4]; 2],
    pub children: [c_ushort; 2],
}

/// Draw segment: one visible wall segment produced by the BSP traversal
/// (`drawseg_t` in `r_defs.h`).
///
/// Built by `R_StoreWallRange`; consumed by sprite clipping and the wall
/// rasteriser. `silhouette` is a bit mask indicating whether the segment
/// occludes from below (`SIL_BOTTOM`), above (`SIL_TOP`), or both.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct drawseg_t {
    pub curline: *mut seg_t,
    pub x1: c_int,
    pub x2: c_int,
    pub scale1: c_int,
    pub scale2: c_int,
    pub scalestep: c_int,
    pub silhouette: c_int,
    pub bsilheight: c_int,
    pub tsilheight: c_int,
    pub sprtopclip: *mut c_short,
    pub sprbottomclip: *mut c_short,
    pub maskedtexturecol: *mut c_short,
}

/// Visible sprite: a thing that is (partly) visible in the current frame
/// (`vissprite_t` in `r_defs.h`).
///
/// Built by `R_ProjectSprite`; sorted by `scale` (depth) before
/// rasterisation. `colormap` selects light level / palette translation
/// per-column.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct vissprite_t {
    pub prev: *mut vissprite_t,
    pub next: *mut vissprite_t,
    pub x1: c_int,
    pub x2: c_int,
    pub gx: c_int,
    pub gy: c_int,
    pub gz: c_int,
    pub gzt: c_int,
    pub startfrac: c_int,
    pub scale: c_int,
    pub xiscale: c_int,
    pub texturemid: c_int,
    pub patch: c_int,
    _pad: [u8; 4],
    pub colormap: *mut u8,
    pub mobjflags: c_int,
    _pad2: [u8; 4],
}

/// One animation-frame of a sprite, matching r_defs.h `spriteframe_t`.
///
/// Layout (28 bytes):
///   +0  rotate (boolean/int – 4 bytes)
///   +4  lump[8] (short[8] – 16 bytes)
///   +20 flip[8] (byte[8] – 8 bytes)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct spriteframe_t {
    /// 0 = use frame 0 for all rotations; 1 = use rotation-specific lumps.
    pub rotate: c_int,
    /// WAD lump index (relative to firstspritelump) for each of 8 rotations.
    pub lump: [c_short; 8],
    /// 1 = horizontally flip the lump for this rotation; 0 = no flip.
    pub flip: [u8; 8],
}

// ---------------------------------------------------------------------------
// g_game.c — movement tables and game-state globals.
// forwardmove / sidemove / angleturn are static initialisers (non-zero).
// ---------------------------------------------------------------------------

extern "C" {
    /// Forward movement speed table: [slow, fast] (fixed_t, unit/tic).
    /// Values: {0x19, 0x32} = {25, 50}.
    pub static mut forwardmove: [c_int; 2];
    /// Lateral (strafe) movement speed table: [slow, fast] (fixed_t, unit/tic).
    /// Values: {0x18, 0x28} = {24, 40}.
    pub static mut sidemove: [c_int; 2];
    /// Turn-speed table: [normal, fast, slow] (BAM units/tic).
    /// Values: {640, 1280, 320}.  Index 2 is used for the first SLOWTURNTICS (6)
    /// tics; after that index 0 (or 1 with run) is used.
    pub static mut angleturn: [c_int; 3];
    /// Next slot in the circular body-queue ring buffer.
    pub static mut bodyqueslot: c_int;
    /// When true, savegame file size is capped at the vanilla limit (0x2c000).
    pub static mut vanilla_savegame_limit: c_int;
    /// When true, demo file size is capped at the vanilla limit.
    pub static mut vanilla_demo_limit: c_int;
    /// When true, all graphics are preloaded at level start.
    pub static mut precache: c_int;
    /// When true (set by -testcontrols), exit after the first tic.
    pub static mut testcontrols: c_int;
    /// Gametic at which the current level started.
    pub static mut levelstarttic: c_int;
    /// Total enemy count on the current level (for intermission).
    pub static mut totalkills: c_int;
    /// Total item count on the current level (for intermission).
    pub static mut totalitems: c_int;
    /// Total secret count on the current level (for intermission).
    pub static mut totalsecret: c_int;
}

/// Version code for cph's longtics hack ("v1.91") from `doomdef.h`.
pub const DOOM_191_VERSION: c_int = 111;

// ---------------------------------------------------------------------------
// i_scale.c
// Screen-scaling mode descriptors.
// ---------------------------------------------------------------------------

/// A screen scaling mode descriptor, matching `screen_mode_t` in `i_video.h`.
///
/// Layout on 64-bit (32 bytes):
///   +0  width          (int, 4 bytes)
///   +4  height         (int, 4 bytes)
///   +8  InitMode       (fn ptr, 8 bytes)
///   +16 DrawScreen     (fn ptr, 8 bytes)
///   +24 poor_quality   (int/boolean, 4 bytes)
///   +28 [4 bytes tail-padding to align struct to 8]
#[repr(C)]
pub struct screen_mode_t {
    /// Output buffer width in pixels.
    pub width: c_int,
    /// Output buffer height in pixels.
    pub height: c_int,
    /// Optional initialiser called once with the game palette.
    pub init_mode: Option<unsafe extern "C" fn(*mut u8)>,
    /// Draw function: copies src buffer → dest buffer for the given rectangle.
    pub draw_screen: Option<unsafe extern "C" fn(c_int, c_int, c_int, c_int) -> c_int>,
    /// True when this mode uses blended interpolation (lower visual quality).
    pub poor_quality: c_int,
}

// Re-exported from the Rust port of i_scale.c.
pub use crate::doom::i_scale::{
    mode_scale_1x, mode_scale_2x, mode_scale_3x, mode_scale_4x, mode_scale_5x, mode_squash_1x,
    mode_squash_2x, mode_squash_3x, mode_squash_4x, mode_squash_5x, mode_stretch_1x,
    mode_stretch_2x, mode_stretch_3x, mode_stretch_4x, mode_stretch_5x,
};

// ---------------------------------------------------------------------------
// Additional constants from C headers
// ---------------------------------------------------------------------------

/// Screen width used by the "squash" scale modes (SCREENWIDTH_4_3 in i_video.h).
pub const SCREENWIDTH_4_3: c_int = 256;
/// Screen height used by the "stretch" scale modes (SCREENHEIGHT_4_3 in i_video.h).
pub const SCREENHEIGHT_4_3: c_int = 240;

/// Size of the body-object circular queue (BODYQUESIZE in g_game.c).
pub const BODYQUESIZE: usize = 32;
/// Number of tics during which slow-turn speed is used before switching to
/// normal turn speed (SLOWTURNTICS in g_game.c).
pub const SLOWTURNTICS: c_int = 6;
/// Speed threshold above which the turbo-cheat detector fires
/// (TURBOTHRESHOLD in g_game.c = 0x32 = 50).
pub const TURBOTHRESHOLD: c_int = 0x32;

/// Weapon-sprite lower speed (LOWERSPEED in p_pspr.c = FRACUNIT × 6).
pub const LOWERSPEED: c_int = FRACUNIT * 6;
/// Weapon-sprite raise speed (RAISESPEED in p_pspr.c = FRACUNIT × 6).
pub const RAISESPEED: c_int = FRACUNIT * 6;
/// Y-position of the weapon at its lowest (off-screen) rest point
/// (WEAPONBOTTOM in p_pspr.c = 128 × FRACUNIT).
pub const WEAPONBOTTOM: c_int = 128 * FRACUNIT;
/// Y-position of the weapon at its highest (ready) position
/// (WEAPONTOP in p_pspr.c = 32 × FRACUNIT).
pub const WEAPONTOP: c_int = 32 * FRACUNIT;

/// Minimum sprite Z distance; sprites closer than this are not projected
/// (MINZ in r_things.c = FRACUNIT × 4).
pub const MINZ: c_int = FRACUNIT * 4;
/// Vertical centre of the screen in pixels, used for sprite projection
/// (BASEYCENTER in r_things.c = 100).
pub const BASEYCENTER: c_int = 100;

// ---------------------------------------------------------------------------
// p_inter.c — ammo and interaction constants
// ---------------------------------------------------------------------------

/// Number of ammo types (NUMAMMO in doomdef.h = 4: clip, shell, cell, misl).
pub const NUMAMMO: usize = 4;
/// Bonus count added per pick-up for the screen flash effect
/// (BONUSADD in p_inter.c = 6).
pub const BONUSADD: c_int = 6;

// ---------------------------------------------------------------------------
// p_mobj.c — map-object physics constants
// ---------------------------------------------------------------------------

/// Velocity below which horizontal momentum is zeroed (STOPSPEED = 0x1000).
pub const STOPSPEED: c_int = 0x1000;
/// Friction factor applied to horizontal velocity each tic (FRICTION = 0xe800).
pub const FRICTION: c_int = 0xe800_u32 as c_int;

// ---------------------------------------------------------------------------
// p_spec.c — sector-special and animation constants / globals
// ---------------------------------------------------------------------------

/// Maximum number of running floor/ceiling/door animations at once
/// (MAXANIMS in p_spec.c = 32).
pub const MAXANIMS: c_int = 32;
/// Maximum number of active line specials in a level
/// (MAXLINEANIMS in p_spec.c = 64).
pub const MAXLINEANIMS: c_int = 64;

/// Glow effect speed (GLOWSPEED in p_spec.h = 8).
pub const GLOWSPEED: c_int = 8;
/// Bright strobe level (STROBEBRIGHT in p_spec.h = 5).
pub const STROBEBRIGHT: c_int = 5;
/// Fast strobe dark duration in tics (FASTDARK in p_spec.h = 15).
pub const FASTDARK: c_int = 15;
/// Slow strobe dark duration in tics (SLOWDARK in p_spec.h = 35).
pub const SLOWDARK: c_int = 35;

/// Vertical door speed (VDOORSPEED = FRACUNIT × 2).
pub const VDOORSPEED: c_int = FRACUNIT * 2;
/// Vertical door wait time in tics (VDOORWAIT = 150).
pub const VDOORWAIT: c_int = 150;

/// Ceiling movement speed (CEILSPEED = FRACUNIT).
pub const CEILSPEED: c_int = FRACUNIT;
/// Maximum simultaneously active ceilings (MAXCEILINGS = 30).
pub const MAXCEILINGS: c_int = 30;
/// Ceiling movement wait time in tics (CEILWAIT = 150).
pub const CEILWAIT: c_int = 150;

/// Platform movement speed (PLATSPEED = FRACUNIT).
pub const PLATSPEED: c_int = FRACUNIT;
/// Maximum simultaneously active platforms (MAXPLATS = 30).
pub const MAXPLATS: c_int = 30;
/// Platform wait time in seconds (PLATWAIT = 3).
pub const PLATWAIT: c_int = 3;

/// Floor movement speed (FLOORSPEED = FRACUNIT).
pub const FLOORSPEED: c_int = FRACUNIT;

// ---------------------------------------------------------------------------
// p_map.c — collision detection globals and constants
// ---------------------------------------------------------------------------

/// Sentinel magic value used to detect the vanilla spechit overflow bug
/// (DEFAULT_SPECHIT_MAGIC in p_map.c = 0x01C09C98).
pub const DEFAULT_SPECHIT_MAGIC: c_uint = 0x01C09C98;

// All p_map globals are now exported from `room/src/doom/p_map.rs`.

// ---------------------------------------------------------------------------
// d_loop.rs — main game-loop state globals and constants
// ---------------------------------------------------------------------------

pub use crate::doom::d_loop::{gametic, offsetms, singletics, ticdup, BACKUPTICS};

// ---------------------------------------------------------------------------
// p_saveg.c — save-game serialization constants and globals
// ---------------------------------------------------------------------------

/// End-of-file marker byte written at the end of every save game
/// (SAVEGAME_EOF in p_saveg.c = 0x1d).
pub const SAVEGAME_EOF: u8 = 0x1d;
/// Length of the version string embedded in save-game headers
/// (VERSIONSIZE in p_saveg.c = 16).
pub const VERSIONSIZE: usize = 16;

extern "C" {
    /// Total number of bytes written to the current save-game stream.
    pub static mut savegamelength: c_int;
    /// True when a write error has occurred during save serialization.
    pub static mut savegame_error: c_int; // boolean
}

// ---------------------------------------------------------------------------
// p_enemy.c — enemy AI globals and constants
// ---------------------------------------------------------------------------

/// Angle turned per tic when pursuing a lost target
/// (TRACEANGLE in p_enemy.c = 0xc000000).
pub const TRACEANGLE: c_uint = 0xc000000;
/// Arc spread angle for the Mancubus fireball pattern
/// (FATSPREAD = ANG90 / 8).
pub const FATSPREAD: c_uint = ANG90 / 8;
/// Speed of the Lost Soul charge attack in fixed-point units/tic
/// (SKULLSPEED = 20 × FRACUNIT).
pub const SKULLSPEED: c_int = 20 * FRACUNIT;

extern "C" {
    /// Per-direction X velocity multipliers for 8-directional monster movement.
    /// Values: {FRACUNIT, 47000, 0, -47000, -FRACUNIT, -47000, 0, 47000}.
    pub static mut xspeed: [c_int; 8];
    /// Per-direction Y velocity multipliers for 8-directional monster movement.
    /// Values: {0, 47000, FRACUNIT, 47000, 0, -47000, -FRACUNIT, -47000}.
    pub static mut yspeed: [c_int; 8];
    /// Array of potential brain-teleport destinations (Spider Mastermind).
    pub static mut braintargets: [*mut c_void; 32];
    /// Number of valid braintargets entries (set by P_SpawnBrainTargets).
    pub static mut numbraintargets: c_int;
    /// Index of the next braintarget to use in the round-robin sequence.
    pub static mut braintargeton: c_int;
}

// ---------------------------------------------------------------------------
// d_main.rs — startup flag defaults and global state.
// Re-exported from the Rust port so C tests and remaining FFI consumers can
// continue to access everything through one module.
// ---------------------------------------------------------------------------

pub use crate::doom::d_main::{
    advancedemo, autostart, bfgedition, devparm, fastparm, iwadfile, main_loop_started, nomonsters,
    respawnparm, savegamedir, show_endoom, startepisode, startmap, storedemo,
};

// ---------------------------------------------------------------------------
// wi_stuff.c — intermission screen constants
// ---------------------------------------------------------------------------

/// Number of episodes (NUMEPISODES in wi_stuff.c = 4).
pub const WI_NUMEPISODES: usize = 4;
/// Number of maps per episode (NUMMAPS in wi_stuff.c = 9).
pub const WI_NUMMAPS: usize = 9;
/// Y-position of the level-name title on the intermission screen (WI_TITLEY = 2).
pub const WI_TITLEY: c_int = 2;
/// Vertical spacing between player stats rows (WI_SPACINGY = 33).
pub const WI_SPACINGY: c_int = 33;
/// X-position of single-player stats (SP_STATSX = 50).
pub const SP_STATSX: c_int = 50;
/// Y-position of single-player stats (SP_STATSY = 50).
pub const SP_STATSY: c_int = 50;
/// X-position of the time display (SP_TIMEX = 16).
pub const SP_TIMEX: c_int = 16;
/// Y-position of the time display (SP_TIMEY = SCREENHEIGHT − 32).
pub const SP_TIMEY: c_int = SCREENHEIGHT - 32;
/// Delay in tics before showing the "next level" location (SHOWNEXTLOCDELAY = 4).
pub const SHOWNEXTLOCDELAY: c_int = 4;
/// X spacing between deathmatch matrix columns (DM_SPACINGX = 40).
pub const DM_SPACINGX: c_int = 40;
/// Number of animated elements in episode 0 background (EPSD0ANIMINFO length).
pub const WI_EPSD0_NANIM: usize = crate::doom::wi_stuff::EPSD0_NANIM;
/// Number of animated elements in episode 1 background (EPSD1ANIMINFO length).
pub const WI_EPSD1_NANIM: usize = crate::doom::wi_stuff::EPSD1_NANIM;
/// Number of animated elements in episode 2 background (EPSD2ANIMINFO length).
pub const WI_EPSD2_NANIM: usize = crate::doom::wi_stuff::EPSD2_NANIM;

// ---------------------------------------------------------------------------
// st_stuff.c — status bar constants
// ---------------------------------------------------------------------------

/// Palette index at which red (pain) palette rotations begin (STARTREDPALS = 1).
pub const STARTREDPALS: c_int = 1;
/// Palette index at which bonus (item pick-up) rotations begin (STARTBONUSPALS = 9).
pub const STARTBONUSPALS: c_int = 9;
/// Number of red pain palette entries (NUMREDPALS = 8).
pub const NUMREDPALS: c_int = 8;
/// Number of bonus pick-up palette entries (NUMBONUSPALS = 4).
pub const NUMBONUSPALS: c_int = 4;
/// Palette index used for the radiation suit overlay (RADIATIONPAL = 13).
pub const RADIATIONPAL: c_int = 13;
/// 1-in-N probability of a random face change per tic (ST_FACEPROBABILITY = 96).
pub const ST_FACEPROBABILITY: c_int = 96;
/// Number of pain-level face groups (ST_NUMPAINFACES = 5).
pub const ST_NUMPAINFACES: c_int = 5;
/// Number of straight (non-turning) faces per pain group (ST_NUMSTRAIGHTFACES = 3).
pub const ST_NUMSTRAIGHTFACES: c_int = 3;
/// Number of turning face variants per pain group (ST_NUMTURNFACES = 2).
pub const ST_NUMTURNFACES: c_int = 2;
/// Number of special face frames (ST_NUMSPECIALFACES = 3: ouch, evil-grin, rampage).
pub const ST_NUMSPECIALFACES: c_int = 3;
/// Number of extra face frames beyond the pain table (ST_NUMEXTRAFACES = 2: god, dead).
pub const ST_NUMEXTRAFACES: c_int = 2;
/// Total face sprite count = pain × stride + extra.
pub const ST_NUMFACES: c_int = ST_NUMPAINFACES
    * (ST_NUMSTRAIGHTFACES + ST_NUMTURNFACES + ST_NUMSPECIALFACES)
    + ST_NUMEXTRAFACES;
/// Tics of evil-grin display (ST_EVILGRINCOUNT = 2 × TICRATE).
pub const ST_EVILGRINCOUNT: c_int = 2 * TICRATE;
/// Tics of straight-face display (ST_STRAIGHTFACECOUNT = TICRATE / 2).
pub const ST_STRAIGHTFACECOUNT: c_int = TICRATE / 2;
/// Tics before turn-face times out (ST_TURNCOUNT = 1 × TICRATE).
pub const ST_TURNCOUNT: c_int = TICRATE;
/// Tics of ouch-face display (ST_OUCHCOUNT = 1 × TICRATE).
pub const ST_OUCHCOUNT: c_int = TICRATE;
/// Tics of rampage-face display (ST_RAMPAGEDELAY = 2 × TICRATE).
pub const ST_RAMPAGEDELAY: c_int = 2 * TICRATE;
/// Damage threshold for the rampage face (ST_MUCHPAIN = 20).
pub const ST_MUCHPAIN: c_int = 20;
/// X pixel offset of the status bar (ST_X = 0).
pub const ST_X: c_int = 0;
/// X pixel offset of the arms display (ST_X2 = 104).
pub const ST_X2: c_int = 104;

// ---------------------------------------------------------------------------
// Re-exports of ported globals so C tests and remaining FFI consumers can
// continue to access everything through one module.
// ---------------------------------------------------------------------------

pub use crate::doom::p_tick::leveltime;

pub use crate::doom::p_pspr::{swingx, swingy};

pub use crate::doom::r_draw::{
    dc_colormap, dc_iscale, dc_source, dc_texturemid, dc_x, dc_yh, dc_yl, ds_x1, ds_x2, ds_xfrac,
    ds_xstep, ds_y, ds_yfrac, ds_ystep, fuzzoffset, fuzzpos, scaledviewwidth, viewheight,
    viewwidth, viewwindowx, viewwindowy,
};

pub use crate::doom::r_segs::{
    bottomfrac, bottomstep, bottomtexture, markceiling, markfloor, maskedtexture, maskedtexturecol,
    midtexture, pixhigh, pixhighstep, pixlow, pixlowstep, rw_angle1, rw_bottomtexturemid,
    rw_centerangle, rw_distance, rw_midtexturemid, rw_normalangle, rw_offset, rw_scale,
    rw_scalestep, rw_stopx, rw_toptexturemid, rw_x, segtextured, topfrac, topstep, toptexture,
    walllights, worldbottom, worldhigh, worldlow, worldtop,
};

pub use crate::doom::r_things::{
    maxframe, negonearray, numsprites, pspriteiscale, pspritescale, screenheightarray,
    spritelights, spritename, sprites, sprtemp,
};
