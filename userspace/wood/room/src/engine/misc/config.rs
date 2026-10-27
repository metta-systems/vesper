//! Rust port of vendor/doomgeneric/m_config.c.
//!
//! Configuration-file interface: binds a static table of named variables to
//! locations in memory, parses values from text, and (in the original C)
//! loads / saves them to disk. The variable tables are organised into two
//! collections - `doom_defaults` (the vanilla `.cfg` settings) and
//! `extra_defaults` (chocolate-doom extensions).
//!
//! Notable port-level differences vs. the C source:
//! - The disk I/O code is guarded by `ORIGCODE` upstream and is also a
//!   no-op here; [`M_SaveDefaults`] and [`M_LoadDefaults`] still set up
//!   filenames but do not actually parse or write `.cfg` files.
//! - The Windows-only `GetDefaultConfigDir` resolution paths are skipped;
//!   `GetDefaultConfigDir` always returns `"."`.
//! - Variable definitions are encoded as a `const fn cfg(...)` helper in
//!   place of the C `CONFIG_VARIABLE_*` macros.
//! - A `DEFAULT_STRING` variable is bound to a `Cow<'static, str>`, so its
//!   default text stays a constant until a config value replaces it.
//! - The C file-scope variables, including both collections, live in
//!   [`ConfigState`], held in the [`CONFIG`] global. The variable tables
//!   start as copies of the `DOOM_DEFAULTS_LIST` / `EXTRA_DEFAULTS_LIST`
//!   constants.

#![allow(non_upper_case_globals, non_snake_case)]

use {
    crate::{engine::global::Global, i_error},
    alloc::{
        borrow::{Cow, ToOwned},
        format,
        string::{String, ToString},
    },
    core::{ffi::c_void, ptr},
};

/// Type tag for a configuration variable.
///
/// Mirrors the C `default_type_t` enum (`DEFAULT_INT`, `DEFAULT_INT_HEX`,
/// `DEFAULT_STRING`, `DEFAULT_FLOAT`, `DEFAULT_KEY`). Variants are
/// compared with `PartialEq` in the getters below.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DefaultType {
    /// Decimal integer (also accepts `0x` hex when read from disk).
    Int = 0,
    /// Hexadecimal integer (display form `0x...`).
    IntHex,
    /// Text, bound to a `Cow<'static, str>`.
    String,
    /// Single-precision float.
    Float,
    /// DOS scancode that maps through [`SCANTOKEY`] to an internal key code.
    Key,
}

/// Description of a single configuration variable.
///
/// Mirrors the C `default_t` struct, including the `untranslated` /
/// `original_translated` bookkeeping used to roundtrip DOS scancodes.
struct Default {
    /// Variable name as written in the `.cfg` file, e.g. `"key_fire"`.
    name: &'static str,
    /// Pointer to the in-memory location set by [`M_BindVariable`].
    /// NULL until binding.
    location: *mut c_void,
    /// Data type stored at `location`.
    ty: DefaultType,
    /// For `Key` variables: original DOS scancode read from the config
    /// file before translation through [`SCANTOKEY`]. Zero if never loaded.
    untranslated: i32,
    /// For `Key` variables: the translated internal key code at the time
    /// the value was loaded; used to detect user edits when saving.
    original_translated: i32,
    /// `true` once [`M_BindVariable`] has attached this entry to memory.
    bound: bool,
}

/// A named group of `Default` entries shared by a single config file.
///
/// Mirrors the C `default_collection_t` struct; the C `defaults` pointer
/// and `numdefaults` count become the `N`-entry array itself.
struct DefaultCollection<const N: usize> {
    /// The collection's variables.
    defaults: [Default; N],
    /// Filename used by [`M_LoadDefaults`] / [`M_SaveDefaults`]; empty
    /// until set.
    filename: String,
}

/// Construct a `Default` from a name and a type tag.
///
/// `const`-evaluated so the static variable tables below can be declared at
/// module level. Replaces the C `CONFIG_VARIABLE_*` macros.
const fn cfg(name: &'static str, ty: DefaultType) -> Default {
    Default {
        name,
        location: ptr::null_mut(),
        ty,
        untranslated: 0,
        original_translated: 0,
        bound: false,
    }
}

use crate::engine::misc::{
    M_MakeDirectory, M_StrToInt,
    argv::{M_CheckParmWithArgs, argument},
};

/// Configuration state (C `m_config.c` globals).
pub struct ConfigState {
    /// Global configuration directory (mirrors C `char *configdir`).
    ///
    /// Initialised by [`M_SetConfigDir`] from the command line or from
    /// `GetDefaultConfigDir`. Read by [`M_LoadDefaults`] and
    /// [`M_GetSaveGameDir`].
    configdir: String,
    /// Filename component for the main `.cfg` file (e.g. `"default.cfg"`).
    /// Set by [`M_SetConfigFilenames`].
    default_main_config: &'static str,
    /// Filename component for the extra (chocolate-doom) `.cfg` file. Set
    /// by [`M_SetConfigFilenames`].
    default_extra_config: &'static str,
    /// The vanilla config file's variables. Mirrors C
    /// `static default_collection_t doom_defaults`.
    doom_defaults: DefaultCollection<{ DOOM_DEFAULTS_LIST.len() }>,
    /// The extended chocolate-doom config file's variables. Mirrors C
    /// `static default_collection_t extra_defaults`.
    extra_defaults: DefaultCollection<{ EXTRA_DEFAULTS_LIST.len() }>,
}

impl ConfigState {
    /// No directory or filenames set, every variable unbound.
    pub const fn new() -> Self {
        Self {
            configdir: String::new(),
            default_main_config: "",
            default_extra_config: "",
            doom_defaults: DefaultCollection {
                defaults: DOOM_DEFAULTS_LIST,
                filename: String::new(),
            },
            extra_defaults: DefaultCollection {
                defaults: EXTRA_DEFAULTS_LIST,
                filename: String::new(),
            },
        }
    }
}

impl core::default::Default for ConfigState {
    fn default() -> Self {
        Self::new()
    }
}

/// The configuration state.
pub static CONFIG: Global<ConfigState> = Global::new(ConfigState::new());

/// Table backing the vanilla `doom_defaults` collection.
///
/// Order and names are kept verbatim from the C source so that on-disk
/// `.cfg` files remain compatible. Entries become live once
/// [`M_BindVariable`] attaches a memory location.
const DOOM_DEFAULTS_LIST: [Default; 76] = [
    cfg("mouse_sensitivity", DefaultType::Int),
    cfg("sfx_volume", DefaultType::Int),
    cfg("music_volume", DefaultType::Int),
    cfg("show_talk", DefaultType::Int),
    cfg("voice_volume", DefaultType::Int),
    cfg("show_messages", DefaultType::Int),
    cfg("key_right", DefaultType::Key),
    cfg("key_left", DefaultType::Key),
    cfg("key_up", DefaultType::Key),
    cfg("key_down", DefaultType::Key),
    cfg("key_strafeleft", DefaultType::Key),
    cfg("key_straferight", DefaultType::Key),
    cfg("key_useHealth", DefaultType::Key),
    cfg("key_jump", DefaultType::Key),
    cfg("key_flyup", DefaultType::Key),
    cfg("key_flydown", DefaultType::Key),
    cfg("key_flycenter", DefaultType::Key),
    cfg("key_lookup", DefaultType::Key),
    cfg("key_lookdown", DefaultType::Key),
    cfg("key_lookcenter", DefaultType::Key),
    cfg("key_invquery", DefaultType::Key),
    cfg("key_mission", DefaultType::Key),
    cfg("key_invPop", DefaultType::Key),
    cfg("key_invKey", DefaultType::Key),
    cfg("key_invHome", DefaultType::Key),
    cfg("key_invEnd", DefaultType::Key),
    cfg("key_invleft", DefaultType::Key),
    cfg("key_invright", DefaultType::Key),
    cfg("key_invLeft", DefaultType::Key),
    cfg("key_invRight", DefaultType::Key),
    cfg("key_useartifact", DefaultType::Key),
    cfg("key_invUse", DefaultType::Key),
    cfg("key_invDrop", DefaultType::Key),
    cfg("key_lookUp", DefaultType::Key),
    cfg("key_lookDown", DefaultType::Key),
    cfg("key_fire", DefaultType::Key),
    cfg("key_use", DefaultType::Key),
    cfg("key_strafe", DefaultType::Key),
    cfg("key_speed", DefaultType::Key),
    cfg("use_mouse", DefaultType::Int),
    cfg("mouseb_fire", DefaultType::Int),
    cfg("mouseb_strafe", DefaultType::Int),
    cfg("mouseb_forward", DefaultType::Int),
    cfg("mouseb_jump", DefaultType::Int),
    cfg("use_joystick", DefaultType::Int),
    cfg("joyb_fire", DefaultType::Int),
    cfg("joyb_strafe", DefaultType::Int),
    cfg("joyb_use", DefaultType::Int),
    cfg("joyb_speed", DefaultType::Int),
    cfg("joyb_jump", DefaultType::Int),
    cfg("screenblocks", DefaultType::Int),
    cfg("screensize", DefaultType::Int),
    cfg("detaillevel", DefaultType::Int),
    cfg("snd_channels", DefaultType::Int),
    cfg("snd_musicdevice", DefaultType::Int),
    cfg("snd_sfxdevice", DefaultType::Int),
    cfg("snd_sbport", DefaultType::Int),
    cfg("snd_sbirq", DefaultType::Int),
    cfg("snd_sbdma", DefaultType::Int),
    cfg("snd_mport", DefaultType::Int),
    cfg("usegamma", DefaultType::Int),
    cfg("savedir", DefaultType::String),
    cfg("messageson", DefaultType::Int),
    cfg("back_flat", DefaultType::String),
    cfg("nickname", DefaultType::String),
    cfg("chatmacro0", DefaultType::String),
    cfg("chatmacro1", DefaultType::String),
    cfg("chatmacro2", DefaultType::String),
    cfg("chatmacro3", DefaultType::String),
    cfg("chatmacro4", DefaultType::String),
    cfg("chatmacro5", DefaultType::String),
    cfg("chatmacro6", DefaultType::String),
    cfg("chatmacro7", DefaultType::String),
    cfg("chatmacro8", DefaultType::String),
    cfg("chatmacro9", DefaultType::String),
    cfg("comport", DefaultType::Int),
];

/// Table backing the chocolate-doom `extra_defaults` collection.
///
/// Holds the extended (non-vanilla) settings - video, joystick, mouse and
/// extra key bindings. Order matches the C source.
const EXTRA_DEFAULTS_LIST: [Default; 119] = [
    cfg("graphical_startup", DefaultType::Int),
    cfg("autoadjust_video_settings", DefaultType::Int),
    cfg("fullscreen", DefaultType::Int),
    cfg("aspect_ratio_correct", DefaultType::Int),
    cfg("startup_delay", DefaultType::Int),
    cfg("screen_width", DefaultType::Int),
    cfg("screen_height", DefaultType::Int),
    cfg("screen_bpp", DefaultType::Int),
    cfg("grabmouse", DefaultType::Int),
    cfg("novert", DefaultType::Int),
    cfg("mouse_acceleration", DefaultType::Float),
    cfg("mouse_threshold", DefaultType::Int),
    cfg("snd_samplerate", DefaultType::Int),
    cfg("snd_cachesize", DefaultType::Int),
    cfg("snd_maxslicetime_ms", DefaultType::Int),
    cfg("snd_musiccmd", DefaultType::String),
    cfg("opl_io_port", DefaultType::IntHex),
    cfg("show_endoom", DefaultType::Int),
    cfg("png_screenshots", DefaultType::Int),
    cfg("vanilla_savegame_limit", DefaultType::Int),
    cfg("vanilla_demo_limit", DefaultType::Int),
    cfg("vanilla_keyboard_mapping", DefaultType::Int),
    cfg("video_driver", DefaultType::String),
    cfg("window_position", DefaultType::String),
    cfg("joystick_index", DefaultType::Int),
    cfg("joystick_x_axis", DefaultType::Int),
    cfg("joystick_x_invert", DefaultType::Int),
    cfg("joystick_y_axis", DefaultType::Int),
    cfg("joystick_y_invert", DefaultType::Int),
    cfg("joystick_strafe_axis", DefaultType::Int),
    cfg("joystick_strafe_invert", DefaultType::Int),
    cfg("joystick_physical_button0", DefaultType::Int),
    cfg("joystick_physical_button1", DefaultType::Int),
    cfg("joystick_physical_button2", DefaultType::Int),
    cfg("joystick_physical_button3", DefaultType::Int),
    cfg("joystick_physical_button4", DefaultType::Int),
    cfg("joystick_physical_button5", DefaultType::Int),
    cfg("joystick_physical_button6", DefaultType::Int),
    cfg("joystick_physical_button7", DefaultType::Int),
    cfg("joystick_physical_button8", DefaultType::Int),
    cfg("joystick_physical_button9", DefaultType::Int),
    cfg("joyb_strafeleft", DefaultType::Int),
    cfg("joyb_straferight", DefaultType::Int),
    cfg("joyb_menu_activate", DefaultType::Int),
    cfg("joyb_prevweapon", DefaultType::Int),
    cfg("joyb_nextweapon", DefaultType::Int),
    cfg("mouseb_strafeleft", DefaultType::Int),
    cfg("mouseb_straferight", DefaultType::Int),
    cfg("mouseb_use", DefaultType::Int),
    cfg("mouseb_backward", DefaultType::Int),
    cfg("mouseb_prevweapon", DefaultType::Int),
    cfg("mouseb_nextweapon", DefaultType::Int),
    cfg("dclick_use", DefaultType::Int),
    cfg("key_pause", DefaultType::Key),
    cfg("key_menu_activate", DefaultType::Key),
    cfg("key_menu_up", DefaultType::Key),
    cfg("key_menu_down", DefaultType::Key),
    cfg("key_menu_left", DefaultType::Key),
    cfg("key_menu_right", DefaultType::Key),
    cfg("key_menu_back", DefaultType::Key),
    cfg("key_menu_forward", DefaultType::Key),
    cfg("key_menu_confirm", DefaultType::Key),
    cfg("key_menu_abort", DefaultType::Key),
    cfg("key_menu_help", DefaultType::Key),
    cfg("key_menu_save", DefaultType::Key),
    cfg("key_menu_load", DefaultType::Key),
    cfg("key_menu_volume", DefaultType::Key),
    cfg("key_menu_detail", DefaultType::Key),
    cfg("key_menu_qsave", DefaultType::Key),
    cfg("key_menu_endgame", DefaultType::Key),
    cfg("key_menu_messages", DefaultType::Key),
    cfg("key_menu_qload", DefaultType::Key),
    cfg("key_menu_quit", DefaultType::Key),
    cfg("key_menu_gamma", DefaultType::Key),
    cfg("key_spy", DefaultType::Key),
    cfg("key_menu_incscreen", DefaultType::Key),
    cfg("key_menu_decscreen", DefaultType::Key),
    cfg("key_menu_screenshot", DefaultType::Key),
    cfg("key_map_toggle", DefaultType::Key),
    cfg("key_map_north", DefaultType::Key),
    cfg("key_map_south", DefaultType::Key),
    cfg("key_map_east", DefaultType::Key),
    cfg("key_map_west", DefaultType::Key),
    cfg("key_map_zoomin", DefaultType::Key),
    cfg("key_map_zoomout", DefaultType::Key),
    cfg("key_map_maxzoom", DefaultType::Key),
    cfg("key_map_follow", DefaultType::Key),
    cfg("key_map_grid", DefaultType::Key),
    cfg("key_map_mark", DefaultType::Key),
    cfg("key_map_clearmark", DefaultType::Key),
    cfg("key_weapon1", DefaultType::Key),
    cfg("key_weapon2", DefaultType::Key),
    cfg("key_weapon3", DefaultType::Key),
    cfg("key_weapon4", DefaultType::Key),
    cfg("key_weapon5", DefaultType::Key),
    cfg("key_weapon6", DefaultType::Key),
    cfg("key_weapon7", DefaultType::Key),
    cfg("key_weapon8", DefaultType::Key),
    cfg("key_prevweapon", DefaultType::Key),
    cfg("key_nextweapon", DefaultType::Key),
    cfg("key_arti_all", DefaultType::Key),
    cfg("key_arti_health", DefaultType::Key),
    cfg("key_arti_poisonbag", DefaultType::Key),
    cfg("key_arti_blastradius", DefaultType::Key),
    cfg("key_arti_teleport", DefaultType::Key),
    cfg("key_arti_teleportother", DefaultType::Key),
    cfg("key_arti_egg", DefaultType::Key),
    cfg("key_arti_invulnerability", DefaultType::Key),
    cfg("key_message_refresh", DefaultType::Key),
    cfg("key_demo_quit", DefaultType::Key),
    cfg("key_multi_msg", DefaultType::Key),
    cfg("key_multi_msgplayer1", DefaultType::Key),
    cfg("key_multi_msgplayer2", DefaultType::Key),
    cfg("key_multi_msgplayer3", DefaultType::Key),
    cfg("key_multi_msgplayer4", DefaultType::Key),
    cfg("key_multi_msgplayer5", DefaultType::Key),
    cfg("key_multi_msgplayer6", DefaultType::Key),
    cfg("key_multi_msgplayer7", DefaultType::Key),
    cfg("key_multi_msgplayer8", DefaultType::Key),
];

// scantokey mapping from DOS keyboard scan codes to internal key codes.
// Key constant values from doomkeys.h:
//   KEY_BACKSPACE=0x7f, KEY_RCTRL=0x9D, KEY_RSHIFT=0xB6, KEYP_MULTIPLY='*',
//   KEY_RALT=0xB8, KEY_CAPSLOCK=0xBA, KEY_F1=0xBB, KEY_F2=0xBC, KEY_F3=0xBD,
//   KEY_F4=0xBE, KEY_F5=0xBF, KEY_F6=0xC0, KEY_F7=0xC1, KEY_F8=0xC2,
//   KEY_F9=0xC3, KEY_F10=0xC4, KEY_PAUSE=0xFF, KEY_SCRLCK=0xC6,
//   KEY_HOME=0xC7, KEY_UPARROW=0xAD, KEY_PGUP=0xC9, KEY_MINUS=0x2D,
//   KEY_LEFTARROW=0xAC, KEYP_5='5', KEY_RIGHTARROW=0xAE, KEYP_PLUS='+',
//   KEY_END=0xCF, KEY_DOWNARROW=0xAF, KEY_PGDN=0xD1, KEY_INS=0xD2,
//   KEY_DEL=0xD3, KEY_F11=0xD7, KEY_F12=0xD8, KEY_PRTSCR=0xD9

/// Doom internal code for the backspace key (C `KEY_BACKSPACE` from `doomkeys.h`).
const KEY_BACKSPACE: i32 = 0x7f;
/// Doom internal code for the right Ctrl key (C `KEY_RCTRL`).
const KEY_RCTRL: i32 = 0x9D;
/// Doom internal code for the right Shift key (C `KEY_RSHIFT`).
const KEY_RSHIFT: i32 = 0xB6;
/// Numpad `*` mapped to the ASCII `'*'`, matching C `KEYP_MULTIPLY`.
const KEYP_MULTIPLY: i32 = b'*' as i32;
/// Doom internal code for the right Alt key (C `KEY_RALT`).
const KEY_RALT: i32 = 0xB8;
/// Doom internal code for the Caps Lock key (C `KEY_CAPSLOCK`).
const KEY_CAPSLOCK: i32 = 0xBA;
/// Doom internal code for F1 (C `KEY_F1`).
const KEY_F1: i32 = 0xBB;
/// Doom internal code for F2 (C `KEY_F2`).
const KEY_F2: i32 = 0xBC;
/// Doom internal code for F3 (C `KEY_F3`).
const KEY_F3: i32 = 0xBD;
/// Doom internal code for F4 (C `KEY_F4`).
const KEY_F4: i32 = 0xBE;
/// Doom internal code for F5 (C `KEY_F5`).
const KEY_F5: i32 = 0xBF;
/// Doom internal code for F6 (C `KEY_F6`).
const KEY_F6: i32 = 0xC0;
/// Doom internal code for F7 (C `KEY_F7`).
const KEY_F7: i32 = 0xC1;
/// Doom internal code for F8 (C `KEY_F8`).
const KEY_F8: i32 = 0xC2;
/// Doom internal code for F9 (C `KEY_F9`).
const KEY_F9: i32 = 0xC3;
/// Doom internal code for F10 (C `KEY_F10`).
const KEY_F10: i32 = 0xC4;
/// Doom internal code for the Pause key (C `KEY_PAUSE`).
const KEY_PAUSE: i32 = 0xFF;
/// Doom internal code for Scroll Lock (C `KEY_SCRLCK`).
const KEY_SCRLCK: i32 = 0xC6;
/// Doom internal code for the Home key (C `KEY_HOME`).
const KEY_HOME: i32 = 0xC7;
/// Doom internal code for the up arrow (C `KEY_UPARROW`).
const KEY_UPARROW: i32 = 0xAD;
/// Doom internal code for Page Up (C `KEY_PGUP`).
const KEY_PGUP: i32 = 0xC9;
/// Doom internal code for the `-` key (C `KEY_MINUS`).
const KEY_MINUS: i32 = 0x2D;
/// Doom internal code for the left arrow (C `KEY_LEFTARROW`).
const KEY_LEFTARROW: i32 = 0xAC;
/// Numpad `5` mapped to the ASCII `'5'`, matching C `KEYP_5`.
const KEYP_5: i32 = b'5' as i32;
/// Doom internal code for the right arrow (C `KEY_RIGHTARROW`).
const KEY_RIGHTARROW: i32 = 0xAE;
/// Numpad `+` mapped to the ASCII `'+'`, matching C `KEYP_PLUS`.
const KEYP_PLUS: i32 = b'+' as i32;
/// Doom internal code for the End key (C `KEY_END`).
const KEY_END: i32 = 0xCF;
/// Doom internal code for the down arrow (C `KEY_DOWNARROW`).
const KEY_DOWNARROW: i32 = 0xAF;
/// Doom internal code for Page Down (C `KEY_PGDN`).
const KEY_PGDN: i32 = 0xD1;
/// Doom internal code for the Insert key (C `KEY_INS`).
const KEY_INS: i32 = 0xD2;
/// Doom internal code for the Delete key (C `KEY_DEL`).
const KEY_DEL: i32 = 0xD3;
/// Doom internal code for F11 (C `KEY_F11`).
const KEY_F11: i32 = 0xD7;
/// Doom internal code for F12 (C `KEY_F12`).
const KEY_F12: i32 = 0xD8;
/// Doom internal code for Print Screen (C `KEY_PRTSCR`).
const KEY_PRTSCR: i32 = 0xD9;

/// DOS PC keyboard scan-code -> Doom internal key-code lookup table.
///
/// Indexed by the 7-bit DOS scancode (0-127) read out of a vanilla
/// `default.cfg`; the entry is the corresponding internal key code used
/// throughout the engine. Used by [`set_variable`] to translate the
/// `untranslated` value stored on disk into a runtime key constant.
const SCANTOKEY: [i32; 128] = [
    0,
    27,
    b'1' as i32,
    b'2' as i32,
    b'3' as i32,
    b'4' as i32,
    b'5' as i32,
    b'6' as i32,
    b'7' as i32,
    b'8' as i32,
    b'9' as i32,
    b'0' as i32,
    b'-' as i32,
    b'=' as i32,
    KEY_BACKSPACE,
    9,
    b'q' as i32,
    b'w' as i32,
    b'e' as i32,
    b'r' as i32,
    b't' as i32,
    b'y' as i32,
    b'u' as i32,
    b'i' as i32,
    b'o' as i32,
    b'p' as i32,
    b'[' as i32,
    b']' as i32,
    13,
    KEY_RCTRL,
    b'a' as i32,
    b's' as i32,
    b'd' as i32,
    b'f' as i32,
    b'g' as i32,
    b'h' as i32,
    b'j' as i32,
    b'k' as i32,
    b'l' as i32,
    b';' as i32,
    b'\'' as i32,
    b'`' as i32,
    KEY_RSHIFT,
    b'\\' as i32,
    b'z' as i32,
    b'x' as i32,
    b'c' as i32,
    b'v' as i32,
    b'b' as i32,
    b'n' as i32,
    b'm' as i32,
    b',' as i32,
    b'.' as i32,
    b'/' as i32,
    KEY_RSHIFT,
    KEYP_MULTIPLY,
    KEY_RALT,
    b' ' as i32,
    KEY_CAPSLOCK,
    KEY_F1,
    KEY_F2,
    KEY_F3,
    KEY_F4,
    KEY_F5,
    KEY_F6,
    KEY_F7,
    KEY_F8,
    KEY_F9,
    KEY_F10,
    KEY_PAUSE,
    KEY_SCRLCK,
    KEY_HOME,
    KEY_UPARROW,
    KEY_PGUP,
    KEY_MINUS,
    KEY_LEFTARROW,
    KEYP_5,
    KEY_RIGHTARROW,
    KEYP_PLUS,
    KEY_END,
    KEY_DOWNARROW,
    KEY_PGDN,
    KEY_INS,
    KEY_DEL,
    0,
    0,
    0,
    KEY_F11,
    KEY_F12,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    KEY_PRTSCR,
    0,
];

/// Linear search for the entry named `name` in a [`DefaultCollection`].
///
/// Returns the matching entry, or `None` if no match exists. Mirrors C
/// `SearchCollection`.
fn search_collection<'a, const N: usize>(
    collection: &'a mut DefaultCollection<N>,
    name: &str,
) -> Option<&'a mut Default> {
    collection
        .defaults
        .iter_mut()
        .find(|default| default.name == name)
}

/// Find the named entry in either the main or extra collection and run
/// `action` on it.
///
/// Searches `doom_defaults` first and falls back to `extra_defaults`;
/// aborts via [`crate::i_error!`] if no match is found (an unknown
/// configuration variable is a fatal programming error). Mirrors C
/// `GetDefaultForName`. The config state is released before the error
/// path runs.
fn with_default_for_name<R>(name: &str, action: impl FnOnce(&mut Default) -> R) -> R {
    let result = {
        let mut config = CONFIG.borrow_mut();
        let config = &mut *config;
        search_collection(&mut config.doom_defaults, name)
            .or_else(|| search_collection(&mut config.extra_defaults, name))
            .map(action)
    };
    match result {
        Some(result) => result,
        None => i_error!("Unknown configuration variable: '{name}'"),
    }
}

/// Parse an integer parameter as written in a `.cfg` file.
///
/// C `ParseIntParameter` reads `0x` hex or `sscanf("%i")`, which also
/// takes octal and signed decimal: the same forms as [`M_StrToInt`].
/// Returns `0` on a parse error.
fn parse_int_parameter(value: &str) -> i32 {
    M_StrToInt(value).unwrap_or(0)
}

/// Apply `value` to the bound location of `def`.
///
/// Mirrors C `SetVariable`. For `String` entries the bound `Cow` takes an
/// owned copy. For `Key` entries, the raw scancode is recorded in
/// `untranslated`, then looked up in [`SCANTOKEY`] (out-of-range scancodes
/// resolve to `0`) and the translated value is stored both in
/// `original_translated` and at the bound location.
///
/// # Safety
///
/// `def.location` must have been bound by [`M_BindVariable`] to a memory
/// cell of the matching type.
unsafe fn set_variable(def: &mut Default, value: &str) {
    // SAFETY (every write below): the caller guarantees `def.location` is a
    // live cell of the type `def.ty` names.
    match def.ty {
        DefaultType::String => {
            unsafe { *(def.location as *mut Cow<'static, str>) = Cow::Owned(value.to_owned()) };
        }
        DefaultType::Int | DefaultType::IntHex => {
            unsafe { *(def.location as *mut i32) = parse_int_parameter(value) };
        }
        DefaultType::Key => {
            let intparm = parse_int_parameter(value);
            def.untranslated = intparm;
            let translated = if (0..128).contains(&intparm) {
                SCANTOKEY[intparm as usize]
            } else {
                0
            };
            def.original_translated = translated;
            unsafe { *(def.location as *mut i32) = translated };
        }
        DefaultType::Float => {
            // shortcut: unlike `atof`, trailing non-number text yields 0.0;
            // write a prefix parser if a .cfg file ever needs it.
            unsafe { *(def.location as *mut f32) = value.trim().parse().unwrap_or(0.0) };
        }
    }
}

/// Record the default `.cfg` filenames used by [`M_LoadDefaults`].
///
/// Mirrors C `M_SetConfigFilenames`.
pub fn M_SetConfigFilenames(main_config: &'static str, extra_config: &'static str) {
    let mut config = CONFIG.borrow_mut();
    config.default_main_config = main_config;
    config.default_extra_config = extra_config;
}

/// Write configuration to disk - no-op on this port.
///
/// In the C source the body of `SaveDefaultCollection` is gated behind
/// `#if ORIGCODE`, which is undefined in the chocolate-doom build used
/// here, so the original is also a no-op at runtime.
pub fn M_SaveDefaults() {
    // No-op: ORIGCODE is undefined, so the file-I/O body is never compiled
    // in the original C either.
}

/// Temporarily swap the active filenames and invoke [`M_SaveDefaults`].
///
/// Mirrors C `M_SaveDefaultsAlternate`. Because [`M_SaveDefaults`] is a
/// no-op in this build, the function only swaps the filenames; no actual
/// writes occur.
pub fn M_SaveDefaultsAlternate(main: &str, extra: &str) {
    let (orig_main, orig_extra) = {
        let mut config = CONFIG.borrow_mut();
        (
            core::mem::replace(&mut config.doom_defaults.filename, main.to_owned()),
            core::mem::replace(&mut config.extra_defaults.filename, extra.to_owned()),
        )
    };
    M_SaveDefaults();
    let mut config = CONFIG.borrow_mut();
    config.doom_defaults.filename = orig_main;
    config.extra_defaults.filename = orig_extra;
}

/// Compute filenames for the main/extra config files and announce them.
///
/// Mirrors C `M_LoadDefaults` up to (but not including) the actual file
/// parsing step (`LoadDefaultCollection`), which is itself gated by
/// `ORIGCODE` and so is a no-op upstream. `-config` and `-extraconfig`
/// command-line flags override the default filename construction. The
/// resulting filenames are stored in `doom_defaults.filename` and
/// `extra_defaults.filename` for any future call to [`M_SaveDefaults`].
pub fn M_LoadDefaults() {
    let mut config = CONFIG.borrow_mut();
    let config = &mut *config;

    let i = M_CheckParmWithArgs("-config", 1);
    if i != 0 {
        config.doom_defaults.filename = argument(i + 1).to_owned();
        log::info!("\tdefault file: {}", config.doom_defaults.filename);
    } else {
        config.doom_defaults.filename =
            format!("{}{}", config.configdir, config.default_main_config);
    }

    log::info!("saving config in {}", config.doom_defaults.filename);

    let i = M_CheckParmWithArgs("-extraconfig", 1);
    if i != 0 {
        config.extra_defaults.filename = argument(i + 1).to_owned();
        log::info!(
            "        extra configuration file: {}",
            config.extra_defaults.filename
        );
    } else {
        config.extra_defaults.filename =
            format!("{}{}", config.configdir, config.default_extra_config);
    }

    // No collection is parsed because ORIGCODE is undefined.
}

/// Bind the named configuration variable to a memory `location`.
///
/// After binding, [`M_LoadDefaults`] (and the chocolate-doom config UI)
/// can read and write the value at the supplied pointer: an `i32` for
/// integer and key variables, an `f32` for floats, a `Cow<'static, str>`
/// for strings. Aborts via `I_Error` if the name is unknown. Mirrors C
/// `M_BindVariable`.
///
/// # Safety
/// `location` must point to a value of the variable's type that stays
/// valid for reads and writes for the rest of the program (in practice a
/// field of a static state), since later safe calls such as
/// [`M_SetVariable`] and [`M_LoadDefaults`] access it.
pub unsafe fn M_BindVariable(name: &str, location: *mut c_void) {
    with_default_for_name(name, |variable| {
        variable.location = location;
        variable.bound = true;
    });
}

/// Programmatic setter: parse `value` and store it at the bound location
/// for the named variable. Returns `false` if the variable is unbound.
/// Mirrors C `M_SetVariable`.
pub fn M_SetVariable(name: &str, value: &str) -> bool {
    with_default_for_name(name, |variable| {
        if !variable.bound {
            return false;
        }
        unsafe { set_variable(variable, value) };
        true
    })
}

/// Read the value of a bound `Int` or `IntHex` variable by name.
///
/// Returns `0` if the variable is unknown, unbound, or has a non-integer
/// type. Mirrors C `M_GetIntVariable`.
pub fn M_GetIntVariable(name: &str) -> i32 {
    with_default_for_name(name, |variable| {
        if !variable.bound
            || (variable.ty != DefaultType::Int && variable.ty != DefaultType::IntHex)
        {
            return 0;
        }
        unsafe { *(variable.location as *mut i32) }
    })
}

/// Read the value of a bound `String` variable by name.
///
/// Returns `None` if the variable is unknown, unbound, or has a non-string
/// type. Mirrors C `M_GetStrVariable`.
pub fn M_GetStrVariable(name: &str) -> Option<String> {
    with_default_for_name(name, |variable| {
        if !variable.bound || variable.ty != DefaultType::String {
            return None;
        }
        Some(unsafe { (*(variable.location as *const Cow<'static, str>)).to_string() })
    })
}

/// Read the value of a bound `Float` variable by name.
///
/// Returns `0.0` if the variable is unknown, unbound, or has a non-float
/// type. Mirrors C `M_GetFloatVariable`.
pub fn M_GetFloatVariable(name: &str) -> f32 {
    with_default_for_name(name, |variable| {
        if !variable.bound || variable.ty != DefaultType::Float {
            return 0.0;
        }
        unsafe { *(variable.location as *mut f32) }
    })
}

/// The fallback configuration directory, used when [`M_SetConfigDir`] is
/// given `None`. The C source has elaborate per-platform discovery; this
/// port keeps only the "current directory" fallback.
const DEFAULT_CONFIG_DIR: &str = ".";

/// Set the global `configdir` and create the directory if needed.
///
/// `None` falls back to [`DEFAULT_CONFIG_DIR`]. A non-empty path is
/// logged. The path is created (recursive parents are NOT
/// created - matches C semantics) via [`M_MakeDirectory`]. Mirrors C
/// `M_SetConfigDir`.
pub fn M_SetConfigDir(dir: Option<&str>) {
    let configdir = dir.unwrap_or(DEFAULT_CONFIG_DIR).to_owned();
    CONFIG.borrow_mut().configdir = configdir.clone();

    if !configdir.is_empty() {
        log::info!("Using {configdir} for configuration and saves");
    }

    M_MakeDirectory(&configdir);
}

/// Compute and create the directory used to store save games.
///
/// Returns a path of the form `"<configdir>/.savegame/"` on this port -
/// the C source uses `"<configdir>/savegame/<iwadname>/"` when `ORIGCODE`
/// is defined, so `_iwadname` is unused. Returns an empty path if
/// `configdir` is empty (Windows-style "no config dir" mode).
pub fn M_GetSaveGameDir(_iwadname: &str) -> String {
    let configdir = CONFIG.borrow().configdir.clone();
    if configdir.is_empty() {
        return String::new();
    }
    let savegamedir = format!("{configdir}/.savegame/");
    M_MakeDirectory(&savegamedir);
    log::info!("Using {savegamedir} for savegames");
    savegamedir
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tables start unbound and in C order, with no filenames.
    #[test]
    fn new_state_is_unbound() {
        let config = ConfigState::new();
        assert!(config.configdir.is_empty());
        assert!(config.doom_defaults.filename.is_empty());
        assert!(config.extra_defaults.filename.is_empty());
        assert_eq!(config.doom_defaults.defaults.len(), 76);
        assert_eq!(config.extra_defaults.defaults.len(), 119);
        assert_eq!(config.doom_defaults.defaults[0].name, "mouse_sensitivity");
        assert!(
            config
                .doom_defaults
                .defaults
                .iter()
                .chain(&config.extra_defaults.defaults)
                .all(|default| !default.bound && default.location.is_null())
        );
    }

    /// Locations the test binds, at stable addresses.
    struct TestVariables {
        sensitivity: i32,
        music_command: Cow<'static, str>,
        acceleration: f32,
        key: i32,
    }

    /// A bound variable of each type takes a value through
    /// `M_SetVariable` and reads it back through the typed getters.
    #[test]
    fn set_variable_parses_each_type() {
        static VARIABLES: Global<TestVariables> = Global::new(TestVariables {
            sensitivity: 0,
            music_command: Cow::Borrowed(""),
            acceleration: 0.0,
            key: 0,
        });
        let variables = VARIABLES.as_ptr();
        unsafe {
            M_BindVariable(
                "mouse_sensitivity",
                (&raw mut (*variables).sensitivity).cast(),
            );
            M_BindVariable("snd_musiccmd", (&raw mut (*variables).music_command).cast());
            M_BindVariable(
                "mouse_acceleration",
                (&raw mut (*variables).acceleration).cast(),
            );
            M_BindVariable("key_right", (&raw mut (*variables).key).cast());
        }

        assert!(M_SetVariable("mouse_sensitivity", "010"));
        assert_eq!(M_GetIntVariable("mouse_sensitivity"), 8); // %i reads octal

        assert!(M_SetVariable("snd_musiccmd", "timidity"));
        assert_eq!(
            M_GetStrVariable("snd_musiccmd").as_deref(),
            Some("timidity")
        );

        assert!(M_SetVariable("mouse_acceleration", "0.5"));
        assert_eq!(M_GetFloatVariable("mouse_acceleration"), 0.5);

        assert!(M_SetVariable("key_right", "0x4d")); // DOS scancode 77
        assert_eq!(VARIABLES.borrow().key, SCANTOKEY[0x4d]);
    }
}
