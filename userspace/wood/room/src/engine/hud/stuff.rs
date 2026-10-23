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
//! Exported items (`player_names`, `mapnames`, `mapnames_commercial`, and
//! the [`HudState`] fields `chat_macros`, `hu_font`, `chat_on`,
//! `message_dontfuckwithme`) match the C `extern` declarations in
//! `hu_stuff.h`.
//!
//! Notable Rust-vs-C differences:
//! * C `boolean` maps to `i32`; the `always_off` local uses `i32` rather
//!   than `bool` to keep the ABI identical.
//! * The `SHORT` macro (little-endian swap) is an identity function on x86_64.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use {
    crate::engine::sound::info::Sfx,
    std::{borrow::Cow, ptr},
};

use crate::engine::{
    dehacked::DEH_String,
    doom::{
        self,
        event::event_t,
        player::{MAXPLAYERS, PlayerMessage, PlayerT},
        stat::{gamemission, gameversion},
    },
    global::Global,
    hud::lib::{
        Font, HUlib_addCharToTextLine, HUlib_addMessageToSText, HUlib_drawIText, HUlib_drawSText,
        HUlib_drawTextLine, HUlib_eraseIText, HUlib_eraseSText, HUlib_eraseTextLine,
        HUlib_initIText, HUlib_initSText, HUlib_initTextLine, HUlib_keyInIText, HUlib_resetIText,
        hu_itext_t, hu_stext_t, hu_textline_t,
    },
    interface::{
        keys::{KEY_ENTER, KEY_ESCAPE, KEY_LALT, KEY_RALT, KEY_RSHIFT},
        timer::TICRATE,
    },
    misc::{
        controls::{CONTROLS, ControlsState},
        until_nul,
    },
    wad::cache_patch_name,
};

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
pub const HU_BROADCAST: i32 = 5;

/// Screen X position of the message widget (top-left of screen).
/// Mirrors `HU_MSGX` from `hu_stuff.h`.
pub const HU_MSGX: i32 = 0;

/// Screen Y position of the message widget (top of screen).
/// Mirrors `HU_MSGY` from `hu_stuff.h`.
pub const HU_MSGY: i32 = 0;

/// Width of the message widget in characters (unused in this port; kept for
/// parity with the C constant `HU_MSGWIDTH`).
pub const HU_MSGWIDTH: i32 = 64;

/// Height of the message widget in lines.
/// Mirrors `HU_MSGHEIGHT` / `HU_TITLEHEIGHT` from `hu_stuff.h`.
pub const HU_MSGHEIGHT: i32 = 1;

/// Number of game tics a message remains visible before it disappears
/// automatically (4 seconds at the default 35-tic-per-second rate).
/// Mirrors `HU_MSGTIMEOUT` from `hu_stuff.h`.
pub const HU_MSGTIMEOUT: i32 = 4 * TICRATE;

use crate::engine::zone::PU_STATIC;
/// Capacity of the outgoing chat-character circular buffer.
/// Must be a power of two so the modular index arithmetic uses bitwise AND.
const QUEUESIZE: usize = 128;

/// Return the logical game mission, collapsing Chex Quest and HacX variants.
///
/// Replicates the C `logical_gamemission` macro from `doomstat.h`:
/// * `pack_chex` is treated as `doom`.
/// * `pack_hacx` is treated as `doom2`.
/// * All other missions are returned as-is.
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
// String literals
// ---------------------------------------------------------------------------

/// Shown when the outgoing chat queue is full (C `HUSTR_MSGU`).
const HUSTR_MSGU: &str = "[Message unsent]";
/// Replies to chatting with yourself, escalating with each try (C
/// `HUSTR_TALKTOSELF1`..`5`).
const HUSTR_TALKTOSELF1: &str = "You mumble to yourself";
/// Second-tier reply to chatting with yourself.
const HUSTR_TALKTOSELF2: &str = "Who's there?";
/// Third-tier reply to chatting with yourself.
const HUSTR_TALKTOSELF3: &str = "You scare yourself";
/// Fourth-tier reply to chatting with yourself.
const HUSTR_TALKTOSELF4: &str = "You start to rave";
/// Final reply to chatting with yourself.
const HUSTR_TALKTOSELF5: &str = "You've lost it...";

// ---------------------------------------------------------------------------
// Exported tables
// ---------------------------------------------------------------------------

/// Display names for each of the four multiplayer player colors.
///
/// Corresponds to `player_names[]` in `hu_stuff.c`. Prepended to incoming
/// chat messages so the recipient can identify the sender.
pub const player_names: [&str; 4] = ["Green: ", "Indigo: ", "Brown: ", "Red: "];

/// Display names for Doom 1 / Ultimate Doom maps (E1M1-E4M9 plus stubs).
///
/// Indexed as `mapnames[(gameepisode - 1) * 9 + gamemap - 1]`. The last nine
/// entries are `"NEWLEVEL"` placeholders for episode 4 maps that have no
/// canonical name in the original WAD. Corresponds to `mapnames[]` in
/// `hu_stuff.c`.
pub const mapnames: [&str; 45] = [
    "E1M1: Hangar",
    "E1M2: Nuclear Plant",
    "E1M3: Toxin Refinery",
    "E1M4: Command Control",
    "E1M5: Phobos Lab",
    "E1M6: Central Processing",
    "E1M7: Computer Station",
    "E1M8: Phobos Anomaly",
    "E1M9: Military Base",
    "E2M1: Deimos Anomaly",
    "E2M2: Containment Area",
    "E2M3: Refinery",
    "E2M4: Deimos Lab",
    "E2M5: Command Center",
    "E2M6: Halls of the Damned",
    "E2M7: Spawning Vats",
    "E2M8: Tower of Babel",
    "E2M9: Fortress of Mystery",
    "E3M1: Hell Keep",
    "E3M2: Slough of Despair",
    "E3M3: Pandemonium",
    "E3M4: House of Pain",
    "E3M5: Unholy Cathedral",
    "E3M6: Mt. Erebus",
    "E3M7: Limbo",
    "E3M8: Dis",
    "E3M9: Warrens",
    "E4M1: Hell Beneath",
    "E4M2: Perfect Hatred",
    "E4M3: Sever The Wicked",
    "E4M4: Unruly Evil",
    "E4M5: They Will Repent",
    "E4M6: Against Thee Wickedly",
    "E4M7: And Hell Followed",
    "E4M8: Unto The Cruel",
    "E4M9: Fear",
    "NEWLEVEL",
    "NEWLEVEL",
    "NEWLEVEL",
    "NEWLEVEL",
    "NEWLEVEL",
    "NEWLEVEL",
    "NEWLEVEL",
    "NEWLEVEL",
    "NEWLEVEL",
];

/// Display names for Doom II, Plutonia, and TNT maps (96 entries total).
///
/// Layout: indices 0-31 are Doom II maps, 32-63 are Plutonia, 64-95 are TNT.
/// Indexed as `mapnames_commercial[gamemap - 1]` (Doom II) or with a +32/+64
/// offset for the expansion packs. Corresponds to `mapnames_commercial[]` in
/// `hu_stuff.c`.
pub const mapnames_commercial: [&str; 96] = [
    // DOOM 2
    "level 1: entryway",
    "level 2: underhalls",
    "level 3: the gantlet",
    "level 4: the focus",
    "level 5: the waste tunnels",
    "level 6: the crusher",
    "level 7: dead simple",
    "level 8: tricks and traps",
    "level 9: the pit",
    "level 10: refueling base",
    "level 11: 'o' of destruction!",
    "level 12: the factory",
    "level 13: downtown",
    "level 14: the inmost dens",
    "level 15: industrial zone",
    "level 16: suburbs",
    "level 17: tenements",
    "level 18: the courtyard",
    "level 19: the citadel",
    "level 20: gotcha!",
    "level 21: nirvana",
    "level 22: the catacombs",
    "level 23: barrels o' fun",
    "level 24: the chasm",
    "level 25: bloodfalls",
    "level 26: the abandoned mines",
    "level 27: monster condo",
    "level 28: the spirit world",
    "level 29: the living end",
    "level 30: icon of sin",
    "level 31: wolfenstein",
    "level 32: grosse",
    // Plutonia
    "level 1: congo",
    "level 2: well of souls",
    "level 3: aztec",
    "level 4: caged",
    "level 5: ghost town",
    "level 6: baron's lair",
    "level 7: caughtyard",
    "level 8: realm",
    "level 9: abattoire",
    "level 10: onslaught",
    "level 11: hunted",
    "level 12: speed",
    "level 13: the crypt",
    "level 14: genesis",
    "level 15: the twilight",
    "level 16: the omen",
    "level 17: compound",
    "level 18: neurosphere",
    "level 19: nme",
    "level 20: the death domain",
    "level 21: slayer",
    "level 22: impossible mission",
    "level 23: tombstone",
    "level 24: the final frontier",
    "level 25: the temple of darkness",
    "level 26: bunker",
    "level 27: anti-christ",
    "level 28: the sewers",
    "level 29: odyssey of noises",
    "level 30: the gateway of hell",
    "level 31: cyberden",
    "level 32: go 2 it",
    // TNT
    "level 1: system control",
    "level 2: human bbq",
    "level 3: power control",
    "level 4: wormhole",
    "level 5: hanger",
    "level 6: open season",
    "level 7: prison",
    "level 8: metal",
    "level 9: stronghold",
    "level 10: redemption",
    "level 11: storage facility",
    "level 12: crater",
    "level 13: nukage processing",
    "level 14: steel works",
    "level 15: dead zone",
    "level 16: deepest reaches",
    "level 17: processing area",
    "level 18: mill",
    "level 19: shipping/respawning",
    "level 20: central processing",
    "level 21: administration center",
    "level 22: habitat",
    "level 23: lunar mining project",
    "level 24: quarry",
    "level 25: baron's den",
    "level 26: ballistyx",
    "level 27: mount pain",
    "level 28: heck",
    "level 29: river styx",
    "level 30: last call",
    "level 31: pharaoh",
    "level 32: caribbean",
];

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Heads-up display state (C `hu_stuff.c` globals).
///
/// The widgets keep raw pointers to their `on` flags, and the `HUlib_*`
/// calls take the widgets by pointer, so those go through
/// [`Global::as_ptr`] rather than a guard.
pub struct HudState {
    /// Default chat macro strings bound to Alt+0 through Alt+9.
    ///
    /// Corresponds to `chat_macros[]` in `hu_stuff.c`. Read by
    /// `HU_Responder` when Alt is held and a digit key is pressed during
    /// chat mode. Bound as `chatmacro0`..`chatmacro9` so `m_config` can
    /// replace them.
    pub chat_macros: [Cow<'static, str>; 10],
    /// Loaded HUD font patches, one per glyph from `'!'` to `'_'`.
    ///
    /// Populated by [`HU_Init`] from the `STCFNxxx` WAD lumps; empty before
    /// that. Index `i` corresponds to ASCII code `HU_FONTSTART + i`. Also
    /// read by the menu and the finale text screens.
    pub hu_font: Font,
    /// Non-zero while the chat input widget is active.
    ///
    /// Used as the `on` pointer for `w_chat`; also read by the menu to
    /// decide whether the automap or chat owns the keyboard. Corresponds to
    /// `chat_on` in `hu_stuff.c`.
    pub chat_on: i32,
    /// When non-zero, the next player message is treated as critical and
    /// will not be suppressed even if `showMessages` is off.
    ///
    /// Set by the menu when it toggles messages and cleared after the
    /// message is displayed. Corresponds to `message_dontfuckwithme` in
    /// `hu_stuff.c`.
    pub message_dontfuckwithme: i32,
    /// Pointer to the console player's `PlayerT` struct, set by [`HU_Start`].
    plr: *mut PlayerT,
    /// Map-title text line widget rendered on the automap overlay.
    w_title: hu_textline_t,
    /// Chat input widget; visible only when `chat_on` is non-zero.
    w_chat: hu_itext_t,
    /// Permanent zero used as the `on` pointer for `w_inputbuffer` widgets
    /// so they are never rendered to the screen.
    always_off: i32,
    /// Per-player chat destination byte: 1-4 target that player, 5 =
    /// broadcast.
    chat_dest: [u8; MAXPLAYERS],
    /// Hidden input buffer widgets that accumulate incoming chat keystrokes
    /// for each remote player before the message is confirmed with Enter.
    w_inputbuffer: [hu_itext_t; MAXPLAYERS],
    /// Non-zero while a player message is being displayed.
    /// Used as the `on` pointer for `w_message`.
    message_on: i32,
    /// Non-zero when the current message must not be overridden by a
    /// non-critical new message. Set when a `message_dontfuckwithme` message
    /// is displayed.
    message_nottobefuckedwith: i32,
    /// Scrolling player-message widget displayed at the top of the screen.
    w_message: hu_stext_t,
    /// Countdown timer (in tics) until the current message is hidden.
    /// Reset to `HU_MSGTIMEOUT` each time a new message is posted.
    message_counter: i32,
    /// True after a successful [`HU_Start`]; used by [`HU_Start`] to call
    /// [`HU_Stop`] before re-initializing all widgets.
    headsupactive: bool,
    /// Circular buffer of outgoing chat characters, shared with the network
    /// layer.
    chatchars: [u8; QUEUESIZE],
    /// Write index into `chatchars`; advanced by [`HU_queueChatChar`].
    head: usize,
    /// Read index into `chatchars`; advanced by [`HU_dequeueChatChar`].
    tail: usize,
    /// Whether an Alt key is held, so a digit sends a chat macro
    /// (static local in C `HU_Responder`).
    altdown: bool,
    /// How many times the player tried to chat with themselves (static
    /// local in C `HU_Responder`).
    num_nobrainers: i32,
}

impl HudState {
    /// The state at program start: no font, default chat macros, HUD off.
    pub const fn new() -> Self {
        Self {
            chat_macros: [
                Cow::Borrowed("No"),
                Cow::Borrowed("I'm ready to kick butt!"),
                Cow::Borrowed("I'm OK."),
                Cow::Borrowed("I'm not looking too good!"),
                Cow::Borrowed("Help!"),
                Cow::Borrowed("You suck!"),
                Cow::Borrowed("Next time, scumbag..."),
                Cow::Borrowed("Come here!"),
                Cow::Borrowed("I'll take care of it."),
                Cow::Borrowed("Yes"),
            ],
            hu_font: &[],
            chat_on: 0,
            message_dontfuckwithme: 0,
            plr: ptr::null_mut(),
            w_title: hu_textline_t::EMPTY,
            w_chat: hu_itext_t::EMPTY,
            always_off: 0,
            chat_dest: [0; MAXPLAYERS],
            w_inputbuffer: [hu_itext_t::EMPTY; MAXPLAYERS],
            message_on: 0,
            message_nottobefuckedwith: 0,
            w_message: hu_stext_t::EMPTY,
            message_counter: 0,
            headsupactive: false,
            chatchars: [0; QUEUESIZE],
            head: 0,
            tail: 0,
            altdown: false,
            num_nobrainers: 0,
        }
    }
}

impl Default for HudState {
    fn default() -> Self {
        Self::new()
    }
}

/// The heads-up display state.
pub static HUD: Global<HudState> = Global::new(HudState::new());

use crate::engine::{
    automap::automap_active,
    doom::stat::gamemode,
    game::{console_player, consoleplayer, gameepisode, gamemap, netgame, player, playeringame},
    misc::menu::MENU,
    sound::S_StartNonPositionalSound,
};

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Load the HUD font patches from the WAD and populate `hu_font`.
///
/// Must be called once at startup before any other `HU_*` function. Iterates
/// over `HU_FONTSIZE` slots, caching the `STCFNxxx` lump for each character
/// from `HU_FONTSTART` to `HU_FONTEND` as `PU_STATIC`.
/// Called from `D_DoomMain` in `d_main.c`.
///
/// The font lumps are never released, so their views are `'static`, and the
/// table holding them is leaked to give the widgets a `'static` font (the
/// size of one table per call; `D_DoomMain` calls this once).
pub fn HU_Init() {
    let font: Vec<_> = (HU_FONTSTART..=HU_FONTEND)
        .map(|character| {
            let name = format!("STCFN{:03}", character);
            // SAFETY: the lump stays at PU_STATIC; nothing releases it.
            unsafe { cache_patch_name(name.as_bytes(), PU_STATIC) }
        })
        .collect();
    HUD.borrow_mut().hu_font = font.leak();
}

/// Mark the heads-up display as inactive.
///
/// Sets `headsupactive` to false. Called by [`HU_Start`] before
/// re-initializing widgets, and may also be called directly when the HUD
/// should be torn down (e.g., during intermission).
pub fn HU_Stop() {
    HUD.borrow_mut().headsupactive = false;
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
pub fn HU_Start() {
    unsafe {
        if HUD.borrow().headsupactive {
            HU_Stop();
        }

        let (font, font_h) = {
            let mut hud = HUD.borrow_mut();
            hud.plr = console_player();
            hud.message_on = 0;
            hud.message_dontfuckwithme = 0;
            hud.message_nottobefuckedwith = 0;
            hud.chat_on = 0;

            // compute font height for title/input placement
            let font_h = hud.hu_font.first().map_or(0, |glyph| glyph.height());
            (hud.hu_font, font_h)
        };

        // The widgets keep pointers to their `on` flags, so they all point
        // into the stable address of `HUD`.
        let hud_state = HUD.as_ptr();

        // create the message widget
        HUlib_initSText(
            &raw mut (*hud_state).w_message,
            HU_MSGX,
            HU_MSGY,
            HU_MSGHEIGHT,
            font,
            HU_FONTSTART as i32,
            &raw mut (*hud_state).message_on,
        );

        let title_y = 167 - font_h;

        // create the map title widget
        HUlib_initTextLine(
            &raw mut (*hud_state).w_title,
            0,
            title_y,
            font,
            HU_FONTSTART as i32,
        );

        let (gameepisode, gamemap) = (gameepisode(), gamemap());
        let mut title = match logical_gamemission() {
            doom::mode::doom => mapnames[((gameepisode - 1) * 9 + gamemap - 1) as usize],
            doom::mode::doom2 => mapnames_commercial[(gamemap - 1) as usize],
            doom::mode::pack_plut => mapnames_commercial[(gamemap - 1 + 32) as usize],
            doom::mode::pack_tnt => mapnames_commercial[(gamemap - 1 + 64) as usize],
            _ => "Unknown level",
        };

        if gameversion() == doom::mode::exe_chex {
            title = mapnames[(gamemap - 1) as usize];
        }

        for &byte in DEH_String(title).as_bytes() {
            HUlib_addCharToTextLine(&raw mut (*hud_state).w_title, byte);
        }

        // create the chat widget
        let input_y = HU_MSGY + HU_MSGHEIGHT * (font_h + 1);
        HUlib_initIText(
            &raw mut (*hud_state).w_chat,
            HU_MSGX,
            input_y,
            font,
            HU_FONTSTART as i32,
            &raw mut (*hud_state).chat_on,
        );

        // create the inputbuffer widgets
        for i in 0..MAXPLAYERS {
            HUlib_initIText(
                &raw mut (*hud_state).w_inputbuffer[i],
                0,
                0,
                &[],
                0,
                &raw mut (*hud_state).always_off,
            );
        }

        HUD.borrow_mut().headsupactive = true;
    }
}

/// Render all visible HUD elements for the current frame.
///
/// Draws the scrolling message widget, the chat input widget, and (if the
/// automap is active) the map title line. Called once per frame from the
/// renderer after the view has been rendered.
pub fn HU_Drawer() {
    let hud_state = HUD.as_ptr();
    unsafe {
        HUlib_drawSText(&raw mut (*hud_state).w_message);
        HUlib_drawIText(&raw mut (*hud_state).w_chat);
        if automap_active() {
            HUlib_drawTextLine(&raw mut (*hud_state).w_title, 0);
        }
    }
}

/// Erase all HUD elements from the screen buffer.
///
/// Delegates to the erase functions for `w_message`, `w_chat`, and `w_title`.
/// Called once per frame just before the view is rendered, so that old HUD
/// graphics are removed before the new frame is drawn.
pub fn HU_Erase() {
    let hud_state = HUD.as_ptr();
    unsafe {
        HUlib_eraseSText(&raw mut (*hud_state).w_message);
        HUlib_eraseIText(&raw mut (*hud_state).w_chat);
        HUlib_eraseTextLine(&raw mut (*hud_state).w_title);
    }
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
pub fn HU_Ticker() {
    let hud_state = HUD.as_ptr();
    unsafe {
        let (plr, message_dontfuckwithme, message_nottobefuckedwith) = {
            let mut hud = HUD.borrow_mut();
            // tick down message counter if message is up
            if hud.message_counter != 0 {
                hud.message_counter -= 1;
                if hud.message_counter == 0 {
                    hud.message_on = 0;
                    hud.message_nottobefuckedwith = 0;
                }
            }
            (
                hud.plr,
                hud.message_dontfuckwithme,
                hud.message_nottobefuckedwith,
            )
        };

        if MENU.borrow().showMessages != 0 || message_dontfuckwithme != 0 {
            // display message if necessary
            let message = (*plr).message;
            if !message.is_none() && (message_nottobefuckedwith == 0 || message_dontfuckwithme != 0)
            {
                HUlib_addMessageToSText(&raw mut (*hud_state).w_message, None, message.as_bytes());
                (*plr).message = PlayerMessage::NONE;
                let mut hud = HUD.borrow_mut();
                hud.message_on = 1;
                hud.message_counter = HU_MSGTIMEOUT;
                hud.message_nottobefuckedwith = hud.message_dontfuckwithme;
                hud.message_dontfuckwithme = 0;
            }
        }

        // check for incoming chat characters
        if netgame() != 0 {
            let consoleplayer = consoleplayer();
            for i in 0..MAXPLAYERS {
                if playeringame(i) == 0 {
                    continue;
                }
                if i != consoleplayer as usize {
                    let c = (*player(i)).cmd.chatchar;
                    if c != 0 {
                        if c <= HU_BROADCAST as u8 {
                            HUD.borrow_mut().chat_dest[i] = c;
                        } else {
                            let input = &raw mut (*hud_state).w_inputbuffer[i];
                            let rc = HUlib_keyInIText(input, c);
                            if rc != 0 && c == KEY_ENTER {
                                let (chat_dest, line_len, line) = {
                                    let hud = HUD.borrow();
                                    let input_line = &hud.w_inputbuffer[i].l;
                                    (
                                        hud.chat_dest[i],
                                        input_line.len,
                                        until_nul(&input_line.l).to_vec(),
                                    )
                                };
                                if line_len != 0
                                    && (chat_dest == (consoleplayer + 1) as u8
                                        || chat_dest == HU_BROADCAST as u8)
                                {
                                    HUlib_addMessageToSText(
                                        &raw mut (*hud_state).w_message,
                                        Some(DEH_String(player_names[i]).as_bytes()),
                                        &line,
                                    );
                                    {
                                        let mut hud = HUD.borrow_mut();
                                        hud.message_nottobefuckedwith = 1;
                                        hud.message_on = 1;
                                        hud.message_counter = HU_MSGTIMEOUT;
                                    }
                                    if gamemode() == doom::mode::commercial {
                                        S_StartNonPositionalSound(Sfx::Radio as i32);
                                    } else {
                                        S_StartNonPositionalSound(Sfx::Tink as i32);
                                    }
                                }
                                HUlib_resetIText(input);
                            }
                        }
                        (*player(i)).cmd.chatchar = 0;
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
pub fn HU_queueChatChar(c: u8) {
    let mut hud = HUD.borrow_mut();
    if ((hud.head + 1) & (QUEUESIZE - 1)) == hud.tail {
        unsafe { (*hud.plr).message = PlayerMessage::new(DEH_String(HUSTR_MSGU)) };
    } else {
        let head = hud.head;
        hud.chatchars[head] = c;
        hud.head = (head + 1) & (QUEUESIZE - 1);
    }
}

/// Dequeue the next chat character from the outgoing circular buffer.
///
/// Returns the character at `tail` and advances `tail` modulo `QUEUESIZE` (128).
/// Returns `0` (NUL) if the buffer is empty (`head == tail`).
/// Called by the network layer (`d_net.c`) to read queued chat characters.
pub fn HU_dequeueChatChar() -> u8 {
    let mut hud = HUD.borrow_mut();
    if hud.head != hud.tail {
        let tail = hud.tail;
        let c = hud.chatchars[tail];
        hud.tail = (tail + 1) & (QUEUESIZE - 1);
        c
    } else {
        0
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
///
/// # Safety
/// `ev` must be a valid, non-null pointer to an initialised `event_t`.
pub unsafe fn HU_Responder(ev: *mut event_t) -> i32 {
    let hud_state = HUD.as_ptr();
    unsafe {
        let ev = &mut *ev;

        let mut numplayers = 0;
        for i in 0..MAXPLAYERS {
            if playeringame(i) != 0 {
                numplayers += 1;
            }
        }
        let (netgame, consoleplayer) = (netgame(), consoleplayer());

        if ev.data1 == KEY_RSHIFT as i32 {
            return 0;
        } else if ev.data1 == KEY_RALT as i32 || ev.data1 == KEY_LALT as i32 {
            HUD.borrow_mut().altdown = ev.kind == 0; // ev_keydown
            return 0;
        }

        if ev.kind != 0 {
            // not ev_keydown
            return 0;
        }

        let mut eatkey = false;
        let (chat_on, altdown, plr) = {
            let hud = HUD.borrow();
            (hud.chat_on, hud.altdown, hud.plr)
        };

        let ControlsState {
            key_message_refresh,
            key_multi_msg,
            key_multi_msgplayer,
            ..
        } = CONTROLS.get();
        if chat_on == 0 {
            if ev.data1 == key_message_refresh {
                let mut hud = HUD.borrow_mut();
                hud.message_on = 1;
                hud.message_counter = HU_MSGTIMEOUT;
                eatkey = true;
            } else if netgame != 0 && ev.data2 == key_multi_msg {
                eatkey = true;
                HUD.borrow_mut().chat_on = 1;
                HUlib_resetIText(&raw mut (*hud_state).w_chat);
                HU_queueChatChar(HU_BROADCAST as u8);
            } else if netgame != 0 && numplayers > 2 {
                for i in 0..MAXPLAYERS {
                    if ev.data2 == key_multi_msgplayer[i] {
                        if playeringame(i) != 0 && i != consoleplayer as usize {
                            eatkey = true;
                            HUD.borrow_mut().chat_on = 1;
                            HUlib_resetIText(&raw mut (*hud_state).w_chat);
                            HU_queueChatChar((i + 1) as u8);
                            break;
                        } else if i == consoleplayer as usize {
                            let num_nobrainers = {
                                let mut hud = HUD.borrow_mut();
                                hud.num_nobrainers += 1;
                                hud.num_nobrainers
                            };
                            (*plr).message = PlayerMessage::new(DEH_String(match num_nobrainers {
                                1..=2 => HUSTR_TALKTOSELF1,
                                3..=5 => HUSTR_TALKTOSELF2,
                                6..=8 => HUSTR_TALKTOSELF3,
                                9..=31 => HUSTR_TALKTOSELF4,
                                _ => HUSTR_TALKTOSELF5,
                            }));
                        }
                    }
                }
            }
        } else {
            // send a macro
            if altdown {
                let c = (ev.data1 - b'0' as i32) as u8;
                if c > 9 {
                    return 0;
                }
                let macromessage = HUD.borrow().chat_macros[c as usize].clone();
                let macromessage = macromessage.as_bytes();
                HU_queueChatChar(KEY_ENTER);
                for &byte in macromessage {
                    HU_queueChatChar(byte);
                }
                HU_queueChatChar(KEY_ENTER);
                HUD.borrow_mut().chat_on = 0;
                (*plr).message = PlayerMessage::new(macromessage);
                eatkey = true;
            } else {
                let c = ev.data2 as u8;
                eatkey = HUlib_keyInIText(&raw mut (*hud_state).w_chat, c) != 0;
                if eatkey {
                    HU_queueChatChar(c);
                }
                if c == KEY_ENTER {
                    let mut hud = HUD.borrow_mut();
                    hud.chat_on = 0;
                    if hud.w_chat.l.len != 0 {
                        (*plr).message = PlayerMessage::new(until_nul(&hud.w_chat.l.l));
                    }
                } else if c == KEY_ESCAPE {
                    HUD.borrow_mut().chat_on = 0;
                }
            }
        }

        if eatkey { 1 } else { 0 }
    }
}
