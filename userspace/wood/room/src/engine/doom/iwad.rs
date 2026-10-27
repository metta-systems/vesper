//! Rust port of vendor/doomgeneric/d_iwad.c.
//!
//! IWAD discovery and selection. An IWAD (Internal WAD) is the primary game
//! data file (e.g. `doom.wad`, `doom2.wad`). This module maintains a table of
//! known IWADs, searches a list of candidate directories to locate one on
//! disk, and identifies which game mission and mode a found IWAD belongs to.
//!
//! Differences from the C original:
//! - The IWAD search list is only the current directory (`"."`), omitting the
//!   `DOOMWADDIR`/`DOOMWADPATH` environment-variable paths and the
//!   platform-specific Windows registry / Unix standard paths that the full
//!   Chocolate Doom implementation supports. The `ORIGCODE` conditional in the
//!   C source guards those paths; this port permanently takes the simplified
//!   branch and hardcodes `FILES_DIR` as `"."`.
//! - `D_CheckCorrectIWAD` is intentionally a no-op (matches the C original's
//!   empty stub in doomgeneric).

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::{
    engine::{
        doom,
        misc::{
            M_FileExists,
            argv::{M_CheckParmWithArgs, argument},
        },
    },
    i_error,
};

/// Path component separator character (`/`).
///
/// Used when constructing full file paths from a directory and a filename.
const DIR_SEPARATOR: u8 = b'/';

/// Metadata for a single known IWAD file.
///
/// Corresponds to `iwad_t` / `typedef struct { ... } iwad_t` in `d_iwad.h`.
/// Instances live in the constant `IWADS` table.
pub struct iwad_t {
    /// Canonical filename of the IWAD (e.g. `"doom2.wad"`).
    pub name: &'static str,
    /// Game mission identifier; one of the `doom::mode::*` integer constants.
    pub mission: i32,
    /// Game mode identifier (shareware, retail, commercial, etc.).
    pub mode: i32,
    /// Human-readable name of the game (e.g. `"Doom II"`).
    pub description: &'static str,
}

/// Table of all known IWAD files, in priority order.
///
/// The ordering determines which IWAD is selected when multiple candidates are
/// present in the same directory. Commercial releases are listed before
/// shareware releases. Mirrors `iwads[]` in `d_iwad.c`.
const IWADS: [iwad_t; 14] = [
    iwad_t {
        name: "doom2.wad",
        mission: doom::mode::doom2,
        mode: doom::mode::commercial,
        description: "Doom II",
    },
    iwad_t {
        name: "plutonia.wad",
        mission: doom::mode::pack_plut,
        mode: doom::mode::commercial,
        description: "Final Doom: Plutonia Experiment",
    },
    iwad_t {
        name: "tnt.wad",
        mission: doom::mode::pack_tnt,
        mode: doom::mode::commercial,
        description: "Final Doom: TNT: Evilution",
    },
    iwad_t {
        name: "doom.wad",
        mission: doom::mode::doom,
        mode: doom::mode::retail,
        description: "Doom",
    },
    iwad_t {
        name: "doom1.wad",
        mission: doom::mode::doom,
        mode: doom::mode::shareware,
        description: "Doom Shareware",
    },
    iwad_t {
        name: "chex.wad",
        mission: doom::mode::pack_chex,
        mode: doom::mode::shareware,
        description: "Chex Quest",
    },
    iwad_t {
        name: "hacx.wad",
        mission: doom::mode::pack_hacx,
        mode: doom::mode::commercial,
        description: "Hacx",
    },
    iwad_t {
        name: "freedm.wad",
        mission: doom::mode::doom2,
        mode: doom::mode::commercial,
        description: "FreeDM",
    },
    iwad_t {
        name: "freedoom2.wad",
        mission: doom::mode::doom2,
        mode: doom::mode::commercial,
        description: "Freedoom: Phase 2",
    },
    iwad_t {
        name: "freedoom1.wad",
        mission: doom::mode::doom,
        mode: doom::mode::retail,
        description: "Freedoom: Phase 1",
    },
    iwad_t {
        name: "heretic.wad",
        mission: doom::mode::heretic,
        mode: doom::mode::retail,
        description: "Heretic",
    },
    iwad_t {
        name: "heretic1.wad",
        mission: doom::mode::heretic,
        mode: doom::mode::shareware,
        description: "Heretic Shareware",
    },
    iwad_t {
        name: "hexen.wad",
        mission: doom::mode::hexen,
        mode: doom::mode::commercial,
        description: "Hexen",
    },
    iwad_t {
        name: "strife1.wad",
        mission: doom::mode::strife,
        mode: doom::mode::commercial,
        description: "Strife",
    },
];

/// Directories to search for IWAD files.
///
/// Corresponds to `iwad_dirs[]` as filled by `BuildIWADDirList` in
/// `d_iwad.c`; this port only searches the current directory.
const IWAD_DIRS: [&str; 1] = ["."];

/// Returns `true` if `path` is a file path whose final component equals
/// `filename` (case-insensitive comparison).
///
/// For example, `DirIsFile("/games/doom.wad", "doom.wad")` returns `true`.
/// Corresponds to `DirIsFile` in `d_iwad.c`.
fn DirIsFile(path: &str, filename: &str) -> bool {
    let (path, filename) = (path.as_bytes(), filename.as_bytes());
    path.len() > filename.len()
        && path[path.len() - filename.len() - 1] == DIR_SEPARATOR
        && path[path.len() - filename.len()..].eq_ignore_ascii_case(filename)
}

/// Checks whether `dir` contains the IWAD named `iwadname`, returning the
/// full path on success.
///
/// Two cases are handled:
/// - If `dir` is itself a path to the IWAD file (i.e. `DirIsFile` returns
///   true and the file exists), `dir` is returned.
/// - Otherwise the path `dir/iwadname` is constructed and tested. The special
///   case `dir == "."` elides the directory prefix.
///
/// Corresponds to `CheckDirectoryHasIWAD` in `d_iwad.c`.
fn CheckDirectoryHasIWAD(dir: &str, iwadname: &str) -> Option<String> {
    if DirIsFile(dir, iwadname) && M_FileExists(dir) {
        return Some(dir.to_owned());
    }

    let filename = if dir == "." {
        iwadname.to_owned()
    } else {
        format!("{dir}/{iwadname}")
    };

    println!("Trying IWAD file:{filename}");

    M_FileExists(&filename).then_some(filename)
}

/// Searches a single directory `dir` for the first IWAD entry in `IWADS`
/// that passes the `mask` filter and exists on disk.
///
/// On success, writes the matched mission to `*mission` and returns the
/// path. Corresponds to `SearchDirectoryForIWAD` in `d_iwad.c`.
fn SearchDirectoryForIWAD(dir: &str, mask: i32, mission: &mut i32) -> Option<String> {
    IWADS
        .iter()
        .filter(|iwad| ((1 << iwad.mission) & mask) != 0)
        .find_map(|iwad| {
            let filename = CheckDirectoryHasIWAD(dir, iwad.name)?;
            *mission = iwad.mission;
            Some(filename)
        })
}

/// Identifies the game mission for an IWAD given its filename.
///
/// Strips any leading directory components from `name`, then performs a
/// case-insensitive comparison against each entry in `IWADS` that passes the
/// `mask` filter. Returns the matching `doom::mode::*` mission constant, or
/// `doom::mode::none` if the filename is not recognised.
///
/// Corresponds to `IdentifyIWADByName` in `d_iwad.c`.
fn IdentifyIWADByName(name: &str, mask: i32) -> i32 {
    let name = name
        .rsplit(char::from(DIR_SEPARATOR))
        .next()
        .unwrap_or(name);

    IWADS
        .iter()
        .filter(|iwad| ((1 << iwad.mission) & mask) != 0)
        .find(|iwad| name.eq_ignore_ascii_case(iwad.name))
        .map_or(doom::mode::none, |iwad| iwad.mission)
}

/// Searches IWAD search paths for a WAD file with the given `name`.
///
/// If `name` already refers to an existing file on disk, it is returned
/// as-is. Otherwise every candidate directory is tried, both as the path
/// itself (when `name` is the final component) and as the concatenated path
/// `dir/name`. Returns `None` if the file cannot be found anywhere.
pub fn D_FindWADByName(name: &str) -> Option<String> {
    if M_FileExists(name) {
        return Some(name.to_owned());
    }

    for dir in IWAD_DIRS {
        if DirIsFile(dir, name) && M_FileExists(dir) {
            return Some(dir.to_owned());
        }

        let path = format!("{dir}/{name}");
        if M_FileExists(&path) {
            return Some(path);
        }
    }

    None
}

/// Searches for a WAD by filename, falling back to the original `filename`
/// if the file cannot be found.
///
/// Unlike [`D_FindWADByName`], this function always returns a path: either
/// the found one, or `filename` unchanged on failure. Called from
/// `w_main.c`.
pub fn D_TryFindWADByName(filename: &str) -> String {
    D_FindWADByName(filename).unwrap_or_else(|| filename.to_owned())
}

/// Locates an IWAD file on disk and identifies its game mission.
///
/// If the `-iwad <file>` command-line argument is present, the specified file
/// is located via [`D_FindWADByName`] (aborting with `I_Error` if not found)
/// and its mission is identified by filename. Otherwise, the function scans
/// all IWAD search directories for any IWAD whose mission bit is set in `mask`.
///
/// Returns the path to the found IWAD, or `None` if none was found in the
/// auto-scan path. Writes the identified mission to `*mission`.
///
/// Called from `d_main.c`.
pub fn D_FindIWAD(mask: i32, mission: &mut i32) -> Option<String> {
    let iwadparm = M_CheckParmWithArgs("-iwad", 1);

    if iwadparm != 0 {
        let iwadfile = argument(iwadparm + 1);
        let Some(result) = D_FindWADByName(iwadfile) else {
            i_error!("IWAD file '{}' not found!", iwadfile);
        };

        *mission = IdentifyIWADByName(&result, mask);
        Some(result)
    } else {
        println!("-iwad not specified, trying a few iwad names");

        IWAD_DIRS
            .iter()
            .find_map(|dir| SearchDirectoryForIWAD(dir, mask, mission))
    }
}

/// Returns the canonical IWAD filename for the given `gamemission`.
///
/// Walks `IWADS` and returns the `name` field of the first entry whose
/// mission matches `gamemission`. Falls back to `"unknown.wad"` if no match
/// is found. This name is used as the savegame subdirectory so that
/// `doom.wad` and `doom1.wad` saves share the same location.
pub fn D_SaveGameIWADName(gamemission: i32) -> &'static str {
    IWADS
        .iter()
        .find(|iwad| iwad.mission == gamemission)
        .map_or("unknown.wad", |iwad| iwad.name)
}

/// Returns the canonical IWAD filename that best matches `mission` and `mode`.
///
/// Walks `IWADS` and returns the `name` field of the first entry where both
/// `mission` and `mode` match. Falls back to `"unknown.wad"` if no match is
/// found. Corresponds to `D_SuggestIWADName` in `d_iwad.c`.
pub fn D_SuggestIWADName(mission: i32, mode: i32) -> &'static str {
    IWADS
        .iter()
        .find(|iwad| iwad.mission == mission && iwad.mode == mode)
        .map_or("unknown.wad", |iwad| iwad.name)
}

/// Returns a human-readable game name for the given `mission` and `mode`.
///
/// Walks `IWADS` and returns the `description` field of the first entry
/// where the mission matches and either the mode matches or `mode` is
/// `doom::mode::indetermined`. Falls back to `"Unknown game?"` if no match is
/// found. Called from `w_wad.c`.
pub fn D_SuggestGameName(mission: i32, mode: i32) -> &'static str {
    IWADS
        .iter()
        .find(|iwad| {
            iwad.mission == mission && (mode == doom::mode::indetermined || iwad.mode == mode)
        })
        .map_or("Unknown game?", |iwad| iwad.description)
}

/// Validates that the loaded IWAD matches the expected `_mission`.
///
/// This function is intentionally a no-op. The C original also provides an
/// empty implementation in the doomgeneric fork. No validation is performed.
pub fn D_CheckCorrectIWAD(_mission: i32) {
    // Not implemented in original C codebase.
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `DirIsFile` matches the final path component case-insensitively and
    /// requires a separator before it.
    #[test]
    fn dir_is_file_matches_final_component() {
        assert!(DirIsFile("/games/DOOM.WAD", "doom.wad"));
        assert!(!DirIsFile("doom.wad", "doom.wad"));
        assert!(!DirIsFile("/games/xdoom.wad", "doom.wad"));
    }

    /// `IdentifyIWADByName` strips directories and honours the mission mask.
    #[test]
    fn identify_iwad_by_name() {
        let all_missions = -1;
        assert_eq!(
            IdentifyIWADByName("wad/Doom1.WAD", all_missions),
            doom::mode::doom
        );
        assert_eq!(
            IdentifyIWADByName("tnt.wad", all_missions),
            doom::mode::pack_tnt
        );
        assert_eq!(
            IdentifyIWADByName("doom2.wad", 1 << doom::mode::doom),
            doom::mode::none
        );
        assert_eq!(
            IdentifyIWADByName("other.wad", all_missions),
            doom::mode::none
        );
    }
}
