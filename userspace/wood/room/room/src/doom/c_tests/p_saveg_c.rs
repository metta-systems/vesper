//! Tests for `p_saveg.c` — save-game serialization constants and globals.
//!
//! `p_saveg.c` serializes and deserializes the full game state to/from a save
//! file.  The `SAVEGAME_EOF` sentinel byte and `VERSIONSIZE` string length are
//! embedded in every save file on disk; changing them would break save
//! compatibility.  The `savegamelength` and `savegame_error` globals track the
//! current serialization progress and error state.
//!
//! These tests verify:
//!   * `SAVEGAME_EOF` and `VERSIONSIZE` constant values
//!   * `savegamelength` is zero before any save operation
//!   * `savegame_error` is false before any save operation

#![allow(non_snake_case)]

use crate::doom::c_ffi;

// ---------------------------------------------------------------------------
// SAVEGAME_EOF constant
// ---------------------------------------------------------------------------

/// `SAVEGAME_EOF = 0x1d` is the end-of-file marker byte appended to every
/// save game.  `P_ReadSaveGameEOF` checks for this exact byte; any change
/// would silently corrupt all existing save files.
#[test]
fn savegame_eof_is_0x1d() {
    assert_eq!(c_ffi::SAVEGAME_EOF, 0x1d);
    assert_eq!(c_ffi::SAVEGAME_EOF, 29);
}

// ---------------------------------------------------------------------------
// VERSIONSIZE constant
// ---------------------------------------------------------------------------

/// `VERSIONSIZE = 16` bytes: the version string embedded at the start of
/// every save-game header.  Changing it would break save-file parsing.
#[test]
fn versionsize_is_16() {
    assert_eq!(c_ffi::VERSIONSIZE, 16);
}

/// `VERSIONSIZE` is a power of two, which is typical for fixed-length strings.
#[test]
fn versionsize_is_power_of_two() {
    let n = c_ffi::VERSIONSIZE;
    assert!(
        n > 0 && (n & (n - 1)) == 0,
        "VERSIONSIZE={n} should be a power of two"
    );
}

// ---------------------------------------------------------------------------
// savegamelength — byte counter for the current save stream
// ---------------------------------------------------------------------------

/// `savegamelength` counts bytes written to the current save-game buffer;
/// it is zero before any save operation has been started.
#[test]
fn savegamelength_default_zero() {
    unsafe {
        assert_eq!(
            c_ffi::savegamelength,
            0,
            "savegamelength should be 0 before any P_SaveGame call"
        );
    }
}

// ---------------------------------------------------------------------------
// savegame_error — write-error flag
// ---------------------------------------------------------------------------

/// `savegame_error` is set true if a write to the save buffer fails; it must
/// be false (0) at program start before any save operation.
#[test]
fn savegame_error_default_false() {
    unsafe {
        assert_eq!(
            c_ffi::savegame_error,
            0,
            "savegame_error should be false before any save operation"
        );
    }
}

// ---------------------------------------------------------------------------
// Type-width checks
// ---------------------------------------------------------------------------

/// `savegamelength` and `savegame_error` are C `int` (4 bytes).
#[test]
fn saveg_globals_are_c_int_width() {
    use std::ffi::c_int;
    const _: () = assert!(std::mem::size_of::<c_int>() == 4);
    unsafe {
        let _: c_int = c_ffi::savegamelength;
        let _: c_int = c_ffi::savegame_error;
    }
}
