//! Vesper platform: a placeholder [`Platform`].
//!
//! [`Vesper`] keeps every method's do-nothing default: there is no frame
//! output, input, time or sound yet, and [`run`] only starts the engine and
//! ticks it. Each method will be backed by a Vesper component (the
//! compositor, `input_v1`, the timer, audio) as those land.

use core::ffi::c_char;

use room::doom::doomgeneric::Platform;

/// The Vesper platform the engine runs on.
struct Vesper;

impl Platform for Vesper {}

/// Start the engine and tick it forever.
pub fn run() {
    let mut name = *b"room\0";
    let mut argv = [name.as_mut_ptr().cast::<c_char>(), core::ptr::null_mut()];
    // SAFETY: `argv` is a null-terminated array of one NUL-terminated
    // string, and both outlive the engine, which never returns from here.
    unsafe { room::doom::doomgeneric::doomgeneric_Create(&Vesper, 1, argv.as_mut_ptr()) };
    loop {
        room::doom::d_main::doomgeneric_Tick();
    }
}
