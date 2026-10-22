//! Tests for `hu_stuff.c` — HUD globals, map-name tables, and chat strings.
//!
//! `hu_stuff.c` owns the HUD subsystem: map names, chat macros, player-name
//! prefixes, and a set of toggle flags.  All string arrays are populated with
//! string literals from `d_englsh.h` at program start, so they can be read
//! without calling any initialisation function.
//!
//! These tests verify:
//!   * HUD font constants (`HU_FONTSTART`, `HU_FONTEND`, `HU_FONTSIZE`)
//!   * Chat-macro default strings (verbatim from d_englsh.h)
//!   * Player-name prefixes match the expected color labels
//!   * Map-name pointers are non-null and spot-checked for correct content
//!   * Toggle-flag defaults (`chat_on`, `message_dontfuckwithme`)

#![allow(non_snake_case)]

use std::ffi::{c_char, CStr};

use crate::doom::hu_stuff;

// ---------------------------------------------------------------------------
// HUD font constants
// ---------------------------------------------------------------------------

/// The font starts at '!' (ASCII 33) — the first printable non-space character.
#[test]
fn hu_fontstart_is_exclamation() {
    assert_eq!(
        hu_stuff::HU_FONTSTART,
        b'!',
        "HU_FONTSTART should be '!' (33)"
    );
    assert_eq!(hu_stuff::HU_FONTSTART, 33);
}

/// The font ends at '_' (ASCII 95) — last character in the original bitmap font.
#[test]
fn hu_fontend_is_underscore() {
    assert_eq!(hu_stuff::HU_FONTEND, b'_', "HU_FONTEND should be '_' (95)");
    assert_eq!(hu_stuff::HU_FONTEND, 95);
}

/// `HU_FONTSIZE = HU_FONTEND − HU_FONTSTART + 1 = 63`.
/// This is the number of glyphs available and must match the size of the
/// `hu_font[]` patch array in `HU_Init`.
#[test]
fn hu_fontsize_is_63() {
    assert_eq!(hu_stuff::HU_FONTSIZE, 63);
    // Verify the derivation
    assert_eq!(
        hu_stuff::HU_FONTSIZE,
        (hu_stuff::HU_FONTEND - hu_stuff::HU_FONTSTART + 1) as usize
    );
}

// ---------------------------------------------------------------------------
// HUD layout constants
// ---------------------------------------------------------------------------

/// `HU_BROADCAST = 5`: the index used for "broadcast to all players" in
/// multi-player chat (everyone hears it).
#[test]
fn hu_broadcast_is_5() {
    assert_eq!(hu_stuff::HU_BROADCAST, 5);
}

/// `HU_MSGWIDTH = 64`: maximum number of characters per HUD message line.
#[test]
fn hu_msgwidth_is_64() {
    assert_eq!(hu_stuff::HU_MSGWIDTH, 64);
}

/// `HU_MSGHEIGHT = 1`: the message area is exactly one line tall.
#[test]
fn hu_msgheight_is_1() {
    assert_eq!(hu_stuff::HU_MSGHEIGHT, 1);
}

// ---------------------------------------------------------------------------
// chat_macros – 10 entries, all non-null
// ---------------------------------------------------------------------------

/// The `chat_macros` array has exactly 10 elements (Ctrl+0 … Ctrl+9).
#[test]
fn chat_macros_count_is_10() {
    unsafe {
        assert_eq!(hu_stuff::chat_macros.len(), 10);
    }
}

/// All 10 chat-macro pointers must be non-null at program start.
/// They point to string literals in d_englsh.h and are never deallocated.
#[test]
fn chat_macros_all_non_null() {
    unsafe {
        for (i, &ptr) in hu_stuff::chat_macros.iter().enumerate() {
            assert!(!ptr.is_null(), "chat_macros[{i}] should be non-null");
        }
    }
}

/// The default chat macros match the verbatim strings from `d_englsh.h`.
/// Any deviation means the string table has been changed or a DEH patch was
/// applied at program start, which is not expected in the test binary.
#[test]
fn chat_macros_default_strings() {
    // d_englsh.h order: CHATMACRO0 = "No", CHATMACRO1 = "I'm ready to…", …
    let expected: [&str; 10] = [
        "No",
        "I'm ready to kick butt!",
        "I'm OK.",
        "I'm not looking too good!",
        "Help!",
        "You suck!",
        "Next time, scumbag...",
        "Come here!",
        "I'll take care of it.",
        "Yes",
    ];
    unsafe {
        for (i, (&ptr, &want)) in hu_stuff::chat_macros
            .iter()
            .zip(expected.iter())
            .enumerate()
        {
            let got = CStr::from_ptr(ptr as *const c_char).to_str().unwrap();
            assert_eq!(got, want, "chat_macros[{i}] mismatch");
        }
    }
}

// ---------------------------------------------------------------------------
// player_names – 4 entries (one per MAXPLAYERS)
// ---------------------------------------------------------------------------

/// The `player_names` array has exactly 4 elements, one per supported player
/// (MAXPLAYERS = 4).
#[test]
fn player_names_count_is_4() {
    unsafe {
        assert_eq!(hu_stuff::player_names.len(), 4);
    }
}

/// Player-name prefixes must match the color labels from `d_englsh.h`.
/// Green is player 0, Indigo player 1, Brown player 2, Red player 3.
/// The colon+space suffix is part of the vanilla string; stripping it would
/// break multi-player chat display.
#[test]
fn player_names_default_strings() {
    let expected = ["Green: ", "Indigo: ", "Brown: ", "Red: "];
    unsafe {
        for (i, (&ptr, &want)) in hu_stuff::player_names
            .iter()
            .zip(expected.iter())
            .enumerate()
        {
            assert!(!ptr.is_null(), "player_names[{i}] should be non-null");
            let got = CStr::from_ptr(ptr as *const c_char).to_str().unwrap();
            assert_eq!(got, want, "player_names[{i}] mismatch");
        }
    }
}

// ---------------------------------------------------------------------------
// mapnames – episode/map name table
// ---------------------------------------------------------------------------

/// The first map-name entry must be "E1M1: Hangar".
/// This is index 0 in the shareware/registered/retail DOOM map table.
#[test]
fn mapnames_first_entry_is_e1m1_hangar() {
    unsafe {
        let ptr = hu_stuff::mapnames[0];
        assert!(!ptr.is_null(), "mapnames[0] should not be null");
        let name = CStr::from_ptr(ptr as *const c_char).to_str().unwrap();
        assert_eq!(name, "E1M1: Hangar");
    }
}

/// E2M1 is at index 9 (after the nine E1 maps).
#[test]
fn mapnames_e2m1_is_at_index_9() {
    unsafe {
        let ptr = hu_stuff::mapnames[9];
        assert!(!ptr.is_null(), "mapnames[9] should not be null");
        let name = CStr::from_ptr(ptr as *const c_char).to_str().unwrap();
        assert_eq!(name, "E2M1: Deimos Anomaly");
    }
}

/// E4M9 "Fear" is at index 35 (the last real DOOM retail map entry).
#[test]
fn mapnames_e4m9_is_at_index_35() {
    unsafe {
        let ptr = hu_stuff::mapnames[35];
        assert!(!ptr.is_null(), "mapnames[35] should not be null");
        let name = CStr::from_ptr(ptr as *const c_char).to_str().unwrap();
        assert_eq!(name, "E4M9: Fear");
    }
}

/// Indices 36–44 are placeholder "NEWLEVEL" strings; vanilla Doom overflows
/// into them when a non-standard episode is used.  These must remain "NEWLEVEL"
/// so that the memory layout is preserved for pl2.wad-style overflow access.
#[test]
fn mapnames_placeholders_are_newlevel() {
    unsafe {
        for i in 36..45usize {
            let ptr = hu_stuff::mapnames[i];
            assert!(!ptr.is_null(), "mapnames[{i}] should not be null");
            let name = CStr::from_ptr(ptr as *const c_char).to_str().unwrap();
            assert_eq!(name, "NEWLEVEL", "mapnames[{i}] should be NEWLEVEL");
        }
    }
}

// ---------------------------------------------------------------------------
// mapnames_commercial – DOOM 2 / Plutonia / TNT name table
// ---------------------------------------------------------------------------

/// The first commercial map name is "level 1: entryway" (DOOM 2 MAP01).
#[test]
fn mapnames_commercial_first_is_entryway() {
    unsafe {
        let ptr = hu_stuff::mapnames_commercial[0];
        assert!(!ptr.is_null(), "mapnames_commercial[0] should not be null");
        let name = CStr::from_ptr(ptr as *const c_char).to_str().unwrap();
        assert_eq!(name, "level 1: entryway");
    }
}

/// MAP32 "Grosse" is at index 31 in the commercial table.
#[test]
fn mapnames_commercial_map32_is_grosse() {
    unsafe {
        let ptr = hu_stuff::mapnames_commercial[31];
        assert!(!ptr.is_null(), "mapnames_commercial[31] should not be null");
        let name = CStr::from_ptr(ptr as *const c_char).to_str().unwrap();
        assert_eq!(name, "level 32: grosse");
    }
}

/// Plutonia MAP01 is at index 32 (after the 32 DOOM 2 entries).
/// Vanilla DOOM 2 can read into this region via the `HU_TITLEP` macro when
/// the commercial PWAD is Plutonia.
#[test]
fn mapnames_commercial_plutonia_starts_at_index_32() {
    unsafe {
        let ptr = hu_stuff::mapnames_commercial[32];
        assert!(!ptr.is_null(), "mapnames_commercial[32] should not be null");
        let name = CStr::from_ptr(ptr as *const c_char).to_str().unwrap();
        assert_eq!(name, "level 1: congo");
    }
}

// ---------------------------------------------------------------------------
// Toggle flags – default values
// ---------------------------------------------------------------------------

/// `chat_on` must be false (0) at program start; it is set true only when the
/// player presses the chat key.
#[test]
fn chat_on_default_false() {
    unsafe {
        assert_eq!(hu_stuff::chat_on, 0, "chat_on should be false by default");
    }
}

/// `message_dontfuckwithme` must be false by default; it is set true by
/// `P_TouchSpecialThing` for certain pickups that must override the player's
/// "no messages" preference.
#[test]
fn message_dontfuckwithme_default_false() {
    unsafe {
        assert_eq!(
            hu_stuff::message_dontfuckwithme,
            0,
            "message_dontfuckwithme should be false by default"
        );
    }
}
