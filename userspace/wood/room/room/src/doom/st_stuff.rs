//! Rust port of vendor/doomgeneric/st_stuff.c.
//!
//! Status bar logic: health, ammo, armor, keys, face widget, palette effects,
//! and cheat-code handling. The 32-pixel-tall bar at the bottom of the screen
//! is composed of widget primitives from `st_lib.rs`. Palette cycling (damage
//! flash, berserk, radiation suit) is driven here by `I_SetPalette`.
//!
//! Notable Rust-vs-C differences:
//! - Cheat-sequence tables are built at compile time with `const fn` helpers
//!   instead of being initialised by `ST_Start`.
//! - `DEH_String` is an identity shim; Dehacked string replacement is not yet
//!   wired up.
//! - `logical_gamemission` is duplicated here (it also lives in `st_stuff.c`
//!   as a macro) to keep the file self-contained.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::{c_char, c_int};
use std::ptr;

use crate::c_write;
use crate::doom::am_map::automapactive;
use crate::doom::d_event::event_t;
use crate::doom::d_items::weaponinfo;
use crate::doom::d_mode;
use crate::doom::d_player::{PlayerT, MAXPLAYERS, NUMAMMO, NUMCARDS, NUMWEAPONS};
use crate::doom::doomstat::{gamemission, gamemode, gameversion};
use crate::doom::g_game::G_DeferedInitNew;
use crate::doom::g_game::{consoleplayer, deathmatch, gameskill, netgame, players};
use crate::doom::i_timer::TICRATE;
use crate::doom::i_video::I_SetPalette;
use crate::doom::m_cheat::{cheatseq_t, cht_CheckCheat, cht_GetParam};
use crate::doom::m_random::M_Random;
use crate::doom::p_inter::P_GivePower;
use crate::doom::p_telept::mobj_t;
use crate::doom::r_main::R_PointToAngle2;
use crate::doom::s_sound::S_ChangeMusic;
use crate::doom::st_lib::{
    st_binicon_t, st_multicon_t, st_number_t, st_percent_t, STlib_init, STlib_initBinIcon,
    STlib_initMultIcon, STlib_initNum, STlib_initPercent, STlib_updateBinIcon,
    STlib_updateMultIcon, STlib_updateNum, STlib_updatePercent,
};
use crate::doom::tables::{ANG180, ANG45};
use crate::doom::v_video::patch_t;
use crate::doom::v_video::{V_CopyRect, V_DrawPatch, V_RestoreBuffer, V_UseBuffer};
use crate::doom::w_wad::{W_CacheLumpName, W_CacheLumpNum, W_GetNumForName, W_ReleaseLumpName};
use crate::doom::z_zone::{Z_Malloc, PU_CACHE, PU_STATIC};
use crate::types::Boolean;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Height of the status bar in screen pixels (matches C `ST_HEIGHT`).
const ST_HEIGHT: c_int = 32;
/// Width of the status bar in screen pixels (matches C `ST_WIDTH`).
const ST_WIDTH: c_int = 320;
/// Left edge of the status bar in screen coordinates (matches C `ST_X`).
const ST_X: c_int = 0;
/// Top edge of the status bar in screen coordinates (matches C `ST_Y`).
const ST_Y: c_int = 200 - ST_HEIGHT;

/// X pixel position of the face widget background (matches C `ST_FX`).
const ST_FX: c_int = 143;
/// Y pixel position of the face widget background (matches C `ST_FY`).
const ST_FY: c_int = 169;

/// Number of pain-level face rows (0 = healthy, 4 = critical; matches C `ST_NUMPAINFACES`).
const ST_NUMPAINFACES: c_int = 5;
/// Straight-ahead frames per pain row (matches C `ST_NUMSTRAIGHTFACES`).
const ST_NUMSTRAIGHTFACES: c_int = 3;
/// Turn-direction frames per pain row (matches C `ST_NUMTURNFACES`).
const ST_NUMTURNFACES: c_int = 2;
/// Special-expression frames per pain row: ouch, evil-grin, rampage (matches C `ST_NUMSPECIALFACES`).
const ST_NUMSPECIALFACES: c_int = 3;

/// Total frames per pain-level row (matches C `ST_FACESTRIDE`).
const ST_FACESTRIDE: c_int = ST_NUMSTRAIGHTFACES + ST_NUMTURNFACES + ST_NUMSPECIALFACES;

/// Number of frames appended after all pain rows: god-mode and dead (matches C `ST_NUMEXTRAFACES`).
const ST_NUMEXTRAFACES: c_int = 2;
/// Total face patch count loaded from the WAD (matches C `ST_NUMFACES`).
const ST_NUMFACES: c_int = ST_FACESTRIDE * ST_NUMPAINFACES + ST_NUMEXTRAFACES;

/// Offset within a pain-row to the first turn face (matches C `ST_TURNOFFSET`).
const ST_TURNOFFSET: c_int = ST_NUMSTRAIGHTFACES;
/// Offset within a pain-row to the ouch face (matches C `ST_OUCHOFFSET`).
const ST_OUCHOFFSET: c_int = ST_TURNOFFSET + ST_NUMTURNFACES;
/// Offset within a pain-row to the evil-grin face (matches C `ST_EVILGRINOFFSET`).
const ST_EVILGRINOFFSET: c_int = ST_OUCHOFFSET + 1;
/// Offset within a pain-row to the rampage face (matches C `ST_RAMPAGEOFFSET`).
const ST_RAMPAGEOFFSET: c_int = ST_EVILGRINOFFSET + 1;
/// Index of the god-mode face in the flat `faces` array (matches C `ST_GODFACE`).
const ST_GODFACE: c_int = ST_NUMPAINFACES * ST_FACESTRIDE;
/// Index of the dead face in the flat `faces` array (matches C `ST_DEADFACE`).
const ST_DEADFACE: c_int = ST_GODFACE + 1;

/// X screen coordinate of the face widget (matches C `ST_FACESX`).
const ST_FACESX: c_int = 143;
/// Y screen coordinate of the face widget (matches C `ST_FACESY`).
const ST_FACESY: c_int = 168;

/// Duration of the evil-grin expression in tics (matches C `ST_EVILGRINCOUNT`).
const ST_EVILGRINCOUNT: c_int = 2 * TICRATE;
/// Duration of a straight-ahead expression in tics (matches C `ST_STRAIGHTFACECOUNT`).
const ST_STRAIGHTFACECOUNT: c_int = TICRATE / 2;
/// Duration of turn and ouch expressions in tics (matches C `ST_TURNCOUNT`).
const ST_TURNCOUNT: c_int = TICRATE;
/// Duration of the ouch expression in tics (matches C `ST_OUCHCOUNT`).
const ST_OUCHCOUNT: c_int = TICRATE;
/// Tics of continuous fire before the rampage face appears (matches C `ST_RAMPAGEDELAY`).
const ST_RAMPAGEDELAY: c_int = 2 * TICRATE;

/// Health-point drop threshold that triggers the ouch face (matches C `ST_MUCHPAIN`).
const ST_MUCHPAIN: c_int = 20;

/// Digit width of the ready-ammo number widget (matches C `ST_AMMOWIDTH`).
const ST_AMMOWIDTH: c_int = 3;
/// X coordinate of the ready-ammo display (matches C `ST_AMMOX`).
const ST_AMMOX: c_int = 44;
/// Y coordinate of the ready-ammo display (matches C `ST_AMMOY`).
const ST_AMMOY: c_int = 171;

/// Digit width of the health display (matches C `ST_HEALTHWIDTH`).
const ST_HEALTHWIDTH: c_int = 3;
/// X coordinate of the health display (matches C `ST_HEALTHX`).
const ST_HEALTHX: c_int = 90;
/// Y coordinate of the health display (matches C `ST_HEALTHY`).
const ST_HEALTHY: c_int = 171;

/// X coordinate of the weapons-owned grid (matches C `ST_ARMSX`).
const ST_ARMSX: c_int = 111;
/// Y coordinate of the weapons-owned grid (matches C `ST_ARMSY`).
const ST_ARMSY: c_int = 172;
/// X coordinate of the arms background patch (matches C `ST_ARMSBGX`).
const ST_ARMSBGX: c_int = 104;
/// Y coordinate of the arms background patch (matches C `ST_ARMSBGY`).
const ST_ARMSBGY: c_int = 168;
/// Horizontal spacing between arms-grid cells in pixels (matches C `ST_ARMSXSPACE`).
const ST_ARMSXSPACE: c_int = 12;
/// Vertical spacing between arms-grid cells in pixels (matches C `ST_ARMSYSPACE`).
const ST_ARMSYSPACE: c_int = 10;

/// X coordinate of the frag counter in deathmatch (matches C `ST_FRAGSX`).
const ST_FRAGSX: c_int = 138;
/// Y coordinate of the frag counter in deathmatch (matches C `ST_FRAGSY`).
const ST_FRAGSY: c_int = 171;
/// Digit width of the frag counter (matches C `ST_FRAGSWIDTH`).
const ST_FRAGSWIDTH: c_int = 2;

/// Digit width of the armor display (matches C `ST_ARMORWIDTH`).
const ST_ARMORWIDTH: c_int = 3;
/// X coordinate of the armor display (matches C `ST_ARMORX`).
const ST_ARMORX: c_int = 221;
/// Y coordinate of the armor display (matches C `ST_ARMORY`).
const ST_ARMORY: c_int = 171;

/// X coordinate of the blue key slot (matches C `ST_KEY0X`).
const ST_KEY0X: c_int = 239;
/// Y coordinate of the blue key slot (matches C `ST_KEY0Y`).
const ST_KEY0Y: c_int = 171;
/// X coordinate of the yellow key slot (matches C `ST_KEY1X`).
const ST_KEY1X: c_int = 239;
/// Y coordinate of the yellow key slot (matches C `ST_KEY1Y`).
const ST_KEY1Y: c_int = 181;
/// X coordinate of the red key slot (matches C `ST_KEY2X`).
const ST_KEY2X: c_int = 239;
/// Y coordinate of the red key slot (matches C `ST_KEY2Y`).
const ST_KEY2Y: c_int = 191;

/// Digit width of per-ammo-type current-ammo displays (matches C `ST_AMMO0WIDTH`).
const ST_AMMO0WIDTH: c_int = 3;
/// X coordinate of bullets current-ammo display (matches C `ST_AMMO0X`).
const ST_AMMO0X: c_int = 288;
/// Y coordinate of bullets current-ammo display (matches C `ST_AMMO0Y`).
const ST_AMMO0Y: c_int = 173;
/// X coordinate of shells current-ammo display (matches C `ST_AMMO1X`).
const ST_AMMO1X: c_int = 288;
/// Y coordinate of shells current-ammo display (matches C `ST_AMMO1Y`).
const ST_AMMO1Y: c_int = 179;
/// X coordinate of cells current-ammo display (matches C `ST_AMMO2X`).
const ST_AMMO2X: c_int = 288;
/// Y coordinate of cells current-ammo display (matches C `ST_AMMO2Y`).
const ST_AMMO2Y: c_int = 191;
/// X coordinate of rockets current-ammo display (matches C `ST_AMMO3X`).
const ST_AMMO3X: c_int = 288;
/// Y coordinate of rockets current-ammo display (matches C `ST_AMMO3Y`).
const ST_AMMO3Y: c_int = 185;

/// Digit width of per-ammo-type max-ammo displays (matches C `ST_MAXAMMO0WIDTH`).
const ST_MAXAMMO0WIDTH: c_int = 3;
/// X coordinate of bullets max-ammo display (matches C `ST_MAXAMMO0X`).
const ST_MAXAMMO0X: c_int = 314;
/// Y coordinate of bullets max-ammo display (matches C `ST_MAXAMMO0Y`).
const ST_MAXAMMO0Y: c_int = 173;
/// X coordinate of shells max-ammo display (matches C `ST_MAXAMMO1X`).
const ST_MAXAMMO1X: c_int = 314;
/// Y coordinate of shells max-ammo display (matches C `ST_MAXAMMO1Y`).
const ST_MAXAMMO1Y: c_int = 179;
/// X coordinate of cells max-ammo display (matches C `ST_MAXAMMO2X`).
const ST_MAXAMMO2X: c_int = 314;
/// Y coordinate of cells max-ammo display (matches C `ST_MAXAMMO2Y`).
const ST_MAXAMMO2Y: c_int = 191;
/// X coordinate of rockets max-ammo display (matches C `ST_MAXAMMO3X`).
const ST_MAXAMMO3X: c_int = 314;
/// Y coordinate of rockets max-ammo display (matches C `ST_MAXAMMO3Y`).
const ST_MAXAMMO3Y: c_int = 185;

/// Header magic for automap messages sent via the event system (matches C `AM_MSGHEADER`).
const AM_MSGHEADER: c_int = (('a' as c_int) << 24) + (('m' as c_int) << 16);
/// Event data value signalling that the automap was opened (matches C `AM_MSGENTERED`).
const AM_MSGENTERED: c_int = AM_MSGHEADER | (('e' as c_int) << 8);
/// Event data value signalling that the automap was closed (matches C `AM_MSGEXITED`).
const AM_MSGEXITED: c_int = AM_MSGHEADER | (('x' as c_int) << 8);

/// Index of the first red-damage palette in `PLAYPAL` (matches C `STARTREDPALS`).
const STARTREDPALS: c_int = 1;
/// Index of the first bonus-pickup palette in `PLAYPAL` (matches C `STARTBONUSPALS`).
const STARTBONUSPALS: c_int = 9;
/// Number of red-damage palette entries (matches C `NUMREDPALS`).
const NUMREDPALS: c_int = 8;
/// Number of bonus-pickup palette entries (matches C `NUMBONUSPALS`).
const NUMBONUSPALS: c_int = 4;
/// Index of the radiation-suit palette in `PLAYPAL` (matches C `RADIATIONPAL`).
const RADIATIONPAL: c_int = 13;

// ---------------------------------------------------------------------------
// String literals (from d_englsh.h)
// ---------------------------------------------------------------------------

/// Message shown when god mode is activated (C `STSTR_DQDON` from `d_englsh.h`).
const STSTR_DQDON: *mut c_char = c"Degreelessness Mode On".as_ptr().cast_mut();
/// Message shown when god mode is deactivated (C `STSTR_DQDOFF` from `d_englsh.h`).
const STSTR_DQDOFF: *mut c_char = c"Degreelessness Mode Off".as_ptr().cast_mut();
/// Message shown when `idfa` (ammo, no keys) cheat fires (C `STSTR_FAADDED`).
const STSTR_FAADDED: *mut c_char = c"Ammo (no keys) Added".as_ptr().cast_mut();
/// Message shown when `idkfa` (ammo + keys) cheat fires (C `STSTR_KFAADDED`).
const STSTR_KFAADDED: *mut c_char = c"Very Happy Ammo Added".as_ptr().cast_mut();
/// Message shown when a music-change cheat fires (C `STSTR_MUS`).
const STSTR_MUS: *mut c_char = c"Music Change".as_ptr().cast_mut();
/// Message shown when an invalid music number is entered (C `STSTR_NOMUS`).
const STSTR_NOMUS: *mut c_char = c"IMPOSSIBLE SELECTION".as_ptr().cast_mut();
/// Message shown when no-clip mode is activated (C `STSTR_NCON`).
const STSTR_NCON: *mut c_char = c"No Clipping Mode ON".as_ptr().cast_mut();
/// Message shown when no-clip mode is deactivated (C `STSTR_NCOFF`).
const STSTR_NCOFF: *mut c_char = c"No Clipping Mode OFF".as_ptr().cast_mut();
/// Prompt shown when the `idbehold` cheat prefix fires (C `STSTR_BEHOLD`).
const STSTR_BEHOLD: *mut c_char = c"inVuln, Str, Inviso, Rad, Allmap, or Lite-amp"
    .as_ptr()
    .cast_mut();
/// Message shown when a `idbeholdX` power-up cheat fires (C `STSTR_BEHOLDX`).
const STSTR_BEHOLDX: *mut c_char = c"Power-up Toggled".as_ptr().cast_mut();
/// Message shown when the `idchoppers` cheat fires (C `STSTR_CHOPPERS`).
const STSTR_CHOPPERS: *mut c_char = c"... doesn't suck - GM".as_ptr().cast_mut();
/// Message shown when the level-change cheat (`idclev`) fires (C `STSTR_CLEV`).
const STSTR_CLEV: *mut c_char = c"Changing Level...".as_ptr().cast_mut();

// ---------------------------------------------------------------------------
// DEH_String shim — identity when dehacked is disabled.
// ---------------------------------------------------------------------------

/// Pass-through shim for Dehacked string replacement.
///
/// In the C codebase, `DEH_String` may substitute a string that was patched by
/// a `.deh` file. This port does not yet support Dehacked, so the function
/// returns its argument unchanged.
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

// ---------------------------------------------------------------------------
// Replicate the C `logical_gamemission` macro from `doomstat.h`.
// ---------------------------------------------------------------------------

/// Return the canonical game mission, collapsing Chex Quest and HacX aliases.
///
/// Mirrors the `logical_gamemission` macro from `doomstat.h`:
/// - `pack_chex` maps to `doom`.
/// - `pack_hacx` maps to `doom2`.
/// - All other values are returned as-is.
///
/// # Safety
///
/// Reads the global `gamemission` static. Caller must ensure `D_DoomMain` has
/// initialised the game-mode globals before calling.
unsafe fn logical_gamemission() -> c_int {
    if gamemission == d_mode::pack_chex {
        d_mode::doom
    } else if gamemission == d_mode::pack_hacx {
        d_mode::doom2
    } else {
        gamemission
    }
}

// ---------------------------------------------------------------------------
// Cheat helpers
// ---------------------------------------------------------------------------

/// Copy a byte slice into a 25-element `c_char` array, padding the remainder with zeros.
///
/// Used at compile time to initialise cheat-sequence buffers inside `cheatseq_t`.
const fn make_cheat_seq(seq: &[u8]) -> [c_char; 25] {
    let mut arr = [0i8; 25];
    let mut i = 0;
    while i < seq.len() {
        arr[i] = seq[i] as c_char;
        i += 1;
    }
    arr
}

/// Construct a `cheatseq_t` from a key sequence and a parameter character count.
///
/// All runtime state fields (`chars_read`, `param_chars_read`, `parameter_buf`) are
/// zeroed; they will be updated by `cht_CheckCheat` during gameplay.
const fn cheat(seq: &[u8], params: c_int) -> cheatseq_t {
    cheatseq_t {
        sequence: make_cheat_seq(seq),
        sequence_len: seq.len(),
        parameter_chars: params,
        chars_read: 0,
        param_chars_read: 0,
        parameter_buf: [0; 5],
    }
}

// ---------------------------------------------------------------------------
// Exported globals
// ---------------------------------------------------------------------------

/// Backing pixel buffer for the status bar, allocated in `ST_Init`.
///
/// Exported as `st_backing_screen` for C callers. The buffer is `ST_WIDTH *
/// ST_HEIGHT` bytes and is used by `ST_refreshBackground` to save and restore
/// the pixels beneath the bar.
#[no_mangle]
pub static mut st_backing_screen: *mut u8 = ptr::null_mut();

/// Cheat sequence for changing the playing music track (`idmus##`).
///
/// Exported for C linkage. Two parameter characters encode the track number.
#[no_mangle]
pub static mut cheat_mus: cheatseq_t = cheat(b"idmus", 2);

/// Cheat sequence for toggling god mode (`iddqd`).
///
/// Exported for C linkage.
#[no_mangle]
pub static mut cheat_god: cheatseq_t = cheat(b"iddqd", 0);

/// Cheat sequence for full ammo and armor, including all keys (`idkfa`).
///
/// Exported for C linkage.
#[no_mangle]
pub static mut cheat_ammo: cheatseq_t = cheat(b"idkfa", 0);

/// Cheat sequence for full ammo and armor, without keys (`idfa`).
///
/// Exported for C linkage.
#[no_mangle]
pub static mut cheat_ammonokey: cheatseq_t = cheat(b"idfa", 0);

/// No-clip cheat sequence for Doom episode maps (`idspispopd`).
///
/// Exported for C linkage. Only active when `logical_gamemission() == doom`.
#[no_mangle]
pub static mut cheat_noclip: cheatseq_t = cheat(b"idspispopd", 0);

/// No-clip cheat sequence for Doom II maps (`idclip`).
///
/// Exported for C linkage. Active when `logical_gamemission() != doom`.
#[no_mangle]
pub static mut cheat_commercial_noclip: cheatseq_t = cheat(b"idclip", 0);

/// Power-up cheat sequences (`idbeholdv/s/i/r/a/l` and the bare `idbehold` prefix).
///
/// Indices 0-5 correspond to the six togglable power-ups
/// (invulnerability, berserk, invisibility, radiation suit, automap, light amp).
/// Index 6 is the bare `idbehold` prefix which displays the prompt.
/// Exported for C linkage.
#[no_mangle]
pub static mut cheat_powerup: [cheatseq_t; 7] = [
    cheat(b"idbeholdv", 0),
    cheat(b"idbeholds", 0),
    cheat(b"idbeholdi", 0),
    cheat(b"idbeholdr", 0),
    cheat(b"idbeholda", 0),
    cheat(b"idbeholdl", 0),
    cheat(b"idbehold", 0),
];

/// Cheat sequence that gives the chainsaw and invulnerability (`idchoppers`).
///
/// Exported for C linkage.
#[no_mangle]
pub static mut cheat_choppers: cheatseq_t = cheat(b"idchoppers", 0);

/// Cheat sequence for warping to a specific episode+map (`idclev##`).
///
/// Two parameter characters encode the destination. Exported for C linkage.
#[no_mangle]
pub static mut cheat_clev: cheatseq_t = cheat(b"idclev", 2);

/// Cheat sequence that prints the player's current map position (`idmypos`).
///
/// Exported for C linkage.
#[no_mangle]
pub static mut cheat_mypos: cheatseq_t = cheat(b"idmypos", 0);

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

/// Pointer to the local player's `PlayerT` struct; set by `ST_initData`.
static mut plyr: *mut PlayerT = ptr::null_mut();

/// Non-zero when the status bar needs a full redraw on the next `ST_Drawer` call.
static mut st_firsttime: c_int = 0;

/// WAD lump number of `PLAYPAL`, cached by `ST_loadData` for palette lookups.
static mut lu_palette: c_int = 0;

/// Monotonically increasing tic counter incremented by `ST_Ticker`.
static mut st_clock: u32 = 0;

/// Countdown used to temporarily suppress chat-message override of the HUD message.
static mut st_msgcounter: c_int = 0;

/// Current chat state: 0 = `StartChatState`, used to track HUD message display mode.
static mut st_chatstate: c_int = 0; // StartChatState = 0

/// Current view state: 0 = automap active, 1 = first-person view.
static mut st_gamestate: c_int = 0; // AutomapState = 0

/// Non-zero when the status bar should be rendered (i.e. not in full-screen mode).
static mut st_statusbaron: c_int = 0;

/// Non-zero while a chat message is being composed.
static mut st_chat: c_int = 0;

/// Previous value of `st_chat`; restored after `st_msgcounter` expires.
static mut st_oldchat: c_int = 0;

/// Blink flag for the chat cursor; toggled by `st_msgcounter`.
static mut st_cursoron: c_int = 0;

/// Non-zero in cooperative (non-deathmatch) games; gates the weapons-owned display.
static mut st_notdeathmatch: c_int = 0;

/// Non-zero when the weapons-owned display should be shown.
static mut st_armson: c_int = 0;

/// Non-zero when the frag counter should be shown (deathmatch only).
static mut st_fragson: c_int = 0;

/// Pointer to the `STBAR` background patch.
static mut sbar: *mut patch_t = ptr::null_mut();

/// Tall digit patches 0-9 (`STTNUM0`-`STTNUM9`), used for health, armor, and ammo.
static mut tallnum: [*mut patch_t; 10] = [ptr::null_mut(); 10];

/// Tall percent-sign patch (`STTPRCNT`).
static mut tallpercent: *mut patch_t = ptr::null_mut();

/// Short digit patches 0-9 (`STYSNUM0`-`STYSNUM9`), used in the per-ammo sidebar.
static mut shortnum: [*mut patch_t; 10] = [ptr::null_mut(); 10];

/// Key icon patches, one per `NUMCARDS` card type.
static mut keys: [*mut patch_t; NUMCARDS] = [ptr::null_mut(); NUMCARDS];

/// All face patches in a flat array; indexed by `st_faceindex`.
static mut faces: [*mut patch_t; ST_NUMFACES as usize] = [ptr::null_mut(); ST_NUMFACES as usize];

/// Network-game face-background patch (`STFB#` where `#` is the console player index).
static mut faceback: *mut patch_t = ptr::null_mut();

/// Arms-grid background patch (`STARMS`).
static mut armsbg: *mut patch_t = ptr::null_mut();

/// Arms patches: `arms[weapon][0]` = dim `STGNUM#`, `arms[weapon][1]` = bright short digit.
static mut arms: [[*mut patch_t; 2]; 6] = [[ptr::null_mut(); 2]; 6];

/// Ready-ammo number widget state.
static mut w_ready: st_number_t = unsafe { std::mem::zeroed() };
/// Frag-count number widget state (deathmatch only).
static mut w_frags: st_number_t = unsafe { std::mem::zeroed() };
/// Health percent widget state.
static mut w_health: st_percent_t = unsafe { std::mem::zeroed() };
/// Arms-background binary icon widget state.
static mut w_armsbg: st_binicon_t = unsafe { std::mem::zeroed() };
/// Per-weapon multi-icon widget states (weapons 2-7).
static mut w_arms: [st_multicon_t; 6] = unsafe { std::mem::zeroed() };
/// Face multi-icon widget state.
static mut w_faces: st_multicon_t = unsafe { std::mem::zeroed() };
/// Key-slot multi-icon widget states (three slots: blue, yellow, red).
static mut w_keyboxes: [st_multicon_t; 3] = unsafe { std::mem::zeroed() };
/// Armor percent widget state.
static mut w_armor: st_percent_t = unsafe { std::mem::zeroed() };
/// Per-ammo-type current-ammo number widget states.
static mut w_ammo: [st_number_t; NUMAMMO] = unsafe { std::mem::zeroed() };
/// Per-ammo-type max-ammo number widget states.
static mut w_maxammo: [st_number_t; NUMAMMO] = unsafe { std::mem::zeroed() };

/// Running frag total for the local player, updated each tic.
static mut st_fragscount: c_int = 0;

/// Player health from the previous tic; used to detect large drops for the ouch face.
static mut st_oldhealth: c_int = -1;

/// Snapshot of which weapons the player owned at the previous tic; detects new pickups.
static mut oldweaponsowned: [c_int; NUMWEAPONS] = [0; NUMWEAPONS];

/// Countdown controlling how many tics the current face expression persists.
static mut st_facecount: c_int = 0;

/// Current index into `faces[]` that the face widget displays.
static mut st_faceindex: c_int = 0;

/// Key-slot values: the card/skull index to display, or -1 for empty.
static mut keyboxes: [c_int; 3] = [0; 3];

/// Random number sampled from `M_Random` each tic to vary the idle straight-face frame.
static mut st_randomnumber: c_int = 0;

/// Last palette index passed to `I_SetPalette`; avoids redundant calls.
static mut st_palette: c_int = 0;

/// Non-zero when the status bar subsystem has been stopped via `ST_Stop`.
static mut st_stopped: c_int = 1;

// ---------------------------------------------------------------------------
// Background refresh
// ---------------------------------------------------------------------------

/// Blit the status bar background into the backing buffer, then copy it to screen.
///
/// Only runs when `st_statusbaron` is set. In a network game the face-background
/// patch (`STFB#`) is drawn on top of the bar before the copy.
///
/// # Safety
///
/// Reads the global status-bar visibility flag `st_statusbaron` and the cached
/// patches `sbar`/`faceback`, and writes to the backing screen buffer
/// `st_backing_screen`. Caller must ensure `ST_Init` has loaded the patches
/// and allocated the buffer, and that the video subsystem is ready.
#[no_mangle]
pub unsafe extern "C" fn ST_refreshBackground() {
    if st_statusbaron != 0 {
        V_UseBuffer(st_backing_screen);
        V_DrawPatch(ST_X, 0, sbar);
        if netgame != 0 {
            V_DrawPatch(ST_FX, 0, faceback);
        }
        V_RestoreBuffer();
        V_CopyRect(ST_X, 0, st_backing_screen, ST_WIDTH, ST_HEIGHT, ST_X, ST_Y);
    }
}

// ---------------------------------------------------------------------------
// Cheat responder
// ---------------------------------------------------------------------------

/// Handle status-bar-related input events: automap state changes and cheat codes.
///
/// Returns 1 if the event was consumed, 0 otherwise. The C original
/// (`ST_Responder` in `st_stuff.c`) is called by `G_Responder` in `g_game.c`.
///
/// Cheat codes are suppressed in network games and on the Nightmare skill level.
/// The level-change cheat (`idclev`) is suppressed in network games regardless of
/// skill.
///
/// # Safety
///
/// Caller must ensure `ev` is a valid, non-null, properly aligned pointer to an
/// initialised `event_t`. Reads and mutates global cheat-sequence state and the
/// status-bar game state (`st_gamestate`, `st_firsttime`, etc.); caller must
/// ensure no concurrent access to these globals.
#[no_mangle]
pub unsafe extern "C" fn ST_Responder(ev: *mut event_t) -> c_int {
    let ev = &*ev;

    if ev.type_ == 1 && (ev.data1 as u32 & 0xffff0000) == AM_MSGHEADER as u32 {
        // ev_keyup + automap message
        match ev.data1 {
            AM_MSGENTERED => {
                st_gamestate = 0; // AutomapState
                st_firsttime = 1;
            }
            AM_MSGEXITED => {
                st_gamestate = 1; // FirstPersonState
            }
            _ => {}
        }
    } else if ev.type_ == 0 {
        // ev_keydown
        if netgame == 0 && gameskill != 4 {
            // sk_nightmare = 4
            if cht_CheckCheat(&raw mut cheat_god, ev.data2 as c_char) != 0 {
                (*plyr).cheats ^= 2; // CF_GODMODE
                if (*plyr).cheats & 2 != 0 {
                    if !(*plyr).mo.is_null() {
                        (*((*plyr).mo as *mut mobj_t)).health = 100;
                    }
                    (*plyr).health = 100; // deh_god_mode_health
                    (*plyr).message = DEH_String(STSTR_DQDON);
                } else {
                    (*plyr).message = DEH_String(STSTR_DQDOFF);
                }
            } else if cht_CheckCheat(&raw mut cheat_ammonokey, ev.data2 as c_char) != 0 {
                (*plyr).armorpoints = 200; // deh_idfa_armor
                (*plyr).armortype = 2; // deh_idfa_armor_class
                for i in 0..NUMWEAPONS {
                    (*plyr).weaponowned[i] = 1;
                }
                for i in 0..NUMAMMO {
                    (*plyr).ammo[i] = (*plyr).maxammo[i];
                }
                (*plyr).message = DEH_String(STSTR_FAADDED);
            } else if cht_CheckCheat(&raw mut cheat_ammo, ev.data2 as c_char) != 0 {
                (*plyr).armorpoints = 200; // deh_idkfa_armor
                (*plyr).armortype = 2; // deh_idkfa_armor_class
                for i in 0..NUMWEAPONS {
                    (*plyr).weaponowned[i] = 1;
                }
                for i in 0..NUMAMMO {
                    (*plyr).ammo[i] = (*plyr).maxammo[i];
                }
                for i in 0..NUMCARDS {
                    (*plyr).cards[i] = 1;
                }
                (*plyr).message = DEH_String(STSTR_KFAADDED);
            } else if cht_CheckCheat(&raw mut cheat_mus, ev.data2 as c_char) != 0 {
                let mut buf = [0i8; 3];
                let musnum: c_int;
                (*plyr).message = DEH_String(STSTR_MUS);
                cht_GetParam(&raw mut cheat_mus, buf.as_mut_ptr());

                if gamemode == d_mode::commercial || gameversion < d_mode::exe_ultimate {
                    musnum = 33
                        + (buf[0] as c_int - '0' as c_int) * 10
                        + (buf[1] as c_int - '0' as c_int)
                        - 1;
                    if ((buf[0] as c_int - '0' as c_int) * 10 + (buf[1] as c_int - '0' as c_int))
                        > 35
                    {
                        (*plyr).message = DEH_String(STSTR_NOMUS);
                    } else {
                        S_ChangeMusic(musnum, 1);
                    }
                } else {
                    musnum =
                        1 + (buf[0] as c_int - '1' as c_int) * 9 + (buf[1] as c_int - '1' as c_int);
                    if ((buf[0] as c_int - '1' as c_int) * 9 + (buf[1] as c_int - '1' as c_int))
                        > 31
                    {
                        (*plyr).message = DEH_String(STSTR_NOMUS);
                    } else {
                        S_ChangeMusic(musnum, 1);
                    }
                }
            } else if (logical_gamemission() == d_mode::doom
                && cht_CheckCheat(&raw mut cheat_noclip, ev.data2 as c_char) != 0)
                || (logical_gamemission() != d_mode::doom
                    && cht_CheckCheat(&raw mut cheat_commercial_noclip, ev.data2 as c_char) != 0)
            {
                (*plyr).cheats ^= 1; // CF_NOCLIP
                if (*plyr).cheats & 1 != 0 {
                    (*plyr).message = DEH_String(STSTR_NCON);
                } else {
                    (*plyr).message = DEH_String(STSTR_NCOFF);
                }
            }

            for i in 0..6 {
                if cht_CheckCheat(&mut cheat_powerup[i], ev.data2 as c_char) != 0 {
                    if (*plyr).powers[i] == 0 {
                        P_GivePower(plyr, i as c_int);
                    } else if i != 1 {
                        // pw_strength = 1
                        (*plyr).powers[i] = 1;
                    } else {
                        (*plyr).powers[i] = 0;
                    }
                    (*plyr).message = DEH_String(STSTR_BEHOLDX);
                }
            }

            if cht_CheckCheat(&mut cheat_powerup[6], ev.data2 as c_char) != 0 {
                (*plyr).message = DEH_String(STSTR_BEHOLD);
            } else if cht_CheckCheat(&raw mut cheat_choppers, ev.data2 as c_char) != 0 {
                (*plyr).weaponowned[7] = 1; // wp_chainsaw
                (*plyr).powers[0] = 1; // pw_invulnerability
                (*plyr).message = DEH_String(STSTR_CHOPPERS);
            } else if cht_CheckCheat(&raw mut cheat_mypos, ev.data2 as c_char) != 0 {
                static mut BUF: [c_char; 52] = [0; 52];
                let mo = players[consoleplayer as usize].mo as *mut mobj_t;
                c_write!(
                    BUF,
                    "ang=0x{:x};x,y=(0x{:x},0x{:x})",
                    (*mo).angle,
                    (*mo).x,
                    (*mo).y
                );
                (*plyr).message = std::ptr::addr_of_mut!(BUF[0]);
            }
        }

        if netgame == 0 && cht_CheckCheat(&raw mut cheat_clev, ev.data2 as c_char) != 0 {
            let mut buf = [0i8; 3];
            let mut epsd: c_int;
            let map: c_int;
            cht_GetParam(&raw mut cheat_clev, buf.as_mut_ptr());

            if gamemode == d_mode::commercial {
                epsd = 1;
                map = (buf[0] as c_int - '0' as c_int) * 10 + (buf[1] as c_int - '0' as c_int);
            } else {
                epsd = buf[0] as c_int - '0' as c_int;
                map = buf[1] as c_int - '0' as c_int;
            }

            if gameversion == d_mode::exe_chex {
                epsd = 1;
            }

            if epsd < 1 {
                return 0;
            }
            if map < 1 {
                return 0;
            }
            if gamemode == d_mode::retail && (epsd > 4 || map > 9) {
                return 0;
            }
            if gamemode == d_mode::registered && (epsd > 3 || map > 9) {
                return 0;
            }
            if gamemode == d_mode::shareware && (epsd > 1 || map > 9) {
                return 0;
            }
            if gamemode == d_mode::commercial && (epsd > 1 || map > 40) {
                return 0;
            }

            (*plyr).message = DEH_String(STSTR_CLEV);
            G_DeferedInitNew(gameskill, epsd, map);
        }
    }

    0
}

// ---------------------------------------------------------------------------
// Face widget
// ---------------------------------------------------------------------------

/// Compute the base face-array index for the current pain level.
///
/// Maps the player's clamped health (0-100) to a pain-row offset inside the
/// flat `faces` array. The result is a multiple of `ST_FACESTRIDE`. The
/// computed value is cached in a function-local static so recalculation only
/// happens when `health` changes.
///
/// Precondition: `plyr` is non-null and points to a valid `PlayerT`.
///
/// # Safety
///
/// Dereferences the global `plyr` pointer and reads/writes function-local
/// `static mut` cache slots. Caller must ensure `ST_initData` has set `plyr`
/// to a valid player and that no concurrent access to the cache occurs.
#[no_mangle]
pub unsafe extern "C" fn ST_calcPainOffset() -> c_int {
    static mut lastcalc: c_int = 0;
    static mut oldhealth: c_int = -1;

    let health = if (*plyr).health > 100 {
        100
    } else {
        (*plyr).health
    };

    if health != oldhealth {
        lastcalc = ST_FACESTRIDE * (((100 - health) * ST_NUMPAINFACES) / 101);
        oldhealth = health;
    }
    lastcalc
}

/// Choose the correct face frame for this tic and update `st_faceindex` / `st_facecount`.
///
/// Priority system (highest wins):
/// 1. Dead face (priority 9).
/// 2. Evil grin on weapon pickup (priority 8).
/// 3. Ouch face on large damage (`health - st_oldhealth > ST_MUCHPAIN`) or turn
///    face on normal damage from an attacker (priority 7).
/// 4. Ouch face on large damage or rampage face when taking damage from an
///    unknown source (priority 6).
/// 5. Rampage face after `ST_RAMPAGEDELAY` tics of continuous fire (priority 5).
/// 6. God-mode / invulnerability face (priority 4).
/// 7. Idle random straight face (priority 0, selected when `st_facecount` reaches 0).
///
/// Precondition: `plyr` is non-null and `oldweaponsowned` matches the snapshot
/// from the previous call.
///
/// # Safety
///
/// Dereferences the global `plyr` pointer (and `plyr->mo`/`plyr->attacker`
/// when computing turn directions) and reads/mutates the face-widget globals
/// (`st_faceindex`, `st_facecount`, `st_oldhealth`, `oldweaponsowned`,
/// function-local `priority`/`lastattackdown`). Caller must ensure
/// `ST_Start`/`ST_initData` has been called and that no other thread accesses
/// these globals concurrently.
#[no_mangle]
pub unsafe extern "C" fn ST_updateFaceWidget() {
    static mut lastattackdown: c_int = -1;
    static mut priority: c_int = 0;
    let diffang: u32;
    let i: c_int;

    if priority < 10 && (*plyr).health == 0 {
        priority = 9;
        st_faceindex = ST_DEADFACE;
        st_facecount = 1;
    }

    if priority < 9 && (*plyr).bonuscount != 0 {
        let mut doevilgrin = 0;
        for i in 0..NUMWEAPONS {
            if oldweaponsowned[i] != (*plyr).weaponowned[i] {
                doevilgrin = 1;
                oldweaponsowned[i] = (*plyr).weaponowned[i];
            }
        }
        if doevilgrin != 0 {
            priority = 8;
            st_facecount = ST_EVILGRINCOUNT;
            st_faceindex = ST_calcPainOffset() + ST_EVILGRINOFFSET;
        }
    }

    if priority < 8
        && (*plyr).damagecount != 0
        && !(*plyr).attacker.is_null()
        && (*plyr).attacker != (*plyr).mo
    {
        priority = 7;
        if (*plyr).health - st_oldhealth > ST_MUCHPAIN {
            st_facecount = ST_TURNCOUNT;
            st_faceindex = ST_calcPainOffset() + ST_OUCHOFFSET;
        } else {
            let badguyangle = R_PointToAngle2(
                (*((*plyr).mo as *mut mobj_t)).x,
                (*((*plyr).mo as *mut mobj_t)).y,
                (*((*plyr).attacker as *mut mobj_t)).x,
                (*((*plyr).attacker as *mut mobj_t)).y,
            );
            if badguyangle > (*((*plyr).mo as *mut mobj_t)).angle {
                diffang = badguyangle - (*((*plyr).mo as *mut mobj_t)).angle;
                i = if diffang > ANG180 { 1 } else { 0 };
            } else {
                diffang = (*((*plyr).mo as *mut mobj_t)).angle - badguyangle;
                i = if diffang <= ANG180 { 1 } else { 0 };
            }

            st_facecount = ST_TURNCOUNT;
            st_faceindex = ST_calcPainOffset();

            if diffang < ANG45 {
                st_faceindex += ST_RAMPAGEOFFSET;
            } else if i != 0 {
                st_faceindex += ST_TURNOFFSET;
            } else {
                st_faceindex += ST_TURNOFFSET + 1;
            }
        }
    }

    if priority < 7 && (*plyr).damagecount != 0 {
        if (*plyr).health - st_oldhealth > ST_MUCHPAIN {
            priority = 7;
            st_facecount = ST_TURNCOUNT;
            st_faceindex = ST_calcPainOffset() + ST_OUCHOFFSET;
        } else {
            priority = 6;
            st_facecount = ST_TURNCOUNT;
            st_faceindex = ST_calcPainOffset() + ST_RAMPAGEOFFSET;
        }
    }

    if priority < 6 {
        if (*plyr).attackdown != 0 {
            if lastattackdown == -1 {
                lastattackdown = ST_RAMPAGEDELAY;
            } else {
                lastattackdown -= 1;
                if lastattackdown == 0 {
                    priority = 5;
                    st_faceindex = ST_calcPainOffset() + ST_RAMPAGEOFFSET;
                    st_facecount = 1;
                    lastattackdown = 1;
                }
            }
        } else {
            lastattackdown = -1;
        }
    }

    if priority < 5 && (((*plyr).cheats & 2) != 0 || (*plyr).powers[0] != 0) {
        // CF_GODMODE || pw_invulnerability
        priority = 4;
        st_faceindex = ST_GODFACE;
        st_facecount = 1;
    }

    if st_facecount == 0 {
        st_faceindex = ST_calcPainOffset() + (st_randomnumber % 3);
        st_facecount = ST_STRAIGHTFACECOUNT;
        priority = 0;
    }

    st_facecount -= 1;
}

// ---------------------------------------------------------------------------
// Widget update
// ---------------------------------------------------------------------------

/// Refresh all status-bar widget data pointers and counters from the player state.
///
/// Called once per tic by `ST_Ticker`. Updates the ready-ammo pointer (using a
/// sentinel value of 1994 for weapons with no ammo type), key-slot indices,
/// visibility flags for the arms panel and frag counter, the frag total, and
/// the face widget via `ST_updateFaceWidget`. Also decrements `st_msgcounter`
/// and restores `st_chat` when it expires.
///
/// # Safety
///
/// Dereferences the global `plyr` pointer and mutates the widget globals
/// (`w_ready`, `keyboxes`, `st_notdeathmatch`, `st_armson`, `st_fragson`,
/// `st_fragscount`, `st_chat`, `st_msgcounter`). Caller must ensure
/// `ST_createWidgets` has run and that `plyr` is valid.
#[no_mangle]
pub unsafe extern "C" fn ST_updateWidgets() {
    static mut largeammo: c_int = 1994;

    if weaponinfo[(*plyr).readyweapon as usize].ammo == 5 {
        // am_noammo
        w_ready.num = &raw mut largeammo as *mut c_int;
    } else {
        w_ready.num = (*plyr)
            .ammo
            .as_mut_ptr()
            .add(weaponinfo[(*plyr).readyweapon as usize].ammo as usize);
    }
    w_ready.data = (*plyr).readyweapon;

    for i in 0..3 {
        keyboxes[i] = if (*plyr).cards[i] != 0 {
            i as c_int
        } else {
            -1
        };
        if (*plyr).cards[i + 3] != 0 {
            keyboxes[i] = (i + 3) as c_int;
        }
    }

    ST_updateFaceWidget();

    st_notdeathmatch = if deathmatch == 0 { 1 } else { 0 };
    st_armson = if st_statusbaron != 0 && deathmatch == 0 {
        1
    } else {
        0
    };
    st_fragson = if deathmatch != 0 && st_statusbaron != 0 {
        1
    } else {
        0
    };
    st_fragscount = 0;

    for i in 0..MAXPLAYERS {
        if i != consoleplayer as usize {
            st_fragscount += (*plyr).frags[i];
        } else {
            st_fragscount -= (*plyr).frags[i];
        }
    }

    st_msgcounter -= 1;
    if st_msgcounter == 0 {
        st_chat = st_oldchat;
    }
}

// ---------------------------------------------------------------------------
// Ticker
// ---------------------------------------------------------------------------

/// Advance the status bar by one game tic.
///
/// Increments the internal clock, samples a new random number for face-idle
/// variation, updates all widget data via `ST_updateWidgets`, and records the
/// current health for the ouch-face comparison next tic.
/// Called once per tic by `G_Ticker` in `g_game.c`.
///
/// # Safety
///
/// Mutates `st_clock`, `st_randomnumber`, and `st_oldhealth`, and indirectly
/// touches every widget global via `ST_updateWidgets`. Caller must ensure
/// `ST_Start` has been called and that `plyr` is valid.
#[no_mangle]
pub unsafe extern "C" fn ST_Ticker() {
    st_clock = st_clock.wrapping_add(1);
    st_randomnumber = M_Random();
    ST_updateWidgets();
    st_oldhealth = (*plyr).health;
}

// ---------------------------------------------------------------------------
// Palette effects
// ---------------------------------------------------------------------------

/// Apply the appropriate screen palette based on the player's current status.
///
/// Priority order (highest first):
/// 1. Damage / berserk: red palette shift scaled by damage count.
/// 2. Bonus pickup: gold/yellow palette shift scaled by bonus count.
/// 3. Radiation suit: fixed `RADIATIONPAL` index.
/// 4. Normal: palette 0.
///
/// In Chex Quest the red-damage palettes are replaced with `RADIATIONPAL` to
/// avoid gore. Only calls `I_SetPalette` when the palette index changes.
///
/// # Safety
///
/// Dereferences the global `plyr` pointer and mutates `st_palette`. Calls
/// `W_CacheLumpNum`/`I_SetPalette` so the WAD subsystem and video backend
/// must be initialised, and `lu_palette` must hold a valid lump number set
/// up by `ST_loadData`.
#[no_mangle]
pub unsafe extern "C" fn ST_doPaletteStuff() {
    let mut palette: c_int;
    let mut cnt = (*plyr).damagecount;

    if (*plyr).powers[1] != 0 {
        // pw_strength
        let bzc = 12 - ((*plyr).powers[1] >> 6);
        if bzc > cnt {
            cnt = bzc;
        }
    }

    if cnt != 0 {
        palette = (cnt + 7) >> 3;
        if palette >= NUMREDPALS {
            palette = NUMREDPALS - 1;
        }
        palette += STARTREDPALS;
    } else if (*plyr).bonuscount != 0 {
        palette = ((*plyr).bonuscount + 7) >> 3;
        if palette >= NUMBONUSPALS {
            palette = NUMBONUSPALS - 1;
        }
        palette += STARTBONUSPALS;
    } else if (*plyr).powers[3] > 4 * 32 || (*plyr).powers[3] & 8 != 0 {
        // pw_ironfeet
        palette = RADIATIONPAL;
    } else {
        palette = 0;
    }

    if gameversion == d_mode::exe_chex
        && (STARTREDPALS..STARTREDPALS + NUMREDPALS).contains(&palette)
    {
        palette = RADIATIONPAL;
    }

    if palette != st_palette {
        st_palette = palette;
        let pal = (W_CacheLumpNum(lu_palette, PU_CACHE) as *mut u8).add((palette * 768) as usize);
        I_SetPalette(pal);
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// Drive all st_lib widget update calls for a single frame.
///
/// `refresh` is passed through to each widget: non-zero forces a full redraw,
/// zero redraws only widgets whose value changed since last frame.
/// Updates `st_armson` and `st_fragson` visibility flags before iterating.
///
/// # Safety
///
/// Mutates `st_armson`/`st_fragson` and reads every `w_*` widget global,
/// passing them to the `STlib_update*` family. Caller must ensure
/// `ST_createWidgets` has been called so the widget pointers refer to live
/// player/state values.
#[no_mangle]
pub unsafe extern "C" fn ST_drawWidgets(refresh: c_int) {
    st_armson = if st_statusbaron != 0 && deathmatch == 0 {
        1
    } else {
        0
    };
    st_fragson = if deathmatch != 0 && st_statusbaron != 0 {
        1
    } else {
        0
    };

    STlib_updateNum(&raw mut w_ready, refresh);

    for i in 0..NUMAMMO {
        STlib_updateNum(std::ptr::addr_of_mut!(w_ammo[0]).add(i), refresh);
        STlib_updateNum(std::ptr::addr_of_mut!(w_maxammo[0]).add(i), refresh);
    }

    STlib_updatePercent(&raw mut w_health, refresh);
    STlib_updatePercent(&raw mut w_armor, refresh);
    STlib_updateBinIcon(&raw mut w_armsbg, refresh);

    for i in 0..6 {
        STlib_updateMultIcon(std::ptr::addr_of_mut!(w_arms[0]).add(i), refresh);
    }

    STlib_updateMultIcon(&raw mut w_faces, refresh);

    for i in 0..3 {
        STlib_updateMultIcon(std::ptr::addr_of_mut!(w_keyboxes[0]).add(i), refresh);
    }

    STlib_updateNum(&raw mut w_frags, refresh);
}

/// Perform a full status-bar redraw: clear `st_firsttime`, refresh the background, then redraw all widgets.
///
/// # Safety
///
/// Mutates `st_firsttime` and delegates to `ST_refreshBackground` and
/// `ST_drawWidgets`, which require an initialised status bar and live
/// `plyr` pointer.
#[no_mangle]
pub unsafe extern "C" fn ST_doRefresh() {
    st_firsttime = 0;
    ST_refreshBackground();
    ST_drawWidgets(1);
}

/// Perform a differential redraw: only redraw widgets whose value changed.
///
/// # Safety
///
/// Delegates to `ST_drawWidgets`; same invariants apply (widgets must have
/// been created and `plyr` must be valid).
#[no_mangle]
pub unsafe extern "C" fn ST_diffDraw() {
    ST_drawWidgets(0);
}

/// Draw the status bar for the current frame.
///
/// `fullscreen` indicates that the view fills the entire screen (no bar),
/// `refresh` forces a complete redraw regardless of dirty state. When
/// `fullscreen` is false or the automap is active, the bar is shown; otherwise
/// it is hidden.
///
/// Called once per frame by the main render loop (`D_Display` in `d_main.c`).
///
/// # Safety
///
/// Mutates `st_statusbaron` and `st_firsttime` and dispatches to
/// `ST_doPaletteStuff`, `ST_doRefresh`, and `ST_diffDraw`. Caller must
/// ensure the status-bar subsystem has been started (`ST_Start`) and the
/// video backend is ready.
#[no_mangle]
pub unsafe extern "C" fn ST_Drawer(fullscreen: Boolean, refresh: Boolean) {
    st_statusbaron = if fullscreen.is_false() || automapactive != 0 {
        1
    } else {
        0
    };
    st_firsttime = if st_firsttime != 0 || refresh.is_truthy() {
        1
    } else {
        0
    };
    ST_doPaletteStuff();
    if st_firsttime != 0 {
        ST_doRefresh();
    } else {
        ST_diffDraw();
    }
}

// ---------------------------------------------------------------------------
// Graphics loading / unloading
// ---------------------------------------------------------------------------

/// Function pointer type for the load/unload callback used by `ST_loadUnloadGraphics`.
///
/// The callback receives a WAD lump name and a pointer to the patch pointer that
/// should be updated: either cached (load path) or released and nulled (unload path).
type LoadCallback = unsafe extern "C" fn(*mut c_char, *mut *mut patch_t);

/// Walk every status-bar lump name and invoke `callback` for each.
///
/// Shared by `ST_loadGraphics` (which passes `ST_loadCallback`) and
/// `ST_unloadGraphics` (which passes `ST_unloadCallback`). The face lump names
/// are generated dynamically; all other names are string literals.
///
/// # Safety
///
/// `callback` is invoked as an `unsafe extern "C"` function and is given raw
/// pointers into the static `tallnum`/`shortnum`/`faces`/etc. arrays. Caller
/// must pass a callback that respects those pointers' validity and only
/// reads/writes the single patch slot supplied. The WAD subsystem must be
/// initialised before the load variant is invoked.
unsafe fn ST_loadUnloadGraphics(callback: LoadCallback) {
    let mut namebuf = [0i8; 9];

    for i in 0..10i32 {
        c_write!(namebuf, "STTNUM{}", i);
        callback(namebuf.as_mut_ptr(), &mut tallnum[i as usize]);
        c_write!(namebuf, "STYSNUM{}", i);
        callback(namebuf.as_mut_ptr(), &mut shortnum[i as usize]);
    }

    callback(c"STTPRCNT".as_ptr().cast_mut(), &raw mut tallpercent);

    for i in 0..NUMCARDS as c_int {
        c_write!(namebuf, "STKEYS{}", i);
        callback(namebuf.as_mut_ptr(), &mut keys[i as usize]);
    }

    callback(c"STARMS".as_ptr().cast_mut(), &raw mut armsbg);

    for i in 0..6i32 {
        c_write!(namebuf, "STGNUM{}", i + 2);
        callback(namebuf.as_mut_ptr(), &mut arms[i as usize][0]);
        arms[i as usize][1] = shortnum[(i + 2) as usize];
    }

    c_write!(namebuf, "STFB{}", consoleplayer as c_int);
    callback(namebuf.as_mut_ptr(), &raw mut faceback);

    callback(c"STBAR".as_ptr().cast_mut(), &raw mut sbar);

    let mut facenum: c_int = 0;
    for i in 0..ST_NUMPAINFACES {
        for j in 0..ST_NUMSTRAIGHTFACES {
            c_write!(namebuf, "STFST{}{}", i, j);
            callback(namebuf.as_mut_ptr(), &mut faces[facenum as usize]);
            facenum += 1;
        }
        c_write!(namebuf, "STFTR{}0", i);
        callback(namebuf.as_mut_ptr(), &mut faces[facenum as usize]);
        facenum += 1;
        c_write!(namebuf, "STFTL{}0", i);
        callback(namebuf.as_mut_ptr(), &mut faces[facenum as usize]);
        facenum += 1;
        c_write!(namebuf, "STFOUCH{}", i);
        callback(namebuf.as_mut_ptr(), &mut faces[facenum as usize]);
        facenum += 1;
        c_write!(namebuf, "STFEVL{}", i);
        callback(namebuf.as_mut_ptr(), &mut faces[facenum as usize]);
        facenum += 1;
        c_write!(namebuf, "STFKILL{}", i);
        callback(namebuf.as_mut_ptr(), &mut faces[facenum as usize]);
        facenum += 1;
    }

    callback(c"STFGOD0".as_ptr().cast_mut(), &mut faces[facenum as usize]);
    facenum += 1;
    callback(
        c"STFDEAD0".as_ptr().cast_mut(),
        &mut faces[facenum as usize],
    );
}

/// Load callback: cache the named WAD lump and store the pointer in `*variable`.
///
/// # Safety
///
/// Caller must ensure `lumpname` is a valid NUL-terminated C-string pointer
/// recognised by `W_CacheLumpName`, and `variable` is a valid, non-null,
/// properly aligned pointer to a `*mut patch_t` slot the function may overwrite.
unsafe extern "C" fn ST_loadCallback(lumpname: *mut c_char, variable: *mut *mut patch_t) {
    *variable = W_CacheLumpName(lumpname, PU_STATIC) as *mut patch_t;
}

/// Cache all status-bar graphics from the WAD into `PU_STATIC` memory.
///
/// # Safety
///
/// Mutates every cached patch pointer in the status-bar globals
/// (`tallnum`, `shortnum`, `tallpercent`, `keys`, `armsbg`, `arms`,
/// `faceback`, `sbar`, `faces`). Caller must ensure the WAD subsystem is
/// initialised and that no other code is reading these pointers concurrently.
#[no_mangle]
pub unsafe extern "C" fn ST_loadGraphics() {
    ST_loadUnloadGraphics(ST_loadCallback);
}

/// Cache the palette lump number and load all status-bar graphics.
///
/// # Safety
///
/// Writes the global `lu_palette` and delegates to `ST_loadGraphics`, which
/// mutates the cached patch pointers. Caller must ensure the WAD subsystem
/// is initialised.
#[no_mangle]
pub unsafe extern "C" fn ST_loadData() {
    lu_palette = W_GetNumForName(c"PLAYPAL".as_ptr());
    ST_loadGraphics();
}

/// Unload callback: release the named WAD lump and null the pointer in `*variable`.
///
/// # Safety
///
/// Caller must ensure `lumpname` is a valid NUL-terminated C-string pointer
/// previously cached via `W_CacheLumpName`, and `variable` is a valid,
/// non-null, properly aligned pointer to a `*mut patch_t` slot that this
/// function may overwrite with null.
unsafe extern "C" fn ST_unloadCallback(lumpname: *mut c_char, variable: *mut *mut patch_t) {
    W_ReleaseLumpName(lumpname);
    *variable = ptr::null_mut();
}

/// Release all status-bar graphics and null their pointers.
///
/// # Safety
///
/// Nulls every cached patch pointer in the status-bar globals and releases
/// their WAD lumps. Caller must ensure no other code is currently using
/// those patches (e.g. drawing must be stopped via `ST_Stop`).
#[no_mangle]
pub unsafe extern "C" fn ST_unloadGraphics() {
    ST_loadUnloadGraphics(ST_unloadCallback);
}

/// Release all status-bar data (currently delegates to `ST_unloadGraphics`).
///
/// # Safety
///
/// Delegates to `ST_unloadGraphics`; same invariants apply (status bar must
/// not currently be drawing).
#[no_mangle]
pub unsafe extern "C" fn ST_unloadData() {
    ST_unloadGraphics();
}

// ---------------------------------------------------------------------------
// Init / Start / Stop
// ---------------------------------------------------------------------------

/// Reset all internal status-bar state variables to their defaults.
///
/// Sets `st_firsttime`, clears the clock, chat state, cursor, face index, and
/// palette sentinel. Snapshots the current weapon ownership array and resets
/// key-slot values to -1. Calls `STlib_init` to reset the widget library.
///
/// # Safety
///
/// Writes to every internal status-bar state global and to `plyr`, deriving
/// it from `players[consoleplayer]`. Caller must ensure `consoleplayer` is a
/// valid index into `players` (i.e. `D_DoomMain` has set the network/player
/// globals).
#[no_mangle]
pub unsafe extern "C" fn ST_initData() {
    st_firsttime = 1;
    plyr = std::ptr::addr_of_mut!(players[0]).add(consoleplayer as usize);
    st_clock = 0;
    st_chatstate = 0; // StartChatState
    st_gamestate = 1; // FirstPersonState
    st_statusbaron = 1;
    st_oldchat = 0;
    st_chat = 0;
    st_cursoron = 0;
    st_faceindex = 0;
    st_palette = -1;
    st_oldhealth = -1;

    for i in 0..NUMWEAPONS {
        oldweaponsowned[i] = (*plyr).weaponowned[i];
    }
    for i in 0..3 {
        keyboxes[i] = -1;
    }

    STlib_init();
}

/// Initialise and bind all status-bar widgets to their screen positions.
///
/// Must be called after `ST_initData` (which sets `plyr`) and after graphics
/// have been loaded (which populates `tallnum`, `shortnum`, etc.). Creates
/// widgets for: ready ammo, health, armor, arms background, weapons-owned icons,
/// frag counter, face, key slots, and per-ammo-type current/max displays.
///
/// # Safety
///
/// Initialises every `w_*` widget global with raw pointers into `plyr`'s
/// ammo/health/armor/weapon-owned arrays and the cached patch tables.
/// Caller must ensure `ST_initData` and `ST_loadGraphics` ran first so
/// `plyr` is valid and the patches are loaded.
#[no_mangle]
pub unsafe extern "C" fn ST_createWidgets() {
    STlib_initNum(
        &raw mut w_ready,
        ST_AMMOX,
        ST_AMMOY,
        std::ptr::addr_of_mut!(tallnum[0]),
        (*plyr)
            .ammo
            .as_mut_ptr()
            .add(weaponinfo[(*plyr).readyweapon as usize].ammo as usize),
        &raw mut st_statusbaron,
        ST_AMMOWIDTH,
    );
    w_ready.data = (*plyr).readyweapon;

    STlib_initPercent(
        &raw mut w_health,
        ST_HEALTHX,
        ST_HEALTHY,
        std::ptr::addr_of_mut!(tallnum[0]),
        &mut (*plyr).health,
        &raw mut st_statusbaron,
        tallpercent,
    );

    STlib_initBinIcon(
        &raw mut w_armsbg,
        ST_ARMSBGX,
        ST_ARMSBGY,
        armsbg,
        &raw mut st_notdeathmatch,
        &raw mut st_statusbaron,
    );

    for i in 0..6 {
        let i_i = i as c_int;
        STlib_initMultIcon(
            std::ptr::addr_of_mut!(w_arms[0]).add(i),
            ST_ARMSX + (i_i % 3) * ST_ARMSXSPACE,
            ST_ARMSY + (i_i / 3) * ST_ARMSYSPACE,
            arms[i].as_mut_ptr(),
            (*plyr).weaponowned.as_mut_ptr().add(i + 1),
            &raw mut st_armson,
        );
    }

    STlib_initNum(
        &raw mut w_frags,
        ST_FRAGSX,
        ST_FRAGSY,
        std::ptr::addr_of_mut!(tallnum[0]),
        &raw mut st_fragscount,
        &raw mut st_fragson,
        ST_FRAGSWIDTH,
    );

    STlib_initMultIcon(
        &raw mut w_faces,
        ST_FACESX,
        ST_FACESY,
        std::ptr::addr_of_mut!(faces[0]),
        &raw mut st_faceindex,
        &raw mut st_statusbaron,
    );

    STlib_initPercent(
        &raw mut w_armor,
        ST_ARMORX,
        ST_ARMORY,
        std::ptr::addr_of_mut!(tallnum[0]),
        &mut (*plyr).armorpoints,
        &raw mut st_statusbaron,
        tallpercent,
    );

    STlib_initMultIcon(
        std::ptr::addr_of_mut!(w_keyboxes[0]).add(0),
        ST_KEY0X,
        ST_KEY0Y,
        std::ptr::addr_of_mut!(keys[0]),
        &mut keyboxes[0],
        &raw mut st_statusbaron,
    );
    STlib_initMultIcon(
        std::ptr::addr_of_mut!(w_keyboxes[0]).add(1),
        ST_KEY1X,
        ST_KEY1Y,
        std::ptr::addr_of_mut!(keys[0]),
        &mut keyboxes[1],
        &raw mut st_statusbaron,
    );
    STlib_initMultIcon(
        std::ptr::addr_of_mut!(w_keyboxes[0]).add(2),
        ST_KEY2X,
        ST_KEY2Y,
        std::ptr::addr_of_mut!(keys[0]),
        &mut keyboxes[2],
        &raw mut st_statusbaron,
    );

    for i in 0..NUMAMMO {
        STlib_initNum(
            std::ptr::addr_of_mut!(w_ammo[0]).add(i),
            match i {
                0 => ST_AMMO0X,
                1 => ST_AMMO1X,
                2 => ST_AMMO2X,
                3 => ST_AMMO3X,
                _ => unreachable!(),
            },
            match i {
                0 => ST_AMMO0Y,
                1 => ST_AMMO1Y,
                2 => ST_AMMO2Y,
                3 => ST_AMMO3Y,
                _ => unreachable!(),
            },
            std::ptr::addr_of_mut!(shortnum[0]),
            (*plyr).ammo.as_mut_ptr().add(i),
            &raw mut st_statusbaron,
            ST_AMMO0WIDTH,
        );
    }

    for i in 0..NUMAMMO {
        STlib_initNum(
            std::ptr::addr_of_mut!(w_maxammo[0]).add(i),
            match i {
                0 => ST_MAXAMMO0X,
                1 => ST_MAXAMMO1X,
                2 => ST_MAXAMMO2X,
                3 => ST_MAXAMMO3X,
                _ => unreachable!(),
            },
            match i {
                0 => ST_MAXAMMO0Y,
                1 => ST_MAXAMMO1Y,
                2 => ST_MAXAMMO2Y,
                3 => ST_MAXAMMO3Y,
                _ => unreachable!(),
            },
            std::ptr::addr_of_mut!(shortnum[0]),
            (*plyr).maxammo.as_mut_ptr().add(i),
            &raw mut st_statusbaron,
            ST_MAXAMMO0WIDTH,
        );
    }
}

/// Start the status bar subsystem for a new game or level.
///
/// Calls `ST_Stop` if already running, then re-initialises state, creates
/// widgets, and clears `st_stopped`. Called from `G_DoLoadLevel` in `g_game.c`.
///
/// # Safety
///
/// Reads/mutates `st_stopped` and runs `ST_initData`/`ST_createWidgets`,
/// which mutate every status-bar global and require the WAD/player subsystems
/// to be initialised (see those functions' own safety contracts). Caller must
/// ensure `ST_Init` has run once at startup.
#[no_mangle]
pub unsafe extern "C" fn ST_Start() {
    if st_stopped == 0 {
        ST_Stop();
    }
    ST_initData();
    ST_createWidgets();
    st_stopped = 0;
}

/// Stop the status bar subsystem and restore the normal palette.
///
/// Resets the display palette to `PLAYPAL` index 0 and sets `st_stopped`.
/// Safe to call when already stopped (guard at the top). Called from `G_WorldDone`
/// and `ST_Start` in `g_game.c`.
///
/// # Safety
///
/// Reads/mutates `st_stopped` and calls `I_SetPalette` with the cached
/// `lu_palette` lump. Caller must ensure `ST_loadData` has cached the
/// palette lump and the video backend is ready.
#[no_mangle]
pub unsafe extern "C" fn ST_Stop() {
    if st_stopped != 0 {
        return;
    }
    I_SetPalette(W_CacheLumpNum(lu_palette, PU_CACHE) as *mut u8);
    st_stopped = 1;
}

/// One-time initialisation of the status bar subsystem.
///
/// Loads all WAD graphics into `PU_STATIC` memory and allocates the backing
/// screen buffer. Must be called exactly once during startup, before `ST_Start`.
/// Called from `G_InitNew` in `g_game.c`.
///
/// # Safety
///
/// Allocates `st_backing_screen` via `Z_Malloc` and runs `ST_loadData`,
/// which mutates the cached patch globals. Caller must ensure the WAD and
/// zone-memory subsystems are initialised, and that this function is called
/// exactly once.
#[no_mangle]
pub unsafe extern "C" fn ST_Init() {
    ST_loadData();
    st_backing_screen = Z_Malloc(ST_WIDTH * ST_HEIGHT, PU_STATIC, ptr::null_mut()) as *mut u8;
}
