//! Mapping from winit physical key codes to Doom engine key codes.
//!
//! The Doom engine uses a small set of 8-bit key identifiers defined in
//! `doomkeys.h`.  Most printable characters map to their lower-case ASCII
//! value; special keys use values in the range `0x80–0xFF`.
//!
//! This module mirrors the mapping found in `doomgeneric_sdl.c` so that the
//! winit-based platform behaves identically to the SDL reference port.

use winit::keyboard::KeyCode;

/// Re-export every `KEY_*` constant from [`room::doom::doomkeys`] so the
/// `to_doom_key` mapping below can refer to them unqualified.
pub use room::doom::doomkeys::*;

/// Convert a winit [`KeyCode`] into the corresponding Doom key byte.
///
/// Returns `None` if the key has no Doom equivalent and should be ignored.
///
/// The mapping follows `doomgeneric_sdl.c`'s `convertToDoomKey` function,
/// extended with additional special keys supported by the winit API.
pub fn to_doom_key(code: KeyCode) -> Option<u8> {
    let key = match code {
        KeyCode::Enter => KEY_ENTER,
        KeyCode::Escape => KEY_ESCAPE,
        KeyCode::ArrowLeft => KEY_LEFTARROW,
        KeyCode::ArrowRight => KEY_RIGHTARROW,
        KeyCode::ArrowUp => KEY_UPARROW,
        KeyCode::ArrowDown => KEY_DOWNARROW,
        // Ctrl → fire
        KeyCode::ControlLeft | KeyCode::ControlRight => KEY_FIRE,
        // Space → use
        KeyCode::Space => KEY_USE,
        // Shift → run
        KeyCode::ShiftLeft | KeyCode::ShiftRight => KEY_RSHIFT,
        // Alt → strafe
        KeyCode::AltLeft | KeyCode::AltRight => KEY_LALT,
        KeyCode::F1 => KEY_F1,
        KeyCode::F2 => KEY_F2,
        KeyCode::F3 => KEY_F3,
        KeyCode::F4 => KEY_F4,
        KeyCode::F5 => KEY_F5,
        KeyCode::F6 => KEY_F6,
        KeyCode::F7 => KEY_F7,
        KeyCode::F8 => KEY_F8,
        KeyCode::F9 => KEY_F9,
        KeyCode::F10 => KEY_F10,
        KeyCode::F11 => KEY_F11,
        KeyCode::F12 => KEY_F12,
        KeyCode::Equal => KEY_EQUALS,
        KeyCode::Minus => KEY_MINUS,
        KeyCode::Backspace => KEY_BACKSPACE,
        KeyCode::Tab => KEY_TAB,
        KeyCode::Pause => KEY_PAUSE,
        // Map winit physical key codes to lower-case ASCII.
        KeyCode::KeyA => b'a',
        KeyCode::KeyB => b'b',
        KeyCode::KeyC => b'c',
        KeyCode::KeyD => b'd',
        KeyCode::KeyE => b'e',
        KeyCode::KeyF => b'f',
        KeyCode::KeyG => b'g',
        KeyCode::KeyH => b'h',
        KeyCode::KeyI => b'i',
        KeyCode::KeyJ => b'j',
        KeyCode::KeyK => b'k',
        KeyCode::KeyL => b'l',
        KeyCode::KeyM => b'm',
        KeyCode::KeyN => b'n',
        KeyCode::KeyO => b'o',
        KeyCode::KeyP => b'p',
        KeyCode::KeyQ => b'q',
        KeyCode::KeyR => b'r',
        KeyCode::KeyS => b's',
        KeyCode::KeyT => b't',
        KeyCode::KeyU => b'u',
        KeyCode::KeyV => b'v',
        KeyCode::KeyW => b'w',
        KeyCode::KeyX => b'x',
        KeyCode::KeyY => b'y',
        KeyCode::KeyZ => b'z',
        KeyCode::Digit0 => b'0',
        KeyCode::Digit1 => b'1',
        KeyCode::Digit2 => b'2',
        KeyCode::Digit3 => b'3',
        KeyCode::Digit4 => b'4',
        KeyCode::Digit5 => b'5',
        KeyCode::Digit6 => b'6',
        KeyCode::Digit7 => b'7',
        KeyCode::Digit8 => b'8',
        KeyCode::Digit9 => b'9',

        // Ignore all other keys.
        _ => return None,
    };
    Some(key)
}
