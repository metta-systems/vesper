//! Rust port of vendor/doomgeneric/m_misc.c.
//!
//! Miscellaneous helper routines used throughout the engine: file I/O
//! wrappers, string utilities (case-insensitive search, safe copy/concat,
//! variadic join, replace), filename helpers, integer parsing, the default
//! config directory helpers (HOME / XDG), and the Rust-side formatting
//! helpers `c_write!` / `DEH_snprintf!` / `i_error!` that replace the C
//! variadic helpers `M_snprintf` and `I_Error`.
//!
//! The C source supports both Windows and Unix paths; this port keeps only
//! the Unix branch (`DIR_SEPARATOR = '/'`). Several routines call back into
//! libc directly via `extern "C"` shims rather than reimplementing them.

#![allow(non_upper_case_globals, non_snake_case)]

use std::ffi::{c_char, c_int, c_long, c_void, CStr};

use crate::i_error;
use crate::types::Boolean;

/// Opaque stand-in for libc's `FILE *`. Pointers are passed through to the
/// `extern "C"` shims below; the contents are never accessed from Rust.
pub enum FILE {}

/// Unix directory separator character (C: `DIR_SEPARATOR = '/'`).
const DIR_SEPARATOR: c_char = b'/' as c_char;
/// Unix directory separator string (C: `DIR_SEPARATOR_S = "/"`), with a
/// trailing NUL so the buffer can be passed to C string APIs.
const DIR_SEPARATOR_S: &[u8] = b"/\0";

use crate::doom::z_zone::PU_STATIC;

/// Mirror of libc's `SEEK_END` constant for use with [`fseek`].
const SEEK_END: c_int = 2;
/// Mirror of libc's `SEEK_SET` constant for use with [`fseek`].
const SEEK_SET: c_int = 0;

/// errno value reported by Linux when `fopen` fails because the path is a
/// directory. Used by [`M_FileExists`] to treat directories as existing.
const EISDIR: c_int = 21;

extern "C" {
    /// libc `fopen`: open `path` with `mode`, returns NULL on error.
    fn fopen(path: *const c_char, mode: *const c_char) -> *mut FILE;
    /// libc `fread`: read up to `nmemb * size` bytes from `stream` into `ptr`.
    fn fread(ptr: *mut c_void, size: usize, nmemb: usize, stream: *mut FILE) -> usize;
    /// libc `fwrite`: write up to `nmemb * size` bytes from `ptr` to `stream`.
    fn fwrite(ptr: *const c_void, size: usize, nmemb: usize, stream: *mut FILE) -> usize;
    /// libc `fseek`: reposition stream offset.
    fn fseek(stream: *mut FILE, offset: c_long, whence: c_int) -> c_int;
    /// libc `ftell`: return current stream offset.
    fn ftell(stream: *mut FILE) -> c_long;
    /// libc `fclose`: close the stream and flush any pending output.
    fn fclose(stream: *mut FILE) -> c_int;
    /// libc `getenv`: look up an environment variable.
    fn getenv(name: *const c_char) -> *mut c_char;
    /// libc `malloc`: allocate `size` bytes.
    fn malloc(size: usize) -> *mut c_void;
    /// libc `calloc`: allocate and zero `nmemb * size` bytes.
    fn calloc(nmemb: usize, size: usize) -> *mut c_void;
    /// libc `free`: release memory previously returned by `malloc`/`calloc`.
    fn free(ptr: *mut c_void);
    /// libc `strdup`: allocate a malloc'd copy of a null-terminated string.
    fn strdup(s: *const c_char) -> *mut c_char;
    /// libc per-thread errno location, used by [`errno`].
    fn __errno_location() -> *mut c_int;
    /// libc `mkdir`: create `path` with the given permission bits.
    fn mkdir(path: *const c_char, mode: u32) -> c_int;
    /// libc `toupper`: convert an ASCII character to upper case.
    fn toupper(c: c_int) -> c_int;
    /// libc `tolower`: convert an ASCII character to lower case.
    fn tolower(c: c_int) -> c_int;
    /// libc `strncasecmp`: case-insensitive compare of the first `n` bytes.
    fn strncasecmp(s1: *const c_char, s2: *const c_char, n: usize) -> c_int;
    /// libc `strlen`: length of a null-terminated string.
    fn strlen(s: *const c_char) -> usize;
    /// libc `strcmp`: compare two null-terminated strings.
    fn strcmp(s1: *const c_char, s2: *const c_char) -> c_int;
    /// libc `strncmp`: compare the first `n` bytes of two strings.
    fn strncmp(s1: *const c_char, s2: *const c_char, n: usize) -> c_int;
    /// libc `strncpy`: copy at most `n` bytes (without guaranteeing a NUL).
    fn strncpy(dst: *mut c_char, src: *const c_char, n: usize) -> *mut c_char;
    /// libc `strrchr`: locate the last occurrence of a byte in a string.
    fn strrchr(s: *const c_char, c: c_int) -> *mut c_char;
    /// libc `strchr`: locate the first occurrence of a byte in a string.
    fn strchr(s: *const c_char, c: c_int) -> *mut c_char;
    /// libc `strstr`: locate the first occurrence of a substring.
    fn strstr(haystack: *const c_char, needle: *const c_char) -> *mut c_char;
    /// libc `sscanf`: variadic formatted input parser.
    fn sscanf(s: *const c_char, format: *const c_char, ...) -> c_int;
    /// libc `vsnprintf`: bounded formatted output with a `va_list`.
    fn vsnprintf(s: *mut c_char, n: usize, format: *const c_char, arg: ...) -> c_int;
}

use crate::doom::z_zone::Z_Malloc;

/// Returns the current value of libc `errno` for the calling thread.
///
/// # Safety
///
/// Must be called from a context where libc has been initialised. The
/// caller must not retain the returned `int` across operations that may
/// reset `errno`.
unsafe fn errno() -> c_int {
    *__errno_location()
}

/// Create the directory at `path` with permissions `0o755`.
///
/// Wrapper around libc `mkdir`; the return value is ignored, so calling on
/// an already-existing directory is a no-op. Mirrors the Unix branch of the
/// C `M_MakeDirectory`.
#[no_mangle]
pub extern "C" fn M_MakeDirectory(path: *mut c_char) {
    unsafe {
        mkdir(path, 0o755);
    }
}

/// Returns `1` if `filename` names an existing file or directory, else `0`.
///
/// Tries `fopen("r")`; on success the stream is immediately closed. If the
/// open fails because the path is a directory (errno `EISDIR`), this still
/// reports the entry as existing, matching the C original.
#[no_mangle]
pub extern "C" fn M_FileExists(filename: *mut c_char) -> c_int {
    unsafe {
        let fstream = fopen(filename as *const c_char, c"r".as_ptr());
        if !fstream.is_null() {
            fclose(fstream);
            return 1;
        }
        (errno() == EISDIR) as c_int
    }
}

/// Returns the total length in bytes of an open file `handle`.
///
/// Saves the current position, seeks to end to read the length, then
/// restores the original position. The stream is left in its prior state.
#[no_mangle]
pub extern "C" fn M_FileLength(handle: *mut FILE) -> c_long {
    unsafe {
        let savedpos = ftell(handle);
        fseek(handle, 0, SEEK_END);
        let length = ftell(handle);
        fseek(handle, savedpos, SEEK_SET);
        length
    }
}

/// Write `length` bytes from `source` to the file named `name`.
///
/// Returns `1` on success, `0` if the file could not be opened for writing
/// or if the short-write case is hit. The file is created/truncated (`"wb"`).
#[no_mangle]
pub extern "C" fn M_WriteFile(name: *mut c_char, source: *mut c_void, length: c_int) -> c_int {
    unsafe {
        let handle = fopen(name as *const c_char, c"wb".as_ptr());
        if handle.is_null() {
            return 0;
        }
        let count = fwrite(source, 1, length as usize, handle);
        fclose(handle);
        if count < length as usize {
            return 0;
        }
        1
    }
}

/// Read the entire file `name` into a freshly allocated Z_Malloc buffer.
///
/// On success, `*buffer` is set to point at the allocated buffer and the
/// file length (in bytes) is returned. On failure (file missing or short
/// read), `I_Error` is invoked and the process exits.
#[no_mangle]
pub extern "C" fn M_ReadFile(name: *mut c_char, buffer: *mut *mut c_char) -> c_int {
    unsafe {
        let handle = fopen(name as *const c_char, c"rb".as_ptr());
        if handle.is_null() {
            i_error!(
                "Couldn't read file {}",
                std::ffi::CStr::from_ptr(name).to_string_lossy()
            );
        }
        let length = M_FileLength(handle);
        let buf = Z_Malloc(length as c_int, PU_STATIC, std::ptr::null_mut());
        let count = fread(buf, 1, length as usize, handle);
        fclose(handle);
        if count < length as usize {
            i_error!(
                "Couldn't read file {}",
                std::ffi::CStr::from_ptr(name).to_string_lossy()
            );
        }
        *buffer = buf as *mut c_char;
        length as c_int
    }
}

/// Returns a heap-allocated path of the form `"/tmp/<s>"`.
///
/// Always uses `/tmp` on this port (the C original probes `TEMP` on Windows
/// or `__DJGPP__`). The returned string is allocated by [`M_StringJoinA`]
/// and must be freed by the caller.
#[no_mangle]
pub extern "C" fn M_TempFile(s: *mut c_char) -> *mut c_char {
    let tempdir = c"/tmp".as_ptr();
    let sep = DIR_SEPARATOR_S.as_ptr() as *const c_char;
    let strs: [*const c_char; 4] = [tempdir, sep, s as *const c_char, std::ptr::null()];
    // SAFETY: null-terminated pointer array; ownership transferred to caller via return.
    M_StringJoinA(strs.as_ptr())
}

/// Parse `str` as an integer into `*result`, returning `1` on success.
///
/// Recognised forms (matching C `M_StrToInt`): `0x` / `0X` hex, leading-zero
/// octal, and plain decimal. Returns `0` if none of the formats match. The C
/// original returns a `boolean`; the Rust port keeps the `c_int` ABI.
#[no_mangle]
pub extern "C" fn M_StrToInt(str: *const c_char, result: *mut c_int) -> c_int {
    unsafe {
        if sscanf(str, c" 0x%x".as_ptr(), result) == 1 {
            return 1;
        }
        if sscanf(str, c" 0X%x".as_ptr(), result) == 1 {
            return 1;
        }
        if sscanf(str, c" 0%o".as_ptr(), result) == 1 {
            return 1;
        }
        if sscanf(str, c" %d".as_ptr(), result) == 1 {
            return 1;
        }
        0
    }
}

/// Extract the upper-cased 8.3 base name of `path` into the 8-byte `dest`.
///
/// Scans backwards from the end of `path` to find the final directory
/// separator, copies characters from that point until the next `.` or NUL,
/// up to a maximum of 8 bytes, converting each to upper case. The
/// destination is zero-padded to 8 bytes. Names longer than 8 characters
/// are truncated with a warning printed to stderr.
#[no_mangle]
pub extern "C" fn M_ExtractFileBase(path: *mut c_char, dest: *mut c_char) {
    unsafe {
        let len = strlen(path);
        let mut src = path.add(len).sub(1);
        while src != path && *src.sub(1) != DIR_SEPARATOR {
            src = src.sub(1);
        }
        let filename = src;
        let mut length: usize = 0;
        std::ptr::write_bytes(dest, 0, 8);
        while *src != 0 && *src != b'.' as c_char {
            if length >= 8 {
                let filename_cstr = CStr::from_ptr(filename);
                let dest_cstr = CStr::from_ptr(dest);
                eprintln!(
                    "Warning: Truncated '{}.????????' lump name to '{}'.",
                    filename_cstr.to_string_lossy(),
                    dest_cstr.to_string_lossy()
                );
                break;
            }
            *dest.add(length) = toupper(*src as c_int) as c_char;
            src = src.add(1);
            length += 1;
        }
    }
}

/// Convert the null-terminated string `text` to upper case in place.
///
/// Iterates byte-by-byte using libc `toupper`. The C original only
/// implements the uppercase variant; the lowercase variant below is a
/// symmetric helper added by chocolate-doom / this port.
#[no_mangle]
pub extern "C" fn M_ForceUppercase(text: *mut c_char) {
    unsafe {
        let mut p = text;
        while *p != 0 {
            *p = toupper(*p as c_int) as c_char;
            p = p.add(1);
        }
    }
}

/// Convert the null-terminated string `text` to lower case in place.
///
/// Symmetric counterpart to [`M_ForceUppercase`].
#[no_mangle]
pub extern "C" fn M_ForceLowercase(text: *mut c_char) {
    unsafe {
        let mut p = text;
        while *p != 0 {
            *p = tolower(*p as c_int) as c_char;
            p = p.add(1);
        }
    }
}

/// Case-insensitive `strstr`: find `needle` inside `haystack`.
///
/// Returns a pointer into `haystack` at the first match, or NULL if not
/// found or if `needle` is longer than `haystack`. Mirrors C `M_StrCaseStr`.
#[no_mangle]
pub extern "C" fn M_StrCaseStr(haystack: *mut c_char, needle: *mut c_char) -> *mut c_char {
    unsafe {
        let haystack_len = strlen(haystack);
        let needle_len = strlen(needle);
        if haystack_len < needle_len {
            return std::ptr::null_mut();
        }
        let len = haystack_len - needle_len;
        for i in 0..=len {
            if strncasecmp(haystack.add(i), needle, needle_len) == 0 {
                return haystack.add(i);
            }
        }
        std::ptr::null_mut()
    }
}

/// Safe `strdup` that aborts via `I_Error` if allocation fails.
///
/// Returns a malloc'd copy of `orig` that the caller must `free`.
#[no_mangle]
pub extern "C" fn M_StringDuplicate(orig: *const c_char) -> *mut c_char {
    unsafe {
        let result = strdup(orig);
        if result.is_null() {
            i_error!(
                "Failed to duplicate string (length {})\n",
                strlen(orig) as c_int
            );
        }
        result
    }
}

/// Replace every occurrence of `needle` in `haystack` with `replacement`.
///
/// Computes the final length in a first pass, allocates a single malloc'd
/// buffer, then performs the substitution in a second pass. The returned
/// pointer must be freed with `free`. On allocation failure the function
/// calls `I_Error` and returns NULL (unreachable).
#[no_mangle]
pub extern "C" fn M_StringReplace(
    haystack: *const c_char,
    needle: *const c_char,
    replacement: *const c_char,
) -> *mut c_char {
    unsafe {
        let needle_len = strlen(needle);
        let mut result_len = strlen(haystack) + 1;
        let mut p = haystack;
        loop {
            p = strstr(p, needle);
            if p.is_null() {
                break;
            }
            p = p.add(needle_len);
            result_len = result_len
                .wrapping_add(strlen(replacement))
                .wrapping_sub(needle_len);
        }
        let result = malloc(result_len) as *mut c_char;
        if result.is_null() {
            i_error!("M_StringReplace: Failed to allocate new string");
            return std::ptr::null_mut();
        }
        let mut dst = result;
        let mut dst_len = result_len;
        p = haystack;
        while *p != 0 {
            if strncmp(p, needle, needle_len) == 0 {
                M_StringCopy(dst, replacement, dst_len);
                p = p.add(needle_len);
                let rep_len = strlen(replacement);
                dst = dst.add(rep_len);
                dst_len = dst_len.wrapping_sub(rep_len);
            } else {
                *dst = *p;
                dst = dst.add(1);
                dst_len -= 1;
                p = p.add(1);
            }
        }
        *dst = 0;
        result
    }
}

/// `strlcpy`-style copy: writes at most `dest_size - 1` bytes plus a NUL.
///
/// Returns `TRUE` if the entire source string fit, `FALSE` if it was
/// truncated (or if `dest_size == 0`). Mirrors OpenBSD `strlcpy` semantics.
#[no_mangle]
pub extern "C" fn M_StringCopy(dest: *mut c_char, src: *const c_char, dest_size: usize) -> Boolean {
    unsafe {
        if dest_size >= 1 {
            *dest.add(dest_size - 1) = 0;
            strncpy(dest, src, dest_size - 1);
        } else {
            return Boolean::FALSE;
        }
        let len = strlen(dest);
        Boolean::from(*src.add(len) == 0)
    }
}

/// `strlcat`-style concat: appends `src` to `dest` without overrunning
/// `dest_size`, always leaving the destination NUL-terminated.
///
/// Returns `TRUE` if the entire source string fit, `FALSE` if it was
/// truncated. Mirrors OpenBSD `strlcat` semantics.
#[no_mangle]
pub extern "C" fn M_StringConcat(
    dest: *mut c_char,
    src: *const c_char,
    dest_size: usize,
) -> Boolean {
    unsafe {
        let mut offset = strlen(dest);
        if offset > dest_size {
            offset = dest_size;
        }
        M_StringCopy(dest.add(offset), src, dest_size - offset)
    }
}

/// Returns `TRUE` if `s` starts with `prefix`.
///
/// Note: uses strict `>` against the prefix length (matching the C source),
/// so an exact-length match returns `FALSE`. See the pinned test
/// `test_string_starts_with_exact_match_broken` for details.
#[no_mangle]
pub extern "C" fn M_StringStartsWith(s: *const c_char, prefix: *const c_char) -> Boolean {
    unsafe {
        let s_len = strlen(s);
        let prefix_len = strlen(prefix);
        Boolean::from(s_len > prefix_len && strncmp(s, prefix, prefix_len) == 0)
    }
}

/// Returns `TRUE` if `s` ends with `suffix` (exact match also returns `TRUE`).
#[no_mangle]
pub extern "C" fn M_StringEndsWith(s: *const c_char, suffix: *const c_char) -> Boolean {
    unsafe {
        let s_len = strlen(s);
        let suffix_len = strlen(suffix);
        Boolean::from(s_len >= suffix_len && strcmp(s.add(s_len - suffix_len), suffix) == 0)
    }
}

extern "C" {
    /// libc `snprintf`: variadic bounded formatted output.
    fn snprintf(s: *mut c_char, n: usize, format: *const c_char, ...) -> c_int;
}

/// Array-form replacement for the C variadic `M_StringJoin`.
///
/// Takes a pointer to a NULL-terminated array of C-string pointers and
/// concatenates them into a single freshly malloc'd string. The result
/// must be freed by the caller. Aborts via `I_Error` on allocation
/// failure. Rust callers use this in place of the C variadic API; for the
/// few sites where the C entry point is still needed, a separate shim
/// wraps it.
#[no_mangle]
pub extern "C" fn M_StringJoinA(strs: *const *const c_char) -> *mut c_char {
    unsafe {
        let mut result_len: usize = 1;
        let mut p = strs;
        while !(*p).is_null() {
            result_len += strlen(*p);
            p = p.add(1);
        }

        let result = malloc(result_len) as *mut c_char;
        if result.is_null() {
            i_error!("M_StringJoinA: Failed to allocate new string");
            return std::ptr::null_mut();
        }

        let mut dst = result;
        p = strs;
        while !(*p).is_null() {
            let src = *p;
            let len = strlen(src);
            std::ptr::copy_nonoverlapping(src, dst, len);
            dst = dst.add(len);
            p = p.add(1);
        }
        *dst = 0;
        result
    }
}

/// Clamp the return value of a previously-invoked `snprintf` and ensure the
/// destination is NUL-terminated.
///
/// `result` is the value returned by the underlying `snprintf` call. When
/// the buffer was truncated (`result < 0` or `result >= len`), the last
/// byte of `buf` is set to NUL and `len - 1` is returned; otherwise the
/// original `result` is propagated unchanged. Passing `len == 0` returns
/// `0` without writing.
///
/// This is a clamping helper only - it does NOT perform variadic
/// formatting. Callers must call `snprintf` (or the `c_write!` /
/// `DEH_snprintf!` macros) first to fill `buf`.
pub(crate) fn m_snprintf_clamp(buf: *mut c_char, len: usize, result: c_int) -> c_int {
    if len == 0 {
        return 0;
    }
    if result < 0 || result >= len as c_int {
        unsafe {
            *buf.add(len - 1) = 0;
        }
        (len as c_int) - 1
    } else {
        result
    }
}

/// C-linkage shim around `m_snprintf_clamp` for `extern "C"` callers.
#[no_mangle]
pub extern "C" fn M_snprintf_clamp(buf: *mut c_char, len: usize, result: c_int) -> c_int {
    m_snprintf_clamp(buf, len, result)
}

/// Returns `getenv("HOME")` - the user's home directory, or NULL if unset.
///
/// Note: not present in vanilla `m_misc.c` (that file's per-user paths are
/// resolved inline via `getenv` calls scattered through `M_GetSaveGameDir`
/// and friends); this port lifts the lookup into a named helper.
#[no_mangle]
pub extern "C" fn M_HomeDir() -> *const c_char {
    unsafe { getenv(c"HOME".as_ptr()) }
}

/// Returns the default configuration directory.
///
/// Resolution order:
/// 1. If `HOME` is unset, returns the static `"."`.
/// 2. If `XDG_CONFIG_HOME` is set, returns `"<XDG_CONFIG_HOME>/doom"`.
/// 3. Otherwise returns `"<HOME>/.config/doom"`.
///
/// Cases 2 and 3 return a heap-allocated path (via `M_StringJoinA` /
/// `malloc`); case 1 returns a static literal. Callers cannot distinguish
/// the two, so the returned pointer must NOT be freed.
///
/// Note: not present in vanilla `m_misc.c` (`M_SetConfigDir` in the C port
/// computes a fallback inline). This Rust helper centralises XDG-aware
/// resolution.
#[no_mangle]
pub extern "C" fn M_DefaultConfigDir() -> *const c_char {
    unsafe {
        let home = M_HomeDir();
        if home.is_null() {
            return c".".as_ptr();
        }
        let xdg = getenv(c"XDG_CONFIG_HOME".as_ptr());
        if !xdg.is_null() {
            let strs: [*const c_char; 3] = [xdg, c"/doom".as_ptr(), std::ptr::null()];
            // SAFETY: null-terminated pointer array; result intentionally leaked (see above).
            return M_StringJoinA(strs.as_ptr());
        }
        let strs: [*const c_char; 3] = [home, c"/.config/doom".as_ptr(), std::ptr::null()];
        // SAFETY: null-terminated pointer array; result intentionally leaked (see above).
        M_StringJoinA(strs.as_ptr())
    }
}

/// Stub for the Windows-only `M_OEMToUTF8` (always returns NULL on this port).
///
/// The C original lives behind `#ifdef _WIN32` and converts OEM-encoded
/// strings to UTF-8 via `MultiByteToWideChar` / `WideCharToMultiByte`. The
/// non-Windows build has no such call site; this stub exists only for ABI
/// completeness.
#[no_mangle]
pub extern "C" fn M_OEMToUTF8(_oem: *const c_char) -> *mut c_char {
    std::ptr::null_mut()
}

/// Fill a `[c_char; N]` buffer using Rust format syntax.
///
/// Equivalent to `snprintf(buf, len, fmt, args...)` followed by
/// `m_snprintf_clamp`. No persistent heap allocation.
#[macro_export]
macro_rules! c_write {
    ($buf:expr, $fmt:literal $(, $arg:expr)* $(,)?) => {{
        let ptr = ::std::ptr::addr_of_mut!($buf);
        let s = ::std::format!($fmt $(, $arg)*);
        #[allow(unused_unsafe)]
        unsafe { $crate::doom::m_misc::write_c_buf_ptr(ptr, &s) }
    }};
}

/// Rust replacement for the C `DEH_snprintf` helper.
///
/// Formats into a null-terminated `[c_char]` buffer using Rust format syntax.
/// `DEH_String` is identity in this build; the format string is passed as a
/// Rust literal instead of a `*const c_char`.
#[macro_export]
macro_rules! DEH_snprintf {
    ($buf:expr, $fmt:literal $(, $arg:expr)* $(,)?) => {{
        let ptr = ::std::ptr::addr_of_mut!($buf);
        let s = ::std::format!($fmt $(, $arg)*);
        #[allow(unused_unsafe)]
        unsafe { $crate::doom::m_misc::write_c_buf_ptr(ptr, &s) }
    }};
}

/// Format a message and call `I_Error`, which exits the process.
///
/// The `CString` is a temporary; it is valid for the duration of the call
/// because `I_Error` never returns. Panics if the formatted string contains
/// an interior null byte (game strings never embed `\0`).
#[doc(alias = "I_Error")]
#[doc(alias = "I_ErrorV")]
#[macro_export]
macro_rules! i_error {
    ($fmt:literal $(, $arg:expr)* $(,)?) => {
        $crate::doom::i_system::I_Error(
            ::std::ffi::CString::new(::std::format!($fmt $(, $arg)*))
                .unwrap()
                .as_ptr()
        )
    };
}

/// Writes a Rust `&str` into a `*mut [c_char; N]` buffer with truncation
/// and NUL termination.
///
/// Intended to be invoked by the `c_write!` and `DEH_snprintf!` macros via
/// `addr_of_mut!`, which avoids creating a `&mut` reference to a mutable
/// static. Bytes beyond the `N - 1` boundary are dropped; the final slot is
/// always set to `0`.
///
/// # Safety
///
/// `ptr` must be a valid, properly aligned pointer to an array of `N`
/// `c_char`s for the duration of the call. Aliasing rules must be observed
/// by the caller: no other `&` or `&mut` reference to the buffer may exist
/// during the call.
pub(crate) unsafe fn write_c_buf_ptr<const N: usize>(ptr: *mut [c_char; N], s: &str) {
    // SAFETY: ptr is valid for N c_chars; obtained via addr_of_mut! to avoid
    // creating a reference to a mutable static.
    let slice = unsafe { std::slice::from_raw_parts_mut(ptr.cast::<c_char>(), N) };
    write_c_buf(slice, s);
}

/// Copy `s` into the `c_char` slice `buf`, truncating and NUL-terminating.
///
/// Writes at most `buf.len() - 1` bytes from `s`, then stores `0` in the
/// next slot. If `buf` is empty, the call is a no-op (no panic, no write).
pub(crate) fn write_c_buf(buf: &mut [c_char], s: &str) {
    if buf.is_empty() {
        return;
    }
    let n = s.len().min(buf.len() - 1);
    for (dst, src) in buf[..n].iter_mut().zip(s.bytes()) {
        *dst = src as c_char;
    }
    buf[n] = 0;
}

/// Unit tests for the string / file / formatting helpers in this module.
///
/// The suite focuses on the routines whose semantics are easy to express in
/// pure-Rust fixtures - the `M_String*` family, `m_snprintf_clamp`, and
/// `write_c_buf` - plus pinned tests that document known-broken behaviour
/// inherited from the C source.
#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    // -----------------------------------------------------------------------
    // M_StringCopy
    // -----------------------------------------------------------------------

    /// Copy fits exactly within the destination - returns `TRUE` and the
    /// full source string is present.
    #[test]
    fn test_string_copy_fits() {
        let src = CString::new("Hello").unwrap();
        let mut dest: Vec<c_char> = vec![0; 10];
        let result = unsafe { M_StringCopy(dest.as_mut_ptr(), src.as_ptr(), dest.len()) };
        assert_eq!(result, Boolean::TRUE);
        let cstr = unsafe { CStr::from_ptr(dest.as_ptr()) };
        assert_eq!(cstr.to_str().unwrap(), "Hello");
    }

    /// Copy is truncated to `dest_size - 1` bytes - returns `FALSE` and
    /// the destination is NUL-terminated.
    #[test]
    fn test_string_copy_truncation() {
        let src = CString::new("Hello, World!").unwrap();
        let mut dest: Vec<c_char> = vec![0; 6];
        let result = unsafe { M_StringCopy(dest.as_mut_ptr(), src.as_ptr(), dest.len()) };
        assert_eq!(result, Boolean::FALSE);
        let cstr = unsafe { CStr::from_ptr(dest.as_ptr()) };
        assert_eq!(cstr.to_str().unwrap(), "Hello");
    }

    // -----------------------------------------------------------------------
    // M_StringConcat
    // -----------------------------------------------------------------------

    /// `M_StringConcat` succeeds when the combined string fits in the
    /// destination buffer.
    #[test]
    fn test_string_concat_fits() {
        let mut buf: Vec<c_char> = CString::new("Hello, ")
            .unwrap()
            .into_bytes_with_nul()
            .into_iter()
            .map(|b| b as c_char)
            .collect();
        buf.resize(20, 0);
        let suffix = CString::new("World!").unwrap();
        let result = unsafe { M_StringConcat(buf.as_mut_ptr(), suffix.as_ptr(), buf.len()) };
        assert_eq!(result, Boolean::TRUE);
        let cstr = unsafe { CStr::from_ptr(buf.as_ptr()) };
        assert_eq!(cstr.to_str().unwrap(), "Hello, World!");
    }

    /// `M_StringConcat` truncates and returns `FALSE` when the combined
    /// length exceeds the destination buffer.
    #[test]
    fn test_string_concat_truncation() {
        let mut buf: Vec<c_char> = CString::new("Hello, ")
            .unwrap()
            .into_bytes_with_nul()
            .into_iter()
            .map(|b| b as c_char)
            .collect();
        buf.resize(11, 0);
        let suffix = CString::new("World!").unwrap();
        let result = unsafe { M_StringConcat(buf.as_mut_ptr(), suffix.as_ptr(), buf.len()) };
        // "Hello, " (7) + "World!" (6) = 13 chars, but buffer is 11 => truncated to "Hello, Wor"
        assert_eq!(result, Boolean::FALSE);
        let cstr = unsafe { CStr::from_ptr(buf.as_ptr()) };
        assert_eq!(cstr.to_str().unwrap(), "Hello, Wor");
    }

    // -----------------------------------------------------------------------
    // M_StringStartsWith
    // -----------------------------------------------------------------------

    /// Basic prefix match: `"hello world"` begins with `"hello"`.
    #[test]
    fn test_string_starts_with_basic() {
        let s = CString::new("hello world").unwrap();
        let prefix = CString::new("hello").unwrap();
        let result = unsafe { M_StringStartsWith(s.as_ptr(), prefix.as_ptr()) };
        assert_eq!(result, Boolean::TRUE);
    }

    /// BUG: M_StringStartsWith uses `s_len > prefix_len` (strictly greater
    /// than) instead of `>=`, so an exact-length match incorrectly returns 0.
    /// The correct return value for `M_StringStartsWith("hello", "hello")`
    /// would be 1 (true).  The same bug exists in the original C source; this
    /// test pins the current behaviour so that any future fix is immediately
    /// visible as a test failure that needs updating.
    #[test]
    fn test_string_starts_with_exact_match_broken() {
        let s = CString::new("hello").unwrap();
        let prefix = CString::new("hello").unwrap();
        let result = unsafe { M_StringStartsWith(s.as_ptr(), prefix.as_ptr()) };
        // BUG: should be 1 — a string starts with itself.
        assert_eq!(result, Boolean::FALSE);
    }

    /// Negative case: `"world"` does not begin with `"hello"`.
    #[test]
    fn test_string_starts_with_no_match() {
        let s = CString::new("world").unwrap();
        let prefix = CString::new("hello").unwrap();
        let result = unsafe { M_StringStartsWith(s.as_ptr(), prefix.as_ptr()) };
        assert_eq!(result, Boolean::FALSE);
    }

    /// A prefix longer than the string itself returns `FALSE`.
    #[test]
    fn test_string_starts_with_prefix_longer_than_string() {
        let s = CString::new("hi").unwrap();
        let prefix = CString::new("hello").unwrap();
        let result = unsafe { M_StringStartsWith(s.as_ptr(), prefix.as_ptr()) };
        assert_eq!(result, Boolean::FALSE);
    }

    // -----------------------------------------------------------------------
    // M_StringEndsWith
    // -----------------------------------------------------------------------

    /// Basic suffix match: `"hello world"` ends with `"world"`.
    #[test]
    fn test_string_ends_with_basic() {
        let s = CString::new("hello world").unwrap();
        let suffix = CString::new("world").unwrap();
        let result = unsafe { M_StringEndsWith(s.as_ptr(), suffix.as_ptr()) };
        assert_eq!(result, Boolean::TRUE);
    }

    /// Exact-length suffix match returns `TRUE` (unlike the starts-with
    /// counterpart, this case is intentionally allowed by the C source).
    #[test]
    fn test_string_ends_with_exact_match() {
        let s = CString::new("hello").unwrap();
        let suffix = CString::new("hello").unwrap();
        let result = unsafe { M_StringEndsWith(s.as_ptr(), suffix.as_ptr()) };
        assert_eq!(result, Boolean::TRUE);
    }

    /// Negative case: `"hello"` does not end with `"world"`.
    #[test]
    fn test_string_ends_with_no_match() {
        let s = CString::new("hello").unwrap();
        let suffix = CString::new("world").unwrap();
        let result = unsafe { M_StringEndsWith(s.as_ptr(), suffix.as_ptr()) };
        assert_eq!(result, Boolean::FALSE);
    }

    /// A suffix longer than the string itself returns `FALSE`.
    #[test]
    fn test_string_ends_with_suffix_longer_than_string() {
        let s = CString::new("hi").unwrap();
        let suffix = CString::new("hello world").unwrap();
        let result = unsafe { M_StringEndsWith(s.as_ptr(), suffix.as_ptr()) };
        assert_eq!(result, Boolean::FALSE);
    }

    // -----------------------------------------------------------------------
    // M_StringReplace
    // -----------------------------------------------------------------------

    /// Test helper: release a malloc'd C string with libc `free`.
    ///
    /// # Safety
    ///
    /// `p` must be a non-null pointer returned by `malloc` (or
    /// `M_String*` routines that wrap it) and not previously freed.
    unsafe fn free_cstring(p: *mut c_char) {
        free(p as *mut c_void);
    }

    /// Single-occurrence replacement: `"world"` -> `"earth"` in `"hello world"`.
    #[test]
    fn test_string_replace_basic() {
        let haystack = CString::new("hello world").unwrap();
        let needle = CString::new("world").unwrap();
        let replacement = CString::new("earth").unwrap();
        let result =
            unsafe { M_StringReplace(haystack.as_ptr(), needle.as_ptr(), replacement.as_ptr()) };
        assert!(!result.is_null());
        let s = unsafe { CStr::from_ptr(result).to_str().unwrap().to_owned() };
        unsafe { free_cstring(result) };
        assert_eq!(s, "hello earth");
    }

    /// When the needle is absent, the haystack is returned verbatim
    /// in a fresh allocation.
    #[test]
    fn test_string_replace_not_found() {
        let haystack = CString::new("hello").unwrap();
        let needle = CString::new("world").unwrap();
        let replacement = CString::new("earth").unwrap();
        let result =
            unsafe { M_StringReplace(haystack.as_ptr(), needle.as_ptr(), replacement.as_ptr()) };
        assert!(!result.is_null());
        let s = unsafe { CStr::from_ptr(result).to_str().unwrap().to_owned() };
        unsafe { free_cstring(result) };
        assert_eq!(s, "hello");
    }

    /// Multiple non-overlapping occurrences are all replaced left-to-right.
    #[test]
    fn test_string_replace_multiple_occurrences() {
        let haystack = CString::new("aababab").unwrap();
        let needle = CString::new("ab").unwrap();
        let replacement = CString::new("cd").unwrap();
        let result =
            unsafe { M_StringReplace(haystack.as_ptr(), needle.as_ptr(), replacement.as_ptr()) };
        assert!(!result.is_null());
        let s = unsafe { CStr::from_ptr(result).to_str().unwrap().to_owned() };
        unsafe { free_cstring(result) };
        assert_eq!(s, "acdcdcd");
    }

    // -----------------------------------------------------------------------
    // M_StringJoinA
    // -----------------------------------------------------------------------

    /// Joining a single-element array reproduces the input string.
    #[test]
    fn test_string_join_a_single() {
        let s = CString::new("hello").unwrap();
        let strs: [*const c_char; 2] = [s.as_ptr(), std::ptr::null()];
        // SAFETY: null-terminated pointer array; result freed below with free_cstring.
        let result = unsafe { M_StringJoinA(strs.as_ptr()) };
        assert!(!result.is_null());
        let out = unsafe { CStr::from_ptr(result).to_str().unwrap().to_owned() };
        unsafe { free_cstring(result) };
        assert_eq!(out, "hello");
    }

    /// Joining multiple elements concatenates them in order with no
    /// separator inserted.
    #[test]
    fn test_string_join_a_multiple() {
        let a = CString::new("hello").unwrap();
        let b = CString::new(", ").unwrap();
        let c = CString::new("world").unwrap();
        let strs: [*const c_char; 4] = [a.as_ptr(), b.as_ptr(), c.as_ptr(), std::ptr::null()];
        // SAFETY: null-terminated pointer array; result freed below with free_cstring.
        let result = unsafe { M_StringJoinA(strs.as_ptr()) };
        assert!(!result.is_null());
        let out = unsafe { CStr::from_ptr(result).to_str().unwrap().to_owned() };
        unsafe { free_cstring(result) };
        assert_eq!(out, "hello, world");
    }

    /// An empty (NULL-only) list still returns a fresh allocation
    /// containing just the trailing NUL.
    #[test]
    fn test_string_join_a_empty_list() {
        let strs: [*const c_char; 1] = [std::ptr::null()];
        // SAFETY: null-terminated pointer array; result freed below with free_cstring.
        let result = unsafe { M_StringJoinA(strs.as_ptr()) };
        assert!(!result.is_null());
        let out = unsafe { CStr::from_ptr(result).to_str().unwrap().to_owned() };
        unsafe { free_cstring(result) };
        assert_eq!(out, "");
    }

    // -----------------------------------------------------------------------
    // M_ForceUppercase / M_ForceLowercase
    // -----------------------------------------------------------------------

    /// `M_ForceUppercase` upper-cases an ASCII mixed-case string in place.
    #[test]
    fn test_force_uppercase() {
        let mut buf: Vec<c_char> = b"Hello, World!\0".iter().map(|&b| b as c_char).collect();
        unsafe { M_ForceUppercase(buf.as_mut_ptr()) };
        let s = unsafe { CStr::from_ptr(buf.as_ptr()).to_str().unwrap() };
        assert_eq!(s, "HELLO, WORLD!");
    }

    /// `M_ForceLowercase` lower-cases an ASCII mixed-case string in place.
    #[test]
    fn test_force_lowercase() {
        let mut buf: Vec<c_char> = b"Hello, World!\0".iter().map(|&b| b as c_char).collect();
        unsafe { M_ForceLowercase(buf.as_mut_ptr()) };
        let s = unsafe { CStr::from_ptr(buf.as_ptr()).to_str().unwrap() };
        assert_eq!(s, "hello, world!");
    }

    // -----------------------------------------------------------------------
    // m_snprintf_clamp / M_snprintf_clamp
    //
    // m_snprintf_clamp is a clamping-only helper — it null-terminates and
    // clamps the return value of snprintf but does NOT perform any format-
    // string substitution itself.  Callers must invoke snprintf (or equivalent)
    // first to fill the buffer.
    // -----------------------------------------------------------------------

    /// `len == 0`: the clamp returns `0` without writing anywhere.
    #[test]
    fn test_snprintf_clamp_zero_len() {
        let mut buf = [0i8; 16];
        let r = m_snprintf_clamp(buf.as_mut_ptr(), 0, 5);
        assert_eq!(r, 0);
    }

    /// `result < len`: the underlying `snprintf` result is propagated unchanged.
    #[test]
    fn test_snprintf_clamp_no_truncation() {
        // result < len: the value is returned unchanged.
        let mut buf = [0i8; 16];
        let r = m_snprintf_clamp(buf.as_mut_ptr(), buf.len(), 5);
        assert_eq!(r, 5);
    }

    /// `result == len - 1`: no truncation occurs; the boundary case is
    /// returned verbatim.
    #[test]
    fn test_snprintf_clamp_exact_fit() {
        // result == len - 1: no truncation, value returned unchanged.
        let mut buf = [0i8; 16];
        let r = m_snprintf_clamp(buf.as_mut_ptr(), buf.len(), (buf.len() - 1) as c_int);
        assert_eq!(r, (buf.len() - 1) as c_int);
    }

    /// `result >= len`: the buffer is forced to a NUL at `buf[len - 1]`
    /// and the clamped length `len - 1` is returned.
    #[test]
    fn test_snprintf_clamp_truncation() {
        // result >= len: buffer is null-terminated at len-1, clamped value returned.
        let mut buf: Vec<i8> = b"ABCDEFGHIJKLMNOP".iter().map(|&b| b as i8).collect();
        let len = buf.len();
        let r = m_snprintf_clamp(buf.as_mut_ptr(), len, len as c_int);
        assert_eq!(r, (len - 1) as c_int);
        assert_eq!(buf[len - 1], 0);
    }

    /// Encoding error path: a negative `result` triggers the same NUL
    /// terminator + clamp as the over-length case.
    #[test]
    fn test_snprintf_clamp_error_result() {
        // result < 0 (encoding error): buffer is null-terminated at len-1, clamped.
        let mut buf = [b'X' as i8; 8];
        let len = buf.len();
        let r = m_snprintf_clamp(buf.as_mut_ptr(), len, -1);
        assert_eq!(r, (len - 1) as c_int);
        assert_eq!(buf[len - 1], 0);
    }

    /// Demonstrates that m_snprintf_clamp alone cannot substitute format
    /// arguments.  If the buffer already contains a format string like
    /// `"say %s"` and m_snprintf_clamp is called without a prior snprintf
    /// call, the format specifier is returned literally.
    ///
    /// This pins the known limitation: snprintf must always be called first
    /// to perform the actual substitution.
    #[test]
    fn test_snprintf_clamp_does_not_substitute_format_args() {
        let fmt = b"say %s\0";
        let mut buf = [0i8; 32];
        buf[..fmt.len()].copy_from_slice(unsafe {
            std::slice::from_raw_parts(fmt.as_ptr() as *const i8, fmt.len())
        });
        // Call m_snprintf_clamp without first calling snprintf.
        let r = m_snprintf_clamp(buf.as_mut_ptr(), buf.len(), (fmt.len() - 1) as c_int);
        assert_eq!(r, (fmt.len() - 1) as c_int);
        let s = unsafe { CStr::from_ptr(buf.as_ptr()).to_str().unwrap() };
        // LIMITATION: the format specifier %s is NOT substituted —
        // m_snprintf_clamp is a clamping-only helper with no variadic support
        // and cannot capture format arguments.  The raw format string is
        // returned as-is.  snprintf must always be called first.
        assert_eq!(s, "say %s");
    }

    /// Demonstrates the correct two-step pattern that DOES substitute format
    /// arguments: call snprintf first (which captures the variadic args), then
    /// m_snprintf_clamp to clamp and null-terminate the result.
    #[test]
    fn test_snprintf_then_clamp_substitutes_format_args() {
        let mut buf = [0i8; 32];
        let result = unsafe {
            libc::snprintf(
                buf.as_mut_ptr(),
                buf.len(),
                c"say %s".as_ptr(),
                c"hello".as_ptr(),
            )
        };
        let r = m_snprintf_clamp(buf.as_mut_ptr(), buf.len(), result);
        assert_eq!(r, 9); // "say hello" is 9 characters
        let s = unsafe { CStr::from_ptr(buf.as_ptr()).to_str().unwrap() };
        assert_eq!(s, "say hello");
    }

    // -----------------------------------------------------------------------
    // write_c_buf
    // -----------------------------------------------------------------------

    /// `write_c_buf` copies the string verbatim when it fits.
    #[test]
    fn test_write_c_buf_fits() {
        let mut buf: [c_char; 16] = [0; 16];
        write_c_buf(&mut buf, "hello");
        let s = unsafe { CStr::from_ptr(buf.as_ptr()).to_str().unwrap() };
        assert_eq!(s, "hello");
    }

    /// Boundary case: `s.len() == buf.len() - 1` writes the whole source
    /// and stores the NUL in the final slot.
    #[test]
    fn test_write_c_buf_exact_fit() {
        // s.len() == buf.len() - 1: no truncation, null at last position
        let mut buf: [c_char; 6] = [0; 6];
        write_c_buf(&mut buf, "hello");
        let s = unsafe { CStr::from_ptr(buf.as_ptr()).to_str().unwrap() };
        assert_eq!(s, "hello");
        assert_eq!(buf[5], 0);
    }

    /// Truncation case: extra source bytes are dropped, NUL terminator
    /// stored at `buf[len - 1]`.
    #[test]
    fn test_write_c_buf_truncates() {
        // s.len() > buf.len() - 1: truncated, null at buf[len-1]
        let mut buf: [c_char; 4] = [0; 4];
        write_c_buf(&mut buf, "hello");
        let s = unsafe { CStr::from_ptr(buf.as_ptr()).to_str().unwrap() };
        assert_eq!(s, "hel");
        assert_eq!(buf[3], 0);
    }

    /// Empty source writes a single NUL terminator at `buf[0]`.
    #[test]
    fn test_write_c_buf_empty_str() {
        let mut buf: [c_char; 8] = [0x42; 8];
        write_c_buf(&mut buf, "");
        assert_eq!(buf[0], 0);
    }

    /// Zero-length destination: the helper exits without panicking and
    /// without writing.
    #[test]
    fn test_write_c_buf_empty_buf() {
        // zero-length buffer: no panic, no write
        let mut buf: [c_char; 0] = [];
        write_c_buf(&mut buf, "hello"); // must not panic
    }
}
