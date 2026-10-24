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
//! * C widgets keep `int*` / `boolean*` pointers to the value they show and
//!   to their visibility flag (fields of the player and of the status
//!   state). Here the `STlib_update*` calls receive the value and the flag
//!   instead, so nothing points into the player slots.
//! * The `SHORT` macro (little-endian swap) is an identity function on x86_64.
//! * C `patch_t**` patch arrays are `*const [Option<Patch<'static>>]`
//!   pointers to the patch tables in the status state, and a single
//!   `patch_t*` is an `Option<Patch<'static>>` copy; `None` is C's null.
//!   The status bar caches its patches at `PU_STATIC` and releases them only
//!   in `ST_unloadGraphics`, which must not run while the widgets draw.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use {
    crate::{
        engine::{
            wad::{cache_patch_name, format::Patch},
            zone::PU_STATIC,
        },
        i_error,
    },
    std::ptr,
};

/// A widget's table of patches: a pointer to one of the patch arrays in the
/// status state, C `patch_t**`.
pub type PatchTable = *const [Option<Patch<'static>>];

/// Patch `index` of `table`. Panics if the index is outside the table or
/// the patch is not loaded, where C would read out of bounds or through a
/// null pointer.
///
/// # Safety
/// `table` must point to a live patch table.
unsafe fn table_patch(table: PatchTable, index: i32) -> Patch<'static> {
    let table = unsafe { &*table };
    table[index as usize].expect("status bar patch used before ST_loadGraphics")
}

/// The patch in `slot`. Panics if it is not loaded, where C would read
/// through a null pointer.
fn loaded_patch(slot: Option<Patch<'static>>) -> Patch<'static> {
    slot.expect("status bar patch used before ST_loadGraphics")
}
/// Height of the status bar in pixels.
/// Mirrors `ST_HEIGHT` from `st_stuff.h`.
const ST_HEIGHT: i32 = 32;

/// Y coordinate of the top of the status bar (screen height minus bar height).
/// All widget Y coordinates are expected to be at or below this value.
/// Mirrors `ST_Y` from `st_stuff.h`.
const ST_Y: i32 = 200 - ST_HEIGHT; // 168

/// A right-justified integer display widget.
///
/// Corresponds to `st_number_t` in `st_lib.h`. Digits are rendered
/// right-to-left using a patch font; a minus sign patch is prepended for
/// negative values. The magic value `1994` means "do not draw" (used when
/// a slot is inactive).
///
/// # Invariants
/// * `p` must point to a table of the loaded digit patches 0-9 when drawing.
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
    /// Array of digit patches; `p[d]` is the patch for digit `d` (0-9).
    pub p: PatchTable,
    /// User-defined auxiliary data (unused by the widget library itself).
    pub data: i32,
}

impl st_number_t {
    /// A number widget before [`STlib_initNum`]: no patches.
    pub const EMPTY: Self = Self {
        x: 0,
        y: 0,
        width: 0,
        oldnum: 0,
        p: ptr::slice_from_raw_parts(ptr::null(), 0),
        data: 0,
    };
}

/// A percentage display widget: a number followed by a `%` sign patch.
///
/// Corresponds to `st_percent_t` in `st_lib.h`. The embedded [`st_number_t`]
/// renders the numeric portion; the additional `p` patch is drawn immediately
/// to the right of the number on each refresh.
#[derive(Clone, Copy)]
pub struct st_percent_t {
    /// The underlying number widget (renders the digits).
    pub n: st_number_t,
    /// The `%` percent-sign patch drawn after the number.
    pub p: Option<Patch<'static>>,
}

impl st_percent_t {
    /// A percent widget before [`STlib_initPercent`].
    pub const EMPTY: Self = Self {
        n: st_number_t::EMPTY,
        p: None,
    };
}

/// A widget that displays one patch selected from an indexed array.
///
/// Corresponds to `st_multicon_t` in `st_lib.h`. Used for key-card slots,
/// player face sprites, and any other status bar element that cycles through
/// a discrete set of images. When the selected index changes, the old patch
/// area is erased by blitting from the backing screen before the new patch is
/// drawn.
///
/// # Invariants
/// * `p` must point to a table holding loaded patches for every index the
///   widget is given.
/// * An index of `-1` means "no image"; the widget skips drawing entirely.
#[derive(Clone, Copy)]
pub struct st_multicon_t {
    /// Center-justified screen X position (patch offset subtracted at draw time).
    pub x: i32,
    /// Center-justified screen Y position (patch offset subtracted at draw time).
    pub y: i32,
    /// Index of the patch displayed in the previous frame; `-1` if no patch was
    /// shown.
    pub oldinum: i32,
    /// Array of icon patches indexed by the icon index.
    pub p: PatchTable,
    /// User-defined auxiliary data (unused by the widget library itself).
    pub data: i32,
}

impl st_multicon_t {
    /// A multi-icon widget before [`STlib_initMultIcon`]: no patches.
    pub const EMPTY: Self = Self {
        x: 0,
        y: 0,
        oldinum: 0,
        p: ptr::slice_from_raw_parts(ptr::null(), 0),
        data: 0,
    };
}

/// A widget that shows a single patch when a boolean flag is set.
///
/// Corresponds to `st_binicon_t` in `st_lib.h`. When the value transitions
/// from zero to non-zero the patch is drawn; when it transitions back to zero
/// the patch area is erased from the backing screen. Used for key-card
/// presence indicators and similar binary status elements.
///
/// # Invariants
/// * `p` must be loaded when the widget draws.
#[derive(Clone, Copy)]
pub struct st_binicon_t {
    /// Center-justified screen X position.
    pub x: i32,
    /// Center-justified screen Y position.
    pub y: i32,
    /// The value drawn in the previous frame.
    pub oldval: i32,
    /// The patch to display when the value is non-zero.
    pub p: Option<Patch<'static>>,
    /// User-defined auxiliary data (unused by the widget library itself).
    pub data: i32,
}

impl st_binicon_t {
    /// A binary icon widget before [`STlib_initBinIcon`].
    pub const EMPTY: Self = Self {
        x: 0,
        y: 0,
        oldval: 0,
        p: None,
        data: 0,
    };
}

use crate::engine::{
    render_2d::{V_CopyRect, V_DrawPatch},
    status::stuff::STATUS,
};

/// The status bar backing buffer the widgets erase from.
fn backing_screen() -> *mut u8 {
    STATUS.borrow().st_backing_screen
}

/// Initialize the status bar widget library.
///
/// Loads the `STTMINUS` WAD lump as `PU_STATIC` and stores it in the status
/// state's `sttminus`, the minus sign drawn to the left of the
/// most-significant digit when a number widget displays a negative value.
/// All other widget state is initialized via the individual `STlib_init*`
/// functions. Called from `ST_initData` in `st_stuff.c`.
pub fn STlib_init() {
    // DEH_String("STTMINUS") is identity — just pass the string.
    // SAFETY: STTMINUS is cached at PU_STATIC and never released.
    let sttminus = unsafe { cache_patch_name(b"STTMINUS", PU_STATIC) };
    STATUS.borrow_mut().sttminus = Some(sttminus);
}

/// Initialize a number widget.
///
/// Stores the position, digit-patch array and field width into `n`. Sets
/// `oldnum` to 0. `pl` must point to a table of the 10 digit patches (0-9)
/// whenever the widget draws.
pub fn STlib_initNum(n: &mut st_number_t, x: i32, y: i32, pl: PatchTable, width: i32) {
    n.x = x;
    n.y = y;
    n.oldnum = 0;
    n.width = width;
    n.p = pl;
}

/// Draw a number widget showing `num` unconditionally.
///
/// Algorithm:
/// 1. Clamp negative values only for `width == 2` (floor at -9) and
///    `width == 3` (floor at -99); other widths are not clamped.
/// 2. Erase the current field by blitting from `st_backing_screen`.
/// 3. If the value equals `1994`, skip rendering (magic "inactive" sentinel).
/// 4. Draw digits right-to-left using `p[digit]` patches.
/// 5. If negative, draw the `sttminus` patch 8 pixels left of the field.
///
/// The `_refresh` parameter is accepted for ABI compatibility but is currently
/// unused; erasing and redrawing always happen unconditionally.
/// Called by [`STlib_updateNum`].
///
/// Panics via `i_error!` if the widget's Y position is above the status bar
/// (`n->y - ST_Y < 0`).
///
/// # Safety
/// `n` must be a widget set up by [`STlib_initNum`], whose patch table is
/// live.
pub unsafe fn STlib_drawNum(n: &mut st_number_t, num: i32, _refresh: i32) {
    unsafe {
        let mut numdigits = n.width;

        let digit_zero = table_patch(n.p, 0);
        let w = digit_zero.width();
        let h = digit_zero.height();
        n.oldnum = num;

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
        let mut x = n.x - numdigits * w;

        if n.y - ST_Y < 0 {
            i_error!("drawNum: n->y - ST_Y < 0");
        }

        V_CopyRect(x, n.y - ST_Y, backing_screen(), w * numdigits, h, x, n.y);

        // if non-number, do not draw it
        if num == 1994 {
            return;
        }

        x = n.x;

        // in the special case of 0, you draw 0
        if num == 0 {
            V_DrawPatch(x - w, n.y, digit_zero);
        }

        // draw the new number
        while num != 0 && numdigits > 0 {
            x -= w;
            V_DrawPatch(x, n.y, table_patch(n.p, num % 10));
            num /= 10;
            numdigits -= 1;
        }

        // draw a minus sign if necessary
        if neg {
            let sttminus = STATUS.borrow().sttminus;
            V_DrawPatch(x - 8, n.y, loaded_patch(sttminus));
        }
    }
}

/// Update a number widget showing `num`, redrawing it if the widget is
/// visible (`on`).
///
/// Calls [`STlib_drawNum`] when `on`. In the C original this function
/// also checked whether the value had changed before drawing; this port always
/// redraws when visible (matching the `refresh` path).
///
/// # Safety
/// `n` must be a widget set up by [`STlib_initNum`], whose patch table is
/// live.
pub unsafe fn STlib_updateNum(n: &mut st_number_t, num: i32, on: bool, refresh: i32) {
    unsafe {
        if on {
            STlib_drawNum(n, num, refresh);
        }
    }
}

/// Initialize a percent widget.
///
/// Calls [`STlib_initNum`] with `width = 3` for the embedded number, then
/// stores the `%` sign patch in `p.p`. `pl` must point to a table of the 10
/// digit patches whenever the widget draws.
pub fn STlib_initPercent(
    p: &mut st_percent_t,
    x: i32,
    y: i32,
    pl: PatchTable,
    percent: Option<Patch<'static>>,
) {
    STlib_initNum(&mut p.n, x, y, pl, 3);
    p.p = percent;
}

/// Update a percent widget showing `num`, drawing the `%` patch and updating
/// the number.
///
/// When `refresh != 0` and the widget is visible (`on`), draws the `%` patch
/// at the number's position before delegating to [`STlib_updateNum`].
///
/// # Safety
/// `per` must be a widget set up by [`STlib_initPercent`], whose patch table
/// is live.
pub unsafe fn STlib_updatePercent(per: &mut st_percent_t, num: i32, on: bool, refresh: i32) {
    unsafe {
        if refresh != 0 && on {
            V_DrawPatch(per.n.x, per.n.y, loaded_patch(per.p));
        }
        STlib_updateNum(&mut per.n, num, on, refresh);
    }
}

/// Initialize a multi-icon widget.
///
/// Sets position and patch array. `oldinum` is initialized to `-1` so the
/// first draw is unconditional. `il` must point to a patch table covering
/// every index the widget is given whenever it draws.
pub fn STlib_initMultIcon(i: &mut st_multicon_t, x: i32, y: i32, il: PatchTable) {
    i.x = x;
    i.y = y;
    i.oldinum = -1;
    i.p = il;
}

/// Update a multi-icon widget showing icon `inum`, redrawing if the index
/// changed or refresh is requested.
///
/// When the widget is visible (`on`) and (`oldinum != inum` or
/// `refresh != 0`) and `inum != -1`:
/// * If there was a previous icon (`oldinum != -1`), erases it by blitting its
///   patch area from `st_backing_screen`.
/// * Draws the new icon patch using `V_DrawPatch`.
/// * Updates `oldinum`.
///
/// Skips all work when not `on` or `inum == -1`.
///
/// Panics via `i_error!` if the widget's Y position is above the status bar
/// (`y - ST_Y < 0`) when erasing a previous icon.
///
/// # Safety
/// `mi` must be a widget set up by [`STlib_initMultIcon`], whose patch table
/// is live.
pub unsafe fn STlib_updateMultIcon(mi: &mut st_multicon_t, inum: i32, on: bool, refresh: i32) {
    unsafe {
        if on && (mi.oldinum != inum || refresh != 0) && inum != -1 {
            if mi.oldinum != -1 {
                let old_patch = table_patch(mi.p, mi.oldinum);
                let x = mi.x - old_patch.leftoffset();
                let y = mi.y - old_patch.topoffset();
                let w = old_patch.width();
                let h = old_patch.height();

                if y - ST_Y < 0 {
                    i_error!("updateMultIcon: y - ST_Y < 0");
                }

                V_CopyRect(x, y - ST_Y, backing_screen(), w, h, x, y);
            }
            V_DrawPatch(mi.x, mi.y, table_patch(mi.p, inum));
            mi.oldinum = inum;
        }
    }
}

/// Initialize a binary icon widget.
///
/// Sets position and icon patch. `oldval` is initialized to 0.
pub fn STlib_initBinIcon(b: &mut st_binicon_t, x: i32, y: i32, i: Option<Patch<'static>>) {
    b.x = x;
    b.y = y;
    b.oldval = 0;
    b.p = i;
}

/// Update a binary icon widget showing `val`, toggling the patch when the
/// value changes.
///
/// When the widget is visible (`on`) and (`oldval != val` or
/// `refresh != 0`):
/// * Computes the patch's top-left corner using its `leftoffset` / `topoffset`.
/// * If `val != 0`, draws the patch with `V_DrawPatch`.
/// * If `val == 0`, erases the patch area by blitting from `st_backing_screen`.
/// * Updates `oldval`.
///
/// Skips all work when not `on`.
///
/// Panics via `i_error!` if the widget's Y position is above the status bar
/// (`y - ST_Y < 0`).
///
/// # Safety
/// The status bar's backing screen must be allocated.
pub unsafe fn STlib_updateBinIcon(bi: &mut st_binicon_t, val: i32, on: bool, refresh: i32) {
    unsafe {
        if on && (bi.oldval != val || refresh != 0) {
            let p = loaded_patch(bi.p);
            let x = bi.x - p.leftoffset();
            let y = bi.y - p.topoffset();
            let w = p.width();
            let h = p.height();

            if y - ST_Y < 0 {
                i_error!("updateBinIcon: y - ST_Y < 0");
            }

            if val != 0 {
                V_DrawPatch(bi.x, bi.y, p);
            } else {
                V_CopyRect(x, y - ST_Y, backing_screen(), w, h, x, y);
            }

            bi.oldval = val;
        }
    }
}
