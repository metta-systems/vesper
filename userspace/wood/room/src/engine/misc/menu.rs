//! Rust port of vendor/doomgeneric/m_menu.c.
//!
//! Main menu system: episode selection, skill level, options, load/save game,
//! and "Read This" help screens. Also handles keyboard, mouse, and joystick
//! navigation and all related F-key shortcuts (quicksave, quickload, gamma, etc.).
//!
//! Notable Rust-vs-C differences:
//! - The menu pages (item arrays `MainMenu`, `EpisodeMenu`, etc. and the
//!   `menu_t` descriptors) are fields of [`MenuPages`], held in the
//!   [`MENU_PAGES`] global. `currentMenu`, `prevMenu` and `menuitems` point
//!   into it, so the pages stay in place and those pointers are followed
//!   without holding a guard. The `menuitems` and `prevMenu` pointers are
//!   wired up at runtime in `M_Init` because a const initializer cannot take
//!   the address of the static it initialises.
//! - The other C file-scope variables live in [`MenuState`], held in the
//!   [`MENU`] global. Menu routines call each other and into the game, the
//!   sound code and the config, so every function copies values out and
//!   releases the guard before calling out.
//! - `mi` is a `const fn` helper replacing C compound literals, keeping the
//!   lookup tables in static storage.
//! - Menu and prompt text are `const &str`; the active modal message is an
//!   owned `String`, and save-game descriptions stay fixed `[u8; N]`
//!   buffers because they are edited in place.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::{io::Read, ptr};

use crate::types::Boolean;

use crate::{
    engine::{
        doom,
        doom::{
            event::event_t,
            player::M_Menu_SetPlayerMessage,
            stat::{gamemission, gamemode, gameversion},
        },
        global::Global,
    },
    i_error,
};

/// Return the canonical game mission, collapsing Chex Quest and HacX aliases.
///
/// Mirrors the `logical_gamemission` macro from `doomstat.h`:
/// `pack_chex` maps to `doom`, `pack_hacx` maps to `doom2`,
/// all other values are returned as-is.
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

/// Vertical pixel stride between adjacent menu items (matches C `LINEHEIGHT`).
const LINEHEIGHT: i32 = 16;
/// Horizontal offset of the skull cursor relative to the menu item's x position (matches C `SKULLXOFF`).
const SKULLXOFF: i32 = -32;

/// ASCII code of the first character in the HUD font (matches C `HU_FONTSTART`).
const HU_FONTSTART: i32 = b'!' as i32;
/// ASCII code of the last character in the HUD font (matches C `HU_FONTEND`).
const HU_FONTEND: i32 = b'_' as i32;
/// Number of glyphs in the HUD font (derived from `HU_FONTSTART`/`HU_FONTEND`).
const HU_FONTSIZE: usize = (HU_FONTEND - HU_FONTSTART + 1) as usize;

use crate::engine::interface::video::{SCREENHEIGHT, SCREENWIDTH};

/// Maximum number of characters in a save-game description string (matches C `SAVESTRINGSIZE`).
const SAVESTRINGSIZE: usize = 24;
/// Maximum number of simultaneous players; local alias for `doom::player::MAXPLAYERS`.
const MAXPLAYERS: usize = 4;

/// Event type: key-down (matches C `ev_keydown`).
const EV_KEYDOWN: i32 = 0;
/// Event type: key-up (matches C `ev_keyup`).
const EV_KEYUP: i32 = 1;
/// Event type: mouse motion or button (matches C `ev_mouse`).
const EV_MOUSE: i32 = 2;
/// Event type: joystick input (matches C `ev_joystick`).
const EV_JOYSTICK: i32 = 3;
/// Event type: window close / quit request (matches C `ev_quit`).
const EV_QUIT: i32 = 4;

/// Key code for the Escape key (ASCII 27).
const KEY_ESCAPE: i32 = 27;
/// Key code for the Enter/Return key (ASCII 13).
const KEY_ENTER: i32 = 13;
/// Key code for the Backspace key (0x7f delete).
const KEY_BACKSPACE: i32 = 0x7f;
/// Key code for the Pause key (matches C `KEY_PAUSE`).
const KEY_PAUSE: i32 = 0xff;
/// Key code for Caps Lock (matches C `KEY_CAPSLOCK`).
const KEY_CAPSLOCK: i32 = 0x80 + 0x3a;
/// Key code for Num Lock (matches C `KEY_NUMLOCK`).
const KEY_NUMLOCK: i32 = 0x80 + 0x45;
/// Key code for Scroll Lock (matches C `KEY_SCRLCK`).
const KEY_SCRLCK: i32 = 0x80 + 0x46;

/// Build a `menuitem_t` at compile time.
///
/// `status` encodes the item type: 1 = activatable, 2 = slider, -1 = gap.
/// `name` is the WAD lump name of the item's graphic, empty for none.
/// `routine` is the callback invoked on selection or slider movement.
/// `alpha` is the keyboard shortcut character used for quick navigation.
const fn mi(status: i16, name: &'static str, routine: Option<fn(i32)>, alpha: u8) -> menuitem_t {
    menuitem_t {
        status,
        name,
        routine,
        alphaKey: alpha,
    }
}

/// A single entry in a Doom menu (C typedef `menuitem_t` from `m_menu.h`).
///
/// `status` encodes item kind: `1` = normal action, `2` = slider, `-1` = spacing row.
/// `name` is the WAD lump name of the item's graphic, empty for none.
/// `routine` is called with the item index on activation.
/// `alphaKey` is the lowercase ASCII shortcut character.
pub struct menuitem_t {
    /// Item kind: 1 = action, 2 = slider, -1 = gap row.
    pub status: i16,
    /// WAD lump name for the item's graphic patch; empty for none.
    pub name: &'static str,
    /// Callback invoked when the item is activated or a slider is adjusted.
    pub routine: Option<fn(i32)>,
    /// Keyboard shortcut character for fast navigation within the menu.
    pub alphaKey: u8,
}

/// A Doom menu page descriptor (C typedef `menu_t` from `m_menu.h`).
///
/// Each menu screen is described by one of these structs.
/// `menuitems` points to the item array for this page inside [`MenuPages`];
/// a const initializer cannot take that address, so the pointer is wired up
/// at runtime in `M_Init`.
pub struct menu_t {
    /// Number of items in `menuitems`.
    pub numitems: i16,
    /// Pointer to the parent menu page, or null for the root menu.
    pub prevMenu: *mut menu_t,
    /// Pointer to the first element of this page's `menuitem_t` array.
    pub menuitems: *mut menuitem_t,
    /// Draw callback invoked each frame while this page is active.
    pub routine: Option<fn()>,
    /// Screen x coordinate of the first menu item.
    pub x: i16,
    /// Screen y coordinate of the first menu item.
    pub y: i16,
    /// Index of the item that was last highlighted; restored on re-entry.
    pub lastOn: i16,
}

// ---------------------------------------------------------------------------
// Menu text (from d_englsh.h)
// ---------------------------------------------------------------------------

/// Load refused during a network game (C `LOADNET`).
const LOADNET: &str = "you can't do load while in a net game!\n\npress a key.";
/// Quickload refused during a network game (C `QLOADNET`).
const QLOADNET: &str = "you can't quickload during a netgame!\n\npress a key.";
/// Quickload without a chosen slot (C `QSAVESPOT`).
const QSAVESPOT: &str = "you haven't picked a quicksave slot yet!\n\npress a key.";
/// Save refused outside a game (C `SAVEDEAD`).
const SAVEDEAD: &str = "you can't save if you aren't playing!\n\npress a key.";
/// New game refused during a network game (C `NEWGAME`).
const NEWGAME: &str = "you can't start a new game\nwhile in a network game.\n\npress a key.";
/// Nightmare skill confirmation (C `NIGHTMARE`).
const NIGHTMARE: &str =
    "are you sure? this skill level\nisn't even remotely fair.\n\npress y or n.";
/// Shareware episode restriction (C `SWSTRING`).
const SWSTRING: &str = "this is the shareware version of doom.\n\nyou need to order the entire trilogy.\n\npress a key.";
/// Messages toggled off (C `MSGOFF`).
const MSGOFF: &str = "Messages OFF";
/// Messages toggled on (C `MSGON`).
const MSGON: &str = "Messages ON";
/// End game refused during a network game (C `NETEND`).
const NETEND: &str = "you can't end a netgame!\n\npress a key.";
/// End game confirmation (C `ENDGAME`).
const ENDGAME: &str = "are you sure you want to end the game?\n\npress y or n.";
/// Detail level toggled to high (C `DETAILHI`).
const DETAILHI: &str = "High detail";
/// Detail level toggled to low (C `DETAILLO`).
const DETAILLO: &str = "Low detail";
/// Text placed in a save slot with no save file.
const EMPTYSTRING: &[u8] = b"empty slot";

/// Size of the C `tempstring[80]` buffer the quicksave/quickload prompts
/// are formatted into.
const PROMPT_BUFFER_SIZE: usize = 80;
/// Size of the C `endstring[160]` buffer the quit prompt is formatted into.
const ENDSTRING_SIZE: usize = 160;
/// Size of the C `string[80]` buffer each modal message line is copied into.
const MESSAGE_LINE_SIZE: usize = 80;

/// `text` cut to the `size - 1` bytes a C `M_snprintf(buf, size, ...)`
/// keeps.
fn clamp_to_buffer(mut text: String, size: usize) -> String {
    let mut end = text.len().min(size - 1);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

use crate::engine::{
    automap::automap_active,
    doom::{
        main::DOOM_MAIN,
        main_loop::gametic,
        strings::{doom1_endmsg, doom2_endmsg},
    },
    game::{
        G_DeferedInitNew, G_LoadGame, G_SaveGame, G_ScreenShot, GAME, demoplayback, gamestate,
        netgame,
    },
    hud::stuff::HUD,
    interface::{
        input::vanilla_keyboard_mapping,
        system::I_Quit,
        timer::{I_GetTime, I_WaitVBL},
        video::{I_SetPalette, VIDEO},
    },
    misc::{
        M_StringCopy,
        controls::{CONTROLS, ControlsState},
        until_nul,
    },
    play::saveg::P_SaveGameFile,
    render_2d::V_DrawPatchDirect,
    render_3d::main::R_SetViewSize,
    sound::{S_SetMusicVolume, S_SetSfxVolume, S_StartNonPositionalSound, info::Sfx},
    wad::{W_GetNumForName, cache_lump_bytes, cache_patch_name},
};

/// Menu state (C `m_menu.c` globals other than the menu pages).
///
/// `mouseSensitivity`, `showMessages`, `detailLevel` and `screenblocks` are
/// bound to config variables by address, so they live at a stable place
/// inside [`MENU`].
pub struct MenuState {
    /// Current mouse sensitivity setting (0-9); serialised by `m_config.c`.
    pub mouseSensitivity: i32,
    /// Non-zero when player messages are enabled; read by `hu_stuff.c` and
    /// config.
    pub showMessages: i32,
    /// Graphics detail level: 0 = high, 1 = low; read by `r_main.c` and
    /// config.
    pub detailLevel: i32,
    /// Number of screen blocks to render (3-12); read by `r_main.c` and
    /// config.
    pub screenblocks: i32,
    /// Current display size index (0-8), derived from `screenblocks - 3`;
    /// updated by `M_SizeDisplay`.
    screenSize: i32,
    /// Save slot used for quicksave/quickload (-1 = none, -2 = slot
    /// selection pending).
    quickSaveSlot: i32,
    /// Non-zero when a modal message/confirmation overlay is active.
    messageToPrint: i32,
    /// Text displayed in the active modal overlay.
    messageString: String,
    /// Screen x pixel of the modal message's left edge.
    messx: i32,
    /// Screen y pixel of the modal message's top edge.
    messy: i32,
    /// Value of `menuactive` saved when a modal message was opened, restored
    /// on dismissal.
    messageLastMenuActive: i32,
    /// Non-zero when the modal message requires a y/n or specific key
    /// response.
    messageNeedsInput: i32,
    /// Callback invoked with the key code when the modal message is
    /// dismissed.
    messageRoutine: Option<fn(i32)>,
    /// Non-zero while the player is typing a save-game description string.
    saveStringEnter: i32,
    /// Save slot index currently being named.
    saveSlot: i32,
    /// Cursor position (character count) within the save-game description
    /// string.
    saveCharIndex: i32,
    /// Copy of the original description before editing, restored on Escape.
    saveOldString: [u8; SAVESTRINGSIZE],
    /// Non-zero while a help/read-this screen is being displayed; suppresses
    /// status bar. `d_main.c` checks it before re-drawing the play area.
    pub inhelpscreens: i32,
    /// Non-zero while the menu overlay is open; read by `G_Responder` and
    /// `D_Display`, mostly through [`menu_active`].
    pub menuactive: i32,
    /// Save-game description strings for all 6 save slots, read from each
    /// save file header.
    savegamestrings: [[u8; SAVESTRINGSIZE]; 10],
    /// Index of the currently highlighted menu item within the active page.
    itemOn: i16,
    /// Countdown (in tics) until the skull cursor frame toggles.
    skullAnimCounter: i16,
    /// Index into `skullName`: 0 = `M_SKULL1`, 1 = `M_SKULL2`.
    whichSkull: i16,
    /// Pointer to the currently active menu page descriptor, inside
    /// [`MENU_PAGES`].
    pub currentMenu: *mut menu_t,
    /// Selected episode index (0-based), set by `M_Episode` and consumed by
    /// `M_ChooseSkill`.
    epi: i32,
    /// Tic time after which the next joystick event will be processed
    /// (static local `joywait` in C `M_Responder`).
    joywait: i32,
    /// Tic time after which the next mouse event will be processed (static
    /// local `mousewait` in C `M_Responder`).
    mousewait: i32,
    /// Accumulated mouse y movement since the last `lasty` reset (static
    /// local `mousey` in C `M_Responder`).
    mousey: i32,
    /// Mouse y baseline; updated in 30-unit steps to produce discrete menu
    /// scrolls (static local `lasty` in C `M_Responder`).
    lasty: i32,
    /// Accumulated mouse x movement since the last `lastx` reset (static
    /// local `mousex` in C `M_Responder`).
    mousex: i32,
    /// Mouse x baseline; updated in 30-unit steps to produce discrete menu
    /// navigation (static local `lastx` in C `M_Responder`).
    lastx: i32,
}

impl MenuState {
    /// The C initial values: menu closed, default settings.
    pub const fn new() -> Self {
        Self {
            mouseSensitivity: 5,
            showMessages: 1,
            detailLevel: 0,
            screenblocks: 10,
            screenSize: 0,
            quickSaveSlot: -1,
            messageToPrint: 0,
            messageString: String::new(),
            messx: 0,
            messy: 0,
            messageLastMenuActive: 0,
            messageNeedsInput: 0,
            messageRoutine: None,
            saveStringEnter: 0,
            saveSlot: 0,
            saveCharIndex: 0,
            saveOldString: [0; SAVESTRINGSIZE],
            inhelpscreens: 0,
            menuactive: 0,
            savegamestrings: [[0; SAVESTRINGSIZE]; 10],
            itemOn: 0,
            skullAnimCounter: 0,
            whichSkull: 0,
            currentMenu: ptr::null_mut(),
            epi: 0,
            joywait: 0,
            mousewait: 0,
            mousey: 0,
            lasty: 0,
            mousex: 0,
            lastx: 0,
        }
    }
}

impl Default for MenuState {
    fn default() -> Self {
        Self::new()
    }
}

/// The menu state.
pub static MENU: Global<MenuState> = Global::new(MenuState::new());

/// Whether the menu overlay is open (C `menuactive != 0`).
pub fn menu_active() -> bool {
    MENU.borrow().menuactive != 0
}

/// Gamma-correction status messages displayed when the player cycles the gamma level.
///
/// Index matches the `usegamma` value (0-4).
pub const gammamsg: [&str; 5] = [
    "Gamma correction OFF",
    "Gamma correction level 1",
    "Gamma correction level 2",
    "Gamma correction level 3",
    "Gamma correction level 4",
];

/// WAD lump names for the two skull cursor animation frames.
const skullName: [&[u8]; 2] = [b"M_SKULL1", b"M_SKULL2"];

// ---------------------------------------------------------------------------
// Main menu item indices (C anonymous enums in m_menu.c)
// ---------------------------------------------------------------------------

/// Main menu: "New Game" item index.
const newgame: usize = 0;
/// Main menu: "Options" item index.
const options: usize = 1;
/// Main menu: "Load Game" item index.
const loadgame: usize = 2;
/// Main menu: "Save Game" item index.
const savegame: usize = 3;
/// Main menu: "Read This" item index.
const readthis: usize = 4;
/// Main menu: "Quit DOOM" item index.
const quitdoom: usize = 5;
/// Total items in the main menu.
const main_end: usize = 6;

// ---------------------------------------------------------------------------
// Episode menu item indices
// ---------------------------------------------------------------------------

/// Episode menu: Knee-Deep in the Dead (E1).
const ep1: usize = 0;
/// Episode menu: The Shores of Hell (E2).
const ep2: usize = 1;
/// Episode menu: Inferno (E3).
const ep3: usize = 2;
/// Episode menu: Thy Flesh Consumed (E4, Ultimate Doom only).
const ep4: usize = 3;
/// Total items in the episode menu (trimmed at runtime for non-Ultimate builds).
const ep_end: usize = 4;

// ---------------------------------------------------------------------------
// New-game / skill menu item indices
// ---------------------------------------------------------------------------

/// Skill menu: "I'm Too Young to Die" (easiest).
const killthings: usize = 0;
/// Skill menu: "Hey, Not Too Rough".
const toorough: usize = 1;
/// Skill menu: "Hurt Me Plenty" (default).
const hurtme: usize = 2;
/// Skill menu: "Ultra-Violence".
const violence: usize = 3;
/// Skill menu: "Nightmare!" (triggers confirmation prompt).
const nightmare: usize = 4;
/// Total items in the skill menu.
const newg_end: usize = 5;

// ---------------------------------------------------------------------------
// Options menu item indices
// ---------------------------------------------------------------------------

/// Options menu: "End Game" item index.
const endgame: usize = 0;
/// Options menu: "Messages" toggle index.
const messages: usize = 1;
/// Options menu: "Graphic Detail" toggle index.
const detail: usize = 2;
/// Options menu: "Screen Size" slider index.
const scrnsize: usize = 3;
/// Options menu: "Mouse Sensitivity" slider index.
const mousesens: usize = 5;
/// Options menu: "Sound Volume" link index.
const soundvol: usize = 7;
/// Total items in the options menu.
const opt_end: usize = 8;

// ---------------------------------------------------------------------------
// Sound menu item indices
// ---------------------------------------------------------------------------

/// Sound menu: SFX volume slider index.
const sfx_vol: usize = 0;
/// Sound menu: music volume slider index.
const music_vol: usize = 2;
/// Total items in the sound menu.
const sound_end: usize = 4;

/// Total save/load slots.
const load_end: usize = 6;

/// Total items in the first "Read This" help page.
const read1_end: usize = 1;
/// Total items in the second "Read This" help page.
const read2_end: usize = 1;

/// The menu pages (C `m_menu.c` menu item arrays and `menu_t` descriptors).
///
/// `MenuState::currentMenu`, `menu_t::prevMenu` and `menu_t::menuitems`
/// point into this struct, so it stays in place inside [`MENU_PAGES`];
/// the functions below follow those pointers without holding a guard.
pub struct MenuPages {
    /// Item array for the root main menu (New Game, Options, Load, Save, Read This, Quit).
    MainMenu: [menuitem_t; 6],
    /// Item array for the episode selection menu (E1-E4).
    EpisodeMenu: [menuitem_t; 4],
    /// Item array for the skill-level selection menu.
    NewGameMenu: [menuitem_t; 5],
    /// Item array for the Options menu (end game, messages, detail, screen size, mouse sensitivity, sound).
    OptionsMenu: [menuitem_t; 8],
    /// Single-item array for the first "Read This" help page (advances to page 2).
    ReadMenu1: [menuitem_t; 1],
    /// Single-item array for the second "Read This" help page (returns to main menu).
    ReadMenu2: [menuitem_t; 1],
    /// Item array for the Sound Volume menu (SFX slider, music slider).
    SoundMenu: [menuitem_t; 4],
    /// Item array for the Load Game menu (6 save slots).
    LoadMenu: [menuitem_t; 6],
    /// Item array for the Save Game menu (6 save slots).
    SaveMenu: [menuitem_t; 6],
    /// Page descriptor for the root main menu.
    MainDef: menu_t,
    /// Page descriptor for the episode selection menu.
    EpiDef: menu_t,
    /// Page descriptor for the skill-level selection menu; defaults to "Hurt Me Plenty" (index 2).
    NewDef: menu_t,
    /// Page descriptor for the Options menu.
    OptionsDef: menu_t,
    /// Page descriptor for the first "Read This" help screen.
    /// The skull position (x, y) may be adjusted at runtime by `M_DrawReadThis1`
    /// depending on the game version.
    ReadDef1: menu_t,
    /// Page descriptor for the second "Read This" help screen (Doom 1.x only, non-commercial).
    ReadDef2: menu_t,
    /// Page descriptor for the Sound Volume menu.
    SoundDef: menu_t,
    /// Page descriptor for the Load Game menu.
    LoadDef: menu_t,
    /// Page descriptor for the Save Game menu (reuses `load_end` count and `LoadDef` geometry).
    SaveDef: menu_t,
}

/// Build a `menu_t` at compile time, unlinked (`M_Init` sets `prevMenu`
/// and `menuitems`).
const fn unlinked_page(numitems: usize, routine: fn(), x: i16, y: i16, lastOn: i16) -> menu_t {
    menu_t {
        numitems: numitems as i16,
        prevMenu: ptr::null_mut(),
        menuitems: ptr::null_mut(),
        routine: Some(routine),
        x,
        y,
        lastOn,
    }
}

impl MenuPages {
    /// The pages as `m_menu.c` declares them, not yet linked.
    pub const fn new() -> Self {
        Self {
            MainMenu: [
                mi(1, "M_NGAME", Some(M_NewGame), b'n'),
                mi(1, "M_OPTION", Some(M_Options), b'o'),
                mi(1, "M_LOADG", Some(M_LoadGame), b'l'),
                mi(1, "M_SAVEG", Some(M_SaveGame), b's'),
                mi(1, "M_RDTHIS", Some(M_ReadThis), b'r'),
                mi(1, "M_QUITG", Some(M_QuitDOOM), b'q'),
            ],
            EpisodeMenu: [
                mi(1, "M_EPI1", Some(M_Episode), b'k'),
                mi(1, "M_EPI2", Some(M_Episode), b't'),
                mi(1, "M_EPI3", Some(M_Episode), b'i'),
                mi(1, "M_EPI4", Some(M_Episode), b't'),
            ],
            NewGameMenu: [
                mi(1, "M_JKILL", Some(M_ChooseSkill), b'i'),
                mi(1, "M_ROUGH", Some(M_ChooseSkill), b'h'),
                mi(1, "M_HURT", Some(M_ChooseSkill), b'h'),
                mi(1, "M_ULTRA", Some(M_ChooseSkill), b'u'),
                mi(1, "M_NMARE", Some(M_ChooseSkill), b'n'),
            ],
            OptionsMenu: [
                mi(1, "M_ENDGAM", Some(M_EndGame), b'e'),
                mi(1, "M_MESSG", Some(M_ChangeMessages), b'm'),
                mi(1, "M_DETAIL", Some(M_ChangeDetail), b'g'),
                mi(2, "M_SCRNSZ", Some(M_SizeDisplay), b's'),
                mi(-1, "", None, 0),
                mi(2, "M_MSENS", Some(M_ChangeSensitivity), b'm'),
                mi(-1, "", None, 0),
                mi(1, "M_SVOL", Some(M_Sound), b's'),
            ],
            ReadMenu1: [mi(1, "", Some(M_ReadThis2), 0)],
            ReadMenu2: [mi(1, "", Some(M_FinishReadThis), 0)],
            SoundMenu: [
                mi(2, "M_SFXVOL", Some(M_SfxVol), b's'),
                mi(-1, "", None, 0),
                mi(2, "M_MUSVOL", Some(M_MusicVol), b'm'),
                mi(-1, "", None, 0),
            ],
            LoadMenu: [
                mi(1, "", Some(M_LoadSelect), b'1'),
                mi(1, "", Some(M_LoadSelect), b'2'),
                mi(1, "", Some(M_LoadSelect), b'3'),
                mi(1, "", Some(M_LoadSelect), b'4'),
                mi(1, "", Some(M_LoadSelect), b'5'),
                mi(1, "", Some(M_LoadSelect), b'6'),
            ],
            SaveMenu: [
                mi(1, "", Some(M_SaveSelect), b'1'),
                mi(1, "", Some(M_SaveSelect), b'2'),
                mi(1, "", Some(M_SaveSelect), b'3'),
                mi(1, "", Some(M_SaveSelect), b'4'),
                mi(1, "", Some(M_SaveSelect), b'5'),
                mi(1, "", Some(M_SaveSelect), b'6'),
            ],
            MainDef: unlinked_page(main_end, M_DrawMainMenu, 97, 64, 0),
            EpiDef: unlinked_page(ep_end, M_DrawEpisode, 48, 63, 0),
            NewDef: unlinked_page(newg_end, M_DrawNewGame, 48, 63, 2),
            OptionsDef: unlinked_page(opt_end, M_DrawOptions, 60, 37, 0),
            ReadDef1: unlinked_page(read1_end, M_DrawReadThis1, 280, 185, 0),
            ReadDef2: unlinked_page(read2_end, M_DrawReadThis2, 330, 175, 0),
            SoundDef: unlinked_page(sound_end, M_DrawSound, 80, 64, 0),
            LoadDef: unlinked_page(load_end, M_DrawLoad, 80, 54, 0),
            SaveDef: unlinked_page(load_end, M_DrawSave, 80, 54, 0),
        }
    }
}

impl Default for MenuPages {
    fn default() -> Self {
        Self::new()
    }
}

/// The menu pages.
pub static MENU_PAGES: Global<MenuPages> = Global::new(MenuPages::new());

/// Stable address of the menu page descriptor `$page` inside
/// [`MENU_PAGES`], as stored in `currentMenu` and `prevMenu`.
macro_rules! page {
    ($page:ident) => {{
        let pages = MENU_PAGES.as_ptr();
        // Only the address is taken; `MENU_PAGES` is a static, so it stays valid.
        #[allow(unused_unsafe)]
        let page = unsafe { &raw mut (*pages).$page };
        page
    }};
}

/// Switch to the menu page `$page` of [`MENU_PAGES`] via `M_SetupNextMenu`.
macro_rules! setup_next_menu {
    ($page:ident) => {
        // SAFETY: `page!` yields a page inside the `MENU_PAGES` static.
        unsafe { M_SetupNextMenu(page!($page)) }
    };
}

/// Screen position of the first save/load slot row (`LoadDef.x`, `LoadDef.y`).
fn load_slot_origin() -> (i32, i32) {
    let pages = MENU_PAGES.borrow();
    (pages.LoadDef.x as i32, pages.LoadDef.y as i32)
}

/// Populate `savegamestrings` and `LoadMenu[*].status` by reading the header of each save file.
///
/// Opens each save file by name (via `P_SaveGameFile`), reads the first
/// `SAVESTRINGSIZE` bytes as the description, and marks the slot active.
/// Slots whose file does not exist receive the placeholder text "empty slot"
/// and have their `status` set to 0 (non-selectable in the load menu).
fn M_ReadSaveStrings() {
    for i in 0..load_end {
        let mut slot = MENU.borrow().savegamestrings[i];
        let status = match std::fs::File::open(P_SaveGameFile(i as i32)) {
            Ok(mut file) => {
                // Like the C `fread`, a short read is ignored.
                let _ = file.read(&mut slot);
                1
            }
            Err(_) => {
                M_StringCopy(&mut slot, EMPTYSTRING);
                0
            }
        };
        MENU.borrow_mut().savegamestrings[i] = slot;
        MENU_PAGES.borrow_mut().LoadMenu[i].status = status;
    }
}

/// Cache the patch lump `name` and draw it at (`x`, `y`).
fn draw_patch_lump(x: i32, y: i32, name: &[u8]) {
    // SAFETY: the view is drawn at once; tag 0 is below PU_PURGELEVEL, so
    // the zone does not purge the lump meanwhile either way.
    let patch = unsafe { cache_patch_name(name, 0) };
    V_DrawPatchDirect(x, y, patch);
}

/// Draw the Load Game menu page: title patch plus one bordered slot row per save slot.
fn M_DrawLoad() {
    draw_patch_lump(72, 28, b"M_LOADG");

    let (x, y) = load_slot_origin();
    let savegamestrings = MENU.borrow().savegamestrings;
    for (i, description) in savegamestrings.iter().enumerate().take(load_end) {
        M_DrawSaveLoadBorder(x, y + LINEHEIGHT * i as i32);
        M_WriteText(x, y + LINEHEIGHT * i as i32, until_nul(description));
    }
}

/// Draw the left/center/right border patches around a save-game name text field at `(x, y)`.
fn M_DrawSaveLoadBorder(x: i32, y: i32) {
    draw_patch_lump(x - 8, y + 7, b"M_LSLEFT");

    let mut xi = x;
    for _ in 0..24 {
        draw_patch_lump(xi, y + 7, b"M_LSCNTR");
        xi += 8;
    }

    draw_patch_lump(xi, y + 7, b"M_LSRGHT");
}

/// Load the game from save slot `choice` and close all menus.
fn M_LoadSelect(choice: i32) {
    G_LoadGame(&P_SaveGameFile(choice));
    M_ClearMenus();
}

/// Navigate to the Load Game menu page (suppressed during network games).
fn M_LoadGame(_choice: i32) {
    if netgame() != 0 {
        M_StartMessage(LOADNET, None, 0);
        return;
    }
    setup_next_menu!(LoadDef);
    M_ReadSaveStrings();
}

/// Draw the Save Game menu page: title, bordered slot rows, and a blinking cursor when editing.
fn M_DrawSave() {
    draw_patch_lump(72, 28, b"M_SAVEG");
    let (x, y) = load_slot_origin();
    let (savegamestrings, save_string_enter, save_slot) = {
        let menu = MENU.borrow();
        (menu.savegamestrings, menu.saveStringEnter, menu.saveSlot)
    };
    for (i, description) in savegamestrings.iter().enumerate().take(load_end) {
        M_DrawSaveLoadBorder(x, y + LINEHEIGHT * i as i32);
        M_WriteText(x, y + LINEHEIGHT * i as i32, until_nul(description));
    }

    if save_string_enter != 0 {
        let w = M_StringWidth(until_nul(&savegamestrings[save_slot as usize]));
        M_WriteText(x + w, y + LINEHEIGHT * save_slot, b"_");
    }
}

/// Commit the save to `slot`, clear the menus, and record the slot as the quicksave target.
fn M_DoSave(slot: i32) {
    let description = MENU.borrow().savegamestrings[slot as usize];
    G_SaveGame(slot, &description);
    M_ClearMenus();
    let mut menu = MENU.borrow_mut();
    if menu.quickSaveSlot == -2 {
        menu.quickSaveSlot = slot;
    }
}

/// Enter string-editing mode for save slot `choice`, preserving the old description.
fn M_SaveSelect(choice: i32) {
    let mut menu = MENU.borrow_mut();
    let menu = &mut *menu;
    menu.saveStringEnter = 1;
    menu.saveSlot = choice;
    let slot = &mut menu.savegamestrings[choice as usize];
    M_StringCopy(&mut menu.saveOldString, slot);
    if until_nul(slot) == EMPTYSTRING {
        slot[0] = 0;
    }
    menu.saveCharIndex = until_nul(slot).len() as i32;
}

/// Navigate to the Save Game menu page (suppressed when not in-game or not at `GS_LEVEL`).
fn M_SaveGame(_choice: i32) {
    const GS_LEVEL: i32 = 0;
    if GAME.borrow().usergame == 0 {
        M_StartMessage(SAVEDEAD, None, 0);
        return;
    }
    if gamestate() != GS_LEVEL {
        return;
    }
    setup_next_menu!(SaveDef);
    M_ReadSaveStrings();
}

/// Perform a quicksave to the previously chosen slot, or open the save menu if none was chosen.
///
/// Plays `Sfx::Oof` and returns immediately if the game is not in progress.
/// If `quickSaveSlot` is -1, opens the save menu and sets it to -2 to indicate
/// "waiting for slot selection". Otherwise shows a y/n confirmation prompt.
fn M_QuickSave() {
    const GS_LEVEL: i32 = 0;
    if GAME.borrow().usergame == 0 {
        S_StartNonPositionalSound(Sfx::Oof as i32);
        return;
    }
    if gamestate() != GS_LEVEL {
        return;
    }
    if MENU.borrow().quickSaveSlot < 0 {
        M_StartControlPanel();
        M_ReadSaveStrings();
        setup_next_menu!(SaveDef);
        MENU.borrow_mut().quickSaveSlot = -2;
        return;
    }
    let prompt = format!(
        "quicksave over your game named\n\n'{}'?\n\npress y or n.",
        quick_save_name()
    );
    M_StartMessage(
        &clamp_to_buffer(prompt, PROMPT_BUFFER_SIZE),
        Some(M_QuickSaveResponse),
        1,
    );
}

/// Description of the quicksave slot, for the quicksave/quickload prompts.
///
/// # Panics
/// If `quickSaveSlot` is not a valid slot index.
fn quick_save_name() -> String {
    let menu = MENU.borrow();
    String::from_utf8_lossy(until_nul(
        &menu.savegamestrings[menu.quickSaveSlot as usize],
    ))
    .into_owned()
}

/// Confirmation callback for the quicksave y/n prompt; saves if `key` is the confirm key.
fn M_QuickSaveResponse(key: i32) {
    if key == CONTROLS.borrow().key_menu_confirm {
        let slot = MENU.borrow().quickSaveSlot;
        M_DoSave(slot);
        S_StartNonPositionalSound(Sfx::Swtchx as i32);
    }
}

/// Perform a quickload from the previously chosen slot, or show an error if none was chosen.
///
/// Suppressed during network games. Shows a y/n confirmation before loading.
fn M_QuickLoad() {
    if netgame() != 0 {
        M_StartMessage(QLOADNET, None, 0);
        return;
    }
    if MENU.borrow().quickSaveSlot < 0 {
        M_StartMessage(QSAVESPOT, None, 0);
        return;
    }
    let prompt = format!(
        "do you want to quickload the game named\n\n'{}'?\n\npress y or n.",
        quick_save_name()
    );
    M_StartMessage(
        &clamp_to_buffer(prompt, PROMPT_BUFFER_SIZE),
        Some(M_QuickLoadResponse),
        1,
    );
}

/// Confirmation callback for the quickload y/n prompt; loads if `key` is the confirm key.
fn M_QuickLoadResponse(key: i32) {
    if key == CONTROLS.borrow().key_menu_confirm {
        let slot = MENU.borrow().quickSaveSlot;
        M_LoadSelect(slot);
        S_StartNonPositionalSound(Sfx::Swtchx as i32);
    }
}

/// Draw the first "Read This" help screen and position the skull cursor.
///
/// Selects the correct WAD lump (`HELP`, `HELP1`, or `HELP2`) and skull
/// position based on `gameversion` and `gamemode`.
fn M_DrawReadThis1() {
    MENU.borrow_mut().inhelpscreens = 1;

    let lumpname: &[u8];
    let skullx: i16;
    let skully: i16;

    match gameversion() {
        doom::mode::exe_doom_1_666
        | doom::mode::exe_doom_1_7
        | doom::mode::exe_doom_1_8
        | doom::mode::exe_doom_1_9
        | doom::mode::exe_hacx => {
            if gamemode() == doom::mode::commercial {
                lumpname = b"HELP";
                skullx = 330;
                skully = 165;
            } else {
                lumpname = b"HELP2";
                skullx = 280;
                skully = 185;
            }
        }
        doom::mode::exe_ultimate | doom::mode::exe_chex => {
            lumpname = b"HELP1";
            skullx = 280;
            skully = 185;
        }
        doom::mode::exe_final | doom::mode::exe_final2 => {
            lumpname = b"HELP";
            skullx = 330;
            skully = 165;
        }
        _ => {
            i_error!("Unhandled game version");
        }
    }

    draw_patch_lump(0, 0, lumpname);
    let mut pages = MENU_PAGES.borrow_mut();
    pages.ReadDef1.x = skullx;
    pages.ReadDef1.y = skully;
}

/// Draw the second "Read This" help screen (`HELP1` lump).
fn M_DrawReadThis2() {
    MENU.borrow_mut().inhelpscreens = 1;
    draw_patch_lump(0, 0, b"HELP1");
}

/// Draw the Sound Volume menu page: title patch and two thermometer sliders.
fn M_DrawSound() {
    use crate::engine::sound::{SOUND, SoundState};
    let SoundState {
        sfxVolume,
        musicVolume,
        ..
    } = SOUND.get();
    let (x, y) = {
        let pages = MENU_PAGES.borrow();
        (pages.SoundDef.x as i32, pages.SoundDef.y as i32)
    };
    draw_patch_lump(60, 38, b"M_SVOL");

    M_DrawThermo(x, y + LINEHEIGHT * (sfx_vol as i32 + 1), 16, sfxVolume);

    M_DrawThermo(x, y + LINEHEIGHT * (music_vol as i32 + 1), 16, musicVolume);
}

/// No-op activation callback for the Sound Volume menu item (navigation handled elsewhere).
fn M_Sound(_choice: i32) {}

/// Adjust the SFX volume slider; `choice` 0 decrements, 1 increments (range 0-15).
fn M_SfxVol(choice: i32) {
    use crate::engine::sound::SOUND;
    let sfxVolume = {
        let mut sound = SOUND.borrow_mut();
        match choice {
            0 if sound.sfxVolume > 0 => {
                sound.sfxVolume -= 1;
            }
            1 if sound.sfxVolume < 15 => {
                sound.sfxVolume += 1;
            }
            _ => {}
        }
        sound.sfxVolume
    };
    S_SetSfxVolume(sfxVolume * 8);
}

/// Adjust the music volume slider; `choice` 0 decrements, 1 increments (range 0-15).
fn M_MusicVol(choice: i32) {
    use crate::engine::sound::SOUND;
    let musicVolume = {
        let mut sound = SOUND.borrow_mut();
        match choice {
            0 if sound.musicVolume > 0 => {
                sound.musicVolume -= 1;
            }
            1 if sound.musicVolume < 15 => {
                sound.musicVolume += 1;
            }
            _ => {}
        }
        sound.musicVolume
    };
    S_SetMusicVolume(musicVolume * 8);
}

/// Draw the "DOOM" title graphic at the top of the main menu.
fn M_DrawMainMenu() {
    draw_patch_lump(94, 2, b"M_DOOM");
}

/// Draw the "New Game" and "Skill Level" title patches above the skill menu.
fn M_DrawNewGame() {
    draw_patch_lump(96, 14, b"M_NEWG");
    draw_patch_lump(54, 38, b"M_SKILL");
}

/// Handle "New Game" activation: navigate to the episode or skill menu as appropriate.
///
/// Shows an error message if in a network game (except demo playback).
/// Skips the episode menu for Doom II (commercial) and Chex Quest.
fn M_NewGame(_choice: i32) {
    if netgame() != 0 && demoplayback() == 0 {
        M_StartMessage(NEWGAME, None, 0);
        return;
    }
    if gamemode() == doom::mode::commercial || gameversion() == doom::mode::exe_chex {
        setup_next_menu!(NewDef);
    } else {
        setup_next_menu!(EpiDef);
    }
}

/// Draw the "Which Episode?" title patch above the episode menu.
fn M_DrawEpisode() {
    draw_patch_lump(54, 38, b"M_EPISOD");
}

/// Confirmation callback for the Nightmare skill prompt; starts the game if `key` confirms.
fn M_VerifyNightmare(key: i32) {
    if key != CONTROLS.borrow().key_menu_confirm {
        return;
    }
    let epi = MENU.borrow().epi;
    G_DeferedInitNew(4, epi + 1, 1);
    M_ClearMenus();
}

/// Start a new game at the given skill level, or prompt for confirmation on Nightmare.
fn M_ChooseSkill(choice: i32) {
    if choice as usize == nightmare {
        M_StartMessage(NIGHTMARE, Some(M_VerifyNightmare), 1);
        return;
    }
    let epi = MENU.borrow().epi;
    G_DeferedInitNew(choice, epi + 1, 1);
    M_ClearMenus();
}

/// Store the chosen episode index and navigate to the skill menu.
///
/// Shows a shareware-restriction message if episode > 0 in shareware mode.
/// Clamps episode 4 to episode 0 in registered mode (which only has three episodes).
fn M_Episode(choice: i32) {
    if gamemode() == doom::mode::shareware && choice != 0 {
        M_StartMessage(SWSTRING, None, 0);
        setup_next_menu!(ReadDef1);
        return;
    }
    MENU.borrow_mut().epi = if gamemode() == doom::mode::registered && choice > 2 {
        0
    } else {
        choice
    };
    setup_next_menu!(NewDef);
}

/// Draw the Options menu page: title patch, detail/message toggles, and thermometer sliders.
fn M_DrawOptions() {
    draw_patch_lump(108, 15, b"M_OPTTTL");

    let detail_names: [&[u8]; 2] = [b"M_GDHIGH", b"M_GDLOW"];
    let msg_names: [&[u8]; 2] = [b"M_MSGOFF", b"M_MSGON"];

    let (x, y) = {
        let pages = MENU_PAGES.borrow();
        (pages.OptionsDef.x as i32, pages.OptionsDef.y as i32)
    };
    let (detail_level, show_messages, mouse_sensitivity, screen_size) = {
        let menu = MENU.borrow();
        (
            menu.detailLevel,
            menu.showMessages,
            menu.mouseSensitivity,
            menu.screenSize,
        )
    };

    draw_patch_lump(
        x + 175,
        y + LINEHEIGHT * detail as i32,
        detail_names[detail_level as usize],
    );

    draw_patch_lump(
        x + 120,
        y + LINEHEIGHT * messages as i32,
        msg_names[show_messages as usize],
    );

    M_DrawThermo(
        x,
        y + LINEHEIGHT * (mousesens as i32 + 1),
        10,
        mouse_sensitivity,
    );

    M_DrawThermo(x, y + LINEHEIGHT * (scrnsize as i32 + 1), 9, screen_size);
}

/// No-op activation callback for the Options menu item (navigation handled by `M_Responder`).
fn M_Options(_choice: i32) {}

/// Toggle the `showMessages` setting and display a confirmation HUD message.
fn M_ChangeMessages(_choice: i32) {
    let show_messages = {
        let mut menu = MENU.borrow_mut();
        menu.showMessages = 1 - menu.showMessages;
        menu.showMessages
    };
    if show_messages == 0 {
        M_Menu_SetPlayerMessage(MSGOFF);
    } else {
        M_Menu_SetPlayerMessage(MSGON);
    }
    HUD.borrow_mut().message_dontfuckwithme = 1;
}

/// Confirmation callback for "End Game"; clears menus if the player confirms.
fn M_EndGameResponse(key: i32) {
    if key != CONTROLS.borrow().key_menu_confirm {
        return;
    }
    let (current_menu, item_on) = {
        let menu = MENU.borrow();
        (menu.currentMenu, menu.itemOn)
    };
    unsafe {
        (*current_menu).lastOn = item_on;
    }
    M_ClearMenus();
}

/// Prompt the player to confirm ending the current game (suppressed if not in-game or in a network game).
fn M_EndGame(_choice: i32) {
    if GAME.borrow().usergame == 0 {
        S_StartNonPositionalSound(Sfx::Oof as i32);
        return;
    }
    if netgame() != 0 {
        M_StartMessage(NETEND, None, 0);
        return;
    }
    M_StartMessage(ENDGAME, Some(M_EndGameResponse), 1);
}

/// No-op placeholder for the "Read This" main-menu item (navigation is in `M_Responder`).
fn M_ReadThis(_choice: i32) {}

/// Handle "done" from the first help screen: advance to page 2 or finish, depending on game version.
fn M_ReadThis2(_choice: i32) {
    if gameversion() <= doom::mode::exe_doom_1_9 && gamemode() != doom::mode::commercial {
        setup_next_menu!(ReadDef2);
    } else {
        M_FinishReadThis(0);
    }
}

/// No-op: closing the last help screen returns to the previous menu via `M_Responder`'s back key.
fn M_FinishReadThis(_choice: i32) {}

/// Sound effects played on quit confirmation for Doom episode games (cycled by `gametic`).
static quitsounds: [i32; 8] = [
    Sfx::Pldeth as i32,
    Sfx::Dmpain as i32,
    Sfx::Popain as i32,
    Sfx::Slop as i32,
    Sfx::Telept as i32,
    Sfx::Posit1 as i32,
    Sfx::Posit3 as i32,
    Sfx::Sgtatk as i32,
];
/// Sound effects played on quit confirmation for Doom II (commercial) builds (cycled by `gametic`).
static quitsounds2: [i32; 8] = [
    Sfx::Vilact as i32,
    Sfx::Getpow as i32,
    Sfx::Boscub as i32,
    Sfx::Slop as i32,
    Sfx::Skeswg as i32,
    Sfx::Kntdth as i32,
    Sfx::Bspact as i32,
    Sfx::Sgtatk as i32,
];

/// Confirmation callback for the quit prompt; plays a quit sound and exits if the player confirms.
fn M_QuitResponse(key: i32) {
    if key != CONTROLS.borrow().key_menu_confirm {
        return;
    }
    if netgame() == 0 {
        if gamemode() == doom::mode::commercial {
            S_StartNonPositionalSound(quitsounds2[((gametic() >> 2) & 7) as usize]);
        } else {
            S_StartNonPositionalSound(quitsounds[((gametic() >> 2) & 7) as usize]);
        }
        I_WaitVBL(105);
    }
    I_Quit();
}

/// Select a game-specific quit quip from `doom1_endmsg` or `doom2_endmsg`, keyed by `gametic`.
fn M_SelectEndMessage() -> &'static str {
    if logical_gamemission() == doom::mode::doom {
        doom1_endmsg[(gametic() as usize) & 7]
    } else {
        doom2_endmsg[(gametic() as usize) & 7]
    }
}

/// Show the quit confirmation dialog with a game-specific quip and a y/n prompt.
fn M_QuitDOOM(_choice: i32) {
    let prompt = format!("{}\n\n(press y to quit to dos.)", M_SelectEndMessage());
    M_StartMessage(
        &clamp_to_buffer(prompt, ENDSTRING_SIZE),
        Some(M_QuitResponse),
        1,
    );
}

/// Adjust mouse sensitivity; `choice` 0 decrements, 1 increments (range 0-9).
fn M_ChangeSensitivity(choice: i32) {
    let mut menu = MENU.borrow_mut();
    match choice {
        0 if menu.mouseSensitivity > 0 => {
            menu.mouseSensitivity -= 1;
        }
        1 if menu.mouseSensitivity < 9 => {
            menu.mouseSensitivity += 1;
        }
        _ => {}
    }
}

/// Toggle graphics detail between high (0) and low (1) and display a HUD confirmation.
fn M_ChangeDetail(_choice: i32) {
    let (screenblocks, detail_level) = {
        let mut menu = MENU.borrow_mut();
        menu.detailLevel = 1 - menu.detailLevel;
        (menu.screenblocks, menu.detailLevel)
    };
    R_SetViewSize(screenblocks, detail_level);
    if detail_level == 0 {
        M_Menu_SetPlayerMessage(DETAILHI);
    } else {
        M_Menu_SetPlayerMessage(DETAILLO);
    }
}

/// Adjust the screen-size slider; `choice` 0 shrinks, 1 enlarges (range 0-8).
///
/// Updates both `screenSize` and `screenblocks`, then calls `R_SetViewSize`.
fn M_SizeDisplay(choice: i32) {
    let (screenblocks, detail_level) = {
        let mut menu = MENU.borrow_mut();
        match choice {
            0 if menu.screenSize > 0 => {
                menu.screenblocks -= 1;
                menu.screenSize -= 1;
            }
            1 if menu.screenSize < 8 => {
                menu.screenblocks += 1;
                menu.screenSize += 1;
            }
            _ => {}
        }
        (menu.screenblocks, menu.detailLevel)
    };
    R_SetViewSize(screenblocks, detail_level);
}

/// Draw a thermometer slider at `(x, y)` with `thermWidth` cells and a filled dot at `thermDot`.
///
/// Renders left cap, `thermWidth` middle pieces, right cap, then the movable dot.
fn M_DrawThermo(x: i32, y: i32, thermWidth: i32, thermDot: i32) {
    let mut xx = x;
    draw_patch_lump(xx, y, b"M_THERML");
    xx += 8;
    for _ in 0..thermWidth {
        draw_patch_lump(xx, y, b"M_THERMM");
        xx += 8;
    }
    draw_patch_lump(xx, y, b"M_THERMR");

    draw_patch_lump((x + 8) + thermDot * 8, y, b"M_THERMO");
}

/// No-op: draw callback for an empty (non-selected) grid cell (unused in this port).
fn M_DrawEmptyCell(_menu: *mut menu_t, _item: i32) {}
/// No-op: draw callback for the selected grid cell (unused in this port).
fn M_DrawSelCell(_menu: *mut menu_t, _item: i32) {}

/// Display a modal overlay message.
///
/// `string` is the message to display.
/// `routine` is an optional callback invoked with the key pressed to dismiss.
/// `input` is non-zero when a specific key (y/n or confirm/abort) is required.
fn M_StartMessage(string: &str, routine: Option<fn(i32)>, input: i32) {
    let mut menu = MENU.borrow_mut();
    menu.messageLastMenuActive = menu.menuactive;
    menu.messageToPrint = 1;
    menu.messageString = string.to_owned();
    menu.messageRoutine = routine;
    menu.messageNeedsInput = input;
    menu.menuactive = 1;
}

/// Dismiss the current modal overlay message and restore the previous `menuactive` state.
fn M_StopMessage() {
    let mut menu = MENU.borrow_mut();
    menu.menuactive = menu.messageLastMenuActive;
    menu.messageToPrint = 0;
}

/// Return the pixel width of `string` rendered in the HUD font.
///
/// Non-printable characters (outside `HU_FONTSTART`..`HU_FONTEND`) contribute 4 pixels each.
fn M_StringWidth(text: &[u8]) -> i32 {
    let mut w: i32 = 0;
    for &byte in text {
        let c = byte.to_ascii_uppercase() as i32 - HU_FONTSTART;
        if c < 0 || c as usize >= HU_FONTSIZE {
            w += 4;
        } else {
            w += HUD.borrow().hu_font[c as usize].width();
        }
    }
    w
}

/// Return the pixel height of `text` rendered in the HUD font, accounting for newlines.
fn M_StringHeight(text: &[u8]) -> i32 {
    let height = HUD.borrow().hu_font[0].height();
    let newlines = text.iter().filter(|&&byte| byte == b'\n').count() as i32;
    height + newlines * height
}

/// Render `text` using the HUD font at screen position `(x, y)`.
///
/// Newline characters reset the x cursor and advance y by 12 pixels.
/// Characters outside the font range are rendered as 4-pixel spaces.
/// Rendering stops at the screen right edge.
fn M_WriteText(x: i32, y: i32, text: &[u8]) {
    let mut cx = x;
    let mut cy = y;

    for &c in text {
        if c == b'\n' {
            cx = x;
            cy += 12;
            continue;
        }

        let c = c.to_ascii_uppercase() as i32 - HU_FONTSTART;
        if c < 0 || c as usize >= HU_FONTSIZE {
            cx += 4;
            continue;
        }

        let patch = HUD.borrow().hu_font[c as usize];
        let w = patch.width();
        if cx + w > SCREENWIDTH {
            break;
        }
        V_DrawPatchDirect(cx, cy, patch);
        cx += w;
    }
}

/// Return `true` if `key` is a modifier key that should not terminate shortcut search.
///
/// Pause, Caps Lock, Scroll Lock, and Num Lock are treated as null keys.
fn IsNullKey(key: i32) -> bool {
    key == KEY_PAUSE || key == KEY_CAPSLOCK || key == KEY_SCRLCK || key == KEY_NUMLOCK
}

/// Handle an input event for the menu system.
///
/// Translates joystick, mouse, and keyboard events into menu navigation actions
/// (up/down/left/right/forward/back/activate/abort) and F-key shortcuts
/// (quicksave, quickload, screen-size, gamma, screenshot, etc.).
/// Also handles save-game string editing character-by-character.
///
/// Returns `Boolean::TRUE` if the event was consumed, `Boolean::FALSE` otherwise.
/// Called by `G_Responder` in `g_game.c`.
pub fn M_Responder(ev: &event_t) -> Boolean {
    let ControlsState {
        joybmenu,
        key_menu_abort,
        key_menu_activate,
        key_menu_back,
        key_menu_confirm,
        key_menu_decscreen,
        key_menu_detail,
        key_menu_down,
        key_menu_endgame,
        key_menu_forward,
        key_menu_gamma,
        key_menu_help,
        key_menu_incscreen,
        key_menu_left,
        key_menu_load,
        key_menu_messages,
        key_menu_qload,
        key_menu_qsave,
        key_menu_quit,
        key_menu_right,
        key_menu_save,
        key_menu_screenshot,
        key_menu_up,
        key_menu_volume,
        ..
    } = CONTROLS.get();

    // testcontrols mode
    if GAME.borrow().testcontrols != 0 {
        if ev.kind == EV_QUIT
            || (ev.kind == EV_KEYDOWN
                && (ev.data1 == key_menu_activate || ev.data1 == key_menu_quit))
        {
            I_Quit();
            return Boolean::TRUE;
        }
        return Boolean::FALSE;
    }

    // window close button
    if ev.kind == EV_QUIT {
        let (menuactive, message_to_print, message_routine) = {
            let menu = MENU.borrow();
            (menu.menuactive, menu.messageToPrint, menu.messageRoutine)
        };
        if menuactive != 0
            && message_to_print != 0
            && message_routine.map(|f| f as usize).unwrap_or(0)
                == M_QuitResponse as *const () as usize
        {
            M_QuitResponse(key_menu_confirm);
        } else {
            S_StartNonPositionalSound(Sfx::Swtchn as i32);
            M_QuitDOOM(0);
        }
        return Boolean::TRUE;
    }

    let mut ch: i32 = 0;
    let mut key: i32 = -1;

    let (mut joywait, mut mousewait) = {
        let menu = MENU.borrow();
        (menu.joywait, menu.mousewait)
    };
    if ev.kind == EV_JOYSTICK && joywait < I_GetTime() {
        if ev.data3 < 0 {
            key = key_menu_up;
            joywait = I_GetTime() + 5;
        } else if ev.data3 > 0 {
            key = key_menu_down;
            joywait = I_GetTime() + 5;
        }
        if ev.data2 < 0 {
            key = key_menu_left;
            joywait = I_GetTime() + 2;
        } else if ev.data2 > 0 {
            key = key_menu_right;
            joywait = I_GetTime() + 2;
        }
        if ev.data1 & 1 != 0 {
            key = key_menu_forward;
            joywait = I_GetTime() + 5;
        }
        if ev.data1 & 2 != 0 {
            key = key_menu_back;
            joywait = I_GetTime() + 5;
        }
        if joybmenu >= 0 && (ev.data1 & (1 << joybmenu)) != 0 {
            key = key_menu_activate;
            joywait = I_GetTime() + 5;
        }
        MENU.borrow_mut().joywait = joywait;
    } else if ev.kind == EV_MOUSE && mousewait < I_GetTime() {
        let (mut mousey, mut lasty, mut mousex, mut lastx) = {
            let menu = MENU.borrow();
            (menu.mousey, menu.lasty, menu.mousex, menu.lastx)
        };
        mousey += ev.data3;
        if mousey < lasty - 30 {
            key = key_menu_down;
            mousewait = I_GetTime() + 5;
            lasty -= 30;
            mousey = lasty;
        } else if mousey > lasty + 30 {
            key = key_menu_up;
            mousewait = I_GetTime() + 5;
            lasty += 30;
            mousey = lasty;
        }

        mousex += ev.data2;
        if mousex < lastx - 30 {
            key = key_menu_left;
            mousewait = I_GetTime() + 5;
            lastx -= 30;
            mousex = lastx;
        } else if mousex > lastx + 30 {
            key = key_menu_right;
            mousewait = I_GetTime() + 5;
            lastx += 30;
            mousex = lastx;
        }

        if ev.data1 & 1 != 0 {
            key = key_menu_forward;
            mousewait = I_GetTime() + 15;
        }
        if ev.data1 & 2 != 0 {
            key = key_menu_back;
            mousewait = I_GetTime() + 15;
        }

        let mut menu = MENU.borrow_mut();
        menu.mousewait = mousewait;
        menu.mousey = mousey;
        menu.lasty = lasty;
        menu.mousex = mousex;
        menu.lastx = lastx;
    } else if ev.kind == EV_KEYDOWN {
        key = ev.data1;
        ch = ev.data2;
    }

    if key == -1 {
        return Boolean::FALSE;
    }

    // Save Game string input
    if MENU.borrow().saveStringEnter != 0 {
        if key == KEY_BACKSPACE {
            let mut menu = MENU.borrow_mut();
            if menu.saveCharIndex > 0 {
                menu.saveCharIndex -= 1;
                let (slot, index) = (menu.saveSlot as usize, menu.saveCharIndex as usize);
                menu.savegamestrings[slot][index] = 0;
            }
        } else if key == KEY_ESCAPE {
            let mut menu = MENU.borrow_mut();
            let menu = &mut *menu;
            menu.saveStringEnter = 0;
            M_StringCopy(
                &mut menu.savegamestrings[menu.saveSlot as usize],
                &menu.saveOldString,
            );
        } else if key == KEY_ENTER {
            let (save_slot, description_start) = {
                let mut menu = MENU.borrow_mut();
                menu.saveStringEnter = 0;
                (
                    menu.saveSlot,
                    menu.savegamestrings[menu.saveSlot as usize][0],
                )
            };
            if description_start != 0 {
                M_DoSave(save_slot);
            }
        } else {
            if vanilla_keyboard_mapping != 0 {
                ch = key;
            }
            if let Ok(byte) = u8::try_from(ch) {
                ch = byte.to_ascii_uppercase() as i32;
            }

            if ch != b' ' as i32
                && (ch - HU_FONTSTART < 0 || ch - HU_FONTSTART >= HU_FONTSIZE as i32)
            {
                return Boolean::TRUE;
            }

            let (save_slot, save_char_index, description) = {
                let menu = MENU.borrow();
                (
                    menu.saveSlot as usize,
                    menu.saveCharIndex,
                    menu.savegamestrings[menu.saveSlot as usize],
                )
            };
            if (32..=127).contains(&ch)
                && save_char_index < (SAVESTRINGSIZE as i32 - 1)
                && M_StringWidth(until_nul(&description)) < (SAVESTRINGSIZE as i32 - 2) * 8
            {
                let mut menu = MENU.borrow_mut();
                let index = save_char_index as usize;
                menu.savegamestrings[save_slot][index] = ch as u8;
                menu.saveCharIndex += 1;
                menu.savegamestrings[save_slot][index + 1] = 0;
            }
        }
        return Boolean::TRUE;
    }

    // Messages that need input
    let (message_to_print, message_needs_input) = {
        let menu = MENU.borrow();
        (menu.messageToPrint, menu.messageNeedsInput)
    };
    if message_to_print != 0 {
        if message_needs_input != 0
            && key != b' ' as i32
            && key != KEY_ESCAPE
            && key != key_menu_confirm
            && key != key_menu_abort
        {
            return Boolean::FALSE;
        }

        let message_routine = {
            let mut menu = MENU.borrow_mut();
            menu.menuactive = menu.messageLastMenuActive;
            menu.messageToPrint = 0;
            menu.messageRoutine
        };
        if let Some(r) = message_routine {
            r(key);
        }
        MENU.borrow_mut().menuactive = 0;
        S_StartNonPositionalSound(Sfx::Swtchx as i32);
        return Boolean::TRUE;
    }

    // Screenshot
    let devparm = DOOM_MAIN.borrow().devparm;
    if (devparm != 0 && key == key_menu_help) || (key != 0 && key == key_menu_screenshot) {
        G_ScreenShot();
        return Boolean::TRUE;
    }

    // F-Keys (when menu not active)
    if !menu_active() {
        if key == key_menu_decscreen {
            if automap_active() || HUD.borrow().chat_on != 0 {
                return Boolean::FALSE;
            }
            M_SizeDisplay(0);
            S_StartNonPositionalSound(Sfx::Stnmov as i32);
            return Boolean::TRUE;
        } else if key == key_menu_incscreen {
            if automap_active() || HUD.borrow().chat_on != 0 {
                return Boolean::FALSE;
            }
            M_SizeDisplay(1);
            S_StartNonPositionalSound(Sfx::Stnmov as i32);
            return Boolean::TRUE;
        } else if key == key_menu_help {
            M_StartControlPanel();
            {
                let mut menu = MENU.borrow_mut();
                menu.currentMenu = if gamemode() == doom::mode::retail {
                    page!(ReadDef2)
                } else {
                    page!(ReadDef1)
                };
                menu.itemOn = 0;
            }
            S_StartNonPositionalSound(Sfx::Swtchn as i32);
            return Boolean::TRUE;
        } else if key == key_menu_save {
            M_StartControlPanel();
            S_StartNonPositionalSound(Sfx::Swtchn as i32);
            M_SaveGame(0);
            return Boolean::TRUE;
        } else if key == key_menu_load {
            M_StartControlPanel();
            S_StartNonPositionalSound(Sfx::Swtchn as i32);
            M_LoadGame(0);
            return Boolean::TRUE;
        } else if key == key_menu_volume {
            M_StartControlPanel();
            {
                let mut menu = MENU.borrow_mut();
                menu.currentMenu = page!(SoundDef);
                menu.itemOn = sfx_vol as i16;
            }
            S_StartNonPositionalSound(Sfx::Swtchn as i32);
            return Boolean::TRUE;
        } else if key == key_menu_detail {
            M_ChangeDetail(0);
            S_StartNonPositionalSound(Sfx::Swtchn as i32);
            return Boolean::TRUE;
        } else if key == key_menu_qsave {
            S_StartNonPositionalSound(Sfx::Swtchn as i32);
            M_QuickSave();
            return Boolean::TRUE;
        } else if key == key_menu_endgame {
            S_StartNonPositionalSound(Sfx::Swtchn as i32);
            M_EndGame(0);
            return Boolean::TRUE;
        } else if key == key_menu_messages {
            M_ChangeMessages(0);
            S_StartNonPositionalSound(Sfx::Swtchn as i32);
            return Boolean::TRUE;
        } else if key == key_menu_qload {
            S_StartNonPositionalSound(Sfx::Swtchn as i32);
            M_QuickLoad();
            return Boolean::TRUE;
        } else if key == key_menu_quit {
            S_StartNonPositionalSound(Sfx::Swtchn as i32);
            M_QuitDOOM(0);
            return Boolean::TRUE;
        } else if key == key_menu_gamma {
            let usegamma = {
                let mut video = VIDEO.borrow_mut();
                video.usegamma += 1;
                if video.usegamma > 4 {
                    video.usegamma = 0;
                }
                video.usegamma
            };
            M_Menu_SetPlayerMessage(gammamsg[usegamma as usize]);
            // SAFETY: the view is used before any further zone allocation
            // could purge it.
            I_SetPalette(unsafe { cache_lump_bytes(W_GetNumForName(b"PLAYPAL"), 0) });
            return Boolean::TRUE;
        }
    }

    // Pop-up menu?
    if !menu_active() {
        if key == key_menu_activate {
            M_StartControlPanel();
            S_StartNonPositionalSound(Sfx::Swtchn as i32);
            return Boolean::TRUE;
        }
        return Boolean::FALSE;
    }

    // Keys usable within menu. The page is reached through
    // `currentMenu`; no guard is held while an item routine runs.
    let (current_menu, mut item_on) = {
        let menu = MENU.borrow();
        (menu.currentMenu, menu.itemOn)
    };
    // SAFETY (every page access below): the menu is active, so
    // `currentMenu` points at a page of the `MENU_PAGES` static (it and the
    // `prevMenu` links are set from `page!`), whose `menuitems` hold
    // `numitems` items, and `item_on` and the shortcut loops stay below
    // `numitems`. Each access is a single read or write; no reference into
    // the page is held while an item routine runs.
    let numitems = unsafe { (*current_menu).numitems };
    let item = |index: i16| unsafe { &*(*current_menu).menuitems.offset(index as isize) };
    let item_status = |index: i16| item(index).status;
    let item_routine = |index: i16| item(index).routine;
    let set_last_on = |item_on: i16| unsafe { (*current_menu).lastOn = item_on };
    if key == key_menu_down {
        loop {
            if item_on + 1 > numitems - 1 {
                item_on = 0;
            } else {
                item_on += 1;
            }
            MENU.borrow_mut().itemOn = item_on;
            S_StartNonPositionalSound(Sfx::Pstop as i32);
            if item_status(item_on) != -1 {
                break;
            }
        }
        return Boolean::TRUE;
    } else if key == key_menu_up {
        loop {
            if item_on == 0 {
                item_on = numitems - 1;
            } else {
                item_on -= 1;
            }
            MENU.borrow_mut().itemOn = item_on;
            S_StartNonPositionalSound(Sfx::Pstop as i32);
            if item_status(item_on) != -1 {
                break;
            }
        }
        return Boolean::TRUE;
    } else if key == key_menu_left {
        if item_status(item_on) == 2
            && let Some(routine) = item_routine(item_on)
        {
            S_StartNonPositionalSound(Sfx::Stnmov as i32);
            routine(0);
        }
        return Boolean::TRUE;
    } else if key == key_menu_right {
        if item_status(item_on) == 2
            && let Some(routine) = item_routine(item_on)
        {
            S_StartNonPositionalSound(Sfx::Stnmov as i32);
            routine(1);
        }
        return Boolean::TRUE;
    } else if key == key_menu_forward {
        let status = item_status(item_on);
        if let Some(routine) = item_routine(item_on)
            && status != 0
        {
            set_last_on(item_on);
            if status == 2 {
                routine(1);
                S_StartNonPositionalSound(Sfx::Stnmov as i32);
            } else {
                routine(item_on as i32);
                S_StartNonPositionalSound(Sfx::Pistol as i32);
            }
        }
        return Boolean::TRUE;
    } else if key == key_menu_activate {
        set_last_on(item_on);
        M_ClearMenus();
        S_StartNonPositionalSound(Sfx::Swtchx as i32);
        return Boolean::TRUE;
    } else if key == key_menu_back {
        set_last_on(item_on);
        let previous_menu = unsafe { (*current_menu).prevMenu };
        if !previous_menu.is_null() {
            {
                let mut menu = MENU.borrow_mut();
                menu.currentMenu = previous_menu;
                menu.itemOn = unsafe { (*previous_menu).lastOn };
            }
            S_StartNonPositionalSound(Sfx::Swtchn as i32);
        }
        return Boolean::TRUE;
    }

    // Keyboard shortcut
    if ch != 0 || IsNullKey(key) {
        let ch_u = ch as u8;
        let item_alpha_key = |index: i16| item(index).alphaKey;
        for i in (item_on + 1)..numitems {
            if item_alpha_key(i) == ch_u {
                MENU.borrow_mut().itemOn = i;
                S_StartNonPositionalSound(Sfx::Pstop as i32);
                return Boolean::TRUE;
            }
        }
        for i in 0..=item_on {
            if item_alpha_key(i) == ch_u {
                MENU.borrow_mut().itemOn = i;
                S_StartNonPositionalSound(Sfx::Pstop as i32);
                return Boolean::TRUE;
            }
        }
    }

    Boolean::FALSE
}

/// Open the main menu and set `currentMenu` to `MainDef`.
///
/// No-op if the menu is already active. Called from `M_Responder` and from
/// game code that needs to force the menu open (e.g. after a level warp cheat).
pub fn M_StartControlPanel() {
    let mut menu = MENU.borrow_mut();
    if menu.menuactive != 0 {
        return;
    }
    menu.menuactive = 1;
    menu.currentMenu = page!(MainDef);
    menu.itemOn = unsafe { (*menu.currentMenu).lastOn };
}

/// Draw the menu overlay for the current frame.
///
/// If a modal message (`messageToPrint`) is pending it is drawn centered on
/// screen and the function returns early. Otherwise the current menu page's
/// draw callback is invoked, all item patches are blitted, and the animated
/// skull cursor is drawn at the highlighted item. Called by `D_Display` in
/// `d_main.c` every frame.
pub fn M_Drawer() {
    unsafe {
        let message_to_print = {
            let mut menu = MENU.borrow_mut();
            menu.inhelpscreens = 0;
            menu.messageToPrint
        };

        if message_to_print != 0 {
            let (message, mut messx) = {
                let menu = MENU.borrow();
                (menu.messageString.clone(), menu.messx)
            };
            let message = message.as_bytes();
            let mut messy = SCREENHEIGHT / 2 - M_StringHeight(message) / 2;
            // Each line is cut to the C `string[80]` buffer; text without a
            // newline that does not fit continues on the next line.
            let line_limit = MESSAGE_LINE_SIZE - 1;
            let mut rest = message;
            while !rest.is_empty() {
                let line = match rest.iter().position(|&byte| byte == b'\n') {
                    Some(newline) => {
                        let line = &rest[..newline.min(line_limit)];
                        rest = &rest[newline + 1..];
                        line
                    }
                    None => {
                        let line = &rest[..rest.len().min(line_limit)];
                        rest = &rest[line.len()..];
                        line
                    }
                };

                messx = SCREENWIDTH / 2 - M_StringWidth(line) / 2;
                M_WriteText(messx, messy, line);
                messy += HUD.borrow().hu_font[0].height();
            }
            let mut menu = MENU.borrow_mut();
            menu.messx = messx;
            menu.messy = messy;
            return;
        }

        if !menu_active() {
            return;
        }

        if let Some(r) = (*MENU.borrow().currentMenu).routine {
            r();
        }

        let (current_menu, item_on, which_skull) = {
            let menu = MENU.borrow();
            (menu.currentMenu, menu.itemOn, menu.whichSkull)
        };
        let x = (*current_menu).x as i32;
        let y = (*current_menu).y as i32;
        let max = (*current_menu).numitems as usize;

        // Each view is drawn at once; tag 0 is below PU_PURGELEVEL, so the
        // zone does not purge the lumps meanwhile either way.
        for i in 0..max {
            let name = (*(*current_menu).menuitems.add(i)).name;
            if !name.is_empty() {
                V_DrawPatchDirect(
                    x,
                    y + LINEHEIGHT * i as i32,
                    cache_patch_name(name.as_bytes(), 0),
                );
            }
        }

        V_DrawPatchDirect(
            x + SKULLXOFF,
            (*current_menu).y as i32 - 5 + item_on as i32 * LINEHEIGHT,
            cache_patch_name(skullName[which_skull as usize], 0),
        );
    }
}

/// Close all menus by setting `menuactive` to 0.
fn M_ClearMenus() {
    MENU.borrow_mut().menuactive = 0;
}

/// Switch to `menudef` as the active menu page and restore its last-highlighted item.
///
/// # Safety
/// `menudef` must point to a page in [`MENU_PAGES`] (as `page!` yields):
/// it is read here and kept as `currentMenu` for later menu code.
unsafe fn M_SetupNextMenu(menudef: *mut menu_t) {
    let mut menu = MENU.borrow_mut();
    menu.currentMenu = menudef;
    menu.itemOn = unsafe { (*menudef).lastOn };
}

/// Advance the skull cursor animation by one game tic.
///
/// Decrements `skullAnimCounter` and toggles `whichSkull` every 8 tics.
/// Called by `G_Ticker` in `g_game.c`.
pub fn M_Ticker() {
    let mut menu = MENU.borrow_mut();
    menu.skullAnimCounter -= 1;
    if menu.skullAnimCounter <= 0 {
        menu.whichSkull ^= 1;
        menu.skullAnimCounter = 8;
    }
}

/// One-time initialisation of the menu subsystem.
///
/// Resets all state variables, wires `menuitems` pointers and `prevMenu`
/// cross-links that a const initializer cannot set (they are addresses
/// inside `MENU_PAGES` itself), trims the episode menu to three items for
/// pre-Ultimate builds, and removes "Read This" from the main menu in
/// commercial mode. Called once from `D_DoomMain` in `d_main.c`.
pub fn M_Init() {
    {
        let mut menu = MENU.borrow_mut();
        menu.currentMenu = page!(MainDef);
        menu.menuactive = 0;
        menu.itemOn = unsafe { (*menu.currentMenu).lastOn };
        menu.whichSkull = 0;
        menu.skullAnimCounter = 10;
        menu.screenSize = menu.screenblocks - 3;
        menu.messageToPrint = 0;
        menu.messageString = String::new();
        menu.messageLastMenuActive = menu.menuactive;
        menu.quickSaveSlot = -1;
    }

    // The pages point at each other, so the links are taken from their
    // stable addresses inside `MENU_PAGES`, and written through them.
    let pages = MENU_PAGES.as_ptr();
    unsafe {
        // Set menuitems pointers (couldn't be done at static init time)
        (*pages).MainDef.menuitems = (&raw mut (*pages).MainMenu).cast();
        (*pages).EpiDef.menuitems = (&raw mut (*pages).EpisodeMenu).cast();
        (*pages).NewDef.menuitems = (&raw mut (*pages).NewGameMenu).cast();
        (*pages).OptionsDef.menuitems = (&raw mut (*pages).OptionsMenu).cast();
        (*pages).ReadDef1.menuitems = (&raw mut (*pages).ReadMenu1).cast();
        (*pages).ReadDef2.menuitems = (&raw mut (*pages).ReadMenu2).cast();
        (*pages).SoundDef.menuitems = (&raw mut (*pages).SoundMenu).cast();
        (*pages).LoadDef.menuitems = (&raw mut (*pages).LoadMenu).cast();
        (*pages).SaveDef.menuitems = (&raw mut (*pages).SaveMenu).cast();

        // Set up prevMenu cross-links
        (*pages).EpiDef.prevMenu = &raw mut (*pages).MainDef;
        (*pages).NewDef.prevMenu = &raw mut (*pages).EpiDef;
        (*pages).OptionsDef.prevMenu = &raw mut (*pages).MainDef;
        (*pages).ReadDef1.prevMenu = &raw mut (*pages).MainDef;
        (*pages).ReadDef2.prevMenu = &raw mut (*pages).ReadDef1;
        (*pages).SoundDef.prevMenu = &raw mut (*pages).OptionsDef;
        (*pages).LoadDef.prevMenu = &raw mut (*pages).MainDef;
        (*pages).SaveDef.prevMenu = &raw mut (*pages).MainDef;

        match gamemode() {
            doom::mode::commercial => {
                let quit_item = ptr::read(&raw const (*pages).MainMenu[quitdoom]);
                ptr::write(&raw mut (*pages).MainMenu[readthis], quit_item);
                (*pages).MainDef.numitems -= 1;
                (*pages).MainDef.y += 8;
                (*pages).NewDef.prevMenu = &raw mut (*pages).MainDef;
            }
            doom::mode::shareware | doom::mode::registered | doom::mode::retail => {}
            _ => {}
        }

        if gameversion() < doom::mode::exe_ultimate {
            (*pages).EpiDef.numitems -= 1;
        }
    }
}
