//! Rust port of vendor/doomgeneric/i_scale.c.
//!
//! Screen scale-up code: integer pixel-doubling (1x..5x) and aspect-ratio
//! correcting stretch/squash drivers that present Doom's 320x200 paletted
//! framebuffer at a 4:3 physical aspect.
//!
//! # Background
//!
//! Doom's logical framebuffer is `SCREENWIDTH x SCREENHEIGHT = 320 x 200`
//! 8-bit paletted pixels. On the original 4:3 PC monitors a pixel was
//! roughly 1:1.2 tall, so the 320x200 image appeared as a 320x240
//! (i.e. 4:3) picture. Modern square-pixel displays require an extra
//! correction step:
//!
//! - **Pixel-doubling modes** (`mode_scale_*`) ignore aspect entirely
//!   and produce `(320*N) x (200*N)` output for an integer factor `N`.
//!   These are not 4:3 - the picture is too short - but they are pixel
//!   exact and run from a single memcpy/inner loop.
//! - **Stretch modes** (`mode_stretch_*`) keep the horizontal dimension
//!   intact and stretch 200 source rows into 240 (for `N=1`) or 240*N
//!   output rows. The extra rows come from a two-line blend driven by
//!   the precomputed `stretch_tables` / `half_stretch_table` lookups.
//! - **Squash modes** (`mode_squash_*`) take the opposite tack: keep the
//!   200 lines and crunch 320 source columns into roughly 256 columns
//!   (`SCREENWIDTH_4_3 = 256` in vanilla; really should be 266 for a
//!   true 4:3 - see the comment block at the top of `i_scale.c`).
//!
//! Each driver exposes itself through a [`screen_mode_t`] vtable
//! ([`mode_scale_1x`] etc) which `i_video` selects based on the target
//! window/window-fullscreen dimensions.
//!
//! # Lookup tables
//!
//! Stretch/squash modes need to mix two palette indices into a third
//! palette index because the framebuffer is paletted. Three blend
//! tables (20/80, 40/60, 50/50) are precomputed in
//! `generate_stretch_table` from the active palette via nearest-color
//! search (`find_nearest_color`). Every other blend percentage used
//! by the drivers is one of these tables (optionally with arguments
//! swapped to invert the percentages).
//!
//! # Rust port notes
//!
//! All driver functions are `extern "C"` and stored as function pointers
//! in the public `mode_*` statics consumed by `i_video.c`. The pointer
//! arithmetic mirrors the C source exactly (raw `*mut u8`, byte indexing
//! into `src_buffer`/`dest_buffer`). The per-scale-factor stretch/squash
//! drivers in C were each a unique unrolled function; the Rust port
//! factors the shared "fill N pixels from src" inner loops into the
//! generic `scale_nx`, `write_line_nx` and `write_blended_line_nx`
//! helpers parameterized by a `const N: usize`.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::doom::c_ffi::screen_mode_t;
use crate::doom::m_argv::M_CheckParm;
use crate::doom::z_zone::{Z_Free, Z_Malloc};
use std::ffi::{c_int, c_void};
use std::ptr;

extern "C" {
    /// libc `stdout` stream, referenced for `fflush` during the
    /// "Generating lookup tables.." progress prints.
    static mut stdout: *mut libc::FILE;
}

use crate::doom::i_video::{SCREENHEIGHT, SCREENWIDTH};
use crate::doom::z_zone::PU_STATIC;

/// Source framebuffer for the current scale call, set by
/// [`I_InitScale`]. Points to Doom's `SCREENWIDTH * SCREENHEIGHT`
/// paletted backbuffer (usually `I_VideoBuffer`).
static mut src_buffer: *mut u8 = ptr::null_mut();

/// Destination framebuffer for the current scale call, set by
/// [`I_InitScale`]. Points to the platform surface pixels
/// (e.g. `screen->pixels` in the SDL backend).
static mut dest_buffer: *mut u8 = ptr::null_mut();

/// Pitch (bytes per row) of [`dest_buffer`]. May exceed
/// `width * bytes_per_pixel` when the surface has trailing padding.
static mut dest_pitch: c_int = 0;

/// 20/80 and 40/60 palette blend tables. `stretch_tables[0]` is the
/// 20/80 mix, `stretch_tables[1]` is the 40/60 mix; the 60/40 and 80/20
/// mixes are reached by swapping the two source pixels when indexing.
/// Each table is `256 * 256` bytes mapped to the nearest palette index.
/// Lazily populated by [`i_init_stretch_tables`].
static mut stretch_tables: [*mut u8; 2] = [ptr::null_mut(); 2];

/// 50/50 palette blend table used only by the `mode_squash_3x` mode
/// (800x600). Lazily populated by [`i_init_squash_table`].
static mut half_stretch_table: *mut u8 = ptr::null_mut();

/// Records the source/destination buffers and destination pitch that
/// every subsequent driver call in this module will operate on.
///
/// Called by `i_video.c` once per frame before invoking the active
/// `screen_mode_t::draw_screen` callback. The function stashes the
/// arguments in module-level statics so the C-style driver signature
/// `(x1, y1, x2, y2) -> bool` can remain stateless.
///
/// # Safety
///
/// - `_src_buffer` must point to at least `SCREENWIDTH * SCREENHEIGHT`
///   readable bytes for the lifetime of any subsequent driver call.
/// - `_dest_buffer` must point to a writable surface of at least
///   `_dest_pitch * height` bytes, where `height` is the target mode's
///   reported height.
/// - The buffers must outlive every driver call until the next
///   `I_InitScale` invocation.
#[no_mangle]
pub unsafe extern "C" fn I_InitScale(
    _src_buffer: *mut u8,
    _dest_buffer: *mut u8,
    _dest_pitch: c_int,
) {
    src_buffer = _src_buffer;
    dest_buffer = _dest_buffer;
    dest_pitch = _dest_pitch;
}

// ---------------------------------------------------------------------------
// Pixel-doubling scale-up functions
// ---------------------------------------------------------------------------

/// 1x driver: byte-copies the dirty rectangle `[x1,x2) x [y1,y2)` from
/// `src_buffer` to `dest_buffer` one row at a time. Used when the
/// destination surface has a different pitch from `SCREENWIDTH` and a
/// straight `memcpy` of the full buffer is not safe.
///
/// Always succeeds, so returns `1` (C `true`).
///
/// # Safety
///
/// Callable only after [`I_InitScale`] has installed valid buffers; the
/// rectangle must lie within `[0, SCREENWIDTH) x [0, SCREENHEIGHT)`.
unsafe extern "C" fn i_scale_1x(x1: c_int, y1: c_int, x2: c_int, y2: c_int) -> c_int {
    let w = (x2 - x1) as usize;
    let mut bufp = src_buffer.add((y1 * SCREENWIDTH + x1) as usize);
    let mut screenp = dest_buffer.add((y1 * dest_pitch + x1) as usize);
    for _ in y1..y2 {
        std::ptr::copy_nonoverlapping(bufp, screenp, w);
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
    }
    1
}

/// Generic NxN nearest-neighbor scale-up shared by [`i_scale_2x`]
/// through [`i_scale_5x`]. For each source pixel in the dirty
/// rectangle, writes an `N x N` block of the same palette index into
/// `dest_buffer`.
///
/// The C source had a separate hand-unrolled function per factor; this
/// helper collapses them into one parameterized by `const N: usize`,
/// using a fixed-size 5-row scratch array because the highest factor is
/// 5x. Rows above `N-1` in the scratch are unused.
///
/// Always returns `1`.
///
/// # Safety
///
/// Same preconditions as [`i_scale_1x`]; additionally `N` must be in
/// `1..=5` so the scratch row array is large enough.
unsafe fn scale_nx<const N: usize>(x1: c_int, y1: c_int, x2: c_int, y2: c_int) -> c_int {
    let multi_pitch = dest_pitch * N as c_int;
    let mut bufp = src_buffer.add((y1 * SCREENWIDTH + x1) as usize);
    let base = dest_buffer.add(((y1 * dest_pitch + x1) * N as c_int) as usize);
    let mut rows: [*mut u8; 5] = [ptr::null_mut(); 5];
    for i in 0..N {
        rows[i] = base.add((dest_pitch * i as c_int) as usize);
    }
    let w = (x2 - x1) as usize;

    for _ in y1..y2 {
        let mut bp = bufp;
        for _ in 0..w {
            let c = *bp;
            for i in 0..N {
                let sp = rows[i];
                for j in 0..N {
                    *sp.add(j) = c;
                }
                rows[i] = sp.add(N);
            }
            bp = bp.add(1);
        }
        // Inner loop advanced each row by `w * N` bytes; jump to the
        // next destination row group by adding the remaining
        // `multi_pitch - w*N` bytes.
        for i in 0..N {
            rows[i] = rows[i].add((multi_pitch as usize).wrapping_sub(w * N));
        }
        bufp = bufp.add(SCREENWIDTH as usize);
    }
    1
}

/// 2x pixel-doubled scale producing a 640x400 output. Delegates to
/// [`scale_nx`] with `N = 2`.
///
/// # Safety
///
/// See [`scale_nx`].
unsafe extern "C" fn i_scale_2x(x1: c_int, y1: c_int, x2: c_int, y2: c_int) -> c_int {
    scale_nx::<2>(x1, y1, x2, y2)
}

/// 3x pixel-doubled scale producing a 960x600 output. Delegates to
/// [`scale_nx`] with `N = 3`.
///
/// # Safety
///
/// See [`scale_nx`].
unsafe extern "C" fn i_scale_3x(x1: c_int, y1: c_int, x2: c_int, y2: c_int) -> c_int {
    scale_nx::<3>(x1, y1, x2, y2)
}

/// 4x pixel-doubled scale producing a 1280x800 output. Delegates to
/// [`scale_nx`] with `N = 4`.
///
/// # Safety
///
/// See [`scale_nx`].
unsafe extern "C" fn i_scale_4x(x1: c_int, y1: c_int, x2: c_int, y2: c_int) -> c_int {
    scale_nx::<4>(x1, y1, x2, y2)
}

/// 5x pixel-doubled scale producing a 1600x1000 output. Delegates to
/// [`scale_nx`] with `N = 5`.
///
/// # Safety
///
/// See [`scale_nx`].
unsafe extern "C" fn i_scale_5x(x1: c_int, y1: c_int, x2: c_int, y2: c_int) -> c_int {
    scale_nx::<5>(x1, y1, x2, y2)
}

// ---------------------------------------------------------------------------
// Stretch / squash line helpers
// ---------------------------------------------------------------------------

/// Writes a single full-width destination row of `N`-pixel horizontally
/// expanded source pixels (no vertical blending). Used by every stretch
/// driver as the "100% line k" building block.
///
/// # Safety
///
/// `dest` must point to at least `SCREENWIDTH * N` writable bytes;
/// `src` must point to at least `SCREENWIDTH` readable bytes.
unsafe fn write_line_nx<const N: usize>(dest: *mut u8, src: *mut u8) {
    let mut d = dest;
    let mut s = src;
    for _ in 0..SCREENWIDTH {
        let c = *s;
        for i in 0..N {
            *d.add(i) = c;
        }
        d = d.add(N);
        s = s.add(1);
    }
}

/// Writes a single full-width destination row that is the per-pixel
/// blend of two source rows via `stretch_table`. The blend percentage
/// is fixed by the chosen table; the order of `src1`/`src2` flips the
/// percentage (e.g. `stretch_tables[0]` with `(src1, src2)` is 20/80,
/// with `(src2, src1)` it is 80/20). Each blended pixel is expanded
/// horizontally to `N` destination bytes.
///
/// # Safety
///
/// - `dest` must point to `SCREENWIDTH * N` writable bytes.
/// - `src1` and `src2` must each point to `SCREENWIDTH` readable bytes.
/// - `stretch_table` must point to a 65536-byte palette mix table.
unsafe fn write_blended_line_nx<const N: usize>(
    dest: *mut u8,
    src1: *mut u8,
    src2: *mut u8,
    stretch_table: *mut u8,
) {
    let mut d = dest;
    let mut s1 = src1;
    let mut s2 = src2;
    for _ in 0..SCREENWIDTH {
        let val = *stretch_table.add((*s1 as usize) * 256 + (*s2 as usize));
        for i in 0..N {
            *d.add(i) = val;
        }
        d = d.add(N);
        s1 = s1.add(1);
        s2 = s2.add(1);
    }
}

/// 1x specialization of [`write_blended_line_nx`] used by
/// [`i_stretch_1x`]. Identical algorithm without the inner expansion
/// loop, mirroring the unrolled `WriteBlendedLine1x` in C.
///
/// # Safety
///
/// See [`write_blended_line_nx`].
unsafe fn write_blended_line_1x(
    dest: *mut u8,
    src1: *mut u8,
    src2: *mut u8,
    stretch_table: *mut u8,
) {
    let mut d = dest;
    let mut s1 = src1;
    let mut s2 = src2;
    for _ in 0..SCREENWIDTH {
        *d = *stretch_table.add((*s1 as usize) * 256 + (*s2 as usize));
        d = d.add(1);
        s1 = s1.add(1);
        s2 = s2.add(1);
    }
}

// ---------------------------------------------------------------------------
// Lookup-table generation
// ---------------------------------------------------------------------------

/// Returns the palette index whose RGB triple has the smallest squared
/// Euclidean distance to `(r, g, b)`. Exact matches short-circuit.
///
/// `palette` must point to 256 contiguous `(R, G, B)` byte triples.
///
/// # Safety
///
/// `palette` must point to at least `256 * 3` readable bytes.
unsafe fn find_nearest_color(palette: *mut u8, r: c_int, g: c_int, b: c_int) -> c_int {
    let mut best: c_int = 0;
    let mut best_diff = c_int::MAX;
    for i in 0..256 {
        let col = palette.add(i * 3);
        let dr = r - *col as c_int;
        let dg = g - *col.add(1) as c_int;
        let db = b - *col.add(2) as c_int;
        let diff = dr * dr + dg * dg + db * db;
        if diff == 0 {
            return i as c_int;
        }
        if diff < best_diff {
            best = i as c_int;
            best_diff = diff;
        }
    }
    best
}

/// Builds a 256x256 palette-mix lookup table whose `(x, y)` entry is
/// the palette index closest to `pct` of palette colour `x` plus
/// `100 - pct` of palette colour `y`.
///
/// The result is allocated from the zone heap (`Z_Malloc`, `PU_STATIC`)
/// and returned as a raw owning pointer; callers are responsible for
/// passing it to `Z_Free` when discarding it.
///
/// This is the same construction used in other Doom source ports for
/// translucency tables.
///
/// # Safety
///
/// `palette` must satisfy [`find_nearest_color`]'s contract.
unsafe fn generate_stretch_table(palette: *mut u8, pct: c_int) -> *mut u8 {
    let result = Z_Malloc(256 * 256, PU_STATIC, ptr::null_mut()) as *mut u8;
    for x in 0..256 {
        for y in 0..256 {
            let col1 = palette.add(x * 3);
            let col2 = palette.add(y * 3);
            let r = ((*col1 as c_int) * pct + (*col2 as c_int) * (100 - pct)) / 100;
            let g = ((*col1.add(1) as c_int) * pct + (*col2.add(1) as c_int) * (100 - pct)) / 100;
            let b = ((*col1.add(2) as c_int) * pct + (*col2.add(2) as c_int) * (100 - pct)) / 100;
            *result.add(x * 256 + y) = find_nearest_color(palette, r, g, b) as u8;
        }
    }
    result
}

/// `init_mode` callback for every `mode_stretch_*` and the 1x/2x/4x/5x
/// `mode_squash_*` modes. Lazily generates the 20/80 and 40/60 palette
/// blend tables, printing a one-line progress message. No-op once the
/// tables exist; `I_ResetScaleTables` must be called first to force
/// regeneration after a palette switch.
///
/// # Safety
///
/// See [`generate_stretch_table`].
unsafe extern "C" fn i_init_stretch_tables(palette: *mut u8) {
    if !stretch_tables[0].is_null() {
        return;
    }
    libc::printf(c"I_InitStretchTables: Generating lookup tables..".as_ptr());
    libc::fflush(stdout);
    stretch_tables[0] = generate_stretch_table(palette, 20);
    libc::printf(c"..".as_ptr());
    libc::fflush(stdout);
    stretch_tables[1] = generate_stretch_table(palette, 40);
    libc::puts(c"".as_ptr());
}

/// `init_mode` callback for [`mode_squash_3x`] (800x600). Lazily
/// generates only the 50/50 blend table used by that mode's column
/// expansion. No-op once the table exists.
///
/// # Safety
///
/// See [`generate_stretch_table`].
unsafe extern "C" fn i_init_squash_table(palette: *mut u8) {
    if !half_stretch_table.is_null() {
        return;
    }
    libc::printf(c"I_InitSquashTable: Generating lookup table..".as_ptr());
    libc::fflush(stdout);
    half_stretch_table = generate_stretch_table(palette, 50);
    libc::puts(c"".as_ptr());
}

/// Frees whichever blend tables are currently populated and regenerates
/// them from `palette`. Called by `i_video` after a palette switch (for
/// example, exiting the palette-shifting menus) so that subsequent
/// stretch/squash output stays close to the in-game colour set.
///
/// Tables that were never allocated stay null - this is not a forced
/// generation, only a refresh.
///
/// # Safety
///
/// - `palette` must satisfy `find_nearest_color`'s contract.
/// - Any pointers previously handed out from the `stretch_tables` /
///   `half_stretch_table` statics are invalidated by this call.
#[no_mangle]
pub unsafe extern "C" fn I_ResetScaleTables(palette: *mut u8) {
    if !stretch_tables[0].is_null() {
        Z_Free(stretch_tables[0] as *mut c_void);
        Z_Free(stretch_tables[1] as *mut c_void);
        libc::printf(c"I_ResetScaleTables: Regenerating lookup tables..\n".as_ptr());
        stretch_tables[0] = generate_stretch_table(palette, 20);
        stretch_tables[1] = generate_stretch_table(palette, 40);
    }
    if !half_stretch_table.is_null() {
        Z_Free(half_stretch_table as *mut c_void);
        libc::printf(c"I_ResetScaleTables: Regenerating lookup table..\n".as_ptr());
        half_stretch_table = generate_stretch_table(palette, 50);
    }
}

// ---------------------------------------------------------------------------
// Aspect ratio correcting stretch functions
// ---------------------------------------------------------------------------

/// 1x stretch driver: produces a 320x240 output by inserting four
/// blended rows for every five source rows (200 -> 240). Each 5-row
/// block emits, in order: 100% line0, 20/80 blend(l0,l1),
/// 40/60 blend(l1,l2), 60/40 blend(l2,l3), 80/20 blend(l3,l4),
/// 100% line4.
///
/// Only supports full-screen updates - the dirty rectangle must equal
/// `[0,0)..(SCREENWIDTH,SCREENHEIGHT)`; otherwise the function returns
/// `0` (C `false`) without drawing.
///
/// # Safety
///
/// See [`I_InitScale`]; additionally [`i_init_stretch_tables`] must
/// have been called so [`stretch_tables`] is populated.
unsafe extern "C" fn i_stretch_1x(_x1: c_int, _y1: c_int, _x2: c_int, _y2: c_int) -> c_int {
    if _x1 != 0 || _y1 != 0 || _x2 != SCREENWIDTH || _y2 != SCREENHEIGHT {
        return 0;
    }
    let mut bufp = src_buffer;
    let mut screenp = dest_buffer;
    for _ in (0..SCREENHEIGHT).step_by(5) {
        std::ptr::copy_nonoverlapping(bufp, screenp, SCREENWIDTH as usize);
        screenp = screenp.add(dest_pitch as usize);
        write_blended_line_1x(
            screenp,
            bufp,
            bufp.add(SCREENWIDTH as usize),
            stretch_tables[0],
        );
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
        write_blended_line_1x(
            screenp,
            bufp,
            bufp.add(SCREENWIDTH as usize),
            stretch_tables[1],
        );
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
        write_blended_line_1x(
            screenp,
            bufp.add(SCREENWIDTH as usize),
            bufp,
            stretch_tables[1],
        );
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
        write_blended_line_1x(
            screenp,
            bufp.add(SCREENWIDTH as usize),
            bufp,
            stretch_tables[0],
        );
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
        std::ptr::copy_nonoverlapping(bufp, screenp, SCREENWIDTH as usize);
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
    }
    1
}

/// 2x stretch driver: 640x480 (200 -> 480, 12 rows per 5 source rows).
/// Schedule per 5-source block: line0 x2, blend40/60(l0,l1), line1,
/// blend80/20(l1,l2), line2 x2, blend20/80(l2,l3), line3,
/// blend60/40(l3,l4), line4 x2. Only supports full-screen updates.
///
/// # Safety
///
/// See [`i_stretch_1x`].
unsafe extern "C" fn i_stretch_2x(_x1: c_int, _y1: c_int, _x2: c_int, _y2: c_int) -> c_int {
    if _x1 != 0 || _y1 != 0 || _x2 != SCREENWIDTH || _y2 != SCREENHEIGHT {
        return 0;
    }
    let mut bufp = src_buffer;
    let mut screenp = dest_buffer;
    for _ in (0..SCREENHEIGHT).step_by(5) {
        write_line_nx::<2>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<2>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_blended_line_nx::<2>(
            screenp,
            bufp,
            bufp.add(SCREENWIDTH as usize),
            stretch_tables[1],
        );
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
        write_line_nx::<2>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_blended_line_nx::<2>(
            screenp,
            bufp.add(SCREENWIDTH as usize),
            bufp,
            stretch_tables[0],
        );
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
        write_line_nx::<2>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<2>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_blended_line_nx::<2>(
            screenp,
            bufp,
            bufp.add(SCREENWIDTH as usize),
            stretch_tables[0],
        );
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
        write_line_nx::<2>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_blended_line_nx::<2>(
            screenp,
            bufp.add(SCREENWIDTH as usize),
            bufp,
            stretch_tables[1],
        );
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
        write_line_nx::<2>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<2>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
    }
    1
}

/// 3x stretch driver: 960x720 (200 -> 720, 18 rows per 5 source rows).
/// Schedule per 5-source block (notation `X/Y(a,b,t) = X% a + Y% b`):
/// line0 x3, blend40/60(l1,l0,t1), line1 x3, blend20/80(l1,l2,t0),
/// line2 x2, blend20/80(l3,l2,t0), line3 x3, blend40/60(l3,l4,t1),
/// line4 x3. Only supports full-screen updates.
///
/// # Safety
///
/// See [`i_stretch_1x`].
unsafe extern "C" fn i_stretch_3x(_x1: c_int, _y1: c_int, _x2: c_int, _y2: c_int) -> c_int {
    if _x1 != 0 || _y1 != 0 || _x2 != SCREENWIDTH || _y2 != SCREENHEIGHT {
        return 0;
    }
    let mut bufp = src_buffer;
    let mut screenp = dest_buffer;
    for _ in (0..SCREENHEIGHT).step_by(5) {
        write_line_nx::<3>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<3>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<3>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_blended_line_nx::<3>(
            screenp,
            bufp.add(SCREENWIDTH as usize),
            bufp,
            stretch_tables[1],
        );
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
        write_line_nx::<3>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<3>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<3>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_blended_line_nx::<3>(
            screenp,
            bufp,
            bufp.add(SCREENWIDTH as usize),
            stretch_tables[0],
        );
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
        write_line_nx::<3>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<3>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_blended_line_nx::<3>(
            screenp,
            bufp.add(SCREENWIDTH as usize),
            bufp,
            stretch_tables[0],
        );
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
        write_line_nx::<3>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<3>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<3>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_blended_line_nx::<3>(
            screenp,
            bufp,
            bufp.add(SCREENWIDTH as usize),
            stretch_tables[1],
        );
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
        write_line_nx::<3>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<3>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<3>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
    }
    1
}

/// 4x stretch driver: 1280x960 (200 -> 960, 24 rows per 5 source rows).
/// Schedule per 5-source block (notation `X/Y(a,b,t) = X% a + Y% b`):
/// line0 x4, blend20/80(l1,l0,t0) (the C source's "90% line 0, 20% line 1"
/// comment is a typo - the call is `stretch_tables[0]` with `(line1, line0)`,
/// i.e. 20% line1 + 80% line0), line1 x4, blend40/60(l1,l0,t1), line2 x4,
/// blend40/60(l2,l3,t1), line3 x4, blend20/80(l3,l4,t0), line4 x4. Only
/// supports full-screen updates.
///
/// # Safety
///
/// See [`i_stretch_1x`].
unsafe extern "C" fn i_stretch_4x(_x1: c_int, _y1: c_int, _x2: c_int, _y2: c_int) -> c_int {
    if _x1 != 0 || _y1 != 0 || _x2 != SCREENWIDTH || _y2 != SCREENHEIGHT {
        return 0;
    }
    let mut bufp = src_buffer;
    let mut screenp = dest_buffer;
    for _ in (0..SCREENHEIGHT).step_by(5) {
        write_line_nx::<4>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<4>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<4>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<4>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_blended_line_nx::<4>(
            screenp,
            bufp.add(SCREENWIDTH as usize),
            bufp,
            stretch_tables[0],
        );
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
        write_line_nx::<4>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<4>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<4>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<4>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_blended_line_nx::<4>(
            screenp,
            bufp.add(SCREENWIDTH as usize),
            bufp,
            stretch_tables[1],
        );
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
        write_line_nx::<4>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<4>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<4>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<4>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_blended_line_nx::<4>(
            screenp,
            bufp,
            bufp.add(SCREENWIDTH as usize),
            stretch_tables[1],
        );
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
        write_line_nx::<4>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<4>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<4>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<4>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_blended_line_nx::<4>(
            screenp,
            bufp,
            bufp.add(SCREENWIDTH as usize),
            stretch_tables[0],
        );
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
        write_line_nx::<4>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<4>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<4>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<4>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
    }
    1
}

/// 5x stretch driver: 1600x1200 (200 -> 1200, 6 rows per source row, no
/// blends needed because the ratio is exact). After the main loop,
/// `-scanline` (a legacy diagnostic switch documented in the C source)
/// overwrites every third row of the first 1198 with black to emulate
/// CRT scan lines. The 1600 / 1198 / `dest_pitch * 3` magic numbers are
/// carried over verbatim from the C source.
///
/// Only supports full-screen updates.
///
/// # Safety
///
/// See [`i_stretch_1x`].
unsafe extern "C" fn i_stretch_5x(_x1: c_int, _y1: c_int, _x2: c_int, _y2: c_int) -> c_int {
    if _x1 != 0 || _y1 != 0 || _x2 != SCREENWIDTH || _y2 != SCREENHEIGHT {
        return 0;
    }
    let mut bufp = src_buffer;
    let mut screenp = dest_buffer;
    for _ in 0..SCREENHEIGHT {
        write_line_nx::<5>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<5>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<5>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<5>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<5>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        write_line_nx::<5>(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
    }
    if M_CheckParm(c"-scanline".as_ptr().cast_mut()) > 0 {
        let mut screenp = dest_buffer.add((2 * dest_pitch) as usize);
        let mut y = 0;
        while y < 1198 {
            std::ptr::write_bytes(screenp, 0, 1600);
            screenp = screenp.add((dest_pitch * 3) as usize);
            y += 3;
        }
    }
    1
}

// ---------------------------------------------------------------------------
// Aspect ratio correcting squash functions
// ---------------------------------------------------------------------------

/// Writes the same palette index `$c` to two horizontally adjacent
/// destination pointers and advances both. Used by the 2x squash driver
/// to fill an output column pair on two stacked rows.
//
// Macro invocations elsewhere use `//` (not `///`) per the doc spec.
macro_rules! draw_pixel2 {
    ($dest:expr, $dest2:expr, $c:expr) => {
        *$dest = $c;
        $dest = $dest.add(1);
        *$dest2 = $c;
        $dest2 = $dest2.add(1);
    };
}

/// Writes the same palette index `$c` to three vertically adjacent
/// destination rows and advances all three pointers by one byte. Used
/// by the 3x squash driver.
macro_rules! draw_pixel3 {
    ($dest:expr, $dest2:expr, $dest3:expr, $c:expr) => {
        *$dest = $c;
        $dest = $dest.add(1);
        *$dest2 = $c;
        $dest2 = $dest2.add(1);
        *$dest3 = $c;
        $dest3 = $dest3.add(1);
    };
}

/// Writes the same palette index `$c` to four vertically adjacent
/// destination rows and advances all four pointers. Used by the 4x
/// squash driver.
macro_rules! draw_pixel4 {
    ($dest:expr, $dest2:expr, $dest3:expr, $dest4:expr, $c:expr) => {
        *$dest = $c;
        $dest = $dest.add(1);
        *$dest2 = $c;
        $dest2 = $dest2.add(1);
        *$dest3 = $c;
        $dest3 = $dest3.add(1);
        *$dest4 = $c;
        $dest4 = $dest4.add(1);
    };
}

/// Writes the same palette index `$c` to five vertically adjacent
/// destination rows and advances all five pointers. Used by the 5x
/// squash driver.
macro_rules! draw_pixel5 {
    ($dest:expr, $dest2:expr, $dest3:expr, $dest4:expr, $dest5:expr, $c:expr) => {
        *$dest = $c;
        $dest = $dest.add(1);
        *$dest2 = $c;
        $dest2 = $dest2.add(1);
        *$dest3 = $c;
        $dest3 = $dest3.add(1);
        *$dest4 = $c;
        $dest4 = $dest4.add(1);
        *$dest5 = $c;
        $dest5 = $dest5.add(1);
    };
}

/// Compresses one 320-column source row into 256 destination columns
/// (single row of output) for the 1x squash driver. Each block of 5
/// source pixels produces 4 destination pixels via three blends with
/// the 20/80 and 40/60 tables.
///
/// # Safety
///
/// `dest` must point to at least `(SCREENWIDTH / 5) * 4 = 256` writable
/// bytes; `src` must point to at least `SCREENWIDTH` readable bytes.
/// [`stretch_tables`] must be populated.
unsafe fn write_squashed_line_1x(dest: *mut u8, src: *mut u8) {
    let mut d = dest;
    let mut s = src;
    for _ in 0..(SCREENWIDTH / 5) {
        *d = *stretch_tables[0].add((*s.add(1) as usize) * 256 + (*s as usize));
        d = d.add(1);
        *d = *stretch_tables[1].add((*s.add(2) as usize) * 256 + (*s.add(1) as usize));
        d = d.add(1);
        *d = *stretch_tables[1].add((*s.add(2) as usize) * 256 + (*s.add(3) as usize));
        d = d.add(1);
        *d = *stretch_tables[0].add((*s.add(3) as usize) * 256 + (*s.add(4) as usize));
        d = d.add(1);
        s = s.add(5);
    }
}

/// Compresses 5 source columns into 8 destination columns (writing
/// two stacked rows simultaneously) for the 2x squash driver. The
/// 5 -> 8 expansion uses three 40/60 or 20/80 blends interleaved with
/// straight pixel copies.
///
/// # Safety
///
/// `dest` must point to at least `(SCREENWIDTH / 5) * 8` writable bytes
/// and the row at `dest + dest_pitch` must also be writable for the
/// same width. [`stretch_tables`] must be populated.
unsafe fn write_squashed_line_2x(dest: *mut u8, src: *mut u8) {
    let mut d = dest;
    let mut d2 = dest.add(dest_pitch as usize);
    let mut s = src;
    for _ in 0..(SCREENWIDTH / 5) {
        let mut c = *s;
        draw_pixel2!(d, d2, c);
        c = *stretch_tables[1].add((*s.add(1) as usize) * 256 + (*s as usize));
        draw_pixel2!(d, d2, c);
        c = *s.add(1);
        draw_pixel2!(d, d2, c);
        c = *stretch_tables[0].add((*s.add(1) as usize) * 256 + (*s.add(2) as usize));
        draw_pixel2!(d, d2, c);
        c = *stretch_tables[0].add((*s.add(3) as usize) * 256 + (*s.add(2) as usize));
        draw_pixel2!(d, d2, c);
        c = *s.add(3);
        draw_pixel2!(d, d2, c);
        c = *stretch_tables[1].add((*s.add(3) as usize) * 256 + (*s.add(4) as usize));
        draw_pixel2!(d, d2, c);
        c = *s.add(4);
        draw_pixel2!(d, d2, c);
        s = s.add(5);
    }
}

/// Compresses 2 source columns into 5 destination columns across three
/// stacked rows for the 3x squash driver (800x600 mode). Uses the
/// 50/50 [`half_stretch_table`] for the single blended column in each
/// pair.
///
/// # Safety
///
/// `dest`, `dest + dest_pitch` and `dest + 2*dest_pitch` must each be
/// writable for `(SCREENWIDTH / 2) * 5` bytes. [`half_stretch_table`]
/// must be populated.
unsafe fn write_squashed_line_3x(dest: *mut u8, src: *mut u8) {
    let mut d = dest;
    let mut d2 = dest.add(dest_pitch as usize);
    let mut d3 = dest.add((dest_pitch * 2) as usize);
    let mut s = src;
    for _ in 0..(SCREENWIDTH / 2) {
        let mut c = *s;
        draw_pixel3!(d, d2, d3, c);
        draw_pixel3!(d, d2, d3, c);
        c = *half_stretch_table.add((*s as usize) * 256 + (*s.add(1) as usize));
        draw_pixel3!(d, d2, d3, c);
        c = *s.add(1);
        draw_pixel3!(d, d2, d3, c);
        draw_pixel3!(d, d2, d3, c);
        s = s.add(2);
    }
}

/// Compresses 5 source columns into 16 destination columns across four
/// stacked rows for the 4x squash driver. Each 5-column block produces
/// 16 output columns via four blend points.
///
/// # Safety
///
/// `dest` through `dest + 3*dest_pitch` must each be writable for
/// `(SCREENWIDTH / 5) * 16` bytes. [`stretch_tables`] must be
/// populated.
unsafe fn write_squashed_line_4x(dest: *mut u8, src: *mut u8) {
    let mut d = dest;
    let mut d2 = dest.add(dest_pitch as usize);
    let mut d3 = dest.add((dest_pitch * 2) as usize);
    let mut d4 = dest.add((dest_pitch * 3) as usize);
    let mut s = src;
    for _ in 0..(SCREENWIDTH / 5) {
        let mut c = *s;
        draw_pixel4!(d, d2, d3, d4, c);
        draw_pixel4!(d, d2, d3, d4, c);
        draw_pixel4!(d, d2, d3, d4, c);
        c = *stretch_tables[0].add((*s as usize) * 256 + (*s.add(1) as usize));
        draw_pixel4!(d, d2, d3, d4, c);
        c = *s.add(1);
        draw_pixel4!(d, d2, d3, d4, c);
        draw_pixel4!(d, d2, d3, d4, c);
        c = *stretch_tables[1].add((*s.add(1) as usize) * 256 + (*s.add(2) as usize));
        draw_pixel4!(d, d2, d3, d4, c);
        c = *s.add(2);
        draw_pixel4!(d, d2, d3, d4, c);
        draw_pixel4!(d, d2, d3, d4, c);
        c = *stretch_tables[1].add((*s.add(3) as usize) * 256 + (*s.add(2) as usize));
        draw_pixel4!(d, d2, d3, d4, c);
        c = *s.add(3);
        draw_pixel4!(d, d2, d3, d4, c);
        draw_pixel4!(d, d2, d3, d4, c);
        c = *stretch_tables[0].add((*s.add(4) as usize) * 256 + (*s.add(3) as usize));
        draw_pixel4!(d, d2, d3, d4, c);
        c = *s.add(4);
        draw_pixel4!(d, d2, d3, d4, c);
        draw_pixel4!(d, d2, d3, d4, c);
        draw_pixel4!(d, d2, d3, d4, c);
        s = s.add(5);
    }
}

/// Replicates each source column to 4 destination columns across five
/// stacked rows for the 5x squash driver. No blending - the 5x squash
/// is exactly a 1:4 horizontal repeat over five rows, giving
/// `SCREENWIDTH * 4 = 1280` destination columns per source row, which
/// matches [`mode_squash_5x`]'s declared width.
///
/// (The C source's "Draw in blocks of 5" comment in
/// `WriteSquashedLine5x` is a leftover from the other squash variants;
/// the actual block size in this function is 4.)
///
/// # Safety
///
/// `dest` through `dest + 4*dest_pitch` must each be writable for
/// `SCREENWIDTH * 4 = 1280` bytes.
unsafe fn write_squashed_line_5x(dest: *mut u8, src: *mut u8) {
    let mut d = dest;
    let mut d2 = dest.add(dest_pitch as usize);
    let mut d3 = dest.add((dest_pitch * 2) as usize);
    let mut d4 = dest.add((dest_pitch * 3) as usize);
    let mut d5 = dest.add((dest_pitch * 4) as usize);
    let mut s = src;
    for _ in 0..SCREENWIDTH {
        let c = *s;
        draw_pixel5!(d, d2, d3, d4, d5, c);
        draw_pixel5!(d, d2, d3, d4, d5, c);
        draw_pixel5!(d, d2, d3, d4, d5, c);
        draw_pixel5!(d, d2, d3, d4, d5, c);
        s = s.add(1);
    }
}

/// 1x squash driver: 256x200 output. Walks every source row through
/// [`write_squashed_line_1x`]. Only supports full-screen updates.
///
/// # Safety
///
/// See [`i_stretch_1x`]; same preconditions plus
/// [`write_squashed_line_1x`].
unsafe extern "C" fn i_squash_1x(_x1: c_int, _y1: c_int, _x2: c_int, _y2: c_int) -> c_int {
    if _x1 != 0 || _y1 != 0 || _x2 != SCREENWIDTH || _y2 != SCREENHEIGHT {
        return 0;
    }
    let mut bufp = src_buffer;
    let mut screenp = dest_buffer;
    for _ in 0..SCREENHEIGHT {
        write_squashed_line_1x(screenp, bufp);
        screenp = screenp.add(dest_pitch as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
    }
    1
}

/// 2x squash driver: 512x400 output. Each call to
/// [`write_squashed_line_2x`] emits two output rows, so `screenp`
/// advances by `dest_pitch * 2` per source row. Only supports
/// full-screen updates.
///
/// # Safety
///
/// See [`i_stretch_1x`]; same preconditions plus
/// [`write_squashed_line_2x`].
unsafe extern "C" fn i_squash_2x(_x1: c_int, _y1: c_int, _x2: c_int, _y2: c_int) -> c_int {
    if _x1 != 0 || _y1 != 0 || _x2 != SCREENWIDTH || _y2 != SCREENHEIGHT {
        return 0;
    }
    let mut bufp = src_buffer;
    let mut screenp = dest_buffer;
    for _ in 0..SCREENHEIGHT {
        write_squashed_line_2x(screenp, bufp);
        screenp = screenp.add((dest_pitch * 2) as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
    }
    1
}

/// 3x squash driver: 800x600 output. Each call to
/// [`write_squashed_line_3x`] emits three output rows. This is the
/// special case that relies on [`half_stretch_table`] (initialized by
/// [`i_init_squash_table`]). Only supports full-screen updates.
///
/// # Safety
///
/// See [`i_stretch_1x`]; additionally [`half_stretch_table`] must be
/// populated.
unsafe extern "C" fn i_squash_3x(_x1: c_int, _y1: c_int, _x2: c_int, _y2: c_int) -> c_int {
    if _x1 != 0 || _y1 != 0 || _x2 != SCREENWIDTH || _y2 != SCREENHEIGHT {
        return 0;
    }
    let mut bufp = src_buffer;
    let mut screenp = dest_buffer;
    for _ in 0..SCREENHEIGHT {
        write_squashed_line_3x(screenp, bufp);
        screenp = screenp.add((dest_pitch * 3) as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
    }
    1
}

/// 4x squash driver: 1024x800 output. Each call to
/// [`write_squashed_line_4x`] emits four output rows. Only supports
/// full-screen updates.
///
/// # Safety
///
/// See [`i_stretch_1x`]; same preconditions plus
/// [`write_squashed_line_4x`].
unsafe extern "C" fn i_squash_4x(_x1: c_int, _y1: c_int, _x2: c_int, _y2: c_int) -> c_int {
    if _x1 != 0 || _y1 != 0 || _x2 != SCREENWIDTH || _y2 != SCREENHEIGHT {
        return 0;
    }
    let mut bufp = src_buffer;
    let mut screenp = dest_buffer;
    for _ in 0..SCREENHEIGHT {
        write_squashed_line_4x(screenp, bufp);
        screenp = screenp.add((dest_pitch * 4) as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
    }
    1
}

/// 5x squash driver: 1280x1000 output. Each call to
/// [`write_squashed_line_5x`] emits five output rows. Only supports
/// full-screen updates.
///
/// # Safety
///
/// See [`i_stretch_1x`]; same preconditions plus
/// [`write_squashed_line_5x`].
unsafe extern "C" fn i_squash_5x(_x1: c_int, _y1: c_int, _x2: c_int, _y2: c_int) -> c_int {
    if _x1 != 0 || _y1 != 0 || _x2 != SCREENWIDTH || _y2 != SCREENHEIGHT {
        return 0;
    }
    let mut bufp = src_buffer;
    let mut screenp = dest_buffer;
    for _ in 0..SCREENHEIGHT {
        write_squashed_line_5x(screenp, bufp);
        screenp = screenp.add((dest_pitch * 5) as usize);
        bufp = bufp.add(SCREENWIDTH as usize);
    }
    1
}

// ---------------------------------------------------------------------------
// Screen mode descriptors
// ---------------------------------------------------------------------------

/// 320x200 pixel-doubling mode (1:1 source copy, no aspect correction).
/// `poor_quality = 0` because the output is pixel-exact, just not 4:3.
/// Exported via `#[no_mangle]` so `i_video.c` can select it by symbol.
#[no_mangle]
pub static mut mode_scale_1x: screen_mode_t = screen_mode_t {
    width: 320,
    height: 200,
    init_mode: None,
    draw_screen: Some(i_scale_1x),
    poor_quality: 0,
};

/// 640x400 pixel-doubling mode (2x). Exported for `i_video.c`.
#[no_mangle]
pub static mut mode_scale_2x: screen_mode_t = screen_mode_t {
    width: 640,
    height: 400,
    init_mode: None,
    draw_screen: Some(i_scale_2x),
    poor_quality: 0,
};

/// 960x600 pixel-doubling mode (3x). Exported for `i_video.c`.
#[no_mangle]
pub static mut mode_scale_3x: screen_mode_t = screen_mode_t {
    width: 960,
    height: 600,
    init_mode: None,
    draw_screen: Some(i_scale_3x),
    poor_quality: 0,
};

/// 1280x800 pixel-doubling mode (4x). Exported for `i_video.c`.
#[no_mangle]
pub static mut mode_scale_4x: screen_mode_t = screen_mode_t {
    width: 1280,
    height: 800,
    init_mode: None,
    draw_screen: Some(i_scale_4x),
    poor_quality: 0,
};

/// 1600x1000 pixel-doubling mode (5x). Exported for `i_video.c`.
#[no_mangle]
pub static mut mode_scale_5x: screen_mode_t = screen_mode_t {
    width: 1600,
    height: 1000,
    init_mode: None,
    draw_screen: Some(i_scale_5x),
    poor_quality: 0,
};

/// 320x240 aspect-corrected stretch mode. Flagged `poor_quality = 1`
/// because the 200 -> 240 expansion only adds 40 rows, producing
/// visible interpolation banding; consumers default to a pixel-doubled
/// mode when possible. Uses `i_init_stretch_tables` to populate the
/// blend lookups on first selection. Exported for `i_video.c`.
#[no_mangle]
pub static mut mode_stretch_1x: screen_mode_t = screen_mode_t {
    width: 320,
    height: 240,
    init_mode: Some(i_init_stretch_tables),
    draw_screen: Some(i_stretch_1x),
    poor_quality: 1,
};

/// 640x480 aspect-corrected stretch mode (2x). Exported for
/// `i_video.c`.
#[no_mangle]
pub static mut mode_stretch_2x: screen_mode_t = screen_mode_t {
    width: 640,
    height: 480,
    init_mode: Some(i_init_stretch_tables),
    draw_screen: Some(i_stretch_2x),
    poor_quality: 0,
};

/// 960x720 aspect-corrected stretch mode (3x). Exported for
/// `i_video.c`.
#[no_mangle]
pub static mut mode_stretch_3x: screen_mode_t = screen_mode_t {
    width: 960,
    height: 720,
    init_mode: Some(i_init_stretch_tables),
    draw_screen: Some(i_stretch_3x),
    poor_quality: 0,
};

/// 1280x960 aspect-corrected stretch mode (4x). Exported for
/// `i_video.c`.
#[no_mangle]
pub static mut mode_stretch_4x: screen_mode_t = screen_mode_t {
    width: 1280,
    height: 960,
    init_mode: Some(i_init_stretch_tables),
    draw_screen: Some(i_stretch_4x),
    poor_quality: 0,
};

/// 1600x1200 aspect-corrected stretch mode (5x). Exported for
/// `i_video.c`.
#[no_mangle]
pub static mut mode_stretch_5x: screen_mode_t = screen_mode_t {
    width: 1600,
    height: 1200,
    init_mode: Some(i_init_stretch_tables),
    draw_screen: Some(i_stretch_5x),
    poor_quality: 0,
};

/// 256x200 aspect-corrected squash mode. Flagged `poor_quality = 1`
/// because horizontal compression is more visible than the equivalent
/// vertical stretch. Exported for `i_video.c`.
#[no_mangle]
pub static mut mode_squash_1x: screen_mode_t = screen_mode_t {
    width: 256,
    height: 200,
    init_mode: Some(i_init_stretch_tables),
    draw_screen: Some(i_squash_1x),
    poor_quality: 1,
};

/// 512x400 aspect-corrected squash mode (2x). Exported for
/// `i_video.c`.
#[no_mangle]
pub static mut mode_squash_2x: screen_mode_t = screen_mode_t {
    width: 512,
    height: 400,
    init_mode: Some(i_init_stretch_tables),
    draw_screen: Some(i_squash_2x),
    poor_quality: 0,
};

/// 800x600 aspect-corrected squash mode (3x). The only mode that uses
/// the 50/50 `half_stretch_table`, populated by
/// `i_init_squash_table`. Exported for `i_video.c`.
#[no_mangle]
pub static mut mode_squash_3x: screen_mode_t = screen_mode_t {
    width: 800,
    height: 600,
    init_mode: Some(i_init_squash_table),
    draw_screen: Some(i_squash_3x),
    poor_quality: 0,
};

/// 1024x800 aspect-corrected squash mode (4x). Exported for
/// `i_video.c`.
#[no_mangle]
pub static mut mode_squash_4x: screen_mode_t = screen_mode_t {
    width: 1024,
    height: 800,
    init_mode: Some(i_init_stretch_tables),
    draw_screen: Some(i_squash_4x),
    poor_quality: 0,
};

/// 1280x1000 aspect-corrected squash mode (5x). Exported for
/// `i_video.c`.
#[no_mangle]
pub static mut mode_squash_5x: screen_mode_t = screen_mode_t {
    width: 1280,
    height: 1000,
    init_mode: Some(i_init_stretch_tables),
    draw_screen: Some(i_squash_5x),
    poor_quality: 0,
};
