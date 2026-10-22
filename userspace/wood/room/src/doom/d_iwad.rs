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

use std::ffi::CString;

use crate::{
    doom::{
        d_mode,
        m_argv::{myargv, M_CheckParmWithArgs},
        m_misc::{c_str, M_FileExists},
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
    /// Game mission identifier; one of the `d_mode::*` integer constants.
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
        mission: d_mode::doom2,
        mode: d_mode::commercial,
        description: "Doom II",
    },
    iwad_t {
        name: "plutonia.wad",
        mission: d_mode::pack_plut,
        mode: d_mode::commercial,
        description: "Final Doom: Plutonia Experiment",
    },
    iwad_t {
        name: "tnt.wad",
        mission: d_mode::pack_tnt,
        mode: d_mode::commercial,
        description: "Final Doom: TNT: Evilution",
    },
    iwad_t {
        name: "doom.wad",
        mission: d_mode::doom,
        mode: d_mode::retail,
        description: "Doom",
    },
    iwad_t {
        name: "doom1.wad",
        mission: d_mode::doom,
        mode: d_mode::shareware,
        description: "Doom Shareware",
    },
    iwad_t {
        name: "chex.wad",
        mission: d_mode::pack_chex,
        mode: d_mode::shareware,
        description: "Chex Quest",
    },
    iwad_t {
        name: "hacx.wad",
        mission: d_mode::pack_hacx,
        mode: d_mode::commercial,
        description: "Hacx",
    },
    iwad_t {
        name: "freedm.wad",
        mission: d_mode::doom2,
        mode: d_mode::commercial,
        description: "FreeDM",
    },
    iwad_t {
        name: "freedoom2.wad",
        mission: d_mode::doom2,
        mode: d_mode::commercial,
        description: "Freedoom: Phase 2",
    },
    iwad_t {
        name: "freedoom1.wad",
        mission: d_mode::doom,
        mode: d_mode::retail,
        description: "Freedoom: Phase 1",
    },
    iwad_t {
        name: "heretic.wad",
        mission: d_mode::heretic,
        mode: d_mode::retail,
        description: "Heretic",
    },
    iwad_t {
        name: "heretic1.wad",
        mission: d_mode::heretic,
        mode: d_mode::shareware,
        description: "Heretic Shareware",
    },
    iwad_t {
        name: "hexen.wad",
        mission: d_mode::hexen,
        mode: d_mode::commercial,
        description: "Hexen",
    },
    iwad_t {
        name: "strife1.wad",
        mission: d_mode::strife,
        mode: d_mode::commercial,
        description: "Strife",
    },
];

/// Directories to search for IWAD files.
///
/// Corresponds to `iwad_dirs[]` as filled by `BuildIWADDirList` in
/// `d_iwad.c`; this port only searches the current directory.
const IWAD_DIRS: [&str; 1] = ["."];

/// Hand `text` to code that still takes a C string.
///
/// shortcut: leaks the allocation, like the C original's `malloc`ed paths;
/// drop once the callers take `&str`.
fn into_c_string(text: &str) -> *mut u8 {
    CString::new(text).unwrap().into_raw().cast()
}

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
/// `mask` filter. Returns the matching `d_mode::*` mission constant, or
/// `d_mode::none` if the filename is not recognised.
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
        .map_or(d_mode::none, |iwad| iwad.mission)
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
/// pointer if the file cannot be found.
///
/// Unlike [`D_FindWADByName`], this function always returns a non-null
/// pointer: either the found path, or the original `filename` argument
/// unchanged on failure. Called from `w_main.c`.
///
/// # Safety
/// `filename` must be a valid, null-terminated C string.
pub unsafe fn D_TryFindWADByName(filename: *mut u8) -> *mut u8 {
    std::str::from_utf8(unsafe { c_str(filename) })
        .ok()
        .and_then(D_FindWADByName)
        .map_or(filename, |path| into_c_string(&path))
}

/// Locates an IWAD file on disk and identifies its game mission.
///
/// If the `-iwad <file>` command-line argument is present, the specified file
/// is located via [`D_FindWADByName`] (aborting with `I_Error` if not found)
/// and its mission is identified by filename. Otherwise, the function scans
/// all IWAD search directories for any IWAD whose mission bit is set in `mask`.
///
/// Returns the path to the found IWAD, or null if none was found in the
/// auto-scan path. Writes the identified mission to `*mission`.
///
/// Called from `d_main.c`.
///
/// # Safety
/// `mission` must be a valid, non-null pointer.
pub unsafe fn D_FindIWAD(mask: i32, mission: *mut i32) -> *mut u8 {
    let iwadparm = M_CheckParmWithArgs(c"-iwad".as_ptr().cast(), 1);

    if iwadparm != 0 {
        let iwadfile = String::from_utf8_lossy(c_str(*myargv.offset((iwadparm + 1) as isize)));
        let Some(result) = D_FindWADByName(&iwadfile) else {
            i_error!("IWAD file '{}' not found!", iwadfile);
        };

        *mission = IdentifyIWADByName(&result, mask);
        into_c_string(&result)
    } else {
        println!("-iwad not specified, trying a few iwad names");

        IWAD_DIRS
            .iter()
            .find_map(|dir| SearchDirectoryForIWAD(dir, mask, &mut *mission))
            .map_or(std::ptr::null_mut(), |path| into_c_string(&path))
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
/// `d_mode::indetermined`. Falls back to `"Unknown game?"` if no match is
/// found. Called from `w_wad.c`, which still takes a C string.
pub fn D_SuggestGameName(mission: i32, mode: i32) -> *mut u8 {
    let description = IWADS
        .iter()
        .find(|iwad| iwad.mission == mission && (mode == d_mode::indetermined || iwad.mode == mode))
        .map_or("Unknown game?", |iwad| iwad.description);
    into_c_string(description)
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
            d_mode::doom
        );
        assert_eq!(
            IdentifyIWADByName("tnt.wad", all_missions),
            d_mode::pack_tnt
        );
        assert_eq!(
            IdentifyIWADByName("doom2.wad", 1 << d_mode::doom),
            d_mode::none
        );
        assert_eq!(IdentifyIWADByName("other.wad", all_missions), d_mode::none);
    }
}
