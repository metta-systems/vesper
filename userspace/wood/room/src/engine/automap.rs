//! Rust port of vendor/doomgeneric/am_map.c.
//!
//! The fullscreen automap: player arrow, wall lines, grid, zoom/pan,
//! mark points, and crosshair.
//!
//! The automap renders a top-down view of the level geometry directly into the
//! video framebuffer.  It maintains its own coordinate system: "map" units
//! (fixed-point, same scale as the game world) and "frame" units (screen
//! pixels within the automap window).  Two conversion scale factors,
//! `scale_mtof` (map-to-frame) and `scale_ftom` (frame-to-map), govern the
//! zoom level and are updated by `AM_changeWindowScale`.
//!
//! Notable Rust-vs-C differences:
//! - The C file-scope variables live in [`AutomapState`], held in the
//!   [`AUTOMAP`] global; the internal functions take the state as an
//!   argument.
//! - Coordinate-conversion macros (`FTOM`, `MTOF`, `CXMTOF`, `CYMTOF`) are
//!   inline functions of the state rather than C preprocessor macros.
//! - The Cohen-Sutherland clip loop in `AM_clipMline` uses Rust integer
//!   arithmetic; the OC_* outcode constants are typed `i32`.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ptr;

use crate::{
    c_write,
    engine::{
        dehacked::DEH_String,
        doom::{event::event_t, player::PlayerId},
        game::{GAME, deathmatch, gameepisode, gamemap, netgame, playeringame},
        global::Global,
        interface::video::{SCREENHEIGHT, SCREENWIDTH, video_buffer},
        math::{
            fixed::{FRACBITS, FRACUNIT, FixedDiv, FixedMul, fixed_t},
            tables::{ANGLETOFINESHIFT, finecosine, finesine},
        },
        misc::{
            cheat::{cheatseq_t, cht_CheckCheat},
            controls::{CONTROLS, ControlsState},
        },
        play::{maputl::MAPBLOCKUNITS, setup::LevelState},
        render_2d::{V_DrawPatch, V_MarkRect},
        status::stuff::ST_Responder,
        wad::{
            W_ReleaseLumpName, cache_patch_name,
            format::{LinedefFlag, Patch},
        },
        world::World,
    },
};

use crate::engine::zone::PU_STATIC;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of player-placed mark points on the automap.
///
/// C origin: `AM_NUMMARKPOINTS` in am_map.c.
pub const AM_NUMMARKPOINTS: usize = 10;

/// Initial map-to-frame scale factor (approximately 0.2 in fixed-point).
///
/// The automap starts zoomed out to show roughly 20 % of the map height per
/// screen height.  C origin: `INITSCALEMTOF` in am_map.c.
pub const INITSCALEMTOF: i32 = (0.2 * FRACUNIT as f64) as i32;

/// Pan increment in screen pixels per tick when the player holds a pan key.
///
/// C origin: `F_PANINC` in am_map.c.
pub const F_PANINC: i32 = 4;

/// Scale multiplier applied to `scale_mtof` each tick while zooming in.
///
/// 1.02 in fixed-point; chosen to give a smooth zoom feel.
/// C origin: `M_ZOOMIN` in am_map.c.
pub const M_ZOOMIN: i32 = (1.02 * FRACUNIT as f64) as i32;

/// Scale multiplier applied to `scale_mtof` each tick while zooming out.
///
/// Reciprocal of [`M_ZOOMIN`] in fixed-point.
/// C origin: `M_ZOOMOUT` in am_map.c.
pub const M_ZOOMOUT: i32 = (FRACUNIT as f64 / 1.02) as i32;

// Automap messages (C `AMSTR_*` from `d_englsh.h`).
/// Shown when follow mode is switched on.
const AMSTR_FOLLOWON: &str = "Follow Mode ON";
/// Shown when follow mode is switched off.
const AMSTR_FOLLOWOFF: &str = "Follow Mode OFF";
/// Shown when the grid is switched on.
const AMSTR_GRIDON: &str = "Grid ON";
/// Shown when the grid is switched off.
const AMSTR_GRIDOFF: &str = "Grid OFF";
/// Prefix of the message shown when a mark is placed.
const AMSTR_MARKEDSPOT: &str = "Marked Spot";
/// Shown when all marks are cleared.
const AMSTR_MARKSCLEARED: &str = "All Marks Cleared";

/// Half-diameter of the player object in map units, used for arrow scaling.
///
/// C origin: `PLAYERRADIUS` in am_map.c.
const PLAYERRADIUS: i32 = 16 * FRACUNIT;

// Colour palette indices.

/// Starting palette index of the red colour range.
const REDS: i32 = 256 - 5 * 16;
/// Number of palette entries in the red range.
const REDRANGE: i32 = 16;
/// Starting palette index of the blue colour range.
const BLUES: i32 = 256 - 4 * 16 + 8;
/// Number of palette entries in the blue range.
const BLUERANGE: i32 = 8;
/// Starting palette index of the green colour range.
const GREENS: i32 = 7 * 16;
/// Number of palette entries in the green range.
const GREENRANGE: i32 = 16;
/// Starting palette index of the grey colour range.
const GRAYS: i32 = 6 * 16;
/// Number of palette entries in the grey range.
const GRAYSRANGE: i32 = 16;
/// Starting palette index of the brown colour range.
const BROWNS: i32 = 4 * 16;
/// Number of palette entries in the brown range.
const BROWNRANGE: i32 = 16;
/// Starting palette index of the yellow colour range.
const YELLOWS: i32 = 256 - 32 + 7;
/// Number of palette entries in the yellow range.
const YELLOWRANGE: i32 = 1;
/// Palette index for black (the automap background).
const BLACK: i32 = 0;
/// Palette index for white (the player arrow in single-player).
const WHITE: i32 = 256 - 47;

// Automap colour assignments.

/// Background fill colour index.
const BACKGROUND: i32 = BLACK;
/// Colour used for solid (one-sided) walls.
const WALLCOLORS: i32 = REDS;
/// Colour range width for solid walls.
const WALLRANGE: i32 = REDRANGE;
/// Colour used for two-sided walls where both sides have the same floor and
/// ceiling height (transparent / passable walls).
const TSWALLCOLORS: i32 = GRAYS;
/// Colour range width for transparent walls.
const TSWALLRANGE: i32 = GRAYSRANGE;
/// Colour used for floor-height-change linedefs.
const FDWALLCOLORS: i32 = BROWNS;
/// Colour range width for floor-height-change walls.
const FDWALLRANGE: i32 = BROWNRANGE;
/// Colour used for ceiling-height-change linedefs.
const CDWALLCOLORS: i32 = YELLOWS;
/// Colour range width for ceiling-height-change walls.
const CDWALLRANGE: i32 = YELLOWRANGE;
/// Colour used for thing triangles.
const THINGCOLORS: i32 = GREENS;
/// Colour range width for thing triangles.
const THINGRANGE: i32 = GREENRANGE;
/// Colour used for secret walls (same as solid walls; they look identical
/// unless cheating).
const SECRETWALLCOLORS: i32 = WALLCOLORS;
/// Colour range width for secret walls.
const SECRETWALLRANGE: i32 = WALLRANGE;
/// Colour used for the background grid.
const GRIDCOLORS: i32 = GRAYS + GRAYSRANGE / 2;
/// Colour used for the central crosshair dot.
const XHAIRCOLORS: i32 = GRAYS;

/// Magic header for automap event messages sent to `ST_Responder`.
///
/// The upper bytes encode `'a'` and `'m'`; the lower bytes identify the
/// specific sub-message.  C origin: `AM_MSGHEADER` in am_map.c.
const AM_MSGHEADER: i32 = (('a' as i32) << 24) + (('m' as i32) << 16);

/// Event data value sent to `ST_Responder` when the automap is opened.
///
/// C origin: `AM_MSGENTERED` in am_map.c.
const AM_MSGENTERED: i32 = AM_MSGHEADER | (('e' as i32) << 8);

/// Event data value sent to `ST_Responder` when the automap is closed.
///
/// C origin: `AM_MSGEXITED` in am_map.c.
const AM_MSGEXITED: i32 = AM_MSGHEADER | (('x' as i32) << 8);

// ---------------------------------------------------------------------------
// C globals still provided by unported C modules
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

/// A 2-D point in frame (screen-pixel) coordinates.
///
/// C origin: `fpoint_t` in am_map.c.
#[derive(Clone, Copy)]
struct fpoint_t {
    x: i32,
    y: i32,
}

/// A line segment in frame (screen-pixel) coordinates.
///
/// C origin: `fline_t` in am_map.c.
#[derive(Clone, Copy)]
struct fline_t {
    a: fpoint_t,
    b: fpoint_t,
}

/// A 2-D point in map (fixed-point world) coordinates.
///
/// C origin: `mpoint_t` in am_map.c.
#[derive(Clone, Copy)]
struct mpoint_t {
    x: fixed_t,
    y: fixed_t,
}

/// A line segment in map (fixed-point world) coordinates.
///
/// C origin: `mline_t` in am_map.c.
#[derive(Clone, Copy)]
struct mline_t {
    a: mpoint_t,
    b: mpoint_t,
}

/// Reciprocal-slope pair used by the (unused) slope clipping path.
///
/// `slp` is `dy/dx`; `islp` is `dx/dy`, both in fixed-point.
/// C origin: `islope_t` in am_map.c.
#[derive(Clone, Copy)]
struct islope_t {
    slp: fixed_t,
    islp: fixed_t,
}

// ---------------------------------------------------------------------------
// Player-arrow shape data
// ---------------------------------------------------------------------------

/// Base radius used to scale the player-arrow line segments.
///
/// Chosen as `(8/7) * PLAYERRADIUS` to give the arrow a slightly larger extent
/// than the collision radius.  C origin: `R` in am_map.c.
const R_ARROW: i32 = (8 * PLAYERRADIUS) / 7;

/// Line segments that form the normal (non-cheat) player arrow.
///
/// Defined in map coordinates centred on the origin; scaled and rotated by
/// [`AM_drawLineCharacter`] before drawing.  C origin: `player_arrow[]` in
/// am_map.c.
const PLAYER_ARROW: [mline_t; 7] = [
    mline_t {
        a: mpoint_t {
            x: -R_ARROW + R_ARROW / 8,
            y: 0,
        },
        b: mpoint_t { x: R_ARROW, y: 0 },
    },
    mline_t {
        a: mpoint_t { x: R_ARROW, y: 0 },
        b: mpoint_t {
            x: R_ARROW - R_ARROW / 2,
            y: R_ARROW / 4,
        },
    },
    mline_t {
        a: mpoint_t { x: R_ARROW, y: 0 },
        b: mpoint_t {
            x: R_ARROW - R_ARROW / 2,
            y: -R_ARROW / 4,
        },
    },
    mline_t {
        a: mpoint_t {
            x: -R_ARROW + R_ARROW / 8,
            y: 0,
        },
        b: mpoint_t {
            x: -R_ARROW - R_ARROW / 8,
            y: R_ARROW / 4,
        },
    },
    mline_t {
        a: mpoint_t {
            x: -R_ARROW + R_ARROW / 8,
            y: 0,
        },
        b: mpoint_t {
            x: -R_ARROW - R_ARROW / 8,
            y: -R_ARROW / 4,
        },
    },
    mline_t {
        a: mpoint_t {
            x: -R_ARROW + 3 * R_ARROW / 8,
            y: 0,
        },
        b: mpoint_t {
            x: -R_ARROW + R_ARROW / 8,
            y: R_ARROW / 4,
        },
    },
    mline_t {
        a: mpoint_t {
            x: -R_ARROW + 3 * R_ARROW / 8,
            y: 0,
        },
        b: mpoint_t {
            x: -R_ARROW + R_ARROW / 8,
            y: -R_ARROW / 4,
        },
    },
];

/// Line segments that form the cheat-mode player arrow (spells "DSGN" in the
/// tail).
///
/// Larger and more detailed than [`PLAYER_ARROW`]; displayed when `cheating`
/// is non-zero.  C origin: `cheat_player_arrow[]` in am_map.c.
const CHEAT_ARROW: [mline_t; 16] = [
    mline_t {
        a: mpoint_t {
            x: -R_ARROW + R_ARROW / 8,
            y: 0,
        },
        b: mpoint_t { x: R_ARROW, y: 0 },
    },
    mline_t {
        a: mpoint_t { x: R_ARROW, y: 0 },
        b: mpoint_t {
            x: R_ARROW - R_ARROW / 2,
            y: R_ARROW / 6,
        },
    },
    mline_t {
        a: mpoint_t { x: R_ARROW, y: 0 },
        b: mpoint_t {
            x: R_ARROW - R_ARROW / 2,
            y: -R_ARROW / 6,
        },
    },
    mline_t {
        a: mpoint_t {
            x: -R_ARROW + R_ARROW / 8,
            y: 0,
        },
        b: mpoint_t {
            x: -R_ARROW - R_ARROW / 8,
            y: R_ARROW / 6,
        },
    },
    mline_t {
        a: mpoint_t {
            x: -R_ARROW + R_ARROW / 8,
            y: 0,
        },
        b: mpoint_t {
            x: -R_ARROW - R_ARROW / 8,
            y: -R_ARROW / 6,
        },
    },
    mline_t {
        a: mpoint_t {
            x: -R_ARROW + 3 * R_ARROW / 8,
            y: 0,
        },
        b: mpoint_t {
            x: -R_ARROW + R_ARROW / 8,
            y: R_ARROW / 6,
        },
    },
    mline_t {
        a: mpoint_t {
            x: -R_ARROW + 3 * R_ARROW / 8,
            y: 0,
        },
        b: mpoint_t {
            x: -R_ARROW + R_ARROW / 8,
            y: -R_ARROW / 6,
        },
    },
    mline_t {
        a: mpoint_t {
            x: -R_ARROW / 2,
            y: 0,
        },
        b: mpoint_t {
            x: -R_ARROW / 2,
            y: -R_ARROW / 6,
        },
    },
    mline_t {
        a: mpoint_t {
            x: -R_ARROW / 2,
            y: -R_ARROW / 6,
        },
        b: mpoint_t {
            x: -R_ARROW / 2 + R_ARROW / 6,
            y: -R_ARROW / 6,
        },
    },
    mline_t {
        a: mpoint_t {
            x: -R_ARROW / 2 + R_ARROW / 6,
            y: -R_ARROW / 6,
        },
        b: mpoint_t {
            x: -R_ARROW / 2 + R_ARROW / 6,
            y: R_ARROW / 4,
        },
    },
    mline_t {
        a: mpoint_t {
            x: -R_ARROW / 6,
            y: 0,
        },
        b: mpoint_t {
            x: -R_ARROW / 6,
            y: -R_ARROW / 6,
        },
    },
    mline_t {
        a: mpoint_t {
            x: -R_ARROW / 6,
            y: -R_ARROW / 6,
        },
        b: mpoint_t {
            x: 0,
            y: -R_ARROW / 6,
        },
    },
    mline_t {
        a: mpoint_t {
            x: 0,
            y: -R_ARROW / 6,
        },
        b: mpoint_t {
            x: 0,
            y: R_ARROW / 4,
        },
    },
    mline_t {
        a: mpoint_t {
            x: R_ARROW / 6,
            y: R_ARROW / 4,
        },
        b: mpoint_t {
            x: R_ARROW / 6,
            y: -R_ARROW / 7,
        },
    },
    mline_t {
        a: mpoint_t {
            x: R_ARROW / 6,
            y: -R_ARROW / 7,
        },
        b: mpoint_t {
            x: R_ARROW / 6 + R_ARROW / 32,
            y: -R_ARROW / 7 - R_ARROW / 32,
        },
    },
    mline_t {
        a: mpoint_t {
            x: R_ARROW / 6 + R_ARROW / 32,
            y: -R_ARROW / 7 - R_ARROW / 32,
        },
        b: mpoint_t {
            x: R_ARROW / 6 + R_ARROW / 10,
            y: -R_ARROW / 7,
        },
    },
];

/// Equilateral-triangle shape used to represent non-player things in cheat mode.
///
/// Vertices at roughly ±0.867 and 0.5 FRACUNIT — an equilateral triangle.
/// C origin: `triangle_guy[]` in am_map.c.
const TRIANGLE_GUY: [mline_t; 3] = [
    mline_t {
        a: mpoint_t {
            x: (-0.867f64 * FRACUNIT as f64) as fixed_t,
            y: (-0.5f64 * FRACUNIT as f64) as fixed_t,
        },
        b: mpoint_t {
            x: (0.867f64 * FRACUNIT as f64) as fixed_t,
            y: (-0.5f64 * FRACUNIT as f64) as fixed_t,
        },
    },
    mline_t {
        a: mpoint_t {
            x: (0.867f64 * FRACUNIT as f64) as fixed_t,
            y: (-0.5f64 * FRACUNIT as f64) as fixed_t,
        },
        b: mpoint_t { x: 0, y: FRACUNIT },
    },
    mline_t {
        a: mpoint_t { x: 0, y: FRACUNIT },
        b: mpoint_t {
            x: (-0.867f64 * FRACUNIT as f64) as fixed_t,
            y: (-0.5f64 * FRACUNIT as f64) as fixed_t,
        },
    },
];

/// Thin isosceles triangle shape used to represent things in cheat mode.
///
/// A slender triangle pointing right; used for all things when `cheating == 2`.
/// C origin: `thintriangle_guy[]` in am_map.c.
const THINTRIANGLE_GUY: [mline_t; 3] = [
    mline_t {
        a: mpoint_t {
            x: (-0.5f64 * FRACUNIT as f64) as fixed_t,
            y: (-0.7f64 * FRACUNIT as f64) as fixed_t,
        },
        b: mpoint_t { x: FRACUNIT, y: 0 },
    },
    mline_t {
        a: mpoint_t { x: FRACUNIT, y: 0 },
        b: mpoint_t {
            x: (-0.5f64 * FRACUNIT as f64) as fixed_t,
            y: (0.7f64 * FRACUNIT as f64) as fixed_t,
        },
    },
    mline_t {
        a: mpoint_t {
            x: (-0.5f64 * FRACUNIT as f64) as fixed_t,
            y: (0.7f64 * FRACUNIT as f64) as fixed_t,
        },
        b: mpoint_t {
            x: (-0.5f64 * FRACUNIT as f64) as fixed_t,
            y: (-0.7f64 * FRACUNIT as f64) as fixed_t,
        },
    },
];

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

/// Width of the automap framebuffer window in pixels.
///
/// Initialised to `SCREENWIDTH`; the automap always fills the full screen
/// width.  C origin: `finit_width` in am_map.c.
const finit_width: i32 = SCREENWIDTH;

/// Height of the automap framebuffer window in pixels.
///
/// `SCREENHEIGHT - 32` to leave room for the status bar.
/// C origin: `finit_height` in am_map.c.
const finit_height: i32 = SCREENHEIGHT - 32;

/// Automap state (C `am_map.c` globals).
pub struct AutomapState {
    /// Non-zero while the automap is active and being drawn.
    ///
    /// Other modules read it through [`automap_active`] and clear it when
    /// they leave the level view.  C origin: `automapactive`.
    pub automapactive: i32,
    /// Current cheat level: 0 = none, 1 = show all walls, 2 = show all +
    /// things.
    ///
    /// Cycles through 0-2 when the `iddt` cheat sequence is entered.
    /// C origin: `cheating`.
    cheating: i32,
    /// Non-zero when the background grid is enabled.
    ///
    /// Toggled by the `key_map_grid` key.  C origin: `grid`.
    grid: i32,
    /// Non-zero when the automap has just been initialised for a new level
    /// and `AM_LevelInit` has not yet run.  C origin: `leveljuststarted`.
    leveljuststarted: i32,
    /// Left edge of the automap window in screen pixels.  C origin: `f_x`.
    f_x: i32,
    /// Top edge of the automap window in screen pixels.  C origin: `f_y`.
    f_y: i32,
    /// Width of the automap window in screen pixels.  C origin: `f_w`.
    f_w: i32,
    /// Height of the automap window in screen pixels.  C origin: `f_h`.
    f_h: i32,
    /// Current light level used to offset wall colours (currently unused at
    /// runtime).  C origin: `lightlev`.
    lightlev: i32,
    /// Pointer to the automap's framebuffer region (same as `I_VideoBuffer`).
    ///
    /// Set in [`AM_initVariables`].  C origin: `fb`.
    fb: *mut u8,
    /// Tick counter incremented each game tick while the automap is active.
    ///
    /// Used to pace the (currently disabled) light-level animation.
    /// C origin: `amclock`.
    amclock: i32,
    /// Per-tick map-coordinate pan increment.
    ///
    /// Set to a non-zero value while a pan key is held; reset on key-up.
    /// C origin: `m_paninc`.
    m_paninc: mpoint_t,
    /// Fixed-point multiplier applied to `scale_mtof` each tick while
    /// zooming.
    ///
    /// Normally `FRACUNIT` (no zoom); set to [`M_ZOOMIN`] or [`M_ZOOMOUT`]
    /// while a zoom key is held.  C origin: `mtof_zoommul`.
    mtof_zoommul: fixed_t,
    /// Fixed-point multiplier applied to `scale_ftom` each tick while
    /// zooming.
    ///
    /// Reciprocal of `mtof_zoommul`.  C origin: `ftom_zoommul`.
    ftom_zoommul: fixed_t,
    /// Left edge of the map viewport in map coordinates.  C origin: `m_x`.
    m_x: fixed_t,
    /// Bottom edge of the map viewport in map coordinates.  C origin: `m_y`.
    m_y: fixed_t,
    /// Right edge of the map viewport in map coordinates (`m_x + m_w`).
    /// C origin: `m_x2`.
    m_x2: fixed_t,
    /// Top edge of the map viewport in map coordinates (`m_y + m_h`).
    /// C origin: `m_y2`.
    m_y2: fixed_t,
    /// Width of the map viewport in map coordinates.  C origin: `m_w`.
    m_w: fixed_t,
    /// Height of the map viewport in map coordinates.  C origin: `m_h`.
    m_h: fixed_t,
    /// Minimum x coordinate over all level vertices.  C origin: `min_x`.
    min_x: fixed_t,
    /// Minimum y coordinate over all level vertices.  C origin: `min_y`.
    min_y: fixed_t,
    /// Maximum x coordinate over all level vertices.  C origin: `max_x`.
    max_x: fixed_t,
    /// Maximum y coordinate over all level vertices.  C origin: `max_y`.
    max_y: fixed_t,
    /// Maximum viewport width in map coordinates (bounding-box width of the
    /// level).  C origin: `max_w`.
    max_w: fixed_t,
    /// Maximum viewport height in map coordinates (bounding-box height of
    /// the level).  C origin: `max_h`.
    max_h: fixed_t,
    /// Minimum viewport width: 2 * `PLAYERRADIUS` (prevents over-zoom).
    /// C origin: `min_w`.
    min_w: fixed_t,
    /// Minimum viewport height: 2 * `PLAYERRADIUS` (prevents over-zoom).
    /// C origin: `min_h`.
    min_h: fixed_t,
    /// Minimum allowed `scale_mtof` (most zoomed out to fit the whole
    /// level).  C origin: `min_scale_mtof`.
    min_scale_mtof: fixed_t,
    /// Maximum allowed `scale_mtof` (most zoomed in: 2 * `PLAYERRADIUS`
    /// fills the screen).  C origin: `max_scale_mtof`.
    max_scale_mtof: fixed_t,
    /// Saved map viewport width, used to restore after
    /// `AM_minOutWindowScale`.  C origin: `old_m_w`.
    old_m_w: fixed_t,
    /// Saved map viewport height.  C origin: `old_m_h`.
    old_m_h: fixed_t,
    /// Saved map viewport left edge.  C origin: `old_m_x`.
    old_m_x: fixed_t,
    /// Saved map viewport bottom edge.  C origin: `old_m_y`.
    old_m_y: fixed_t,
    /// Last recorded player position, used by [`AM_doFollowPlayer`] to
    /// detect movement.
    ///
    /// Set to `i32::MAX` to force an update on the next tick.
    /// C origin: `f_oldloc`.
    f_oldloc: mpoint_t,
    /// Map-to-frame scale factor (fixed-point pixels per map unit).
    ///
    /// Initialised to [`INITSCALEMTOF`]; adjusted by zoom operations.
    /// C origin: `scale_mtof`.
    scale_mtof: fixed_t,
    /// Frame-to-map scale factor; reciprocal of `scale_mtof` in
    /// fixed-point.  C origin: `scale_ftom`.
    scale_ftom: fixed_t,
    /// Pointer to the player structure being tracked by the automap.
    ///
    /// Set to the console player (or the first active player in a network
    /// game) in [`AM_initVariables`].  C origin: `plr`.
    plr: Option<PlayerId>,
    /// Cached patches for the ten mark-number glyphs (`AMMNUM0`-`AMMNUM9`),
    /// `None` while not loaded.
    ///
    /// Loaded from the WAD by [`AM_loadPics`] and released by
    /// [`AM_unloadPics`].  C origin: `marknums[]`.
    marknums: [Option<Patch<'static>>; AM_NUMMARKPOINTS],
    /// Map-coordinate positions of the player-placed mark points.
    ///
    /// An `x` value of `-1` indicates an unused slot.
    /// C origin: `markpoints[]`.
    markpoints: [mpoint_t; AM_NUMMARKPOINTS],
    /// Index of the next mark slot to fill, wrapping modulo
    /// `AM_NUMMARKPOINTS`.  C origin: `markpointnum`.
    markpointnum: i32,
    /// Non-zero when the automap camera should track the player position.
    ///
    /// Set to 0 when the player pans the map manually; restored on
    /// `key_map_follow`.  C origin: `followplayer`.
    followplayer: i32,
    /// Cheat sequence for the automap reveal (`iddt`).  C origin:
    /// `cheat_amap`.
    cheat_amap: cheatseq_t,
    /// Non-zero when the automap is fully inactive (after `AM_Stop`).
    ///
    /// Prevents `AM_Stop` from running its shutdown logic more than once
    /// per open/close cycle.  C origin: `stopped`.
    stopped: i32,
    /// Map the level bounds were last computed for (static local
    /// `lastlevel` in C `AM_Start`).
    lastlevel: i32,
    /// Episode the level bounds were last computed for (static local
    /// `lastepisode` in C `AM_Start`).
    lastepisode: i32,
    /// Non-zero while the whole-level zoom is on (static local `bigstate`
    /// in C `AM_Responder`).
    bigstate: i32,
    /// Tick of the next light-level step (static local `nexttic` in C
    /// `AM_updateLightLev`).
    lightlev_nexttic: i32,
    /// Index into `LITELEVELS` (static local `litelevelscnt` in C
    /// `AM_updateLightLev`).
    litelevelscnt: i32,
    /// Count of lines `AM_drawFline` rejected as off-frame (static local
    /// debugging counter `fuck` in C).
    rejected_flines: i32,
}

impl AutomapState {
    /// The state at program start: automap closed, nothing loaded.
    pub const fn new() -> Self {
        Self {
            automapactive: 0,
            cheating: 0,
            grid: 0,
            leveljuststarted: 1,
            f_x: 0,
            f_y: 0,
            f_w: 0,
            f_h: 0,
            lightlev: 0,
            fb: ptr::null_mut(),
            amclock: 0,
            m_paninc: mpoint_t { x: 0, y: 0 },
            mtof_zoommul: FRACUNIT,
            ftom_zoommul: FRACUNIT,
            m_x: 0,
            m_y: 0,
            m_x2: 0,
            m_y2: 0,
            m_w: 0,
            m_h: 0,
            min_x: 0,
            min_y: 0,
            max_x: 0,
            max_y: 0,
            max_w: 0,
            max_h: 0,
            min_w: 0,
            min_h: 0,
            min_scale_mtof: 0,
            max_scale_mtof: 0,
            old_m_w: 0,
            old_m_h: 0,
            old_m_x: 0,
            old_m_y: 0,
            f_oldloc: mpoint_t { x: 0, y: 0 },
            scale_mtof: INITSCALEMTOF,
            scale_ftom: 0,
            plr: None,
            marknums: [None; AM_NUMMARKPOINTS],
            markpoints: [mpoint_t { x: -1, y: -1 }; AM_NUMMARKPOINTS],
            markpointnum: 0,
            followplayer: 1,
            cheat_amap: cheatseq_t {
                sequence: make_cheat_seq(b"iddt"),
                sequence_len: 4,
                parameter_chars: 0,
                chars_read: 0,
                param_chars_read: 0,
                parameter_buf: [0; 5],
            },
            stopped: 1,
            lastlevel: -1,
            lastepisode: -1,
            bigstate: 0,
            lightlev_nexttic: 0,
            litelevelscnt: 0,
            rejected_flines: 0,
        }
    }

    /// The player the automap follows (C `plr`).
    ///
    /// # Panics
    ///
    /// Before [`AM_initVariables`] has set it.
    #[track_caller]
    fn player(&self) -> PlayerId {
        self.plr.expect("automap has no player before AM_Start")
    }
}

impl Default for AutomapState {
    fn default() -> Self {
        Self::new()
    }
}

/// The automap state.
pub static AUTOMAP: Global<AutomapState> = Global::new(AutomapState::new());

/// Whether the automap is shown (C `automapactive != 0`).
pub fn automap_active() -> bool {
    AUTOMAP.borrow().automapactive != 0
}

/// Compile-time helper that copies a byte slice into a fixed-length `u8`
/// array, padding with zeros.
///
/// Used to initialise the `cheat_amap` `sequence` field in a `const`
/// context.
const fn make_cheat_seq(seq: &[u8]) -> [u8; 25] {
    let mut arr = [0u8; 25];
    let mut i = 0;
    while i < seq.len() {
        arr[i] = seq[i];
        i += 1;
    }
    arr
}

// ---------------------------------------------------------------------------
// Helper: FTOM / MTOF macros
// ---------------------------------------------------------------------------

/// Convert a frame (screen-pixel) distance to a map-coordinate distance.
///
/// Equivalent to the C macro `FTOM(x)` which expands to
/// `FixedMul((x) << FRACBITS, scale_ftom)`.
#[inline(always)]
fn FTOM(automap: &AutomapState, x: i32) -> fixed_t {
    FixedMul((x as fixed_t) << FRACBITS, automap.scale_ftom)
}

/// Convert a map-coordinate distance to a frame (screen-pixel) distance.
///
/// Equivalent to the C macro `MTOF(x)` which expands to
/// `FixedMul((x), scale_mtof) >> FRACBITS`.
#[inline(always)]
fn MTOF(automap: &AutomapState, x: fixed_t) -> i32 {
    (FixedMul(x, automap.scale_mtof) >> FRACBITS) as i32
}

/// Convert a map x-coordinate to a frame x-coordinate (absolute screen column).
///
/// Accounts for the current viewport origin `m_x` and the frame offset `f_x`.
/// Equivalent to the C macro `CXMTOF(x)`.
#[inline(always)]
fn CXMTOF(automap: &AutomapState, x: fixed_t) -> i32 {
    automap.f_x + MTOF(automap, x - automap.m_x)
}

/// Convert a map y-coordinate to a frame y-coordinate (absolute screen row).
///
/// Y is flipped: larger map y values correspond to smaller screen y values
/// (map north is screen up).  Equivalent to the C macro `CYMTOF(y)`.
#[inline(always)]
fn CYMTOF(automap: &AutomapState, y: fixed_t) -> i32 {
    automap.f_y + (automap.f_h - MTOF(automap, y - automap.m_y))
}

// ---------------------------------------------------------------------------
// Internal functions
// ---------------------------------------------------------------------------

/// Compute the forward and inverse slopes of the map line `ml`, storing results
/// in `*is`.
///
/// If `dy == 0` (horizontal line) the inverse slope is set to `±INT_MAX` to
/// avoid division by zero; likewise for `dx == 0`.  Used by the (currently
/// unused) slope-clipping code.
///
/// # Safety
///
/// `ml` and `is` must be valid non-null pointers.
unsafe fn AM_getIslope(ml: *mut mline_t, is: *mut islope_t) {
    let dy = (*ml).a.y - (*ml).b.y;
    let dx = (*ml).b.x - (*ml).a.x;
    if dy == 0 {
        (*is).islp = if dx < 0 { -i32::MAX } else { i32::MAX };
    } else {
        (*is).islp = FixedDiv(dx, dy);
    }
    if dx == 0 {
        (*is).slp = if dy < 0 { -i32::MAX } else { i32::MAX };
    } else {
        (*is).slp = FixedDiv(dy, dx);
    }
}

/// Recompute the map viewport dimensions after a zoom change.
///
/// Keeps the viewport centred on its previous midpoint and updates `m_x2` /
/// `m_y2`.  C origin: `AM_activateNewScale` in am_map.c.
///
/// Must only be called with the automap active.
fn AM_activateNewScale(automap: &mut AutomapState) {
    automap.m_x += automap.m_w / 2;
    automap.m_y += automap.m_h / 2;
    automap.m_w = FTOM(automap, automap.f_w);
    automap.m_h = FTOM(automap, automap.f_h);
    automap.m_x -= automap.m_w / 2;
    automap.m_y -= automap.m_h / 2;
    automap.m_x2 = automap.m_x + automap.m_w;
    automap.m_y2 = automap.m_y + automap.m_h;
}

/// Save the current viewport position and zoom level into the `old_m_*` fields.
///
/// Called before switching to min-zoom so that the previous view can be
/// restored.  C origin: `AM_saveScaleAndLoc` in am_map.c.
fn AM_saveScaleAndLoc(automap: &mut AutomapState) {
    automap.old_m_x = automap.m_x;
    automap.old_m_y = automap.m_y;
    automap.old_m_w = automap.m_w;
    automap.old_m_h = automap.m_h;
}

/// Restore the viewport position and zoom level from the `old_m_*` fields.
///
/// If follow mode is active, re-centres the viewport on the player rather than
/// restoring the saved origin.  Recalculates both scale factors.
/// C origin: `AM_restoreScaleAndLoc` in am_map.c.
///
/// # Panics
///
/// If follow mode is on and the player has no map object.
fn AM_restoreScaleAndLoc(automap: &mut AutomapState, world: &World) {
    automap.m_w = automap.old_m_w;
    automap.m_h = automap.old_m_h;
    if automap.followplayer == 0 {
        automap.m_x = automap.old_m_x;
        automap.m_y = automap.old_m_y;
    } else {
        let mo = &world.mobjs[world.players[automap.player()].mo()];
        automap.m_x = mo.x - automap.m_w / 2;
        automap.m_y = mo.y - automap.m_h / 2;
    }
    automap.m_x2 = automap.m_x + automap.m_w;
    automap.m_y2 = automap.m_y + automap.m_h;
    automap.scale_mtof = FixedDiv((automap.f_w as fixed_t) << FRACBITS, automap.m_w);
    automap.scale_ftom = FixedDiv(FRACUNIT, automap.scale_mtof);
}

/// Place a mark at the current viewport centre, advancing the mark slot index.
///
/// Marks wrap around after [`AM_NUMMARKPOINTS`] entries.
/// C origin: `AM_addMark` in am_map.c.
fn AM_addMark(automap: &mut AutomapState) {
    let slot = automap.markpointnum as usize;
    automap.markpoints[slot].x = automap.m_x + automap.m_w / 2;
    automap.markpoints[slot].y = automap.m_y + automap.m_h / 2;
    automap.markpointnum = (automap.markpointnum + 1) % AM_NUMMARKPOINTS as i32;
}

/// Compute the axis-aligned bounding box of all level vertices and derive the
/// min/max scale factors.
///
/// Sets `min_x`, `min_y`, `max_x`, `max_y`, `max_w`, `max_h`, `min_w`,
/// `min_h`, `min_scale_mtof`, and `max_scale_mtof`.
/// `min_scale_mtof` is the smaller of the x- and y-axis "fit whole level"
/// scales.  `max_scale_mtof` fits `2 * PLAYERRADIUS` within the frame height.
///
/// Reads `level`'s `vertexes` (from `P_LoadVertexes`).
///
/// C origin: `AM_findMinMaxBoundaries` in am_map.c.
fn AM_findMinMaxBoundaries(automap: &mut AutomapState, level: &LevelState) {
    automap.min_x = i32::MAX;
    automap.min_y = i32::MAX;
    automap.max_x = -i32::MAX;
    automap.max_y = -i32::MAX;

    for v in level.vertexes.iter() {
        if v.x < automap.min_x {
            automap.min_x = v.x;
        } else if v.x > automap.max_x {
            automap.max_x = v.x;
        }
        if v.y < automap.min_y {
            automap.min_y = v.y;
        } else if v.y > automap.max_y {
            automap.max_y = v.y;
        }
    }

    automap.max_w = automap.max_x - automap.min_x;
    automap.max_h = automap.max_y - automap.min_y;

    automap.min_w = 2 * PLAYERRADIUS;
    automap.min_h = 2 * PLAYERRADIUS;

    let a = FixedDiv((automap.f_w as fixed_t) << FRACBITS, automap.max_w);
    let b = FixedDiv((automap.f_h as fixed_t) << FRACBITS, automap.max_h);
    automap.min_scale_mtof = if a < b { a } else { b };
    automap.max_scale_mtof = FixedDiv((automap.f_h as fixed_t) << FRACBITS, 2 * PLAYERRADIUS);
}

/// Apply the pending pan increment and clamp the viewport to the level bounds.
///
/// If panning is active, disables follow mode (sets `followplayer = 0` and
/// invalidates `f_oldloc`).  The viewport centre is clamped so it cannot move
/// beyond the level bounding box.
///
/// C origin: `AM_changeWindowLoc` in am_map.c.
///
/// Must only be called with the automap active.
fn AM_changeWindowLoc(automap: &mut AutomapState) {
    if automap.m_paninc.x != 0 || automap.m_paninc.y != 0 {
        automap.followplayer = 0;
        automap.f_oldloc.x = i32::MAX;
    }

    automap.m_x += automap.m_paninc.x;
    automap.m_y += automap.m_paninc.y;

    if automap.m_x + automap.m_w / 2 > automap.max_x {
        automap.m_x = automap.max_x - automap.m_w / 2;
    } else if automap.m_x + automap.m_w / 2 < automap.min_x {
        automap.m_x = automap.min_x - automap.m_w / 2;
    }

    if automap.m_y + automap.m_h / 2 > automap.max_y {
        automap.m_y = automap.max_y - automap.m_h / 2;
    } else if automap.m_y + automap.m_h / 2 < automap.min_y {
        automap.m_y = automap.min_y - automap.m_h / 2;
    }

    automap.m_x2 = automap.m_x + automap.m_w;
    automap.m_y2 = automap.m_y + automap.m_h;
}

/// Initialise all automap variables for a new session.
///
/// Sets `automapactive = 1`, assigns the framebuffer pointer, resets clocks
/// and pan/zoom multipliers, selects the tracked player, and centres the
/// viewport on that player.  [`AM_Start`] then sends `AM_MSGENTERED` to the
/// status bar.
///
/// C origin: `AM_initVariables` in am_map.c.
///
/// # Panics
///
/// If the tracked player has no map object.
fn AM_initVariables(automap: &mut AutomapState, world: &World) {
    automap.automapactive = 1;
    automap.fb = video_buffer();

    automap.f_oldloc.x = i32::MAX;
    automap.amclock = 0;
    automap.lightlev = 0;

    automap.m_paninc.x = 0;
    automap.m_paninc.y = 0;
    automap.ftom_zoommul = FRACUNIT;
    automap.mtof_zoommul = FRACUNIT;

    automap.m_w = FTOM(automap, automap.f_w);
    automap.m_h = FTOM(automap, automap.f_h);

    let console = PlayerId::console();
    let plr = if playeringame(console.index()) != 0 {
        console
    } else {
        PlayerId::all()
            .find(|player| playeringame(player.index()) != 0)
            .unwrap_or(PlayerId::new(0))
    };
    automap.plr = Some(plr);

    {
        let mo = &world.mobjs[world.players[plr].mo()];
        automap.m_x = mo.x - automap.m_w / 2;
        automap.m_y = mo.y - automap.m_h / 2;
    }
    AM_changeWindowLoc(automap);

    automap.old_m_x = automap.m_x;
    automap.old_m_y = automap.m_y;
    automap.old_m_w = automap.m_w;
    automap.old_m_h = automap.m_h;
}

/// Send the automap message `event` to the status bar, passing on the
/// caller's `world` for its cheat checks.
///
/// Callers release their automap guard first: `ST_Responder` belongs to
/// another subsystem.
fn AM_notifyStatusBar(world: &mut World, event: event_t) {
    ST_Responder(world, &event);
}

/// Load the ten `AMMNUM0`-`AMMNUM9` mark-point glyph patches from the WAD.
///
/// Cached as `PU_STATIC` so they remain resident while the automap is open.
/// C origin: `AM_loadPics` in am_map.c.
fn AM_loadPics(automap: &mut AutomapState) {
    let mut namebuf: [u8; 9] = [0; 9];
    for i in 0..10i32 {
        c_write!(namebuf, "AMMNUM{}", i);
        // SAFETY: the lump stays at PU_STATIC until `AM_unloadPics`
        // releases it and clears the slot.
        automap.marknums[i as usize] = Some(unsafe { cache_patch_name(&namebuf, PU_STATIC) });
    }
}

/// Release the ten `AMMNUM*` mark-point glyph patches back to the WAD cache
/// and clear their slots.
///
/// Called by [`AM_Stop`] when the automap closes.
/// C origin: `AM_unloadPics` in am_map.c.
fn AM_unloadPics(automap: &mut AutomapState) {
    let mut namebuf: [u8; 9] = [0; 9];
    for i in 0..10i32 {
        c_write!(namebuf, "AMMNUM{}", i);
        W_ReleaseLumpName(&namebuf);
        automap.marknums[i as usize] = None;
    }
}

/// Reset all mark points: set each `x` field to `-1` and reset the slot index.
///
/// C origin: `AM_clearMarks` in am_map.c.
///
fn AM_clearMarks(automap: &mut AutomapState) {
    for i in 0..AM_NUMMARKPOINTS {
        automap.markpoints[i].x = -1;
    }
    automap.markpointnum = 0;
}

/// Perform per-level automap initialisation.
///
/// Clears marks, finds the level bounding box, and sets the initial scale
/// factor to show approximately 70 % of the minimum fit (clamped to the
/// maximum fit if that produces a smaller value).
///
/// Reads `level`'s vertexes.
///
/// C origin: `AM_LevelInit` in am_map.c.
fn AM_LevelInit(automap: &mut AutomapState, level: &LevelState) {
    automap.leveljuststarted = 0;

    automap.f_x = 0;
    automap.f_y = 0;
    automap.f_w = finit_width;
    automap.f_h = finit_height;

    AM_clearMarks(automap);
    AM_findMinMaxBoundaries(automap, level);

    automap.scale_mtof = FixedDiv(automap.min_scale_mtof, (0.7 * FRACUNIT as f64) as fixed_t);
    if automap.scale_mtof > automap.max_scale_mtof {
        automap.scale_mtof = automap.min_scale_mtof;
    }
    automap.scale_ftom = FixedDiv(FRACUNIT, automap.scale_mtof);
}

// ---------------------------------------------------------------------------
// Public functions
// ---------------------------------------------------------------------------

/// Deactivate the automap, release patch resources, and notify the status bar.
///
/// Sets `automapactive = 0`, sends `AM_MSGEXITED` to `ST_Responder`, and
/// unloads the mark-point patches.  Sets `stopped = 1` so that a subsequent
/// `AM_Start` will not call `AM_Stop` again.
///
/// Called by `G_DoCompleted`, `P_KillMobj` and the automap itself, with
/// the world they hold, which the status bar's responder receives.
/// C origin: `AM_Stop` in am_map.c.
///
/// Called only when the automap was previously started, so the mark-point
/// lumps it releases are cached.
pub fn AM_Stop(world: &mut World) {
    let st_notify = event_t {
        kind: 0,  // ev_keydown
        data1: 1, // ev_keyup
        data2: AM_MSGEXITED,
        data3: 0,
        data4: 0,
    };
    AM_unloadPics(&mut AUTOMAP.borrow_mut());
    AUTOMAP.borrow_mut().automapactive = 0;
    AM_notifyStatusBar(world, st_notify);
    AUTOMAP.borrow_mut().stopped = 1;
}

/// Activate the automap.
///
/// If the automap is already open (`stopped == 0`), closes it first.
/// Re-runs `AM_LevelInit` whenever the level or episode changes, then calls
/// `AM_initVariables` and `AM_loadPics`.
///
/// Called by [`AM_Responder`] when the toggle key is pressed, with the
/// world it was given.  C origin: `AM_Start` in am_map.c.
///
/// The game is in an active level: the console player's map object and
/// the level's map data in `world` are read.
pub fn AM_Start(world: &mut World) {
    if AUTOMAP.borrow().stopped == 0 {
        AM_Stop(world);
    }
    {
        // Released before the status bar is notified: its responder
        // belongs to another subsystem.
        let mut automap = AUTOMAP.borrow_mut();
        automap.stopped = 0;
        let (gamemap, gameepisode) = (gamemap(), gameepisode());
        if automap.lastlevel != gamemap || automap.lastepisode != gameepisode {
            AM_LevelInit(&mut automap, &world.level);
            automap.lastlevel = gamemap;
            automap.lastepisode = gameepisode;
        }
        AM_initVariables(&mut automap, world);
    }
    AM_notifyStatusBar(
        world,
        event_t {
            kind: 1, // ev_keyup
            data1: AM_MSGENTERED,
            data2: 0,
            data3: 0,
            data4: 0,
        },
    );
    AM_loadPics(&mut AUTOMAP.borrow_mut());
}

/// Set the zoom to the minimum scale (most zoomed out; fits the whole level).
///
/// C origin: `AM_minOutWindowScale` in am_map.c.
///
/// Must be called with the automap active and map boundaries already computed.
fn AM_minOutWindowScale(automap: &mut AutomapState) {
    automap.scale_mtof = automap.min_scale_mtof;
    automap.scale_ftom = FixedDiv(FRACUNIT, automap.scale_mtof);
    AM_activateNewScale(automap);
}

/// Set the zoom to the maximum scale (most zoomed in; `2 * PLAYERRADIUS` fills
/// the frame height).
///
/// C origin: `AM_maxOutWindowScale` in am_map.c.
///
/// Must be called with the automap active and map boundaries already computed.
fn AM_maxOutWindowScale(automap: &mut AutomapState) {
    automap.scale_mtof = automap.max_scale_mtof;
    automap.scale_ftom = FixedDiv(FRACUNIT, automap.scale_mtof);
    AM_activateNewScale(automap);
}

/// Process a keyboard/mouse event for the automap.
///
/// When the automap is closed: opens it on `key_map_toggle`.
/// When the automap is open and a key-down event arrives: handles pan, zoom,
/// toggle, follow mode, grid toggle, mark placement/clear, and the `iddt` cheat.
/// On key-up: stops ongoing pan and zoom.
///
/// Returns 1 if the event was consumed, 0 if it should be forwarded.
///
/// Called from `G_Responder` with the world it borrowed for the level
/// responders; the messages and the status bar notifications use it.
/// C origin: `AM_Responder` in am_map.c.
pub fn AM_Responder(world: &mut World, ev: &event_t) -> i32 {
    let mut rc: i32 = 0;
    let ControlsState {
        key_map_north,
        key_map_south,
        key_map_east,
        key_map_west,
        key_map_zoomin,
        key_map_zoomout,
        key_map_toggle,
        key_map_maxzoom,
        key_map_follow,
        key_map_grid,
        key_map_mark,
        key_map_clearmark,
        ..
    } = CONTROLS.get();

    if !automap_active() {
        if ev.kind == 0 && ev.data1 == key_map_toggle {
            // ev_keydown
            AM_Start(world);
            GAME.borrow_mut().viewactive = 0;
            rc = 1;
        }
    } else if ev.kind == 0 {
        // ev_keydown
        rc = 1;
        let key = ev.data1;
        let mut automap = AUTOMAP.borrow_mut();

        if key == key_map_east {
            if automap.followplayer == 0 {
                automap.m_paninc.x = FTOM(&automap, F_PANINC);
            } else {
                rc = 0;
            }
        } else if key == key_map_west {
            if automap.followplayer == 0 {
                automap.m_paninc.x = -FTOM(&automap, F_PANINC);
            } else {
                rc = 0;
            }
        } else if key == key_map_north {
            if automap.followplayer == 0 {
                automap.m_paninc.y = FTOM(&automap, F_PANINC);
            } else {
                rc = 0;
            }
        } else if key == key_map_south {
            if automap.followplayer == 0 {
                automap.m_paninc.y = -FTOM(&automap, F_PANINC);
            } else {
                rc = 0;
            }
        } else if key == key_map_zoomout {
            automap.mtof_zoommul = M_ZOOMOUT;
            automap.ftom_zoommul = M_ZOOMIN;
        } else if key == key_map_zoomin {
            automap.mtof_zoommul = M_ZOOMIN;
            automap.ftom_zoommul = M_ZOOMOUT;
        } else if key == key_map_toggle {
            automap.bigstate = 0;
            GAME.borrow_mut().viewactive = 1;
            drop(automap);
            AM_Stop(world);
            automap = AUTOMAP.borrow_mut();
        } else if key == key_map_maxzoom {
            automap.bigstate = !automap.bigstate;
            if automap.bigstate != 0 {
                AM_saveScaleAndLoc(&mut automap);
                AM_minOutWindowScale(&mut automap);
            } else {
                AM_restoreScaleAndLoc(&mut automap, world);
            }
        } else if key == key_map_follow {
            automap.followplayer = !automap.followplayer;
            automap.f_oldloc.x = i32::MAX;
            if automap.followplayer != 0 {
                set_player_message(world, &automap, DEH_String(AMSTR_FOLLOWON));
            } else {
                set_player_message(world, &automap, DEH_String(AMSTR_FOLLOWOFF));
            }
        } else if key == key_map_grid {
            automap.grid = !automap.grid;
            if automap.grid != 0 {
                set_player_message(world, &automap, DEH_String(AMSTR_GRIDON));
            } else {
                set_player_message(world, &automap, DEH_String(AMSTR_GRIDOFF));
            }
        } else if key == key_map_mark {
            set_player_message(
                world,
                &automap,
                format!("{} {}", DEH_String(AMSTR_MARKEDSPOT), automap.markpointnum),
            );
            AM_addMark(&mut automap);
        } else if key == key_map_clearmark {
            AM_clearMarks(&mut automap);
            set_player_message(world, &automap, DEH_String(AMSTR_MARKSCLEARED));
        } else {
            rc = 0;
        }

        if deathmatch() == 0 && cht_CheckCheat(&mut automap.cheat_amap, ev.data2 as u8) != 0 {
            rc = 0;
            automap.cheating = (automap.cheating + 1) % 3;
        }
    } else if ev.kind == 1 {
        // ev_keyup
        rc = 0;
        let key = ev.data1;
        let mut automap = AUTOMAP.borrow_mut();

        if key == key_map_east || key == key_map_west {
            if automap.followplayer == 0 {
                automap.m_paninc.x = 0;
            }
        } else if key == key_map_north || key == key_map_south {
            if automap.followplayer == 0 {
                automap.m_paninc.y = 0;
            }
        } else if key == key_map_zoomout || key == key_map_zoomin {
            automap.mtof_zoommul = FRACUNIT;
            automap.ftom_zoommul = FRACUNIT;
        }
    }

    rc
}

/// Set the HUD message of the player the automap shows in `world`.
fn set_player_message(world: &mut World, automap: &AutomapState, text: impl AsRef<[u8]>) {
    world.players[automap.player()].set_message(text);
}

/// Apply the current zoom multipliers and clamp to the allowed scale range.
///
/// If the new scale would fall below `min_scale_mtof`, snaps to minimum zoom.
/// If it exceeds `max_scale_mtof`, snaps to maximum zoom.  Otherwise calls
/// `AM_activateNewScale` to recompute the viewport dimensions.
///
/// C origin: `AM_changeWindowScale` in am_map.c.
///
/// Must only be called with the automap active.
fn AM_changeWindowScale(automap: &mut AutomapState) {
    automap.scale_mtof = FixedMul(automap.scale_mtof, automap.mtof_zoommul);
    automap.scale_ftom = FixedDiv(FRACUNIT, automap.scale_mtof);

    if automap.scale_mtof < automap.min_scale_mtof {
        AM_minOutWindowScale(automap);
    } else if automap.scale_mtof > automap.max_scale_mtof {
        AM_maxOutWindowScale(automap);
    } else {
        AM_activateNewScale(automap);
    }
}

/// Centre the automap viewport on the player if the player has moved.
///
/// Compares the player's current position against `f_oldloc`; if different,
/// recentres the viewport and updates `f_oldloc`.  The centre is snapped to
/// the nearest map unit that corresponds to an integer screen pixel in order
/// to reduce jitter (`FTOM(MTOF(pos))`).
///
/// C origin: `AM_doFollowPlayer` in am_map.c.
///
/// # Panics
///
/// If the player has no map object.
fn AM_doFollowPlayer(automap: &mut AutomapState, world: &World) {
    let mo = &world.mobjs[world.players[automap.player()].mo()];
    if automap.f_oldloc.x != mo.x || automap.f_oldloc.y != mo.y {
        automap.m_x = FTOM(automap, MTOF(automap, mo.x)) - automap.m_w / 2;
        automap.m_y = FTOM(automap, MTOF(automap, mo.y)) - automap.m_h / 2;
        automap.m_x2 = automap.m_x + automap.m_w;
        automap.m_y2 = automap.m_y + automap.m_h;
        automap.f_oldloc.x = mo.x;
        automap.f_oldloc.y = mo.y;
    }
}

/// Advance the `lightlev` animation to the next level in the table.
///
/// This function is compiled but currently disabled (call site is commented
/// out in [`AM_Ticker`]).  It steps through `LITELEVELS` every 6 ticks.
/// C origin: `AM_updateLightLev` in am_map.c.
///
/// Must only be called with the automap active.
#[allow(dead_code)]
fn AM_updateLightLev(automap: &mut AutomapState) {
    static LITELEVELS: [i32; 8] = [0, 4, 7, 10, 12, 14, 15, 15];

    if automap.amclock > automap.lightlev_nexttic {
        automap.lightlev = LITELEVELS[automap.litelevelscnt as usize];
        automap.litelevelscnt += 1;
        if automap.litelevelscnt == LITELEVELS.len() as i32 {
            automap.litelevelscnt = 0;
        }
        automap.lightlev_nexttic = automap.amclock + 6 - (automap.amclock % 6);
    }
}

/// Advance the automap state by one game tick.
///
/// Returns immediately if `automapactive == 0`.  Otherwise: increments
/// `amclock`, updates the follow-player position, applies zoom, and pans the
/// viewport.
///
/// Called from `g_game` once per game tick, with the world `G_Ticker`
/// borrowed for the level phase.
/// C origin: `AM_Ticker` in am_map.c.
///
/// # Panics
///
/// If the automap follows a player that has no map object.
pub fn AM_Ticker(world: &World) {
    let mut automap = AUTOMAP.borrow_mut();
    if automap.automapactive == 0 {
        return;
    }

    automap.amclock += 1;

    if automap.followplayer != 0 {
        AM_doFollowPlayer(&mut automap, world);
    }

    if automap.ftom_zoommul != FRACUNIT {
        AM_changeWindowScale(&mut automap);
    }

    if automap.m_paninc.x != 0 || automap.m_paninc.y != 0 {
        AM_changeWindowLoc(&mut automap);
    }

    // AM_updateLightLev(&mut automap);
}

/// Fill the automap framebuffer rectangle with `color`.
///
/// Uses `ptr::write_bytes` to set `f_w * f_h` bytes starting at `fb`.
/// C origin: `AM_clearFB` in am_map.c.
///
/// # Safety
///
/// `fb` must point to a buffer of at least `f_w * f_h` bytes.
unsafe fn AM_clearFB(automap: &AutomapState, color: i32) {
    std::ptr::write_bytes(
        automap.fb,
        color as u8,
        (automap.f_w * automap.f_h) as usize,
    );
}

// Cohen-Sutherland outcode constants.

/// Cohen-Sutherland outcode bit: point is to the left of the clip rectangle.
const OC_LEFT: i32 = 1;
/// Cohen-Sutherland outcode bit: point is to the right of the clip rectangle.
const OC_RIGHT: i32 = 2;
/// Cohen-Sutherland outcode bit: point is below the clip rectangle (y > f_h).
const OC_BOTTOM: i32 = 4;
/// Cohen-Sutherland outcode bit: point is above the clip rectangle (y < 0).
const OC_TOP: i32 = 8;

/// Compute the Cohen-Sutherland outcode for frame-coordinate point `(mx, my)`.
///
/// Returns a bitmask of [`OC_LEFT`], [`OC_RIGHT`], [`OC_TOP`], [`OC_BOTTOM`]
/// indicating which clip edges the point lies outside.
///
/// Reads the frame size `f_w` / `f_h`.
#[inline(always)]
fn dooutcode(automap: &AutomapState, mx: i32, my: i32) -> i32 {
    let mut oc = 0;
    if my < 0 {
        oc |= OC_TOP;
    } else if my >= automap.f_h {
        oc |= OC_BOTTOM;
    }
    if mx < 0 {
        oc |= OC_LEFT;
    } else if mx >= automap.f_w {
        oc |= OC_RIGHT;
    }
    oc
}

/// Clip map line `ml` to the current viewport and convert the result to frame
/// coordinates in `*fl`.
///
/// First performs a trivial reject in map coordinates (both endpoints outside
/// the same edge), then transforms to frame coordinates and applies a
/// Cohen-Sutherland iterative clip.  Returns 1 if the clipped segment is
/// visible, 0 if it was entirely clipped away.
///
/// C origin: `AM_clipMline` in am_map.c.
///
/// # Safety
///
/// `ml` and `fl` must be valid non-null pointers.
unsafe fn AM_clipMline(automap: &AutomapState, ml: *mut mline_t, fl: *mut fline_t) -> i32 {
    let (m_x, m_y, m_x2, m_y2) = (automap.m_x, automap.m_y, automap.m_x2, automap.m_y2);
    let (f_w, f_h) = (automap.f_w, automap.f_h);
    let mut outcode1: i32 = 0;
    let mut outcode2: i32 = 0;
    let mut outside: i32;
    let mut tmp: fpoint_t = fpoint_t { x: 0, y: 0 };
    let mut dx: i32;
    let mut dy: i32;

    // Trivial rejects and outcodes in map coords.
    if (*ml).a.y > m_y2 {
        outcode1 = OC_TOP;
    } else if (*ml).a.y < m_y {
        outcode1 = OC_BOTTOM;
    }
    if (*ml).b.y > m_y2 {
        outcode2 = OC_TOP;
    } else if (*ml).b.y < m_y {
        outcode2 = OC_BOTTOM;
    }
    if outcode1 & outcode2 != 0 {
        return 0;
    }

    if (*ml).a.x < m_x {
        outcode1 |= OC_LEFT;
    } else if (*ml).a.x > m_x2 {
        outcode1 |= OC_RIGHT;
    }
    if (*ml).b.x < m_x {
        outcode2 |= OC_LEFT;
    } else if (*ml).b.x > m_x2 {
        outcode2 |= OC_RIGHT;
    }
    if outcode1 & outcode2 != 0 {
        return 0;
    }

    // Transform to frame-buffer coordinates.
    (*fl).a.x = CXMTOF(automap, (*ml).a.x);
    (*fl).a.y = CYMTOF(automap, (*ml).a.y);
    (*fl).b.x = CXMTOF(automap, (*ml).b.x);
    (*fl).b.y = CYMTOF(automap, (*ml).b.y);

    outcode1 = dooutcode(automap, (*fl).a.x, (*fl).a.y);
    outcode2 = dooutcode(automap, (*fl).b.x, (*fl).b.y);

    if outcode1 & outcode2 != 0 {
        return 0;
    }

    while outcode1 | outcode2 != 0 {
        if outcode1 != 0 {
            outside = outcode1;
        } else {
            outside = outcode2;
        }

        if outside & OC_TOP != 0 {
            dy = (*fl).a.y - (*fl).b.y;
            dx = (*fl).b.x - (*fl).a.x;
            tmp.x = (*fl).a.x + (dx * (*fl).a.y) / dy;
            tmp.y = 0;
        } else if outside & OC_BOTTOM != 0 {
            dy = (*fl).a.y - (*fl).b.y;
            dx = (*fl).b.x - (*fl).a.x;
            tmp.x = (*fl).a.x + (dx * ((*fl).a.y - f_h)) / dy;
            tmp.y = f_h - 1;
        } else if outside & OC_RIGHT != 0 {
            dy = (*fl).b.y - (*fl).a.y;
            dx = (*fl).b.x - (*fl).a.x;
            tmp.y = (*fl).a.y + (dy * (f_w - 1 - (*fl).a.x)) / dx;
            tmp.x = f_w - 1;
        } else if outside & OC_LEFT != 0 {
            dy = (*fl).b.y - (*fl).a.y;
            dx = (*fl).b.x - (*fl).a.x;
            tmp.y = (*fl).a.y + (dy * (-(*fl).a.x)) / dx;
            tmp.x = 0;
        } else {
            tmp.x = 0;
            tmp.y = 0;
        }

        if outside == outcode1 {
            (*fl).a = tmp;
            outcode1 = dooutcode(automap, (*fl).a.x, (*fl).a.y);
        } else {
            (*fl).b = tmp;
            outcode2 = dooutcode(automap, (*fl).b.x, (*fl).b.y);
        }

        if outcode1 & outcode2 != 0 {
            return 0;
        }
    }

    1
}

/// Rasterise a frame-coordinate line segment `fl` into the framebuffer using
/// Bresenham's algorithm.
///
/// If either endpoint lies outside the frame bounds, the function increments a
/// debug counter (`rejected_flines`) and returns without drawing; this matches the C
/// behaviour and is intended as an assertion in debug builds.
///
/// C origin: `AM_drawFline` in am_map.c.
///
/// # Safety
///
/// `fl` must be a valid non-null pointer; `fb` must point to a buffer of at
/// least `f_w * f_h` bytes.
unsafe fn AM_drawFline(automap: &mut AutomapState, fl: *mut fline_t, color: i32) {
    let (fb, f_w, f_h) = (automap.fb, automap.f_w, automap.f_h);

    if (*fl).a.x < 0
        || (*fl).a.x >= f_w
        || (*fl).a.y < 0
        || (*fl).a.y >= f_h
        || (*fl).b.x < 0
        || (*fl).b.x >= f_w
        || (*fl).b.y < 0
        || (*fl).b.y >= f_h
    {
        // For debugging only — matches C behaviour of returning without drawing.
        automap.rejected_flines += 1;
        return;
    }

    let dx = (*fl).b.x - (*fl).a.x;
    let ax = 2 * (if dx < 0 { -dx } else { dx });
    let sx = if dx < 0 { -1 } else { 1 };

    let dy = (*fl).b.y - (*fl).a.y;
    let ay = 2 * (if dy < 0 { -dy } else { dy });
    let sy = if dy < 0 { -1 } else { 1 };

    let mut x = (*fl).a.x;
    let mut y = (*fl).a.y;

    if ax > ay {
        let mut d = ay - ax / 2;
        loop {
            *fb.add((y * f_w + x) as usize) = color as u8;
            if x == (*fl).b.x {
                return;
            }
            if d >= 0 {
                y += sy;
                d -= ax;
            }
            x += sx;
            d += ay;
        }
    } else {
        let mut d = ax - ay / 2;
        loop {
            *fb.add((y * f_w + x) as usize) = color as u8;
            if y == (*fl).b.y {
                return;
            }
            if d >= 0 {
                x += sx;
                d -= ay;
            }
            y += sy;
            d += ax;
        }
    }
}

/// Clip and draw map line `ml` in `color`.
///
/// Clips `ml` to the current viewport via [`AM_clipMline`]; if the result is
/// visible, rasterises it with [`AM_drawFline`].
///
/// C origin: `AM_drawMline` in am_map.c.
///
/// # Safety
///
/// `ml` must be a valid non-null pointer.
unsafe fn AM_drawMline(automap: &mut AutomapState, ml: *mut mline_t, color: i32) {
    // Scratch line; `AM_clipMline` writes all of it before a visible line is drawn.
    let mut fl = fline_t {
        a: fpoint_t { x: 0, y: 0 },
        b: fpoint_t { x: 0, y: 0 },
    };
    if AM_clipMline(automap, ml, &mut fl) != 0 {
        AM_drawFline(automap, &mut fl, color);
    }
}

/// Draw the blockmap-aligned background grid in `color`.
///
/// Grid lines are spaced `MAPBLOCKUNITS << FRACBITS` apart and are aligned to
/// the blockmap origin (`bmaporgx`, `bmaporgy`) so that the grid matches the
/// collision-detection grid.  Draws vertical lines first, then horizontal.
///
/// C origin: `AM_drawGrid` in am_map.c.
///
/// # Safety
///
/// The automap framebuffer must be valid. Reads `level`'s blockmap origin.
unsafe fn AM_drawGrid(automap: &mut AutomapState, level: &LevelState, color: i32) {
    let (m_x, m_y, m_w, m_h) = (automap.m_x, automap.m_y, automap.m_w, automap.m_h);
    let mut start: fixed_t;
    let mut end: fixed_t;
    let mut ml: mline_t = mline_t {
        a: mpoint_t { x: 0, y: 0 },
        b: mpoint_t { x: 0, y: 0 },
    };

    let (bmaporgx, bmaporgy) = level.blockmap_origin();

    // Vertical gridlines.
    start = m_x;
    if (start - bmaporgx) % (MAPBLOCKUNITS << FRACBITS) != 0 {
        start += (MAPBLOCKUNITS << FRACBITS) - ((start - bmaporgx) % (MAPBLOCKUNITS << FRACBITS));
    }
    end = m_x + m_w;

    ml.a.y = m_y;
    ml.b.y = m_y + m_h;
    let mut x = start;
    while x < end {
        ml.a.x = x;
        ml.b.x = x;
        AM_drawMline(automap, &mut ml, color);
        x += MAPBLOCKUNITS << FRACBITS;
    }

    // Horizontal gridlines.
    start = m_y;
    if (start - bmaporgy) % (MAPBLOCKUNITS << FRACBITS) != 0 {
        start += (MAPBLOCKUNITS << FRACBITS) - ((start - bmaporgy) % (MAPBLOCKUNITS << FRACBITS));
    }
    end = m_y + m_h;

    ml.a.x = m_x;
    ml.b.x = m_x + m_w;
    let mut y = start;
    while y < end {
        ml.a.y = y;
        ml.b.y = y;
        AM_drawMline(automap, &mut ml, color);
        y += MAPBLOCKUNITS << FRACBITS;
    }
}

/// Draw all visible level linedefs with appropriate colours.
///
/// Colour selection rules (in priority order):
/// - Lines with `MAPPED` flag or cheat mode on: drawn; unless `DONTDRAW` and
///   not cheating.
///   - One-sided (no backsector): `WALLCOLORS`.
///   - Special 39 (teleporter): mid-range red.
///   - `SECRET` flag: secret wall colour (red while not cheating; same when
///     cheating).
///   - Floor-height difference: `FDWALLCOLORS` (brown).
///   - Ceiling-height difference: `CDWALLCOLORS` (yellow).
///   - Otherwise cheating: `TSWALLCOLORS` (gray).
/// - Computer-area-map powerup (`powers[4]` / `pw_allmap`): gray (no
///   `DONTDRAW` lines).
///
/// C origin: `AM_drawWalls` in am_map.c.
///
/// # Safety
///
/// The level must be loaded.
unsafe fn AM_drawWalls(automap: &mut AutomapState, world: &World) {
    let (cheating, lightlev) = (automap.cheating, automap.lightlev);
    let allmap = world.players[automap.player()].powers[4] != 0;
    // Scratch line; every iteration sets all of it.
    let mut l = mline_t {
        a: mpoint_t { x: 0, y: 0 },
        b: mpoint_t { x: 0, y: 0 },
    };

    let level = &world.level;
    for li in &level.lines {
        let (v1, v2) = (level.vertexes[li.v1], level.vertexes[li.v2]);
        l.a.x = v1.x;
        l.a.y = v1.y;
        l.b.x = v2.x;
        l.b.y = v2.y;

        let flags = li.flags as i32;

        if cheating != 0 || (flags & LinedefFlag::MAPPED as i32) != 0 {
            if (flags & LinedefFlag::DONTDRAW as i32) != 0 && cheating == 0 {
                continue;
            }
            if let Some(back) = li.backsector {
                let back = &level.sectors[back];
                let front =
                    &level.sectors[li.frontsector.expect("two-sided line without front sector")];
                if li.special == 39 {
                    // teleporters
                    AM_drawMline(automap, &mut l, WALLCOLORS + WALLRANGE / 2);
                } else if (flags & LinedefFlag::SECRET as i32) != 0 {
                    if cheating != 0 {
                        AM_drawMline(automap, &mut l, SECRETWALLCOLORS + lightlev);
                    } else {
                        AM_drawMline(automap, &mut l, WALLCOLORS + lightlev);
                    }
                } else if back.floorheight != front.floorheight {
                    AM_drawMline(automap, &mut l, FDWALLCOLORS + lightlev);
                } else if back.ceilingheight != front.ceilingheight {
                    AM_drawMline(automap, &mut l, CDWALLCOLORS + lightlev);
                } else if cheating != 0 {
                    AM_drawMline(automap, &mut l, TSWALLCOLORS + lightlev);
                }
            } else {
                AM_drawMline(automap, &mut l, WALLCOLORS + lightlev);
            }
        } else if allmap {
            // pw_allmap
            if (flags & LinedefFlag::DONTDRAW as i32) == 0 {
                AM_drawMline(automap, &mut l, GRAYS + 3);
            }
        }
    }
}

/// Rotate map-coordinate vector `(*x, *y)` by angle `a`.
///
/// Uses the fine-angle lookup tables (`finecosine`, `finesine`) to apply a 2-D
/// rotation in fixed-point arithmetic.  `a` is a Doom angle (0 = east,
/// increasing counter-clockwise), shifted right by `ANGLETOFINESHIFT` to
/// index the lookup table.
///
/// C origin: `AM_rotate` in am_map.c.
///
/// # Safety
///
/// `x` and `y` must be valid non-null pointers; fine-angle tables must be
/// initialised.
unsafe fn AM_rotate(x: *mut fixed_t, y: *mut fixed_t, a: u32) {
    let tmpx = FixedMul(*x, *finecosine.0.add((a >> ANGLETOFINESHIFT) as usize))
        - FixedMul(*y, finesine[(a >> ANGLETOFINESHIFT) as usize]);
    *y = FixedMul(*x, finesine[(a >> ANGLETOFINESHIFT) as usize])
        + FixedMul(*y, *finecosine.0.add((a >> ANGLETOFINESHIFT) as usize));
    *x = tmpx;
}

/// Scale, rotate, translate, and draw a multi-segment line-character shape.
///
/// For each segment in `lineguy`: optionally scales each endpoint by `scale`
/// (if non-zero), optionally rotates by `angle` (if non-zero), then
/// translates to `(x, y)` and draws the resulting map line.  Used for player
/// arrows and thing triangles.
///
/// C origin: `AM_drawLineCharacter` in am_map.c (which takes the shape as a
/// pointer and a segment count).
///
/// # Safety
///
/// The automap framebuffer must be valid.
unsafe fn AM_drawLineCharacter(
    automap: &mut AutomapState,
    lineguy: &[mline_t],
    scale: fixed_t,
    angle: u32,
    color: i32,
    x: fixed_t,
    y: fixed_t,
) {
    let mut l: mline_t = mline_t {
        a: mpoint_t { x: 0, y: 0 },
        b: mpoint_t { x: 0, y: 0 },
    };

    for segment in lineguy {
        l.a.x = segment.a.x;
        l.a.y = segment.a.y;

        if scale != 0 {
            l.a.x = FixedMul(scale, l.a.x);
            l.a.y = FixedMul(scale, l.a.y);
        }

        if angle != 0 {
            AM_rotate(&mut l.a.x, &mut l.a.y, angle);
        }

        l.a.x += x;
        l.a.y += y;

        l.b.x = segment.b.x;
        l.b.y = segment.b.y;

        if scale != 0 {
            l.b.x = FixedMul(scale, l.b.x);
            l.b.y = FixedMul(scale, l.b.y);
        }

        if angle != 0 {
            AM_rotate(&mut l.b.x, &mut l.b.y, angle);
        }

        l.b.x += x;
        l.b.y += y;

        AM_drawMline(automap, &mut l, color);
    }
}

/// Draw player arrows for all active players.
///
/// In a non-network game, draws the console player's arrow (cheat arrow if
/// `cheating != 0`, normal arrow otherwise) in white.  In a network game,
/// draws each active player in a player-colour (green/grey/brown/red);
/// invisible players are drawn in colour 246.  In deathmatch outside a demo,
/// only the console player is drawn.
///
/// C origin: `AM_drawPlayers` in am_map.c.
///
/// # Safety
///
/// The `mo` of every player in the game must name a map object of the
/// level.
unsafe fn AM_drawPlayers(automap: &mut AutomapState, world: &World) {
    let plr = automap.player();
    let their_colors: [i32; 4] = [GREENS, GRAYS, BROWNS, REDS];
    let mut their_color: i32 = -1;
    let mut color: i32;

    if netgame() == 0 {
        let (angle, x, y) = {
            let mo = &world.mobjs[world.players[plr].mo()];
            (mo.angle, mo.x, mo.y)
        };
        let arrow: &[mline_t] = if automap.cheating != 0 {
            &CHEAT_ARROW
        } else {
            &PLAYER_ARROW
        };
        AM_drawLineCharacter(automap, arrow, 0, angle, WHITE, x, y);
        return;
    }

    for p in PlayerId::all() {
        their_color += 1;

        if deathmatch() != 0 && GAME.borrow().singledemo == 0 && p != plr {
            continue;
        }

        if playeringame(p.index()) == 0 {
            continue;
        }

        if world.players[p].powers[2] != 0 {
            // pw_invisibility
            color = 246;
        } else {
            color = their_colors[their_color as usize];
        }

        let (angle, x, y) = {
            let mo = &world.mobjs[world.players[p].mo()];
            (mo.angle, mo.x, mo.y)
        };
        AM_drawLineCharacter(automap, &PLAYER_ARROW, 0, angle, color, x, y);
    }
}

/// Draw thin-triangle icons for all things in every sector.
///
/// Iterates `sectors[0..numsectors]` and follows each sector's `thinglist`
/// linked list.  Each thing is drawn as a [`THINTRIANGLE_GUY`] scaled to
/// `16 << FRACBITS` map units.  Only used when `cheating == 2`.
///
/// C origin: `AM_drawThings` in am_map.c.
///
/// # Safety
///
/// The level must be loaded; thing linked lists must be properly
/// terminated.
unsafe fn AM_drawThings(automap: &mut AutomapState, world: &World, colors: i32, _colorrange: i32) {
    let level = &world.level;
    for sector in &level.sectors[..level.numsectors as usize] {
        let mut thing = sector.thinglist;
        while let Some(current) = thing {
            let (angle, x, y, snext) = {
                let t = &world.mobjs[current];
                (t.angle, t.x, t.y, t.snext)
            };
            let lightlev = automap.lightlev;
            AM_drawLineCharacter(
                automap,
                &THINTRIANGLE_GUY,
                16 << FRACBITS,
                angle,
                colors + lightlev,
                x,
                y,
            );
            thing = snext;
        }
    }
}

/// Draw the `AMMNUM*` glyph for each placed mark point.
///
/// Only draws marks whose `x` field is not `-1` and whose screen position lies
/// within the frame bounds (with a 5x6 pixel margin for the glyph size).
///
/// C origin: `AM_drawMarks` in am_map.c.
///
/// Panics if a mark is drawn while the `marknums` patches are not loaded by
/// [`AM_loadPics`].
fn AM_drawMarks(automap: &AutomapState) {
    for i in 0..AM_NUMMARKPOINTS {
        let mark = automap.markpoints[i];
        if mark.x != -1 {
            let w = 5;
            let h = 6;
            let fx = CXMTOF(automap, mark.x);
            let fy = CYMTOF(automap, mark.y);
            if fx >= automap.f_x
                && fx <= automap.f_w - w
                && fy >= automap.f_y
                && fy <= automap.f_h - h
            {
                let marknum = automap.marknums[i].expect("automap mark drawn before AM_loadPics");
                V_DrawPatch(fx, fy, marknum);
            }
        }
    }
}

/// Draw a single crosshair pixel at the centre of the automap frame.
///
/// Sets the pixel at `fb[f_w * (f_h + 1) / 2]` to `color`.
/// C origin: `AM_drawCrosshair` in am_map.c.
///
/// # Safety
///
/// `fb` must point to a buffer of at least `f_w * f_h` bytes; the computed
/// index must not overflow.
unsafe fn AM_drawCrosshair(automap: &AutomapState, color: i32) {
    *automap
        .fb
        .add(((automap.f_w * (automap.f_h + 1)) / 2) as usize) = color as u8;
}

/// Render the automap for the current frame.
///
/// Returns immediately if `automapactive == 0`.  Otherwise: clears the
/// framebuffer, optionally draws the grid, draws walls, players, things (if
/// `cheating == 2`), crosshair, and mark points, then marks the dirty
/// rectangle via `V_MarkRect`.
///
/// Called from `D_Display` once per frame, with a shared borrow of the
/// world it holds for this drawer; reads the players and the level from
/// `world`. C origin: `AM_Drawer` in am_map.c.
///
/// # Safety
///
/// All automap state must be valid (automap must be active with a level
/// loaded).
pub unsafe fn AM_Drawer(world: &World) {
    let mut automap = AUTOMAP.borrow_mut();
    if automap.automapactive == 0 {
        return;
    }

    AM_clearFB(&automap, BACKGROUND);
    if automap.grid != 0 {
        AM_drawGrid(&mut automap, &world.level, GRIDCOLORS);
    }
    AM_drawWalls(&mut automap, world);
    AM_drawPlayers(&mut automap, world);
    if automap.cheating == 2 {
        AM_drawThings(&mut automap, world, THINGCOLORS, THINGRANGE);
    }
    AM_drawCrosshair(&automap, XHAIRCOLORS);
    AM_drawMarks(&automap);

    V_MarkRect(automap.f_x, automap.f_y, automap.f_w, automap.f_h);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Marks go to the viewport centre and wrap after `AM_NUMMARKPOINTS`;
    /// clearing empties every slot and restarts at slot 0.
    #[test]
    fn marks_wrap_and_clear() {
        let mut automap = AutomapState::new();
        automap.m_x = 100;
        automap.m_y = 200;
        automap.m_w = 40;
        automap.m_h = 60;

        for _ in 0..AM_NUMMARKPOINTS + 1 {
            AM_addMark(&mut automap);
        }
        assert_eq!(automap.markpointnum, 1);
        assert_eq!(
            (automap.markpoints[0].x, automap.markpoints[0].y),
            (120, 230)
        );

        AM_clearMarks(&mut automap);
        assert_eq!(automap.markpointnum, 0);
        assert!(automap.markpoints.iter().all(|mark| mark.x == -1));
    }

    /// Panning turns off follow mode and the viewport centre stays inside the
    /// level bounds.
    #[test]
    fn window_pan_clamps_to_level_bounds() {
        let mut automap = AutomapState::new();
        automap.min_x = 0;
        automap.max_x = 1000;
        automap.min_y = 0;
        automap.max_y = 1000;
        automap.m_w = 100;
        automap.m_h = 100;
        automap.m_paninc = mpoint_t { x: 5000, y: -5000 };

        AM_changeWindowLoc(&mut automap);

        assert_eq!(automap.followplayer, 0);
        assert_eq!(automap.f_oldloc.x, i32::MAX);
        assert_eq!((automap.m_x, automap.m_y), (950, -50));
        assert_eq!((automap.m_x2, automap.m_y2), (1050, 50));
    }
}
