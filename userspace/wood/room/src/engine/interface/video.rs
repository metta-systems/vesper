//! Rust port of vendor/doomgeneric/i_video.c.
//!
//! Video output shim: allocates the palette-indexed `I_VideoBuffer`,
//! expands it through the palette into the platform's frame on
//! `I_FinishUpdate`, and forwards calls to the platform layer.
//!
//! The C source supports two compile-time paths: an 8-bit `CMAP256`
//! direct-blit and a 16/32-bit framebuffer path chosen with `-gfxmode
//! rgba8888|rgb565`. This port keeps one 32-bit path whose pixel layout
//! the platform decides ([`Platform::finish_pixel`]): `I_SetPalette`
//! turns the palette into 256 finished pixels, and [`convert_frame`]
//! only looks them up. The frame goes straight into the platform's own
//! buffer when it lends one ([`Platform::draw_in_place`]), else into a
//! buffer kept here for [`Platform::draw_frame`]. The C globals live in
//! [`VideoState`], held in the [`VIDEO`] global. Many of the stub entry
//! points (`I_BeginRead`, `I_BindVideoVariables`, ...) exist purely to
//! satisfy the engine's call sites without doing any work on the generic
//! platform.
//!
//! [`Platform::finish_pixel`]: crate::engine::platform::Platform::finish_pixel
//! [`Platform::draw_in_place`]: crate::engine::platform::Platform::draw_in_place
//! [`Platform::draw_frame`]: crate::engine::platform::Platform::draw_frame

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]
// `I_InitGraphics` and `I_ShutdownGraphics` call the zone allocator in C's
// whole-body `unsafe fn` style.
#![allow(unsafe_op_in_unsafe_fn)]

use {
    crate::{
        engine::{
            global::Global,
            interface::input::{I_GetEvent, I_InitInput},
            math::tables::gammatable,
            misc::argv::{M_CheckParmWithArgs, argument},
            platform::{DOOMGENERIC_PIXELS, DOOMGENERIC_RESX, DOOMGENERIC_RESY, platform},
            zone::{Z_Free, Z_Malloc},
        },
        types::Boolean,
    },
    alloc::vec::Vec,
    core::{ffi::c_void, mem, ptr, slice},
};

use crate::engine::zone::PU_STATIC;

/// Doom's logical screen width in palette-indexed pixels. The
/// renderer always paints into a `SCREENWIDTH * SCREENHEIGHT`
/// buffer; `I_FinishUpdate` scales/letterboxes from there.
pub const SCREENWIDTH: i32 = 320;

/// Doom's logical screen height in palette-indexed pixels.
pub const SCREENHEIGHT: i32 = 200;

/// The palette-indexed frame's size in pixels.
const SCREEN_PIXELS: usize = (SCREENWIDTH * SCREENHEIGHT) as usize;

/// One gamma-corrected palette entry. Mirrors `struct color` in
/// `i_video.c`; `I_SetPalette` turns each into a finished pixel and
/// `I_GetPaletteIndex` searches them.
#[derive(Clone, Copy)]
struct Color {
    b: u8,
    g: u8,
    r: u8,
    a: u8,
}

/// Video state (C `i_video.c` globals).
pub struct VideoState {
    /// Pointer to the palette-indexed Doom screen buffer. Allocated in
    /// the zone heap (`PU_STATIC`) during `I_InitGraphics`. Mirrors
    /// the C global `I_VideoBuffer`; read through [`video_buffer`].
    pub video_buffer: *mut u8,
    /// Non-zero when the screen is visible; the C source sets this to
    /// `true` after init and never clears it. Mirrors `screenvisible`.
    pub screenvisible: i32,
    /// Gamma-correction level index into `gammatable`. Mirrors `usegamma`.
    /// Bound via `m_config` (in the C source).
    pub usegamma: i32,
    /// Integer upscaling factor used by `I_FinishUpdate` to enlarge the
    /// 320x200 Doom buffer to fit the frame, at least 1. Picked
    /// automatically in `I_InitGraphics` or overridden with `-scaling
    /// <n>`. Mirrors `fb_scaling`.
    fb_scaling: usize,
    /// Active 256-entry gamma-corrected palette. Filled from the WAD
    /// PLAYPAL by `I_SetPalette`. Mirrors the C `colors[256]` static array.
    colors: [Color; 256],
    /// `colors` as the platform's finished pixels
    /// ([`Platform::finish_pixel`](crate::engine::platform::Platform::finish_pixel)),
    /// rebuilt by `I_SetPalette`; the frame conversion only looks them up.
    /// Replaces the C `s_Fb` pixel layout.
    finished: [u32; 256],
    /// The frame for a platform that lends no buffer, allocated at its
    /// first frame. Replaces doomgeneric's `DG_ScreenBuffer`.
    frame: Vec<u32>,
    /// Whether the platform lends its buffer
    /// ([`Platform::draw_in_place`](crate::engine::platform::Platform::draw_in_place)),
    /// learnt from the first frame.
    lends_buffer: bool,
    /// For a platform that lends its buffer: the palette-indexed frame it
    /// shows, to skip presenting the same picture again.
    presented: Vec<u8>,
    /// `finished` changed since the last presented frame.
    palette_changed: bool,
}

impl VideoState {
    /// The state before `I_InitGraphics`: no buffer, black palette.
    pub const fn new() -> Self {
        Self {
            video_buffer: ptr::null_mut(),
            screenvisible: 0,
            usegamma: 0,
            fb_scaling: 1,
            colors: [Color {
                b: 0,
                g: 0,
                r: 0,
                a: 0,
            }; 256],
            finished: [0; 256],
            frame: Vec::new(),
            lends_buffer: false,
            presented: Vec::new(),
            palette_changed: true,
        }
    }
}

impl Default for VideoState {
    fn default() -> Self {
        Self::new()
    }
}

/// The video state.
pub static VIDEO: Global<VideoState> = Global::new(VideoState::new());

/// The palette-indexed screen buffer (C `I_VideoBuffer`), null before
/// `I_InitGraphics`.
pub fn video_buffer() -> *mut u8 {
    VIDEO.borrow().video_buffer
}

/// Non-zero if the engine is running as a screensaver. Always 0 in
/// this port. Mirrors `screensaver_mode`.
pub const screensaver_mode: i32 = 0;

/// DOS-style mouse acceleration factor. Movement above
/// `mouse_threshold` is multiplied by this. Mirrors `mouse_acceleration`.
pub const mouse_acceleration: f32 = 2.0;

/// Mouse-movement threshold above which acceleration kicks in.
/// Mirrors `mouse_threshold`.
pub const mouse_threshold: i32 = 10;

/// Non-zero to enable mouse input. Always 0 in this port. Mirrors
/// `usemouse`.
pub const usemouse: i32 = 0;

/// The largest scale at which the 320x200 frame fits the platform's frame.
const fn fitting_scale() -> usize {
    let horizontal = DOOMGENERIC_RESX / SCREENWIDTH as usize;
    let vertical = DOOMGENERIC_RESY / SCREENHEIGHT as usize;
    if horizontal < vertical {
        horizontal
    } else {
        vertical
    }
}

/// Expand the palette-indexed `source` frame ([`SCREENWIDTH`] by
/// [`SCREENHEIGHT`]) through `palette` (finished pixels) into `pixels`,
/// whose rows are `pitch` pixels apart: each source pixel becomes
/// `scaling` by `scaling` pixels, centred in the [`DOOMGENERIC_RESX`] by
/// [`DOOMGENERIC_RESY`] frame. Mirrors the loops of `I_FinishUpdate` and
/// `cmap_to_fb` in `i_video.c`, except that each source line is converted
/// once and its first output row copied into the other `scaling - 1`.
///
/// Pixels outside the scaled picture are left as they are.
///
/// # Panics
///
/// If `source` is shorter than a frame, or `pixels` is too short for the
/// scaled picture at `pitch`.
fn convert_frame(
    source: &[u8],
    palette: &[u32; 256],
    scaling: usize,
    pixels: &mut [u32],
    pitch: usize,
) {
    let source_width = SCREENWIDTH as usize;
    let row_pixels = source_width * scaling;
    let left = DOOMGENERIC_RESX.saturating_sub(row_pixels) / 2;
    let top = DOOMGENERIC_RESY.saturating_sub(SCREENHEIGHT as usize * scaling) / 2;

    for (source_row, line) in source[..SCREEN_PIXELS]
        .chunks_exact(source_width)
        .enumerate()
    {
        let row_start = (top + source_row * scaling) * pitch + left;
        let (converted, below) = pixels.split_at_mut(row_start + row_pixels);
        let converted = &mut converted[row_start..];
        // Doom's own 2x (640x400 from 320x200) gets a pair store per pixel.
        if scaling == 2 {
            let (pairs, _) = converted.as_chunks_mut::<2>();
            for (pair, &index) in pairs.iter_mut().zip(line) {
                *pair = [palette[usize::from(index)]; 2];
            }
        } else {
            for (run, &index) in converted.chunks_exact_mut(scaling).zip(line) {
                run.fill(palette[usize::from(index)]);
            }
        }
        // `below` starts where the converted row ends, so the k-th copy
        // starts `k * pitch - row_pixels` into it.
        for copy in 1..scaling {
            let copy_start = copy * pitch - row_pixels;
            below[copy_start..copy_start + row_pixels].copy_from_slice(converted);
        }
    }
}

/// Initialise the video subsystem: compute the integer scaling factor
/// (`-scaling <n>` or auto-fit), allocate the 320x200 palette buffer,
/// mark the screen visible, and start input.
///
/// Mirrors `I_InitGraphics` from `i_video.c`. The Rust version reads
/// `-scaling` digits manually (no `atoi`), keeps the factor between 1 and
/// the largest that fits (C writes past the frame for a larger one),
/// skips the C `printf` diagnostics, and has no `-gfxmode`: the platform
/// decides the pixel layout ([`Platform::finish_pixel`]).
///
/// [`Platform::finish_pixel`]: crate::engine::platform::Platform::finish_pixel
///
/// # Safety
///
/// Sets the video state (`video_buffer`, `fb_scaling`, `screenvisible`)
/// and calls back into the zone allocator. Must be called exactly once at
/// startup before any other I_* graphics function.
pub unsafe fn I_InitGraphics() {
    // Auto-scaling factor
    let scale_parm = M_CheckParmWithArgs("-scaling", 1);
    let fb_scaling = if scale_parm != 0 {
        // Leading digits only (no sign or whitespace), as the C loop reads.
        argument(scale_parm + 1)
            .bytes()
            .take_while(u8::is_ascii_digit)
            .fold(0_usize, |scaling, digit| {
                scaling
                    .saturating_mul(10)
                    .saturating_add(usize::from(digit - b'0'))
            })
            .clamp(1, fitting_scale())
    } else {
        fitting_scale()
    };

    // Allocate video buffer
    let video_buffer = Z_Malloc(SCREENWIDTH * SCREENHEIGHT, PU_STATIC, ptr::null_mut()) as *mut u8;

    {
        let mut video = VIDEO.borrow_mut();
        video.fb_scaling = fb_scaling;
        video.video_buffer = video_buffer;
        video.screenvisible = 1;
    }

    I_InitInput();
}

/// Release the palette-indexed screen buffer. Mirrors
/// `I_ShutdownGraphics` from `i_video.c`. The Rust version also
/// nulls `I_VideoBuffer` so a subsequent stray dereference faults
/// immediately rather than touching freed zone memory.
///
/// # Safety
///
/// Calls into the zone allocator. After return, `I_VideoBuffer` is
/// null - callers must not access it without calling
/// `I_InitGraphics` again.
pub unsafe fn I_ShutdownGraphics() {
    Z_Free(video_buffer() as *mut c_void);
    VIDEO.borrow_mut().video_buffer = ptr::null_mut();
}

/// Per-frame "start" hook. No-op in this port (the C source also
/// does nothing here for the generic backend).
pub fn I_StartFrame() {}

/// Per-tic "start" hook. Pumps one input event via `I_GetEvent`.
/// Mirrors `I_StartTic` from `i_video.c`.
pub fn I_StartTic() {
    I_GetEvent();
}

/// Stub that originally allowed an SDL "no blit" intermediate stage.
/// Always a no-op for the generic backend.
pub fn I_UpdateNoBlit() {}

/// Blit and present one frame.
///
/// Converts the 320x200 palette buffer at `I_VideoBuffer` into the
/// platform's frame ([`convert_frame`]): straight into the buffer the
/// platform lends ([`Platform::draw_in_place`]), or into this module's
/// own buffer, which then goes to `DG_DrawFrame`
/// ([`Platform::draw_frame`]).
///
/// A platform that lends its buffer still shows the last frame, so when
/// neither the palette-indexed picture nor the palette changed since then
/// there is nothing to do and the frame is skipped. That happens when
/// `TryRunTics` returns without running a tic (it gives up waiting once
/// the clock reaches the next tic, so the menu stays live) and on screens
/// that stand still, such as the title page. A platform that takes
/// frames through `DG_DrawFrame` gets every frame, as in C.
///
/// [`Platform::draw_in_place`]: crate::engine::platform::Platform::draw_in_place
/// [`Platform::draw_frame`]: crate::engine::platform::Platform::draw_frame
///
/// # Safety
///
/// Reads `SCREENWIDTH * SCREENHEIGHT` bytes from `I_VideoBuffer`, which
/// `I_InitGraphics` must have allocated.
pub unsafe fn I_FinishUpdate() {
    // SAFETY: `I_InitGraphics` allocated `SCREEN_PIXELS` bytes there; the
    // conversion below only reads them.
    let source = unsafe { slice::from_raw_parts(video_buffer(), SCREEN_PIXELS) };
    let platform = platform();
    {
        let mut video = VIDEO.borrow_mut();
        if video.lends_buffer {
            if !video.palette_changed && video.presented == source {
                return;
            }
            video.presented.clear();
            video.presented.extend_from_slice(source);
        }
        video.palette_changed = false;
    }

    let lent = {
        let video = VIDEO.borrow();
        platform.draw_in_place(&mut |pixels, pitch| {
            convert_frame(source, &video.finished, video.fb_scaling, pixels, pitch);
        })
    };
    if lent {
        let mut video = VIDEO.borrow_mut();
        if !video.lends_buffer {
            video.lends_buffer = true;
            video.presented.extend_from_slice(source);
        }
        return;
    }

    // The platform's `draw_frame` gets the frame without the state borrowed.
    let mut frame = mem::take(&mut VIDEO.borrow_mut().frame);
    frame.resize(DOOMGENERIC_PIXELS, 0);
    {
        let video = VIDEO.borrow();
        convert_frame(
            source,
            &video.finished,
            video.fb_scaling,
            &mut frame,
            DOOMGENERIC_RESX,
        );
    }
    platform.draw_frame(&frame);
    VIDEO.borrow_mut().frame = frame;
}

/// Copy the entire palette-indexed screen buffer into `scr`. Used by
/// the wipe/melt screen-transition code. Mirrors `I_ReadScreen`.
///
/// # Safety
///
/// `scr` must be writable for `SCREENWIDTH * SCREENHEIGHT` bytes.
/// `I_VideoBuffer` must be initialised.
pub unsafe fn I_ReadScreen(scr: *mut u8) {
    core::ptr::copy(video_buffer(), scr, (SCREENWIDTH * SCREENHEIGHT) as usize);
}

/// Apply a new 768-byte (256 x R/G/B) PLAYPAL palette by
/// gamma-correcting each component through `gammatable[usegamma]`
/// and storing into `colors`, then turning each colour into the
/// platform's finished pixel
/// ([`Platform::finish_pixel`](crate::engine::platform::Platform::finish_pixel))
/// for the frame conversion. Mirrors `I_SetPalette` from `i_video.c`.
///
/// `palette` starts at the palette to apply; bytes after the first
/// `256 * 3` (the later PLAYPAL palettes) are ignored.
///
/// # Panics
///
/// If `palette` is shorter than `256 * 3` bytes, or `usegamma` is out of
/// range for `gammatable`.
pub fn I_SetPalette(palette: &[u8]) {
    assert!(palette.len() >= 256 * 3, "I_SetPalette: short palette");
    let platform = platform();
    let mut video = VIDEO.borrow_mut();
    let gamma = &gammatable[video.usegamma as usize];
    let (rgb_triples, _) = palette.as_chunks::<3>();
    for (color, &[red, green, blue]) in video.colors.iter_mut().zip(rgb_triples) {
        color.a = 0;
        color.r = gamma[red as usize];
        color.g = gamma[green as usize];
        color.b = gamma[blue as usize];
    }
    let colors = video.colors;
    let finished = colors.map(|color| platform.finish_pixel(color.r, color.g, color.b));
    if finished != video.finished {
        video.finished = finished;
        video.palette_changed = true;
    }
}

/// Return the palette index closest to the given RGB triple.
///
/// Mirrors `I_GetPaletteIndex` in `i_video.c`: linear scan over the
/// 256-entry active palette, returning the index with the smallest
/// squared RGB distance. Exits early on the first exact match. The C
/// source searches the gamma-corrected `rgb565_palette`; this port
/// searches the gamma-corrected `colors` array directly (no 5/6/5
/// quantization because the Rust framebuffer is 32 bpp).
///
/// The diff is accumulated in `i64` so out-of-range `r`/`g`/`b` values
/// (the C ABI accepts any `int`) cannot overflow even though all real
/// callers pass values in `0..=255`.
///
/// The result is only meaningful once `I_SetPalette` has run.
pub fn I_GetPaletteIndex(r: i32, g: i32, b: i32) -> i32 {
    let r = r as i64;
    let g = g as i64;
    let b = b as i64;
    // `best = 0` is a sound default only because the palette is
    // always 256 entries and the loop visits all of them.
    let mut best: i32 = 0;
    let mut best_diff: i64 = i64::MAX;
    let video = VIDEO.borrow();
    for (i, color) in video.colors.iter().enumerate() {
        let dr = r - color.r as i64;
        let dg = g - color.g as i64;
        let db = b - color.b as i64;
        let diff = dr * dr + dg * dg + db * db;
        if diff < best_diff {
            best = i as i32;
            best_diff = diff;
        }
        if diff == 0 {
            break;
        }
    }
    best
}

/// SDL-era hook to signal the start of a disk read so a "loading"
/// icon can be displayed. No-op in this port.
pub fn I_BeginRead() {}

/// SDL-era hook to signal the end of a disk read. No-op here.
pub fn I_EndRead() {}

/// Set the host-window title via [`Platform::set_window_title`](crate::engine::platform::Platform::set_window_title).
/// Mirrors `I_SetWindowTitle` from `i_video.c`.
pub fn I_SetWindowTitle(title: &str) {
    crate::engine::platform::platform().set_window_title(title);
}

/// Parse video-related command-line switches. No-op in this port;
/// the SDL backend in chocolate-doom handles things like `-window`
/// here.
pub fn I_GraphicsCheckCommandLine() {}

/// Register a callback that returns whether the mouse should be
/// grabbed by the window. No-op in this port (no mouse capture).
///
/// The callback argument is stored nowhere.
pub fn I_SetGrabMouseCallback(_func: fn() -> Boolean) {}

/// Enable the disk-activity icon overlay. No-op in this port.
pub fn I_EnableLoadingDisk() {}

/// Bind video-related `m_config` variables. No-op in this port;
/// chocolate-doom binds things like `fullscreen`, `aspect_ratio_correct`,
/// etc. here.
pub fn I_BindVideoVariables() {}

/// Toggle the on-screen FPS dot indicator. No-op in this port.
pub fn I_DisplayFPSDots(_dots_on: Boolean) {}

/// Detect whether the engine was launched as a screensaver. No-op
/// in this port; `screensaver_mode` stays 0.
pub fn I_CheckIsScreensaver() {}

#[cfg(test)]
mod tests {
    use {
        super::*,
        std::sync::{Mutex, MutexGuard},
    };

    /// The palette lives in the shared video state; the tests below
    /// mutate it, so they must run serially.
    static PALETTE_LOCK: Mutex<()> = Mutex::new(());

    /// RAII guard that owns the palette lock for the duration of a
    /// test and restores the palette snapshot captured at acquisition
    /// time on drop, even on panic. Keeps the global palette
    /// invisible to subsequent tests in the same binary.
    struct PaletteGuard {
        _lock: MutexGuard<'static, ()>,
        saved: [Color; 256],
    }

    impl PaletteGuard {
        fn acquire() -> Self {
            let lock = PALETTE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let saved = VIDEO.borrow().colors;
            Self { _lock: lock, saved }
        }
    }

    impl Drop for PaletteGuard {
        fn drop(&mut self) {
            VIDEO.borrow_mut().colors = self.saved;
        }
    }

    fn set_test_palette(entries: &[(u8, u8, u8)]) {
        let mut video = VIDEO.borrow_mut();
        for (i, color) in video.colors.iter_mut().enumerate() {
            let (r, g, b) = entries.get(i).copied().unwrap_or((0, 0, 0));
            *color = Color { b, g, r, a: 0 };
        }
    }

    /// Before `I_InitGraphics` there is no buffer and the scale is 1.
    #[test]
    fn new_state_has_no_buffer() {
        let video = VideoState::new();
        assert!(video.video_buffer.is_null());
        assert_eq!(video.screenvisible, 0);
        assert_eq!(video.usegamma, 0);
        assert_eq!(video.fb_scaling, 1);
        assert!(video.frame.is_empty());
        assert!(!video.lends_buffer);
    }

    /// A source frame whose pixel at (column, row) has palette index
    /// `(column + row) % 256`, and a palette whose pixel for index `i` is
    /// `0xff00_0000 | i`, so every output pixel names its source.
    fn numbered_frame() -> (Vec<u8>, [u32; 256]) {
        let width = SCREENWIDTH as usize;
        let source = (0..SCREEN_PIXELS)
            .map(|position| ((position % width + position / width) % 256) as u8)
            .collect();
        let palette = core::array::from_fn(|index| 0xff00_0000 | index as u32);
        (source, palette)
    }

    /// At Doom's 2x every source pixel covers a 2 by 2 block, rows land
    /// `pitch` apart, and the pixels past each row's end stay untouched.
    #[test]
    fn convert_frame_doubles_into_a_wider_pitch() {
        let (source, palette) = numbered_frame();
        let pitch = DOOMGENERIC_RESX + 16;
        let untouched = 0x5555_5555;
        let mut pixels = vec![untouched; pitch * DOOMGENERIC_RESY];
        convert_frame(&source, &palette, 2, &mut pixels, pitch);

        for row in 0..DOOMGENERIC_RESY {
            let line = &pixels[row * pitch..(row + 1) * pitch];
            for (column, &pixel) in line[..DOOMGENERIC_RESX].iter().enumerate() {
                let index = (column / 2 + row / 2) % 256;
                assert_eq!(pixel, palette[index], "pixel ({column}, {row})");
            }
            assert!(
                line[DOOMGENERIC_RESX..].iter().all(|&pixel| pixel == untouched),
                "padding of row {row}"
            );
        }
    }

    /// At 1x the picture is centred and the border is left alone.
    #[test]
    fn convert_frame_centres_an_unscaled_picture() {
        let (source, palette) = numbered_frame();
        let untouched = 0x5555_5555;
        let mut pixels = vec![untouched; DOOMGENERIC_PIXELS];
        convert_frame(&source, &palette, 1, &mut pixels, DOOMGENERIC_RESX);

        let left = (DOOMGENERIC_RESX - SCREENWIDTH as usize) / 2;
        let top = (DOOMGENERIC_RESY - SCREENHEIGHT as usize) / 2;
        for row in 0..DOOMGENERIC_RESY {
            for column in 0..DOOMGENERIC_RESX {
                let pixel = pixels[row * DOOMGENERIC_RESX + column];
                let inside = (left..left + SCREENWIDTH as usize).contains(&column)
                    && (top..top + SCREENHEIGHT as usize).contains(&row);
                let expected = if inside {
                    palette[(column - left + row - top) % 256]
                } else {
                    untouched
                };
                assert_eq!(pixel, expected, "pixel ({column}, {row})");
            }
        }
    }

    /// The default fits Doom's frame twice into 640x400.
    #[test]
    fn fitting_scale_is_two() {
        assert_eq!(fitting_scale(), 2);
    }

    /// `I_SetPalette` keeps the platform's finished pixels (the default
    /// platform's `0x00RRGGBB`) and marks the palette changed only when
    /// they differ.
    #[test]
    fn set_palette_finishes_pixels() {
        let _guard = PaletteGuard::acquire();
        let saved_finished = VIDEO.borrow().finished;
        let mut palette = [0_u8; 256 * 3];
        palette[3..6].copy_from_slice(&[0x12, 0x34, 0x56]);
        let gamma = &gammatable[VIDEO.borrow().usegamma as usize];
        let expected = u32::from(gamma[0x12]) << 16
            | u32::from(gamma[0x34]) << 8
            | u32::from(gamma[0x56]);

        VIDEO.borrow_mut().palette_changed = false;
        VIDEO.borrow_mut().finished = [1; 256];
        I_SetPalette(&palette);
        assert_eq!(VIDEO.borrow().finished[1], expected);
        assert!(VIDEO.borrow().palette_changed);

        VIDEO.borrow_mut().palette_changed = false;
        I_SetPalette(&palette);
        assert!(!VIDEO.borrow().palette_changed, "same palette again");

        VIDEO.borrow_mut().finished = saved_finished;
    }

    #[test]
    fn get_palette_index_nearest_rgb() {
        let _guard = PaletteGuard::acquire();
        let palette: [(u8, u8, u8); 4] = [(0, 0, 0), (255, 0, 0), (0, 255, 0), (0, 0, 255)];
        set_test_palette(&palette);

        assert_eq!(I_GetPaletteIndex(0, 0, 0), 0, "exact black");
        assert_eq!(I_GetPaletteIndex(255, 0, 0), 1, "exact red");
        assert_eq!(I_GetPaletteIndex(0, 255, 0), 2, "exact green");
        assert_eq!(I_GetPaletteIndex(0, 0, 255), 3, "exact blue");

        assert_eq!(I_GetPaletteIndex(250, 5, 5), 1, "near red");
        assert_eq!(I_GetPaletteIndex(5, 250, 5), 2, "near green");
        assert_eq!(I_GetPaletteIndex(10, 10, 10), 0, "near black");

        assert_eq!(
            I_GetPaletteIndex(128, 0, 0),
            1,
            "128 is one step closer to 255 (diff 127^2) than to 0 (diff 128^2)"
        );
        assert_eq!(
            I_GetPaletteIndex(127, 0, 0),
            0,
            "127 is one step closer to 0 than to 255"
        );

        // Out-of-range inputs must not overflow the diff accumulator.
        assert_eq!(I_GetPaletteIndex(i32::MAX, 0, 0), 1, "saturate to red");
        assert_eq!(I_GetPaletteIndex(i32::MIN, 0, 0), 0, "saturate to black");
    }

    #[test]
    fn get_palette_index_returns_first_exact_match() {
        let _guard = PaletteGuard::acquire();

        // Index 42 is the target; index 200 is a duplicate. Strict
        // `diff < best_diff` plus the outer-scope `if diff == 0
        // { break }` together guarantee the first exact match wins
        // and the loop terminates there; this test pins the
        // first-match contract so regressions in either branch show
        // up as a wrong index.
        let mut palette: Vec<(u8, u8, u8)> = (0..256).map(|_| (200, 0, 0)).collect();
        palette[42] = (42, 0, 0);
        palette[200] = (42, 0, 0);
        set_test_palette(&palette);

        assert_eq!(I_GetPaletteIndex(42, 0, 0), 42);
    }
}
