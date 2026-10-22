//! Rust port of vendor/doomgeneric/i_joystick.c.
//!
//! Stub for the SDL joystick backend. Chocolate-doom uses SDL_joystick to
//! open a device, read axes / buttons / hats and post `ev_joystick` events
//! into the input queue. Doomgeneric compiles all of that out behind
//! `#ifdef ORIGCODE`; only the [`I_BindJoystickVariables`] entry point
//! survives so the configuration file format remains compatible. The Rust
//! port mirrors that: the lifecycle functions are no-ops, and the
//! configuration variables are still registered with `M_BindVariable` so
//! reads/writes of `default.cfg` round-trip the joystick settings even
//! though they have no runtime effect.

#![allow(non_upper_case_globals, non_snake_case)]

use std::ffi::{c_char, c_int, c_void};

/// Number of virtual joystick buttons exposed to the config system. Must
/// match the `NUM_VIRTUAL_BUTTONS` define in `i_joystick.h`.
const NUM_VIRTUAL_BUTTONS: usize = 10;

/// Master enable flag bound to the `use_joystick` config variable.
static mut usejoystick: c_int = 0;
/// SDL joystick index to open, bound to `joystick_index`. `-1` means none
/// selected.
static mut joystick_index: c_int = -1;
/// Axis used for horizontal (turn) movement, bound to `joystick_x_axis`.
static mut joystick_x_axis: c_int = 0;
/// Inversion flag for the horizontal axis, bound to `joystick_x_invert`.
static mut joystick_x_invert: c_int = 0;
/// Axis used for vertical (forward/back) movement, bound to
/// `joystick_y_axis`.
static mut joystick_y_axis: c_int = 1;
/// Inversion flag for the vertical axis, bound to `joystick_y_invert`.
static mut joystick_y_invert: c_int = 0;
/// Axis used for strafing, bound to `joystick_strafe_axis`. `-1` disables
/// strafing.
static mut joystick_strafe_axis: c_int = -1;
/// Inversion flag for the strafe axis, bound to `joystick_strafe_invert`.
static mut joystick_strafe_invert: c_int = 0;
/// Virtual-to-physical button mapping table, bound element-wise to
/// `joystick_physical_button0` .. `joystick_physical_button9`. Default is
/// the identity mapping.
static mut joystick_physical_buttons: [c_int; NUM_VIRTUAL_BUTTONS] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];

extern "C" {
    /// Bind a config variable name to its backing storage. Defined in
    /// `m_config.c`; called below to expose the joystick statics to the
    /// `default.cfg` parser.
    fn M_BindVariable(name: *mut c_char, variable: *mut c_void);
}

use crate::c_write;

/// Initialise the joystick subsystem.
///
/// No-op stub. The original opened the SDL joystick, validated the
/// configured axes and registered an at-exit hook to shut it down. This
/// port has no joystick backend.
#[no_mangle]
pub extern "C" fn I_InitJoystick() {}

/// Shut down the joystick subsystem. No-op stub.
#[no_mangle]
pub extern "C" fn I_ShutdownJoystick() {}

/// Sample the joystick state and post an `ev_joystick` event into the
/// input queue.
///
/// No-op stub. The original read button mask plus three axes and called
/// `D_PostEvent`. With no joystick backend the input queue simply never
/// sees joystick events.
#[no_mangle]
pub extern "C" fn I_UpdateJoystick() {}

/// Register all joystick configuration variables with `M_BindVariable` so
/// they are persisted to / loaded from `default.cfg`.
///
/// Although the runtime hooks are no-ops, the config bindings are still
/// installed: this preserves the on-disk config schema and lets the user
/// edit values that a future backend could honour. Variable names and
/// order match the chocolate-doom original byte-for-byte so the resulting
/// `default.cfg` is round-trip compatible.
#[no_mangle]
pub extern "C" fn I_BindJoystickVariables() {
    unsafe {
        M_BindVariable(
            c"use_joystick".as_ptr().cast_mut(),
            &raw mut usejoystick as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            c"joystick_index".as_ptr().cast_mut(),
            &raw mut joystick_index as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            c"joystick_x_axis".as_ptr().cast_mut(),
            &raw mut joystick_x_axis as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            c"joystick_y_axis".as_ptr().cast_mut(),
            &raw mut joystick_y_axis as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            c"joystick_strafe_axis".as_ptr().cast_mut(),
            &raw mut joystick_strafe_axis as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            c"joystick_x_invert".as_ptr().cast_mut(),
            &raw mut joystick_x_invert as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            c"joystick_y_invert".as_ptr().cast_mut(),
            &raw mut joystick_y_invert as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            c"joystick_strafe_invert".as_ptr().cast_mut(),
            &raw mut joystick_strafe_invert as *mut c_int as *mut c_void,
        );

        for i in 0..NUM_VIRTUAL_BUTTONS {
            let mut name: [c_char; 32] = [0; 32];
            c_write!(name, "joystick_physical_button{}", i);
            M_BindVariable(
                name.as_ptr() as *mut c_char,
                &mut joystick_physical_buttons[i] as *mut c_int as *mut c_void,
            );
        }
    }
}
