//! Rust port of vendor/doomgeneric/i_cdmus.c.
//!
//! Stubs for the Hexen-style audio-CD interface. The original chocolate-doom
//! implementation wraps SDL_cdrom to play tracks from a physical CD-ROM,
//! but doomgeneric compiles all of that out (the SDL paths sit behind
//! `#ifdef ORIGCODE`, which is never defined). The Rust port mirrors that:
//! every function is a no-op that returns success and resets the `cd_Error`
//! flag. The symbols remain exported because Hexen and shared Heretic code
//! still reference them.

#![allow(non_upper_case_globals, non_snake_case)]

/// Last CD-ROM error code, mirroring the C `cd_Error` global. Set to a
/// non-zero value by the original SDL backend on failure; in the
/// doomgeneric stub it is only ever cleared to 0.
pub static mut cd_Error: i32 = 0;

/// Initialise the CD-ROM music subsystem.
///
/// Stub that simply clears `cd_Error` and returns 0 (success). The original
/// implementation opened an `SDL_CD` handle and reported drive presence.
pub fn I_CDMusInit() -> i32 {
    unsafe {
        cd_Error = 0;
    }
    0
}

/// Print deferred CD startup status messages.
///
/// No-op stub. The original printed the drive name and any startup error
/// captured in `I_CDMusInit`.
pub fn I_CDMusPrintStartup() {}

/// Begin playback of the given audio track (1-indexed). Stub returns 0.
pub fn I_CDMusPlay(_track: i32) -> i32 {
    0
}

/// Stop the currently playing track. Stub returns 0.
pub fn I_CDMusStop() -> i32 {
    0
}

/// Resume paused playback. Stub returns 0.
pub fn I_CDMusResume() -> i32 {
    0
}

/// Set the CD-ROM playback volume. Stub clears `cd_Error` and returns 0,
/// matching the C version which was never implemented even outside
/// `ORIGCODE` ("Not supported yet").
pub fn I_CDMusSetVolume(_volume: i32) -> i32 {
    unsafe {
        cd_Error = 0;
    }
    0
}

/// Return the first audio track number (1-indexed) on the inserted CD.
/// Stub returns 0; the original scanned the SDL track table for the first
/// `SDL_AUDIO_TRACK` entry.
pub fn I_CDMusFirstTrack() -> i32 {
    0
}

/// Return the index of the last track on the inserted CD. Stub returns 0.
pub fn I_CDMusLastTrack() -> i32 {
    0
}

/// Return the length of the given track, rounded up to the next second.
/// Stub returns 0; the original computed `(track->length + CD_FPS - 1) /
/// CD_FPS` from the SDL track table.
pub fn I_CDMusTrackLength(_track_num: i32) -> i32 {
    0
}
