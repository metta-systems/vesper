//! Vesper platform: a placeholder [`Platform`].
//!
//! [`Vesper`] keeps every method's do-nothing default: there is no frame
//! output, input, time or sound yet, and [`run`] only starts the engine and
//! ticks it. Each method will be backed by a Vesper component (the
//! compositor, `input_v1`, the timer, audio) as those land.

use room::doom::doomgeneric::Platform;

/// The Vesper platform the engine runs on.
struct Vesper;

impl Platform for Vesper {}

/// Start the engine and tick it forever.
pub fn run() {
    // SAFETY: called once, at startup.
    unsafe { room::doom::doomgeneric::doomgeneric_Create(&Vesper, vec!["room".to_owned()]) };
    loop {
        room::doom::d_main::doomgeneric_Tick();
    }
}
