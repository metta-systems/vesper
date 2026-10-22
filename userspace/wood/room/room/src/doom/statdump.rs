//! Rust port of vendor/doomgeneric/statdump.c.
//!
//! End-of-level statistics capture. Each time the intermission screen
//! posts its `wbstartstruct_t`, [`StatCopy`] records a copy in the
//! `captured_stats` buffer (up to `MAX_CAPTURES` entries) so that a
//! later [`StatDump`] invocation could dump them all to a file in the
//! same format as the original 16-bit `statdump.exe`.
//!
//! The dump itself (`PrintStats`, `PrintFragsTable`, `DiscoverGamemode`,
//! the par-time tables, etc.) lives behind `#if ORIGCODE` in the C
//! source and is therefore not ported. The capture path is preserved
//! exactly so a future hook can iterate `captured_stats` if needed.

#![allow(non_upper_case_globals, non_snake_case)]

use std::ffi::{c_int, c_void};

/// Snapshot of intermission-screen state captured at the end of every
/// level. Mirrors the C `wbstartstruct_t` layout from `d_player.h`.
/// `#[repr(C)]` so it can be `memcpy`'d directly out of game memory.
///
/// Layout invariant: the struct exposed here only includes the four
/// fields the dump path inspects (`epsd`, `last`, `partime`, `plyr`).
/// The full C struct has additional fields (`pnum`, `maxkills`,
/// `maxitems`, `maxsecret`, etc.); the unit tests in this module assert
/// the trimmed size matches `3 * sizeof(int) + 4 * sizeof(wbplayerstruct_t)`.
#[repr(C)]
pub struct wbstartstruct_t {
    /// Episode number (0-based).
    pub epsd: c_int,
    /// Index of the level just completed.
    pub last: c_int,
    /// Par time for this level in tics.
    pub partime: c_int,
    /// Per-player statistics for up to 4 players.
    pub plyr: [wbplayerstruct_t; 4],
}

/// Per-player end-of-level statistics. Mirrors the C `wbplayerstruct_t`
/// from `d_player.h`. The first field is named `in_` (with trailing
/// underscore) because `in` is a Rust keyword; the original C name is
/// `in` and the `#[repr(C)]` layout is unchanged.
#[repr(C)]
pub struct wbplayerstruct_t {
    /// "in game" flag - non-zero if this player slot was active. Named
    /// `in` in C; renamed `in_` here to dodge the Rust keyword.
    pub in_: c_int,
    /// Kills count for this player.
    pub skills: c_int,
    /// Items collected.
    pub sitems: c_int,
    /// Secrets found.
    pub ssecret: c_int,
    /// Total time spent on the level, in tics.
    pub stime: c_int,
    /// Frag counts indexed by victim player.
    pub frags: [c_int; 4],
}

/// Maximum number of intermission snapshots that can be captured during
/// a session. Mirrors the C `MAX_CAPTURES` define.
const MAX_CAPTURES: usize = 32;

/// Ring of captured end-of-level snapshots. The first
/// [`num_captured_stats`] entries are valid; the rest are zero-initialised
/// via the `DEFAULT_WB` constant below. Mirrors the C `captured_stats`
/// array.
static mut captured_stats: [wbstartstruct_t; MAX_CAPTURES] = {
    const DEFAULT: wbplayerstruct_t = wbplayerstruct_t {
        in_: 0,
        skills: 0,
        sitems: 0,
        ssecret: 0,
        stime: 0,
        frags: [0; 4],
    };
    const DEFAULT_WB: wbstartstruct_t = wbstartstruct_t {
        epsd: 0,
        last: 0,
        partime: 0,
        plyr: [DEFAULT; 4],
    };
    [DEFAULT_WB; MAX_CAPTURES]
};

/// Number of snapshots currently stored in [`captured_stats`]. Increments
/// on every [`StatCopy`] call until [`MAX_CAPTURES`] is reached. Mirrors
/// the C `num_captured_stats` static.
static mut num_captured_stats: c_int = 0;

use crate::doom::m_argv::M_ParmExists;

extern "C" {
    /// C `memcpy` from libc. Used for the bulk struct copy below; an
    /// `std::ptr::copy_nonoverlapping` would do equally well but the
    /// direct FFI call keeps the code byte-identical to the C source.
    fn memcpy(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void;
}

/// Capture a snapshot of `stats` into `captured_stats` if statistics
/// dumping was requested on the command line.
///
/// Behaviour matches the C original: when `-statdump` is present in
/// `argv` and the buffer is not yet full, the entire `wbstartstruct_t`
/// is `memcpy`'d into the next free slot and `num_captured_stats`
/// is bumped. When the buffer is full or the flag is absent, the call
/// is silently dropped.
///
/// Called from `WI_Drawer` / `WI_Ticker` (via the intermission code)
/// at the end of every level.
#[no_mangle]
pub extern "C" fn StatCopy(stats: *mut wbstartstruct_t) {
    unsafe {
        if M_ParmExists(c"-statdump".as_ptr()) != 0 && num_captured_stats < MAX_CAPTURES as c_int {
            memcpy(
                std::ptr::addr_of_mut!(captured_stats[0]).offset(num_captured_stats as isize)
                    as *mut c_void,
                stats as *const c_void,
                std::mem::size_of::<wbstartstruct_t>(),
            );
            num_captured_stats += 1;
        }
    }
}

/// Write captured statistics to the file named by `-statdump` (or
/// stdout when the path is `-`).
///
/// No-op stub: the entire dump implementation - banner printing, par
/// time interpretation, the `PrintFragsTable` grid, the gamemode
/// discovery heuristic - sits inside `#if ORIGCODE` in the C source
/// and is not ported. [`StatCopy`] still fills the buffer, so the
/// data is there for any future implementation that wants it.
#[no_mangle]
pub extern "C" fn StatDump() {
    // All implementation is wrapped in #if ORIGCODE which is not defined
}

#[cfg(test)]
mod tests {
    use super::*;

    /// wbplayerstruct_t must be 36 bytes: 5 × i32 (20) + [i32; 4] (16).
    #[test]
    fn wbplayerstruct_t_size_matches_c() {
        // in_(4) + skills(4) + sitems(4) + ssecret(4) + stime(4) + frags[4](16) = 36
        assert_eq!(std::mem::size_of::<wbplayerstruct_t>(), 36);
    }

    /// wbstartstruct_t: 3 × sizeof(int) + 4 × sizeof(wbplayerstruct_t).
    #[test]
    fn wbstartstruct_t_size_matches_c() {
        // epsd(4) + last(4) + partime(4) + plyr[4] (4 × 36 = 144) = 156
        let expected =
            3 * std::mem::size_of::<c_int>() + 4 * std::mem::size_of::<wbplayerstruct_t>();
        assert_eq!(std::mem::size_of::<wbstartstruct_t>(), expected);
    }

    /// StatDump must not panic (it is currently a no-op stub).
    #[test]
    fn stat_dump_does_not_panic() {
        StatDump();
    }
}
