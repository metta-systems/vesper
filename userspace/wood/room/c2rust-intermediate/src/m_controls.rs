extern "C" {
    fn snprintf(
        __s: *mut ::core::ffi::c_char,
        __maxlen: size_t,
        __format: *const ::core::ffi::c_char,
        ...
    ) -> ::core::ffi::c_int;
    fn M_BindVariable(name: *mut ::core::ffi::c_char, variable: *mut ::core::ffi::c_void);
    fn M_snprintf_clamp(
        buf: *mut ::core::ffi::c_char,
        len: size_t,
        result: ::core::ffi::c_int,
    ) -> ::core::ffi::c_int;
}
pub type size_t = usize;
pub const KEY_RIGHTARROW: ::core::ffi::c_int = 0xae as ::core::ffi::c_int;
pub const KEY_LEFTARROW: ::core::ffi::c_int = 0xac as ::core::ffi::c_int;
pub const KEY_UPARROW: ::core::ffi::c_int = 0xad as ::core::ffi::c_int;
pub const KEY_DOWNARROW: ::core::ffi::c_int = 0xaf as ::core::ffi::c_int;
pub const KEY_STRAFE_L: ::core::ffi::c_int = 0xa0 as ::core::ffi::c_int;
pub const KEY_STRAFE_R: ::core::ffi::c_int = 0xa1 as ::core::ffi::c_int;
pub const KEY_USE: ::core::ffi::c_int = 0xa2 as ::core::ffi::c_int;
pub const KEY_FIRE: ::core::ffi::c_int = 0xa3 as ::core::ffi::c_int;
pub const KEY_ESCAPE: ::core::ffi::c_int = 27 as ::core::ffi::c_int;
pub const KEY_ENTER: ::core::ffi::c_int = 13 as ::core::ffi::c_int;
pub const KEY_TAB: ::core::ffi::c_int = 9 as ::core::ffi::c_int;
pub const KEY_F1: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x3b as ::core::ffi::c_int;
pub const KEY_F2: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x3c as ::core::ffi::c_int;
pub const KEY_F3: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x3d as ::core::ffi::c_int;
pub const KEY_F4: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x3e as ::core::ffi::c_int;
pub const KEY_F5: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x3f as ::core::ffi::c_int;
pub const KEY_F6: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x40 as ::core::ffi::c_int;
pub const KEY_F7: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x41 as ::core::ffi::c_int;
pub const KEY_F8: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x42 as ::core::ffi::c_int;
pub const KEY_F9: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x43 as ::core::ffi::c_int;
pub const KEY_F10: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x44 as ::core::ffi::c_int;
pub const KEY_F11: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x57 as ::core::ffi::c_int;
pub const KEY_F12: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x58 as ::core::ffi::c_int;
pub const KEY_BACKSPACE: ::core::ffi::c_int = 0x7f as ::core::ffi::c_int;
pub const KEY_PAUSE: ::core::ffi::c_int = 0xff as ::core::ffi::c_int;
pub const KEY_EQUALS: ::core::ffi::c_int = 0x3d as ::core::ffi::c_int;
pub const KEY_MINUS: ::core::ffi::c_int = 0x2d as ::core::ffi::c_int;
pub const KEY_RSHIFT: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x36 as ::core::ffi::c_int;
pub const KEY_RALT: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x38 as ::core::ffi::c_int;
pub const KEY_HOME: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x47 as ::core::ffi::c_int;
pub const KEY_END: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x4f as ::core::ffi::c_int;
pub const KEY_PGUP: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x49 as ::core::ffi::c_int;
pub const KEY_PGDN: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x51 as ::core::ffi::c_int;
pub const KEY_INS: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x52 as ::core::ffi::c_int;
pub const KEY_DEL: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x53 as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_right: ::core::ffi::c_int = KEY_RIGHTARROW;
#[no_mangle]
pub static mut key_left: ::core::ffi::c_int = KEY_LEFTARROW;
#[no_mangle]
pub static mut key_up: ::core::ffi::c_int = KEY_UPARROW;
#[no_mangle]
pub static mut key_down: ::core::ffi::c_int = KEY_DOWNARROW;
#[no_mangle]
pub static mut key_strafeleft: ::core::ffi::c_int = KEY_STRAFE_L;
#[no_mangle]
pub static mut key_straferight: ::core::ffi::c_int = KEY_STRAFE_R;
#[no_mangle]
pub static mut key_fire: ::core::ffi::c_int = KEY_FIRE;
#[no_mangle]
pub static mut key_use: ::core::ffi::c_int = KEY_USE;
#[no_mangle]
pub static mut key_strafe: ::core::ffi::c_int = KEY_RALT;
#[no_mangle]
pub static mut key_speed: ::core::ffi::c_int = KEY_RSHIFT;
#[no_mangle]
pub static mut key_flyup: ::core::ffi::c_int = KEY_PGUP;
#[no_mangle]
pub static mut key_flydown: ::core::ffi::c_int = KEY_INS;
#[no_mangle]
pub static mut key_flycenter: ::core::ffi::c_int = KEY_HOME;
#[no_mangle]
pub static mut key_lookup: ::core::ffi::c_int = KEY_PGDN;
#[no_mangle]
pub static mut key_lookdown: ::core::ffi::c_int = KEY_DEL;
#[no_mangle]
pub static mut key_lookcenter: ::core::ffi::c_int = KEY_END;
#[no_mangle]
pub static mut key_invleft: ::core::ffi::c_int = '[' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_invright: ::core::ffi::c_int = ']' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_useartifact: ::core::ffi::c_int = KEY_ENTER;
#[no_mangle]
pub static mut key_jump: ::core::ffi::c_int = '/' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_arti_all: ::core::ffi::c_int = KEY_BACKSPACE;
#[no_mangle]
pub static mut key_arti_health: ::core::ffi::c_int = '\\' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_arti_poisonbag: ::core::ffi::c_int = '0' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_arti_blastradius: ::core::ffi::c_int = '9' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_arti_teleport: ::core::ffi::c_int = '8' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_arti_teleportother: ::core::ffi::c_int = '7' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_arti_egg: ::core::ffi::c_int = '6' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_arti_invulnerability: ::core::ffi::c_int = '5' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_usehealth: ::core::ffi::c_int = 'h' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_invquery: ::core::ffi::c_int = 'q' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_mission: ::core::ffi::c_int = 'w' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_invpop: ::core::ffi::c_int = 'z' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_invkey: ::core::ffi::c_int = 'k' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_invhome: ::core::ffi::c_int = KEY_HOME;
#[no_mangle]
pub static mut key_invend: ::core::ffi::c_int = KEY_END;
#[no_mangle]
pub static mut key_invuse: ::core::ffi::c_int = KEY_ENTER;
#[no_mangle]
pub static mut key_invdrop: ::core::ffi::c_int = KEY_BACKSPACE;
#[no_mangle]
pub static mut mousebfire: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
#[no_mangle]
pub static mut mousebstrafe: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
#[no_mangle]
pub static mut mousebforward: ::core::ffi::c_int = 2 as ::core::ffi::c_int;
#[no_mangle]
pub static mut mousebjump: ::core::ffi::c_int = -1 as ::core::ffi::c_int;
#[no_mangle]
pub static mut mousebstrafeleft: ::core::ffi::c_int = -1 as ::core::ffi::c_int;
#[no_mangle]
pub static mut mousebstraferight: ::core::ffi::c_int = -1 as ::core::ffi::c_int;
#[no_mangle]
pub static mut mousebbackward: ::core::ffi::c_int = -1 as ::core::ffi::c_int;
#[no_mangle]
pub static mut mousebuse: ::core::ffi::c_int = -1 as ::core::ffi::c_int;
#[no_mangle]
pub static mut mousebprevweapon: ::core::ffi::c_int = -1 as ::core::ffi::c_int;
#[no_mangle]
pub static mut mousebnextweapon: ::core::ffi::c_int = -1 as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_message_refresh: ::core::ffi::c_int = KEY_ENTER;
#[no_mangle]
pub static mut key_pause: ::core::ffi::c_int = KEY_PAUSE;
#[no_mangle]
pub static mut key_demo_quit: ::core::ffi::c_int = 'q' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_spy: ::core::ffi::c_int = KEY_F12;
#[no_mangle]
pub static mut key_multi_msg: ::core::ffi::c_int = 't' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_multi_msgplayer: [::core::ffi::c_int; 8] = [0; 8];
#[no_mangle]
pub static mut key_weapon1: ::core::ffi::c_int = '1' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_weapon2: ::core::ffi::c_int = '2' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_weapon3: ::core::ffi::c_int = '3' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_weapon4: ::core::ffi::c_int = '4' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_weapon5: ::core::ffi::c_int = '5' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_weapon6: ::core::ffi::c_int = '6' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_weapon7: ::core::ffi::c_int = '7' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_weapon8: ::core::ffi::c_int = '8' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_prevweapon: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_nextweapon: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_map_north: ::core::ffi::c_int = KEY_UPARROW;
#[no_mangle]
pub static mut key_map_south: ::core::ffi::c_int = KEY_DOWNARROW;
#[no_mangle]
pub static mut key_map_east: ::core::ffi::c_int = KEY_RIGHTARROW;
#[no_mangle]
pub static mut key_map_west: ::core::ffi::c_int = KEY_LEFTARROW;
#[no_mangle]
pub static mut key_map_zoomin: ::core::ffi::c_int = '=' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_map_zoomout: ::core::ffi::c_int = '-' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_map_toggle: ::core::ffi::c_int = KEY_TAB;
#[no_mangle]
pub static mut key_map_maxzoom: ::core::ffi::c_int = '0' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_map_follow: ::core::ffi::c_int = 'f' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_map_grid: ::core::ffi::c_int = 'g' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_map_mark: ::core::ffi::c_int = 'm' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_map_clearmark: ::core::ffi::c_int = 'c' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_menu_activate: ::core::ffi::c_int = KEY_ESCAPE;
#[no_mangle]
pub static mut key_menu_up: ::core::ffi::c_int = KEY_UPARROW;
#[no_mangle]
pub static mut key_menu_down: ::core::ffi::c_int = KEY_DOWNARROW;
#[no_mangle]
pub static mut key_menu_left: ::core::ffi::c_int = KEY_LEFTARROW;
#[no_mangle]
pub static mut key_menu_right: ::core::ffi::c_int = KEY_RIGHTARROW;
#[no_mangle]
pub static mut key_menu_back: ::core::ffi::c_int = KEY_BACKSPACE;
#[no_mangle]
pub static mut key_menu_forward: ::core::ffi::c_int = KEY_ENTER;
#[no_mangle]
pub static mut key_menu_confirm: ::core::ffi::c_int = 'y' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_menu_abort: ::core::ffi::c_int = 'n' as ::core::ffi::c_int;
#[no_mangle]
pub static mut key_menu_help: ::core::ffi::c_int = KEY_F1;
#[no_mangle]
pub static mut key_menu_save: ::core::ffi::c_int = KEY_F2;
#[no_mangle]
pub static mut key_menu_load: ::core::ffi::c_int = KEY_F3;
#[no_mangle]
pub static mut key_menu_volume: ::core::ffi::c_int = KEY_F4;
#[no_mangle]
pub static mut key_menu_detail: ::core::ffi::c_int = KEY_F5;
#[no_mangle]
pub static mut key_menu_qsave: ::core::ffi::c_int = KEY_F6;
#[no_mangle]
pub static mut key_menu_endgame: ::core::ffi::c_int = KEY_F7;
#[no_mangle]
pub static mut key_menu_messages: ::core::ffi::c_int = KEY_F8;
#[no_mangle]
pub static mut key_menu_qload: ::core::ffi::c_int = KEY_F9;
#[no_mangle]
pub static mut key_menu_quit: ::core::ffi::c_int = KEY_F10;
#[no_mangle]
pub static mut key_menu_gamma: ::core::ffi::c_int = KEY_F11;
#[no_mangle]
pub static mut key_menu_incscreen: ::core::ffi::c_int = KEY_EQUALS;
#[no_mangle]
pub static mut key_menu_decscreen: ::core::ffi::c_int = KEY_MINUS;
#[no_mangle]
pub static mut key_menu_screenshot: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
#[no_mangle]
pub static mut joybfire: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
#[no_mangle]
pub static mut joybstrafe: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
#[no_mangle]
pub static mut joybuse: ::core::ffi::c_int = 3 as ::core::ffi::c_int;
#[no_mangle]
pub static mut joybspeed: ::core::ffi::c_int = 2 as ::core::ffi::c_int;
#[no_mangle]
pub static mut joybstrafeleft: ::core::ffi::c_int = -1 as ::core::ffi::c_int;
#[no_mangle]
pub static mut joybstraferight: ::core::ffi::c_int = -1 as ::core::ffi::c_int;
#[no_mangle]
pub static mut joybjump: ::core::ffi::c_int = -1 as ::core::ffi::c_int;
#[no_mangle]
pub static mut joybprevweapon: ::core::ffi::c_int = -1 as ::core::ffi::c_int;
#[no_mangle]
pub static mut joybnextweapon: ::core::ffi::c_int = -1 as ::core::ffi::c_int;
#[no_mangle]
pub static mut joybmenu: ::core::ffi::c_int = -1 as ::core::ffi::c_int;
#[no_mangle]
pub static mut dclick_use: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
#[no_mangle]
pub unsafe extern "C" fn M_BindBaseControls() {
    M_BindVariable(
        b"key_right\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_right as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_left\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_left as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_up\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_up as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_down\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_down as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_strafeleft\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_strafeleft as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_straferight\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_straferight as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_fire\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_fire as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_use\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_use as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_strafe\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_strafe as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_speed\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_speed as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"mouseb_fire\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut mousebfire as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"mouseb_strafe\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut mousebstrafe as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"mouseb_forward\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut mousebforward as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"joyb_fire\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut joybfire as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"joyb_strafe\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut joybstrafe as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"joyb_use\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut joybuse as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"joyb_speed\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut joybspeed as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"joyb_menu_activate\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut joybmenu as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"joyb_strafeleft\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut joybstrafeleft as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"joyb_straferight\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut joybstraferight as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"mouseb_strafeleft\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut mousebstrafeleft as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"mouseb_straferight\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut mousebstraferight as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"mouseb_use\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut mousebuse as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"mouseb_backward\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut mousebbackward as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"dclick_use\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut dclick_use as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_pause\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_pause as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_message_refresh\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_message_refresh as *mut ::core::ffi::c_void,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_BindHereticControls() {
    M_BindVariable(
        b"key_flyup\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_flyup as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_flydown\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_flydown as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_flycenter\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_flycenter as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_lookup\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_lookup as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_lookdown\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_lookdown as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_lookcenter\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_lookcenter as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_invleft\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_invleft as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_invright\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_invright as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_useartifact\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_useartifact as *mut ::core::ffi::c_void,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_BindHexenControls() {
    M_BindVariable(
        b"key_jump\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_jump as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"mouseb_jump\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut mousebjump as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"joyb_jump\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut joybjump as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_arti_all\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_arti_all as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_arti_health\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_arti_health as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_arti_poisonbag\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_arti_poisonbag as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_arti_blastradius\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        &raw mut key_arti_blastradius as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_arti_teleport\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_arti_teleport as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_arti_teleportother\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        &raw mut key_arti_teleportother as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_arti_egg\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_arti_egg as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_arti_invulnerability\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        &raw mut key_arti_invulnerability as *mut ::core::ffi::c_void,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_BindStrifeControls() {
    key_message_refresh = '/' as ::core::ffi::c_int;
    key_jump = 'a' as ::core::ffi::c_int;
    key_lookup = KEY_PGUP;
    key_lookdown = KEY_PGDN;
    key_invleft = KEY_INS;
    key_invright = KEY_DEL;
    M_BindVariable(
        b"key_jump\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_jump as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_lookUp\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_lookup as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_lookDown\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_lookdown as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_invLeft\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_invleft as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_invRight\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_invright as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_useHealth\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_usehealth as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_invquery\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_invquery as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_mission\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_mission as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_invPop\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_invpop as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_invKey\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_invkey as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_invHome\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_invhome as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_invEnd\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_invend as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_invUse\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_invuse as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_invDrop\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_invdrop as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"mouseb_jump\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut mousebjump as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"joyb_jump\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut joybjump as *mut ::core::ffi::c_void,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_BindWeaponControls() {
    M_BindVariable(
        b"key_weapon1\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_weapon1 as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_weapon2\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_weapon2 as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_weapon3\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_weapon3 as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_weapon4\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_weapon4 as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_weapon5\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_weapon5 as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_weapon6\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_weapon6 as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_weapon7\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_weapon7 as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_weapon8\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_weapon8 as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_prevweapon\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_prevweapon as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_nextweapon\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_nextweapon as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"joyb_prevweapon\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut joybprevweapon as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"joyb_nextweapon\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut joybnextweapon as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"mouseb_prevweapon\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut mousebprevweapon as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"mouseb_nextweapon\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut mousebnextweapon as *mut ::core::ffi::c_void,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_BindMapControls() {
    M_BindVariable(
        b"key_map_north\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_map_north as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_map_south\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_map_south as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_map_east\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_map_east as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_map_west\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_map_west as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_map_zoomin\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_map_zoomin as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_map_zoomout\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_map_zoomout as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_map_toggle\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_map_toggle as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_map_maxzoom\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_map_maxzoom as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_map_follow\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_map_follow as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_map_grid\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_map_grid as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_map_mark\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_map_mark as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_map_clearmark\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_map_clearmark as *mut ::core::ffi::c_void,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_BindMenuControls() {
    M_BindVariable(
        b"key_menu_activate\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_activate as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_up\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_up as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_down\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_down as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_left\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_left as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_right\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_right as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_back\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_back as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_forward\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_forward as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_confirm\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_confirm as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_abort\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_abort as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_help\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_help as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_save\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_save as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_load\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_load as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_volume\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_volume as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_detail\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_detail as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_qsave\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_qsave as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_endgame\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_endgame as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_messages\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_messages as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_qload\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_qload as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_quit\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_quit as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_gamma\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_gamma as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_incscreen\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_incscreen as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_decscreen\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_decscreen as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_menu_screenshot\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_menu_screenshot as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_demo_quit\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_demo_quit as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"key_spy\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_spy as *mut ::core::ffi::c_void,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_BindChatControls(mut num_players: ::core::ffi::c_uint) {
    let mut name: [::core::ffi::c_char; 32] = [0; 32];
    let mut i: ::core::ffi::c_uint = 0;
    M_BindVariable(
        b"key_multi_msg\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut key_multi_msg as *mut ::core::ffi::c_void,
    );
    i = 0 as ::core::ffi::c_uint;
    while i < num_players {
        M_snprintf_clamp(
            &raw mut name as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 32]>() as size_t,
            snprintf(
                &raw mut name as *mut ::core::ffi::c_char,
                ::core::mem::size_of::<[::core::ffi::c_char; 32]>() as size_t,
                b"key_multi_msgplayer%i\0".as_ptr() as *const ::core::ffi::c_char,
                i.wrapping_add(1 as ::core::ffi::c_uint),
            ),
        );
        M_BindVariable(
            &raw mut name as *mut ::core::ffi::c_char,
            (&raw mut key_multi_msgplayer as *mut ::core::ffi::c_int).offset(i as isize)
                as *mut ::core::ffi::c_int as *mut ::core::ffi::c_void,
        );
        i = i.wrapping_add(1);
    }
}
#[no_mangle]
pub unsafe extern "C" fn M_ApplyPlatformDefaults() {}
