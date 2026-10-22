//! The platform the engine runs on.
//!
//! The engine (`room::doom`) reaches the platform only through `extern "C"`
//! `DG_*` functions, which each platform module exports with `#[no_mangle]`,
//! plus a `run()` that starts the game:
//!
//! | Function | Purpose |
//! |----------|---------|
//! | `DG_Init` | Platform initialisation, inside `doomgeneric_Create` |
//! | `DG_DrawFrame` | Present the frame in `DG_ScreenBuffer` |
//! | `DG_SleepMs` | Sleep for a number of milliseconds |
//! | `DG_GetTicksMs` | Milliseconds since startup |
//! | `DG_GetKey` | Pop the next key event |
//! | `DG_SetWindowTitle` | Show the game's title |
//! | `DG_SoundInit`, `DG_SoundShutdown` | Open and close the audio output |
//! | `DG_SoundStart`, `DG_SoundStop`, `DG_SoundUpdate`, `DG_SoundIsPlaying` | Play DMX sound lumps on 8 channels |
//! | `DG_MusicInit`, `DG_MusicPlay`, `DG_MusicStop`, `DG_MusicPause`, `DG_MusicResume`, `DG_MusicSetVolume`, `DG_MusicIsPlaying` | Play MIDI songs |
//!
//! The first six are doomgeneric's; the audio hooks are this port's, called
//! from `room::doom::i_sound`.
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
