//! Rust port of vendor/doomgeneric/m_controls.c.
//!
//! Default keyboard / mouse / joystick bindings for every game in the
//! chocolate-doom family (Doom, Heretic, Hexen, Strife), plus the
//! `M_Bind*` registration helpers that wire each binding into the
//! `m_config` variable table so they can be read or written through
//! `default.cfg`.
//!
//! All bindings are `pub static mut` so the rest of the engine can read
//! and mutate them directly. Each
//! `M_Bind<Game>Controls` function calls into `M_BindVariable` (from
//! `m_config`) for the keys relevant to that game's binding subset.

#![allow(non_upper_case_globals, non_snake_case)]

use std::ffi::c_void;

use crate::engine::interface::keys::{
    KEY_BACKSPACE, KEY_DEL, KEY_DOWNARROW, KEY_END, KEY_ENTER, KEY_EQUALS, KEY_ESCAPE, KEY_F1,
    KEY_F10, KEY_F11, KEY_F12, KEY_F2, KEY_F3, KEY_F4, KEY_F5, KEY_F6, KEY_F7, KEY_F8, KEY_F9,
    KEY_HOME, KEY_INS, KEY_LEFTARROW, KEY_MINUS, KEY_PAUSE, KEY_PGDN, KEY_PGUP, KEY_RALT,
    KEY_RIGHTARROW, KEY_RSHIFT, KEY_STRAFE_L, KEY_STRAFE_R, KEY_TAB, KEY_UPARROW, KEY_USE,
};

//
// Keyboard controls
//

/// Turn right (default: right arrow).
pub static mut key_right: i32 = KEY_RIGHTARROW as i32;
/// Turn left (default: left arrow).
pub static mut key_left: i32 = KEY_LEFTARROW as i32;
/// Move forward (default: up arrow).
pub static mut key_up: i32 = KEY_UPARROW as i32;
/// Move backward (default: down arrow).
pub static mut key_down: i32 = KEY_DOWNARROW as i32;
/// Strafe left (default: comma `KEY_STRAFE_L`).
pub static mut key_strafeleft: i32 = KEY_STRAFE_L as i32;
/// Strafe right (default: period `KEY_STRAFE_R`).
pub static mut key_straferight: i32 = KEY_STRAFE_R as i32;
/// Fire current weapon (default: `KEY_FIRE`, the right Ctrl).
pub static mut key_fire: i32 = crate::engine::interface::keys::KEY_FIRE as i32;
/// Use / open / activate trigger (default: space `KEY_USE`).
pub static mut key_use: i32 = KEY_USE as i32;
/// Hold to convert turn into strafe (default: right Alt).
pub static mut key_strafe: i32 = KEY_RALT as i32;
/// Hold to run (default: right Shift).
pub static mut key_speed: i32 = KEY_RSHIFT as i32;

//
// Heretic keyboard controls
//

/// Heretic: fly up (default: Page Up).
pub static mut key_flyup: i32 = KEY_PGUP as i32;
/// Heretic: fly down (default: Insert).
pub static mut key_flydown: i32 = KEY_INS as i32;
/// Heretic: stop flying / centre vertically (default: Home).
pub static mut key_flycenter: i32 = KEY_HOME as i32;

/// Heretic: look up (default: Page Down).
pub static mut key_lookup: i32 = KEY_PGDN as i32;
/// Heretic: look down (default: Delete).
pub static mut key_lookdown: i32 = KEY_DEL as i32;
/// Heretic: centre vertical look (default: End).
pub static mut key_lookcenter: i32 = KEY_END as i32;

/// Heretic/Strife: cycle inventory left (default: `[`; Strife uses Insert).
pub static mut key_invleft: i32 = b'[' as i32;
/// Heretic/Strife: cycle inventory right (default: `]`; Strife uses Delete).
pub static mut key_invright: i32 = b']' as i32;
/// Heretic: use selected inventory artifact (default: Enter).
pub static mut key_useartifact: i32 = KEY_ENTER as i32;

//
// Hexen key controls
//

/// Hexen/Strife: jump (default: `/`; Strife rebinds to `a`).
pub static mut key_jump: i32 = b'/' as i32;

/// Hexen: use all artifacts of selected kind (default: Backspace).
pub static mut key_arti_all: i32 = KEY_BACKSPACE as i32;
/// Hexen: use Quartz Flask / Mystic Urn (default: `\\`).
pub static mut key_arti_health: i32 = b'\\' as i32;
/// Hexen: use poison bag artifact (default: `0`).
pub static mut key_arti_poisonbag: i32 = b'0' as i32;
/// Hexen: use disc of repulsion (default: `9`).
pub static mut key_arti_blastradius: i32 = b'9' as i32;
/// Hexen: use teleport artifact (default: `8`).
pub static mut key_arti_teleport: i32 = b'8' as i32;
/// Hexen: use banishment device (default: `7`).
pub static mut key_arti_teleportother: i32 = b'7' as i32;
/// Hexen: use porkalator (default: `6`).
pub static mut key_arti_egg: i32 = b'6' as i32;
/// Hexen: use icon of the defender (default: `5`).
pub static mut key_arti_invulnerability: i32 = b'5' as i32;

//
// Strife key controls
//

/// Strife: use a medkit from inventory (default: `h`).
pub static mut key_usehealth: i32 = b'h' as i32;
/// Strife: query selected NPC / object (default: `q`).
pub static mut key_invquery: i32 = b'q' as i32;
/// Strife: open mission log (default: `w`).
pub static mut key_mission: i32 = b'w' as i32;
/// Strife: open the inventory popup (default: `z`).
pub static mut key_invpop: i32 = b'z' as i32;
/// Strife: open the keys popup (default: `k`).
pub static mut key_invkey: i32 = b'k' as i32;
/// Strife: cycle to the first inventory item (default: Home).
pub static mut key_invhome: i32 = KEY_HOME as i32;
/// Strife: cycle to the last inventory item (default: End).
pub static mut key_invend: i32 = KEY_END as i32;
/// Strife: use selected inventory item (default: Enter).
pub static mut key_invuse: i32 = KEY_ENTER as i32;
/// Strife: drop selected inventory item (default: Backspace).
pub static mut key_invdrop: i32 = KEY_BACKSPACE as i32;

//
// Mouse controls
//

/// Mouse button bound to fire (default: button 0).
pub static mut mousebfire: i32 = 0;
/// Mouse button bound to strafe (default: button 1).
pub static mut mousebstrafe: i32 = 1;
/// Mouse button bound to move forward (default: button 2).
pub static mut mousebforward: i32 = 2;

/// Mouse button bound to jump (default: -1 = unbound; Hexen/Strife only).
pub static mut mousebjump: i32 = -1;

/// Mouse button bound to strafe left (default: -1 = unbound).
pub static mut mousebstrafeleft: i32 = -1;
/// Mouse button bound to strafe right (default: -1 = unbound).
pub static mut mousebstraferight: i32 = -1;
/// Mouse button bound to move backward (default: -1 = unbound).
pub static mut mousebbackward: i32 = -1;
/// Mouse button bound to "use" (default: -1 = unbound).
pub static mut mousebuse: i32 = -1;

/// Mouse button bound to previous weapon (default: -1 = unbound).
pub static mut mousebprevweapon: i32 = -1;
/// Mouse button bound to next weapon (default: -1 = unbound).
pub static mut mousebnextweapon: i32 = -1;

/// Re-display last HUD message (default: Enter).
pub static mut key_message_refresh: i32 = KEY_ENTER as i32;
/// Pause the game (default: Pause).
pub static mut key_pause: i32 = KEY_PAUSE as i32;
/// Abort current demo playback (default: `q`).
pub static mut key_demo_quit: i32 = b'q' as i32;
/// Spy on next player in multiplayer (default: F12).
pub static mut key_spy: i32 = KEY_F12 as i32;

// Multiplayer chat keys:

/// Send chat to all players (default: `t`).
pub static mut key_multi_msg: i32 = b't' as i32;
/// Per-player private-chat keys (default: 0 / unbound for each).
/// Indexed by player number 0..7; populated by [`M_BindChatControls`].
pub static mut key_multi_msgplayer: [i32; 8] = [0; 8];

// Weapon selection keys:

/// Select weapon slot 1 (default: `1`).
pub static mut key_weapon1: i32 = b'1' as i32;
/// Select weapon slot 2 (default: `2`).
pub static mut key_weapon2: i32 = b'2' as i32;
/// Select weapon slot 3 (default: `3`).
pub static mut key_weapon3: i32 = b'3' as i32;
/// Select weapon slot 4 (default: `4`).
pub static mut key_weapon4: i32 = b'4' as i32;
/// Select weapon slot 5 (default: `5`).
pub static mut key_weapon5: i32 = b'5' as i32;
/// Select weapon slot 6 (default: `6`).
pub static mut key_weapon6: i32 = b'6' as i32;
/// Select weapon slot 7 (default: `7`).
pub static mut key_weapon7: i32 = b'7' as i32;
/// Select weapon slot 8 (default: `8`).
pub static mut key_weapon8: i32 = b'8' as i32;
/// Cycle to previous weapon (default: 0 = unbound).
pub static mut key_prevweapon: i32 = 0;
/// Cycle to next weapon (default: 0 = unbound).
pub static mut key_nextweapon: i32 = 0;

// Map control keys:

/// Automap: pan north (default: up arrow).
pub static mut key_map_north: i32 = KEY_UPARROW as i32;
/// Automap: pan south (default: down arrow).
pub static mut key_map_south: i32 = KEY_DOWNARROW as i32;
/// Automap: pan east (default: right arrow).
pub static mut key_map_east: i32 = KEY_RIGHTARROW as i32;
/// Automap: pan west (default: left arrow).
pub static mut key_map_west: i32 = KEY_LEFTARROW as i32;
/// Automap: zoom in (default: `=`).
pub static mut key_map_zoomin: i32 = b'=' as i32;
/// Automap: zoom out (default: `-`).
pub static mut key_map_zoomout: i32 = b'-' as i32;
/// Toggle automap on/off (default: Tab).
pub static mut key_map_toggle: i32 = KEY_TAB as i32;
/// Automap: fully zoom out / max zoom (default: `0`).
pub static mut key_map_maxzoom: i32 = b'0' as i32;
/// Automap: toggle follow-player mode (default: `f`).
pub static mut key_map_follow: i32 = b'f' as i32;
/// Automap: toggle grid overlay (default: `g`).
pub static mut key_map_grid: i32 = b'g' as i32;
/// Automap: drop a marker at the cursor (default: `m`).
pub static mut key_map_mark: i32 = b'm' as i32;
/// Automap: clear all markers (default: `c`).
pub static mut key_map_clearmark: i32 = b'c' as i32;

// menu keys:

/// Toggle the in-game menu (default: Escape).
pub static mut key_menu_activate: i32 = KEY_ESCAPE as i32;
/// Menu: move cursor up (default: up arrow).
pub static mut key_menu_up: i32 = KEY_UPARROW as i32;
/// Menu: move cursor down (default: down arrow).
pub static mut key_menu_down: i32 = KEY_DOWNARROW as i32;
/// Menu: decrease slider value / left (default: left arrow).
pub static mut key_menu_left: i32 = KEY_LEFTARROW as i32;
/// Menu: increase slider value / right (default: right arrow).
pub static mut key_menu_right: i32 = KEY_RIGHTARROW as i32;
/// Menu: go back one screen (default: Backspace).
pub static mut key_menu_back: i32 = KEY_BACKSPACE as i32;
/// Menu: select / activate item (default: Enter).
pub static mut key_menu_forward: i32 = KEY_ENTER as i32;
/// Menu: confirm `Yes` (default: `y`).
pub static mut key_menu_confirm: i32 = b'y' as i32;
/// Menu: confirm `No` / cancel (default: `n`).
pub static mut key_menu_abort: i32 = b'n' as i32;

/// Function shortcut: Help / read this (default: F1).
pub static mut key_menu_help: i32 = KEY_F1 as i32;
/// Function shortcut: Save Game menu (default: F2).
pub static mut key_menu_save: i32 = KEY_F2 as i32;
/// Function shortcut: Load Game menu (default: F3).
pub static mut key_menu_load: i32 = KEY_F3 as i32;
/// Function shortcut: Volume menu (default: F4).
pub static mut key_menu_volume: i32 = KEY_F4 as i32;
/// Function shortcut: toggle detail level (default: F5).
pub static mut key_menu_detail: i32 = KEY_F5 as i32;
/// Function shortcut: quick save (default: F6).
pub static mut key_menu_qsave: i32 = KEY_F6 as i32;
/// Function shortcut: end current game (default: F7).
pub static mut key_menu_endgame: i32 = KEY_F7 as i32;
/// Function shortcut: toggle HUD messages (default: F8).
pub static mut key_menu_messages: i32 = KEY_F8 as i32;
/// Function shortcut: quick load (default: F9).
pub static mut key_menu_qload: i32 = KEY_F9 as i32;
/// Function shortcut: quit to OS (default: F10).
pub static mut key_menu_quit: i32 = KEY_F10 as i32;
/// Function shortcut: cycle gamma correction (default: F11).
pub static mut key_menu_gamma: i32 = KEY_F11 as i32;

/// Increase screen size / shrink HUD borders (default: `=`).
pub static mut key_menu_incscreen: i32 = KEY_EQUALS as i32;
/// Decrease screen size / grow HUD borders (default: `-`).
pub static mut key_menu_decscreen: i32 = KEY_MINUS as i32;
/// Take a screenshot (default: 0 = unbound; Vanilla does not bind).
pub static mut key_menu_screenshot: i32 = 0;

//
// Joystick controls
//

/// Joystick button bound to fire (default: button 0).
pub static mut joybfire: i32 = 0;
/// Joystick button bound to strafe modifier (default: button 1).
pub static mut joybstrafe: i32 = 1;
/// Joystick button bound to use (default: button 3).
pub static mut joybuse: i32 = 3;
/// Joystick button bound to run / speed modifier (default: button 2).
pub static mut joybspeed: i32 = 2;

/// Joystick button bound to strafe left (default: -1 = unbound).
pub static mut joybstrafeleft: i32 = -1;
/// Joystick button bound to strafe right (default: -1 = unbound).
pub static mut joybstraferight: i32 = -1;

/// Joystick button bound to jump (default: -1 = unbound; Hexen/Strife only).
pub static mut joybjump: i32 = -1;

/// Joystick button bound to previous weapon (default: -1 = unbound).
pub static mut joybprevweapon: i32 = -1;
/// Joystick button bound to next weapon (default: -1 = unbound).
pub static mut joybnextweapon: i32 = -1;

/// Joystick button bound to activate menu (default: -1 = unbound).
pub static mut joybmenu: i32 = -1;

// Control whether if a mouse button is double clicked, it acts like
// "use" has been pressed

/// If non-zero, a mouse double-click acts as a "use" press (default: 1).
pub static mut dclick_use: i32 = 1;

/// `m_config` callback that registers a named variable against a
/// memory location. Wired through the `M_Bind*` functions below.
use crate::engine::misc::config::M_BindVariable;

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
pub fn M_BindBaseControls() {
    M_BindVariable("key_right", &raw mut key_right as *mut c_void);
    M_BindVariable("key_left", &raw mut key_left as *mut c_void);
    M_BindVariable("key_up", &raw mut key_up as *mut c_void);
    M_BindVariable("key_down", &raw mut key_down as *mut c_void);
    M_BindVariable("key_strafeleft", &raw mut key_strafeleft as *mut c_void);
    M_BindVariable("key_straferight", &raw mut key_straferight as *mut c_void);
    M_BindVariable("key_fire", &raw mut key_fire as *mut c_void);
    M_BindVariable("key_use", &raw mut key_use as *mut c_void);
    M_BindVariable("key_strafe", &raw mut key_strafe as *mut c_void);
    M_BindVariable("key_speed", &raw mut key_speed as *mut c_void);

    M_BindVariable("mouseb_fire", &raw mut mousebfire as *mut c_void);
    M_BindVariable("mouseb_strafe", &raw mut mousebstrafe as *mut c_void);
    M_BindVariable("mouseb_forward", &raw mut mousebforward as *mut c_void);

    M_BindVariable("joyb_fire", &raw mut joybfire as *mut c_void);
    M_BindVariable("joyb_strafe", &raw mut joybstrafe as *mut c_void);
    M_BindVariable("joyb_use", &raw mut joybuse as *mut c_void);
    M_BindVariable("joyb_speed", &raw mut joybspeed as *mut c_void);

    M_BindVariable("joyb_menu_activate", &raw mut joybmenu as *mut c_void);

    // Extra controls that are not in the Vanilla versions:

    M_BindVariable("joyb_strafeleft", &raw mut joybstrafeleft as *mut c_void);
    M_BindVariable("joyb_straferight", &raw mut joybstraferight as *mut c_void);
    M_BindVariable(
        "mouseb_strafeleft",
        &raw mut mousebstrafeleft as *mut c_void,
    );
    M_BindVariable(
        "mouseb_straferight",
        &raw mut mousebstraferight as *mut c_void,
    );
    M_BindVariable("mouseb_use", &raw mut mousebuse as *mut c_void);
    M_BindVariable("mouseb_backward", &raw mut mousebbackward as *mut c_void);
    M_BindVariable("dclick_use", &raw mut dclick_use as *mut c_void);
    M_BindVariable("key_pause", &raw mut key_pause as *mut c_void);
    M_BindVariable(
        "key_message_refresh",
        &raw mut key_message_refresh as *mut c_void,
    );
}

/// Register the Heretic-specific keyboard bindings (fly / look / inventory
/// keys).
pub fn M_BindHereticControls() {
    M_BindVariable("key_flyup", &raw mut key_flyup as *mut c_void);
    M_BindVariable("key_flydown", &raw mut key_flydown as *mut c_void);
    M_BindVariable("key_flycenter", &raw mut key_flycenter as *mut c_void);

    M_BindVariable("key_lookup", &raw mut key_lookup as *mut c_void);
    M_BindVariable("key_lookdown", &raw mut key_lookdown as *mut c_void);
    M_BindVariable("key_lookcenter", &raw mut key_lookcenter as *mut c_void);

    M_BindVariable("key_invleft", &raw mut key_invleft as *mut c_void);
    M_BindVariable("key_invright", &raw mut key_invright as *mut c_void);
    M_BindVariable("key_useartifact", &raw mut key_useartifact as *mut c_void);
}

/// Register the Hexen-specific bindings: jump on key / mouse / joystick
/// plus the eight artifact hotkeys.
pub fn M_BindHexenControls() {
    M_BindVariable("key_jump", &raw mut key_jump as *mut c_void);
    M_BindVariable("mouseb_jump", &raw mut mousebjump as *mut c_void);
    M_BindVariable("joyb_jump", &raw mut joybjump as *mut c_void);

    M_BindVariable("key_arti_all", &raw mut key_arti_all as *mut c_void);
    M_BindVariable("key_arti_health", &raw mut key_arti_health as *mut c_void);
    M_BindVariable(
        "key_arti_poisonbag",
        &raw mut key_arti_poisonbag as *mut c_void,
    );
    M_BindVariable(
        "key_arti_blastradius",
        &raw mut key_arti_blastradius as *mut c_void,
    );
    M_BindVariable(
        "key_arti_teleport",
        &raw mut key_arti_teleport as *mut c_void,
    );
    M_BindVariable(
        "key_arti_teleportother",
        &raw mut key_arti_teleportother as *mut c_void,
    );
    M_BindVariable("key_arti_egg", &raw mut key_arti_egg as *mut c_void);
    M_BindVariable(
        "key_arti_invulnerability",
        &raw mut key_arti_invulnerability as *mut c_void,
    );
}

/// Register the Strife-specific bindings.
///
/// Mutates several shared bindings to their Strife defaults before
/// registration: `key_message_refresh = '/'`, `key_jump = 'a'`,
/// `key_lookup = PGUP`, `key_lookdown = PGDN`, `key_invleft = INS`,
/// `key_invright = DEL`. Mirrors C `M_BindStrifeControls`.
pub fn M_BindStrifeControls() {
    unsafe {
        // These are shared with all games, but have different defaults:
        key_message_refresh = b'/' as i32;

        // These keys are shared with Heretic/Hexen but have different defaults:
        key_jump = b'a' as i32;
        key_lookup = KEY_PGUP as i32;
        key_lookdown = KEY_PGDN as i32;
        key_invleft = KEY_INS as i32;
        key_invright = KEY_DEL as i32;

        M_BindVariable("key_jump", &raw mut key_jump as *mut c_void);
        M_BindVariable("key_lookUp", &raw mut key_lookup as *mut c_void);
        M_BindVariable("key_lookDown", &raw mut key_lookdown as *mut c_void);
        M_BindVariable("key_invLeft", &raw mut key_invleft as *mut c_void);
        M_BindVariable("key_invRight", &raw mut key_invright as *mut c_void);

        // Custom Strife-only Keys:
        M_BindVariable("key_useHealth", &raw mut key_usehealth as *mut c_void);
        M_BindVariable("key_invquery", &raw mut key_invquery as *mut c_void);
        M_BindVariable("key_mission", &raw mut key_mission as *mut c_void);
        M_BindVariable("key_invPop", &raw mut key_invpop as *mut c_void);
        M_BindVariable("key_invKey", &raw mut key_invkey as *mut c_void);
        M_BindVariable("key_invHome", &raw mut key_invhome as *mut c_void);
        M_BindVariable("key_invEnd", &raw mut key_invend as *mut c_void);
        M_BindVariable("key_invUse", &raw mut key_invuse as *mut c_void);
        M_BindVariable("key_invDrop", &raw mut key_invdrop as *mut c_void);

        // Strife also supports jump on mouse and joystick, and in the exact same
        // manner as Hexen!
        M_BindVariable("mouseb_jump", &raw mut mousebjump as *mut c_void);
        M_BindVariable("joyb_jump", &raw mut joybjump as *mut c_void);
    }
}

/// Register the weapon-selection key bindings (`key_weapon1`..`8`,
/// `key_prevweapon`, `key_nextweapon`) plus the corresponding mouse and
/// joystick prev/next bindings.
pub fn M_BindWeaponControls() {
    M_BindVariable("key_weapon1", &raw mut key_weapon1 as *mut c_void);
    M_BindVariable("key_weapon2", &raw mut key_weapon2 as *mut c_void);
    M_BindVariable("key_weapon3", &raw mut key_weapon3 as *mut c_void);
    M_BindVariable("key_weapon4", &raw mut key_weapon4 as *mut c_void);
    M_BindVariable("key_weapon5", &raw mut key_weapon5 as *mut c_void);
    M_BindVariable("key_weapon6", &raw mut key_weapon6 as *mut c_void);
    M_BindVariable("key_weapon7", &raw mut key_weapon7 as *mut c_void);
    M_BindVariable("key_weapon8", &raw mut key_weapon8 as *mut c_void);

    M_BindVariable("key_prevweapon", &raw mut key_prevweapon as *mut c_void);
    M_BindVariable("key_nextweapon", &raw mut key_nextweapon as *mut c_void);

    M_BindVariable("joyb_prevweapon", &raw mut joybprevweapon as *mut c_void);
    M_BindVariable("joyb_nextweapon", &raw mut joybnextweapon as *mut c_void);

    M_BindVariable(
        "mouseb_prevweapon",
        &raw mut mousebprevweapon as *mut c_void,
    );
    M_BindVariable(
        "mouseb_nextweapon",
        &raw mut mousebnextweapon as *mut c_void,
    );
}

/// Register the automap movement / zoom / overlay bindings.
pub fn M_BindMapControls() {
    M_BindVariable("key_map_north", &raw mut key_map_north as *mut c_void);
    M_BindVariable("key_map_south", &raw mut key_map_south as *mut c_void);
    M_BindVariable("key_map_east", &raw mut key_map_east as *mut c_void);
    M_BindVariable("key_map_west", &raw mut key_map_west as *mut c_void);
    M_BindVariable("key_map_zoomin", &raw mut key_map_zoomin as *mut c_void);
    M_BindVariable("key_map_zoomout", &raw mut key_map_zoomout as *mut c_void);
    M_BindVariable("key_map_toggle", &raw mut key_map_toggle as *mut c_void);
    M_BindVariable("key_map_maxzoom", &raw mut key_map_maxzoom as *mut c_void);
    M_BindVariable("key_map_follow", &raw mut key_map_follow as *mut c_void);
    M_BindVariable("key_map_grid", &raw mut key_map_grid as *mut c_void);
    M_BindVariable("key_map_mark", &raw mut key_map_mark as *mut c_void);
    M_BindVariable(
        "key_map_clearmark",
        &raw mut key_map_clearmark as *mut c_void,
    );
}

/// Register the in-game menu navigation keys plus the F1..F12 function
/// shortcuts (help / save / load / volume / detail / qsave / endgame /
/// messages / qload / quit / gamma) and the increment/decrement screen-
/// size and screenshot bindings.
pub fn M_BindMenuControls() {
    M_BindVariable(
        "key_menu_activate",
        &raw mut key_menu_activate as *mut c_void,
    );
    M_BindVariable("key_menu_up", &raw mut key_menu_up as *mut c_void);
    M_BindVariable("key_menu_down", &raw mut key_menu_down as *mut c_void);
    M_BindVariable("key_menu_left", &raw mut key_menu_left as *mut c_void);
    M_BindVariable("key_menu_right", &raw mut key_menu_right as *mut c_void);
    M_BindVariable("key_menu_back", &raw mut key_menu_back as *mut c_void);
    M_BindVariable("key_menu_forward", &raw mut key_menu_forward as *mut c_void);
    M_BindVariable("key_menu_confirm", &raw mut key_menu_confirm as *mut c_void);
    M_BindVariable("key_menu_abort", &raw mut key_menu_abort as *mut c_void);

    M_BindVariable("key_menu_help", &raw mut key_menu_help as *mut c_void);
    M_BindVariable("key_menu_save", &raw mut key_menu_save as *mut c_void);
    M_BindVariable("key_menu_load", &raw mut key_menu_load as *mut c_void);
    M_BindVariable("key_menu_volume", &raw mut key_menu_volume as *mut c_void);
    M_BindVariable("key_menu_detail", &raw mut key_menu_detail as *mut c_void);
    M_BindVariable("key_menu_qsave", &raw mut key_menu_qsave as *mut c_void);
    M_BindVariable("key_menu_endgame", &raw mut key_menu_endgame as *mut c_void);
    M_BindVariable(
        "key_menu_messages",
        &raw mut key_menu_messages as *mut c_void,
    );
    M_BindVariable("key_menu_qload", &raw mut key_menu_qload as *mut c_void);
    M_BindVariable("key_menu_quit", &raw mut key_menu_quit as *mut c_void);
    M_BindVariable("key_menu_gamma", &raw mut key_menu_gamma as *mut c_void);

    M_BindVariable(
        "key_menu_incscreen",
        &raw mut key_menu_incscreen as *mut c_void,
    );
    M_BindVariable(
        "key_menu_decscreen",
        &raw mut key_menu_decscreen as *mut c_void,
    );
    M_BindVariable(
        "key_menu_screenshot",
        &raw mut key_menu_screenshot as *mut c_void,
    );
    M_BindVariable("key_demo_quit", &raw mut key_demo_quit as *mut c_void);
    M_BindVariable("key_spy", &raw mut key_spy as *mut c_void);
}

/// Register multiplayer chat bindings: the broadcast `key_multi_msg`
/// plus one `key_multi_msgplayerN` binding per active player.
///
/// `num_players` should be at most 8, matching the size of
/// [`key_multi_msgplayer`]. Each per-player name is formatted into a local
/// 32-byte buffer with [`c_write!`] before being passed to
/// `M_BindVariable`; this replaces the C `M_snprintf` call.
pub fn M_BindChatControls(num_players: u32) {
    unsafe {
        M_BindVariable("key_multi_msg", &raw mut key_multi_msg as *mut c_void);

        for i in 0..num_players as usize {
            M_BindVariable(
                &format!("key_multi_msgplayer{}", i + 1),
                &raw mut key_multi_msgplayer[i] as *mut c_void,
            );
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
pub fn M_ApplyPlatformDefaults() {
    // no-op. Add your platform-specific patches here.
}
