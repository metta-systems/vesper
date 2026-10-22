//! Rust port of vendor/doomgeneric/doomgeneric.c.
//!
//! Platform abstraction entry point for the doomgeneric framework.
//!
//! `doomgeneric` is a thin portability shim that decouples the Doom engine
//! core from any particular windowing/audio backend.  Platform implementors
//! provide six callbacks (`DG_Init`, `DG_DrawFrame`, `DG_SleepMs`,
//! `DG_GetTicksMs`, `DG_GetKey`, `DG_SetWindowTitle`); the engine calls these
//! instead of talking to the OS directly.
//!
//! This module covers the library side: it owns the shared frame-buffer
//! pointer [`DG_ScreenBuffer`] and implements [`doomgeneric_Create`], which
//! allocates that buffer, stores the command-line arguments, and launches
//! the main engine entry point.
//!
//! Rust differences from C:
//! - The C code allocates the screen buffer with `malloc` and never frees it
//!   (intentional leak in a long-running process).  This port allocates via
//!   `Vec<u32>` and uses `std::mem::forget` to produce the same intentional
//!   leak, maintaining ABI equivalence while using Rust allocation.
//! - Several ported Rust modules (`p_ceilng`, `p_doors`, etc.) export
//!   `#[no_mangle]` functions that are only called from C.  Without an
//!   explicit reference in Rust, the linker's dead-code elimination (LTO)
//!   would strip them.  [`doomgeneric_Create`] therefore calls a zero-cost
//!   "link anchor" function from each such module to keep them alive.

#![allow(non_upper_case_globals, non_snake_case)]

use std::ffi::{c_char, c_int};
use std::ptr;

// Pull in the anchors so all #[no_mangle] functions survive link-time dead-code elimination
// (they are only called from C).
use super::hu_lib::HUlib_init;
use super::i_input::I_Input_Link_Anchor;
use super::p_ceilng::P_Ceilng_Link_Anchor;
use super::p_doors::P_Doors_Link_Anchor;
use super::p_floor::P_Floor_Link_Anchor;
use super::p_lights::P_Lights_Link_Anchor;
use super::p_plats::P_Plats_Link_Anchor;
use super::p_pspr::P_Pspr_Link_Anchor;
use super::p_sight::P_Sight_Link_Anchor;
use super::p_switch::P_Switch_Link_Anchor;
use super::p_telept::P_Telept_Link_Anchor;
use super::p_user::P_User_Link_Anchor;
use super::r_main::R_Main_Link_Anchor;

/// Width of the doomgeneric frame buffer in pixels.
///
/// Matches `DOOMGENERIC_RESX` in `doomgeneric.h` (default 640).
pub const DOOMGENERIC_RESX: usize = 640;

/// Height of the doomgeneric frame buffer in pixels.
///
/// Matches `DOOMGENERIC_RESY` in `doomgeneric.h` (default 400).
pub const DOOMGENERIC_RESY: usize = 400;

/// Total pixel count for one complete frame (`RESX * RESY`).
///
/// Convenience constant used when allocating or iterating over [`DG_ScreenBuffer`].
pub const DOOMGENERIC_PIXELS: usize = DOOMGENERIC_RESX * DOOMGENERIC_RESY;

/// Shared frame-buffer pointer written by the renderer and read by the platform backend.
///
/// Corresponds to `pixel_t *DG_ScreenBuffer` in `doomgeneric.c`.  After
/// [`doomgeneric_Create`] runs, this points to a heap-allocated array of
/// [`DOOMGENERIC_PIXELS`] `u32` values (RGBA8 or BGRA8 depending on the
/// build).  The renderer (`R_DrawColumn`, `R_DrawSpan`, etc.) writes pixels
/// into this buffer each frame; the platform backend reads from it inside
/// `DG_DrawFrame` to upload the frame to the display.
///
/// Initialised to `null`; valid only after [`doomgeneric_Create`] has been
/// called.  Referenced from C as `DG_ScreenBuffer`.
#[no_mangle]
pub static mut DG_ScreenBuffer: *mut u32 = ptr::null_mut();

extern "C" {
    /// Scan `argv` for `@file` response-file arguments and expand them in-place.
    ///
    /// Defined in `m_argv.c`.  Must be called before any argument parsing so
    /// that response files are transparent to the rest of the engine.
    fn M_FindResponseFile();

    /// Platform-provided initialisation callback.
    ///
    /// The platform implementation (e.g., `gpu.rs` or `headless.rs`) must
    /// define this symbol.  Called once by [`doomgeneric_Create`] after the
    /// screen buffer is allocated; responsible for creating the window,
    /// setting up audio, etc.
    fn DG_Init();

    /// Main engine entry point; never returns under normal operation.
    ///
    /// Defined in `d_main.c`.  Runs the game loop until the process exits.
    fn D_DoomMain();
}

extern "C" {
    /// Argument count forwarded to the engine from the host process.
    ///
    /// Defined as `int myargc` in `m_argv.c`.  Stored here by
    /// [`doomgeneric_Create`] before `D_DoomMain` reads it via `M_Arg*`.
    static mut myargc: c_int;

    /// Argument vector forwarded to the engine from the host process.
    ///
    /// Defined as `char **myargv` in `m_argv.c`.  The pointers in the array
    /// must remain valid for the lifetime of the process (typically `argv`
    /// from `main`).
    static mut myargv: *mut *mut c_char;
}

/// Initialise the Doom engine and enter the main game loop.
///
/// Corresponds to `doomgeneric_Create` in `doomgeneric.c`.  This is the
/// primary entry point called by the host application after setting up any
/// platform-specific context.
///
/// Steps performed:
/// 1. Call each ported module's link-anchor to prevent LTO from removing
///    `#[no_mangle]` symbols that are only referenced from C.
/// 2. Store `argc`/`argv` in `myargc`/`myargv` so `M_Arg*` functions work.
/// 3. Call `M_FindResponseFile` to expand any `@file` arguments.
/// 4. Allocate the screen buffer ([`DOOMGENERIC_PIXELS`] × 4 bytes) via
///    `Vec` and intentionally leak it so the lifetime matches the C `malloc`
///    version.
/// 5. Call `DG_Init` so the platform backend can create its window/context.
/// 6. Call `D_DoomMain`, which never returns under normal operation.
///
/// # Safety
/// - `argv` must point to an array of at least `argc` valid NUL-terminated
///   C strings, all of which must remain valid for the lifetime of the
///   process.
/// - This function must be called exactly once; calling it a second time
///   would leak the first screen buffer and overwrite `myargc`/`myargv`.
#[no_mangle]
pub unsafe extern "C" fn doomgeneric_Create(argc: c_int, argv: *mut *mut c_char) {
    // Anchor all ported module symbols so they survive LTO (called only from C).
    P_Ceilng_Link_Anchor();
    P_Doors_Link_Anchor();
    P_Floor_Link_Anchor();
    P_Lights_Link_Anchor();
    P_Plats_Link_Anchor();
    P_Pspr_Link_Anchor();
    P_Sight_Link_Anchor();
    P_Switch_Link_Anchor();
    P_Telept_Link_Anchor();
    P_User_Link_Anchor();
    I_Input_Link_Anchor();
    R_Main_Link_Anchor();
    let _ = HUlib_init as *const () as usize;

    myargc = argc;
    myargv = argv;

    M_FindResponseFile();

    let total_pixels = DOOMGENERIC_RESX * DOOMGENERIC_RESY;
    let mut buffer = vec![0u32; total_pixels];
    DG_ScreenBuffer = buffer.as_mut_ptr();
    std::mem::forget(buffer);

    DG_Init();
    D_DoomMain();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dg_screenbuffer_is_null_init() {
        unsafe {
            assert!(DG_ScreenBuffer.is_null());
        }
    }
}
