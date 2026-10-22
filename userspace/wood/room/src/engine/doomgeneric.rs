//! Rust port of vendor/doomgeneric/doomgeneric.c.
//!
//! Platform abstraction entry point for the doomgeneric framework.
//!
//! `doomgeneric` is a thin portability shim that decouples the Doom engine
//! core from any particular windowing/audio backend.  A platform implements
//! [`Platform`]: doomgeneric's six callbacks (`DG_Init`, `DG_DrawFrame`,
//! `DG_SleepMs`, `DG_GetTicksMs`, `DG_GetKey`, `DG_SetWindowTitle`) plus this
//! port's sound and music hooks; the engine calls these instead of talking to
//! the OS directly.
//!
//! This module covers the library side: it owns the shared frame-buffer
//! pointer [`DG_ScreenBuffer`] and implements [`doomgeneric_Create`], which
//! records the platform, allocates that buffer, stores the command-line
//! arguments, and launches the main engine entry point.
//!
//! Rust differences from C:
//! - The C code allocates the screen buffer with `malloc` and never frees it
//!   (intentional leak in a long-running process).  This port allocates via
//!   `Vec<u32>` and uses `std::mem::forget` to produce the same intentional
//!   leak, while using Rust allocation.
//! - The platform callbacks are the methods of [`Platform`], installed by
//!   [`doomgeneric_Create`], instead of functions the platform links in.

#![allow(non_upper_case_globals, non_snake_case)]

use std::ptr;

use super::{
    doom::main::D_DoomMain,
    misc::argv::{myargv, M_FindResponseFile},
};

/// Width of the doomgeneric frame buffer in pixels.
///
/// Matches `DOOMGENERIC_RESX` in `doomgeneric.h` (default 640).
pub const DOOMGENERIC_RESX: usize = 640;

/// Height of the doomgeneric frame buffer in pixels.
///
/// Matches `DOOMGENERIC_RESY` in `doomgeneric.h` (default 400).
pub const DOOMGENERIC_RESY: usize = 400;

/// Total pixel count for one complete frame (`RESX * RESY`).
///
/// Convenience constant used when allocating or iterating over [`DG_ScreenBuffer`].
pub const DOOMGENERIC_PIXELS: usize = DOOMGENERIC_RESX * DOOMGENERIC_RESY;

/// Shared frame-buffer pointer written by the renderer and read by the platform backend.
///
/// Corresponds to `pixel_t *DG_ScreenBuffer` in `doomgeneric.c`.  After
/// [`doomgeneric_Create`] runs, this points to a heap-allocated array of
/// [`DOOMGENERIC_PIXELS`] `u32` values (RGBA8 or BGRA8 depending on the
/// build).  The renderer (`R_DrawColumn`, `R_DrawSpan`, etc.) writes pixels
/// into this buffer each frame; the platform backend reads from it inside
/// `DG_DrawFrame` to upload the frame to the display.
///
/// Initialised to `null`; valid only after [`doomgeneric_Create`] has been
/// called.  Referenced from C as `DG_ScreenBuffer`.
pub static mut DG_ScreenBuffer: *mut u32 = ptr::null_mut();

/// What the engine needs from the platform it runs on.
///
/// The first six methods are doomgeneric's `DG_*` callbacks; the sound and
/// music methods replace the SDL mixer behind `i_sound`. Every method has a
/// do-nothing default, so a platform implements only what it has: the
/// default platform is silent, shows nothing and reads no input.
pub trait Platform {
    /// `DG_Init`: called once by [`doomgeneric_Create`], after the screen
    /// buffer is allocated and before the engine starts.
    fn init(&self) {}

    /// `DG_DrawFrame`: present `frame`, [`DOOMGENERIC_PIXELS`] pixels of
    /// [`DOOMGENERIC_RESX`] per row, as the renderer wrote them into
    /// [`DG_ScreenBuffer`].
    fn draw_frame(&self, _frame: &[u32]) {}

    /// `DG_SleepMs`: sleep for `ms` milliseconds.
    fn sleep_ms(&self, _ms: u32) {}

    /// `DG_GetTicksMs`: milliseconds since startup.
    fn ticks_ms(&self) -> u32 {
        0
    }

    /// `DG_GetKey`: the next key event, as `(pressed, doom_key)`.
    fn key(&self) -> Option<(bool, u8)> {
        None
    }

    /// `DG_SetWindowTitle`: show the game's title.
    fn set_window_title(&self, _title: &str) {}

    /// Open the audio output; `true` when audio is available.
    fn sound_init(&self) -> bool {
        false
    }

    /// Close the audio output, stopping every sound and song.
    fn sound_shutdown(&self) {}

    /// Play the DMX sound lump `lump` on SFX `channel` (`0..8`) at volume
    /// `vol` (`0..=127`) and separation `sep` (`0..=254`, 128 is centred);
    /// `true` on success.
    fn sound_start(&self, _lump: &[u8], _channel: usize, _vol: i32, _sep: i32) -> bool {
        false
    }

    /// Stop SFX `channel`.
    fn sound_stop(&self, _channel: usize) {}

    /// Change the volume and separation of the sound playing on `channel`.
    fn sound_update(&self, _channel: usize, _vol: i32, _sep: i32) {}

    /// `true` while SFX `channel` is still playing.
    fn sound_is_playing(&self, _channel: usize) -> bool {
        false
    }

    /// Prepare music playback (load a synthesiser's instruments).
    fn music_init(&self) {}

    /// Play the Standard MIDI File `midi`, looping if `looping`.
    fn music_play(&self, _midi: &[u8], _looping: bool) {}

    /// Stop the current song.
    fn music_stop(&self) {}

    /// Pause the current song.
    fn music_pause(&self) {}

    /// Resume a paused song.
    fn music_resume(&self) {}

    /// Set the music volume (`0..=127`).
    fn music_set_volume(&self, _volume: i32) {}

    /// `true` while a song is playing.
    fn music_is_playing(&self) -> bool {
        false
    }
}

/// The default platform: every [`Platform`] method keeps its do-nothing
/// default. Used until [`doomgeneric_Create`] installs one, which is what
/// unit tests that never start the engine run on.
struct Silent;

impl Platform for Silent {}

/// The platform [`doomgeneric_Create`] installed.
static mut PLATFORM: Option<&'static dyn Platform> = None;

/// The platform the engine runs on, or the silent default before
/// [`doomgeneric_Create`].
pub fn platform() -> &'static dyn Platform {
    // SAFETY: written once by `doomgeneric_Create` on the engine's thread,
    // before anything reads it; this copies the reference out.
    unsafe { PLATFORM }.unwrap_or(&Silent)
}

/// Initialise the Doom engine on `platform` and enter the main game loop.
///
/// Corresponds to `doomgeneric_Create` in `doomgeneric.c`.  This is the
/// primary entry point called by the host application after setting up any
/// platform-specific context.
///
/// Steps performed:
/// 1. Install `platform` for every later [`platform()`] call.
/// 2. Store `args` (program name first) in `myargv` so `M_Arg*` functions
///    work.
/// 3. Call `M_FindResponseFile` to expand any `@file` arguments.
/// 4. Allocate the screen buffer ([`DOOMGENERIC_PIXELS`] × 4 bytes) via
///    `Vec` and intentionally leak it so the lifetime matches the C `malloc`
///    version.
/// 5. Call [`Platform::init`] so the platform backend can create its
///    window/context.
/// 6. Call `D_DoomMain`, which never returns under normal operation.
///
/// # Safety
/// This function must be called exactly once; calling it a second time
/// would leak the first screen buffer and overwrite `myargv`.
pub unsafe fn doomgeneric_Create(platform: &'static dyn Platform, args: Vec<String>) {
    PLATFORM = Some(platform);

    myargv = args;

    M_FindResponseFile();

    let total_pixels = DOOMGENERIC_RESX * DOOMGENERIC_RESY;
    let mut buffer = vec![0u32; total_pixels];
    DG_ScreenBuffer = buffer.as_mut_ptr();
    std::mem::forget(buffer);

    platform.init();
    D_DoomMain();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dg_screenbuffer_is_null_init() {
        unsafe {
            assert!(DG_ScreenBuffer.is_null());
        }
    }
}
