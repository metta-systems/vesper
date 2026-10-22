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
//! `gamedescription`, and `modifiedgame`.

#![allow(non_upper_case_globals, non_snake_case)]

use crate::{engine::doom, types::Boolean};

/// Identifies which IWAD variant is loaded (shareware, registered, retail, etc.).
///
/// Corresponds to `GameMode_t gamemode` in `doomstat.c`.  The value is one of
/// the `d_mode` integer constants (e.g., `doom::mode::indetermined`,
/// `doom::mode::shareware`, `doom::mode::registered`, `doom::mode::retail`).
/// Initialised to `indetermined`; set by `D_IdentifyVersion` during startup.
/// Referenced by virtually every module that needs to branch on game edition.
pub static mut gamemode: i32 = doom::mode::indetermined;

/// Identifies the game mission/product line (Doom, Doom II, TNT, Plutonia, etc.).
///
/// Corresponds to `GameMission_t gamemission` in `doomstat.c`.  The value is
/// one of the `d_mode` integer constants (e.g., `doom::mode::doom`,
/// `doom::mode::doom2`).  Initialised to `doom`; updated alongside `gamemode`
/// during IWAD detection.
pub static mut gamemission: i32 = doom::mode::doom;

/// Identifies the target engine version for compatibility purposes.
///
/// Corresponds to `GameVersion_t gameversion` in `doomstat.c`.  Controls
/// which demo format is used, whether certain Vanilla quirks are active, etc.
/// Initialised to `exe_final2` and may be overridden by `-gameversion` on the
/// command line.
pub static mut gameversion: i32 = doom::mode::exe_final2;

/// Human-readable description of the loaded IWAD (e.g. `"DOOM Registered"`).
///
/// Corresponds to `char *gamedescription` in `doomstat.c`.  Set by
/// `D_SetGameDescription` during startup and used by the window title and
/// help text.  Empty until initialised.
pub static mut gamedescription: String = String::new();

/// `true` if any PWAD files have been loaded on top of the base IWAD.
///
/// Corresponds to `boolean modifiedgame` in `doomstat.c`.  Set in
/// `W_CheckNumLumps` whenever a non-IWAD WAD is added.  When true, savegame
/// compatibility warnings may be shown and certain strict-mode features are
/// disabled.
pub static mut modifiedgame: Boolean = Boolean::FALSE;

#[cfg(test)]
mod tests {
    use {super::*, std::sync::Mutex};

    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn defaults_match_c() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert_eq!({ gamemode }, doom::mode::indetermined);
            assert_eq!({ gamemission }, doom::mode::doom);
            assert_eq!({ gameversion }, doom::mode::exe_final2);
            assert!((&*std::ptr::addr_of!(gamedescription)).is_empty());
            assert_eq!({ modifiedgame }, Boolean::FALSE);
        }
    }
}
