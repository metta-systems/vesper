//! Vesper platform: placeholder `DG_*` functions.
//!
//! Nothing here reaches a device yet: there is no frame output, input, time
//! or sound, and [`run`] only starts the engine and ticks it. Each function
//! will be backed by a Vesper component (the compositor, `input_v1`, the
//! timer, audio) as those land.

use core::ffi::{c_char, c_int};

/// Start the engine and tick it forever.
pub fn run() {
    let mut name = *b"room\0";
    let mut argv = [name.as_mut_ptr().cast::<c_char>(), core::ptr::null_mut()];
    // SAFETY: `argv` is a null-terminated array of one NUL-terminated
    // string, and both outlive the engine, which never returns from here.
    unsafe { room::doom::doomgeneric::doomgeneric_Create(1, argv.as_mut_ptr()) };
    loop {
        room::doom::d_main::doomgeneric_Tick();
    }
}

// ---------------------------------------------------------------------------
// doomgeneric video, input and timer callbacks
// ---------------------------------------------------------------------------

/// Nothing to set up yet.
#[no_mangle]
pub extern "C" fn DG_Init() {}

/// Frames go nowhere yet.
#[no_mangle]
pub extern "C" fn DG_DrawFrame() {}

/// Returns at once: there is no timer to sleep on yet.
#[no_mangle]
pub extern "C" fn DG_SleepMs(_ms: u32) {}

/// Time stands still at 0 until there is a clock.
#[no_mangle]
pub extern "C" fn DG_GetTicksMs() -> u32 {
    0
}

/// No key events yet.
#[no_mangle]
pub extern "C" fn DG_GetKey(_pressed: *mut c_int, _doom_key: *mut u8) -> c_int {
    0
}

/// No window to title.
#[no_mangle]
pub extern "C" fn DG_SetWindowTitle(_title: *const c_char) {}

// ---------------------------------------------------------------------------
// Audio hooks
// ---------------------------------------------------------------------------

/// No audio output: the engine runs silent.
#[no_mangle]
pub extern "C" fn DG_SoundInit() -> c_int {
    0
}

/// Nothing to close.
#[no_mangle]
pub extern "C" fn DG_SoundShutdown() {}

/// Sounds are dropped.
#[no_mangle]
pub extern "C" fn DG_SoundStart(
    _data: *const u8,
    _len: usize,
    _channel: c_int,
    _vol: c_int,
    _sep: c_int,
) -> c_int {
    0
}

/// Nothing plays, so nothing stops.
#[no_mangle]
pub extern "C" fn DG_SoundStop(_channel: c_int) {}

/// Nothing plays, so nothing changes.
#[no_mangle]
pub extern "C" fn DG_SoundUpdate(_channel: c_int, _vol: c_int, _sep: c_int) {}

/// No channel ever plays.
#[no_mangle]
pub extern "C" fn DG_SoundIsPlaying(_channel: c_int) -> c_int {
    0
}

/// No synthesiser to prepare.
#[no_mangle]
pub extern "C" fn DG_MusicInit() {}

/// Songs are dropped.
#[no_mangle]
pub extern "C" fn DG_MusicPlay(_midi: *const u8, _len: usize, _looping: c_int) {}

/// Nothing plays, so nothing stops.
#[no_mangle]
pub extern "C" fn DG_MusicStop() {}

/// Nothing plays, so nothing pauses.
#[no_mangle]
pub extern "C" fn DG_MusicPause() {}

/// Nothing is paused.
#[no_mangle]
pub extern "C" fn DG_MusicResume() {}

/// No volume to set.
#[no_mangle]
pub extern "C" fn DG_MusicSetVolume(_volume: c_int) {}

/// No song ever plays.
#[no_mangle]
pub extern "C" fn DG_MusicIsPlaying() -> c_int {
    0
}
