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
//! - `make_gamma_msg` and `mi` are `const fn` helpers replacing C compound
//!   literals, keeping the lookup tables in static storage.
//! - `logical_gamemission` has a safe signature (`fn`, not `unsafe fn`); the
//!   C-linked global read is wrapped in an internal `unsafe { ... }` block.

#![allow(non_upper_case_globals, non_snake_case)]

use std::ffi::{c_char, c_int, c_void};
use std::ptr;

use crate::types::Boolean;

use super::d_event::event_t;
use super::d_mode;
use super::d_player::M_Menu_SetPlayerMessage;
use super::doomstat::{gamemission, gamemode, gameversion};
use crate::{c_write, i_error};

/// Return the canonical game mission, collapsing Chex Quest and HacX aliases.
///
/// Mirrors the `logical_gamemission` macro from `doomstat.h`:
/// `pack_chex` maps to `doom`, `pack_hacx` maps to `doom2`,
/// all other values are returned as-is.
fn logical_gamemission() -> c_int {
    unsafe {
        if gamemission == d_mode::pack_chex {
            d_mode::doom
        } else if gamemission == d_mode::pack_hacx {
            d_mode::doom2
        } else {
            gamemission
        }
    }
}

/// Vertical pixel stride between adjacent menu items (matches C `LINEHEIGHT`).
const LINEHEIGHT: c_int = 16;
/// Horizontal offset of the skull cursor relative to the menu item's x position (matches C `SKULLXOFF`).
const SKULLXOFF: c_int = -32;

/// ASCII code of the first character in the HUD font (matches C `HU_FONTSTART`).
const HU_FONTSTART: c_int = b'!' as c_int;
/// ASCII code of the last character in the HUD font (matches C `HU_FONTEND`).
const HU_FONTEND: c_int = b'_' as c_int;
/// Number of glyphs in the HUD font (derived from `HU_FONTSTART`/`HU_FONTEND`).
const HU_FONTSIZE: usize = (HU_FONTEND - HU_FONTSTART + 1) as usize;

use crate::doom::i_video::{SCREENHEIGHT, SCREENWIDTH};

/// Maximum number of characters in a save-game description string (matches C `SAVESTRINGSIZE`).
const SAVESTRINGSIZE: usize = 24;
/// Maximum number of simultaneous players; local alias for `d_player::MAXPLAYERS`.
const MAXPLAYERS: usize = 4;

/// Event type: key-down (matches C `ev_keydown`).
const EV_KEYDOWN: c_int = 0;
/// Event type: key-up (matches C `ev_keyup`).
const EV_KEYUP: c_int = 1;
/// Event type: mouse motion or button (matches C `ev_mouse`).
const EV_MOUSE: c_int = 2;
/// Event type: joystick input (matches C `ev_joystick`).
const EV_JOYSTICK: c_int = 3;
/// Event type: window close / quit request (matches C `ev_quit`).
const EV_QUIT: c_int = 4;

/// Key code for the Escape key (ASCII 27).
const KEY_ESCAPE: c_int = 27;
/// Key code for the Enter/Return key (ASCII 13).
const KEY_ENTER: c_int = 13;
/// Key code for the Backspace key (0x7f delete).
const KEY_BACKSPACE: c_int = 0x7f;
/// Key code for the Pause key (matches C `KEY_PAUSE`).
const KEY_PAUSE: c_int = 0xff;
/// Key code for Caps Lock (matches C `KEY_CAPSLOCK`).
const KEY_CAPSLOCK: c_int = 0x80 + 0x3a;
/// Key code for Num Lock (matches C `KEY_NUMLOCK`).
const KEY_NUMLOCK: c_int = 0x80 + 0x45;
/// Key code for Scroll Lock (matches C `KEY_SCRLCK`).
const KEY_SCRLCK: c_int = 0x80 + 0x46;

/// Build a `menuitem_t` at compile time.
///
/// `status` encodes the item type: 1 = activatable, 2 = slider, -1 = gap.
/// `name` is the WAD lump name (up to 10 bytes, NUL-padded).
/// `routine` is the callback invoked on selection or slider movement.
/// `alpha` is the keyboard shortcut character used for quick navigation.
const fn mi(
    status: i16,
    name: &[u8],
    routine: Option<extern "C" fn(c_int)>,
    alpha: u8,
) -> menuitem_t {
    let mut n = [0i8; 10];
    let mut i = 0;
    while i < name.len() && i < 10 {
        n[i] = name[i] as c_char;
        i += 1;
    }
    menuitem_t {
        status,
        name: n,
        routine,
        alphaKey: alpha as c_char,
    }
}

/// Convert a string literal into a fixed-length `[c_char; 26]` buffer at compile time.
///
/// Used to initialise the `gammamsg` table without heap allocation.
const fn make_gamma_msg(s: &str) -> [c_char; 26] {
    let bytes = s.as_bytes();
    let mut arr = [0i8; 26];
    let mut i = 0;
    while i < bytes.len() {
        arr[i] = bytes[i] as c_char;
        i += 1;
    }
    arr
}

/// A single entry in a Doom menu (C typedef `menuitem_t` from `m_menu.h`).
///
/// Layout is ABI-identical to the C struct; `#[repr(C)]` ensures this.
/// `status` encodes item kind: `1` = normal action, `2` = slider, `-1` = spacing row.
/// `name` is a NUL-terminated WAD lump name (10 bytes, 0-padded).
/// `routine` is called with the item index on activation.
/// `alphaKey` is the lowercase ASCII shortcut character.
#[repr(C)]
pub struct menuitem_t {
    /// Item kind: 1 = action, 2 = slider, -1 = gap row.
    pub status: i16,
    /// WAD lump name for the item's graphic patch (NUL-terminated, 10 bytes).
    pub name: [c_char; 10],
    /// Callback invoked when the item is activated or a slider is adjusted.
    pub routine: Option<extern "C" fn(c_int)>,
    /// Keyboard shortcut character for fast navigation within the menu.
    pub alphaKey: c_char,
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
    pub routine: Option<extern "C" fn()>,
    /// Screen x coordinate of the first menu item.
    pub x: i16,
    /// Screen y coordinate of the first menu item.
    pub y: i16,
    /// Index of the item that was last highlighted; restored on re-entry.
    pub lastOn: i16,
}

const _: () = assert!(
    std::mem::size_of::<menuitem_t>() == 32,
    "menuitem_t size mismatch"
);
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

extern "C" {
    fn fopen(path: *const c_char, mode: *const c_char) -> *mut c_void;
    fn fread(ptr: *mut c_void, size: usize, nmemb: usize, stream: *mut c_void) -> usize;
    fn fclose(stream: *mut c_void) -> c_int;
    fn toupper(c: c_int) -> c_int;
    fn strlen(s: *const c_char) -> usize;
    fn strcmp(s1: *const c_char, s2: *const c_char) -> c_int;
}

use crate::doom::am_map::automapactive;
use crate::doom::d_loop::gametic;
use crate::doom::d_main::devparm;
use crate::doom::dstrings::{doom1_endmsg, doom2_endmsg};
use crate::doom::g_game::{
    demoplayback, gamestate, netgame, testcontrols, usergame, G_DeferedInitNew, G_LoadGame,
    G_SaveGame, G_ScreenShot,
};
use crate::doom::hu_stuff::{chat_on, hu_font, message_dontfuckwithme};
use crate::doom::i_input::vanilla_keyboard_mapping;
use crate::doom::i_system::I_Quit;
use crate::doom::i_timer::{I_GetTime, I_WaitVBL};
use crate::doom::i_video::{usegamma, I_SetPalette};
use crate::doom::m_misc::M_StringCopy;
use crate::doom::p_saveg::P_SaveGameFile;
use crate::doom::r_main::R_SetViewSize;
use crate::doom::s_sound::{S_SetMusicVolume, S_SetSfxVolume, S_StartSound};
use crate::doom::sounds::Sfx;
use crate::doom::v_video::{patch_t, V_DrawPatchDirect};
use crate::doom::w_wad::W_CacheLumpName;

/// Current mouse sensitivity setting (0-9); exported to C for `m_config.c` serialization.
#[no_mangle]
pub static mut mouseSensitivity: c_int = 5;
/// Non-zero when player messages are enabled; exported for `hu_stuff.c` and config.
#[no_mangle]
pub static mut showMessages: c_int = 1;
/// Graphics detail level: 0 = high, 1 = low; exported for `r_main.c` and config.
#[no_mangle]
pub static mut detailLevel: c_int = 0;
/// Number of screen blocks to render (3-12); exported for `r_main.c` and config.
#[no_mangle]
pub static mut screenblocks: c_int = 10;

/// Current display size index (0-8), derived from `screenblocks - 3`; updated by `M_SizeDisplay`.
static mut screenSize: c_int = 0;
/// Save slot used for quicksave/quickload (-1 = none, -2 = slot selection pending).
static mut quickSaveSlot: c_int = -1;
/// Non-zero when a modal message/confirmation overlay is active.
static mut messageToPrint: c_int = 0;
/// Pointer to the C-string displayed in the active modal overlay.
static mut messageString: *mut c_char = ptr::null_mut();
/// Screen x pixel of the modal message's left edge.
static mut messx: c_int = 0;
/// Screen y pixel of the modal message's top edge.
static mut messy: c_int = 0;
/// Value of `menuactive` saved when a modal message was opened, restored on dismissal.
static mut messageLastMenuActive: c_int = 0;
/// Non-zero when the modal message requires a y/n or specific key response.
static mut messageNeedsInput: c_int = 0;
/// Callback invoked with the key code when the modal message is dismissed.
static mut messageRoutine: Option<extern "C" fn(c_int)> = None;

/// Gamma-correction status messages displayed when the player cycles the gamma level.
///
/// Index matches the `usegamma` value (0-4). Exported for C callers in `m_config.c`.
#[no_mangle]
pub static mut gammamsg: [[c_char; 26]; 5] = [
    make_gamma_msg("Gamma correction OFF"),
    make_gamma_msg("Gamma correction level 1"),
    make_gamma_msg("Gamma correction level 2"),
    make_gamma_msg("Gamma correction level 3"),
    make_gamma_msg("Gamma correction level 4"),
];

/// Non-zero while the player is typing a save-game description string.
static mut saveStringEnter: c_int = 0;
/// Save slot index currently being named.
static mut saveSlot: c_int = 0;
/// Cursor position (character count) within the save-game description string.
static mut saveCharIndex: c_int = 0;
/// Copy of the original description before editing, restored on Escape.
static mut saveOldString: [c_char; SAVESTRINGSIZE] = [0; SAVESTRINGSIZE];

/// Non-zero while a help/read-this screen is being displayed; suppresses status bar.
/// Exported for `d_main.c` which checks it before re-drawing the play area.
#[no_mangle]
pub static mut inhelpscreens: c_int = 0;
/// Non-zero while the menu overlay is open; read by `G_Responder` and `D_Display`.
#[no_mangle]
pub static mut menuactive: c_int = 0;

/// Save-game description strings for all 6 save slots, read from each save file header.
static mut savegamestrings: [[c_char; SAVESTRINGSIZE]; 10] = [[0; SAVESTRINGSIZE]; 10];
/// Formatted quit confirmation string (game-specific quip + y/n prompt).
static mut endstring: [c_char; 160] = [0; 160];

/// Index of the currently highlighted menu item within the active page.
static mut itemOn: i16 = 0;
/// Countdown (in tics) until the skull cursor frame toggles.
static mut skullAnimCounter: i16 = 0;
/// Index into `skullName`: 0 = `M_SKULL1`, 1 = `M_SKULL2`.
static mut whichSkull: i16 = 0;

/// WAD lump names for the two skull cursor animation frames.
static mut skullName: [*const c_char; 2] = [c"M_SKULL1".as_ptr(), c"M_SKULL2".as_ptr()];

/// Pointer to the currently active menu page descriptor.
/// Exported for C callers (e.g. `d_main.c`).
#[no_mangle]
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
    mi(1, b"M_NGAME\0\0\0", Some(M_NewGame), b'n'),
    mi(1, b"M_OPTION\0\0", Some(M_Options), b'o'),
    mi(1, b"M_LOADG\0\0\0", Some(M_LoadGame), b'l'),
    mi(1, b"M_SAVEG\0\0\0", Some(M_SaveGame), b's'),
    mi(1, b"M_RDTHIS\0\0", Some(M_ReadThis), b'r'),
    mi(1, b"M_QUITG\0\0\0", Some(M_QuitDOOM), b'q'),
];

/// Item array for the episode selection menu (E1-E4).
static mut EpisodeMenu: [menuitem_t; 4] = [
    mi(1, b"M_EPI1\0\0\0\0", Some(M_Episode), b'k'),
    mi(1, b"M_EPI2\0\0\0\0", Some(M_Episode), b't'),
    mi(1, b"M_EPI3\0\0\0\0", Some(M_Episode), b'i'),
    mi(1, b"M_EPI4\0\0\0\0", Some(M_Episode), b't'),
];

/// Item array for the skill-level selection menu.
static mut NewGameMenu: [menuitem_t; 5] = [
    mi(1, b"M_JKILL\0\0\0", Some(M_ChooseSkill), b'i'),
    mi(1, b"M_ROUGH\0\0\0", Some(M_ChooseSkill), b'h'),
    mi(1, b"M_HURT\0\0\0\0", Some(M_ChooseSkill), b'h'),
    mi(1, b"M_ULTRA\0\0\0", Some(M_ChooseSkill), b'u'),
    mi(1, b"M_NMARE\0\0\0", Some(M_ChooseSkill), b'n'),
];

/// Item array for the Options menu (end game, messages, detail, screen size, mouse sensitivity, sound).
static mut OptionsMenu: [menuitem_t; 8] = [
    mi(1, b"M_ENDGAM\0\0", Some(M_EndGame), b'e'),
    mi(1, b"M_MESSG\0\0\0", Some(M_ChangeMessages), b'm'),
    mi(1, b"M_DETAIL\0\0", Some(M_ChangeDetail), b'g'),
    mi(2, b"M_SCRNSZ\0\0", Some(M_SizeDisplay), b's'),
    mi(-1, b"", None, 0),
    mi(2, b"M_MSENS\0\0\0", Some(M_ChangeSensitivity), b'm'),
    mi(-1, b"", None, 0),
    mi(1, b"M_SVOL\0\0\0\0", Some(M_Sound), b's'),
];

/// Single-item array for the first "Read This" help page (advances to page 2).
static mut ReadMenu1: [menuitem_t; 1] = [mi(1, b"", Some(M_ReadThis2), 0)];

/// Single-item array for the second "Read This" help page (returns to main menu).
static mut ReadMenu2: [menuitem_t; 1] = [mi(1, b"", Some(M_FinishReadThis), 0)];

/// Item array for the Sound Volume menu (SFX slider, music slider).
static mut SoundMenu: [menuitem_t; 4] = [
    mi(2, b"M_SFXVOL\0\0", Some(M_SfxVol), b's'),
    mi(-1, b"", None, 0),
    mi(2, b"M_MUSVOL\0\0", Some(M_MusicVol), b'm'),
    mi(-1, b"", None, 0),
];

/// Item array for the Load Game menu (6 save slots).
static mut LoadMenu: [menuitem_t; 6] = [
    mi(1, b"", Some(M_LoadSelect), b'1'),
    mi(1, b"", Some(M_LoadSelect), b'2'),
    mi(1, b"", Some(M_LoadSelect), b'3'),
    mi(1, b"", Some(M_LoadSelect), b'4'),
    mi(1, b"", Some(M_LoadSelect), b'5'),
    mi(1, b"", Some(M_LoadSelect), b'6'),
];

/// Item array for the Save Game menu (6 save slots).
static mut SaveMenu: [menuitem_t; 6] = [
    mi(1, b"", Some(M_SaveSelect), b'1'),
    mi(1, b"", Some(M_SaveSelect), b'2'),
    mi(1, b"", Some(M_SaveSelect), b'3'),
    mi(1, b"", Some(M_SaveSelect), b'4'),
    mi(1, b"", Some(M_SaveSelect), b'5'),
    mi(1, b"", Some(M_SaveSelect), b'6'),
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
            let name_ptr = P_SaveGameFile(i as c_int);
            let mut name: [c_char; 256] = [0; 256];
            M_StringCopy(name.as_mut_ptr(), name_ptr, name.len());

            let handle = fopen(name.as_ptr() as *const c_char, c"rb".as_ptr());
            if handle.is_null() {
                M_StringCopy(
                    savegamestrings[i].as_mut_ptr(),
                    c"empty slot".as_ptr(),
                    SAVESTRINGSIZE,
                );
                LoadMenu[i].status = 0;
                continue;
            }
            fread(
                savegamestrings[i].as_mut_ptr() as *mut c_void,
                1,
                SAVESTRINGSIZE,
                handle,
            );
            fclose(handle);
            LoadMenu[i].status = 1;
        }
    }
}

/// Draw the Load Game menu page: title patch plus one bordered slot row per save slot.
extern "C" fn M_DrawLoad() {
    unsafe {
        V_DrawPatchDirect(
            72,
            28,
            W_CacheLumpName(c"M_LOADG".as_ptr(), 0) as *mut patch_t,
        );

        for i in 0..load_end {
            M_DrawSaveLoadBorder(
                LoadDef.x as c_int,
                LoadDef.y as c_int + LINEHEIGHT * i as c_int,
            );
            M_WriteText(
                LoadDef.x as c_int,
                LoadDef.y as c_int + LINEHEIGHT * i as c_int,
                savegamestrings[i].as_mut_ptr(),
            );
        }
    }
}

/// Draw the left/center/right border patches around a save-game name text field at `(x, y)`.
fn M_DrawSaveLoadBorder(x: c_int, y: c_int) {
    V_DrawPatchDirect(
        x - 8,
        y + 7,
        W_CacheLumpName(c"M_LSLEFT".as_ptr(), 0) as *mut patch_t,
    );

    let mut xi = x;
    for _ in 0..24 {
        V_DrawPatchDirect(
            xi,
            y + 7,
            W_CacheLumpName(c"M_LSCNTR".as_ptr(), 0) as *mut patch_t,
        );
        xi += 8;
    }

    V_DrawPatchDirect(
        xi,
        y + 7,
        W_CacheLumpName(c"M_LSRGHT".as_ptr(), 0) as *mut patch_t,
    );
}

/// Load the game from save slot `choice` and close all menus.
extern "C" fn M_LoadSelect(choice: c_int) {
    unsafe {
        let mut name: [c_char; 256] = [0; 256];
        M_StringCopy(name.as_mut_ptr(), P_SaveGameFile(choice), name.len());
        G_LoadGame(name.as_mut_ptr());
        M_ClearMenus();
    }
}

/// Navigate to the Load Game menu page (suppressed during network games).
extern "C" fn M_LoadGame(_choice: c_int) {
    unsafe {
        if netgame != 0 {
            M_StartMessage(
                c"you can't do load while in a net game!\n\npress a key."
                    .as_ptr()
                    .cast_mut(),
                None,
                0,
            );
            return;
        }
        M_SetupNextMenu(&raw mut LoadDef);
    }
    M_ReadSaveStrings();
}

/// Draw the Save Game menu page: title, bordered slot rows, and a blinking cursor when editing.
extern "C" fn M_DrawSave() {
    unsafe {
        V_DrawPatchDirect(
            72,
            28,
            W_CacheLumpName(c"M_SAVEG".as_ptr(), 0) as *mut patch_t,
        );
        for i in 0..load_end {
            M_DrawSaveLoadBorder(
                LoadDef.x as c_int,
                LoadDef.y as c_int + LINEHEIGHT * i as c_int,
            );
            M_WriteText(
                LoadDef.x as c_int,
                LoadDef.y as c_int + LINEHEIGHT * i as c_int,
                savegamestrings[i].as_mut_ptr(),
            );
        }

        if saveStringEnter != 0 {
            let w = M_StringWidth(savegamestrings[saveSlot as usize].as_mut_ptr());
            M_WriteText(
                LoadDef.x as c_int + w,
                LoadDef.y as c_int + LINEHEIGHT * saveSlot as c_int,
                c"_".as_ptr().cast_mut(),
            );
        }
    }
}

/// Commit the save to `slot`, clear the menus, and record the slot as the quicksave target.
fn M_DoSave(slot: c_int) {
    unsafe {
        G_SaveGame(slot, savegamestrings[slot as usize].as_ptr());
        M_ClearMenus();
        if quickSaveSlot == -2 {
            quickSaveSlot = slot;
        }
    }
}

/// Enter string-editing mode for save slot `choice`, preserving the old description.
extern "C" fn M_SaveSelect(choice: c_int) {
    unsafe {
        saveStringEnter = 1;
        saveSlot = choice;
        M_StringCopy(
            std::ptr::addr_of_mut!(saveOldString[0]),
            savegamestrings[choice as usize].as_ptr(),
            SAVESTRINGSIZE,
        );
        if strcmp(
            savegamestrings[choice as usize].as_ptr(),
            c"empty slot".as_ptr(),
        ) == 0
        {
            savegamestrings[choice as usize][0] = 0;
        }
        saveCharIndex = strlen(savegamestrings[choice as usize].as_ptr()) as c_int;
    }
}

/// Navigate to the Save Game menu page (suppressed when not in-game or not at `GS_LEVEL`).
extern "C" fn M_SaveGame(_choice: c_int) {
    const GS_LEVEL: c_int = 0;
    unsafe {
        if usergame == 0 {
            M_StartMessage(
                c"you can't save if you aren't playing!\n\npress a key."
                    .as_ptr()
                    .cast_mut(),
                None,
                0,
            );
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
    const GS_LEVEL: c_int = 0;
    unsafe {
        if usergame == 0 {
            S_StartSound(ptr::null_mut(), Sfx::Oof as c_int);
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
        static mut QUICK_SAVE_MSG: [c_char; 80] = [0; 80];
        let slot_str = std::ffi::CStr::from_ptr(savegamestrings[quickSaveSlot as usize].as_ptr())
            .to_string_lossy();
        c_write!(
            QUICK_SAVE_MSG,
            "quicksave over your game named\n\n'{}'?\n\npress y or n.",
            slot_str
        );
        M_StartMessage(
            std::ptr::addr_of_mut!(QUICK_SAVE_MSG[0]),
            Some(M_QuickSaveResponse),
            1,
        );
    }
}

/// Confirmation callback for the quicksave y/n prompt; saves if `key` is the confirm key.
extern "C" fn M_QuickSaveResponse(key: c_int) {
    use super::m_controls::key_menu_confirm;
    unsafe {
        if key == key_menu_confirm {
            M_DoSave(quickSaveSlot);
            S_StartSound(ptr::null_mut(), Sfx::Swtchx as c_int);
        }
    }
}

/// Perform a quickload from the previously chosen slot, or show an error if none was chosen.
///
/// Suppressed during network games. Shows a y/n confirmation before loading.
fn M_QuickLoad() {
    unsafe {
        if netgame != 0 {
            M_StartMessage(
                c"you can't quickload during a netgame!\n\npress a key."
                    .as_ptr()
                    .cast_mut(),
                None,
                0,
            );
            return;
        }
        if quickSaveSlot < 0 {
            M_StartMessage(
                c"you haven't picked a quicksave slot yet!\n\npress a key.".as_ptr() as *mut c_char,
                None,
                0,
            );
            return;
        }
        static mut QUICK_LOAD_MSG: [c_char; 80] = [0; 80];
        let slot_str = std::ffi::CStr::from_ptr(savegamestrings[quickSaveSlot as usize].as_ptr())
            .to_string_lossy();
        c_write!(
            QUICK_LOAD_MSG,
            "do you want to quickload the game named\n\n'{}'?\n\npress y or n.",
            slot_str
        );
        M_StartMessage(
            std::ptr::addr_of_mut!(QUICK_LOAD_MSG[0]),
            Some(M_QuickLoadResponse),
            1,
        );
    }
}

/// Confirmation callback for the quickload y/n prompt; loads if `key` is the confirm key.
extern "C" fn M_QuickLoadResponse(key: c_int) {
    use super::m_controls::key_menu_confirm;
    unsafe {
        if key == key_menu_confirm {
            M_LoadSelect(quickSaveSlot);
            S_StartSound(ptr::null_mut(), Sfx::Swtchx as c_int);
        }
    }
}

/// Draw the first "Read This" help screen and position the skull cursor.
///
/// Selects the correct WAD lump (`HELP`, `HELP1`, or `HELP2`) and skull
/// position based on `gameversion` and `gamemode`.
extern "C" fn M_DrawReadThis1() {
    unsafe {
        inhelpscreens = 1;

        let lumpname: *const c_char;
        let skullx: i16;
        let skully: i16;

        match gameversion {
            d_mode::exe_doom_1_666
            | d_mode::exe_doom_1_7
            | d_mode::exe_doom_1_8
            | d_mode::exe_doom_1_9
            | d_mode::exe_hacx => {
                if gamemode == d_mode::commercial {
                    lumpname = c"HELP".as_ptr();
                    skullx = 330;
                    skully = 165;
                } else {
                    lumpname = c"HELP2".as_ptr();
                    skullx = 280;
                    skully = 185;
                }
            }
            d_mode::exe_ultimate | d_mode::exe_chex => {
                lumpname = c"HELP1".as_ptr();
                skullx = 280;
                skully = 185;
            }
            d_mode::exe_final | d_mode::exe_final2 => {
                lumpname = c"HELP".as_ptr();
                skullx = 330;
                skully = 165;
            }
            _ => {
                i_error!("Unhandled game version");
                return;
            }
        }

        V_DrawPatchDirect(0, 0, W_CacheLumpName(lumpname, 0) as *mut patch_t);
        ReadDef1.x = skullx;
        ReadDef1.y = skully;
    }
}

/// Draw the second "Read This" help screen (`HELP1` lump).
extern "C" fn M_DrawReadThis2() {
    unsafe {
        inhelpscreens = 1;
        V_DrawPatchDirect(0, 0, W_CacheLumpName(c"HELP1".as_ptr(), 0) as *mut patch_t);
    }
}

/// Draw the Sound Volume menu page: title patch and two thermometer sliders.
extern "C" fn M_DrawSound() {
    use super::s_sound::{musicVolume, sfxVolume};
    unsafe {
        V_DrawPatchDirect(
            60,
            38,
            W_CacheLumpName(c"M_SVOL".as_ptr(), 0) as *mut patch_t,
        );

        M_DrawThermo(
            SoundDef.x as c_int,
            SoundDef.y as c_int + LINEHEIGHT * (sfx_vol as c_int + 1),
            16,
            sfxVolume,
        );

        M_DrawThermo(
            SoundDef.x as c_int,
            SoundDef.y as c_int + LINEHEIGHT * (music_vol as c_int + 1),
            16,
            musicVolume,
        );
    }
}

/// No-op activation callback for the Sound Volume menu item (navigation handled elsewhere).
extern "C" fn M_Sound(_choice: c_int) {}

/// Adjust the SFX volume slider; `choice` 0 decrements, 1 increments (range 0-15).
extern "C" fn M_SfxVol(choice: c_int) {
    use super::s_sound::sfxVolume;
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
extern "C" fn M_MusicVol(choice: c_int) {
    use super::s_sound::musicVolume;
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
extern "C" fn M_DrawMainMenu() {
    V_DrawPatchDirect(
        94,
        2,
        W_CacheLumpName(c"M_DOOM".as_ptr(), 0) as *mut patch_t,
    );
}

/// Draw the "New Game" and "Skill Level" title patches above the skill menu.
extern "C" fn M_DrawNewGame() {
    V_DrawPatchDirect(
        96,
        14,
        W_CacheLumpName(c"M_NEWG".as_ptr(), 0) as *mut patch_t,
    );
    V_DrawPatchDirect(
        54,
        38,
        W_CacheLumpName(c"M_SKILL".as_ptr(), 0) as *mut patch_t,
    );
}

/// Handle "New Game" activation: navigate to the episode or skill menu as appropriate.
///
/// Shows an error message if in a network game (except demo playback).
/// Skips the episode menu for Doom II (commercial) and Chex Quest.
extern "C" fn M_NewGame(_choice: c_int) {
    unsafe {
        if netgame != 0 && demoplayback == 0 {
            M_StartMessage(
                c"you can't start a new game\nwhile in a network game.\n\npress a key.".as_ptr()
                    as *mut c_char,
                None,
                0,
            );
            return;
        }
        if gamemode == d_mode::commercial || gameversion == d_mode::exe_chex {
            M_SetupNextMenu(&raw mut NewDef);
        } else {
            M_SetupNextMenu(&raw mut EpiDef);
        }
    }
}

/// Draw the "Which Episode?" title patch above the episode menu.
extern "C" fn M_DrawEpisode() {
    V_DrawPatchDirect(
        54,
        38,
        W_CacheLumpName(c"M_EPISOD".as_ptr(), 0) as *mut patch_t,
    );
}

/// Selected episode index (0-based), set by `M_Episode` and consumed by `M_ChooseSkill`.
static mut epi: c_int = 0;

/// Confirmation callback for the Nightmare skill prompt; starts the game if `key` confirms.
extern "C" fn M_VerifyNightmare(key: c_int) {
    use super::m_controls::key_menu_confirm;
    unsafe {
        if key != key_menu_confirm {
            return;
        }
        G_DeferedInitNew(4, epi + 1, 1);
        M_ClearMenus();
    }
}

/// Start a new game at the given skill level, or prompt for confirmation on Nightmare.
extern "C" fn M_ChooseSkill(choice: c_int) {
    if choice as usize == nightmare {
        M_StartMessage(
            c"are you sure? this skill level\nisn't even remotely fair.\n\npress y or n.".as_ptr()
                as *mut c_char,
            Some(M_VerifyNightmare),
            1,
        );
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
extern "C" fn M_Episode(choice: c_int) {
    unsafe {
        if gamemode == d_mode::shareware && choice != 0 {
            M_StartMessage(
                c"this is the shareware version of doom.\n\nyou need to order the entire trilogy.\n\npress a key.".as_ptr()
                    as *mut c_char,
                None,
                0,
            );
            M_SetupNextMenu(&raw mut ReadDef1);
            return;
        }
        if gamemode == d_mode::registered && choice > 2 {
            epi = 0;
        } else {
            epi = choice;
        }
        M_SetupNextMenu(&raw mut NewDef);
    }
}

/// Draw the Options menu page: title patch, detail/message toggles, and thermometer sliders.
extern "C" fn M_DrawOptions() {
    unsafe {
        V_DrawPatchDirect(
            108,
            15,
            W_CacheLumpName(c"M_OPTTTL".as_ptr(), 0) as *mut patch_t,
        );

        let detail_names: [*const c_char; 2] = [c"M_GDHIGH".as_ptr(), c"M_GDLOW".as_ptr()];
        let msg_names: [*const c_char; 2] = [c"M_MSGOFF".as_ptr(), c"M_MSGON".as_ptr()];

        V_DrawPatchDirect(
            OptionsDef.x as c_int + 175,
            OptionsDef.y as c_int + LINEHEIGHT * detail as c_int,
            W_CacheLumpName(detail_names[detailLevel as usize], 0) as *mut patch_t,
        );

        V_DrawPatchDirect(
            OptionsDef.x as c_int + 120,
            OptionsDef.y as c_int + LINEHEIGHT * messages as c_int,
            W_CacheLumpName(msg_names[showMessages as usize], 0) as *mut patch_t,
        );

        M_DrawThermo(
            OptionsDef.x as c_int,
            OptionsDef.y as c_int + LINEHEIGHT * (mousesens as c_int + 1),
            10,
            mouseSensitivity,
        );

        M_DrawThermo(
            OptionsDef.x as c_int,
            OptionsDef.y as c_int + LINEHEIGHT * (scrnsize as c_int + 1),
            9,
            screenSize,
        );
    }
}

/// No-op activation callback for the Options menu item (navigation handled by `M_Responder`).
extern "C" fn M_Options(_choice: c_int) {}

/// Toggle the `showMessages` setting and display a confirmation HUD message.
extern "C" fn M_ChangeMessages(_choice: c_int) {
    unsafe {
        showMessages = 1 - showMessages;
        if showMessages == 0 {
            M_Menu_SetPlayerMessage(c"Messages OFF".as_ptr());
        } else {
            M_Menu_SetPlayerMessage(c"Messages ON".as_ptr());
        }
        message_dontfuckwithme = 1;
    }
}

/// Confirmation callback for "End Game"; clears menus if the player confirms.
extern "C" fn M_EndGameResponse(key: c_int) {
    use super::m_controls::key_menu_confirm;
    unsafe {
        if key != key_menu_confirm {
            return;
        }
        (*currentMenu).lastOn = itemOn;
    }
    M_ClearMenus();
}

/// Prompt the player to confirm ending the current game (suppressed if not in-game or in a network game).
extern "C" fn M_EndGame(_choice: c_int) {
    unsafe {
        if usergame == 0 {
            S_StartSound(ptr::null_mut(), Sfx::Oof as c_int);
            return;
        }
        if netgame != 0 {
            M_StartMessage(
                c"you can't end a netgame!\n\npress a key."
                    .as_ptr()
                    .cast_mut(),
                None,
                0,
            );
            return;
        }
        M_StartMessage(
            c"are you sure you want to end the game?\n\npress y or n."
                .as_ptr()
                .cast_mut(),
            Some(M_EndGameResponse),
            1,
        );
    }
}

/// No-op placeholder for the "Read This" main-menu item (navigation is in `M_Responder`).
extern "C" fn M_ReadThis(_choice: c_int) {}

/// Handle "done" from the first help screen: advance to page 2 or finish, depending on game version.
extern "C" fn M_ReadThis2(_choice: c_int) {
    unsafe {
        if gameversion <= d_mode::exe_doom_1_9 && gamemode != d_mode::commercial {
            M_SetupNextMenu(&raw mut ReadDef2);
        } else {
            M_FinishReadThis(0);
        }
    }
}

/// No-op: closing the last help screen returns to the previous menu via `M_Responder`'s back key.
extern "C" fn M_FinishReadThis(_choice: c_int) {}

/// Sound effects played on quit confirmation for Doom episode games (cycled by `gametic`).
static mut quitsounds: [c_int; 8] = [
    Sfx::Pldeth as c_int,
    Sfx::Dmpain as c_int,
    Sfx::Popain as c_int,
    Sfx::Slop as c_int,
    Sfx::Telept as c_int,
    Sfx::Posit1 as c_int,
    Sfx::Posit3 as c_int,
    Sfx::Sgtatk as c_int,
];
/// Sound effects played on quit confirmation for Doom II (commercial) builds (cycled by `gametic`).
static mut quitsounds2: [c_int; 8] = [
    Sfx::Vilact as c_int,
    Sfx::Getpow as c_int,
    Sfx::Boscub as c_int,
    Sfx::Slop as c_int,
    Sfx::Skeswg as c_int,
    Sfx::Kntdth as c_int,
    Sfx::Bspact as c_int,
    Sfx::Sgtatk as c_int,
];

/// Confirmation callback for the quit prompt; plays a quit sound and exits if the player confirms.
extern "C" fn M_QuitResponse(key: c_int) {
    use super::m_controls::key_menu_confirm;
    unsafe {
        if key != key_menu_confirm {
            return;
        }
        if netgame == 0 {
            if gamemode == d_mode::commercial {
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
fn M_SelectEndMessage() -> *const c_char {
    unsafe {
        if logical_gamemission() == d_mode::doom {
            doom1_endmsg[(gametic as usize) & 7].0
        } else {
            doom2_endmsg[(gametic as usize) & 7].0
        }
    }
}

/// Show the quit confirmation dialog with a game-specific quip and a y/n prompt.
extern "C" fn M_QuitDOOM(_choice: c_int) {
    unsafe {
        let msg = M_SelectEndMessage();
        let msg_str = std::ffi::CStr::from_ptr(msg).to_string_lossy();
        c_write!(endstring, "{}\n\n(press y to quit to dos.)", msg_str);
        M_StartMessage(
            std::ptr::addr_of_mut!(endstring[0]),
            Some(M_QuitResponse),
            1,
        );
    }
}

/// Adjust mouse sensitivity; `choice` 0 decrements, 1 increments (range 0-9).
extern "C" fn M_ChangeSensitivity(choice: c_int) {
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
extern "C" fn M_ChangeDetail(_choice: c_int) {
    unsafe {
        detailLevel = 1 - detailLevel;
        R_SetViewSize(screenblocks, detailLevel);
        if detailLevel == 0 {
            M_Menu_SetPlayerMessage(c"High detail".as_ptr());
        } else {
            M_Menu_SetPlayerMessage(c"Low detail".as_ptr());
        }
    }
}

/// Adjust the screen-size slider; `choice` 0 shrinks, 1 enlarges (range 0-8).
///
/// Updates both `screenSize` and `screenblocks`, then calls `R_SetViewSize`.
extern "C" fn M_SizeDisplay(choice: c_int) {
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
fn M_DrawThermo(x: c_int, y: c_int, thermWidth: c_int, thermDot: c_int) {
    let mut xx = x;
    V_DrawPatchDirect(
        xx,
        y,
        W_CacheLumpName(c"M_THERML".as_ptr(), 0) as *mut patch_t,
    );
    xx += 8;
    for _ in 0..thermWidth {
        V_DrawPatchDirect(
            xx,
            y,
            W_CacheLumpName(c"M_THERMM".as_ptr(), 0) as *mut patch_t,
        );
        xx += 8;
    }
    V_DrawPatchDirect(
        xx,
        y,
        W_CacheLumpName(c"M_THERMR".as_ptr(), 0) as *mut patch_t,
    );

    V_DrawPatchDirect(
        (x + 8) + thermDot * 8,
        y,
        W_CacheLumpName(c"M_THERMO".as_ptr(), 0) as *mut patch_t,
    );
}

/// No-op: draw callback for an empty (non-selected) grid cell (unused in this port).
fn M_DrawEmptyCell(_menu: *mut menu_t, _item: c_int) {}
/// No-op: draw callback for the selected grid cell (unused in this port).
fn M_DrawSelCell(_menu: *mut menu_t, _item: c_int) {}

/// Display a modal overlay message.
///
/// `string` is the message to display.
/// `routine` is an optional callback invoked with the key pressed to dismiss.
/// `input` is non-zero when a specific key (y/n or confirm/abort) is required.
fn M_StartMessage(string: *mut c_char, routine: Option<extern "C" fn(c_int)>, input: c_int) {
    unsafe {
        messageLastMenuActive = menuactive;
        messageToPrint = 1;
        messageString = string;
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
fn M_StringWidth(string: *mut c_char) -> c_int {
    unsafe {
        let len = strlen(string);
        let mut w: c_int = 0;
        for i in 0..len {
            let c = toupper(*string.add(i) as c_int) - HU_FONTSTART;
            if c < 0 || c as usize >= HU_FONTSIZE {
                w += 4;
            } else {
                let patch = hu_font[c as usize] as *const patch_stub;
                w += (*patch).width as c_int;
            }
        }
        w
    }
}

/// Return the pixel height of `string` rendered in the HUD font, accounting for newlines.
fn M_StringHeight(string: *mut c_char) -> c_int {
    unsafe {
        let patch = hu_font[0] as *const patch_stub;
        let height = (*patch).height as c_int;
        let mut h = height;
        let len = strlen(string);
        for i in 0..len {
            if *string.add(i) == b'\n' as c_char {
                h += height;
            }
        }
        h
    }
}

/// Render `string` using the HUD font at screen position `(x, y)`.
///
/// Newline characters reset the x cursor and advance y by 12 pixels.
/// Characters outside the font range are rendered as 4-pixel spaces.
/// Rendering stops at the screen right edge.
fn M_WriteText(x: c_int, y: c_int, string: *mut c_char) {
    unsafe {
        let mut ch_ptr = string;
        let mut cx = x;
        let mut cy = y;

        loop {
            let c = *ch_ptr;
            if c == 0 {
                break;
            }
            ch_ptr = ch_ptr.add(1);
            if c == b'\n' as c_char {
                cx = x;
                cy += 12;
                continue;
            }

            let c = toupper(c as c_int) - HU_FONTSTART;
            if c < 0 || c as usize >= HU_FONTSIZE {
                cx += 4;
                continue;
            }

            let patch = hu_font[c as usize] as *const patch_stub;
            let w = (*patch).width as c_int;
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
fn IsNullKey(key: c_int) -> bool {
    key == KEY_PAUSE || key == KEY_CAPSLOCK || key == KEY_SCRLCK || key == KEY_NUMLOCK
}

/// Tic time after which the next joystick event will be processed (rate-limiting).
static mut RESP_joywait: c_int = 0;
/// Tic time after which the next mouse event will be processed (rate-limiting).
static mut RESP_mousewait: c_int = 0;
/// Accumulated mouse y movement since the last `RESP_lasty` reset.
static mut RESP_mousey: c_int = 0;
/// Mouse y baseline; updated in 30-unit steps to produce discrete menu scrolls.
static mut RESP_lasty: c_int = 0;
/// Accumulated mouse x movement since the last `RESP_lastx` reset.
static mut RESP_mousex: c_int = 0;
/// Mouse x baseline; updated in 30-unit steps to produce discrete menu navigation.
static mut RESP_lastx: c_int = 0;

/// Handle an input event for the menu system.
///
/// Translates joystick, mouse, and keyboard events into menu navigation actions
/// (up/down/left/right/forward/back/activate/abort) and F-key shortcuts
/// (quicksave, quickload, screen-size, gamma, screenshot, etc.).
/// Also handles save-game string editing character-by-character.
///
/// Returns `Boolean::TRUE` if the event was consumed, `Boolean::FALSE` otherwise.
/// Called by `G_Responder` in `g_game.c`.
#[no_mangle]
pub extern "C" fn M_Responder(ev: *mut event_t) -> Boolean {
    use super::m_controls::{
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
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as c_int);
                M_QuitDOOM(0);
            }
            return Boolean::TRUE;
        }

        let mut ch: c_int = 0;
        let mut key: c_int = -1;

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
                    savegamestrings[saveSlot as usize].as_mut_ptr(),
                    std::ptr::addr_of!(saveOldString[0]),
                    SAVESTRINGSIZE,
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
                ch = toupper(ch);

                if ch != b' ' as c_int
                    && (ch - HU_FONTSTART < 0 || ch - HU_FONTSTART >= HU_FONTSIZE as c_int)
                {
                    return Boolean::TRUE;
                }

                if (32..=127).contains(&ch)
                    && saveCharIndex < (SAVESTRINGSIZE as c_int - 1)
                    && M_StringWidth(savegamestrings[saveSlot as usize].as_mut_ptr())
                        < (SAVESTRINGSIZE as c_int - 2) * 8
                {
                    savegamestrings[saveSlot as usize][saveCharIndex as usize] = ch as c_char;
                    saveCharIndex += 1;
                    savegamestrings[saveSlot as usize][saveCharIndex as usize] = 0;
                }
            }
            return Boolean::TRUE;
        }

        // Messages that need input
        if messageToPrint != 0 {
            if messageNeedsInput != 0
                && key != b' ' as c_int
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
            S_StartSound(ptr::null_mut(), Sfx::Swtchx as c_int);
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
                S_StartSound(ptr::null_mut(), Sfx::Stnmov as c_int);
                return Boolean::TRUE;
            } else if key == key_menu_incscreen {
                if automapactive != 0 || chat_on != 0 {
                    return Boolean::FALSE;
                }
                M_SizeDisplay(1);
                S_StartSound(ptr::null_mut(), Sfx::Stnmov as c_int);
                return Boolean::TRUE;
            } else if key == key_menu_help {
                M_StartControlPanel();
                if gamemode == d_mode::retail {
                    currentMenu = &raw mut ReadDef2;
                } else {
                    currentMenu = &raw mut ReadDef1;
                }
                itemOn = 0;
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as c_int);
                return Boolean::TRUE;
            } else if key == key_menu_save {
                M_StartControlPanel();
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as c_int);
                M_SaveGame(0);
                return Boolean::TRUE;
            } else if key == key_menu_load {
                M_StartControlPanel();
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as c_int);
                M_LoadGame(0);
                return Boolean::TRUE;
            } else if key == key_menu_volume {
                M_StartControlPanel();
                currentMenu = &raw mut SoundDef;
                itemOn = sfx_vol as i16;
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as c_int);
                return Boolean::TRUE;
            } else if key == key_menu_detail {
                M_ChangeDetail(0);
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as c_int);
                return Boolean::TRUE;
            } else if key == key_menu_qsave {
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as c_int);
                M_QuickSave();
                return Boolean::TRUE;
            } else if key == key_menu_endgame {
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as c_int);
                M_EndGame(0);
                return Boolean::TRUE;
            } else if key == key_menu_messages {
                M_ChangeMessages(0);
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as c_int);
                return Boolean::TRUE;
            } else if key == key_menu_qload {
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as c_int);
                M_QuickLoad();
                return Boolean::TRUE;
            } else if key == key_menu_quit {
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as c_int);
                M_QuitDOOM(0);
                return Boolean::TRUE;
            } else if key == key_menu_gamma {
                usegamma += 1;
                if usegamma > 4 {
                    usegamma = 0;
                }
                M_Menu_SetPlayerMessage(gammamsg[usegamma as usize].as_ptr());
                I_SetPalette(W_CacheLumpName(c"PLAYPAL".as_ptr(), 0) as *mut u8);
                return Boolean::TRUE;
            }
        }

        // Pop-up menu?
        if menuactive == 0 {
            if key == key_menu_activate {
                M_StartControlPanel();
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as c_int);
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
                S_StartSound(ptr::null_mut(), Sfx::Pstop as c_int);
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
                S_StartSound(ptr::null_mut(), Sfx::Pstop as c_int);
                if (*(*currentMenu).menuitems.offset(itemOn as isize)).status != -1 {
                    break;
                }
            }
            return Boolean::TRUE;
        } else if key == key_menu_left {
            let item = &*(*currentMenu).menuitems.offset(itemOn as isize);
            if item.status == 2 {
                if let Some(routine) = item.routine {
                    S_StartSound(ptr::null_mut(), Sfx::Stnmov as c_int);
                    routine(0);
                }
            }
            return Boolean::TRUE;
        } else if key == key_menu_right {
            let item = &*(*currentMenu).menuitems.offset(itemOn as isize);
            if item.status == 2 {
                if let Some(routine) = item.routine {
                    S_StartSound(ptr::null_mut(), Sfx::Stnmov as c_int);
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
                        S_StartSound(ptr::null_mut(), Sfx::Stnmov as c_int);
                    } else {
                        routine(itemOn as c_int);
                        S_StartSound(ptr::null_mut(), Sfx::Pistol as c_int);
                    }
                }
            }
            return Boolean::TRUE;
        } else if key == key_menu_activate {
            (*currentMenu).lastOn = itemOn;
            M_ClearMenus();
            S_StartSound(ptr::null_mut(), Sfx::Swtchx as c_int);
            return Boolean::TRUE;
        } else if key == key_menu_back {
            (*currentMenu).lastOn = itemOn;
            if !(*currentMenu).prevMenu.is_null() {
                currentMenu = (*currentMenu).prevMenu;
                itemOn = (*currentMenu).lastOn;
                S_StartSound(ptr::null_mut(), Sfx::Swtchn as c_int);
            }
            return Boolean::TRUE;
        }

        // Keyboard shortcut
        if ch != 0 || IsNullKey(key) {
            let ch_u = ch as c_char;
            for i in (itemOn + 1)..(*currentMenu).numitems {
                if (*(*currentMenu).menuitems.offset(i as isize)).alphaKey == ch_u {
                    itemOn = i;
                    S_StartSound(ptr::null_mut(), Sfx::Pstop as c_int);
                    return Boolean::TRUE;
                }
            }
            for i in 0..=itemOn {
                if (*(*currentMenu).menuitems.offset(i as isize)).alphaKey == ch_u {
                    itemOn = i;
                    S_StartSound(ptr::null_mut(), Sfx::Pstop as c_int);
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
#[no_mangle]
pub extern "C" fn M_StartControlPanel() {
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
#[no_mangle]
pub extern "C" fn M_Drawer() {
    unsafe {
        inhelpscreens = 0;

        if messageToPrint != 0 {
            let mut start: usize = 0;
            messy = SCREENHEIGHT / 2 - M_StringHeight(messageString) / 2;
            while *messageString.add(start) != 0 {
                let mut foundnewline = 0;
                let mut string: [c_char; 80] = [0; 80];
                let remaining = strlen(messageString.add(start));
                for i in 0..remaining {
                    if *messageString.add(start + i) == b'\n' as c_char {
                        M_StringCopy(string.as_mut_ptr(), messageString.add(start), string.len());
                        if i < string.len() {
                            string[i] = 0;
                        }
                        foundnewline = 1;
                        start += i + 1;
                        break;
                    }
                }
                if foundnewline == 0 {
                    M_StringCopy(string.as_mut_ptr(), messageString.add(start), string.len());
                    start += strlen(string.as_ptr());
                }

                messx = SCREENWIDTH / 2 - M_StringWidth(string.as_mut_ptr()) / 2;
                M_WriteText(messx, messy, string.as_mut_ptr());
                let patch = hu_font[0] as *const patch_stub;
                messy += (*patch).height as c_int;
            }
            return;
        }

        if menuactive == 0 {
            return;
        }

        if let Some(r) = (*currentMenu).routine {
            r();
        }

        let x = (*currentMenu).x as c_int;
        let y = (*currentMenu).y as c_int;
        let max = (*currentMenu).numitems as usize;

        for i in 0..max {
            let name = (*(*currentMenu).menuitems.add(i)).name.as_ptr();
            if *name != 0 {
                V_DrawPatchDirect(
                    x,
                    y + LINEHEIGHT * i as c_int,
                    W_CacheLumpName(name, 0) as *mut patch_t,
                );
            }
        }

        V_DrawPatchDirect(
            x + SKULLXOFF,
            (*currentMenu).y as c_int - 5 + itemOn as c_int * LINEHEIGHT,
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
#[no_mangle]
pub extern "C" fn M_Ticker() {
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
#[no_mangle]
pub extern "C" fn M_Init() {
    unsafe {
        currentMenu = &raw mut MainDef;
        menuactive = 0;
        itemOn = (*currentMenu).lastOn;
        whichSkull = 0;
        skullAnimCounter = 10;
        screenSize = screenblocks - 3;
        messageToPrint = 0;
        messageString = ptr::null_mut();
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
            d_mode::commercial => {
                ptr::write(&mut MainMenu[readthis], ptr::read(&MainMenu[quitdoom]));
                MainDef.numitems -= 1;
                MainDef.y += 8;
                NewDef.prevMenu = &raw mut MainDef;
            }
            d_mode::shareware | d_mode::registered | d_mode::retail => {}
            _ => {}
        }

        if gameversion < d_mode::exe_ultimate {
            EpiDef.numitems -= 1;
        }
    }
}
