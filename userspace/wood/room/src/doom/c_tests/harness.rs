//! Test harness: type-size assertions, serial-test lock, and reset utilities.

#![allow(non_snake_case)]

use std::{
    ffi::{c_int, c_long, c_short, c_uint, c_ulong, c_ushort, c_void},
    sync::Mutex,
};

// ---------------------------------------------------------------------------
// Compile-time type-size assertions.
// These catch the exact data-type-size bugs we've hit before.
// ---------------------------------------------------------------------------

const _: () = assert!(std::mem::size_of::<c_int>() == 4);
const _: () = assert!(std::mem::size_of::<c_uint>() == 4);
const _: () = assert!(std::mem::size_of::<c_short>() == 2);
const _: () = assert!(std::mem::size_of::<c_ushort>() == 2);
const _: () = assert!(std::mem::size_of::<c_long>() == 8); // Linux x86_64
const _: () = assert!(std::mem::size_of::<c_ulong>() == 8);
const _: () = assert!(std::mem::size_of::<*mut c_void>() == 8); // pointer size

/// Serialise tests that mutate C global state.
pub static C_GLOBAL_LOCK: Mutex<()> = Mutex::new(());

/// Reset the RNG cursors to a known state.
pub fn reset_rng() {
    crate::doom::m_random::M_ClearRandom();
}

// ---------------------------------------------------------------------------
// Silent audio hooks: the unit tests link no platform (src/platform/), and
// the sound code they reach calls these.
// ---------------------------------------------------------------------------

#[no_mangle]
extern "C" fn DG_SoundInit() -> c_int {
    0
}

#[no_mangle]
extern "C" fn DG_SoundShutdown() {}

#[no_mangle]
extern "C" fn DG_SoundStart(
    _data: *const u8,
    _len: usize,
    _channel: c_int,
    _vol: c_int,
    _sep: c_int,
) -> c_int {
    0
}

#[no_mangle]
extern "C" fn DG_SoundStop(_channel: c_int) {}

#[no_mangle]
extern "C" fn DG_SoundUpdate(_channel: c_int, _vol: c_int, _sep: c_int) {}

#[no_mangle]
extern "C" fn DG_SoundIsPlaying(_channel: c_int) -> c_int {
    0
}

#[no_mangle]
extern "C" fn DG_MusicInit() {}

#[no_mangle]
extern "C" fn DG_MusicPlay(_midi: *const u8, _len: usize, _looping: c_int) {}

#[no_mangle]
extern "C" fn DG_MusicStop() {}

#[no_mangle]
extern "C" fn DG_MusicPause() {}

#[no_mangle]
extern "C" fn DG_MusicResume() {}

#[no_mangle]
extern "C" fn DG_MusicSetVolume(_volume: c_int) {}

#[no_mangle]
extern "C" fn DG_MusicIsPlaying() -> c_int {
    0
}
