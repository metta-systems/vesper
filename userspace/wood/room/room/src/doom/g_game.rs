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
//! [`rndindex`] cookie tracked by `m_random`; a mismatch raises `I_Error`.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::doom::sounds::Sfx;
use std::ffi::{c_char, c_int, c_uint, c_void};

use std::ptr;

use crate::doom::d_mode::{
    commercial, doom, exe_chex, exe_doom_1_2, exe_doom_1_666, exe_doom_1_7, exe_doom_1_8,
    exe_final2, exe_ultimate, shareware,
};
use crate::doom::d_player::{PlayerT, TiccmdT, MAXPLAYERS};
use crate::doom::doomstat::{gamemission, gamemode, gameversion};
use crate::doom::m_random::P_Random;
use crate::doom::p_inter::maxammo;
use crate::doom::p_setup::{
    deathmatch_p, deathmatchstarts, mapthing_t as SetupMapThing, playerstarts,
};
use crate::doom::p_telept::{mapthing_t, mobj_t};
use crate::doom::tables::{finecosine, finesine, finetangent};
use crate::doom::wi_stuff::{wbplayerstruct_t, wbstartstruct_t};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Vanilla Doom `boolean`, matching the C `int` truthy/falsy convention.
type boolean = c_int;
/// Vanilla Doom `byte` (unsigned 8-bit).
type byte = u8;
/// Vanilla Doom `skill_t` enum stored as `int` (sk_baby=0 .. sk_nightmare=4).
type skill_t = c_int;
/// Vanilla Doom 16.16 fixed-point type (`int`, with 16 fractional bits).
type fixed_t = c_int;
/// Platform `long`, used here for the savegame size limit.
type c_long = libc::c_long;

// ---------------------------------------------------------------------------
// Game-state constants (from doomstat.h)
// ---------------------------------------------------------------------------

/// `GS_LEVEL` - actively playing a map; `P_Ticker` advances world simulation.
const GS_LEVEL: c_int = 0;
/// `GS_INTERMISSION` - between-level statistics screen driven by `WI_Ticker`.
const GS_INTERMISSION: c_int = 1;
/// `GS_FINALE` - end-of-episode text crawl / cast call, driven by `F_Ticker`.
const GS_FINALE: c_int = 2;
/// `GS_DEMOSCREEN` - title/credits/demo cycle, driven by `D_PageTicker`.
const GS_DEMOSCREEN: c_int = 3;

// ---------------------------------------------------------------------------
// Game-action constants (from doomstat.h)
// ---------------------------------------------------------------------------

/// No deferred action pending; `G_Ticker` falls through to normal state.
const ga_nothing: c_int = 0;
/// Defer a level (re)load via `G_DoLoadLevel`.
const ga_loadlevel: c_int = 1;
/// Defer a new game start via `G_DoNewGame`.
const ga_newgame: c_int = 2;
/// Defer a savegame load via `G_DoLoadGame`.
const ga_loadgame: c_int = 3;
/// Defer a savegame write via `G_DoSaveGame`.
const ga_savegame: c_int = 4;
/// Defer demo playback via `G_DoPlayDemo`.
const ga_playdemo: c_int = 5;
/// Defer the end-of-level transition via `G_DoCompleted`.
const ga_completed: c_int = 6;
/// Defer the end-of-game finale via `F_StartFinale`.
const ga_victory: c_int = 7;
/// Defer the intermission-to-next-level transition via `G_DoWorldDone`.
const ga_worlddone: c_int = 8;
/// Defer a screenshot grab via `V_ScreenShot`.
const ga_screenshot: c_int = 9;

// ---------------------------------------------------------------------------
// Player state constants (from d_player.h)
// ---------------------------------------------------------------------------

/// `PST_LIVE` - player is alive and active.
const PST_LIVE: c_int = 0;
/// `PST_DEAD` - player is dead, awaiting respawn input.
const PST_DEAD: c_int = 1;
/// `PST_REBORN` - player should be respawned on the next tick.
const PST_REBORN: c_int = 2;

// ---------------------------------------------------------------------------
// Weapon constants (from doomdef.h)
// ---------------------------------------------------------------------------

/// `wp_fist` weapon index (slot 1).
const wp_fist: c_int = 0;
/// `wp_pistol` weapon index (slot 2).
const wp_pistol: c_int = 1;
/// `wp_chainsaw` weapon index (also slot 1).
const wp_chainsaw: c_int = 7;
/// `wp_supershotgun` weapon index (Doom II only, also slot 3).
const wp_supershotgun: c_int = 8;
/// `wp_plasma` weapon index (slot 6).
const wp_plasma: c_int = 5;
/// `wp_bfg` weapon index (slot 7).
const wp_bfg: c_int = 6;
/// `wp_nochange` sentinel - pending weapon means "keep current".
const wp_nochange: c_int = 9;

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
const SAVEGAMESIZE: c_long = 0x2c000;
/// End-of-stream sentinel byte written at the tail of every demo `.lmp`.
const DEMOMARKER: byte = 0x80;
/// Size of the version-text field in some legacy savegame headers.
const VERSIONSIZE: usize = 16;
/// Ammo-type index for the clip (bullets) ammo class.
const am_clip: usize = 0;
/// `MT_TFOG` mobj type index - teleport fog spawned at player respawn spots.
const MT_TFOG: c_int = 28;

/// Default starting health (100); patched by Dehacked in vanilla.
const DEH_INITIAL_HEALTH: c_int = 100;
/// Default starting bullet count (50); patched by Dehacked in vanilla.
const DEH_INITIAL_BULLETS: c_int = 50;

/// Re-export of [`event_t`] from `d_event.rs` for input handling.
use crate::doom::d_event::event_t;

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
    weapon: c_int,
    /// Slot number (1-8) encoded into the `BT_CHANGE` ticcmd field.
    weapon_num: c_int,
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
// Public globals — #[no_mangle] so other Rust modules can access them via
// `extern "C"` declarations (the same pattern used throughout this codebase).
// ---------------------------------------------------------------------------

/// Value of `gamestate` from the previous `G_Ticker` invocation, used to
/// detect transitions (e.g. dismissing the intermission screen).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut oldgamestate: c_int = GS_DEMOSCREEN;

/// Deferred game action queue (`ga_*`); drained at the top of `G_Ticker`.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut gameaction: c_int = ga_nothing;

/// Active gameplay state machine slot (`GS_LEVEL`/`GS_INTERMISSION`/...).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut gamestate: c_int = GS_DEMOSCREEN;

/// Currently selected skill level (`sk_baby` .. `sk_nightmare`).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut gameskill: c_int = 0;

/// Non-zero when monsters respawn (nightmare skill or `-respawn` parm).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut respawnmonsters: boolean = 0;

/// Currently loaded episode number (1-based; 1-3 for Doom, up to 4 with Ultimate).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut gameepisode: c_int = 0;

/// Currently loaded map number (1-based; 1-9 in Doom, 1-32 in Doom II).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut gamemap: c_int = 0;

/// If non-zero, exit the level after this number of minutes.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut timelimit: c_int = 0;

/// Non-zero while gameplay is paused (sound is paused, ticker suspended).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut paused: boolean = 0;

/// One-tic flag requesting a pause-toggle ticcmd from `G_BuildTiccmd`.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut sendpause: boolean = 0;

/// One-tic flag requesting a savegame ticcmd from `G_BuildTiccmd`.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut sendsave: boolean = 0;

/// Non-zero while a user-controlled game is in progress (vs demo / title).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut usergame: boolean = 0;

/// Set by `-timedemo`; on demo end prints fps stats via `I_Error`.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut timingdemo: boolean = 0;

/// Set by `-nodraw`; disables rendering for benchmarking purposes.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut nodrawers: boolean = 0;

/// `I_GetTime()` value captured when a timed demo started.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut starttime: c_int = 0;

/// Non-zero while the 3D view is being rendered (false during intermission).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut viewactive: boolean = 0;

/// Deathmatch mode (`0`=co-op, `1`=DM, `2`=DM2 / altdeath).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut deathmatch: c_int = 0;

/// Non-zero in a networked game (changes consistency-check behaviour).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut netgame: boolean = 0;

/// Per-slot presence flag for the four player slots (`MAXPLAYERS = 4`).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut playeringame: [boolean; MAXPLAYERS] = [0; MAXPLAYERS];

/// Player state slots (inventory, position, view angle, etc.).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut players: [PlayerT; MAXPLAYERS] = unsafe { std::mem::zeroed() };

/// Per-player "turbo" detection latch consumed by `G_Ticker`.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut turbodetected: [boolean; MAXPLAYERS] = [0; MAXPLAYERS];

/// Index of the local player receiving input events.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut consoleplayer: c_int = 0;

/// Index of the player whose first-person view is being drawn (spy mode).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut displayplayer: c_int = 0;

/// `gametic` value captured when the current level was loaded.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut levelstarttic: c_int = 0;

/// Sum of monster kills across all players for the current level.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut totalkills: c_int = 0;

/// Sum of item pickups across all players for the current level.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut totalitems: c_int = 0;

/// Sum of secret-sector finds across all players for the current level.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut totalsecret: c_int = 0;

/// File name of the demo currently being recorded (Z_Malloc'd).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut demoname: *mut c_char = ptr::null_mut();

/// Non-zero while a `.lmp` demo is being written from input.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut demorecording: boolean = 0;

/// cph's Doom 1.91 longtics hack - encodes angleturn in 2 bytes per tic
/// instead of 1, enabling smooth high-res turning in demos.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut longtics: boolean = 0;

/// Round per-tic angleturn to the nearest 256 BAM when recording vanilla
/// (non-longtics) demos so the 1-byte demo angleturn replays accurately.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut lowres_turn: boolean = 0;

/// Non-zero while a `.lmp` demo is being played back.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut demoplayback: boolean = 0;

/// Non-zero when the active demo was recorded in a networked session.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut netdemo: boolean = 0;

/// Base pointer to the current demo I/O buffer (Z_Malloc'd).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut demobuffer: *mut byte = ptr::null_mut();

/// Read/write cursor within `demobuffer`.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut demo_p: *mut byte = ptr::null_mut();

/// One-past-the-end pointer for `demobuffer`.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut demoend: *mut byte = ptr::null_mut();

/// Non-zero when launched with `-playdemo`; quits after the demo ends.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut singledemo: boolean = 0;

/// Non-zero (default) to precache all level graphics during map load.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut precache: boolean = 1; // true by default

/// Non-zero while invoked from the setup utility's "test controls" mode.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut testcontrols: boolean = 0;

/// Low-pass-filtered mouse speed displayed by the test-controls thermometer.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut testcontrols_mousespeed: c_int = 0;

/// Parameters for the world-map / intermission screen, populated by
/// `G_DoCompleted` before transitioning to `GS_INTERMISSION`.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut wminfo: wbstartstruct_t = wbstartstruct_t {
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
};

/// Net consistency check ring: `consistancy[player][gametic/ticdup % BACKUPTICS]`.
/// A mismatch on a remote command triggers `I_Error("consistency failure ...")`.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut consistancy: [[byte; 128]; MAXPLAYERS] = [[0; 128]; MAXPLAYERS];

/// Index into `bodyque` for the next corpse to add (modulo 32).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut bodyqueslot: c_int = 0;

/// Non-zero enforces the vanilla `SAVEGAMESIZE` cap (`I_Error` on overrun).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut vanilla_savegame_limit: c_int = 1;

/// Non-zero enforces the vanilla demo buffer cap; zero auto-grows it.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut vanilla_demo_limit: c_int = 1;

/// Forward-movement speed table: `[slow=0x19, fast=0x32]` (`fixed_t` per tic).
/// Indexed by the `speed` flag computed in `G_BuildTiccmd`.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut forwardmove: [fixed_t; 2] = [0x19, 0x32];

/// Lateral strafe speed table: `[slow=0x18, fast=0x28]` (`fixed_t` per tic).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut sidemove: [fixed_t; 2] = [0x18, 0x28];

/// Turn-speed table: `[normal=640, fast=1280, slow=320]` (BAM units per tic).
/// The "slow" entry is selected for the first `SLOWTURNTICS` (6) of held input.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut angleturn: [fixed_t; 3] = [640, 1280, 320];

/// Non-zero when the next level transition should route through the secret
/// exit (set by `G_SecretExitLevel`).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut secretexit: boolean = 0;

/// File name of the demo deferred for playback (set by `G_DeferedPlayDemo`).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut defdemoname: *mut c_char = ptr::null_mut();

/// File name buffer for the savegame currently being loaded.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut savename: [c_char; 256] = [0; 256];

/// Deferred `G_DeferedInitNew` parameter: skill level.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut d_skill: skill_t = 0;
/// Deferred `G_DeferedInitNew` parameter: episode.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut d_episode: c_int = 0;
/// Deferred `G_DeferedInitNew` parameter: map number.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut d_map: c_int = 0;

/// Doom episode par times (episodes 1-3, maps 1-9), in seconds.
/// Index `[0]` and `[*][0]` are dummies to keep the table 1-based.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut pars: [[c_int; 10]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 30, 75, 120, 90, 165, 180, 180, 30, 165],
    [0, 90, 90, 90, 120, 90, 360, 240, 30, 170],
    [0, 90, 45, 90, 150, 90, 90, 165, 30, 135],
];

/// Doom II par times (maps 1-32), in seconds. Index `[0]` is map 1.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut cpars: [c_int; 32] = [
    30, 90, 120, 120, 90, 150, 120, 120, 270, 90, 210, 150, 150, 150, 210, 150, 420, 150, 210, 150,
    240, 150, 180, 150, 150, 300, 330, 420, 300, 180, 120, 30,
];

/// Circular queue of `BODYQUESIZE=32` recent player corpses; the oldest is
/// removed when the queue wraps (see `G_CheckSpot`).
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub static mut bodyque: [*mut mobj_t; 32] = [ptr::null_mut(); 32];

// ---------------------------------------------------------------------------
// Module-private statics
// ---------------------------------------------------------------------------

/// Per-key "is held down" flags, indexed by Doom key code.
static mut GAMEKEYDOWN: [boolean; NUMKEYS] = [0; NUMKEYS];
/// Tics that turn input has been held; drives two-stage accelerative turning.
static mut TURNHELD: c_int = 0;

/// Mouse button held-down flags (slot 0 unused to allow `[-1]` indexing).
static mut MOUSEARRAY: [boolean; MAX_MOUSE_BUTTONS + 1] = [0; MAX_MOUSE_BUTTONS + 1];
// mousebuttons is &mousearray[1] in C — negative indexing; access via MOUSEARRAY[1+n]
/// Most recent raw mouse delta on the X axis (cleared each tic by `G_BuildTiccmd`).
static mut MOUSEX: c_int = 0;
/// Most recent raw mouse delta on the Y axis (cleared each tic by `G_BuildTiccmd`).
static mut MOUSEY: c_int = 0;

/// Tics since the last forward-mouse click for double-click-as-use detection.
static mut DCLICKTIME: c_int = 0;
/// Last sampled state of the mousebforward button for double-click detection.
static mut DCLICKSTATE: boolean = 0;
/// Click count toward a forward-mouse double-click (2 triggers `BT_USE`).
static mut DCLICKS: c_int = 0;
/// Tics since the last strafe-button click for double-click detection.
static mut DCLICKTIME2: c_int = 0;
/// Last sampled state of the strafe button for double-click detection.
static mut DCLICKSTATE2: boolean = 0;
/// Click count toward a strafe-button double-click (2 triggers `BT_USE`).
static mut DCLICKS2: c_int = 0;

/// Most recent joystick X (turn / strafe) axis value.
static mut JOYXMOVE: c_int = 0;
/// Most recent joystick Y (forward / back) axis value.
static mut JOYYMOVE: c_int = 0;
/// Most recent joystick strafe axis value.
static mut JOYSTRAFEMOVE: c_int = 0;
/// Joystick button held-down flags (slot 0 unused to allow `[-1]` indexing).
static mut JOYARRAY: [boolean; MAX_JOY_BUTTONS + 1] = [0; MAX_JOY_BUTTONS + 1];
// joybuttons is &joyarray[1] in C — negative indexing; access via JOYARRAY[1+n]

/// Selected savegame slot for the next save (set by `G_SaveGame`).
static mut SAVEGAMESLOT: c_int = 0;
/// Description text for the next savegame (set by `G_SaveGame`).
static mut SAVEDESCRIPTION: [c_char; 32] = [0; 32];

/// Pending weapon-cycle direction: `-1` previous, `+1` next, `0` none.
static mut NEXT_WEAPON: c_int = 0;

/// Carry for low-resolution turn rounding (static local in `G_BuildTiccmd`).
static mut LOWRES_TURN_CARRY: i16 = 0;

/// Buffer for turbo-cheat message (static local in `G_Ticker`).
static mut TURBOMESSAGE: [c_char; 80] = [0; 80];

/// Buffer for `DemoVersionDescription` (static local in `G_DoPlayDemo`).
static mut DEMOVERSIONBUF: [c_char; 16] = [0; 16];

// ---------------------------------------------------------------------------
// External function and variable declarations
// ---------------------------------------------------------------------------

extern "C" {
    /// libc `snprintf` - variadic; only the buffer-pointer / length form is
    /// actually invoked from this module (turbo banner and demo-version text).
    fn snprintf(buf: *mut c_char, len: usize, fmt: *const c_char, ...) -> c_int;

    /// Engine-wide fatal error from `i_system.c`. Kept variadic because call
    /// sites pass `printf`-style format arguments.
    fn I_Error(format: *const c_char, ...) -> !;
    /// Engine-wide clean shutdown from `i_system.c` (used by single demos).
    fn I_Quit() -> !;
}

/// `gametic` is the global engine tic counter; `ticdup` is the demo dup factor.
/// Both originate from `d_loop.c`.
use crate::doom::d_loop::{gametic, ticdup};

/// Command-line `-fast`, `-nomonsters`, `-respawn` toggles, the wipe-state
/// latch, demo advancement, and the title-screen page ticker from `d_main.c`.
use crate::doom::d_main::{
    fastparm, nomonsters, respawnparm, wipegamestate, D_AdvanceDemo, D_PageTicker,
};

/// Automap toggle, responder, stop hook and ticker from `am_map.c`.
use crate::doom::am_map::{automapactive, AM_Responder, AM_Stop, AM_Ticker};

/// Per-player command buffer ring from `d_net.c`.
use crate::doom::d_net::netcmds;

/// Savegame I/O primitives (header read/write, world/thinker/specials
/// archive/unarchive, file-path helpers) from `p_saveg.c`.
use crate::doom::p_saveg::{
    save_stream, savegame_error, P_ArchivePlayers, P_ArchiveSpecials, P_ArchiveThinkers,
    P_ArchiveWorld, P_ReadSaveGameEOF, P_ReadSaveGameHeader, P_SaveGameFile, P_TempSaveGameFile,
    P_UnArchivePlayers, P_UnArchiveSpecials, P_UnArchiveThinkers, P_UnArchiveWorld,
    P_WriteSaveGameEOF, P_WriteSaveGameHeader,
};

/// Top-level map loader from `p_setup.c`.
use crate::doom::p_setup::P_SetupLevel;

/// Mobj lifecycle and spawn helpers from `p_mobj.c`.
use crate::doom::p_mobj::{P_RemoveMobj, P_SpawnMobj, P_SpawnPlayer};

/// Movement collision check used to validate respawn spots from `p_map.c`.
use crate::doom::p_map::P_CheckPosition;

/// 3D view sizing helpers and point-to-subsector lookup from `r_main.c`.
use crate::doom::r_main::{setsizeneeded, R_ExecuteSetViewSize, R_PointInSubsector};

/// Texture/flat name resolution from `r_data.c`.
use crate::doom::r_data::{R_FlatNumForName, R_TextureNumForName};

/// Sky-flat index and active sky texture id from `r_sky.c`.
use crate::doom::r_sky::{skyflatnum, skytexture};

/// Status-bar back-screen painter from `r_draw.c`.
use crate::doom::r_draw::R_FillBackScreen;

/// Zone-allocator heap check and (de)allocation primitives from `z_zone.c`.
use crate::doom::z_zone::{Z_CheckHeap, Z_Free, Z_Malloc};

/// `leveltime` counter (tics since level start) and thinker tick driver from
/// `p_tick.c`.
use crate::doom::p_tick::{leveltime, P_Ticker};

/// Status-bar event responder and ticker from `st_stuff.c`.
use crate::doom::st_stuff::{ST_Responder, ST_Ticker};

/// HUD chat / message primitives and player-name table from `hu_stuff.c`.
use crate::doom::hu_stuff::{player_names, HU_Responder, HU_Ticker, HU_dequeueChatChar};

/// Intermission start/end/tick hooks from `wi_stuff.c`.
use crate::doom::wi_stuff::{WI_End, WI_Start, WI_Ticker};

/// Finale (text crawl / cast call) responder, starter and ticker from `f_finale.c`.
use crate::doom::f_finale::{F_Responder, F_StartFinale, F_Ticker};

/// Sound channel pause/resume and one-shot sfx start from `s_sound.c`.
use crate::doom::s_sound::{S_PauseSound, S_ResumeSound, S_StartSound};

/// Mouse-sensitivity slider and main-menu opener from `m_menu.c`.
use crate::doom::m_menu::{mouseSensitivity, M_StartControlPanel};

/// String / file / formatted-print utilities from `m_misc.c`.
use crate::doom::m_misc::{M_StringCopy, M_TempFile, M_WriteFile, M_snprintf_clamp};

/// Command-line argument table and lookup helpers from `m_argv.c`.
use crate::doom::m_argv::{myargv, M_CheckParm, M_CheckParmWithArgs};

/// `rndindex` consistency-check cookie and seed reset from `m_random.c`.
use crate::doom::m_random::{rndindex, M_ClearRandom};

/// Statistics-dump hook used after a level completes, from `statdump.c`.
use crate::doom::statdump::StatCopy;

/// Screenshot grabber from `v_video.c`.
use crate::doom::v_video::V_ScreenShot;

/// Realtime tick counter from `i_timer.c`.
use crate::doom::i_timer::I_GetTime;

/// WAD lump cache / release / lookup primitives from `w_wad.c`.
use crate::doom::w_wad::{W_CacheLumpName, W_CheckNumForName, W_ReleaseLumpName};

/// Configurable key, joystick and mouse button bindings from `m_controls.c`.
/// Imported in bulk because `G_BuildTiccmd` and `G_Responder` interrogate
/// almost every binding to assemble each tic's command.
use crate::doom::m_controls::{
    dclick_use, joybfire, joybnextweapon, joybprevweapon, joybspeed, joybstrafe, joybstrafeleft,
    joybstraferight, joybuse, key_demo_quit, key_down, key_fire, key_left, key_nextweapon,
    key_pause, key_prevweapon, key_right, key_speed, key_spy, key_strafe, key_strafeleft,
    key_straferight, key_up, key_use, key_weapon1, key_weapon2, key_weapon3, key_weapon4,
    key_weapon5, key_weapon6, key_weapon7, key_weapon8, mousebbackward, mousebfire, mousebforward,
    mousebnextweapon, mousebprevweapon, mousebstrafe, mousebstrafeleft, mousebstraferight,
    mousebuse,
};

/// `PU_STATIC` Z_Malloc purge tag used for the demo buffer and demo filename.
use crate::doom::z_zone::PU_STATIC;

// ---------------------------------------------------------------------------
// DEH_String identity (no dehacked support)
// ---------------------------------------------------------------------------

/// Stand-in for the Dehacked string-substitution macro from `deh_str.h`.
///
/// This port does not implement Dehacked patches, so the lookup is the
/// identity function. Kept as a wrapper to make the original C call sites
/// translate cleanly and to leave a single seam where Dehacked support could
/// later be added.
///
/// # Safety
/// Caller must ensure `s` is a valid NUL-terminated C string for the
/// lifetime of the returned pointer.
#[inline]
unsafe fn DEH_String(s: *const c_char) -> *const c_char {
    s
}

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
unsafe fn logical_gamemission() -> c_int {
    use crate::doom::d_mode::{doom2, pack_plut, pack_tnt};
    if gamemission == pack_tnt || gamemission == pack_plut {
        doom2
    } else {
        gamemission
    }
}

// ---------------------------------------------------------------------------
// Mouse/joystick button accessors
// (In C: mousebuttons = &mousearray[1], joybuttons = &joyarray[1],
//  allowing negative index -1 meaning "no button".)
// ---------------------------------------------------------------------------

/// Read a mouse button state by C-style "may be `-1`" index.
///
/// Vanilla Doom stores mouse buttons in `mousearray[MAX_MOUSE_BUTTONS+1]`
/// with `mousebuttons = &mousearray[1]`, so `mousebuttons[-1]` is a legal
/// "no button bound" sentinel that always reads false. This helper restores
/// the same behaviour without aliasing.
///
/// Returns `0` for `n < 0` or `n >= MAX_MOUSE_BUTTONS`, otherwise the
/// currently latched state of mouse button `n`.
///
/// # Safety
/// Reads the `MOUSEARRAY` global; safe under the single-threaded contract.
#[inline]
unsafe fn mousebutton(n: c_int) -> boolean {
    if n < 0 || n >= MAX_MOUSE_BUTTONS as c_int {
        return 0;
    }
    MOUSEARRAY[(n + 1) as usize]
}

/// Joystick equivalent of [`mousebutton`]; same `-1`-as-unbound semantics.
///
/// # Safety
/// Reads the `JOYARRAY` global; safe under the single-threaded contract.
#[inline]
unsafe fn joybutton(n: c_int) -> boolean {
    if n < 0 || n >= MAX_JOY_BUTTONS as c_int {
        return 0;
    }
    JOYARRAY[(n + 1) as usize]
}

// ---------------------------------------------------------------------------
// G_CmdChecksum
// ---------------------------------------------------------------------------

/// Compute a checksum over the first `sizeof(ticcmd_t)/4 - 1` 4-byte words
/// of `cmd` using wrapping integer addition.
///
/// Vanilla Doom uses this checksum for netgame and demo-validation purposes.
/// The final word is excluded so that fields stored at the tail of the
/// struct (e.g. inventory padding) do not affect compatibility.
///
/// # Safety
/// `cmd` must point to a fully-initialised `TiccmdT` with at least
/// `sizeof::<TiccmdT>()` valid bytes; reads through the pointer as a raw
/// `c_int` array. Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_CmdChecksum(cmd: *const TiccmdT) -> c_int {
    let n = std::mem::size_of::<TiccmdT>() / 4 - 1;
    let words = cmd as *const c_int;
    let mut sum: c_int = 0;
    for i in 0..n {
        sum = sum.wrapping_add(*words.add(i));
    }
    sum
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
unsafe fn weapon_selectable(weapon: c_int) -> bool {
    // Can't select supershotgun in Doom 1.
    if weapon == wp_supershotgun && logical_gamemission() == doom {
        return false;
    }
    // Plasma and BFG unavailable in shareware.
    if (weapon == wp_plasma || weapon == wp_bfg) && gamemission == doom && gamemode == shareware {
        return false;
    }
    let cp = consoleplayer as usize;
    if players[cp].weaponowned[weapon as usize] == 0 {
        return false;
    }
    // Can't select fist if we have chainsaw, unless we also have berserk.
    if weapon == wp_fist
        && players[cp].weaponowned[wp_chainsaw as usize] != 0
        && players[cp].powers[pw_strength] == 0
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
unsafe fn g_next_weapon(direction: c_int) -> c_int {
    let cp = consoleplayer as usize;
    let weapon = if players[cp].pendingweapon == wp_nochange {
        players[cp].readyweapon
    } else {
        players[cp].pendingweapon
    };

    let n = WEAPON_ORDER_TABLE.len() as c_int;
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
/// before assembly. Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_BuildTiccmd(cmd: *mut TiccmdT, maketic: c_int) {
    use crate::doom::c_ffi::BACKUPTICS;

    let cmd = &mut *cmd;
    *cmd = std::mem::zeroed();

    cmd.consistancy = consistancy[consoleplayer as usize][(maketic as usize) % BACKUPTICS];

    let strafe = (GAMEKEYDOWN[key_strafe as usize] != 0)
        || (mousebutton(mousebstrafe) != 0)
        || (joybutton(joybstrafe) != 0);

    // "joyb_speed = 31" autorun hack: key_speed >= NUMKEYS means always running
    let speed = (key_speed >= NUMKEYS as c_int)
        || (joybspeed >= MAX_JOY_BUTTONS as c_int)
        || (GAMEKEYDOWN[key_speed as usize] != 0)
        || (joybutton(joybspeed) != 0);
    let speed = speed as usize;

    let mut forward: c_int = 0;
    let mut side: c_int = 0;

    // Two-stage accelerative turning
    if JOYXMOVE != 0 || GAMEKEYDOWN[key_right as usize] != 0 || GAMEKEYDOWN[key_left as usize] != 0
    {
        TURNHELD += ticdup;
    } else {
        TURNHELD = 0;
    }

    let tspeed = if TURNHELD < 6 { 2usize } else { speed };

    if strafe {
        if GAMEKEYDOWN[key_right as usize] != 0 {
            side += sidemove[speed];
        }
        if GAMEKEYDOWN[key_left as usize] != 0 {
            side -= sidemove[speed];
        }
        if JOYXMOVE > 0 {
            side += sidemove[speed];
        }
        if JOYXMOVE < 0 {
            side -= sidemove[speed];
        }
    } else {
        if GAMEKEYDOWN[key_right as usize] != 0 {
            cmd.angleturn -= angleturn[tspeed] as i16;
        }
        if GAMEKEYDOWN[key_left as usize] != 0 {
            cmd.angleturn += angleturn[tspeed] as i16;
        }
        if JOYXMOVE > 0 {
            cmd.angleturn -= angleturn[tspeed] as i16;
        }
        if JOYXMOVE < 0 {
            cmd.angleturn += angleturn[tspeed] as i16;
        }
    }

    if GAMEKEYDOWN[key_up as usize] != 0 {
        forward += forwardmove[speed];
    }
    if GAMEKEYDOWN[key_down as usize] != 0 {
        forward -= forwardmove[speed];
    }
    if JOYYMOVE < 0 {
        forward += forwardmove[speed];
    }
    if JOYYMOVE > 0 {
        forward -= forwardmove[speed];
    }

    if GAMEKEYDOWN[key_strafeleft as usize] != 0
        || joybutton(joybstrafeleft) != 0
        || mousebutton(mousebstrafeleft) != 0
        || JOYSTRAFEMOVE < 0
    {
        side -= sidemove[speed];
    }
    if GAMEKEYDOWN[key_straferight as usize] != 0
        || joybutton(joybstraferight) != 0
        || mousebutton(mousebstraferight) != 0
        || JOYSTRAFEMOVE > 0
    {
        side += sidemove[speed];
    }

    // Buttons
    cmd.chatchar = HU_dequeueChatChar() as u8;

    if GAMEKEYDOWN[key_fire as usize] != 0
        || mousebutton(mousebfire) != 0
        || joybutton(joybfire) != 0
    {
        cmd.buttons |= BT_ATTACK;
    }

    if GAMEKEYDOWN[key_use as usize] != 0 || joybutton(joybuse) != 0 || mousebutton(mousebuse) != 0
    {
        cmd.buttons |= BT_USE;
        DCLICKS = 0;
    }

    // Weapon cycling
    if gamestate == GS_LEVEL && NEXT_WEAPON != 0 {
        let i = g_next_weapon(NEXT_WEAPON);
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
            if GAMEKEYDOWN[key as usize] != 0 {
                cmd.buttons |= BT_CHANGE;
                cmd.buttons |= (i as u8) << BT_WEAPONSHIFT;
                break;
            }
        }
    }
    NEXT_WEAPON = 0;

    // Mouse forward/backward
    if mousebutton(mousebforward) != 0 {
        forward += forwardmove[speed];
    }
    if mousebutton(mousebbackward) != 0 {
        forward -= forwardmove[speed];
    }

    // Double-click use
    if dclick_use != 0 {
        if mousebutton(mousebforward) != DCLICKSTATE && DCLICKTIME > 1 {
            DCLICKSTATE = mousebutton(mousebforward);
            if DCLICKSTATE != 0 {
                DCLICKS += 1;
            }
            if DCLICKS == 2 {
                cmd.buttons |= BT_USE;
                DCLICKS = 0;
            } else {
                DCLICKTIME = 0;
            }
        } else {
            DCLICKTIME += ticdup;
            if DCLICKTIME > 20 {
                DCLICKS = 0;
                DCLICKSTATE = 0;
            }
        }

        let bstrafe = (mousebutton(mousebstrafe) != 0 || joybutton(joybstrafe) != 0) as boolean;
        if bstrafe != DCLICKSTATE2 && DCLICKTIME2 > 1 {
            DCLICKSTATE2 = bstrafe;
            if DCLICKSTATE2 != 0 {
                DCLICKS2 += 1;
            }
            if DCLICKS2 == 2 {
                cmd.buttons |= BT_USE;
                DCLICKS2 = 0;
            } else {
                DCLICKTIME2 = 0;
            }
        } else {
            DCLICKTIME2 += ticdup;
            if DCLICKTIME2 > 20 {
                DCLICKS2 = 0;
                DCLICKSTATE2 = 0;
            }
        }
    }

    forward += MOUSEY;
    if strafe {
        side += MOUSEX * 2;
    } else {
        cmd.angleturn -= (MOUSEX * 0x8) as i16;
    }

    if MOUSEX == 0 {
        testcontrols_mousespeed = 0;
    }
    MOUSEX = 0;
    MOUSEY = 0;

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

    cmd.forwardmove = (cmd.forwardmove as c_int + forward) as i8;
    cmd.sidemove = (cmd.sidemove as c_int + side) as i8;

    // Special buttons
    if sendpause != 0 {
        sendpause = 0;
        cmd.buttons = BT_SPECIAL | BTS_PAUSE;
    }
    if sendsave != 0 {
        sendsave = 0;
        cmd.buttons = BT_SPECIAL | BTS_SAVEGAME | (SAVEGAMESLOT as u8) << BTS_SAVESHIFT;
    }

    // Low-resolution turning
    if lowres_turn != 0 {
        let desired: i16 = cmd.angleturn.wrapping_add(LOWRES_TURN_CARRY);
        cmd.angleturn = ((desired as i32 + 128) & 0xff00) as i16;
        LOWRES_TURN_CARRY = desired.wrapping_sub(cmd.angleturn);
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
/// the input rings, the players array). Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_DoLoadLevel() {
    skyflatnum = R_FlatNumForName(DEH_String(c"F_SKY1".as_ptr()) as *mut c_char);

    // Fix sky texture for Final Doom / Chex
    if gamemode == commercial && (gameversion == exe_final2 || gameversion == exe_chex) {
        let skytexturename: *const c_char = if gamemap < 12 {
            c"SKY1".as_ptr()
        } else if gamemap < 21 {
            c"SKY2".as_ptr()
        } else {
            c"SKY3".as_ptr()
        };
        skytexture = R_TextureNumForName(DEH_String(skytexturename) as *mut c_char);
    }

    levelstarttic = gametic;

    if wipegamestate == GS_LEVEL {
        wipegamestate = -1; // force a wipe
    }

    gamestate = GS_LEVEL;

    for i in 0..MAXPLAYERS {
        turbodetected[i] = 0;
        if playeringame[i] != 0 && players[i].playerstate == PST_DEAD {
            players[i].playerstate = PST_REBORN;
        }
        players[i].frags = [0; MAXPLAYERS];
    }

    P_SetupLevel(gameepisode, gamemap, 0, gameskill);
    displayplayer = consoleplayer;
    gameaction = ga_nothing;
    Z_CheckHeap();

    // Clear input state
    GAMEKEYDOWN = [0; NUMKEYS];
    JOYXMOVE = 0;
    JOYYMOVE = 0;
    JOYSTRAFEMOVE = 0;
    MOUSEX = 0;
    MOUSEY = 0;
    sendpause = 0;
    sendsave = 0;
    paused = 0;
    MOUSEARRAY = [0; MAX_MOUSE_BUTTONS + 1];
    JOYARRAY = [0; MAX_JOY_BUTTONS + 1];

    if testcontrols != 0 {
        players[consoleplayer as usize].message = c"Press escape to quit.".as_ptr().cast_mut();
    }
}

// ---------------------------------------------------------------------------
// SetJoyButtons / SetMouseButtons (static helpers)
// ---------------------------------------------------------------------------

/// Update the joystick button latch from a packed bitmask and detect
/// rising edges of the prev/next weapon buttons, scheduling a weapon cycle
/// in `NEXT_WEAPON` for the next `G_BuildTiccmd`.
///
/// # Safety
/// Mutates `JOYARRAY` and `NEXT_WEAPON`; safe under the single-threaded
/// engine convention.
unsafe fn set_joy_buttons(buttons_mask: c_uint) {
    for i in 0..MAX_JOY_BUTTONS {
        let button_on = ((buttons_mask >> i) & 1) != 0;
        if JOYARRAY[i + 1] == 0 && button_on {
            if i as c_int == joybprevweapon {
                NEXT_WEAPON = -1;
            } else if i as c_int == joybnextweapon {
                NEXT_WEAPON = 1;
            }
        }
        JOYARRAY[i + 1] = button_on as boolean;
    }
}

/// Mouse-button equivalent of [`set_joy_buttons`]: latches per-button state
/// and arms a weapon-cycle on the rising edge of the bound mouse buttons.
///
/// # Safety
/// Mutates `MOUSEARRAY` and `NEXT_WEAPON`; safe under the single-threaded
/// engine convention.
unsafe fn set_mouse_buttons(buttons_mask: c_uint) {
    for i in 0..MAX_MOUSE_BUTTONS {
        let button_on = ((buttons_mask >> i) & 1) != 0;
        if MOUSEARRAY[i + 1] == 0 && button_on {
            if i as c_int == mousebprevweapon {
                NEXT_WEAPON = -1;
            } else if i as c_int == mousebnextweapon {
                NEXT_WEAPON = 1;
            }
        }
        MOUSEARRAY[i + 1] = button_on as boolean;
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
/// * Otherwise routes by `event_t::type_`: keydown updates `GAMEKEYDOWN`
///   (with `key_pause` setting `sendpause`), keyup clears it, mouse and
///   joystick events latch axes and buttons.
///
/// # Safety
/// Dereferences `ev` and mutates many static input globals; safe under the
/// single-threaded engine convention. Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_Responder(ev: *mut event_t) -> boolean {
    let ev = &*ev;

    // Spy mode changes even during demo
    if gamestate == GS_LEVEL
        && ev.type_ == 0 // ev_keydown
        && ev.data1 == key_spy
        && (singledemo != 0 || deathmatch == 0)
    {
        loop {
            displayplayer += 1;
            if displayplayer == MAXPLAYERS as c_int {
                displayplayer = 0;
            }
            if playeringame[displayplayer as usize] != 0 || displayplayer == consoleplayer {
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
        if ev.type_ == 0 // ev_keydown
            || (ev.type_ == 2 && ev.data1 != 0) // ev_mouse with buttons
            || (ev.type_ == 3 && ev.data1 != 0)
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

    if gamestate == GS_FINALE && F_Responder(ev as *const event_t as *mut event_t) != 0 {
        return 1;
    }

    if testcontrols != 0 && ev.type_ == 2 {
        // ev_mouse
        testcontrols_mousespeed = ev.data2.abs();
    }

    // Prev/next weapon keys
    if ev.type_ == 0 && ev.data1 == key_prevweapon {
        NEXT_WEAPON = -1;
    } else if ev.type_ == 0 && ev.data1 == key_nextweapon {
        NEXT_WEAPON = 1;
    }

    match ev.type_ {
        0 => {
            // ev_keydown
            if ev.data1 == key_pause {
                sendpause = 1;
            } else if (ev.data1 as usize) < NUMKEYS {
                GAMEKEYDOWN[ev.data1 as usize] = 1;
            }
            return 1;
        }
        1 => {
            // ev_keyup
            if (ev.data1 as usize) < NUMKEYS {
                GAMEKEYDOWN[ev.data1 as usize] = 0;
            }
            return 0;
        }
        2 => {
            // ev_mouse
            set_mouse_buttons(ev.data1 as c_uint);
            MOUSEX = ev.data2 * (mouseSensitivity + 5) / 10;
            MOUSEY = ev.data3 * (mouseSensitivity + 5) / 10;
            return 1;
        }
        3 => {
            // ev_joystick
            set_joy_buttons(ev.data1 as c_uint);
            JOYXMOVE = ev.data2;
            JOYYMOVE = ev.data3;
            JOYSTRAFEMOVE = ev.data4;
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
/// once per tic from the engine main loop. Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_Ticker() {
    use crate::doom::c_ffi::BACKUPTICS;

    // Player reborns
    for i in 0..MAXPLAYERS {
        if playeringame[i] != 0 && players[i].playerstate == PST_REBORN {
            G_DoReborn(i as c_int);
        }
    }

    // Process pending game actions
    while gameaction != ga_nothing {
        match gameaction {
            ga_loadlevel => G_DoLoadLevel(),
            ga_newgame => G_DoNewGame(),
            ga_loadgame => G_DoLoadGame(),
            ga_savegame => G_DoSaveGame(),
            ga_playdemo => G_DoPlayDemo(),
            ga_completed => G_DoCompleted(),
            ga_victory => F_StartFinale(),
            ga_worlddone => G_DoWorldDone(),
            ga_screenshot => {
                V_ScreenShot(c"DOOM%02i.%s".as_ptr().cast_mut());
                players[consoleplayer as usize].message =
                    DEH_String(c"screen shot".as_ptr()) as *mut c_char;
                gameaction = ga_nothing;
            }
            _ => {}
        }
    }

    // Get commands, check consistency, build new consistency check
    let buf = ((gametic / ticdup) as usize) % BACKUPTICS;

    for i in 0..MAXPLAYERS {
        if playeringame[i] != 0 {
            let cmd = &mut players[i].cmd as *mut TiccmdT;

            // Copy net command into player command
            std::ptr::copy_nonoverlapping(netcmds.add(i), cmd, 1);

            if demoplayback != 0 {
                G_ReadDemoTiccmd(cmd);
            }
            if demorecording != 0 {
                G_WriteDemoTiccmd(cmd);
            }

            // Turbo detection
            if (*cmd).forwardmove > 0x32 {
                turbodetected[i] = 1;
            }

            if (gametic & 31) == 0
                && ((gametic >> 5) % MAXPLAYERS as c_int) == i as c_int
                && turbodetected[i] != 0
            {
                M_snprintf_clamp(
                    std::ptr::addr_of_mut!(TURBOMESSAGE[0]),
                    80,
                    snprintf(
                        std::ptr::addr_of_mut!(TURBOMESSAGE[0]),
                        80,
                        c"%s is turbo!".as_ptr(),
                        player_names[i],
                    ),
                );
                players[consoleplayer as usize].message = std::ptr::addr_of_mut!(TURBOMESSAGE[0]);
                turbodetected[i] = 0;
            }

            if netgame != 0 && netdemo == 0 && (gametic % ticdup) == 0 {
                if gametic > BACKUPTICS as c_int && consistancy[i][buf] != (*cmd).consistancy {
                    I_Error(
                        c"consistency failure (%i should be %i)".as_ptr(),
                        (*cmd).consistancy as c_int,
                        consistancy[i][buf] as c_int,
                    );
                }
                let mo = players[i].mo as *mut mobj_t;
                if !mo.is_null() {
                    consistancy[i][buf] = (*mo).x as u8;
                } else {
                    consistancy[i][buf] = rndindex as u8;
                }
            }
        }
    }

    // Check for special buttons
    for i in 0..MAXPLAYERS {
        if playeringame[i] != 0 {
            let buttons = players[i].cmd.buttons;
            if buttons & BT_SPECIAL != 0 {
                match buttons & BT_SPECIALMASK {
                    BTS_PAUSE => {
                        paused ^= 1;
                        if paused != 0 {
                            S_PauseSound();
                        } else {
                            S_ResumeSound();
                        }
                    }
                    BTS_SAVEGAME => {
                        if SAVEDESCRIPTION[0] == 0 {
                            M_StringCopy(
                                std::ptr::addr_of_mut!(SAVEDESCRIPTION[0]),
                                c"NET GAME".as_ptr(),
                                32,
                            );
                        }
                        SAVEGAMESLOT = ((buttons & BTS_SAVEMASK) >> BTS_SAVESHIFT) as c_int;
                        gameaction = ga_savegame;
                    }
                    _ => {}
                }
            }
        }
    }

    // Check if intermission screen just ended
    if oldgamestate == GS_INTERMISSION && gamestate != GS_INTERMISSION {
        WI_End();
    }
    oldgamestate = gamestate;

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
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_InitPlayer(player: c_int) {
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
/// caller is `G_DoCompleted`, which guarantees this). Exported as
/// `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_PlayerFinishLevel(player: c_int) {
    let p = &mut players[player as usize];
    p.powers = [0; 6];
    p.cards = [0; 6];
    (*(p.mo as *mut mobj_t)).flags &= !crate::doom::info::MF_SHADOW;
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
/// `player` must be a valid index into `players[]`. Exported as
/// `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_PlayerReborn(player: c_int) {
    let idx = player as usize;
    let frags = players[idx].frags;
    let killcount = players[idx].killcount;
    let itemcount = players[idx].itemcount;
    let secretcount = players[idx].secretcount;

    players[idx] = std::mem::zeroed();

    players[idx].frags = frags;
    players[idx].killcount = killcount;
    players[idx].itemcount = itemcount;
    players[idx].secretcount = secretcount;

    players[idx].usedown = 1;
    players[idx].attackdown = 1;
    players[idx].playerstate = PST_LIVE;
    players[idx].health = DEH_INITIAL_HEALTH;
    players[idx].readyweapon = wp_pistol;
    players[idx].pendingweapon = wp_pistol;
    players[idx].weaponowned[wp_fist as usize] = 1;
    players[idx].weaponowned[wp_pistol as usize] = 1;
    players[idx].ammo[am_clip] = DEH_INITIAL_BULLETS;

    for i in 0..4 {
        // NUMAMMO = 4
        players[idx].maxammo[i] = maxammo[i];
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
/// queue globals. Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_CheckSpot(playernum: c_int, mthing: *mut mapthing_t) -> boolean {
    if players[playernum as usize].mo.is_null() {
        // First spawn of level, before corpses
        for i in 0..playernum as usize {
            let mo = players[i].mo as *mut mobj_t;
            if (*mo).x == ((*mthing).x as fixed_t) << 16
                && (*mo).y == ((*mthing).y as fixed_t) << 16
            {
                return 0;
            }
        }
        return 1;
    }

    let x = ((*mthing).x as fixed_t) << 16;
    let y = ((*mthing).y as fixed_t) << 16;

    if P_CheckPosition(
        players[playernum as usize].mo as *mut crate::doom::c_ffi::mobj_t,
        x,
        y,
    ) == 0
    {
        return 0;
    }

    // Flush old corpse
    if bodyqueslot >= 32 {
        P_RemoveMobj(bodyque[(bodyqueslot % 32) as usize]);
    }
    bodyque[(bodyqueslot % 32) as usize] = players[playernum as usize].mo as *mut mobj_t;
    bodyqueslot += 1;

    // Spawn teleport fog
    let ss = R_PointInSubsector(x, y);

    // Replicate vanilla signed-angle overflow (from PrBoom+)
    let an_raw = (0x10000000i32).wrapping_mul((*mthing).angle as i32 / 45);
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
            I_Error(c"G_CheckSpot: unexpected angle %d\n".as_ptr(), an_raw);
        }
    }

    let floorheight = (*(*ss).sector).floorheight;
    let mo = P_SpawnMobj(x + 20 * xa, y + 20 * ya, floorheight, MT_TFOG);

    if players[consoleplayer as usize].viewz != 1 {
        S_StartSound(mo as *mut c_void, Sfx::Telept as c_int);
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
/// table must already be populated by `P_SetupLevel`. Exported as
/// `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_DeathMatchSpawnPlayer(playernum: c_int) {
    let selections = deathmatch_p.offset_from(std::ptr::addr_of!(deathmatchstarts[0])) as c_int;
    if selections < 4 {
        I_Error(c"Only %i deathmatch spots, 4 required".as_ptr(), selections);
    }

    for _ in 0..20 {
        let i = (P_Random() % selections as c_int) as usize;
        if G_CheckSpot(
            playernum,
            &mut deathmatchstarts[i] as *mut SetupMapThing as *mut mapthing_t,
        ) != 0
        {
            deathmatchstarts[i].r#type = (playernum + 1) as i16;
            P_SpawnPlayer(&mut deathmatchstarts[i] as *mut SetupMapThing as *mut mapthing_t);
            return;
        }
    }
    P_SpawnPlayer(&mut playerstarts[playernum as usize] as *mut SetupMapThing as *mut mapthing_t);
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
/// `playernum` must be a valid player index. Exported as `#[no_mangle]` for
/// C callers.
#[no_mangle]
pub unsafe extern "C" fn G_DoReborn(playernum: c_int) {
    if netgame == 0 {
        gameaction = ga_loadlevel;
    } else {
        let mo = players[playernum as usize].mo as *mut mobj_t;
        if !mo.is_null() {
            (*mo).player = ptr::null_mut();
        }

        if deathmatch != 0 {
            G_DeathMatchSpawnPlayer(playernum);
            return;
        }

        if G_CheckSpot(
            playernum,
            &mut playerstarts[playernum as usize] as *mut SetupMapThing as *mut mapthing_t,
        ) != 0
        {
            P_SpawnPlayer(
                &mut playerstarts[playernum as usize] as *mut SetupMapThing as *mut mapthing_t,
            );
            return;
        }

        for i in 0..MAXPLAYERS as c_int {
            if G_CheckSpot(
                playernum,
                &mut playerstarts[i as usize] as *mut SetupMapThing as *mut mapthing_t,
            ) != 0
            {
                playerstarts[i as usize].r#type = (playernum + 1) as i16;
                P_SpawnPlayer(
                    &mut playerstarts[i as usize] as *mut SetupMapThing as *mut mapthing_t,
                );
                playerstarts[i as usize].r#type = (i + 1) as i16;
                return;
            }
        }
        P_SpawnPlayer(
            &mut playerstarts[playernum as usize] as *mut SetupMapThing as *mut mapthing_t,
        );
    }
}

// ---------------------------------------------------------------------------
// G_ScreenShot
// ---------------------------------------------------------------------------

/// Defer a screenshot to the next `G_Ticker` pass via `gameaction =
/// ga_screenshot`.
///
/// # Safety
/// Writes the `gameaction` global. Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_ScreenShot() {
    gameaction = ga_screenshot;
}

// ---------------------------------------------------------------------------
// G_ExitLevel / G_SecretExitLevel
// ---------------------------------------------------------------------------

/// Request a normal end-of-level transition; clears `secretexit` so the
/// next intermission picks the standard "next map" target.
///
/// # Safety
/// Writes the `secretexit` and `gameaction` globals. Exported as
/// `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_ExitLevel() {
    secretexit = 0;
    gameaction = ga_completed;
}

/// Request a secret-exit end-of-level transition.
///
/// On Doom II the secret exit only applies when MAP31 is actually present
/// in the loaded WAD ("if no Wolf3D levels, no secret exit" - the German
/// edition retail patch removed those maps).
///
/// # Safety
/// Writes the `secretexit` and `gameaction` globals. Exported as
/// `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_SecretExitLevel() {
    if gamemode == commercial && W_CheckNumForName(c"map31".as_ptr()) < 0 {
        secretexit = 0;
    } else {
        secretexit = 1;
    }
    gameaction = ga_completed;
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
/// `players` array. Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_DoCompleted() {
    gameaction = ga_nothing;

    for i in 0..MAXPLAYERS {
        if playeringame[i] != 0 {
            G_PlayerFinishLevel(i as c_int);
        }
    }

    if automapactive != 0 {
        AM_Stop();
    }

    if gamemode != commercial {
        if gameversion == exe_chex {
            if gamemap == 5 {
                gameaction = ga_victory;
                return;
            }
        } else {
            match gamemap {
                8 => {
                    gameaction = ga_victory;
                    return;
                }
                9 => {
                    for i in 0..MAXPLAYERS {
                        players[i].didsecret = 1;
                    }
                }
                _ => {}
            }
        }
    }

    if gamemap == 8 && gamemode != commercial {
        gameaction = ga_victory;
        return;
    }
    if gamemap == 9 && gamemode != commercial {
        for i in 0..MAXPLAYERS {
            players[i].didsecret = 1;
        }
    }

    wminfo.didsecret = players[consoleplayer as usize].didsecret;
    wminfo.epsd = gameepisode - 1;
    wminfo.last = gamemap - 1;

    if gamemode == commercial {
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

    wminfo.maxkills = totalkills;
    wminfo.maxitems = totalitems;
    wminfo.maxsecret = totalsecret;
    wminfo.maxfrags = 0;

    wminfo.partime = if gamemode == commercial {
        35 * cpars[(gamemap - 1) as usize]
    } else if gameepisode < 4 {
        35 * pars[gameepisode as usize][gamemap as usize]
    } else {
        35 * cpars[gamemap as usize]
    };

    wminfo.pnum = consoleplayer;

    for i in 0..MAXPLAYERS {
        wminfo.plyr[i].in_ = playeringame[i];
        wminfo.plyr[i].skills = players[i].killcount;
        wminfo.plyr[i].sitems = players[i].itemcount;
        wminfo.plyr[i].ssecret = players[i].secretcount;
        wminfo.plyr[i].stime = leveltime;
        wminfo.plyr[i].frags = players[i].frags;
    }

    gamestate = GS_INTERMISSION;
    viewactive = 0;
    automapactive = 0;

    StatCopy(&raw mut wminfo as *mut _ as *mut crate::doom::statdump::wbstartstruct_t);
    WI_Start(&raw mut wminfo);
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
/// Writes `gameaction`, `players[consoleplayer].didsecret`. Exported as
/// `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_WorldDone() {
    gameaction = ga_worlddone;

    if secretexit != 0 {
        players[consoleplayer as usize].didsecret = 1;
    }

    if gamemode == commercial {
        match gamemap {
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
/// Mutates `gamestate`, `gamemap`, `viewactive`, `gameaction`. Exported as
/// `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_DoWorldDone() {
    gamestate = GS_LEVEL;
    gamemap = wminfo.next + 1;
    G_DoLoadLevel();
    gameaction = ga_nothing;
    viewactive = 1;
}

// ---------------------------------------------------------------------------
// G_LoadGame / G_DoLoadGame
// ---------------------------------------------------------------------------

/// Defer a savegame load: copy `name` into the `savename` buffer and queue
/// `ga_loadgame` for the next `G_Ticker`.
///
/// # Safety
/// `name` must be a valid NUL-terminated C string. Exported as
/// `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_LoadGame(name: *mut c_char) {
    M_StringCopy(std::ptr::addr_of_mut!(savename[0]), name, 256);
    gameaction = ga_loadgame;
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
/// Mutates a large amount of game state and performs raw file I/O. Exported
/// as `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_DoLoadGame() {
    gameaction = ga_nothing;

    save_stream = libc::fopen(
        std::ptr::addr_of!(savename[0]),
        c"rb".as_ptr() as *const libc::c_char,
    );
    if save_stream.is_null() {
        return;
    }

    savegame_error = 0;

    if P_ReadSaveGameHeader() == 0 {
        libc::fclose(save_stream);
        return;
    }

    let savedleveltime = leveltime;

    G_InitNew(gameskill, gameepisode, gamemap);

    leveltime = savedleveltime;

    P_UnArchivePlayers();
    P_UnArchiveWorld();
    P_UnArchiveThinkers();
    P_UnArchiveSpecials();

    if P_ReadSaveGameEOF() == 0 {
        I_Error(c"Bad savegame".as_ptr());
    }

    libc::fclose(save_stream);

    if setsizeneeded.is_truthy() {
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
///
/// # Safety
/// `description` must be a valid NUL-terminated C string. Exported as
/// `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_SaveGame(slot: c_int, description: *const c_char) {
    SAVEGAMESLOT = slot;
    M_StringCopy(std::ptr::addr_of_mut!(SAVEDESCRIPTION[0]), description, 32);
    sendsave = 1;
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
/// # Safety
/// Performs raw `libc` file I/O (`fopen`/`ftell`/`fclose`/`remove`/`rename`)
/// and writes through the global `save_stream`. Exported as `#[no_mangle]`
/// for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_DoSaveGame() {
    let recovery_savegame_file: *mut c_char;
    let temp_savegame_file = P_TempSaveGameFile();
    let savegame_file = P_SaveGameFile(SAVEGAMESLOT);

    save_stream = libc::fopen(temp_savegame_file, c"wb".as_ptr() as *const libc::c_char);

    if save_stream.is_null() {
        let recovery = M_TempFile(c"recovery.dsg".as_ptr().cast_mut());
        recovery_savegame_file = recovery;
        save_stream = libc::fopen(recovery, c"wb".as_ptr() as *const libc::c_char);
        if save_stream.is_null() {
            I_Error(
                c"Failed to open either '%s' or '%s' to write savegame.".as_ptr() as *const c_char,
                temp_savegame_file,
                recovery_savegame_file,
            );
        }
    } else {
        recovery_savegame_file = ptr::null_mut();
    }

    savegame_error = 0;

    P_WriteSaveGameHeader(std::ptr::addr_of_mut!(SAVEDESCRIPTION[0]));
    P_ArchivePlayers();
    P_ArchiveWorld();
    P_ArchiveThinkers();
    P_ArchiveSpecials();
    P_WriteSaveGameEOF();

    if vanilla_savegame_limit != 0 && libc::ftell(save_stream) > SAVEGAMESIZE {
        I_Error(c"Savegame buffer overrun".as_ptr());
    }

    libc::fclose(save_stream);

    if !recovery_savegame_file.is_null() {
        I_Error(
            c"Failed to open savegame file '%s' for writing.\nBut your game has been saved to '%s' for recovery.".as_ptr(),
            temp_savegame_file,
            recovery_savegame_file,
        );
    }

    libc::remove(savegame_file);
    libc::rename(temp_savegame_file, savegame_file);

    gameaction = ga_nothing;
    M_StringCopy(std::ptr::addr_of_mut!(SAVEDESCRIPTION[0]), c"".as_ptr(), 32);

    players[consoleplayer as usize].message = DEH_String(c"game saved.".as_ptr()) as *mut c_char;

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
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_DeferedInitNew(skill: skill_t, episode: c_int, map: c_int) {
    d_skill = skill;
    d_episode = episode;
    d_map = map;
    gameaction = ga_newgame;
}

/// Process `ga_newgame`: clear network / demo state, reset extra players
/// out of the game, then call `G_InitNew` with the deferred `(skill, episode,
/// map)`.
///
/// # Safety
/// Mutates many engine globals. Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_DoNewGame() {
    demoplayback = 0;
    netdemo = 0;
    netgame = 0;
    deathmatch = 0;
    playeringame[1] = 0;
    playeringame[2] = 0;
    playeringame[3] = 0;
    respawnparm = 0;
    fastparm = 0;
    nomonsters = 0;
    consoleplayer = 0;
    G_InitNew(d_skill, d_episode, d_map);
    gameaction = ga_nothing;
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
/// Mutates virtually every game-loop global. Exported as `#[no_mangle]` for
/// C callers.
#[no_mangle]
pub unsafe extern "C" fn G_InitNew(skill: skill_t, episode: c_int, map: c_int) {
    let mut skill = skill;
    let mut episode = episode;
    let mut map = map;

    if paused != 0 {
        paused = 0;
        S_ResumeSound();
    }

    if skill > 4 {
        // sk_nightmare = 4
        skill = 4;
    }

    if gameversion >= exe_ultimate {
        if episode == 0 {
            episode = 4;
        }
    } else {
        episode = episode.clamp(1, 3);
    }

    if episode > 1 && gamemode == shareware {
        episode = 1;
    }

    if map < 1 {
        map = 1;
    }
    if map > 9 && gamemode != commercial {
        map = 9;
    }

    M_ClearRandom();

    if skill == 4 || respawnparm != 0 {
        // sk_nightmare = 4
        respawnmonsters = 1;
    } else {
        respawnmonsters = 0;
    }

    // Fast monsters for nightmare or fastparm
    if fastparm != 0 || (skill == 4 && gameskill != 4) {
        set_fast_monsters(true);
    } else if skill != 4 && gameskill == 4 {
        set_fast_monsters(false);
    }

    // Force players to be reborn on first level load
    for i in 0..MAXPLAYERS {
        players[i].playerstate = PST_REBORN;
    }

    usergame = 1;
    paused = 0;
    demoplayback = 0;
    automapactive = 0;
    viewactive = 1;
    gameepisode = episode;
    gamemap = map;
    gameskill = skill;

    // Set sky texture (vanilla Doom broken behaviour for Doom II: set once at
    // game start, not per level — deliberately preserved for compatibility).
    let skytexturename: *const c_char = if gamemode == commercial {
        if gamemap < 12 {
            c"SKY1".as_ptr()
        } else if gamemap < 21 {
            c"SKY2".as_ptr()
        } else {
            c"SKY3".as_ptr()
        }
    } else {
        match gameepisode {
            1 => c"SKY1".as_ptr(),
            2 => c"SKY2".as_ptr(),
            3 => c"SKY3".as_ptr(),
            4 => c"SKY4".as_ptr(),
            _ => c"SKY1".as_ptr(),
        }
    };

    skytexture = R_TextureNumForName(DEH_String(skytexturename) as *mut c_char);

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
/// Mutates the shared `states` and `mobjinfo` arrays. Exported as
/// `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_SetFastMonsters(fast: c_int) {
    use crate::doom::info::{
        mobjinfo, states, MT_BRUISERSHOT, MT_HEADSHOT, MT_TROOPSHOT, S_SARG_PAIN2, S_SARG_RUN1,
    };

    for i in S_SARG_RUN1 as usize..=S_SARG_PAIN2 as usize {
        if fast != 0 {
            states[i].tics >>= 1;
        } else {
            states[i].tics <<= 1;
        }
    }

    let (bruiser_spd, head_spd, troop_spd): (fixed_t, fixed_t, fixed_t) = if fast != 0 {
        (20 * 65536, 20 * 65536, 20 * 65536)
    } else {
        (15 * 65536, 10 * 65536, 10 * 65536)
    };

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
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_ReadDemoTiccmd(cmd: *mut TiccmdT) {
    if *demo_p == DEMOMARKER {
        G_CheckDemoStatus();
        return;
    }

    let cmd = &mut *cmd;

    // forwardmove: byte reinterpreted as signed char
    cmd.forwardmove = (*demo_p) as i8;
    demo_p = demo_p.add(1);

    // sidemove: byte reinterpreted as signed char
    cmd.sidemove = (*demo_p) as i8;
    demo_p = demo_p.add(1);

    if longtics != 0 {
        // Little-endian 16-bit value; each byte read as unsigned
        let lo = *demo_p;
        demo_p = demo_p.add(1);
        let hi = *demo_p;
        demo_p = demo_p.add(1);
        // Matches C: angleturn = lo; angleturn |= hi << 8;
        cmd.angleturn = (lo as i16) | ((hi as i16) << 8);
    } else {
        // Non-longtics: one byte read as UNSIGNED, shifted left 8
        // C: cmd->angleturn = ((unsigned char)*demo_p++) << 8
        let byte = *demo_p;
        demo_p = demo_p.add(1);
        cmd.angleturn = ((byte as u32) << 8) as i16;
    }

    // buttons: unsigned byte
    cmd.buttons = *demo_p;
    demo_p = demo_p.add(1);
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
    let current_length = demoend.offset_from(demobuffer) as c_int;
    let new_length = current_length * 2;
    let new_demobuffer = Z_Malloc(new_length, PU_STATIC, ptr::null_mut()) as *mut byte;
    let new_demop = new_demobuffer.add(demo_p.offset_from(demobuffer) as usize);
    std::ptr::copy_nonoverlapping(demobuffer, new_demobuffer, current_length as usize);
    Z_Free(demobuffer as *mut c_void);
    demobuffer = new_demobuffer;
    demo_p = new_demop;
    demoend = demobuffer.add(new_length as usize);
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
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_WriteDemoTiccmd(cmd: *mut TiccmdT) {
    if GAMEKEYDOWN[key_demo_quit as usize] != 0 {
        G_CheckDemoStatus();
    }

    let cmd = &*cmd;
    let demo_start = demo_p;

    *demo_p = cmd.forwardmove as byte;
    demo_p = demo_p.add(1);
    *demo_p = cmd.sidemove as byte;
    demo_p = demo_p.add(1);

    if longtics != 0 {
        *demo_p = (cmd.angleturn & 0xff) as byte;
        demo_p = demo_p.add(1);
        *demo_p = ((cmd.angleturn >> 8) & 0xff) as byte;
        demo_p = demo_p.add(1);
    } else {
        *demo_p = (cmd.angleturn >> 8) as byte;
        demo_p = demo_p.add(1);
    }

    *demo_p = cmd.buttons;
    demo_p = demo_p.add(1);

    // Reset demo pointer — write then re-read to validate consistency
    demo_p = demo_start;

    if demo_p > demoend.sub(16) {
        if vanilla_demo_limit != 0 {
            G_CheckDemoStatus();
            return;
        } else {
            increase_demo_buffer();
        }
    }

    G_ReadDemoTiccmd(cmd as *const TiccmdT as *mut TiccmdT);
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
/// `name` must be a valid NUL-terminated C string. Exported as
/// `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_RecordDemo(name: *mut c_char) {
    usergame = 0;
    let name_len = libc::strlen(name);
    let demoname_size = name_len + 5;
    demoname = Z_Malloc(demoname_size as c_int, PU_STATIC, ptr::null_mut()) as *mut c_char;
    M_snprintf_clamp(
        demoname,
        demoname_size,
        snprintf(demoname, demoname_size, c"%s.lmp".as_ptr(), name),
    );
    let mut maxsize: c_int = 0x20000;
    let i = M_CheckParmWithArgs(c"-maxdemo".as_ptr().cast_mut(), 1);
    if i != 0 {
        maxsize = libc::atoi(*myargv.add(i as usize + 1) as *const libc::c_char);
        maxsize *= 1024;
    }
    demobuffer = Z_Malloc(maxsize, PU_STATIC, ptr::null_mut()) as *mut byte;
    demoend = demobuffer.add(maxsize as usize);
    demorecording = 1;
}

/// Return the single-byte demo version code corresponding to the active
/// [`gameversion`] (e.g. 109 for v1.9 and every later vanilla variant).
///
/// Raises `I_Error` for v1.2, which never had a demo version code.
///
/// # Safety
/// Reads the `gameversion` global. Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_VanillaVersionCode() -> c_int {
    g_vanilla_version_code_for(gameversion)
}

/// Pure helper for [`G_VanillaVersionCode`]: maps a `gameversion` value to
/// its 1-byte demo header code without touching globals, so unit tests can
/// exercise the table directly.
fn g_vanilla_version_code_for(gv: c_int) -> c_int {
    match gv {
        v if v == exe_doom_1_2 => unsafe {
            I_Error(c"Doom 1.2 does not have a version code!".as_ptr())
        },
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
/// `DOOM_191_VERSION` marker and disables [`lowres_turn`], so each tic
/// stores `angleturn` in 2 bytes instead of 1.
///
/// # Safety
/// Mutates `longtics`, `lowres_turn`, the demo cursor and the demo buffer.
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_BeginRecording() {
    use crate::doom::c_ffi::DOOM_191_VERSION;

    longtics = (M_CheckParm(c"-longtics".as_ptr().cast_mut()) != 0) as boolean;
    lowres_turn = (longtics == 0) as boolean;

    demo_p = demobuffer;

    if longtics != 0 {
        *demo_p = DOOM_191_VERSION as byte;
    } else {
        *demo_p = G_VanillaVersionCode() as byte;
    }
    demo_p = demo_p.add(1);

    *demo_p = gameskill as byte;
    demo_p = demo_p.add(1);
    *demo_p = gameepisode as byte;
    demo_p = demo_p.add(1);
    *demo_p = gamemap as byte;
    demo_p = demo_p.add(1);
    *demo_p = deathmatch as byte;
    demo_p = demo_p.add(1);
    *demo_p = respawnparm as byte;
    demo_p = demo_p.add(1);
    *demo_p = fastparm as byte;
    demo_p = demo_p.add(1);
    *demo_p = nomonsters as byte;
    demo_p = demo_p.add(1);
    *demo_p = consoleplayer as byte;
    demo_p = demo_p.add(1);

    for i in 0..MAXPLAYERS {
        *demo_p = playeringame[i] as byte;
        demo_p = demo_p.add(1);
    }
}

// ---------------------------------------------------------------------------
// G_DeferedPlayDemo / DemoVersionDescription / G_DoPlayDemo
// ---------------------------------------------------------------------------

/// Defer demo playback for `name`: latch `defdemoname` and queue
/// `ga_playdemo` for the next `G_Ticker` pass.
///
/// # Safety
/// `name` must point to a NUL-terminated C string that lives at least until
/// `G_DoPlayDemo` runs. Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_DeferedPlayDemo(name: *const c_char) {
    defdemoname = name as *mut c_char;
    gameaction = ga_playdemo;
}

/// Map a demo version byte to a human-readable engine identifier
/// (`"v1.9"`, `"v1.6/v1.666"`, etc.).
///
/// Unknown values in the range 0..=4 are treated as pre-v1.4 IWAD demos and
/// labelled `"v1.0/v1.1/v1.2"`. Anything else is formatted into the
/// `DEMOVERSIONBUF` static as `"major.minor (unknown)"` and a pointer into
/// that buffer is returned (mirroring the C function's use of a static
/// `resultbuf`).
///
/// # Safety
/// Writes the `DEMOVERSIONBUF` static; the returned pointer is invalidated
/// by the next call.
unsafe fn demo_version_description(version: c_int) -> *const c_char {
    match version {
        104 => c"v1.4".as_ptr(),
        105 => c"v1.5".as_ptr(),
        106 => c"v1.6/v1.666".as_ptr(),
        107 => c"v1.7/v1.7a".as_ptr(),
        108 => c"v1.8".as_ptr(),
        109 => c"v1.9".as_ptr(),
        _ => {
            if (0..=4).contains(&version) {
                c"v1.0/v1.1/v1.2".as_ptr()
            } else {
                M_snprintf_clamp(
                    std::ptr::addr_of_mut!(DEMOVERSIONBUF[0]),
                    16,
                    snprintf(
                        std::ptr::addr_of_mut!(DEMOVERSIONBUF[0]),
                        16,
                        c"%i.%i (unknown)".as_ptr(),
                        version / 100,
                        version % 100,
                    ),
                );
                std::ptr::addr_of!(DEMOVERSIONBUF[0])
            }
        }
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
/// Mutates global engine state extensively. Exported as `#[no_mangle]` for
/// C callers.
#[no_mangle]
pub unsafe extern "C" fn G_DoPlayDemo() {
    use crate::doom::c_ffi::DOOM_191_VERSION;

    gameaction = ga_nothing;
    demobuffer = W_CacheLumpName(defdemoname, PU_STATIC) as *mut byte;
    demo_p = demobuffer;

    let demoversion = *demo_p as c_int;
    demo_p = demo_p.add(1);

    if demoversion == G_VanillaVersionCode() {
        longtics = 0;
    } else if demoversion == DOOM_191_VERSION {
        longtics = 1;
    } else {
        let message = b"Demo is from a different game version!\n\
            (read %i, should be %i)\n\n\
            *** You may need to upgrade your version of Doom to v1.9. ***\n\
            See: https://www.doomworld.com/classicdoom/info/patches.php\n\
            This appears to be %s.\0";
        // C code uses printf (not I_Error) here so demo playback continues
        libc::printf(
            message.as_ptr() as *const libc::c_char,
            demoversion,
            G_VanillaVersionCode(),
            demo_version_description(demoversion),
        );
    }

    let skill = *demo_p as skill_t;
    demo_p = demo_p.add(1);
    let episode = *demo_p as c_int;
    demo_p = demo_p.add(1);
    let map = *demo_p as c_int;
    demo_p = demo_p.add(1);
    deathmatch = *demo_p as c_int;
    demo_p = demo_p.add(1);
    respawnparm = *demo_p as c_int;
    demo_p = demo_p.add(1);
    fastparm = *demo_p as c_int;
    demo_p = demo_p.add(1);
    nomonsters = *demo_p as c_int;
    demo_p = demo_p.add(1);
    consoleplayer = *demo_p as c_int;
    demo_p = demo_p.add(1);
    if consoleplayer < 0 || consoleplayer >= MAXPLAYERS as c_int {
        I_Error(
            c"G_DoPlayDemo: consoleplayer %d out of range\n".as_ptr(),
            consoleplayer,
        );
    }

    for i in 0..MAXPLAYERS {
        playeringame[i] = *demo_p as boolean;
        demo_p = demo_p.add(1);
    }

    if playeringame[1] != 0
        || M_CheckParm(c"-solo-net".as_ptr().cast_mut()) > 0
        || M_CheckParm(c"-netdemo".as_ptr().cast_mut()) > 0
    {
        netgame = 1;
        netdemo = 1;
    }

    precache = 0;
    G_InitNew(skill, episode, map);
    precache = 1;
    starttime = I_GetTime();

    usergame = 0;
    demoplayback = 1;
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
/// Exported as `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_TimeDemo(name: *mut c_char) {
    nodrawers = M_CheckParm(c"-nodraw".as_ptr().cast_mut());
    timingdemo = 1;
    use crate::doom::d_loop::singletics;
    singletics = 1;
    defdemoname = name;
    gameaction = ga_playdemo;
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
/// Mutates demo / playback globals and performs file I/O. Exported as
/// `#[no_mangle]` for C callers.
#[no_mangle]
pub unsafe extern "C" fn G_CheckDemoStatus() -> boolean {
    if timingdemo != 0 {
        let endtime = I_GetTime();
        let realtics = endtime - starttime;
        let fps = (gametic as f32 * 35.0) / realtics as f32;
        timingdemo = 0;
        demoplayback = 0;
        I_Error(
            c"timed %i gametics in %i realtics (%f fps)".as_ptr(),
            gametic,
            realtics,
            fps as f64,
        );
    }

    if demoplayback != 0 {
        W_ReleaseLumpName(defdemoname);
        demoplayback = 0;
        netdemo = 0;
        netgame = 0;
        deathmatch = 0;
        playeringame[1] = 0;
        playeringame[2] = 0;
        playeringame[3] = 0;
        respawnparm = 0;
        fastparm = 0;
        nomonsters = 0;
        consoleplayer = 0;

        if singledemo != 0 {
            I_Quit();
        } else {
            D_AdvanceDemo();
        }
        return 1;
    }

    if demorecording != 0 {
        *demo_p = DEMOMARKER;
        demo_p = demo_p.add(1);
        M_WriteFile(
            demoname,
            demobuffer as *mut c_void,
            demo_p.offset_from(demobuffer) as c_int,
        );
        Z_Free(demobuffer as *mut c_void);
        demorecording = 0;
        I_Error(c"Demo %s recorded".as_ptr(), demoname);
    }

    0
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

/// Unit-test suite covering ticcmd checksumming, demo I/O byte layout and
/// version-code lookup. The demo tests exercise `read_demo_ticcmd_inner`
/// directly so they avoid mutating the global `demo_p` cursor and the
/// `DEMOMARKER` end-of-stream shortcut.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::doom::d_mode::exe_doom_1_9;
    use crate::doom::d_player::TiccmdT;

    /// Helper returning a freshly zeroed [`TiccmdT`] for test assembly.
    fn zeroed_cmd() -> TiccmdT {
        unsafe { std::mem::zeroed() }
    }

    // --- G_CmdChecksum ---

    /// A fully-zero ticcmd checksums to zero.
    #[test]
    fn cmdchecksum_all_zeros_is_zero() {
        let cmd = zeroed_cmd();
        unsafe {
            assert_eq!(G_CmdChecksum(&cmd as *const TiccmdT), 0);
        }
    }

    /// `forwardmove = 1` produces a single bit in the first 4-byte word.
    #[test]
    fn cmdchecksum_forwardmove_one() {
        // sizeof(TiccmdT) = 16 bytes = 4 ints; loop runs for 3 ints.
        // First int = bytes [forwardmove(i8), sidemove(i8), angleturn_lo(u8), angleturn_hi(u8)]
        // With forwardmove=1, rest zero: first int = 0x00_00_00_01 = 1 (little-endian).
        let mut cmd = zeroed_cmd();
        cmd.forwardmove = 1;
        unsafe {
            assert_eq!(G_CmdChecksum(&cmd as *const TiccmdT), 1);
        }
    }

    /// The tail-word `lookfly` / `arti` / `_pad` fields are excluded from the sum.
    #[test]
    fn cmdchecksum_excludes_last_int() {
        // G_CmdChecksum sums sizeof(ticcmd_t)/4 - 1 = 3 words (bytes 0-11).
        // The last word (bytes 12-15: lookfly + arti + _pad) is NOT summed.
        let mut cmd = zeroed_cmd();
        cmd.lookfly = 0x12;
        cmd.arti = 0x34;
        unsafe {
            assert_eq!(G_CmdChecksum(&cmd as *const TiccmdT), 0);
        }
    }

    /// `inventory` sits in word 2 and is included in the checksum.
    #[test]
    fn cmdchecksum_includes_inventory() {
        // inventory is at bytes 8-11 (word 2), which IS included in the sum.
        let mut cmd = zeroed_cmd();
        cmd.inventory = 1;
        unsafe {
            assert_eq!(G_CmdChecksum(&cmd as *const TiccmdT), 1);
        }
    }

    // --- G_ReadDemoTiccmd (non-longtics) ---

    /// Test that G_ReadDemoTiccmd reads the non-longtics format correctly.
    /// Byte layout: [forwardmove, sidemove, angleturn_byte, buttons]
    #[test]
    fn read_demo_ticcmd_non_longtics_basic() {
        unsafe {
            let buf: [u8; 4] = [0x10, 0x20, 0x30, 0x01];
            demo_p = buf.as_ptr() as *mut u8;
            longtics = 0;
            let mut cmd = zeroed_cmd();
            // Don't call G_ReadDemoTiccmd (it calls G_CheckDemoStatus on DEMOMARKER),
            // instead test the inner read logic directly.
            read_demo_ticcmd_inner(&mut demo_p, &mut cmd, false);
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
        unsafe {
            assert_eq!(forwardmove[0], 0x19);
        }
    }

    /// `forwardmove[1]` (fast) baseline of 0x32 - also the turbo threshold.
    #[test]
    fn forwardmove_fast_is_0x32() {
        unsafe {
            assert_eq!(forwardmove[1], 0x32);
        }
    }

    /// `sidemove[0]` (slow) baseline of 0x18.
    #[test]
    fn sidemove_slow_is_0x18() {
        unsafe {
            assert_eq!(sidemove[0], 0x18);
        }
    }

    /// `sidemove[1]` (fast) baseline of 0x28.
    #[test]
    fn sidemove_fast_is_0x28() {
        unsafe {
            assert_eq!(sidemove[1], 0x28);
        }
    }

    /// `angleturn[]` baseline values (normal / fast / slow).
    #[test]
    fn angleturn_values() {
        unsafe {
            assert_eq!(angleturn[0], 640);
            assert_eq!(angleturn[1], 1280);
            assert_eq!(angleturn[2], 320);
        }
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
