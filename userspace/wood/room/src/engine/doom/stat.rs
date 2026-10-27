//! Rust port of vendor/doomgeneric/doomstat.c.
//!
//! Global game state variables shared by every module in the engine.
//! The header (`doomstat.h`) serves as the single source of truth for all
//! game-wide state: IWAD identity, active game mode, savegame directory,
//! player bookkeeping, demo flags, and more.  In practice only a small
//! subset of those variables lives here; the rest are defined in the C
//! modules that own them and declared `extern` in the header.
//!
//! This file covers the five variables whose definitions appeared in the
//! original `doomstat.c`: `gamemode`, `gamemission`, `gameversion`,
//! `gamedescription`, and `modifiedgame`. They live in [`StatState`], held
//! in the [`STAT`] global; readers use the [`gamemode`], [`gamemission`],
//! [`gameversion`] and [`modifiedgame`] accessors.

#![allow(non_upper_case_globals, non_snake_case)]

use alloc::string::String;

use crate::{
    engine::{doom, global::Global},
    types::Boolean,
};

/// Game identity (C `doomstat.c` globals).
pub struct StatState {
    /// Identifies which IWAD variant is loaded (shareware, registered,
    /// retail, etc.).
    ///
    /// Corresponds to `GameMode_t gamemode` in `doomstat.c`.  The value is
    /// one of the `d_mode` integer constants (e.g.,
    /// `doom::mode::indetermined`, `doom::mode::shareware`,
    /// `doom::mode::registered`, `doom::mode::retail`). Initialised to
    /// `indetermined`; set by `D_IdentifyVersion` during startup.
    /// Referenced by virtually every module that needs to branch on game
    /// edition.
    pub gamemode: i32,
    /// Identifies the game mission/product line (Doom, Doom II, TNT,
    /// Plutonia, etc.).
    ///
    /// Corresponds to `GameMission_t gamemission` in `doomstat.c`.  The
    /// value is one of the `d_mode` integer constants (e.g.,
    /// `doom::mode::doom`, `doom::mode::doom2`).  Initialised to `doom`;
    /// updated alongside `gamemode` during IWAD detection.
    pub gamemission: i32,
    /// Identifies the target engine version for compatibility purposes.
    ///
    /// Corresponds to `GameVersion_t gameversion` in `doomstat.c`.  Controls
    /// which demo format is used, whether certain Vanilla quirks are active,
    /// etc. Initialised to `exe_final2` and may be overridden by
    /// `-gameversion` on the command line.
    pub gameversion: i32,
    /// Human-readable description of the loaded IWAD (e.g.
    /// `"DOOM Registered"`).
    ///
    /// Corresponds to `char *gamedescription` in `doomstat.c`.  Set by
    /// `D_SetGameDescription` during startup and used by the window title
    /// and help text.  Empty until initialised.
    pub gamedescription: String,
    /// `true` if any PWAD files have been loaded on top of the base IWAD.
    ///
    /// Corresponds to `boolean modifiedgame` in `doomstat.c`.  Set in
    /// `W_CheckNumLumps` whenever a non-IWAD WAD is added.  When true,
    /// savegame compatibility warnings may be shown and certain strict-mode
    /// features are disabled.
    pub modifiedgame: Boolean,
}

impl StatState {
    /// The C initial values: mode not yet identified.
    pub const fn new() -> Self {
        Self {
            gamemode: doom::mode::indetermined,
            gamemission: doom::mode::doom,
            gameversion: doom::mode::exe_final2,
            gamedescription: String::new(),
            modifiedgame: Boolean::FALSE,
        }
    }
}

impl Default for StatState {
    fn default() -> Self {
        Self::new()
    }
}

/// The game identity.
pub static STAT: Global<StatState> = Global::new(StatState::new());

/// The loaded IWAD variant (C `gamemode`).
pub fn gamemode() -> i32 {
    STAT.borrow().gamemode
}

/// The game mission (C `gamemission`).
pub fn gamemission() -> i32 {
    STAT.borrow().gamemission
}

/// The emulated executable version (C `gameversion`).
pub fn gameversion() -> i32 {
    STAT.borrow().gameversion
}

/// Whether PWADs modify the game (C `modifiedgame`).
pub fn modifiedgame() -> Boolean {
    STAT.borrow().modifiedgame
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_c() {
        let stat = StatState::new();
        assert_eq!(stat.gamemode, doom::mode::indetermined);
        assert_eq!(stat.gamemission, doom::mode::doom);
        assert_eq!(stat.gameversion, doom::mode::exe_final2);
        assert!(stat.gamedescription.is_empty());
        assert_eq!(stat.modifiedgame, Boolean::FALSE);
    }
}
