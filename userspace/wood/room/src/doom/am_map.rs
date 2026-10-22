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
//! - All globals use `static mut` with `unsafe` accessors instead of bare C
//!   file-scope variables.
//! - Coordinate-conversion macros (`FTOM`, `MTOF`, `CXMTOF`, `CYMTOF`) are
//!   `unsafe` inline functions rather than C preprocessor macros.
//! - The Cohen-Sutherland clip loop in `AM_clipMline` uses Rust integer
//!   arithmetic; the OC_* outcode constants are typed `c_int`.
//! - `AM_Map_Link_Anchor` is a Rust addition: it forces the linker to retain
//!   all exported symbols that would otherwise be dead-stripped.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::{c_char, c_int};
use std::os::raw::c_uint;
use std::ptr;

use crate::c_write;
use crate::doom::c_ffi::{mobj_t, sector_t, LinedefFlag, MAPBLOCKUNITS};
use crate::doom::d_event::event_t;
use crate::doom::d_player::{PlayerT, MAXPLAYERS};
use crate::doom::g_game::{
    consoleplayer, deathmatch, gameepisode, gamemap, netgame, playeringame, players, singledemo,
    viewactive,
};
use crate::doom::i_video::I_VideoBuffer;
use crate::doom::i_video::{SCREENHEIGHT, SCREENWIDTH};
use crate::doom::m_cheat::{cheatseq_t, cht_CheckCheat};
use crate::doom::m_controls::{
    key_map_clearmark, key_map_east, key_map_follow, key_map_grid, key_map_mark, key_map_maxzoom,
    key_map_north, key_map_south, key_map_toggle, key_map_west, key_map_zoomin, key_map_zoomout,
};
use crate::doom::m_fixed::{fixed_t, FixedDiv, FixedMul};
use crate::doom::m_fixed::{FRACBITS, FRACUNIT};
use crate::doom::p_setup::{
    bmaporgx, bmaporgy, lines, numlines, numsectors, numvertexes, sectors, vertexes,
};
use crate::doom::st_stuff::ST_Responder;
use crate::doom::tables::ANGLETOFINESHIFT;
use crate::doom::tables::{finecosine, finesine};
use crate::doom::v_video::{patch_t, V_DrawPatch, V_MarkRect};
use crate::doom::w_wad::{W_CacheLumpName, W_ReleaseLumpName};

use crate::doom::z_zone::PU_STATIC;

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
pub const INITSCALEMTOF: c_int = (0.2 * FRACUNIT as f64) as c_int;

/// Pan increment in screen pixels per tick when the player holds a pan key.
///
/// C origin: `F_PANINC` in am_map.c.
pub const F_PANINC: c_int = 4;

/// Scale multiplier applied to `scale_mtof` each tick while zooming in.
///
/// 1.02 in fixed-point; chosen to give a smooth zoom feel.
/// C origin: `M_ZOOMIN` in am_map.c.
pub const M_ZOOMIN: c_int = (1.02 * FRACUNIT as f64) as c_int;

/// Scale multiplier applied to `scale_mtof` each tick while zooming out.
///
/// Reciprocal of [`M_ZOOMIN`] in fixed-point.
/// C origin: `M_ZOOMOUT` in am_map.c.
pub const M_ZOOMOUT: c_int = (FRACUNIT as f64 / 1.02) as c_int;

/// Half-diameter of the player object in map units, used for arrow scaling.
///
/// C origin: `PLAYERRADIUS` in am_map.c.
const PLAYERRADIUS: c_int = 16 * FRACUNIT;

// Colour palette indices.

/// Starting palette index of the red colour range.
const REDS: c_int = 256 - 5 * 16;
/// Number of palette entries in the red range.
const REDRANGE: c_int = 16;
/// Starting palette index of the blue colour range.
const BLUES: c_int = 256 - 4 * 16 + 8;
/// Number of palette entries in the blue range.
const BLUERANGE: c_int = 8;
/// Starting palette index of the green colour range.
const GREENS: c_int = 7 * 16;
/// Number of palette entries in the green range.
const GREENRANGE: c_int = 16;
/// Starting palette index of the grey colour range.
const GRAYS: c_int = 6 * 16;
/// Number of palette entries in the grey range.
const GRAYSRANGE: c_int = 16;
/// Starting palette index of the brown colour range.
const BROWNS: c_int = 4 * 16;
/// Number of palette entries in the brown range.
const BROWNRANGE: c_int = 16;
/// Starting palette index of the yellow colour range.
const YELLOWS: c_int = 256 - 32 + 7;
/// Number of palette entries in the yellow range.
const YELLOWRANGE: c_int = 1;
/// Palette index for black (the automap background).
const BLACK: c_int = 0;
/// Palette index for white (the player arrow in single-player).
const WHITE: c_int = 256 - 47;

// Automap colour assignments.

/// Background fill colour index.
const BACKGROUND: c_int = BLACK;
/// Colour used for solid (one-sided) walls.
const WALLCOLORS: c_int = REDS;
/// Colour range width for solid walls.
const WALLRANGE: c_int = REDRANGE;
/// Colour used for two-sided walls where both sides have the same floor and
/// ceiling height (transparent / passable walls).
const TSWALLCOLORS: c_int = GRAYS;
/// Colour range width for transparent walls.
const TSWALLRANGE: c_int = GRAYSRANGE;
/// Colour used for floor-height-change linedefs.
const FDWALLCOLORS: c_int = BROWNS;
/// Colour range width for floor-height-change walls.
const FDWALLRANGE: c_int = BROWNRANGE;
/// Colour used for ceiling-height-change linedefs.
const CDWALLCOLORS: c_int = YELLOWS;
/// Colour range width for ceiling-height-change walls.
const CDWALLRANGE: c_int = YELLOWRANGE;
/// Colour used for thing triangles.
const THINGCOLORS: c_int = GREENS;
/// Colour range width for thing triangles.
const THINGRANGE: c_int = GREENRANGE;
/// Colour used for secret walls (same as solid walls; they look identical
/// unless cheating).
const SECRETWALLCOLORS: c_int = WALLCOLORS;
/// Colour range width for secret walls.
const SECRETWALLRANGE: c_int = WALLRANGE;
/// Colour used for the background grid.
const GRIDCOLORS: c_int = GRAYS + GRAYSRANGE / 2;
/// Colour used for the central crosshair dot.
const XHAIRCOLORS: c_int = GRAYS;

/// Magic header for automap event messages sent to `ST_Responder`.
///
/// The upper bytes encode `'a'` and `'m'`; the lower bytes identify the
/// specific sub-message.  C origin: `AM_MSGHEADER` in am_map.c.
const AM_MSGHEADER: c_int = (('a' as c_int) << 24) + (('m' as c_int) << 16);

/// Event data value sent to `ST_Responder` when the automap is opened.
///
/// C origin: `AM_MSGENTERED` in am_map.c.
const AM_MSGENTERED: c_int = AM_MSGHEADER | (('e' as c_int) << 8);

/// Event data value sent to `ST_Responder` when the automap is closed.
///
/// C origin: `AM_MSGEXITED` in am_map.c.
const AM_MSGEXITED: c_int = AM_MSGHEADER | (('x' as c_int) << 8);

/// Pass-through stub for DEH_String when dehacked patching is disabled.
///
/// In a dehacked build this would look up the string in a replacement table;
/// here it simply returns its argument unchanged.
/// C origin: `DEH_String` macro/function pattern used throughout Chocolate Doom.
#[inline(always)]
unsafe fn DEH_String(s: *mut c_char) -> *mut c_char {
    s
}

// ---------------------------------------------------------------------------
// C globals still provided by unported C modules
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

/// A 2-D point in frame (screen-pixel) coordinates.
///
/// C origin: `fpoint_t` in am_map.c.
#[repr(C)]
#[derive(Clone, Copy)]
struct fpoint_t {
    x: c_int,
    y: c_int,
}

/// A line segment in frame (screen-pixel) coordinates.
///
/// C origin: `fline_t` in am_map.c.
#[repr(C)]
#[derive(Clone, Copy)]
struct fline_t {
    a: fpoint_t,
    b: fpoint_t,
}

/// A 2-D point in map (fixed-point world) coordinates.
///
/// C origin: `mpoint_t` in am_map.c.
#[repr(C)]
#[derive(Clone, Copy)]
struct mpoint_t {
    x: fixed_t,
    y: fixed_t,
}

/// A line segment in map (fixed-point world) coordinates.
///
/// C origin: `mline_t` in am_map.c.
#[repr(C)]
#[derive(Clone, Copy)]
struct mline_t {
    a: mpoint_t,
    b: mpoint_t,
}

/// Reciprocal-slope pair used by the (unused) slope clipping path.
///
/// `slp` is `dy/dx`; `islp` is `dx/dy`, both in fixed-point.
/// C origin: `islope_t` in am_map.c.
#[repr(C)]
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
const R_ARROW: c_int = (8 * PLAYERRADIUS) / 7;

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

/// Current cheat level: 0 = none, 1 = show all walls, 2 = show all + things.
///
/// Cycles through 0-2 when the `iddt` cheat sequence is entered.
/// C origin: `cheating` in am_map.c.
static mut cheating: c_int = 0;

/// Non-zero when the background grid is enabled.
///
/// Toggled by the `key_map_grid` key.  C origin: `grid` in am_map.c.
static mut grid: c_int = 0;

/// Non-zero when the automap has just been initialised for a new level and
/// `AM_LevelInit` has not yet run.
///
/// C origin: `leveljuststarted` in am_map.c.
static mut leveljuststarted: c_int = 1;

/// Non-zero while the automap is active and being drawn.
///
/// Exported so that other modules (e.g. `f_finale.rs`) can read/clear it.
/// C origin: `automapactive` in am_map.c.
#[no_mangle]
pub static mut automapactive: c_int = 0;

/// Width of the automap framebuffer window in pixels.
///
/// Initialised to `SCREENWIDTH`; the automap always fills the full screen
/// width.  C origin: `finit_width` in am_map.c.
static mut finit_width: c_int = SCREENWIDTH;

/// Height of the automap framebuffer window in pixels.
///
/// `SCREENHEIGHT - 32` to leave room for the status bar.
/// C origin: `finit_height` in am_map.c.
static mut finit_height: c_int = SCREENHEIGHT - 32;

/// Left edge of the automap window in screen pixels.
///
/// C origin: `f_x` in am_map.c.
static mut f_x: c_int = 0;

/// Top edge of the automap window in screen pixels.
///
/// C origin: `f_y` in am_map.c.
static mut f_y: c_int = 0;

/// Width of the automap window in screen pixels.
///
/// C origin: `f_w` in am_map.c.
static mut f_w: c_int = 0;

/// Height of the automap window in screen pixels.
///
/// C origin: `f_h` in am_map.c.
static mut f_h: c_int = 0;

/// Current light level used to offset wall colours (currently unused at runtime).
///
/// C origin: `lightlev` in am_map.c.
static mut lightlev: c_int = 0;

/// Pointer to the automap's framebuffer region (same as `I_VideoBuffer`).
///
/// Set in [`AM_initVariables`].  C origin: `fb` in am_map.c.
static mut fb: *mut u8 = ptr::null_mut();

/// Tick counter incremented each game tick while the automap is active.
///
/// Used to pace the (currently disabled) light-level animation.
/// C origin: `amclock` in am_map.c.
static mut amclock: c_int = 0;

/// Per-tick map-coordinate pan increment.
///
/// Set to a non-zero value while a pan key is held; reset on key-up.
/// C origin: `m_paninc` in am_map.c.
static mut m_paninc: mpoint_t = mpoint_t { x: 0, y: 0 };

/// Fixed-point multiplier applied to `scale_mtof` each tick while zooming.
///
/// Normally `FRACUNIT` (no zoom); set to [`M_ZOOMIN`] or [`M_ZOOMOUT`] while
/// a zoom key is held.  C origin: `mtof_zoommul` in am_map.c.
static mut mtof_zoommul: fixed_t = FRACUNIT;

/// Fixed-point multiplier applied to `scale_ftom` each tick while zooming.
///
/// Reciprocal of [`mtof_zoommul`].  C origin: `ftom_zoommul` in am_map.c.
static mut ftom_zoommul: fixed_t = FRACUNIT;

/// Left edge of the map viewport in map coordinates.
///
/// C origin: `m_x` in am_map.c.
static mut m_x: fixed_t = 0;

/// Bottom edge of the map viewport in map coordinates.
///
/// C origin: `m_y` in am_map.c.
static mut m_y: fixed_t = 0;

/// Right edge of the map viewport in map coordinates (`m_x + m_w`).
///
/// C origin: `m_x2` in am_map.c.
static mut m_x2: fixed_t = 0;

/// Top edge of the map viewport in map coordinates (`m_y + m_h`).
///
/// C origin: `m_y2` in am_map.c.
static mut m_y2: fixed_t = 0;

/// Width of the map viewport in map coordinates.
///
/// C origin: `m_w` in am_map.c.
static mut m_w: fixed_t = 0;

/// Height of the map viewport in map coordinates.
///
/// C origin: `m_h` in am_map.c.
static mut m_h: fixed_t = 0;

/// Minimum x coordinate over all level vertices.
///
/// C origin: `min_x` in am_map.c.
static mut min_x: fixed_t = 0;

/// Minimum y coordinate over all level vertices.
///
/// C origin: `min_y` in am_map.c.
static mut min_y: fixed_t = 0;

/// Maximum x coordinate over all level vertices.
///
/// C origin: `max_x` in am_map.c.
static mut max_x: fixed_t = 0;

/// Maximum y coordinate over all level vertices.
///
/// C origin: `max_y` in am_map.c.
static mut max_y: fixed_t = 0;

/// Maximum viewport width in map coordinates (bounding-box width of the level).
///
/// C origin: `max_w` in am_map.c.
static mut max_w: fixed_t = 0;

/// Maximum viewport height in map coordinates (bounding-box height of the level).
///
/// C origin: `max_h` in am_map.c.
static mut max_h: fixed_t = 0;

/// Minimum viewport width: 2 * `PLAYERRADIUS` (prevents over-zoom).
///
/// C origin: `min_w` in am_map.c.
static mut min_w: fixed_t = 0;

/// Minimum viewport height: 2 * `PLAYERRADIUS` (prevents over-zoom).
///
/// C origin: `min_h` in am_map.c.
static mut min_h: fixed_t = 0;

/// Minimum allowed `scale_mtof` (most zoomed out to fit the whole level).
///
/// C origin: `min_scale_mtof` in am_map.c.
static mut min_scale_mtof: fixed_t = 0;

/// Maximum allowed `scale_mtof` (most zoomed in: 2 * `PLAYERRADIUS` fills the
/// screen).
///
/// C origin: `max_scale_mtof` in am_map.c.
static mut max_scale_mtof: fixed_t = 0;

/// Saved map viewport origin x, used to restore after `AM_minOutWindowScale`.
///
/// C origin: `old_m_w`, `old_m_h`, `old_m_x`, `old_m_y` in am_map.c.
static mut old_m_w: fixed_t = 0;
/// Saved map viewport height.
static mut old_m_h: fixed_t = 0;
/// Saved map viewport left edge.
static mut old_m_x: fixed_t = 0;
/// Saved map viewport bottom edge.
static mut old_m_y: fixed_t = 0;

/// Last recorded player position, used by [`AM_doFollowPlayer`] to detect
/// movement.
///
/// Initialised to `c_int::MAX` to force an update on the first tick.
/// C origin: `f_oldloc` in am_map.c.
static mut f_oldloc: mpoint_t = mpoint_t { x: 0, y: 0 };

/// Map-to-frame scale factor (fixed-point pixels per map unit).
///
/// Initialised to [`INITSCALEMTOF`]; adjusted by zoom operations.
/// C origin: `scale_mtof` in am_map.c.
static mut scale_mtof: fixed_t = INITSCALEMTOF;

/// Frame-to-map scale factor; reciprocal of [`scale_mtof`] in fixed-point.
///
/// C origin: `scale_ftom` in am_map.c.
static mut scale_ftom: fixed_t = 0;

/// Pointer to the player structure being tracked by the automap.
///
/// Set to the console player (or the first active player in a network game)
/// in [`AM_initVariables`].  C origin: `plr` in am_map.c.
static mut plr: *mut PlayerT = ptr::null_mut();

/// Cached patch pointers for the ten mark-number glyphs (`AMMNUM0`-`AMMNUM9`).
///
/// Loaded from the WAD by [`AM_loadPics`] and released by [`AM_unloadPics`].
/// C origin: `marknums[]` in am_map.c.
static mut marknums: [*mut patch_t; AM_NUMMARKPOINTS] = [ptr::null_mut(); AM_NUMMARKPOINTS];

/// Map-coordinate positions of the player-placed mark points.
///
/// An `x` value of `-1` indicates an unused slot.
/// C origin: `markpoints[]` in am_map.c.
static mut markpoints: [mpoint_t; AM_NUMMARKPOINTS] = [mpoint_t { x: -1, y: -1 }; AM_NUMMARKPOINTS];

/// Index of the next mark slot to fill, wrapping modulo `AM_NUMMARKPOINTS`.
///
/// C origin: `markpointnum` in am_map.c.
static mut markpointnum: c_int = 0;

/// Non-zero when the automap camera should track the player position.
///
/// Set to 0 when the player pans the map manually; restored on `key_map_follow`.
/// C origin: `followplayer` in am_map.c.
static mut followplayer: c_int = 1;

/// Cheat sequence for the automap reveal (`iddt`).
///
/// Exported so that `AM_Responder` can pass a pointer to it to
/// `cht_CheckCheat`.  C origin: `cheat_amap` in am_map.c.
#[no_mangle]
pub static mut cheat_amap: cheatseq_t = cheatseq_t {
    sequence: make_cheat_seq(b"iddt"),
    sequence_len: 4,
    parameter_chars: 0,
    chars_read: 0,
    param_chars_read: 0,
    parameter_buf: [0; 5],
};

/// Compile-time helper that copies a byte slice into a fixed-length `c_char`
/// array, padding with zeros.
///
/// Used to initialise [`cheat_amap`]'s `sequence` field in a `const` context.
const fn make_cheat_seq(seq: &[u8]) -> [c_char; 25] {
    let mut arr = [0i8; 25];
    let mut i = 0;
    while i < seq.len() {
        arr[i] = seq[i] as c_char;
        i += 1;
    }
    arr
}

/// Non-zero when the automap is fully inactive (after `AM_Stop`).
///
/// Prevents `AM_Stop` from running its shutdown logic more than once per
/// open/close cycle.  C origin: `stopped` in am_map.c.
static mut stopped: c_int = 1;

// ---------------------------------------------------------------------------
// Helper: FTOM / MTOF macros
// ---------------------------------------------------------------------------

/// Convert a frame (screen-pixel) distance to a map-coordinate distance.
///
/// Equivalent to the C macro `FTOM(x)` which expands to
/// `FixedMul((x) << FRACBITS, scale_ftom)`.
///
/// # Safety
///
/// Reads `scale_ftom` which is a mutable static; must only be called while
/// the automap invariants hold.
#[inline(always)]
unsafe fn FTOM(x: c_int) -> fixed_t {
    FixedMul((x as fixed_t) << FRACBITS, scale_ftom)
}

/// Convert a map-coordinate distance to a frame (screen-pixel) distance.
///
/// Equivalent to the C macro `MTOF(x)` which expands to
/// `FixedMul((x), scale_mtof) >> FRACBITS`.
///
/// # Safety
///
/// Reads `scale_mtof` which is a mutable static; must only be called while
/// the automap invariants hold.
#[inline(always)]
unsafe fn MTOF(x: fixed_t) -> c_int {
    (FixedMul(x, scale_mtof) >> FRACBITS) as c_int
}

/// Convert a map x-coordinate to a frame x-coordinate (absolute screen column).
///
/// Accounts for the current viewport origin `m_x` and the frame offset `f_x`.
/// Equivalent to the C macro `CXMTOF(x)`.
///
/// # Safety
///
/// Reads multiple mutable statics; must only be called while the automap
/// invariants hold.
#[inline(always)]
unsafe fn CXMTOF(x: fixed_t) -> c_int {
    f_x + MTOF(x - m_x)
}

/// Convert a map y-coordinate to a frame y-coordinate (absolute screen row).
///
/// Y is flipped: larger map y values correspond to smaller screen y values
/// (map north is screen up).  Equivalent to the C macro `CYMTOF(y)`.
///
/// # Safety
///
/// Reads multiple mutable statics; must only be called while the automap
/// invariants hold.
#[inline(always)]
unsafe fn CYMTOF(y: fixed_t) -> c_int {
    f_y + (f_h - MTOF(y - m_y))
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
        (*is).islp = if dx < 0 { -c_int::MAX } else { c_int::MAX };
    } else {
        (*is).islp = FixedDiv(dx, dy);
    }
    if dx == 0 {
        (*is).slp = if dy < 0 { -c_int::MAX } else { c_int::MAX };
    } else {
        (*is).slp = FixedDiv(dy, dx);
    }
}

/// Recompute the map viewport dimensions after a zoom change.
///
/// Keeps the viewport centred on its previous midpoint and updates `m_x2` /
/// `m_y2`.  C origin: `AM_activateNewScale` in am_map.c.
///
/// # Safety
///
/// Reads and writes multiple mutable statics; must only be called with the
/// automap active.
unsafe fn AM_activateNewScale() {
    m_x += m_w / 2;
    m_y += m_h / 2;
    m_w = FTOM(f_w);
    m_h = FTOM(f_h);
    m_x -= m_w / 2;
    m_y -= m_h / 2;
    m_x2 = m_x + m_w;
    m_y2 = m_y + m_h;
}

/// Save the current viewport position and zoom level into the `old_m_*` statics.
///
/// Called before switching to min-zoom so that the previous view can be
/// restored.  C origin: `AM_saveScaleAndLoc` in am_map.c.
///
/// # Safety
///
/// Reads mutable statics; must only be called with the automap active.
unsafe fn AM_saveScaleAndLoc() {
    old_m_x = m_x;
    old_m_y = m_y;
    old_m_w = m_w;
    old_m_h = m_h;
}

/// Restore the viewport position and zoom level from the `old_m_*` statics.
///
/// If follow mode is active, re-centres the viewport on the player rather than
/// restoring the saved origin.  Recalculates both scale factors.
/// C origin: `AM_restoreScaleAndLoc` in am_map.c.
///
/// # Safety
///
/// Reads and writes multiple mutable statics; `plr` must be a valid pointer.
unsafe fn AM_restoreScaleAndLoc() {
    m_w = old_m_w;
    m_h = old_m_h;
    if followplayer == 0 {
        m_x = old_m_x;
        m_y = old_m_y;
    } else {
        m_x = (*((*plr).mo as *mut mobj_t)).x - m_w / 2;
        m_y = (*((*plr).mo as *mut mobj_t)).y - m_h / 2;
    }
    m_x2 = m_x + m_w;
    m_y2 = m_y + m_h;
    scale_mtof = FixedDiv((f_w as fixed_t) << FRACBITS, m_w);
    scale_ftom = FixedDiv(FRACUNIT, scale_mtof);
}

/// Place a mark at the current viewport centre, advancing the mark slot index.
///
/// Marks wrap around after [`AM_NUMMARKPOINTS`] entries.
/// C origin: `AM_addMark` in am_map.c.
///
/// # Safety
///
/// Reads and writes multiple mutable statics; must only be called with the
/// automap active.
unsafe fn AM_addMark() {
    markpoints[markpointnum as usize].x = m_x + m_w / 2;
    markpoints[markpointnum as usize].y = m_y + m_h / 2;
    markpointnum = (markpointnum + 1) % AM_NUMMARKPOINTS as c_int;
}

/// Compute the axis-aligned bounding box of all level vertices and derive the
/// min/max scale factors.
///
/// Sets `min_x`, `min_y`, `max_x`, `max_y`, `max_w`, `max_h`, `min_w`,
/// `min_h`, `min_scale_mtof`, and `max_scale_mtof`.
/// `min_scale_mtof` is the smaller of the x- and y-axis "fit whole level"
/// scales.  `max_scale_mtof` fits `2 * PLAYERRADIUS` within the frame height.
///
/// C origin: `AM_findMinMaxBoundaries` in am_map.c.
///
/// # Safety
///
/// `vertexes` and `numvertexes` must be valid (populated by `P_LoadVertexes`).
unsafe fn AM_findMinMaxBoundaries() {
    min_x = c_int::MAX;
    min_y = c_int::MAX;
    max_x = -c_int::MAX;
    max_y = -c_int::MAX;

    for i in 0..numvertexes {
        let v = &*vertexes.add(i as usize);
        if v.x < min_x {
            min_x = v.x;
        } else if v.x > max_x {
            max_x = v.x;
        }
        if v.y < min_y {
            min_y = v.y;
        } else if v.y > max_y {
            max_y = v.y;
        }
    }

    max_w = max_x - min_x;
    max_h = max_y - min_y;

    min_w = 2 * PLAYERRADIUS;
    min_h = 2 * PLAYERRADIUS;

    let a = FixedDiv((f_w as fixed_t) << FRACBITS, max_w);
    let b = FixedDiv((f_h as fixed_t) << FRACBITS, max_h);
    min_scale_mtof = if a < b { a } else { b };
    max_scale_mtof = FixedDiv((f_h as fixed_t) << FRACBITS, 2 * PLAYERRADIUS);
}

/// Apply the pending pan increment and clamp the viewport to the level bounds.
///
/// If panning is active, disables follow mode (sets `followplayer = 0` and
/// invalidates `f_oldloc`).  The viewport centre is clamped so it cannot move
/// beyond the level bounding box.
///
/// C origin: `AM_changeWindowLoc` in am_map.c.
///
/// # Safety
///
/// Reads and writes multiple mutable statics; must only be called with the
/// automap active.
unsafe fn AM_changeWindowLoc() {
    if m_paninc.x != 0 || m_paninc.y != 0 {
        followplayer = 0;
        f_oldloc.x = c_int::MAX;
    }

    m_x += m_paninc.x;
    m_y += m_paninc.y;

    if m_x + m_w / 2 > max_x {
        m_x = max_x - m_w / 2;
    } else if m_x + m_w / 2 < min_x {
        m_x = min_x - m_w / 2;
    }

    if m_y + m_h / 2 > max_y {
        m_y = max_y - m_h / 2;
    } else if m_y + m_h / 2 < min_y {
        m_y = min_y - m_h / 2;
    }

    m_x2 = m_x + m_w;
    m_y2 = m_y + m_h;
}

/// Initialise all automap variables for a new session.
///
/// Sets `automapactive = 1`, assigns the framebuffer pointer, resets clocks
/// and pan/zoom multipliers, selects the tracked player, centres the viewport
/// on that player, and sends `AM_MSGENTERED` to the status bar.
///
/// C origin: `AM_initVariables` in am_map.c.
///
/// # Safety
///
/// Reads and writes multiple mutable statics; `players` and `playeringame`
/// must be valid.
unsafe fn AM_initVariables() {
    automapactive = 1;
    fb = I_VideoBuffer;

    f_oldloc.x = c_int::MAX;
    amclock = 0;
    lightlev = 0;

    m_paninc.x = 0;
    m_paninc.y = 0;
    ftom_zoommul = FRACUNIT;
    mtof_zoommul = FRACUNIT;

    m_w = FTOM(f_w);
    m_h = FTOM(f_h);

    if playeringame[consoleplayer as usize] != 0 {
        plr = std::ptr::addr_of_mut!(players[0]).add(consoleplayer as usize);
    } else {
        plr = std::ptr::addr_of_mut!(players[0]);
        for pnum in 0..MAXPLAYERS {
            if playeringame[pnum] != 0 {
                plr = std::ptr::addr_of_mut!(players[0]).add(pnum);
                break;
            }
        }
    }

    m_x = (*((*plr).mo as *mut mobj_t)).x - m_w / 2;
    m_y = (*((*plr).mo as *mut mobj_t)).y - m_h / 2;
    AM_changeWindowLoc();

    old_m_x = m_x;
    old_m_y = m_y;
    old_m_w = m_w;
    old_m_h = m_h;

    let st_notify = event_t {
        type_: 1, // ev_keyup
        data1: AM_MSGENTERED,
        data2: 0,
        data3: 0,
        data4: 0,
    };
    ST_Responder(&st_notify as *const _ as *mut event_t);
}

/// Load the ten `AMMNUM0`-`AMMNUM9` mark-point glyph patches from the WAD.
///
/// Cached as `PU_STATIC` so they remain resident while the automap is open.
/// C origin: `AM_loadPics` in am_map.c.
///
/// # Safety
///
/// `W_CacheLumpName` must succeed; WAD must be loaded.
unsafe fn AM_loadPics() {
    let mut namebuf: [c_char; 9] = [0; 9];
    for i in 0..10i32 {
        c_write!(namebuf, "AMMNUM{}", i);
        marknums[i as usize] = W_CacheLumpName(namebuf.as_mut_ptr(), PU_STATIC) as *mut patch_t;
    }
}

/// Release the ten `AMMNUM*` mark-point glyph patches back to the WAD cache.
///
/// Called by [`AM_Stop`] when the automap closes.
/// C origin: `AM_unloadPics` in am_map.c.
///
/// # Safety
///
/// WAD must be loaded; patch lumps must have been loaded by [`AM_loadPics`].
unsafe fn AM_unloadPics() {
    let mut namebuf: [c_char; 9] = [0; 9];
    for i in 0..10i32 {
        c_write!(namebuf, "AMMNUM{}", i);
        W_ReleaseLumpName(namebuf.as_mut_ptr());
    }
}

/// Reset all mark points: set each `x` field to `-1` and reset the slot index.
///
/// C origin: `AM_clearMarks` in am_map.c.
///
/// # Safety
///
/// Writes mutable statics; safe as long as the automap is active.
unsafe fn AM_clearMarks() {
    for i in 0..AM_NUMMARKPOINTS {
        markpoints[i].x = -1;
    }
    markpointnum = 0;
}

/// Perform per-level automap initialisation.
///
/// Clears marks, finds the level bounding box, and sets the initial scale
/// factor to show approximately 70 % of the minimum fit (clamped to the
/// maximum fit if that produces a smaller value).
///
/// C origin: `AM_LevelInit` in am_map.c.
///
/// # Safety
///
/// `vertexes` / `numvertexes` must be valid (level must be loaded).
unsafe fn AM_LevelInit() {
    leveljuststarted = 0;

    f_x = 0;
    f_y = 0;
    f_w = finit_width;
    f_h = finit_height;

    AM_clearMarks();
    AM_findMinMaxBoundaries();

    scale_mtof = FixedDiv(min_scale_mtof, (0.7 * FRACUNIT as f64) as fixed_t);
    if scale_mtof > max_scale_mtof {
        scale_mtof = min_scale_mtof;
    }
    scale_ftom = FixedDiv(FRACUNIT, scale_mtof);
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
/// Called by C code in `g_game.c` and `am_map.c`.
/// C origin: `AM_Stop` in am_map.c.
///
/// # Safety
///
/// Must be called only when the automap was previously started; WAD must be
/// loaded.
#[no_mangle]
pub unsafe extern "C" fn AM_Stop() {
    let st_notify = event_t {
        type_: 0, // ev_keydown
        data1: 1, // ev_keyup
        data2: AM_MSGEXITED,
        data3: 0,
        data4: 0,
    };
    AM_unloadPics();
    automapactive = 0;
    ST_Responder(&st_notify as *const _ as *mut event_t);
    stopped = 1;
}

/// Activate the automap.
///
/// If the automap is already open (`stopped == 0`), closes it first.
/// Re-runs `AM_LevelInit` whenever the level or episode changes, then calls
/// `AM_initVariables` and `AM_loadPics`.
///
/// Called by C code in `g_game.c` and by [`AM_Responder`] when the toggle key
/// is pressed.  C origin: `AM_Start` in am_map.c.
///
/// # Safety
///
/// The game must be in an active level with valid map data loaded.
#[no_mangle]
pub unsafe extern "C" fn AM_Start() {
    static mut lastlevel: c_int = -1;
    static mut lastepisode: c_int = -1;

    if stopped == 0 {
        AM_Stop();
    }
    stopped = 0;
    if lastlevel != gamemap || lastepisode != gameepisode {
        AM_LevelInit();
        lastlevel = gamemap;
        lastepisode = gameepisode;
    }
    AM_initVariables();
    AM_loadPics();
}

/// Set the zoom to the minimum scale (most zoomed out; fits the whole level).
///
/// C origin: `AM_minOutWindowScale` in am_map.c.
///
/// # Safety
///
/// Must be called with the automap active and map boundaries already computed.
unsafe fn AM_minOutWindowScale() {
    scale_mtof = min_scale_mtof;
    scale_ftom = FixedDiv(FRACUNIT, scale_mtof);
    AM_activateNewScale();
}

/// Set the zoom to the maximum scale (most zoomed in; `2 * PLAYERRADIUS` fills
/// the frame height).
///
/// C origin: `AM_maxOutWindowScale` in am_map.c.
///
/// # Safety
///
/// Must be called with the automap active and map boundaries already computed.
unsafe fn AM_maxOutWindowScale() {
    scale_mtof = max_scale_mtof;
    scale_ftom = FixedDiv(FRACUNIT, scale_mtof);
    AM_activateNewScale();
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
/// Called from C code in `g_game.c`.  C origin: `AM_Responder` in am_map.c.
///
/// # Safety
///
/// `ev` must be a valid pointer to an `event_t`; mutable statics are accessed.
#[no_mangle]
pub unsafe extern "C" fn AM_Responder(ev: *mut event_t) -> c_int {
    let mut rc: c_int = 0;
    static mut bigstate: c_int = 0;

    if automapactive == 0 {
        if (*ev).type_ == 0 && (*ev).data1 == key_map_toggle {
            // ev_keydown
            AM_Start();
            viewactive = 0;
            rc = 1;
        }
    } else if (*ev).type_ == 0 {
        // ev_keydown
        rc = 1;
        let key = (*ev).data1;

        if key == key_map_east {
            if followplayer == 0 {
                m_paninc.x = FTOM(F_PANINC);
            } else {
                rc = 0;
            }
        } else if key == key_map_west {
            if followplayer == 0 {
                m_paninc.x = -FTOM(F_PANINC);
            } else {
                rc = 0;
            }
        } else if key == key_map_north {
            if followplayer == 0 {
                m_paninc.y = FTOM(F_PANINC);
            } else {
                rc = 0;
            }
        } else if key == key_map_south {
            if followplayer == 0 {
                m_paninc.y = -FTOM(F_PANINC);
            } else {
                rc = 0;
            }
        } else if key == key_map_zoomout {
            mtof_zoommul = M_ZOOMOUT;
            ftom_zoommul = M_ZOOMIN;
        } else if key == key_map_zoomin {
            mtof_zoommul = M_ZOOMIN;
            ftom_zoommul = M_ZOOMOUT;
        } else if key == key_map_toggle {
            bigstate = 0;
            viewactive = 1;
            AM_Stop();
        } else if key == key_map_maxzoom {
            bigstate = !bigstate;
            if bigstate != 0 {
                AM_saveScaleAndLoc();
                AM_minOutWindowScale();
            } else {
                AM_restoreScaleAndLoc();
            }
        } else if key == key_map_follow {
            followplayer = !followplayer;
            f_oldloc.x = c_int::MAX;
            if followplayer != 0 {
                (*plr).message = DEH_String(c"Follow Mode ON".as_ptr().cast_mut());
            } else {
                (*plr).message = DEH_String(c"Follow Mode OFF".as_ptr().cast_mut());
            }
        } else if key == key_map_grid {
            grid = !grid;
            if grid != 0 {
                (*plr).message = DEH_String(c"Grid ON".as_ptr().cast_mut());
            } else {
                (*plr).message = DEH_String(c"Grid OFF".as_ptr().cast_mut());
            }
        } else if key == key_map_mark {
            static mut AM_MARK_MSG: [c_char; 20] = [0; 20];
            c_write!(AM_MARK_MSG, "Marked Spot {}", markpointnum as c_int);
            (*plr).message = std::ptr::addr_of_mut!(AM_MARK_MSG[0]);
            AM_addMark();
        } else if key == key_map_clearmark {
            AM_clearMarks();
            (*plr).message = DEH_String(c"All Marks Cleared".as_ptr().cast_mut());
        } else {
            rc = 0;
        }

        if deathmatch == 0 && cht_CheckCheat(&raw mut cheat_amap, (*ev).data2 as c_char) != 0 {
            rc = 0;
            cheating = (cheating + 1) % 3;
        }
    } else if (*ev).type_ == 1 {
        // ev_keyup
        rc = 0;
        let key = (*ev).data1;

        if key == key_map_east || key == key_map_west {
            if followplayer == 0 {
                m_paninc.x = 0;
            }
        } else if key == key_map_north || key == key_map_south {
            if followplayer == 0 {
                m_paninc.y = 0;
            }
        } else if key == key_map_zoomout || key == key_map_zoomin {
            mtof_zoommul = FRACUNIT;
            ftom_zoommul = FRACUNIT;
        }
    }

    rc
}

/// Apply the current zoom multipliers and clamp to the allowed scale range.
///
/// If the new scale would fall below `min_scale_mtof`, snaps to minimum zoom.
/// If it exceeds `max_scale_mtof`, snaps to maximum zoom.  Otherwise calls
/// `AM_activateNewScale` to recompute the viewport dimensions.
///
/// C origin: `AM_changeWindowScale` in am_map.c.
///
/// # Safety
///
/// Reads and writes mutable statics; must only be called with the automap active.
unsafe fn AM_changeWindowScale() {
    scale_mtof = FixedMul(scale_mtof, mtof_zoommul);
    scale_ftom = FixedDiv(FRACUNIT, scale_mtof);

    if scale_mtof < min_scale_mtof {
        AM_minOutWindowScale();
    } else if scale_mtof > max_scale_mtof {
        AM_maxOutWindowScale();
    } else {
        AM_activateNewScale();
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
/// # Safety
///
/// `plr` must be a valid pointer; reads mutable statics.
unsafe fn AM_doFollowPlayer() {
    if f_oldloc.x != (*((*plr).mo as *mut mobj_t)).x
        || f_oldloc.y != (*((*plr).mo as *mut mobj_t)).y
    {
        m_x = FTOM(MTOF((*((*plr).mo as *mut mobj_t)).x)) - m_w / 2;
        m_y = FTOM(MTOF((*((*plr).mo as *mut mobj_t)).y)) - m_h / 2;
        m_x2 = m_x + m_w;
        m_y2 = m_y + m_h;
        f_oldloc.x = (*((*plr).mo as *mut mobj_t)).x;
        f_oldloc.y = (*((*plr).mo as *mut mobj_t)).y;
    }
}

/// Advance the `lightlev` animation to the next level in the table.
///
/// This function is compiled but currently disabled (call site is commented
/// out in [`AM_Ticker`]).  It steps through `LITELEVELS` every 6 ticks.
/// C origin: `AM_updateLightLev` in am_map.c.
///
/// # Safety
///
/// Reads and writes mutable statics; must only be called with the automap
/// active.
#[allow(dead_code)]
unsafe fn AM_updateLightLev() {
    static mut nexttic: c_int = 0;
    static LITELEVELS: [c_int; 8] = [0, 4, 7, 10, 12, 14, 15, 15];
    static mut litelevelscnt: c_int = 0;

    if amclock > nexttic {
        lightlev = LITELEVELS[litelevelscnt as usize];
        litelevelscnt += 1;
        if litelevelscnt == LITELEVELS.len() as c_int {
            litelevelscnt = 0;
        }
        nexttic = amclock + 6 - (amclock % 6);
    }
}

/// Advance the automap state by one game tick.
///
/// Returns immediately if `automapactive == 0`.  Otherwise: increments
/// `amclock`, updates the follow-player position, applies zoom, and pans the
/// viewport.
///
/// Called from C code in `g_game.c` once per game tick.
/// C origin: `AM_Ticker` in am_map.c.
///
/// # Safety
///
/// Reads and writes mutable statics; `plr` must be valid while the automap is
/// active.
#[no_mangle]
pub unsafe extern "C" fn AM_Ticker() {
    if automapactive == 0 {
        return;
    }

    amclock += 1;

    if followplayer != 0 {
        AM_doFollowPlayer();
    }

    if ftom_zoommul != FRACUNIT {
        AM_changeWindowScale();
    }

    if m_paninc.x != 0 || m_paninc.y != 0 {
        AM_changeWindowLoc();
    }

    // AM_updateLightLev();
}

/// Fill the automap framebuffer rectangle with `color`.
///
/// Uses `ptr::write_bytes` to set `f_w * f_h` bytes starting at `fb`.
/// C origin: `AM_clearFB` in am_map.c.
///
/// # Safety
///
/// `fb` must point to a buffer of at least `f_w * f_h` bytes.
unsafe fn AM_clearFB(color: c_int) {
    std::ptr::write_bytes(fb, color as u8, (f_w * f_h) as usize);
}

// Cohen-Sutherland outcode constants.

/// Cohen-Sutherland outcode bit: point is to the left of the clip rectangle.
const OC_LEFT: c_int = 1;
/// Cohen-Sutherland outcode bit: point is to the right of the clip rectangle.
const OC_RIGHT: c_int = 2;
/// Cohen-Sutherland outcode bit: point is below the clip rectangle (y > f_h).
const OC_BOTTOM: c_int = 4;
/// Cohen-Sutherland outcode bit: point is above the clip rectangle (y < 0).
const OC_TOP: c_int = 8;

/// Compute the Cohen-Sutherland outcode for frame-coordinate point `(mx, my)`.
///
/// Returns a bitmask of [`OC_LEFT`], [`OC_RIGHT`], [`OC_TOP`], [`OC_BOTTOM`]
/// indicating which clip edges the point lies outside.
///
/// # Safety
///
/// Reads `f_h` and `f_w` mutable statics.
#[inline(always)]
unsafe fn dooutcode(mx: c_int, my: c_int) -> c_int {
    let mut oc = 0;
    if my < 0 {
        oc |= OC_TOP;
    } else if my >= f_h {
        oc |= OC_BOTTOM;
    }
    if mx < 0 {
        oc |= OC_LEFT;
    } else if mx >= f_w {
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
/// `ml` and `fl` must be valid non-null pointers; mutable statics are read.
unsafe fn AM_clipMline(ml: *mut mline_t, fl: *mut fline_t) -> c_int {
    let mut outcode1: c_int = 0;
    let mut outcode2: c_int = 0;
    let mut outside: c_int;
    let mut tmp: fpoint_t = fpoint_t { x: 0, y: 0 };
    let mut dx: c_int;
    let mut dy: c_int;

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
    (*fl).a.x = CXMTOF((*ml).a.x);
    (*fl).a.y = CYMTOF((*ml).a.y);
    (*fl).b.x = CXMTOF((*ml).b.x);
    (*fl).b.y = CYMTOF((*ml).b.y);

    outcode1 = dooutcode((*fl).a.x, (*fl).a.y);
    outcode2 = dooutcode((*fl).b.x, (*fl).b.y);

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
            outcode1 = dooutcode((*fl).a.x, (*fl).a.y);
        } else {
            (*fl).b = tmp;
            outcode2 = dooutcode((*fl).b.x, (*fl).b.y);
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
/// debug counter (`fuck`) and returns without drawing; this matches the C
/// behaviour and is intended as an assertion in debug builds.
///
/// C origin: `AM_drawFline` in am_map.c.
///
/// # Safety
///
/// `fl` must be a valid non-null pointer; `fb` must point to a buffer of at
/// least `f_w * f_h` bytes.
unsafe fn AM_drawFline(fl: *mut fline_t, color: c_int) {
    static mut fuck: c_int = 0;

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
        fuck += 1;
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
/// `ml` must be a valid non-null pointer; mutable statics must be valid.
unsafe fn AM_drawMline(ml: *mut mline_t, color: c_int) {
    static mut fl: fline_t = fline_t {
        a: fpoint_t { x: 0, y: 0 },
        b: fpoint_t { x: 0, y: 0 },
    };
    if AM_clipMline(ml, &raw mut fl) != 0 {
        AM_drawFline(&raw mut fl, color);
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
/// Reads `bmaporgx` / `bmaporgy` mutable statics; `lines` / `numlines` must
/// be valid.
unsafe fn AM_drawGrid(color: c_int) {
    let mut start: fixed_t;
    let mut end: fixed_t;
    let mut ml: mline_t = mline_t {
        a: mpoint_t { x: 0, y: 0 },
        b: mpoint_t { x: 0, y: 0 },
    };

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
        AM_drawMline(&mut ml, color);
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
        AM_drawMline(&mut ml, color);
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
/// `lines`, `numlines`, `sectors`, and `plr` must be valid.
unsafe fn AM_drawWalls() {
    static mut l: mline_t = mline_t {
        a: mpoint_t { x: 0, y: 0 },
        b: mpoint_t { x: 0, y: 0 },
    };

    for i in 0..numlines {
        let li = &*lines.add(i as usize);
        l.a.x = (*li.v1).x;
        l.a.y = (*li.v1).y;
        l.b.x = (*li.v2).x;
        l.b.y = (*li.v2).y;

        let flags = li.flags as c_int;

        if cheating != 0 || (flags & LinedefFlag::MAPPED as c_int) != 0 {
            if (flags & LinedefFlag::DONTDRAW as c_int) != 0 && cheating == 0 {
                continue;
            }
            if li.backsector.is_null() {
                AM_drawMline(&raw mut l, WALLCOLORS + lightlev);
            } else {
                let back = li.backsector as *mut sector_t;
                let front = li.frontsector as *mut sector_t;
                if li.special == 39 {
                    // teleporters
                    AM_drawMline(&raw mut l, WALLCOLORS + WALLRANGE / 2);
                } else if (flags & LinedefFlag::SECRET as c_int) != 0 {
                    if cheating != 0 {
                        AM_drawMline(&raw mut l, SECRETWALLCOLORS + lightlev);
                    } else {
                        AM_drawMline(&raw mut l, WALLCOLORS + lightlev);
                    }
                } else if (*back).floorheight != (*front).floorheight {
                    AM_drawMline(&raw mut l, FDWALLCOLORS + lightlev);
                } else if (*back).ceilingheight != (*front).ceilingheight {
                    AM_drawMline(&raw mut l, CDWALLCOLORS + lightlev);
                } else if cheating != 0 {
                    AM_drawMline(&raw mut l, TSWALLCOLORS + lightlev);
                }
            }
        } else if (*plr).powers[4] != 0 {
            // pw_allmap
            if (flags & LinedefFlag::DONTDRAW as c_int) == 0 {
                AM_drawMline(&raw mut l, GRAYS + 3);
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
unsafe fn AM_rotate(x: *mut fixed_t, y: *mut fixed_t, a: c_uint) {
    let tmpx = FixedMul(*x, *finecosine.0.add((a >> ANGLETOFINESHIFT) as usize))
        - FixedMul(*y, finesine[(a >> ANGLETOFINESHIFT) as usize]);
    *y = FixedMul(*x, finesine[(a >> ANGLETOFINESHIFT) as usize])
        + FixedMul(*y, *finecosine.0.add((a >> ANGLETOFINESHIFT) as usize));
    *x = tmpx;
}

/// Scale, rotate, translate, and draw a multi-segment line-character shape.
///
/// For each segment in `lineguy[0..lineguylines]`: optionally scales each
/// endpoint by `scale` (if non-zero), optionally rotates by `angle` (if
/// non-zero), then translates to `(x, y)` and draws the resulting map line.
/// Used for player arrows and thing triangles.
///
/// C origin: `AM_drawLineCharacter` in am_map.c.
///
/// # Safety
///
/// `lineguy` must point to at least `lineguylines` valid `mline_t` entries;
/// mutable statics must be valid.
unsafe fn AM_drawLineCharacter(
    lineguy: *mut mline_t,
    lineguylines: c_int,
    scale: fixed_t,
    angle: c_uint,
    color: c_int,
    x: fixed_t,
    y: fixed_t,
) {
    let mut l: mline_t = mline_t {
        a: mpoint_t { x: 0, y: 0 },
        b: mpoint_t { x: 0, y: 0 },
    };

    for i in 0..lineguylines {
        l.a.x = (*lineguy.add(i as usize)).a.x;
        l.a.y = (*lineguy.add(i as usize)).a.y;

        if scale != 0 {
            l.a.x = FixedMul(scale, l.a.x);
            l.a.y = FixedMul(scale, l.a.y);
        }

        if angle != 0 {
            AM_rotate(&mut l.a.x, &mut l.a.y, angle);
        }

        l.a.x += x;
        l.a.y += y;

        l.b.x = (*lineguy.add(i as usize)).b.x;
        l.b.y = (*lineguy.add(i as usize)).b.y;

        if scale != 0 {
            l.b.x = FixedMul(scale, l.b.x);
            l.b.y = FixedMul(scale, l.b.y);
        }

        if angle != 0 {
            AM_rotate(&mut l.b.x, &mut l.b.y, angle);
        }

        l.b.x += x;
        l.b.y += y;

        AM_drawMline(&mut l, color);
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
/// `plr` and `players` must be valid; `mobj_t` pointers within player structs
/// must be valid.
unsafe fn AM_drawPlayers() {
    let their_colors: [c_int; 4] = [GREENS, GRAYS, BROWNS, REDS];
    let mut their_color: c_int = -1;
    let mut color: c_int;

    if netgame == 0 {
        if cheating != 0 {
            AM_drawLineCharacter(
                CHEAT_ARROW.as_ptr() as *mut mline_t,
                CHEAT_ARROW.len() as c_int,
                0,
                (*((*plr).mo as *mut mobj_t)).angle,
                WHITE,
                (*((*plr).mo as *mut mobj_t)).x,
                (*((*plr).mo as *mut mobj_t)).y,
            );
        } else {
            AM_drawLineCharacter(
                PLAYER_ARROW.as_ptr() as *mut mline_t,
                PLAYER_ARROW.len() as c_int,
                0,
                (*((*plr).mo as *mut mobj_t)).angle,
                WHITE,
                (*((*plr).mo as *mut mobj_t)).x,
                (*((*plr).mo as *mut mobj_t)).y,
            );
        }
        return;
    }

    for i in 0..MAXPLAYERS {
        their_color += 1;
        let p = std::ptr::addr_of_mut!(players[0]).add(i);

        if deathmatch != 0 && singledemo == 0 && p != plr {
            continue;
        }

        if playeringame[i] == 0 {
            continue;
        }

        if (*p).powers[2] != 0 {
            // pw_invisibility
            color = 246;
        } else {
            color = their_colors[their_color as usize];
        }

        AM_drawLineCharacter(
            PLAYER_ARROW.as_ptr() as *mut mline_t,
            PLAYER_ARROW.len() as c_int,
            0,
            (*((*p).mo as *mut mobj_t)).angle,
            color,
            (*((*p).mo as *mut mobj_t)).x,
            (*((*p).mo as *mut mobj_t)).y,
        );
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
/// `sectors` / `numsectors` must be valid; thing linked lists must be
/// properly terminated.
unsafe fn AM_drawThings(colors: c_int, _colorrange: c_int) {
    for i in 0..numsectors {
        let mut t = (*sectors.add(i as usize)).thinglist as *mut mobj_t;
        while !t.is_null() {
            AM_drawLineCharacter(
                THINTRIANGLE_GUY.as_ptr() as *mut mline_t,
                THINTRIANGLE_GUY.len() as c_int,
                16 << FRACBITS,
                (*t).angle,
                colors + lightlev,
                (*t).x,
                (*t).y,
            );
            t = (*t).snext as *mut mobj_t;
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
/// # Safety
///
/// `marknums` patches must have been loaded by [`AM_loadPics`]; mutable
/// statics must be valid.
unsafe fn AM_drawMarks() {
    for i in 0..AM_NUMMARKPOINTS {
        if markpoints[i].x != -1 {
            let w = 5;
            let h = 6;
            let fx = CXMTOF(markpoints[i].x);
            let fy = CYMTOF(markpoints[i].y);
            if fx >= f_x && fx <= f_w - w && fy >= f_y && fy <= f_h - h {
                V_DrawPatch(fx, fy, marknums[i]);
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
unsafe fn AM_drawCrosshair(color: c_int) {
    *fb.add(((f_w * (f_h + 1)) / 2) as usize) = color as u8;
}

/// Render the automap for the current frame.
///
/// Returns immediately if `automapactive == 0`.  Otherwise: clears the
/// framebuffer, optionally draws the grid, draws walls, players, things (if
/// `cheating == 2`), crosshair, and mark points, then marks the dirty
/// rectangle via `V_MarkRect`.
///
/// Called from C code in `g_game.c` once per frame.
/// C origin: `AM_Drawer` in am_map.c.
///
/// # Safety
///
/// All automap state must be valid (automap must be active with a level
/// loaded).
#[no_mangle]
pub unsafe extern "C" fn AM_Drawer() {
    if automapactive == 0 {
        return;
    }

    AM_clearFB(BACKGROUND);
    if grid != 0 {
        AM_drawGrid(GRIDCOLORS);
    }
    AM_drawWalls();
    AM_drawPlayers();
    if cheating == 2 {
        AM_drawThings(THINGCOLORS, THINGRANGE);
    }
    AM_drawCrosshair(XHAIRCOLORS);
    AM_drawMarks();

    V_MarkRect(f_x, f_y, f_w, f_h);
}

// ---------------------------------------------------------------------------
// Link anchor — ensures symbols are not dropped by the linker.
// ---------------------------------------------------------------------------

/// Ensure all exported automap symbols are retained by the linker.
///
/// Calls every public `extern "C"` function in this module with null/zero
/// arguments.  Never intended to be called at runtime.
/// C origin: not present in am_map.c; added for the Rust link model.
///
/// # Safety
///
/// This function must never be called at runtime; it exists solely to prevent
/// the linker from discarding exported symbols during dead-code elimination.
#[no_mangle]
pub unsafe extern "C" fn AM_Map_Link_Anchor() {
    AM_Responder(ptr::null_mut());
    AM_Ticker();
    AM_Drawer();
    AM_Stop();
}
