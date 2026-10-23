//! Rust port of vendor/doomgeneric/m_misc.c.
//!
//! Miscellaneous helper routines used throughout the engine: file helpers,
//! string utilities (bounded copy into fixed buffers, integer parsing,
//! lump base names), and the formatting macros `c_write!` /
//! `DEH_snprintf!` / `i_error!` that replace the C variadic helpers
//! `M_snprintf` and `I_Error`.
//!
//! Text types used across the engine:
//! - constant text is `&'static str`;
//! - runtime-built text and paths are `String`;
//! - editable text lives in fixed `[u8; N]` buffers, NUL-terminated, and
//!   is read with [`until_nul`];
//! - lump names are `&[u8]`, compared over at most 8 bytes up to a NUL
//!   (see `wad::lump_name`).
//!
//! The C source supports both Windows and Unix paths; this port keeps only
//! the Unix branch (`DIR_SEPARATOR = '/'`).

#![allow(non_upper_case_globals, non_snake_case)]

pub mod argv;
pub mod cheat;
pub mod config;
pub mod controls;
pub mod menu;
pub mod random;
pub mod sha1;

use std::path::Path;

/// Unix directory separator character (C: `DIR_SEPARATOR = '/'`).
const DIR_SEPARATOR: u8 = b'/';

/// Create the directory at `path`.
///
/// Errors are ignored, so calling on an already-existing directory is a
/// no-op. Mirrors the Unix branch of the C `M_MakeDirectory`.
pub fn M_MakeDirectory(path: impl AsRef<Path>) {
    let _ = std::fs::create_dir(path);
}

/// Returns `true` if `path` names a readable file or a directory.
///
/// Matches the C original, which tries `fopen("r")` and also accepts the
/// `EISDIR` failure.
pub fn M_FileExists(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();
    std::fs::File::open(path).is_ok() || path.is_dir()
}

/// The part of a fixed text buffer before its first NUL (all of it if
/// there is none).
pub fn until_nul(buffer: &[u8]) -> &[u8] {
    buffer
        .iter()
        .position(|&byte| byte == 0)
        .map_or(buffer, |nul| &buffer[..nul])
}

/// Write `data` to the file `path`, creating or truncating it.
///
/// Returns `false` if the file could not be written in full.
pub fn M_WriteFile(path: impl AsRef<Path>, data: &[u8]) -> bool {
    std::fs::write(path, data).is_ok()
}

/// Returns the path `"/tmp/<name>"`.
///
/// Always uses `/tmp` on this port (the C original probes `TEMP` on Windows
/// or `__DJGPP__`).
pub fn M_TempFile(name: &str) -> String {
    format!("/tmp/{name}")
}

/// Parse `text` as an integer, as the C `M_StrToInt` does with `sscanf`.
///
/// Leading whitespace is skipped. Tries, in order, `0x`/`0X` hex, octal
/// after a leading `0`, then signed decimal; trailing characters after
/// the digits are ignored. Returns `None` if no form matches.
pub fn M_StrToInt(text: impl AsRef<[u8]>) -> Option<i32> {
    let text = skip_whitespace(text.as_ref());

    if let Some(hex) = text
        .strip_prefix(b"0x")
        .or_else(|| text.strip_prefix(b"0X"))
        && let Some(value) = parse_digits(hex, 16)
    {
        return Some(value);
    }
    if let Some(octal) = text.strip_prefix(b"0")
        && let Some(value) = parse_digits(octal, 8)
    {
        return Some(value);
    }
    parse_signed_decimal(text)
}

/// C `atoi`: leading whitespace, an optional sign and decimal digits, with
/// trailing characters ignored; `0` if there are no digits.
pub fn atoi(text: impl AsRef<[u8]>) -> i32 {
    parse_signed_decimal(skip_whitespace(text.as_ref())).unwrap_or(0)
}

/// `text` without its leading whitespace.
fn skip_whitespace(text: &[u8]) -> &[u8] {
    &text[text
        .iter()
        .take_while(|byte| byte.is_ascii_whitespace())
        .count()..]
}

/// Optionally signed decimal number at the start of `text`.
fn parse_signed_decimal(text: &[u8]) -> Option<i32> {
    match text.split_first() {
        Some((b'-', digits)) => parse_digits(digits, 10).map(i32::wrapping_neg),
        Some((b'+', digits)) => parse_digits(digits, 10),
        _ => parse_digits(text, 10),
    }
}

/// Value of the leading `radix` digits of `text`, or `None` if it does
/// not start with one. Overflow wraps, as C's unchecked conversions do.
fn parse_digits(text: &[u8], radix: u32) -> Option<i32> {
    let digits: Vec<u32> = text
        .iter()
        .map_while(|&byte| char::from(byte).to_digit(radix))
        .collect();
    if digits.is_empty() {
        return None;
    }
    let value = digits.iter().fold(0u32, |value, &digit| {
        value.wrapping_mul(radix).wrapping_add(digit)
    });
    Some(value as i32)
}

/// The upper-cased 8.3 base name of `path` as a NUL-padded lump name.
///
/// Takes the part after the final directory separator, up to the first
/// `.`. Names longer than 8 characters are truncated with a warning on
/// stderr.
pub fn M_ExtractFileBase(path: &[u8]) -> [u8; 8] {
    let filename = path
        .rsplit(|&byte| byte == DIR_SEPARATOR)
        .next()
        .unwrap_or(path);
    let base = filename
        .split(|&byte| byte == b'.')
        .next()
        .unwrap_or(filename);
    let mut lump_name = [0u8; 8];
    for (dest, &source) in lump_name.iter_mut().zip(base) {
        *dest = source.to_ascii_uppercase();
    }
    if base.len() > lump_name.len() {
        eprintln!(
            "Warning: Truncated '{}.????????' lump name to '{}'.",
            String::from_utf8_lossy(filename),
            String::from_utf8_lossy(&lump_name)
        );
    }
    lump_name
}

/// `strlcpy`-style copy of `src` into the fixed text buffer `dest`.
///
/// Copies `src` up to its first NUL, truncated to `dest.len() - 1` bytes,
/// and zero-fills the rest of `dest` (as `strncpy` does). Returns `true`
/// if the whole of `src` fit, `false` if it was truncated or `dest` is
/// empty.
pub fn M_StringCopy(dest: &mut [u8], src: &[u8]) -> bool {
    let Some(capacity) = dest.len().checked_sub(1) else {
        return false;
    };
    let src = until_nul(src);
    let copied = src.len().min(capacity);
    dest[..copied].copy_from_slice(&src[..copied]);
    dest[copied..].fill(0);
    copied == src.len()
}

/// Fill a `[u8; N]` buffer using Rust format syntax.
///
/// Equivalent to `M_snprintf(buf, sizeof(buf), fmt, args...)`: the text is
/// truncated to `N - 1` bytes and NUL-terminated.
#[macro_export]
macro_rules! c_write {
    ($buf:expr, $fmt:literal $(, $arg:expr)* $(,)?) => {{
        let s = ::std::format!($fmt $(, $arg)*);
        $crate::engine::misc::write_c_buf(&mut $buf, &s)
    }};
}

/// Rust replacement for the C `DEH_snprintf` helper.
///
/// Formats into a null-terminated `[u8]` buffer using Rust format syntax.
/// `DEH_String` is identity in this build; the format string is passed as a
/// Rust literal.
#[macro_export]
macro_rules! DEH_snprintf {
    ($buf:expr, $fmt:literal $(, $arg:expr)* $(,)?) => {{
        let s = ::std::format!($fmt $(, $arg)*);
        $crate::engine::misc::write_c_buf(&mut $buf, &s)
    }};
}

/// Format a message and call `I_Error`, which exits the process.
#[doc(alias = "I_Error")]
#[macro_export]
macro_rules! i_error {
    ($fmt:literal $(, $arg:expr)* $(,)?) => {
        $crate::engine::interface::system::I_Error(&::std::format!($fmt $(, $arg)*))
    };
}

/// Copy `s` into the `u8` slice `buf`, truncating and NUL-terminating.
///
/// Writes at most `buf.len() - 1` bytes from `s`, then stores `0` in the
/// next slot. If `buf` is empty, the call is a no-op (no panic, no write).
/// The `c_write!` and `DEH_snprintf!` macros format into buffers with it.
pub(crate) fn write_c_buf(buf: &mut [u8], s: &str) {
    if buf.is_empty() {
        return;
    }
    let n = s.len().min(buf.len() - 1);
    for (dst, src) in buf[..n].iter_mut().zip(s.bytes()) {
        *dst = src;
    }
    buf[n] = 0;
}

/// Unit tests for the string / file / formatting helpers in this module.
#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // M_StringCopy
    // -----------------------------------------------------------------------

    /// Copy fits within the destination: returns `true`, the rest of the
    /// buffer is zero-filled.
    #[test]
    fn test_string_copy_fits() {
        let mut dest = [0xAAu8; 10];
        assert!(M_StringCopy(&mut dest, b"Hello"));
        assert_eq!(&dest, b"Hello\0\0\0\0\0");
    }

    /// Copy is truncated to `dest.len() - 1` bytes: returns `false` and the
    /// destination is NUL-terminated.
    #[test]
    fn test_string_copy_truncation() {
        let mut dest = [0u8; 6];
        assert!(!M_StringCopy(&mut dest, b"Hello, World!"));
        assert_eq!(&dest, b"Hello\0");
    }

    /// The source is read up to its first NUL, like a C string.
    #[test]
    fn test_string_copy_stops_at_source_nul() {
        let mut dest = [0u8; 8];
        assert!(M_StringCopy(&mut dest, b"ab\0cd"));
        assert_eq!(until_nul(&dest), b"ab");
    }

    /// An empty destination cannot hold even the terminator.
    #[test]
    fn test_string_copy_empty_dest() {
        assert!(!M_StringCopy(&mut [], b"x"));
    }

    // -----------------------------------------------------------------------
    // M_StrToInt
    // -----------------------------------------------------------------------

    /// Hex, octal and decimal forms parse as C `sscanf` would.
    #[test]
    fn test_str_to_int_forms() {
        assert_eq!(M_StrToInt("0x1F"), Some(31));
        assert_eq!(M_StrToInt("  0X10"), Some(16));
        assert_eq!(M_StrToInt("017"), Some(15));
        assert_eq!(M_StrToInt("08"), Some(8));
        assert_eq!(M_StrToInt("0"), Some(0));
        assert_eq!(M_StrToInt("0x"), Some(0));
        assert_eq!(M_StrToInt("-12abc"), Some(-12));
        assert_eq!(M_StrToInt("+5"), Some(5));
        assert_eq!(M_StrToInt("abc"), None);
        assert_eq!(M_StrToInt(""), None);
    }

    /// `atoi` reads decimal only, so a leading zero is not octal.
    #[test]
    fn test_atoi() {
        assert_eq!(atoi(" 010"), 10);
        assert_eq!(atoi("-7x"), -7);
        assert_eq!(atoi("0x10"), 0);
        assert_eq!(atoi("junk"), 0);
    }

    // -----------------------------------------------------------------------
    // M_ExtractFileBase
    // -----------------------------------------------------------------------

    /// The base name is taken after the last `/`, up to the first `.`, and
    /// upper-cased into a NUL-padded 8-byte lump name.
    #[test]
    fn test_extract_file_base() {
        assert_eq!(&M_ExtractFileBase(b"/maps/e1m1.lmp"), b"E1M1\0\0\0\0");
        assert_eq!(&M_ExtractFileBase(b"demo"), b"DEMO\0\0\0\0");
        assert_eq!(&M_ExtractFileBase(b"dir/longfilename.wad"), b"LONGFILE");
    }

    // -----------------------------------------------------------------------
    // until_nul
    // -----------------------------------------------------------------------

    /// `until_nul` stops at the first NUL, or takes the whole buffer.
    #[test]
    fn test_until_nul() {
        assert_eq!(until_nul(b"ab\0cd"), b"ab");
        assert_eq!(until_nul(b"abc"), b"abc");
        assert_eq!(until_nul(b""), b"");
    }

    // -----------------------------------------------------------------------
    // write_c_buf
    // -----------------------------------------------------------------------

    /// `write_c_buf` copies the string verbatim when it fits.
    #[test]
    fn test_write_c_buf_fits() {
        let mut buf: [u8; 16] = [0; 16];
        write_c_buf(&mut buf, "hello");
        assert_eq!(until_nul(&buf), b"hello");
    }

    /// Boundary case: `s.len() == buf.len() - 1` writes the whole source
    /// and stores the NUL in the final slot.
    #[test]
    fn test_write_c_buf_exact_fit() {
        let mut buf: [u8; 6] = [0; 6];
        write_c_buf(&mut buf, "hello");
        assert_eq!(&buf, b"hello\0");
    }

    /// Truncation case: extra source bytes are dropped, NUL terminator
    /// stored at `buf[len - 1]`.
    #[test]
    fn test_write_c_buf_truncates() {
        let mut buf: [u8; 4] = [0; 4];
        write_c_buf(&mut buf, "hello");
        assert_eq!(&buf, b"hel\0");
    }

    /// Empty source writes a single NUL terminator at `buf[0]`.
    #[test]
    fn test_write_c_buf_empty_str() {
        let mut buf: [u8; 8] = [0x42; 8];
        write_c_buf(&mut buf, "");
        assert_eq!(buf[0], 0);
    }

    /// Zero-length destination: the helper exits without panicking and
    /// without writing.
    #[test]
    fn test_write_c_buf_empty_buf() {
        let mut buf: [u8; 0] = [];
        write_c_buf(&mut buf, "hello"); // must not panic
    }
}
