//! Rust port of `vendor/doomgeneric/g_game.c`.
//!
//! This module is the gameplay controller - the central dispatcher between
//! input, demo recording/playback, map loading, player command building, and
//! game-tick advancement. It owns the deferred-action state machine
//! ([`gameaction`]) that `G_Ticker` drains every tic, the [`gamestate`]
//! enumeration (`GS_LEVEL`, `GS_INTERMISSION`, `GS_FINALE`, `GS_DEMOSCREEN`),
//! and the per-player slot tables consumed throughout the engine.
//!
//! Key responsibilities exposed by this module:
//!
//! * `G_BuildTiccmd` - converts raw input events into a [`TiccmdT`] for one
//!   tic (1/35 second). Performs mouse/joystick scaling, two-stage
//!   accelerative turning, weapon cycling and low-resolution turn rounding.
//! * `G_Ticker` - the per-tic dispatcher. Drains pending `gameaction` work
//!   first, then routes per-player ticcmds, applies special buttons
//!   (pause/save), and advances the active sub-state via `P_Ticker`,
//!   `WI_Ticker`, `F_Ticker` or `D_PageTicker`.
//! * `G_Responder` - handles `event_t` input (key/mouse/joystick) during
//!   gameplay and demo screens. Owns the spy-mode toggle, weapon cycling
//!   hotkeys, pause request and menu pop-up logic.
//! * Demo I/O - `G_RecordDemo`, `G_BeginRecording`, `G_WriteDemoTiccmd`,
//!   `G_ReadDemoTiccmd`, `G_PlayDemo`, `G_DeferedPlayDemo`, `G_DoPlayDemo`,
//!   `G_TimeDemo` and `G_CheckDemoStatus` - record and play back `.lmp` demo
//!   files. The byte layout matches vanilla Doom exactly (4 bytes per tic
//!   non-longtics, 5 bytes per tic longtics) so demos remain bit-compatible
//!   with the original DOS engine.
//! * Save/Load - `G_SaveGame`, `G_LoadGame`, `G_DoSaveGame`, `G_DoLoadGame`
//!   serialise the entire game state through `p_saveg`.
//! * Game initialisation - `G_DeferedInitNew`, `G_DoNewGame`, `G_InitNew` and
//!   `G_DoLoadLevel` configure skill, episode, map and sky texture, then
//!   trigger map setup.
//!
//! Net/demo consistency is maintained via the `consistancy[]` table and the
//! `rndindex` cookie tracked by `m_random`; a mismatch raises `I_Error`.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use {
    crate::engine::sound::info::Sfx,
    std::{
        borrow::Cow,
        ffi::c_void,
        io::{Cursor, Write},
    },
};

use std::ptr;

use crate::engine::{
    doom::{
        mode::{
            commercial, doom, exe_chex, exe_doom_1_2, exe_doom_1_666, exe_doom_1_7, exe_doom_1_8,
            exe_final2, exe_ultimate, shareware,
        },
        player::{PlayerMessage, PlayerT, TiccmdT, MAXPLAYERS},
        stat::{gamemission, gamemode, gameversion},
    },
    intermission::{wbplayerstruct_t, wbstartstruct_t},
    math::tables::{finecosine, finesine, finetangent},
    misc::random::P_Random,
    play::{
        inter::maxammo,
        mobj::mobj_t,
        setup::{deathmatch_p, deathmatchstart, playerstart},
    },
    wad::format::mapthing_t,
};

use crate::engine::global::Global;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Vanilla Doom `boolean`, matching the C `int` truthy/falsy convention.
type boolean = i32;
/// Vanilla Doom `byte` (unsigned 8-bit).
type byte = u8;
/// Vanilla Doom `skill_t` enum stored as `int` (sk_baby=0 .. sk_nightmare=4).
type skill_t = i32;
/// Vanilla Doom 16.16 fixed-point type (`int`, with 16 fractional bits).
type fixed_t = i32;

// ---------------------------------------------------------------------------
// Game-state constants (from doomstat.h)
// ---------------------------------------------------------------------------

/// `GS_LEVEL` - actively playing a map; `P_Ticker` advances world simulation.
const GS_LEVEL: i32 = 0;
/// `GS_INTERMISSION` - between-level statistics screen driven by `WI_Ticker`.
const GS_INTERMISSION: i32 = 1;
/// `GS_FINALE` - end-of-episode text crawl / cast call, driven by `F_Ticker`.
const GS_FINALE: i32 = 2;
/// `GS_DEMOSCREEN` - title/credits/demo cycle, driven by `D_PageTicker`.
const GS_DEMOSCREEN: i32 = 3;

// ---------------------------------------------------------------------------
// Game-action constants (from doomstat.h)
// ---------------------------------------------------------------------------

/// No deferred action pending; `G_Ticker` falls through to normal state.
const ga_nothing: i32 = 0;
/// Defer a level (re)load via `G_DoLoadLevel`.
const ga_loadlevel: i32 = 1;
/// Defer a new game start via `G_DoNewGame`.
const ga_newgame: i32 = 2;
/// Defer a savegame load via `G_DoLoadGame`.
const ga_loadgame: i32 = 3;
/// Defer a savegame write via `G_DoSaveGame`.
const ga_savegame: i32 = 4;
/// Defer demo playback via `G_DoPlayDemo`.
const ga_playdemo: i32 = 5;
/// Defer the end-of-level transition via `G_DoCompleted`.
const ga_completed: i32 = 6;
/// Defer the end-of-game finale via `F_StartFinale`.
const ga_victory: i32 = 7;
/// Defer the intermission-to-next-level transition via `G_DoWorldDone`.
const ga_worlddone: i32 = 8;
/// Defer a screenshot grab via `V_ScreenShot`.
const ga_screenshot: i32 = 9;

// ---------------------------------------------------------------------------
// Player state constants (from d_player.h)
// ---------------------------------------------------------------------------

/// `PST_LIVE` - player is alive and active.
const PST_LIVE: i32 = 0;
/// `PST_DEAD` - player is dead, awaiting respawn input.
const PST_DEAD: i32 = 1;
/// `PST_REBORN` - player should be respawned on the next tick.
const PST_REBORN: i32 = 2;

// ---------------------------------------------------------------------------
// Weapon constants (from doomdef.h)
// ---------------------------------------------------------------------------

/// `wp_fist` weapon index (slot 1).
const wp_fist: i32 = 0;
/// `wp_pistol` weapon index (slot 2).
const wp_pistol: i32 = 1;
/// `wp_chainsaw` weapon index (also slot 1).
const wp_chainsaw: i32 = 7;
/// `wp_supershotgun` weapon index (Doom II only, also slot 3).
const wp_supershotgun: i32 = 8;
/// `wp_plasma` weapon index (slot 6).
const wp_plasma: i32 = 5;
/// `wp_bfg` weapon index (slot 7).
const wp_bfg: i32 = 6;
/// `wp_nochange` sentinel - pending weapon means "keep current".
const wp_nochange: i32 = 9;

// ---------------------------------------------------------------------------
// Power types (from doomdef.h)
// ---------------------------------------------------------------------------

/// `pw_strength` index into `players[].powers` - berserk pack timer.
const pw_strength: usize = 1;

// ---------------------------------------------------------------------------
// Button constants (from d_event.h)
// ---------------------------------------------------------------------------

/// `BT_ATTACK` ticcmd button bit - fire weapon.
const BT_ATTACK: u8 = 1;
/// `BT_USE` ticcmd button bit - activate door / switch.
const BT_USE: u8 = 2;
/// `BT_CHANGE` ticcmd button bit - request weapon change.
const BT_CHANGE: u8 = 4;
/// Mask used to extract the weapon-change index encoded in `buttons`.
const BT_WEAPONMASK: u8 = 8 + 16 + 32;
/// Left shift for encoding the weapon-change index into `buttons`.
const BT_WEAPONSHIFT: u8 = 3;
/// `BT_SPECIAL` flag - the ticcmd carries a pause/save/load request.
const BT_SPECIAL: u8 = 128;
/// Mask used to decode the special-button kind after `BT_SPECIAL`.
const BT_SPECIALMASK: u8 = 3;
/// Special-button value for "pause toggle".
const BTS_PAUSE: u8 = 1;
/// Special-button value for "savegame".
const BTS_SAVEGAME: u8 = 2;
/// Mask for the savegame-slot field carried by a `BTS_SAVEGAME` request.
const BTS_SAVEMASK: u8 = 4 + 8 + 16;
/// Left shift for the savegame-slot field carried by `BTS_SAVEGAME`.
const BTS_SAVESHIFT: u8 = 2;

// ---------------------------------------------------------------------------
// Miscellaneous constants
// ---------------------------------------------------------------------------

/// Size of the `gamekeydown` keystate array - maximum supported key codes.
const NUMKEYS: usize = 256;
/// Maximum number of mouse buttons recognised by the engine.
const MAX_MOUSE_BUTTONS: usize = 8;
/// Maximum number of joystick buttons recognised by the engine.
const MAX_JOY_BUTTONS: usize = 20;
/// Vanilla Doom savegame size cap (bytes); enforced when `vanilla_savegame_limit` is set.
const SAVEGAMESIZE: i64 = 0x2c000;
/// End-of-stream sentinel byte written at the tail of every demo `.lmp`.
const DEMOMARKER: byte = 0x80;
/// Version code for cph's longtics hack ("v1.91") from `doomdef.h`.
pub const DOOM_191_VERSION: i32 = 111;
/// Size of the body-object circular queue (BODYQUESIZE in g_game.c).
pub const BODYQUESIZE: usize = 32;
/// Number of tics during which slow-turn speed is used before switching to
/// normal turn speed (SLOWTURNTICS in g_game.c).
pub const SLOWTURNTICS: i32 = 6;
/// Speed threshold above which the turbo-cheat detector fires
/// (TURBOTHRESHOLD in g_game.c = 0x32 = 50).
pub const TURBOTHRESHOLD: i32 = 0x32;
/// Ammo-type index for the clip (bullets) ammo class.
const am_clip: usize = 0;
/// `MT_TFOG` mobj type index - teleport fog spawned at player respawn spots.
const MT_TFOG: i32 = 28;

/// Default starting health (100); patched by Dehacked in vanilla.
const DEH_INITIAL_HEALTH: i32 = 100;
/// Default starting bullet count (50); patched by Dehacked in vanilla.
const DEH_INITIAL_BULLETS: i32 = 50;

/// Re-export of [`event_t`] from `d_event.rs` for input handling.
use crate::engine::doom::event::event_t;

// ---------------------------------------------------------------------------
// Weapon ordering table (for prev/next weapon cycling)
// ---------------------------------------------------------------------------

/// One entry of the prev/next weapon cycling table.
///
/// `weapon` is the concrete weapon checked for availability; `weapon_num` is
/// the slot index ultimately emitted in the ticcmd (e.g. both fist and
/// chainsaw cycle to slot 1, both shotgun and supershotgun to slot 3).
struct WeaponOrder {
    /// Concrete weapon identifier (`wp_*`) used for selectability tests.
    weapon: i32,
    /// Slot number (1-8) encoded into the `BT_CHANGE` ticcmd field.
    weapon_num: i32,
}

/// Cyclic ordering of weapons used by next/previous-weapon hotkeys.
///
/// Mirrors `weapon_order_table[]` in `g_game.c`. The order determines the
/// scan direction: indices 0..=8 are walked clockwise (forward direction)
/// or counter-clockwise (back), skipping unavailable weapons.
static WEAPON_ORDER_TABLE: [WeaponOrder; 9] = [
    WeaponOrder {
        weapon: wp_fist,
        weapon_num: wp_fist,
    },
    WeaponOrder {
        weapon: wp_chainsaw,
        weapon_num: wp_fist,
    },
    WeaponOrder {
        weapon: wp_pistol,
        weapon_num: wp_pistol,
    },
    WeaponOrder {
        weapon: 3,
        /* shotgun */ weapon_num: 3,
    },
    WeaponOrder {
        weapon: 8,
        /* supershotgun */ weapon_num: 3,
    },
    WeaponOrder {
        weapon: 4,
        /* chaingun */ weapon_num: 4,
    },
    WeaponOrder {
        weapon: wp_plasma - 1,
        /* missile */ weapon_num: wp_plasma - 1,
    },
    WeaponOrder {
        weapon: wp_plasma,
        weapon_num: wp_plasma,
    },
    WeaponOrder {
        weapon: wp_bfg,
        weapon_num: wp_bfg,
    },
];

// ---------------------------------------------------------------------------
// Game state, read by the other engine modules.
// ---------------------------------------------------------------------------

/// The game controller's state (`g_game.c` globals other than the player
/// slots and the ticcmd input latches).
///
/// Other modules read the common fields through the accessor functions
/// below (`gamestate()`, `consoleplayer()`, `playeringame(i)`, ...) and write
/// them through short `GAME.borrow_mut()` borrows.
pub struct GameState {
    /// Value of `gamestate` from the previous `G_Ticker` invocation, used to
    /// detect transitions (e.g. dismissing the intermission screen).
    oldgamestate: i32,
    /// Deferred game action queue (`ga_*`); drained at the top of `G_Ticker`.
    pub gameaction: i32,
    /// Active gameplay state machine slot (`GS_LEVEL`/`GS_INTERMISSION`/...).
    pub gamestate: i32,
    /// Currently selected skill level (`sk_baby` .. `sk_nightmare`).
    pub gameskill: i32,
    /// Non-zero when monsters respawn (nightmare skill or `-respawn` parm).
    pub respawnmonsters: boolean,
    /// Currently loaded episode number (1-based; 1-3 for Doom, up to 4 with Ultimate).
    pub gameepisode: i32,
    /// Currently loaded map number (1-based; 1-9 in Doom, 1-32 in Doom II).
    pub gamemap: i32,
    /// If non-zero, exit the level after this number of minutes.
    pub timelimit: i32,
    /// Non-zero while gameplay is paused (sound is paused, ticker suspended).
    pub paused: boolean,
    /// Non-zero while a user-controlled game is in progress (vs demo / title).
    pub usergame: boolean,
    /// Set by `-timedemo`; on demo end prints fps stats via `I_Error`.
    timingdemo: boolean,
    /// Set by `-nodraw`; disables rendering for benchmarking purposes.
    pub nodrawers: boolean,
    /// `I_GetTime()` value captured when a timed demo started.
    starttime: i32,
    /// Non-zero while the 3D view is being rendered (false during intermission).
    pub viewactive: boolean,
    /// Deathmatch mode (`0`=co-op, `1`=DM, `2`=DM2 / altdeath).
    pub deathmatch: i32,
    /// Non-zero in a networked game (changes consistency-check behaviour).
    pub netgame: boolean,
    /// Per-slot presence flag for the four player slots (`MAXPLAYERS = 4`).
    pub playeringame: [boolean; MAXPLAYERS],
    /// Per-player "turbo" detection latch consumed by `G_Ticker`.
    turbodetected: [boolean; MAXPLAYERS],
    /// Index of the local player receiving input events.
    pub consoleplayer: i32,
    /// Index of the player whose first-person view is being drawn (spy mode).
    pub displayplayer: i32,
    /// `gametic` value captured when the current level was loaded.
    pub levelstarttic: i32,
    /// Sum of monster kills across all players for the current level.
    pub totalkills: i32,
    /// Sum of item pickups across all players for the current level.
    pub totalitems: i32,
    /// Sum of secret-sector finds across all players for the current level.
    pub totalsecret: i32,
    /// File name of the demo currently being recorded.
    demoname: String,
    /// Non-zero while a `.lmp` demo is being written from input.
    pub demorecording: boolean,
    /// cph's Doom 1.91 longtics hack - encodes angleturn in 2 bytes per tic
    /// instead of 1, enabling smooth high-res turning in demos.
    pub longtics: boolean,
    /// Round per-tic angleturn to the nearest 256 BAM when recording vanilla
    /// (non-longtics) demos so the 1-byte demo angleturn replays accurately.
    pub lowres_turn: boolean,
    /// Non-zero while a `.lmp` demo is being played back.
    pub demoplayback: boolean,
    /// Non-zero when the active demo was recorded in a networked session.
    netdemo: boolean,
    /// Base pointer to the current demo I/O buffer (Z_Malloc'd).
    demobuffer: *mut byte,
    /// Read/write cursor within `demobuffer`.
    demo_p: *mut byte,
    /// One-past-the-end pointer for `demobuffer`.
    demoend: *mut byte,
    /// Non-zero when launched with `-playdemo`; quits after the demo ends.
    pub singledemo: boolean,
    /// Non-zero (default) to precache all level graphics during map load.
    pub precache: boolean,
    /// Non-zero while invoked from the setup utility's "test controls" mode.
    pub testcontrols: boolean,
    /// Low-pass-filtered mouse speed displayed by the test-controls thermometer.
    pub testcontrols_mousespeed: i32,
    /// Parameters for the world-map / intermission screen, populated by
    /// `G_DoCompleted` before transitioning to `GS_INTERMISSION`. The
    /// intermission keeps a pointer to it, taken from [`GAME`]'s stable
    /// address.
    pub wminfo: wbstartstruct_t,
    /// Net consistency check ring: `consistancy[player][gametic/ticdup % BACKUPTICS]`.
    /// A mismatch on a remote command triggers `I_Error("consistency failure ...")`.
    consistancy: [[byte; 128]; MAXPLAYERS],
    /// Index into `bodyque` for the next corpse to add (modulo [`BODYQUESIZE`]).
    pub bodyqueslot: i32,
    /// Circular queue of [`BODYQUESIZE`] recent player corpses; the oldest is
    /// removed when the queue wraps (see `G_CheckSpot`).
    bodyque: [*mut mobj_t; BODYQUESIZE],
    /// Non-zero enforces the vanilla `SAVEGAMESIZE` cap (`I_Error` on overrun).
    /// The config table binds it by its stable address.
    pub vanilla_savegame_limit: i32,
    /// Non-zero enforces the vanilla demo buffer cap; zero auto-grows it.
    /// The config table binds it by its stable address.
    pub vanilla_demo_limit: i32,
    /// Forward-movement speed table: `[slow=0x19, fast=0x32]` (`fixed_t` per tic).
    /// Indexed by the `speed` flag computed in `G_BuildTiccmd`; `-turbo`
    /// scales it.
    pub forwardmove: [fixed_t; 2],
    /// Lateral strafe speed table: `[slow=0x18, fast=0x28]` (`fixed_t` per tic).
    pub sidemove: [fixed_t; 2],
    /// Turn-speed table: `[normal=640, fast=1280, slow=320]` (BAM units per tic).
    /// The "slow" entry is selected for the first [`SLOWTURNTICS`] (6) of held input.
    pub angleturn: [fixed_t; 3],
    /// Non-zero when the next level transition should route through the secret
    /// exit (set by `G_SecretExitLevel`).
    secretexit: boolean,
    /// Lump name of the demo deferred for playback (set by `G_DeferedPlayDemo`).
    defdemoname: String,
    /// File name of the savegame currently being loaded.
    savename: String,
    /// Deferred `G_DeferedInitNew` parameter: skill level.
    d_skill: skill_t,
    /// Deferred `G_DeferedInitNew` parameter: episode.
    d_episode: i32,
    /// Deferred `G_DeferedInitNew` parameter: map number.
    d_map: i32,
}

impl GameState {
    /// The state at program start: title/demo screen, no game, no demo.
    pub const fn new() -> Self {
        Self {
            oldgamestate: GS_DEMOSCREEN,
            gameaction: ga_nothing,
            gamestate: GS_DEMOSCREEN,
            gameskill: 0,
            respawnmonsters: 0,
            gameepisode: 0,
            gamemap: 0,
            timelimit: 0,
            paused: 0,
            usergame: 0,
            timingdemo: 0,
            nodrawers: 0,
            starttime: 0,
            viewactive: 0,
            deathmatch: 0,
            netgame: 0,
            playeringame: [0; MAXPLAYERS],
            turbodetected: [0; MAXPLAYERS],
            consoleplayer: 0,
            displayplayer: 0,
            levelstarttic: 0,
            totalkills: 0,
            totalitems: 0,
            totalsecret: 0,
            demoname: String::new(),
            demorecording: 0,
            longtics: 0,
            lowres_turn: 0,
            demoplayback: 0,
            netdemo: 0,
            demobuffer: ptr::null_mut(),
            demo_p: ptr::null_mut(),
            demoend: ptr::null_mut(),
            singledemo: 0,
            precache: 1, // true by default
            testcontrols: 0,
            testcontrols_mousespeed: 0,
            wminfo: wbstartstruct_t {
                epsd: 0,
                didsecret: 0,
                last: 0,
                next: 0,
                maxkills: 0,
                maxitems: 0,
                maxsecret: 0,
                maxfrags: 0,
                partime: 0,
                pnum: 0,
                plyr: [wbplayerstruct_t {
                    in_: 0,
                    skills: 0,
                    sitems: 0,
                    ssecret: 0,
                    stime: 0,
                    frags: [0; 4],
                    score: 0,
                }; MAXPLAYERS],
            },
            consistancy: [[0; 128]; MAXPLAYERS],
            bodyqueslot: 0,
            bodyque: [ptr::null_mut(); BODYQUESIZE],
            vanilla_savegame_limit: 1,
            vanilla_demo_limit: 1,
            forwardmove: [0x19, 0x32],
            sidemove: [0x18, 0x28],
            angleturn: [640, 1280, 320],
            secretexit: 0,
            defdemoname: String::new(),
            savename: String::new(),
            d_skill: 0,
            d_episode: 0,
            d_map: 0,
        }
    }
}

impl Default for GameState {
    fn default() -> Self {
        Self::new()
    }
}

/// The game controller's state.
pub static GAME: Global<GameState> = Global::new(GameState::new());

/// The gameplay state machine slot (C `gamestate`).
pub fn gamestate() -> i32 {
    GAME.borrow().gamestate
}

/// The pending deferred action (C `gameaction`).
pub fn gameaction() -> i32 {
    GAME.borrow().gameaction
}

/// The current skill level (C `gameskill`).
pub fn gameskill() -> i32 {
    GAME.borrow().gameskill
}

/// The current episode (C `gameepisode`).
pub fn gameepisode() -> i32 {
    GAME.borrow().gameepisode
}

/// The current map (C `gamemap`).
pub fn gamemap() -> i32 {
    GAME.borrow().gamemap
}

/// The deathmatch mode (C `deathmatch`).
pub fn deathmatch() -> i32 {
    GAME.borrow().deathmatch
}

/// Whether this is a network game (C `netgame`).
pub fn netgame() -> boolean {
    GAME.borrow().netgame
}

/// Whether player slot `player` is in the game (C `playeringame[player]`).
pub fn playeringame(player: usize) -> boolean {
    GAME.borrow().playeringame[player]
}

/// The local player's index (C `consoleplayer`).
pub fn consoleplayer() -> i32 {
    GAME.borrow().consoleplayer
}

/// The index of the player whose view is drawn (C `displayplayer`).
pub fn displayplayer() -> i32 {
    GAME.borrow().displayplayer
}

/// Whether the game is paused (C `paused`).
pub fn paused() -> boolean {
    GAME.borrow().paused
}

/// Whether a demo is playing back (C `demoplayback`).
pub fn demoplayback() -> boolean {
    GAME.borrow().demoplayback
}

/// Whether monsters respawn (C `respawnmonsters`).
pub fn respawnmonsters() -> boolean {
    GAME.borrow().respawnmonsters
}

/// Stable pointer to the intermission parameters (C `&wminfo`), which the
/// intermission keeps while it runs.
pub fn wminfo_ptr() -> *mut wbstartstruct_t {
    // SAFETY: field projection on the stable state pointer, no reference made.
    unsafe { &raw mut (*GAME.as_ptr()).wminfo }
}

/// The player slots (C `players[MAXPLAYERS]`).
///
/// Map objects (`mobj_t.player`), the status bar, the HUD and the automap
/// keep `*mut PlayerT` pointers into these slots while play code mutates
/// them through those pointers, so the slots are only reached through
/// [`player`]'s stable [`Global::as_ptr`] address.
pub static PLAYERS: Global<[PlayerT; MAXPLAYERS]> =
    // SAFETY: an all-zero `PlayerT` is a valid empty player slot.
    Global::new(unsafe { std::mem::zeroed() });

/// Stable pointer to player slot `index` (C `&players[index]`).
pub fn player(index: usize) -> *mut PlayerT {
    debug_assert!(index < MAXPLAYERS);
    // SAFETY: in-bounds projection on the stable state pointer.
    unsafe { PLAYERS.as_ptr().cast::<PlayerT>().add(index) }
}

/// Stable pointer to the console player's slot (C `&players[consoleplayer]`).
pub fn console_player() -> *mut PlayerT {
    player(consoleplayer() as usize)
}

/// Doom episode par times (episodes 1-3, maps 1-9), in seconds.
/// Index `[0]` and `[*][0]` are dummies to keep the table 1-based.
pub static pars: [[i32; 10]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 30, 75, 120, 90, 165, 180, 180, 30, 165],
    [0, 90, 90, 90, 120, 90, 360, 240, 30, 170],
    [0, 90, 45, 90, 150, 90, 90, 165, 30, 135],
];

/// Doom II par times (maps 1-32), in seconds. Index `[0]` is map 1.
pub static cpars: [i32; 32] = [
    30, 90, 120, 120, 90, 150, 120, 120, 270, 90, 210, 150, 150, 150, 210, 150, 420, 150, 210, 150,
    240, 150, 180, 150, 150, 300, 330, 420, 300, 180, 120, 30,
];

// ---------------------------------------------------------------------------
// Module-private input state
// ---------------------------------------------------------------------------

/// Input latches feeding `G_BuildTiccmd`, plus the pending save request
/// (module-private `g_game.c` statics).
struct GameInputState {
    /// Per-key "is held down" flags, indexed by Doom key code.
    gamekeydown: [boolean; NUMKEYS],
    /// Tics that turn input has been held; drives two-stage accelerative turning.
    turnheld: i32,
    /// Mouse button held-down flags (slot 0 unused to allow `[-1]` indexing).
    // mousebuttons is &mousearray[1] in C — negative indexing; access via mousearray[1+n]
    mousearray: [boolean; MAX_MOUSE_BUTTONS + 1],
    /// Most recent raw mouse delta on the X axis (cleared each tic by `G_BuildTiccmd`).
    mousex: i32,
    /// Most recent raw mouse delta on the Y axis (cleared each tic by `G_BuildTiccmd`).
    mousey: i32,
    /// Tics since the last forward-mouse click for double-click-as-use detection.
    dclicktime: i32,
    /// Last sampled state of the mousebforward button for double-click detection.
    dclickstate: boolean,
    /// Click count toward a forward-mouse double-click (2 triggers `BT_USE`).
    dclicks: i32,
    /// Tics since the last strafe-button click for double-click detection.
    dclicktime2: i32,
    /// Last sampled state of the strafe button for double-click detection.
    dclickstate2: boolean,
    /// Click count toward a strafe-button double-click (2 triggers `BT_USE`).
    dclicks2: i32,
    /// Most recent joystick X (turn / strafe) axis value.
    joyxmove: i32,
    /// Most recent joystick Y (forward / back) axis value.
    joyymove: i32,
    /// Most recent joystick strafe axis value.
    joystrafemove: i32,
    /// Joystick button held-down flags (slot 0 unused to allow `[-1]` indexing).
    // joybuttons is &joyarray[1] in C — negative indexing; access via joyarray[1+n]
    joyarray: [boolean; MAX_JOY_BUTTONS + 1],
    /// One-tic flag requesting a pause-toggle ticcmd from `G_BuildTiccmd`.
    sendpause: boolean,
    /// One-tic flag requesting a savegame ticcmd from `G_BuildTiccmd`.
    sendsave: boolean,
    /// Selected savegame slot for the next save (set by `G_SaveGame`).
    savegameslot: i32,
    /// Description text for the next savegame (set by `G_SaveGame`).
    savedescription: [u8; 32],
    /// Pending weapon-cycle direction: `-1` previous, `+1` next, `0` none.
    next_weapon: i32,
    /// Carry for low-resolution turn rounding (static local in `G_BuildTiccmd`).
    lowres_turn_carry: i16,
}

impl GameInputState {
    /// No input held, no request pending.
    const fn new() -> Self {
        Self {
            gamekeydown: [0; NUMKEYS],
            turnheld: 0,
            mousearray: [0; MAX_MOUSE_BUTTONS + 1],
            mousex: 0,
            mousey: 0,
            dclicktime: 0,
            dclickstate: 0,
            dclicks: 0,
            dclicktime2: 0,
            dclickstate2: 0,
            dclicks2: 0,
            joyxmove: 0,
            joyymove: 0,
            joystrafemove: 0,
            joyarray: [0; MAX_JOY_BUTTONS + 1],
            sendpause: 0,
            sendsave: 0,
            savegameslot: 0,
            savedescription: [0; 32],
            next_weapon: 0,
            lowres_turn_carry: 0,
        }
    }

    /// Read a mouse button state by C-style "may be `-1`" index.
    ///
    /// Vanilla Doom stores mouse buttons in `mousearray[MAX_MOUSE_BUTTONS+1]`
    /// with `mousebuttons = &mousearray[1]`, so `mousebuttons[-1]` is a legal
    /// "no button bound" sentinel that always reads false. This helper restores
    /// the same behaviour without aliasing.
    ///
    /// Returns `0` for `n < 0` or `n >= MAX_MOUSE_BUTTONS`, otherwise the
    /// currently latched state of mouse button `n`.
    fn mousebutton(&self, n: i32) -> boolean {
        if n < 0 || n >= MAX_MOUSE_BUTTONS as i32 {
            return 0;
        }
        self.mousearray[(n + 1) as usize]
    }

    /// Joystick equivalent of [`GameInputState::mousebutton`]; same
    /// `-1`-as-unbound semantics.
    fn joybutton(&self, n: i32) -> boolean {
        if n < 0 || n >= MAX_JOY_BUTTONS as i32 {
            return 0;
        }
        self.joyarray[(n + 1) as usize]
    }
}

/// Input latches and the pending save request.
static GAME_INPUT: Global<GameInputState> = Global::new(GameInputState::new());

// ---------------------------------------------------------------------------
// External function and variable declarations
// ---------------------------------------------------------------------------

/// Engine-wide fatal error and clean shutdown from `i_system.c` (the
/// latter used by single demos). Messages with arguments go through
/// `i_error!`, which formats them with Rust syntax.
use crate::engine::interface::system::{I_Error, I_Quit};

/// `gametic` is the global engine tic counter; `ticdup` is the demo dup factor.
/// Both originate from `d_loop.c`.
use crate::engine::doom::main_loop::{gametic, ticdup};

/// Command-line `-fast`, `-nomonsters`, `-respawn` toggles, the wipe-state
/// latch, demo advancement, and the title-screen page ticker from `d_main.c`.
use crate::engine::doom::main::{
    fastparm, nomonsters, respawnparm, D_AdvanceDemo, D_PageTicker, DOOM_MAIN,
};

/// Clear the `-respawn`, `-fast` and `-nomonsters` toggles.
fn clear_monster_parms() {
    let mut main = DOOM_MAIN.borrow_mut();
    main.respawnparm = 0;
    main.fastparm = 0;
    main.nomonsters = 0;
}

/// Automap toggle, responder, stop hook and ticker from `am_map.c`.
use crate::engine::automap::{automap_active, AM_Responder, AM_Stop, AM_Ticker, AUTOMAP};

/// Per-player command buffer ring from `d_net.c`.
use crate::engine::doom::net::netcmds;

/// Savegame I/O primitives (header read/write, world/thinker/specials
/// archive/unarchive, file-path helpers) from `p_saveg.c`.
use crate::engine::play::saveg::{
    P_ArchivePlayers, P_ArchiveSpecials, P_ArchiveThinkers, P_ArchiveWorld, P_ReadSaveGameEOF,
    P_ReadSaveGameHeader, P_SaveGameFile, P_TempSaveGameFile, P_UnArchivePlayers,
    P_UnArchiveSpecials, P_UnArchiveThinkers, P_UnArchiveWorld, P_WriteSaveGameEOF,
    P_WriteSaveGameHeader, SAVEG,
};

/// Top-level map loader from `p_setup.c`.
use crate::engine::play::setup::P_SetupLevel;

/// Mobj lifecycle and spawn helpers from `p_mobj.c`.
use crate::engine::play::mobj::{P_RemoveMobj, P_SpawnMobj, P_SpawnPlayer};

/// Movement collision check used to validate respawn spots from `p_map.c`.
use crate::engine::play::map::P_CheckPosition;

/// 3D view sizing helpers and point-to-subsector lookup from `r_main.c`.
use crate::engine::render_3d::main::{R_ExecuteSetViewSize, R_PointInSubsector, VIEW};

/// Texture/flat name resolution from `r_data.c`.
use crate::engine::render_3d::data::{R_FlatNumForName, R_TextureNumForName};

/// Sky-flat index and active sky texture id from `r_sky.c`.
use crate::engine::render_3d::sky::SKY;

/// Status-bar back-screen painter from `r_draw.c`.
use crate::engine::render_3d::draw::R_FillBackScreen;

/// Zone-allocator heap check and (de)allocation primitives from `z_zone.c`.
use crate::engine::zone::{Z_CheckHeap, Z_Free, Z_Malloc};

/// `leveltime` counter (tics since level start) and thinker tick driver from
/// `p_tick.c`.
use crate::engine::play::tick::{leveltime, P_Ticker, TICK};

/// Status-bar event responder and ticker from `st_stuff.c`.
use crate::engine::status::stuff::{ST_Responder, ST_Ticker};

/// HUD chat / message primitives and player-name table from `hu_stuff.c`.
use crate::engine::hud::stuff::{player_names, HU_Responder, HU_Ticker, HU_dequeueChatChar};

/// Intermission start/end/tick hooks from `wi_stuff.c`.
use crate::engine::intermission::{WI_End, WI_Start, WI_Ticker};

/// Finale (text crawl / cast call) responder, starter and ticker from `f_finale.c`.
use crate::engine::finale::{F_Responder, F_StartFinale, F_Ticker};

/// Sound channel pause/resume and one-shot sfx start from `s_sound.c`.
use crate::engine::sound::{S_PauseSound, S_ResumeSound, S_StartSound};

/// Mouse-sensitivity slider and main-menu opener from `m_menu.c`.
use crate::engine::misc::menu::{M_StartControlPanel, MENU};

/// String / file / formatted-print utilities from `m_misc.c`.
use crate::engine::misc::{atoi, M_StringCopy, M_TempFile, M_WriteFile};

/// Command-line argument table and lookup helpers from `m_argv.c`.
use crate::engine::misc::argv::{argument, M_CheckParm, M_CheckParmWithArgs};

/// `rndindex` consistency-check cookie and seed reset from `m_random.c`.
use crate::engine::misc::random::{M_ClearRandom, RANDOM};

/// Statistics-dump hook used after a level completes, from `statdump.c`.
use crate::engine::intermission::statdump::StatCopy;

/// Screenshot grabber from `v_video.c`.
use crate::engine::render_2d::V_ScreenShot;

/// Realtime tick counter from `i_timer.c`.
use crate::engine::interface::timer::I_GetTime;

/// WAD lump cache / release / lookup primitives from `w_wad.c`.
use crate::engine::wad::{W_CacheLumpName, W_CheckNumForName, W_ReleaseLumpName};

/// Configurable key, joystick and mouse button bindings from `m_controls.c`.
/// `G_BuildTiccmd` and `G_Responder` copy the bindings they interrogate out
/// of `CONTROLS` once per call.
use crate::engine::misc::controls::{ControlsState, CONTROLS};

/// `PU_STATIC` Z_Malloc purge tag used for the demo buffer and demo filename.
use crate::engine::zone::PU_STATIC;

/// Dehacked string-substitution hook (identity in this build).
use crate::engine::dehacked::DEH_String;

// ---------------------------------------------------------------------------
// logical_gamemission helper (mirrors doomstat.h macro)
// ---------------------------------------------------------------------------

/// Mirror of the `logical_gamemission` macro from `doomstat.h`.
///
/// Collapses the TNT/Plutonia mission types onto plain `doom2`, since they
/// share the Doom II ruleset; called by `weapon_selectable` to avoid
/// branching on every Doom II variant individually.
///
/// # Safety
/// Reads the `gamemission` global; safe as long as the caller respects the
/// single-threaded engine convention.
#[inline]
unsafe fn logical_gamemission() -> i32 {
    use crate::engine::doom::mode::{doom2, pack_plut, pack_tnt};
    let gamemission = gamemission();
    if gamemission == pack_tnt || gamemission == pack_plut {
        doom2
    } else {
        gamemission
    }
}

// ---------------------------------------------------------------------------
// G_CmdChecksum
// ---------------------------------------------------------------------------

/// Wrapping sum of the first `sizeof(ticcmd_t)/4 - 1` little-endian 4-byte
/// words of the C `ticcmd_t`, rebuilt from the named fields.
///
/// The three summed words are `forwardmove`, `sidemove`, `angleturn`;
/// `chatchar`, `buttons`, `consistancy`, `buttons2`; and `inventory`. The
/// final word (`lookfly`, `arti` and padding) is excluded. As in vanilla
/// `g_game.c`, nothing in the engine calls it.
pub fn G_CmdChecksum(cmd: &TiccmdT) -> i32 {
    let movement_word = i32::from_le_bytes([
        cmd.forwardmove as u8,
        cmd.sidemove as u8,
        cmd.angleturn.to_le_bytes()[0],
        cmd.angleturn.to_le_bytes()[1],
    ]);
    let button_word =
        i32::from_le_bytes([cmd.chatchar, cmd.buttons, cmd.consistancy, cmd.buttons2]);
    movement_word
        .wrapping_add(button_word)
        .wrapping_add(cmd.inventory)
}

// ---------------------------------------------------------------------------
// WeaponSelectable (static)
// ---------------------------------------------------------------------------

/// Decide whether `weapon` is eligible for selection by the prev/next
/// weapon cycler.
///
/// Mirrors `WeaponSelectable` in `g_game.c`. Returns `false` when the
/// weapon is unavailable in the current `gamemission` / `gamemode` (e.g.
/// supershotgun in Doom 1, plasma/BFG in shareware), when the console
/// player does not own it, or when it is the fist while the chainsaw is
/// owned without an active berserk power.
///
/// # Safety
/// Reads several engine globals (`gamemission`, `gamemode`, `players`,
/// `consoleplayer`); safe under the single-threaded engine convention.
unsafe fn weapon_selectable(weapon: i32) -> bool {
    // Can't select supershotgun in Doom 1.
    if weapon == wp_supershotgun && logical_gamemission() == doom {
        return false;
    }
    // Plasma and BFG unavailable in shareware.
    if (weapon == wp_plasma || weapon == wp_bfg) && gamemission() == doom && gamemode() == shareware
    {
        return false;
    }
    let console = &*console_player();
    if console.weaponowned[weapon as usize] == 0 {
        return false;
    }
    // Can't select fist if we have chainsaw, unless we also have berserk.
    if weapon == wp_fist
        && console.weaponowned[wp_chainsaw as usize] != 0
        && console.powers[pw_strength] == 0
    {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// G_NextWeapon (static)
// ---------------------------------------------------------------------------

/// Walk [`WEAPON_ORDER_TABLE`] from the player's current weapon and return
/// the slot number to switch to.
///
/// `direction` is `+1` for "next weapon" or `-1` for "previous weapon". The
/// search wraps and stops on the first selectable entry; if none are
/// selectable it returns the slot for the player's current weapon (the
/// `i == start_i` guard prevents an infinite loop).
///
/// Mirrors `G_NextWeapon` in `g_game.c`.
///
/// # Safety
/// Reads `players[consoleplayer]` and `WEAPON_ORDER_TABLE`; safe under the
/// single-threaded engine convention.
unsafe fn g_next_weapon(direction: i32) -> i32 {
    let console = &*console_player();
    let weapon = if console.pendingweapon == wp_nochange {
        console.readyweapon
    } else {
        console.pendingweapon
    };

    let n = WEAPON_ORDER_TABLE.len() as i32;
    let mut i = 0;
    while i < n {
        if WEAPON_ORDER_TABLE[i as usize].weapon == weapon {
            break;
        }
        i += 1;
    }

    let start_i = i;
    loop {
        i += direction;
        i = (i + n) % n;
        if i == start_i || weapon_selectable(WEAPON_ORDER_TABLE[i as usize].weapon) {
            break;
        }
    }
    WEAPON_ORDER_TABLE[i as usize].weapon_num
}

// ---------------------------------------------------------------------------
// G_BuildTiccmd
// ---------------------------------------------------------------------------

/// Assemble one tic's `TiccmdT` for the local console player from the latest
/// input snapshot, and apply low-resolution turn rounding when recording a
/// vanilla demo.
///
/// `maketic` is the tic number being built; it indexes the consistency-check
/// ring buffer for the local player. Behaviour mirrors `G_BuildTiccmd` in
/// `g_game.c` exactly:
///
/// * Forward / strafe / turn input from keys, joystick and mouse is summed
///   with two-stage accelerative turning (first 6 tics use the "slow" turn
///   table, beyond that the regular table or the speed-button "fast" table).
/// * Mouse movement is added to `forward` directly and to either `side`
///   (when strafing) or `angleturn` (otherwise) scaled by `mouseSensitivity`.
/// * Weapon-cycle hotkeys are encoded into the `BT_CHANGE` field.
/// * Mouse forward/strafe double-clicks synthesise `BT_USE` when
///   `dclick_use` is on.
/// * Pause and savegame requests are encoded into the `BT_SPECIAL` field.
/// * When `lowres_turn` is set, `angleturn` is rounded to a 256-BAM boundary
///   and the residual carried into the next tic (so successive small turns
///   accumulate accurately in 1-byte-per-tic demos).
///
/// # Safety
/// `cmd` must point to a writable `TiccmdT`; the entire struct is zeroed
/// before assembly.
pub unsafe fn G_BuildTiccmd(cmd: *mut TiccmdT, maketic: i32) {
    use crate::engine::doom::main_loop::BACKUPTICS;

    let ControlsState {
        key_strafe,
        mousebstrafe,
        joybstrafe,
        key_speed,
        joybspeed,
        key_right,
        key_left,
        key_up,
        key_down,
        key_strafeleft,
        joybstrafeleft,
        mousebstrafeleft,
        key_straferight,
        joybstraferight,
        mousebstraferight,
        key_fire,
        mousebfire,
        joybfire,
        key_use,
        joybuse,
        mousebuse,
        key_weapon1,
        key_weapon2,
        key_weapon3,
        key_weapon4,
        key_weapon5,
        key_weapon6,
        key_weapon7,
        key_weapon8,
        mousebforward,
        mousebbackward,
        dclick_use,
        ..
    } = CONTROLS.get();
    let ticdup = ticdup();
    let (consistancy, gamestate, forwardmove, sidemove, angleturn, lowres_turn) = {
        let game = GAME.borrow();
        (
            game.consistancy[game.consoleplayer as usize][(maketic as usize) % BACKUPTICS],
            game.gamestate,
            game.forwardmove,
            game.sidemove,
            game.angleturn,
            game.lowres_turn,
        )
    };
    // Neither the chat queue nor the weapon cycler touches the input latches.
    let mut input = GAME_INPUT.borrow_mut();

    let cmd = &mut *cmd;
    *cmd = std::mem::zeroed();

    cmd.consistancy = consistancy;

    let strafe = (input.gamekeydown[key_strafe as usize] != 0)
        || (input.mousebutton(mousebstrafe) != 0)
        || (input.joybutton(joybstrafe) != 0);

    // "joyb_speed = 31" autorun hack: key_speed >= NUMKEYS means always running
    let speed = (key_speed >= NUMKEYS as i32)
        || (joybspeed >= MAX_JOY_BUTTONS as i32)
        || (input.gamekeydown[key_speed as usize] != 0)
        || (input.joybutton(joybspeed) != 0);
    let speed = speed as usize;

    let mut forward: i32 = 0;
    let mut side: i32 = 0;

    // Two-stage accelerative turning
    if input.joyxmove != 0
        || input.gamekeydown[key_right as usize] != 0
        || input.gamekeydown[key_left as usize] != 0
    {
        input.turnheld += ticdup;
    } else {
        input.turnheld = 0;
    }

    let tspeed = if input.turnheld < SLOWTURNTICS {
        2usize
    } else {
        speed
    };

    if strafe {
        if input.gamekeydown[key_right as usize] != 0 {
            side += sidemove[speed];
        }
        if input.gamekeydown[key_left as usize] != 0 {
            side -= sidemove[speed];
        }
        if input.joyxmove > 0 {
            side += sidemove[speed];
        }
        if input.joyxmove < 0 {
            side -= sidemove[speed];
        }
    } else {
        if input.gamekeydown[key_right as usize] != 0 {
            cmd.angleturn -= angleturn[tspeed] as i16;
        }
        if input.gamekeydown[key_left as usize] != 0 {
            cmd.angleturn += angleturn[tspeed] as i16;
        }
        if input.joyxmove > 0 {
            cmd.angleturn -= angleturn[tspeed] as i16;
        }
        if input.joyxmove < 0 {
            cmd.angleturn += angleturn[tspeed] as i16;
        }
    }

    if input.gamekeydown[key_up as usize] != 0 {
        forward += forwardmove[speed];
    }
    if input.gamekeydown[key_down as usize] != 0 {
        forward -= forwardmove[speed];
    }
    if input.joyymove < 0 {
        forward += forwardmove[speed];
    }
    if input.joyymove > 0 {
        forward -= forwardmove[speed];
    }

    if input.gamekeydown[key_strafeleft as usize] != 0
        || input.joybutton(joybstrafeleft) != 0
        || input.mousebutton(mousebstrafeleft) != 0
        || input.joystrafemove < 0
    {
        side -= sidemove[speed];
    }
    if input.gamekeydown[key_straferight as usize] != 0
        || input.joybutton(joybstraferight) != 0
        || input.mousebutton(mousebstraferight) != 0
        || input.joystrafemove > 0
    {
        side += sidemove[speed];
    }

    // Buttons
    cmd.chatchar = HU_dequeueChatChar();

    if input.gamekeydown[key_fire as usize] != 0
        || input.mousebutton(mousebfire) != 0
        || input.joybutton(joybfire) != 0
    {
        cmd.buttons |= BT_ATTACK;
    }

    if input.gamekeydown[key_use as usize] != 0
        || input.joybutton(joybuse) != 0
        || input.mousebutton(mousebuse) != 0
    {
        cmd.buttons |= BT_USE;
        input.dclicks = 0;
    }

    // Weapon cycling
    if gamestate == GS_LEVEL && input.next_weapon != 0 {
        let i = g_next_weapon(input.next_weapon);
        cmd.buttons |= BT_CHANGE;
        cmd.buttons |= (i as u8) << BT_WEAPONSHIFT;
    } else {
        let weapon_keys_vals = [
            key_weapon1,
            key_weapon2,
            key_weapon3,
            key_weapon4,
            key_weapon5,
            key_weapon6,
            key_weapon7,
            key_weapon8,
        ];
        for (i, &key) in weapon_keys_vals.iter().enumerate() {
            if input.gamekeydown[key as usize] != 0 {
                cmd.buttons |= BT_CHANGE;
                cmd.buttons |= (i as u8) << BT_WEAPONSHIFT;
                break;
            }
        }
    }
    input.next_weapon = 0;

    // Mouse forward/backward
    if input.mousebutton(mousebforward) != 0 {
        forward += forwardmove[speed];
    }
    if input.mousebutton(mousebbackward) != 0 {
        forward -= forwardmove[speed];
    }

    // Double-click use
    if dclick_use != 0 {
        if input.mousebutton(mousebforward) != input.dclickstate && input.dclicktime > 1 {
            input.dclickstate = input.mousebutton(mousebforward);
            if input.dclickstate != 0 {
                input.dclicks += 1;
            }
            if input.dclicks == 2 {
                cmd.buttons |= BT_USE;
                input.dclicks = 0;
            } else {
                input.dclicktime = 0;
            }
        } else {
            input.dclicktime += ticdup;
            if input.dclicktime > 20 {
                input.dclicks = 0;
                input.dclickstate = 0;
            }
        }

        let bstrafe =
            (input.mousebutton(mousebstrafe) != 0 || input.joybutton(joybstrafe) != 0) as boolean;
        if bstrafe != input.dclickstate2 && input.dclicktime2 > 1 {
            input.dclickstate2 = bstrafe;
            if input.dclickstate2 != 0 {
                input.dclicks2 += 1;
            }
            if input.dclicks2 == 2 {
                cmd.buttons |= BT_USE;
                input.dclicks2 = 0;
            } else {
                input.dclicktime2 = 0;
            }
        } else {
            input.dclicktime2 += ticdup;
            if input.dclicktime2 > 20 {
                input.dclicks2 = 0;
                input.dclickstate2 = 0;
            }
        }
    }

    forward += input.mousey;
    if strafe {
        side += input.mousex * 2;
    } else {
        cmd.angleturn -= (input.mousex * 0x8) as i16;
    }

    if input.mousex == 0 {
        GAME.borrow_mut().testcontrols_mousespeed = 0;
    }
    input.mousex = 0;
    input.mousey = 0;

    // Clamp movement
    let maxplmove = forwardmove[1];
    if forward > maxplmove {
        forward = maxplmove;
    } else if forward < -maxplmove {
        forward = -maxplmove;
    }
    if side > maxplmove {
        side = maxplmove;
    } else if side < -maxplmove {
        side = -maxplmove;
    }

    cmd.forwardmove = (cmd.forwardmove as i32 + forward) as i8;
    cmd.sidemove = (cmd.sidemove as i32 + side) as i8;

    // Special buttons
    if input.sendpause != 0 {
        input.sendpause = 0;
        cmd.buttons = BT_SPECIAL | BTS_PAUSE;
    }
    if input.sendsave != 0 {
        input.sendsave = 0;
        cmd.buttons = BT_SPECIAL | BTS_SAVEGAME | (input.savegameslot as u8) << BTS_SAVESHIFT;
    }

    // Low-resolution turning
    if lowres_turn != 0 {
        let desired: i16 = cmd.angleturn.wrapping_add(input.lowres_turn_carry);
        cmd.angleturn = ((desired as i32 + 128) & 0xff00) as i16;
        input.lowres_turn_carry = desired.wrapping_sub(cmd.angleturn);
    }
}

// ---------------------------------------------------------------------------
// G_DoLoadLevel
// ---------------------------------------------------------------------------

/// Execute the deferred `ga_loadlevel` action: load the current
/// `gameepisode`/`gamemap`, reset per-player input state, force a wipe and
/// transition into `GS_LEVEL`.
///
/// Also fixes up the Doom II / Final Doom / Chex sky textures (`SKY1`/`SKY2`
/// /`SKY3` based on `gamemap`) and resets the input latches so movement
/// keys held across the load do not produce phantom input.
///
/// Players in `PST_DEAD` are flipped to `PST_REBORN` so the per-player
/// reborn loop in `G_Ticker` will respawn them.
///
/// # Safety
/// Mutates many engine globals (`gamestate`, `wipegamestate`, `levelstarttic`,
/// the input rings, the players array).
pub unsafe fn G_DoLoadLevel() {
    SKY.borrow_mut().skyflatnum = R_FlatNumForName(DEH_String(b"F_SKY1"));

    // Fix sky texture for Final Doom / Chex
    if gamemode() == commercial && (gameversion() == exe_final2 || gameversion() == exe_chex) {
        let gamemap = gamemap();
        let skytexturename: &[u8] = if gamemap < 12 {
            b"SKY1"
        } else if gamemap < 21 {
            b"SKY2"
        } else {
            b"SKY3"
        };
        SKY.borrow_mut().skytexture = R_TextureNumForName(DEH_String(skytexturename));
    }

    GAME.borrow_mut().levelstarttic = gametic();

    {
        let mut main = DOOM_MAIN.borrow_mut();
        if main.wipegamestate == GS_LEVEL {
            main.wipegamestate = -1; // force a wipe
        }
    }

    let (gameepisode, gamemap, gameskill) = {
        let mut game = GAME.borrow_mut();
        game.gamestate = GS_LEVEL;

        for i in 0..MAXPLAYERS {
            game.turbodetected[i] = 0;
            let player = &mut *player(i);
            if game.playeringame[i] != 0 && player.playerstate == PST_DEAD {
                player.playerstate = PST_REBORN;
            }
            player.frags = [0; MAXPLAYERS];
        }
        (game.gameepisode, game.gamemap, game.gameskill)
    };

    P_SetupLevel(gameepisode, gamemap, 0, gameskill);
    let testcontrols = {
        let mut game = GAME.borrow_mut();
        game.displayplayer = game.consoleplayer;
        game.gameaction = ga_nothing;
        game.paused = 0;
        game.testcontrols
    };
    Z_CheckHeap();

    // Clear input state
    {
        let mut input = GAME_INPUT.borrow_mut();
        input.gamekeydown = [0; NUMKEYS];
        input.joyxmove = 0;
        input.joyymove = 0;
        input.joystrafemove = 0;
        input.mousex = 0;
        input.mousey = 0;
        input.sendpause = 0;
        input.sendsave = 0;
        input.mousearray = [0; MAX_MOUSE_BUTTONS + 1];
        input.joyarray = [0; MAX_JOY_BUTTONS + 1];
    }

    if testcontrols != 0 {
        (*console_player()).message = PlayerMessage::new("Press escape to quit.");
    }
}

// ---------------------------------------------------------------------------
// SetJoyButtons / SetMouseButtons (static helpers)
// ---------------------------------------------------------------------------

/// Update the joystick button latch from a packed bitmask and detect
/// rising edges of the prev/next weapon buttons, scheduling a weapon cycle
/// in `next_weapon` for the next `G_BuildTiccmd`.
fn set_joy_buttons(buttons_mask: u32) {
    let ControlsState {
        joybprevweapon,
        joybnextweapon,
        ..
    } = CONTROLS.get();
    let mut input = GAME_INPUT.borrow_mut();
    for i in 0..MAX_JOY_BUTTONS {
        let button_on = ((buttons_mask >> i) & 1) != 0;
        if input.joyarray[i + 1] == 0 && button_on {
            if i as i32 == joybprevweapon {
                input.next_weapon = -1;
            } else if i as i32 == joybnextweapon {
                input.next_weapon = 1;
            }
        }
        input.joyarray[i + 1] = button_on as boolean;
    }
}

/// Mouse-button equivalent of [`set_joy_buttons`]: latches per-button state
/// and arms a weapon-cycle on the rising edge of the bound mouse buttons.
fn set_mouse_buttons(buttons_mask: u32) {
    let ControlsState {
        mousebprevweapon,
        mousebnextweapon,
        ..
    } = CONTROLS.get();
    let mut input = GAME_INPUT.borrow_mut();
    for i in 0..MAX_MOUSE_BUTTONS {
        let button_on = ((buttons_mask >> i) & 1) != 0;
        if input.mousearray[i + 1] == 0 && button_on {
            if i as i32 == mousebprevweapon {
                input.next_weapon = -1;
            } else if i as i32 == mousebnextweapon {
                input.next_weapon = 1;
            }
        }
        input.mousearray[i + 1] = button_on as boolean;
    }
}

// ---------------------------------------------------------------------------
// G_Responder
// ---------------------------------------------------------------------------

/// Handle one input event during gameplay or the demo loop.
///
/// Returns non-zero ("event consumed") when the event was handled here and
/// should not propagate further; zero lets later responders (menu, console)
/// see it. Mirrors `G_Responder` in `g_game.c`:
///
/// * Spy-mode (`key_spy`) cycles `displayplayer` even during demo playback.
/// * During the demo loop / playback, any key, mouse-click or joystick
///   button press pops the main menu.
/// * In `GS_LEVEL`, defers to HU / ST / AM responders in order.
/// * In `GS_FINALE`, defers to `F_Responder`.
/// * Otherwise routes by `event_t::kind`: keydown updates `gamekeydown`
///   (with `key_pause` setting `sendpause`), keyup clears it, mouse and
///   joystick events latch axes and buttons.
///
/// # Safety
/// Dereferences `ev` and mutates the input latches; safe under the
/// single-threaded engine convention.
pub unsafe fn G_Responder(ev: *mut event_t) -> boolean {
    let ev = &*ev;
    let ControlsState {
        key_spy,
        key_prevweapon,
        key_nextweapon,
        key_pause,
        ..
    } = CONTROLS.get();

    let (gamestate, gameaction, singledemo, deathmatch, demoplayback, testcontrols) = {
        let game = GAME.borrow();
        (
            game.gamestate,
            game.gameaction,
            game.singledemo,
            game.deathmatch,
            game.demoplayback,
            game.testcontrols,
        )
    };

    // Spy mode changes even during demo
    if gamestate == GS_LEVEL
        && ev.kind == 0 // ev_keydown
        && ev.data1 == key_spy
        && (singledemo != 0 || deathmatch == 0)
    {
        let mut game = GAME.borrow_mut();
        loop {
            game.displayplayer += 1;
            if game.displayplayer == MAXPLAYERS as i32 {
                game.displayplayer = 0;
            }
            if game.playeringame[game.displayplayer as usize] != 0
                || game.displayplayer == game.consoleplayer
            {
                break;
            }
        }
        return 1;
    }

    // Any key pops up menu if in demos
    if gameaction == ga_nothing
        && singledemo == 0
        && (demoplayback != 0 || gamestate == GS_DEMOSCREEN)
    {
        if ev.kind == 0 // ev_keydown
            || (ev.kind == 2 && ev.data1 != 0) // ev_mouse with buttons
            || (ev.kind == 3 && ev.data1 != 0)
        // ev_joystick with buttons
        {
            M_StartControlPanel();
            return 1;
        }
        return 0;
    }

    if gamestate == GS_LEVEL {
        if HU_Responder(ev as *const event_t as *mut event_t) != 0 {
            return 1;
        }
        if ST_Responder(ev as *const event_t as *mut event_t) != 0 {
            return 1;
        }
        if AM_Responder(ev as *const event_t as *mut event_t) != 0 {
            return 1;
        }
    }

    // The level responders above may have changed the state.
    if self::gamestate() == GS_FINALE && F_Responder(ev as *const event_t as *mut event_t) != 0 {
        return 1;
    }

    if testcontrols != 0 && ev.kind == 2 {
        // ev_mouse
        GAME.borrow_mut().testcontrols_mousespeed = ev.data2.abs();
    }

    // Prev/next weapon keys
    if ev.kind == 0 && ev.data1 == key_prevweapon {
        GAME_INPUT.borrow_mut().next_weapon = -1;
    } else if ev.kind == 0 && ev.data1 == key_nextweapon {
        GAME_INPUT.borrow_mut().next_weapon = 1;
    }

    match ev.kind {
        0 => {
            // ev_keydown
            let mut input = GAME_INPUT.borrow_mut();
            if ev.data1 == key_pause {
                input.sendpause = 1;
            } else if (ev.data1 as usize) < NUMKEYS {
                input.gamekeydown[ev.data1 as usize] = 1;
            }
            return 1;
        }
        1 => {
            // ev_keyup
            if (ev.data1 as usize) < NUMKEYS {
                GAME_INPUT.borrow_mut().gamekeydown[ev.data1 as usize] = 0;
            }
            return 0;
        }
        2 => {
            // ev_mouse
            set_mouse_buttons(ev.data1 as u32);
            let mouse_sensitivity = MENU.borrow().mouseSensitivity;
            let mut input = GAME_INPUT.borrow_mut();
            input.mousex = ev.data2 * (mouse_sensitivity + 5) / 10;
            input.mousey = ev.data3 * (mouse_sensitivity + 5) / 10;
            return 1;
        }
        3 => {
            // ev_joystick
            set_joy_buttons(ev.data1 as u32);
            let mut input = GAME_INPUT.borrow_mut();
            input.joyxmove = ev.data2;
            input.joyymove = ev.data3;
            input.joystrafemove = ev.data4;
            return 1;
        }
        _ => {}
    }
    0
}

// ---------------------------------------------------------------------------
// G_Ticker
// ---------------------------------------------------------------------------

/// Advance the gameplay state machine by exactly one tic (1/35 s).
///
/// The tic dispatches in three phases, in order:
///
/// 1. **Reborn pass** - every player in `PST_REBORN` is respawned via
///    `G_DoReborn`.
/// 2. **Gameaction drain** - the `gameaction` queue is processed until empty,
///    dispatching to `G_DoLoadLevel`, `G_DoNewGame`, `G_DoLoadGame`,
///    `G_DoSaveGame`, `G_DoPlayDemo`, `G_DoCompleted`, `F_StartFinale`,
///    `G_DoWorldDone` or a screenshot grab.
/// 3. **Per-player ticcmd pass** - net commands are copied into each player
///    slot, demo I/O runs, turbo banners are emitted (every ~4 seconds,
///    offset per player), and the consistency-check ring is updated when
///    netgame / non-netdemo / `gametic % ticdup == 0`.
///
/// Special buttons (pause toggle, savegame request) are then decoded; finally
/// the active state ticker runs - `P_Ticker` / `ST_Ticker` / `AM_Ticker` /
/// `HU_Ticker` for `GS_LEVEL`, `WI_Ticker` for `GS_INTERMISSION`,
/// `F_Ticker` for `GS_FINALE`, `D_PageTicker` for `GS_DEMOSCREEN`.
///
/// # Safety
/// Mutates virtually every game-loop global; intended to be called at most
/// once per tic from the engine main loop.
pub unsafe fn G_Ticker() {
    use crate::engine::doom::main_loop::BACKUPTICS;

    // Player reborns
    for i in 0..MAXPLAYERS {
        if playeringame(i) != 0 && (*player(i)).playerstate == PST_REBORN {
            G_DoReborn(i as i32);
        }
    }

    // Process pending game actions
    loop {
        match gameaction() {
            ga_nothing => break,
            ga_loadlevel => G_DoLoadLevel(),
            ga_newgame => G_DoNewGame(),
            ga_loadgame => G_DoLoadGame(),
            ga_savegame => G_DoSaveGame(),
            ga_playdemo => G_DoPlayDemo(),
            ga_completed => G_DoCompleted(),
            ga_victory => F_StartFinale(),
            ga_worlddone => G_DoWorldDone(),
            ga_screenshot => {
                V_ScreenShot();
                (*console_player()).message = PlayerMessage::new(DEH_String("screen shot"));
                GAME.borrow_mut().gameaction = ga_nothing;
            }
            _ => {}
        }
    }

    // Get commands, check consistency, build new consistency check
    let (gametic, ticdup) = (gametic(), ticdup());
    let buf = ((gametic / ticdup) as usize) % BACKUPTICS;

    for i in 0..MAXPLAYERS {
        if playeringame(i) != 0 {
            let cmd = &raw mut (*player(i)).cmd;

            // Copy net command into player command
            std::ptr::copy_nonoverlapping(netcmds().add(i), cmd, 1);

            // Reading can end the demo and writing can stop the recording,
            // so each flag is read just before it is tested.
            if demoplayback() != 0 {
                G_ReadDemoTiccmd(cmd);
            }
            if GAME.borrow().demorecording != 0 {
                G_WriteDemoTiccmd(cmd);
            }

            let mut game = GAME.borrow_mut();

            // Turbo detection
            if (*cmd).forwardmove as i32 > TURBOTHRESHOLD {
                game.turbodetected[i] = 1;
            }

            if (gametic & 31) == 0
                && ((gametic >> 5) % MAXPLAYERS as i32) == i as i32
                && game.turbodetected[i] != 0
            {
                (*player(game.consoleplayer as usize)).message =
                    PlayerMessage::new(format!("{} is turbo!", player_names[i]));
                game.turbodetected[i] = 0;
            }

            if game.netgame != 0 && game.netdemo == 0 && (gametic % ticdup) == 0 {
                if gametic > BACKUPTICS as i32 && game.consistancy[i][buf] != (*cmd).consistancy {
                    crate::i_error!(
                        "consistency failure ({} should be {})",
                        (*cmd).consistancy as i32,
                        game.consistancy[i][buf] as i32,
                    );
                }
                let mo = (*player(i)).mo;
                if !mo.is_null() {
                    game.consistancy[i][buf] = (*mo).x as u8;
                } else {
                    game.consistancy[i][buf] = RANDOM.get().rndindex as u8;
                }
            }
        }
    }

    // Check for special buttons
    for i in 0..MAXPLAYERS {
        if playeringame(i) != 0 {
            let buttons = (*player(i)).cmd.buttons;
            if buttons & BT_SPECIAL != 0 {
                match buttons & BT_SPECIALMASK {
                    BTS_PAUSE => {
                        let paused = {
                            let mut game = GAME.borrow_mut();
                            game.paused ^= 1;
                            game.paused
                        };
                        if paused != 0 {
                            S_PauseSound();
                        } else {
                            S_ResumeSound();
                        }
                    }
                    BTS_SAVEGAME => {
                        {
                            let mut input = GAME_INPUT.borrow_mut();
                            if input.savedescription[0] == 0 {
                                M_StringCopy(&mut input.savedescription, b"NET GAME");
                            }
                            input.savegameslot = ((buttons & BTS_SAVEMASK) >> BTS_SAVESHIFT) as i32;
                        }
                        GAME.borrow_mut().gameaction = ga_savegame;
                    }
                    _ => {}
                }
            }
        }
    }

    // Check if intermission screen just ended
    let (oldgamestate, gamestate) = {
        let game = GAME.borrow();
        (game.oldgamestate, game.gamestate)
    };
    if oldgamestate == GS_INTERMISSION && gamestate != GS_INTERMISSION {
        WI_End();
    }
    GAME.borrow_mut().oldgamestate = gamestate;

    // Main game-state dispatch
    match gamestate {
        GS_LEVEL => {
            P_Ticker();
            ST_Ticker();
            AM_Ticker();
            HU_Ticker();
        }
        GS_INTERMISSION => {
            WI_Ticker();
        }
        GS_FINALE => {
            F_Ticker();
        }
        GS_DEMOSCREEN => {
            D_PageTicker();
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// G_InitPlayer
// ---------------------------------------------------------------------------

/// Initialise a player slot to its default state at game start.
///
/// Currently a thin wrapper around [`G_PlayerReborn`] (which clears the
/// slot and seeds it with starting health, weapons and ammo); kept as a
/// distinct entry point because vanilla C code invokes it at startup time.
///
/// # Safety
/// `player` must be a valid index into `players[]` (0..`MAXPLAYERS`).
pub unsafe fn G_InitPlayer(player: i32) {
    G_PlayerReborn(player);
}

// ---------------------------------------------------------------------------
// G_PlayerFinishLevel
// ---------------------------------------------------------------------------

/// Strip transient powerups, key cards and HUD effects from `player` when
/// a level completes; keeps weapons, ammo and frags intact.
///
/// Clears `powers[]`, `cards[]`, the invisibility (`MF_SHADOW`) flag on the
/// player's mobj, extra-light, fixed colormap, damage-flash and bonus-flash
/// counters.
///
/// # Safety
/// `player` must index a valid slot whose `mo` pointer is non-null (the
/// caller is `G_DoCompleted`, which guarantees this).
pub unsafe fn G_PlayerFinishLevel(player: i32) {
    let p = &mut *self::player(player as usize);
    p.powers = [0; 6];
    p.cards = [0; 6];
    (*p.mo).flags &= !crate::engine::play::info::MF_SHADOW;
    p.extralight = 0;
    p.fixedcolormap = 0;
    p.damagecount = 0;
    p.bonuscount = 0;
}

// ---------------------------------------------------------------------------
// G_PlayerReborn
// ---------------------------------------------------------------------------

/// Reset a player slot after death, preserving frags and kill/item/secret
/// counters; everything else is zeroed and re-seeded with the vanilla
/// starting inventory (fist + pistol, 50 bullets, `DEH_INITIAL_HEALTH`).
///
/// The `usedown`/`attackdown` latches are set so the player cannot
/// immediately fire or activate switches on the first tic after respawn.
///
/// # Safety
/// `player` must be a valid index into `players[]`.
pub unsafe fn G_PlayerReborn(player: i32) {
    let slot = &mut *self::player(player as usize);
    let frags = slot.frags;
    let killcount = slot.killcount;
    let itemcount = slot.itemcount;
    let secretcount = slot.secretcount;

    *slot = std::mem::zeroed();

    slot.frags = frags;
    slot.killcount = killcount;
    slot.itemcount = itemcount;
    slot.secretcount = secretcount;

    slot.usedown = 1;
    slot.attackdown = 1;
    slot.playerstate = PST_LIVE;
    slot.health = DEH_INITIAL_HEALTH;
    slot.readyweapon = wp_pistol;
    slot.pendingweapon = wp_pistol;
    slot.weaponowned[wp_fist as usize] = 1;
    slot.weaponowned[wp_pistol as usize] = 1;
    slot.ammo[am_clip] = DEH_INITIAL_BULLETS;

    for i in 0..4 {
        // NUMAMMO = 4
        slot.maxammo[i] = maxammo[i];
    }
}

// ---------------------------------------------------------------------------
// G_CheckSpot
// ---------------------------------------------------------------------------

/// Test whether `playernum` can respawn at `mthing` (a `mapthing_t` spawn
/// spot) and, if so, evict the oldest corpse and spawn a teleport-fog mobj.
///
/// Returns non-zero when the spot is usable. On the very first spawn of a
/// level (before any player has a mobj) this only checks against earlier
/// player spawn positions. Otherwise it calls `P_CheckPosition` to verify
/// the spot is clear of monsters / players.
///
/// The teleport-fog placement mirrors the vanilla Doom bug carried in PrBoom+
/// where the `an` angle index overflows into `finetangent[]` for spawns
/// facing certain compass directions; the four special-case `an` values
/// (4096, 5120, 6144, 7168) reproduce that table lookup exactly to keep
/// demos compatible.
///
/// # Safety
/// Dereferences `mthing`; reads and mutates the players / bodyque / corpse
/// queue globals.
pub unsafe fn G_CheckSpot(playernum: i32, mthing: *mut mapthing_t) -> boolean {
    let spawning_mo = (*player(playernum as usize)).mo;
    if spawning_mo.is_null() {
        // First spawn of level, before corpses
        for i in 0..playernum as usize {
            let mo = (*player(i)).mo;
            if (*mo).x == ((*mthing).x.get() as fixed_t) << 16
                && (*mo).y == ((*mthing).y.get() as fixed_t) << 16
            {
                return 0;
            }
        }
        return 1;
    }

    let x = ((*mthing).x.get() as fixed_t) << 16;
    let y = ((*mthing).y.get() as fixed_t) << 16;

    if P_CheckPosition(spawning_mo, x, y) == 0 {
        return 0;
    }

    // Flush old corpse
    let (bodyqueslot, oldest_corpse) = {
        let game = GAME.borrow();
        let slot = (game.bodyqueslot % BODYQUESIZE as i32) as usize;
        (game.bodyqueslot, game.bodyque[slot])
    };
    if bodyqueslot >= BODYQUESIZE as i32 {
        P_RemoveMobj(oldest_corpse);
    }
    {
        let mut game = GAME.borrow_mut();
        let slot = (bodyqueslot % BODYQUESIZE as i32) as usize;
        game.bodyque[slot] = (*player(playernum as usize)).mo;
        game.bodyqueslot += 1;
    }

    // Spawn teleport fog
    let ss = R_PointInSubsector(x, y);

    // Replicate vanilla signed-angle overflow (from PrBoom+)
    let an_raw = (0x10000000i32).wrapping_mul((*mthing).angle.get() as i32 / 45);
    let xa: fixed_t;
    let ya: fixed_t;
    match an_raw {
        4096 => {
            xa = finetangent[2048];
            ya = finetangent[0];
        }
        5120 => {
            xa = finetangent[3072];
            ya = finetangent[1024];
        }
        6144 => {
            xa = finesine[0];
            ya = finetangent[2048];
        }
        7168 => {
            xa = finesine[1024];
            ya = finetangent[3072];
        }
        0 | 1024 | 2048 | 3072 => {
            xa = unsafe { *finecosine.0.add(an_raw as usize) };
            ya = finesine[an_raw as usize];
        }
        _ => {
            crate::i_error!("G_CheckSpot: unexpected angle {}\n", an_raw);
        }
    }

    let floorheight = (*(*ss).sector).floorheight;
    let mo = P_SpawnMobj(x + 20 * xa, y + 20 * ya, floorheight, MT_TFOG);

    if (*console_player()).viewz != 1 {
        S_StartSound(mo, Sfx::Telept as i32);
    }
    1
}

// ---------------------------------------------------------------------------
// G_DeathMatchSpawnPlayer
// ---------------------------------------------------------------------------

/// Spawn `playernum` at a random deathmatch start; falls back to the
/// player's normal start spot after 20 failed attempts.
///
/// Requires at least 4 deathmatch starts on the map (vanilla limit); raises
/// `I_Error` otherwise. The chosen start has its `type` temporarily
/// rewritten to `playernum + 1` so `P_SpawnPlayer` treats it as the player's
/// own spawn.
///
/// # Safety
/// `playernum` must be in `0..MAXPLAYERS` and the level's deathmatch start
/// table must already be populated by `P_SetupLevel`.
pub unsafe fn G_DeathMatchSpawnPlayer(playernum: i32) {
    let selections = deathmatch_p().offset_from(deathmatchstart(0)) as i32;
    if selections < 4 {
        crate::i_error!("Only {} deathmatch spots, 4 required", selections);
    }

    for _ in 0..20 {
        let i = (P_Random() % selections) as usize;
        if G_CheckSpot(playernum, deathmatchstart(i)) != 0 {
            (*deathmatchstart(i)).kind.set((playernum + 1) as i16);
            P_SpawnPlayer(deathmatchstart(i));
            return;
        }
    }
    P_SpawnPlayer(playerstart(playernum as usize));
}

// ---------------------------------------------------------------------------
// G_DoReborn
// ---------------------------------------------------------------------------

/// Respawn `playernum` either by reloading the level (single-player) or
/// by selecting a fresh spawn point (netgame).
///
/// In netgames the player's existing corpse is detached (`mo->player =
/// NULL`), then deathmatch routes through `G_DeathMatchSpawnPlayer` and
/// co-op tries the player's own start first before falling through to other
/// players' starts (temporarily faking the `type` field so `P_SpawnPlayer`
/// accepts them).
///
/// # Safety
/// `playernum` must be a valid player index.
pub unsafe fn G_DoReborn(playernum: i32) {
    if netgame() == 0 {
        GAME.borrow_mut().gameaction = ga_loadlevel;
    } else {
        let mo = (*player(playernum as usize)).mo;
        if !mo.is_null() {
            (*mo).player = ptr::null_mut();
        }

        if deathmatch() != 0 {
            G_DeathMatchSpawnPlayer(playernum);
            return;
        }

        if G_CheckSpot(playernum, playerstart(playernum as usize)) != 0 {
            P_SpawnPlayer(playerstart(playernum as usize));
            return;
        }

        for i in 0..MAXPLAYERS as i32 {
            let start = playerstart(i as usize);
            if G_CheckSpot(playernum, start) != 0 {
                (*start).kind.set((playernum + 1) as i16);
                P_SpawnPlayer(start);
                (*start).kind.set((i + 1) as i16);
                return;
            }
        }
        P_SpawnPlayer(playerstart(playernum as usize));
    }
}

// ---------------------------------------------------------------------------
// G_ScreenShot
// ---------------------------------------------------------------------------

/// Defer a screenshot to the next `G_Ticker` pass via `gameaction =
/// ga_screenshot`.
///
/// # Safety
/// Writes the `gameaction` global.
pub unsafe fn G_ScreenShot() {
    GAME.borrow_mut().gameaction = ga_screenshot;
}

// ---------------------------------------------------------------------------
// G_ExitLevel / G_SecretExitLevel
// ---------------------------------------------------------------------------

/// Request a normal end-of-level transition; clears `secretexit` so the
/// next intermission picks the standard "next map" target.
///
/// # Safety
/// Writes the `secretexit` and `gameaction` globals.
pub unsafe fn G_ExitLevel() {
    let mut game = GAME.borrow_mut();
    game.secretexit = 0;
    game.gameaction = ga_completed;
}

/// Request a secret-exit end-of-level transition.
///
/// On Doom II the secret exit only applies when MAP31 is actually present
/// in the loaded WAD ("if no Wolf3D levels, no secret exit" - the German
/// edition retail patch removed those maps).
///
/// # Safety
/// Writes the `secretexit` and `gameaction` globals.
pub unsafe fn G_SecretExitLevel() {
    let secretexit = if gamemode() == commercial && W_CheckNumForName(b"map31") < 0 {
        0
    } else {
        1
    };
    let mut game = GAME.borrow_mut();
    game.secretexit = secretexit;
    game.gameaction = ga_completed;
}

// ---------------------------------------------------------------------------
// G_DoCompleted
// ---------------------------------------------------------------------------

/// Process the deferred `ga_completed` action: tear down level state, set up
/// the intermission `wbstartstruct_t`, hand off to the WI subsystem and stop
/// the automap if it was active.
///
/// Map-routing rules (mirroring vanilla):
///
/// * Chex ends after MAP05 (instead of MAP08).
/// * Doom 1: MAP08 of any episode triggers `ga_victory`; MAP09 sets
///   `didsecret` on every player.
/// * Doom II: secret-exit on MAP15 -> MAP31, on MAP31 -> MAP32; normal-exit
///   on MAP31 or MAP32 -> MAP16.
/// * Doom 1 episode-4 par-time deliberately reads off the end of `pars[]`
///   into `cpars[]` to reproduce the vanilla overflow bug used by statcheck
///   regression tests.
///
/// # Safety
/// Mutates `wminfo`, `gamestate`, `viewactive`, `automapactive` and the
/// `players` array.
pub unsafe fn G_DoCompleted() {
    GAME.borrow_mut().gameaction = ga_nothing;

    for i in 0..MAXPLAYERS {
        if playeringame(i) != 0 {
            G_PlayerFinishLevel(i as i32);
        }
    }

    if automap_active() {
        AM_Stop();
    }

    let gamemap = gamemap();
    if gamemode() != commercial {
        if gameversion() == exe_chex {
            if gamemap == 5 {
                GAME.borrow_mut().gameaction = ga_victory;
                return;
            }
        } else {
            match gamemap {
                8 => {
                    GAME.borrow_mut().gameaction = ga_victory;
                    return;
                }
                9 => {
                    for i in 0..MAXPLAYERS {
                        (*player(i)).didsecret = 1;
                    }
                }
                _ => {}
            }
        }
    }

    if gamemap == 8 && gamemode() != commercial {
        GAME.borrow_mut().gameaction = ga_victory;
        return;
    }
    if gamemap == 9 && gamemode() != commercial {
        for i in 0..MAXPLAYERS {
            (*player(i)).didsecret = 1;
        }
    }

    {
        let mut game_guard = GAME.borrow_mut();
        let game = &mut *game_guard;
        let gameepisode = game.gameepisode;
        let secretexit = game.secretexit;
        let wminfo = &mut game.wminfo;
        wminfo.didsecret = (*player(game.consoleplayer as usize)).didsecret;
        wminfo.epsd = gameepisode - 1;
        wminfo.last = gamemap - 1;

        if gamemode() == commercial {
            wminfo.next = if secretexit != 0 {
                match gamemap {
                    15 => 30,
                    31 => 31,
                    _ => gamemap,
                }
            } else {
                match gamemap {
                    31 | 32 => 15,
                    _ => gamemap,
                }
            };
        } else {
            wminfo.next = if secretexit != 0 {
                8
            } else if gamemap == 9 {
                match gameepisode {
                    1 => 3,
                    2 => 5,
                    3 => 6,
                    4 => 2,
                    _ => gamemap,
                }
            } else {
                gamemap
            };
        }

        wminfo.maxkills = game.totalkills;
        wminfo.maxitems = game.totalitems;
        wminfo.maxsecret = game.totalsecret;
        wminfo.maxfrags = 0;

        wminfo.partime = if gamemode() == commercial {
            35 * cpars[(gamemap - 1) as usize]
        } else if gameepisode < 4 {
            35 * pars[gameepisode as usize][gamemap as usize]
        } else {
            35 * cpars[gamemap as usize]
        };

        wminfo.pnum = game.consoleplayer;

        let leveltime = leveltime();
        for i in 0..MAXPLAYERS {
            let slot = &*player(i);
            wminfo.plyr[i].in_ = game.playeringame[i];
            wminfo.plyr[i].skills = slot.killcount;
            wminfo.plyr[i].sitems = slot.itemcount;
            wminfo.plyr[i].ssecret = slot.secretcount;
            wminfo.plyr[i].stime = leveltime;
            wminfo.plyr[i].frags = slot.frags;
        }

        game.gamestate = GS_INTERMISSION;
        game.viewactive = 0;
    }
    AUTOMAP.borrow_mut().automapactive = 0;

    StatCopy(wminfo_ptr());
    WI_Start(wminfo_ptr());
}

// ---------------------------------------------------------------------------
// G_WorldDone / G_DoWorldDone
// ---------------------------------------------------------------------------

/// Called by WI when the intermission screen finishes: schedule a
/// `ga_worlddone` action and, on Doom II, kick off the per-cluster finale at
/// the appropriate "end of segment" maps (6, 11, 20, 30 - and 15/31 only via
/// the secret exit).
///
/// # Safety
/// Writes `gameaction`, `players[consoleplayer].didsecret`.
pub unsafe fn G_WorldDone() {
    let secretexit = {
        let mut game = GAME.borrow_mut();
        game.gameaction = ga_worlddone;
        game.secretexit
    };

    if secretexit != 0 {
        (*console_player()).didsecret = 1;
    }

    if gamemode() == commercial {
        match gamemap() {
            15 | 31 => {
                if secretexit == 0 {
                    return;
                }
                F_StartFinale();
            }
            6 | 11 | 20 | 30 => {
                F_StartFinale();
            }
            _ => {}
        }
    }
}

/// Process the deferred `ga_worlddone`: enter `GS_LEVEL`, advance `gamemap`
/// to `wminfo.next + 1`, load the new level and re-enable the 3D view.
///
/// # Safety
/// Mutates `gamestate`, `gamemap`, `viewactive`, `gameaction`.
pub unsafe fn G_DoWorldDone() {
    {
        let mut game = GAME.borrow_mut();
        game.gamestate = GS_LEVEL;
        game.gamemap = game.wminfo.next + 1;
    }
    G_DoLoadLevel();
    let mut game = GAME.borrow_mut();
    game.gameaction = ga_nothing;
    game.viewactive = 1;
}

// ---------------------------------------------------------------------------
// G_LoadGame / G_DoLoadGame
// ---------------------------------------------------------------------------

/// Defer a savegame load: remember `name` in `savename` and queue
/// `ga_loadgame` for the next `G_Ticker`.
pub fn G_LoadGame(name: &str) {
    let mut game = GAME.borrow_mut();
    game.savename = name.to_owned();
    game.gameaction = ga_loadgame;
}

/// Execute the deferred `ga_loadgame` action: open `savename`, validate the
/// header, set up the level via `G_InitNew`, then unarchive players, world
/// geometry, thinkers and specials from `p_saveg`.
///
/// On a missing or corrupt file the function returns silently after the
/// `fopen`; on a bad EOF marker it raises `I_Error("Bad savegame")`.
/// `leveltime` is preserved across the `G_InitNew` call so the unarchived
/// state continues from the saved tic.
///
/// # Safety
/// Mutates a large amount of game state and performs raw file I/O.
pub unsafe fn G_DoLoadGame() {
    let savename = {
        let mut game = GAME.borrow_mut();
        game.gameaction = ga_nothing;
        game.savename.clone()
    };

    let Ok(contents) = std::fs::read(&savename) else {
        return;
    };
    {
        let mut saveg = SAVEG.borrow_mut();
        saveg.save_stream = Cursor::new(contents);
        saveg.savegame_error = 0;
    }

    if P_ReadSaveGameHeader() == 0 {
        SAVEG.borrow_mut().save_stream = Cursor::default();
        return;
    }

    let savedleveltime = leveltime();

    G_InitNew(gameskill(), gameepisode(), gamemap());

    TICK.borrow_mut().leveltime = savedleveltime;

    P_UnArchivePlayers();
    P_UnArchiveWorld();
    P_UnArchiveThinkers();
    P_UnArchiveSpecials();

    if P_ReadSaveGameEOF() == 0 {
        I_Error("Bad savegame");
    }

    SAVEG.borrow_mut().save_stream = Cursor::default();

    if VIEW.borrow().setsizeneeded.is_truthy() {
        R_ExecuteSetViewSize();
    }

    R_FillBackScreen();
}

// ---------------------------------------------------------------------------
// G_SaveGame / G_DoSaveGame
// ---------------------------------------------------------------------------

/// Defer a savegame write: latch the slot index and 24-byte description,
/// then set `sendsave` so the next `G_BuildTiccmd` emits a
/// `BT_SPECIAL | BTS_SAVEGAME` ticcmd. `G_Ticker` decodes that and triggers
/// `ga_savegame` -> `G_DoSaveGame`.
/// `description` is read up to its first NUL, so a fixed text buffer can
/// be passed whole.
///
/// # Safety
/// Mutates the pending save slot, description and `sendsave` latch.
pub unsafe fn G_SaveGame(slot: i32, description: &[u8]) {
    let mut input = GAME_INPUT.borrow_mut();
    input.savegameslot = slot;
    M_StringCopy(&mut input.savedescription, description);
    input.sendsave = 1;
}

/// Execute the deferred `ga_savegame` action.
///
/// Writes to a temporary file first, then renames it over the real savegame
/// path so a crash mid-save cannot destroy an older save. On
/// `fopen`-failure, a recovery file in the temp directory is opened instead;
/// if both fail, `I_Error` aborts the engine.
///
/// When `vanilla_savegame_limit` is set, exceeding `SAVEGAMESIZE` raises
/// `I_Error("Savegame buffer overrun")` to match the DOS limit; otherwise
/// the save is allowed to grow.
///
/// The save is archived into the in-memory `save_stream`, then written to
/// the file opened at the start, which the C code wrote to directly.
///
/// # Safety
/// Writes through the save stream held in [`SAVEG`].
pub unsafe fn G_DoSaveGame() {
    let mut recovery_savegame_file: Option<String> = None;
    let temp_savegame_file = P_TempSaveGameFile();
    let (savegameslot, savedescription) = {
        let input = GAME_INPUT.borrow();
        (input.savegameslot, input.savedescription)
    };
    let savegame_file = P_SaveGameFile(savegameslot);

    let mut save_file = match std::fs::File::create(&temp_savegame_file) {
        Ok(file) => file,
        Err(_) => {
            let recovery = M_TempFile("recovery.dsg");
            let Ok(file) = std::fs::File::create(&recovery) else {
                crate::i_error!(
                    "Failed to open either '{}' or '{}' to write savegame.",
                    temp_savegame_file,
                    recovery,
                );
            };
            recovery_savegame_file = Some(recovery);
            file
        }
    };

    {
        let mut saveg = SAVEG.borrow_mut();
        saveg.save_stream = Cursor::default();
        saveg.savegame_error = 0;
    }

    P_WriteSaveGameHeader(&savedescription);
    P_ArchivePlayers();
    P_ArchiveWorld();
    P_ArchiveThinkers();
    P_ArchiveSpecials();
    P_WriteSaveGameEOF();

    let save_length = SAVEG.borrow().save_stream.position();
    if GAME.borrow().vanilla_savegame_limit != 0 && save_length > SAVEGAMESIZE as u64 {
        I_Error("Savegame buffer overrun");
    }

    // A failed write would have failed the C `fwrite`, which sets the flag.
    let saved = std::mem::take(&mut SAVEG.borrow_mut().save_stream);
    if save_file.write_all(saved.get_ref()).is_err() {
        SAVEG.borrow_mut().savegame_error = 1;
    }
    drop(save_file);

    if let Some(recovery) = recovery_savegame_file {
        crate::i_error!(
            "Failed to open savegame file '{}' for writing.\nBut your game has been saved to '{}' for recovery.",
            temp_savegame_file,
            recovery,
        );
    }

    // Errors are ignored, as the C `remove`/`rename` results were.
    let _ = std::fs::remove_file(&savegame_file);
    let _ = std::fs::rename(&temp_savegame_file, &savegame_file);

    GAME.borrow_mut().gameaction = ga_nothing;
    M_StringCopy(&mut GAME_INPUT.borrow_mut().savedescription, b"");

    (*console_player()).message = PlayerMessage::new(DEH_String("game saved."));

    R_FillBackScreen();
}

// ---------------------------------------------------------------------------
// G_DeferedInitNew / G_DoNewGame / G_InitNew
// ---------------------------------------------------------------------------

/// Defer a new-game start: latch `(skill, episode, map)` into `d_*` and
/// queue `ga_newgame`. `G_Ticker` will then call `G_DoNewGame` -> `G_InitNew`.
///
/// # Safety
/// Writes the `d_skill`, `d_episode`, `d_map` and `gameaction` globals.
pub unsafe fn G_DeferedInitNew(skill: skill_t, episode: i32, map: i32) {
    let mut game = GAME.borrow_mut();
    game.d_skill = skill;
    game.d_episode = episode;
    game.d_map = map;
    game.gameaction = ga_newgame;
}

/// Process `ga_newgame`: clear network / demo state, reset extra players
/// out of the game, then call `G_InitNew` with the deferred `(skill, episode,
/// map)`.
///
/// # Safety
/// Mutates many engine globals.
pub unsafe fn G_DoNewGame() {
    let (d_skill, d_episode, d_map) = {
        let mut game = GAME.borrow_mut();
        game.demoplayback = 0;
        game.netdemo = 0;
        game.netgame = 0;
        game.deathmatch = 0;
        game.playeringame[1] = 0;
        game.playeringame[2] = 0;
        game.playeringame[3] = 0;
        (game.d_skill, game.d_episode, game.d_map)
    };
    clear_monster_parms();
    GAME.borrow_mut().consoleplayer = 0;
    G_InitNew(d_skill, d_episode, d_map);
    GAME.borrow_mut().gameaction = ga_nothing;
}

/// Initialise a new game with the given skill, episode and map.
///
/// Clamps `skill` to `sk_nightmare`, normalises `episode`/`map` for the
/// active `gamemode` (shareware caps at episode 1; pre-Ultimate caps at 3),
/// reseeds the random number generator, flips fast-monster bookkeeping for
/// nightmare or `-fast`, marks every player `PST_REBORN`, then selects the
/// sky texture.
///
/// The sky-texture selection preserves the vanilla "sky never changes in
/// Doom II" behaviour: the sky is bound here at game start rather than per
/// level, which causes Doom II's sky to be wrong on later maps unless a
/// savegame is loaded. This is intentional for demo compatibility.
///
/// Finally dispatches into `G_DoLoadLevel` to actually load the chosen map.
///
/// # Safety
/// Mutates virtually every game-loop global.
pub unsafe fn G_InitNew(skill: skill_t, episode: i32, map: i32) {
    let mut skill = skill;
    let mut episode = episode;
    let mut map = map;

    if paused() != 0 {
        GAME.borrow_mut().paused = 0;
        S_ResumeSound();
    }

    if skill > 4 {
        // sk_nightmare = 4
        skill = 4;
    }

    if gameversion() >= exe_ultimate {
        if episode == 0 {
            episode = 4;
        }
    } else {
        episode = episode.clamp(1, 3);
    }

    if episode > 1 && gamemode() == shareware {
        episode = 1;
    }

    if map < 1 {
        map = 1;
    }
    if map > 9 && gamemode() != commercial {
        map = 9;
    }

    M_ClearRandom();

    // sk_nightmare = 4
    let respawnmonsters = (skill == 4 || respawnparm() != 0) as boolean;
    GAME.borrow_mut().respawnmonsters = respawnmonsters;

    // Fast monsters for nightmare or fastparm
    let gameskill = gameskill();
    if fastparm() != 0 || (skill == 4 && gameskill != 4) {
        set_fast_monsters(true);
    } else if skill != 4 && gameskill == 4 {
        set_fast_monsters(false);
    }

    // Force players to be reborn on first level load
    for i in 0..MAXPLAYERS {
        (*player(i)).playerstate = PST_REBORN;
    }

    {
        let mut game = GAME.borrow_mut();
        game.usergame = 1;
        game.paused = 0;
        game.demoplayback = 0;
    }
    AUTOMAP.borrow_mut().automapactive = 0;
    {
        let mut game = GAME.borrow_mut();
        game.viewactive = 1;
        game.gameepisode = episode;
        game.gamemap = map;
        game.gameskill = skill;
    }
    let (gameepisode, gamemap) = (episode, map);

    // Set sky texture (vanilla Doom broken behaviour for Doom II: set once at
    // game start, not per level — deliberately preserved for compatibility).
    let skytexturename: &[u8] = if gamemode() == commercial {
        if gamemap < 12 {
            b"SKY1"
        } else if gamemap < 21 {
            b"SKY2"
        } else {
            b"SKY3"
        }
    } else {
        match gameepisode {
            1 => b"SKY1",
            2 => b"SKY2",
            3 => b"SKY3",
            4 => b"SKY4",
            _ => b"SKY1",
        }
    };

    SKY.borrow_mut().skytexture = R_TextureNumForName(DEH_String(skytexturename));

    G_DoLoadLevel();
}

// ---------------------------------------------------------------------------
// Fast-monster helper for G_InitNew
// ---------------------------------------------------------------------------

/// Convenience wrapper around [`G_SetFastMonsters`] that takes a `bool`.
///
/// Exists to keep the `G_InitNew` call sites readable; the state-tic and
/// `mobjinfo.speed` patches themselves live in `G_SetFastMonsters` so the
/// arithmetic appears once.
///
/// # Safety
/// Calls into `G_SetFastMonsters`, which mutates the global `states` and
/// `mobjinfo` tables.
unsafe fn set_fast_monsters(fast: bool) {
    // S_SARG_RUN1 .. S_SARG_PAIN2 state range (states 134..160 in vanilla Doom)
    // mobjinfo adjustments for MT_BRUISERSHOT, MT_HEADSHOT, MT_TROOPSHOT
    // These are byte-offset operations into the info tables.
    // Delegate to a safe extern to avoid duplicating the state-offset arithmetic.
    if fast {
        G_SetFastMonsters(1);
    } else {
        G_SetFastMonsters(0);
    }
}

// ---------------------------------------------------------------------------
// G_SetFastMonsters — adjusts state tics and monster shot speeds
// ---------------------------------------------------------------------------

/// Toggle the "fast monsters" rule used by nightmare skill and `-fast`.
///
/// When `fast` is non-zero, halves the per-tic duration of the demon "run"
/// and "pain" states (`S_SARG_RUN1`..`S_SARG_PAIN2`) and raises Baron of Hell,
/// Cacodemon and Imp projectile speeds to 20 `FRACUNIT` per tic; when zero,
/// restores the original durations and the slower projectile speeds
/// (15/10/10).
///
/// This change is global to the `states` and `mobjinfo` tables and persists
/// across maps until inverted again - it is `G_InitNew`'s responsibility to
/// only call this when the skill flips into or out of nightmare.
///
/// # Safety
/// Mutates the shared `states` and `mobjinfo` arrays.
pub unsafe fn G_SetFastMonsters(fast: i32) {
    use crate::engine::play::info::{
        MOBJINFO, MT_BRUISERSHOT, MT_HEADSHOT, MT_TROOPSHOT, STATES, S_SARG_PAIN2, S_SARG_RUN1,
    };

    {
        let mut states = STATES.borrow_mut();
        for i in S_SARG_RUN1 as usize..=S_SARG_PAIN2 as usize {
            if fast != 0 {
                states[i].tics >>= 1;
            } else {
                states[i].tics <<= 1;
            }
        }
    }

    let (bruiser_spd, head_spd, troop_spd): (fixed_t, fixed_t, fixed_t) = if fast != 0 {
        (20 * 65536, 20 * 65536, 20 * 65536)
    } else {
        (15 * 65536, 10 * 65536, 10 * 65536)
    };

    let mut mobjinfo = MOBJINFO.borrow_mut();
    mobjinfo[MT_BRUISERSHOT as usize].speed = bruiser_spd;
    mobjinfo[MT_HEADSHOT as usize].speed = head_spd;
    mobjinfo[MT_TROOPSHOT as usize].speed = troop_spd;
}

// ---------------------------------------------------------------------------
// G_ReadDemoTiccmd  ← CRITICAL for demo accuracy
// ---------------------------------------------------------------------------

/// Read one tic command from the demo buffer into `cmd`.
///
/// * Non-longtics: 4 bytes (`forwardmove`, `sidemove`, `angleturn-hi`,
///   `buttons`).
/// * Longtics: 5 bytes (`forwardmove`, `sidemove`, `angleturn-lo`,
///   `angleturn-hi`, `buttons`).
///
/// The `angleturn` encoding matches vanilla exactly:
///
/// * Non-longtics: the byte is read as **unsigned**, then shifted left 8 -
///   fits `[-32768, 32512]`.
/// * Longtics: two bytes little-endian, with each byte read unsigned (no
///   sign extension on the individual bytes).
///
/// If the next byte is `DEMOMARKER` (0x80), the read instead calls
/// `G_CheckDemoStatus` to wrap up the demo and returns without touching
/// `cmd`.
///
/// # Safety
/// Reads through `demo_p` and writes through `cmd`. Both must be valid.
pub unsafe fn G_ReadDemoTiccmd(cmd: *mut TiccmdT) {
    let (mut demo_p, longtics) = {
        let game = GAME.borrow();
        (game.demo_p, game.longtics)
    };
    if *demo_p == DEMOMARKER {
        G_CheckDemoStatus();
        return;
    }

    read_demo_ticcmd_inner(&mut demo_p, &mut *cmd, longtics != 0);
    GAME.borrow_mut().demo_p = demo_p;
}

// ---------------------------------------------------------------------------
// IncreaseDemoBuffer (static)
// ---------------------------------------------------------------------------

/// Double the size of the demo buffer (used only when `vanilla_demo_limit`
/// is off, to allow recording arbitrarily long demos).
///
/// Allocates a new `Z_Malloc` block of twice the current size, copies the
/// existing demo data over, frees the old buffer and rebases `demobuffer`,
/// `demo_p` and `demoend` onto the new allocation.
///
/// # Safety
/// All three demo pointer globals must be in a consistent state pointing
/// into the same allocation before the call.
unsafe fn increase_demo_buffer() {
    let (demobuffer, demo_p, demoend) = {
        let game = GAME.borrow();
        (game.demobuffer, game.demo_p, game.demoend)
    };
    let current_length = demoend.offset_from(demobuffer) as i32;
    let new_length = current_length * 2;
    let new_demobuffer = Z_Malloc(new_length, PU_STATIC, ptr::null_mut()) as *mut byte;
    let new_demop = new_demobuffer.add(demo_p.offset_from(demobuffer) as usize);
    std::ptr::copy_nonoverlapping(demobuffer, new_demobuffer, current_length as usize);
    Z_Free(demobuffer as *mut c_void);
    let mut game = GAME.borrow_mut();
    game.demobuffer = new_demobuffer;
    game.demo_p = new_demop;
    game.demoend = new_demobuffer.add(new_length as usize);
}

// ---------------------------------------------------------------------------
// G_WriteDemoTiccmd
// ---------------------------------------------------------------------------

/// Append `cmd` to the demo buffer using the same byte layout as
/// [`G_ReadDemoTiccmd`] (4 bytes vanilla, 5 bytes longtics).
///
/// Pressing the `key_demo_quit` ends recording immediately via
/// `G_CheckDemoStatus`. After writing, the demo cursor is rewound and the
/// just-written record is read back through `G_ReadDemoTiccmd` so the
/// recorded value is exactly what playback will see (round-trip
/// consistency).
///
/// If the cursor approaches `demoend - 16`, either `G_CheckDemoStatus` ends
/// recording (vanilla limit on) or `increase_demo_buffer` grows the buffer
/// (vanilla limit off).
///
/// # Safety
/// Writes through `demo_p` and reads back through it; requires the demo
/// buffer to have at least 16 bytes of headroom or `vanilla_demo_limit == 0`.
pub unsafe fn G_WriteDemoTiccmd(cmd: *mut TiccmdT) {
    let key_demo_quit = CONTROLS.get().key_demo_quit;
    if GAME_INPUT.borrow().gamekeydown[key_demo_quit as usize] != 0 {
        G_CheckDemoStatus();
    }

    let command = &*cmd;
    let (demo_start, longtics) = {
        let game = GAME.borrow();
        (game.demo_p, game.longtics)
    };
    let mut demo_p = demo_start;

    *demo_p = command.forwardmove as byte;
    demo_p = demo_p.add(1);
    *demo_p = command.sidemove as byte;
    demo_p = demo_p.add(1);

    if longtics != 0 {
        *demo_p = (command.angleturn & 0xff) as byte;
        demo_p = demo_p.add(1);
        *demo_p = ((command.angleturn >> 8) & 0xff) as byte;
        demo_p = demo_p.add(1);
    } else {
        *demo_p = (command.angleturn >> 8) as byte;
        demo_p = demo_p.add(1);
    }

    *demo_p = command.buttons;

    // Reset demo pointer — write then re-read to validate consistency
    let (demoend, vanilla_demo_limit) = {
        let mut game = GAME.borrow_mut();
        game.demo_p = demo_start;
        (game.demoend, game.vanilla_demo_limit)
    };

    if demo_start > demoend.sub(16) {
        if vanilla_demo_limit != 0 {
            G_CheckDemoStatus();
            return;
        } else {
            increase_demo_buffer();
        }
    }

    G_ReadDemoTiccmd(cmd);
}

// ---------------------------------------------------------------------------
// G_RecordDemo / G_VanillaVersionCode / G_BeginRecording
// ---------------------------------------------------------------------------

/// Start recording a demo to `name.lmp`.
///
/// Allocates the demo buffer (default 128 KiB, overridable via the
/// `-maxdemo <kib>` command-line argument), constructs the output filename
/// by appending `.lmp`, and sets `demorecording = 1`. `usergame` is cleared
/// so save/load menus are disabled during recording.
///
/// # Safety
/// Mutates the demo-recording globals.
pub unsafe fn G_RecordDemo(name: &str) {
    {
        let mut game = GAME.borrow_mut();
        game.usergame = 0;
        game.demoname = format!("{name}.lmp");
    }
    let mut maxsize: i32 = 0x20000;
    let i = M_CheckParmWithArgs("-maxdemo", 1);
    if i != 0 {
        maxsize = atoi(argument(i + 1));
        maxsize *= 1024;
    }
    let demobuffer = Z_Malloc(maxsize, PU_STATIC, ptr::null_mut()) as *mut byte;
    let mut game = GAME.borrow_mut();
    game.demobuffer = demobuffer;
    game.demoend = demobuffer.add(maxsize as usize);
    game.demorecording = 1;
}

/// Return the single-byte demo version code corresponding to the active
/// [`gameversion`] (e.g. 109 for v1.9 and every later vanilla variant).
///
/// Raises `I_Error` for v1.2, which never had a demo version code.
///
/// # Safety
/// Reads the `gameversion` global.
pub unsafe fn G_VanillaVersionCode() -> i32 {
    g_vanilla_version_code_for(gameversion())
}

/// Pure helper for [`G_VanillaVersionCode`]: maps a `gameversion` value to
/// its 1-byte demo header code without touching globals, so unit tests can
/// exercise the table directly.
fn g_vanilla_version_code_for(gv: i32) -> i32 {
    match gv {
        v if v == exe_doom_1_2 => I_Error("Doom 1.2 does not have a version code!"),
        v if v == exe_doom_1_666 => 106,
        v if v == exe_doom_1_7 => 107,
        v if v == exe_doom_1_8 => 108,
        _ => 109, // exe_doom_1_9 and all later variants
    }
}

/// Write the demo file header (version byte, skill, episode, map,
/// deathmatch / respawn / fast / nomonsters flags, console player and
/// per-slot playeringame bytes) at the start of the demo buffer.
///
/// Honours the `-longtics` command-line flag: when set, writes the special
/// `DOOM_191_VERSION` marker and disables `lowres_turn`, so each tic
/// stores `angleturn` in 2 bytes instead of 1.
///
/// # Safety
/// Mutates `longtics`, `lowres_turn`, the demo cursor and the demo buffer.
pub unsafe fn G_BeginRecording() {
    let longtics = (M_CheckParm("-longtics") != 0) as boolean;
    let version = if longtics != 0 {
        DOOM_191_VERSION
    } else {
        G_VanillaVersionCode()
    };
    let (respawnparm, fastparm, nomonsters) = (respawnparm(), fastparm(), nomonsters());

    let mut game = GAME.borrow_mut();
    game.longtics = longtics;
    game.lowres_turn = (longtics == 0) as boolean;

    let mut demo_p = game.demobuffer;

    *demo_p = version as byte;
    demo_p = demo_p.add(1);

    *demo_p = game.gameskill as byte;
    demo_p = demo_p.add(1);
    *demo_p = game.gameepisode as byte;
    demo_p = demo_p.add(1);
    *demo_p = game.gamemap as byte;
    demo_p = demo_p.add(1);
    *demo_p = game.deathmatch as byte;
    demo_p = demo_p.add(1);
    *demo_p = respawnparm as byte;
    demo_p = demo_p.add(1);
    *demo_p = fastparm as byte;
    demo_p = demo_p.add(1);
    *demo_p = nomonsters as byte;
    demo_p = demo_p.add(1);
    *demo_p = game.consoleplayer as byte;
    demo_p = demo_p.add(1);

    for i in 0..MAXPLAYERS {
        *demo_p = game.playeringame[i] as byte;
        demo_p = demo_p.add(1);
    }
    game.demo_p = demo_p;
}

// ---------------------------------------------------------------------------
// G_DeferedPlayDemo / DemoVersionDescription / G_DoPlayDemo
// ---------------------------------------------------------------------------

/// Defer demo playback for `name`: latch `defdemoname` and queue
/// `ga_playdemo` for the next `G_Ticker` pass.
///
/// # Safety
/// Mutates the `defdemoname` and `gameaction` globals.
pub unsafe fn G_DeferedPlayDemo(name: &str) {
    let mut game = GAME.borrow_mut();
    game.defdemoname = name.to_owned();
    game.gameaction = ga_playdemo;
}

/// Map a demo version byte to a human-readable engine identifier
/// (`"v1.9"`, `"v1.6/v1.666"`, etc.).
///
/// Unknown values in the range 0..=4 are treated as pre-v1.4 IWAD demos and
/// labelled `"v1.0/v1.1/v1.2"`. Anything else is formatted as
/// `"major.minor (unknown)"`.
fn demo_version_description(version: i32) -> Cow<'static, str> {
    match version {
        104 => "v1.4".into(),
        105 => "v1.5".into(),
        106 => "v1.6/v1.666".into(),
        107 => "v1.7/v1.7a".into(),
        108 => "v1.8".into(),
        109 => "v1.9".into(),
        0..=4 => "v1.0/v1.1/v1.2".into(),
        _ => format!("{}.{} (unknown)", version / 100, version % 100).into(),
    }
}

/// Execute the deferred `ga_playdemo` action: cache the demo lump, parse
/// its header, configure netgame / netdemo / game parameters, then call
/// `G_InitNew` and flip `demoplayback = 1`.
///
/// The version handling matches vanilla exactly:
///
/// * If the version byte matches the current engine's vanilla code, clears
///   `longtics`.
/// * If it equals `DOOM_191_VERSION`, sets `longtics`.
/// * Otherwise prints a warning via `printf` (not `I_Error`) and continues
///   playback - this matches the C source, which deliberately allowed
///   wrong-version demos to attempt playback rather than aborting.
///
/// `precache` is temporarily cleared around `G_InitNew` so map loading
/// during timing demos does not skew the fps measurement.
///
/// # Safety
/// Mutates global engine state extensively.
pub unsafe fn G_DoPlayDemo() {
    let defdemoname = {
        let mut game = GAME.borrow_mut();
        game.gameaction = ga_nothing;
        game.defdemoname.clone()
    };
    let demobuffer = W_CacheLumpName(defdemoname.as_bytes(), PU_STATIC) as *mut byte;
    GAME.borrow_mut().demobuffer = demobuffer;
    let mut demo_p = demobuffer;

    let demoversion = *demo_p as i32;
    demo_p = demo_p.add(1);

    if demoversion == G_VanillaVersionCode() {
        GAME.borrow_mut().longtics = 0;
    } else if demoversion == DOOM_191_VERSION {
        GAME.borrow_mut().longtics = 1;
    } else {
        // C code uses printf (not I_Error) here so demo playback continues
        print!(
            "Demo is from a different game version!\n\
            (read {}, should be {})\n\n\
            *** You may need to upgrade your version of Doom to v1.9. ***\n\
            See: https://www.doomworld.com/classicdoom/info/patches.php\n\
            This appears to be {}.",
            demoversion,
            G_VanillaVersionCode(),
            demo_version_description(demoversion),
        );
    }

    let skill = *demo_p as skill_t;
    demo_p = demo_p.add(1);
    let episode = *demo_p as i32;
    demo_p = demo_p.add(1);
    let map = *demo_p as i32;
    demo_p = demo_p.add(1);
    GAME.borrow_mut().deathmatch = *demo_p as i32;
    demo_p = demo_p.add(1);
    DOOM_MAIN.borrow_mut().respawnparm = *demo_p as i32;
    demo_p = demo_p.add(1);
    DOOM_MAIN.borrow_mut().fastparm = *demo_p as i32;
    demo_p = demo_p.add(1);
    DOOM_MAIN.borrow_mut().nomonsters = *demo_p as i32;
    demo_p = demo_p.add(1);
    let consoleplayer = *demo_p as i32;
    GAME.borrow_mut().consoleplayer = consoleplayer;
    demo_p = demo_p.add(1);
    if consoleplayer < 0 || consoleplayer >= MAXPLAYERS as i32 {
        crate::i_error!(
            "G_DoPlayDemo: consoleplayer {} out of range\n",
            consoleplayer
        );
    }

    {
        let mut game = GAME.borrow_mut();
        for i in 0..MAXPLAYERS {
            game.playeringame[i] = *demo_p as boolean;
            demo_p = demo_p.add(1);
        }
        game.demo_p = demo_p;
    }

    if playeringame(1) != 0 || M_CheckParm("-solo-net") > 0 || M_CheckParm("-netdemo") > 0 {
        let mut game = GAME.borrow_mut();
        game.netgame = 1;
        game.netdemo = 1;
    }

    GAME.borrow_mut().precache = 0;
    G_InitNew(skill, episode, map);
    let starttime = I_GetTime();
    let mut game = GAME.borrow_mut();
    game.precache = 1;
    game.starttime = starttime;

    game.usergame = 0;
    game.demoplayback = 1;
}

// ---------------------------------------------------------------------------
// G_TimeDemo
// ---------------------------------------------------------------------------

/// Start a benchmark playback of demo `name`.
///
/// Honours `-nodraw` to suppress rendering, sets `singletics` so the engine
/// runs every tic immediately (no `I_GetTime`-pacing), then defers playback
/// the usual way through `ga_playdemo`. On demo end, `G_CheckDemoStatus`
/// prints the result via `I_Error("timed ... fps")`.
///
/// # Safety
/// Mutates `nodrawers`, `timingdemo`, `singletics`, `defdemoname`, `gameaction`.
pub unsafe fn G_TimeDemo(name: &str) {
    let nodrawers = M_CheckParm("-nodraw");
    {
        let mut game = GAME.borrow_mut();
        game.nodrawers = nodrawers;
        game.timingdemo = 1;
    }
    crate::engine::doom::main_loop::MAIN_LOOP
        .borrow_mut()
        .singletics = 1;
    let mut game = GAME.borrow_mut();
    game.defdemoname = name.to_owned();
    game.gameaction = ga_playdemo;
}

// ---------------------------------------------------------------------------
// G_CheckDemoStatus
// ---------------------------------------------------------------------------

/// End-of-demo cleanup; called by both reader and writer paths.
///
/// Three mutually-exclusive paths:
///
/// * **Timing demo**: compute fps, clear the timing/playback flags and
///   raise `I_Error("timed ... fps")` (which prints and exits).
/// * **Playback**: release the demo lump, clear demo / netgame / dm flags
///   and either `I_Quit()` (singledemo) or `D_AdvanceDemo()` (loop). Returns
///   `1`.
/// * **Recording**: append `DEMOMARKER`, flush the buffer to `demoname` via
///   `M_WriteFile`, free the buffer and raise `I_Error("Demo %s recorded")`.
///
/// Returns `0` when the call was a no-op (none of the conditions matched).
///
/// # Safety
/// Mutates demo / playback globals and performs file I/O.
pub unsafe fn G_CheckDemoStatus() -> boolean {
    if GAME.borrow().timingdemo != 0 {
        let endtime = I_GetTime();
        let realtics = endtime - GAME.borrow().starttime;
        let gametic = gametic();
        let fps = (gametic as f32 * 35.0) / realtics as f32;
        {
            let mut game = GAME.borrow_mut();
            game.timingdemo = 0;
            game.demoplayback = 0;
        }
        crate::i_error!(
            "timed {} gametics in {} realtics ({:.6} fps)",
            gametic,
            realtics,
            fps as f64,
        );
    }

    if demoplayback() != 0 {
        let defdemoname = GAME.borrow().defdemoname.clone();
        W_ReleaseLumpName(defdemoname.as_bytes());
        let singledemo = {
            let mut game = GAME.borrow_mut();
            game.demoplayback = 0;
            game.netdemo = 0;
            game.netgame = 0;
            game.deathmatch = 0;
            game.playeringame[1] = 0;
            game.playeringame[2] = 0;
            game.playeringame[3] = 0;
            game.singledemo
        };
        clear_monster_parms();
        GAME.borrow_mut().consoleplayer = 0;

        if singledemo != 0 {
            I_Quit();
        } else {
            D_AdvanceDemo();
        }
        return 1;
    }

    if GAME.borrow().demorecording != 0 {
        let (demobuffer, demo_p, demoname) = {
            let mut game = GAME.borrow_mut();
            *game.demo_p = DEMOMARKER;
            game.demo_p = game.demo_p.add(1);
            (game.demobuffer, game.demo_p, game.demoname.clone())
        };
        M_WriteFile(
            &demoname,
            std::slice::from_raw_parts(demobuffer, demo_p.offset_from(demobuffer) as usize),
        );
        Z_Free(demobuffer as *mut c_void);
        GAME.borrow_mut().demorecording = 0;
        crate::i_error!("Demo {} recorded", demoname);
    }

    0
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

/// Unit-test suite covering ticcmd checksumming, demo I/O byte layout and
/// version-code lookup. The demo tests exercise `read_demo_ticcmd_inner`
/// directly so they avoid mutating the game's `demo_p` cursor and the
/// `DEMOMARKER` end-of-stream shortcut.
#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::engine::doom::{mode::exe_doom_1_9, player::TiccmdT},
    };

    /// Helper returning a freshly zeroed [`TiccmdT`] for test assembly.
    fn zeroed_cmd() -> TiccmdT {
        unsafe { std::mem::zeroed() }
    }

    // --- G_CmdChecksum ---

    /// A fully-zero ticcmd checksums to zero.
    #[test]
    fn cmdchecksum_all_zeros_is_zero() {
        let cmd = zeroed_cmd();
        assert_eq!(G_CmdChecksum(&cmd), 0);
    }

    /// `forwardmove = 1` produces a single bit in the first 4-byte word.
    #[test]
    fn cmdchecksum_forwardmove_one() {
        // First C word = bytes [forwardmove, sidemove, angleturn_lo, angleturn_hi].
        // With forwardmove=1, rest zero: first word = 0x00_00_00_01 = 1 (little-endian).
        let mut cmd = zeroed_cmd();
        cmd.forwardmove = 1;
        assert_eq!(G_CmdChecksum(&cmd), 1);
    }

    /// The tail-word `lookfly` / `arti` fields are excluded from the sum.
    #[test]
    fn cmdchecksum_excludes_last_int() {
        // G_CmdChecksum sums sizeof(ticcmd_t)/4 - 1 = 3 words (bytes 0-11).
        // The last word (bytes 12-15: lookfly + arti + padding) is NOT summed.
        let mut cmd = zeroed_cmd();
        cmd.lookfly = 0x12;
        cmd.arti = 0x34;
        assert_eq!(G_CmdChecksum(&cmd), 0);
    }

    /// `inventory` sits in word 2 and is included in the checksum.
    #[test]
    fn cmdchecksum_includes_inventory() {
        // inventory is at bytes 8-11 (word 2), which IS included in the sum.
        let mut cmd = zeroed_cmd();
        cmd.inventory = 1;
        assert_eq!(G_CmdChecksum(&cmd), 1);
    }

    /// The field-wise sum equals the C sum of raw `int` words over the
    /// 16-byte `ticcmd_t` layout, including sign bits and wrapping.
    #[test]
    fn cmdchecksum_matches_c_raw_word_sum() {
        let cmd = TiccmdT {
            forwardmove: -50,
            sidemove: 40,
            angleturn: -1280,
            chatchar: b'x',
            buttons: 0x83,
            consistancy: 0xfe,
            buttons2: 0x91,
            inventory: i32::MAX,
            lookfly: 7,
            arti: 9,
        };
        let [angleturn_low, angleturn_high] = cmd.angleturn.to_le_bytes();
        let c_layout: [u8; 16] = {
            let mut bytes = [0u8; 16];
            bytes[..8].copy_from_slice(&[
                cmd.forwardmove as u8,
                cmd.sidemove as u8,
                angleturn_low,
                angleturn_high,
                cmd.chatchar,
                cmd.buttons,
                cmd.consistancy,
                cmd.buttons2,
            ]);
            bytes[8..12].copy_from_slice(&cmd.inventory.to_le_bytes());
            bytes[12] = cmd.lookfly;
            bytes[13] = cmd.arti;
            bytes
        };
        let (words, _) = c_layout.as_chunks::<4>();
        let raw_word_sum = words
            .iter()
            .take(3)
            .map(|word| i32::from_le_bytes(*word))
            .fold(0i32, i32::wrapping_add);
        assert_eq!(G_CmdChecksum(&cmd), raw_word_sum);
    }

    // --- G_ReadDemoTiccmd (non-longtics) ---

    /// Test that G_ReadDemoTiccmd reads the non-longtics format correctly.
    /// Byte layout: [forwardmove, sidemove, angleturn_byte, buttons]
    #[test]
    fn read_demo_ticcmd_non_longtics_basic() {
        unsafe {
            let buf: [u8; 4] = [0x10, 0x20, 0x30, 0x01];
            let mut p = buf.as_ptr() as *mut u8;
            let mut cmd = zeroed_cmd();
            // Don't call G_ReadDemoTiccmd (it calls G_CheckDemoStatus on DEMOMARKER),
            // instead test the inner read logic directly.
            read_demo_ticcmd_inner(&mut p, &mut cmd, false);
            assert_eq!(cmd.forwardmove, 0x10i8);
            assert_eq!(cmd.sidemove, 0x20i8);
            // angleturn: (0x30 as u8 as u32) << 8 = 0x3000 = 12288 as i16
            assert_eq!(cmd.angleturn, 0x3000u16 as i16);
            assert_eq!(cmd.buttons, 0x01);
        }
    }

    /// Negative forwardmove/sidemove: byte 0xFF → -1 as i8.
    #[test]
    fn read_demo_ticcmd_negative_moves() {
        unsafe {
            let buf: [u8; 4] = [0xFF, 0x80, 0x00, 0x00];
            let mut p = buf.as_ptr() as *mut u8;
            let mut cmd = zeroed_cmd();
            read_demo_ticcmd_inner(&mut p, &mut cmd, false);
            assert_eq!(cmd.forwardmove, -1i8);
            assert_eq!(cmd.sidemove, -128i8);
        }
    }

    /// High angleturn byte 0xFF: must produce (0xFF as u32) << 8 = 0xFF00 = -256 as i16.
    /// This is the key signed-unsigned handling that must match vanilla C.
    #[test]
    fn read_demo_ticcmd_high_angleturn_byte() {
        unsafe {
            let buf: [u8; 4] = [0x00, 0x00, 0xFF, 0x00];
            let mut p = buf.as_ptr() as *mut u8;
            let mut cmd = zeroed_cmd();
            read_demo_ticcmd_inner(&mut p, &mut cmd, false);
            // (0xFF as u32) << 8 = 0xFF00; as i16 = -256
            assert_eq!(cmd.angleturn, -256i16);
        }
    }

    /// Angleturn byte 0x80 → 0x8000 = -32768 as i16 (maximum left turn).
    #[test]
    fn read_demo_ticcmd_angleturn_0x80() {
        unsafe {
            let buf: [u8; 4] = [0x00, 0x00, 0x80, 0x00];
            let mut p = buf.as_ptr() as *mut u8;
            let mut cmd = zeroed_cmd();
            read_demo_ticcmd_inner(&mut p, &mut cmd, false);
            assert_eq!(cmd.angleturn, i16::MIN); // 0x8000 = -32768
        }
    }

    // --- G_ReadDemoTiccmd (longtics) ---

    /// Longtics: two-byte little-endian angleturn, each byte treated as unsigned.
    #[test]
    fn read_demo_ticcmd_longtics_basic() {
        unsafe {
            let buf: [u8; 5] = [0x10, 0x20, 0xAB, 0xCD, 0x01];
            let mut p = buf.as_ptr() as *mut u8;
            let mut cmd = zeroed_cmd();
            read_demo_ticcmd_inner(&mut p, &mut cmd, true);
            assert_eq!(cmd.forwardmove, 0x10i8);
            assert_eq!(cmd.sidemove, 0x20i8);
            // angleturn = 0xAB | (0xCD << 8) = 0xCDAB as i16
            assert_eq!(cmd.angleturn, 0xCDABu16 as i16);
            assert_eq!(cmd.buttons, 0x01);
        }
    }

    /// Longtics with bytes [0x00, 0x80]: angleturn = 0 | (0x80 << 8) = 0x8000 = -32768.
    #[test]
    fn read_demo_ticcmd_longtics_high_hi_byte() {
        unsafe {
            let buf: [u8; 5] = [0x00, 0x00, 0x00, 0x80, 0x00];
            let mut p = buf.as_ptr() as *mut u8;
            let mut cmd = zeroed_cmd();
            read_demo_ticcmd_inner(&mut p, &mut cmd, true);
            assert_eq!(cmd.angleturn, i16::MIN);
        }
    }

    /// Longtics zero angleturn.
    #[test]
    fn read_demo_ticcmd_longtics_zero_angleturn() {
        unsafe {
            let buf: [u8; 5] = [0x00, 0x00, 0x00, 0x00, 0x00];
            let mut p = buf.as_ptr() as *mut u8;
            let mut cmd = zeroed_cmd();
            read_demo_ticcmd_inner(&mut p, &mut cmd, true);
            assert_eq!(cmd.angleturn, 0);
        }
    }

    // --- G_VanillaVersionCode ---

    /// Doom v1.6/v1.666 maps to demo version code 106.
    #[test]
    fn vanilla_version_exe_doom_1_666_is_106() {
        assert_eq!(g_vanilla_version_code_for(exe_doom_1_666), 106);
    }

    /// Doom v1.7/v1.7a maps to demo version code 107.
    #[test]
    fn vanilla_version_exe_doom_1_7_is_107() {
        assert_eq!(g_vanilla_version_code_for(exe_doom_1_7), 107);
    }

    /// Doom v1.8 maps to demo version code 108.
    #[test]
    fn vanilla_version_exe_doom_1_8_is_108() {
        assert_eq!(g_vanilla_version_code_for(exe_doom_1_8), 108);
    }

    /// Doom v1.9 maps to demo version code 109.
    #[test]
    fn vanilla_version_exe_doom_1_9_is_109() {
        assert_eq!(g_vanilla_version_code_for(exe_doom_1_9), 109);
    }

    /// Ultimate Doom and later variants share v1.9's demo code (109).
    #[test]
    fn vanilla_version_ultimate_is_109() {
        // exe_ultimate and all later variants map to 109
        assert_eq!(g_vanilla_version_code_for(exe_ultimate), 109);
    }

    /// Final Doom (`exe_final2`) shares v1.9's demo code (109).
    #[test]
    fn vanilla_version_exe_final2_is_109() {
        assert_eq!(g_vanilla_version_code_for(exe_final2), 109);
    }

    // --- Movement table values (regression baseline) ---

    /// `forwardmove[0]` (slow) baseline of 0x19 - guards against accidental edits.
    #[test]
    fn forwardmove_slow_is_0x19() {
        assert_eq!(GameState::new().forwardmove[0], 0x19);
    }

    /// `forwardmove[1]` (fast) baseline of 0x32 - also the turbo threshold.
    #[test]
    fn forwardmove_fast_is_0x32() {
        assert_eq!(GameState::new().forwardmove[1], 0x32);
    }

    /// `sidemove[0]` (slow) baseline of 0x18.
    #[test]
    fn sidemove_slow_is_0x18() {
        assert_eq!(GameState::new().sidemove[0], 0x18);
    }

    /// `sidemove[1]` (fast) baseline of 0x28.
    #[test]
    fn sidemove_fast_is_0x28() {
        assert_eq!(GameState::new().sidemove[1], 0x28);
    }

    /// `angleturn[]` baseline values (normal / fast / slow).
    #[test]
    fn angleturn_values() {
        let angleturn = GameState::new().angleturn;
        assert_eq!(angleturn[0], 640);
        assert_eq!(angleturn[1], 1280);
        assert_eq!(angleturn[2], 320);
    }

    /// Player slot pointers step through the slot array, so the identities
    /// map objects keep (`mobj_t.player`) stay stable.
    #[test]
    fn player_pointers_index_the_slots() {
        assert_eq!(player(0), PLAYERS.as_ptr().cast::<PlayerT>());
        assert_eq!(unsafe { player(3).offset_from(player(0)) }, 3);
    }

    /// `BT_*` / `BTS_*` button-bit constants match `d_event.h`.
    #[test]
    fn button_constants_match_d_event_h() {
        assert_eq!(BT_ATTACK, 1);
        assert_eq!(BT_USE, 2);
        assert_eq!(BT_CHANGE, 4);
        assert_eq!(BT_SPECIAL, 128);
        assert_eq!(BT_SPECIALMASK, 3);
        assert_eq!(BTS_PAUSE, 1);
        assert_eq!(BTS_SAVEGAME, 2);
        assert_eq!(BTS_SAVEMASK, 28);
        assert_eq!(BTS_SAVESHIFT, 2);
    }
}

// ---------------------------------------------------------------------------
// Inner read helper — used by tests and by G_ReadDemoTiccmd
// ---------------------------------------------------------------------------

/// Pure implementation of the demo ticcmd read, parameterised over the
/// demo-pointer and longtics flag so unit tests can call it without globals.
///
/// # Safety
///
/// `*p` must point into a valid, sufficiently-long demo buffer: at least 4 bytes
/// remaining for non-longtics demos, or 5 bytes for longtics. The pointer is advanced
/// past the bytes consumed. `cmd` must be a valid, writable `TiccmdT`.
#[inline]
unsafe fn read_demo_ticcmd_inner(p: &mut *mut u8, cmd: &mut TiccmdT, is_longtics: bool) {
    cmd.forwardmove = (**p) as i8;
    *p = p.add(1);

    cmd.sidemove = (**p) as i8;
    *p = p.add(1);

    if is_longtics {
        let lo = **p;
        *p = p.add(1);
        let hi = **p;
        *p = p.add(1);
        // C: angleturn = lo; angleturn |= (hi << 8);  — both lo and hi unsigned
        cmd.angleturn = (lo as i16) | ((hi as i16) << 8);
    } else {
        let byte = **p;
        *p = p.add(1);
        // C: cmd->angleturn = ((unsigned char)*demo_p++) << 8
        cmd.angleturn = ((byte as u32) << 8) as i16;
    }

    cmd.buttons = **p;
    *p = p.add(1);
}
