//! Rust port of vendor/doomgeneric/st_lib.c.
//!
//! Status bar widget library: number, percent, multi-icon, and binary-icon widgets.
//!
//! Four widget types are provided, mirroring `st_lib.h`:
//! * [`st_number_t`] - right-justified integer rendered digit-by-digit with a
//!   patch font; supports negative values and a magic "no-draw" sentinel (1994).
//! * [`st_percent_t`] - wraps an [`st_number_t`] and appends a `%` patch.
//! * [`st_multicon_t`] - displays one patch from an indexed array, e.g. for
//!   key-card icons or face sprites.
//! * [`st_binicon_t`] - shows a patch when a boolean flag is non-zero, hides
//!   it otherwise.
//!
//! All widgets use a "dirty bit" pattern: the previous value is cached and the
//! widget is only redrawn when the value changes or `refresh` is requested.
//! Erasing is done by blitting from `st_backing_screen` (a saved copy of the
//! status bar background).
//!
//! Notable Rust-vs-C differences:
//! * C `boolean*` is `*mut i32` throughout.
//! * The `SHORT` macro (little-endian swap) is an identity function on x86_64.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::{
    engine::{render_2d::patch_t, zone::PU_STATIC},
    i_error,
};
/// Height of the status bar in pixels.
/// Mirrors `ST_HEIGHT` from `st_stuff.h`.
const ST_HEIGHT: i32 = 32;

/// Y coordinate of the top of the status bar (screen height minus bar height).
/// All widget Y coordinates are expected to be at or below this value.
/// Mirrors `ST_Y` from `st_stuff.h`.
const ST_Y: i32 = 200 - ST_HEIGHT; // 168

/// Byte-swap for little-endian (SHORT macro from i_swap.h).
/// On x86_64 this is an identity cast.
#[inline(always)]
fn short_swap(v: i16) -> i16 {
    v
}

/// A right-justified integer display widget.
///
/// Corresponds to `st_number_t` in `st_lib.h`. Digits are rendered
/// right-to-left using a patch font; a minus sign patch is prepended for
/// negative values. The magic value `1994` means "do not draw" (used when
/// a slot is inactive).
///
/// # Layout invariants
/// * `p[0..9]` must point to valid digit patches when drawing.
/// * `on` and `num` must be valid non-null pointers for the widget's lifetime.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct st_number_t {
    /// Right edge X coordinate of the number (digits extend leftward from here).
    pub x: i32,
    /// Top Y coordinate of the number.
    pub y: i32,
    /// Maximum number of digits to display (controls field width).
    pub width: i32,
    /// Cached value from the previous frame; used to detect changes.
    pub oldnum: i32,
    /// Pointer to the current integer value to display.
    pub num: *mut i32,
    /// Visibility flag pointer; widget is only drawn when `*on != 0`.
    /// Maps to `boolean*` in C.
    pub on: *mut i32, // boolean* (i32 in C)
    /// Array of digit patches; `p[d]` is the patch for digit `d` (0-9).
    pub p: *mut *mut patch_t,
    /// User-defined auxiliary data (unused by the widget library itself).
    pub data: i32,
}

/// A percentage display widget: a number followed by a `%` sign patch.
///
/// Corresponds to `st_percent_t` in `st_lib.h`. The embedded [`st_number_t`]
/// renders the numeric portion; the additional `p` patch is drawn immediately
/// to the right of the number on each refresh.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct st_percent_t {
    /// The underlying number widget (renders the digits).
    pub n: st_number_t,
    /// The `%` percent-sign patch drawn after the number.
    pub p: *mut patch_t,
}

/// A widget that displays one patch selected from an indexed array.
///
/// Corresponds to `st_multicon_t` in `st_lib.h`. Used for key-card slots,
/// player face sprites, and any other status bar element that cycles through
/// a discrete set of images. When the selected index changes, the old patch
/// area is erased by blitting from the backing screen before the new patch is
/// drawn.
///
/// # Layout invariants
/// * `p[0..N]` must contain valid patch pointers for all possible values of
///   `*inum`.
/// * `inum == -1` means "no image"; the widget skips drawing entirely.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct st_multicon_t {
    /// Center-justified screen X position (patch offset subtracted at draw time).
    pub x: i32,
    /// Center-justified screen Y position (patch offset subtracted at draw time).
    pub y: i32,
    /// Index of the patch displayed in the previous frame; `-1` if no patch was
    /// shown.
    pub oldinum: i32,
    /// Pointer to the current icon index; `-1` suppresses drawing.
    pub inum: *mut i32,
    /// Visibility flag pointer; widget is only drawn when `*on != 0`.
    pub on: *mut i32,
    /// Array of icon patches indexed by `*inum`.
    pub p: *mut *mut patch_t,
    /// User-defined auxiliary data (unused by the widget library itself).
    pub data: i32,
}

/// A widget that shows a single patch when a boolean flag is set.
///
/// Corresponds to `st_binicon_t` in `st_lib.h`. When `*val` transitions from
/// zero to non-zero the patch is drawn; when it transitions back to zero the
/// patch area is erased from the backing screen. Used for key-card presence
/// indicators and similar binary status elements.
///
/// # Layout invariants
/// * `p` must point to a valid patch for the lifetime of the widget.
/// * `val` and `on` must be valid non-null pointers.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct st_binicon_t {
    /// Center-justified screen X position.
    pub x: i32,
    /// Center-justified screen Y position.
    pub y: i32,
    /// Cached value of `*val` from the previous frame.
    pub oldval: i32,
    /// Pointer to the current boolean value (non-zero = draw, zero = hide).
    pub val: *mut i32,
    /// Visibility flag pointer; widget is only drawn when `*on != 0`.
    pub on: *mut i32,
    /// The patch to display when `*val != 0`.
    pub p: *mut patch_t,
    /// User-defined auxiliary data (unused by the widget library itself).
    pub data: i32,
}

/// The minus-sign patch (`STTMINUS`) used when rendering negative numbers.
///
/// Loaded by [`STlib_init`] from the WAD and drawn to the left of the
/// most-significant digit when a number widget displays a negative value.
/// Corresponds to `sttminus` in `st_lib.c`; exported so `st_stuff.c` can
/// reference it directly.
pub static mut sttminus: *mut patch_t = std::ptr::null_mut();

use crate::engine::{
    render_2d::{V_CopyRect, V_DrawPatch},
    status::stuff::st_backing_screen,
    wad::W_CacheLumpName,
};

/// Initialize the status bar widget library.
///
/// Loads the `STTMINUS` WAD lump as `PU_STATIC` and stores it in
/// [`sttminus`]. All other widget state is initialized via the individual
/// `STlib_init*` functions. Called once at startup from `ST_Init` in
/// `st_stuff.c`.
pub fn STlib_init() {
    unsafe {
        // DEH_String("STTMINUS") is identity — just pass the string.
        sttminus = W_CacheLumpName(b"STTMINUS", PU_STATIC) as *mut patch_t;
    }
}

/// Initialize a number widget.
///
/// Stores the position, digit-patch array, value pointer, visibility flag,
/// and field width into `*n`. Sets `oldnum` to 0.
///
/// # Preconditions
/// * `pl` must point to at least 10 valid patch pointers (digits 0-9).
/// * `num` and `on` must be valid non-null pointers for the widget's lifetime.
pub fn STlib_initNum(
    n: *mut st_number_t,
    x: i32,
    y: i32,
    pl: *mut *mut patch_t,
    num: *mut i32,
    on: *mut i32,
    width: i32,
) {
    unsafe {
        (*n).x = x;
        (*n).y = y;
        (*n).oldnum = 0;
        (*n).width = width;
        (*n).num = num;
        (*n).on = on;
        (*n).p = pl;
    }
}

/// Draw a number widget unconditionally.
///
/// Algorithm:
/// 1. Clamp negative values only for `width == 2` (floor at -9) and
///    `width == 3` (floor at -99); other widths are not clamped.
/// 2. Erase the current field by blitting from `st_backing_screen`.
/// 3. If the value equals `1994`, skip rendering (magic "inactive" sentinel).
/// 4. Draw digits right-to-left using `p[digit]` patches.
/// 5. If negative, draw the [`sttminus`] patch 8 pixels left of the field.
///
/// The `_refresh` parameter is accepted for ABI compatibility but is currently
/// unused; erasing and redrawing always happen unconditionally.
/// Called by [`STlib_updateNum`].
///
/// Panics via `i_error!` if the widget's Y position is above the status bar
/// (`n->y - ST_Y < 0`).
pub fn STlib_drawNum(n: *mut st_number_t, _refresh: i32) {
    unsafe {
        let mut numdigits = (*n).width;
        let num = *(*n).num;

        let w = short_swap((**(*n).p).width) as i32;
        let h = short_swap((**(*n).p).height) as i32;
        (*n).oldnum = num;

        let neg = num < 0;
        let mut num = num;

        if neg {
            if numdigits == 2 && num < -9 {
                num = -9;
            } else if numdigits == 3 && num < -99 {
                num = -99;
            }
            num = -num;
        }

        // clear the area
        let mut x = (*n).x - numdigits * w;

        if (*n).y - ST_Y < 0 {
            i_error!("drawNum: n->y - ST_Y < 0");
        }

        V_CopyRect(
            x,
            (*n).y - ST_Y,
            st_backing_screen,
            w * numdigits,
            h,
            x,
            (*n).y,
        );

        // if non-number, do not draw it
        if num == 1994 {
            return;
        }

        x = (*n).x;

        // in the special case of 0, you draw 0
        if num == 0 {
            V_DrawPatch(x - w, (*n).y, *(*n).p);
        }

        // draw the new number
        while num != 0 && numdigits > 0 {
            x -= w;
            V_DrawPatch(x, (*n).y, *(*n).p.offset((num % 10) as isize));
            num /= 10;
            numdigits -= 1;
        }

        // draw a minus sign if necessary
        if neg {
            V_DrawPatch(x - 8, (*n).y, sttminus);
        }
    }
}

/// Update a number widget, redrawing it if the widget is visible.
///
/// Calls [`STlib_drawNum`] when `*n.on != 0`. In the C original this function
/// also checked whether the value had changed before drawing; this port always
/// redraws when visible (matching the `refresh` path).
pub fn STlib_updateNum(n: *mut st_number_t, refresh: i32) {
    unsafe {
        if *(*n).on != 0 {
            STlib_drawNum(n, refresh);
        }
    }
}

/// Initialize a percent widget.
///
/// Calls [`STlib_initNum`] with `width = 3` for the embedded number, then
/// stores the `%` sign patch in `p.p`.
///
/// # Preconditions
/// * `pl` must point to at least 10 valid digit patches.
/// * `num`, `on`, and `percent` must be valid non-null pointers.
pub fn STlib_initPercent(
    p: *mut st_percent_t,
    x: i32,
    y: i32,
    pl: *mut *mut patch_t,
    num: *mut i32,
    on: *mut i32,
    percent: *mut patch_t,
) {
    unsafe {
        STlib_initNum(&mut (*p).n, x, y, pl, num, on, 3);
    }
    unsafe {
        (*p).p = percent;
    }
}

/// Update a percent widget, drawing the `%` patch and updating the number.
///
/// When `refresh != 0` and the widget is visible, draws the `%` patch at the
/// number's position before delegating to [`STlib_updateNum`].
pub fn STlib_updatePercent(per: *mut st_percent_t, refresh: i32) {
    unsafe {
        if refresh != 0 && *(*per).n.on != 0 {
            V_DrawPatch((*per).n.x, (*per).n.y, (*per).p);
        }
        STlib_updateNum(&mut (*per).n, refresh);
    }
}

/// Initialize a multi-icon widget.
///
/// Sets position, patch array, current-index pointer, and visibility flag.
/// `oldinum` is initialized to `-1` so the first draw is unconditional.
///
/// # Preconditions
/// * `inum` and `on` must be valid non-null pointers.
/// * `il` must point to a valid patch array covering all expected index values.
pub fn STlib_initMultIcon(
    i: *mut st_multicon_t,
    x: i32,
    y: i32,
    il: *mut *mut patch_t,
    inum: *mut i32,
    on: *mut i32,
) {
    unsafe {
        (*i).x = x;
        (*i).y = y;
        (*i).oldinum = -1;
        (*i).inum = inum;
        (*i).on = on;
        (*i).p = il;
    }
}

/// Update a multi-icon widget, redrawing if the index changed or refresh is
/// requested.
///
/// When the widget is visible and (`oldinum != *inum` or `refresh != 0`) and
/// `*inum != -1`:
/// * If there was a previous icon (`oldinum != -1`), erases it by blitting its
///   patch area from `st_backing_screen`.
/// * Draws the new icon patch using `V_DrawPatch`.
/// * Updates `oldinum`.
///
/// Skips all work when `*on == 0` or `*inum == -1`.
///
/// Panics via `i_error!` if the widget's Y position is above the status bar
/// (`y - ST_Y < 0`) when erasing a previous icon.
pub fn STlib_updateMultIcon(mi: *mut st_multicon_t, refresh: i32) {
    unsafe {
        if *(*mi).on != 0 && ((*mi).oldinum != *(*mi).inum || refresh != 0) && *(*mi).inum != -1 {
            if (*mi).oldinum != -1 {
                let old_patch = *(*mi).p.offset((*mi).oldinum as isize);
                let x = (*mi).x - short_swap((*old_patch).leftoffset) as i32;
                let y = (*mi).y - short_swap((*old_patch).topoffset) as i32;
                let w = short_swap((*old_patch).width) as i32;
                let h = short_swap((*old_patch).height) as i32;

                if y - ST_Y < 0 {
                    i_error!("updateMultIcon: y - ST_Y < 0");
                }

                V_CopyRect(x, y - ST_Y, st_backing_screen, w, h, x, y);
            }
            V_DrawPatch((*mi).x, (*mi).y, *(*mi).p.offset(*(*mi).inum as isize));
            (*mi).oldinum = *(*mi).inum;
        }
    }
}

/// Initialize a binary icon widget.
///
/// Sets position, icon patch, value pointer, and visibility flag. `oldval` is
/// initialized to 0.
///
/// # Preconditions
/// * `i`, `val`, and `on` must be valid non-null pointers.
pub fn STlib_initBinIcon(
    b: *mut st_binicon_t,
    x: i32,
    y: i32,
    i: *mut patch_t,
    val: *mut i32,
    on: *mut i32,
) {
    unsafe {
        (*b).x = x;
        (*b).y = y;
        (*b).oldval = 0;
        (*b).val = val;
        (*b).on = on;
        (*b).p = i;
    }
}

/// Update a binary icon widget, toggling the patch when the value changes.
///
/// When the widget is visible and (`oldval != *val` or `refresh != 0`):
/// * Computes the patch's top-left corner using its `leftoffset` / `topoffset`.
/// * If `*val != 0`, draws the patch with `V_DrawPatch`.
/// * If `*val == 0`, erases the patch area by blitting from `st_backing_screen`.
/// * Updates `oldval`.
///
/// Skips all work when `*on == 0`.
///
/// Panics via `i_error!` if the widget's Y position is above the status bar
/// (`y - ST_Y < 0`).
pub fn STlib_updateBinIcon(bi: *mut st_binicon_t, refresh: i32) {
    unsafe {
        if *(*bi).on != 0 && ((*bi).oldval != *(*bi).val || refresh != 0) {
            let p = (*bi).p;
            let x = (*bi).x - short_swap((*p).leftoffset) as i32;
            let y = (*bi).y - short_swap((*p).topoffset) as i32;
            let w = short_swap((*p).width) as i32;
            let h = short_swap((*p).height) as i32;

            if y - ST_Y < 0 {
                i_error!("updateBinIcon: y - ST_Y < 0");
            }

            if *(*bi).val != 0 {
                V_DrawPatch((*bi).x, (*bi).y, p);
            } else {
                V_CopyRect(x, y - ST_Y, st_backing_screen, w, h, x, y);
            }

            (*bi).oldval = *(*bi).val;
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, std::sync::Mutex};

    static LOCK: Mutex<()> = Mutex::new(());

    const ST_NUMBER_T_SIZEOF: usize = 48;
    const ST_PERCENT_T_SIZEOF: usize = 56;
    const ST_MULTICON_T_SIZEOF: usize = 48;
    const ST_BINICON_T_SIZEOF: usize = 48;

    #[test]
    fn st_number_t_size_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(
            std::mem::size_of::<st_number_t>(),
            ST_NUMBER_T_SIZEOF,
            "st_number_t size mismatch: Rust={}, expected={}",
            std::mem::size_of::<st_number_t>(),
            ST_NUMBER_T_SIZEOF,
        );
    }

    #[test]
    fn st_percent_t_size_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(
            std::mem::size_of::<st_percent_t>(),
            ST_PERCENT_T_SIZEOF,
            "st_percent_t size mismatch: Rust={}, expected={}",
            std::mem::size_of::<st_percent_t>(),
            ST_PERCENT_T_SIZEOF,
        );
    }

    #[test]
    fn st_multicon_t_size_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(
            std::mem::size_of::<st_multicon_t>(),
            ST_MULTICON_T_SIZEOF,
            "st_multicon_t size mismatch: Rust={}, expected={}",
            std::mem::size_of::<st_multicon_t>(),
            ST_MULTICON_T_SIZEOF,
        );
    }

    #[test]
    fn st_binicon_t_size_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(
            std::mem::size_of::<st_binicon_t>(),
            ST_BINICON_T_SIZEOF,
            "st_binicon_t size mismatch: Rust={}, expected={}",
            std::mem::size_of::<st_binicon_t>(),
            ST_BINICON_T_SIZEOF,
        );
    }
}
