//! Rust port of vendor/doomgeneric/hu_stuff.c.
//!
//! Heads-up display: map title, player messages, and chat input.
//!
//! This module owns the three HUD widgets that are visible during normal play:
//! * `w_message` ([`hu_stext_t`]) - scrolling player messages (ammo pickups,
//!   key cards, etc.).
//! * `w_title` ([`hu_textline_t`]) - current map name shown on the automap.
//! * `w_chat` ([`hu_itext_t`]) - live chat input line in multiplayer games.
//!
//! Exported globals (`chat_macros`, `player_names`, `hu_font`, `mapnames`,
//! `mapnames_commercial`, `chat_on`, `message_dontfuckwithme`, `chat_char`)
//! match the C `extern` declarations in `hu_stuff.h` and are accessed by the
//! C side of the engine.
//!
//! Notable Rust-vs-C differences:
//! * The `DEH_String` macro (Dehacked string substitution) is omitted; all
//!   string literals are used directly.
//! * C `boolean` maps to `c_int`; the `always_off` local uses `c_int` rather
//!   than `bool` to keep the ABI identical.
//! * The `SHORT` macro (little-endian swap) is an identity function on x86_64.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::doom::sounds::Sfx;
use std::ffi::{c_char, c_int, c_void};
use std::ptr;

use crate::doom::d_event::event_t;
use crate::doom::d_mode;
use crate::doom::d_player::{PlayerT, MAXPLAYERS};
use crate::doom::doomkeys::{KEY_ENTER, KEY_ESCAPE, KEY_LALT, KEY_RALT, KEY_RSHIFT};
use crate::doom::doomstat::{gamemission, gameversion};
use crate::doom::hu_lib::{
    hu_itext_t, hu_stext_t, hu_textline_t, HUlib_addCharToTextLine, HUlib_addMessageToSText,
    HUlib_drawIText, HUlib_drawSText, HUlib_drawTextLine, HUlib_eraseIText, HUlib_eraseSText,
    HUlib_eraseTextLine, HUlib_initIText, HUlib_initSText, HUlib_initTextLine, HUlib_keyInIText,
    HUlib_resetIText,
};
use crate::doom::i_timer::TICRATE;
use crate::doom::m_controls::{key_message_refresh, key_multi_msg, key_multi_msgplayer};
use crate::doom::m_misc::M_StringCopy;
use crate::doom::v_video::patch_t;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// ASCII code of the first character in the HUD font patch array (`'!'`).
/// Mirrors `HU_FONTSTART` from `hu_stuff.h`.
pub const HU_FONTSTART: u8 = b'!';

/// ASCII code of the last character in the HUD font patch array (`'_'`).
/// Mirrors `HU_FONTEND` from `hu_stuff.h`.
pub const HU_FONTEND: u8 = b'_';

/// Number of glyphs in the HUD font; equals `HU_FONTEND - HU_FONTSTART + 1`.
/// Determines the size of the [`hu_font`] array. Mirrors `HU_FONTSIZE`.
pub const HU_FONTSIZE: usize = (HU_FONTEND - HU_FONTSTART + 1) as usize;

/// Chat destination constant meaning "send to all players".
/// Values 1-4 target individual players; 5 is the broadcast value.
/// Mirrors `HU_BROADCAST` from `hu_stuff.h`.
pub const HU_BROADCAST: c_int = 5;

/// Screen X position of the message widget (top-left of screen).
/// Mirrors `HU_MSGX` from `hu_stuff.h`.
pub const HU_MSGX: c_int = 0;

/// Screen Y position of the message widget (top of screen).
/// Mirrors `HU_MSGY` from `hu_stuff.h`.
pub const HU_MSGY: c_int = 0;

/// Width of the message widget in characters (unused in this port; kept for
/// parity with the C constant `HU_MSGWIDTH`).
pub const HU_MSGWIDTH: c_int = 64;

/// Height of the message widget in lines.
/// Mirrors `HU_MSGHEIGHT` / `HU_TITLEHEIGHT` from `hu_stuff.h`.
pub const HU_MSGHEIGHT: c_int = 1;

/// Number of game tics a message remains visible before it disappears
/// automatically (4 seconds at the default 35-tic-per-second rate).
/// Mirrors `HU_MSGTIMEOUT` from `hu_stuff.h`.
pub const HU_MSGTIMEOUT: c_int = 4 * TICRATE;

use crate::doom::z_zone::PU_STATIC;
/// Capacity of the outgoing chat-character circular buffer.
/// Must be a power of two so the modular index arithmetic uses bitwise AND.
const QUEUESIZE: usize = 128;

/// Byte-swap for little-endian (SHORT macro from `i_swap.h`).
/// On x86_64 this is an identity cast; the value is returned unchanged.
#[inline(always)]
fn short_swap(v: i16) -> i16 {
    v
}

/// Return the logical game mission, collapsing Chex Quest and HacX variants.
///
/// Replicates the C `logical_gamemission` macro from `doomstat.h`:
/// * `pack_chex` is treated as `doom`.
/// * `pack_hacx` is treated as `doom2`.
/// * All other missions are returned as-is.
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
// String literals
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Exported globals
// ---------------------------------------------------------------------------

/// Default chat macro strings bound to Alt+0 through Alt+9.
///
/// Corresponds to `chat_macros[]` in `hu_stuff.c`. Read by `HU_Responder`
/// when Alt is held and a digit key is pressed during chat mode. Also exported
/// to the C side so that `m_config.c` can bind user-configurable strings.
#[no_mangle]
pub static mut chat_macros: [*mut c_char; 10] = [
    c"No".as_ptr().cast_mut(),
    c"I'm ready to kick butt!".as_ptr().cast_mut(),
    c"I'm OK.".as_ptr().cast_mut(),
    c"I'm not looking too good!".as_ptr().cast_mut(),
    c"Help!".as_ptr().cast_mut(),
    c"You suck!".as_ptr().cast_mut(),
    c"Next time, scumbag...".as_ptr().cast_mut(),
    c"Come here!".as_ptr().cast_mut(),
    c"I'll take care of it.".as_ptr().cast_mut(),
    c"Yes".as_ptr().cast_mut(),
];

/// Display names for each of the four multiplayer player colors.
///
/// Corresponds to `player_names[]` in `hu_stuff.c`. Prepended to incoming
/// chat messages so the recipient can identify the sender. Exported so the
/// C side can substitute Dehacked strings (not implemented in this port).
#[no_mangle]
pub static mut player_names: [*mut c_char; 4] = [
    c"Green: ".as_ptr().cast_mut(),
    c"Indigo: ".as_ptr().cast_mut(),
    c"Brown: ".as_ptr().cast_mut(),
    c"Red: ".as_ptr().cast_mut(),
];

/// Most-recently-transmitted chat character from the local player.
///
/// Written by `HU_queueChatChar` and read by `D_ProcessEvents` / the network
/// layer in `d_net.c`. Corresponds to `chat_char` in `hu_stuff.c`.
#[no_mangle]
pub static mut chat_char: c_char = 0;

/// Loaded HUD font patches, one per glyph from `'!'` to `'_'`.
///
/// Populated by [`HU_Init`] from the `STCFNxxx` WAD lumps. Index `i`
/// corresponds to ASCII code `HU_FONTSTART + i`. Exported for use by
/// `st_stuff.c` and other C modules that reference the font.
#[no_mangle]
pub static mut hu_font: [*mut patch_t; HU_FONTSIZE] = [ptr::null_mut(); HU_FONTSIZE];

/// Non-zero while the chat input widget is active.
///
/// Used as the `on` pointer for `w_chat`; also read by `g_game.c` to decide
/// whether typing should be consumed by the chat system. Corresponds to
/// `chat_on` in `hu_stuff.c`.
#[no_mangle]
pub static mut chat_on: c_int = 0;

/// When non-zero, the next player message is treated as critical and will not
/// be suppressed even if `showMessages` is off.
///
/// Set by the game logic (e.g., picked-up key messages) and cleared after
/// the message is displayed. Corresponds to `message_dontfuckwithme` in
/// `hu_stuff.c`. Also exported so `p_inter.c` can set it from the C side.
#[no_mangle]
pub static mut message_dontfuckwithme: c_int = 0;

/// Display names for Doom 1 / Ultimate Doom maps (E1M1-E4M9 plus stubs).
///
/// Indexed as `mapnames[(gameepisode - 1) * 9 + gamemap - 1]`. The last nine
/// entries are `"NEWLEVEL"` placeholders for episode 4 maps that have no
/// canonical name in the original WAD. Corresponds to `mapnames[]` in
/// `hu_stuff.c` and is also exported for access from the C side.
#[no_mangle]
pub static mut mapnames: [*mut c_char; 45] = [
    c"E1M1: Hangar".as_ptr().cast_mut(),
    c"E1M2: Nuclear Plant".as_ptr().cast_mut(),
    c"E1M3: Toxin Refinery".as_ptr().cast_mut(),
    c"E1M4: Command Control".as_ptr().cast_mut(),
    c"E1M5: Phobos Lab".as_ptr().cast_mut(),
    c"E1M6: Central Processing".as_ptr().cast_mut(),
    c"E1M7: Computer Station".as_ptr().cast_mut(),
    c"E1M8: Phobos Anomaly".as_ptr().cast_mut(),
    c"E1M9: Military Base".as_ptr().cast_mut(),
    c"E2M1: Deimos Anomaly".as_ptr().cast_mut(),
    c"E2M2: Containment Area".as_ptr().cast_mut(),
    c"E2M3: Refinery".as_ptr().cast_mut(),
    c"E2M4: Deimos Lab".as_ptr().cast_mut(),
    c"E2M5: Command Center".as_ptr().cast_mut(),
    c"E2M6: Halls of the Damned".as_ptr().cast_mut(),
    c"E2M7: Spawning Vats".as_ptr().cast_mut(),
    c"E2M8: Tower of Babel".as_ptr().cast_mut(),
    c"E2M9: Fortress of Mystery".as_ptr().cast_mut(),
    c"E3M1: Hell Keep".as_ptr().cast_mut(),
    c"E3M2: Slough of Despair".as_ptr().cast_mut(),
    c"E3M3: Pandemonium".as_ptr().cast_mut(),
    c"E3M4: House of Pain".as_ptr().cast_mut(),
    c"E3M5: Unholy Cathedral".as_ptr().cast_mut(),
    c"E3M6: Mt. Erebus".as_ptr().cast_mut(),
    c"E3M7: Limbo".as_ptr().cast_mut(),
    c"E3M8: Dis".as_ptr().cast_mut(),
    c"E3M9: Warrens".as_ptr().cast_mut(),
    c"E4M1: Hell Beneath".as_ptr().cast_mut(),
    c"E4M2: Perfect Hatred".as_ptr().cast_mut(),
    c"E4M3: Sever The Wicked".as_ptr().cast_mut(),
    c"E4M4: Unruly Evil".as_ptr().cast_mut(),
    c"E4M5: They Will Repent".as_ptr().cast_mut(),
    c"E4M6: Against Thee Wickedly".as_ptr().cast_mut(),
    c"E4M7: And Hell Followed".as_ptr().cast_mut(),
    c"E4M8: Unto The Cruel".as_ptr().cast_mut(),
    c"E4M9: Fear".as_ptr().cast_mut(),
    c"NEWLEVEL".as_ptr().cast_mut(),
    c"NEWLEVEL".as_ptr().cast_mut(),
    c"NEWLEVEL".as_ptr().cast_mut(),
    c"NEWLEVEL".as_ptr().cast_mut(),
    c"NEWLEVEL".as_ptr().cast_mut(),
    c"NEWLEVEL".as_ptr().cast_mut(),
    c"NEWLEVEL".as_ptr().cast_mut(),
    c"NEWLEVEL".as_ptr().cast_mut(),
    c"NEWLEVEL".as_ptr().cast_mut(),
];

/// Display names for Doom II, Plutonia, and TNT maps (96 entries total).
///
/// Layout: indices 0-31 are Doom II maps, 32-63 are Plutonia, 64-95 are TNT.
/// Indexed as `mapnames_commercial[gamemap - 1]` (Doom II) or with a +32/+64
/// offset for the expansion packs. Corresponds to `mapnames_commercial[]` in
/// `hu_stuff.c`.
#[no_mangle]
pub static mut mapnames_commercial: [*mut c_char; 96] = [
    // DOOM 2
    c"level 1: entryway".as_ptr().cast_mut(),
    c"level 2: underhalls".as_ptr().cast_mut(),
    c"level 3: the gantlet".as_ptr().cast_mut(),
    c"level 4: the focus".as_ptr().cast_mut(),
    c"level 5: the waste tunnels".as_ptr().cast_mut(),
    c"level 6: the crusher".as_ptr().cast_mut(),
    c"level 7: dead simple".as_ptr().cast_mut(),
    c"level 8: tricks and traps".as_ptr().cast_mut(),
    c"level 9: the pit".as_ptr().cast_mut(),
    c"level 10: refueling base".as_ptr().cast_mut(),
    c"level 11: 'o' of destruction!".as_ptr().cast_mut(),
    c"level 12: the factory".as_ptr().cast_mut(),
    c"level 13: downtown".as_ptr().cast_mut(),
    c"level 14: the inmost dens".as_ptr().cast_mut(),
    c"level 15: industrial zone".as_ptr().cast_mut(),
    c"level 16: suburbs".as_ptr().cast_mut(),
    c"level 17: tenements".as_ptr().cast_mut(),
    c"level 18: the courtyard".as_ptr().cast_mut(),
    c"level 19: the citadel".as_ptr().cast_mut(),
    c"level 20: gotcha!".as_ptr().cast_mut(),
    c"level 21: nirvana".as_ptr().cast_mut(),
    c"level 22: the catacombs".as_ptr().cast_mut(),
    c"level 23: barrels o' fun".as_ptr().cast_mut(),
    c"level 24: the chasm".as_ptr().cast_mut(),
    c"level 25: bloodfalls".as_ptr().cast_mut(),
    c"level 26: the abandoned mines".as_ptr().cast_mut(),
    c"level 27: monster condo".as_ptr().cast_mut(),
    c"level 28: the spirit world".as_ptr().cast_mut(),
    c"level 29: the living end".as_ptr().cast_mut(),
    c"level 30: icon of sin".as_ptr().cast_mut(),
    c"level 31: wolfenstein".as_ptr().cast_mut(),
    c"level 32: grosse".as_ptr().cast_mut(),
    // Plutonia
    c"level 1: congo".as_ptr().cast_mut(),
    c"level 2: well of souls".as_ptr().cast_mut(),
    c"level 3: aztec".as_ptr().cast_mut(),
    c"level 4: caged".as_ptr().cast_mut(),
    c"level 5: ghost town".as_ptr().cast_mut(),
    c"level 6: baron's lair".as_ptr().cast_mut(),
    c"level 7: caughtyard".as_ptr().cast_mut(),
    c"level 8: realm".as_ptr().cast_mut(),
    c"level 9: abattoire".as_ptr().cast_mut(),
    c"level 10: onslaught".as_ptr().cast_mut(),
    c"level 11: hunted".as_ptr().cast_mut(),
    c"level 12: speed".as_ptr().cast_mut(),
    c"level 13: the crypt".as_ptr().cast_mut(),
    c"level 14: genesis".as_ptr().cast_mut(),
    c"level 15: the twilight".as_ptr().cast_mut(),
    c"level 16: the omen".as_ptr().cast_mut(),
    c"level 17: compound".as_ptr().cast_mut(),
    c"level 18: neurosphere".as_ptr().cast_mut(),
    c"level 19: nme".as_ptr().cast_mut(),
    c"level 20: the death domain".as_ptr().cast_mut(),
    c"level 21: slayer".as_ptr().cast_mut(),
    c"level 22: impossible mission".as_ptr().cast_mut(),
    c"level 23: tombstone".as_ptr().cast_mut(),
    c"level 24: the final frontier".as_ptr().cast_mut(),
    c"level 25: the temple of darkness".as_ptr().cast_mut(),
    c"level 26: bunker".as_ptr().cast_mut(),
    c"level 27: anti-christ".as_ptr().cast_mut(),
    c"level 28: the sewers".as_ptr().cast_mut(),
    c"level 29: odyssey of noises".as_ptr().cast_mut(),
    c"level 30: the gateway of hell".as_ptr().cast_mut(),
    c"level 31: cyberden".as_ptr().cast_mut(),
    c"level 32: go 2 it".as_ptr().cast_mut(),
    // TNT
    c"level 1: system control".as_ptr().cast_mut(),
    c"level 2: human bbq".as_ptr().cast_mut(),
    c"level 3: power control".as_ptr().cast_mut(),
    c"level 4: wormhole".as_ptr().cast_mut(),
    c"level 5: hanger".as_ptr().cast_mut(),
    c"level 6: open season".as_ptr().cast_mut(),
    c"level 7: prison".as_ptr().cast_mut(),
    c"level 8: metal".as_ptr().cast_mut(),
    c"level 9: stronghold".as_ptr().cast_mut(),
    c"level 10: redemption".as_ptr().cast_mut(),
    c"level 11: storage facility".as_ptr().cast_mut(),
    c"level 12: crater".as_ptr().cast_mut(),
    c"level 13: nukage processing".as_ptr().cast_mut(),
    c"level 14: steel works".as_ptr().cast_mut(),
    c"level 15: dead zone".as_ptr().cast_mut(),
    c"level 16: deepest reaches".as_ptr().cast_mut(),
    c"level 17: processing area".as_ptr().cast_mut(),
    c"level 18: mill".as_ptr().cast_mut(),
    c"level 19: shipping/respawning".as_ptr().cast_mut(),
    c"level 20: central processing".as_ptr().cast_mut(),
    c"level 21: administration center".as_ptr().cast_mut(),
    c"level 22: habitat".as_ptr().cast_mut(),
    c"level 23: lunar mining project".as_ptr().cast_mut(),
    c"level 24: quarry".as_ptr().cast_mut(),
    c"level 25: baron's den".as_ptr().cast_mut(),
    c"level 26: ballistyx".as_ptr().cast_mut(),
    c"level 27: mount pain".as_ptr().cast_mut(),
    c"level 28: heck".as_ptr().cast_mut(),
    c"level 29: river styx".as_ptr().cast_mut(),
    c"level 30: last call".as_ptr().cast_mut(),
    c"level 31: pharaoh".as_ptr().cast_mut(),
    c"level 32: caribbean".as_ptr().cast_mut(),
];

// ---------------------------------------------------------------------------
// Local statics
// ---------------------------------------------------------------------------

/// Pointer to the console player's `PlayerT` struct, set by [`HU_Start`].
static mut plr: *mut PlayerT = ptr::null_mut();
/// Map-title text line widget rendered on the automap overlay.
static mut w_title: hu_textline_t = unsafe { std::mem::zeroed() };
/// Chat input widget; visible only when [`chat_on`] is non-zero.
static mut w_chat: hu_itext_t = unsafe { std::mem::zeroed() };
/// Permanent zero used as the `on` pointer for [`w_inputbuffer`] widgets so
/// they are never rendered to the screen.
static mut always_off: c_int = 0;
/// Per-player chat destination byte: 1-4 target that player, 5 = broadcast.
static mut chat_dest: [c_char; MAXPLAYERS] = [0; MAXPLAYERS];
/// Hidden input buffer widgets that accumulate incoming chat keystrokes for
/// each remote player before the message is confirmed with Enter.
static mut w_inputbuffer: [hu_itext_t; MAXPLAYERS] = unsafe { std::mem::zeroed() };
/// Non-zero while a player message is being displayed.
/// Used as the `on` pointer for [`w_message`].
static mut message_on: c_int = 0;
/// Non-zero when the current message must not be overridden by a non-critical
/// new message. Set when a `message_dontfuckwithme` message is displayed.
static mut message_nottobefuckedwith: c_int = 0;
/// Scrolling player-message widget displayed at the top of the screen.
static mut w_message: hu_stext_t = unsafe { std::mem::zeroed() };
/// Countdown timer (in tics) until the current message is hidden.
/// Reset to `HU_MSGTIMEOUT` each time a new message is posted.
static mut message_counter: c_int = 0;
/// True after a successful [`HU_Start`]; used by [`HU_Start`] to call
/// [`HU_Stop`] before re-initializing all widgets.
static mut headsupactive: bool = false;

/// Circular buffer of outgoing chat characters, shared with the network layer.
static mut chatchars: [c_char; QUEUESIZE] = [0; QUEUESIZE];
/// Write index into [`chatchars`]; advanced by [`HU_queueChatChar`].
static mut head: usize = 0;
/// Read index into [`chatchars`]; advanced by [`HU_dequeueChatChar`].
static mut tail: usize = 0;

extern "C" {
    /// Load a WAD lump by name and return a pointer to its data (w_wad.c).
    fn W_CacheLumpName(name: *const c_char, tag: c_int) -> *mut c_void;
    /// Start a sound effect, optionally at a map-object origin (s_sound.c).
    fn S_StartSound(origin_p: *mut c_void, sfx_id: c_int);
    /// Non-zero while the automap overlay is active (am_map.c).
    static mut automapactive: c_int;
    /// Non-zero for network games (d_net.c).
    static mut netgame: c_int;
    /// Per-player presence flags; `playeringame[i]` is non-zero if player `i`
    /// is active (doomstat.c / g_game.c).
    static mut playeringame: [c_int; MAXPLAYERS];
    /// Index of the local player (0-3) (doomstat.c).
    static mut consoleplayer: c_int;
    /// Non-zero if the "show messages" option is enabled (doomstat.c).
    static mut showMessages: c_int;
    /// Current game mode (retail, shareware, commercial, etc.) (doomstat.c).
    static mut gamemode: c_int;
    /// Current episode number (1-4) for Doom 1 (doomstat.c).
    static mut gameepisode: c_int;
    /// Current map number within the episode (doomstat.c).
    static mut gamemap: c_int;
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Load the HUD font patches from the WAD and populate [`hu_font`].
///
/// Must be called once at startup before any other `HU_*` function. Iterates
/// over `HU_FONTSIZE` slots, caching the `STCFNxxx` lump for each character
/// from `HU_FONTSTART` to `HU_FONTEND` as `PU_STATIC`.
/// Called from `D_DoomMain` in `d_main.c`.
#[no_mangle]
pub extern "C" fn HU_Init() {
    unsafe {
        let mut j = HU_FONTSTART as c_int;
        for i in 0..HU_FONTSIZE {
            let name = format!("STCFN{:03}\0", j);
            hu_font[i] = W_CacheLumpName(name.as_ptr() as *const c_char, PU_STATIC) as *mut patch_t;
            j += 1;
        }
    }
}

/// Mark the heads-up display as inactive.
///
/// Sets `headsupactive` to false. Called by [`HU_Start`] before
/// re-initializing widgets, and may also be called directly when the HUD
/// should be torn down (e.g., during intermission).
#[no_mangle]
pub extern "C" fn HU_Stop() {
    unsafe {
        headsupactive = false;
    }
}

/// Initialize all HUD widgets for the current level.
///
/// Calls [`HU_Stop`] if already active, then creates and positions the
/// message widget (`w_message`), title widget (`w_title`), chat input widget
/// (`w_chat`), and per-player input buffer widgets. The map title string is
/// looked up from [`mapnames`] / [`mapnames_commercial`] based on the current
/// game mission and map number.
///
/// Called from `G_DoLoadLevel` in `g_game.c` at the start of every level.
#[no_mangle]
pub extern "C" fn HU_Start() {
    unsafe {
        if headsupactive {
            HU_Stop();
        }

        plr = std::ptr::addr_of_mut!(crate::doom::d_player::players[0])
            .offset(consoleplayer as isize);
        message_on = 0;
        message_dontfuckwithme = 0;
        message_nottobefuckedwith = 0;
        chat_on = 0;

        // create the message widget
        HUlib_initSText(
            &raw mut w_message,
            HU_MSGX,
            HU_MSGY,
            HU_MSGHEIGHT,
            std::ptr::addr_of_mut!(hu_font[0]),
            HU_FONTSTART as c_int,
            &raw mut message_on,
        );

        // compute font height for title/input placement
        let font_h = if !hu_font[0].is_null() {
            short_swap((*hu_font[0]).height) as c_int
        } else {
            0
        };
        let title_y = 167 - font_h;

        // create the map title widget
        HUlib_initTextLine(
            &raw mut w_title,
            0,
            title_y,
            std::ptr::addr_of_mut!(hu_font[0]),
            HU_FONTSTART as c_int,
        );

        let mut s: *mut c_char = match logical_gamemission() {
            d_mode::doom => mapnames[((gameepisode - 1) * 9 + gamemap - 1) as usize],
            d_mode::doom2 => mapnames_commercial[(gamemap - 1) as usize],
            d_mode::pack_plut => mapnames_commercial[(gamemap - 1 + 32) as usize],
            d_mode::pack_tnt => mapnames_commercial[(gamemap - 1 + 64) as usize],
            _ => c"Unknown level".as_ptr().cast_mut(),
        };

        if gameversion == d_mode::exe_chex {
            s = mapnames[(gamemap - 1) as usize];
        }

        // dehacked substitution is identity in this build
        while *s != 0 {
            HUlib_addCharToTextLine(&raw mut w_title, *s);
            s = s.add(1);
        }

        // create the chat widget
        let input_y = HU_MSGY + HU_MSGHEIGHT * (font_h + 1);
        HUlib_initIText(
            &raw mut w_chat,
            HU_MSGX,
            input_y,
            std::ptr::addr_of_mut!(hu_font[0]),
            HU_FONTSTART as c_int,
            &raw mut chat_on,
        );

        // create the inputbuffer widgets
        for i in 0..MAXPLAYERS {
            HUlib_initIText(
                &mut w_inputbuffer[i],
                0,
                0,
                ptr::null_mut(),
                0,
                &raw mut always_off,
            );
        }

        headsupactive = true;
    }
}

/// Render all visible HUD elements for the current frame.
///
/// Draws the scrolling message widget, the chat input widget, and (if the
/// automap is active) the map title line. Called once per frame from the
/// renderer after the view has been rendered.
#[no_mangle]
pub extern "C" fn HU_Drawer() {
    unsafe {
        HUlib_drawSText(&raw mut w_message);
        HUlib_drawIText(&raw mut w_chat);
        if automapactive != 0 {
            HUlib_drawTextLine(&raw mut w_title, 0);
        }
    }
}

/// Erase all HUD elements from the screen buffer.
///
/// Delegates to the erase functions for `w_message`, `w_chat`, and `w_title`.
/// Called once per frame just before the view is rendered, so that old HUD
/// graphics are removed before the new frame is drawn.
#[no_mangle]
pub extern "C" fn HU_Erase() {
    HUlib_eraseSText(&raw mut w_message);
    HUlib_eraseIText(&raw mut w_chat);
    HUlib_eraseTextLine(&raw mut w_title);
}

/// Advance the HUD state by one game tic.
///
/// Performs three tasks each tic:
/// 1. Counts down `message_counter`; clears `message_on` and
///    `message_nottobefuckedwith` when it reaches zero.
/// 2. Posts a new player message from `(*plr).message` to `w_message` if
///    `showMessages` is on (or `message_dontfuckwithme` is set) and the
///    message has not already been displayed.
/// 3. In networked games, reads incoming chat characters from each remote
///    player's `cmd.chatchar`, feeds them into the corresponding input buffer
///    widget, and displays the completed message when Enter is received.
///
/// Called from `G_Ticker` in `g_game.c` once per game tic.
#[no_mangle]
pub extern "C" fn HU_Ticker() {
    unsafe {
        // tick down message counter if message is up
        if message_counter != 0 {
            message_counter -= 1;
            if message_counter == 0 {
                message_on = 0;
                message_nottobefuckedwith = 0;
            }
        }

        if showMessages != 0 || message_dontfuckwithme != 0 {
            // display message if necessary
            if (!(*plr).message.is_null() && message_nottobefuckedwith == 0)
                || (!(*plr).message.is_null() && message_dontfuckwithme != 0)
            {
                HUlib_addMessageToSText(&raw mut w_message, ptr::null_mut(), (*plr).message);
                (*plr).message = ptr::null_mut();
                message_on = 1;
                message_counter = HU_MSGTIMEOUT;
                message_nottobefuckedwith = message_dontfuckwithme;
                message_dontfuckwithme = 0;
            }
        }

        // check for incoming chat characters
        if netgame != 0 {
            for i in 0..MAXPLAYERS {
                if playeringame[i] == 0 {
                    continue;
                }
                if i != consoleplayer as usize {
                    let c = (*plr.add(i)).cmd.chatchar;
                    if c != 0 {
                        if c <= HU_BROADCAST as u8 {
                            chat_dest[i] = c as c_char;
                        } else {
                            let rc = HUlib_keyInIText(&mut w_inputbuffer[i], c);
                            if rc != 0 && c == KEY_ENTER {
                                if w_inputbuffer[i].l.len != 0
                                    && (chat_dest[i] == (consoleplayer + 1) as c_char
                                        || chat_dest[i] == HU_BROADCAST as c_char)
                                {
                                    HUlib_addMessageToSText(
                                        &raw mut w_message,
                                        player_names[i],
                                        w_inputbuffer[i].l.l.as_mut_ptr(),
                                    );
                                    message_nottobefuckedwith = 1;
                                    message_on = 1;
                                    message_counter = HU_MSGTIMEOUT;
                                    if gamemode == d_mode::commercial {
                                        S_StartSound(ptr::null_mut(), Sfx::Radio as c_int);
                                    } else {
                                        S_StartSound(ptr::null_mut(), Sfx::Tink as c_int);
                                    }
                                }
                                HUlib_resetIText(&mut w_inputbuffer[i]);
                            }
                        }
                        (*plr.add(i)).cmd.chatchar = 0;
                    }
                }
            }
        }
    }
}

/// Enqueue a chat character into the outgoing circular buffer.
///
/// If the buffer is full (writing would overlap `tail`), posts an
/// `"[Message unsent]"` notice to the player instead of enqueuing.
/// The head index advances modulo `QUEUESIZE` (128; power-of-two bitmask).
/// Called from `HU_Responder` and the macro-expansion path.
#[no_mangle]
pub extern "C" fn HU_queueChatChar(c: c_char) {
    unsafe {
        if ((head + 1) & (QUEUESIZE - 1)) == tail {
            (*plr).message = c"[Message unsent]".as_ptr().cast_mut();
        } else {
            chatchars[head] = c;
            head = (head + 1) & (QUEUESIZE - 1);
        }
    }
}

/// Dequeue the next chat character from the outgoing circular buffer.
///
/// Returns the character at `tail` and advances `tail` modulo `QUEUESIZE` (128).
/// Returns `0` (NUL) if the buffer is empty (`head == tail`).
/// Called by the network layer (`d_net.c`) to read queued chat characters.
#[no_mangle]
pub extern "C" fn HU_dequeueChatChar() -> c_char {
    unsafe {
        if head != tail {
            let c = chatchars[tail];
            tail = (tail + 1) & (QUEUESIZE - 1);
            c
        } else {
            0
        }
    }
}

/// Handle a keyboard event for the HUD system.
///
/// Returns 1 if the event was consumed (and should not be passed to the game),
/// 0 otherwise. Handles the following cases:
/// * Shift, RAlt, LAlt: track modifier state; not consumed.
/// * Non-keydown events: ignored (return 0).
/// * When chat is off: message-refresh key, broadcast-chat key, and
///   player-targeted chat keys (multiplayer only).
/// * When chat is on: Alt+digit sends a chat macro; other printable keys are
///   fed to the chat input widget and queued for transmission; Enter confirms
///   and sends the message; Escape cancels.
///
/// Called from `G_Responder` in `g_game.c`.
#[no_mangle]
pub extern "C" fn HU_Responder(ev: *mut event_t) -> c_int {
    unsafe {
        static mut lastmessage: [c_char; 81] = [0; 81];
        static mut altdown: bool = false;
        static mut num_nobrainers: c_int = 0;

        let ev = &mut *ev;

        let mut numplayers = 0;
        for i in 0..MAXPLAYERS {
            if playeringame[i] != 0 {
                numplayers += 1;
            }
        }

        if ev.data1 == KEY_RSHIFT as c_int {
            return 0;
        } else if ev.data1 == KEY_RALT as c_int || ev.data1 == KEY_LALT as c_int {
            altdown = ev.type_ == 0; // ev_keydown
            return 0;
        }

        if ev.type_ != 0 {
            // not ev_keydown
            return 0;
        }

        let mut eatkey = false;

        if chat_on == 0 {
            if ev.data1 == key_message_refresh {
                message_on = 1;
                message_counter = HU_MSGTIMEOUT;
                eatkey = true;
            } else if netgame != 0 && ev.data2 == key_multi_msg {
                eatkey = true;
                chat_on = 1;
                HUlib_resetIText(&raw mut w_chat);
                HU_queueChatChar(HU_BROADCAST as c_char);
            } else if netgame != 0 && numplayers > 2 {
                for i in 0..MAXPLAYERS {
                    if ev.data2 == key_multi_msgplayer[i] {
                        if playeringame[i] != 0 && i != consoleplayer as usize {
                            eatkey = true;
                            chat_on = 1;
                            HUlib_resetIText(&raw mut w_chat);
                            HU_queueChatChar((i + 1) as c_char);
                            break;
                        } else if i == consoleplayer as usize {
                            num_nobrainers += 1;
                            (*plr).message = match num_nobrainers {
                                1..=2 => c"You mumble to yourself".as_ptr().cast_mut(),
                                3..=5 => c"Who's there?".as_ptr().cast_mut(),
                                6..=8 => c"You scare yourself".as_ptr().cast_mut(),
                                9..=31 => c"You start to rave".as_ptr().cast_mut(),
                                _ => c"You've lost it...".as_ptr().cast_mut(),
                            };
                        }
                    }
                }
            }
        } else {
            // send a macro
            if altdown {
                let c = (ev.data1 - b'0' as c_int) as u8;
                if c > 9 {
                    return 0;
                }
                let macromessage = chat_macros[c as usize];
                HU_queueChatChar(KEY_ENTER as c_char);
                let mut p = macromessage;
                while *p != 0 {
                    HU_queueChatChar(*p);
                    p = p.add(1);
                }
                HU_queueChatChar(KEY_ENTER as c_char);
                chat_on = 0;
                M_StringCopy(std::ptr::addr_of_mut!(lastmessage[0]), macromessage, 81);
                (*plr).message = std::ptr::addr_of_mut!(lastmessage[0]);
                eatkey = true;
            } else {
                let c = ev.data2 as u8;
                eatkey = HUlib_keyInIText(&raw mut w_chat, c) != 0;
                if eatkey {
                    HU_queueChatChar(c as c_char);
                }
                if c == KEY_ENTER {
                    chat_on = 0;
                    if w_chat.l.len != 0 {
                        M_StringCopy(
                            std::ptr::addr_of_mut!(lastmessage[0]),
                            std::ptr::addr_of_mut!(w_chat.l.l[0]),
                            81,
                        );
                        (*plr).message = std::ptr::addr_of_mut!(lastmessage[0]);
                    }
                } else if c == KEY_ESCAPE {
                    chat_on = 0;
                }
            }
        }

        if eatkey {
            1
        } else {
            0
        }
    }
}
