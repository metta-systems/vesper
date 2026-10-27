//! Vesper platform: a placeholder [`Platform`].
//!
//! [`Vesper`] keeps every method's default: there is no frame output,
//! input, time, sound or file yet (so no WAD: the engine stops at
//! `D_FindIWAD`), quitting panics, and [`run`] only starts the engine and
//! ticks it. Each method will be backed by a Vesper component (the
//! compositor, `input_v1`, the timer, audio, the WAD `[[resource]]` blob)
//! as those land.

use room::engine::platform::Platform;

/// The Vesper platform the engine runs on.
struct Vesper;

impl Platform for Vesper {}

/// Start the engine and tick it forever.
pub fn run() {
    // SAFETY: called once, at startup.
    unsafe { room::engine::platform::doomgeneric_Create(&Vesper, vec!["room".to_owned()]) };
    loop {
        room::engine::doom::main::doomgeneric_Tick();
    }
}
