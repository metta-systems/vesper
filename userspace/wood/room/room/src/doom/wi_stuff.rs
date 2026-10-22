//! Rust port of vendor/doomgeneric/wi_stuff.c.
//!
//! Intermission / victory screen: kill/item/secret percentages, par time, animated
//! episode map backgrounds, and level-entry/exit animations. Supports single-player,
//! cooperative netgame, and deathmatch scoring modes for up to 4 players.
//!
//! Notable Rust-vs-C differences:
//! - The C `anim_t` struct is split here into an immutable `anim_config_t`
//!   (compile-time data) and a mutable `anim_state_t` (runtime state), stored in
//!   separate `static` arrays. This avoids the need for `static mut` on the config
//!   data and makes borrowing semantics clearer.
//! - `AnimStateTable` wraps `UnsafeCell` to give interior mutability to the state
//!   arrays, which must be `Sync` for use as module-level statics.
//! - `DEH_String` and `SHORT` are identity shims (Dehacked and endianness
//!   conversion are not yet active in this port).

#![allow(
    non_upper_case_globals,
    non_snake_case,
    non_camel_case_types,
    static_mut_refs,
    clippy::missing_safety_doc,
    clippy::not_unsafe_ptr_arg_deref,
    clippy::needless_range_loop,
    clippy::ptr_offset_with_cast,
    clippy::manual_clamp,
    clippy::manual_c_str_literals
)]

use crate::doom::sounds::Sfx;
use std::cell::UnsafeCell;
use std::ffi::{c_char, c_int};
use std::ptr;

use crate::doom::d_event::event_t;
use crate::doom::d_mode;
use crate::doom::d_player::MAXPLAYERS;
use crate::doom::doomstat::gamemode;
use crate::doom::g_game::{deathmatch, netgame, playeringame, players, G_WorldDone};
use crate::doom::i_timer::TICRATE;
use crate::doom::i_video::{SCREENHEIGHT, SCREENWIDTH};
use crate::doom::m_misc::M_StringCopy;
use crate::doom::m_random::M_Random;
use crate::doom::s_sound::{S_ChangeMusic, S_StartSound};
use crate::doom::sounds::Mus;
use crate::doom::v_video::patch_t;
use crate::doom::v_video::V_DrawPatch;
use crate::doom::w_wad::{W_CacheLumpName, W_CheckNumForName, W_ReleaseLumpName};
use crate::doom::z_zone::{Z_Malloc, PU_STATIC};
use crate::{c_write, DEH_snprintf};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Number of Doom episodes (E1-E4, matches C `NUMEPISODES`).
const NUMEPISODES: usize = 4;
/// Maps per episode (1-9, matches C `NUMMAPS`).
const NUMMAPS: usize = 9;

/// Screen y coordinate of the level-name title patch (matches C `WI_TITLEY`).
const WI_TITLEY: c_int = 2;
/// Vertical pixel spacing between player rows in the netgame and deathmatch views (matches C `WI_SPACINGY`).
const WI_SPACINGY: c_int = 33;

/// Screen x of the stats column in single-player view (matches C `SP_STATSX`).
const SP_STATSX: c_int = 50;
/// Screen y of the stats area in single-player view (matches C `SP_STATSY`).
const SP_STATSY: c_int = 50;
/// Screen x of the time display in single-player view (matches C `SP_TIMEX`).
const SP_TIMEX: c_int = 16;
/// Screen y of the time display in single-player view (matches C `SP_TIMEY`).
const SP_TIMEY: c_int = SCREENHEIGHT - 32;

/// Screen y of the stats header row in netgame view (matches C `NG_STATSY`).
const NG_STATSY: c_int = 50;
/// Horizontal pixel spacing between stat columns in netgame view (matches C `NG_SPACINGX`).
const NG_SPACINGX: c_int = 64;

/// Screen x of the frag matrix top-left in deathmatch view (matches C `DM_MATRIXX`).
const DM_MATRIXX: c_int = 42;
/// Screen y of the frag matrix top in deathmatch view (matches C `DM_MATRIXY`).
const DM_MATRIXY: c_int = 68;
/// Horizontal spacing between columns in the deathmatch frag matrix (matches C `DM_SPACINGX`).
const DM_SPACINGX: c_int = 40;
/// Screen x of the "Totals" column in deathmatch view (matches C `DM_TOTALSX`).
const DM_TOTALSX: c_int = 269;
/// Screen x of the "Killers" label in deathmatch view (matches C `DM_KILLERSX`).
const DM_KILLERSX: c_int = 10;
/// Screen y of the "Killers" label in deathmatch view (matches C `DM_KILLERSY`).
const DM_KILLERSY: c_int = 100;
/// Screen x of the "Victims" label in deathmatch view (matches C `DM_VICTIMSX`).
const DM_VICTIMSX: c_int = 5;
/// Screen y of the "Victims" label in deathmatch view (matches C `DM_VICTIMSY`).
const DM_VICTIMSY: c_int = 50;

/// Number of seconds (in `TICRATE` units) the "Show Next Location" map is displayed (matches C `SHOWNEXTLOCDELAY`).
const SHOWNEXTLOCDELAY: c_int = 4;

// ---------------------------------------------------------------------------
// Types that must match C layout (g_game.c is still C)
// ---------------------------------------------------------------------------

/// Per-player intermission data passed in from the game loop (C typedef `wbplayerstruct_t`).
///
/// Layout must be ABI-identical to the C struct because `g_game.c` populates it.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct wbplayerstruct_t {
    /// Non-zero if this player slot is in use.
    pub in_: c_int,
    /// Number of kills this player scored on the level.
    pub skills: c_int,
    /// Number of items this player collected.
    pub sitems: c_int,
    /// Number of secrets this player found.
    pub ssecret: c_int,
    /// Elapsed level time in tics.
    pub stime: c_int,
    /// Frag counts against each of the 4 possible players.
    pub frags: [c_int; 4],
    /// Unused score field (carried from the C struct for ABI compatibility).
    pub score: c_int,
}

/// Overall intermission input record passed from `G_WorldDone` (C typedef `wbstartstruct_t`).
///
/// Layout must be ABI-identical to the C struct; `#[repr(C)]` ensures this.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct wbstartstruct_t {
    /// Episode index (0-based).
    pub epsd: c_int,
    /// Non-zero if the player found the secret level on this episode.
    pub didsecret: c_int,
    /// Index of the map the player just completed (0-based).
    pub last: c_int,
    /// Index of the map the player is going to next (0-based).
    pub next: c_int,
    /// Total killable monsters on the level (denominator for kill percentage).
    pub maxkills: c_int,
    /// Total collectible items on the level (denominator for item percentage).
    pub maxitems: c_int,
    /// Total secrets on the level (denominator for secret percentage).
    pub maxsecret: c_int,
    /// Maximum frag count (not used in single-player).
    pub maxfrags: c_int,
    /// Par time for the level in tics.
    pub partime: c_int,
    /// Console player number (0-based).
    pub pnum: c_int,
    /// Per-player stats for up to `MAXPLAYERS` players.
    pub plyr: [wbplayerstruct_t; MAXPLAYERS],
}

/// Intermission state machine states (C `stateenum_t` in `wi_stuff.c`).
#[derive(Clone, Copy, PartialEq)]
enum stateenum_t {
    /// Transitioning to the next level; brief pause before `G_WorldDone`.
    NoState = -1,
    /// Counting up kill/item/secret/time statistics.
    StatCount,
    /// Showing the episode map with the "you are here" pointer.
    ShowNextLoc,
}

/// Screen coordinate pair used for level-node positions and animation locations (C `point_t`).
#[derive(Clone, Copy)]
struct point_t {
    /// Horizontal screen pixel.
    x: c_int,
    /// Vertical screen pixel.
    y: c_int,
}

/// Animation playback mode for background animations (C `animenum_t`).
#[derive(Clone, Copy, PartialEq, Debug)]
enum animenum_t {
    /// Play every `period` tics regardless of game state.
    ANIM_ALWAYS,
    /// Play randomly with a delay between cycles.
    ANIM_RANDOM,
    /// Play only when the "next" map index matches `data1`.
    ANIM_LEVEL,
}

/// Immutable per-animation configuration (set at compile time, never written at runtime).
///
/// Corresponds to the read-only fields of C `anim_t` in `wi_stuff.c`.
#[derive(Clone, Copy)]
struct anim_config_t {
    /// Playback mode: always, random, or level-triggered.
    type_: animenum_t,
    /// Tics between frame advances (or between random retriggers for `ANIM_RANDOM`).
    period: c_int,
    /// Number of patch frames in this animation (1-3).
    nanims: c_int,
    /// Screen position where the animation is drawn.
    loc: point_t,
    /// For `ANIM_LEVEL`: the map index that triggers playback.
    /// For `ANIM_RANDOM`: the max additional delay (in tics) before the next play.
    data1: c_int,
    /// For `ANIM_RANDOM`: the minimum delay (in tics) before the next play.
    data2: c_int,
}

/// Mutable per-animation runtime state (zeroed at startup, written every frame).
///
/// Corresponds to the mutable fields of C `anim_t` in `wi_stuff.c`.
#[derive(Clone, Copy)]
struct anim_state_t {
    /// Cached patch pointers for each frame; loaded by `WI_loadData`.
    p: [*mut patch_t; 3],
    /// Game tic on which the next frame advance is scheduled.
    nexttic: c_int,
    /// Frame index drawn on the previous tic (unused in this port; kept for layout parity).
    lastdrawn: c_int,
    /// Current frame index within `p` (-1 = not yet started).
    ctr: c_int,
    /// Internal sub-state for `ANIM_RANDOM` sequencing.
    state: c_int,
}

/// Zero-initialised animation state used to fill state tables at program start.
const ZERO_STATE: anim_state_t = anim_state_t {
    p: [ptr::null_mut(); 3],
    nexttic: 0,
    lastdrawn: 0,
    ctr: 0,
    state: 0,
};

/// Interior-mutable wrapper for a fixed-size animation state array.
/// Single-threaded Doom: safe because all access is from the main game thread.
struct AnimStateTable<const N: usize>(UnsafeCell<[anim_state_t; N]>);
// SAFETY: Doom is single-threaded; no concurrent access to these tables.
unsafe impl<const N: usize> Sync for AnimStateTable<N> {}

// ---------------------------------------------------------------------------
// Static data tables
// ---------------------------------------------------------------------------

/// Level-node screen positions for Episode 1 (Knee-Deep in the Dead) map graphic.
static LNODESEPSD0: [point_t; NUMMAPS] = [
    point_t { x: 185, y: 164 },
    point_t { x: 148, y: 143 },
    point_t { x: 69, y: 122 },
    point_t { x: 209, y: 102 },
    point_t { x: 116, y: 89 },
    point_t { x: 166, y: 55 },
    point_t { x: 71, y: 56 },
    point_t { x: 135, y: 29 },
    point_t { x: 71, y: 24 },
];

/// Level-node screen positions for Episode 2 (The Shores of Hell) map graphic.
static LNODESEPSD1: [point_t; NUMMAPS] = [
    point_t { x: 254, y: 25 },
    point_t { x: 97, y: 50 },
    point_t { x: 188, y: 64 },
    point_t { x: 128, y: 78 },
    point_t { x: 214, y: 92 },
    point_t { x: 133, y: 130 },
    point_t { x: 208, y: 136 },
    point_t { x: 148, y: 140 },
    point_t { x: 235, y: 158 },
];

/// Level-node screen positions for Episode 3 (Inferno) map graphic.
static LNODESEPSD2: [point_t; NUMMAPS] = [
    point_t { x: 156, y: 168 },
    point_t { x: 48, y: 154 },
    point_t { x: 174, y: 95 },
    point_t { x: 265, y: 75 },
    point_t { x: 130, y: 48 },
    point_t { x: 279, y: 23 },
    point_t { x: 198, y: 48 },
    point_t { x: 140, y: 25 },
    point_t { x: 281, y: 136 },
];

/// Level-node positions for Episode 4 (Thy Flesh Consumed) - all zeroed; no map graphic used.
static LNODESEPSD3: [point_t; NUMMAPS] = [point_t { x: 0, y: 0 }; NUMMAPS];

/// Combined level-node position table indexed by `[episode][map]` (C `lnodes` in `wi_stuff.c`).
static LNODES: [[point_t; NUMMAPS]; NUMEPISODES] =
    [LNODESEPSD0, LNODESEPSD1, LNODESEPSD2, LNODESEPSD3];

/// Number of animated patches in Episode 1's background (matches C `NUMANIMS[0]`).
pub(crate) const EPSD0_NANIM: usize = 10;
/// Number of animated patches in Episode 2's background (matches C `NUMANIMS[1]`).
pub(crate) const EPSD1_NANIM: usize = 9;
/// Number of animated patches in Episode 3's background (matches C `NUMANIMS[2]`).
pub(crate) const EPSD2_NANIM: usize = 6;

/// Animation configurations for Episode 1's background overlay (C `epsd0animinfo`).
static EPSD0_CONFIG: [anim_config_t; EPSD0_NANIM] = [
    anim_config_t {
        type_: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 224, y: 104 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 184, y: 160 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 112, y: 136 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 72, y: 112 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 88, y: 96 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 64, y: 48 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 192, y: 40 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 136, y: 16 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 80, y: 16 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 64, y: 24 },
        data1: 0,
        data2: 0,
    },
];

/// Animation configurations for Episode 2's background overlay (C `epsd1animinfo`).
static EPSD1_CONFIG: [anim_config_t; EPSD1_NANIM] = [
    anim_config_t {
        type_: animenum_t::ANIM_LEVEL,
        period: TICRATE / 3,
        nanims: 1,
        loc: point_t { x: 128, y: 136 },
        data1: 1,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_LEVEL,
        period: TICRATE / 3,
        nanims: 1,
        loc: point_t { x: 128, y: 136 },
        data1: 2,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_LEVEL,
        period: TICRATE / 3,
        nanims: 1,
        loc: point_t { x: 128, y: 136 },
        data1: 3,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_LEVEL,
        period: TICRATE / 3,
        nanims: 1,
        loc: point_t { x: 128, y: 136 },
        data1: 4,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_LEVEL,
        period: TICRATE / 3,
        nanims: 1,
        loc: point_t { x: 128, y: 136 },
        data1: 5,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_LEVEL,
        period: TICRATE / 3,
        nanims: 1,
        loc: point_t { x: 128, y: 136 },
        data1: 6,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_LEVEL,
        period: TICRATE / 3,
        nanims: 1,
        loc: point_t { x: 128, y: 136 },
        data1: 7,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_LEVEL,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 192, y: 144 },
        data1: 8,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_LEVEL,
        period: TICRATE / 3,
        nanims: 1,
        loc: point_t { x: 128, y: 136 },
        data1: 8,
        data2: 0,
    },
];

/// Animation configurations for Episode 3's background overlay (C `epsd2animinfo`).
static EPSD2_CONFIG: [anim_config_t; EPSD2_NANIM] = [
    anim_config_t {
        type_: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 104, y: 168 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 40, y: 136 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 160, y: 96 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 104, y: 80 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 120, y: 32 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        type_: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 4,
        nanims: 3,
        loc: point_t { x: 40, y: 0 },
        data1: 0,
        data2: 0,
    },
];

/// Mutable runtime animation states for Episode 1's background animations.
static EPSD0_STATE: AnimStateTable<EPSD0_NANIM> =
    AnimStateTable(UnsafeCell::new([ZERO_STATE; EPSD0_NANIM]));
/// Mutable runtime animation states for Episode 2's background animations.
static EPSD1_STATE: AnimStateTable<EPSD1_NANIM> =
    AnimStateTable(UnsafeCell::new([ZERO_STATE; EPSD1_NANIM]));
/// Mutable runtime animation states for Episode 3's background animations.
static EPSD2_STATE: AnimStateTable<EPSD2_NANIM> =
    AnimStateTable(UnsafeCell::new([ZERO_STATE; EPSD2_NANIM]));

/// Return a reference to the compile-time animation configuration for entry `j` in episode `epsd`.
///
/// Panics if `epsd` is out of range (0-2).
fn anim_config(epsd: usize, j: usize) -> &'static anim_config_t {
    match epsd {
        0 => &EPSD0_CONFIG[j],
        1 => &EPSD1_CONFIG[j],
        2 => &EPSD2_CONFIG[j],
        _ => panic!("anim_config: invalid episode {}", epsd),
    }
}

/// Returns a raw pointer to the mutable state for animation `j` in episode `epsd`.
///
/// # Safety
///
/// Callers must uphold all three invariants:
/// 1. `epsd` must be 0, 1, or 2 — any other value panics.
/// 2. `j` must be in bounds for the selected episode's state table.
/// 3. No data races: Doom is single-threaded; callers must not use the returned
///    pointer concurrently with any other access to the same `AnimStateTable`.
///
/// Note: raw pointers produced by this function may alias different elements of
/// the same table — that is intentional and sound as long as invariant 3 holds.
unsafe fn anim_state_ptr(epsd: usize, j: usize) -> *mut anim_state_t {
    match epsd {
        0 => {
            debug_assert!(
                j < EPSD0_NANIM,
                "j={j} out of bounds for epsd0 (len={EPSD0_NANIM})"
            );
            (EPSD0_STATE.0.get() as *mut anim_state_t).add(j)
        }
        1 => {
            debug_assert!(
                j < EPSD1_NANIM,
                "j={j} out of bounds for epsd1 (len={EPSD1_NANIM})"
            );
            (EPSD1_STATE.0.get() as *mut anim_state_t).add(j)
        }
        2 => {
            debug_assert!(
                j < EPSD2_NANIM,
                "j={j} out of bounds for epsd2 (len={EPSD2_NANIM})"
            );
            (EPSD2_STATE.0.get() as *mut anim_state_t).add(j)
        }
        _ => panic!("anim_state_ptr: invalid episode {epsd}"),
    }
}

/// Count of background animations per episode, indexed by episode number (C `numanims`).
/// Episode 3 (index 3) has no animations (0).
static NUMANIMS: [c_int; NUMEPISODES] = [
    EPSD0_NANIM as c_int,
    EPSD1_NANIM as c_int,
    EPSD2_NANIM as c_int,
    0,
];

// ---------------------------------------------------------------------------
// Internal globals
// ---------------------------------------------------------------------------

/// Non-zero when the player pressed fire/use to skip the current count-up animation.
static mut acceleratestage: c_int = 0;
/// Console player index (0-based); set from `wbs.pnum` by `WI_initVariables`.
static mut me: c_int = 0;
/// Current intermission state machine state.
static mut state: stateenum_t = stateenum_t::NoState;
/// Pointer to the level-start record filled by `G_WorldDone`; valid for the duration of the intermission.
static mut wbs: *mut wbstartstruct_t = ptr::null_mut();
/// Pointer to `wbs.plyr[0]`; used to index per-player stats by offset.
static mut plrs: *mut wbplayerstruct_t = ptr::null_mut();
/// Generic countdown used in `NoState` and `ShowNextLoc` states.
static mut cnt: c_int = 0;
/// Background animation beat counter; incremented every tic by `WI_Ticker`.
static mut bcnt: c_int = 0;
/// Non-zero on the first draw call after `WI_Start`, triggers a full background blit.
static mut firstrefresh: c_int = 0;

/// Running kill-percentage display values (count up toward actual percentage).
static mut cnt_kills: [c_int; MAXPLAYERS] = [0; MAXPLAYERS];
/// Running item-percentage display values.
static mut cnt_items: [c_int; MAXPLAYERS] = [0; MAXPLAYERS];
/// Running secret-percentage display values.
static mut cnt_secret: [c_int; MAXPLAYERS] = [0; MAXPLAYERS];
/// Running displayed level time in seconds (counts up from 0).
static mut cnt_time: c_int = 0;
/// Running displayed par time in seconds (counts up from 0).
static mut cnt_par: c_int = 0;
/// Tic countdown used as a pause between successive stat reveals.
static mut cnt_pause: c_int = 0;

/// Number of Doom II maps to load level-name patches for (32 for the retail release).
static mut NUMCMAPS: c_int = 0;

// ---------------------------------------------------------------------------
// Cached WAD patches (loaded by WI_loadData, released by WI_unloadData)
// ---------------------------------------------------------------------------

/// "You Are Here" arrow patches (2 frames, `WIURH0`/`WIURH1`); third slot unused.
static mut yah: [*mut patch_t; 3] = [ptr::null_mut(); 3];
/// Completed-level splat patches (`WISPLAT`); second slot unused.
static mut splat: [*mut patch_t; 2] = [ptr::null_mut(); 2];
/// Percent sign patch (`WIPCNT`).
static mut percent: *mut patch_t = ptr::null_mut();
/// Colon separator patch (`WICOLON`) for time display.
static mut colon: *mut patch_t = ptr::null_mut();
/// Digit patches 0-9 (`WINUM0`-`WINUM9`).
static mut num: [*mut patch_t; 10] = [ptr::null_mut(); 10];
/// Minus sign patch (`WIMINUS`) for negative frag counts.
static mut wiminus: *mut patch_t = ptr::null_mut();
/// "Finished" label patch (`WIF`).
static mut finished: *mut patch_t = ptr::null_mut();
/// "Entering" label patch (`WIENTER`).
static mut entering: *mut patch_t = ptr::null_mut();
/// Single-player secret label patch (`WISCRT2`).
static mut sp_secret: *mut patch_t = ptr::null_mut();
/// Kills column header patch (`WIOSTK`).
static mut kills: *mut patch_t = ptr::null_mut();
/// Secrets column header patch (`WIOSTS`).
static mut secret: *mut patch_t = ptr::null_mut();
/// Items column header patch (`WIOSTI` or `WIOBJ` in co-op).
static mut items: *mut patch_t = ptr::null_mut();
/// Frags column header patch (`WIFRGS`).
static mut frags: *mut patch_t = ptr::null_mut();
/// Time label patch (`WITIME`).
static mut timepatch: *mut patch_t = ptr::null_mut();
/// Par-time label patch (`WIPAR`).
static mut par: *mut patch_t = ptr::null_mut();
/// "Sucks" patch displayed when the level time exceeds the representable maximum (`WISUCKS`).
static mut sucks: *mut patch_t = ptr::null_mut();
/// "Killers" row label patch (`WIKILRS`) used in deathmatch view.
static mut killers: *mut patch_t = ptr::null_mut();
/// "Victims" column label patch (`WIVCTMS`) used in deathmatch view.
static mut victims: *mut patch_t = ptr::null_mut();
/// "Total" column header patch (`WIMSTT`) used in deathmatch view.
static mut total: *mut patch_t = ptr::null_mut();
/// "You are here" star patch (`STFST01`) marking the local player in netgame view.
static mut star: *mut patch_t = ptr::null_mut();
/// Dead-face patch (`STFDEAD0`) marking the local player when dead.
static mut bstar: *mut patch_t = ptr::null_mut();
/// Player face patches for each slot (`STPB0`-`STPB3`).
static mut p: [*mut patch_t; MAXPLAYERS] = [ptr::null_mut(); MAXPLAYERS];
/// Alternative player face patches for each slot (`WIBP1`-`WIBP4`).
static mut bp: [*mut patch_t; MAXPLAYERS] = [ptr::null_mut(); MAXPLAYERS];
/// Heap-allocated array of level-name patches (`WILV##` or `CWILV##`); length is `NUMCMAPS` or `NUMMAPS`.
static mut lnames: *mut *mut patch_t = ptr::null_mut();
/// Background map graphic for the current episode (`WIMAP#` or `INTERPIC`).
static mut background: *mut patch_t = ptr::null_mut();

/// Deathmatch count-up sub-state index (odd = pause, even = ticking).
static mut dm_state: c_int = 0;
/// Running frag-count display for each `[killer][victim]` pair in deathmatch view.
static mut dm_frags: [[c_int; MAXPLAYERS]; MAXPLAYERS] = [[0; MAXPLAYERS]; MAXPLAYERS];
/// Running per-player frag totals for deathmatch view.
static mut dm_totals: [c_int; MAXPLAYERS] = [0; MAXPLAYERS];

/// Running frag-count display per player in cooperative netgame view.
static mut cnt_frags: [c_int; MAXPLAYERS] = [0; MAXPLAYERS];
/// Non-zero when at least one player has a non-zero frag count (gates frags column display).
static mut dofrags: c_int = 0;
/// Netgame count-up sub-state index.
static mut ng_state: c_int = 0;

/// Single-player count-up sub-state index.
static mut sp_state: c_int = 0;
/// Non-zero when the "you are here" pointer should be drawn in `ShowNextLoc` state.
static mut snl_pointeron: bool = false;

// ---------------------------------------------------------------------------
// Externs
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Pass-through shim for Dehacked string replacement (not yet active in this port).
///
/// # Safety
///
/// Caller must ensure `s` is either null or a valid C-string pointer; this
/// implementation does not dereference it and simply returns the pointer
/// unchanged.
#[inline(always)]
unsafe fn DEH_String(s: *mut c_char) -> *mut c_char {
    s
}

/// Identity endian-swap shim; this port runs on little-endian hosts matching the WAD format.
#[inline(always)]
fn SHORT(x: i16) -> i16 {
    x
}

// ---------------------------------------------------------------------------
// WI_slamBackground
// ---------------------------------------------------------------------------

/// Draw the episode background graphic at the origin, covering the entire screen.
///
/// Called at the start of each draw function to paint the base image before
/// overlaying animations, labels, and statistics.
///
/// # Safety
///
/// Reads the global `background` patch pointer and calls `V_DrawPatch`.
/// Caller must ensure `WI_loadData` has cached the patch and the video
/// backend is ready.
#[no_mangle]
pub unsafe extern "C" fn WI_slamBackground() {
    V_DrawPatch(0, 0, background);
}

// ---------------------------------------------------------------------------
// WI_Responder
// ---------------------------------------------------------------------------

/// Intermission event responder - always returns 0 (all input goes through `WI_checkForAccelerate`).
///
/// Called by `G_Responder` in `g_game.c`.
///
/// # Safety
///
/// `_ev` is not dereferenced. Caller may pass any pointer (including null).
/// The function is `unsafe extern "C"` for ABI compatibility with the C
/// responder signature only.
#[no_mangle]
pub unsafe extern "C" fn WI_Responder(_ev: *mut event_t) -> c_int {
    0
}

// ---------------------------------------------------------------------------
// WI_drawLF
// ---------------------------------------------------------------------------

/// Draw the "Finished" overlay: level name and "Finished" text at the top of the screen.
///
/// In commercial mode, does nothing for MAP33 and triggers an intentional patch
/// bounds error for map numbers above `NUMCMAPS` (preserving vanilla quirk - the
/// out-of-range patch lookup is an upstream bug retained for demo compatibility).
///
/// # Safety
///
/// Dereferences the global `wbs` pointer plus the `lnames` array and the
/// `finished` patch pointer. Caller must ensure `WI_Start`/`WI_loadData`
/// have run so these globals are valid.
unsafe fn WI_drawLF() {
    let mut y = WI_TITLEY;

    if gamemode != d_mode::commercial || (*wbs).last < NUMCMAPS {
        V_DrawPatch(
            (SCREENWIDTH - SHORT((*(*lnames.offset((*wbs).last as isize))).width) as c_int) / 2,
            y,
            *lnames.offset((*wbs).last as isize),
        );
        y += (5 * SHORT((*(*lnames.offset((*wbs).last as isize))).height) as c_int) / 4;
        V_DrawPatch(
            (SCREENWIDTH - SHORT((*finished).width) as c_int) / 2,
            y,
            finished,
        );
    } else if (*wbs).last == NUMCMAPS {
        // MAP33 - nothing is displayed!
    } else if (*wbs).last > NUMCMAPS {
        // Deliberately trigger a V_DrawPatch error
        let tmp: patch_t = patch_t {
            width: SCREENWIDTH as i16,
            height: SCREENHEIGHT as i16,
            leftoffset: 1,
            topoffset: 1,
        };
        V_DrawPatch(0, y, &tmp as *const patch_t as *mut patch_t);
    }
}

// ---------------------------------------------------------------------------
// WI_drawEL
// ---------------------------------------------------------------------------

/// Draw the "Entering" overlay: "Entering" text and the next level's name.
///
/// # Safety
///
/// Dereferences the global `wbs` pointer plus the `entering` patch and the
/// `lnames` array. Caller must ensure `WI_Start`/`WI_loadData` have run so
/// these globals are valid.
unsafe fn WI_drawEL() {
    let mut y = WI_TITLEY;

    V_DrawPatch(
        (SCREENWIDTH - SHORT((*entering).width) as c_int) / 2,
        y,
        entering,
    );

    y += (5 * SHORT((*(*lnames.offset((*wbs).next as isize))).height) as c_int) / 4;

    V_DrawPatch(
        (SCREENWIDTH - SHORT((*(*lnames.offset((*wbs).next as isize))).width) as c_int) / 2,
        y,
        *lnames.offset((*wbs).next as isize),
    );
}

// ---------------------------------------------------------------------------
// WI_drawOnLnode
// ---------------------------------------------------------------------------

/// Draw the patch array `c` centred on the `n`-th level node of the current episode map.
///
/// Tries each frame in `c` (indices 0 then 1) and draws the first that fits
/// within the screen bounds. If neither fits, prints a diagnostic to stdout.
///
/// # Safety
///
/// `c` must point to an array of at least two `*mut patch_t` slots; the
/// function reads index 0 unconditionally and index 1 only if index 0 does
/// not fit on screen. Each non-null slot must point to a valid `patch_t`.
/// Dereferences the global `wbs` for the current episode and reads `LNODES`.
unsafe fn WI_drawOnLnode(n: c_int, c: *mut *mut patch_t) {
    let mut i = 0;
    let mut fits = false;

    loop {
        let left = LNODES[(*wbs).epsd as usize][n as usize].x
            - SHORT((**c.offset(i as isize)).leftoffset) as c_int;
        let top = LNODES[(*wbs).epsd as usize][n as usize].y
            - SHORT((**c.offset(i as isize)).topoffset) as c_int;
        let right = left + SHORT((**c.offset(i as isize)).width) as c_int;
        let bottom = top + SHORT((**c.offset(i as isize)).height) as c_int;

        if left >= 0 && right < SCREENWIDTH && top >= 0 && bottom < SCREENHEIGHT {
            fits = true;
            break;
        }

        i += 1;
        if i == 2 || (*c.offset(i as isize)).is_null() {
            break;
        }
    }

    if fits && i < 2 {
        V_DrawPatch(
            LNODES[(*wbs).epsd as usize][n as usize].x,
            LNODES[(*wbs).epsd as usize][n as usize].y,
            *c.offset(i as isize),
        );
    } else {
        libc::printf(c"Could not place patch on level %d".as_ptr(), n + 1);
    }
}

// ---------------------------------------------------------------------------
// Animated background
// ---------------------------------------------------------------------------

/// Reset all background animation states for the current episode at the start of an intermission.
///
/// No-op in commercial mode and for episode 3 (no animated background exists).
/// Schedules the first frame of each animation by computing `nexttic` from
/// `bcnt` plus a random offset.
///
/// # Safety
///
/// Dereferences the global `wbs` pointer and mutates the per-episode
/// animation-state tables via `anim_state_ptr`. Caller must ensure `wbs`
/// has been initialised (typically by `WI_initVariables`) and that no other
/// code is concurrently reading the animation tables.
unsafe fn WI_initAnimatedBack() {
    if gamemode == d_mode::commercial {
        return;
    }
    if (*wbs).epsd > 2 {
        return;
    }

    let epsd = (*wbs).epsd as usize;
    let count = NUMANIMS[epsd] as usize;

    for i in 0..count {
        let cfg = anim_config(epsd, i);
        let st = anim_state_ptr(epsd, i);
        (*st).ctr = -1;
        if cfg.type_ == animenum_t::ANIM_ALWAYS {
            (*st).nexttic = bcnt + 1 + (M_Random() % cfg.period);
        } else if cfg.type_ == animenum_t::ANIM_RANDOM {
            (*st).nexttic = bcnt + 1 + cfg.data2 + (M_Random() % cfg.data1);
        } else if cfg.type_ == animenum_t::ANIM_LEVEL {
            (*st).nexttic = bcnt + 1;
        }
    }
}

/// Advance all background animation states by one tic.
///
/// No-op in commercial mode and for episode 3. For each animation whose
/// `nexttic` matches the current `bcnt`, increments `ctr` and schedules the
/// next frame according to the animation mode.
///
/// # Safety
///
/// Dereferences the global `wbs` pointer and mutates the per-episode
/// animation-state tables via `anim_state_ptr`. Caller must ensure `wbs`
/// is valid and the animation state was initialised by `WI_initAnimatedBack`.
unsafe fn WI_updateAnimatedBack() {
    if gamemode == d_mode::commercial {
        return;
    }
    if (*wbs).epsd > 2 {
        return;
    }

    let epsd = (*wbs).epsd as usize;
    let count = NUMANIMS[epsd] as usize;

    for i in 0..count {
        let cfg = anim_config(epsd, i);
        let st = anim_state_ptr(epsd, i);
        if bcnt == (*st).nexttic {
            match cfg.type_ {
                animenum_t::ANIM_ALWAYS => {
                    (*st).ctr += 1;
                    if (*st).ctr >= cfg.nanims {
                        (*st).ctr = 0;
                    }
                    (*st).nexttic = bcnt + cfg.period;
                }
                animenum_t::ANIM_RANDOM => {
                    (*st).ctr += 1;
                    if (*st).ctr == cfg.nanims {
                        (*st).ctr = -1;
                        (*st).nexttic = bcnt + cfg.data2 + (M_Random() % cfg.data1);
                    } else {
                        (*st).nexttic = bcnt + cfg.period;
                    }
                }
                animenum_t::ANIM_LEVEL => {
                    // gawd-awful hack for level anims
                    if !(state == stateenum_t::StatCount && i == 7) && (*wbs).next == cfg.data1 {
                        (*st).ctr += 1;
                        if (*st).ctr == cfg.nanims {
                            (*st).ctr -= 1;
                        }
                        (*st).nexttic = bcnt + cfg.period;
                    }
                }
            }
        }
    }
}

/// Draw the current frame of each active background animation over the already-slammed background.
///
/// Skips animations whose `ctr` is negative (not yet started).
/// No-op in commercial mode and for episode 3.
///
/// # Safety
///
/// Dereferences the global `wbs` pointer and reads the per-episode
/// animation-state tables via `anim_state_ptr` plus the patch arrays they
/// reference. Caller must ensure `wbs` is valid and animations have been
/// initialised by `WI_initAnimatedBack`.
unsafe fn WI_drawAnimatedBack() {
    if gamemode == d_mode::commercial {
        return;
    }
    if (*wbs).epsd > 2 {
        return;
    }

    let epsd = (*wbs).epsd as usize;
    let count = NUMANIMS[epsd] as usize;

    for i in 0..count {
        let cfg = anim_config(epsd, i);
        let st = anim_state_ptr(epsd, i);
        if (*st).ctr >= 0 {
            V_DrawPatch(cfg.loc.x, cfg.loc.y, (*st).p[(*st).ctr as usize]);
        }
    }
}

// ---------------------------------------------------------------------------
// Number drawing
// ---------------------------------------------------------------------------

/// Draw integer `n` right-justified at `(x, y)` using `digits` digit patches.
///
/// Returns the x position of the leftmost digit drawn (useful for chaining
/// displays). If `digits` is negative, the required digit count is computed
/// from `n`. The sentinel value 1994 suppresses drawing entirely (used when
/// no ammo type applies). Negative values prepend a minus sign.
///
/// # Safety
///
/// Reads the global `num` digit patches and `wiminus` patch. Caller must
/// ensure `WI_loadData` has cached these patches before invoking.
unsafe fn WI_drawNum(mut x: c_int, y: c_int, mut n: c_int, mut digits: c_int) -> c_int {
    let fontwidth = SHORT((*num[0]).width) as c_int;
    let neg = n < 0;
    if neg {
        n = -n;
    }

    if digits < 0 {
        if n == 0 {
            digits = 1;
        } else {
            digits = 0;
            let mut temp = n;
            while temp != 0 {
                temp /= 10;
                digits += 1;
            }
        }
    }

    // if non-number, do not draw it
    if n == 1994 {
        return 0;
    }

    while digits > 0 {
        digits -= 1;
        x -= fontwidth;
        V_DrawPatch(x, y, num[(n % 10) as usize]);
        n /= 10;
    }

    if neg {
        x -= 8;
        V_DrawPatch(x, y, wiminus);
    }

    x
}

/// Draw a percentage value: percent sign at `x`, then the number right-justified to the left of it.
///
/// No-op if `pct` is negative (stat not yet tallied).
///
/// # Safety
///
/// Reads the global `percent` patch and delegates to `WI_drawNum`. Caller
/// must ensure `WI_loadData` has cached the patches.
unsafe fn WI_drawPercent(x: c_int, y: c_int, pct: c_int) {
    if pct < 0 {
        return;
    }
    V_DrawPatch(x, y, percent);
    WI_drawNum(x, y, pct, -1);
}

/// Draw elapsed time `t` (in seconds) right-justified at `(x, y)` in MM:SS format.
///
/// No-op if `t` is negative. If `t` exceeds the representable range (61 minutes 59 seconds),
/// draws the "SUCKS" patch instead.
///
/// # Safety
///
/// Reads the global `colon` and `sucks` patches and delegates to
/// `WI_drawNum`. Caller must ensure `WI_loadData` has cached the patches.
unsafe fn WI_drawTime(mut x: c_int, y: c_int, t: c_int) {
    if t < 0 {
        return;
    }

    if t <= 61 * 59 {
        let mut div = 1;
        loop {
            let n = (t / div) % 60;
            x = WI_drawNum(x, y, n, 2) - SHORT((*colon).width) as c_int;
            div *= 60;
            if div == 60 || t / div != 0 {
                V_DrawPatch(x, y, colon);
            }
            if t / div == 0 {
                break;
            }
        }
    } else {
        V_DrawPatch(x - SHORT((*sucks).width) as c_int, y, sucks);
    }
}

// ---------------------------------------------------------------------------
// State: NoState
// ---------------------------------------------------------------------------

/// Tear down the intermission subsystem after the screen is dismissed.
///
/// Releases all WAD patches loaded by `WI_loadData`. Called by `G_WorldDone`
/// before the game transitions to the next level.
///
/// # Safety
///
/// Delegates to `WI_unloadData`, which nulls every cached patch pointer.
/// Caller must ensure no draw functions are still executing.
#[no_mangle]
pub unsafe extern "C" fn WI_End() {
    WI_unloadData();
}

/// Enter the `NoState` phase: count down 10 tics then call `G_WorldDone`.
///
/// # Safety
///
/// Mutates the intermission state globals `state`, `acceleratestage`, and
/// `cnt`. Caller must ensure `WI_Start` has been invoked.
unsafe fn WI_initNoState() {
    state = stateenum_t::NoState;
    acceleratestage = 0;
    cnt = 10;
}

/// Tick the `NoState` phase; advances animations and calls `G_WorldDone` when `cnt` reaches 0.
///
/// # Safety
///
/// Mutates `cnt` and runs `WI_updateAnimatedBack` (which dereferences `wbs`
/// and the animation state). Caller must ensure `WI_Start` has been invoked.
unsafe fn WI_updateNoState() {
    WI_updateAnimatedBack();
    cnt -= 1;
    if cnt == 0 {
        G_WorldDone();
    }
}

// ---------------------------------------------------------------------------
// State: ShowNextLoc
// ---------------------------------------------------------------------------

/// Enter the `ShowNextLoc` phase: show the episode map with a blinking "you are here" pointer.
///
/// # Safety
///
/// Mutates `state`, `acceleratestage`, `cnt`, and resets the animated
/// background via `WI_initAnimatedBack` (which dereferences `wbs`). Caller
/// must ensure `WI_Start` has been invoked.
unsafe fn WI_initShowNextLoc() {
    state = stateenum_t::ShowNextLoc;
    acceleratestage = 0;
    cnt = SHOWNEXTLOCDELAY * TICRATE;
    WI_initAnimatedBack();
}

/// Tick the `ShowNextLoc` phase; blinks the pointer and transitions to `NoState` when done.
///
/// # Safety
///
/// Mutates `cnt` and `snl_pointeron` and may transition to `NoState` via
/// `WI_initNoState`. Caller must ensure `WI_Start` has been invoked.
unsafe fn WI_updateShowNextLoc() {
    WI_updateAnimatedBack();
    cnt -= 1;
    if cnt == 0 || acceleratestage != 0 {
        WI_initNoState();
    } else {
        snl_pointeron = (cnt & 31) < 20;
    }
}

/// Draw the `ShowNextLoc` phase: episode map with completed-level splats and optional pointer.
///
/// # Safety
///
/// Dereferences the global `wbs` pointer and the `splat`/`yah` patch arrays,
/// and delegates to `WI_slamBackground`, `WI_drawAnimatedBack`, `WI_drawEL`,
/// and `WI_drawOnLnode`. Caller must ensure `WI_Start`/`WI_loadData` have run.
unsafe fn WI_drawShowNextLoc() {
    WI_slamBackground();
    WI_drawAnimatedBack();

    if gamemode != d_mode::commercial {
        if (*wbs).epsd > 2 {
            WI_drawEL();
            return;
        }

        let last = if (*wbs).last == 8 {
            (*wbs).next - 1
        } else {
            (*wbs).last
        };

        for i in 0..=last {
            WI_drawOnLnode(i, std::ptr::addr_of_mut!(splat) as *mut *mut patch_t);
        }

        if (*wbs).didsecret != 0 {
            WI_drawOnLnode(8, std::ptr::addr_of_mut!(splat) as *mut *mut patch_t);
        }

        if snl_pointeron {
            WI_drawOnLnode(
                (*wbs).next,
                std::ptr::addr_of_mut!(yah) as *mut *mut patch_t,
            );
        }
    }

    if gamemode != d_mode::commercial || (*wbs).next != 30 {
        WI_drawEL();
    }
}

/// Draw the `NoState` phase: same as `ShowNextLoc` but with the pointer always visible.
///
/// # Safety
///
/// Mutates `snl_pointeron` and delegates to `WI_drawShowNextLoc`; same
/// invariants apply (intermission must be started and patches loaded).
unsafe fn WI_drawNoState() {
    snl_pointeron = true;
    WI_drawShowNextLoc();
}

// ---------------------------------------------------------------------------
// Frag helpers
// ---------------------------------------------------------------------------

/// Compute the net frag total for `playernum`: sum of frags against other active players
/// minus self-frags.
///
/// # Safety
///
/// Reads the global `playeringame` array and indexes into `plrs[playernum]`
/// (a pointer into `wbs.plyr`). Caller must ensure `WI_initVariables` has
/// set `plrs` to a valid array and that `playernum` is within
/// `0..MAXPLAYERS`.
unsafe fn WI_fragSum(playernum: c_int) -> c_int {
    let mut sum = 0;
    for i in 0..MAXPLAYERS {
        if playeringame[i] != 0 && i as c_int != playernum {
            sum += (*plrs.offset(playernum as isize)).frags[i];
        }
    }
    sum -= (*plrs.offset(playernum as isize)).frags[playernum as usize];
    sum
}

// ---------------------------------------------------------------------------
// State: Deathmatch stats
// ---------------------------------------------------------------------------

/// Initialise the deathmatch stats phase: zero all counters and start counting up.
///
/// # Safety
///
/// Mutates the intermission state globals (`state`, `acceleratestage`,
/// `dm_state`, `cnt_pause`, `dm_frags`, `dm_totals`) and reads
/// `playeringame`. Caller must ensure `WI_initVariables` has been invoked.
unsafe fn WI_initDeathmatchStats() {
    state = stateenum_t::StatCount;
    acceleratestage = 0;
    dm_state = 1;
    cnt_pause = TICRATE;

    for i in 0..MAXPLAYERS {
        if playeringame[i] != 0 {
            for j in 0..MAXPLAYERS {
                if playeringame[j] != 0 {
                    dm_frags[i][j] = 0;
                }
            }
            dm_totals[i] = 0;
        }
    }

    WI_initAnimatedBack();
}

/// Tick the deathmatch stats phase: count frag values up toward the actual totals.
///
/// State machine: odd states are pauses, state 2 ticks frags, state 4 waits for acceleration.
///
/// # Safety
///
/// Mutates the deathmatch-stats globals (`dm_frags`, `dm_totals`,
/// `dm_state`, `cnt_pause`, `acceleratestage`) and reads `plrs`,
/// `playeringame`, `gamemode`. Triggers sound effects via `S_StartSound`.
/// Caller must ensure `WI_initDeathmatchStats` has run and the sound
/// subsystem is initialised.
unsafe fn WI_updateDeathmatchStats() {
    WI_updateAnimatedBack();

    if acceleratestage != 0 && dm_state != 4 {
        acceleratestage = 0;
        for i in 0..MAXPLAYERS {
            if playeringame[i] != 0 {
                for j in 0..MAXPLAYERS {
                    if playeringame[j] != 0 {
                        dm_frags[i][j] = (*plrs.offset(i as isize)).frags[j];
                    }
                }
                dm_totals[i] = WI_fragSum(i as c_int);
            }
        }
        S_StartSound(ptr::null_mut(), Sfx::Barexp as c_int);
        dm_state = 4;
    }

    if dm_state == 2 {
        if bcnt & 3 == 0 {
            S_StartSound(ptr::null_mut(), Sfx::Pistol as c_int);
        }
        let mut stillticking = false;
        for i in 0..MAXPLAYERS {
            if playeringame[i] != 0 {
                for j in 0..MAXPLAYERS {
                    if playeringame[j] != 0 && dm_frags[i][j] != (*plrs.offset(i as isize)).frags[j]
                    {
                        if (*plrs.offset(i as isize)).frags[j] < 0 {
                            dm_frags[i][j] -= 1;
                        } else {
                            dm_frags[i][j] += 1;
                        }
                        if dm_frags[i][j] > 99 {
                            dm_frags[i][j] = 99;
                        }
                        if dm_frags[i][j] < -99 {
                            dm_frags[i][j] = -99;
                        }
                        stillticking = true;
                    }
                }
                dm_totals[i] = WI_fragSum(i as c_int);
                if dm_totals[i] > 99 {
                    dm_totals[i] = 99;
                }
                if dm_totals[i] < -99 {
                    dm_totals[i] = -99;
                }
            }
        }
        if !stillticking {
            S_StartSound(ptr::null_mut(), Sfx::Barexp as c_int);
            dm_state += 1;
        }
    } else if dm_state == 4 {
        if acceleratestage != 0 {
            S_StartSound(ptr::null_mut(), Sfx::Slop as c_int);
            if gamemode == d_mode::commercial {
                WI_initNoState();
            } else {
                WI_initShowNextLoc();
            }
        }
    } else if dm_state & 1 != 0 {
        cnt_pause -= 1;
        if cnt_pause == 0 {
            dm_state += 1;
            cnt_pause = TICRATE;
        }
    }
}

/// Draw the deathmatch stats page: background, level name, frag matrix, and totals column.
///
/// # Safety
///
/// Reads the deathmatch-stats globals plus the `total`/`killers`/`victims`/
/// `p`/`star`/`bstar` patches and `playeringame`. Caller must ensure
/// `WI_loadData` cached the patches and the deathmatch stats phase has
/// been initialised.
unsafe fn WI_drawDeathmatchStats() {
    WI_slamBackground();
    WI_drawAnimatedBack();
    WI_drawLF();

    V_DrawPatch(
        DM_TOTALSX - SHORT((*total).width) as c_int / 2,
        DM_MATRIXY - WI_SPACINGY + 10,
        total,
    );
    V_DrawPatch(DM_KILLERSX, DM_KILLERSY, killers);
    V_DrawPatch(DM_VICTIMSX, DM_VICTIMSY, victims);

    let mut x = DM_MATRIXX + DM_SPACINGX;
    let mut y = DM_MATRIXY;

    for i in 0..MAXPLAYERS {
        if playeringame[i] != 0 {
            V_DrawPatch(
                x - SHORT((*p[i]).width) as c_int / 2,
                DM_MATRIXY - WI_SPACINGY,
                p[i],
            );
            V_DrawPatch(DM_MATRIXX - SHORT((*p[i]).width) as c_int / 2, y, p[i]);
            if i as c_int == me {
                V_DrawPatch(
                    x - SHORT((*p[i]).width) as c_int / 2,
                    DM_MATRIXY - WI_SPACINGY,
                    bstar,
                );
                V_DrawPatch(DM_MATRIXX - SHORT((*p[i]).width) as c_int / 2, y, star);
            }
        }
        x += DM_SPACINGX;
        y += WI_SPACINGY;
    }

    let mut y = DM_MATRIXY + 10;
    let w = SHORT((*num[0]).width) as c_int;

    for i in 0..MAXPLAYERS {
        let mut x = DM_MATRIXX + DM_SPACINGX;
        if playeringame[i] != 0 {
            for j in 0..MAXPLAYERS {
                if playeringame[j] != 0 {
                    WI_drawNum(x + w, y, dm_frags[i][j], 2);
                }
                x += DM_SPACINGX;
            }
            WI_drawNum(DM_TOTALSX + w, y, dm_totals[i], 2);
        }
        y += WI_SPACINGY;
    }
}

// ---------------------------------------------------------------------------
// State: Netgame stats
// ---------------------------------------------------------------------------

/// Initialise the cooperative netgame stats phase: zero per-player counters and check if frags exist.
///
/// # Safety
///
/// Mutates the netgame-stats globals (`state`, `ng_state`, `cnt_pause`,
/// `cnt_kills`, `cnt_items`, `cnt_secret`, `cnt_frags`, `dofrags`) and
/// reads `playeringame`. Caller must ensure `WI_initVariables` has run.
unsafe fn WI_initNetgameStats() {
    state = stateenum_t::StatCount;
    acceleratestage = 0;
    ng_state = 1;
    cnt_pause = TICRATE;

    for i in 0..MAXPLAYERS {
        if playeringame[i] == 0 {
            continue;
        }
        cnt_kills[i] = 0;
        cnt_items[i] = 0;
        cnt_secret[i] = 0;
        cnt_frags[i] = 0;
        dofrags += WI_fragSum(i as c_int);
    }

    dofrags = if dofrags != 0 { 1 } else { 0 };

    WI_initAnimatedBack();
}

/// Tick the netgame stats phase: sequentially count up kills, items, secrets, and frags.
///
/// States 2/4/6/8 are counting states; odd states are pauses between categories.
///
/// # Safety
///
/// Mutates the netgame-stats globals (`cnt_kills`, `cnt_items`,
/// `cnt_secret`, `cnt_frags`, `ng_state`, `cnt_pause`, `acceleratestage`)
/// and reads `plrs`, `wbs`, `playeringame`. Triggers sound effects via
/// `S_StartSound`. Caller must ensure `WI_initNetgameStats` has run.
unsafe fn WI_updateNetgameStats() {
    WI_updateAnimatedBack();

    if acceleratestage != 0 && ng_state != 10 {
        acceleratestage = 0;
        for i in 0..MAXPLAYERS {
            if playeringame[i] == 0 {
                continue;
            }
            cnt_kills[i] = ((*plrs.offset(i as isize)).skills * 100) / (*wbs).maxkills;
            cnt_items[i] = ((*plrs.offset(i as isize)).sitems * 100) / (*wbs).maxitems;
            cnt_secret[i] = ((*plrs.offset(i as isize)).ssecret * 100) / (*wbs).maxsecret;
            if dofrags != 0 {
                cnt_frags[i] = WI_fragSum(i as c_int);
            }
        }
        S_StartSound(ptr::null_mut(), Sfx::Barexp as c_int);
        ng_state = 10;
    }

    if ng_state == 2 {
        if bcnt & 3 == 0 {
            S_StartSound(ptr::null_mut(), Sfx::Pistol as c_int);
        }
        let mut stillticking = false;
        for i in 0..MAXPLAYERS {
            if playeringame[i] == 0 {
                continue;
            }
            cnt_kills[i] += 2;
            let target = ((*plrs.offset(i as isize)).skills * 100) / (*wbs).maxkills;
            if cnt_kills[i] >= target {
                cnt_kills[i] = target;
            } else {
                stillticking = true;
            }
        }
        if !stillticking {
            S_StartSound(ptr::null_mut(), Sfx::Barexp as c_int);
            ng_state += 1;
        }
    } else if ng_state == 4 {
        if bcnt & 3 == 0 {
            S_StartSound(ptr::null_mut(), Sfx::Pistol as c_int);
        }
        let mut stillticking = false;
        for i in 0..MAXPLAYERS {
            if playeringame[i] == 0 {
                continue;
            }
            cnt_items[i] += 2;
            let target = ((*plrs.offset(i as isize)).sitems * 100) / (*wbs).maxitems;
            if cnt_items[i] >= target {
                cnt_items[i] = target;
            } else {
                stillticking = true;
            }
        }
        if !stillticking {
            S_StartSound(ptr::null_mut(), Sfx::Barexp as c_int);
            ng_state += 1;
        }
    } else if ng_state == 6 {
        if bcnt & 3 == 0 {
            S_StartSound(ptr::null_mut(), Sfx::Pistol as c_int);
        }
        let mut stillticking = false;
        for i in 0..MAXPLAYERS {
            if playeringame[i] == 0 {
                continue;
            }
            cnt_secret[i] += 2;
            let target = ((*plrs.offset(i as isize)).ssecret * 100) / (*wbs).maxsecret;
            if cnt_secret[i] >= target {
                cnt_secret[i] = target;
            } else {
                stillticking = true;
            }
        }
        if !stillticking {
            S_StartSound(ptr::null_mut(), Sfx::Barexp as c_int);
            ng_state += 1 + 2 * if dofrags == 0 { 1 } else { 0 };
        }
    } else if ng_state == 8 {
        if bcnt & 3 == 0 {
            S_StartSound(ptr::null_mut(), Sfx::Pistol as c_int);
        }
        let mut stillticking = false;
        for i in 0..MAXPLAYERS {
            if playeringame[i] == 0 {
                continue;
            }
            cnt_frags[i] += 1;
            let fsum = WI_fragSum(i as c_int);
            if cnt_frags[i] >= fsum {
                cnt_frags[i] = fsum;
            } else {
                stillticking = true;
            }
        }
        if !stillticking {
            S_StartSound(ptr::null_mut(), Sfx::Pldeth as c_int);
            ng_state += 1;
        }
    } else if ng_state == 10 {
        if acceleratestage != 0 {
            S_StartSound(ptr::null_mut(), Sfx::Sgcock as c_int);
            if gamemode == d_mode::commercial {
                WI_initNoState();
            } else {
                WI_initShowNextLoc();
            }
        }
    } else if ng_state & 1 != 0 {
        cnt_pause -= 1;
        if cnt_pause == 0 {
            ng_state += 1;
            cnt_pause = TICRATE;
        }
    }
}

/// Draw the cooperative netgame stats page: background, column headers, and per-player rows.
///
/// # Safety
///
/// Reads the netgame-stats globals plus the column-header patches
/// (`kills`, `items`, `secret`, `frags`, `percent`, `p`, `star`) and
/// `playeringame`. Caller must ensure `WI_loadData` cached the patches
/// and the netgame stats phase has been initialised.
unsafe fn WI_drawNetgameStats() {
    let pwidth = SHORT((*percent).width) as c_int;

    WI_slamBackground();
    WI_drawAnimatedBack();
    WI_drawLF();

    let ng_statsx = 32 + SHORT((*star).width) as c_int / 2 + 32 * if dofrags == 0 { 1 } else { 0 };

    V_DrawPatch(
        ng_statsx + NG_SPACINGX - SHORT((*kills).width) as c_int,
        NG_STATSY,
        kills,
    );
    V_DrawPatch(
        ng_statsx + 2 * NG_SPACINGX - SHORT((*items).width) as c_int,
        NG_STATSY,
        items,
    );
    V_DrawPatch(
        ng_statsx + 3 * NG_SPACINGX - SHORT((*secret).width) as c_int,
        NG_STATSY,
        secret,
    );
    if dofrags != 0 {
        V_DrawPatch(
            ng_statsx + 4 * NG_SPACINGX - SHORT((*frags).width) as c_int,
            NG_STATSY,
            frags,
        );
    }

    let mut y = NG_STATSY + SHORT((*kills).height) as c_int;

    for i in 0..MAXPLAYERS {
        if playeringame[i] == 0 {
            continue;
        }
        let mut x = ng_statsx;
        V_DrawPatch(x - SHORT((*p[i]).width) as c_int, y, p[i]);
        if i as c_int == me {
            V_DrawPatch(x - SHORT((*p[i]).width) as c_int, y, star);
        }
        x += NG_SPACINGX;
        WI_drawPercent(x - pwidth, y + 10, cnt_kills[i]);
        x += NG_SPACINGX;
        WI_drawPercent(x - pwidth, y + 10, cnt_items[i]);
        x += NG_SPACINGX;
        WI_drawPercent(x - pwidth, y + 10, cnt_secret[i]);
        x += NG_SPACINGX;
        if dofrags != 0 {
            WI_drawNum(x, y + 10, cnt_frags[i], -1);
        }
        y += WI_SPACINGY;
    }
}

// ---------------------------------------------------------------------------
// State: Single-player stats
// ---------------------------------------------------------------------------

/// Initialise the single-player stats phase: set all display values to -1 (not yet drawn).
///
/// # Safety
///
/// Mutates the single-player stats globals (`state`, `sp_state`,
/// `cnt_kills[0]`, `cnt_items[0]`, `cnt_secret[0]`, `cnt_time`, `cnt_par`,
/// `cnt_pause`, `acceleratestage`) and resets the animated background.
/// Caller must ensure `WI_initVariables` has run.
unsafe fn WI_initStats() {
    state = stateenum_t::StatCount;
    acceleratestage = 0;
    sp_state = 1;
    cnt_kills[0] = -1;
    cnt_items[0] = -1;
    cnt_secret[0] = -1;
    cnt_time = -1;
    cnt_par = -1;
    cnt_pause = TICRATE;

    WI_initAnimatedBack();
}

/// Tick the single-player stats phase: sequentially count up kills, items, secrets, then time.
///
/// States 2/4/6 count up kill/item/secret percentages; state 8 counts time and par
/// simultaneously; state 10 waits for the player to accelerate.
///
/// # Safety
///
/// Mutates the single-player stats counters (`cnt_kills[0]`, `cnt_items[0]`,
/// `cnt_secret[0]`, `cnt_time`, `cnt_par`, `sp_state`, `cnt_pause`,
/// `acceleratestage`) and reads `plrs`, `wbs`. Triggers sound effects via
/// `S_StartSound`. Caller must ensure `WI_initStats` has run.
unsafe fn WI_updateStats() {
    WI_updateAnimatedBack();

    if acceleratestage != 0 && sp_state != 10 {
        acceleratestage = 0;
        cnt_kills[0] = ((*plrs.offset(me as isize)).skills * 100) / (*wbs).maxkills;
        cnt_items[0] = ((*plrs.offset(me as isize)).sitems * 100) / (*wbs).maxitems;
        cnt_secret[0] = ((*plrs.offset(me as isize)).ssecret * 100) / (*wbs).maxsecret;
        cnt_time = (*plrs.offset(me as isize)).stime / TICRATE;
        cnt_par = (*wbs).partime / TICRATE;
        S_StartSound(ptr::null_mut(), Sfx::Barexp as c_int);
        sp_state = 10;
    }

    if sp_state == 2 {
        cnt_kills[0] += 2;
        if bcnt & 3 == 0 {
            S_StartSound(ptr::null_mut(), Sfx::Pistol as c_int);
        }
        let target = ((*plrs.offset(me as isize)).skills * 100) / (*wbs).maxkills;
        if cnt_kills[0] >= target {
            cnt_kills[0] = target;
            S_StartSound(ptr::null_mut(), Sfx::Barexp as c_int);
            sp_state += 1;
        }
    } else if sp_state == 4 {
        cnt_items[0] += 2;
        if bcnt & 3 == 0 {
            S_StartSound(ptr::null_mut(), Sfx::Pistol as c_int);
        }
        let target = ((*plrs.offset(me as isize)).sitems * 100) / (*wbs).maxitems;
        if cnt_items[0] >= target {
            cnt_items[0] = target;
            S_StartSound(ptr::null_mut(), Sfx::Barexp as c_int);
            sp_state += 1;
        }
    } else if sp_state == 6 {
        cnt_secret[0] += 2;
        if bcnt & 3 == 0 {
            S_StartSound(ptr::null_mut(), Sfx::Pistol as c_int);
        }
        let target = ((*plrs.offset(me as isize)).ssecret * 100) / (*wbs).maxsecret;
        if cnt_secret[0] >= target {
            cnt_secret[0] = target;
            S_StartSound(ptr::null_mut(), Sfx::Barexp as c_int);
            sp_state += 1;
        }
    } else if sp_state == 8 {
        if bcnt & 3 == 0 {
            S_StartSound(ptr::null_mut(), Sfx::Pistol as c_int);
        }
        cnt_time += 3;
        let target_time = (*plrs.offset(me as isize)).stime / TICRATE;
        if cnt_time >= target_time {
            cnt_time = target_time;
        }
        cnt_par += 3;
        let target_par = (*wbs).partime / TICRATE;
        if cnt_par >= target_par {
            cnt_par = target_par;
            if cnt_time >= target_time {
                S_StartSound(ptr::null_mut(), Sfx::Barexp as c_int);
                sp_state += 1;
            }
        }
    } else if sp_state == 10 {
        if acceleratestage != 0 {
            S_StartSound(ptr::null_mut(), Sfx::Sgcock as c_int);
            if gamemode == d_mode::commercial {
                WI_initNoState();
            } else {
                WI_initShowNextLoc();
            }
        }
    } else if sp_state & 1 != 0 {
        cnt_pause -= 1;
        if cnt_pause == 0 {
            sp_state += 1;
            cnt_pause = TICRATE;
        }
    }
}

/// Draw the single-player stats page: background, level name, kill/item/secret/time/par values.
///
/// # Safety
///
/// Reads the single-player stats counters plus the patches `num`, `kills`,
/// `items`, `sp_secret`, `timepatch`, `par`, and dereferences `wbs` to
/// decide whether to draw the par-time row. Caller must ensure
/// `WI_loadData` cached the patches and the stats phase has been initialised.
unsafe fn WI_drawStats() {
    let lh = (3 * SHORT((*num[0]).height) as c_int) / 2;

    WI_slamBackground();
    WI_drawAnimatedBack();
    WI_drawLF();

    V_DrawPatch(SP_STATSX, SP_STATSY, kills);
    WI_drawPercent(SCREENWIDTH - SP_STATSX, SP_STATSY, cnt_kills[0]);

    V_DrawPatch(SP_STATSX, SP_STATSY + lh, items);
    WI_drawPercent(SCREENWIDTH - SP_STATSX, SP_STATSY + lh, cnt_items[0]);

    V_DrawPatch(SP_STATSX, SP_STATSY + 2 * lh, sp_secret);
    WI_drawPercent(SCREENWIDTH - SP_STATSX, SP_STATSY + 2 * lh, cnt_secret[0]);

    V_DrawPatch(SP_TIMEX, SP_TIMEY, timepatch);
    WI_drawTime(SCREENWIDTH / 2 - SP_TIMEX, SP_TIMEY, cnt_time);

    if (*wbs).epsd < 3 {
        V_DrawPatch(SCREENWIDTH / 2 + SP_TIMEX, SP_TIMEY, par);
        WI_drawTime(SCREENWIDTH - SP_TIMEX, SP_TIMEY, cnt_par);
    }
}

// ---------------------------------------------------------------------------
// Accelerate check
// ---------------------------------------------------------------------------

/// Poll all active players' attack and use buttons and set `acceleratestage` if any are newly pressed.
///
/// This is the mechanism by which the player can skip the count-up animation by
/// pressing fire or use during the intermission screen.
///
/// # Safety
///
/// Reads `playeringame` and mutates each active player's `attackdown`/
/// `usedown` flags via the global `players` array, and may set
/// `acceleratestage`. Caller must ensure the player array is initialised.
unsafe fn WI_checkForAccelerate() {
    for i in 0..MAXPLAYERS {
        if playeringame[i] != 0 {
            let player = players.as_mut_ptr().add(i);
            if (*player).cmd.buttons & 1 != 0 {
                // BT_ATTACK
                if (*player).attackdown == 0 {
                    acceleratestage = 1;
                }
                (*player).attackdown = 1;
            } else {
                (*player).attackdown = 0;
            }
            if (*player).cmd.buttons & 2 != 0 {
                // BT_USE
                if (*player).usedown == 0 {
                    acceleratestage = 1;
                }
                (*player).usedown = 1;
            } else {
                (*player).usedown = 0;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// WI_Ticker
// ---------------------------------------------------------------------------

/// Advance the intermission screen by one game tic.
///
/// Increments `bcnt`, starts the intermission music on the first tic,
/// checks for acceleration input, and dispatches to the appropriate state
/// update function. Called by `G_Ticker` in `g_game.c`.
///
/// # Safety
///
/// Mutates `bcnt` and `state` (indirectly via the update functions) and
/// reads `gamemode`/`deathmatch`/`netgame`. Triggers `S_ChangeMusic` and
/// invokes `WI_checkForAccelerate` and one of the per-state updaters, each
/// with their own state-global dependencies. Caller must ensure `WI_Start`
/// has been invoked and the sound subsystem is initialised.
#[no_mangle]
pub unsafe extern "C" fn WI_Ticker() {
    bcnt += 1;

    if bcnt == 1 {
        if gamemode == d_mode::commercial {
            S_ChangeMusic(Mus::Dm2int as c_int, 1);
        } else {
            S_ChangeMusic(Mus::Inter as c_int, 1);
        }
    }

    WI_checkForAccelerate();

    match state {
        stateenum_t::StatCount => {
            if deathmatch != 0 {
                WI_updateDeathmatchStats();
            } else if netgame != 0 {
                WI_updateNetgameStats();
            } else {
                WI_updateStats();
            }
        }
        stateenum_t::ShowNextLoc => {
            WI_updateShowNextLoc();
        }
        stateenum_t::NoState => {
            WI_updateNoState();
        }
    }
}

// ---------------------------------------------------------------------------
// Data loading / unloading
// ---------------------------------------------------------------------------

/// Function pointer type for the load/unload callback used by `WI_loadUnloadData`.
///
/// The callback receives the lump name and a pointer to the patch pointer slot.
type LoadCallback = unsafe extern "C" fn(*mut c_char, *mut *mut patch_t);

/// Walk every intermission lump name and invoke `callback` for each.
///
/// Shared by `WI_loadData` and `WI_unloadData`. Handles both episode (Doom 1)
/// and commercial (Doom 2) map lists, animation frame patches, and all UI
/// patches (numbers, labels, background).
///
/// # Safety
///
/// `callback` is invoked as an `unsafe extern "C"` function and is given raw
/// pointers into the static patch globals (`lnames`, `yah`, `splat`, `num`,
/// `percent`, `finished`, `entering`, etc.) and the per-episode animation
/// state. Caller must pass a callback that respects those pointers' validity
/// and only reads/writes the single slot supplied. Dereferences `wbs` and
/// the global `gamemode`; the WAD subsystem must be initialised.
unsafe fn WI_loadUnloadData(callback: LoadCallback) {
    let mut name: [c_char; 9] = [0; 9];

    if gamemode == d_mode::commercial {
        for i in 0..NUMCMAPS {
            DEH_snprintf!(name, "CWILV{:02}", i);
            callback(name.as_mut_ptr(), lnames.offset(i as isize));
        }
    } else {
        for i in 0..NUMMAPS as c_int {
            c_write!(name, "WILV{}{}", (*wbs).epsd, i);
            callback(name.as_mut_ptr(), lnames.offset(i as isize));
        }

        callback(DEH_String(c"WIURH0".as_ptr().cast_mut()), &mut yah[0]);
        callback(DEH_String(c"WIURH1".as_ptr().cast_mut()), &mut yah[1]);
        callback(DEH_String(c"WISPLAT".as_ptr().cast_mut()), &mut splat[0]);

        if (*wbs).epsd < 3 {
            let epsd = (*wbs).epsd as usize;
            for j in 0..NUMANIMS[epsd] as usize {
                let cfg = anim_config(epsd, j);
                let st = anim_state_ptr(epsd, j);
                for i in 0..cfg.nanims as usize {
                    if (*wbs).epsd != 1 || j != 8 {
                        c_write!(name, "WIA{}{:02}{:02}", (*wbs).epsd, j, i);
                        callback(name.as_mut_ptr(), &mut (*st).p[i]);
                    } else {
                        (*st).p[i] = (*anim_state_ptr(1, 4)).p[i];
                    }
                }
            }
        }
    }

    callback(DEH_String(c"WIMINUS".as_ptr().cast_mut()), &mut wiminus);

    for i in 0..10i32 {
        DEH_snprintf!(name, "WINUM{}", i);
        callback(name.as_mut_ptr(), &mut num[i as usize]);
    }

    callback(DEH_String(c"WIPCNT".as_ptr().cast_mut()), &mut percent);
    callback(DEH_String(c"WIF".as_ptr().cast_mut()), &mut finished);
    callback(DEH_String(c"WIENTER".as_ptr().cast_mut()), &mut entering);
    callback(DEH_String(c"WIOSTK".as_ptr().cast_mut()), &mut kills);
    callback(DEH_String(c"WIOSTS".as_ptr().cast_mut()), &mut secret);
    callback(DEH_String(c"WISCRT2".as_ptr().cast_mut()), &mut sp_secret);

    if W_CheckNumForName(DEH_String(c"WIOBJ".as_ptr().cast_mut())) >= 0 {
        if netgame != 0 && deathmatch == 0 {
            callback(DEH_String(c"WIOBJ".as_ptr().cast_mut()), &mut items);
        } else {
            callback(DEH_String(c"WIOSTI".as_ptr().cast_mut()), &mut items);
        }
    } else {
        callback(DEH_String(c"WIOSTI".as_ptr().cast_mut()), &mut items);
    }

    callback(DEH_String(c"WIFRGS".as_ptr().cast_mut()), &mut frags);
    callback(DEH_String(c"WICOLON".as_ptr().cast_mut()), &mut colon);
    callback(DEH_String(c"WITIME".as_ptr().cast_mut()), &mut timepatch);
    callback(DEH_String(c"WISUCKS".as_ptr().cast_mut()), &mut sucks);
    callback(DEH_String(c"WIPAR".as_ptr().cast_mut()), &mut par);
    callback(DEH_String(c"WIKILRS".as_ptr().cast_mut()), &mut killers);
    callback(DEH_String(c"WIVCTMS".as_ptr().cast_mut()), &mut victims);
    callback(DEH_String(c"WIMSTT".as_ptr().cast_mut()), &mut total);

    for i in 0..MAXPLAYERS {
        DEH_snprintf!(name, "STPB{}", i);
        callback(name.as_mut_ptr(), &mut p[i]);
        DEH_snprintf!(name, "WIBP{}", i + 1);
        callback(name.as_mut_ptr(), &mut bp[i]);
    }

    if gamemode == d_mode::commercial || (gamemode == d_mode::retail && (*wbs).epsd == 3) {
        M_StringCopy(
            name.as_mut_ptr(),
            DEH_String(c"INTERPIC".as_ptr().cast_mut()),
            name.len(),
        );
    } else {
        DEH_snprintf!(name, "WIMAP{}", (*wbs).epsd);
    }

    callback(name.as_mut_ptr(), &mut background);
}

/// Load callback: cache the named lump at `PU_STATIC` priority and store the pointer.
///
/// # Safety
///
/// Caller must ensure `name` is a valid NUL-terminated C-string pointer
/// recognised by `W_CacheLumpName`, and `variable` is a valid, non-null,
/// properly aligned pointer to a `*mut patch_t` slot the function may
/// overwrite.
unsafe extern "C" fn WI_loadCallback(name: *mut c_char, variable: *mut *mut patch_t) {
    *variable = W_CacheLumpName(name, PU_STATIC) as *mut patch_t;
}

/// Allocate the `lnames` pointer array and cache all intermission WAD patches.
///
/// Must be called before the first `WI_Drawer` call. `lnames` is allocated from
/// the zone heap at `PU_STATIC` with a size matching either `NUMCMAPS` (commercial)
/// or `NUMMAPS` (episode).  Also loads the `star`/`bstar` patches directly.
///
/// # Safety
///
/// Allocates `lnames` via `Z_Malloc` and mutates every cached patch global
/// (`star`, `bstar`, and everything touched by `WI_loadUnloadData`). Caller
/// must ensure the WAD and zone-memory subsystems are initialised and that
/// `wbs` is valid (via `WI_initVariables`).
#[no_mangle]
pub unsafe extern "C" fn WI_loadData() {
    if gamemode == d_mode::commercial {
        NUMCMAPS = 32;
        lnames = Z_Malloc(
            (std::mem::size_of::<*mut patch_t>() * NUMCMAPS as usize) as c_int,
            PU_STATIC,
            ptr::null_mut(),
        ) as *mut *mut patch_t;
    } else {
        lnames = Z_Malloc(
            (std::mem::size_of::<*mut patch_t>() * NUMMAPS) as c_int,
            PU_STATIC,
            ptr::null_mut(),
        ) as *mut *mut patch_t;
    }

    WI_loadUnloadData(WI_loadCallback);

    star = W_CacheLumpName(DEH_String(c"STFST01".as_ptr().cast_mut()), PU_STATIC) as *mut patch_t;
    bstar = W_CacheLumpName(DEH_String(c"STFDEAD0".as_ptr().cast_mut()), PU_STATIC) as *mut patch_t;
}

/// Unload callback: release the named lump from the WAD cache and null the pointer.
///
/// # Safety
///
/// Caller must ensure `name` is a valid NUL-terminated C-string pointer
/// previously cached via `W_CacheLumpName`, and `variable` is a valid,
/// non-null, properly aligned pointer to a `*mut patch_t` slot that this
/// function may overwrite with null.
unsafe extern "C" fn WI_unloadCallback(name: *mut c_char, variable: *mut *mut patch_t) {
    W_ReleaseLumpName(name);
    *variable = ptr::null_mut();
}

/// Release all intermission WAD patches loaded by `WI_loadData`.
///
/// Called indirectly by `WI_End` at the close of the intermission screen.
///
/// # Safety
///
/// Nulls every cached patch pointer in the intermission globals and
/// releases their WAD lumps. Caller must ensure no draw or update
/// functions are still executing.
#[no_mangle]
pub unsafe extern "C" fn WI_unloadData() {
    WI_loadUnloadData(WI_unloadCallback);
}

// ---------------------------------------------------------------------------
// WI_Drawer
// ---------------------------------------------------------------------------

/// Draw the intermission screen for the current frame.
///
/// Dispatches to the appropriate draw function based on `state` and `deathmatch`/`netgame` flags.
/// Called by `D_Display` in `d_main.c` every frame while `gamestate == GS_INTERMISSION`.
///
/// # Safety
///
/// Reads the global `state`, `deathmatch`, and `netgame` flags and
/// dispatches to one of the per-phase draw functions, each of which
/// dereferences the intermission globals (`wbs`, patch pointers, etc.).
/// Caller must ensure `WI_Start` has been invoked.
#[no_mangle]
pub unsafe extern "C" fn WI_Drawer() {
    match state {
        stateenum_t::StatCount => {
            if deathmatch != 0 {
                WI_drawDeathmatchStats();
            } else if netgame != 0 {
                WI_drawNetgameStats();
            } else {
                WI_drawStats();
            }
        }
        stateenum_t::ShowNextLoc => {
            WI_drawShowNextLoc();
        }
        stateenum_t::NoState => {
            WI_drawNoState();
        }
    }
}

// ---------------------------------------------------------------------------
// WI_initVariables / WI_Start
// ---------------------------------------------------------------------------

/// Initialise all intermission globals from the level-start record `wbstartstruct`.
///
/// Clamps all max-count fields to a minimum of 1 to avoid division-by-zero in
/// percentage calculations. Adjusts `wbs.epsd` downward by 3 for non-retail
/// builds that were given an out-of-range episode number.
///
/// # Safety
///
/// Caller must ensure `wbstartstruct` is a valid, non-null, properly aligned
/// pointer to an initialised `wbstartstruct_t` that remains live for the
/// duration of the intermission. Mutates the global state pointers `wbs`,
/// `plrs` and the counter globals (`acceleratestage`, `cnt`, `bcnt`,
/// `firstrefresh`, `me`).
unsafe fn WI_initVariables(wbstartstruct: *mut wbstartstruct_t) {
    wbs = wbstartstruct;
    plrs = (*wbs).plyr.as_mut_ptr();

    acceleratestage = 0;
    cnt = 0;
    bcnt = 0;
    firstrefresh = 1;
    me = (*wbs).pnum;

    if (*wbs).maxkills == 0 {
        (*wbs).maxkills = 1;
    }
    if (*wbs).maxitems == 0 {
        (*wbs).maxitems = 1;
    }
    if (*wbs).maxsecret == 0 {
        (*wbs).maxsecret = 1;
    }

    if gamemode != d_mode::retail && (*wbs).epsd > 2 {
        (*wbs).epsd -= 3;
    }
}

/// Start the intermission screen for a newly completed level.
///
/// Initialises all state variables, loads WAD patches, and enters the appropriate
/// stats phase (deathmatch, netgame, or single-player). Called from `G_WorldDone`
/// in `g_game.c`.
///
/// # Safety
///
/// Caller must ensure `wbstartstruct` is a valid, non-null, properly aligned
/// pointer to an initialised `wbstartstruct_t` whose lifetime spans the
/// intermission. Mutates every intermission global via `WI_initVariables`,
/// `WI_loadData`, and the per-mode `WI_init*Stats` helpers; the WAD, zone,
/// and sound subsystems must be initialised.
#[no_mangle]
pub unsafe extern "C" fn WI_Start(wbstartstruct: *mut wbstartstruct_t) {
    WI_initVariables(wbstartstruct);
    WI_loadData();

    if deathmatch != 0 {
        WI_initDeathmatchStats();
    } else if netgame != 0 {
        WI_initNetgameStats();
    } else {
        WI_initStats();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epsd0_config_entry0_fields() {
        let c = &EPSD0_CONFIG[0];
        assert_eq!(c.type_, animenum_t::ANIM_ALWAYS);
        assert_eq!(c.nanims, 3);
        assert_eq!(c.loc.x, 224);
        assert_eq!(c.loc.y, 104);
    }

    #[test]
    fn epsd1_config_entry0_is_level_anim() {
        let c = &EPSD1_CONFIG[0];
        assert_eq!(c.type_, animenum_t::ANIM_LEVEL);
        assert_eq!(c.data1, 1);
    }

    #[test]
    fn epsd2_last_entry_has_quarter_ticrate_period() {
        let last = &EPSD2_CONFIG[EPSD2_NANIM - 1];
        assert_eq!(last.period, TICRATE / 4);
    }

    #[test]
    fn anim_state_initializes_zeroed() {
        unsafe {
            let st = anim_state_ptr(0, 0);
            assert_eq!((*st).ctr, 0);
            assert_eq!((*st).nexttic, 0);
            assert_eq!((*st).lastdrawn, 0);
            assert_eq!((*st).state, 0);
        }
    }
}
