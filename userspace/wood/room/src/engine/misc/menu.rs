//! Rust port of vendor/doomgeneric/m_menu.c.
//!
//! Main menu system: episode selection, skill level, options, load/save game,
//! and "Read This" help screens. Also handles keyboard, mouse, and joystick
//! navigation and all related F-key shortcuts (quicksave, quickload, gamma, etc.).
//!
//! Notable Rust-vs-C differences:
//! - Menu item arrays (`MainMenu`, `EpisodeMenu`, etc.) are `static mut` values
//!   rather than global arrays; the `menuitems` pointers inside each `menu_t` are
//!   wired up at runtime in `M_Init` because raw-pointer field initializers cannot
//!   reference other statics in Rust's const evaluator.
//! - `mi` is a `const fn` helper replacing C compound literals, keeping the
//!   lookup tables in static storage.
//! - Menu and prompt text are `const &str`; the active modal message is an
//!   owned `String`, and save-game descriptions stay fixed `[u8; N]`
//!   buffers because they are edited in place.
//! - `logical_gamemission` has a safe signature (`fn`, not `unsafe fn`); the
//!   C-linked global read is wrapped in an internal `unsafe { ... }` block.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::{io::Read, ptr};

use crate::types::Boolean;

use crate::{
    engine::{
        doom,
        doom::{event::event_t, player::M_Menu_SetPlayerMessage},
        doomstat::{gamemission, gamemode, gameversion},
    },
    i_error,
};

/// Return the canonical game mission, collapsing Chex Quest and HacX aliases.
///
/// Mirrors the `logical_gamemission` macro from `doomstat.h`:
/// `pack_chex` maps to `doom`, `pack_hacx` maps to `doom2`,
/// all other values are returned as-is.
fn logical_gamemission() -> i32 {
    unsafe {
        if gamemission == doom::mode::pack_chex {
            doom::mode::doom
        } else if gamemission == doom::mode::pack_hacx {
            doom::mode::doom2
        } else {
            gamemission
        }
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
/// `menuitems` points to the item array for this page; it cannot be initialised
/// as a static const because Rust forbids raw-pointer cross-references between
/// statics, so the pointer is wired up at runtime in `M_Init`.
#[repr(C)]
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

const _: () = assert!(std::mem::size_of::<menu_t>() == 40, "menu_t size mismatch");

/// Minimal prefix of `patch_t` needed to read width/height without pulling in the full type.
///
/// Used when measuring HUD font glyphs for text centering.
#[repr(C)]
struct patch_stub {
    /// Pixel width of the patch.
    width: i16,
    /// Pixel height of the patch.
    height: i16,
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
const SWSTRING: &str =
    "this is the shareware version of doom.\n\nyou need to order the entire trilogy.\n\npress a key.";
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
    automap::automapactive,
    doom::{main::devparm, main_loop::gametic},
    dstrings::{doom1_endmsg, doom2_endmsg},
    game::{
        demoplayback, gamestate, netgame, testcontrols, usergame, G_DeferedInitNew, G_LoadGame,
        G_SaveGame, G_ScreenShot,
    },
    hud::stuff::{chat_on, hu_font, message_dontfuckwithme},
    interface::{
        input::vanilla_keyboard_mapping,
        system::I_Quit,
        timer::{I_GetTime, I_WaitVBL},
        video::{usegamma, I_SetPalette},
    },
    misc::{until_nul, M_StringCopy},
    play::saveg::P_SaveGameFile,
    render_2d::{patch_t, V_DrawPatchDirect},
    render_3d::main::R_SetViewSize,
    sound::{S_SetMusicVolume, S_SetSfxVolume, S_StartSound},
    sounds::Sfx,
    wad::W_CacheLumpName,
};

/// Current mouse sensitivity setting (0-9); exported to C for `m_config.c` serialization.
pub static mut mouseSensitivity: i32 = 5;
/// Non-zero when player messages are enabled; exported for `hu_stuff.c` and config.
pub static mut showMessages: i32 = 1;
/// Graphics detail level: 0 = high, 1 = low; exported for `r_main.c` and config.
pub static mut detailLevel: i32 = 0;
/// Number of screen blocks to render (3-12); exported for `r_main.c` and config.
pub static mut screenblocks: i32 = 10;

/// Current display size index (0-8), derived from `screenblocks - 3`; updated by `M_SizeDisplay`.
static mut screenSize: i32 = 0;
/// Save slot used for quicksave/quickload (-1 = none, -2 = slot selection pending).
static mut quickSaveSlot: i32 = -1;
/// Non-zero when a modal message/confirmation overlay is active.
static mut messageToPrint: i32 = 0;
/// Text displayed in the active modal overlay.
static mut messageString: String = String::new();
/// Screen x pixel of the modal message's left edge.
static mut messx: i32 = 0;
/// Screen y pixel of the modal message's top edge.
static mut messy: i32 = 0;
/// Value of `menuactive` saved when a modal message was opened, restored on dismissal.
static mut messageLastMenuActive: i32 = 0;
/// Non-zero when the modal message requires a y/n or specific key response.
static mut messageNeedsInput: i32 = 0;
/// Callback invoked with the key code when the modal message is dismissed.
static mut messageRoutine: Option<fn(i32)> = None;

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

/// Non-zero while the player is typing a save-game description string.
static mut saveStringEnter: i32 = 0;
/// Save slot index currently being named.
static mut saveSlot: i32 = 0;
/// Cursor position (character count) within the save-game description string.
static mut saveCharIndex: i32 = 0;
/// Copy of the original description before editing, restored on Escape.
static mut saveOldString: [u8; SAVESTRINGSIZE] = [0; SAVESTRINGSIZE];

/// Non-zero while a help/read-this screen is being displayed; suppresses status bar.
/// Exported for `d_main.c` which checks it before re-drawing the play area.
pub static mut inhelpscreens: i32 = 0;
/// Non-zero while the menu overlay is open; read by `G_Responder` and `D_Display`.
pub static mut menuactive: i32 = 0;

/// Save-game description strings for all 6 save slots, read from each save file header.
static mut savegamestrings: [[u8; SAVESTRINGSIZE]; 10] = [[0; SAVESTRINGSIZE]; 10];

/// Index of the currently highlighted menu item within the active page.
static mut itemOn: i16 = 0;
/// Countdown (in tics) until the skull cursor frame toggles.
static mut skullAnimCounter: i16 = 0;
/// Index into `skullName`: 0 = `M_SKULL1`, 1 = `M_SKULL2`.
static mut whichSkull: i16 = 0;

/// WAD lump names for the two skull cursor animation frames.
const skullName: [&[u8]; 2] = [b"M_SKULL1", b"M_SKULL2"];

/// Pointer to the currently active menu page descriptor.
/// Exported for C callers (e.g. `d_main.c`).
pub static mut currentMenu: *mut menu_t = ptr::null_mut();

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

/// Item array for the root main menu (New Game, Options, Load, Save, Read This, Quit).
static mut MainMenu: [menuitem_t; 6] = [
    mi(1, "M_NGAME", Some(M_NewGame), b'n'),
    mi(1, "M_OPTION", Some(M_Options), b'o'),
    mi(1, "M_LOADG", Some(M_LoadGame), b'l'),
    mi(1, "M_SAVEG", Some(M_SaveGame), b's'),
    mi(1, "M_RDTHIS", Some(M_ReadThis), b'r'),
    mi(1, "M_QUITG", Some(M_QuitDOOM), b'q'),
];

/// Item array for the episode selection menu (E1-E4).
static mut EpisodeMenu: [menuitem_t; 4] = [
    mi(1, "M_EPI1", Some(M_Episode), b'k'),
    mi(1, "M_EPI2", Some(M_Episode), b't'),
    mi(1, "M_EPI3", Some(M_Episode), b'i'),
    mi(1, "M_EPI4", Some(M_Episode), b't'),
];

/// Item array for the skill-level selection menu.
static mut NewGameMenu: [menuitem_t; 5] = [
    mi(1, "M_JKILL", Some(M_ChooseSkill), b'i'),
    mi(1, "M_ROUGH", Some(M_ChooseSkill), b'h'),
    mi(1, "M_HURT", Some(M_ChooseSkill), b'h'),
    mi(1, "M_ULTRA", Some(M_ChooseSkill), b'u'),
    mi(1, "M_NMARE", Some(M_ChooseSkill), b'n'),
];

/// Item array for the Options menu (end game, messages, detail, screen size, mouse sensitivity, sound).
static mut OptionsMenu: [menuitem_t; 8] = [
    mi(1, "M_ENDGAM", Some(M_EndGame), b'e'),
    mi(1, "M_MESSG", Some(M_ChangeMessages), b'm'),
    mi(1, "M_DETAIL", Some(M_ChangeDetail), b'g'),
    mi(2, "M_SCRNSZ", Some(M_SizeDisplay), b's'),
    mi(-1, "", None, 0),
    mi(2, "M_MSENS", Some(M_ChangeSensitivity), b'm'),
    mi(-1, "", None, 0),
    mi(1, "M_SVOL", Some(M_Sound), b's'),
];

/// Single-item array for the first "Read This" help page (advances to page 2).
static mut ReadMenu1: [menuitem_t; 1] = [mi(1, "", Some(M_ReadThis2), 0)];

/// Single-item array for the second "Read This" help page (returns to main menu).
static mut ReadMenu2: [menuitem_t; 1] = [mi(1, "", Some(M_FinishReadThis), 0)];

/// Item array for the Sound Volume menu (SFX slider, music slider).
static mut SoundMenu: [menuitem_t; 4] = [
    mi(2, "M_SFXVOL", Some(M_SfxVol), b's'),
    mi(-1, "", None, 0),
    mi(2, "M_MUSVOL", Some(M_MusicVol), b'm'),
    mi(-1, "", None, 0),
];

/// Item array for the Load Game menu (6 save slots).
static mut LoadMenu: [menuitem_t; 6] = [
    mi(1, "", Some(M_LoadSelect), b'1'),
    mi(1, "", Some(M_LoadSelect), b'2'),
    mi(1, "", Some(M_LoadSelect), b'3'),
    mi(1, "", Some(M_LoadSelect), b'4'),
    mi(1, "", Some(M_LoadSelect), b'5'),
    mi(1, "", Some(M_LoadSelect), b'6'),
];

/// Item array for the Save Game menu (6 save slots).
static mut SaveMenu: [menuitem_t; 6] = [
    mi(1, "", Some(M_SaveSelect), b'1'),
    mi(1, "", Some(M_SaveSelect), b'2'),
    mi(1, "", Some(M_SaveSelect), b'3'),
    mi(1, "", Some(M_SaveSelect), b'4'),
    mi(1, "", Some(M_SaveSelect), b'5'),
    mi(1, "", Some(M_SaveSelect), b'6'),
];

/// Page descriptor for the root main menu.
static mut MainDef: menu_t = menu_t {
    numitems: main_end as i16,
    prevMenu: ptr::null_mut(),
    menuitems: ptr::null_mut(),
    routine: Some(M_DrawMainMenu),
    x: 97,
    y: 64,
    lastOn: 0,
};

/// Page descriptor for the episode selection menu.
static mut EpiDef: menu_t = menu_t {
    numitems: ep_end as i16,
    prevMenu: ptr::null_mut(),
    menuitems: ptr::null_mut(),
    routine: Some(M_DrawEpisode),
    x: 48,
    y: 63,
    lastOn: 0,
};

/// Page descriptor for the skill-level selection menu; defaults to "Hurt Me Plenty" (index 2).
static mut NewDef: menu_t = menu_t {
    numitems: newg_end as i16,
    prevMenu: ptr::null_mut(),
    menuitems: ptr::null_mut(),
    routine: Some(M_DrawNewGame),
    x: 48,
    y: 63,
    lastOn: 2,
};

/// Page descriptor for the Options menu.
static mut OptionsDef: menu_t = menu_t {
    numitems: opt_end as i16,
    prevMenu: ptr::null_mut(),
    menuitems: ptr::null_mut(),
    routine: Some(M_DrawOptions),
    x: 60,
    y: 37,
    lastOn: 0,
};

/// Page descriptor for the first "Read This" help screen.
/// The skull position (x, y) may be adjusted at runtime by `M_DrawReadThis1`
/// depending on the game version.
static mut ReadDef1: menu_t = menu_t {
    numitems: read1_end as i16,
    prevMenu: ptr::null_mut(),
    menuitems: ptr::null_mut(),
    routine: Some(M_DrawReadThis1),
    x: 280,
    y: 185,
    lastOn: 0,
};

/// Page descriptor for the second "Read This" help screen (Doom 1.x only, non-commercial).
static mut ReadDef2: menu_t = menu_t {
    numitems: read2_end as i16,
    prevMenu: ptr::null_mut(),
    menuitems: ptr::null_mut(),
    routine: Some(M_DrawReadThis2),
    x: 330,
    y: 175,
    lastOn: 0,
};

/// Page descriptor for the Sound Volume menu.
static mut SoundDef: menu_t = menu_t {
    numitems: sound_end as i16,
    prevMenu: ptr::null_mut(),
    menuitems: ptr::null_mut(),
    routine: Some(M_DrawSound),
    x: 80,
    y: 64,
    lastOn: 0,
};

/// Page descriptor for the Load Game menu.
static mut LoadDef: menu_t = menu_t {
    numitems: load_end as i16,
    prevMenu: ptr::null_mut(),
    menuitems: ptr::null_mut(),
    routine: Some(M_DrawLoad),
    x: 80,
    y: 54,
    lastOn: 0,
};

/// Page descriptor for the Save Game menu (reuses `load_end` count and `LoadDef` geometry).
static mut SaveDef: menu_t = menu_t {
    numitems: load_end as i16,
    prevMenu: ptr::null_mut(),
    menuitems: ptr::null_mut(),
    routine: Some(M_DrawSave),
    x: 80,
    y: 54,
    lastOn: 0,
};

/// Populate `savegamestrings` and `LoadMenu[*].status` by reading the header of each save file.
///
/// Opens each save file by name (via `P_SaveGameFile`), reads the first
/// `SAVESTRINGSIZE` bytes as the description, and marks the slot active.
/// Slots whose file does not exist receive the placeholder text "empty slot"
/// and have their `status` set to 0 (non-selectable in the load menu).
fn M_ReadSaveStrings() {
    unsafe {
        for i in 0..load_end {
            let slot = &mut *ptr::addr_of_mut!(savegamestrings[i]);
            let Ok(mut file) = std::fs::File::open(P_SaveGameFile(i as i32)) else {
                M_StringCopy(slot, EMPTYSTRING);
                LoadMenu[i].status = 0;
                continue;
            };
            // Like the C `fread`, a short read is ignored.
            let _ = file.read(slot);
            LoadMenu[i].status = 1;
        }
    }
}

/// Draw the Load Game menu page: title patch plus one bordered slot row per save slot.
fn M_DrawLoad() {
    unsafe {
        V_DrawPatchDirect(72, 28, W_CacheLumpName(b"M_LOADG", 0) as *mut patch_t);

        for i in 0..load_end {
            M_DrawSaveLoadBorder(LoadDef.x as i32, LoadDef.y as i32 + LINEHEIGHT * i as i32);
            M_WriteText(
                LoadDef.x as i32,
                LoadDef.y as i32 + LINEHEIGHT * i as i32,
                until_nul(&*ptr::addr_of!(savegamestrings[i])),
            );
        }
    }
}

/// Draw the left/center/right border patches around a save-game name text field at `(x, y)`.
fn M_DrawSaveLoadBorder(x: i32, y: i32) {
    V_DrawPatchDirect(
        x - 8,
        y + 7,
        W_CacheLumpName(b"M_LSLEFT", 0) as *mut patch_t,
    );

    let mut xi = x;
    for _ in 0..24 {
        V_DrawPatchDirect(xi, y + 7, W_CacheLumpName(b"M_LSCNTR", 0) as *mut patch_t);
        xi += 8;
    }

    V_DrawPatchDirect(xi, y + 7, W_CacheLumpName(b"M_LSRGHT", 0) as *mut patch_t);
}

/// Load the game from save slot `choice` and close all menus.
fn M_LoadSelect(choice: i32) {
    unsafe {
        G_LoadGame(&P_SaveGameFile(choice));
        M_ClearMenus();
    }
}

/// Navigate to the Load Game menu page (suppressed during network games).
fn M_LoadGame(_choice: i32) {
    unsafe {
        if netgame != 0 {
            M_StartMessage(LOADNET, None, 0);
            return;
        }
        M_SetupNextMenu(&raw mut LoadDef);
    }
    M_ReadSaveStrings();
}

/// Draw the Save Game menu page: title, bordered slot rows, and a blinking cursor when editing.
fn M_DrawSave() {
    unsafe {
        V_DrawPatchDirect(72, 28, W_CacheLumpName(b"M_SAVEG", 0) as *mut patch_t);
        for i in 0..load_end {
            M_DrawSaveLoadBorder(LoadDef.x as i32, LoadDef.y as i32 + LINEHEIGHT * i as i32);
            M_WriteText(
                LoadDef.x as i32,
                LoadDef.y as i32 + LINEHEIGHT * i as i32,
                until_nul(&*ptr::addr_of!(savegamestrings[i])),
            );
        }

        if saveStringEnter != 0 {
            let w = M_StringWidth(until_nul(&*ptr::addr_of!(
                savegamestrings[saveSlot as usize]
            )));
            M_WriteText(
                LoadDef.x as i32 + w,
                LoadDef.y as i32 + LINEHEIGHT * saveSlot,
                b"_",
            );
        }
    }
}

/// Commit the save to `slot`, clear the menus, and record the slot as the quicksave target.
fn M_DoSave(slot: i32) {
    unsafe {
        G_SaveGame(slot, &*ptr::addr_of!(savegamestrings[slot as usize]));
        M_ClearMenus();
        if quickSaveSlot == -2 {
            quickSaveSlot = slot;
        }
    }
}

/// Enter string-editing mode for save slot `choice`, preserving the old description.
fn M_SaveSelect(choice: i32) {
    unsafe {
        saveStringEnter = 1;
        saveSlot = choice;
        let slot = &mut *ptr::addr_of_mut!(savegamestrings[choice as usize]);
        M_StringCopy(&mut *ptr::addr_of_mut!(saveOldString), slot);
        if until_nul(slot) == EMPTYSTRING {
            slot[0] = 0;
        }
        saveCharIndex = until_nul(slot).len() as i32;
    }
}

/// Navigate to the Save Game menu page (suppressed when not in-game or not at `GS_LEVEL`).
fn M_SaveGame(_choice: i32) {
    const GS_LEVEL: i32 = 0;
    unsafe {
        if usergame == 0 {
            M_StartMessage(SAVEDEAD, None, 0);
            return;
        }
        if gamestate != GS_LEVEL {
            return;
        }
        M_SetupNextMenu(&raw mut SaveDef);
    }
    M_ReadSaveStrings();
}

/// Perform a quicksave to the previously chosen slot, or open the save menu if none was chosen.
///
/// Plays `Sfx::Oof` and returns immediately if the game is not in progress.
/// If `quickSaveSlot` is -1, opens the save menu and sets it to -2 to indicate
/// "waiting for slot selection". Otherwise shows a y/n confirmation prompt.
fn M_QuickSave() {
    const GS_LEVEL: i32 = 0;
    unsafe {
        if usergame == 0 {
            S_StartSound(ptr::null_mut(), Sfx::Oof as i32);
            return;
        }
        if gamestate != GS_LEVEL {
            return;
        }
        if quickSaveSlot < 0 {
            M_StartControlPanel();
            M_ReadSaveStrings();
            M_SetupNextMenu(&raw mut SaveDef);
            quickSaveSlot = -2;
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
}

/// Description of the quicksave slot, for the quicksave/quickload prompts.
///
/// # Safety
/// `quickSaveSlot` must be a valid slot index.
unsafe fn quick_save_name() -> String {
    String::from_utf8_lossy(until_nul(&*ptr::addr_of!(
        savegamestrings[quickSaveSlot as usize]
    )))
    .into_owned()
}

/// Confirmation callback for the quicksave y/n prompt; saves if `key` is the confirm key.
fn M_QuickSaveResponse(key: i32) {
    use crate::engine::misc::controls::key_menu_confirm;
    unsafe {
        if key == key_menu_confirm {
            M_DoSave(quickSaveSlot);
            S_StartSound(ptr::null_mut(), Sfx::Swtchx as i32);
        }
    }
}

/// Perform a quickload from the previously chosen slot, or show an error if none was chosen.
///
/// Suppressed during network games. Shows a y/n confirmation before loading.
fn M_QuickLoad() {
    unsafe {
        if netgame != 0 {
            M_StartMessage(QLOADNET, None, 0);
            return;
        }
        if quickSaveSlot < 0 {
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
}

/// Confirmation callback for the quickload y/n prompt; loads if `key` is the confirm key.
fn M_QuickLoadResponse(key: i32) {
    use crate::engine::misc::controls::key_menu_confirm;
    unsafe {
        if key == key_menu_confirm {
            M_LoadSelect(quickSaveSlot);
            S_StartSound(ptr::null_mut(), Sfx::Swtchx as i32);
        }
    }
}

/// Draw the first "Read This" help screen and position the skull cursor.
///
/// Selects the correct WAD lump (`HELP`, `HELP1`, or `HELP2`) and skull
/// position based on `gameversion` and `gamemode`.
fn M_DrawReadThis1() {
    unsafe {
        inhelpscreens = 1;

        let lumpname: &[u8];
        let skullx: i16;
        let skully: i16;

        match gameversion {
            doom::mode::exe_doom_1_666
            | doom::mode::exe_doom_1_7
            | doom::mode::exe_doom_1_8
            | doom::mode::exe_doom_1_9
            | doom::mode::exe_hacx => {
                if gamemode == doom::mode::commercial {
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

        V_DrawPatchDirect(0, 0, W_CacheLumpName(lumpname, 0) as *mut patch_t);
        ReadDef1.x = skullx;
        ReadDef1.y = skully;
    }
}

/// Draw the second "Read This" help screen (`HELP1` lump).
fn M_DrawReadThis2() {
    unsafe {
        inhelpscreens = 1;
        V_DrawPatchDirect(0, 0, W_CacheLumpName(b"HELP1", 0) as *mut patch_t);
    }
}

/// Draw the Sound Volume menu page: title patch and two thermometer sliders.
fn M_DrawSound() {
    use crate::engine::sound::{musicVolume, sfxVolume};
    unsafe {
        V_DrawPatchDirect(60, 38, W_CacheLumpName(b"M_SVOL", 0) as *mut patch_t);

        M_DrawThermo(
            SoundDef.x as i32,
            SoundDef.y as i32 + LINEHEIGHT * (sfx_vol as i32 + 1),
            16,
            sfxVolume,
        );

        M_DrawThermo(
            SoundDef.x as i32,
            SoundDef.y as i32 + LINEHEIGHT * (music_vol as i32 + 1),
            16,
            musicVolume,
        );
    }
}

/// No-op activation callback for the Sound Volume menu item (navigation handled elsewhere).
fn M_Sound(_choice: i32) {}

/// Adjust the SFX volume slider; `choice` 0 decrements, 1 increments (range 0-15).
fn M_SfxVol(choice: i32) {
    use crate::engine::sound::sfxVolume;
    unsafe {
        match choice {
            0 if sfxVolume > 0 => {
                sfxVolume -= 1;
            }
            1 if sfxVolume < 15 => {
                sfxVolume += 1;
            }
            _ => {}
        }
        S_SetSfxVolume(sfxVolume * 8);
    }
}

/// Adjust the music volume slider; `choice` 0 decrements, 1 increments (range 0-15).
fn M_MusicVol(choice: i32) {
    use crate::engine::sound::musicVolume;
    unsafe {
        match choice {
            0 if musicVolume > 0 => {
                musicVolume -= 1;
            }
            1 if musicVolume < 15 => {
                musicVolume += 1;
            }
            _ => {}
        }
        S_SetMusicVolume(musicVolume * 8);
    }
}

/// Draw the "DOOM" title graphic at the top of the main menu.
fn M_DrawMainMenu() {
    V_DrawPatchDirect(94, 2, W_CacheLumpName(b"M_DOOM", 0) as *mut patch_t);
}

/// Draw the "New Game" and "Skill Level" title patches above the skill menu.
fn M_DrawNewGame() {
    V_DrawPatchDirect(96, 14, W_CacheLumpName(b"M_NEWG", 0) as *mut patch_t);
    V_DrawPatchDirect(54, 38, W_CacheLumpName(b"M_SKILL", 0) as *mut patch_t);
}

/// Handle "New Game" activation: navigate to the episode or skill menu as appropriate.
///
/// Shows an error message if in a network game (except demo playback).
/// Skips the episode menu for Doom II (commercial) and Chex Quest.
fn M_NewGame(_choice: i32) {
    unsafe {
        if netgame != 0 && demoplayback == 0 {
            M_StartMessage(NEWGAME, None, 0);
            return;
        }
        if gamemode == doom::mode::commercial || gameversion == doom::mode::exe_chex {
            M_SetupNextMenu(&raw mut NewDef);
        } else {
            M_SetupNextMenu(&raw mut EpiDef);
        }
    }
}

/// Draw the "Which Episode?" title patch above the episode menu.
fn M_DrawEpisode() {
    V_DrawPatchDirect(54, 38, W_CacheLumpName(b"M_EPISOD", 0) as *mut patch_t);
}

/// Selected episode index (0-based), set by `M_Episode` and consumed by `M_ChooseSkill`.
static mut epi: i32 = 0;

/// Confirmation callback for the Nightmare skill prompt; starts the game if `key` confirms.
fn M_VerifyNightmare(key: i32) {
    use crate::engine::misc::controls::key_menu_confirm;
    unsafe {
        if key != key_menu_confirm {
            return;
        }
        G_DeferedInitNew(4, epi + 1, 1);
        M_ClearMenus();
    }
}

/// Start a new game at the given skill level, or prompt for confirmation on Nightmare.
fn M_ChooseSkill(choice: i32) {
    if choice as usize == nightmare {
        M_StartMessage(NIGHTMARE, Some(M_VerifyNightmare), 1);
        return;
    }
    unsafe {
        G_DeferedInitNew(choice, epi + 1, 1);
        M_ClearMenus();
    }
}

/// Store the chosen episode index and navigate to the skill menu.
///
/// Shows a shareware-restriction message if episode > 0 in shareware mode.
/// Clamps episode 4 to episode 0 in registered mode (which only has three episodes).
fn M_Episode(choice: i32) {
    unsafe {
        if gamemode == doom::mode::shareware && choice != 0 {
            M_StartMessage(SWSTRING, None, 0);
            M_SetupNextMenu(&raw mut ReadDef1);
            return;
        }
        if gamemode == doom::mode::registered && choice > 2 {
            epi = 0;
        } else {
            epi = choice;
        }
        M_SetupNextMenu(&raw mut NewDef);
    }
}

/// Draw the Options menu page: title patch, detail/message toggles, and thermometer sliders.
fn M_DrawOptions() {
    unsafe {
        V_DrawPatchDirect(108, 15, W_CacheLumpName(b"M_OPTTTL", 0) as *mut patch_t);

        let detail_names: [&[u8]; 2] = [b"M_GDHIGH", b"M_GDLOW"];
        let msg_names: [&[u8]; 2] = [b"M_MSGOFF", b"M_MSGON"];

        V_DrawPatchDirect(
            OptionsDef.x as i32 + 175,
            OptionsDef.y as i32 + LINEHEIGHT * detail as i32,
            W_CacheLumpName(detail_names[detailLevel as usize], 0) as *mut patch_t,
        );

        V_DrawPatchDirect(
            OptionsDef.x as i32 + 120,
            OptionsDef.y as i32 + LINEHEIGHT * messages as i32,
            W_CacheLumpName(msg_names[showMessages as usize], 0) as *mut patch_t,
        );

        M_DrawThermo(
            OptionsDef.x as i32,
            OptionsDef.y as i32 + LINEHEIGHT * (mousesens as i32 + 1),
            10,
            mouseSensitivity,
        );

        M_DrawThermo(
            OptionsDef.x as i32,
            OptionsDef.y as i32 + LINEHEIGHT * (scrnsize as i32 + 1),
            9,
            screenSize,
        );
    }
}

/// No-op activation callback for the Options menu item (navigation handled by `M_Responder`).
fn M_Options(_choice: i32) {}

/// Toggle the `showMessages` setting and display a confirmation HUD message.
fn M_ChangeMessages(_choice: i32) {
    unsafe {
        showMessages = 1 - showMessages;
        if showMessages == 0 {
            M_Menu_SetPlayerMessage(MSGOFF);
        } else {
            M_Menu_SetPlayerMessage(MSGON);
        }
        message_dontfuckwithme = 1;
    }
}

/// Confirmation callback for "End Game"; clears menus if the player confirms.
fn M_EndGameResponse(key: i32) {
    use crate::engine::misc::controls::key_menu_confirm;
    unsafe {
        if key != key_menu_confirm {
            return;
        }
        (*currentMenu).lastOn = itemOn;
    }
    M_ClearMenus();
}

/// Prompt the player to confirm ending the current game (suppressed if not in-game or in a network game).
fn M_EndGame(_choice: i32) {
    unsafe {
        if usergame == 0 {
            S_StartSound(ptr::null_mut(), Sfx::Oof as i32);
            return;
        }
        if netgame != 0 {
            M_StartMessage(NETEND, None, 0);
            return;
        }
        M_StartMessage(ENDGAME, Some(M_EndGameResponse), 1);
    }
}

/// No-op placeholder for the "Read This" main-menu item (navigation is in `M_Responder`).
fn M_ReadThis(_choice: i32) {}

/// Handle "done" from the first help screen: advance to page 2 or finish, depending on game version.
fn M_ReadThis2(_choice: i32) {
    unsafe {
        if gameversion <= doom::mode::exe_doom_1_9 && gamemode != doom::mode::commercial {
            M_SetupNextMenu(&raw mut ReadDef2);
        } else {
            M_FinishReadThis(0);
        }
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
    use crate::engine::misc::controls::key_menu_confirm;
    unsafe {
        if key != key_menu_confirm {
            return;
        }
        if netgame == 0 {
            if gamemode == doom::mode::commercial {
                S_StartSound(ptr::null_mut(), quitsounds2[((gametic >> 2) & 7) as usize]);
            } else {
                S_StartSound(ptr::null_mut(), quitsounds[((gametic >> 2) & 7) as usize]);
            }
            I_WaitVBL(105);
        }
        I_Quit();
    }
}

/// Select a game-specific quit quip from `doom1_endmsg` or `doom2_endmsg`, keyed by `gametic`.
fn M_SelectEndMessage() -> &'static str {
    unsafe {
        if logical_gamemission() == doom::mode::doom {
            doom1_endmsg[(gametic as usize) & 7]
        } else {
            doom2_endmsg[(gametic as usize) & 7]
        }
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
    unsafe {
        match choice {
            0 if mouseSensitivity > 0 => {
                mouseSensitivity -= 1;
            }
            1 if mouseSensitivity < 9 => {
                mouseSensitivity += 1;
            }
            _ => {}
        }
    }
}

/// Toggle graphics detail between high (0) and low (1) and display a HUD confirmation.
fn M_ChangeDetail(_choice: i32) {
    unsafe {
        detailLevel = 1 - detailLevel;
        R_SetViewSize(screenblocks, detailLevel);
        if detailLevel == 0 {
            M_Menu_SetPlayerMessage(DETAILHI);
        } else {
            M_Menu_SetPlayerMessage(DETAILLO);
        }
    }
}

/// Adjust the screen-size slider; `choice` 0 shrinks, 1 enlarges (range 0-8).
///
/// Updates both `screenSize` and `screenblocks`, then calls `R_SetViewSize`.
fn M_SizeDisplay(choice: i32) {
    unsafe {
        match choice {
            0 if screenSize > 0 => {
                screenblocks -= 1;
                screenSize -= 1;
            }
            1 if screenSize < 8 => {
                screenblocks += 1;
                screenSize += 1;
            }
            _ => {}
        }
        R_SetViewSize(screenblocks, detailLevel);
    }
}

/// Draw a thermometer slider at `(x, y)` with `thermWidth` cells and a filled dot at `thermDot`.
///
/// Renders left cap, `thermWidth` middle pieces, right cap, then the movable dot.
fn M_DrawThermo(x: i32, y: i32, thermWidth: i32, thermDot: i32) {
    let mut xx = x;
    V_DrawPatchDirect(xx, y, W_CacheLumpName(b"M_THERML", 0) as *mut patch_t);
    xx += 8;
    for _ in 0..thermWidth {
        V_DrawPatchDirect(xx, y, W_CacheLumpName(b"M_THERMM", 0) as *mut patch_t);
        xx += 8;
    }
    V_DrawPatchDirect(xx, y, W_CacheLumpName(b"M_THERMR", 0) as *mut patch_t);

    V_DrawPatchDirect(
        (x + 8) + thermDot * 8,
        y,
        W_CacheLumpName(b"M_THERMO", 0) as *mut patch_t,
    );
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
    unsafe {
        messageLastMenuActive = menuactive;
        messageToPrint = 1;
        messageString = string.to_owned();
        messageRoutine = routine;
        messageNeedsInput = input;
        menuactive = 1;
    }
}

/// Dismiss the current modal overlay message and restore the previous `menuactive` state.
fn M_StopMessage() {
    unsafe {
        menuactive = messageLastMenuActive;
        messageToPrint = 0;
    }
}

/// Return the pixel width of `string` rendered in the HUD font.
///
/// Non-printable characters (outside `HU_FONTSTART`..`HU_FONTEND`) contribute 4 pixels each.
fn M_StringWidth(text: &[u8]) -> i32 {
    unsafe {
        let mut w: i32 = 0;
        for &byte in text {
            let c = byte.to_ascii_uppercase() as i32 - HU_FONTSTART;
            if c < 0 || c as usize >= HU_FONTSIZE {
                w += 4;
            } else {
                let patch = hu_font[c as usize] as *const patch_stub;
                w += (*patch).width as i32;
            }
        }
        w
    }
}

/// Return the pixel height of `text` rendered in the HUD font, accounting for newlines.
fn M_StringHeight(text: &[u8]) -> i32 {
    unsafe {
        let patch = hu_font[0] as *const patch_stub;
        let height = (*patch).height as i32;
        let newlines = text.iter().filter(|&&byte| byte == b'\n').count() as i32;
        height + newlines * height
    }
}

/// Render `text` using the HUD font at screen position `(x, y)`.
///
/// Newline characters reset the x cursor and advance y by 12 pixels.
/// Characters outside the font range are rendered as 4-pixel spaces.
/// Rendering stops at the screen right edge.
fn M_WriteText(x: i32, y: i32, text: &[u8]) {
    unsafe {
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

            let patch = hu_font[c as usize] as *const patch_stub;
            let w = (*patch).width as i32;
            if cx + w > SCREENWIDTH {
                break;
            }
            V_DrawPatchDirect(cx, cy, hu_font[c as usize]);
            cx += w;
        }
    }
}

/// Return `true` if `key` is a modifier key that should not terminate shortcut search.
///
/// Pause, Caps Lock, Scroll Lock, and Num Lock are treated as null keys.
fn IsNullKey(key: i32) -> bool {
    key == KEY_PAUSE || key == KEY_CAPSLOCK || key == KEY_SCRLCK || key == KEY_NUMLOCK
}

/// Tic time after which the next joystick event will be processed (rate-limiting).
static mut RESP_joywait: i32 = 0;
/// Tic time after which the next mouse event will be processed (rate-limiting).
static mut RESP_mousewait: i32 = 0;
/// Accumulated mouse y movement since the last `RESP_lasty` reset.
static mut RESP_mousey: i32 = 0;
/// Mouse y baseline; updated in 30-unit steps to produce discrete menu scrolls.
static mut RESP_lasty: i32 = 0;
/// Accumulated mouse x movement since the last `RESP_lastx` reset.
static mut RESP_mousex: i32 = 0;
/// Mouse x baseline; updated in 30-unit steps to produce discrete menu navigation.
static mut RESP_lastx: i32 = 0;

/// Handle an input event for the menu system.
///
/// Translates joystick, mouse, and keyboard events into menu navigation actions
/// (up/down/left/right/forward/back/activate/abort) and F-key shortcuts
/// (quicksave, quickload, screen-size, gamma, screenshot, etc.).
/// Also handles save-game string editing character-by-character.
///
/// Returns `Boolean::TRUE` if the event was consumed, `Boolean::FALSE` otherwise.
/// Called by `G_Responder` in `g_game.c`.
pub fn M_Responder(ev: *mut event_t) -> Boolean {
    use crate::engine::misc::controls::{
        joybmenu, key_menu_abort, key_menu_activate, key_menu_back, key_menu_confirm,
        key_menu_decscreen, key_menu_detail, key_menu_down, key_menu_endgame, key_menu_forward,
        key_menu_gamma, key_menu_help, key_menu_incscreen, key_menu_left, key_menu_load,
        key_menu_messages, key_menu_qload, key_menu_qsave, key_menu_quit, key_menu_right,
        key_menu_save, key_menu_screenshot, key_menu_up, key_menu_volume,
    };

    unsafe {
        let ev = &*ev;

        // testcontrols mode
        if testcontrols != 0 {
            if ev.type_ == EV_QUIT
                || (ev.type_ == EV_KEYDOWN
                    && (ev.data1 == key_menu_activate || ev.data1 == key_menu_quit))
            {
                I_Quit();
                return Boolean::TRUE;
            }
            return Boolean::FALSE;
        }

        // window close button
        if ev.type_ == EV_QUIT {
            if menuactive != 0
                && messageToPrint != 0
                && messageRoutine.map(|f| f as usize).unwrap_or(0)
                    == M_QuitResponse as *const () as usize
            {
                M_QuitResponse(key_menu_confirm);
            } else {
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as i32);
                M_QuitDOOM(0);
            }
            return Boolean::TRUE;
        }

        let mut ch: i32 = 0;
        let mut key: i32 = -1;

        if ev.type_ == EV_JOYSTICK && RESP_joywait < I_GetTime() {
            if ev.data3 < 0 {
                key = key_menu_up;
                RESP_joywait = I_GetTime() + 5;
            } else if ev.data3 > 0 {
                key = key_menu_down;
                RESP_joywait = I_GetTime() + 5;
            }
            if ev.data2 < 0 {
                key = key_menu_left;
                RESP_joywait = I_GetTime() + 2;
            } else if ev.data2 > 0 {
                key = key_menu_right;
                RESP_joywait = I_GetTime() + 2;
            }
            if ev.data1 & 1 != 0 {
                key = key_menu_forward;
                RESP_joywait = I_GetTime() + 5;
            }
            if ev.data1 & 2 != 0 {
                key = key_menu_back;
                RESP_joywait = I_GetTime() + 5;
            }
            if joybmenu >= 0 && (ev.data1 & (1 << joybmenu)) != 0 {
                key = key_menu_activate;
                RESP_joywait = I_GetTime() + 5;
            }
        } else if ev.type_ == EV_MOUSE && RESP_mousewait < I_GetTime() {
            RESP_mousey += ev.data3;
            if RESP_mousey < RESP_lasty - 30 {
                key = key_menu_down;
                RESP_mousewait = I_GetTime() + 5;
                RESP_lasty -= 30;
                RESP_mousey = RESP_lasty;
            } else if RESP_mousey > RESP_lasty + 30 {
                key = key_menu_up;
                RESP_mousewait = I_GetTime() + 5;
                RESP_lasty += 30;
                RESP_mousey = RESP_lasty;
            }

            RESP_mousex += ev.data2;
            if RESP_mousex < RESP_lastx - 30 {
                key = key_menu_left;
                RESP_mousewait = I_GetTime() + 5;
                RESP_lastx -= 30;
                RESP_mousex = RESP_lastx;
            } else if RESP_mousex > RESP_lastx + 30 {
                key = key_menu_right;
                RESP_mousewait = I_GetTime() + 5;
                RESP_lastx += 30;
                RESP_mousex = RESP_lastx;
            }

            if ev.data1 & 1 != 0 {
                key = key_menu_forward;
                RESP_mousewait = I_GetTime() + 15;
            }
            if ev.data1 & 2 != 0 {
                key = key_menu_back;
                RESP_mousewait = I_GetTime() + 15;
            }
        } else if ev.type_ == EV_KEYDOWN {
            key = ev.data1;
            ch = ev.data2;
        }

        if key == -1 {
            return Boolean::FALSE;
        }

        // Save Game string input
        if saveStringEnter != 0 {
            if key == KEY_BACKSPACE {
                if saveCharIndex > 0 {
                    saveCharIndex -= 1;
                    savegamestrings[saveSlot as usize][saveCharIndex as usize] = 0;
                }
            } else if key == KEY_ESCAPE {
                saveStringEnter = 0;
                M_StringCopy(
                    &mut *ptr::addr_of_mut!(savegamestrings[saveSlot as usize]),
                    &*ptr::addr_of!(saveOldString),
                );
            } else if key == KEY_ENTER {
                saveStringEnter = 0;
                if savegamestrings[saveSlot as usize][0] != 0 {
                    M_DoSave(saveSlot);
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

                if (32..=127).contains(&ch)
                    && saveCharIndex < (SAVESTRINGSIZE as i32 - 1)
                    && M_StringWidth(until_nul(&*ptr::addr_of!(
                        savegamestrings[saveSlot as usize]
                    ))) < (SAVESTRINGSIZE as i32 - 2) * 8
                {
                    savegamestrings[saveSlot as usize][saveCharIndex as usize] = ch as u8;
                    saveCharIndex += 1;
                    savegamestrings[saveSlot as usize][saveCharIndex as usize] = 0;
                }
            }
            return Boolean::TRUE;
        }

        // Messages that need input
        if messageToPrint != 0 {
            if messageNeedsInput != 0
                && key != b' ' as i32
                && key != KEY_ESCAPE
                && key != key_menu_confirm
                && key != key_menu_abort
            {
                return Boolean::FALSE;
            }

            menuactive = messageLastMenuActive;
            messageToPrint = 0;
            if let Some(r) = messageRoutine {
                r(key);
            }
            menuactive = 0;
            S_StartSound(ptr::null_mut(), Sfx::Swtchx as i32);
            return Boolean::TRUE;
        }

        // Screenshot
        if (devparm != 0 && key == key_menu_help) || (key != 0 && key == key_menu_screenshot) {
            G_ScreenShot();
            return Boolean::TRUE;
        }

        // F-Keys (when menu not active)
        if menuactive == 0 {
            if key == key_menu_decscreen {
                if automapactive != 0 || chat_on != 0 {
                    return Boolean::FALSE;
                }
                M_SizeDisplay(0);
                S_StartSound(ptr::null_mut(), Sfx::Stnmov as i32);
                return Boolean::TRUE;
            } else if key == key_menu_incscreen {
                if automapactive != 0 || chat_on != 0 {
                    return Boolean::FALSE;
                }
                M_SizeDisplay(1);
                S_StartSound(ptr::null_mut(), Sfx::Stnmov as i32);
                return Boolean::TRUE;
            } else if key == key_menu_help {
                M_StartControlPanel();
                if gamemode == doom::mode::retail {
                    currentMenu = &raw mut ReadDef2;
                } else {
                    currentMenu = &raw mut ReadDef1;
                }
                itemOn = 0;
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as i32);
                return Boolean::TRUE;
            } else if key == key_menu_save {
                M_StartControlPanel();
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as i32);
                M_SaveGame(0);
                return Boolean::TRUE;
            } else if key == key_menu_load {
                M_StartControlPanel();
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as i32);
                M_LoadGame(0);
                return Boolean::TRUE;
            } else if key == key_menu_volume {
                M_StartControlPanel();
                currentMenu = &raw mut SoundDef;
                itemOn = sfx_vol as i16;
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as i32);
                return Boolean::TRUE;
            } else if key == key_menu_detail {
                M_ChangeDetail(0);
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as i32);
                return Boolean::TRUE;
            } else if key == key_menu_qsave {
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as i32);
                M_QuickSave();
                return Boolean::TRUE;
            } else if key == key_menu_endgame {
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as i32);
                M_EndGame(0);
                return Boolean::TRUE;
            } else if key == key_menu_messages {
                M_ChangeMessages(0);
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as i32);
                return Boolean::TRUE;
            } else if key == key_menu_qload {
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as i32);
                M_QuickLoad();
                return Boolean::TRUE;
            } else if key == key_menu_quit {
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as i32);
                M_QuitDOOM(0);
                return Boolean::TRUE;
            } else if key == key_menu_gamma {
                usegamma += 1;
                if usegamma > 4 {
                    usegamma = 0;
                }
                M_Menu_SetPlayerMessage(gammamsg[usegamma as usize]);
                I_SetPalette(W_CacheLumpName(b"PLAYPAL", 0) as *mut u8);
                return Boolean::TRUE;
            }
        }

        // Pop-up menu?
        if menuactive == 0 {
            if key == key_menu_activate {
                M_StartControlPanel();
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as i32);
                return Boolean::TRUE;
            }
            return Boolean::FALSE;
        }

        // Keys usable within menu
        if key == key_menu_down {
            loop {
                if itemOn + 1 > (*currentMenu).numitems - 1 {
                    itemOn = 0;
                } else {
                    itemOn += 1;
                }
                S_StartSound(ptr::null_mut(), Sfx::Pstop as i32);
                if (*(*currentMenu).menuitems.offset(itemOn as isize)).status != -1 {
                    break;
                }
            }
            return Boolean::TRUE;
        } else if key == key_menu_up {
            loop {
                if itemOn == 0 {
                    itemOn = (*currentMenu).numitems - 1;
                } else {
                    itemOn -= 1;
                }
                S_StartSound(ptr::null_mut(), Sfx::Pstop as i32);
                if (*(*currentMenu).menuitems.offset(itemOn as isize)).status != -1 {
                    break;
                }
            }
            return Boolean::TRUE;
        } else if key == key_menu_left {
            let item = &*(*currentMenu).menuitems.offset(itemOn as isize);
            if item.status == 2 {
                if let Some(routine) = item.routine {
                    S_StartSound(ptr::null_mut(), Sfx::Stnmov as i32);
                    routine(0);
                }
            }
            return Boolean::TRUE;
        } else if key == key_menu_right {
            let item = &*(*currentMenu).menuitems.offset(itemOn as isize);
            if item.status == 2 {
                if let Some(routine) = item.routine {
                    S_StartSound(ptr::null_mut(), Sfx::Stnmov as i32);
                    routine(1);
                }
            }
            return Boolean::TRUE;
        } else if key == key_menu_forward {
            let item = &*(*currentMenu).menuitems.offset(itemOn as isize);
            if let Some(routine) = item.routine {
                if item.status != 0 {
                    (*currentMenu).lastOn = itemOn;
                    if item.status == 2 {
                        routine(1);
                        S_StartSound(ptr::null_mut(), Sfx::Stnmov as i32);
                    } else {
                        routine(itemOn as i32);
                        S_StartSound(ptr::null_mut(), Sfx::Pistol as i32);
                    }
                }
            }
            return Boolean::TRUE;
        } else if key == key_menu_activate {
            (*currentMenu).lastOn = itemOn;
            M_ClearMenus();
            S_StartSound(ptr::null_mut(), Sfx::Swtchx as i32);
            return Boolean::TRUE;
        } else if key == key_menu_back {
            (*currentMenu).lastOn = itemOn;
            if !(*currentMenu).prevMenu.is_null() {
                currentMenu = (*currentMenu).prevMenu;
                itemOn = (*currentMenu).lastOn;
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as i32);
            }
            return Boolean::TRUE;
        }

        // Keyboard shortcut
        if ch != 0 || IsNullKey(key) {
            let ch_u = ch as u8;
            for i in (itemOn + 1)..(*currentMenu).numitems {
                if (*(*currentMenu).menuitems.offset(i as isize)).alphaKey == ch_u {
                    itemOn = i;
                    S_StartSound(ptr::null_mut(), Sfx::Pstop as i32);
                    return Boolean::TRUE;
                }
            }
            for i in 0..=itemOn {
                if (*(*currentMenu).menuitems.offset(i as isize)).alphaKey == ch_u {
                    itemOn = i;
                    S_StartSound(ptr::null_mut(), Sfx::Pstop as i32);
                    return Boolean::TRUE;
                }
            }
        }

        Boolean::FALSE
    }
}

/// Open the main menu and set `currentMenu` to `MainDef`.
///
/// No-op if the menu is already active. Called from `M_Responder` and from
/// game code that needs to force the menu open (e.g. after a level warp cheat).
pub fn M_StartControlPanel() {
    unsafe {
        if menuactive != 0 {
            return;
        }
        menuactive = 1;
        currentMenu = &raw mut MainDef;
        itemOn = (*currentMenu).lastOn;
    }
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
        inhelpscreens = 0;

        if messageToPrint != 0 {
            let message = (*ptr::addr_of!(messageString)).as_bytes();
            messy = SCREENHEIGHT / 2 - M_StringHeight(message) / 2;
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
                let patch = hu_font[0] as *const patch_stub;
                messy += (*patch).height as i32;
            }
            return;
        }

        if menuactive == 0 {
            return;
        }

        if let Some(r) = (*currentMenu).routine {
            r();
        }

        let x = (*currentMenu).x as i32;
        let y = (*currentMenu).y as i32;
        let max = (*currentMenu).numitems as usize;

        for i in 0..max {
            let name = (*(*currentMenu).menuitems.add(i)).name;
            if !name.is_empty() {
                V_DrawPatchDirect(
                    x,
                    y + LINEHEIGHT * i as i32,
                    W_CacheLumpName(name.as_bytes(), 0) as *mut patch_t,
                );
            }
        }

        V_DrawPatchDirect(
            x + SKULLXOFF,
            (*currentMenu).y as i32 - 5 + itemOn as i32 * LINEHEIGHT,
            W_CacheLumpName(skullName[whichSkull as usize], 0) as *mut patch_t,
        );
    }
}

/// Close all menus by setting `menuactive` to 0.
fn M_ClearMenus() {
    unsafe {
        menuactive = 0;
    }
}

/// Switch to `menudef` as the active menu page and restore its last-highlighted item.
fn M_SetupNextMenu(menudef: *mut menu_t) {
    unsafe {
        currentMenu = menudef;
        itemOn = (*currentMenu).lastOn;
    }
}

/// Advance the skull cursor animation by one game tic.
///
/// Decrements `skullAnimCounter` and toggles `whichSkull` every 8 tics.
/// Called by `G_Ticker` in `g_game.c`.
pub fn M_Ticker() {
    unsafe {
        skullAnimCounter -= 1;
        if skullAnimCounter <= 0 {
            whichSkull ^= 1;
            skullAnimCounter = 8;
        }
    }
}

/// One-time initialisation of the menu subsystem.
///
/// Resets all state variables, wires `menuitems` pointers and `prevMenu`
/// cross-links that cannot be set at static-initialisation time (Rust forbids
/// raw-pointer cross-references between statics), trims the episode menu to
/// three items for pre-Ultimate builds, and removes "Read This" from the main
/// menu in commercial mode. Called once from `D_DoomMain` in `d_main.c`.
pub fn M_Init() {
    unsafe {
        currentMenu = &raw mut MainDef;
        menuactive = 0;
        itemOn = (*currentMenu).lastOn;
        whichSkull = 0;
        skullAnimCounter = 10;
        screenSize = screenblocks - 3;
        messageToPrint = 0;
        messageString = String::new();
        messageLastMenuActive = menuactive;
        quickSaveSlot = -1;

        // Set menuitems pointers (couldn't be done at static init time)
        MainDef.menuitems = std::ptr::addr_of_mut!(MainMenu[0]);
        EpiDef.menuitems = std::ptr::addr_of_mut!(EpisodeMenu[0]);
        NewDef.menuitems = std::ptr::addr_of_mut!(NewGameMenu[0]);
        OptionsDef.menuitems = std::ptr::addr_of_mut!(OptionsMenu[0]);
        ReadDef1.menuitems = std::ptr::addr_of_mut!(ReadMenu1[0]);
        ReadDef2.menuitems = std::ptr::addr_of_mut!(ReadMenu2[0]);
        SoundDef.menuitems = std::ptr::addr_of_mut!(SoundMenu[0]);
        LoadDef.menuitems = std::ptr::addr_of_mut!(LoadMenu[0]);
        SaveDef.menuitems = std::ptr::addr_of_mut!(SaveMenu[0]);

        // Set up prevMenu cross-links
        EpiDef.prevMenu = &raw mut MainDef;
        NewDef.prevMenu = &raw mut EpiDef;
        OptionsDef.prevMenu = &raw mut MainDef;
        ReadDef1.prevMenu = &raw mut MainDef;
        ReadDef2.prevMenu = &raw mut ReadDef1;
        SoundDef.prevMenu = &raw mut OptionsDef;
        LoadDef.prevMenu = &raw mut MainDef;
        SaveDef.prevMenu = &raw mut MainDef;

        match gamemode {
            doom::mode::commercial => {
                ptr::write(&mut MainMenu[readthis], ptr::read(&MainMenu[quitdoom]));
                MainDef.numitems -= 1;
                MainDef.y += 8;
                NewDef.prevMenu = &raw mut MainDef;
            }
            doom::mode::shareware | doom::mode::registered | doom::mode::retail => {}
            _ => {}
        }

        if gameversion < doom::mode::exe_ultimate {
            EpiDef.numitems -= 1;
        }
    }
}
