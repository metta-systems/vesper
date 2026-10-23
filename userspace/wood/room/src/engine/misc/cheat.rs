//! Rust port of vendor/doomgeneric/m_cheat.c.
//!
//! Cheat sequence checking. Keystrokes are fed one at a time to
//! [`cht_CheckCheat`]; it tracks progress through a fixed sequence,
//! optionally collects trailing parameter characters, and reports when a
//! cheat is fully entered. Matches the vanilla-Doom behaviour exactly,
//! including the quirk where a too-short sequence on a cheat with
//! parameters never fires.

#![allow(non_snake_case, non_camel_case_types)]

/// Maximum length of a cheat sequence including the NUL terminator,
/// matching `MAX_CHEAT_LEN` in `m_cheat.h`.
pub const MAX_CHEAT_LEN: usize = 25;

/// Maximum number of trailing parameter characters captured per cheat,
/// matching `MAX_CHEAT_PARAMS` in `m_cheat.h`.
pub const MAX_CHEAT_PARAMS: usize = 5;

/// Matches `cheatseq_t` from `m_cheat.h`.
pub struct cheatseq_t {
    /// NUL-terminated cheat sequence bytes (e.g. `"IDKFA"`).
    pub sequence: [u8; MAX_CHEAT_LEN],
    /// Declared sequence length (vanilla uses this to suppress short
    /// cheats on parameter-bearing entries; see `cht_CheckCheat`).
    pub sequence_len: usize,
    /// Number of trailing parameter characters expected after the
    /// sequence matches (e.g. 2 for `IDCLEVxx`).
    pub parameter_chars: i32,
    /// Cursor into `sequence`: number of correct keystrokes consumed.
    pub chars_read: usize,
    /// Cursor into `parameter_buf`: number of parameter chars consumed.
    pub param_chars_read: i32,
    /// Captured parameter characters; readable via `cht_GetParam`.
    pub parameter_buf: [u8; MAX_CHEAT_PARAMS],
}

/// Helper: strlen for a raw u8 array (no C call needed).
fn raw_strlen(buf: &[u8]) -> usize {
    buf.iter().position(|&c| c == 0).unwrap_or(buf.len())
}

/// `int cht_CheckCheat(cheatseq_t *cht, char key)`
///
/// Returns 1 (true) if the cheat was successfully entered, 0 otherwise.
pub fn cht_CheckCheat(cht: &mut cheatseq_t, key: u8) -> i32 {
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
///
/// `buffer` must hold at least `cht.parameter_chars` bytes.
pub fn cht_GetParam(cht: &cheatseq_t, buffer: &mut [u8]) {
    let param_chars = cht.parameter_chars as usize;
    buffer[..param_chars].copy_from_slice(&cht.parameter_buf[..param_chars]);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `cheatseq_t` from a Rust string for use in the tests below.
    /// `sequence_len` is set to the string length so the
    /// short-sequence-on-parameter-cheat guard does not fire.
    fn make_cheat(seq: &str, param_chars: i32) -> cheatseq_t {
        let mut sequence = [0u8; MAX_CHEAT_LEN];
        let bytes = seq.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            sequence[i] = b;
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
        assert_eq!(cht_CheckCheat(&mut cheat, b'I'), 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'D'), 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'K'), 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'F'), 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'A'), 1); // matched!
    }

    /// Same flow as `simple_cheat_five_chars` but with a 4-character
    /// cheat, confirming the implementation isn't length-specific.
    #[test]
    fn simple_cheat_four_chars() {
        let mut cheat = make_cheat("IDFA", 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'I'), 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'D'), 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'F'), 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'A'), 1); // matched!
    }

    /// A wrong keystroke must reset `chars_read` to 0 so the user has to
    /// restart the cheat from scratch.
    #[test]
    fn wrong_char_resets() {
        let mut cheat = make_cheat("IDFA", 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'I'), 0);
        assert_eq!(cht_CheckCheat(&mut cheat, b'X'), 0); // wrong, resets
        assert_eq!(cht_CheckCheat(&mut cheat, b'I'), 0); // restart
    }

    /// Cheats with `parameter_chars > 0` only fire once the parameter
    /// characters have also been entered.
    #[test]
    fn cheat_with_params() {
        let mut cheat = make_cheat("IDKFA", 1);
        for ch in b"IDKFA" {
            let result = cht_CheckCheat(&mut cheat, *ch);
            // The cheat has 1 param char, so after typing the sequence
            // we need to also type a param character
            if *ch == b'A' {
                assert_eq!(result, 0); // need param still
            } else {
                assert_eq!(result, 0);
            }
        }
        // Now type a param char
        assert_eq!(cht_CheckCheat(&mut cheat, b'7'), 1); // matched!
    }

    /// After a successful cheat with parameters, `cht_GetParam` must
    /// copy the captured bytes into the caller-provided buffer.
    #[test]
    fn get_param_copies_buffer() {
        let mut cheat = make_cheat("IDKFA", 1);
        for ch in b"IDKFA" {
            cht_CheckCheat(&mut cheat, *ch);
        }
        cht_CheckCheat(&mut cheat, b'7'); // completes the cheat

        let mut param_buf = [0u8; MAX_CHEAT_PARAMS];
        cht_GetParam(&cheat, &mut param_buf);
        assert_eq!(param_buf[0], b'7');
    }
}
