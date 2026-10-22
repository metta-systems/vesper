//! Doom engine key code constants.
//!
//! These match the `#define KEY_*` constants in `vendor/doomgeneric/doomkeys.h`.
//! The `to_doom_key` mapping lives in the binary's `platform/keys.rs` because
//! it depends on winit.
//!
//! Key codes use a simple encoding: printable ASCII characters map to their
//! ASCII values, while special and extended keys are encoded in the range
//! `0x80`-`0xff` using IBM PC scan-code offsets added to `0x80`.  This
//! encoding was established in the original DOS Doom and has been preserved
//! verbatim in all Chocolate Doom variants.
//!
//! Note: `doomkeys.h` defines several additional constants (`KEY_CAPSLOCK`,
//! `KEY_NUMLOCK`, `KEY_SCRLCK`, `KEY_PRTSCR`, and the full numpad `KEYP_*`
//! aliases) that are not yet needed by this port and are intentionally
//! omitted.

/// Move right / turn right.  IBM PC extended key: `0xae`.
pub const KEY_RIGHTARROW: u8 = 0xae;

/// Move left / turn left.  IBM PC extended key: `0xac`.
pub const KEY_LEFTARROW: u8 = 0xac;

/// Move forward / scroll up.  IBM PC extended key: `0xad`.
pub const KEY_UPARROW: u8 = 0xad;

/// Move backward / scroll down.  IBM PC extended key: `0xaf`.
pub const KEY_DOWNARROW: u8 = 0xaf;

/// Strafe left action key.  IBM PC extended key: `0xa0`.
pub const KEY_STRAFE_L: u8 = 0xa0;

/// Strafe right action key.  IBM PC extended key: `0xa1`.
pub const KEY_STRAFE_R: u8 = 0xa1;

/// "Use" action (open doors, activate switches).  IBM PC extended key: `0xa2`.
pub const KEY_USE: u8 = 0xa2;

/// "Fire" action (shoot).  IBM PC extended key: `0xa3`.
pub const KEY_FIRE: u8 = 0xa3;

/// Escape key.  Standard ASCII `ESC` (27 / `0x1b`).
pub const KEY_ESCAPE: u8 = 27;

/// Enter / Return key.  Standard ASCII `CR` (13 / `0x0d`).
pub const KEY_ENTER: u8 = 13;

/// Tab key.  Standard ASCII `HT` (9 / `0x09`).
pub const KEY_TAB: u8 = 9;

/// F1 function key.  Encoded as `0x80 + PC scan code 0x3b`.
pub const KEY_F1: u8 = 0x80 + 0x3b;

/// F2 function key.  Encoded as `0x80 + PC scan code 0x3c`.
pub const KEY_F2: u8 = 0x80 + 0x3c;

/// F3 function key.  Encoded as `0x80 + PC scan code 0x3d`.
pub const KEY_F3: u8 = 0x80 + 0x3d;

/// F4 function key.  Encoded as `0x80 + PC scan code 0x3e`.
pub const KEY_F4: u8 = 0x80 + 0x3e;

/// F5 function key.  Encoded as `0x80 + PC scan code 0x3f`.
pub const KEY_F5: u8 = 0x80 + 0x3f;

/// F6 function key.  Encoded as `0x80 + PC scan code 0x40`.
pub const KEY_F6: u8 = 0x80 + 0x40;

/// F7 function key.  Encoded as `0x80 + PC scan code 0x41`.
pub const KEY_F7: u8 = 0x80 + 0x41;

/// F8 function key.  Encoded as `0x80 + PC scan code 0x42`.
pub const KEY_F8: u8 = 0x80 + 0x42;

/// F9 function key.  Encoded as `0x80 + PC scan code 0x43`.
pub const KEY_F9: u8 = 0x80 + 0x43;

/// F10 function key.  Encoded as `0x80 + PC scan code 0x44`.
pub const KEY_F10: u8 = 0x80 + 0x44;

/// F11 function key.  Encoded as `0x80 + PC scan code 0x57`.
pub const KEY_F11: u8 = 0x80 + 0x57;

/// F12 function key.  Encoded as `0x80 + PC scan code 0x58`.
pub const KEY_F12: u8 = 0x80 + 0x58;

/// Backspace / delete-backward key.  ASCII `DEL` (127 / `0x7f`).
pub const KEY_BACKSPACE: u8 = 0x7f;

/// Pause key.  Encoded as `0xff` (top of the extended range).
pub const KEY_PAUSE: u8 = 0xff;

/// Equals `=` key.  Direct ASCII `b'='` (`0x3d`).
pub const KEY_EQUALS: u8 = b'=';

/// Minus `-` key.  Direct ASCII `b'-'` (`0x2d`).
pub const KEY_MINUS: u8 = b'-';

/// Right Shift key.  Encoded as `0x80 + PC scan code 0x36`.
pub const KEY_RSHIFT: u8 = 0x80 + 0x36;

/// Right Control key.  Encoded as `0x80 + PC scan code 0x1d`.
pub const KEY_RCTRL: u8 = 0x80 + 0x1d;

/// Right Alt key.  Encoded as `0x80 + PC scan code 0x38`.
pub const KEY_RALT: u8 = 0x80 + 0x38;

/// Left Alt key.  Aliased to [`KEY_RALT`] exactly as in `doomkeys.h`, because
/// the original Doom engine treated both Alt keys identically.
pub const KEY_LALT: u8 = KEY_RALT;

/// Home key.  Encoded as `0x80 + PC scan code 0x47`.
pub const KEY_HOME: u8 = 0x80 + 0x47;

/// End key.  Encoded as `0x80 + PC scan code 0x4f`.
pub const KEY_END: u8 = 0x80 + 0x4f;

/// Page Up key.  Encoded as `0x80 + PC scan code 0x49`.
pub const KEY_PGUP: u8 = 0x80 + 0x49;

/// Page Down key.  Encoded as `0x80 + PC scan code 0x51`.
pub const KEY_PGDN: u8 = 0x80 + 0x51;

/// Insert key.  Encoded as `0x80 + PC scan code 0x52`.
pub const KEY_INS: u8 = 0x80 + 0x52;

/// Delete key.  Encoded as `0x80 + PC scan code 0x53`.
pub const KEY_DEL: u8 = 0x80 + 0x53;

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that key constants match the values in vendor/doomgeneric/doomkeys.h.
    #[test]
    fn directional_keys_match_header() {
        assert_eq!(KEY_RIGHTARROW, 0xae);
        assert_eq!(KEY_LEFTARROW, 0xac);
        assert_eq!(KEY_UPARROW, 0xad);
        assert_eq!(KEY_DOWNARROW, 0xaf);
    }

    #[test]
    fn action_keys_match_header() {
        assert_eq!(KEY_USE, 0xa2);
        assert_eq!(KEY_FIRE, 0xa3);
        assert_eq!(KEY_STRAFE_L, 0xa0);
        assert_eq!(KEY_STRAFE_R, 0xa1);
    }

    #[test]
    fn control_keys_match_header() {
        assert_eq!(KEY_ESCAPE, 27);
        assert_eq!(KEY_ENTER, 13);
        assert_eq!(KEY_TAB, 9);
        assert_eq!(KEY_BACKSPACE, 0x7f);
        assert_eq!(KEY_PAUSE, 0xff);
    }

    #[test]
    fn function_keys_match_header() {
        assert_eq!(KEY_F1, 0x80 + 0x3b);
        assert_eq!(KEY_F2, 0x80 + 0x3c);
        assert_eq!(KEY_F10, 0x80 + 0x44);
        assert_eq!(KEY_F11, 0x80 + 0x57);
        assert_eq!(KEY_F12, 0x80 + 0x58);
    }

    #[test]
    fn modifier_keys_match_header() {
        assert_eq!(KEY_RSHIFT, 0x80 + 0x36);
        assert_eq!(KEY_RCTRL, 0x80 + 0x1d);
        assert_eq!(KEY_RALT, 0x80 + 0x38);
        // LALT is aliased to RALT in the original header
        assert_eq!(KEY_LALT, KEY_RALT);
    }

    #[test]
    fn navigation_keys_match_header() {
        assert_eq!(KEY_HOME, 0x80 + 0x47);
        assert_eq!(KEY_END, 0x80 + 0x4f);
        assert_eq!(KEY_PGUP, 0x80 + 0x49);
        assert_eq!(KEY_PGDN, 0x80 + 0x51);
        assert_eq!(KEY_INS, 0x80 + 0x52);
        assert_eq!(KEY_DEL, 0x80 + 0x53);
    }

    #[test]
    fn equals_and_minus_match_ascii() {
        assert_eq!(KEY_EQUALS, b'=');
        assert_eq!(KEY_MINUS, b'-');
    }
}
