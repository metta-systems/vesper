//! Rust port of vendor/doomgeneric/m_controls.c.
//!
//! Default keyboard / mouse / joystick bindings for every game in the
//! chocolate-doom family (Doom, Heretic, Hexen, Strife), plus the
//! `M_Bind*` registration helpers that wire each binding into the
//! `m_config` variable table so they can be read or written through
//! `default.cfg`.
//!
//! All bindings are exposed as `#[no_mangle] pub static mut` so the
//! original C call sites can read and mutate them directly. Each
//! `M_Bind<Game>Controls` function calls into `M_BindVariable` (from
//! `m_config`) for the keys relevant to that game's binding subset.

#![allow(non_upper_case_globals, non_snake_case)]

use std::ffi::{c_char, c_int, c_void};
use std::os::raw::c_uint;

use super::doomkeys::{
    KEY_BACKSPACE, KEY_DEL, KEY_DOWNARROW, KEY_END, KEY_ENTER, KEY_EQUALS, KEY_ESCAPE, KEY_F1,
    KEY_F10, KEY_F11, KEY_F12, KEY_F2, KEY_F3, KEY_F4, KEY_F5, KEY_F6, KEY_F7, KEY_F8, KEY_F9,
    KEY_HOME, KEY_INS, KEY_LEFTARROW, KEY_MINUS, KEY_PAUSE, KEY_PGDN, KEY_PGUP, KEY_RALT,
    KEY_RIGHTARROW, KEY_RSHIFT, KEY_STRAFE_L, KEY_STRAFE_R, KEY_TAB, KEY_UPARROW, KEY_USE,
};

//
// Keyboard controls
//

/// Turn right (default: right arrow).
#[no_mangle]
pub static mut key_right: c_int = KEY_RIGHTARROW as c_int;
/// Turn left (default: left arrow).
#[no_mangle]
pub static mut key_left: c_int = KEY_LEFTARROW as c_int;
/// Move forward (default: up arrow).
#[no_mangle]
pub static mut key_up: c_int = KEY_UPARROW as c_int;
/// Move backward (default: down arrow).
#[no_mangle]
pub static mut key_down: c_int = KEY_DOWNARROW as c_int;
/// Strafe left (default: comma `KEY_STRAFE_L`).
#[no_mangle]
pub static mut key_strafeleft: c_int = KEY_STRAFE_L as c_int;
/// Strafe right (default: period `KEY_STRAFE_R`).
#[no_mangle]
pub static mut key_straferight: c_int = KEY_STRAFE_R as c_int;
/// Fire current weapon (default: `KEY_FIRE`, the right Ctrl).
#[no_mangle]
pub static mut key_fire: c_int = super::doomkeys::KEY_FIRE as c_int;
/// Use / open / activate trigger (default: space `KEY_USE`).
#[no_mangle]
pub static mut key_use: c_int = KEY_USE as c_int;
/// Hold to convert turn into strafe (default: right Alt).
#[no_mangle]
pub static mut key_strafe: c_int = KEY_RALT as c_int;
/// Hold to run (default: right Shift).
#[no_mangle]
pub static mut key_speed: c_int = KEY_RSHIFT as c_int;

//
// Heretic keyboard controls
//

/// Heretic: fly up (default: Page Up).
#[no_mangle]
pub static mut key_flyup: c_int = KEY_PGUP as c_int;
/// Heretic: fly down (default: Insert).
#[no_mangle]
pub static mut key_flydown: c_int = KEY_INS as c_int;
/// Heretic: stop flying / centre vertically (default: Home).
#[no_mangle]
pub static mut key_flycenter: c_int = KEY_HOME as c_int;

/// Heretic: look up (default: Page Down).
#[no_mangle]
pub static mut key_lookup: c_int = KEY_PGDN as c_int;
/// Heretic: look down (default: Delete).
#[no_mangle]
pub static mut key_lookdown: c_int = KEY_DEL as c_int;
/// Heretic: centre vertical look (default: End).
#[no_mangle]
pub static mut key_lookcenter: c_int = KEY_END as c_int;

/// Heretic/Strife: cycle inventory left (default: `[`; Strife uses Insert).
#[no_mangle]
pub static mut key_invleft: c_int = b'[' as c_int;
/// Heretic/Strife: cycle inventory right (default: `]`; Strife uses Delete).
#[no_mangle]
pub static mut key_invright: c_int = b']' as c_int;
/// Heretic: use selected inventory artifact (default: Enter).
#[no_mangle]
pub static mut key_useartifact: c_int = KEY_ENTER as c_int;

//
// Hexen key controls
//

/// Hexen/Strife: jump (default: `/`; Strife rebinds to `a`).
#[no_mangle]
pub static mut key_jump: c_int = b'/' as c_int;

/// Hexen: use all artifacts of selected kind (default: Backspace).
#[no_mangle]
pub static mut key_arti_all: c_int = KEY_BACKSPACE as c_int;
/// Hexen: use Quartz Flask / Mystic Urn (default: `\\`).
#[no_mangle]
pub static mut key_arti_health: c_int = b'\\' as c_int;
/// Hexen: use poison bag artifact (default: `0`).
#[no_mangle]
pub static mut key_arti_poisonbag: c_int = b'0' as c_int;
/// Hexen: use disc of repulsion (default: `9`).
#[no_mangle]
pub static mut key_arti_blastradius: c_int = b'9' as c_int;
/// Hexen: use teleport artifact (default: `8`).
#[no_mangle]
pub static mut key_arti_teleport: c_int = b'8' as c_int;
/// Hexen: use banishment device (default: `7`).
#[no_mangle]
pub static mut key_arti_teleportother: c_int = b'7' as c_int;
/// Hexen: use porkalator (default: `6`).
#[no_mangle]
pub static mut key_arti_egg: c_int = b'6' as c_int;
/// Hexen: use icon of the defender (default: `5`).
#[no_mangle]
pub static mut key_arti_invulnerability: c_int = b'5' as c_int;

//
// Strife key controls
//

/// Strife: use a medkit from inventory (default: `h`).
#[no_mangle]
pub static mut key_usehealth: c_int = b'h' as c_int;
/// Strife: query selected NPC / object (default: `q`).
#[no_mangle]
pub static mut key_invquery: c_int = b'q' as c_int;
/// Strife: open mission log (default: `w`).
#[no_mangle]
pub static mut key_mission: c_int = b'w' as c_int;
/// Strife: open the inventory popup (default: `z`).
#[no_mangle]
pub static mut key_invpop: c_int = b'z' as c_int;
/// Strife: open the keys popup (default: `k`).
#[no_mangle]
pub static mut key_invkey: c_int = b'k' as c_int;
/// Strife: cycle to the first inventory item (default: Home).
#[no_mangle]
pub static mut key_invhome: c_int = KEY_HOME as c_int;
/// Strife: cycle to the last inventory item (default: End).
#[no_mangle]
pub static mut key_invend: c_int = KEY_END as c_int;
/// Strife: use selected inventory item (default: Enter).
#[no_mangle]
pub static mut key_invuse: c_int = KEY_ENTER as c_int;
/// Strife: drop selected inventory item (default: Backspace).
#[no_mangle]
pub static mut key_invdrop: c_int = KEY_BACKSPACE as c_int;

//
// Mouse controls
//

/// Mouse button bound to fire (default: button 0).
#[no_mangle]
pub static mut mousebfire: c_int = 0;
/// Mouse button bound to strafe (default: button 1).
#[no_mangle]
pub static mut mousebstrafe: c_int = 1;
/// Mouse button bound to move forward (default: button 2).
#[no_mangle]
pub static mut mousebforward: c_int = 2;

/// Mouse button bound to jump (default: -1 = unbound; Hexen/Strife only).
#[no_mangle]
pub static mut mousebjump: c_int = -1;

/// Mouse button bound to strafe left (default: -1 = unbound).
#[no_mangle]
pub static mut mousebstrafeleft: c_int = -1;
/// Mouse button bound to strafe right (default: -1 = unbound).
#[no_mangle]
pub static mut mousebstraferight: c_int = -1;
/// Mouse button bound to move backward (default: -1 = unbound).
#[no_mangle]
pub static mut mousebbackward: c_int = -1;
/// Mouse button bound to "use" (default: -1 = unbound).
#[no_mangle]
pub static mut mousebuse: c_int = -1;

/// Mouse button bound to previous weapon (default: -1 = unbound).
#[no_mangle]
pub static mut mousebprevweapon: c_int = -1;
/// Mouse button bound to next weapon (default: -1 = unbound).
#[no_mangle]
pub static mut mousebnextweapon: c_int = -1;

/// Re-display last HUD message (default: Enter).
#[no_mangle]
pub static mut key_message_refresh: c_int = KEY_ENTER as c_int;
/// Pause the game (default: Pause).
#[no_mangle]
pub static mut key_pause: c_int = KEY_PAUSE as c_int;
/// Abort current demo playback (default: `q`).
#[no_mangle]
pub static mut key_demo_quit: c_int = b'q' as c_int;
/// Spy on next player in multiplayer (default: F12).
#[no_mangle]
pub static mut key_spy: c_int = KEY_F12 as c_int;

// Multiplayer chat keys:

/// Send chat to all players (default: `t`).
#[no_mangle]
pub static mut key_multi_msg: c_int = b't' as c_int;
/// Per-player private-chat keys (default: 0 / unbound for each).
/// Indexed by player number 0..7; populated by [`M_BindChatControls`].
#[no_mangle]
pub static mut key_multi_msgplayer: [c_int; 8] = [0; 8];

// Weapon selection keys:

/// Select weapon slot 1 (default: `1`).
#[no_mangle]
pub static mut key_weapon1: c_int = b'1' as c_int;
/// Select weapon slot 2 (default: `2`).
#[no_mangle]
pub static mut key_weapon2: c_int = b'2' as c_int;
/// Select weapon slot 3 (default: `3`).
#[no_mangle]
pub static mut key_weapon3: c_int = b'3' as c_int;
/// Select weapon slot 4 (default: `4`).
#[no_mangle]
pub static mut key_weapon4: c_int = b'4' as c_int;
/// Select weapon slot 5 (default: `5`).
#[no_mangle]
pub static mut key_weapon5: c_int = b'5' as c_int;
/// Select weapon slot 6 (default: `6`).
#[no_mangle]
pub static mut key_weapon6: c_int = b'6' as c_int;
/// Select weapon slot 7 (default: `7`).
#[no_mangle]
pub static mut key_weapon7: c_int = b'7' as c_int;
/// Select weapon slot 8 (default: `8`).
#[no_mangle]
pub static mut key_weapon8: c_int = b'8' as c_int;
/// Cycle to previous weapon (default: 0 = unbound).
#[no_mangle]
pub static mut key_prevweapon: c_int = 0;
/// Cycle to next weapon (default: 0 = unbound).
#[no_mangle]
pub static mut key_nextweapon: c_int = 0;

// Map control keys:

/// Automap: pan north (default: up arrow).
#[no_mangle]
pub static mut key_map_north: c_int = KEY_UPARROW as c_int;
/// Automap: pan south (default: down arrow).
#[no_mangle]
pub static mut key_map_south: c_int = KEY_DOWNARROW as c_int;
/// Automap: pan east (default: right arrow).
#[no_mangle]
pub static mut key_map_east: c_int = KEY_RIGHTARROW as c_int;
/// Automap: pan west (default: left arrow).
#[no_mangle]
pub static mut key_map_west: c_int = KEY_LEFTARROW as c_int;
/// Automap: zoom in (default: `=`).
#[no_mangle]
pub static mut key_map_zoomin: c_int = b'=' as c_int;
/// Automap: zoom out (default: `-`).
#[no_mangle]
pub static mut key_map_zoomout: c_int = b'-' as c_int;
/// Toggle automap on/off (default: Tab).
#[no_mangle]
pub static mut key_map_toggle: c_int = KEY_TAB as c_int;
/// Automap: fully zoom out / max zoom (default: `0`).
#[no_mangle]
pub static mut key_map_maxzoom: c_int = b'0' as c_int;
/// Automap: toggle follow-player mode (default: `f`).
#[no_mangle]
pub static mut key_map_follow: c_int = b'f' as c_int;
/// Automap: toggle grid overlay (default: `g`).
#[no_mangle]
pub static mut key_map_grid: c_int = b'g' as c_int;
/// Automap: drop a marker at the cursor (default: `m`).
#[no_mangle]
pub static mut key_map_mark: c_int = b'm' as c_int;
/// Automap: clear all markers (default: `c`).
#[no_mangle]
pub static mut key_map_clearmark: c_int = b'c' as c_int;

// menu keys:

/// Toggle the in-game menu (default: Escape).
#[no_mangle]
pub static mut key_menu_activate: c_int = KEY_ESCAPE as c_int;
/// Menu: move cursor up (default: up arrow).
#[no_mangle]
pub static mut key_menu_up: c_int = KEY_UPARROW as c_int;
/// Menu: move cursor down (default: down arrow).
#[no_mangle]
pub static mut key_menu_down: c_int = KEY_DOWNARROW as c_int;
/// Menu: decrease slider value / left (default: left arrow).
#[no_mangle]
pub static mut key_menu_left: c_int = KEY_LEFTARROW as c_int;
/// Menu: increase slider value / right (default: right arrow).
#[no_mangle]
pub static mut key_menu_right: c_int = KEY_RIGHTARROW as c_int;
/// Menu: go back one screen (default: Backspace).
#[no_mangle]
pub static mut key_menu_back: c_int = KEY_BACKSPACE as c_int;
/// Menu: select / activate item (default: Enter).
#[no_mangle]
pub static mut key_menu_forward: c_int = KEY_ENTER as c_int;
/// Menu: confirm `Yes` (default: `y`).
#[no_mangle]
pub static mut key_menu_confirm: c_int = b'y' as c_int;
/// Menu: confirm `No` / cancel (default: `n`).
#[no_mangle]
pub static mut key_menu_abort: c_int = b'n' as c_int;

/// Function shortcut: Help / read this (default: F1).
#[no_mangle]
pub static mut key_menu_help: c_int = KEY_F1 as c_int;
/// Function shortcut: Save Game menu (default: F2).
#[no_mangle]
pub static mut key_menu_save: c_int = KEY_F2 as c_int;
/// Function shortcut: Load Game menu (default: F3).
#[no_mangle]
pub static mut key_menu_load: c_int = KEY_F3 as c_int;
/// Function shortcut: Volume menu (default: F4).
#[no_mangle]
pub static mut key_menu_volume: c_int = KEY_F4 as c_int;
/// Function shortcut: toggle detail level (default: F5).
#[no_mangle]
pub static mut key_menu_detail: c_int = KEY_F5 as c_int;
/// Function shortcut: quick save (default: F6).
#[no_mangle]
pub static mut key_menu_qsave: c_int = KEY_F6 as c_int;
/// Function shortcut: end current game (default: F7).
#[no_mangle]
pub static mut key_menu_endgame: c_int = KEY_F7 as c_int;
/// Function shortcut: toggle HUD messages (default: F8).
#[no_mangle]
pub static mut key_menu_messages: c_int = KEY_F8 as c_int;
/// Function shortcut: quick load (default: F9).
#[no_mangle]
pub static mut key_menu_qload: c_int = KEY_F9 as c_int;
/// Function shortcut: quit to OS (default: F10).
#[no_mangle]
pub static mut key_menu_quit: c_int = KEY_F10 as c_int;
/// Function shortcut: cycle gamma correction (default: F11).
#[no_mangle]
pub static mut key_menu_gamma: c_int = KEY_F11 as c_int;

/// Increase screen size / shrink HUD borders (default: `=`).
#[no_mangle]
pub static mut key_menu_incscreen: c_int = KEY_EQUALS as c_int;
/// Decrease screen size / grow HUD borders (default: `-`).
#[no_mangle]
pub static mut key_menu_decscreen: c_int = KEY_MINUS as c_int;
/// Take a screenshot (default: 0 = unbound; Vanilla does not bind).
#[no_mangle]
pub static mut key_menu_screenshot: c_int = 0;

//
// Joystick controls
//

/// Joystick button bound to fire (default: button 0).
#[no_mangle]
pub static mut joybfire: c_int = 0;
/// Joystick button bound to strafe modifier (default: button 1).
#[no_mangle]
pub static mut joybstrafe: c_int = 1;
/// Joystick button bound to use (default: button 3).
#[no_mangle]
pub static mut joybuse: c_int = 3;
/// Joystick button bound to run / speed modifier (default: button 2).
#[no_mangle]
pub static mut joybspeed: c_int = 2;

/// Joystick button bound to strafe left (default: -1 = unbound).
#[no_mangle]
pub static mut joybstrafeleft: c_int = -1;
/// Joystick button bound to strafe right (default: -1 = unbound).
#[no_mangle]
pub static mut joybstraferight: c_int = -1;

/// Joystick button bound to jump (default: -1 = unbound; Hexen/Strife only).
#[no_mangle]
pub static mut joybjump: c_int = -1;

/// Joystick button bound to previous weapon (default: -1 = unbound).
#[no_mangle]
pub static mut joybprevweapon: c_int = -1;
/// Joystick button bound to next weapon (default: -1 = unbound).
#[no_mangle]
pub static mut joybnextweapon: c_int = -1;

/// Joystick button bound to activate menu (default: -1 = unbound).
#[no_mangle]
pub static mut joybmenu: c_int = -1;

// Control whether if a mouse button is double clicked, it acts like
// "use" has been pressed

/// If non-zero, a mouse double-click acts as a "use" press (default: 1).
#[no_mangle]
pub static mut dclick_use: c_int = 1;

extern "C" {
    /// `m_config` callback that registers a named variable against a
    /// memory location. Wired through the `M_Bind*` functions below.
    fn M_BindVariable(name: *mut c_char, variable: *mut c_void);
}

use crate::c_write;

//
// Bind all of the common controls used by Doom and all other games.
//

/// Register the keyboard / mouse / joystick bindings shared by every game.
///
/// Called once during startup. Each `M_BindVariable` call associates a
/// `.cfg` variable name (matching the entry in `m_config`'s
/// `doom_defaults` / `extra_defaults` tables) with the address of the
/// corresponding `static mut` in this module, so loading a config file
/// updates the live binding.
#[no_mangle]
pub extern "C" fn M_BindBaseControls() {
    unsafe {
        M_BindVariable(
            cstr(b"key_right\0"),
            &raw mut key_right as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_left\0"),
            &raw mut key_left as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_up\0"),
            &raw mut key_up as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_down\0"),
            &raw mut key_down as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_strafeleft\0"),
            &raw mut key_strafeleft as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_straferight\0"),
            &raw mut key_straferight as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_fire\0"),
            &raw mut key_fire as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_use\0"),
            &raw mut key_use as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_strafe\0"),
            &raw mut key_strafe as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_speed\0"),
            &raw mut key_speed as *mut c_int as *mut c_void,
        );

        M_BindVariable(
            cstr(b"mouseb_fire\0"),
            &raw mut mousebfire as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"mouseb_strafe\0"),
            &raw mut mousebstrafe as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"mouseb_forward\0"),
            &raw mut mousebforward as *mut c_int as *mut c_void,
        );

        M_BindVariable(
            cstr(b"joyb_fire\0"),
            &raw mut joybfire as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"joyb_strafe\0"),
            &raw mut joybstrafe as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"joyb_use\0"),
            &raw mut joybuse as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"joyb_speed\0"),
            &raw mut joybspeed as *mut c_int as *mut c_void,
        );

        M_BindVariable(
            cstr(b"joyb_menu_activate\0"),
            &raw mut joybmenu as *mut c_int as *mut c_void,
        );

        // Extra controls that are not in the Vanilla versions:

        M_BindVariable(
            cstr(b"joyb_strafeleft\0"),
            &raw mut joybstrafeleft as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"joyb_straferight\0"),
            &raw mut joybstraferight as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"mouseb_strafeleft\0"),
            &raw mut mousebstrafeleft as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"mouseb_straferight\0"),
            &raw mut mousebstraferight as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"mouseb_use\0"),
            &raw mut mousebuse as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"mouseb_backward\0"),
            &raw mut mousebbackward as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"dclick_use\0"),
            &raw mut dclick_use as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_pause\0"),
            &raw mut key_pause as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_message_refresh\0"),
            &raw mut key_message_refresh as *mut c_int as *mut c_void,
        );
    }
}

/// Register the Heretic-specific keyboard bindings (fly / look / inventory
/// keys).
#[no_mangle]
pub extern "C" fn M_BindHereticControls() {
    unsafe {
        M_BindVariable(
            cstr(b"key_flyup\0"),
            &raw mut key_flyup as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_flydown\0"),
            &raw mut key_flydown as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_flycenter\0"),
            &raw mut key_flycenter as *mut c_int as *mut c_void,
        );

        M_BindVariable(
            cstr(b"key_lookup\0"),
            &raw mut key_lookup as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_lookdown\0"),
            &raw mut key_lookdown as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_lookcenter\0"),
            &raw mut key_lookcenter as *mut c_int as *mut c_void,
        );

        M_BindVariable(
            cstr(b"key_invleft\0"),
            &raw mut key_invleft as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_invright\0"),
            &raw mut key_invright as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_useartifact\0"),
            &raw mut key_useartifact as *mut c_int as *mut c_void,
        );
    }
}

/// Register the Hexen-specific bindings: jump on key / mouse / joystick
/// plus the eight artifact hotkeys.
#[no_mangle]
pub extern "C" fn M_BindHexenControls() {
    unsafe {
        M_BindVariable(
            cstr(b"key_jump\0"),
            &raw mut key_jump as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"mouseb_jump\0"),
            &raw mut mousebjump as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"joyb_jump\0"),
            &raw mut joybjump as *mut c_int as *mut c_void,
        );

        M_BindVariable(
            cstr(b"key_arti_all\0"),
            &raw mut key_arti_all as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_arti_health\0"),
            &raw mut key_arti_health as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_arti_poisonbag\0"),
            &raw mut key_arti_poisonbag as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_arti_blastradius\0"),
            &raw mut key_arti_blastradius as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_arti_teleport\0"),
            &raw mut key_arti_teleport as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_arti_teleportother\0"),
            &raw mut key_arti_teleportother as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_arti_egg\0"),
            &raw mut key_arti_egg as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_arti_invulnerability\0"),
            &raw mut key_arti_invulnerability as *mut c_int as *mut c_void,
        );
    }
}

/// Register the Strife-specific bindings.
///
/// Mutates several shared bindings to their Strife defaults before
/// registration: `key_message_refresh = '/'`, `key_jump = 'a'`,
/// `key_lookup = PGUP`, `key_lookdown = PGDN`, `key_invleft = INS`,
/// `key_invright = DEL`. Mirrors C `M_BindStrifeControls`.
#[no_mangle]
pub extern "C" fn M_BindStrifeControls() {
    unsafe {
        // These are shared with all games, but have different defaults:
        key_message_refresh = b'/' as c_int;

        // These keys are shared with Heretic/Hexen but have different defaults:
        key_jump = b'a' as c_int;
        key_lookup = KEY_PGUP as c_int;
        key_lookdown = KEY_PGDN as c_int;
        key_invleft = KEY_INS as c_int;
        key_invright = KEY_DEL as c_int;

        M_BindVariable(
            cstr(b"key_jump\0"),
            &raw mut key_jump as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_lookUp\0"),
            &raw mut key_lookup as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_lookDown\0"),
            &raw mut key_lookdown as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_invLeft\0"),
            &raw mut key_invleft as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_invRight\0"),
            &raw mut key_invright as *mut c_int as *mut c_void,
        );

        // Custom Strife-only Keys:
        M_BindVariable(
            cstr(b"key_useHealth\0"),
            &raw mut key_usehealth as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_invquery\0"),
            &raw mut key_invquery as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_mission\0"),
            &raw mut key_mission as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_invPop\0"),
            &raw mut key_invpop as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_invKey\0"),
            &raw mut key_invkey as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_invHome\0"),
            &raw mut key_invhome as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_invEnd\0"),
            &raw mut key_invend as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_invUse\0"),
            &raw mut key_invuse as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_invDrop\0"),
            &raw mut key_invdrop as *mut c_int as *mut c_void,
        );

        // Strife also supports jump on mouse and joystick, and in the exact same
        // manner as Hexen!
        M_BindVariable(
            cstr(b"mouseb_jump\0"),
            &raw mut mousebjump as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"joyb_jump\0"),
            &raw mut joybjump as *mut c_int as *mut c_void,
        );
    }
}

/// Register the weapon-selection key bindings (`key_weapon1`..`8`,
/// `key_prevweapon`, `key_nextweapon`) plus the corresponding mouse and
/// joystick prev/next bindings.
#[no_mangle]
pub extern "C" fn M_BindWeaponControls() {
    unsafe {
        M_BindVariable(
            cstr(b"key_weapon1\0"),
            &raw mut key_weapon1 as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_weapon2\0"),
            &raw mut key_weapon2 as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_weapon3\0"),
            &raw mut key_weapon3 as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_weapon4\0"),
            &raw mut key_weapon4 as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_weapon5\0"),
            &raw mut key_weapon5 as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_weapon6\0"),
            &raw mut key_weapon6 as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_weapon7\0"),
            &raw mut key_weapon7 as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_weapon8\0"),
            &raw mut key_weapon8 as *mut c_int as *mut c_void,
        );

        M_BindVariable(
            cstr(b"key_prevweapon\0"),
            &raw mut key_prevweapon as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_nextweapon\0"),
            &raw mut key_nextweapon as *mut c_int as *mut c_void,
        );

        M_BindVariable(
            cstr(b"joyb_prevweapon\0"),
            &raw mut joybprevweapon as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"joyb_nextweapon\0"),
            &raw mut joybnextweapon as *mut c_int as *mut c_void,
        );

        M_BindVariable(
            cstr(b"mouseb_prevweapon\0"),
            &raw mut mousebprevweapon as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"mouseb_nextweapon\0"),
            &raw mut mousebnextweapon as *mut c_int as *mut c_void,
        );
    }
}

/// Register the automap movement / zoom / overlay bindings.
#[no_mangle]
pub extern "C" fn M_BindMapControls() {
    unsafe {
        M_BindVariable(
            cstr(b"key_map_north\0"),
            &raw mut key_map_north as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_map_south\0"),
            &raw mut key_map_south as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_map_east\0"),
            &raw mut key_map_east as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_map_west\0"),
            &raw mut key_map_west as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_map_zoomin\0"),
            &raw mut key_map_zoomin as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_map_zoomout\0"),
            &raw mut key_map_zoomout as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_map_toggle\0"),
            &raw mut key_map_toggle as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_map_maxzoom\0"),
            &raw mut key_map_maxzoom as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_map_follow\0"),
            &raw mut key_map_follow as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_map_grid\0"),
            &raw mut key_map_grid as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_map_mark\0"),
            &raw mut key_map_mark as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_map_clearmark\0"),
            &raw mut key_map_clearmark as *mut c_int as *mut c_void,
        );
    }
}

/// Register the in-game menu navigation keys plus the F1..F12 function
/// shortcuts (help / save / load / volume / detail / qsave / endgame /
/// messages / qload / quit / gamma) and the increment/decrement screen-
/// size and screenshot bindings.
#[no_mangle]
pub extern "C" fn M_BindMenuControls() {
    unsafe {
        M_BindVariable(
            cstr(b"key_menu_activate\0"),
            &raw mut key_menu_activate as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_menu_up\0"),
            &raw mut key_menu_up as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_menu_down\0"),
            &raw mut key_menu_down as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_menu_left\0"),
            &raw mut key_menu_left as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_menu_right\0"),
            &raw mut key_menu_right as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_menu_back\0"),
            &raw mut key_menu_back as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_menu_forward\0"),
            &raw mut key_menu_forward as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_menu_confirm\0"),
            &raw mut key_menu_confirm as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_menu_abort\0"),
            &raw mut key_menu_abort as *mut c_int as *mut c_void,
        );

        M_BindVariable(
            cstr(b"key_menu_help\0"),
            &raw mut key_menu_help as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_menu_save\0"),
            &raw mut key_menu_save as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_menu_load\0"),
            &raw mut key_menu_load as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_menu_volume\0"),
            &raw mut key_menu_volume as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_menu_detail\0"),
            &raw mut key_menu_detail as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_menu_qsave\0"),
            &raw mut key_menu_qsave as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_menu_endgame\0"),
            &raw mut key_menu_endgame as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_menu_messages\0"),
            &raw mut key_menu_messages as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_menu_qload\0"),
            &raw mut key_menu_qload as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_menu_quit\0"),
            &raw mut key_menu_quit as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_menu_gamma\0"),
            &raw mut key_menu_gamma as *mut c_int as *mut c_void,
        );

        M_BindVariable(
            cstr(b"key_menu_incscreen\0"),
            &raw mut key_menu_incscreen as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_menu_decscreen\0"),
            &raw mut key_menu_decscreen as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_menu_screenshot\0"),
            &raw mut key_menu_screenshot as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_demo_quit\0"),
            &raw mut key_demo_quit as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            cstr(b"key_spy\0"),
            &raw mut key_spy as *mut c_int as *mut c_void,
        );
    }
}

/// Register multiplayer chat bindings: the broadcast `key_multi_msg`
/// plus one `key_multi_msgplayerN` binding per active player.
///
/// `num_players` should be at most 8, matching the size of
/// [`key_multi_msgplayer`]. Each per-player name is formatted into a local
/// 32-byte buffer with [`c_write!`] before being passed to
/// `M_BindVariable`; this replaces the C `M_snprintf` call.
#[no_mangle]
pub extern "C" fn M_BindChatControls(num_players: c_uint) {
    unsafe {
        let mut name: [c_char; 32] = [0; 32];
        let mut i: c_uint = 0;

        M_BindVariable(
            cstr(b"key_multi_msg\0"),
            &raw mut key_multi_msg as *mut c_int as *mut c_void,
        );

        while i < num_players {
            c_write!(name, "key_multi_msgplayer{}", i + 1);
            M_BindVariable(
                name.as_ptr() as *mut c_char,
                &mut key_multi_msgplayer[i as usize] as *mut c_int as *mut c_void,
            );
            i += 1;
        }
    }
}

//
// Apply custom patches to the default values depending on the
// platform we are running on.
//

/// Apply per-platform overrides to default bindings.
///
/// Currently a no-op on every platform - mirrors the C source. Exists as
/// an extension point for porters who want to retune defaults.
#[no_mangle]
pub extern "C" fn M_ApplyPlatformDefaults() {
    // no-op. Add your platform-specific patches here.
}

/// Cast a `'static` NUL-terminated byte slice to a `*mut c_char` for use
/// as a configuration variable name.
///
/// The slice is conceptually `const`, but `M_BindVariable` takes
/// `*mut c_char` (matching the C signature), so the cast strips the
/// const-ness. The pointee is never mutated by the binding code; callers
/// must continue to honour that invariant.
const fn cstr(bytes: &[u8]) -> *mut c_char {
    bytes.as_ptr() as *mut c_char
}
