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
//! pointer ([`screen_buffer`], in [`PlatformState`]) and implements [`doomgeneric_Create`], which
//! records the platform, allocates that buffer, stores the command-line
//! arguments, and launches the main engine entry point.
//!
//! Rust differences from C:
//! - The C code allocates the screen buffer with `malloc` and never frees it
//!   (intentional leak in a long-running process).  This port allocates via
//!   `Vec<u32>` and uses `mem::forget` to produce the same intentional
//!   leak, while using Rust allocation.
//! - The platform callbacks are the methods of [`Platform`], installed by
//!   [`doomgeneric_Create`], instead of functions the platform links in.
//! - [`Platform`] also carries what the C engine took from the C library
//!   and the OS: file access, exit and the error dialog, so the engine
//!   needs only `core` and `alloc`.

#![allow(non_upper_case_globals, non_snake_case)]

use {
    alloc::{string::String, vec::Vec},
    core::ptr,
};

use super::{
    doom::main::D_DoomMain,
    global::Global,
    misc::argv::{M_FindResponseFile, set_arguments},
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
/// Convenience constant used when allocating or iterating over [`screen_buffer`].
pub const DOOMGENERIC_PIXELS: usize = DOOMGENERIC_RESX * DOOMGENERIC_RESY;

/// What the engine needs from the platform it runs on.
///
/// The first six methods are doomgeneric's `DG_*` callbacks; the sound and
/// music methods replace the SDL mixer behind `i_sound`; the file methods
/// replace stdio (handles for the WAD, whole files for everything else);
/// [`Platform::error_box`] and [`Platform::quit`] replace the error dialog
/// and `exit`. Every method has a default, so a platform implements only
/// what it has: the default platform is silent, shows nothing, reads no
/// input, has no files and panics on quit.
pub trait Platform {
    /// `DG_Init`: called once by [`doomgeneric_Create`], after the screen
    /// buffer is allocated and before the engine starts.
    fn init(&self) {}

    /// `DG_DrawFrame`: present `frame`, [`DOOMGENERIC_PIXELS`] pixels of
    /// [`DOOMGENERIC_RESX`] per row, as the renderer wrote them into
    /// [`screen_buffer`].
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

    /// Open the file at `path` (a WAD) for [`Platform::read_at`]; `None` if
    /// it does not exist or cannot be read.
    fn open(&self, _path: &str) -> Option<FileHandle> {
        None
    }

    /// The size in bytes of the open file `file`.
    fn len(&self, _file: FileHandle) -> u64 {
        0
    }

    /// Read up to `buffer.len()` bytes at `offset` of the open file `file`
    /// into `buffer`, as `fread` does; the number of bytes read, short at
    /// the end of the file or on an error.
    fn read_at(&self, _file: FileHandle, _offset: u64, _buffer: &mut [u8]) -> usize {
        0
    }

    /// Close the open file `file`; its handle is not used again.
    fn close(&self, _file: FileHandle) {}

    /// The contents of the file at `path`, or `None` if it cannot be read.
    fn read_file(&self, _path: &str) -> Option<Vec<u8>> {
        None
    }

    /// Create or truncate the file at `path` and write `data` to it; `true`
    /// if all of it was written.
    fn write_file(&self, _path: &str, _data: &[u8]) -> bool {
        false
    }

    /// Rename the file `from` to `to`, replacing `to`; `true` on success.
    fn rename(&self, _from: &str, _to: &str) -> bool {
        false
    }

    /// Delete the file at `path`; `true` on success.
    fn remove(&self, _path: &str) -> bool {
        false
    }

    /// `true` if `path` names a readable file or a directory.
    fn exists(&self, _path: &str) -> bool {
        false
    }

    /// Create the directory `path` (not its parents); `true` on success.
    fn create_dir(&self, _path: &str) -> bool {
        false
    }

    /// Show `message`, the reason the engine is about to quit with an
    /// error, where a user sees it (a dialog box), beyond the log.
    fn error_box(&self, _message: &str) {}

    /// End the program with exit status `code`.
    ///
    /// The default, for a platform with nowhere to exit to, panics with
    /// the status, so a test or the platform's panic handler reports it.
    fn quit(&self, code: i32) -> ! {
        panic!("the engine quit with exit status {code}")
    }
}

/// An open file of a [`Platform`], from [`Platform::open`]. The platform
/// gives the number its meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileHandle(pub usize);

/// The default platform: every [`Platform`] method keeps its do-nothing
/// default. Used until [`doomgeneric_Create`] installs one, which is what
/// unit tests that never start the engine run on.
struct Silent;

impl Platform for Silent {}

/// Platform state (C `doomgeneric.c` globals).
#[derive(Clone, Copy)]
pub struct PlatformState {
    /// Shared frame-buffer pointer written by the renderer and read by the
    /// platform backend.
    ///
    /// Corresponds to `pixel_t *DG_ScreenBuffer` in `doomgeneric.c`.  After
    /// [`doomgeneric_Create`] runs, this points to a heap-allocated array of
    /// [`DOOMGENERIC_PIXELS`] `u32` values (RGBA8 or BGRA8 depending on the
    /// build).  The renderer (`R_DrawColumn`, `R_DrawSpan`, etc.) writes
    /// pixels into this buffer each frame; the platform backend reads from it
    /// inside `DG_DrawFrame` to upload the frame to the display.
    ///
    /// Initialised to `null`; valid only after [`doomgeneric_Create`] has
    /// been called.
    screen_buffer: *mut u32,
    /// The platform [`doomgeneric_Create`] installed.
    platform: Option<&'static dyn Platform>,
}

impl PlatformState {
    /// No platform and no screen buffer yet.
    pub const fn new() -> Self {
        Self {
            screen_buffer: ptr::null_mut(),
            platform: None,
        }
    }
}

impl Default for PlatformState {
    fn default() -> Self {
        Self::new()
    }
}

/// The platform state.
pub static PLATFORM: Global<PlatformState> = Global::new(PlatformState::new());

/// The platform the engine runs on, or the silent default before
/// [`doomgeneric_Create`].
pub fn platform() -> &'static dyn Platform {
    PLATFORM.get().platform.unwrap_or(&Silent)
}

/// `DG_ScreenBuffer`: the [`DOOMGENERIC_PIXELS`]-pixel frame buffer, null
/// before [`doomgeneric_Create`].
pub fn screen_buffer() -> *mut u32 {
    PLATFORM.get().screen_buffer
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
    PLATFORM.borrow_mut().platform = Some(platform);

    set_arguments(args);

    M_FindResponseFile();

    let total_pixels = DOOMGENERIC_RESX * DOOMGENERIC_RESY;
    let mut buffer = alloc::vec![0u32; total_pixels];
    PLATFORM.borrow_mut().screen_buffer = buffer.as_mut_ptr();
    core::mem::forget(buffer);

    platform.init();
    D_DoomMain();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dg_screenbuffer_is_null_init() {
        let state = PlatformState::new();
        assert!(state.screen_buffer.is_null());
        assert!(state.platform.is_none());
    }
}
