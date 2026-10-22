//! Raw FFI bindings to the doomgeneric C library.
//!
//! This crate wraps the vendored [doomgeneric] C source, providing the
//! minimal set of foreign function declarations needed by the `room` crate
//! (the Rust platform implementation).
//!
//! # Safety
//!
//! All functions in this crate are `unsafe` because they operate on raw C
//! pointers and global mutable state inherited from the original Doom engine.
//! Callers are responsible for ensuring:
//!
//! - [`doomgeneric_Create`] is called exactly once before any other function.
//! - [`doomgeneric_Tick`] is called only after [`doomgeneric_Create`] returns.
//! - The Doom platform callbacks (`DG_Init`, `DG_DrawFrame`, etc.) are
//!   implemented and exported with `#[no_mangle] pub extern "C"` before
//!   `doomgeneric_Create` is called.
//!
//! [doomgeneric]: https://github.com/ozkl/doomgeneric

// Re-export types used in the public API.
pub use std::ffi::{c_char, c_int, c_uint};

/// Default horizontal resolution of the Doom frame buffer.
///
/// Matches `DOOMGENERIC_RESX` from `doomgeneric.h`.
pub const DOOMGENERIC_RESX: usize = 640;

/// Default vertical resolution of the Doom frame buffer.
///
/// Matches `DOOMGENERIC_RESY` from `doomgeneric.h`.
pub const DOOMGENERIC_RESY: usize = 400;

/// Total number of pixels in the Doom frame buffer.
pub const DOOMGENERIC_PIXELS: usize = DOOMGENERIC_RESX * DOOMGENERIC_RESY;

extern "C" {
    /// Pointer to the Doom screen buffer.
    ///
    /// After [`doomgeneric_Create`] returns, this points to a heap-allocated
    /// buffer of `DOOMGENERIC_RESX * DOOMGENERIC_RESY * 4` bytes containing
    /// the current frame in BGRA (little-endian) pixel format.
    ///
    /// The buffer is written by the engine's `I_FinishUpdate` function and
    /// should be read inside the `DG_DrawFrame` callback.
    ///
    /// # Safety
    ///
    /// Valid only after [`doomgeneric_Create`] has been called. Must not be
    /// freed or resized by the caller.
    pub static mut DG_ScreenBuffer: *mut u32;

    /// Initialise the Doom engine.
    ///
    /// Parses `argc`/`argv`, allocates the screen buffer, calls the platform
    /// callback `DG_Init()`, and then calls `D_DoomMain()` which performs
    /// all Doom subsystem initialisation and runs a single tick.
    ///
    /// This function returns after one tick; subsequent ticks must be driven
    /// by calling [`doomgeneric_Tick`] in a loop.
    ///
    /// # Safety
    ///
    /// - Must be called exactly once.
    /// - `argc` and `argv` must form a valid C argument list.
    /// - The DG_* platform callbacks must be available as C-callable symbols
    ///   before this is called.
    pub fn doomgeneric_Create(argc: c_int, argv: *mut *mut c_char);

    /// Advance the Doom engine by one tick.
    ///
    /// Each call to this function processes input events, runs one or more
    /// game tics, and calls `DG_DrawFrame()` to present the rendered frame.
    ///
    /// Typically called in a tight loop driven by the platform's event loop:
    ///
    /// ```text
    /// loop {
    ///     doomgeneric_Tick();
    /// }
    /// ```
    ///
    /// # Safety
    ///
    /// Must be called only after [`doomgeneric_Create`] has returned.
    pub fn doomgeneric_Tick();

    /// Return the C-side value of `DOOM_191_VERSION` (defined in `doomdef.h`).
    ///
    /// Used by integration tests to verify that a Rust-ported constant matches
    /// the original `#define`.
    pub fn room_test_get_doom_191_version() -> c_int;

    /// `sizeof(state_t)` as the C compiler sees it.
    pub fn room_test_get_state_t_sizeof() -> c_int;
    /// `offsetof(state_t, tics)` as the C compiler sees it.
    pub fn room_test_get_state_t_tics_offset() -> c_int;

    /// `sizeof(mobjinfo_t)` as the C compiler sees it.
    pub fn room_test_get_mobjinfo_t_sizeof() -> c_int;
    /// `offsetof(mobjinfo_t, speed)` as the C compiler sees it.
    pub fn room_test_get_mobjinfo_t_speed_offset() -> c_int;

    /// `S_SARG_RUN1` enum value from `statenum_t`.
    pub fn room_test_get_s_sarg_run1() -> c_int;
    /// `S_SARG_PAIN2` enum value from `statenum_t`.
    pub fn room_test_get_s_sarg_pain2() -> c_int;

    /// `MT_BRUISERSHOT` enum value from `mobjtype_t`.
    pub fn room_test_get_mt_bruisershot() -> c_int;
    /// `MT_HEADSHOT` enum value from `mobjtype_t`.
    pub fn room_test_get_mt_headshot() -> c_int;
    /// `MT_TROOPSHOT` enum value from `mobjtype_t`.
    pub fn room_test_get_mt_troopshot() -> c_int;
}
