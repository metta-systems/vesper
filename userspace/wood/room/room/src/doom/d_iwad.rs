//! Rust port of vendor/doomgeneric/d_iwad.c.
//!
//! IWAD discovery and selection. An IWAD (Internal WAD) is the primary game
//! data file (e.g. `doom.wad`, `doom2.wad`). This module maintains a table of
//! known IWADs, searches a list of candidate directories to locate one on
//! disk, and identifies which game mission and mode a found IWAD belongs to.
//!
//! Differences from the C original:
//! - `BuildIWADDirList` adds only the current directory (`"."`), omitting the
//!   `DOOMWADDIR`/`DOOMWADPATH` environment-variable paths and the
//!   platform-specific Windows registry / Unix standard paths that the full
//!   Chocolate Doom implementation supports. The `ORIGCODE` conditional in the
//!   C source guards those paths; this port permanently takes the simplified
//!   branch and hardcodes `FILES_DIR` as `"."`.
//! - `D_CheckCorrectIWAD` is intentionally a no-op (matches the C original's
//!   empty stub in doomgeneric).

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::{c_char, c_int, c_void};
use std::ptr;

use crate::doom::d_mode;
use crate::doom::m_misc::M_StringJoinA;
use crate::i_error;

/// Hard limit on the number of IWAD search directories.
///
/// Corresponds to `MAX_IWAD_DIRS` in `d_iwad.c`.
const MAX_IWAD_DIRS: usize = 128;

/// Path component separator character (`/`).
///
/// Used when constructing full file paths from a directory and a filename.
const DIR_SEPARATOR: c_char = b'/' as c_char;

/// Null-terminated string form of [`DIR_SEPARATOR`], suitable for passing to
/// C string-joining functions.
const DIR_SEPARATOR_S: &[u8] = b"/\0";

extern "C" {
    /// C standard `printf`; used to emit diagnostic messages during IWAD
    /// search.
    fn printf(fmt: *const c_char, ...) -> c_int;

    /// Returns non-zero if `filename` names an existing regular file.
    fn M_FileExists(filename: *mut c_char) -> c_int;

    /// Case-insensitive string comparison (POSIX).
    fn strcasecmp(s1: *const c_char, s2: *const c_char) -> c_int;
    /// Locates the last occurrence of character `c` in string `s`.
    fn strrchr(s: *const c_char, c: c_int) -> *mut c_char;
    /// Case-sensitive string comparison.
    fn strcmp(s1: *const c_char, s2: *const c_char) -> c_int;
    /// Returns the length of a null-terminated C string (not including the
    /// terminator).
    fn strlen(s: *const c_char) -> usize;
    /// Duplicates a C string, returning a heap-allocated copy.
    fn strdup(s: *const c_char) -> *mut c_char;
    /// Frees a heap-allocated block.
    fn free(ptr: *mut c_void);
    /// Allocates `size` bytes of uninitialised heap memory.
    fn malloc(size: usize) -> *mut c_void;

    /// Number of command-line arguments (equivalent to C `argc`).
    static mut myargc: c_int;
    /// Command-line argument vector (equivalent to C `argv`).
    static mut myargv: *mut *mut c_char;
    /// Returns the index of the first occurrence of `check` in `myargv`,
    /// provided that at least `num_args` further arguments follow it, or 0
    /// if not found.
    fn M_CheckParmWithArgs(check: *const c_char, num_args: c_int) -> c_int;
}

/// Metadata for a single known IWAD file.
///
/// Corresponds to `iwad_t` / `typedef struct { ... } iwad_t` in `d_iwad.h`.
/// Instances live in the static `IWADS` table.
///
/// # Layout invariant
/// This type is `#[repr(C)]` to match the C struct layout exactly, allowing
/// pointers to array elements to be passed back to C callers.
#[repr(C)]
pub struct iwad_t {
    /// Canonical filename of the IWAD (e.g. `"doom2.wad\0"`).
    pub name: *mut c_char,
    /// Game mission identifier; one of the `d_mode::*` integer constants.
    pub mission: c_int,
    /// Game mode identifier (shareware, retail, commercial, etc.).
    pub mode: c_int,
    /// Human-readable name of the game (e.g. `"Doom II\0"`).
    pub description: *mut c_char,
}

// SAFETY: All `*mut c_char` fields point into static string literals and are
// never mutated; the struct is effectively immutable once constructed.
unsafe impl Sync for iwad_t {}

/// Table of all known IWAD files, in priority order.
///
/// The ordering determines which IWAD is selected when multiple candidates are
/// present in the same directory. Commercial releases are listed before
/// shareware releases. Mirrors `iwads[]` in `d_iwad.c`.
static IWADS: [iwad_t; 14] = [
    iwad_t {
        name: c"doom2.wad".as_ptr().cast_mut(),
        mission: d_mode::doom2,
        mode: d_mode::commercial,
        description: c"Doom II".as_ptr().cast_mut(),
    },
    iwad_t {
        name: c"plutonia.wad".as_ptr().cast_mut(),
        mission: d_mode::pack_plut,
        mode: d_mode::commercial,
        description: c"Final Doom: Plutonia Experiment".as_ptr().cast_mut(),
    },
    iwad_t {
        name: c"tnt.wad".as_ptr().cast_mut(),
        mission: d_mode::pack_tnt,
        mode: d_mode::commercial,
        description: c"Final Doom: TNT: Evilution".as_ptr().cast_mut(),
    },
    iwad_t {
        name: c"doom.wad".as_ptr().cast_mut(),
        mission: d_mode::doom,
        mode: d_mode::retail,
        description: c"Doom".as_ptr().cast_mut(),
    },
    iwad_t {
        name: c"doom1.wad".as_ptr().cast_mut(),
        mission: d_mode::doom,
        mode: d_mode::shareware,
        description: c"Doom Shareware".as_ptr().cast_mut(),
    },
    iwad_t {
        name: c"chex.wad".as_ptr().cast_mut(),
        mission: d_mode::pack_chex,
        mode: d_mode::shareware,
        description: c"Chex Quest".as_ptr().cast_mut(),
    },
    iwad_t {
        name: c"hacx.wad".as_ptr().cast_mut(),
        mission: d_mode::pack_hacx,
        mode: d_mode::commercial,
        description: c"Hacx".as_ptr().cast_mut(),
    },
    iwad_t {
        name: c"freedm.wad".as_ptr().cast_mut(),
        mission: d_mode::doom2,
        mode: d_mode::commercial,
        description: c"FreeDM".as_ptr().cast_mut(),
    },
    iwad_t {
        name: c"freedoom2.wad".as_ptr().cast_mut(),
        mission: d_mode::doom2,
        mode: d_mode::commercial,
        description: c"Freedoom: Phase 2".as_ptr().cast_mut(),
    },
    iwad_t {
        name: c"freedoom1.wad".as_ptr().cast_mut(),
        mission: d_mode::doom,
        mode: d_mode::retail,
        description: c"Freedoom: Phase 1".as_ptr().cast_mut(),
    },
    iwad_t {
        name: c"heretic.wad".as_ptr().cast_mut(),
        mission: d_mode::heretic,
        mode: d_mode::retail,
        description: c"Heretic".as_ptr().cast_mut(),
    },
    iwad_t {
        name: c"heretic1.wad".as_ptr().cast_mut(),
        mission: d_mode::heretic,
        mode: d_mode::shareware,
        description: c"Heretic Shareware".as_ptr().cast_mut(),
    },
    iwad_t {
        name: c"hexen.wad".as_ptr().cast_mut(),
        mission: d_mode::hexen,
        mode: d_mode::commercial,
        description: c"Hexen".as_ptr().cast_mut(),
    },
    iwad_t {
        name: c"strife1.wad".as_ptr().cast_mut(),
        mission: d_mode::strife,
        mode: d_mode::commercial,
        description: c"Strife".as_ptr().cast_mut(),
    },
];

/// Whether `BuildIWADDirList` has already been called.
///
/// Guards against rebuilding the directory list on repeated calls.
static mut iwad_dirs_built: bool = false;

/// Array of directories to search for IWAD files.
///
/// Populated lazily by `BuildIWADDirList`. At most [`MAX_IWAD_DIRS`]
/// entries are stored. Corresponds to `iwad_dirs[]` in `d_iwad.c`.
static mut iwad_dirs: [*mut c_char; MAX_IWAD_DIRS] = [ptr::null_mut(); MAX_IWAD_DIRS];

/// Number of valid entries in [`iwad_dirs`].
static mut num_iwad_dirs: c_int = 0;

/// Appends `dir` to the global IWAD search directory list if the list is not
/// already full.
///
/// Silently drops `dir` when [`MAX_IWAD_DIRS`] has been reached.
/// Corresponds to `AddIWADDir` in `d_iwad.c`.
///
/// # Safety
/// `dir` must be a valid, non-null pointer to a null-terminated C string that
/// remains valid for as long as it may be read from `iwad_dirs`. Callers must
/// only invoke this function from the single-threaded game-startup path, as it
/// writes to the mutable globals `iwad_dirs` and `num_iwad_dirs` without
/// synchronisation.
unsafe fn AddIWADDir(dir: *mut c_char) {
    if num_iwad_dirs < MAX_IWAD_DIRS as c_int {
        iwad_dirs[num_iwad_dirs as usize] = dir;
        num_iwad_dirs += 1;
    }
}

/// Returns non-zero if `path` is a file path whose final component equals
/// `filename` (case-insensitive comparison).
///
/// For example, `DirIsFile("/games/doom.wad", "doom.wad")` returns `1`.
/// Corresponds to `DirIsFile` in `d_iwad.c`.
///
/// # Safety
/// Both `path` and `filename` must be valid, non-null pointers to
/// null-terminated C strings. The strings must remain valid for the duration
/// of the call. The function passes these pointers directly to the C FFI
/// functions `strlen` and `strcasecmp`, which impose the same requirements.
unsafe fn DirIsFile(path: *mut c_char, filename: *mut c_char) -> c_int {
    let path_len = strlen(path);
    let filename_len = strlen(filename);

    if path_len > filename_len
        && *path.add(path_len - filename_len - 1) == DIR_SEPARATOR
        && strcasecmp(path.add(path_len - filename_len), filename) == 0
    {
        1
    } else {
        0
    }
}

/// Checks whether `dir` contains the IWAD named `iwadname`, returning a
/// heap-allocated full path on success or null on failure.
///
/// Two cases are handled:
/// - If `dir` is itself a path to the IWAD file (i.e. `DirIsFile` returns
///   true and the file exists), a `strdup` of `dir` is returned.
/// - Otherwise the path `dir/iwadname` is constructed and tested. The special
///   case `dir == "."` elides the directory prefix.
///
/// The caller is responsible for `free`-ing the returned string.
/// Corresponds to `CheckDirectoryHasIWAD` in `d_iwad.c`.
///
/// # Safety
/// Both `dir` and `iwadname` must be valid, non-null pointers to
/// null-terminated C strings that remain valid for the duration of the call.
/// These pointers are passed to `strlen`, `strcasecmp`, `strcmp`, `strdup`,
/// and `free` via C FFI, all of which require the same pointer-validity
/// guarantee. The heap-allocated string returned on success must be freed by
/// the caller using `free`.
unsafe fn CheckDirectoryHasIWAD(dir: *mut c_char, iwadname: *mut c_char) -> *mut c_char {
    if DirIsFile(dir, iwadname) != 0 && M_FileExists(dir) != 0 {
        return strdup(dir);
    }

    let filename = if strcmp(dir, c".".as_ptr()) == 0 {
        strdup(iwadname)
    } else {
        let strs: [*const c_char; 4] = [
            dir as *const c_char,
            DIR_SEPARATOR_S.as_ptr() as *const c_char,
            iwadname as *const c_char,
            ptr::null(),
        ];
        // SAFETY: null-terminated pointer array; ownership transferred to caller via return.
        M_StringJoinA(strs.as_ptr())
    };

    printf(c"Trying IWAD file:%s\n".as_ptr(), filename);

    if M_FileExists(filename) != 0 {
        return filename;
    }

    free(filename as *mut c_void);
    ptr::null_mut()
}

/// Searches a single directory `dir` for the first IWAD entry in `IWADS`
/// that passes the `mask` filter and exists on disk.
///
/// On success, writes the matched mission to `*mission` and returns a
/// heap-allocated path string (caller must `free` it). Returns null if no
/// matching IWAD is found. Corresponds to `SearchDirectoryForIWAD` in
/// `d_iwad.c`.
///
/// # Safety
/// `mission` must be a valid, non-null pointer.
unsafe fn SearchDirectoryForIWAD(
    dir: *mut c_char,
    mask: c_int,
    mission: *mut c_int,
) -> *mut c_char {
    for i in 0..IWADS.len() {
        if ((1 << IWADS[i].mission) & mask) == 0 {
            continue;
        }

        let filename = CheckDirectoryHasIWAD(dir, IWADS[i].name);

        if !filename.is_null() {
            *mission = IWADS[i].mission;
            return filename;
        }
    }

    ptr::null_mut()
}

/// Identifies the game mission for an IWAD given its filename.
///
/// Strips any leading directory components from `name`, then performs a
/// case-insensitive comparison against each entry in `IWADS` that passes the
/// `mask` filter. Returns the matching `d_mode::*` mission constant, or
/// `d_mode::none` if the filename is not recognised.
///
/// Corresponds to `IdentifyIWADByName` in `d_iwad.c`.
///
/// # Safety
/// `name` must be a valid, non-null pointer to a null-terminated C string that
/// remains valid for the duration of the call. The pointer is passed to the C
/// FFI functions `strrchr` and `strcasecmp`, which require a valid
/// null-terminated string. The pointer returned by `strrchr` (if non-null) is
/// an interior pointer into the same string and is used only within this call.
unsafe fn IdentifyIWADByName(mut name: *mut c_char, mask: c_int) -> c_int {
    let p = strrchr(name, DIR_SEPARATOR as c_int);
    if !p.is_null() {
        name = p.add(1);
    }

    let mut mission = d_mode::none;

    for i in 0..IWADS.len() {
        if ((1 << IWADS[i].mission) & mask) == 0 {
            continue;
        }

        if strcasecmp(name, IWADS[i].name) == 0 {
            mission = IWADS[i].mission;
            break;
        }
    }

    mission
}

/// Populates the global IWAD directory list if it has not been built yet.
///
/// This simplified port always adds only the current directory (`"."`).  The
/// full Chocolate Doom implementation (guarded by `ORIGCODE` and `_WIN32` in
/// the C source) additionally checks `DOOMWADDIR`, `DOOMWADPATH`, Windows
/// registry keys, and standard Unix paths -- none of which are supported here.
/// Corresponds to `BuildIWADDirList` in `d_iwad.c`.
///
/// # Safety
/// Must be called from the single-threaded game-startup path only. The
/// function writes to the mutable globals `iwad_dirs`, `num_iwad_dirs`, and
/// `iwad_dirs_built` without synchronisation, and calls `AddIWADDir` which
/// imposes the same requirement.
unsafe fn BuildIWADDirList() {
    AddIWADDir(c".".as_ptr().cast_mut());
    iwad_dirs_built = true;
}

/// Searches IWAD search paths for a WAD file with the given `name`.
///
/// If `name` already refers to an existing file on disk, it is returned as-is
/// (no allocation). Otherwise the function calls `BuildIWADDirList` and
/// iterates over every candidate directory, trying both the directory path
/// itself (when `name` is the final component) and the concatenated path
/// `dir/name`. Returns a heap-allocated path string on success, or null if the
/// file cannot be found anywhere. The caller is responsible for `free`-ing any
/// returned string that is not identical to `name`.
///
/// Called from `d_main.c` and `w_main.c`.
///
/// # Safety
/// `name` must be a valid, null-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn D_FindWADByName(name: *mut c_char) -> *mut c_char {
    if M_FileExists(name) != 0 {
        return name;
    }

    BuildIWADDirList();

    for i in 0..num_iwad_dirs {
        if DirIsFile(iwad_dirs[i as usize], name) != 0 && M_FileExists(iwad_dirs[i as usize]) != 0 {
            return strdup(iwad_dirs[i as usize]);
        }

        let strs: [*const c_char; 4] = [
            iwad_dirs[i as usize] as *const c_char,
            DIR_SEPARATOR_S.as_ptr() as *const c_char,
            name as *const c_char,
            ptr::null(),
        ];
        // SAFETY: null-terminated pointer array; freed below on miss, transferred to caller on hit.
        let path = M_StringJoinA(strs.as_ptr());

        if M_FileExists(path) != 0 {
            return path;
        }

        free(path as *mut c_void);
    }

    ptr::null_mut()
}

/// Searches for a WAD by filename, falling back to the original `filename`
/// pointer if the file cannot be found.
///
/// Unlike [`D_FindWADByName`], this function always returns a non-null
/// pointer: either a heap-allocated path string on success, or the original
/// `filename` argument unchanged on failure. Called from `w_main.c`.
///
/// # Safety
/// `filename` must be a valid, null-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn D_TryFindWADByName(filename: *mut c_char) -> *mut c_char {
    let result = D_FindWADByName(filename);
    if !result.is_null() {
        result
    } else {
        filename
    }
}

/// Locates an IWAD file on disk and identifies its game mission.
///
/// If the `-iwad <file>` command-line argument is present, the specified file
/// is located via [`D_FindWADByName`] (aborting with `I_Error` if not found)
/// and its mission is identified by filename. Otherwise, the function scans
/// all IWAD search directories for any IWAD whose mission bit is set in `mask`.
///
/// Returns a heap-allocated path to the found IWAD, or null if none was found
/// in the auto-scan path. Writes the identified mission to `*mission`.
///
/// Called from `d_main.c`.
///
/// # Safety
/// `mission` must be a valid, non-null pointer.
#[no_mangle]
pub unsafe extern "C" fn D_FindIWAD(mask: c_int, mission: *mut c_int) -> *mut c_char {
    let iwadparm = M_CheckParmWithArgs(c"-iwad".as_ptr(), 1);

    if iwadparm != 0 {
        let iwadfile = *myargv.offset((iwadparm + 1) as isize);
        let result = D_FindWADByName(iwadfile);

        if result.is_null() {
            i_error!(
                "IWAD file '{}' not found!",
                std::ffi::CStr::from_ptr(iwadfile).to_string_lossy()
            );
        }

        *mission = IdentifyIWADByName(result, mask);
        result
    } else {
        printf(c"-iwad not specified, trying a few iwad names\n".as_ptr());

        let mut result: *mut c_char = ptr::null_mut();

        BuildIWADDirList();

        for i in 0..num_iwad_dirs {
            if !result.is_null() {
                break;
            }
            result = SearchDirectoryForIWAD(iwad_dirs[i as usize], mask, mission);
        }

        result
    }
}

/// Finds all IWAD files on disk whose mission bits are set in `mask`.
///
/// Allocates and returns a null-terminated array of pointers into `IWADS`.
/// Each element points to a static `iwad_t` whose backing WAD file was found
/// on disk. The array itself is heap-allocated and must be freed by the caller
/// (the pointed-to `iwad_t` entries must not be freed). Corresponds to
/// `D_FindAllIWADs` in `d_iwad.c`.
///
/// # Safety
/// The returned pointer must be freed with `free` when no longer needed.
#[no_mangle]
pub unsafe extern "C" fn D_FindAllIWADs(mask: c_int) -> *mut *const iwad_t {
    let result =
        malloc(std::mem::size_of::<*const iwad_t>() * (IWADS.len() + 1)) as *mut *const iwad_t;
    let mut result_len: usize = 0;

    for i in 0..IWADS.len() {
        if ((1 << IWADS[i].mission) & mask) == 0 {
            continue;
        }

        let filename = D_FindWADByName(IWADS[i].name);

        if !filename.is_null() {
            *result.add(result_len) = &IWADS[i];
            result_len += 1;
        }
    }

    *result.add(result_len) = ptr::null();

    result
}

/// Returns the canonical IWAD filename for the given `gamemission`.
///
/// Walks `IWADS` and returns the `name` field of the first entry whose
/// mission matches `gamemission`. Falls back to `"unknown.wad"` if no match
/// is found. The returned pointer points into a static string and must not be
/// freed. This name is used as the savegame subdirectory so that `doom.wad`
/// and `doom1.wad` saves share the same location.
///
/// Called from `d_main.c`.
#[no_mangle]
pub unsafe extern "C" fn D_SaveGameIWADName(gamemission: c_int) -> *mut c_char {
    for i in 0..IWADS.len() {
        if gamemission == IWADS[i].mission {
            return IWADS[i].name;
        }
    }
    c"unknown.wad".as_ptr().cast_mut()
}

/// Returns the canonical IWAD filename that best matches `mission` and `mode`.
///
/// Walks `IWADS` and returns the `name` field of the first entry where both
/// `mission` and `mode` match. Falls back to `"unknown.wad"` if no match is
/// found. The returned pointer points into a static string and must not be
/// freed. Corresponds to `D_SuggestIWADName` in `d_iwad.c`.
#[no_mangle]
pub unsafe extern "C" fn D_SuggestIWADName(mission: c_int, mode: c_int) -> *mut c_char {
    for i in 0..IWADS.len() {
        if IWADS[i].mission == mission && IWADS[i].mode == mode {
            return IWADS[i].name;
        }
    }
    c"unknown.wad".as_ptr().cast_mut()
}

/// Returns a human-readable game name for the given `mission` and `mode`.
///
/// Walks `IWADS` and returns the `description` field of the first entry
/// where the mission matches and either the mode matches or `mode` is
/// `d_mode::indetermined`. Falls back to `"Unknown game?"` if no match is
/// found. The returned pointer points into a static string and must not be
/// freed. Called from `w_wad.c`.
#[no_mangle]
pub unsafe extern "C" fn D_SuggestGameName(mission: c_int, mode: c_int) -> *mut c_char {
    for i in 0..IWADS.len() {
        if IWADS[i].mission == mission && (mode == d_mode::indetermined || IWADS[i].mode == mode) {
            return IWADS[i].description;
        }
    }
    c"Unknown game?".as_ptr().cast_mut()
}

/// Validates that the loaded IWAD matches the expected `_mission`.
///
/// This function is intentionally a no-op. The C original also provides an
/// empty implementation in the doomgeneric fork. No validation is performed.
#[no_mangle]
pub extern "C" fn D_CheckCorrectIWAD(_mission: c_int) {
    // Not implemented in original C codebase.
}
