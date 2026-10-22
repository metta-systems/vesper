//! Rust port of vendor/doomgeneric/w_main.c.
//!
//! Common command-line WAD loader. Scans `argv` for `-file <wad>...` and
//! adds each matching WAD to the lump directory via `W_AddFile`. The C
//! original also handled `-merge`, `-nwtmerge`, `-af`, `-as` and `-aa`
//! under `#ifdef FEATURE_WAD_MERGE`, but doomgeneric `#undef`s that
//! feature in `doomfeatures.h`, so the Rust port only ports the `-file`
//! path. Returns whether any additional WAD was loaded (the "homebrew
//! levels" / modified-game flag).

#![allow(non_upper_case_globals, non_snake_case)]

use std::ffi::c_char;

use libc::printf;

use crate::types::Boolean;

use crate::doom::d_iwad::D_TryFindWADByName;
use crate::doom::m_argv::{myargc, myargv, M_CheckParmWithArgs};
use crate::doom::w_wad::W_AddFile;

/// Parse `-file <wad>...` from the command line and append each WAD to the
/// lump directory.
///
/// Walks `myargv` starting after the `-file` parameter and feeds each
/// non-flag argument through `D_TryFindWADByName` and `W_AddFile`. Logs
/// `" adding <filename>\n"` via `printf` for each WAD loaded.
///
/// Returns `Boolean::TRUE` if at least one WAD was added (the original
/// "homebrew levels" / `modifiedgame` flag, used downstream to disable
/// network play and demo recording), otherwise `Boolean::FALSE`.
///
/// Called from `D_DoomMain` during startup. Unlike the C original this
/// port does not implement the `FEATURE_WAD_MERGE` parameters
/// (`-merge`, `-nwtmerge`, `-af`, `-as`, `-aa`) because doomgeneric
/// `#undef`s that feature.
#[no_mangle]
pub extern "C" fn W_ParseCommandLine() -> Boolean {
    let mut modifiedgame: Boolean = Boolean::FALSE;

    unsafe {
        let p = M_CheckParmWithArgs(c"-file".as_ptr(), 1);
        if p != 0 {
            let mut idx = p + 1;
            modifiedgame = Boolean::TRUE;
            while idx < myargc && **myargv.offset(idx as isize) != b'-' as c_char {
                let filename = D_TryFindWADByName(*myargv.offset(idx as isize));
                printf(c" adding %s\n".as_ptr(), filename);
                W_AddFile(filename);
                idx += 1;
            }
        }
    }

    modifiedgame
}
