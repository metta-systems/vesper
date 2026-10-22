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

#![allow(non_upper_case_globals, non_snake_case, static_mut_refs)]
#![allow(clippy::manual_c_str_literals)]

use {
    crate::i_error,
    std::{
        ffi::{c_void, CStr},
        ptr,
    },
};

/// Type tag for a configuration variable.
///
/// Mirrors the C `default_type_t` enum (`DEFAULT_INT`, `DEFAULT_INT_HEX`,
/// `DEFAULT_STRING`, `DEFAULT_FLOAT`, `DEFAULT_KEY`). The order is
/// significant because the variant is serialised across the FFI boundary
/// and compared with `PartialEq` in the getters below.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum DefaultType {
    /// Decimal integer (also accepts `0x` hex when read from disk).
    Int = 0,
    /// Hexadecimal integer (display form `0x...`).
    IntHex,
    /// Heap-allocated null-terminated string.
    String,
    /// Single-precision float parsed via libc `atof`.
    Float,
    /// DOS scancode that maps through [`SCANTOKEY`] to an internal key code.
    Key,
}

/// Description of a single configuration variable.
///
/// Mirrors the C `default_t` struct, including the `untranslated` /
/// `original_translated` bookkeeping used to roundtrip DOS scancodes.
#[repr(C)]
struct Default {
    /// Variable name, e.g. `"key_fire\0"`; pointers into `'static` byte
    /// literals supplied by the `cfg` helper.
    name: *const u8,
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
/// Mirrors the C `default_collection_t` struct.
struct DefaultCollection {
    /// Pointer to the first entry of an array of `numdefaults` `Default`s.
    defaults: *mut Default,
    /// Number of entries in the `defaults` array.
    numdefaults: i32,
    /// Filename used by [`M_LoadDefaults`] / [`M_SaveDefaults`]. NULL until
    /// set, owned by the caller's argv or by [`M_StringJoinA`].
    filename: *mut u8,
}

/// Construct a `Default` from a NUL-terminated name literal and a type tag.
///
/// `const`-evaluated so the static variable tables below can be declared at
/// module level. Replaces the C `CONFIG_VARIABLE_*` macros.
const fn cfg(name: &'static [u8], ty: DefaultType) -> Default {
    Default {
        name: name.as_ptr(),
        location: ptr::null_mut(),
        ty,
        untranslated: 0,
        original_translated: 0,
        bound: false,
    }
}

extern "C" {
    /// libc `printf`: used for the boot-time status messages.
    fn printf(fmt: *const u8, ...) -> i32;
    /// libc `strdup`: heap-duplicate a string (for `String` variables).
    fn strdup(s: *const u8) -> *mut u8;
    /// libc `strcmp`: compare two C strings.
    fn strcmp(a: *const u8, b: *const u8) -> i32;
    /// libc `atof`: parse a `u8*` as a double-precision number.
    fn atof(s: *const u8) -> f64;
    /// libc `malloc`: used by `GetDefaultConfigDir`.
    fn malloc(n: usize) -> *mut c_void;
}

use crate::doom::{
    m_argv::{myargv, M_CheckParmWithArgs},
    m_misc::{M_MakeDirectory, M_StringJoinA},
};

/// Global configuration directory (mirrors C `char *configdir`).
///
/// Initialised by [`M_SetConfigDir`] from the command line or from
/// `GetDefaultConfigDir`. Read by [`M_LoadDefaults`] and
/// [`M_GetSaveGameDir`].
pub static mut configdir: *mut u8 = ptr::null_mut();

/// Filename component for the main `.cfg` file (e.g. `"default.cfg"`).
/// Set by [`M_SetConfigFilenames`].
static mut default_main_config: *mut u8 = ptr::null_mut();
/// Filename component for the extra (chocolate-doom) `.cfg` file. Set by
/// [`M_SetConfigFilenames`].
static mut default_extra_config: *mut u8 = ptr::null_mut();

/// Static table backing the vanilla `doom_defaults` collection.
///
/// Order and names are kept verbatim from the C source so that on-disk
/// `.cfg` files remain compatible. Entries become live once
/// [`M_BindVariable`] attaches a memory location.
static mut DOOM_DEFAULTS_LIST: [Default; 76] = [
    cfg(b"mouse_sensitivity\0", DefaultType::Int),
    cfg(b"sfx_volume\0", DefaultType::Int),
    cfg(b"music_volume\0", DefaultType::Int),
    cfg(b"show_talk\0", DefaultType::Int),
    cfg(b"voice_volume\0", DefaultType::Int),
    cfg(b"show_messages\0", DefaultType::Int),
    cfg(b"key_right\0", DefaultType::Key),
    cfg(b"key_left\0", DefaultType::Key),
    cfg(b"key_up\0", DefaultType::Key),
    cfg(b"key_down\0", DefaultType::Key),
    cfg(b"key_strafeleft\0", DefaultType::Key),
    cfg(b"key_straferight\0", DefaultType::Key),
    cfg(b"key_useHealth\0", DefaultType::Key),
    cfg(b"key_jump\0", DefaultType::Key),
    cfg(b"key_flyup\0", DefaultType::Key),
    cfg(b"key_flydown\0", DefaultType::Key),
    cfg(b"key_flycenter\0", DefaultType::Key),
    cfg(b"key_lookup\0", DefaultType::Key),
    cfg(b"key_lookdown\0", DefaultType::Key),
    cfg(b"key_lookcenter\0", DefaultType::Key),
    cfg(b"key_invquery\0", DefaultType::Key),
    cfg(b"key_mission\0", DefaultType::Key),
    cfg(b"key_invPop\0", DefaultType::Key),
    cfg(b"key_invKey\0", DefaultType::Key),
    cfg(b"key_invHome\0", DefaultType::Key),
    cfg(b"key_invEnd\0", DefaultType::Key),
    cfg(b"key_invleft\0", DefaultType::Key),
    cfg(b"key_invright\0", DefaultType::Key),
    cfg(b"key_invLeft\0", DefaultType::Key),
    cfg(b"key_invRight\0", DefaultType::Key),
    cfg(b"key_useartifact\0", DefaultType::Key),
    cfg(b"key_invUse\0", DefaultType::Key),
    cfg(b"key_invDrop\0", DefaultType::Key),
    cfg(b"key_lookUp\0", DefaultType::Key),
    cfg(b"key_lookDown\0", DefaultType::Key),
    cfg(b"key_fire\0", DefaultType::Key),
    cfg(b"key_use\0", DefaultType::Key),
    cfg(b"key_strafe\0", DefaultType::Key),
    cfg(b"key_speed\0", DefaultType::Key),
    cfg(b"use_mouse\0", DefaultType::Int),
    cfg(b"mouseb_fire\0", DefaultType::Int),
    cfg(b"mouseb_strafe\0", DefaultType::Int),
    cfg(b"mouseb_forward\0", DefaultType::Int),
    cfg(b"mouseb_jump\0", DefaultType::Int),
    cfg(b"use_joystick\0", DefaultType::Int),
    cfg(b"joyb_fire\0", DefaultType::Int),
    cfg(b"joyb_strafe\0", DefaultType::Int),
    cfg(b"joyb_use\0", DefaultType::Int),
    cfg(b"joyb_speed\0", DefaultType::Int),
    cfg(b"joyb_jump\0", DefaultType::Int),
    cfg(b"screenblocks\0", DefaultType::Int),
    cfg(b"screensize\0", DefaultType::Int),
    cfg(b"detaillevel\0", DefaultType::Int),
    cfg(b"snd_channels\0", DefaultType::Int),
    cfg(b"snd_musicdevice\0", DefaultType::Int),
    cfg(b"snd_sfxdevice\0", DefaultType::Int),
    cfg(b"snd_sbport\0", DefaultType::Int),
    cfg(b"snd_sbirq\0", DefaultType::Int),
    cfg(b"snd_sbdma\0", DefaultType::Int),
    cfg(b"snd_mport\0", DefaultType::Int),
    cfg(b"usegamma\0", DefaultType::Int),
    cfg(b"savedir\0", DefaultType::String),
    cfg(b"messageson\0", DefaultType::Int),
    cfg(b"back_flat\0", DefaultType::String),
    cfg(b"nickname\0", DefaultType::String),
    cfg(b"chatmacro0\0", DefaultType::String),
    cfg(b"chatmacro1\0", DefaultType::String),
    cfg(b"chatmacro2\0", DefaultType::String),
    cfg(b"chatmacro3\0", DefaultType::String),
    cfg(b"chatmacro4\0", DefaultType::String),
    cfg(b"chatmacro5\0", DefaultType::String),
    cfg(b"chatmacro6\0", DefaultType::String),
    cfg(b"chatmacro7\0", DefaultType::String),
    cfg(b"chatmacro8\0", DefaultType::String),
    cfg(b"chatmacro9\0", DefaultType::String),
    cfg(b"comport\0", DefaultType::Int),
];

/// Collection wrapper around [`DOOM_DEFAULTS_LIST`] for the vanilla config
/// file. Mirrors C `static default_collection_t doom_defaults`.
static mut doom_defaults: DefaultCollection = DefaultCollection {
    defaults: unsafe { &mut DOOM_DEFAULTS_LIST as *mut _ },
    numdefaults: 76,
    filename: ptr::null_mut(),
};

/// Static table backing the chocolate-doom `extra_defaults` collection.
///
/// Holds the extended (non-vanilla) settings - video, joystick, mouse and
/// extra key bindings. Order matches the C source.
static mut EXTRA_DEFAULTS_LIST: [Default; 119] = [
    cfg(b"graphical_startup\0", DefaultType::Int),
    cfg(b"autoadjust_video_settings\0", DefaultType::Int),
    cfg(b"fullscreen\0", DefaultType::Int),
    cfg(b"aspect_ratio_correct\0", DefaultType::Int),
    cfg(b"startup_delay\0", DefaultType::Int),
    cfg(b"screen_width\0", DefaultType::Int),
    cfg(b"screen_height\0", DefaultType::Int),
    cfg(b"screen_bpp\0", DefaultType::Int),
    cfg(b"grabmouse\0", DefaultType::Int),
    cfg(b"novert\0", DefaultType::Int),
    cfg(b"mouse_acceleration\0", DefaultType::Float),
    cfg(b"mouse_threshold\0", DefaultType::Int),
    cfg(b"snd_samplerate\0", DefaultType::Int),
    cfg(b"snd_cachesize\0", DefaultType::Int),
    cfg(b"snd_maxslicetime_ms\0", DefaultType::Int),
    cfg(b"snd_musiccmd\0", DefaultType::String),
    cfg(b"opl_io_port\0", DefaultType::IntHex),
    cfg(b"show_endoom\0", DefaultType::Int),
    cfg(b"png_screenshots\0", DefaultType::Int),
    cfg(b"vanilla_savegame_limit\0", DefaultType::Int),
    cfg(b"vanilla_demo_limit\0", DefaultType::Int),
    cfg(b"vanilla_keyboard_mapping\0", DefaultType::Int),
    cfg(b"video_driver\0", DefaultType::String),
    cfg(b"window_position\0", DefaultType::String),
    cfg(b"joystick_index\0", DefaultType::Int),
    cfg(b"joystick_x_axis\0", DefaultType::Int),
    cfg(b"joystick_x_invert\0", DefaultType::Int),
    cfg(b"joystick_y_axis\0", DefaultType::Int),
    cfg(b"joystick_y_invert\0", DefaultType::Int),
    cfg(b"joystick_strafe_axis\0", DefaultType::Int),
    cfg(b"joystick_strafe_invert\0", DefaultType::Int),
    cfg(b"joystick_physical_button0\0", DefaultType::Int),
    cfg(b"joystick_physical_button1\0", DefaultType::Int),
    cfg(b"joystick_physical_button2\0", DefaultType::Int),
    cfg(b"joystick_physical_button3\0", DefaultType::Int),
    cfg(b"joystick_physical_button4\0", DefaultType::Int),
    cfg(b"joystick_physical_button5\0", DefaultType::Int),
    cfg(b"joystick_physical_button6\0", DefaultType::Int),
    cfg(b"joystick_physical_button7\0", DefaultType::Int),
    cfg(b"joystick_physical_button8\0", DefaultType::Int),
    cfg(b"joystick_physical_button9\0", DefaultType::Int),
    cfg(b"joyb_strafeleft\0", DefaultType::Int),
    cfg(b"joyb_straferight\0", DefaultType::Int),
    cfg(b"joyb_menu_activate\0", DefaultType::Int),
    cfg(b"joyb_prevweapon\0", DefaultType::Int),
    cfg(b"joyb_nextweapon\0", DefaultType::Int),
    cfg(b"mouseb_strafeleft\0", DefaultType::Int),
    cfg(b"mouseb_straferight\0", DefaultType::Int),
    cfg(b"mouseb_use\0", DefaultType::Int),
    cfg(b"mouseb_backward\0", DefaultType::Int),
    cfg(b"mouseb_prevweapon\0", DefaultType::Int),
    cfg(b"mouseb_nextweapon\0", DefaultType::Int),
    cfg(b"dclick_use\0", DefaultType::Int),
    cfg(b"key_pause\0", DefaultType::Key),
    cfg(b"key_menu_activate\0", DefaultType::Key),
    cfg(b"key_menu_up\0", DefaultType::Key),
    cfg(b"key_menu_down\0", DefaultType::Key),
    cfg(b"key_menu_left\0", DefaultType::Key),
    cfg(b"key_menu_right\0", DefaultType::Key),
    cfg(b"key_menu_back\0", DefaultType::Key),
    cfg(b"key_menu_forward\0", DefaultType::Key),
    cfg(b"key_menu_confirm\0", DefaultType::Key),
    cfg(b"key_menu_abort\0", DefaultType::Key),
    cfg(b"key_menu_help\0", DefaultType::Key),
    cfg(b"key_menu_save\0", DefaultType::Key),
    cfg(b"key_menu_load\0", DefaultType::Key),
    cfg(b"key_menu_volume\0", DefaultType::Key),
    cfg(b"key_menu_detail\0", DefaultType::Key),
    cfg(b"key_menu_qsave\0", DefaultType::Key),
    cfg(b"key_menu_endgame\0", DefaultType::Key),
    cfg(b"key_menu_messages\0", DefaultType::Key),
    cfg(b"key_menu_qload\0", DefaultType::Key),
    cfg(b"key_menu_quit\0", DefaultType::Key),
    cfg(b"key_menu_gamma\0", DefaultType::Key),
    cfg(b"key_spy\0", DefaultType::Key),
    cfg(b"key_menu_incscreen\0", DefaultType::Key),
    cfg(b"key_menu_decscreen\0", DefaultType::Key),
    cfg(b"key_menu_screenshot\0", DefaultType::Key),
    cfg(b"key_map_toggle\0", DefaultType::Key),
    cfg(b"key_map_north\0", DefaultType::Key),
    cfg(b"key_map_south\0", DefaultType::Key),
    cfg(b"key_map_east\0", DefaultType::Key),
    cfg(b"key_map_west\0", DefaultType::Key),
    cfg(b"key_map_zoomin\0", DefaultType::Key),
    cfg(b"key_map_zoomout\0", DefaultType::Key),
    cfg(b"key_map_maxzoom\0", DefaultType::Key),
    cfg(b"key_map_follow\0", DefaultType::Key),
    cfg(b"key_map_grid\0", DefaultType::Key),
    cfg(b"key_map_mark\0", DefaultType::Key),
    cfg(b"key_map_clearmark\0", DefaultType::Key),
    cfg(b"key_weapon1\0", DefaultType::Key),
    cfg(b"key_weapon2\0", DefaultType::Key),
    cfg(b"key_weapon3\0", DefaultType::Key),
    cfg(b"key_weapon4\0", DefaultType::Key),
    cfg(b"key_weapon5\0", DefaultType::Key),
    cfg(b"key_weapon6\0", DefaultType::Key),
    cfg(b"key_weapon7\0", DefaultType::Key),
    cfg(b"key_weapon8\0", DefaultType::Key),
    cfg(b"key_prevweapon\0", DefaultType::Key),
    cfg(b"key_nextweapon\0", DefaultType::Key),
    cfg(b"key_arti_all\0", DefaultType::Key),
    cfg(b"key_arti_health\0", DefaultType::Key),
    cfg(b"key_arti_poisonbag\0", DefaultType::Key),
    cfg(b"key_arti_blastradius\0", DefaultType::Key),
    cfg(b"key_arti_teleport\0", DefaultType::Key),
    cfg(b"key_arti_teleportother\0", DefaultType::Key),
    cfg(b"key_arti_egg\0", DefaultType::Key),
    cfg(b"key_arti_invulnerability\0", DefaultType::Key),
    cfg(b"key_message_refresh\0", DefaultType::Key),
    cfg(b"key_demo_quit\0", DefaultType::Key),
    cfg(b"key_multi_msg\0", DefaultType::Key),
    cfg(b"key_multi_msgplayer1\0", DefaultType::Key),
    cfg(b"key_multi_msgplayer2\0", DefaultType::Key),
    cfg(b"key_multi_msgplayer3\0", DefaultType::Key),
    cfg(b"key_multi_msgplayer4\0", DefaultType::Key),
    cfg(b"key_multi_msgplayer5\0", DefaultType::Key),
    cfg(b"key_multi_msgplayer6\0", DefaultType::Key),
    cfg(b"key_multi_msgplayer7\0", DefaultType::Key),
    cfg(b"key_multi_msgplayer8\0", DefaultType::Key),
];

/// Collection wrapper around [`EXTRA_DEFAULTS_LIST`] for the extended
/// chocolate-doom config file. Mirrors C `static default_collection_t
/// extra_defaults`.
static mut extra_defaults: DefaultCollection = DefaultCollection {
    defaults: unsafe { &mut EXTRA_DEFAULTS_LIST as *mut _ },
    numdefaults: 119,
    filename: ptr::null_mut(),
};

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

/// Unix directory separator string with trailing NUL (C
/// `DIR_SEPARATOR_S = "/"`). Used by [`M_GetSaveGameDir`].
const DIR_SEPARATOR_S: &[u8] = b"/\0";

/// Linear search for the entry named `name` in a [`DefaultCollection`].
///
/// Returns a raw pointer to the matching entry or NULL if no match exists.
/// Mirrors C `SearchCollection`.
///
/// # Safety
///
/// `collection` must point to a valid, initialised [`DefaultCollection`].
/// `name` must be a valid NUL-terminated C string.
unsafe fn search_collection(collection: *const DefaultCollection, name: *const u8) -> *mut Default {
    let collection = &*collection;
    for i in 0..collection.numdefaults {
        let def = collection.defaults.add(i as usize);
        if strcmp(name, (*def).name) == 0 {
            return def;
        }
    }
    ptr::null_mut()
}

/// Find the named entry in either the main or extra collection.
///
/// Searches [`doom_defaults`] first and falls back to [`extra_defaults`];
/// aborts via [`crate::i_error!`] if no match is found (an unknown
/// configuration variable is a fatal programming error). Mirrors C
/// `GetDefaultForName`.
///
/// # Safety
///
/// `name` must be a valid NUL-terminated C string.
unsafe fn get_default_for_name(name: *const u8) -> *mut Default {
    let mut result = search_collection(&doom_defaults, name);
    if result.is_null() {
        result = search_collection(&extra_defaults, name);
    }
    if result.is_null() {
        i_error!(
            "Unknown configuration variable: '{}'",
            std::ffi::CStr::from_ptr(name.cast()).to_string_lossy()
        );
    }
    result
}

/// Parse an integer parameter as written in a `.cfg` file.
///
/// Recognises `0x` / `0X` hexadecimal prefixes (case-insensitive) and
/// decimal otherwise. Returns `0` on any parse error. Used for the
/// `DEFAULT_INT*` / `DEFAULT_KEY` cases. Diverges from C
/// `ParseIntParameter` in two ways flagged inline below: this port
/// accepts the upper-case `0X` prefix and drops C's leading-zero octal
/// fallback.
fn parse_int_parameter(strparm: &CStr) -> i32 {
    let bytes = strparm.to_bytes();
    // FIXME: C `ParseIntParameter` only checks the lowercase `0x` prefix;
    // accepting `0X` is a Rust-port extension. Verify no `.cfg` file relied
    // on the C behaviour rejecting upper-case prefixes.
    if bytes.len() >= 2 && bytes[0] == b'0' && (bytes[1] == b'x' || bytes[1] == b'X') {
        if let Ok(val) = i32::from_str_radix(std::str::from_utf8(&bytes[2..]).unwrap_or("0"), 16) {
            return val;
        }
    }
    // FIXME: C `ParseIntParameter` falls back to `sscanf("%i", ...)` which
    // also recognises leading-zero octal; Rust `parse::<i32>()` rejects it
    // and returns 0. Octal config values would silently become 0 here.
    if let Ok(val) = std::str::from_utf8(bytes).unwrap_or("0").parse::<i32>() {
        return val;
    }
    0
}

/// Apply `value` (raw C string) to the bound location of `def`.
///
/// Mirrors C `SetVariable`. For `String` entries, the value is heap-
/// duplicated via `strdup` and the previous pointer is overwritten without
/// being freed (matching the C behaviour - the old buffer leaks). For
/// `Key` entries, the raw scancode is recorded in `untranslated`, then
/// looked up in [`SCANTOKEY`] (out-of-range scancodes resolve to `0`) and
/// the translated value is stored both in `original_translated` and at the
/// bound location.
///
/// # Safety
///
/// `def` must be a valid pointer to a `Default` whose `location` field has
/// been bound by [`M_BindVariable`] to a memory cell of the matching type.
/// `value` must be a valid NUL-terminated C string.
unsafe fn set_variable(def: *mut Default, value: *const u8) {
    let def = &mut *def;
    match def.ty {
        DefaultType::String => {
            *(def.location as *mut *mut u8) = strdup(value);
        }
        DefaultType::Int | DefaultType::IntHex => {
            let strparm = CStr::from_ptr(value.cast());
            *(def.location as *mut i32) = parse_int_parameter(strparm);
        }
        DefaultType::Key => {
            let strparm = CStr::from_ptr(value.cast());
            let intparm = parse_int_parameter(strparm);
            def.untranslated = intparm;
            let translated = if (0..128).contains(&intparm) {
                SCANTOKEY[intparm as usize]
            } else {
                0
            };
            def.original_translated = translated;
            *(def.location as *mut i32) = translated;
        }
        DefaultType::Float => {
            *(def.location as *mut f32) = atof(value) as f32;
        }
    }
}

/// Record the default `.cfg` filenames used by [`M_LoadDefaults`].
///
/// Mirrors C `M_SetConfigFilenames`. Stores the pointers without copying;
/// the caller owns the underlying buffers and must keep them alive for the
/// remainder of the program.
pub fn M_SetConfigFilenames(main_config: *mut u8, extra_config: *mut u8) {
    unsafe {
        default_main_config = main_config;
        default_extra_config = extra_config;
    }
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
/// no-op in this build, the function only manipulates filename pointers;
/// no actual writes occur.
pub fn M_SaveDefaultsAlternate(main: *mut u8, extra: *mut u8) {
    unsafe {
        let orig_main = doom_defaults.filename;
        let orig_extra = extra_defaults.filename;
        doom_defaults.filename = main;
        extra_defaults.filename = extra;
        M_SaveDefaults();
        doom_defaults.filename = orig_main;
        extra_defaults.filename = orig_extra;
    }
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
    unsafe {
        let i = M_CheckParmWithArgs(b"-config\0".as_ptr().cast_mut(), 1);
        if i != 0 {
            doom_defaults.filename = *myargv.offset((i + 1) as isize);
            printf(b"\tdefault file: %s\n\0".as_ptr(), doom_defaults.filename);
        } else {
            let strs: [*const u8; 3] = [configdir, default_main_config, std::ptr::null()];
            // SAFETY: null-terminated pointer array; result lives for the duration of the program.
            doom_defaults.filename = M_StringJoinA(strs.as_ptr());
        }

        printf(b"saving config in %s\n\0".as_ptr(), doom_defaults.filename);

        let i = M_CheckParmWithArgs(b"-extraconfig\0".as_ptr().cast_mut(), 1);
        if i != 0 {
            extra_defaults.filename = *myargv.offset((i + 1) as isize);
            printf(
                b"        extra configuration file: %s\n\0".as_ptr(),
                extra_defaults.filename,
            );
        } else {
            let strs: [*const u8; 3] = [configdir, default_extra_config, std::ptr::null()];
            // SAFETY: null-terminated pointer array; result lives for the duration of the program.
            extra_defaults.filename = M_StringJoinA(strs.as_ptr());
        }

        // No-ops because ORIGCODE is undefined
        let _ = &doom_defaults;
        let _ = &extra_defaults;
    }
}

/// Bind the named configuration variable to a memory `location`.
///
/// After binding, [`M_LoadDefaults`] (and the chocolate-doom config UI)
/// can read and write the value at the supplied pointer. Aborts via
/// `I_Error` if the name is unknown. Mirrors C `M_BindVariable`.
pub fn M_BindVariable(name: *mut u8, location: *mut c_void) {
    unsafe {
        let variable = get_default_for_name(name as *const u8);
        (*variable).location = location;
        (*variable).bound = true;
    }
}

/// Programmatic setter: parse `value` and store it at the bound location
/// for the named variable. Returns `false` if the variable is unknown or
/// unbound. Mirrors C `M_SetVariable`.
pub fn M_SetVariable(name: *mut u8, value: *mut u8) -> bool {
    unsafe {
        let variable = get_default_for_name(name as *const u8);
        if variable.is_null() || !(*variable).bound {
            return false;
        }
        set_variable(variable, value as *const u8);
        true
    }
}

/// Read the value of a bound `Int` or `IntHex` variable by name.
///
/// Returns `0` if the variable is unknown, unbound, or has a non-integer
/// type. Mirrors C `M_GetIntVariable`.
pub fn M_GetIntVariable(name: *mut u8) -> i32 {
    unsafe {
        let variable = get_default_for_name(name as *const u8);
        if variable.is_null()
            || !(*variable).bound
            || ((*variable).ty != DefaultType::Int && (*variable).ty != DefaultType::IntHex)
        {
            return 0;
        }
        *((*variable).location as *mut i32)
    }
}

/// Read the value of a bound `String` variable by name.
///
/// Returns NULL if the variable is unknown, unbound, or has a non-string
/// type. The returned pointer aliases the storage held by the caller of
/// [`M_BindVariable`]. Mirrors C `M_GetStrVariable`.
pub fn M_GetStrVariable(name: *mut u8) -> *const u8 {
    unsafe {
        let variable = get_default_for_name(name as *const u8);
        if variable.is_null() || !(*variable).bound || (*variable).ty != DefaultType::String {
            return ptr::null();
        }
        *((*variable).location as *mut *const u8)
    }
}

/// Read the value of a bound `Float` variable by name.
///
/// Returns `0.0` if the variable is unknown, unbound, or has a non-float
/// type. Mirrors C `M_GetFloatVariable`.
pub fn M_GetFloatVariable(name: *mut u8) -> f32 {
    unsafe {
        let variable = get_default_for_name(name as *const u8);
        if variable.is_null() || !(*variable).bound || (*variable).ty != DefaultType::Float {
            return 0.0;
        }
        *((*variable).location as *mut f32)
    }
}

/// Return a fresh malloc'd `"."` string as the fallback configuration
/// directory.
///
/// Used when [`M_SetConfigDir`] is called with NULL. The C source has
/// elaborate per-platform discovery; this port keeps only the
/// "current directory" fallback. The returned pointer must be freed by
/// the caller (or, in practice, leaks for the lifetime of the program).
///
/// # Safety
///
/// Relies on libc `malloc` succeeding; the result is not null-checked,
/// matching the C source.
unsafe fn GetDefaultConfigDir() -> *mut u8 {
    let result = malloc(2) as *mut u8;
    *result = b'.';
    *result.offset(1) = 0;
    result
}

/// Set the global [`configdir`] and create the directory if needed.
///
/// Passing NULL falls back to `GetDefaultConfigDir`. A non-empty path is
/// echoed to the console. The path is created (recursive parents are NOT
/// created - matches C semantics) via [`M_MakeDirectory`]. Mirrors C
/// `M_SetConfigDir`.
pub fn M_SetConfigDir(dir: *mut u8) {
    unsafe {
        if !dir.is_null() {
            configdir = dir;
        } else {
            configdir = GetDefaultConfigDir();
        }

        if strcmp(configdir, b"\0".as_ptr()) != 0 {
            printf(
                b"Using %s for configuration and saves\n\0".as_ptr(),
                configdir,
            );
        }

        M_MakeDirectory(configdir);
    }
}

/// Compute and create the directory used to store save games.
///
/// Returns a heap-allocated path of the form
/// `"<configdir>/.savegame/"` on this port - the C source uses
/// `"<configdir>/savegame/<iwadname>/"` when `ORIGCODE` is defined. The
/// `_iwadname` parameter is therefore unused but kept for ABI
/// compatibility. Returns an empty `strdup("")` if `configdir` is empty
/// (Windows-style "no config dir" mode). The returned pointer is owned by
/// the caller and must be freed.
pub fn M_GetSaveGameDir(_iwadname: *mut u8) -> *mut u8 {
    unsafe {
        if strcmp(configdir, b"\0".as_ptr()) == 0 {
            strdup(b"\0".as_ptr())
        } else {
            let strs: [*const u8; 4] = [
                configdir,
                DIR_SEPARATOR_S.as_ptr(),
                b".savegame/\0".as_ptr(),
                std::ptr::null(),
            ];
            // SAFETY: null-terminated pointer array; ownership transferred to caller via return.
            let savegamedir = M_StringJoinA(strs.as_ptr());
            M_MakeDirectory(savegamedir);
            printf(b"Using %s for savegames\n\0".as_ptr(), savegamedir);
            savegamedir
        }
    }
}
