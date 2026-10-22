//! Rust port of vendor/doomgeneric/i_input.c.
//!
//! Keyboard input handling: reads keys from the doomgeneric platform layer
//! (`DG_GetKey`) and posts them as Doom `event_t` events through `D_PostEvent`.
//! The platform layer already returns Doom key codes, so the AT-scancode
//! translation table from the original C source is omitted; `TranslateKey`
//! is effectively an identity function, matching the active behaviour in
//! `i_input.c` (the lookup table there is dead code behind a comment block).

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::os::raw::c_int;

use crate::doom::{d_event::event_t, doomkeys::KEY_RSHIFT};

/// Configuration flag mirroring the C global `vanilla_keyboard_mapping`.
///
/// When non-zero, classic Doom keyboard mapping is used. Exported with C
/// linkage so the rest of the (still C) code base can read and write it.
pub static mut vanilla_keyboard_mapping: c_int = 1;

/// Net count of shift presses minus shift releases.
///
/// Treated as boolean-ish: `> 0` means a shift key is currently held. Using
/// a counter rather than a boolean tolerates lost key-up events without
/// permanently latching the shift state, matching the C source.
static mut shiftdown: c_int = 0;

use crate::doom::d_event::D_PostEvent;

/// US-layout shift transform table.
///
/// Maps an unshifted ASCII byte (0..=127) to the character produced when the
/// shift key is held. Mirrors `shiftxform[]` in `i_input.c` verbatim,
/// including the well-known Watcom quirk that maps shift-backslash to `'!'`.
static SHIFTXFORM: [u8; 128] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
    26, 27, 28, 29, 30, 31, b' ', b'!', b'"', b'#', b'$', b'%', b'&', b'"', // shift-'
    b'(', b')', b'*', b'+', b'<', // shift-,
    b'_', // shift--
    b'>', // shift-.
    b'?', // shift-/
    b')', // shift-0
    b'!', // shift-1
    b'@', // shift-2
    b'#', // shift-3
    b'$', // shift-4
    b'%', // shift-5
    b'^', // shift-6
    b'&', // shift-7
    b'*', // shift-8
    b'(', // shift-9
    b':', b':', // shift-;
    b'<', b'+', // shift-=
    b'>', b'?', b'@', b'A', b'B', b'C', b'D', b'E', b'F', b'G', b'H', b'I', b'J', b'K', b'L', b'M',
    b'N', b'O', b'P', b'Q', b'R', b'S', b'T', b'U', b'V', b'W', b'X', b'Y', b'Z',
    b'[', // shift-[
    b'!', // shift-backslash
    b']', // shift-]
    b'"', b'_', b'\'', // shift-`
    b'A', b'B', b'C', b'D', b'E', b'F', b'G', b'H', b'I', b'J', b'K', b'L', b'M', b'N', b'O', b'P',
    b'Q', b'R', b'S', b'T', b'U', b'V', b'W', b'X', b'Y', b'Z', b'{', b'|', b'}', b'~', 127,
];

/// Translates a raw platform key code to a Doom key code.
///
/// Identity function: `DG_GetKey` already returns Doom key codes, so no
/// remapping is needed. Preserved as a named call so the structure mirrors
/// the C source, where the body is also `return key;` followed by a commented
/// AT-to-Doom lookup.
fn TranslateKey(key: u8) -> u8 {
    // The platform layer (DG_GetKey) already returns Doom key codes,
    // so this is an identity function (matching the active code in
    // the original C source).
    key
}

/// Returns the printable character produced by a key press, applying the
/// shift transform if shift is currently held.
///
/// Out-of-range key codes (`>= SHIFTXFORM.len()`) collapse to 0 when shift is
/// held, matching the C `arrlen(shiftxform)` guard. Reads the `shiftdown`
/// static, hence the `unsafe` block.
fn GetTypedChar(key: u8) -> u8 {
    let mut key = TranslateKey(key);
    unsafe {
        if shiftdown > 0 {
            if (key as usize) < SHIFTXFORM.len() {
                key = SHIFTXFORM[key as usize];
            } else {
                key = 0;
            }
        }
    }
    key
}

/// Updates `shiftdown` for a shift key event.
///
/// Increments on press, decrements on release. Only `KEY_RSHIFT` is tracked,
/// matching the C source. Mutates the `shiftdown` static.
fn UpdateShiftStatus(pressed: c_int, key: u8) {
    let change = if pressed != 0 { 1 } else { -1 };
    if key == KEY_RSHIFT {
        unsafe {
            shiftdown += change;
        }
    }
}

/// One-time input subsystem initialisation hook.
///
/// No-op in the doomgeneric platform layer (all input setup happens in the
/// platform's `init` / `key`). `i_video` still calls it.
pub fn I_InitInput() {}

/// Pumps the platform key queue, posting one `ev_keydown` event per pressed
/// key and a single `ev_keyup` event before returning.
///
/// Mirrors the C control flow exactly: keydown events are posted in a loop
/// (so multiple keys pressed in the same frame all flow through), but the
/// first keyup event ends the call. Suppresses events whose `data1` is 0.
/// `d_main` calls it from the main loop.
pub fn I_GetEvent() {
    while let Some((is_pressed, key)) = super::doomgeneric::platform().key() {
        let pressed = c_int::from(is_pressed);
        UpdateShiftStatus(pressed, key);

        let mut event = event_t {
            type_: 0,
            data1: 0,
            data2: 0,
            data3: 0,
            data4: 0,
        };

        if pressed != 0 {
            event.type_ = 0; // ev_keydown
            event.data1 = TranslateKey(key) as c_int;
            event.data2 = GetTypedChar(key) as c_int;
            if event.data1 != 0 {
                D_PostEvent(&event);
            }
        } else {
            event.type_ = 1; // ev_keyup
            event.data1 = TranslateKey(key) as c_int;
            event.data2 = 0;
            if event.data1 != 0 {
                D_PostEvent(&event);
            }
            break;
        }
    }
}
