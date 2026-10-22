//! Rust port of vendor/doomgeneric/i_endoom.c.
//!
//! Stub for the text-mode ENDOOM screen displayed after the game quits. The
//! original chocolate-doom code initialises the textgraphics library and
//! blits 80x25 character cells from the ENDOOM lump until the user presses a
//! key. Doomgeneric (and this port) does not ship a text-mode backend, so
//! the function returns immediately without rendering anything.

#![allow(non_snake_case)]

/// Display the text-mode ENDOOM screen.
///
/// In chocolate-doom this initialises the textgraphics subsystem, copies the
/// 80x25 character/attribute cells from `endoom_data` into the text screen
/// buffer and waits for a keypress. This port has no text-mode backend, so
/// the call is a no-op and `endoom_data` is ignored.
///
/// Called from `D_DoomMain` during shutdown when the user has not requested
/// `-noendoom`.
#[no_mangle]
pub extern "C" fn I_Endoom(_endoom_data: *mut u8) {}
