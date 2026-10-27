//! The host's files, exit and error dialog, behind [`Host`](super::Host)'s
//! file, `error_box` and `quit` methods.
//!
//! Files are `std::fs` files at the engine's paths, relative to the
//! current directory. An open WAD is a [`File`] in [`OPEN_FILES`], named by
//! its index there.

use std::{
    fs::File,
    io::{ErrorKind, Read, Seek, SeekFrom, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{Mutex, PoisonError},
};

use room::engine::platform::FileHandle;

/// The open files, indexed by [`FileHandle`]; a closed file's entry is
/// `None` until `open` reuses it.
static OPEN_FILES: Mutex<Vec<Option<File>>> = Mutex::new(Vec::new());

/// Run `action` on the open file `file`; `None` if it is not open.
fn with_open_file<R>(file: FileHandle, action: impl FnOnce(&mut File) -> R) -> Option<R> {
    let mut open_files = OPEN_FILES.lock().unwrap_or_else(PoisonError::into_inner);
    open_files
        .get_mut(file.0)
        .and_then(Option::as_mut)
        .map(action)
}

/// Open the file at `path` for reading.
pub fn open(path: &str) -> Option<FileHandle> {
    let file = File::open(path).ok()?;
    let mut open_files = OPEN_FILES.lock().unwrap_or_else(PoisonError::into_inner);
    let index = match open_files.iter().position(Option::is_none) {
        Some(free) => {
            open_files[free] = Some(file);
            free
        }
        None => {
            open_files.push(Some(file));
            open_files.len() - 1
        }
    };
    Some(FileHandle(index))
}

/// The size of the open file `file`, `0` if unknown.
pub fn len(file: FileHandle) -> u64 {
    with_open_file(file, |file| {
        file.metadata().map_or(0, |metadata| metadata.len())
    })
    .unwrap_or(0)
}

/// Read into `buffer` from `offset` of `file` until it is full, the file
/// ends or a read fails; the number of bytes read.
pub fn read_at(file: FileHandle, offset: u64, buffer: &mut [u8]) -> usize {
    with_open_file(file, |file| {
        if file.seek(SeekFrom::Start(offset)).is_err() {
            return 0;
        }
        let mut transferred = 0;
        while transferred < buffer.len() {
            match file.read(&mut buffer[transferred..]) {
                Ok(0) => break,
                Ok(count) => transferred += count,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        transferred
    })
    .unwrap_or(0)
}

/// Close `file`.
pub fn close(file: FileHandle) {
    let mut open_files = OPEN_FILES.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(entry) = open_files.get_mut(file.0) {
        *entry = None;
    }
}

/// `true` if `path` names a readable file or a directory, as the C
/// `M_FileExists` tried `fopen("r")` and also accepted `EISDIR`.
pub fn exists(path: &str) -> bool {
    File::open(path).is_ok() || Path::new(path).is_dir()
}

/// Flush the console and the log, then exit with status `code`.
pub fn quit(code: i32) -> ! {
    // C `exit` flushes stdio; `process::exit` skips Rust's stdout buffer.
    let _ = std::io::stdout().flush();
    log::logger().flush();
    std::process::exit(code);
}

/// Probe whether the Zenity GUI dialog binary is available on this
/// system by invoking `/usr/bin/zenity --help`.
fn zenity_available() -> bool {
    Command::new("/usr/bin/zenity")
        .arg("--help")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Display `message` in a native error dialog using Zenity, if it is
/// installed (C `ZenityErrorBox`).
///
/// Unlike the C source's `EscapeShellString` + `system()` approach, the
/// message is passed as an argv element to [`Command`], so no
/// shell-escaping is required.
pub fn error_box(message: &str) {
    if !zenity_available() {
        return;
    }
    let _ = Command::new("/usr/bin/zenity")
        .arg("--error")
        .arg("--text")
        .arg(message)
        .status();
}
