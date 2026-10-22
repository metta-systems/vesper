//! Rust port of vendor/doomgeneric/f_wipe.c.
//!
//! Mission begin melt / wipe screen special effect.
//!
//! Implements two wipe algorithms that transition between two captured screen
//! buffers: a palette-index colour cross-fade (`color_xform`) and the classic
//! "melt" (columns slide downward at different speeds, revealing the new
//! screen beneath the old one).  Each algorithm is represented by three
//! function pointers - init, step, and exit - stored in the `WIPES` table
//! indexed as `wipeno * 3 + {0,1,2}`.
//!
//! Notable Rust-vs-C differences:
//! - Global statics use `static mut` with `unsafe` accessors rather than bare
//!   C globals.
//! - `wipe_shittyColMajorXform` is renamed to `wipe_shitty_col_major_xform`
//!   to follow Rust naming conventions while matching the comment in the C
//!   source.

#![allow(non_upper_case_globals, non_snake_case)]

use std::ffi::c_void;
use std::os::raw::c_int;

use crate::doom::m_random::M_Random;

use crate::doom::i_video::{SCREENHEIGHT, SCREENWIDTH};
use crate::doom::z_zone::PU_STATIC;

extern "C" {
    /// Allocate `size` bytes from zone memory with the given cache tag.
    fn Z_Malloc(size: c_int, tag: c_int, user: *mut c_void) -> *mut c_void;
    /// Free a zone-allocated block back to the heap.
    fn Z_Free(ptr: *mut c_void);
    /// Capture the current video buffer into `scr` (`SCREENWIDTH * SCREENHEIGHT` bytes).
    fn I_ReadScreen(scr: *mut u8);
    /// Blit a `width x height` block from `src` to the video buffer at `(x, y)`.
    fn V_DrawBlock(x: c_int, y: c_int, width: c_int, height: c_int, src: *mut u8);
    /// Mark a rectangle of the video buffer as dirty so it gets blitted to the display.
    fn V_MarkRect(x: c_int, y: c_int, width: c_int, height: c_int);
    /// Pointer to the primary video framebuffer; `SCREENWIDTH * SCREENHEIGHT` bytes.
    static mut I_VideoBuffer: *mut u8;
}

/// Whether the current wipe is active (`1`) or idle (`0`).
///
/// Set to 1 by [`wipe_ScreenWipe`] on the first call and cleared when the
/// wipe algorithm signals completion.  C origin: `static int go` in f_wipe.c.
static mut GO: c_int = 0;

/// Snapshot of the screen taken at wipe-start (the "old" frame).
///
/// Allocated in [`wipe_StartScreen`] and freed by `wipe_exit_melt`.
/// C origin: `wipe_scr_start` in f_wipe.c.
static mut WIPE_SCR_START: *mut u8 = std::ptr::null_mut();

/// Snapshot of the screen taken at wipe-end (the "new" frame).
///
/// Allocated in [`wipe_EndScreen`] and freed by `wipe_exit_melt`.
/// C origin: `wipe_scr_end` in f_wipe.c.
static mut WIPE_SCR_END: *mut u8 = std::ptr::null_mut();

/// Working buffer used during the wipe; points into the live video buffer.
///
/// Set to [`I_VideoBuffer`] at the start of each wipe pass.
/// C origin: `wipe_scr` in f_wipe.c.
static mut WIPE_SCR: *mut u8 = std::ptr::null_mut();

/// Per-column Y position array used by the melt algorithm.
///
/// One `c_int` per screen column; negative values mean the column has not yet
/// started falling.  Allocated by `wipe_init_melt` and freed by
/// `wipe_exit_melt`.  C origin: `y` (static) in f_wipe.c.
static mut Y: *mut c_int = std::ptr::null_mut();

/// Transpose a `width x height` array of `i16` from row-major to column-major
/// in-place.
///
/// Allocates a temporary buffer via `Z_Malloc`, performs the transposition,
/// copies the result back, then frees the buffer.  The function is named after
/// the original C identifier `wipe_shittyColMajorXform`.
///
/// Preconditions: `array` must point to at least `width * height` valid `i16`
/// elements; `Z_Malloc` must succeed.
///
/// C origin: `wipe_shittyColMajorXform` in f_wipe.c.
///
/// # Safety
///
/// `array` must be a valid, non-null pointer to `width * height` `i16` values.
/// The caller is responsible for ensuring no aliasing with the temporary
/// `Z_Malloc` buffer.
unsafe fn wipe_shitty_col_major_xform(array: *mut i16, width: c_int, height: c_int) {
    let total = (width * height) as usize;
    let dest = Z_Malloc(total as c_int * 2, PU_STATIC, std::ptr::null_mut()) as *mut i16;

    for y in 0..height {
        for x in 0..width {
            *dest.add((x * height + y) as usize) = *array.add((y * width + x) as usize);
        }
    }

    std::ptr::copy(dest, array, total);
    Z_Free(dest as *mut c_void);
}

/// Initialise the colour cross-fade wipe by copying the start screen into the
/// working buffer.
///
/// Returns 0 on success (matches the C convention: non-zero means done).
/// C origin: `wipe_initColorXForm` in f_wipe.c.
///
/// # Safety
///
/// `WIPE_SCR_START` and `WIPE_SCR` must both be valid pointers to at least
/// `width * height` bytes.
unsafe extern "C" fn wipe_init_color_xform(width: c_int, height: c_int, _ticks: c_int) -> c_int {
    let len = (width * height) as usize;
    std::ptr::copy(WIPE_SCR_START, WIPE_SCR, len);
    0
}

/// Advance the colour cross-fade wipe by `ticks` steps.
///
/// Each pixel in the working buffer is nudged toward the corresponding pixel in
/// the end screen by `ticks` palette index units per call.  Returns 0 while
/// the transition is still in progress and 1 when every pixel has reached its
/// target value.
///
/// C origin: `wipe_doColorXForm` in f_wipe.c.
///
/// # Safety
///
/// `WIPE_SCR`, `WIPE_SCR_END` must both be valid pointers to at least
/// `width * height` bytes.
unsafe extern "C" fn wipe_do_color_xform(width: c_int, height: c_int, ticks: c_int) -> c_int {
    let len = (width * height) as usize;
    let mut changed = false;

    for i in 0..len {
        let w = *WIPE_SCR.add(i);
        let e = *WIPE_SCR_END.add(i);

        if w != e {
            let newval: c_int;
            if w > e {
                newval = (w as c_int) - ticks;
                if newval < e as c_int {
                    *WIPE_SCR.add(i) = e;
                } else {
                    *WIPE_SCR.add(i) = newval as u8;
                }
                changed = true;
            } else {
                newval = (w as c_int) + ticks;
                if newval > e as c_int {
                    *WIPE_SCR.add(i) = e;
                } else {
                    *WIPE_SCR.add(i) = newval as u8;
                }
                changed = true;
            }
        }
    }

    if changed {
        0
    } else {
        1
    }
}

/// Clean up after the colour cross-fade wipe; a no-op that always returns 0.
///
/// C origin: `wipe_exitColorXForm` in f_wipe.c.
///
/// # Safety
///
/// No preconditions beyond those required by `extern "C"` calling convention.
unsafe extern "C" fn wipe_exit_color_xform(_width: c_int, _height: c_int, _ticks: c_int) -> c_int {
    0
}

/// Initialise the melt wipe.
///
/// Copies the start screen into the working buffer, transposes both pixel
/// buffers into column-major order for efficient column access, allocates the
/// per-column Y-position array, and seeds each column's starting offset with a
/// random negative value so columns begin falling at slightly different times.
///
/// Returns 0.  C origin: `wipe_initMelt` in f_wipe.c.
///
/// # Safety
///
/// `WIPE_SCR_START`, `WIPE_SCR_END`, and `WIPE_SCR` must be valid pointers to
/// at least `width * height` bytes.  `M_Random` must be safe to call.
unsafe extern "C" fn wipe_init_melt(width: c_int, height: c_int, _ticks: c_int) -> c_int {
    let len = (width * height) as usize;
    std::ptr::copy(WIPE_SCR_START, WIPE_SCR, len);

    wipe_shitty_col_major_xform(WIPE_SCR_START as *mut i16, width / 2, height);
    wipe_shitty_col_major_xform(WIPE_SCR_END as *mut i16, width / 2, height);

    Y = Z_Malloc(
        width * std::mem::size_of::<c_int>() as c_int,
        PU_STATIC,
        std::ptr::null_mut(),
    ) as *mut c_int;

    *Y.add(0) = -((M_Random() % 16) as c_int);
    for i in 1..width as usize {
        let r = (M_Random() % 3) as c_int - 1;
        *Y.add(i) = *Y.add(i - 1) + r;
        if *Y.add(i) > 0 {
            *Y.add(i) = 0;
        } else if *Y.add(i) == -16 {
            *Y.add(i) = -15;
        }
    }

    0
}

/// Advance the melt wipe by `ticks` game-ticks.
///
/// For each column that has not yet fully melted, the column slides down
/// according to an accelerating schedule (speed increases up to a maximum of 8
/// pixels per tick once the column has moved 16 or more pixels).  The
/// end-screen pixels are revealed from the top; the start-screen pixels are
/// pushed down and off the bottom.  Returns 1 when all columns are done, 0
/// otherwise.
///
/// The algorithm works on transposed (column-major) copies of the pixel
/// buffers, hence the `width / 2` column count (each 16-bit word covers two
/// 8-bit pixels).
///
/// C origin: `wipe_doMelt` in f_wipe.c.
///
/// # Safety
///
/// `Y`, `WIPE_SCR_START`, `WIPE_SCR_END`, and `WIPE_SCR` must be valid
/// pointers initialised by `wipe_init_melt`.
unsafe extern "C" fn wipe_do_melt(width: c_int, height: c_int, ticks: c_int) -> c_int {
    let width = width / 2;
    let mut done = true;
    let mut ticks = ticks;

    while ticks > 0 {
        ticks -= 1;
        for i in 0..width as usize {
            if *Y.add(i) < 0 {
                *Y.add(i) += 1;
                done = false;
            } else if *Y.add(i) < height {
                let mut dy: c_int;
                if *Y.add(i) < 16 {
                    dy = *Y.add(i) + 1;
                } else {
                    dy = 8;
                }
                if *Y.add(i) + dy >= height {
                    dy = height - *Y.add(i);
                }

                let yi = *Y.add(i) as usize;
                let src = (WIPE_SCR_END as *mut i16).add(i * height as usize + yi);
                let dst = (WIPE_SCR as *mut i16).add(yi * width as usize + i);
                for j in 0..dy as usize {
                    *dst.add(j * width as usize) = *src.add(j);
                }

                *Y.add(i) += dy;

                let src = (WIPE_SCR_START as *mut i16).add(i * height as usize);
                let dst = (WIPE_SCR as *mut i16).add((*Y.add(i) as usize) * width as usize + i);
                for j in 0..(height as usize - *Y.add(i) as usize) {
                    *dst.add(j * width as usize) = *src.add(j);
                }

                done = false;
            }
        }
    }

    if done {
        1
    } else {
        0
    }
}

/// Clean up after the melt wipe by freeing the Y-position and pixel snapshot
/// buffers.
///
/// Always returns 0.  C origin: `wipe_exitMelt` in f_wipe.c.
///
/// # Safety
///
/// `Y`, `WIPE_SCR_START`, and `WIPE_SCR_END` must have been allocated by
/// `wipe_init_melt` / `wipe_StartScreen` / `wipe_EndScreen`.
unsafe extern "C" fn wipe_exit_melt(_width: c_int, _height: c_int, _ticks: c_int) -> c_int {
    Z_Free(Y as *mut c_void);
    Z_Free(WIPE_SCR_START as *mut c_void);
    Z_Free(WIPE_SCR_END as *mut c_void);
    Y = std::ptr::null_mut();
    WIPE_SCR_START = std::ptr::null_mut();
    WIPE_SCR_END = std::ptr::null_mut();
    0
}

/// Function-pointer type shared by all wipe init/step/exit callbacks.
///
/// Arguments are `(width, height, ticks)`.  Return value: 0 = still running,
/// 1 = complete.  C origin: the implicit function-pointer type used in the
/// `wipes` table in f_wipe.c.
type WipeFn = unsafe extern "C" fn(c_int, c_int, c_int) -> c_int;

/// Dispatch table for wipe algorithms.
///
/// Indexed as `wipeno * 3 + phase`, where `phase` is 0 (init), 1 (step), or
/// 2 (exit).  `wipeno` 0 selects the colour cross-fade; `wipeno` 1 selects the
/// melt.  C origin: `wipes[2][3]` in f_wipe.c.
const WIPES: [WipeFn; 6] = [
    wipe_init_color_xform,
    wipe_do_color_xform,
    wipe_exit_color_xform,
    wipe_init_melt,
    wipe_do_melt,
    wipe_exit_melt,
];

/// Capture the current screen as the wipe start ("old") frame.
///
/// Allocates `SCREENWIDTH * SCREENHEIGHT` bytes via `Z_Malloc` and fills it
/// with a copy of the video buffer.  The `x`, `y`, `width`, and `height`
/// parameters are accepted for ABI compatibility but unused; the full screen is
/// always captured.  Returns 0.
///
/// Called by C code in `d_main.c` before the scene transition.
/// C origin: `wipe_StartScreen` in f_wipe.c.
#[no_mangle]
pub extern "C" fn wipe_StartScreen(_x: c_int, _y: c_int, _width: c_int, _height: c_int) -> c_int {
    unsafe {
        WIPE_SCR_START =
            Z_Malloc(SCREENWIDTH * SCREENHEIGHT, PU_STATIC, std::ptr::null_mut()) as *mut u8;
        I_ReadScreen(WIPE_SCR_START);
    }
    0
}

/// Capture the current screen as the wipe end ("new") frame, then restore the
/// start frame to the video buffer.
///
/// Allocates `SCREENWIDTH * SCREENHEIGHT` bytes via `Z_Malloc`, captures the
/// video buffer into it, and then blits the start frame back so that the
/// display still shows the old scene until the wipe begins.  Returns 0.
///
/// Precondition: [`wipe_StartScreen`] must have been called first.
///
/// Called by C code in `d_main.c` after drawing the new scene.
/// C origin: `wipe_EndScreen` in f_wipe.c.
#[no_mangle]
pub extern "C" fn wipe_EndScreen(x: c_int, y: c_int, width: c_int, height: c_int) -> c_int {
    unsafe {
        WIPE_SCR_END =
            Z_Malloc(SCREENWIDTH * SCREENHEIGHT, PU_STATIC, std::ptr::null_mut()) as *mut u8;
        I_ReadScreen(WIPE_SCR_END);
        V_DrawBlock(x, y, width, height, WIPE_SCR_START);
    }
    0
}

/// Drive the screen wipe one frame forward and return whether the wipe is
/// complete.
///
/// On the first call (`GO == 0`), initialises the chosen algorithm, sets
/// `WIPE_SCR` to the live video buffer, and calls the init function.
/// Subsequently calls the step function; if it signals completion, calls the
/// exit function and resets `GO`.  Marks the updated rectangle as dirty via
/// `V_MarkRect` each call.
///
/// Returns 1 when the wipe is finished, 0 while it is still running.
///
/// The `_x` and `_y` parameters are accepted for ABI compatibility but are
/// unused; the full `(0, 0, width, height)` rectangle is always marked.
///
/// Called by C code in `d_main.c` each game tick while a wipe is active.
/// C origin: `wipe_ScreenWipe` in f_wipe.c.
#[no_mangle]
pub extern "C" fn wipe_ScreenWipe(
    wipeno: c_int,
    _x: c_int,
    _y: c_int,
    width: c_int,
    height: c_int,
    ticks: c_int,
) -> c_int {
    unsafe {
        if GO == 0 {
            GO = 1;
            WIPE_SCR = I_VideoBuffer;
            WIPES[(wipeno * 3) as usize](width, height, ticks);
        }

        V_MarkRect(0, 0, width, height);
        let rc = WIPES[(wipeno * 3 + 1) as usize](width, height, ticks);

        if rc != 0 {
            GO = 0;
            WIPES[(wipeno * 3 + 2) as usize](width, height, ticks);
        }

        if GO == 0 {
            1
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wipe_fns_are_callable() {
        // Sanity: verify all six function pointers are distinct and
        // non-null at compile-time.
        let fns: [*const (); 6] = WIPES.map(|f| f as *const ());
        for i in 0..6 {
            for j in (i + 1)..6 {
                assert_ne!(fns[i], fns[j], "WIPES[{i}] == WIPES[{j}]");
            }
        }
    }
}
