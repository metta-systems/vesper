//! Rust port of vendor/doomgeneric/wi_stuff.c.
//!
//! Intermission / victory screen: kill/item/secret percentages, par time, animated
//! episode map backgrounds, and level-entry/exit animations. Supports single-player,
//! cooperative netgame, and deathmatch scoring modes for up to 4 players.
//!
//! Notable Rust-vs-C differences:
//! - The C `anim_t` struct is split here into an immutable `anim_config_t`
//!   (compile-time data in `static` arrays) and a mutable `anim_state_t`
//!   (runtime state, kept in [`IntermissionState`] with the other C file
//!   statics).
//! - `DEH_String` (from `dehacked`) and `SHORT` are identities (Dehacked and
//!   endianness conversion are not yet active in this port).

#![allow(
    non_upper_case_globals,
    non_snake_case,
    non_camel_case_types,
    clippy::missing_safety_doc,
    clippy::needless_range_loop,
    clippy::ptr_offset_with_cast,
    clippy::manual_clamp,
    clippy::manual_c_str_literals
)]

pub mod statdump;

use {crate::engine::sound::info::Sfx, std::ptr, zerocopy::little_endian::I16};

use crate::{
    DEH_snprintf, c_write,
    engine::{
        dehacked::DEH_String,
        doom::{self, event::event_t, player::MAXPLAYERS, stat::gamemode},
        game::{G_WorldDone, deathmatch, netgame, player as game_player, playeringame},
        global::Global,
        interface::{
            timer::TICRATE,
            video::{SCREENHEIGHT, SCREENWIDTH},
        },
        misc::random::M_Random,
        render_2d::V_DrawPatch,
        sound::{S_ChangeMusic, S_StartNonPositionalSound, info::Mus},
        wad::{W_CacheLumpName, W_CheckNumForName, W_ReleaseLumpName, format::patch_t},
        zone::{PU_STATIC, Z_Malloc},
    },
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Number of Doom episodes (E1-E4, matches C `NUMEPISODES`).
pub const NUMEPISODES: usize = 4;
/// Maps per episode (1-9, matches C `NUMMAPS`).
pub const NUMMAPS: usize = 9;

/// Screen y coordinate of the level-name title patch (matches C `WI_TITLEY`).
pub const WI_TITLEY: i32 = 2;
/// Vertical pixel spacing between player rows in the netgame and deathmatch views (matches C `WI_SPACINGY`).
pub const WI_SPACINGY: i32 = 33;

/// Screen x of the stats column in single-player view (matches C `SP_STATSX`).
pub const SP_STATSX: i32 = 50;
/// Screen y of the stats area in single-player view (matches C `SP_STATSY`).
pub const SP_STATSY: i32 = 50;
/// Screen x of the time display in single-player view (matches C `SP_TIMEX`).
pub const SP_TIMEX: i32 = 16;
/// Screen y of the time display in single-player view (matches C `SP_TIMEY`).
pub const SP_TIMEY: i32 = SCREENHEIGHT - 32;

/// Screen y of the stats header row in netgame view (matches C `NG_STATSY`).
const NG_STATSY: i32 = 50;
/// Horizontal pixel spacing between stat columns in netgame view (matches C `NG_SPACINGX`).
const NG_SPACINGX: i32 = 64;

/// Screen x of the frag matrix top-left in deathmatch view (matches C `DM_MATRIXX`).
const DM_MATRIXX: i32 = 42;
/// Screen y of the frag matrix top in deathmatch view (matches C `DM_MATRIXY`).
const DM_MATRIXY: i32 = 68;
/// Horizontal spacing between columns in the deathmatch frag matrix (matches C `DM_SPACINGX`).
pub const DM_SPACINGX: i32 = 40;
/// Screen x of the "Totals" column in deathmatch view (matches C `DM_TOTALSX`).
const DM_TOTALSX: i32 = 269;
/// Screen x of the "Killers" label in deathmatch view (matches C `DM_KILLERSX`).
const DM_KILLERSX: i32 = 10;
/// Screen y of the "Killers" label in deathmatch view (matches C `DM_KILLERSY`).
const DM_KILLERSY: i32 = 100;
/// Screen x of the "Victims" label in deathmatch view (matches C `DM_VICTIMSX`).
const DM_VICTIMSX: i32 = 5;
/// Screen y of the "Victims" label in deathmatch view (matches C `DM_VICTIMSY`).
const DM_VICTIMSY: i32 = 50;

/// Number of seconds (in `TICRATE` units) the "Show Next Location" map is displayed (matches C `SHOWNEXTLOCDELAY`).
pub const SHOWNEXTLOCDELAY: i32 = 4;

// ---------------------------------------------------------------------------
// Intermission input types (filled in by `G_DoCompleted`)
// ---------------------------------------------------------------------------

/// Per-player intermission data passed in from the game loop (C typedef `wbplayerstruct_t`).
#[derive(Clone, Copy)]
pub struct wbplayerstruct_t {
    /// Non-zero if this player slot is in use.
    pub in_: i32,
    /// Number of kills this player scored on the level.
    pub skills: i32,
    /// Number of items this player collected.
    pub sitems: i32,
    /// Number of secrets this player found.
    pub ssecret: i32,
    /// Elapsed level time in tics.
    pub stime: i32,
    /// Frag counts against each of the 4 possible players.
    pub frags: [i32; 4],
    /// Unused score field, kept from the C struct.
    pub score: i32,
}

/// Overall intermission input record passed from `G_WorldDone` (C typedef `wbstartstruct_t`).
#[derive(Clone, Copy)]
pub struct wbstartstruct_t {
    /// Episode index (0-based).
    pub epsd: i32,
    /// Non-zero if the player found the secret level on this episode.
    pub didsecret: i32,
    /// Index of the map the player just completed (0-based).
    pub last: i32,
    /// Index of the map the player is going to next (0-based).
    pub next: i32,
    /// Total killable monsters on the level (denominator for kill percentage).
    pub maxkills: i32,
    /// Total collectible items on the level (denominator for item percentage).
    pub maxitems: i32,
    /// Total secrets on the level (denominator for secret percentage).
    pub maxsecret: i32,
    /// Maximum frag count (not used in single-player).
    pub maxfrags: i32,
    /// Par time for the level in tics.
    pub partime: i32,
    /// Console player number (0-based).
    pub pnum: i32,
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
    x: i32,
    /// Vertical screen pixel.
    y: i32,
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
    kind: animenum_t,
    /// Tics between frame advances (or between random retriggers for `ANIM_RANDOM`).
    period: i32,
    /// Number of patch frames in this animation (1-3).
    nanims: i32,
    /// Screen position where the animation is drawn.
    loc: point_t,
    /// For `ANIM_LEVEL`: the map index that triggers playback.
    /// For `ANIM_RANDOM`: the max additional delay (in tics) before the next play.
    data1: i32,
    /// For `ANIM_RANDOM`: the minimum delay (in tics) before the next play.
    data2: i32,
}

/// Mutable per-animation runtime state (zeroed at startup, written every frame).
///
/// Corresponds to the mutable fields of C `anim_t` in `wi_stuff.c`.
#[derive(Clone, Copy)]
struct anim_state_t {
    /// Cached patch pointers for each frame; loaded by `WI_loadData`.
    p: [*mut patch_t; 3],
    /// Game tic on which the next frame advance is scheduled.
    nexttic: i32,
    /// Frame index drawn on the previous tic (unused in this port; kept for layout parity).
    lastdrawn: i32,
    /// Current frame index within `p` (-1 = not yet started).
    ctr: i32,
    /// Internal sub-state for `ANIM_RANDOM` sequencing.
    state: i32,
}

/// Zero-initialised animation state used to fill state tables at program start.
const ZERO_STATE: anim_state_t = anim_state_t {
    p: [ptr::null_mut(); 3],
    nexttic: 0,
    lastdrawn: 0,
    ctr: 0,
    state: 0,
};

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
pub const EPSD0_NANIM: usize = 10;
/// Number of animated patches in Episode 2's background (matches C `NUMANIMS[1]`).
pub const EPSD1_NANIM: usize = 9;
/// Number of animated patches in Episode 3's background (matches C `NUMANIMS[2]`).
pub const EPSD2_NANIM: usize = 6;

/// Animation configurations for Episode 1's background overlay (C `epsd0animinfo`).
static EPSD0_CONFIG: [anim_config_t; EPSD0_NANIM] = [
    anim_config_t {
        kind: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 224, y: 104 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 184, y: 160 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 112, y: 136 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 72, y: 112 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 88, y: 96 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 64, y: 48 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 192, y: 40 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 136, y: 16 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 80, y: 16 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_ALWAYS,
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
        kind: animenum_t::ANIM_LEVEL,
        period: TICRATE / 3,
        nanims: 1,
        loc: point_t { x: 128, y: 136 },
        data1: 1,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_LEVEL,
        period: TICRATE / 3,
        nanims: 1,
        loc: point_t { x: 128, y: 136 },
        data1: 2,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_LEVEL,
        period: TICRATE / 3,
        nanims: 1,
        loc: point_t { x: 128, y: 136 },
        data1: 3,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_LEVEL,
        period: TICRATE / 3,
        nanims: 1,
        loc: point_t { x: 128, y: 136 },
        data1: 4,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_LEVEL,
        period: TICRATE / 3,
        nanims: 1,
        loc: point_t { x: 128, y: 136 },
        data1: 5,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_LEVEL,
        period: TICRATE / 3,
        nanims: 1,
        loc: point_t { x: 128, y: 136 },
        data1: 6,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_LEVEL,
        period: TICRATE / 3,
        nanims: 1,
        loc: point_t { x: 128, y: 136 },
        data1: 7,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_LEVEL,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 192, y: 144 },
        data1: 8,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_LEVEL,
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
        kind: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 104, y: 168 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 40, y: 136 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 160, y: 96 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 104, y: 80 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 3,
        nanims: 3,
        loc: point_t { x: 120, y: 32 },
        data1: 0,
        data2: 0,
    },
    anim_config_t {
        kind: animenum_t::ANIM_ALWAYS,
        period: TICRATE / 4,
        nanims: 3,
        loc: point_t { x: 40, y: 0 },
        data1: 0,
        data2: 0,
    },
];

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

/// Count of background animations per episode, indexed by episode number (C `numanims`).
/// Episode 3 (index 3) has no animations (0).
static NUMANIMS: [i32; NUMEPISODES] = [
    EPSD0_NANIM as i32,
    EPSD1_NANIM as i32,
    EPSD2_NANIM as i32,
    0,
];

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Intermission state (C `wi_stuff.c` file statics).
pub struct IntermissionState {
    /// Non-zero when the player pressed fire/use to skip the current count-up animation.
    acceleratestage: i32,
    /// Console player index (0-based); set from `wbs.pnum` by `WI_initVariables`.
    me: i32,
    /// Current intermission state machine state.
    state: stateenum_t,
    /// Pointer to the level-start record filled by `G_WorldDone`; valid for the duration of the intermission.
    wbs: *mut wbstartstruct_t,
    /// Pointer to `wbs.plyr[0]`; used to index per-player stats by offset.
    plrs: *mut wbplayerstruct_t,
    /// Generic countdown used in `NoState` and `ShowNextLoc` states.
    cnt: i32,
    /// Background animation beat counter; incremented every tic by `WI_Ticker`.
    bcnt: i32,
    /// Non-zero on the first draw call after `WI_Start`, triggers a full background blit.
    firstrefresh: i32,
    /// Running kill-percentage display values (count up toward actual percentage).
    cnt_kills: [i32; MAXPLAYERS],
    /// Running item-percentage display values.
    cnt_items: [i32; MAXPLAYERS],
    /// Running secret-percentage display values.
    cnt_secret: [i32; MAXPLAYERS],
    /// Running displayed level time in seconds (counts up from 0).
    cnt_time: i32,
    /// Running displayed par time in seconds (counts up from 0).
    cnt_par: i32,
    /// Tic countdown used as a pause between successive stat reveals.
    cnt_pause: i32,
    /// Number of Doom II maps to load level-name patches for (32 for the
    /// retail release; C `NUMCMAPS`).
    numcmaps: i32,
    /// Mutable runtime animation states for Episode 1's background animations.
    epsd0_anims: [anim_state_t; EPSD0_NANIM],
    /// Mutable runtime animation states for Episode 2's background animations.
    epsd1_anims: [anim_state_t; EPSD1_NANIM],
    /// Mutable runtime animation states for Episode 3's background animations.
    epsd2_anims: [anim_state_t; EPSD2_NANIM],

    // Cached WAD patches (loaded by WI_loadData, released by WI_unloadData)
    /// "You Are Here" arrow patches (2 frames, `WIURH0`/`WIURH1`); third slot unused.
    yah: [*mut patch_t; 3],
    /// Completed-level splat patches (`WISPLAT`); second slot unused.
    splat: [*mut patch_t; 2],
    /// Percent sign patch (`WIPCNT`).
    percent: *mut patch_t,
    /// Colon separator patch (`WICOLON`) for time display.
    colon: *mut patch_t,
    /// Digit patches 0-9 (`WINUM0`-`WINUM9`).
    num: [*mut patch_t; 10],
    /// Minus sign patch (`WIMINUS`) for negative frag counts.
    wiminus: *mut patch_t,
    /// "Finished" label patch (`WIF`).
    finished: *mut patch_t,
    /// "Entering" label patch (`WIENTER`).
    entering: *mut patch_t,
    /// Single-player secret label patch (`WISCRT2`).
    sp_secret: *mut patch_t,
    /// Kills column header patch (`WIOSTK`).
    kills: *mut patch_t,
    /// Secrets column header patch (`WIOSTS`).
    secret: *mut patch_t,
    /// Items column header patch (`WIOSTI` or `WIOBJ` in co-op).
    items: *mut patch_t,
    /// Frags column header patch (`WIFRGS`).
    frags: *mut patch_t,
    /// Time label patch (`WITIME`).
    timepatch: *mut patch_t,
    /// Par-time label patch (`WIPAR`).
    par: *mut patch_t,
    /// "Sucks" patch displayed when the level time exceeds the representable maximum (`WISUCKS`).
    sucks: *mut patch_t,
    /// "Killers" row label patch (`WIKILRS`) used in deathmatch view.
    killers: *mut patch_t,
    /// "Victims" column label patch (`WIVCTMS`) used in deathmatch view.
    victims: *mut patch_t,
    /// "Total" column header patch (`WIMSTT`) used in deathmatch view.
    total: *mut patch_t,
    /// "You are here" star patch (`STFST01`) marking the local player in netgame view.
    star: *mut patch_t,
    /// Dead-face patch (`STFDEAD0`) marking the local player when dead.
    bstar: *mut patch_t,
    /// Player face patches for each slot (`STPB0`-`STPB3`).
    p: [*mut patch_t; MAXPLAYERS],
    /// Alternative player face patches for each slot (`WIBP1`-`WIBP4`).
    bp: [*mut patch_t; MAXPLAYERS],
    /// Heap-allocated array of level-name patches (`WILV##` or `CWILV##`); length is `numcmaps` or `NUMMAPS`.
    lnames: *mut *mut patch_t,
    /// Background map graphic for the current episode (`WIMAP#` or `INTERPIC`).
    background: *mut patch_t,

    /// Deathmatch count-up sub-state index (odd = pause, even = ticking).
    dm_state: i32,
    /// Running frag-count display for each `[killer][victim]` pair in deathmatch view.
    dm_frags: [[i32; MAXPLAYERS]; MAXPLAYERS],
    /// Running per-player frag totals for deathmatch view.
    dm_totals: [i32; MAXPLAYERS],
    /// Running frag-count display per player in cooperative netgame view.
    cnt_frags: [i32; MAXPLAYERS],
    /// Non-zero when at least one player has a non-zero frag count (gates frags column display).
    dofrags: i32,
    /// Netgame count-up sub-state index.
    ng_state: i32,
    /// Single-player count-up sub-state index.
    sp_state: i32,
    /// Non-zero when the "you are here" pointer should be drawn in `ShowNextLoc` state.
    snl_pointeron: bool,
}

impl IntermissionState {
    /// The state at program start: no intermission running, nothing loaded.
    pub const fn new() -> Self {
        Self {
            acceleratestage: 0,
            me: 0,
            state: stateenum_t::NoState,
            wbs: ptr::null_mut(),
            plrs: ptr::null_mut(),
            cnt: 0,
            bcnt: 0,
            firstrefresh: 0,
            cnt_kills: [0; MAXPLAYERS],
            cnt_items: [0; MAXPLAYERS],
            cnt_secret: [0; MAXPLAYERS],
            cnt_time: 0,
            cnt_par: 0,
            cnt_pause: 0,
            numcmaps: 0,
            epsd0_anims: [ZERO_STATE; EPSD0_NANIM],
            epsd1_anims: [ZERO_STATE; EPSD1_NANIM],
            epsd2_anims: [ZERO_STATE; EPSD2_NANIM],
            yah: [ptr::null_mut(); 3],
            splat: [ptr::null_mut(); 2],
            percent: ptr::null_mut(),
            colon: ptr::null_mut(),
            num: [ptr::null_mut(); 10],
            wiminus: ptr::null_mut(),
            finished: ptr::null_mut(),
            entering: ptr::null_mut(),
            sp_secret: ptr::null_mut(),
            kills: ptr::null_mut(),
            secret: ptr::null_mut(),
            items: ptr::null_mut(),
            frags: ptr::null_mut(),
            timepatch: ptr::null_mut(),
            par: ptr::null_mut(),
            sucks: ptr::null_mut(),
            killers: ptr::null_mut(),
            victims: ptr::null_mut(),
            total: ptr::null_mut(),
            star: ptr::null_mut(),
            bstar: ptr::null_mut(),
            p: [ptr::null_mut(); MAXPLAYERS],
            bp: [ptr::null_mut(); MAXPLAYERS],
            lnames: ptr::null_mut(),
            background: ptr::null_mut(),
            dm_state: 0,
            dm_frags: [[0; MAXPLAYERS]; MAXPLAYERS],
            dm_totals: [0; MAXPLAYERS],
            cnt_frags: [0; MAXPLAYERS],
            dofrags: 0,
            ng_state: 0,
            sp_state: 0,
            snl_pointeron: false,
        }
    }

    /// The runtime state of animation `j` in episode `epsd`.
    ///
    /// Panics if `epsd` is not 0, 1 or 2, or `j` is out of bounds for it.
    fn anim_state(&self, epsd: usize, j: usize) -> &anim_state_t {
        match epsd {
            0 => &self.epsd0_anims[j],
            1 => &self.epsd1_anims[j],
            2 => &self.epsd2_anims[j],
            _ => panic!("anim_state: invalid episode {epsd}"),
        }
    }

    /// The runtime state of animation `j` in episode `epsd`, for writing.
    ///
    /// Panics if `epsd` is not 0, 1 or 2, or `j` is out of bounds for it.
    fn anim_state_mut(&mut self, epsd: usize, j: usize) -> &mut anim_state_t {
        match epsd {
            0 => &mut self.epsd0_anims[j],
            1 => &mut self.epsd1_anims[j],
            2 => &mut self.epsd2_anims[j],
            _ => panic!("anim_state_mut: invalid episode {epsd}"),
        }
    }
}

impl Default for IntermissionState {
    fn default() -> Self {
        Self::new()
    }
}

/// The intermission state.
pub static INTERMISSION: Global<IntermissionState> = Global::new(IntermissionState::new());

/// Play `sounds` in order, after the caller has released its guard.
///
/// The count-up updaters collect their sounds while they hold the
/// intermission state and play them once they are done with it. The sound
/// code reads neither that state nor the random number generator, so the
/// later start changes nothing else.
fn play_sounds(sounds: &[Sfx]) {
    for &sound in sounds {
        S_StartNonPositionalSound(sound as i32);
    }
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
/// Reads the `background` patch pointer and calls `V_DrawPatch`.
/// Caller must ensure `WI_loadData` has cached the patch and the video
/// backend is ready.
unsafe fn WI_slamBackground(wi: &IntermissionState) {
    V_DrawPatch(0, 0, wi.background);
}

// ---------------------------------------------------------------------------
// WI_Responder
// ---------------------------------------------------------------------------

/// Intermission event responder - always returns 0 (all input goes through `WI_checkForAccelerate`).
///
/// Called by `G_Responder` in `g_game.c`.
///
/// `_ev` is not dereferenced, so any pointer (including null) is accepted.
pub fn WI_Responder(_ev: *mut event_t) -> i32 {
    0
}

// ---------------------------------------------------------------------------
// WI_drawLF
// ---------------------------------------------------------------------------

/// Draw the "Finished" overlay: level name and "Finished" text at the top of the screen.
///
/// In commercial mode, does nothing for MAP33 and triggers an intentional patch
/// bounds error for map numbers above `numcmaps` (preserving vanilla quirk - the
/// out-of-range patch lookup is an upstream bug retained for demo compatibility).
///
/// # Safety
///
/// Dereferences the `wbs` pointer plus the `lnames` array and the
/// `finished` patch pointer. Caller must ensure `WI_Start`/`WI_loadData`
/// have run so these are valid.
unsafe fn WI_drawLF(wi: &IntermissionState) {
    let wbs = wi.wbs;
    let lnames = wi.lnames;
    let finished = wi.finished;
    let numcmaps = wi.numcmaps;
    let mut y = WI_TITLEY;

    if gamemode() != doom::mode::commercial || (*wbs).last < numcmaps {
        V_DrawPatch(
            (SCREENWIDTH - (*(*lnames.offset((*wbs).last as isize))).width.get() as i32) / 2,
            y,
            *lnames.offset((*wbs).last as isize),
        );
        y += (5 * (*(*lnames.offset((*wbs).last as isize))).height.get() as i32) / 4;
        V_DrawPatch(
            (SCREENWIDTH - (*finished).width.get() as i32) / 2,
            y,
            finished,
        );
    } else if (*wbs).last == numcmaps {
        // MAP33 - nothing is displayed!
    } else if (*wbs).last > numcmaps {
        // Deliberately trigger a V_DrawPatch error
        let tmp: patch_t = patch_t {
            width: I16::new(SCREENWIDTH as i16),
            height: I16::new(SCREENHEIGHT as i16),
            leftoffset: I16::new(1),
            topoffset: I16::new(1),
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
/// Dereferences the `wbs` pointer plus the `entering` patch and the
/// `lnames` array. Caller must ensure `WI_Start`/`WI_loadData` have run so
/// these are valid.
unsafe fn WI_drawEL(wi: &IntermissionState) {
    let wbs = wi.wbs;
    let lnames = wi.lnames;
    let entering = wi.entering;
    let mut y = WI_TITLEY;

    V_DrawPatch(
        (SCREENWIDTH - (*entering).width.get() as i32) / 2,
        y,
        entering,
    );

    y += (5 * (*(*lnames.offset((*wbs).next as isize))).height.get() as i32) / 4;

    V_DrawPatch(
        (SCREENWIDTH - (*(*lnames.offset((*wbs).next as isize))).width.get() as i32) / 2,
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
/// Dereferences `wbs` for the current episode and reads `LNODES`.
unsafe fn WI_drawOnLnode(wbs: *mut wbstartstruct_t, n: i32, c: *const *mut patch_t) {
    let mut i = 0;
    let mut fits = false;

    loop {
        let left = LNODES[(*wbs).epsd as usize][n as usize].x
            - (**c.offset(i as isize)).leftoffset.get() as i32;
        let top = LNODES[(*wbs).epsd as usize][n as usize].y
            - (**c.offset(i as isize)).topoffset.get() as i32;
        let right = left + (**c.offset(i as isize)).width.get() as i32;
        let bottom = top + (**c.offset(i as isize)).height.get() as i32;

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
        print!("Could not place patch on level {}", n + 1);
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
/// Dereferences the `wbs` pointer. Caller must ensure `wbs` has been
/// initialised (typically by `WI_initVariables`).
unsafe fn WI_initAnimatedBack(wi: &mut IntermissionState) {
    if gamemode() == doom::mode::commercial {
        return;
    }
    if (*wi.wbs).epsd > 2 {
        return;
    }

    let epsd = (*wi.wbs).epsd as usize;
    let count = NUMANIMS[epsd] as usize;
    let bcnt = wi.bcnt;

    for i in 0..count {
        let cfg = anim_config(epsd, i);
        let st = wi.anim_state_mut(epsd, i);
        st.ctr = -1;
        if cfg.kind == animenum_t::ANIM_ALWAYS {
            st.nexttic = bcnt + 1 + (M_Random() % cfg.period);
        } else if cfg.kind == animenum_t::ANIM_RANDOM {
            st.nexttic = bcnt + 1 + cfg.data2 + (M_Random() % cfg.data1);
        } else if cfg.kind == animenum_t::ANIM_LEVEL {
            st.nexttic = bcnt + 1;
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
/// Dereferences the `wbs` pointer. Caller must ensure `wbs` is valid and the
/// animation state was initialised by `WI_initAnimatedBack`.
unsafe fn WI_updateAnimatedBack(wi: &mut IntermissionState) {
    if gamemode() == doom::mode::commercial {
        return;
    }
    let wbs = wi.wbs;
    if (*wbs).epsd > 2 {
        return;
    }

    let epsd = (*wbs).epsd as usize;
    let count = NUMANIMS[epsd] as usize;
    let bcnt = wi.bcnt;
    let counting_stats = wi.state == stateenum_t::StatCount;

    for i in 0..count {
        let cfg = anim_config(epsd, i);
        let st = wi.anim_state_mut(epsd, i);
        if bcnt == st.nexttic {
            match cfg.kind {
                animenum_t::ANIM_ALWAYS => {
                    st.ctr += 1;
                    if st.ctr >= cfg.nanims {
                        st.ctr = 0;
                    }
                    st.nexttic = bcnt + cfg.period;
                }
                animenum_t::ANIM_RANDOM => {
                    st.ctr += 1;
                    if st.ctr == cfg.nanims {
                        st.ctr = -1;
                        st.nexttic = bcnt + cfg.data2 + (M_Random() % cfg.data1);
                    } else {
                        st.nexttic = bcnt + cfg.period;
                    }
                }
                animenum_t::ANIM_LEVEL => {
                    // gawd-awful hack for level anims
                    if !(counting_stats && i == 7) && (*wbs).next == cfg.data1 {
                        st.ctr += 1;
                        if st.ctr == cfg.nanims {
                            st.ctr -= 1;
                        }
                        st.nexttic = bcnt + cfg.period;
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
/// Dereferences the `wbs` pointer and the patches the animation states
/// reference. Caller must ensure `wbs` is valid and animations have been
/// initialised by `WI_initAnimatedBack`.
unsafe fn WI_drawAnimatedBack(wi: &IntermissionState) {
    if gamemode() == doom::mode::commercial {
        return;
    }
    if (*wi.wbs).epsd > 2 {
        return;
    }

    let epsd = (*wi.wbs).epsd as usize;
    let count = NUMANIMS[epsd] as usize;

    for i in 0..count {
        let cfg = anim_config(epsd, i);
        let st = wi.anim_state(epsd, i);
        if st.ctr >= 0 {
            V_DrawPatch(cfg.loc.x, cfg.loc.y, st.p[st.ctr as usize]);
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
/// Reads the `num` digit patches and `wiminus` patch. Caller must ensure
/// `WI_loadData` has cached these patches before invoking.
unsafe fn WI_drawNum(
    wi: &IntermissionState,
    mut x: i32,
    y: i32,
    mut n: i32,
    mut digits: i32,
) -> i32 {
    let num = wi.num;
    let fontwidth = (*num[0]).width.get() as i32;
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
        V_DrawPatch(x, y, wi.wiminus);
    }

    x
}

/// Draw a percentage value: percent sign at `x`, then the number right-justified to the left of it.
///
/// No-op if `pct` is negative (stat not yet tallied).
///
/// # Safety
///
/// Reads the `percent` patch and delegates to `WI_drawNum`. Caller must
/// ensure `WI_loadData` has cached the patches.
unsafe fn WI_drawPercent(wi: &IntermissionState, x: i32, y: i32, pct: i32) {
    if pct < 0 {
        return;
    }
    V_DrawPatch(x, y, wi.percent);
    WI_drawNum(wi, x, y, pct, -1);
}

/// Draw elapsed time `t` (in seconds) right-justified at `(x, y)` in MM:SS format.
///
/// No-op if `t` is negative. If `t` exceeds the representable range (61 minutes 59 seconds),
/// draws the "SUCKS" patch instead.
///
/// # Safety
///
/// Reads the `colon` and `sucks` patches and delegates to `WI_drawNum`.
/// Caller must ensure `WI_loadData` has cached the patches.
unsafe fn WI_drawTime(wi: &IntermissionState, mut x: i32, y: i32, t: i32) {
    if t < 0 {
        return;
    }
    let colon = wi.colon;
    let sucks = wi.sucks;

    if t <= 61 * 59 {
        let mut div = 1;
        loop {
            let n = (t / div) % 60;
            x = WI_drawNum(wi, x, y, n, 2) - (*colon).width.get() as i32;
            div *= 60;
            if div == 60 || t / div != 0 {
                V_DrawPatch(x, y, colon);
            }
            if t / div == 0 {
                break;
            }
        }
    } else {
        V_DrawPatch(x - (*sucks).width.get() as i32, y, sucks);
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
pub unsafe fn WI_End() {
    WI_unloadData();
}

/// Enter the `NoState` phase: count down 10 tics then call `G_WorldDone`.
///
/// # Safety
///
/// Mutates the intermission state fields `state`, `acceleratestage`, and
/// `cnt`. Caller must ensure `WI_Start` has been invoked.
unsafe fn WI_initNoState(wi: &mut IntermissionState) {
    wi.state = stateenum_t::NoState;
    wi.acceleratestage = 0;
    wi.cnt = 10;
}

/// Tick the `NoState` phase; advances animations and calls `G_WorldDone` when `cnt` reaches 0.
///
/// # Safety
///
/// Mutates `cnt` and runs `WI_updateAnimatedBack` (which dereferences `wbs`
/// and the animation state). Caller must ensure `WI_Start` has been invoked.
unsafe fn WI_updateNoState() {
    let world_done = {
        let mut wi = INTERMISSION.borrow_mut();
        WI_updateAnimatedBack(&mut wi);
        wi.cnt -= 1;
        wi.cnt == 0
    };
    if world_done {
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
unsafe fn WI_initShowNextLoc(wi: &mut IntermissionState) {
    wi.state = stateenum_t::ShowNextLoc;
    wi.acceleratestage = 0;
    wi.cnt = SHOWNEXTLOCDELAY * TICRATE;
    WI_initAnimatedBack(wi);
}

/// Tick the `ShowNextLoc` phase; blinks the pointer and transitions to `NoState` when done.
///
/// # Safety
///
/// Mutates `cnt` and `snl_pointeron` and may transition to `NoState` via
/// `WI_initNoState`. Caller must ensure `WI_Start` has been invoked.
unsafe fn WI_updateShowNextLoc() {
    let mut wi = INTERMISSION.borrow_mut();
    WI_updateAnimatedBack(&mut wi);
    wi.cnt -= 1;
    if wi.cnt == 0 || wi.acceleratestage != 0 {
        WI_initNoState(&mut wi);
    } else {
        wi.snl_pointeron = (wi.cnt & 31) < 20;
    }
}

/// Draw the `ShowNextLoc` phase: episode map with completed-level splats and optional pointer.
///
/// # Safety
///
/// Dereferences the `wbs` pointer and the `splat`/`yah` patch arrays, and
/// delegates to `WI_slamBackground`, `WI_drawAnimatedBack`, `WI_drawEL`,
/// and `WI_drawOnLnode`. Caller must ensure `WI_Start`/`WI_loadData` have run.
unsafe fn WI_drawShowNextLoc(wi: &IntermissionState) {
    let wbs = wi.wbs;
    WI_slamBackground(wi);
    WI_drawAnimatedBack(wi);

    if gamemode() != doom::mode::commercial {
        if (*wbs).epsd > 2 {
            WI_drawEL(wi);
            return;
        }

        let last = if (*wbs).last == 8 {
            (*wbs).next - 1
        } else {
            (*wbs).last
        };

        for i in 0..=last {
            WI_drawOnLnode(wbs, i, wi.splat.as_ptr());
        }

        if (*wbs).didsecret != 0 {
            WI_drawOnLnode(wbs, 8, wi.splat.as_ptr());
        }

        if wi.snl_pointeron {
            WI_drawOnLnode(wbs, (*wbs).next, wi.yah.as_ptr());
        }
    }

    if gamemode() != doom::mode::commercial || (*wbs).next != 30 {
        WI_drawEL(wi);
    }
}

/// Draw the `NoState` phase: same as `ShowNextLoc` but with the pointer always visible.
///
/// # Safety
///
/// Mutates `snl_pointeron` and delegates to `WI_drawShowNextLoc`; same
/// invariants apply (intermission must be started and patches loaded).
unsafe fn WI_drawNoState() {
    INTERMISSION.borrow_mut().snl_pointeron = true;
    WI_drawShowNextLoc(&INTERMISSION.borrow());
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
/// (a pointer into `wbs.plyr`). Caller must ensure `plrs` points to a valid
/// array and that `playernum` is within `0..MAXPLAYERS`.
unsafe fn WI_fragSum(plrs: *mut wbplayerstruct_t, playernum: i32) -> i32 {
    let mut sum = 0;
    for i in 0..MAXPLAYERS {
        if playeringame(i) != 0 && i as i32 != playernum {
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
/// Mutates the intermission state fields (`state`, `acceleratestage`,
/// `dm_state`, `cnt_pause`, `dm_frags`, `dm_totals`) and reads
/// `playeringame`. Caller must ensure `WI_initVariables` has been invoked.
unsafe fn WI_initDeathmatchStats(wi: &mut IntermissionState) {
    wi.state = stateenum_t::StatCount;
    wi.acceleratestage = 0;
    wi.dm_state = 1;
    wi.cnt_pause = TICRATE;

    for i in 0..MAXPLAYERS {
        if playeringame(i) != 0 {
            for j in 0..MAXPLAYERS {
                if playeringame(j) != 0 {
                    wi.dm_frags[i][j] = 0;
                }
            }
            wi.dm_totals[i] = 0;
        }
    }

    WI_initAnimatedBack(wi);
}

/// Tick the deathmatch stats phase: count frag values up toward the actual totals.
///
/// State machine: odd states are pauses, state 2 ticks frags, state 4 waits for acceleration.
///
/// # Safety
///
/// Mutates the deathmatch-stats fields (`dm_frags`, `dm_totals`,
/// `dm_state`, `cnt_pause`, `acceleratestage`) and reads `plrs`,
/// `playeringame`, `gamemode`. Triggers sound effects via `S_StartSound`.
/// Caller must ensure `WI_initDeathmatchStats` has run and the sound
/// subsystem is initialised.
unsafe fn WI_updateDeathmatchStats() {
    let mut sounds = Vec::new();
    let mut wi = INTERMISSION.borrow_mut();
    WI_updateAnimatedBack(&mut wi);
    let plrs = wi.plrs;

    if wi.acceleratestage != 0 && wi.dm_state != 4 {
        wi.acceleratestage = 0;
        for i in 0..MAXPLAYERS {
            if playeringame(i) != 0 {
                for j in 0..MAXPLAYERS {
                    if playeringame(j) != 0 {
                        wi.dm_frags[i][j] = (*plrs.offset(i as isize)).frags[j];
                    }
                }
                wi.dm_totals[i] = WI_fragSum(plrs, i as i32);
            }
        }
        sounds.push(Sfx::Barexp);
        wi.dm_state = 4;
    }

    if wi.dm_state == 2 {
        if wi.bcnt & 3 == 0 {
            sounds.push(Sfx::Pistol);
        }
        let mut stillticking = false;
        for i in 0..MAXPLAYERS {
            if playeringame(i) != 0 {
                for j in 0..MAXPLAYERS {
                    if playeringame(j) != 0
                        && wi.dm_frags[i][j] != (*plrs.offset(i as isize)).frags[j]
                    {
                        if (*plrs.offset(i as isize)).frags[j] < 0 {
                            wi.dm_frags[i][j] -= 1;
                        } else {
                            wi.dm_frags[i][j] += 1;
                        }
                        if wi.dm_frags[i][j] > 99 {
                            wi.dm_frags[i][j] = 99;
                        }
                        if wi.dm_frags[i][j] < -99 {
                            wi.dm_frags[i][j] = -99;
                        }
                        stillticking = true;
                    }
                }
                wi.dm_totals[i] = WI_fragSum(plrs, i as i32);
                if wi.dm_totals[i] > 99 {
                    wi.dm_totals[i] = 99;
                }
                if wi.dm_totals[i] < -99 {
                    wi.dm_totals[i] = -99;
                }
            }
        }
        if !stillticking {
            sounds.push(Sfx::Barexp);
            wi.dm_state += 1;
        }
    } else if wi.dm_state == 4 {
        if wi.acceleratestage != 0 {
            sounds.push(Sfx::Slop);
            if gamemode() == doom::mode::commercial {
                WI_initNoState(&mut wi);
            } else {
                WI_initShowNextLoc(&mut wi);
            }
        }
    } else if wi.dm_state & 1 != 0 {
        wi.cnt_pause -= 1;
        if wi.cnt_pause == 0 {
            wi.dm_state += 1;
            wi.cnt_pause = TICRATE;
        }
    }
    drop(wi);
    play_sounds(&sounds);
}

/// Draw the deathmatch stats page: background, level name, frag matrix, and totals column.
///
/// # Safety
///
/// Reads the deathmatch-stats fields plus the `total`/`killers`/`victims`/
/// `p`/`star`/`bstar` patches and `playeringame`. Caller must ensure
/// `WI_loadData` cached the patches and the deathmatch stats phase has
/// been initialised.
unsafe fn WI_drawDeathmatchStats(wi: &IntermissionState) {
    let (total, killers, victims, star, bstar, p, me) = (
        wi.total, wi.killers, wi.victims, wi.star, wi.bstar, wi.p, wi.me,
    );
    WI_slamBackground(wi);
    WI_drawAnimatedBack(wi);
    WI_drawLF(wi);

    V_DrawPatch(
        DM_TOTALSX - (*total).width.get() as i32 / 2,
        DM_MATRIXY - WI_SPACINGY + 10,
        total,
    );
    V_DrawPatch(DM_KILLERSX, DM_KILLERSY, killers);
    V_DrawPatch(DM_VICTIMSX, DM_VICTIMSY, victims);

    let mut x = DM_MATRIXX + DM_SPACINGX;
    let mut y = DM_MATRIXY;

    for i in 0..MAXPLAYERS {
        if playeringame(i) != 0 {
            V_DrawPatch(
                x - (*p[i]).width.get() as i32 / 2,
                DM_MATRIXY - WI_SPACINGY,
                p[i],
            );
            V_DrawPatch(DM_MATRIXX - (*p[i]).width.get() as i32 / 2, y, p[i]);
            if i as i32 == me {
                V_DrawPatch(
                    x - (*p[i]).width.get() as i32 / 2,
                    DM_MATRIXY - WI_SPACINGY,
                    bstar,
                );
                V_DrawPatch(DM_MATRIXX - (*p[i]).width.get() as i32 / 2, y, star);
            }
        }
        x += DM_SPACINGX;
        y += WI_SPACINGY;
    }

    let mut y = DM_MATRIXY + 10;
    let w = (*wi.num[0]).width.get() as i32;

    for i in 0..MAXPLAYERS {
        let mut x = DM_MATRIXX + DM_SPACINGX;
        if playeringame(i) != 0 {
            for j in 0..MAXPLAYERS {
                if playeringame(j) != 0 {
                    WI_drawNum(wi, x + w, y, wi.dm_frags[i][j], 2);
                }
                x += DM_SPACINGX;
            }
            WI_drawNum(wi, DM_TOTALSX + w, y, wi.dm_totals[i], 2);
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
/// Mutates the netgame-stats fields (`state`, `ng_state`, `cnt_pause`,
/// `cnt_kills`, `cnt_items`, `cnt_secret`, `cnt_frags`, `dofrags`) and
/// reads `playeringame`. Caller must ensure `WI_initVariables` has run.
unsafe fn WI_initNetgameStats(wi: &mut IntermissionState) {
    wi.state = stateenum_t::StatCount;
    wi.acceleratestage = 0;
    wi.ng_state = 1;
    wi.cnt_pause = TICRATE;

    for i in 0..MAXPLAYERS {
        if playeringame(i) == 0 {
            continue;
        }
        wi.cnt_kills[i] = 0;
        wi.cnt_items[i] = 0;
        wi.cnt_secret[i] = 0;
        wi.cnt_frags[i] = 0;
        wi.dofrags += WI_fragSum(wi.plrs, i as i32);
    }

    wi.dofrags = if wi.dofrags != 0 { 1 } else { 0 };

    WI_initAnimatedBack(wi);
}

/// Tick the netgame stats phase: sequentially count up kills, items, secrets, and frags.
///
/// States 2/4/6/8 are counting states; odd states are pauses between categories.
///
/// # Safety
///
/// Mutates the netgame-stats fields (`cnt_kills`, `cnt_items`,
/// `cnt_secret`, `cnt_frags`, `ng_state`, `cnt_pause`, `acceleratestage`)
/// and reads `plrs`, `wbs`, `playeringame`. Triggers sound effects via
/// `S_StartSound`. Caller must ensure `WI_initNetgameStats` has run.
unsafe fn WI_updateNetgameStats() {
    let mut sounds = Vec::new();
    let mut wi = INTERMISSION.borrow_mut();
    WI_updateAnimatedBack(&mut wi);
    let (plrs, wbs) = (wi.plrs, wi.wbs);

    if wi.acceleratestage != 0 && wi.ng_state != 10 {
        wi.acceleratestage = 0;
        for i in 0..MAXPLAYERS {
            if playeringame(i) == 0 {
                continue;
            }
            wi.cnt_kills[i] = ((*plrs.offset(i as isize)).skills * 100) / (*wbs).maxkills;
            wi.cnt_items[i] = ((*plrs.offset(i as isize)).sitems * 100) / (*wbs).maxitems;
            wi.cnt_secret[i] = ((*plrs.offset(i as isize)).ssecret * 100) / (*wbs).maxsecret;
            if wi.dofrags != 0 {
                wi.cnt_frags[i] = WI_fragSum(plrs, i as i32);
            }
        }
        sounds.push(Sfx::Barexp);
        wi.ng_state = 10;
    }

    if wi.ng_state == 2 {
        if wi.bcnt & 3 == 0 {
            sounds.push(Sfx::Pistol);
        }
        let mut stillticking = false;
        for i in 0..MAXPLAYERS {
            if playeringame(i) == 0 {
                continue;
            }
            wi.cnt_kills[i] += 2;
            let target = ((*plrs.offset(i as isize)).skills * 100) / (*wbs).maxkills;
            if wi.cnt_kills[i] >= target {
                wi.cnt_kills[i] = target;
            } else {
                stillticking = true;
            }
        }
        if !stillticking {
            sounds.push(Sfx::Barexp);
            wi.ng_state += 1;
        }
    } else if wi.ng_state == 4 {
        if wi.bcnt & 3 == 0 {
            sounds.push(Sfx::Pistol);
        }
        let mut stillticking = false;
        for i in 0..MAXPLAYERS {
            if playeringame(i) == 0 {
                continue;
            }
            wi.cnt_items[i] += 2;
            let target = ((*plrs.offset(i as isize)).sitems * 100) / (*wbs).maxitems;
            if wi.cnt_items[i] >= target {
                wi.cnt_items[i] = target;
            } else {
                stillticking = true;
            }
        }
        if !stillticking {
            sounds.push(Sfx::Barexp);
            wi.ng_state += 1;
        }
    } else if wi.ng_state == 6 {
        if wi.bcnt & 3 == 0 {
            sounds.push(Sfx::Pistol);
        }
        let mut stillticking = false;
        for i in 0..MAXPLAYERS {
            if playeringame(i) == 0 {
                continue;
            }
            wi.cnt_secret[i] += 2;
            let target = ((*plrs.offset(i as isize)).ssecret * 100) / (*wbs).maxsecret;
            if wi.cnt_secret[i] >= target {
                wi.cnt_secret[i] = target;
            } else {
                stillticking = true;
            }
        }
        if !stillticking {
            sounds.push(Sfx::Barexp);
            wi.ng_state += 1 + 2 * if wi.dofrags == 0 { 1 } else { 0 };
        }
    } else if wi.ng_state == 8 {
        if wi.bcnt & 3 == 0 {
            sounds.push(Sfx::Pistol);
        }
        let mut stillticking = false;
        for i in 0..MAXPLAYERS {
            if playeringame(i) == 0 {
                continue;
            }
            wi.cnt_frags[i] += 1;
            let fsum = WI_fragSum(plrs, i as i32);
            if wi.cnt_frags[i] >= fsum {
                wi.cnt_frags[i] = fsum;
            } else {
                stillticking = true;
            }
        }
        if !stillticking {
            sounds.push(Sfx::Pldeth);
            wi.ng_state += 1;
        }
    } else if wi.ng_state == 10 {
        if wi.acceleratestage != 0 {
            sounds.push(Sfx::Sgcock);
            if gamemode() == doom::mode::commercial {
                WI_initNoState(&mut wi);
            } else {
                WI_initShowNextLoc(&mut wi);
            }
        }
    } else if wi.ng_state & 1 != 0 {
        wi.cnt_pause -= 1;
        if wi.cnt_pause == 0 {
            wi.ng_state += 1;
            wi.cnt_pause = TICRATE;
        }
    }
    drop(wi);
    play_sounds(&sounds);
}

/// Draw the cooperative netgame stats page: background, column headers, and per-player rows.
///
/// # Safety
///
/// Reads the netgame-stats fields plus the column-header patches
/// (`kills`, `items`, `secret`, `frags`, `percent`, `p`, `star`) and
/// `playeringame`. Caller must ensure `WI_loadData` cached the patches
/// and the netgame stats phase has been initialised.
unsafe fn WI_drawNetgameStats(wi: &IntermissionState) {
    let (kills, items, secret, frags, star, p, me, dofrags) = (
        wi.kills, wi.items, wi.secret, wi.frags, wi.star, wi.p, wi.me, wi.dofrags,
    );
    let pwidth = (*wi.percent).width.get() as i32;

    WI_slamBackground(wi);
    WI_drawAnimatedBack(wi);
    WI_drawLF(wi);

    let ng_statsx = 32 + (*star).width.get() as i32 / 2 + 32 * if dofrags == 0 { 1 } else { 0 };

    V_DrawPatch(
        ng_statsx + NG_SPACINGX - (*kills).width.get() as i32,
        NG_STATSY,
        kills,
    );
    V_DrawPatch(
        ng_statsx + 2 * NG_SPACINGX - (*items).width.get() as i32,
        NG_STATSY,
        items,
    );
    V_DrawPatch(
        ng_statsx + 3 * NG_SPACINGX - (*secret).width.get() as i32,
        NG_STATSY,
        secret,
    );
    if dofrags != 0 {
        V_DrawPatch(
            ng_statsx + 4 * NG_SPACINGX - (*frags).width.get() as i32,
            NG_STATSY,
            frags,
        );
    }

    let mut y = NG_STATSY + (*kills).height.get() as i32;

    for i in 0..MAXPLAYERS {
        if playeringame(i) == 0 {
            continue;
        }
        let mut x = ng_statsx;
        V_DrawPatch(x - (*p[i]).width.get() as i32, y, p[i]);
        if i as i32 == me {
            V_DrawPatch(x - (*p[i]).width.get() as i32, y, star);
        }
        x += NG_SPACINGX;
        WI_drawPercent(wi, x - pwidth, y + 10, wi.cnt_kills[i]);
        x += NG_SPACINGX;
        WI_drawPercent(wi, x - pwidth, y + 10, wi.cnt_items[i]);
        x += NG_SPACINGX;
        WI_drawPercent(wi, x - pwidth, y + 10, wi.cnt_secret[i]);
        x += NG_SPACINGX;
        if dofrags != 0 {
            WI_drawNum(wi, x, y + 10, wi.cnt_frags[i], -1);
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
/// Mutates the single-player stats fields (`state`, `sp_state`,
/// `cnt_kills[0]`, `cnt_items[0]`, `cnt_secret[0]`, `cnt_time`, `cnt_par`,
/// `cnt_pause`, `acceleratestage`) and resets the animated background.
/// Caller must ensure `WI_initVariables` has run.
unsafe fn WI_initStats(wi: &mut IntermissionState) {
    wi.state = stateenum_t::StatCount;
    wi.acceleratestage = 0;
    wi.sp_state = 1;
    wi.cnt_kills[0] = -1;
    wi.cnt_items[0] = -1;
    wi.cnt_secret[0] = -1;
    wi.cnt_time = -1;
    wi.cnt_par = -1;
    wi.cnt_pause = TICRATE;

    WI_initAnimatedBack(wi);
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
    let mut sounds = Vec::new();
    let mut wi = INTERMISSION.borrow_mut();
    WI_updateAnimatedBack(&mut wi);
    let (wbs, me) = (wi.wbs, wi.me);
    let plr = wi.plrs.offset(me as isize);

    if wi.acceleratestage != 0 && wi.sp_state != 10 {
        wi.acceleratestage = 0;
        wi.cnt_kills[0] = ((*plr).skills * 100) / (*wbs).maxkills;
        wi.cnt_items[0] = ((*plr).sitems * 100) / (*wbs).maxitems;
        wi.cnt_secret[0] = ((*plr).ssecret * 100) / (*wbs).maxsecret;
        wi.cnt_time = (*plr).stime / TICRATE;
        wi.cnt_par = (*wbs).partime / TICRATE;
        sounds.push(Sfx::Barexp);
        wi.sp_state = 10;
    }

    if wi.sp_state == 2 {
        wi.cnt_kills[0] += 2;
        if wi.bcnt & 3 == 0 {
            sounds.push(Sfx::Pistol);
        }
        let target = ((*plr).skills * 100) / (*wbs).maxkills;
        if wi.cnt_kills[0] >= target {
            wi.cnt_kills[0] = target;
            sounds.push(Sfx::Barexp);
            wi.sp_state += 1;
        }
    } else if wi.sp_state == 4 {
        wi.cnt_items[0] += 2;
        if wi.bcnt & 3 == 0 {
            sounds.push(Sfx::Pistol);
        }
        let target = ((*plr).sitems * 100) / (*wbs).maxitems;
        if wi.cnt_items[0] >= target {
            wi.cnt_items[0] = target;
            sounds.push(Sfx::Barexp);
            wi.sp_state += 1;
        }
    } else if wi.sp_state == 6 {
        wi.cnt_secret[0] += 2;
        if wi.bcnt & 3 == 0 {
            sounds.push(Sfx::Pistol);
        }
        let target = ((*plr).ssecret * 100) / (*wbs).maxsecret;
        if wi.cnt_secret[0] >= target {
            wi.cnt_secret[0] = target;
            sounds.push(Sfx::Barexp);
            wi.sp_state += 1;
        }
    } else if wi.sp_state == 8 {
        if wi.bcnt & 3 == 0 {
            sounds.push(Sfx::Pistol);
        }
        wi.cnt_time += 3;
        let target_time = (*plr).stime / TICRATE;
        if wi.cnt_time >= target_time {
            wi.cnt_time = target_time;
        }
        wi.cnt_par += 3;
        let target_par = (*wbs).partime / TICRATE;
        if wi.cnt_par >= target_par {
            wi.cnt_par = target_par;
            if wi.cnt_time >= target_time {
                sounds.push(Sfx::Barexp);
                wi.sp_state += 1;
            }
        }
    } else if wi.sp_state == 10 {
        if wi.acceleratestage != 0 {
            sounds.push(Sfx::Sgcock);
            if gamemode() == doom::mode::commercial {
                WI_initNoState(&mut wi);
            } else {
                WI_initShowNextLoc(&mut wi);
            }
        }
    } else if wi.sp_state & 1 != 0 {
        wi.cnt_pause -= 1;
        if wi.cnt_pause == 0 {
            wi.sp_state += 1;
            wi.cnt_pause = TICRATE;
        }
    }
    drop(wi);
    play_sounds(&sounds);
}

/// Draw the single-player stats page: background, level name, kill/item/secret/time/par values.
///
/// # Safety
///
/// Reads the single-player stats counters plus the patches `num`, `kills`,
/// `items`, `sp_secret`, `timepatch`, `par`, and dereferences `wbs` to
/// decide whether to draw the par-time row. Caller must ensure
/// `WI_loadData` cached the patches and the stats phase has been initialised.
unsafe fn WI_drawStats(wi: &IntermissionState) {
    let lh = (3 * (*wi.num[0]).height.get() as i32) / 2;

    WI_slamBackground(wi);
    WI_drawAnimatedBack(wi);
    WI_drawLF(wi);

    V_DrawPatch(SP_STATSX, SP_STATSY, wi.kills);
    WI_drawPercent(wi, SCREENWIDTH - SP_STATSX, SP_STATSY, wi.cnt_kills[0]);

    V_DrawPatch(SP_STATSX, SP_STATSY + lh, wi.items);
    WI_drawPercent(wi, SCREENWIDTH - SP_STATSX, SP_STATSY + lh, wi.cnt_items[0]);

    V_DrawPatch(SP_STATSX, SP_STATSY + 2 * lh, wi.sp_secret);
    WI_drawPercent(
        wi,
        SCREENWIDTH - SP_STATSX,
        SP_STATSY + 2 * lh,
        wi.cnt_secret[0],
    );

    V_DrawPatch(SP_TIMEX, SP_TIMEY, wi.timepatch);
    WI_drawTime(wi, SCREENWIDTH / 2 - SP_TIMEX, SP_TIMEY, wi.cnt_time);

    if (*wi.wbs).epsd < 3 {
        V_DrawPatch(SCREENWIDTH / 2 + SP_TIMEX, SP_TIMEY, wi.par);
        WI_drawTime(wi, SCREENWIDTH - SP_TIMEX, SP_TIMEY, wi.cnt_par);
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
unsafe fn WI_checkForAccelerate(wi: &mut IntermissionState) {
    for i in 0..MAXPLAYERS {
        if playeringame(i) != 0 {
            let player = game_player(i);
            if (*player).cmd.buttons & 1 != 0 {
                // BT_ATTACK
                if (*player).attackdown == 0 {
                    wi.acceleratestage = 1;
                }
                (*player).attackdown = 1;
            } else {
                (*player).attackdown = 0;
            }
            if (*player).cmd.buttons & 2 != 0 {
                // BT_USE
                if (*player).usedown == 0 {
                    wi.acceleratestage = 1;
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
pub unsafe fn WI_Ticker() {
    let first_tic = {
        let mut wi = INTERMISSION.borrow_mut();
        wi.bcnt += 1;
        wi.bcnt == 1
    };

    if first_tic {
        if gamemode() == doom::mode::commercial {
            S_ChangeMusic(Mus::Dm2int as i32, 1);
        } else {
            S_ChangeMusic(Mus::Inter as i32, 1);
        }
    }

    let state = {
        let mut wi = INTERMISSION.borrow_mut();
        WI_checkForAccelerate(&mut wi);
        wi.state
    };

    match state {
        stateenum_t::StatCount => {
            if deathmatch() != 0 {
                WI_updateDeathmatchStats();
            } else if netgame() != 0 {
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
type LoadCallback = unsafe fn(&[u8], *mut *mut patch_t);

/// Walk every intermission lump name and invoke `callback` for each.
///
/// Shared by `WI_loadData` and `WI_unloadData`. Handles both episode (Doom 1)
/// and commercial (Doom 2) map lists, animation frame patches, and all UI
/// patches (numbers, labels, background).
///
/// # Safety
///
/// `callback` is invoked as an `unsafe` function and is given pointers
/// into the patch fields (`lnames`, `yah`, `splat`, `num`, `percent`,
/// `finished`, `entering`, etc.) and the per-episode animation state.
/// Caller must pass a callback that only reads/writes the single slot
/// supplied and does not touch the intermission state. Dereferences `wbs`
/// and the global `gamemode`; the WAD subsystem must be initialised.
unsafe fn WI_loadUnloadData(callback: LoadCallback) {
    let mut name: [u8; 9] = [0; 9];
    let mut wi = INTERMISSION.borrow_mut();
    let wbs = wi.wbs;
    let lnames = wi.lnames;

    if gamemode() == doom::mode::commercial {
        for i in 0..wi.numcmaps {
            DEH_snprintf!(name, "CWILV{:02}", i);
            callback(&name, lnames.offset(i as isize));
        }
    } else {
        for i in 0..NUMMAPS as i32 {
            c_write!(name, "WILV{}{}", (*wbs).epsd, i);
            callback(&name, lnames.offset(i as isize));
        }

        callback(DEH_String(b"WIURH0"), &mut wi.yah[0]);
        callback(DEH_String(b"WIURH1"), &mut wi.yah[1]);
        callback(DEH_String(b"WISPLAT"), &mut wi.splat[0]);

        if (*wbs).epsd < 3 {
            let epsd = (*wbs).epsd as usize;
            for j in 0..NUMANIMS[epsd] as usize {
                let cfg = anim_config(epsd, j);
                for i in 0..cfg.nanims as usize {
                    if (*wbs).epsd != 1 || j != 8 {
                        c_write!(name, "WIA{}{:02}{:02}", (*wbs).epsd, j, i);
                        callback(&name, &mut wi.anim_state_mut(epsd, j).p[i]);
                    } else {
                        let frame = wi.anim_state(1, 4).p[i];
                        wi.anim_state_mut(epsd, j).p[i] = frame;
                    }
                }
            }
        }
    }

    callback(DEH_String(b"WIMINUS"), &mut wi.wiminus);

    for i in 0..10i32 {
        DEH_snprintf!(name, "WINUM{}", i);
        callback(&name, &mut wi.num[i as usize]);
    }

    callback(DEH_String(b"WIPCNT"), &mut wi.percent);
    callback(DEH_String(b"WIF"), &mut wi.finished);
    callback(DEH_String(b"WIENTER"), &mut wi.entering);
    callback(DEH_String(b"WIOSTK"), &mut wi.kills);
    callback(DEH_String(b"WIOSTS"), &mut wi.secret);
    callback(DEH_String(b"WISCRT2"), &mut wi.sp_secret);

    if W_CheckNumForName(DEH_String(b"WIOBJ")) >= 0 {
        if netgame() != 0 && deathmatch() == 0 {
            callback(DEH_String(b"WIOBJ"), &mut wi.items);
        } else {
            callback(DEH_String(b"WIOSTI"), &mut wi.items);
        }
    } else {
        callback(DEH_String(b"WIOSTI"), &mut wi.items);
    }

    callback(DEH_String(b"WIFRGS"), &mut wi.frags);
    callback(DEH_String(b"WICOLON"), &mut wi.colon);
    callback(DEH_String(b"WITIME"), &mut wi.timepatch);
    callback(DEH_String(b"WISUCKS"), &mut wi.sucks);
    callback(DEH_String(b"WIPAR"), &mut wi.par);
    callback(DEH_String(b"WIKILRS"), &mut wi.killers);
    callback(DEH_String(b"WIVCTMS"), &mut wi.victims);
    callback(DEH_String(b"WIMSTT"), &mut wi.total);

    for i in 0..MAXPLAYERS {
        DEH_snprintf!(name, "STPB{}", i);
        callback(&name, &mut wi.p[i]);
        DEH_snprintf!(name, "WIBP{}", i + 1);
        callback(&name, &mut wi.bp[i]);
    }

    if gamemode() == doom::mode::commercial
        || (gamemode() == doom::mode::retail && (*wbs).epsd == 3)
    {
        name = *DEH_String(b"INTERPIC\0");
    } else {
        DEH_snprintf!(name, "WIMAP{}", (*wbs).epsd);
    }

    callback(&name, &mut wi.background);
}

/// Load callback: cache the named lump at `PU_STATIC` priority and store the pointer.
///
/// # Safety
///
/// Caller must ensure `name` names a lump present in the WAD, and
/// `variable` is a valid, non-null, properly aligned pointer to a
/// `*mut patch_t` slot the function may overwrite.
unsafe fn WI_loadCallback(name: &[u8], variable: *mut *mut patch_t) {
    *variable = W_CacheLumpName(name, PU_STATIC) as *mut patch_t;
}

/// Allocate the `lnames` pointer array and cache all intermission WAD patches.
///
/// Must be called before the first `WI_Drawer` call. `lnames` is allocated from
/// the zone heap at `PU_STATIC` with a size matching either `numcmaps` (commercial)
/// or `NUMMAPS` (episode).  Also loads the `star`/`bstar` patches directly.
///
/// # Safety
///
/// Allocates `lnames` via `Z_Malloc` and mutates every cached patch field
/// (`star`, `bstar`, and everything touched by `WI_loadUnloadData`). Caller
/// must ensure the WAD and zone-memory subsystems are initialised and that
/// `wbs` is valid (via `WI_initVariables`).
pub unsafe fn WI_loadData() {
    {
        let mut wi = INTERMISSION.borrow_mut();
        if gamemode() == doom::mode::commercial {
            wi.numcmaps = 32;
            wi.lnames = Z_Malloc(
                (std::mem::size_of::<*mut patch_t>() * wi.numcmaps as usize) as i32,
                PU_STATIC,
                ptr::null_mut(),
            ) as *mut *mut patch_t;
        } else {
            wi.lnames = Z_Malloc(
                (std::mem::size_of::<*mut patch_t>() * NUMMAPS) as i32,
                PU_STATIC,
                ptr::null_mut(),
            ) as *mut *mut patch_t;
        }
    }

    WI_loadUnloadData(WI_loadCallback);

    let star = W_CacheLumpName(DEH_String(b"STFST01"), PU_STATIC) as *mut patch_t;
    let bstar = W_CacheLumpName(DEH_String(b"STFDEAD0"), PU_STATIC) as *mut patch_t;
    let mut wi = INTERMISSION.borrow_mut();
    wi.star = star;
    wi.bstar = bstar;
}

/// Unload callback: release the named lump from the WAD cache and null the pointer.
///
/// # Safety
///
/// Caller must ensure `name` names a lump previously cached via
/// `W_CacheLumpName`, and `variable` is a valid, non-null, properly
/// aligned pointer to a `*mut patch_t` slot that this function may
/// overwrite with null.
unsafe fn WI_unloadCallback(name: &[u8], variable: *mut *mut patch_t) {
    W_ReleaseLumpName(name);
    *variable = ptr::null_mut();
}

/// Release all intermission WAD patches loaded by `WI_loadData`.
///
/// Called indirectly by `WI_End` at the close of the intermission screen.
///
/// # Safety
///
/// Nulls every cached patch pointer in the intermission state and
/// releases their WAD lumps. Caller must ensure no draw or update
/// functions are still executing.
pub unsafe fn WI_unloadData() {
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
/// Reads the intermission `state` and the global `deathmatch` and `netgame` flags and
/// dispatches to one of the per-phase draw functions, each of which
/// dereferences the intermission state (`wbs`, patch pointers, etc.).
/// Caller must ensure `WI_Start` has been invoked.
pub unsafe fn WI_Drawer() {
    let state = INTERMISSION.borrow().state;
    match state {
        stateenum_t::StatCount => {
            let wi = INTERMISSION.borrow();
            if deathmatch() != 0 {
                WI_drawDeathmatchStats(&wi);
            } else if netgame() != 0 {
                WI_drawNetgameStats(&wi);
            } else {
                WI_drawStats(&wi);
            }
        }
        stateenum_t::ShowNextLoc => {
            WI_drawShowNextLoc(&INTERMISSION.borrow());
        }
        stateenum_t::NoState => {
            WI_drawNoState();
        }
    }
}

// ---------------------------------------------------------------------------
// WI_initVariables / WI_Start
// ---------------------------------------------------------------------------

/// Initialise the intermission state from the level-start record `wbstartstruct`.
///
/// Clamps all max-count fields to a minimum of 1 to avoid division-by-zero in
/// percentage calculations. Adjusts `wbs.epsd` downward by 3 for non-retail
/// builds that were given an out-of-range episode number.
///
/// # Safety
///
/// Caller must ensure `wbstartstruct` is a valid, non-null, properly aligned
/// pointer to an initialised `wbstartstruct_t` that remains live for the
/// duration of the intermission. Mutates the state pointers `wbs`, `plrs`
/// and the counters (`acceleratestage`, `cnt`, `bcnt`, `firstrefresh`,
/// `me`).
unsafe fn WI_initVariables(wi: &mut IntermissionState, wbstartstruct: *mut wbstartstruct_t) {
    let wbs = wbstartstruct;
    wi.wbs = wbs;
    wi.plrs = (*wbs).plyr.as_mut_ptr();

    wi.acceleratestage = 0;
    wi.cnt = 0;
    wi.bcnt = 0;
    wi.firstrefresh = 1;
    wi.me = (*wbs).pnum;

    if (*wbs).maxkills == 0 {
        (*wbs).maxkills = 1;
    }
    if (*wbs).maxitems == 0 {
        (*wbs).maxitems = 1;
    }
    if (*wbs).maxsecret == 0 {
        (*wbs).maxsecret = 1;
    }

    if gamemode() != doom::mode::retail && (*wbs).epsd > 2 {
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
/// intermission. Mutates every intermission field via `WI_initVariables`,
/// `WI_loadData`, and the per-mode `WI_init*Stats` helpers; the WAD, zone,
/// and sound subsystems must be initialised.
pub unsafe fn WI_Start(wbstartstruct: *mut wbstartstruct_t) {
    WI_initVariables(&mut INTERMISSION.borrow_mut(), wbstartstruct);
    WI_loadData();

    let mut wi = INTERMISSION.borrow_mut();
    if deathmatch() != 0 {
        WI_initDeathmatchStats(&mut wi);
    } else if netgame() != 0 {
        WI_initNetgameStats(&mut wi);
    } else {
        WI_initStats(&mut wi);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epsd0_config_entry0_fields() {
        let c = &EPSD0_CONFIG[0];
        assert_eq!(c.kind, animenum_t::ANIM_ALWAYS);
        assert_eq!(c.nanims, 3);
        assert_eq!(c.loc.x, 224);
        assert_eq!(c.loc.y, 104);
    }

    #[test]
    fn epsd1_config_entry0_is_level_anim() {
        let c = &EPSD1_CONFIG[0];
        assert_eq!(c.kind, animenum_t::ANIM_LEVEL);
        assert_eq!(c.data1, 1);
    }

    #[test]
    fn epsd2_last_entry_has_quarter_ticrate_period() {
        let last = &EPSD2_CONFIG[EPSD2_NANIM - 1];
        assert_eq!(last.period, TICRATE / 4);
    }

    #[test]
    fn anim_state_initializes_zeroed() {
        let intermission = IntermissionState::new();
        let st = intermission.anim_state(0, 0);
        assert_eq!(st.ctr, 0);
        assert_eq!(st.nexttic, 0);
        assert_eq!(st.lastdrawn, 0);
        assert_eq!(st.state, 0);
    }
}
