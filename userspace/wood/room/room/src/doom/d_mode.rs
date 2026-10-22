//! Rust port of vendor/doomgeneric/d_mode.c.
//!
//! Game mode and mission validation helpers used throughout the engine to
//! determine what content is available, which maps are valid, and which
//! executable version to emulate.
//!
//! The three central concepts are:
//!
//! * **`GameMission_t`** - which game is being played (Doom 1, Doom 2, Heretic,
//!   Hexen, Strife, or a commercial expansion pack).
//! * **`GameMode_t`** - the release tier of the IWAD (shareware, registered,
//!   commercial, or retail/ultimate).
//! * **`GameVersion_t`** - which engine executable version is being emulated,
//!   used primarily for demo compatibility.
//!
//! All three concepts are modelled here as `pub const c_int` groups rather
//! than Rust enums, preserving bit-for-bit compatibility with the C enums they
//! were ported from and allowing the values to be passed through the FFI
//! boundary without conversion. The corresponding C enums are defined in
//! `d_mode.h`.
//!
//! Notable Rust-vs-C differences:
//! * Enum discriminants are `pub const c_int` rather than Rust `enum` variants
//!   so that they can be stored in `c_int` globals and passed directly over FFI.
//! * `D_GameMissionString` returns `*mut c_char` pointing into static string
//!   literals rather than stack-allocated `char *` (the C version returns
//!   read-only string literals cast to `char *` which would be UB to mutate;
//!   the Rust port matches the C signature but the same caveat applies).
//! * The `skill_t` enum from `d_mode.h` is not present here; it lives in
//!   `doomdef.h` and is ported elsewhere.

#![allow(non_snake_case, non_upper_case_globals)]

use std::ffi::{c_char, c_int};

use crate::types::Boolean;

// ---------------------------------------------------------------------------
// GameMission_t constants
//
// Identify which game (IWAD) is loaded. Corresponds to `GameMission_t` in
// `d_mode.h`. Stored as plain `c_int` for direct FFI compatibility.
// ---------------------------------------------------------------------------

/// No game mission; used as a sentinel / unset value.
///
/// Maps to `none` in `GameMission_t`. Stored as `9` in the C enum.
pub const none: c_int = 9;

/// Doom / Ultimate Doom (IWAD: `doom.wad`, `doom1.wad`, or `doomu.wad`).
///
/// Maps to `doom` (discriminant 0) in `GameMission_t`.
pub const doom: c_int = 0;

/// Doom II: Hell on Earth (IWAD: `doom2.wad`).
///
/// Maps to `doom2` (discriminant 1) in `GameMission_t`.
pub const doom2: c_int = 1;

/// Final Doom: TNT Evilution (IWAD: `tnt.wad`).
///
/// Maps to `pack_tnt` (discriminant 2) in `GameMission_t`.
pub const pack_tnt: c_int = 2;

/// Final Doom: The Plutonia Experiment (IWAD: `plutonia.wad`).
///
/// Maps to `pack_plut` (discriminant 3) in `GameMission_t`.
pub const pack_plut: c_int = 3;

/// Chex Quest (shareware Doom mod; IWAD: `chex.wad`).
///
/// Maps to `pack_chex` (discriminant 4) in `GameMission_t`. Uses episode-based
/// map layout like Doom 1 rather than the `MAPxx` layout of Doom 2.
pub const pack_chex: c_int = 4;

/// Hacx: Twitch 'n Kill (Doom 2 mod; IWAD: `hacx.wad`).
///
/// Maps to `pack_hacx` (discriminant 5) in `GameMission_t`.
pub const pack_hacx: c_int = 5;

/// Heretic: Shadow of the Serpent Riders (IWAD: `heretic.wad`).
///
/// Maps to `heretic` (discriminant 6) in `GameMission_t`.
pub const heretic: c_int = 6;

/// Hexen: Beyond Heretic (IWAD: `hexen.wad`).
///
/// Maps to `hexen` (discriminant 7) in `GameMission_t`.
pub const hexen: c_int = 7;

/// Strife: Quest for the Sigil (IWAD: `strife1.wad`).
///
/// Maps to `strife` (discriminant 8) in `GameMission_t`.
pub const strife: c_int = 8;

// ---------------------------------------------------------------------------
// GameMode_t constants
//
// Identify the release tier of the loaded IWAD. Corresponds to `GameMode_t`
// in `d_mode.h`. Stored as plain `c_int` for direct FFI compatibility.
// ---------------------------------------------------------------------------

/// Shareware release of Doom or Heretic (one episode, freely distributable).
///
/// Maps to `shareware` (discriminant 0) in `GameMode_t`.
pub const shareware: c_int = 0;

/// Registered (three-episode) release of Doom or Heretic.
///
/// Maps to `registered` (discriminant 1) in `GameMode_t`.
pub const registered: c_int = 1;

/// Commercial (MAPxx-based) release: Doom II, Final Doom, Hexen, Strife, etc.
///
/// Maps to `commercial` (discriminant 2) in `GameMode_t`.
pub const commercial: c_int = 2;

/// Retail / Ultimate Doom (four-episode release, `doom.wad`).
///
/// Maps to `retail` (discriminant 3) in `GameMode_t`.
pub const retail: c_int = 3;

/// Unknown or undetected game mode (IWAD not yet loaded or not recognised).
///
/// Maps to `indetermined` (discriminant 4) in `GameMode_t`.
pub const indetermined: c_int = 4;

// ---------------------------------------------------------------------------
// GameVersion_t constants
//
// Identify which executable version is being emulated, primarily for demo
// compatibility. Corresponds to `GameVersion_t` in `d_mode.h`. Stored as
// plain `c_int` for direct FFI compatibility.
// ---------------------------------------------------------------------------

/// Doom v1.2: earliest shareware and registered release.
///
/// Maps to `exe_doom_1_2` (discriminant 0) in `GameVersion_t`.
pub const exe_doom_1_2: c_int = 0;

/// Doom v1.666: first release compatible with all three edition types.
///
/// Maps to `exe_doom_1_666` (discriminant 1) in `GameVersion_t`.
pub const exe_doom_1_666: c_int = 1;

/// Doom v1.7 / v1.7a.
///
/// Maps to `exe_doom_1_7` (discriminant 2) in `GameVersion_t`.
pub const exe_doom_1_7: c_int = 2;

/// Doom v1.8.
///
/// Maps to `exe_doom_1_8` (discriminant 3) in `GameVersion_t`.
pub const exe_doom_1_8: c_int = 3;

/// Doom v1.9: the most widely distributed version; default emulation target.
///
/// Maps to `exe_doom_1_9` (discriminant 4) in `GameVersion_t`.
pub const exe_doom_1_9: c_int = 4;

/// Hacx standalone executable (based on Doom 1.9).
///
/// Maps to `exe_hacx` (discriminant 5) in `GameVersion_t`.
pub const exe_hacx: c_int = 5;

/// Ultimate Doom (retail four-episode) executable.
///
/// Maps to `exe_ultimate` (discriminant 6) in `GameVersion_t`.
pub const exe_ultimate: c_int = 6;

/// Final Doom executable (v1.9 variant used by `tnt.wad` and `plutonia.wad`).
///
/// Maps to `exe_final` (discriminant 7) in `GameVersion_t`.
pub const exe_final: c_int = 7;

/// Alternate Final Doom executable (second `final.exe` binary).
///
/// Maps to `exe_final2` (discriminant 8) in `GameVersion_t`.
pub const exe_final2: c_int = 8;

/// Chex Quest executable (derived from the Final Doom binary).
///
/// Maps to `exe_chex` (discriminant 9) in `GameVersion_t`.
pub const exe_chex: c_int = 9;

/// Heretic v1.3 executable.
///
/// Maps to `exe_heretic_1_3` (discriminant 10) in `GameVersion_t`.
pub const exe_heretic_1_3: c_int = 10;

/// Hexen v1.1 executable.
///
/// Maps to `exe_hexen_1_1` (discriminant 11) in `GameVersion_t`.
pub const exe_hexen_1_1: c_int = 11;

/// Strife v1.2 executable.
///
/// Maps to `exe_strife_1_2` (discriminant 12) in `GameVersion_t`.
pub const exe_strife_1_2: c_int = 12;

/// Strife v1.31 executable.
///
/// Maps to `exe_strife_1_31` (discriminant 13) in `GameVersion_t`.
pub const exe_strife_1_31: c_int = 13;

/// A single valid (mission, mode) combination with its episode and map bounds.
///
/// Each entry in [`VALID_MODES`] records the maximum episode and maximum map
/// number accessible in that combination. [`D_ValidEpisodeMap`] uses these
/// bounds for range-checking. Corresponds to the anonymous struct inside the
/// `valid_modes[]` array in `d_mode.c`.
struct ValidMode {
    /// `GameMission_t` constant for this entry.
    mission: c_int,
    /// `GameMode_t` constant for this entry.
    mode: c_int,
    /// Maximum valid episode number (inclusive).
    episode: c_int,
    /// Maximum valid map number within any episode (inclusive).
    map: c_int,
}

/// Table of all valid (mission, mode) combinations and their map bounds.
///
/// Iterated by [`D_ValidGameMode`], [`D_ValidEpisodeMap`], and
/// [`D_GetNumEpisodes`]. Entries are ordered as in the C `valid_modes[]` array
/// in `d_mode.c`. There is no entry for unknown / indetermined combinations;
/// those return false from the validation functions.
static VALID_MODES: [ValidMode; 13] = [
    ValidMode {
        mission: pack_chex,
        mode: shareware,
        episode: 1,
        map: 5,
    },
    ValidMode {
        mission: doom,
        mode: shareware,
        episode: 1,
        map: 9,
    },
    ValidMode {
        mission: doom,
        mode: registered,
        episode: 3,
        map: 9,
    },
    ValidMode {
        mission: doom,
        mode: retail,
        episode: 4,
        map: 9,
    },
    ValidMode {
        mission: doom2,
        mode: commercial,
        episode: 1,
        map: 32,
    },
    ValidMode {
        mission: pack_tnt,
        mode: commercial,
        episode: 1,
        map: 32,
    },
    ValidMode {
        mission: pack_plut,
        mode: commercial,
        episode: 1,
        map: 32,
    },
    ValidMode {
        mission: pack_hacx,
        mode: commercial,
        episode: 1,
        map: 32,
    },
    ValidMode {
        mission: heretic,
        mode: shareware,
        episode: 1,
        map: 9,
    },
    ValidMode {
        mission: heretic,
        mode: registered,
        episode: 3,
        map: 9,
    },
    ValidMode {
        mission: heretic,
        mode: retail,
        episode: 5,
        map: 9,
    },
    ValidMode {
        mission: hexen,
        mode: commercial,
        episode: 1,
        map: 60,
    },
    ValidMode {
        mission: strife,
        mode: commercial,
        episode: 1,
        map: 34,
    },
];

/// A single valid (mission, version) pair for game-version checking.
///
/// Each entry in [`VALID_VERSIONS`] asserts that a given `GameVersion_t` is
/// legal for a given `GameMission_t`. Corresponds to the anonymous struct
/// inside `valid_versions[]` in `d_mode.c`.
struct ValidVersion {
    /// `GameMission_t` constant for this entry.
    mission: c_int,
    /// `GameVersion_t` constant for this entry.
    version: c_int,
}

/// Table of valid (mission, version) pairs.
///
/// Iterated by [`D_ValidGameVersion`]. Doom-family variants (`doom2`,
/// `pack_plut`, `pack_tnt`, `pack_hacx`, `pack_chex`) are normalised to
/// `doom` before the lookup, so only `doom` entries need to appear here for
/// those games. Corresponds to `valid_versions[]` in `d_mode.c`.
static VALID_VERSIONS: [ValidVersion; 10] = [
    ValidVersion {
        mission: doom,
        version: exe_doom_1_9,
    },
    ValidVersion {
        mission: doom,
        version: exe_hacx,
    },
    ValidVersion {
        mission: doom,
        version: exe_ultimate,
    },
    ValidVersion {
        mission: doom,
        version: exe_final,
    },
    ValidVersion {
        mission: doom,
        version: exe_final2,
    },
    ValidVersion {
        mission: doom,
        version: exe_chex,
    },
    ValidVersion {
        mission: heretic,
        version: exe_heretic_1_3,
    },
    ValidVersion {
        mission: hexen,
        version: exe_hexen_1_1,
    },
    ValidVersion {
        mission: strife,
        version: exe_strife_1_2,
    },
    ValidVersion {
        mission: strife,
        version: exe_strife_1_31,
    },
];

/// Return `TRUE` if `(mission, mode)` is a recognised game configuration.
///
/// Scans `VALID_MODES` for a matching entry. Used to validate a
/// game-mode/mission pair received over the network before accepting it.
/// Returns `FALSE` for unrecognised combinations (e.g., `doom2` + `shareware`).
///
/// Exported as `#[no_mangle]` for C callers. Corresponds to `D_ValidGameMode` in `d_mode.c`.
#[no_mangle]
pub extern "C" fn D_ValidGameMode(mission: c_int, mode: c_int) -> Boolean {
    for vm in &VALID_MODES {
        if vm.mission == mission && vm.mode == mode {
            return Boolean::TRUE;
        }
    }
    Boolean::FALSE
}

/// Return `TRUE` if `episode`/`map` is reachable in the given `(mission, mode)`.
///
/// Checks that `episode` and `map` are both at least 1 and do not exceed the
/// bounds recorded in `VALID_MODES`. Two Heretic-specific secret episodes are
/// handled as special cases before the table lookup:
///
/// * Heretic retail, episode 6: only maps 1-3 are valid (the secret episode).
/// * Heretic registered, episode 4: only map 1 is valid.
///
/// Returns `FALSE` for unknown mission/mode combinations.
///
/// Exported as `#[no_mangle]` for C callers. Corresponds to `D_ValidEpisodeMap` in `d_mode.c`.
#[no_mangle]
pub extern "C" fn D_ValidEpisodeMap(
    mission: c_int,
    mode: c_int,
    episode: c_int,
    map: c_int,
) -> Boolean {
    // Hacks for Heretic secret episodes
    if mission == heretic {
        if mode == retail && episode == 6 {
            return Boolean::from((1..=3).contains(&map));
        } else if mode == registered && episode == 4 {
            return Boolean::from(map == 1);
        }
    }

    for vm in &VALID_MODES {
        if mission == vm.mission && mode == vm.mode {
            return Boolean::from(
                episode >= 1 && episode <= vm.episode && map >= 1 && map <= vm.map,
            );
        }
    }

    Boolean::FALSE
}

/// Return the number of valid episodes for the given `(mission, mode)`.
///
/// Increments an episode counter starting at 1, calling [`D_ValidEpisodeMap`]
/// with map 1 until it returns false, then returns the last valid episode
/// number. Commercial games (Doom 2, Hexen, Strife) have only episode 1.
/// Returns 0 for unknown combinations.
///
/// Exported as `#[no_mangle]` for C callers. Corresponds to `D_GetNumEpisodes` in `d_mode.c`.
#[no_mangle]
pub extern "C" fn D_GetNumEpisodes(mission: c_int, mode: c_int) -> c_int {
    let mut episode = 1;
    while D_ValidEpisodeMap(mission, mode, episode, 1).is_truthy() {
        episode += 1;
    }
    episode - 1
}

/// Return `TRUE` if `version` is a valid executable version for `mission`.
///
/// All Doom-family variants (`doom2`, `pack_plut`, `pack_tnt`, `pack_hacx`,
/// `pack_chex`) are normalised to `doom` before the lookup because they share
/// the same set of valid executable versions. Returns `FALSE` for unknown
/// combinations.
///
/// Exported as `#[no_mangle]` for C callers. Corresponds to `D_ValidGameVersion` in `d_mode.c`.
#[no_mangle]
pub extern "C" fn D_ValidGameVersion(mission: c_int, version: c_int) -> Boolean {
    let mission = if mission == doom2
        || mission == pack_plut
        || mission == pack_tnt
        || mission == pack_hacx
        || mission == pack_chex
    {
        doom
    } else {
        mission
    };

    for vv in &VALID_VERSIONS {
        if vv.mission == mission && vv.version == version {
            return Boolean::TRUE;
        }
    }

    Boolean::FALSE
}

/// Return `TRUE` if `mission` uses `ExMy` episode-map naming rather than `MAPxx`.
///
/// `doom`, `heretic`, and `pack_chex` use the `ExMy` format (e.g., `E1M1`).
/// All other missions use `MAPxx` (e.g., `MAP01`). This distinction drives
/// level-name formatting and warp/cheat parsing throughout the engine.
///
/// Exported as `#[no_mangle]` for C callers. Corresponds to `D_IsEpisodeMap` in `d_mode.c`.
#[no_mangle]
pub extern "C" fn D_IsEpisodeMap(mission: c_int) -> Boolean {
    match mission {
        doom | heretic | pack_chex => Boolean::TRUE,
        _ => Boolean::FALSE,
    }
}

/// Return a NUL-terminated C string naming the given mission.
///
/// Returns one of `"doom"`, `"doom2"`, `"tnt"`, `"plutonia"`, `"hacx"`,
/// `"chex"`, `"heretic"`, `"hexen"`, `"strife"`, or `"none"` for unrecognised
/// values. The returned pointer refers to a static string literal embedded in
/// the binary and must not be freed or written through.
///
/// The return type is `*mut c_char` to match the C signature, but the memory
/// is read-only; writing to it is undefined behaviour, as it would be in the C
/// original.
///
/// Exported as `#[no_mangle]`; called from `w_wad.c` during WAD loading.
/// Corresponds to `D_GameMissionString` in `d_mode.c`.
#[no_mangle]
pub extern "C" fn D_GameMissionString(mission: c_int) -> *mut c_char {
    match mission {
        doom => c"doom".as_ptr().cast_mut(),
        doom2 => c"doom2".as_ptr().cast_mut(),
        pack_tnt => c"tnt".as_ptr().cast_mut(),
        pack_plut => c"plutonia".as_ptr().cast_mut(),
        pack_hacx => c"hacx".as_ptr().cast_mut(),
        pack_chex => c"chex".as_ptr().cast_mut(),
        heretic => c"heretic".as_ptr().cast_mut(),
        hexen => c"hexen".as_ptr().cast_mut(),
        strife => c"strife".as_ptr().cast_mut(),
        _ => c"none".as_ptr().cast_mut(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Boolean;
    use std::ffi::CStr;

    /// Verifies that `doom` shareware is a valid game mode combination.
    #[test]
    fn valid_game_mode_doom_shareware() {
        assert_eq!(D_ValidGameMode(doom, shareware), Boolean::TRUE);
    }

    /// Verifies that `doom2` shareware is not a valid game mode combination.
    #[test]
    fn valid_game_mode_doom2_shareware_invalid() {
        assert_eq!(D_ValidGameMode(doom2, shareware), Boolean::FALSE);
    }

    /// Verifies that Doom retail has exactly 4 episodes.
    #[test]
    fn get_num_episodes_doom_retail() {
        assert_eq!(D_GetNumEpisodes(doom, retail), 4);
    }

    /// Verifies that `doom2` does not use the episode-map (`ExMy`) naming scheme.
    #[test]
    fn is_episode_map_doom2_false() {
        assert_eq!(D_IsEpisodeMap(doom2), Boolean::FALSE);
    }

    /// Verifies that `doom` uses the episode-map (`ExMy`) naming scheme.
    #[test]
    fn is_episode_map_doom_true() {
        assert_eq!(D_IsEpisodeMap(doom), Boolean::TRUE);
    }

    /// Verifies that `D_GameMissionString` returns `"heretic"` for the Heretic mission.
    #[test]
    fn game_mission_string_heretic() {
        let ptr = D_GameMissionString(heretic);
        let s = unsafe { CStr::from_ptr(ptr) };
        assert_eq!(s.to_str().unwrap(), "heretic");
    }

    /// Verifies that an unrecognised mission value returns `"none"`.
    #[test]
    fn game_mission_string_unknown() {
        let ptr = D_GameMissionString(42);
        let s = unsafe { CStr::from_ptr(ptr) };
        assert_eq!(s.to_str().unwrap(), "none");
    }

    /// Verifies that `exe_final2` is a valid game version for the `doom` mission.
    #[test]
    fn valid_game_version_doom_final2() {
        assert_eq!(D_ValidGameVersion(doom, exe_final2), Boolean::TRUE);
    }

    /// Verifies that `doom2` is normalised to `doom` when checking game versions.
    #[test]
    fn valid_game_version_doom2_mapped_to_doom() {
        assert_eq!(D_ValidGameVersion(doom2, exe_final2), Boolean::TRUE);
    }

    /// Verifies that episode 4 map 9 is valid for Doom retail.
    #[test]
    fn valid_episode_map_doom_retail_ep4_map9() {
        assert_eq!(D_ValidEpisodeMap(doom, retail, 4, 9), Boolean::TRUE);
    }

    /// Verifies that episode 5 is out of bounds for Doom retail (max is 4).
    #[test]
    fn valid_episode_map_doom_retail_ep5_map1_invalid() {
        assert_eq!(D_ValidEpisodeMap(doom, retail, 5, 1), Boolean::FALSE);
    }

    /// Heretic retail secret episode 6 allows maps 1-3 only.
    #[test]
    fn valid_episode_map_heretic_retail_ep6_maps_1_to_3() {
        assert_eq!(D_ValidEpisodeMap(heretic, retail, 6, 1), Boolean::TRUE);
        assert_eq!(D_ValidEpisodeMap(heretic, retail, 6, 2), Boolean::TRUE);
        assert_eq!(D_ValidEpisodeMap(heretic, retail, 6, 3), Boolean::TRUE);
        assert_eq!(D_ValidEpisodeMap(heretic, retail, 6, 4), Boolean::FALSE);
        assert_eq!(D_ValidEpisodeMap(heretic, retail, 6, 0), Boolean::FALSE);
    }

    /// Heretic registered secret episode 4 allows only map 1.
    #[test]
    fn valid_episode_map_heretic_registered_ep4_map1_only() {
        assert_eq!(D_ValidEpisodeMap(heretic, registered, 4, 1), Boolean::TRUE);
        assert_eq!(D_ValidEpisodeMap(heretic, registered, 4, 2), Boolean::FALSE);
        assert_eq!(D_ValidEpisodeMap(heretic, registered, 4, 0), Boolean::FALSE);
    }

    /// Doom 2 only has episode 1; requesting episode 2 should fail.
    #[test]
    fn valid_episode_map_doom2_ep2_invalid() {
        assert_eq!(D_ValidEpisodeMap(doom2, commercial, 2, 1), Boolean::FALSE);
        assert_eq!(D_ValidEpisodeMap(doom2, commercial, 1, 1), Boolean::TRUE);
        assert_eq!(D_ValidEpisodeMap(doom2, commercial, 1, 32), Boolean::TRUE);
        assert_eq!(D_ValidEpisodeMap(doom2, commercial, 1, 33), Boolean::FALSE);
    }

    /// Map 0 is always invalid.
    #[test]
    fn valid_episode_map_map_zero_invalid() {
        assert_eq!(D_ValidEpisodeMap(doom, retail, 1, 0), Boolean::FALSE);
        assert_eq!(D_ValidEpisodeMap(doom2, commercial, 1, 0), Boolean::FALSE);
    }

    /// D_GetNumEpisodes for doom shareware has 1 episode.
    #[test]
    fn get_num_episodes_doom_shareware() {
        assert_eq!(D_GetNumEpisodes(doom, shareware), 1);
    }

    /// D_GetNumEpisodes for doom2 (commercial) returns 1.
    #[test]
    fn get_num_episodes_doom2() {
        assert_eq!(D_GetNumEpisodes(doom2, commercial), 1);
    }

    /// D_GetNumEpisodes for doom registered returns 3.
    #[test]
    fn get_num_episodes_doom_registered() {
        assert_eq!(D_GetNumEpisodes(doom, registered), 3);
    }

    /// D_IsEpisodeMap is true for pack_chex (chex.wad is episode-based).
    #[test]
    fn is_episode_map_pack_chex_true() {
        assert_eq!(D_IsEpisodeMap(pack_chex), Boolean::TRUE);
    }

    /// pack_tnt / pack_plut are commercial (MAP01–MAP32), not episode-based.
    #[test]
    fn is_episode_map_pack_tnt_false() {
        assert_eq!(D_IsEpisodeMap(pack_tnt), Boolean::FALSE);
        assert_eq!(D_IsEpisodeMap(pack_plut), Boolean::FALSE);
    }

    /// D_ValidGameMode: all expected valid combinations succeed.
    #[test]
    fn valid_game_mode_all_missions() {
        assert_eq!(D_ValidGameMode(doom, retail), Boolean::TRUE);
        assert_eq!(D_ValidGameMode(doom, registered), Boolean::TRUE);
        assert_eq!(D_ValidGameMode(doom2, commercial), Boolean::TRUE);
        assert_eq!(D_ValidGameMode(heretic, shareware), Boolean::TRUE);
        assert_eq!(D_ValidGameMode(hexen, commercial), Boolean::TRUE);
        assert_eq!(D_ValidGameMode(strife, commercial), Boolean::TRUE);
    }

    /// D_ValidGameVersion: doom2 maps to doom for version checks.
    #[test]
    fn valid_game_version_all_doom_variants_map_to_doom() {
        for mission in [doom2, pack_plut, pack_tnt, pack_hacx, pack_chex] {
            assert_eq!(
                D_ValidGameVersion(mission, exe_doom_1_9),
                Boolean::TRUE,
                "mission {mission} should accept exe_doom_1_9"
            );
        }
    }

    /// D_GameMissionString returns the right string for every known mission.
    #[test]
    fn game_mission_string_all_missions() {
        use std::ffi::CStr;
        let cases: &[(c_int, &str)] = &[
            (doom, "doom"),
            (doom2, "doom2"),
            (pack_tnt, "tnt"),
            (pack_plut, "plutonia"),
            (pack_hacx, "hacx"),
            (pack_chex, "chex"),
            (heretic, "heretic"),
            (hexen, "hexen"),
            (strife, "strife"),
            (none, "none"),
        ];
        for (mission, expected) in cases {
            let ptr = D_GameMissionString(*mission);
            let s = unsafe { CStr::from_ptr(ptr) };
            assert_eq!(s.to_str().unwrap(), *expected, "mission={mission}");
        }
    }
}
