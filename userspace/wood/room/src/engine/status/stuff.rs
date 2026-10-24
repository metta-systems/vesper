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
//! - `DEH_String` (from `dehacked`) is an identity; Dehacked string
//!   replacement is not yet wired up.
//! - `logical_gamemission` is duplicated here (it also lives in `st_stuff.c`
//!   as a macro) to keep the file self-contained.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ptr;

use crate::{
    c_write,
    engine::{
        automap::automap_active,
        dehacked::DEH_String,
        doom::{
            self,
            event::event_t,
            items::weaponinfo,
            player::{MAXPLAYERS, NUMAMMO, NUMCARDS, NUMWEAPONS, PlayerId, PlayerMessage, PlayerT},
            stat::{gamemission, gamemode, gameversion},
        },
        game::{G_DeferedInitNew, consoleplayer, deathmatch, gameskill, netgame},
        global::Global,
        interface::{timer::TICRATE, video::I_SetPalette},
        math::tables::{ANG45, ANG180},
        misc::{
            cheat::{cheatseq_t, cht_CheckCheat, cht_GetParam},
            random::M_Random,
        },
        play::inter::P_GivePower,
        render_2d::{V_CopyRect, V_DrawPatch, V_RestoreBuffer, V_UseBuffer},
        render_3d::main::R_PointToAngle2,
        sound::S_ChangeMusic,
        status::lib::{
            PatchTable, STlib_init, STlib_initBinIcon, STlib_initMultIcon, STlib_initNum,
            STlib_initPercent, STlib_updateBinIcon, STlib_updateMultIcon, STlib_updateNum,
            STlib_updatePercent, st_binicon_t, st_multicon_t, st_number_t, st_percent_t,
        },
        wad::{
            W_CacheLumpNum, W_GetNumForName, W_ReleaseLumpName, cache_patch_name, format::Patch,
        },
        world::{WORLD, World},
        zone::{PU_CACHE, PU_STATIC, Z_Malloc},
    },
    types::Boolean,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Height of the status bar in screen pixels (matches C `ST_HEIGHT`).
const ST_HEIGHT: i32 = 32;
/// Width of the status bar in screen pixels (matches C `ST_WIDTH`).
const ST_WIDTH: i32 = 320;
/// Left edge of the status bar in screen coordinates (matches C `ST_X`).
pub const ST_X: i32 = 0;
/// X pixel offset of the arms display (matches C `ST_X2`).
pub const ST_X2: i32 = 104;
/// Top edge of the status bar in screen coordinates (matches C `ST_Y`).
const ST_Y: i32 = 200 - ST_HEIGHT;

/// X pixel position of the face widget background (matches C `ST_FX`).
const ST_FX: i32 = 143;
/// Y pixel position of the face widget background (matches C `ST_FY`).
const ST_FY: i32 = 169;

/// 1-in-N probability of a random face change per tic (matches C `ST_FACEPROBABILITY`).
pub const ST_FACEPROBABILITY: i32 = 96;
/// Number of pain-level face rows (0 = healthy, 4 = critical; matches C `ST_NUMPAINFACES`).
pub const ST_NUMPAINFACES: i32 = 5;
/// Straight-ahead frames per pain row (matches C `ST_NUMSTRAIGHTFACES`).
pub const ST_NUMSTRAIGHTFACES: i32 = 3;
/// Turn-direction frames per pain row (matches C `ST_NUMTURNFACES`).
pub const ST_NUMTURNFACES: i32 = 2;
/// Special-expression frames per pain row: ouch, evil-grin, rampage (matches C `ST_NUMSPECIALFACES`).
pub const ST_NUMSPECIALFACES: i32 = 3;

/// Total frames per pain-level row (matches C `ST_FACESTRIDE`).
const ST_FACESTRIDE: i32 = ST_NUMSTRAIGHTFACES + ST_NUMTURNFACES + ST_NUMSPECIALFACES;

/// Number of frames appended after all pain rows: god-mode and dead (matches C `ST_NUMEXTRAFACES`).
pub const ST_NUMEXTRAFACES: i32 = 2;
/// Total face patch count loaded from the WAD (matches C `ST_NUMFACES`).
pub const ST_NUMFACES: i32 = ST_FACESTRIDE * ST_NUMPAINFACES + ST_NUMEXTRAFACES;

/// Offset within a pain-row to the first turn face (matches C `ST_TURNOFFSET`).
const ST_TURNOFFSET: i32 = ST_NUMSTRAIGHTFACES;
/// Offset within a pain-row to the ouch face (matches C `ST_OUCHOFFSET`).
const ST_OUCHOFFSET: i32 = ST_TURNOFFSET + ST_NUMTURNFACES;
/// Offset within a pain-row to the evil-grin face (matches C `ST_EVILGRINOFFSET`).
const ST_EVILGRINOFFSET: i32 = ST_OUCHOFFSET + 1;
/// Offset within a pain-row to the rampage face (matches C `ST_RAMPAGEOFFSET`).
const ST_RAMPAGEOFFSET: i32 = ST_EVILGRINOFFSET + 1;
/// Index of the god-mode face in the flat `faces` array (matches C `ST_GODFACE`).
const ST_GODFACE: i32 = ST_NUMPAINFACES * ST_FACESTRIDE;
/// Index of the dead face in the flat `faces` array (matches C `ST_DEADFACE`).
const ST_DEADFACE: i32 = ST_GODFACE + 1;

/// X screen coordinate of the face widget (matches C `ST_FACESX`).
const ST_FACESX: i32 = 143;
/// Y screen coordinate of the face widget (matches C `ST_FACESY`).
const ST_FACESY: i32 = 168;

/// Duration of the evil-grin expression in tics (matches C `ST_EVILGRINCOUNT`).
pub const ST_EVILGRINCOUNT: i32 = 2 * TICRATE;
/// Duration of a straight-ahead expression in tics (matches C `ST_STRAIGHTFACECOUNT`).
pub const ST_STRAIGHTFACECOUNT: i32 = TICRATE / 2;
/// Duration of turn and ouch expressions in tics (matches C `ST_TURNCOUNT`).
pub const ST_TURNCOUNT: i32 = TICRATE;
/// Duration of the ouch expression in tics (matches C `ST_OUCHCOUNT`).
pub const ST_OUCHCOUNT: i32 = TICRATE;
/// Tics of continuous fire before the rampage face appears (matches C `ST_RAMPAGEDELAY`).
pub const ST_RAMPAGEDELAY: i32 = 2 * TICRATE;

/// Health-point drop threshold that triggers the ouch face (matches C `ST_MUCHPAIN`).
pub const ST_MUCHPAIN: i32 = 20;

/// Digit width of the ready-ammo number widget (matches C `ST_AMMOWIDTH`).
const ST_AMMOWIDTH: i32 = 3;
/// X coordinate of the ready-ammo display (matches C `ST_AMMOX`).
const ST_AMMOX: i32 = 44;
/// Y coordinate of the ready-ammo display (matches C `ST_AMMOY`).
const ST_AMMOY: i32 = 171;

/// Digit width of the health display (matches C `ST_HEALTHWIDTH`).
const ST_HEALTHWIDTH: i32 = 3;
/// X coordinate of the health display (matches C `ST_HEALTHX`).
const ST_HEALTHX: i32 = 90;
/// Y coordinate of the health display (matches C `ST_HEALTHY`).
const ST_HEALTHY: i32 = 171;

/// X coordinate of the weapons-owned grid (matches C `ST_ARMSX`).
const ST_ARMSX: i32 = 111;
/// Y coordinate of the weapons-owned grid (matches C `ST_ARMSY`).
const ST_ARMSY: i32 = 172;
/// X coordinate of the arms background patch (matches C `ST_ARMSBGX`).
const ST_ARMSBGX: i32 = 104;
/// Y coordinate of the arms background patch (matches C `ST_ARMSBGY`).
const ST_ARMSBGY: i32 = 168;
/// Horizontal spacing between arms-grid cells in pixels (matches C `ST_ARMSXSPACE`).
const ST_ARMSXSPACE: i32 = 12;
/// Vertical spacing between arms-grid cells in pixels (matches C `ST_ARMSYSPACE`).
const ST_ARMSYSPACE: i32 = 10;

/// X coordinate of the frag counter in deathmatch (matches C `ST_FRAGSX`).
const ST_FRAGSX: i32 = 138;
/// Y coordinate of the frag counter in deathmatch (matches C `ST_FRAGSY`).
const ST_FRAGSY: i32 = 171;
/// Digit width of the frag counter (matches C `ST_FRAGSWIDTH`).
const ST_FRAGSWIDTH: i32 = 2;

/// Digit width of the armor display (matches C `ST_ARMORWIDTH`).
const ST_ARMORWIDTH: i32 = 3;
/// X coordinate of the armor display (matches C `ST_ARMORX`).
const ST_ARMORX: i32 = 221;
/// Y coordinate of the armor display (matches C `ST_ARMORY`).
const ST_ARMORY: i32 = 171;

/// X coordinate of the blue key slot (matches C `ST_KEY0X`).
const ST_KEY0X: i32 = 239;
/// Y coordinate of the blue key slot (matches C `ST_KEY0Y`).
const ST_KEY0Y: i32 = 171;
/// X coordinate of the yellow key slot (matches C `ST_KEY1X`).
const ST_KEY1X: i32 = 239;
/// Y coordinate of the yellow key slot (matches C `ST_KEY1Y`).
const ST_KEY1Y: i32 = 181;
/// X coordinate of the red key slot (matches C `ST_KEY2X`).
const ST_KEY2X: i32 = 239;
/// Y coordinate of the red key slot (matches C `ST_KEY2Y`).
const ST_KEY2Y: i32 = 191;

/// Digit width of per-ammo-type current-ammo displays (matches C `ST_AMMO0WIDTH`).
const ST_AMMO0WIDTH: i32 = 3;
/// X coordinate of bullets current-ammo display (matches C `ST_AMMO0X`).
const ST_AMMO0X: i32 = 288;
/// Y coordinate of bullets current-ammo display (matches C `ST_AMMO0Y`).
const ST_AMMO0Y: i32 = 173;
/// X coordinate of shells current-ammo display (matches C `ST_AMMO1X`).
const ST_AMMO1X: i32 = 288;
/// Y coordinate of shells current-ammo display (matches C `ST_AMMO1Y`).
const ST_AMMO1Y: i32 = 179;
/// X coordinate of cells current-ammo display (matches C `ST_AMMO2X`).
const ST_AMMO2X: i32 = 288;
/// Y coordinate of cells current-ammo display (matches C `ST_AMMO2Y`).
const ST_AMMO2Y: i32 = 191;
/// X coordinate of rockets current-ammo display (matches C `ST_AMMO3X`).
const ST_AMMO3X: i32 = 288;
/// Y coordinate of rockets current-ammo display (matches C `ST_AMMO3Y`).
const ST_AMMO3Y: i32 = 185;

/// Digit width of per-ammo-type max-ammo displays (matches C `ST_MAXAMMO0WIDTH`).
const ST_MAXAMMO0WIDTH: i32 = 3;
/// X coordinate of bullets max-ammo display (matches C `ST_MAXAMMO0X`).
const ST_MAXAMMO0X: i32 = 314;
/// Y coordinate of bullets max-ammo display (matches C `ST_MAXAMMO0Y`).
const ST_MAXAMMO0Y: i32 = 173;
/// X coordinate of shells max-ammo display (matches C `ST_MAXAMMO1X`).
const ST_MAXAMMO1X: i32 = 314;
/// Y coordinate of shells max-ammo display (matches C `ST_MAXAMMO1Y`).
const ST_MAXAMMO1Y: i32 = 179;
/// X coordinate of cells max-ammo display (matches C `ST_MAXAMMO2X`).
const ST_MAXAMMO2X: i32 = 314;
/// Y coordinate of cells max-ammo display (matches C `ST_MAXAMMO2Y`).
const ST_MAXAMMO2Y: i32 = 191;
/// X coordinate of rockets max-ammo display (matches C `ST_MAXAMMO3X`).
const ST_MAXAMMO3X: i32 = 314;
/// Y coordinate of rockets max-ammo display (matches C `ST_MAXAMMO3Y`).
const ST_MAXAMMO3Y: i32 = 185;

/// Header magic for automap messages sent via the event system (matches C `AM_MSGHEADER`).
const AM_MSGHEADER: i32 = (('a' as i32) << 24) + (('m' as i32) << 16);
/// Event data value signalling that the automap was opened (matches C `AM_MSGENTERED`).
const AM_MSGENTERED: i32 = AM_MSGHEADER | (('e' as i32) << 8);
/// Event data value signalling that the automap was closed (matches C `AM_MSGEXITED`).
const AM_MSGEXITED: i32 = AM_MSGHEADER | (('x' as i32) << 8);

/// Index of the first red-damage palette in `PLAYPAL` (matches C `STARTREDPALS`).
pub const STARTREDPALS: i32 = 1;
/// Index of the first bonus-pickup palette in `PLAYPAL` (matches C `STARTBONUSPALS`).
pub const STARTBONUSPALS: i32 = 9;
/// Number of red-damage palette entries (matches C `NUMREDPALS`).
pub const NUMREDPALS: i32 = 8;
/// Number of bonus-pickup palette entries (matches C `NUMBONUSPALS`).
pub const NUMBONUSPALS: i32 = 4;
/// Index of the radiation-suit palette in `PLAYPAL` (matches C `RADIATIONPAL`).
pub const RADIATIONPAL: i32 = 13;

// ---------------------------------------------------------------------------
// String literals (from d_englsh.h)
// ---------------------------------------------------------------------------

/// Message shown when god mode is activated (C `STSTR_DQDON` from `d_englsh.h`).
const STSTR_DQDON: &str = "Degreelessness Mode On";
/// Message shown when god mode is deactivated (C `STSTR_DQDOFF` from `d_englsh.h`).
const STSTR_DQDOFF: &str = "Degreelessness Mode Off";
/// Message shown when `idfa` (ammo, no keys) cheat fires (C `STSTR_FAADDED`).
const STSTR_FAADDED: &str = "Ammo (no keys) Added";
/// Message shown when `idkfa` (ammo + keys) cheat fires (C `STSTR_KFAADDED`).
const STSTR_KFAADDED: &str = "Very Happy Ammo Added";
/// Message shown when a music-change cheat fires (C `STSTR_MUS`).
const STSTR_MUS: &str = "Music Change";
/// Message shown when an invalid music number is entered (C `STSTR_NOMUS`).
const STSTR_NOMUS: &str = "IMPOSSIBLE SELECTION";
/// Message shown when no-clip mode is activated (C `STSTR_NCON`).
const STSTR_NCON: &str = "No Clipping Mode ON";
/// Message shown when no-clip mode is deactivated (C `STSTR_NCOFF`).
const STSTR_NCOFF: &str = "No Clipping Mode OFF";
/// Prompt shown when the `idbehold` cheat prefix fires (C `STSTR_BEHOLD`).
const STSTR_BEHOLD: &str = "inVuln, Str, Inviso, Rad, Allmap, or Lite-amp";
/// Message shown when a `idbeholdX` power-up cheat fires (C `STSTR_BEHOLDX`).
const STSTR_BEHOLDX: &str = "Power-up Toggled";
/// Message shown when the `idchoppers` cheat fires (C `STSTR_CHOPPERS`).
const STSTR_CHOPPERS: &str = "... doesn't suck - GM";
/// Message shown when the level-change cheat (`idclev`) fires (C `STSTR_CLEV`).
const STSTR_CLEV: &str = "Changing Level...";

// ---------------------------------------------------------------------------
// Replicate the C `logical_gamemission` macro from `doomstat.h`.
// ---------------------------------------------------------------------------

/// Return the canonical game mission, collapsing Chex Quest and HacX aliases.
///
/// Mirrors the `logical_gamemission` macro from `doomstat.h`:
/// - `pack_chex` maps to `doom`.
/// - `pack_hacx` maps to `doom2`.
/// - All other values are returned as-is.
fn logical_gamemission() -> i32 {
    let gamemission = gamemission();
    if gamemission == doom::mode::pack_chex {
        doom::mode::doom
    } else if gamemission == doom::mode::pack_hacx {
        doom::mode::doom2
    } else {
        gamemission
    }
}

// ---------------------------------------------------------------------------
// Cheat helpers
// ---------------------------------------------------------------------------

/// Copy a byte slice into a 25-element `u8` array, padding the remainder with zeros.
///
/// Used at compile time to initialise cheat-sequence buffers inside `cheatseq_t`.
const fn make_cheat_seq(seq: &[u8]) -> [u8; 25] {
    let mut arr = [0u8; 25];
    let mut i = 0;
    while i < seq.len() {
        arr[i] = seq[i];
        i += 1;
    }
    arr
}

/// Construct a `cheatseq_t` from a key sequence and a parameter character count.
///
/// All runtime state fields (`chars_read`, `param_chars_read`, `parameter_buf`) are
/// zeroed; they will be updated by `cht_CheckCheat` during gameplay.
const fn cheat(seq: &[u8], params: i32) -> cheatseq_t {
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
// State
// ---------------------------------------------------------------------------

/// Status bar state (C `st_stuff.c` and `st_lib.c` globals).
///
/// The widgets keep raw pointers to the patch tables here, so those are
/// taken through [`Global::as_ptr`]. The values and visibility flags the
/// widgets show (player fields and fields here) are passed to the
/// `STlib_update*` calls when they draw.
///
/// The patches are `'static` views of lumps cached at `PU_STATIC` by
/// `ST_loadGraphics` (and `STlib_init` for `sttminus`); `None` is a patch
/// not loaded. Only `ST_unloadGraphics` releases those lumps, clearing the
/// slots, and it must not run while the widgets, which copy some patches,
/// still draw.
pub struct StatusState {
    /// Backing pixel buffer for the status bar, allocated in `ST_Init`.
    ///
    /// The buffer is `ST_WIDTH * ST_HEIGHT` bytes and is used by
    /// `ST_refreshBackground` and the `st_lib` widgets to save and restore
    /// the pixels beneath the bar.
    pub(super) st_backing_screen: *mut u8,
    /// The minus-sign patch (`STTMINUS`) used when rendering negative
    /// numbers, loaded by `STlib_init` (C `sttminus` in `st_lib.c`).
    pub(super) sttminus: Option<Patch<'static>>,
    /// Cheat sequence for changing the playing music track (`idmus##`).
    ///
    /// Two parameter characters encode the track number.
    cheat_mus: cheatseq_t,
    /// Cheat sequence for toggling god mode (`iddqd`).
    cheat_god: cheatseq_t,
    /// Cheat sequence for full ammo and armor, including all keys (`idkfa`).
    cheat_ammo: cheatseq_t,
    /// Cheat sequence for full ammo and armor, without keys (`idfa`).
    cheat_ammonokey: cheatseq_t,
    /// No-clip cheat sequence for Doom episode maps (`idspispopd`).
    ///
    /// Only active when `logical_gamemission() == doom`.
    cheat_noclip: cheatseq_t,
    /// No-clip cheat sequence for Doom II maps (`idclip`).
    ///
    /// Active when `logical_gamemission() != doom`.
    cheat_commercial_noclip: cheatseq_t,
    /// Power-up cheat sequences (`idbeholdv/s/i/r/a/l` and the bare
    /// `idbehold` prefix).
    ///
    /// Indices 0-5 correspond to the six togglable power-ups
    /// (invulnerability, berserk, invisibility, radiation suit, automap,
    /// light amp). Index 6 is the bare `idbehold` prefix which displays the
    /// prompt.
    cheat_powerup: [cheatseq_t; 7],
    /// Cheat sequence that gives the chainsaw and invulnerability
    /// (`idchoppers`).
    cheat_choppers: cheatseq_t,
    /// Cheat sequence for warping to a specific episode+map (`idclev##`).
    ///
    /// Two parameter characters encode the destination.
    cheat_clev: cheatseq_t,
    /// Cheat sequence that prints the player's current map position
    /// (`idmypos`).
    cheat_mypos: cheatseq_t,
    /// The local player's slot; set by `ST_initData`.
    plyr: Option<PlayerId>,
    /// Non-zero when the status bar needs a full redraw on the next
    /// `ST_Drawer` call.
    st_firsttime: i32,
    /// WAD lump number of `PLAYPAL`, cached by `ST_loadData` for palette
    /// lookups.
    lu_palette: i32,
    /// Monotonically increasing tic counter incremented by `ST_Ticker`.
    st_clock: u32,
    /// Countdown used to temporarily suppress chat-message override of the
    /// HUD message.
    st_msgcounter: i32,
    /// Current chat state: 0 = `StartChatState`, used to track HUD message
    /// display mode.
    st_chatstate: i32,
    /// Current view state: 0 = automap active, 1 = first-person view.
    st_gamestate: i32,
    /// Non-zero when the status bar should be rendered (i.e. not in
    /// full-screen mode).
    st_statusbaron: i32,
    /// Non-zero while a chat message is being composed.
    st_chat: i32,
    /// Previous value of `st_chat`; restored after `st_msgcounter` expires.
    st_oldchat: i32,
    /// Blink flag for the chat cursor; toggled by `st_msgcounter`.
    st_cursoron: i32,
    /// Non-zero in cooperative (non-deathmatch) games; gates the
    /// weapons-owned display.
    st_notdeathmatch: i32,
    /// Non-zero when the weapons-owned display should be shown.
    st_armson: i32,
    /// Non-zero when the frag counter should be shown (deathmatch only).
    st_fragson: i32,
    /// The `STBAR` background patch.
    sbar: Option<Patch<'static>>,
    /// Tall digit patches 0-9 (`STTNUM0`-`STTNUM9`), used for health,
    /// armor, and ammo.
    tallnum: [Option<Patch<'static>>; 10],
    /// Tall percent-sign patch (`STTPRCNT`).
    tallpercent: Option<Patch<'static>>,
    /// Short digit patches 0-9 (`STYSNUM0`-`STYSNUM9`), used in the
    /// per-ammo sidebar.
    shortnum: [Option<Patch<'static>>; 10],
    /// Key icon patches, one per `NUMCARDS` card type.
    keys: [Option<Patch<'static>>; NUMCARDS],
    /// All face patches in a flat array; indexed by `st_faceindex`.
    faces: [Option<Patch<'static>>; ST_NUMFACES as usize],
    /// Network-game face-background patch (`STFB#` where `#` is the console
    /// player index).
    faceback: Option<Patch<'static>>,
    /// Arms-grid background patch (`STARMS`).
    armsbg: Option<Patch<'static>>,
    /// Arms patches: `arms[weapon][0]` = dim `STGNUM#`, `arms[weapon][1]` =
    /// bright short digit.
    arms: [[Option<Patch<'static>>; 2]; 6],
    /// Ready-ammo number widget state.
    w_ready: st_number_t,
    /// Frag-count number widget state (deathmatch only).
    w_frags: st_number_t,
    /// Health percent widget state.
    w_health: st_percent_t,
    /// Arms-background binary icon widget state.
    w_armsbg: st_binicon_t,
    /// Per-weapon multi-icon widget states (weapons 2-7).
    w_arms: [st_multicon_t; 6],
    /// Face multi-icon widget state.
    w_faces: st_multicon_t,
    /// Key-slot multi-icon widget states (three slots: blue, yellow, red).
    w_keyboxes: [st_multicon_t; 3],
    /// Armor percent widget state.
    w_armor: st_percent_t,
    /// Per-ammo-type current-ammo number widget states.
    w_ammo: [st_number_t; NUMAMMO],
    /// Per-ammo-type max-ammo number widget states.
    w_maxammo: [st_number_t; NUMAMMO],
    /// Running frag total for the local player, updated each tic.
    st_fragscount: i32,
    /// Player health from the previous tic; used to detect large drops for
    /// the ouch face.
    st_oldhealth: i32,
    /// Snapshot of which weapons the player owned at the previous tic;
    /// detects new pickups.
    oldweaponsowned: [i32; NUMWEAPONS],
    /// Countdown controlling how many tics the current face expression
    /// persists.
    st_facecount: i32,
    /// Current index into `faces[]` that the face widget displays.
    st_faceindex: i32,
    /// Key-slot values: the card/skull index to display, or -1 for empty.
    keyboxes: [i32; 3],
    /// Random number sampled from `M_Random` each tic to vary the idle
    /// straight-face frame.
    st_randomnumber: i32,
    /// Last palette index passed to `I_SetPalette`; avoids redundant calls.
    st_palette: i32,
    /// Non-zero when the status bar subsystem has been stopped via
    /// `ST_Stop`.
    st_stopped: i32,
    /// Pain offset computed for `pain_offset_health` (static local
    /// `lastcalc` in C `ST_calcPainOffset`).
    pain_offset: i32,
    /// Health the cached `pain_offset` belongs to (static local `oldhealth`
    /// in C `ST_calcPainOffset`).
    pain_offset_health: i32,
    /// Tics left until a held trigger shows the rampage face (static local
    /// in C `ST_updateFaceWidget`).
    lastattackdown: i32,
    /// Priority of the face currently shown (static local in C
    /// `ST_updateFaceWidget`).
    priority: i32,
    /// Value the ready-ammo widget shows for weapons without ammo (static
    /// local in C `ST_updateWidgets`).
    largeammo: i32,
}

impl StatusState {
    /// The state at program start: nothing loaded, status bar stopped.
    pub const fn new() -> Self {
        Self {
            st_backing_screen: ptr::null_mut(),
            sttminus: None,
            cheat_mus: cheat(b"idmus", 2),
            cheat_god: cheat(b"iddqd", 0),
            cheat_ammo: cheat(b"idkfa", 0),
            cheat_ammonokey: cheat(b"idfa", 0),
            cheat_noclip: cheat(b"idspispopd", 0),
            cheat_commercial_noclip: cheat(b"idclip", 0),
            cheat_powerup: [
                cheat(b"idbeholdv", 0),
                cheat(b"idbeholds", 0),
                cheat(b"idbeholdi", 0),
                cheat(b"idbeholdr", 0),
                cheat(b"idbeholda", 0),
                cheat(b"idbeholdl", 0),
                cheat(b"idbehold", 0),
            ],
            cheat_choppers: cheat(b"idchoppers", 0),
            cheat_clev: cheat(b"idclev", 2),
            cheat_mypos: cheat(b"idmypos", 0),
            plyr: None,
            st_firsttime: 0,
            lu_palette: 0,
            st_clock: 0,
            st_msgcounter: 0,
            st_chatstate: 0, // StartChatState = 0
            st_gamestate: 0, // AutomapState = 0
            st_statusbaron: 0,
            st_chat: 0,
            st_oldchat: 0,
            st_cursoron: 0,
            st_notdeathmatch: 0,
            st_armson: 0,
            st_fragson: 0,
            sbar: None,
            tallnum: [None; 10],
            tallpercent: None,
            shortnum: [None; 10],
            keys: [None; NUMCARDS],
            faces: [None; ST_NUMFACES as usize],
            faceback: None,
            armsbg: None,
            arms: [[None; 2]; 6],
            w_ready: st_number_t::EMPTY,
            w_frags: st_number_t::EMPTY,
            w_health: st_percent_t::EMPTY,
            w_armsbg: st_binicon_t::EMPTY,
            w_arms: [st_multicon_t::EMPTY; 6],
            w_faces: st_multicon_t::EMPTY,
            w_keyboxes: [st_multicon_t::EMPTY; 3],
            w_armor: st_percent_t::EMPTY,
            w_ammo: [st_number_t::EMPTY; NUMAMMO],
            w_maxammo: [st_number_t::EMPTY; NUMAMMO],
            st_fragscount: 0,
            st_oldhealth: -1,
            oldweaponsowned: [0; NUMWEAPONS],
            st_facecount: 0,
            st_faceindex: 0,
            keyboxes: [0; 3],
            st_randomnumber: 0,
            st_palette: 0,
            st_stopped: 1,
            pain_offset: 0,
            pain_offset_health: -1,
            lastattackdown: -1,
            priority: 0,
            largeammo: 1994,
        }
    }

    /// The player the status bar shows (C `plyr`).
    ///
    /// # Panics
    ///
    /// Before `ST_initData` has set it.
    #[track_caller]
    fn player(&self) -> PlayerId {
        self.plyr.expect("status bar has no player before ST_Start")
    }
}

impl Default for StatusState {
    fn default() -> Self {
        Self::new()
    }
}

/// The status bar state.
pub static STATUS: Global<StatusState> = Global::new(StatusState::new());

/// Feed `key` to the cheat sequence `select` picks out of the status state.
///
/// Returns whether the key completed the sequence.
fn check_cheat(select: impl FnOnce(&mut StatusState) -> &mut cheatseq_t, key: u8) -> bool {
    let mut status = STATUS.borrow_mut();
    cht_CheckCheat(select(&mut status), key) != 0
}

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
pub unsafe fn ST_refreshBackground() {
    let (statusbaron, backing_screen, sbar, faceback) = {
        let status = STATUS.borrow();
        (
            status.st_statusbaron,
            status.st_backing_screen,
            status.sbar,
            status.faceback,
        )
    };
    if statusbaron != 0 {
        V_UseBuffer(backing_screen, (ST_WIDTH * ST_HEIGHT) as usize);
        V_DrawPatch(
            ST_X,
            0,
            sbar.expect("ST_refreshBackground before ST_loadGraphics"),
        );
        if netgame() != 0 {
            V_DrawPatch(
                ST_FX,
                0,
                faceback.expect("ST_refreshBackground before ST_loadGraphics"),
            );
        }
        V_RestoreBuffer();
        V_CopyRect(ST_X, 0, backing_screen, ST_WIDTH, ST_HEIGHT, ST_X, ST_Y);
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
/// initialised `event_t`. Reads and mutates the cheat sequences and the
/// status-bar game state (`st_gamestate`, `st_firsttime`, etc.) in
/// [`STATUS`].
pub unsafe fn ST_Responder(ev: *mut event_t) -> i32 {
    let ev = &*ev;

    if ev.kind == 1 && (ev.data1 as u32 & 0xffff0000) == AM_MSGHEADER as u32 {
        // ev_keyup + automap message
        let mut status = STATUS.borrow_mut();
        match ev.data1 {
            AM_MSGENTERED => {
                status.st_gamestate = 0; // AutomapState
                status.st_firsttime = 1;
            }
            AM_MSGEXITED => {
                status.st_gamestate = 1; // FirstPersonState
            }
            _ => {}
        }
    } else if ev.kind == 0 {
        // ev_keydown
        let key = ev.data2 as u8;
        // Responders run only in a level, after `ST_Start`, and outside any
        // world borrow: the cheats borrow the world for this key.
        let player = STATUS.borrow().player();
        let mut world_guard = WORLD.borrow_mut();
        let world = &mut *world_guard;
        if netgame() == 0 && gameskill() != 4 {
            // sk_nightmare = 4
            let plyr = &mut world.players[player];
            if check_cheat(|status| &mut status.cheat_god, key) {
                plyr.cheats ^= 2; // CF_GODMODE
                if plyr.cheats & 2 != 0 {
                    if let Some(mo) = plyr.mo {
                        (*mo.ptr()).health = 100;
                    }
                    plyr.health = 100; // deh_god_mode_health
                    plyr.message = PlayerMessage::new(DEH_String(STSTR_DQDON));
                } else {
                    plyr.message = PlayerMessage::new(DEH_String(STSTR_DQDOFF));
                }
            } else if check_cheat(|status| &mut status.cheat_ammonokey, key) {
                plyr.armorpoints = 200; // deh_idfa_armor
                plyr.armortype = 2; // deh_idfa_armor_class
                for i in 0..NUMWEAPONS {
                    plyr.weaponowned[i] = 1;
                }
                for i in 0..NUMAMMO {
                    plyr.ammo[i] = plyr.maxammo[i];
                }
                plyr.message = PlayerMessage::new(DEH_String(STSTR_FAADDED));
            } else if check_cheat(|status| &mut status.cheat_ammo, key) {
                plyr.armorpoints = 200; // deh_idkfa_armor
                plyr.armortype = 2; // deh_idkfa_armor_class
                for i in 0..NUMWEAPONS {
                    plyr.weaponowned[i] = 1;
                }
                for i in 0..NUMAMMO {
                    plyr.ammo[i] = plyr.maxammo[i];
                }
                for i in 0..NUMCARDS {
                    plyr.cards[i] = 1;
                }
                plyr.message = PlayerMessage::new(DEH_String(STSTR_KFAADDED));
            } else if check_cheat(|status| &mut status.cheat_mus, key) {
                let mut buf = [0u8; 3];
                let musnum: i32;
                plyr.message = PlayerMessage::new(DEH_String(STSTR_MUS));
                cht_GetParam(&STATUS.borrow().cheat_mus, &mut buf);

                if gamemode() == doom::mode::commercial || gameversion() < doom::mode::exe_ultimate
                {
                    musnum =
                        33 + (buf[0] as i32 - '0' as i32) * 10 + (buf[1] as i32 - '0' as i32) - 1;
                    if ((buf[0] as i32 - '0' as i32) * 10 + (buf[1] as i32 - '0' as i32)) > 35 {
                        plyr.message = PlayerMessage::new(DEH_String(STSTR_NOMUS));
                    } else {
                        S_ChangeMusic(musnum, 1);
                    }
                } else {
                    musnum = 1 + (buf[0] as i32 - '1' as i32) * 9 + (buf[1] as i32 - '1' as i32);
                    if ((buf[0] as i32 - '1' as i32) * 9 + (buf[1] as i32 - '1' as i32)) > 31 {
                        plyr.message = PlayerMessage::new(DEH_String(STSTR_NOMUS));
                    } else {
                        S_ChangeMusic(musnum, 1);
                    }
                }
            } else if (logical_gamemission() == doom::mode::doom
                && check_cheat(|status| &mut status.cheat_noclip, key))
                || (logical_gamemission() != doom::mode::doom
                    && check_cheat(|status| &mut status.cheat_commercial_noclip, key))
            {
                plyr.cheats ^= 1; // CF_NOCLIP
                if plyr.cheats & 1 != 0 {
                    plyr.message = PlayerMessage::new(DEH_String(STSTR_NCON));
                } else {
                    plyr.message = PlayerMessage::new(DEH_String(STSTR_NCOFF));
                }
            }

            for i in 0..6 {
                if check_cheat(|status| &mut status.cheat_powerup[i], key) {
                    if world.players[player].powers[i] == 0 {
                        P_GivePower(world, player, i as i32);
                    } else if i != 1 {
                        // pw_strength = 1
                        world.players[player].powers[i] = 1;
                    } else {
                        world.players[player].powers[i] = 0;
                    }
                    world.players[player].message = PlayerMessage::new(DEH_String(STSTR_BEHOLDX));
                }
            }

            if check_cheat(|status| &mut status.cheat_powerup[6], key) {
                world.players[player].message = PlayerMessage::new(DEH_String(STSTR_BEHOLD));
            } else if check_cheat(|status| &mut status.cheat_choppers, key) {
                let plyr = &mut world.players[player];
                plyr.weaponowned[7] = 1; // wp_chainsaw
                plyr.powers[0] = 1; // pw_invulnerability
                plyr.message = PlayerMessage::new(DEH_String(STSTR_CHOPPERS));
            } else if check_cheat(|status| &mut status.cheat_mypos, key) {
                let mo = world.players[PlayerId::console()].mo().ptr();
                world.players[player].message = PlayerMessage::new(format!(
                    "ang=0x{:x};x,y=(0x{:x},0x{:x})",
                    (*mo).angle,
                    (*mo).x,
                    (*mo).y
                ));
            }
        }

        if netgame() == 0 && check_cheat(|status| &mut status.cheat_clev, key) {
            let mut buf = [0u8; 3];
            let mut epsd: i32;
            let map: i32;
            cht_GetParam(&STATUS.borrow().cheat_clev, &mut buf);

            if gamemode() == doom::mode::commercial {
                epsd = 1;
                map = (buf[0] as i32 - '0' as i32) * 10 + (buf[1] as i32 - '0' as i32);
            } else {
                epsd = buf[0] as i32 - '0' as i32;
                map = buf[1] as i32 - '0' as i32;
            }

            if gameversion() == doom::mode::exe_chex {
                epsd = 1;
            }

            if epsd < 1 {
                return 0;
            }
            if map < 1 {
                return 0;
            }
            if gamemode() == doom::mode::retail && (epsd > 4 || map > 9) {
                return 0;
            }
            if gamemode() == doom::mode::registered && (epsd > 3 || map > 9) {
                return 0;
            }
            if gamemode() == doom::mode::shareware && (epsd > 1 || map > 9) {
                return 0;
            }
            if gamemode() == doom::mode::commercial && (epsd > 1 || map > 40) {
                return 0;
            }

            world.players[player].message = PlayerMessage::new(DEH_String(STSTR_CLEV));
            G_DeferedInitNew(gameskill(), epsd, map);
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
/// computed value is cached in `status` so recalculation only happens when
/// `health` changes.
///
/// `health` is the player's health (C reads `plyr->health`).
pub fn ST_calcPainOffset(status: &mut StatusState, health: i32) -> i32 {
    let health = health.min(100);

    if health != status.pain_offset_health {
        status.pain_offset = ST_FACESTRIDE * (((100 - health) * ST_NUMPAINFACES) / 101);
        status.pain_offset_health = health;
    }
    status.pain_offset
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
/// `plyr` is the status bar's player. Precondition: `oldweaponsowned`
/// matches the snapshot from the previous call.
///
/// # Safety
///
/// The player's `mo` and `attacker` must name map objects of the level
/// (read when computing turn directions).
pub unsafe fn ST_updateFaceWidget(status: &mut StatusState, plyr: &PlayerT) {
    let health = plyr.health;
    let diffang: u32;
    let i: i32;

    if status.priority < 10 && plyr.health == 0 {
        status.priority = 9;
        status.st_faceindex = ST_DEADFACE;
        status.st_facecount = 1;
    }

    if status.priority < 9 && plyr.bonuscount != 0 {
        let mut doevilgrin = 0;
        for i in 0..NUMWEAPONS {
            if status.oldweaponsowned[i] != plyr.weaponowned[i] {
                doevilgrin = 1;
                status.oldweaponsowned[i] = plyr.weaponowned[i];
            }
        }
        if doevilgrin != 0 {
            status.priority = 8;
            status.st_facecount = ST_EVILGRINCOUNT;
            status.st_faceindex = ST_calcPainOffset(status, health) + ST_EVILGRINOFFSET;
        }
    }

    if status.priority < 8
        && plyr.damagecount != 0
        && let Some(attacker) = plyr.attacker
        && Some(attacker) != plyr.mo
    {
        status.priority = 7;
        if plyr.health - status.st_oldhealth > ST_MUCHPAIN {
            status.st_facecount = ST_TURNCOUNT;
            status.st_faceindex = ST_calcPainOffset(status, health) + ST_OUCHOFFSET;
        } else {
            let mo = plyr.mo().ptr();
            let attacker = attacker.ptr();
            let badguyangle = R_PointToAngle2((*mo).x, (*mo).y, (*attacker).x, (*attacker).y);
            if badguyangle > (*mo).angle {
                diffang = badguyangle - (*mo).angle;
                i = if diffang > ANG180 { 1 } else { 0 };
            } else {
                diffang = (*mo).angle - badguyangle;
                i = if diffang <= ANG180 { 1 } else { 0 };
            }

            status.st_facecount = ST_TURNCOUNT;
            status.st_faceindex = ST_calcPainOffset(status, health);

            if diffang < ANG45 {
                status.st_faceindex += ST_RAMPAGEOFFSET;
            } else if i != 0 {
                status.st_faceindex += ST_TURNOFFSET;
            } else {
                status.st_faceindex += ST_TURNOFFSET + 1;
            }
        }
    }

    if status.priority < 7 && plyr.damagecount != 0 {
        if plyr.health - status.st_oldhealth > ST_MUCHPAIN {
            status.priority = 7;
            status.st_facecount = ST_TURNCOUNT;
            status.st_faceindex = ST_calcPainOffset(status, health) + ST_OUCHOFFSET;
        } else {
            status.priority = 6;
            status.st_facecount = ST_TURNCOUNT;
            status.st_faceindex = ST_calcPainOffset(status, health) + ST_RAMPAGEOFFSET;
        }
    }

    if status.priority < 6 {
        if plyr.attackdown != 0 {
            if status.lastattackdown == -1 {
                status.lastattackdown = ST_RAMPAGEDELAY;
            } else {
                status.lastattackdown -= 1;
                if status.lastattackdown == 0 {
                    status.priority = 5;
                    status.st_faceindex = ST_calcPainOffset(status, health) + ST_RAMPAGEOFFSET;
                    status.st_facecount = 1;
                    status.lastattackdown = 1;
                }
            }
        } else {
            status.lastattackdown = -1;
        }
    }

    if status.priority < 5 && ((plyr.cheats & 2) != 0 || plyr.powers[0] != 0) {
        // CF_GODMODE || pw_invulnerability
        status.priority = 4;
        status.st_faceindex = ST_GODFACE;
        status.st_facecount = 1;
    }

    if status.st_facecount == 0 {
        status.st_faceindex = ST_calcPainOffset(status, health) + (status.st_randomnumber % 3);
        status.st_facecount = ST_STRAIGHTFACECOUNT;
        status.priority = 0;
    }

    status.st_facecount -= 1;
}

// ---------------------------------------------------------------------------
// Widget update
// ---------------------------------------------------------------------------

/// Refresh the status-bar widget data and counters from the player state
/// `plyr`.
///
/// Called once per tic by `ST_Ticker`. Records the ready weapon, whose ammo
/// the ready-ammo widget shows (the value 1994 for weapons with no ammo
/// type), key-slot indices, visibility flags for the arms panel and frag
/// counter, the frag total, and the face widget via `ST_updateFaceWidget`.
/// Also decrements `st_msgcounter` and restores `st_chat` when it expires.
///
/// # Safety
///
/// Caller must ensure `ST_createWidgets` has run, and the player's `mo`
/// and `attacker` must name map objects of the level.
pub unsafe fn ST_updateWidgets(status: &mut StatusState, plyr: &PlayerT) {
    // C points the ready-ammo widget at the ammo of the weapon ready now;
    // the weapon recorded here picks that ammo when the widget draws.
    status.w_ready.data = plyr.readyweapon;

    for i in 0..3 {
        status.keyboxes[i] = if plyr.cards[i] != 0 { i as i32 } else { -1 };
        if plyr.cards[i + 3] != 0 {
            status.keyboxes[i] = (i + 3) as i32;
        }
    }

    ST_updateFaceWidget(status, plyr);

    status.st_notdeathmatch = if deathmatch() == 0 { 1 } else { 0 };
    status.st_armson = if status.st_statusbaron != 0 && deathmatch() == 0 {
        1
    } else {
        0
    };
    status.st_fragson = if deathmatch() != 0 && status.st_statusbaron != 0 {
        1
    } else {
        0
    };
    status.st_fragscount = 0;

    let consoleplayer = consoleplayer() as usize;
    for i in 0..MAXPLAYERS {
        if i != consoleplayer {
            status.st_fragscount += plyr.frags[i];
        } else {
            status.st_fragscount -= plyr.frags[i];
        }
    }

    status.st_msgcounter -= 1;
    if status.st_msgcounter == 0 {
        status.st_chat = status.st_oldchat;
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
/// Called once per tic by `G_Ticker` in `g_game.c`, with the world it
/// borrowed for the level phase.
///
/// # Safety
///
/// Mutates `st_clock`, `st_randomnumber`, and `st_oldhealth`, and indirectly
/// touches every widget global via `ST_updateWidgets`. Caller must ensure
/// `ST_Start` has been called, and the player's `mo` and `attacker` must
/// name map objects of the level.
pub unsafe fn ST_Ticker(world: &World) {
    let random_number = M_Random();
    let mut status = STATUS.borrow_mut();
    let plyr = &world.players[status.player()];
    status.st_clock = status.st_clock.wrapping_add(1);
    status.st_randomnumber = random_number;
    ST_updateWidgets(&mut status, plyr);
    status.st_oldhealth = plyr.health;
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
/// `plyr` is the status bar's player.
///
/// # Safety
///
/// Mutates `st_palette`. Calls `W_CacheLumpNum`/`I_SetPalette` so the WAD
/// subsystem and video backend must be initialised, and `lu_palette` must
/// hold a valid lump number set up by `ST_loadData`.
pub unsafe fn ST_doPaletteStuff(plyr: &PlayerT) {
    let mut palette: i32;
    let mut cnt = plyr.damagecount;

    if plyr.powers[1] != 0 {
        // pw_strength
        let bzc = 12 - (plyr.powers[1] >> 6);
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
    } else if plyr.bonuscount != 0 {
        palette = (plyr.bonuscount + 7) >> 3;
        if palette >= NUMBONUSPALS {
            palette = NUMBONUSPALS - 1;
        }
        palette += STARTBONUSPALS;
    } else if plyr.powers[3] > 4 * 32 || plyr.powers[3] & 8 != 0 {
        // pw_ironfeet
        palette = RADIATIONPAL;
    } else {
        palette = 0;
    }

    if gameversion() == doom::mode::exe_chex
        && (STARTREDPALS..STARTREDPALS + NUMREDPALS).contains(&palette)
    {
        palette = RADIATIONPAL;
    }

    let changed_palette_lump = {
        let mut status = STATUS.borrow_mut();
        let changed = palette != status.st_palette;
        status.st_palette = palette;
        changed.then_some(status.lu_palette)
    };
    if let Some(lu_palette) = changed_palette_lump {
        let pal = (W_CacheLumpNum(lu_palette, PU_CACHE) as *mut u8).add((palette * 768) as usize);
        I_SetPalette(pal);
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// Run `update` on a copy of the widget `select` picks out of the status
/// state, then store the copy back. The widget library reads the status state
/// (backing screen, minus patch) while it draws, so the state is not
/// borrowed during `update`.
fn update_widget<Widget: Copy>(
    select: impl Fn(&mut StatusState) -> &mut Widget,
    update: impl FnOnce(&mut Widget),
) {
    let mut widget = *select(&mut STATUS.borrow_mut());
    update(&mut widget);
    *select(&mut STATUS.borrow_mut()) = widget;
}

/// Drive all st_lib widget update calls for a single frame, showing the
/// values of the status bar's player `plyr` and of the status state.
///
/// `refresh` is passed through to each widget: non-zero forces a full redraw,
/// zero redraws only widgets whose value changed since last frame.
/// Updates `st_armson` and `st_fragson` visibility flags before iterating.
///
/// # Safety
///
/// Mutates `st_armson`/`st_fragson` and updates every `w_*` widget through
/// the `STlib_update*` family. Caller must ensure `ST_createWidgets` has
/// been called so the widgets' patch tables are set up.
pub unsafe fn ST_drawWidgets(plyr: &PlayerT, refresh: i32) {
    let status = {
        let mut status = STATUS.borrow_mut();
        status.st_armson = if status.st_statusbaron != 0 && deathmatch() == 0 {
            1
        } else {
            0
        };
        status.st_fragson = if deathmatch() != 0 && status.st_statusbaron != 0 {
            1
        } else {
            0
        };
        // The values the widgets show: none of them changes while they draw.
        StatusValues {
            statusbaron: status.st_statusbaron != 0,
            armson: status.st_armson != 0,
            fragson: status.st_fragson != 0,
            ready_weapon: status.w_ready.data,
            largeammo: status.largeammo,
            notdeathmatch: status.st_notdeathmatch,
            faceindex: status.st_faceindex,
            keyboxes: status.keyboxes,
            fragscount: status.st_fragscount,
        }
    };

    let ready_ammo = weaponinfo[status.ready_weapon as usize].ammo;
    let ready_value = if ready_ammo == 5 {
        // am_noammo
        status.largeammo
    } else {
        plyr.ammo[ready_ammo as usize]
    };
    update_widget(
        |status| &mut status.w_ready,
        |widget| STlib_updateNum(widget, ready_value, status.statusbaron, refresh),
    );

    for i in 0..NUMAMMO {
        update_widget(
            |status| &mut status.w_ammo[i],
            |widget| STlib_updateNum(widget, plyr.ammo[i], status.statusbaron, refresh),
        );
        update_widget(
            |status| &mut status.w_maxammo[i],
            |widget| STlib_updateNum(widget, plyr.maxammo[i], status.statusbaron, refresh),
        );
    }

    update_widget(
        |status| &mut status.w_health,
        |widget| STlib_updatePercent(widget, plyr.health, status.statusbaron, refresh),
    );
    update_widget(
        |status| &mut status.w_armor,
        |widget| STlib_updatePercent(widget, plyr.armorpoints, status.statusbaron, refresh),
    );
    update_widget(
        |status| &mut status.w_armsbg,
        |widget| STlib_updateBinIcon(widget, status.notdeathmatch, status.statusbaron, refresh),
    );

    for i in 0..6 {
        update_widget(
            |status| &mut status.w_arms[i],
            |widget| STlib_updateMultIcon(widget, plyr.weaponowned[i + 1], status.armson, refresh),
        );
    }

    update_widget(
        |status| &mut status.w_faces,
        |widget| STlib_updateMultIcon(widget, status.faceindex, status.statusbaron, refresh),
    );

    for i in 0..3 {
        update_widget(
            |status| &mut status.w_keyboxes[i],
            |widget| STlib_updateMultIcon(widget, status.keyboxes[i], status.statusbaron, refresh),
        );
    }

    update_widget(
        |status| &mut status.w_frags,
        |widget| STlib_updateNum(widget, status.fragscount, status.fragson, refresh),
    );
}

/// The status-state values and visibility flags the widgets show in one
/// frame (the C widgets' `num`/`inum`/`val`/`on` pointer targets).
struct StatusValues {
    /// Whether the bar is shown (`st_statusbaron`).
    statusbaron: bool,
    /// Whether the weapons-owned display is shown (`st_armson`).
    armson: bool,
    /// Whether the frag counter is shown (`st_fragson`).
    fragson: bool,
    /// The ready weapon recorded by the last `ST_updateWidgets`.
    ready_weapon: i32,
    /// The ready-ammo value for weapons without ammo (`largeammo`).
    largeammo: i32,
    /// Whether the arms background is shown (`st_notdeathmatch`).
    notdeathmatch: i32,
    /// The face to show (`st_faceindex`).
    faceindex: i32,
    /// The key icons to show (`keyboxes`).
    keyboxes: [i32; 3],
    /// The frag total (`st_fragscount`).
    fragscount: i32,
}

/// Perform a full status-bar redraw of `plyr`: clear `st_firsttime`, refresh
/// the background, then redraw all widgets.
///
/// # Safety
///
/// Mutates `st_firsttime` and delegates to `ST_refreshBackground` and
/// `ST_drawWidgets`, which require an initialised status bar.
pub unsafe fn ST_doRefresh(plyr: &PlayerT) {
    STATUS.borrow_mut().st_firsttime = 0;
    ST_refreshBackground();
    ST_drawWidgets(plyr, 1);
}

/// Perform a differential redraw of `plyr`: only redraw widgets whose value
/// changed.
///
/// # Safety
///
/// Delegates to `ST_drawWidgets`; same invariants apply (widgets must have
/// been created).
pub unsafe fn ST_diffDraw(plyr: &PlayerT) {
    ST_drawWidgets(plyr, 0);
}

/// Draw the status bar for the current frame.
///
/// `fullscreen` indicates that the view fills the entire screen (no bar),
/// `refresh` forces a complete redraw regardless of dirty state. When
/// `fullscreen` is false or the automap is active, the bar is shown; otherwise
/// it is hidden.
///
/// Called once per frame by the main render loop (`D_Display` in `d_main.c`),
/// outside any world borrow; reads the player through its own shared borrow
/// of the world.
///
/// # Safety
///
/// Mutates `st_statusbaron` and `st_firsttime` and dispatches to
/// `ST_doPaletteStuff`, `ST_doRefresh`, and `ST_diffDraw`. Caller must
/// ensure the status-bar subsystem has been started (`ST_Start`) and the
/// video backend is ready.
pub unsafe fn ST_Drawer(fullscreen: Boolean, refresh: Boolean) {
    let automap_shown = automap_active();
    let player = {
        let mut status = STATUS.borrow_mut();
        status.st_statusbaron = if fullscreen.is_false() || automap_shown {
            1
        } else {
            0
        };
        status.st_firsttime = if status.st_firsttime != 0 || refresh.is_truthy() {
            1
        } else {
            0
        };
        status.player()
    };
    let world = WORLD.borrow();
    let plyr = &world.players[player];
    ST_doPaletteStuff(plyr);
    if STATUS.borrow().st_firsttime != 0 {
        ST_doRefresh(plyr);
    } else {
        ST_diffDraw(plyr);
    }
}

// ---------------------------------------------------------------------------
// Graphics loading / unloading
// ---------------------------------------------------------------------------

/// Function pointer type for the load/unload callback used by `ST_loadUnloadGraphics`.
///
/// The callback receives a WAD lump name and the patch slot that should be
/// updated: either cached (load path) or released and cleared (unload path).
type LoadCallback = unsafe fn(&[u8], &mut Option<Patch<'static>>);

/// Walk every status-bar lump name and invoke `callback` for each.
///
/// Shared by `ST_loadGraphics` (which passes `ST_loadCallback`) and
/// `ST_unloadGraphics` (which passes `ST_unloadCallback`). The face lump names
/// are generated dynamically; all other names are string literals.
///
/// # Safety
///
/// `callback` is invoked as an `unsafe` function and is given slots of
/// the `tallnum`/`shortnum`/`faces`/etc. arrays. Caller must pass a
/// callback that only reads/writes the single patch slot supplied and does
/// not touch the status state. The WAD subsystem must be initialised before
/// the load variant is invoked.
unsafe fn ST_loadUnloadGraphics(callback: LoadCallback) {
    let mut namebuf = [0u8; 9];
    let mut status = STATUS.borrow_mut();

    for i in 0..10i32 {
        c_write!(namebuf, "STTNUM{}", i);
        callback(&namebuf, &mut status.tallnum[i as usize]);
        c_write!(namebuf, "STYSNUM{}", i);
        callback(&namebuf, &mut status.shortnum[i as usize]);
    }

    callback(b"STTPRCNT", &mut status.tallpercent);

    for i in 0..NUMCARDS as i32 {
        c_write!(namebuf, "STKEYS{}", i);
        callback(&namebuf, &mut status.keys[i as usize]);
    }

    callback(b"STARMS", &mut status.armsbg);

    for i in 0..6i32 {
        c_write!(namebuf, "STGNUM{}", i + 2);
        callback(&namebuf, &mut status.arms[i as usize][0]);
        status.arms[i as usize][1] = status.shortnum[(i + 2) as usize];
    }

    c_write!(namebuf, "STFB{}", consoleplayer());
    callback(&namebuf, &mut status.faceback);

    callback(b"STBAR", &mut status.sbar);

    let mut facenum: i32 = 0;
    for i in 0..ST_NUMPAINFACES {
        for j in 0..ST_NUMSTRAIGHTFACES {
            c_write!(namebuf, "STFST{}{}", i, j);
            callback(&namebuf, &mut status.faces[facenum as usize]);
            facenum += 1;
        }
        c_write!(namebuf, "STFTR{}0", i);
        callback(&namebuf, &mut status.faces[facenum as usize]);
        facenum += 1;
        c_write!(namebuf, "STFTL{}0", i);
        callback(&namebuf, &mut status.faces[facenum as usize]);
        facenum += 1;
        c_write!(namebuf, "STFOUCH{}", i);
        callback(&namebuf, &mut status.faces[facenum as usize]);
        facenum += 1;
        c_write!(namebuf, "STFEVL{}", i);
        callback(&namebuf, &mut status.faces[facenum as usize]);
        facenum += 1;
        c_write!(namebuf, "STFKILL{}", i);
        callback(&namebuf, &mut status.faces[facenum as usize]);
        facenum += 1;
    }

    callback(b"STFGOD0", &mut status.faces[facenum as usize]);
    facenum += 1;
    callback(b"STFDEAD0", &mut status.faces[facenum as usize]);
}

/// Load callback: cache the named WAD lump at `PU_STATIC` and store its
/// patch view in `*variable`.
///
/// # Safety
///
/// Caller must ensure `lumpname` names a patch lump present in the WAD. The
/// view is `'static` because the lump stays at `PU_STATIC` until
/// `ST_unloadCallback` releases it and clears the slot.
unsafe fn ST_loadCallback(lumpname: &[u8], variable: &mut Option<Patch<'static>>) {
    *variable = Some(cache_patch_name(lumpname, PU_STATIC));
}

/// Cache all status-bar graphics from the WAD into `PU_STATIC` memory.
///
/// # Safety
///
/// Mutates every cached patch pointer in the status-bar state
/// (`tallnum`, `shortnum`, `tallpercent`, `keys`, `armsbg`, `arms`,
/// `faceback`, `sbar`, `faces`). Caller must ensure the WAD subsystem is
/// initialised and that no other code is reading these pointers concurrently.
pub unsafe fn ST_loadGraphics() {
    ST_loadUnloadGraphics(ST_loadCallback);
}

/// Cache the palette lump number and load all status-bar graphics.
///
/// # Safety
///
/// Writes `lu_palette` and delegates to `ST_loadGraphics`, which mutates
/// the cached patch pointers. Caller must ensure the WAD subsystem is
/// initialised.
pub unsafe fn ST_loadData() {
    let lu_palette = W_GetNumForName(b"PLAYPAL");
    STATUS.borrow_mut().lu_palette = lu_palette;
    ST_loadGraphics();
}

/// Unload callback: release the named WAD lump and clear `*variable`.
///
/// # Safety
///
/// Caller must ensure `lumpname` names a lump previously cached by
/// `ST_loadCallback`, and that no copy of the slot's patch is drawn after
/// the release.
unsafe fn ST_unloadCallback(lumpname: &[u8], variable: &mut Option<Patch<'static>>) {
    W_ReleaseLumpName(lumpname);
    *variable = None;
}

/// Release all status-bar graphics and null their pointers.
///
/// # Safety
///
/// Nulls every cached patch pointer in the status-bar state and releases
/// their WAD lumps. Caller must ensure no other code is currently using
/// those patches (e.g. drawing must be stopped via `ST_Stop`).
pub unsafe fn ST_unloadGraphics() {
    ST_loadUnloadGraphics(ST_unloadCallback);
}

/// Release all status-bar data (currently delegates to `ST_unloadGraphics`).
///
/// # Safety
///
/// Delegates to `ST_unloadGraphics`; same invariants apply (status bar must
/// not currently be drawing).
pub unsafe fn ST_unloadData() {
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
/// Writes to every internal status-bar state field and sets `plyr` to the
/// console player, whose slot in `world` gives the weapon snapshot.
///
/// # Panics
///
/// If `consoleplayer` is not a player slot.
pub fn ST_initData(world: &World) {
    {
        let mut status = STATUS.borrow_mut();
        status.st_firsttime = 1;
        let player = PlayerId::console();
        status.plyr = Some(player);
        status.st_clock = 0;
        status.st_chatstate = 0; // StartChatState
        status.st_gamestate = 1; // FirstPersonState
        status.st_statusbaron = 1;
        status.st_oldchat = 0;
        status.st_chat = 0;
        status.st_cursoron = 0;
        status.st_faceindex = 0;
        status.st_palette = -1;
        status.st_oldhealth = -1;

        status.oldweaponsowned = world.players[player].weaponowned;
        for i in 0..3 {
            status.keyboxes[i] = -1;
        }
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
/// `plyr` is the status bar's player, whose ready weapon the ready-ammo
/// widget starts with.
///
/// # Safety
///
/// Initialises every `w_*` widget global with raw pointers to the cached
/// patch tables. Caller must ensure `ST_initData` and `ST_loadGraphics` ran
/// first so the patches are loaded.
pub unsafe fn ST_createWidgets(plyr: &PlayerT) {
    // The widgets keep pointers to the patch tables, so those point into the
    // stable address of `STATUS`.
    let status = STATUS.as_ptr();
    let tallnum: PatchTable = &raw const (*status).tallnum;
    let shortnum: PatchTable = &raw const (*status).shortnum;
    let keys: PatchTable = &raw const (*status).keys;

    STlib_initNum(
        &mut (*status).w_ready,
        ST_AMMOX,
        ST_AMMOY,
        tallnum,
        ST_AMMOWIDTH,
    );
    (*status).w_ready.data = plyr.readyweapon;

    STlib_initPercent(
        &mut (*status).w_health,
        ST_HEALTHX,
        ST_HEALTHY,
        tallnum,
        (*status).tallpercent,
    );

    STlib_initBinIcon(
        &mut (*status).w_armsbg,
        ST_ARMSBGX,
        ST_ARMSBGY,
        (*status).armsbg,
    );

    for i in 0..6 {
        let i_i = i as i32;
        STlib_initMultIcon(
            &mut (*status).w_arms[i],
            ST_ARMSX + (i_i % 3) * ST_ARMSXSPACE,
            ST_ARMSY + (i_i / 3) * ST_ARMSYSPACE,
            &raw const (*status).arms[i] as PatchTable,
        );
    }

    STlib_initNum(
        &mut (*status).w_frags,
        ST_FRAGSX,
        ST_FRAGSY,
        tallnum,
        ST_FRAGSWIDTH,
    );

    STlib_initMultIcon(
        &mut (*status).w_faces,
        ST_FACESX,
        ST_FACESY,
        &raw const (*status).faces as PatchTable,
    );

    STlib_initPercent(
        &mut (*status).w_armor,
        ST_ARMORX,
        ST_ARMORY,
        tallnum,
        (*status).tallpercent,
    );

    STlib_initMultIcon(&mut (*status).w_keyboxes[0], ST_KEY0X, ST_KEY0Y, keys);
    STlib_initMultIcon(&mut (*status).w_keyboxes[1], ST_KEY1X, ST_KEY1Y, keys);
    STlib_initMultIcon(&mut (*status).w_keyboxes[2], ST_KEY2X, ST_KEY2Y, keys);

    for i in 0..NUMAMMO {
        STlib_initNum(
            &mut (*status).w_ammo[i],
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
            shortnum,
            ST_AMMO0WIDTH,
        );
    }

    for i in 0..NUMAMMO {
        STlib_initNum(
            &mut (*status).w_maxammo[i],
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
            shortnum,
            ST_MAXAMMO0WIDTH,
        );
    }
}

/// Start the status bar subsystem for a new game or level.
///
/// Calls `ST_Stop` if already running, then re-initialises state, creates
/// widgets, and clears `st_stopped`. Called from `P_SpawnPlayer` with the
/// world the play code holds.
///
/// # Safety
///
/// Reads/mutates `st_stopped` and runs `ST_initData`/`ST_createWidgets`,
/// which mutate every status-bar global and require the WAD subsystem
/// to be initialised (see those functions' own safety contracts). Caller must
/// ensure `ST_Init` has run once at startup.
pub unsafe fn ST_Start(world: &World) {
    if STATUS.borrow().st_stopped == 0 {
        ST_Stop();
    }
    ST_initData(world);
    ST_createWidgets(&world.players[PlayerId::console()]);
    STATUS.borrow_mut().st_stopped = 0;
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
pub unsafe fn ST_Stop() {
    let lu_palette = {
        let status = STATUS.borrow();
        if status.st_stopped != 0 {
            return;
        }
        status.lu_palette
    };
    I_SetPalette(W_CacheLumpNum(lu_palette, PU_CACHE) as *mut u8);
    STATUS.borrow_mut().st_stopped = 1;
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
/// which mutates the cached patch pointers. Caller must ensure the WAD and
/// zone-memory subsystems are initialised, and that this function is called
/// exactly once.
pub unsafe fn ST_Init() {
    ST_loadData();
    let backing_screen = Z_Malloc(ST_WIDTH * ST_HEIGHT, PU_STATIC, ptr::null_mut()) as *mut u8;
    STATUS.borrow_mut().st_backing_screen = backing_screen;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pain offset follows the player's health in `ST_FACESTRIDE`
    /// steps and is cached in the state.
    #[test]
    fn pain_offset_tracks_health() {
        let mut status = StatusState::new();
        let pain_offset_at = ST_calcPainOffset;

        assert_eq!(pain_offset_at(&mut status, 100), 0);
        assert_eq!(status.pain_offset_health, 100);

        assert_eq!(
            pain_offset_at(&mut status, 10),
            ST_FACESTRIDE * (((100 - 10) * ST_NUMPAINFACES) / 101)
        );
        assert_eq!(status.pain_offset_health, 10);

        // Health above 100 counts as 100.
        assert_eq!(pain_offset_at(&mut status, 200), 0);
        assert_eq!(status.pain_offset_health, 100);
    }

    /// A fresh state has every cheat sequence unread and the bar stopped.
    #[test]
    fn new_state_starts_stopped_with_unread_cheats() {
        let status = StatusState::new();
        assert_eq!(status.st_stopped, 1);
        assert_eq!(status.st_oldhealth, -1);
        assert_eq!(status.largeammo, 1994);
        assert_eq!(status.cheat_god.chars_read, 0);
        assert_eq!(status.cheat_clev.parameter_chars, 2);
        assert_eq!(&status.cheat_powerup[6].sequence[..8], b"idbehold");
    }
}
