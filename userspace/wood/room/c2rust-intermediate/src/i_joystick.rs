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
pub const NUM_VIRTUAL_BUTTONS: ::core::ffi::c_int = 10 as ::core::ffi::c_int;
static mut usejoystick: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
static mut joystick_index: ::core::ffi::c_int = -1 as ::core::ffi::c_int;
static mut joystick_x_axis: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
static mut joystick_x_invert: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
static mut joystick_y_axis: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
static mut joystick_y_invert: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
static mut joystick_strafe_axis: ::core::ffi::c_int = -1 as ::core::ffi::c_int;
static mut joystick_strafe_invert: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
static mut joystick_physical_buttons: [::core::ffi::c_int; 10] = [
    0 as ::core::ffi::c_int,
    1 as ::core::ffi::c_int,
    2 as ::core::ffi::c_int,
    3 as ::core::ffi::c_int,
    4 as ::core::ffi::c_int,
    5 as ::core::ffi::c_int,
    6 as ::core::ffi::c_int,
    7 as ::core::ffi::c_int,
    8 as ::core::ffi::c_int,
    9 as ::core::ffi::c_int,
];
#[no_mangle]
pub unsafe extern "C" fn I_ShutdownJoystick() {}
#[no_mangle]
pub unsafe extern "C" fn I_InitJoystick() {}
#[no_mangle]
pub unsafe extern "C" fn I_UpdateJoystick() {}
#[no_mangle]
pub unsafe extern "C" fn I_BindJoystickVariables() {
    let mut i: ::core::ffi::c_int = 0;
    M_BindVariable(
        b"use_joystick\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut usejoystick as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"joystick_index\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut joystick_index as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"joystick_x_axis\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut joystick_x_axis as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"joystick_y_axis\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut joystick_y_axis as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"joystick_strafe_axis\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        &raw mut joystick_strafe_axis as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"joystick_x_invert\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut joystick_x_invert as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"joystick_y_invert\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut joystick_y_invert as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"joystick_strafe_invert\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
        &raw mut joystick_strafe_invert as *mut ::core::ffi::c_void,
    );
    i = 0 as ::core::ffi::c_int;
    while i < NUM_VIRTUAL_BUTTONS {
        let mut name: [::core::ffi::c_char; 32] = [0; 32];
        M_snprintf_clamp(
            &raw mut name as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 32]>() as size_t,
            snprintf(
                &raw mut name as *mut ::core::ffi::c_char,
                ::core::mem::size_of::<[::core::ffi::c_char; 32]>() as size_t,
                b"joystick_physical_button%i\0".as_ptr() as *const ::core::ffi::c_char,
                i,
            ),
        );
        M_BindVariable(
            &raw mut name as *mut ::core::ffi::c_char,
            (&raw mut joystick_physical_buttons as *mut ::core::ffi::c_int).offset(i as isize)
                as *mut ::core::ffi::c_int as *mut ::core::ffi::c_void,
        );
        i += 1;
    }
}
