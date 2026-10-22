//! Rust port of vendor/doomgeneric/hu_lib.c.
//!
//! Heads-up text and input code: text lines, scrolling text, and input widgets.
//!
//! Three widget types are provided, mirroring the C originals:
//! * [`hu_textline_t`] - a single line of patch-font text drawn at a fixed
//!   screen position; parent type for the other two.
//! * [`hu_stext_t`] - a scrolling message widget backed by a ring of up to
//!   `HU_MAXLINES` (4) text lines.
//! * [`hu_itext_t`] - a text-input widget with a protected left-margin prefix
//!   (used for chat entry and cheat codes).
//!
//! Rust differences from C: `boolean` is `c_int` (0/1); padding fields have
//! been added to maintain identical ABI layout on 64-bit targets (verified by
//! compile-time `assert!` in `layout_checks`).

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::{c_char, c_int, c_uint};

use crate::doom::v_video::patch_t;

use crate::doom::i_video::SCREENWIDTH;

/// Maximum number of text lines in a scrolling text widget (`hu_stext_t`).
/// Mirrors the C constant `HU_MAXLINES` from `hu_lib.h`.
const HU_MAXLINES: usize = 4;

/// Maximum number of characters per text line, excluding the NUL terminator.
/// Mirrors the C constant `HU_MAXLINELENGTH` from `hu_lib.h`.
const HU_MAXLINELENGTH: usize = 80;

/// A single line of text rendered with a patch font.
///
/// Corresponds to `hu_textline_t` in `hu_lib.h`. All other HUD text widgets
/// embed or inherit this struct. The layout is identical to the C struct on
/// 64-bit targets (verified by `layout_checks`).
///
/// # Layout invariants
/// * `l[len] == 0` at all times (NUL-terminated prefix).
/// * `len <= HU_MAXLINELENGTH` (the array has `HU_MAXLINELENGTH + 1` elements).
/// * `f` points into the `hu_font` array and must not be null when drawing.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct hu_textline_t {
    /// Left-justified screen X position of the text line.
    pub x: c_int,
    /// Screen Y position of the text line.
    pub y: c_int,
    /// Pointer to the start of the font patch array; `f.add(c - sc)` yields a
    /// pointer to the patch for character `c`. Mirrors `patch_t **f` in
    /// `hu_textline_t`. Access requires pointer arithmetic - direct indexing is
    /// unsafe.
    pub f: *mut *mut patch_t,
    /// ASCII code of the first character in the font array (`l->sc` in C).
    /// Characters below this value are rendered as spaces.
    pub sc: c_int,
    /// NUL-terminated text buffer; `l[0..len]` holds the visible characters.
    pub l: [c_char; HU_MAXLINELENGTH + 1],
    /// Number of valid characters currently in `l` (excludes the NUL terminator).
    pub len: c_int,
    /// Dirty-flag countdown: non-zero means the line must be redrawn / erased.
    /// Set to 4 on modification, decremented by `HUlib_eraseTextLine` each
    /// frame until it reaches 0.
    pub needsupdate: c_int,
}

/// A scrolling message widget backed by a ring of text lines.
///
/// Corresponds to `hu_stext_t` in `hu_lib.h`. Lines are stored in a circular
/// buffer; `cl` is the index of the most-recently-added line, and older lines
/// are at `(cl - i + h) % h` for `i in 1..h`.
///
/// # Layout invariants
/// * `h <= HU_MAXLINES`.
/// * `cl` is always in `0..h`.
/// * `on` is a non-null pointer to a `c_int` flag; 0 = hidden, non-zero = visible.
/// * The four-byte `_pad` field exists solely to match the C ABI on 64-bit
///   platforms where `boolean` following a pointer leaves a gap.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct hu_stext_t {
    /// Ring of text lines; only `l[0..h]` are used.
    pub l: [hu_textline_t; HU_MAXLINES],
    /// Height of the widget in lines (`s->h` in C).
    pub h: c_int,
    /// Index of the current (most recently written) line in `l` (`s->cl` in C).
    pub cl: c_int,
    /// Pointer to the visibility flag; widget is drawn only when `*on != 0`.
    pub on: *mut c_int,
    /// Cached value of `*on` from the previous frame, used to detect
    /// transitions from visible to hidden so dirty flags can be set.
    pub laston: c_int,
    _pad: [u8; 4],
}

/// A text-input widget with a protected prefix region.
///
/// Corresponds to `hu_itext_t` in `hu_lib.h`. Used for chat entry and
/// (conceptually) cheat-code input. The first `lm` characters of `l` form
/// an immutable prefix set by [`HUlib_addPrefixToIText`]; delete operations
/// via [`HUlib_delCharFromIText`] and [`HUlib_eraseLineFromIText`] refuse to
/// go past this left margin.
///
/// # Layout invariants
/// * `lm <= l.len` at all times.
/// * `on` must be a non-null pointer.
/// * `_pad0` and `_pad1` are ABI-alignment fillers only.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct hu_itext_t {
    /// The underlying text line that receives key input.
    pub l: hu_textline_t,
    /// Left-margin character count: characters at indices `0..lm` are
    /// protected from deletion (`it->lm` in C).
    pub lm: c_int,
    _pad0: [u8; 4],
    /// Pointer to the visibility flag; widget is drawn only when `*on != 0`.
    pub on: *mut c_int,
    /// Cached value of `*on` from the previous frame (see [`hu_stext_t::laston`]).
    pub laston: c_int,
    _pad1: [u8; 4],
}

#[cfg(target_pointer_width = "64")]
mod layout_checks {
    use super::*;
    const _: () = assert!(std::mem::size_of::<hu_textline_t>() == 112);
    const _: () = assert!(std::mem::offset_of!(hu_textline_t, x) == 0);
    const _: () = assert!(std::mem::offset_of!(hu_textline_t, y) == 4);
    const _: () = assert!(std::mem::offset_of!(hu_textline_t, f) == 8);
    const _: () = assert!(std::mem::offset_of!(hu_textline_t, sc) == 16);
    const _: () = assert!(std::mem::offset_of!(hu_textline_t, l) == 20);
    const _: () = assert!(std::mem::offset_of!(hu_textline_t, len) == 104);
    const _: () = assert!(std::mem::offset_of!(hu_textline_t, needsupdate) == 108);

    const _: () = assert!(std::mem::size_of::<hu_stext_t>() == 472);
    const _: () = assert!(std::mem::offset_of!(hu_stext_t, l) == 0);
    const _: () = assert!(std::mem::offset_of!(hu_stext_t, h) == 448);
    const _: () = assert!(std::mem::offset_of!(hu_stext_t, cl) == 452);
    const _: () = assert!(std::mem::offset_of!(hu_stext_t, on) == 456);
    const _: () = assert!(std::mem::offset_of!(hu_stext_t, laston) == 464);

    const _: () = assert!(std::mem::size_of::<hu_itext_t>() == 136);
    const _: () = assert!(std::mem::offset_of!(hu_itext_t, l) == 0);
    const _: () = assert!(std::mem::offset_of!(hu_itext_t, lm) == 112);
    const _: () = assert!(std::mem::offset_of!(hu_itext_t, on) == 120);
    const _: () = assert!(std::mem::offset_of!(hu_itext_t, laston) == 128);
}

extern "C" {
    /// Draw a patch at (`x`, `y`) directly to the screen buffer (v_video.c).
    fn V_DrawPatchDirect(x: c_int, y: c_int, patch: *mut patch_t);
    /// Erase `count` bytes of the screen buffer starting at byte offset `ofs`
    /// by copying from the background buffer (r_draw.c).
    fn R_VideoErase(ofs: c_uint, count: c_int);
    /// Non-zero while the automap overlay is active (am_map.c).
    static mut automapactive: c_int;
    /// X offset of the rendered view within the full screen (r_main.c).
    static mut viewwindowx: c_int;
    /// Y offset of the rendered view within the full screen (r_main.c).
    static mut viewwindowy: c_int;
    /// Width of the rendered view in pixels (r_main.c).
    static mut viewwidth: c_int;
    /// Height of the rendered view in pixels (r_main.c).
    static mut viewheight: c_int;
}

/// Byte-swap for little-endian (SHORT macro from i_swap.h).
#[inline(always)]
fn short_swap(v: i16) -> i16 {
    v
}

/// Initialize the heads-up widget library (no-op in this port, matching C).
///
/// Called once at startup from `HU_Init`. The C original also had no body.
#[no_mangle]
pub extern "C" fn HUlib_init() {}

/// Reset a text line to empty, marking it for redisplay.
///
/// Sets `len` to 0, NUL-terminates `l[0]`, and sets `needsupdate` to 1.
/// Called from `HU_Init`, `HUlib_addLineToSText`, and `HUlib_resetIText`.
#[no_mangle]
pub extern "C" fn HUlib_clearTextLine(t: *mut hu_textline_t) {
    unsafe {
        (*t).len = 0;
        (*t).l[0] = 0;
        (*t).needsupdate = 1;
    }
}

/// Initialize a text line widget with its screen position and font.
///
/// Sets the position (`x`, `y`), font pointer `f`, and start character `sc`,
/// then calls [`HUlib_clearTextLine`] to zero the text buffer.
///
/// # Preconditions
/// * `t` must be a valid non-null pointer.
/// * `f` must point to a valid array of at least `('_' - sc + 1)` patch
///   pointers when drawing is later requested.
#[no_mangle]
pub extern "C" fn HUlib_initTextLine(
    t: *mut hu_textline_t,
    x: c_int,
    y: c_int,
    f: *mut *mut patch_t,
    sc: c_int,
) {
    unsafe {
        (*t).x = x;
        (*t).y = y;
        (*t).f = f;
        (*t).sc = sc;
        HUlib_clearTextLine(t);
    }
}

/// Append a character to a text line.
///
/// Returns 1 (true) on success, 0 (false) if the line is already at
/// `HU_MAXLINELENGTH` (80) characters. On success, sets `needsupdate` to 4.
/// The buffer remains NUL-terminated after the call.
#[no_mangle]
pub extern "C" fn HUlib_addCharToTextLine(t: *mut hu_textline_t, ch: c_char) -> c_int {
    unsafe {
        if (*t).len == HU_MAXLINELENGTH as c_int {
            0
        } else {
            (*t).l[(*t).len as usize] = ch;
            (*t).len += 1;
            (*t).l[(*t).len as usize] = 0;
            (*t).needsupdate = 4;
            1
        }
    }
}

/// Delete the last character from a text line.
///
/// Returns 1 (true) on success, 0 (false) if the line is already empty.
/// On success, NUL-terminates the new end and sets `needsupdate` to 4.
#[no_mangle]
pub extern "C" fn HUlib_delCharFromTextLine(t: *mut hu_textline_t) -> c_int {
    unsafe {
        if (*t).len == 0 {
            0
        } else {
            (*t).len -= 1;
            (*t).l[(*t).len as usize] = 0;
            (*t).needsupdate = 4;
            1
        }
    }
}

/// Render a text line to the screen using its patch font.
///
/// Iterates over `l[0..len]`, uppercases each character, and calls
/// `V_DrawPatchDirect` for characters in the range `[sc, '_']`; spaces and
/// out-of-range characters advance the x cursor by 4 pixels. Rendering stops
/// early if the cursor would exceed `SCREENWIDTH`.
///
/// If `drawcursor` is non-zero, the `'_'` patch is drawn at the current
/// position (provided it fits), giving a text-entry cursor appearance.
///
/// Called from `HU_Drawer` and `HUlib_drawSText`/`HUlib_drawIText`.
#[no_mangle]
pub extern "C" fn HUlib_drawTextLine(l: *mut hu_textline_t, drawcursor: c_int) {
    unsafe {
        let mut x = (*l).x;
        let f = (*l).f;
        let sc = (*l).sc;

        for i in 0..(*l).len as usize {
            let c = ((*l).l[i] as u8).to_ascii_uppercase();
            if c != b' ' && c >= sc as u8 && c <= b'_' {
                let patch = *f.add((c as c_int - sc) as usize);
                let w = short_swap((*patch).width) as c_int;
                if x + w > SCREENWIDTH {
                    break;
                }
                V_DrawPatchDirect(x, (*l).y, patch);
                x += w;
            } else {
                x += 4;
                if x >= SCREENWIDTH {
                    break;
                }
            }
        }

        if drawcursor != 0 {
            let cursor_patch = *f.add((b'_' as c_int - sc) as usize);
            let cursor_w = short_swap((*cursor_patch).width) as c_int;
            if x + cursor_w <= SCREENWIDTH {
                V_DrawPatchDirect(x, (*l).y, cursor_patch);
            }
        }
    }
}

/// Erase the screen region occupied by a text line and decrement the dirty
/// counter.
///
/// Erasing only occurs when the automap is inactive and the view window is
/// reduced (`viewwindowx != 0`). For each scanline in the font-height range,
/// the function erases either the full line (if it is outside the view window)
/// or the left and right border strips (if it falls within the view window).
/// `needsupdate` is decremented by 1 each call (never below 0), so the line
/// stays "dirty" for up to 4 frames after a change.
///
/// Called from `HU_Erase`, `HUlib_eraseSText`, and `HUlib_eraseIText`.
#[no_mangle]
pub extern "C" fn HUlib_eraseTextLine(l: *mut hu_textline_t) {
    unsafe {
        if automapactive == 0 && viewwindowx != 0 && (*l).needsupdate != 0 {
            let lh = short_swap((**(*l).f).height) as c_int + 1;
            for y in (*l).y..(*l).y + lh {
                let yoffset = y * SCREENWIDTH;
                if y < viewwindowy || y >= viewwindowy + viewheight {
                    R_VideoErase(yoffset as c_uint, SCREENWIDTH);
                } else {
                    R_VideoErase(yoffset as c_uint, viewwindowx);
                    R_VideoErase((yoffset + viewwindowx + viewwidth) as c_uint, viewwindowx);
                }
            }
        }

        if (*l).needsupdate != 0 {
            (*l).needsupdate -= 1;
        }
    }
}

/// Initialize a scrolling text widget.
///
/// Sets height `h`, visibility pointer `on`, resets the current-line index
/// to 0, and initializes each of the `h` text lines. Lines are stacked
/// upward: line 0 is at `y`, line 1 at `y - font_height - 1`, and so on.
///
/// # Preconditions
/// * `h <= HU_MAXLINES`.
/// * `font` must point to valid patch data so the font height can be read.
/// * `on` must be a valid non-null pointer for the widget's lifetime.
#[no_mangle]
pub extern "C" fn HUlib_initSText(
    s: *mut hu_stext_t,
    x: c_int,
    y: c_int,
    h: c_int,
    font: *mut *mut patch_t,
    startchar: c_int,
    on: *mut c_int,
) {
    unsafe {
        (*s).h = h;
        (*s).on = on;
        (*s).laston = 1;
        (*s).cl = 0;
        let font_h = short_swap((**font).height) as c_int + 1;
        for i in 0..h as usize {
            HUlib_initTextLine(
                &mut (*s).l[i],
                x,
                y - (i as c_int) * font_h,
                font,
                startchar,
            );
        }
    }
}

/// Advance the ring-buffer cursor and clear the new current line.
///
/// Increments `cl` modulo `h`, clears the new current text line, and sets
/// `needsupdate` to 4 on every line so they are all redrawn.
#[no_mangle]
pub extern "C" fn HUlib_addLineToSText(s: *mut hu_stext_t) {
    unsafe {
        (*s).cl += 1;
        if (*s).cl == (*s).h {
            (*s).cl = 0;
        }
        HUlib_clearTextLine(&mut (*s).l[(*s).cl as usize]);

        for i in 0..(*s).h as usize {
            (*s).l[i].needsupdate = 4;
        }
    }
}

/// Append a message (with optional prefix) to a scrolling text widget.
///
/// Calls [`HUlib_addLineToSText`] to advance the ring buffer, then appends
/// each character of `prefix` (if non-null) followed by each character of
/// `msg` to the current line using [`HUlib_addCharToTextLine`].
///
/// # Preconditions
/// * `msg` must be a valid NUL-terminated C string.
/// * `prefix` may be null; if non-null it must also be NUL-terminated.
#[no_mangle]
pub extern "C" fn HUlib_addMessageToSText(
    s: *mut hu_stext_t,
    prefix: *mut c_char,
    msg: *mut c_char,
) {
    unsafe {
        HUlib_addLineToSText(s);
        if !prefix.is_null() {
            let mut p = prefix;
            while *p != 0 {
                HUlib_addCharToTextLine(&mut (*s).l[(*s).cl as usize], *p);
                p = p.add(1);
            }
        }
        let mut m = msg;
        while *m != 0 {
            HUlib_addCharToTextLine(&mut (*s).l[(*s).cl as usize], *m);
            m = m.add(1);
        }
    }
}

/// Render all lines of a scrolling text widget.
///
/// Skips drawing if `*s.on == 0`. Otherwise iterates `h` lines in ring order
/// (newest first) and calls [`HUlib_drawTextLine`] for each without a cursor.
#[no_mangle]
pub extern "C" fn HUlib_drawSText(s: *mut hu_stext_t) {
    unsafe {
        if *(*s).on == 0 {
            return;
        }
        for i in 0..(*s).h as usize {
            let mut idx = (*s).cl as isize - i as isize;
            if idx < 0 {
                idx += (*s).h as isize;
            }
            HUlib_drawTextLine(&mut (*s).l[idx as usize], 0);
        }
    }
}

/// Erase all lines of a scrolling text widget and update the visibility cache.
///
/// If the widget just transitioned from visible to hidden (`laston != 0` and
/// `*on == 0`), marks every line dirty so they are erased from the screen.
/// Then calls [`HUlib_eraseTextLine`] on each line and updates `laston`.
#[no_mangle]
pub extern "C" fn HUlib_eraseSText(s: *mut hu_stext_t) {
    unsafe {
        for i in 0..(*s).h as usize {
            if (*s).laston != 0 && *(*s).on == 0 {
                (*s).l[i].needsupdate = 4;
            }
            HUlib_eraseTextLine(&mut (*s).l[i]);
        }
        (*s).laston = if *(*s).on != 0 { 1 } else { 0 };
    }
}

/// Initialize a text-input widget.
///
/// Zeroes the left margin (`lm = 0`), stores the visibility pointer, sets
/// `laston = 1`, and initialises the underlying text line via
/// [`HUlib_initTextLine`].
///
/// # Preconditions
/// * `on` must be a valid non-null pointer for the widget's lifetime.
#[no_mangle]
pub extern "C" fn HUlib_initIText(
    it: *mut hu_itext_t,
    x: c_int,
    y: c_int,
    font: *mut *mut patch_t,
    startchar: c_int,
    on: *mut c_int,
) {
    unsafe {
        (*it).lm = 0;
        (*it).on = on;
        (*it).laston = 1;
        HUlib_initTextLine(&mut (*it).l, x, y, font, startchar);
    }
}

/// Delete the last character from an input widget, respecting the left margin.
///
/// Calls [`HUlib_delCharFromTextLine`] only when `l.len > lm`; characters
/// within the protected prefix are never removed.
#[no_mangle]
pub extern "C" fn HUlib_delCharFromIText(it: *mut hu_itext_t) {
    unsafe {
        if (*it).l.len != (*it).lm {
            HUlib_delCharFromTextLine(&mut (*it).l);
        }
    }
}

/// Delete all user-entered characters from an input widget, stopping at the
/// left margin.
///
/// Repeatedly calls [`HUlib_delCharFromTextLine`] until `l.len == lm`,
/// effectively clearing everything after the prefix.
#[no_mangle]
pub extern "C" fn HUlib_eraseLineFromIText(it: *mut hu_itext_t) {
    unsafe {
        while (*it).lm != (*it).l.len {
            HUlib_delCharFromTextLine(&mut (*it).l);
        }
    }
}

/// Reset an input widget to a fully empty state, including the prefix.
///
/// Sets `lm` to 0 and calls [`HUlib_clearTextLine`], discarding both the
/// prefix and any user input. Used at the start of a new chat session.
#[no_mangle]
pub extern "C" fn HUlib_resetIText(it: *mut hu_itext_t) {
    unsafe {
        (*it).lm = 0;
        HUlib_clearTextLine(&mut (*it).l);
    }
}

/// Append a prefix string to an input widget and lock it as the left margin.
///
/// Appends each byte of the NUL-terminated `str` to the underlying text line,
/// then sets `lm = l.len` so that subsequent delete operations cannot remove
/// those characters.
///
/// # Preconditions
/// * `str` must be a valid NUL-terminated C string.
#[no_mangle]
pub extern "C" fn HUlib_addPrefixToIText(it: *mut hu_itext_t, str: *mut c_char) {
    unsafe {
        let mut p = str;
        while *p != 0 {
            HUlib_addCharToTextLine(&mut (*it).l, *p);
            p = p.add(1);
        }
        (*it).lm = (*it).l.len;
    }
}

/// Process a keypress for an input text widget.
///
/// Uppercases `ch` before dispatch:
/// * Printable range `[' ', '_']`: appended via [`HUlib_addCharToTextLine`].
/// * `KEY_BACKSPACE`: deletes via [`HUlib_delCharFromIText`] (honours margin).
/// * `KEY_ENTER`: accepted as a terminator (no text change).
/// * Any other value: returns 0 to signal the key was not consumed.
///
/// Returns 1 if the key was consumed, 0 otherwise.
#[no_mangle]
pub extern "C" fn HUlib_keyInIText(it: *mut hu_itext_t, ch: u8) -> c_int {
    unsafe {
        let ch = ch.to_ascii_uppercase();
        if (b' '..=b'_').contains(&ch) {
            HUlib_addCharToTextLine(&mut (*it).l, ch as c_char);
        } else if ch == crate::doom::doomkeys::KEY_BACKSPACE {
            HUlib_delCharFromIText(it);
        } else if ch != crate::doom::doomkeys::KEY_ENTER {
            return 0;
        }
        1
    }
}

/// Render the input text widget, including the cursor glyph.
///
/// Skips drawing if `*it.on == 0`. Otherwise delegates to
/// [`HUlib_drawTextLine`] with `drawcursor = 1`.
#[no_mangle]
pub extern "C" fn HUlib_drawIText(it: *mut hu_itext_t) {
    unsafe {
        if *(*it).on == 0 {
            return;
        }
        HUlib_drawTextLine(&mut (*it).l, 1);
    }
}

/// Erase the input text widget from the screen and update the visibility cache.
///
/// Marks the line dirty if the widget just became hidden (transition from
/// `laston != 0` to `*on == 0`), then calls [`HUlib_eraseTextLine`] and
/// updates `laston`.
#[no_mangle]
pub extern "C" fn HUlib_eraseIText(it: *mut hu_itext_t) {
    unsafe {
        if (*it).laston != 0 && *(*it).on == 0 {
            (*it).l.needsupdate = 4;
        }
        HUlib_eraseTextLine(&mut (*it).l);
        (*it).laston = if *(*it).on != 0 { 1 } else { 0 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    const HU_TEXTLINE_T_SIZEOF: usize = 112;
    const HU_STEXT_T_SIZEOF: usize = 472;
    const HU_ITEXT_T_SIZEOF: usize = 136;

    #[test]
    fn hu_textline_t_layout_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(std::mem::size_of::<hu_textline_t>(), HU_TEXTLINE_T_SIZEOF);
        assert_eq!(std::mem::offset_of!(hu_textline_t, x), 0);
        assert_eq!(std::mem::offset_of!(hu_textline_t, y), 4);
        assert_eq!(std::mem::offset_of!(hu_textline_t, f), 8);
        assert_eq!(std::mem::offset_of!(hu_textline_t, sc), 16);
        assert_eq!(std::mem::offset_of!(hu_textline_t, l), 20);
        assert_eq!(std::mem::offset_of!(hu_textline_t, len), 104);
        assert_eq!(std::mem::offset_of!(hu_textline_t, needsupdate), 108);
    }

    #[test]
    fn hu_stext_t_layout_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(std::mem::size_of::<hu_stext_t>(), HU_STEXT_T_SIZEOF);
        assert_eq!(std::mem::offset_of!(hu_stext_t, l), 0);
        assert_eq!(std::mem::offset_of!(hu_stext_t, h), 448);
        assert_eq!(std::mem::offset_of!(hu_stext_t, cl), 452);
        assert_eq!(std::mem::offset_of!(hu_stext_t, on), 456);
        assert_eq!(std::mem::offset_of!(hu_stext_t, laston), 464);
    }

    #[test]
    fn hu_itext_t_layout_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(std::mem::size_of::<hu_itext_t>(), HU_ITEXT_T_SIZEOF);
        assert_eq!(std::mem::offset_of!(hu_itext_t, l), 0);
        assert_eq!(std::mem::offset_of!(hu_itext_t, lm), 112);
        assert_eq!(std::mem::offset_of!(hu_itext_t, on), 120);
        assert_eq!(std::mem::offset_of!(hu_itext_t, laston), 128);
    }

    /// Build a zero-initialised hu_textline_t without a valid font pointer.
    /// Safe to use with clear/add/del because those don't dereference `f`.
    fn make_textline() -> hu_textline_t {
        hu_textline_t {
            x: 0,
            y: 0,
            f: std::ptr::null_mut(),
            sc: 0,
            l: [0; HU_MAXLINELENGTH + 1],
            len: 0,
            needsupdate: 0,
        }
    }

    #[test]
    fn clear_text_line_resets_fields() {
        let _g = LOCK.lock().unwrap();
        let mut tl = make_textline();
        tl.len = 5;
        tl.needsupdate = 0;
        tl.l[0] = b'X' as c_char;

        HUlib_clearTextLine(&mut tl);

        assert_eq!(tl.len, 0, "len must be reset to 0");
        assert_eq!(tl.l[0], 0, "first char must be nul after clear");
        assert_eq!(tl.needsupdate, 1, "needsupdate must be set to 1");
    }

    #[test]
    fn add_char_increments_len_and_nul_terminates() {
        let _g = LOCK.lock().unwrap();
        let mut tl = make_textline();
        HUlib_clearTextLine(&mut tl);

        let ret = HUlib_addCharToTextLine(&mut tl, b'A' as c_char);

        assert_eq!(ret, 1, "successful add must return 1");
        assert_eq!(tl.len, 1);
        assert_eq!(tl.l[0], b'A' as c_char);
        assert_eq!(tl.l[1], 0, "character after the last must be nul");
        assert_eq!(tl.needsupdate, 4);
    }

    #[test]
    fn add_multiple_chars_builds_string() {
        let _g = LOCK.lock().unwrap();
        let mut tl = make_textline();
        HUlib_clearTextLine(&mut tl);

        for ch in b"HI" {
            HUlib_addCharToTextLine(&mut tl, *ch as c_char);
        }
        assert_eq!(tl.len, 2);
        assert_eq!(tl.l[0], b'H' as c_char);
        assert_eq!(tl.l[1], b'I' as c_char);
        assert_eq!(tl.l[2], 0);
    }

    #[test]
    fn add_char_at_max_capacity_returns_zero() {
        let _g = LOCK.lock().unwrap();
        let mut tl = make_textline();
        HUlib_clearTextLine(&mut tl);

        // Fill to HU_MAXLINELENGTH
        for _ in 0..HU_MAXLINELENGTH {
            HUlib_addCharToTextLine(&mut tl, b'X' as c_char);
        }
        assert_eq!(tl.len, HU_MAXLINELENGTH as c_int);

        // One more must be rejected
        let ret = HUlib_addCharToTextLine(&mut tl, b'Y' as c_char);
        assert_eq!(ret, 0, "add beyond max length must return 0");
        assert_eq!(tl.len, HU_MAXLINELENGTH as c_int, "len must not change");
    }

    #[test]
    fn del_char_decrements_len_and_nul_terminates() {
        let _g = LOCK.lock().unwrap();
        let mut tl = make_textline();
        HUlib_clearTextLine(&mut tl);
        HUlib_addCharToTextLine(&mut tl, b'A' as c_char);
        HUlib_addCharToTextLine(&mut tl, b'B' as c_char);

        let ret = HUlib_delCharFromTextLine(&mut tl);

        assert_eq!(ret, 1, "successful delete must return 1");
        assert_eq!(tl.len, 1);
        assert_eq!(tl.l[1], 0, "position after new end must be nul");
        assert_eq!(tl.needsupdate, 4);
    }

    #[test]
    fn del_char_on_empty_line_returns_zero() {
        let _g = LOCK.lock().unwrap();
        let mut tl = make_textline();
        HUlib_clearTextLine(&mut tl);

        let ret = HUlib_delCharFromTextLine(&mut tl);
        assert_eq!(ret, 0, "delete on empty line must return 0");
        assert_eq!(tl.len, 0, "len must stay 0");
    }

    /// HUlib_keyInIText with a printable character in [' ', '_'] must add it.
    #[test]
    fn key_in_itext_printable_adds_char() {
        let _g = LOCK.lock().unwrap();
        let mut on: c_int = 1;
        let mut it = hu_itext_t {
            l: make_textline(),
            lm: 0,
            _pad0: [0; 4],
            on: &mut on,
            laston: 0,
            _pad1: [0; 4],
        };
        HUlib_clearTextLine(&mut it.l);

        let ret = HUlib_keyInIText(&mut it, b'a'); // lowercase → uppercased to 'A'
        assert_eq!(ret, 1);
        assert_eq!(it.l.len, 1);
        assert_eq!(it.l.l[0], b'A' as c_char);
    }

    /// HUlib_keyInIText with KEY_BACKSPACE removes the last character.
    #[test]
    fn key_in_itext_backspace_removes_char() {
        use crate::doom::doomkeys::KEY_BACKSPACE;
        let _g = LOCK.lock().unwrap();
        let mut on: c_int = 1;
        let mut it = hu_itext_t {
            l: make_textline(),
            lm: 0,
            _pad0: [0; 4],
            on: &mut on,
            laston: 0,
            _pad1: [0; 4],
        };
        HUlib_clearTextLine(&mut it.l);
        HUlib_addCharToTextLine(&mut it.l, b'Z' as c_char);
        assert_eq!(it.l.len, 1);

        let ret = HUlib_keyInIText(&mut it, KEY_BACKSPACE);
        assert_eq!(ret, 1);
        assert_eq!(it.l.len, 0);
    }

    /// Characters outside [' ', '_'] (except Enter/Backspace) return 0.
    #[test]
    fn key_in_itext_unknown_key_returns_zero() {
        let _g = LOCK.lock().unwrap();
        let mut on: c_int = 1;
        let mut it = hu_itext_t {
            l: make_textline(),
            lm: 0,
            _pad0: [0; 4],
            on: &mut on,
            laston: 0,
            _pad1: [0; 4],
        };
        HUlib_clearTextLine(&mut it.l);

        // 0x01 is below ' ' (0x20) and is not Enter or Backspace
        let ret = HUlib_keyInIText(&mut it, 0x01);
        assert_eq!(ret, 0, "unknown control char must return 0");
        assert_eq!(it.l.len, 0, "no char should be added");
    }
}
