//! Rust port of vendor/doomgeneric/m_controls.c.
//!
//! Default keyboard / mouse / joystick bindings for every game in the
//! chocolate-doom family (Doom, Heretic, Hexen, Strife), plus the
//! `M_Bind*` registration helpers that wire each binding into the
//! `m_config` variable table so they can be read or written through
//! `default.cfg`.
//!
//! All bindings are public fields of [`ControlsState`], held in the
//! [`CONTROLS`] global, so the rest of the engine can read and change
//! them. Each `M_Bind<Game>Controls` function calls into `M_BindVariable`
//! (from `m_config`) with the stable address of each binding relevant to
//! that game's binding subset.

#![allow(non_upper_case_globals, non_snake_case)]

use crate::engine::{
    global::Global,
    interface::keys::{
        KEY_BACKSPACE, KEY_DEL, KEY_DOWNARROW, KEY_END, KEY_ENTER, KEY_EQUALS, KEY_ESCAPE, KEY_F1,
        KEY_F10, KEY_F11, KEY_F12, KEY_F2, KEY_F3, KEY_F4, KEY_F5, KEY_F6, KEY_F7, KEY_F8, KEY_F9,
        KEY_FIRE, KEY_HOME, KEY_INS, KEY_LEFTARROW, KEY_MINUS, KEY_PAUSE, KEY_PGDN, KEY_PGUP,
        KEY_RALT, KEY_RIGHTARROW, KEY_RSHIFT, KEY_STRAFE_L, KEY_STRAFE_R, KEY_TAB, KEY_UPARROW,
        KEY_USE,
    },
};

/// Every key, mouse and joystick binding (C `m_controls.c` globals).
///
/// Config binds each field by address, so the bindings live at a stable
/// place inside [`CONTROLS`].
#[derive(Clone, Copy)]
pub struct ControlsState {
    //
    // Keyboard controls
    //
    /// Turn right (default: right arrow).
    pub key_right: i32,
    /// Turn left (default: left arrow).
    pub key_left: i32,
    /// Move forward (default: up arrow).
    pub key_up: i32,
    /// Move backward (default: down arrow).
    pub key_down: i32,
    /// Strafe left (default: comma `KEY_STRAFE_L`).
    pub key_strafeleft: i32,
    /// Strafe right (default: period `KEY_STRAFE_R`).
    pub key_straferight: i32,
    /// Fire current weapon (default: `KEY_FIRE`, the right Ctrl).
    pub key_fire: i32,
    /// Use / open / activate trigger (default: space `KEY_USE`).
    pub key_use: i32,
    /// Hold to convert turn into strafe (default: right Alt).
    pub key_strafe: i32,
    /// Hold to run (default: right Shift).
    pub key_speed: i32,

    //
    // Heretic keyboard controls
    //
    /// Heretic: fly up (default: Page Up).
    pub key_flyup: i32,
    /// Heretic: fly down (default: Insert).
    pub key_flydown: i32,
    /// Heretic: stop flying / centre vertically (default: Home).
    pub key_flycenter: i32,

    /// Heretic: look up (default: Page Down).
    pub key_lookup: i32,
    /// Heretic: look down (default: Delete).
    pub key_lookdown: i32,
    /// Heretic: centre vertical look (default: End).
    pub key_lookcenter: i32,

    /// Heretic/Strife: cycle inventory left (default: `[`; Strife uses Insert).
    pub key_invleft: i32,
    /// Heretic/Strife: cycle inventory right (default: `]`; Strife uses Delete).
    pub key_invright: i32,
    /// Heretic: use selected inventory artifact (default: Enter).
    pub key_useartifact: i32,

    //
    // Hexen key controls
    //
    /// Hexen/Strife: jump (default: `/`; Strife rebinds to `a`).
    pub key_jump: i32,

    /// Hexen: use all artifacts of selected kind (default: Backspace).
    pub key_arti_all: i32,
    /// Hexen: use Quartz Flask / Mystic Urn (default: `\\`).
    pub key_arti_health: i32,
    /// Hexen: use poison bag artifact (default: `0`).
    pub key_arti_poisonbag: i32,
    /// Hexen: use disc of repulsion (default: `9`).
    pub key_arti_blastradius: i32,
    /// Hexen: use teleport artifact (default: `8`).
    pub key_arti_teleport: i32,
    /// Hexen: use banishment device (default: `7`).
    pub key_arti_teleportother: i32,
    /// Hexen: use porkalator (default: `6`).
    pub key_arti_egg: i32,
    /// Hexen: use icon of the defender (default: `5`).
    pub key_arti_invulnerability: i32,

    //
    // Strife key controls
    //
    /// Strife: use a medkit from inventory (default: `h`).
    pub key_usehealth: i32,
    /// Strife: query selected NPC / object (default: `q`).
    pub key_invquery: i32,
    /// Strife: open mission log (default: `w`).
    pub key_mission: i32,
    /// Strife: open the inventory popup (default: `z`).
    pub key_invpop: i32,
    /// Strife: open the keys popup (default: `k`).
    pub key_invkey: i32,
    /// Strife: cycle to the first inventory item (default: Home).
    pub key_invhome: i32,
    /// Strife: cycle to the last inventory item (default: End).
    pub key_invend: i32,
    /// Strife: use selected inventory item (default: Enter).
    pub key_invuse: i32,
    /// Strife: drop selected inventory item (default: Backspace).
    pub key_invdrop: i32,

    //
    // Mouse controls
    //
    /// Mouse button bound to fire (default: button 0).
    pub mousebfire: i32,
    /// Mouse button bound to strafe (default: button 1).
    pub mousebstrafe: i32,
    /// Mouse button bound to move forward (default: button 2).
    pub mousebforward: i32,

    /// Mouse button bound to jump (default: -1 = unbound; Hexen/Strife only).
    pub mousebjump: i32,

    /// Mouse button bound to strafe left (default: -1 = unbound).
    pub mousebstrafeleft: i32,
    /// Mouse button bound to strafe right (default: -1 = unbound).
    pub mousebstraferight: i32,
    /// Mouse button bound to move backward (default: -1 = unbound).
    pub mousebbackward: i32,
    /// Mouse button bound to "use" (default: -1 = unbound).
    pub mousebuse: i32,

    /// Mouse button bound to previous weapon (default: -1 = unbound).
    pub mousebprevweapon: i32,
    /// Mouse button bound to next weapon (default: -1 = unbound).
    pub mousebnextweapon: i32,

    /// Re-display last HUD message (default: Enter).
    pub key_message_refresh: i32,
    /// Pause the game (default: Pause).
    pub key_pause: i32,
    /// Abort current demo playback (default: `q`).
    pub key_demo_quit: i32,
    /// Spy on next player in multiplayer (default: F12).
    pub key_spy: i32,

    // Multiplayer chat keys:
    /// Send chat to all players (default: `t`).
    pub key_multi_msg: i32,
    /// Per-player private-chat keys (default: 0 / unbound for each).
    /// Indexed by player number 0..7; populated by [`M_BindChatControls`].
    pub key_multi_msgplayer: [i32; 8],

    // Weapon selection keys:
    /// Select weapon slot 1 (default: `1`).
    pub key_weapon1: i32,
    /// Select weapon slot 2 (default: `2`).
    pub key_weapon2: i32,
    /// Select weapon slot 3 (default: `3`).
    pub key_weapon3: i32,
    /// Select weapon slot 4 (default: `4`).
    pub key_weapon4: i32,
    /// Select weapon slot 5 (default: `5`).
    pub key_weapon5: i32,
    /// Select weapon slot 6 (default: `6`).
    pub key_weapon6: i32,
    /// Select weapon slot 7 (default: `7`).
    pub key_weapon7: i32,
    /// Select weapon slot 8 (default: `8`).
    pub key_weapon8: i32,
    /// Cycle to previous weapon (default: 0 = unbound).
    pub key_prevweapon: i32,
    /// Cycle to next weapon (default: 0 = unbound).
    pub key_nextweapon: i32,

    // Map control keys:
    /// Automap: pan north (default: up arrow).
    pub key_map_north: i32,
    /// Automap: pan south (default: down arrow).
    pub key_map_south: i32,
    /// Automap: pan east (default: right arrow).
    pub key_map_east: i32,
    /// Automap: pan west (default: left arrow).
    pub key_map_west: i32,
    /// Automap: zoom in (default: `=`).
    pub key_map_zoomin: i32,
    /// Automap: zoom out (default: `-`).
    pub key_map_zoomout: i32,
    /// Toggle automap on/off (default: Tab).
    pub key_map_toggle: i32,
    /// Automap: fully zoom out / max zoom (default: `0`).
    pub key_map_maxzoom: i32,
    /// Automap: toggle follow-player mode (default: `f`).
    pub key_map_follow: i32,
    /// Automap: toggle grid overlay (default: `g`).
    pub key_map_grid: i32,
    /// Automap: drop a marker at the cursor (default: `m`).
    pub key_map_mark: i32,
    /// Automap: clear all markers (default: `c`).
    pub key_map_clearmark: i32,

    // menu keys:
    /// Toggle the in-game menu (default: Escape).
    pub key_menu_activate: i32,
    /// Menu: move cursor up (default: up arrow).
    pub key_menu_up: i32,
    /// Menu: move cursor down (default: down arrow).
    pub key_menu_down: i32,
    /// Menu: decrease slider value / left (default: left arrow).
    pub key_menu_left: i32,
    /// Menu: increase slider value / right (default: right arrow).
    pub key_menu_right: i32,
    /// Menu: go back one screen (default: Backspace).
    pub key_menu_back: i32,
    /// Menu: select / activate item (default: Enter).
    pub key_menu_forward: i32,
    /// Menu: confirm `Yes` (default: `y`).
    pub key_menu_confirm: i32,
    /// Menu: confirm `No` / cancel (default: `n`).
    pub key_menu_abort: i32,

    /// Function shortcut: Help / read this (default: F1).
    pub key_menu_help: i32,
    /// Function shortcut: Save Game menu (default: F2).
    pub key_menu_save: i32,
    /// Function shortcut: Load Game menu (default: F3).
    pub key_menu_load: i32,
    /// Function shortcut: Volume menu (default: F4).
    pub key_menu_volume: i32,
    /// Function shortcut: toggle detail level (default: F5).
    pub key_menu_detail: i32,
    /// Function shortcut: quick save (default: F6).
    pub key_menu_qsave: i32,
    /// Function shortcut: end current game (default: F7).
    pub key_menu_endgame: i32,
    /// Function shortcut: toggle HUD messages (default: F8).
    pub key_menu_messages: i32,
    /// Function shortcut: quick load (default: F9).
    pub key_menu_qload: i32,
    /// Function shortcut: quit to OS (default: F10).
    pub key_menu_quit: i32,
    /// Function shortcut: cycle gamma correction (default: F11).
    pub key_menu_gamma: i32,

    /// Increase screen size / shrink HUD borders (default: `=`).
    pub key_menu_incscreen: i32,
    /// Decrease screen size / grow HUD borders (default: `-`).
    pub key_menu_decscreen: i32,
    /// Take a screenshot (default: 0 = unbound; Vanilla does not bind).
    pub key_menu_screenshot: i32,

    //
    // Joystick controls
    //
    /// Joystick button bound to fire (default: button 0).
    pub joybfire: i32,
    /// Joystick button bound to strafe modifier (default: button 1).
    pub joybstrafe: i32,
    /// Joystick button bound to use (default: button 3).
    pub joybuse: i32,
    /// Joystick button bound to run / speed modifier (default: button 2).
    pub joybspeed: i32,

    /// Joystick button bound to strafe left (default: -1 = unbound).
    pub joybstrafeleft: i32,
    /// Joystick button bound to strafe right (default: -1 = unbound).
    pub joybstraferight: i32,

    /// Joystick button bound to jump (default: -1 = unbound; Hexen/Strife only).
    pub joybjump: i32,

    /// Joystick button bound to previous weapon (default: -1 = unbound).
    pub joybprevweapon: i32,
    /// Joystick button bound to next weapon (default: -1 = unbound).
    pub joybnextweapon: i32,

    /// Joystick button bound to activate menu (default: -1 = unbound).
    pub joybmenu: i32,

    // Control whether if a mouse button is double clicked, it acts like
    // "use" has been pressed
    /// If non-zero, a mouse double-click acts as a "use" press (default: 1).
    pub dclick_use: i32,
}

impl ControlsState {
    /// The default bindings from `m_controls.c`.
    pub const fn new() -> Self {
        Self {
            key_right: KEY_RIGHTARROW as i32,
            key_left: KEY_LEFTARROW as i32,
            key_up: KEY_UPARROW as i32,
            key_down: KEY_DOWNARROW as i32,
            key_strafeleft: KEY_STRAFE_L as i32,
            key_straferight: KEY_STRAFE_R as i32,
            key_fire: KEY_FIRE as i32,
            key_use: KEY_USE as i32,
            key_strafe: KEY_RALT as i32,
            key_speed: KEY_RSHIFT as i32,

            key_flyup: KEY_PGUP as i32,
            key_flydown: KEY_INS as i32,
            key_flycenter: KEY_HOME as i32,

            key_lookup: KEY_PGDN as i32,
            key_lookdown: KEY_DEL as i32,
            key_lookcenter: KEY_END as i32,

            key_invleft: b'[' as i32,
            key_invright: b']' as i32,
            key_useartifact: KEY_ENTER as i32,

            key_jump: b'/' as i32,

            key_arti_all: KEY_BACKSPACE as i32,
            key_arti_health: b'\\' as i32,
            key_arti_poisonbag: b'0' as i32,
            key_arti_blastradius: b'9' as i32,
            key_arti_teleport: b'8' as i32,
            key_arti_teleportother: b'7' as i32,
            key_arti_egg: b'6' as i32,
            key_arti_invulnerability: b'5' as i32,

            key_usehealth: b'h' as i32,
            key_invquery: b'q' as i32,
            key_mission: b'w' as i32,
            key_invpop: b'z' as i32,
            key_invkey: b'k' as i32,
            key_invhome: KEY_HOME as i32,
            key_invend: KEY_END as i32,
            key_invuse: KEY_ENTER as i32,
            key_invdrop: KEY_BACKSPACE as i32,

            mousebfire: 0,
            mousebstrafe: 1,
            mousebforward: 2,

            mousebjump: -1,

            mousebstrafeleft: -1,
            mousebstraferight: -1,
            mousebbackward: -1,
            mousebuse: -1,

            mousebprevweapon: -1,
            mousebnextweapon: -1,

            key_message_refresh: KEY_ENTER as i32,
            key_pause: KEY_PAUSE as i32,
            key_demo_quit: b'q' as i32,
            key_spy: KEY_F12 as i32,

            key_multi_msg: b't' as i32,
            key_multi_msgplayer: [0; 8],

            key_weapon1: b'1' as i32,
            key_weapon2: b'2' as i32,
            key_weapon3: b'3' as i32,
            key_weapon4: b'4' as i32,
            key_weapon5: b'5' as i32,
            key_weapon6: b'6' as i32,
            key_weapon7: b'7' as i32,
            key_weapon8: b'8' as i32,
            key_prevweapon: 0,
            key_nextweapon: 0,

            key_map_north: KEY_UPARROW as i32,
            key_map_south: KEY_DOWNARROW as i32,
            key_map_east: KEY_RIGHTARROW as i32,
            key_map_west: KEY_LEFTARROW as i32,
            key_map_zoomin: b'=' as i32,
            key_map_zoomout: b'-' as i32,
            key_map_toggle: KEY_TAB as i32,
            key_map_maxzoom: b'0' as i32,
            key_map_follow: b'f' as i32,
            key_map_grid: b'g' as i32,
            key_map_mark: b'm' as i32,
            key_map_clearmark: b'c' as i32,

            key_menu_activate: KEY_ESCAPE as i32,
            key_menu_up: KEY_UPARROW as i32,
            key_menu_down: KEY_DOWNARROW as i32,
            key_menu_left: KEY_LEFTARROW as i32,
            key_menu_right: KEY_RIGHTARROW as i32,
            key_menu_back: KEY_BACKSPACE as i32,
            key_menu_forward: KEY_ENTER as i32,
            key_menu_confirm: b'y' as i32,
            key_menu_abort: b'n' as i32,

            key_menu_help: KEY_F1 as i32,
            key_menu_save: KEY_F2 as i32,
            key_menu_load: KEY_F3 as i32,
            key_menu_volume: KEY_F4 as i32,
            key_menu_detail: KEY_F5 as i32,
            key_menu_qsave: KEY_F6 as i32,
            key_menu_endgame: KEY_F7 as i32,
            key_menu_messages: KEY_F8 as i32,
            key_menu_qload: KEY_F9 as i32,
            key_menu_quit: KEY_F10 as i32,
            key_menu_gamma: KEY_F11 as i32,

            key_menu_incscreen: KEY_EQUALS as i32,
            key_menu_decscreen: KEY_MINUS as i32,
            key_menu_screenshot: 0,

            joybfire: 0,
            joybstrafe: 1,
            joybuse: 3,
            joybspeed: 2,

            joybstrafeleft: -1,
            joybstraferight: -1,

            joybjump: -1,

            joybprevweapon: -1,
            joybnextweapon: -1,

            joybmenu: -1,

            dclick_use: 1,
        }
    }
}

impl Default for ControlsState {
    fn default() -> Self {
        Self::new()
    }
}

/// The key, mouse and joystick bindings.
pub static CONTROLS: Global<ControlsState> = Global::new(ControlsState::new());

/// `m_config` callback that registers a named variable against a
/// memory location. Wired through the `M_Bind*` functions below.
use crate::engine::misc::config::M_BindVariable;

/// Bind the config variable `$name` to the `CONTROLS` field `$field`.
///
/// The config table keeps the pointer, so it takes the field's stable
/// address inside the `CONTROLS` static.
macro_rules! bind_control {
    ($name:expr, $($field:tt)+) => {
        M_BindVariable($name, unsafe { &raw mut (*CONTROLS.as_ptr()).$($field)+ }.cast())
    };
}

//
// Bind all of the common controls used by Doom and all other games.
//

/// Register the keyboard / mouse / joystick bindings shared by every game.
///
/// Called once during startup. Each `M_BindVariable` call associates a
/// `.cfg` variable name (matching the entry in `m_config`'s
/// `doom_defaults` / `extra_defaults` tables) with the address of the
/// corresponding [`ControlsState`] field, so loading a config file
/// updates the live binding.
pub fn M_BindBaseControls() {
    bind_control!("key_right", key_right);
    bind_control!("key_left", key_left);
    bind_control!("key_up", key_up);
    bind_control!("key_down", key_down);
    bind_control!("key_strafeleft", key_strafeleft);
    bind_control!("key_straferight", key_straferight);
    bind_control!("key_fire", key_fire);
    bind_control!("key_use", key_use);
    bind_control!("key_strafe", key_strafe);
    bind_control!("key_speed", key_speed);

    bind_control!("mouseb_fire", mousebfire);
    bind_control!("mouseb_strafe", mousebstrafe);
    bind_control!("mouseb_forward", mousebforward);

    bind_control!("joyb_fire", joybfire);
    bind_control!("joyb_strafe", joybstrafe);
    bind_control!("joyb_use", joybuse);
    bind_control!("joyb_speed", joybspeed);

    bind_control!("joyb_menu_activate", joybmenu);

    // Extra controls that are not in the Vanilla versions:

    bind_control!("joyb_strafeleft", joybstrafeleft);
    bind_control!("joyb_straferight", joybstraferight);
    bind_control!("mouseb_strafeleft", mousebstrafeleft);
    bind_control!("mouseb_straferight", mousebstraferight);
    bind_control!("mouseb_use", mousebuse);
    bind_control!("mouseb_backward", mousebbackward);
    bind_control!("dclick_use", dclick_use);
    bind_control!("key_pause", key_pause);
    bind_control!("key_message_refresh", key_message_refresh);
}

/// Register the Heretic-specific keyboard bindings (fly / look / inventory
/// keys).
pub fn M_BindHereticControls() {
    bind_control!("key_flyup", key_flyup);
    bind_control!("key_flydown", key_flydown);
    bind_control!("key_flycenter", key_flycenter);

    bind_control!("key_lookup", key_lookup);
    bind_control!("key_lookdown", key_lookdown);
    bind_control!("key_lookcenter", key_lookcenter);

    bind_control!("key_invleft", key_invleft);
    bind_control!("key_invright", key_invright);
    bind_control!("key_useartifact", key_useartifact);
}

/// Register the Hexen-specific bindings: jump on key / mouse / joystick
/// plus the eight artifact hotkeys.
pub fn M_BindHexenControls() {
    bind_control!("key_jump", key_jump);
    bind_control!("mouseb_jump", mousebjump);
    bind_control!("joyb_jump", joybjump);

    bind_control!("key_arti_all", key_arti_all);
    bind_control!("key_arti_health", key_arti_health);
    bind_control!("key_arti_poisonbag", key_arti_poisonbag);
    bind_control!("key_arti_blastradius", key_arti_blastradius);
    bind_control!("key_arti_teleport", key_arti_teleport);
    bind_control!("key_arti_teleportother", key_arti_teleportother);
    bind_control!("key_arti_egg", key_arti_egg);
    bind_control!("key_arti_invulnerability", key_arti_invulnerability);
}

/// Register the Strife-specific bindings.
///
/// Mutates several shared bindings to their Strife defaults before
/// registration: `key_message_refresh = '/'`, `key_jump = 'a'`,
/// `key_lookup = PGUP`, `key_lookdown = PGDN`, `key_invleft = INS`,
/// `key_invright = DEL`. Mirrors C `M_BindStrifeControls`.
pub fn M_BindStrifeControls() {
    {
        let mut controls = CONTROLS.borrow_mut();

        // These are shared with all games, but have different defaults:
        controls.key_message_refresh = b'/' as i32;

        // These keys are shared with Heretic/Hexen but have different defaults:
        controls.key_jump = b'a' as i32;
        controls.key_lookup = KEY_PGUP as i32;
        controls.key_lookdown = KEY_PGDN as i32;
        controls.key_invleft = KEY_INS as i32;
        controls.key_invright = KEY_DEL as i32;
    }

    bind_control!("key_jump", key_jump);
    bind_control!("key_lookUp", key_lookup);
    bind_control!("key_lookDown", key_lookdown);
    bind_control!("key_invLeft", key_invleft);
    bind_control!("key_invRight", key_invright);

    // Custom Strife-only Keys:
    bind_control!("key_useHealth", key_usehealth);
    bind_control!("key_invquery", key_invquery);
    bind_control!("key_mission", key_mission);
    bind_control!("key_invPop", key_invpop);
    bind_control!("key_invKey", key_invkey);
    bind_control!("key_invHome", key_invhome);
    bind_control!("key_invEnd", key_invend);
    bind_control!("key_invUse", key_invuse);
    bind_control!("key_invDrop", key_invdrop);

    // Strife also supports jump on mouse and joystick, and in the exact same
    // manner as Hexen!
    bind_control!("mouseb_jump", mousebjump);
    bind_control!("joyb_jump", joybjump);
}

/// Register the weapon-selection key bindings (`key_weapon1`..`8`,
/// `key_prevweapon`, `key_nextweapon`) plus the corresponding mouse and
/// joystick prev/next bindings.
pub fn M_BindWeaponControls() {
    bind_control!("key_weapon1", key_weapon1);
    bind_control!("key_weapon2", key_weapon2);
    bind_control!("key_weapon3", key_weapon3);
    bind_control!("key_weapon4", key_weapon4);
    bind_control!("key_weapon5", key_weapon5);
    bind_control!("key_weapon6", key_weapon6);
    bind_control!("key_weapon7", key_weapon7);
    bind_control!("key_weapon8", key_weapon8);

    bind_control!("key_prevweapon", key_prevweapon);
    bind_control!("key_nextweapon", key_nextweapon);

    bind_control!("joyb_prevweapon", joybprevweapon);
    bind_control!("joyb_nextweapon", joybnextweapon);

    bind_control!("mouseb_prevweapon", mousebprevweapon);
    bind_control!("mouseb_nextweapon", mousebnextweapon);
}

/// Register the automap movement / zoom / overlay bindings.
pub fn M_BindMapControls() {
    bind_control!("key_map_north", key_map_north);
    bind_control!("key_map_south", key_map_south);
    bind_control!("key_map_east", key_map_east);
    bind_control!("key_map_west", key_map_west);
    bind_control!("key_map_zoomin", key_map_zoomin);
    bind_control!("key_map_zoomout", key_map_zoomout);
    bind_control!("key_map_toggle", key_map_toggle);
    bind_control!("key_map_maxzoom", key_map_maxzoom);
    bind_control!("key_map_follow", key_map_follow);
    bind_control!("key_map_grid", key_map_grid);
    bind_control!("key_map_mark", key_map_mark);
    bind_control!("key_map_clearmark", key_map_clearmark);
}

/// Register the in-game menu navigation keys plus the F1..F12 function
/// shortcuts (help / save / load / volume / detail / qsave / endgame /
/// messages / qload / quit / gamma) and the increment/decrement screen-
/// size and screenshot bindings.
pub fn M_BindMenuControls() {
    bind_control!("key_menu_activate", key_menu_activate);
    bind_control!("key_menu_up", key_menu_up);
    bind_control!("key_menu_down", key_menu_down);
    bind_control!("key_menu_left", key_menu_left);
    bind_control!("key_menu_right", key_menu_right);
    bind_control!("key_menu_back", key_menu_back);
    bind_control!("key_menu_forward", key_menu_forward);
    bind_control!("key_menu_confirm", key_menu_confirm);
    bind_control!("key_menu_abort", key_menu_abort);

    bind_control!("key_menu_help", key_menu_help);
    bind_control!("key_menu_save", key_menu_save);
    bind_control!("key_menu_load", key_menu_load);
    bind_control!("key_menu_volume", key_menu_volume);
    bind_control!("key_menu_detail", key_menu_detail);
    bind_control!("key_menu_qsave", key_menu_qsave);
    bind_control!("key_menu_endgame", key_menu_endgame);
    bind_control!("key_menu_messages", key_menu_messages);
    bind_control!("key_menu_qload", key_menu_qload);
    bind_control!("key_menu_quit", key_menu_quit);
    bind_control!("key_menu_gamma", key_menu_gamma);

    bind_control!("key_menu_incscreen", key_menu_incscreen);
    bind_control!("key_menu_decscreen", key_menu_decscreen);
    bind_control!("key_menu_screenshot", key_menu_screenshot);
    bind_control!("key_demo_quit", key_demo_quit);
    bind_control!("key_spy", key_spy);
}

/// Register multiplayer chat bindings: the broadcast `key_multi_msg`
/// plus one `key_multi_msgplayerN` binding per active player.
///
/// `num_players` should be at most 8, matching the size of
/// [`ControlsState::key_multi_msgplayer`]. Each per-player name is
/// formatted with `format!` before being passed to `M_BindVariable`; this
/// replaces the C `M_snprintf` call.
pub fn M_BindChatControls(num_players: u32) {
    bind_control!("key_multi_msg", key_multi_msg);

    for i in 0..num_players as usize {
        bind_control!(
            &format!("key_multi_msgplayer{}", i + 1),
            key_multi_msgplayer[i]
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A few spot checks of the `m_controls.c` defaults.
    #[test]
    fn new_state_has_vanilla_defaults() {
        let controls = ControlsState::new();
        assert_eq!(controls.key_fire, KEY_FIRE as i32);
        assert_eq!(controls.key_menu_activate, KEY_ESCAPE as i32);
        assert_eq!(controls.key_map_toggle, KEY_TAB as i32);
        assert_eq!(controls.key_multi_msgplayer, [0; 8]);
        assert_eq!(controls.joybuse, 3);
        assert_eq!(controls.dclick_use, 1);
    }
}
