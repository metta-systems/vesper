//! Rust port of vendor/doomgeneric/m_argv.c.
//!
//! Command-line argument handling: parameter lookup, `@responsefile`
//! expansion, and basename extraction. `myargc`/`myargv` are the engine's
//! single mutable argv pair.

#![allow(non_upper_case_globals, non_snake_case, clippy::upper_case_acronyms)]

use {crate::i_error, std::ffi::c_void};

/// Opaque stdio `FILE` handle used by the local `fopen`/`fread`/`fclose`
/// declarations below. Mirrors the C `FILE` typedef without exposing its
/// internals to Rust.
enum FILE {}

/// Path separator used to extract the executable basename in
/// `M_GetExecutableName`. Matches `DIR_SEPARATOR` from the C headers
/// (always `'/'` in this port; the upstream code uses `'\\'` on Windows).
const DIR_SEPARATOR: u8 = b'/';

/// Maximum number of arguments produced by a response-file expansion,
/// matching `MAXARGVS` in `m_argv.c`.
const MAXARGVS: usize = 100;

/// `int myargc` — process argument count.
pub static mut myargc: i32 = 0;

/// `char **myargv` — process argument vector.
pub static mut myargv: *mut *mut u8 = std::ptr::null_mut();

extern "C" {
    /// libc `strcasecmp` — case-insensitive string compare.
    fn strcasecmp(s1: *const u8, s2: *const u8) -> i32;
    /// libc `fopen` — open a file by path with the given mode string.
    fn fopen(path: *const u8, mode: *const u8) -> *mut FILE;
    /// libc `fread` — read up to `nmemb` elements of `size` bytes each.
    fn fread(ptr: *mut c_void, size: usize, nmemb: usize, stream: *mut FILE) -> usize;
    /// libc `fclose` — close a file handle previously opened with `fopen`.
    fn fclose(stream: *mut FILE) -> i32;
    /// libc `malloc` — uninitialised heap allocation.
    fn malloc(size: usize) -> *mut c_void;
    /// libc `memset` — fill `n` bytes at `s` with the byte value `c`.
    fn memset(s: *mut c_void, c: i32, n: usize) -> *mut c_void;
    /// libc `strrchr` — locate the last occurrence of `c` in the string `s`.
    fn strrchr(s: *const u8, c: i32) -> *mut u8;
    /// libc `isspace` — classify `c` as whitespace.
    fn isspace(c: i32) -> i32;
    /// libc `printf` — formatted output to stdout.
    fn printf(format: *const u8, ...) -> i32;
}

use crate::doom::m_misc::{M_FileLength, FILE as MiscFILE};

/// `int M_CheckParmWithArgs(char *check, int num_args)` — search the
/// program command line for `check`, requiring at least `num_args`
/// remaining arguments after the match.
///
/// Returns the matched argument index (1..argc-num_args) or 0 if not
/// present. Comparison is case-insensitive.
pub fn M_CheckParmWithArgs(check: *const u8, num_args: i32) -> i32 {
    unsafe {
        let mut i: i32 = 1;
        while i < myargc - num_args {
            if strcasecmp(check, *myargv.offset(i as isize) as *const u8) == 0 {
                return i;
            }
            i += 1;
        }
        0
    }
}

/// `boolean M_ParmExists(char *check)` — returns nonzero if `check` is
/// present on the command line, 0 otherwise.
pub fn M_ParmExists(check: *const u8) -> i32 {
    (M_CheckParm(check) != 0) as i32
}

/// `int M_CheckParm(char *check)` — convenience wrapper for
/// `M_CheckParmWithArgs(check, 0)`.
pub fn M_CheckParm(check: *const u8) -> i32 {
    M_CheckParmWithArgs(check, 0)
}

/// Load a response file (`@filename` on the command line) and splice its
/// tokens into `myargv` in place of the response-file argument.
///
/// Behaviour mirrors `LoadResponseFile` in `m_argv.c`: words are
/// whitespace-separated, double-quoted runs become a single argument, and
/// missing-file / unclosed-quote conditions raise an `I_Error`.
///
/// # Safety
/// - `argv_index` must be a valid index into the current `myargv`.
/// - `myargv[argv_index]` must be a NUL-terminated C string whose first
///   character is `'@'`; the response-file path is the remainder.
/// - All entries in `myargv` must remain readable for the duration of the
///   call.
unsafe fn LoadResponseFile(argv_index: i32) {
    let response_filename: *mut u8 = (*myargv.offset(argv_index as isize)).offset(1);

    let handle: *mut FILE = fopen(response_filename, c"rb".as_ptr().cast());
    if handle.is_null() {
        printf(c"\nNo such response file!".as_ptr().cast());
        return;
    }

    printf(
        c"Found response file %s!\n".as_ptr().cast(),
        response_filename,
    );

    let size: i64 = M_FileLength(handle as *mut MiscFILE);

    let file: *mut u8 = malloc(size as usize + 1) as *mut u8;
    let mut i: usize = 0;
    while i < size as usize {
        let k: usize = fread(file.add(i) as *mut c_void, 1, size as usize - i, handle);
        if k == 0 {
            i_error!(
                "Failed to read full contents of '{}'",
                std::ffi::CStr::from_ptr(response_filename.cast()).to_string_lossy()
            );
        }
        i += k;
    }
    fclose(handle);
    *file.add(size as usize) = 0;

    let newargv: *mut *mut u8 = malloc(std::mem::size_of::<*mut u8>() * MAXARGVS) as *mut *mut u8;
    let mut newargc: i32 = 0;
    memset(
        newargv as *mut c_void,
        0,
        std::mem::size_of::<*mut u8>() * MAXARGVS,
    );

    let mut idx: i32 = 0;
    while idx < argv_index {
        *newargv.offset(newargc as isize) = *myargv.offset(idx as isize);
        newargc += 1;
        idx += 1;
    }

    let infile: *mut u8 = file;
    let mut k: usize = 0;
    while (k as i64) < size {
        while (k as i64) < size && isspace(*infile.add(k) as i32) != 0 {
            k += 1;
        }
        if (k as i64) >= size {
            break;
        }

        if *infile.add(k) == b'"' {
            k += 1;
            *newargv.offset(newargc as isize) = infile.add(k);
            newargc += 1;
            while (k as i64) < size && *infile.add(k) != b'"' && *infile.add(k) != b'\n' {
                k += 1;
            }
            if (k as i64) >= size || *infile.add(k) == b'\n' {
                i_error!(
                    "Quotes unclosed in response file '{}'",
                    std::ffi::CStr::from_ptr(response_filename.cast()).to_string_lossy()
                );
            }
            *infile.add(k) = 0;
            k += 1;
        } else {
            *newargv.offset(newargc as isize) = infile.add(k);
            newargc += 1;
            while (k as i64) < size && isspace(*infile.add(k) as i32) == 0 {
                k += 1;
            }
            *infile.add(k) = 0;
            k += 1;
        }
    }

    idx = argv_index + 1;
    while idx < myargc {
        *newargv.offset(newargc as isize) = *myargv.offset(idx as isize);
        newargc += 1;
        idx += 1;
    }

    myargv = newargv;
    myargc = newargc;
}

/// `void M_FindResponseFile(void)` — scan `myargv` for arguments
/// beginning with `'@'` and expand each via `LoadResponseFile`.
pub fn M_FindResponseFile() {
    unsafe {
        let mut i: i32 = 1;
        while i < myargc {
            if **myargv.offset(i as isize) == b'@' {
                LoadResponseFile(i);
            }
            i += 1;
        }
    }
}

/// `char *M_GetExecutableName(void)` — return the basename portion of
/// `myargv[0]` (everything after the last `DIR_SEPARATOR`), or the whole
/// `argv[0]` if no separator is found.
pub fn M_GetExecutableName() -> *mut u8 {
    unsafe {
        let sep: *mut u8 = strrchr(*myargv as *const u8, DIR_SEPARATOR as i32);
        if sep.is_null() {
            *myargv
        } else {
            sep.offset(1)
        }
    }
}
