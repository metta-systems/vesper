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
//! - The C file statics live in [`WipeState`], held in the [`WIPE`] global.
//! - `wipe_shittyColMajorXform` is renamed to `wipe_shitty_col_major_xform`
//!   to follow Rust naming conventions while matching the comment in the C
//!   source.

#![allow(non_snake_case)]

use std::ffi::c_void;

use crate::engine::misc::random::M_Random;

use crate::engine::{
    global::Global,
    interface::video::{SCREENHEIGHT, SCREENWIDTH},
    zone::PU_STATIC,
};

use crate::engine::{
    interface::video::{I_ReadScreen, video_buffer},
    render_2d::{V_DrawBlock, V_MarkRect},
    zone::{Z_Free, Z_Malloc},
};

/// Screen wipe state (C `f_wipe.c` file statics).
pub struct WipeState {
    /// Whether the current wipe is active (`1`) or idle (`0`).
    ///
    /// Set to 1 by [`wipe_ScreenWipe`] on the first call and cleared when
    /// the wipe algorithm signals completion.  C origin: `static int go`.
    go: i32,
    /// Snapshot of the screen taken at wipe-start (the "old" frame).
    ///
    /// Allocated in [`wipe_StartScreen`] and freed by `wipe_exit_melt`.
    /// C origin: `wipe_scr_start`.
    scr_start: *mut u8,
    /// Snapshot of the screen taken at wipe-end (the "new" frame).
    ///
    /// Allocated in [`wipe_EndScreen`] and freed by `wipe_exit_melt`.
    /// C origin: `wipe_scr_end`.
    scr_end: *mut u8,
    /// Working buffer used during the wipe; points into the live video
    /// buffer.
    ///
    /// Set to [`I_VideoBuffer`] at the start of each wipe pass.
    /// C origin: `wipe_scr`.
    scr: *mut u8,
    /// Per-column Y position array used by the melt algorithm.
    ///
    /// One `i32` per screen column; negative values mean the column has not
    /// yet started falling.  Allocated by `wipe_init_melt` and freed by
    /// `wipe_exit_melt`.  C origin: `y` (static).
    column_y: *mut i32,
}

impl WipeState {
    /// The state at program start: no wipe running, no buffers.
    pub const fn new() -> Self {
        Self {
            go: 0,
            scr_start: std::ptr::null_mut(),
            scr_end: std::ptr::null_mut(),
            scr: std::ptr::null_mut(),
            column_y: std::ptr::null_mut(),
        }
    }
}

impl Default for WipeState {
    fn default() -> Self {
        Self::new()
    }
}

/// The screen wipe state.
pub static WIPE: Global<WipeState> = Global::new(WipeState::new());

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
unsafe fn wipe_shitty_col_major_xform(array: *mut i16, width: i32, height: i32) {
    let total = (width * height) as usize;
    let dest = Z_Malloc(total as i32 * 2, PU_STATIC, std::ptr::null_mut()) as *mut i16;

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
/// `scr_start` and `scr` must both be valid pointers to at least
/// `width * height` bytes.
unsafe fn wipe_init_color_xform(width: i32, height: i32, _ticks: i32) -> i32 {
    let len = (width * height) as usize;
    let (scr_start, scr) = {
        let wipe = WIPE.borrow();
        (wipe.scr_start, wipe.scr)
    };
    std::ptr::copy(scr_start, scr, len);
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
/// `scr` and `scr_end` must both be valid pointers to at least
/// `width * height` bytes.
unsafe fn wipe_do_color_xform(width: i32, height: i32, ticks: i32) -> i32 {
    let len = (width * height) as usize;
    let mut changed = false;
    let (scr, scr_end) = {
        let wipe = WIPE.borrow();
        (wipe.scr, wipe.scr_end)
    };

    for i in 0..len {
        let w = *scr.add(i);
        let e = *scr_end.add(i);

        if w != e {
            let newval: i32;
            if w > e {
                newval = (w as i32) - ticks;
                if newval < e as i32 {
                    *scr.add(i) = e;
                } else {
                    *scr.add(i) = newval as u8;
                }
                changed = true;
            } else {
                newval = (w as i32) + ticks;
                if newval > e as i32 {
                    *scr.add(i) = e;
                } else {
                    *scr.add(i) = newval as u8;
                }
                changed = true;
            }
        }
    }

    if changed { 0 } else { 1 }
}

/// Clean up after the colour cross-fade wipe; a no-op that always returns 0.
///
/// C origin: `wipe_exitColorXForm` in f_wipe.c.
///
/// # Safety
///
/// No preconditions; `unsafe` only to match the other wipe callbacks.
unsafe fn wipe_exit_color_xform(_width: i32, _height: i32, _ticks: i32) -> i32 {
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
/// `scr_start`, `scr_end`, and `scr` must be valid pointers to at least
/// `width * height` bytes.  `M_Random` must be safe to call.
unsafe fn wipe_init_melt(width: i32, height: i32, _ticks: i32) -> i32 {
    let len = (width * height) as usize;
    let (scr_start, scr_end, scr) = {
        let wipe = WIPE.borrow();
        (wipe.scr_start, wipe.scr_end, wipe.scr)
    };
    std::ptr::copy(scr_start, scr, len);

    wipe_shitty_col_major_xform(scr_start as *mut i16, width / 2, height);
    wipe_shitty_col_major_xform(scr_end as *mut i16, width / 2, height);

    let column_y = Z_Malloc(
        width * std::mem::size_of::<i32>() as i32,
        PU_STATIC,
        std::ptr::null_mut(),
    ) as *mut i32;
    WIPE.borrow_mut().column_y = column_y;

    *column_y.add(0) = -(M_Random() % 16);
    for i in 1..width as usize {
        let r = M_Random() % 3 - 1;
        *column_y.add(i) = *column_y.add(i - 1) + r;
        if *column_y.add(i) > 0 {
            *column_y.add(i) = 0;
        } else if *column_y.add(i) == -16 {
            *column_y.add(i) = -15;
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
/// `column_y`, `scr_start`, `scr_end`, and `scr` must be valid pointers
/// initialised by `wipe_init_melt`.
unsafe fn wipe_do_melt(width: i32, height: i32, ticks: i32) -> i32 {
    let width = width / 2;
    let mut done = true;
    let mut ticks = ticks;
    let (column_y, scr_start, scr_end, scr) = {
        let wipe = WIPE.borrow();
        (wipe.column_y, wipe.scr_start, wipe.scr_end, wipe.scr)
    };

    while ticks > 0 {
        ticks -= 1;
        for i in 0..width as usize {
            if *column_y.add(i) < 0 {
                *column_y.add(i) += 1;
                done = false;
            } else if *column_y.add(i) < height {
                let mut dy: i32;
                if *column_y.add(i) < 16 {
                    dy = *column_y.add(i) + 1;
                } else {
                    dy = 8;
                }
                if *column_y.add(i) + dy >= height {
                    dy = height - *column_y.add(i);
                }

                let yi = *column_y.add(i) as usize;
                let src = (scr_end as *mut i16).add(i * height as usize + yi);
                let dst = (scr as *mut i16).add(yi * width as usize + i);
                for j in 0..dy as usize {
                    *dst.add(j * width as usize) = *src.add(j);
                }

                *column_y.add(i) += dy;

                let src = (scr_start as *mut i16).add(i * height as usize);
                let dst = (scr as *mut i16).add((*column_y.add(i) as usize) * width as usize + i);
                for j in 0..(height as usize - *column_y.add(i) as usize) {
                    *dst.add(j * width as usize) = *src.add(j);
                }

                done = false;
            }
        }
    }

    if done { 1 } else { 0 }
}

/// Clean up after the melt wipe by freeing the Y-position and pixel snapshot
/// buffers.
///
/// Always returns 0.  C origin: `wipe_exitMelt` in f_wipe.c.
///
/// # Safety
///
/// `column_y`, `scr_start`, and `scr_end` must have been allocated by
/// `wipe_init_melt` / `wipe_StartScreen` / `wipe_EndScreen`.
unsafe fn wipe_exit_melt(_width: i32, _height: i32, _ticks: i32) -> i32 {
    let (column_y, scr_start, scr_end) = {
        let mut wipe = WIPE.borrow_mut();
        let buffers = (wipe.column_y, wipe.scr_start, wipe.scr_end);
        wipe.column_y = std::ptr::null_mut();
        wipe.scr_start = std::ptr::null_mut();
        wipe.scr_end = std::ptr::null_mut();
        buffers
    };
    Z_Free(column_y as *mut c_void);
    Z_Free(scr_start as *mut c_void);
    Z_Free(scr_end as *mut c_void);
    0
}

/// Function-pointer type shared by all wipe init/step/exit callbacks.
///
/// Arguments are `(width, height, ticks)`.  Return value: 0 = still running,
/// 1 = complete.  C origin: the implicit function-pointer type used in the
/// `wipes` table in f_wipe.c.
type WipeFn = unsafe fn(i32, i32, i32) -> i32;

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
pub fn wipe_StartScreen(_x: i32, _y: i32, _width: i32, _height: i32) -> i32 {
    let scr_start =
        unsafe { Z_Malloc(SCREENWIDTH * SCREENHEIGHT, PU_STATIC, std::ptr::null_mut()) as *mut u8 };
    WIPE.borrow_mut().scr_start = scr_start;
    unsafe { I_ReadScreen(scr_start) };
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
pub fn wipe_EndScreen(x: i32, y: i32, width: i32, height: i32) -> i32 {
    let scr_end =
        unsafe { Z_Malloc(SCREENWIDTH * SCREENHEIGHT, PU_STATIC, std::ptr::null_mut()) as *mut u8 };
    let scr_start = {
        let mut wipe = WIPE.borrow_mut();
        wipe.scr_end = scr_end;
        wipe.scr_start
    };
    unsafe {
        I_ReadScreen(scr_end);
        V_DrawBlock(x, y, width, height, scr_start);
    }
    0
}

/// Drive the screen wipe one frame forward and return whether the wipe is
/// complete.
///
/// On the first call (`go == 0`), initialises the chosen algorithm, sets
/// `scr` to the live video buffer, and calls the init function.
/// Subsequently calls the step function; if it signals completion, calls the
/// exit function and resets `go`.  Marks the updated rectangle as dirty via
/// `V_MarkRect` each call.
///
/// Returns 1 when the wipe is finished, 0 while it is still running.
///
/// The `_x` and `_y` parameters are accepted for ABI compatibility but are
/// unused; the full `(0, 0, width, height)` rectangle is always marked.
///
/// Called by C code in `d_main.c` each game tick while a wipe is active.
/// C origin: `wipe_ScreenWipe` in f_wipe.c.
pub fn wipe_ScreenWipe(wipeno: i32, _x: i32, _y: i32, width: i32, height: i32, ticks: i32) -> i32 {
    unsafe {
        let starting = {
            let mut wipe = WIPE.borrow_mut();
            let starting = wipe.go == 0;
            if starting {
                wipe.go = 1;
                wipe.scr = video_buffer();
            }
            starting
        };
        if starting {
            WIPES[(wipeno * 3) as usize](width, height, ticks);
        }

        V_MarkRect(0, 0, width, height);
        let rc = WIPES[(wipeno * 3 + 1) as usize](width, height, ticks);

        if rc != 0 {
            WIPE.borrow_mut().go = 0;
            WIPES[(wipeno * 3 + 2) as usize](width, height, ticks);
        }

        if WIPE.borrow().go == 0 { 1 } else { 0 }
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
