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

#![allow(non_snake_case)]

use crate::engine::{
    doom::player::MAXPLAYERS,
    global::Global,
    intermission::{wbplayerstruct_t, wbstartstruct_t},
    misc::argv::M_ParmExists,
};

/// Maximum number of intermission snapshots that can be captured during
/// a session. Mirrors the C `MAX_CAPTURES` define.
const MAX_CAPTURES: usize = 32;

/// Captured end-of-level snapshots (C `statdump.c` file statics).
pub struct StatdumpState {
    /// Ring of captured end-of-level snapshots. The first
    /// `num_captured_stats` entries are valid; the rest are
    /// zero-initialised via the `DEFAULT_WB` constant below. Mirrors the
    /// C `captured_stats` array.
    captured_stats: [wbstartstruct_t; MAX_CAPTURES],
    /// Number of snapshots currently stored in `captured_stats`.
    /// Increments on every [`StatCopy`] call until [`MAX_CAPTURES`] is
    /// reached. Mirrors the C `num_captured_stats` static.
    num_captured_stats: i32,
}

impl StatdumpState {
    /// The state at program start: nothing captured.
    pub const fn new() -> Self {
        const DEFAULT: wbplayerstruct_t = wbplayerstruct_t {
            in_: 0,
            skills: 0,
            sitems: 0,
            ssecret: 0,
            stime: 0,
            frags: [0; 4],
            score: 0,
        };
        const DEFAULT_WB: wbstartstruct_t = wbstartstruct_t {
            epsd: 0,
            didsecret: 0,
            last: 0,
            next: 0,
            maxkills: 0,
            maxitems: 0,
            maxsecret: 0,
            maxfrags: 0,
            partime: 0,
            pnum: 0,
            plyr: [DEFAULT; MAXPLAYERS],
        };
        Self {
            captured_stats: [DEFAULT_WB; MAX_CAPTURES],
            num_captured_stats: 0,
        }
    }
}

impl Default for StatdumpState {
    fn default() -> Self {
        Self::new()
    }
}

/// The statistics capture buffer.
pub static STATDUMP: Global<StatdumpState> = Global::new(StatdumpState::new());

/// Capture a snapshot of `stats` into `captured_stats` if statistics
/// dumping was requested on the command line.
///
/// Behaviour matches the C original: when `-statdump` is present in
/// `argv` and the buffer is not yet full, the entire `wbstartstruct_t`
/// is copied into the next free slot and `num_captured_stats`
/// is bumped. When the buffer is full or the flag is absent, the call
/// is silently dropped.
///
/// Called from `G_DoCompleted` at the end of every level, just before
/// `WI_Start`.
pub fn StatCopy(stats: &wbstartstruct_t) {
    if !M_ParmExists("-statdump") {
        return;
    }
    let mut statdump = STATDUMP.borrow_mut();
    if statdump.num_captured_stats < MAX_CAPTURES as i32 {
        let slot = statdump.num_captured_stats as usize;
        statdump.captured_stats[slot] = *stats;
        statdump.num_captured_stats += 1;
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
pub fn StatDump() {
    // All implementation is wrapped in #if ORIGCODE which is not defined
}

#[cfg(test)]
mod tests {
    use super::*;

    /// StatDump must not panic (it is currently a no-op stub).
    #[test]
    fn stat_dump_does_not_panic() {
        StatDump();
    }
}
