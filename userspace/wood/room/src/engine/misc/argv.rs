//! Rust port of vendor/doomgeneric/m_argv.c.
//!
//! Command-line argument handling: parameter lookup and `@responsefile`
//! expansion. `myargv` is the engine's single argument vector; it is set
//! once by `doomgeneric_Create` and only rewritten by response-file
//! expansion during startup.

#![allow(non_upper_case_globals, non_snake_case)]

use {
    crate::i_error,
    std::{io::Read, ptr},
};

/// `char **myargv` — process argument vector, `myargv[0]` being the
/// program name.
pub static mut myargv: Vec<String> = Vec::new();

/// `myargc` — number of entries in [`myargv`].
pub fn argument_count() -> i32 {
    unsafe { (&*ptr::addr_of!(myargv)).len() as i32 }
}

/// `myargv[index]`.
///
/// The `'static` borrow is sound because `myargv` is only replaced during
/// startup, before any caller keeps an argument.
///
/// # Panics
/// If `index` is not below [`argument_count`].
pub fn argument(index: i32) -> &'static str {
    unsafe { &(&*ptr::addr_of!(myargv))[index as usize] }
}

/// `int M_CheckParmWithArgs(char *check, int num_args)` — search the
/// program command line for `check`, requiring at least `num_args`
/// remaining arguments after the match.
///
/// Returns the matched argument index (1..argc-num_args) or 0 if not
/// present. Comparison is case-insensitive.
pub fn M_CheckParmWithArgs(check: &str, num_args: i32) -> i32 {
    (1..argument_count() - num_args)
        .find(|&index| argument(index).eq_ignore_ascii_case(check))
        .unwrap_or(0)
}

/// `boolean M_ParmExists(char *check)` — whether `check` is present on
/// the command line.
pub fn M_ParmExists(check: &str) -> bool {
    M_CheckParm(check) != 0
}

/// `int M_CheckParm(char *check)` — convenience wrapper for
/// `M_CheckParmWithArgs(check, 0)`.
pub fn M_CheckParm(check: &str) -> i32 {
    M_CheckParmWithArgs(check, 0)
}

/// C `isspace` in the "C" locale.
fn is_c_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// Split response-file `contents` into arguments, as `LoadResponseFile` in
/// `m_argv.c` does: words are whitespace-separated, and a double-quoted run
/// is one argument.
///
/// Returns `None` if a quoted run is not closed before a newline or the
/// end of the file.
fn response_file_arguments(contents: &[u8]) -> Option<Vec<String>> {
    let mut arguments = Vec::new();
    let mut rest = contents;
    loop {
        let start = rest.iter().position(|&byte| !is_c_space(byte));
        let Some(start) = start else {
            return Some(arguments);
        };
        rest = &rest[start..];

        let (word, remainder) = if let Some(quoted) = rest.strip_prefix(b"\"") {
            let end = quoted
                .iter()
                .position(|&byte| byte == b'"' || byte == b'\n')?;
            if quoted[end] == b'\n' {
                return None;
            }
            (&quoted[..end], &quoted[end + 1..])
        } else {
            let end = rest
                .iter()
                .position(|&byte| is_c_space(byte))
                .unwrap_or(rest.len());
            (&rest[..end], rest.get(end + 1..).unwrap_or_default())
        };
        arguments.push(String::from_utf8_lossy(word).into_owned());
        rest = remainder;
    }
}

/// Load a response file (`@filename` on the command line) and splice its
/// arguments into `myargv` in place of the response-file argument.
///
/// Behaviour mirrors `LoadResponseFile` in `m_argv.c`: a missing file is
/// reported and skipped; a read failure or an unclosed quote raises an
/// `I_Error`.
fn LoadResponseFile(argv_index: i32) {
    let response_filename = &argument(argv_index)[1..];

    let Ok(mut handle) = std::fs::File::open(response_filename) else {
        print!("\nNo such response file!");
        return;
    };
    println!("Found response file {response_filename}!");

    let mut contents = Vec::new();
    if handle.read_to_end(&mut contents).is_err() {
        i_error!("Failed to read full contents of '{response_filename}'");
    }
    let Some(file_arguments) = response_file_arguments(&contents) else {
        i_error!("Quotes unclosed in response file '{response_filename}'");
    };

    unsafe {
        let arguments = &mut *ptr::addr_of_mut!(myargv);
        let argv_index = argv_index as usize;
        arguments.splice(argv_index..=argv_index, file_arguments);
    }
}

/// `void M_FindResponseFile(void)` — scan `myargv` for arguments
/// beginning with `'@'` and expand each via `LoadResponseFile`.
pub fn M_FindResponseFile() {
    let mut i = 1;
    while i < argument_count() {
        if argument(i).starts_with('@') {
            LoadResponseFile(i);
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Words split on C whitespace; a quoted run is one argument.
    #[test]
    fn response_file_splits_words_and_quotes() {
        let arguments =
            response_file_arguments(b" -file a.wad\t\"my demo.lmp\"\n-warp\x0B1 1\r\n").unwrap();
        assert_eq!(
            arguments,
            ["-file", "a.wad", "my demo.lmp", "-warp", "1", "1"]
        );
    }

    /// An empty or blank file adds no arguments.
    #[test]
    fn response_file_blank() {
        assert_eq!(response_file_arguments(b"").unwrap(), Vec::<String>::new());
        assert_eq!(
            response_file_arguments(b" \n\t").unwrap(),
            Vec::<String>::new()
        );
    }

    /// A quote left open at a newline or at the end of the file is an error.
    #[test]
    fn response_file_unclosed_quote() {
        assert!(response_file_arguments(b"\"open\nx").is_none());
        assert!(response_file_arguments(b"\"open").is_none());
    }
}
