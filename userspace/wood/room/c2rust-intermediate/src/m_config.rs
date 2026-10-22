extern "C" {
    fn printf(__format: *const ::core::ffi::c_char, ...) -> ::core::ffi::c_int;
    fn snprintf(
        __s: *mut ::core::ffi::c_char,
        __maxlen: size_t,
        __format: *const ::core::ffi::c_char,
        ...
    ) -> ::core::ffi::c_int;
    fn sscanf(
        __s: *const ::core::ffi::c_char,
        __format: *const ::core::ffi::c_char,
        ...
    ) -> ::core::ffi::c_int;
    fn atof(__nptr: *const ::core::ffi::c_char) -> ::core::ffi::c_double;
    fn malloc(__size: size_t) -> *mut ::core::ffi::c_void;
    fn strcmp(
        __s1: *const ::core::ffi::c_char,
        __s2: *const ::core::ffi::c_char,
    ) -> ::core::ffi::c_int;
    fn strdup(__s: *const ::core::ffi::c_char) -> *mut ::core::ffi::c_char;
    fn I_ErrorV(msg: *const ::core::ffi::c_char);
    static mut myargv: *mut *mut ::core::ffi::c_char;
    fn M_CheckParmWithArgs(
        check: *mut ::core::ffi::c_char,
        num_args: ::core::ffi::c_int,
    ) -> ::core::ffi::c_int;
    fn M_MakeDirectory(dir: *mut ::core::ffi::c_char);
    fn M_StringJoinA(strs: *const *const ::core::ffi::c_char) -> *mut ::core::ffi::c_char;
}
pub type size_t = usize;
pub type boolean = ::core::ffi::c_uint;
pub type default_type_t = ::core::ffi::c_uint;
pub const DEFAULT_KEY: default_type_t = 4;
pub const DEFAULT_FLOAT: default_type_t = 3;
pub const DEFAULT_STRING: default_type_t = 2;
pub const DEFAULT_INT_HEX: default_type_t = 1;
pub const DEFAULT_INT: default_type_t = 0;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct default_t {
    pub name: *mut ::core::ffi::c_char,
    pub location: *mut ::core::ffi::c_void,
    pub type_0: default_type_t,
    pub untranslated: ::core::ffi::c_int,
    pub original_translated: ::core::ffi::c_int,
    pub bound: boolean,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct default_collection_t {
    pub defaults: *mut default_t,
    pub numdefaults: ::core::ffi::c_int,
    pub filename: *mut ::core::ffi::c_char,
}
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
pub const true_0: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
pub const false_0: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
pub const KEY_RIGHTARROW: ::core::ffi::c_int = 0xae as ::core::ffi::c_int;
pub const KEY_LEFTARROW: ::core::ffi::c_int = 0xac as ::core::ffi::c_int;
pub const KEY_UPARROW: ::core::ffi::c_int = 0xad as ::core::ffi::c_int;
pub const KEY_DOWNARROW: ::core::ffi::c_int = 0xaf as ::core::ffi::c_int;
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
pub const KEY_MINUS: ::core::ffi::c_int = 0x2d as ::core::ffi::c_int;
pub const KEY_RSHIFT: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x36 as ::core::ffi::c_int;
pub const KEY_RCTRL: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x1d as ::core::ffi::c_int;
pub const KEY_RALT: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x38 as ::core::ffi::c_int;
pub const KEY_CAPSLOCK: ::core::ffi::c_int =
    0x80 as ::core::ffi::c_int + 0x3a as ::core::ffi::c_int;
pub const KEY_SCRLCK: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x46 as ::core::ffi::c_int;
pub const KEY_PRTSCR: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x59 as ::core::ffi::c_int;
pub const KEY_HOME: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x47 as ::core::ffi::c_int;
pub const KEY_END: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x4f as ::core::ffi::c_int;
pub const KEY_PGUP: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x49 as ::core::ffi::c_int;
pub const KEY_PGDN: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x51 as ::core::ffi::c_int;
pub const KEY_INS: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x52 as ::core::ffi::c_int;
pub const KEY_DEL: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x53 as ::core::ffi::c_int;
pub const KEYP_5: ::core::ffi::c_int = '5' as ::core::ffi::c_int;
pub const KEYP_PLUS: ::core::ffi::c_int = '+' as ::core::ffi::c_int;
pub const KEYP_MULTIPLY: ::core::ffi::c_int = '*' as ::core::ffi::c_int;
#[no_mangle]
pub static mut configdir: *mut ::core::ffi::c_char = ::core::ptr::null_mut::<::core::ffi::c_char>();
static mut default_main_config: *mut ::core::ffi::c_char =
    ::core::ptr::null_mut::<::core::ffi::c_char>();
static mut default_extra_config: *mut ::core::ffi::c_char =
    ::core::ptr::null_mut::<::core::ffi::c_char>();
static mut doom_defaults_list: [default_t; 76] = [
    default_t {
        name: b"mouse_sensitivity\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"sfx_volume\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"music_volume\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"show_talk\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"voice_volume\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"show_messages\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_right\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_left\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_up\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_down\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_strafeleft\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_straferight\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_useHealth\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_jump\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_flyup\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_flydown\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_flycenter\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_lookup\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_lookdown\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_lookcenter\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_invquery\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_mission\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_invPop\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_invKey\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_invHome\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_invEnd\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_invleft\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_invright\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_invLeft\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_invRight\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_useartifact\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_invUse\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_invDrop\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_lookUp\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_lookDown\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_fire\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_use\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_strafe\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_speed\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"use_mouse\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"mouseb_fire\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"mouseb_strafe\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"mouseb_forward\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"mouseb_jump\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"use_joystick\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joyb_fire\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joyb_strafe\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joyb_use\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joyb_speed\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joyb_jump\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"screenblocks\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"screensize\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"detaillevel\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"snd_channels\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"snd_musicdevice\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"snd_sfxdevice\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"snd_sbport\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"snd_sbirq\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"snd_sbdma\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"snd_mport\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"usegamma\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"savedir\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_STRING,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"messageson\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"back_flat\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_STRING,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"nickname\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_STRING,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"chatmacro0\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_STRING,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"chatmacro1\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_STRING,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"chatmacro2\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_STRING,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"chatmacro3\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_STRING,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"chatmacro4\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_STRING,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"chatmacro5\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_STRING,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"chatmacro6\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_STRING,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"chatmacro7\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_STRING,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"chatmacro8\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_STRING,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"chatmacro9\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_STRING,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"comport\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
];
static mut doom_defaults: default_collection_t = default_collection_t {
    defaults: ::core::ptr::null_mut::<default_t>(),
    numdefaults: 0,
    filename: ::core::ptr::null_mut::<::core::ffi::c_char>(),
};
static mut extra_defaults_list: [default_t; 119] = [
    default_t {
        name: b"graphical_startup\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"autoadjust_video_settings\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"fullscreen\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"aspect_ratio_correct\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"startup_delay\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"screen_width\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"screen_height\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"screen_bpp\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"grabmouse\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"novert\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"mouse_acceleration\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_FLOAT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"mouse_threshold\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"snd_samplerate\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"snd_cachesize\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"snd_maxslicetime_ms\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"snd_musiccmd\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_STRING,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"opl_io_port\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT_HEX,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"show_endoom\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"png_screenshots\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"vanilla_savegame_limit\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"vanilla_demo_limit\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"vanilla_keyboard_mapping\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"video_driver\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_STRING,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"window_position\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_STRING,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joystick_index\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joystick_x_axis\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joystick_x_invert\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joystick_y_axis\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joystick_y_invert\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joystick_strafe_axis\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joystick_strafe_invert\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joystick_physical_button0\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joystick_physical_button1\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joystick_physical_button2\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joystick_physical_button3\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joystick_physical_button4\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joystick_physical_button5\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joystick_physical_button6\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joystick_physical_button7\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joystick_physical_button8\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joystick_physical_button9\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joyb_strafeleft\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joyb_straferight\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joyb_menu_activate\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joyb_prevweapon\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"joyb_nextweapon\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"mouseb_strafeleft\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"mouseb_straferight\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"mouseb_use\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"mouseb_backward\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"mouseb_prevweapon\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"mouseb_nextweapon\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"dclick_use\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_INT,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_pause\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_activate\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_up\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_down\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_left\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_right\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_back\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_forward\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_confirm\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_abort\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_help\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_save\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_load\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_volume\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_detail\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_qsave\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_endgame\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_messages\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_qload\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_quit\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_gamma\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_spy\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_incscreen\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_decscreen\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_menu_screenshot\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_map_toggle\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_map_north\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_map_south\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_map_east\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_map_west\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_map_zoomin\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_map_zoomout\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_map_maxzoom\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_map_follow\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_map_grid\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_map_mark\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_map_clearmark\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_weapon1\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_weapon2\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_weapon3\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_weapon4\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_weapon5\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_weapon6\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_weapon7\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_weapon8\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_prevweapon\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_nextweapon\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_arti_all\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_arti_health\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_arti_poisonbag\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_arti_blastradius\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_arti_teleport\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_arti_teleportother\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_arti_egg\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_arti_invulnerability\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_message_refresh\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_demo_quit\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_multi_msg\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_multi_msgplayer1\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_multi_msgplayer2\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_multi_msgplayer3\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_multi_msgplayer4\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_multi_msgplayer5\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_multi_msgplayer6\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_multi_msgplayer7\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
    default_t {
        name: b"key_multi_msgplayer8\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        location: NULL,
        type_0: DEFAULT_KEY,
        untranslated: 0 as ::core::ffi::c_int,
        original_translated: 0 as ::core::ffi::c_int,
        bound: false_0 as boolean,
    },
];
static mut extra_defaults: default_collection_t = default_collection_t {
    defaults: ::core::ptr::null_mut::<default_t>(),
    numdefaults: 0,
    filename: ::core::ptr::null_mut::<::core::ffi::c_char>(),
};
unsafe extern "C" fn SearchCollection(
    mut collection: *mut default_collection_t,
    mut name: *mut ::core::ffi::c_char,
) -> *mut default_t {
    let mut i: ::core::ffi::c_int = 0;
    i = 0 as ::core::ffi::c_int;
    while i < (*collection).numdefaults {
        if strcmp(name, (*(*collection).defaults.offset(i as isize)).name) == 0 {
            return (*collection).defaults.offset(i as isize) as *mut default_t;
        }
        i += 1;
    }
    return ::core::ptr::null_mut::<default_t>();
}
static mut scantokey: [::core::ffi::c_int; 128] = [
    0 as ::core::ffi::c_int,
    27 as ::core::ffi::c_int,
    '1' as ::core::ffi::c_int,
    '2' as ::core::ffi::c_int,
    '3' as ::core::ffi::c_int,
    '4' as ::core::ffi::c_int,
    '5' as ::core::ffi::c_int,
    '6' as ::core::ffi::c_int,
    '7' as ::core::ffi::c_int,
    '8' as ::core::ffi::c_int,
    '9' as ::core::ffi::c_int,
    '0' as ::core::ffi::c_int,
    '-' as ::core::ffi::c_int,
    '=' as ::core::ffi::c_int,
    KEY_BACKSPACE,
    9 as ::core::ffi::c_int,
    'q' as ::core::ffi::c_int,
    'w' as ::core::ffi::c_int,
    'e' as ::core::ffi::c_int,
    'r' as ::core::ffi::c_int,
    't' as ::core::ffi::c_int,
    'y' as ::core::ffi::c_int,
    'u' as ::core::ffi::c_int,
    'i' as ::core::ffi::c_int,
    'o' as ::core::ffi::c_int,
    'p' as ::core::ffi::c_int,
    '[' as ::core::ffi::c_int,
    ']' as ::core::ffi::c_int,
    13 as ::core::ffi::c_int,
    KEY_RCTRL,
    'a' as ::core::ffi::c_int,
    's' as ::core::ffi::c_int,
    'd' as ::core::ffi::c_int,
    'f' as ::core::ffi::c_int,
    'g' as ::core::ffi::c_int,
    'h' as ::core::ffi::c_int,
    'j' as ::core::ffi::c_int,
    'k' as ::core::ffi::c_int,
    'l' as ::core::ffi::c_int,
    ';' as ::core::ffi::c_int,
    '\'' as ::core::ffi::c_int,
    '`' as ::core::ffi::c_int,
    KEY_RSHIFT,
    '\\' as ::core::ffi::c_int,
    'z' as ::core::ffi::c_int,
    'x' as ::core::ffi::c_int,
    'c' as ::core::ffi::c_int,
    'v' as ::core::ffi::c_int,
    'b' as ::core::ffi::c_int,
    'n' as ::core::ffi::c_int,
    'm' as ::core::ffi::c_int,
    ',' as ::core::ffi::c_int,
    '.' as ::core::ffi::c_int,
    '/' as ::core::ffi::c_int,
    KEY_RSHIFT,
    KEYP_MULTIPLY,
    KEY_RALT,
    ' ' as ::core::ffi::c_int,
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
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    KEY_F11,
    KEY_F12,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    0 as ::core::ffi::c_int,
    KEY_PRTSCR,
    0 as ::core::ffi::c_int,
];
unsafe extern "C" fn SaveDefaultCollection(mut collection: *mut default_collection_t) {}
unsafe extern "C" fn ParseIntParameter(
    mut strparm: *mut ::core::ffi::c_char,
) -> ::core::ffi::c_int {
    let mut parm: ::core::ffi::c_int = 0;
    if *strparm.offset(0 as ::core::ffi::c_int as isize) as ::core::ffi::c_int
        == '0' as ::core::ffi::c_int
        && *strparm.offset(1 as ::core::ffi::c_int as isize) as ::core::ffi::c_int
            == 'x' as ::core::ffi::c_int
    {
        sscanf(
            strparm.offset(2 as ::core::ffi::c_int as isize),
            b"%x\0".as_ptr() as *const ::core::ffi::c_char,
            &raw mut parm,
        );
    } else {
        sscanf(
            strparm,
            b"%i\0".as_ptr() as *const ::core::ffi::c_char,
            &raw mut parm,
        );
    }
    return parm;
}
unsafe extern "C" fn SetVariable(mut def: *mut default_t, mut value: *mut ::core::ffi::c_char) {
    let mut intparm: ::core::ffi::c_int = 0;
    match (*def).type_0 as ::core::ffi::c_uint {
        2 => {
            let ref mut c2rust_fresh0 = *((*def).location as *mut *mut ::core::ffi::c_char);
            *c2rust_fresh0 = strdup(value);
        }
        0 | 1 => {
            *((*def).location as *mut ::core::ffi::c_int) = ParseIntParameter(value);
        }
        4 => {
            intparm = ParseIntParameter(value);
            (*def).untranslated = intparm;
            if intparm >= 0 as ::core::ffi::c_int && intparm < 128 as ::core::ffi::c_int {
                intparm = scantokey[intparm as usize];
            } else {
                intparm = 0 as ::core::ffi::c_int;
            }
            (*def).original_translated = intparm;
            *((*def).location as *mut ::core::ffi::c_int) = intparm;
        }
        3 => {
            *((*def).location as *mut ::core::ffi::c_float) = atof(value) as ::core::ffi::c_float;
        }
        _ => {}
    };
}
unsafe extern "C" fn LoadDefaultCollection(mut collection: *mut default_collection_t) {}
#[no_mangle]
pub unsafe extern "C" fn M_SetConfigFilenames(
    mut main_config: *mut ::core::ffi::c_char,
    mut extra_config: *mut ::core::ffi::c_char,
) {
    default_main_config = main_config;
    default_extra_config = extra_config;
}
#[no_mangle]
pub unsafe extern "C" fn M_SaveDefaults() {
    SaveDefaultCollection(&raw mut doom_defaults);
    SaveDefaultCollection(&raw mut extra_defaults);
}
#[no_mangle]
pub unsafe extern "C" fn M_SaveDefaultsAlternate(
    mut main_0: *mut ::core::ffi::c_char,
    mut extra: *mut ::core::ffi::c_char,
) {
    let mut orig_main: *mut ::core::ffi::c_char = ::core::ptr::null_mut::<::core::ffi::c_char>();
    let mut orig_extra: *mut ::core::ffi::c_char = ::core::ptr::null_mut::<::core::ffi::c_char>();
    orig_main = doom_defaults.filename;
    orig_extra = extra_defaults.filename;
    doom_defaults.filename = main_0;
    extra_defaults.filename = extra;
    M_SaveDefaults();
    doom_defaults.filename = orig_main;
    extra_defaults.filename = orig_extra;
}
#[no_mangle]
pub unsafe extern "C" fn M_LoadDefaults() {
    let mut i: ::core::ffi::c_int = 0;
    i = M_CheckParmWithArgs(
        b"-config\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        1 as ::core::ffi::c_int,
    );
    if i != 0 {
        doom_defaults.filename = *myargv.offset((i + 1 as ::core::ffi::c_int) as isize);
        printf(
            b"\tdefault file: %s\n\0".as_ptr() as *const ::core::ffi::c_char,
            doom_defaults.filename,
        );
    } else {
        let c2rust_fresh1: [*const ::core::ffi::c_char; 3] = [
            configdir as *const ::core::ffi::c_char,
            default_main_config as *const ::core::ffi::c_char,
            ::core::ptr::null::<::core::ffi::c_char>(),
        ];
        doom_defaults.filename =
            M_StringJoinA(&raw const c2rust_fresh1 as *const *const ::core::ffi::c_char);
    }
    printf(
        b"saving config in %s\n\0".as_ptr() as *const ::core::ffi::c_char,
        doom_defaults.filename,
    );
    i = M_CheckParmWithArgs(
        b"-extraconfig\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        1 as ::core::ffi::c_int,
    );
    if i != 0 {
        extra_defaults.filename = *myargv.offset((i + 1 as ::core::ffi::c_int) as isize);
        printf(
            b"        extra configuration file: %s\n\0".as_ptr() as *const ::core::ffi::c_char,
            extra_defaults.filename,
        );
    } else {
        let c2rust_fresh2: [*const ::core::ffi::c_char; 3] = [
            configdir as *const ::core::ffi::c_char,
            default_extra_config as *const ::core::ffi::c_char,
            ::core::ptr::null::<::core::ffi::c_char>(),
        ];
        extra_defaults.filename =
            M_StringJoinA(&raw const c2rust_fresh2 as *const *const ::core::ffi::c_char);
    }
    LoadDefaultCollection(&raw mut doom_defaults);
    LoadDefaultCollection(&raw mut extra_defaults);
}
unsafe extern "C" fn GetDefaultForName(mut name: *mut ::core::ffi::c_char) -> *mut default_t {
    let mut result: *mut default_t = ::core::ptr::null_mut::<default_t>();
    result = SearchCollection(&raw mut doom_defaults, name);
    if result.is_null() {
        result = SearchCollection(&raw mut extra_defaults, name);
    }
    if result.is_null() {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"Unknown configuration variable: '%s'\0".as_ptr() as *const ::core::ffi::c_char,
            name,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    return result;
}
#[no_mangle]
pub unsafe extern "C" fn M_BindVariable(
    mut name: *mut ::core::ffi::c_char,
    mut location: *mut ::core::ffi::c_void,
) {
    let mut variable: *mut default_t = ::core::ptr::null_mut::<default_t>();
    variable = GetDefaultForName(name);
    (*variable).location = location;
    (*variable).bound = true_0 as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn M_SetVariable(
    mut name: *mut ::core::ffi::c_char,
    mut value: *mut ::core::ffi::c_char,
) -> boolean {
    let mut variable: *mut default_t = ::core::ptr::null_mut::<default_t>();
    variable = GetDefaultForName(name);
    if variable.is_null() || (*variable).bound == 0 {
        return false_0 as boolean;
    }
    SetVariable(variable, value);
    return true_0 as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn M_GetIntVariable(
    mut name: *mut ::core::ffi::c_char,
) -> ::core::ffi::c_int {
    let mut variable: *mut default_t = ::core::ptr::null_mut::<default_t>();
    variable = GetDefaultForName(name);
    if variable.is_null()
        || (*variable).bound == 0
        || (*variable).type_0 as ::core::ffi::c_uint
            != DEFAULT_INT as ::core::ffi::c_int as ::core::ffi::c_uint
            && (*variable).type_0 as ::core::ffi::c_uint
                != DEFAULT_INT_HEX as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        return 0 as ::core::ffi::c_int;
    }
    return *((*variable).location as *mut ::core::ffi::c_int);
}
#[no_mangle]
pub unsafe extern "C" fn M_GetStrVariable(
    mut name: *mut ::core::ffi::c_char,
) -> *const ::core::ffi::c_char {
    let mut variable: *mut default_t = ::core::ptr::null_mut::<default_t>();
    variable = GetDefaultForName(name);
    if variable.is_null()
        || (*variable).bound == 0
        || (*variable).type_0 as ::core::ffi::c_uint
            != DEFAULT_STRING as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        return ::core::ptr::null::<::core::ffi::c_char>();
    }
    return *((*variable).location as *mut *const ::core::ffi::c_char);
}
#[no_mangle]
pub unsafe extern "C" fn M_GetFloatVariable(
    mut name: *mut ::core::ffi::c_char,
) -> ::core::ffi::c_float {
    let mut variable: *mut default_t = ::core::ptr::null_mut::<default_t>();
    variable = GetDefaultForName(name);
    if variable.is_null()
        || (*variable).bound == 0
        || (*variable).type_0 as ::core::ffi::c_uint
            != DEFAULT_FLOAT as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        return 0 as ::core::ffi::c_int as ::core::ffi::c_float;
    }
    return *((*variable).location as *mut ::core::ffi::c_float);
}
unsafe extern "C" fn GetDefaultConfigDir() -> *mut ::core::ffi::c_char {
    let mut result: *mut ::core::ffi::c_char = malloc(2 as size_t) as *mut ::core::ffi::c_char;
    *result.offset(0 as ::core::ffi::c_int as isize) = '.' as ::core::ffi::c_char;
    *result.offset(1 as ::core::ffi::c_int as isize) = '\0' as ::core::ffi::c_char;
    return result;
}
#[no_mangle]
pub unsafe extern "C" fn M_SetConfigDir(mut dir: *mut ::core::ffi::c_char) {
    if !dir.is_null() {
        configdir = dir;
    } else {
        configdir = GetDefaultConfigDir();
    }
    if strcmp(configdir, b"\0".as_ptr() as *const ::core::ffi::c_char) != 0 as ::core::ffi::c_int {
        printf(
            b"Using %s for configuration and saves\n\0".as_ptr() as *const ::core::ffi::c_char,
            configdir,
        );
    }
    M_MakeDirectory(configdir);
}
#[no_mangle]
pub unsafe extern "C" fn M_GetSaveGameDir(
    mut iwadname: *mut ::core::ffi::c_char,
) -> *mut ::core::ffi::c_char {
    let mut savegamedir: *mut ::core::ffi::c_char = ::core::ptr::null_mut::<::core::ffi::c_char>();
    if strcmp(configdir, b"\0".as_ptr() as *const ::core::ffi::c_char) == 0 {
        savegamedir = strdup(b"\0".as_ptr() as *const ::core::ffi::c_char);
    } else {
        let c2rust_fresh3: [*const ::core::ffi::c_char; 4] = [
            configdir as *const ::core::ffi::c_char,
            b"/\0".as_ptr() as *const ::core::ffi::c_char,
            b".savegame/\0".as_ptr() as *const ::core::ffi::c_char,
            ::core::ptr::null::<::core::ffi::c_char>(),
        ];
        savegamedir = M_StringJoinA(&raw const c2rust_fresh3 as *const *const ::core::ffi::c_char);
        M_MakeDirectory(savegamedir);
        printf(
            b"Using %s for savegames\n\0".as_ptr() as *const ::core::ffi::c_char,
            savegamedir,
        );
    }
    return savegamedir;
}
unsafe extern "C" fn c2rust_run_static_initializers() {
    doom_defaults = default_collection_t {
        defaults: &raw mut doom_defaults_list as *mut default_t,
        numdefaults: (::core::mem::size_of::<[default_t; 76]>() as usize)
            .wrapping_div(::core::mem::size_of::<default_t>() as usize)
            as ::core::ffi::c_int,
        filename: ::core::ptr::null_mut::<::core::ffi::c_char>(),
    };
    extra_defaults = default_collection_t {
        defaults: &raw mut extra_defaults_list as *mut default_t,
        numdefaults: (::core::mem::size_of::<[default_t; 119]>() as usize)
            .wrapping_div(::core::mem::size_of::<default_t>() as usize)
            as ::core::ffi::c_int,
        filename: ::core::ptr::null_mut::<::core::ffi::c_char>(),
    };
}
#[used]
#[cfg_attr(target_os = "linux", link_section = ".init_array")]
#[cfg_attr(target_os = "windows", link_section = ".CRT$XIB")]
#[cfg_attr(target_os = "macos", link_section = "__DATA,__mod_init_func")]
static INIT_ARRAY: [unsafe extern "C" fn(); 1] = [c2rust_run_static_initializers];
