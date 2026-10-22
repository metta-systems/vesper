extern "C" {
    fn D_PostEvent(ev: *mut event_t);
    fn DG_GetKey(
        pressed: *mut ::core::ffi::c_int,
        key: *mut ::core::ffi::c_uchar,
    ) -> ::core::ffi::c_int;
}
pub type evtype_t = ::core::ffi::c_uint;
pub const ev_quit: evtype_t = 4;
pub const ev_joystick: evtype_t = 3;
pub const ev_mouse: evtype_t = 2;
pub const ev_keyup: evtype_t = 1;
pub const ev_keydown: evtype_t = 0;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct event_t {
    pub type_0: evtype_t,
    pub data1: ::core::ffi::c_int,
    pub data2: ::core::ffi::c_int,
    pub data3: ::core::ffi::c_int,
    pub data4: ::core::ffi::c_int,
}
#[no_mangle]
pub static mut vanilla_keyboard_mapping: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
static mut shiftdown: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
static mut shiftxform: [::core::ffi::c_char; 128] = [
    0 as ::core::ffi::c_char,
    1 as ::core::ffi::c_char,
    2 as ::core::ffi::c_char,
    3 as ::core::ffi::c_char,
    4 as ::core::ffi::c_char,
    5 as ::core::ffi::c_char,
    6 as ::core::ffi::c_char,
    7 as ::core::ffi::c_char,
    8 as ::core::ffi::c_char,
    9 as ::core::ffi::c_char,
    10 as ::core::ffi::c_char,
    11 as ::core::ffi::c_char,
    12 as ::core::ffi::c_char,
    13 as ::core::ffi::c_char,
    14 as ::core::ffi::c_char,
    15 as ::core::ffi::c_char,
    16 as ::core::ffi::c_char,
    17 as ::core::ffi::c_char,
    18 as ::core::ffi::c_char,
    19 as ::core::ffi::c_char,
    20 as ::core::ffi::c_char,
    21 as ::core::ffi::c_char,
    22 as ::core::ffi::c_char,
    23 as ::core::ffi::c_char,
    24 as ::core::ffi::c_char,
    25 as ::core::ffi::c_char,
    26 as ::core::ffi::c_char,
    27 as ::core::ffi::c_char,
    28 as ::core::ffi::c_char,
    29 as ::core::ffi::c_char,
    30 as ::core::ffi::c_char,
    31 as ::core::ffi::c_char,
    ' ' as ::core::ffi::c_char,
    '!' as ::core::ffi::c_char,
    '"' as ::core::ffi::c_char,
    '#' as ::core::ffi::c_char,
    '$' as ::core::ffi::c_char,
    '%' as ::core::ffi::c_char,
    '&' as ::core::ffi::c_char,
    '"' as ::core::ffi::c_char,
    '(' as ::core::ffi::c_char,
    ')' as ::core::ffi::c_char,
    '*' as ::core::ffi::c_char,
    '+' as ::core::ffi::c_char,
    '<' as ::core::ffi::c_char,
    '_' as ::core::ffi::c_char,
    '>' as ::core::ffi::c_char,
    '?' as ::core::ffi::c_char,
    ')' as ::core::ffi::c_char,
    '!' as ::core::ffi::c_char,
    '@' as ::core::ffi::c_char,
    '#' as ::core::ffi::c_char,
    '$' as ::core::ffi::c_char,
    '%' as ::core::ffi::c_char,
    '^' as ::core::ffi::c_char,
    '&' as ::core::ffi::c_char,
    '*' as ::core::ffi::c_char,
    '(' as ::core::ffi::c_char,
    ':' as ::core::ffi::c_char,
    ':' as ::core::ffi::c_char,
    '<' as ::core::ffi::c_char,
    '+' as ::core::ffi::c_char,
    '>' as ::core::ffi::c_char,
    '?' as ::core::ffi::c_char,
    '@' as ::core::ffi::c_char,
    'A' as ::core::ffi::c_char,
    'B' as ::core::ffi::c_char,
    'C' as ::core::ffi::c_char,
    'D' as ::core::ffi::c_char,
    'E' as ::core::ffi::c_char,
    'F' as ::core::ffi::c_char,
    'G' as ::core::ffi::c_char,
    'H' as ::core::ffi::c_char,
    'I' as ::core::ffi::c_char,
    'J' as ::core::ffi::c_char,
    'K' as ::core::ffi::c_char,
    'L' as ::core::ffi::c_char,
    'M' as ::core::ffi::c_char,
    'N' as ::core::ffi::c_char,
    'O' as ::core::ffi::c_char,
    'P' as ::core::ffi::c_char,
    'Q' as ::core::ffi::c_char,
    'R' as ::core::ffi::c_char,
    'S' as ::core::ffi::c_char,
    'T' as ::core::ffi::c_char,
    'U' as ::core::ffi::c_char,
    'V' as ::core::ffi::c_char,
    'W' as ::core::ffi::c_char,
    'X' as ::core::ffi::c_char,
    'Y' as ::core::ffi::c_char,
    'Z' as ::core::ffi::c_char,
    '[' as ::core::ffi::c_char,
    '!' as ::core::ffi::c_char,
    ']' as ::core::ffi::c_char,
    '"' as ::core::ffi::c_char,
    '_' as ::core::ffi::c_char,
    '\'' as ::core::ffi::c_char,
    'A' as ::core::ffi::c_char,
    'B' as ::core::ffi::c_char,
    'C' as ::core::ffi::c_char,
    'D' as ::core::ffi::c_char,
    'E' as ::core::ffi::c_char,
    'F' as ::core::ffi::c_char,
    'G' as ::core::ffi::c_char,
    'H' as ::core::ffi::c_char,
    'I' as ::core::ffi::c_char,
    'J' as ::core::ffi::c_char,
    'K' as ::core::ffi::c_char,
    'L' as ::core::ffi::c_char,
    'M' as ::core::ffi::c_char,
    'N' as ::core::ffi::c_char,
    'O' as ::core::ffi::c_char,
    'P' as ::core::ffi::c_char,
    'Q' as ::core::ffi::c_char,
    'R' as ::core::ffi::c_char,
    'S' as ::core::ffi::c_char,
    'T' as ::core::ffi::c_char,
    'U' as ::core::ffi::c_char,
    'V' as ::core::ffi::c_char,
    'W' as ::core::ffi::c_char,
    'X' as ::core::ffi::c_char,
    'Y' as ::core::ffi::c_char,
    'Z' as ::core::ffi::c_char,
    '{' as ::core::ffi::c_char,
    '|' as ::core::ffi::c_char,
    '}' as ::core::ffi::c_char,
    '~' as ::core::ffi::c_char,
    127 as ::core::ffi::c_char,
];
unsafe extern "C" fn TranslateKey(mut key: ::core::ffi::c_uchar) -> ::core::ffi::c_uchar {
    return key;
}
unsafe extern "C" fn GetTypedChar(mut key: ::core::ffi::c_uchar) -> ::core::ffi::c_uchar {
    key = TranslateKey(key);
    if shiftdown > 0 as ::core::ffi::c_int {
        if key as ::core::ffi::c_int >= 0 as ::core::ffi::c_int
            && (key as usize)
                < (::core::mem::size_of::<[::core::ffi::c_char; 128]>() as usize)
                    .wrapping_div(::core::mem::size_of::<::core::ffi::c_char>() as usize)
        {
            key = shiftxform[key as usize] as ::core::ffi::c_uchar;
        } else {
            key = 0 as ::core::ffi::c_uchar;
        }
    }
    return key;
}
unsafe extern "C" fn UpdateShiftStatus(
    mut pressed: ::core::ffi::c_int,
    mut key: ::core::ffi::c_uchar,
) {
    let mut change: ::core::ffi::c_int = 0;
    if pressed != 0 {
        change = 1 as ::core::ffi::c_int;
    } else {
        change = -1 as ::core::ffi::c_int;
    }
    if key as ::core::ffi::c_int == KEY_RSHIFT {
        shiftdown += change;
    }
}
#[no_mangle]
pub unsafe extern "C" fn I_GetEvent() {
    let mut event: event_t = event_t {
        type_0: ev_keydown,
        data1: 0,
        data2: 0,
        data3: 0,
        data4: 0,
    };
    let mut pressed: ::core::ffi::c_int = 0;
    let mut key: ::core::ffi::c_uchar = 0;
    while DG_GetKey(&raw mut pressed, &raw mut key) != 0 {
        UpdateShiftStatus(pressed, key);
        if pressed != 0 {
            event.type_0 = ev_keydown;
            event.data1 = TranslateKey(key) as ::core::ffi::c_int;
            event.data2 = GetTypedChar(key) as ::core::ffi::c_int;
            if event.data1 != 0 as ::core::ffi::c_int {
                D_PostEvent(&raw mut event);
            }
        } else {
            event.type_0 = ev_keyup;
            event.data1 = TranslateKey(key) as ::core::ffi::c_int;
            event.data2 = 0 as ::core::ffi::c_int;
            if event.data1 != 0 as ::core::ffi::c_int {
                D_PostEvent(&raw mut event);
            }
            break;
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn I_InitInput() {}
pub const KEY_RSHIFT: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x36 as ::core::ffi::c_int;
