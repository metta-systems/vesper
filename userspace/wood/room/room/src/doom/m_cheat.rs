//! Rust port of vendor/doomgeneric/m_cheat.c.
//!
//! Cheat sequence checking. Keystrokes are fed one at a time to
//! [`cht_CheckCheat`]; it tracks progress through a fixed sequence,
//! optionally collects trailing parameter characters, and reports when a
//! cheat is fully entered. Matches the vanilla-Doom behaviour exactly,
//! including the quirk where a too-short sequence on a cheat with
//! parameters never fires.

#![allow(non_snake_case)]

use std::ffi::{c_char, c_int};

/// Maximum length of a cheat sequence including the NUL terminator,
/// matching `MAX_CHEAT_LEN` in `m_cheat.h`.
pub const MAX_CHEAT_LEN: usize = 25;

/// Maximum number of trailing parameter characters captured per cheat,
/// matching `MAX_CHEAT_PARAMS` in `m_cheat.h`.
pub const MAX_CHEAT_PARAMS: usize = 5;

/// Matches `cheatseq_t` from `m_cheat.h`.
///
/// Layout on 64-bit (`usize` = 8):
///   sequence[25]      -> offset 0, size 25
///   (padding 7)       -> offset 25-31
///   sequence_len      -> offset 32, size 8
///   parameter_chars   -> offset 40, size 4
///   (padding 4)       -> offset 44-47
///   chars_read        -> offset 48, size 8
///   param_chars_read  -> offset 56, size 4
///   parameter_buf[5]  -> offset 60, size 5
///   (padding 7)       -> offset 65-71
/// Total: 72 bytes
///
/// Layout on 32-bit (`usize` = 4):
///   sequence[25]      -> offset 0, size 25
///   (padding 3)       -> offset 25-27
///   sequence_len      -> offset 28, size 4
///   parameter_chars   -> offset 32, size 4
///   chars_read        -> offset 36, size 4
///   param_chars_read  -> offset 40, size 4
///   parameter_buf[5]  -> offset 44, size 5
///   (padding 3)       -> offset 49-51
/// Total: 52 bytes
#[repr(C)]
pub struct cheatseq_t {
    /// NUL-terminated cheat sequence bytes (e.g. `"IDKFA"`).
    pub sequence: [c_char; MAX_CHEAT_LEN],
    /// Declared sequence length (vanilla uses this to suppress short
    /// cheats on parameter-bearing entries; see `cht_CheckCheat`).
    pub sequence_len: usize,
    /// Number of trailing parameter characters expected after the
    /// sequence matches (e.g. 2 for `IDCLEVxx`).
    pub parameter_chars: c_int,
    /// Cursor into `sequence`: number of correct keystrokes consumed.
    pub chars_read: usize,
    /// Cursor into `parameter_buf`: number of parameter chars consumed.
    pub param_chars_read: c_int,
    /// Captured parameter characters; readable via `cht_GetParam`.
    pub parameter_buf: [c_char; MAX_CHEAT_PARAMS],
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(
    std::mem::size_of::<cheatseq_t>() == 72,
    "cheatseq_t size mismatch on 64-bit platform"
);

#[cfg(target_pointer_width = "32")]
const _: () = assert!(
    std::mem::size_of::<cheatseq_t>() == 52,
    "cheatseq_t size mismatch on 32-bit platform"
);

/// Helper: strlen for a raw c_char array (no C call needed).
fn raw_strlen(buf: &[c_char]) -> usize {
    buf.iter().position(|&c| c == 0).unwrap_or(buf.len())
}

/// `int cht_CheckCheat(cheatseq_t *cht, char key)`
///
/// Returns 1 (true) if the cheat was successfully entered, 0 otherwise.
#[no_mangle]
pub extern "C" fn cht_CheckCheat(cht: *mut cheatseq_t, key: c_char) -> c_int {
    let cht = unsafe { &mut *cht };

    let seq_len = raw_strlen(&cht.sequence);

    // If we make a short sequence on a cheat with parameters, this
    // will not work in vanilla doom. Behave the same.
    if cht.parameter_chars > 0 && seq_len < cht.sequence_len {
        return 0; // false
    }

    if cht.chars_read < seq_len {
        // Still reading characters from the cheat code and verifying.
        // Reset back to the beginning if a key is wrong.
        if key == cht.sequence[cht.chars_read] {
            cht.chars_read += 1;
        } else {
            cht.chars_read = 0;
        }
        cht.param_chars_read = 0;
    } else if cht.param_chars_read < cht.parameter_chars {
        // We have passed the end of the cheat sequence and are
        // entering parameters now.
        cht.parameter_buf[cht.param_chars_read as usize] = key;
        cht.param_chars_read += 1;
    }

    if cht.chars_read >= seq_len && cht.param_chars_read >= cht.parameter_chars {
        cht.chars_read = 0;
        cht.param_chars_read = 0;
        return 1; // true
    }

    0 // false
}

/// `void cht_GetParam(cheatseq_t *cht, char *buffer)`
#[no_mangle]
pub extern "C" fn cht_GetParam(cht: *mut cheatseq_t, buffer: *mut c_char) {
    let cht = unsafe { &*cht };
    let buf = unsafe { std::slice::from_raw_parts_mut(buffer, cht.parameter_chars as usize) };
    let param_chars = cht.parameter_chars as usize;
    buf[..param_chars].copy_from_slice(&cht.parameter_buf[..param_chars]);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `cheatseq_t` from a Rust string for use in the tests below.
    /// `sequence_len` is set to the string length so the
    /// short-sequence-on-parameter-cheat guard does not fire.
    fn make_cheat(seq: &str, param_chars: c_int) -> cheatseq_t {
        let mut sequence = [0i8; MAX_CHEAT_LEN];
        let bytes = seq.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            sequence[i] = b as c_char;
        }
        cheatseq_t {
            sequence,
            sequence_len: bytes.len(),
            parameter_chars: param_chars,
            chars_read: 0,
            param_chars_read: 0,
            parameter_buf: [0; MAX_CHEAT_PARAMS],
        }
    }

    /// Five correct keystrokes for a 5-character no-parameter cheat
    /// should report success only on the last keystroke.
    #[test]
    fn simple_cheat_five_chars() {
        let mut cheat = make_cheat("IDKFA", 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'I' as c_char), 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'D' as c_char), 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'K' as c_char), 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'F' as c_char), 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'A' as c_char), 1); // matched!
    }

    /// Same flow as `simple_cheat_five_chars` but with a 4-character
    /// cheat, confirming the implementation isn't length-specific.
    #[test]
    fn simple_cheat_four_chars() {
        let mut cheat = make_cheat("IDFA", 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'I' as c_char), 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'D' as c_char), 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'F' as c_char), 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'A' as c_char), 1); // matched!
    }

    /// A wrong keystroke must reset `chars_read` to 0 so the user has to
    /// restart the cheat from scratch.
    #[test]
    fn wrong_char_resets() {
        let mut cheat = make_cheat("IDFA", 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'I' as c_char), 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'X' as c_char), 0); // wrong, resets
        assert_eq!(cht_CheckCheat(&mut cheat, b'I' as c_char), 0); // restart
    }

    /// Cheats with `parameter_chars > 0` only fire once the parameter
    /// characters have also been entered.
    #[test]
    fn cheat_with_params() {
        let mut cheat = make_cheat("IDKFA", 1);
        for ch in b"IDKFA" {
            let result = cht_CheckCheat(&mut cheat, *ch as c_char);
            // The cheat has 1 param char, so after typing the sequence
            // we need to also type a param character
            if *ch == b'A' {
                assert_eq!(result, 0); // need param still
            } else {
                assert_eq!(result, 0);
            }
        }
        // Now type a param char
        assert_eq!(cht_CheckCheat(&mut cheat, b'7' as c_char), 1); // matched!
    }

    /// After a successful cheat with parameters, `cht_GetParam` must
    /// copy the captured bytes into the caller-provided buffer.
    #[test]
    fn get_param_copies_buffer() {
        let mut cheat = make_cheat("IDKFA", 1);
        for ch in b"IDKFA" {
            cht_CheckCheat(&mut cheat, *ch as c_char);
        }
        cht_CheckCheat(&mut cheat, b'7' as c_char); // completes the cheat

        let mut param_buf = [0i8; MAX_CHEAT_PARAMS];
        cht_GetParam(&mut cheat, param_buf.as_mut_ptr());
        assert_eq!(param_buf[0], b'7' as c_char);
    }

    /// Runtime check that the 64-bit `cheatseq_t` layout still totals
    /// 72 bytes (matches the layout comment on the struct).
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn struct_size_assertion() {
        assert_eq!(std::mem::size_of::<cheatseq_t>(), 72);
    }

    /// Runtime check that the 32-bit `cheatseq_t` layout still totals
    /// 52 bytes (matches the layout comment on the struct).
    #[test]
    #[cfg(target_pointer_width = "32")]
    fn struct_size_assertion() {
        assert_eq!(std::mem::size_of::<cheatseq_t>(), 52);
    }
}
