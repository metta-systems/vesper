//! Rust port of vendor/doomgeneric/i_system.c.
//!
//! System interface: zone memory allocation, error handling, exit hooks,
//! startup banners, and DOS memory-dump emulation for compatibility.
//!
//! The entry points keep the original C names. The Rust implementations use libc for
//! the original printf/malloc behaviour and `std::process` for the
//! Linux-only Zenity error dialog (the C source has Win32 / macOS /
//! DJGPP variants which this port intentionally drops).

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use {
    crate::engine::misc::{
        argv::{argument, argument_count, M_CheckParmWithArgs, M_ParmExists},
        atoi, M_StrToInt,
    },
    std::{ffi::c_void, ptr},
};

use crate::{i_error, types::Boolean};

/// Default size of the zone heap, in MiB, when `-mb` is not supplied.
/// Mirrors the `DEFAULT_RAM` macro in `i_system.c`.
const DEFAULT_RAM: i32 = 6; // MiB

/// Minimum size of the zone heap, in MiB. `AutoAllocMemory` keeps
/// halving the request until it succeeds or drops below this floor.
/// Mirrors the `MIN_RAM` macro in `i_system.c`.
const MIN_RAM: i32 = 6; // MiB

/// C signature for an exit callback registered via `I_AtExit`.
/// Matches `typedef void (*atexit_func_t)(void)` from `i_system.h`.
type atexit_func_t = fn();

/// Single entry in the linked list of registered exit callbacks.
///
/// Fields mirror `struct atexit_listentry_s`.
struct atexit_listentry_t {
    /// Callback to invoke when the engine exits or errors out.
    func: atexit_func_t,
    /// If truthy, the callback is invoked from `I_Error` too, not
    /// only from `I_Quit`.
    run_on_error: Boolean,
    /// Next entry in the singly-linked list (most-recently registered
    /// first). Null terminates the list.
    next: *mut atexit_listentry_t,
}

/// Head of the singly-linked list of registered exit callbacks.
/// Mirrors the file-scope `exit_funcs` variable in `i_system.c`.
/// Mutated only by `I_AtExit`, read by `I_Quit` and `I_Error`.
static mut exit_funcs: *mut atexit_listentry_t = ptr::null_mut();

/// Allocate zone memory, shrinking the requested size by 1 MiB at a
/// time until `malloc` succeeds or the size drops below `min_ram`.
///
/// Writes the chosen size in bytes through `size` and returns a
/// pointer to the freshly allocated block. If even `min_ram` cannot
/// be allocated, the function calls `I_Error` (which never returns).
///
/// # Safety
///
/// `size` must be a valid mutable pointer to a `i32`. The returned block
/// is never freed: the zone lives for the whole run. Mirrors the static
/// helper of the same name in `i_system.c`.
unsafe fn AutoAllocMemory(size: *mut i32, mut default_ram: i32, min_ram: i32) -> *mut u8 {
    let mut zonemem: *mut u8 = ptr::null_mut();

    while zonemem.is_null() {
        if default_ram < min_ram {
            i_error!("Unable to allocate {} MiB of RAM for zone", default_ram);
        }

        *size = default_ram * 1024 * 1024;
        // `alloc` returns null on failure, as `malloc` did, so a smaller
        // size can be tried; 16 is malloc's alignment.
        let layout = std::alloc::Layout::from_size_align(*size as usize, 16).unwrap();
        zonemem = std::alloc::alloc(layout);

        if zonemem.is_null() {
            default_ram -= 1;
        }
    }

    zonemem
}

/// Register `func` as an exit callback. If `run_on_error` is truthy,
/// the callback also fires from `I_Error`, not just `I_Quit`.
///
/// Entries form a stack (LIFO): the most recently registered callback
/// runs first. Mirrors `I_AtExit` from `i_system.c`. List nodes are
/// never freed, as in C.
pub fn I_AtExit(func: atexit_func_t, run_on_error: Boolean) {
    unsafe {
        exit_funcs = Box::into_raw(Box::new(atexit_listentry_t {
            func,
            run_on_error,
            next: exit_funcs,
        }));
    }
}

/// Tactile feedback hook. Originally targeted the Logitech Cyberman in
/// the DOS Doom source; a no-op stub here, as in the C reference.
pub fn I_Tactile(_on: i32, _off: i32, _total: i32) {}

/// Allocate the engine's zone heap and return a pointer to its base.
///
/// Honours the `-mb <MiB>` command-line argument; otherwise uses
/// `DEFAULT_RAM` MiB and shrinks via `AutoAllocMemory`. The chosen
/// size in bytes is written through `size`.
///
/// Mirrors `I_ZoneBase` from `i_system.c`. Called once at startup from
/// `Z_Init` in `z_zone.c`.
pub fn I_ZoneBase(size: *mut i32) -> *mut u8 {
    unsafe {
        let mut default_ram = DEFAULT_RAM;
        let mut min_ram = MIN_RAM;

        let p = M_CheckParmWithArgs("-mb", 1);
        if p > 0 {
            default_ram = atoi(argument(p + 1));
            min_ram = default_ram;
        }

        let zonemem = AutoAllocMemory(size, default_ram, min_ram);

        println!("zone memory: {zonemem:p}, {:x} allocated for zone", *size);

        zonemem
    }
}

/// Print a banner string centred in a 70-character column (35 leading
/// spaces minus half the message length), followed by a newline.
/// Mirrors `I_PrintBanner` from `i_system.c`.
pub fn I_PrintBanner(msg: &str) {
    let spaces = (35 - msg.len() as i32 / 2).max(0) as usize;
    println!("{:spaces$}{msg}", "");
}

/// Print a 75-character horizontal divider made of `=` followed by a
/// newline. Mirrors `I_PrintDivider` from `i_system.c`. Used by the
/// startup banner and a few menu screens.
pub fn I_PrintDivider() {
    println!("{}", "=".repeat(75));
}

/// The licence notice printed by [`I_PrintStartupBanner`].
const STARTUP_NOTICE: &str =
    " Room, like Doom Generic, is free software, covered by the GNU General Public
 License.  There is NO warranty; not even for MERCHANTABILITY or FITNESS
 FOR A PARTICULAR PURPOSE. You are welcome to change and distribute
 copies under certain conditions. See the source for more information.
";

/// Print the startup banner: a divider, the game description centred
/// between dividers, and the GPL copyright notice. Mirrors
/// `I_PrintStartupBanner` from `i_system.c`, but substitutes the
/// project name (the C source pulls it from `PACKAGE_NAME`).
pub fn I_PrintStartupBanner(gamedescription: &str) {
    I_PrintDivider();
    I_PrintBanner(gamedescription);
    I_PrintDivider();
    print!("{STARTUP_NOTICE}");
    I_PrintDivider();
}

/// Return non-zero if stdout is a real interactive console.
///
/// The port always returns 0, matching the non-`ORIGCODE` branch in
/// `i_system.c`: without that flag the C source also returns 0. The
/// result is used by `I_Error` to decide whether to pop up a GUI
/// dialog when no console is available to display the message.
pub fn I_ConsoleStdout() -> i32 {
    0
}

/// Walk the `exit_funcs` list and invoke every registered callback.
///
/// Unlike the C version this does **not** call `SDL_Quit` or
/// `std::process::exit`: the C source only does so when `ORIGCODE` is
/// defined. The caller is expected to terminate after `I_Quit`
/// returns. Mirrors `I_Quit` from `i_system.c`.
pub fn I_Quit() {
    unsafe {
        let mut entry = exit_funcs;
        while !entry.is_null() {
            ((*entry).func)();
            entry = (*entry).next;
        }
    }
}

/// Probe whether the Zenity GUI dialog binary is available on this
/// system by invoking `/usr/bin/zenity --help`.
///
/// # Safety
///
/// Marked `unsafe` only for symmetry with `ZenityErrorBox`; the body
/// itself uses safe `std::process` APIs. Always safe to call.
unsafe fn ZenityAvailable() -> bool {
    std::process::Command::new("/usr/bin/zenity")
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Display `message` in a native error dialog using Zenity. Returns 1
/// if Zenity launched successfully and exited cleanly, 0 otherwise.
///
/// Unlike the C source's `EscapeShellString` + `system()` approach,
/// this implementation passes the message as an argv element to
/// `std::process::Command`, so no shell-escaping is required.
///
/// # Safety
///
/// `message` must point to a valid NUL-terminated C string.
unsafe fn ZenityErrorBox(message: &str) -> i32 {
    if !ZenityAvailable() {
        return 0;
    }

    let status = std::process::Command::new("/usr/bin/zenity")
        .arg("--error")
        .arg("--text")
        .arg(message)
        .status();

    match status {
        Ok(s) => {
            if s.success() {
                1
            } else {
                0
            }
        }
        Err(_) => 0,
    }
}

/// Fatal error handler: prints `msg` to stderr, runs all
/// `run_on_error` exit callbacks, optionally pops up a GUI dialog,
/// then terminates with exit code -1. Never returns.
///
/// Recursive calls are detected (via the function-local
/// `already_quitting` static) and trigger only a warning print before
/// continuing; this matches the safety net in `I_Error` from
/// `i_system.c`. Unlike the C source, which takes a printf-style
/// format string and varargs, this entry point accepts a single
/// pre-formatted message - the `i_error!` macro takes care of
/// formatting on the caller side.
pub fn I_Error(msg: &str) -> ! {
    unsafe {
        static mut already_quitting: bool = false;

        if already_quitting {
            eprintln!("Warning: recursive call to I_Error detected.");
        } else {
            already_quitting = true;
        }

        eprintln!("{msg}");
        eprintln!();

        let mut entry = exit_funcs;
        while !entry.is_null() {
            if (*entry).run_on_error.is_truthy() {
                ((*entry).func)();
            }
            entry = (*entry).next;
        }

        let exit_gui_popup = !M_ParmExists("-nogui");
        if exit_gui_popup && I_ConsoleStdout() == 0 {
            ZenityErrorBox(msg);
        }

        // C `exit` flushes stdio; `process::exit` skips Rust's stdout buffer.
        let _ = std::io::Write::flush(&mut std::io::stdout());
        std::process::exit(-1);
    }
}

/// Length, in bytes, of the simulated low-memory dump exposed via
/// `I_GetMemoryValue`. Mirrors the `DOS_MEM_DUMP_SIZE` macro.
const DOS_MEM_DUMP_SIZE: usize = 10;

/// Snapshot of the first 10 bytes at DOS 6.22 segment 0:0, used to
/// emulate the read access violation hack from PrBoom+ for demo
/// playback compatibility. Mirrors `mem_dump_dos622` in `i_system.c`.
static MEM_DUMP_DOS622: [u8; DOS_MEM_DUMP_SIZE] =
    [0x57, 0x92, 0x19, 0x00, 0xF4, 0x06, 0x70, 0x00, 0x16, 0x00];

/// Snapshot of the first 10 bytes at DOS 7.1 / Windows 98 segment 0:0
/// for the same NULL-pointer-dereference emulation. Mirrors
/// `mem_dump_win98` in `i_system.c`.
static MEM_DUMP_WIN98: [u8; DOS_MEM_DUMP_SIZE] =
    [0x9E, 0x0F, 0xC9, 0x00, 0x65, 0x04, 0x70, 0x00, 0x16, 0x00];

/// Snapshot of the first 10 bytes seen when running DOSBox under
/// Windows XP. Mirrors `mem_dump_dosbox` in `i_system.c`.
static MEM_DUMP_DOSBOX: [u8; DOS_MEM_DUMP_SIZE] =
    [0x00, 0x00, 0x00, 0xF1, 0x00, 0x00, 0x00, 0x00, 0x07, 0x00];

/// User-supplied custom memory dump, populated from `-setmem <bytes>`
/// on the command line. Initially zeroed. Mirrors `mem_dump_custom`.
static mut MEM_DUMP_CUSTOM: [u8; DOS_MEM_DUMP_SIZE] = [0; DOS_MEM_DUMP_SIZE];

/// Which of the four built-in memory dumps `I_GetMemoryValue` returns.
/// Selected from the `-setmem dos622|dos71|dosbox|<bytes...>` argument
/// on first invocation. In the C source this is a `const unsigned
/// char *` rebinding rather than an enum.
#[derive(Clone, Copy)]
enum DosMemDump {
    Dos622,
    Win98,
    Dosbox,
    Custom,
}

/// Currently selected DOS memory dump. Mirrors the file-scope
/// `dos_mem_dump` pointer in `i_system.c`, which is initialized to
/// `mem_dump_dos622`.
static mut dos_mem_dump: DosMemDump = DosMemDump::Dos622;

/// PrBoom+ read-access-violation emulator: returns the byte/word/dword
/// at `offset` in the currently selected DOS memory dump. Writes the
/// value through `value` and returns 1 on success, 0 on out-of-range
/// access or unsupported `size`.
///
/// On the first invocation the function parses `-setmem` from the
/// command line to choose between `dos622`, `dos71` (treated as
/// Win98), `dosbox`, or an explicit byte sequence. The byte sequence
/// is parsed via `M_StrToInt` and copied into `MEM_DUMP_CUSTOM`.
///
/// Mirrors `I_GetMemoryValue` from `i_system.c`. The Rust port uses
/// an `else if` chain for the dos622/dos71/dosbox cases, which avoids
/// a latent bug in the C source (missing `else` after the `dos622`
/// branch lets it fall through to `dos71`). The custom-byte loop also
/// uses a single index, fixing a double-increment bug in C.
pub fn I_GetMemoryValue(offset: u32, value: *mut c_void, size: i32) -> i32 {
    unsafe {
        static mut firsttime: bool = true;

        if firsttime {
            firsttime = false;

            let p = M_CheckParmWithArgs("-setmem", 1);
            if p > 0 {
                let arg = argument(p + 1);
                if arg.eq_ignore_ascii_case("dos622") {
                    dos_mem_dump = DosMemDump::Dos622;
                } else if arg.eq_ignore_ascii_case("dos71") {
                    dos_mem_dump = DosMemDump::Win98;
                } else if arg.eq_ignore_ascii_case("dosbox") {
                    dos_mem_dump = DosMemDump::Dosbox;
                } else {
                    let mut idx: usize = 0;
                    let mut pp = p + 1;
                    while idx < DOS_MEM_DUMP_SIZE {
                        pp += 1;
                        if pp >= argument_count() || argument(pp).starts_with('-') {
                            break;
                        }
                        let val = M_StrToInt(argument(pp)).unwrap_or(0);
                        MEM_DUMP_CUSTOM[idx] = val as u8;
                        idx += 1;
                    }
                    dos_mem_dump = DosMemDump::Custom;
                }
            }
        }

        let dump: *const u8 = match dos_mem_dump {
            DosMemDump::Dos622 => MEM_DUMP_DOS622.as_ptr(),
            DosMemDump::Win98 => MEM_DUMP_WIN98.as_ptr(),
            DosMemDump::Dosbox => MEM_DUMP_DOSBOX.as_ptr(),
            DosMemDump::Custom => std::ptr::addr_of!(MEM_DUMP_CUSTOM[0]),
        };

        let offset = offset as usize;
        if offset >= DOS_MEM_DUMP_SIZE {
            return 0;
        }

        match size {
            1 => {
                *(value as *mut u8) = *dump.add(offset);
                1
            }
            2 => {
                if offset + 1 >= DOS_MEM_DUMP_SIZE {
                    return 0;
                }
                *(value as *mut u16) =
                    (*dump.add(offset) as u16) | ((*dump.add(offset + 1) as u16) << 8);
                1
            }
            4 => {
                if offset + 3 >= DOS_MEM_DUMP_SIZE {
                    return 0;
                }
                *(value as *mut u32) = (*dump.add(offset) as u32)
                    | ((*dump.add(offset + 1) as u32) << 8)
                    | ((*dump.add(offset + 2) as u32) << 16)
                    | ((*dump.add(offset + 3) as u32) << 24);
                1
            }
            _ => 0,
        }
    }
}
