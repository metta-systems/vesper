//! The platform the engine runs on.
//!
//! The engine (`room::doom`) reaches the platform only through the
//! `room::doom::doomgeneric::Platform` trait: doomgeneric's six `DG_*`
//! callbacks (init, draw a frame, sleep, the clock, keys, the window title)
//! plus this port's sound and music methods, which `room::doom::i_sound`
//! calls. Each platform module implements it and has a `run()` that hands
//! its implementation to `doomgeneric_Create` and drives the game.
//!
//! - `host`: winit, wgpu and rodio, for desktop builds.
//! - `vesper`: placeholders for the Vesper EL0 component, selected on
//!   `target_os = "vesper"`, or with the `platform-vesper` feature to
//!   compile-check it on a host.

#[cfg(not(any(target_os = "vesper", feature = "platform-vesper")))]
mod host;
#[cfg(not(any(target_os = "vesper", feature = "platform-vesper")))]
pub use host::run;

#[cfg(any(target_os = "vesper", feature = "platform-vesper"))]
mod vesper;
#[cfg(any(target_os = "vesper", feature = "platform-vesper"))]
pub use vesper::run;
