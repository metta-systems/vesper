//! Rust port of vendor/doomgeneric/i_video.c.
//!
//! Video output shim: allocates the palette-indexed `I_VideoBuffer`,
//! expands it through the gamma-corrected palette into BGRA
//! `DG_ScreenBuffer` on `I_FinishUpdate`, and forwards calls to the
//! platform layer.
//!
//! The C source supports two compile-time paths: an 8-bit `CMAP256`
//! direct-blit and a 16/32-bit framebuffer path. Only the latter is
//! ported here. The C globals live in [`VideoState`], held in the
//! [`VIDEO`] global. Its `screen_info` (C `s_Fb`) tracks pixel layout chosen at
//! init time via `-gfxmode rgba8888|rgb565`; `cmap_to_fb` /
//! `cmap_to_rgb565` perform the conversion from palette indices to
//! the chosen format. Many of the stub entry points
//! (`I_BeginRead`, `I_BindVideoVariables`, ...) exist purely to
//! satisfy the engine's call sites without doing any work on the
//! generic platform.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]
// The framebuffer conversion writes through raw zone pointers in C's
// whole-body `unsafe fn` style.
#![allow(unsafe_op_in_unsafe_fn)]

use {
    crate::{
        engine::{
            global::Global,
            interface::input::{I_GetEvent, I_InitInput},
            math::tables::gammatable,
            misc::argv::{M_CheckParmWithArgs, argument},
            zone::{Z_Free, Z_Malloc},
        },
        i_error,
    },
    std::ffi::c_void,
};

use {
    crate::types::Boolean,
    std::{mem, ptr},
};

use crate::engine::zone::PU_STATIC;

/// Doom's logical screen width in palette-indexed pixels. The
/// renderer always paints into a `SCREENWIDTH * SCREENHEIGHT`
/// buffer; `I_FinishUpdate` scales/letterboxes from there.
pub const SCREENWIDTH: i32 = 320;

/// Doom's logical screen height in palette-indexed pixels.
pub const SCREENHEIGHT: i32 = 200;

/// One gamma-corrected palette entry. Mirrors `struct color` in
/// `i_video.c`; the channels are read by name and packed into
/// framebuffer pixels by `cmap_to_fb` / `cmap_to_rgb565`.
#[derive(Clone, Copy)]
struct Color {
    b: u8,
    g: u8,
    r: u8,
    a: u8,
}

/// Bit-field descriptor for one colour channel inside the
/// framebuffer pixel layout. Mirrors `struct FB_BitField` in
/// `i_video.c`.
struct FB_BitField {
    /// Bit position of the channel's least-significant bit within a pixel.
    offset: u32,
    /// Number of bits the channel occupies.
    length: u32,
}

/// Framebuffer layout descriptor populated by `I_InitGraphics`.
/// Mirrors `struct FB_ScreenInfo` in `i_video.c`.
struct FB_ScreenInfo {
    /// Visible framebuffer width in pixels.
    xres: u32,
    /// Visible framebuffer height in pixels.
    yres: u32,
    /// Allocated framebuffer width (>= `xres`).
    xres_virtual: u32,
    /// Allocated framebuffer height (>= `yres`).
    yres_virtual: u32,
    /// Pixel size in bits. Only 16 and 32 are supported.
    bits_per_pixel: u32,
    /// Red channel position and width.
    red: FB_BitField,
    /// Green channel position and width.
    green: FB_BitField,
    /// Blue channel position and width.
    blue: FB_BitField,
    /// Alpha / transparency channel position and width.
    transp: FB_BitField,
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
    /// 320x200 Doom buffer to fit the framebuffer. Picked automatically
    /// in `I_InitGraphics` or overridden with `-scaling <n>`. Mirrors
    /// `fb_scaling`.
    fb_scaling: i32,
    /// Active 256-entry gamma-corrected palette. Filled from the WAD
    /// PLAYPAL by `I_SetPalette` and consumed by `cmap_to_fb` /
    /// `cmap_to_rgb565`. Mirrors the C `colors[256]` static array.
    colors: [Color; 256],
    /// Active framebuffer layout. Initialised by `I_InitGraphics`,
    /// consumed by `cmap_to_fb` / `I_FinishUpdate`. Mirrors the C file
    /// static `s_Fb`.
    screen_info: FB_ScreenInfo,
}

impl VideoState {
    /// The state before `I_InitGraphics`: no buffer, black palette.
    pub const fn new() -> Self {
        const NO_BITS: FB_BitField = FB_BitField {
            offset: 0,
            length: 0,
        };
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
            screen_info: FB_ScreenInfo {
                xres: 0,
                yres: 0,
                xres_virtual: 0,
                yres_virtual: 0,
                bits_per_pixel: 0,
                red: NO_BITS,
                green: NO_BITS,
                blue: NO_BITS,
                transp: NO_BITS,
            },
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

/// Convert `in_pixels` palette indices at `inp` into framebuffer
/// pixels at `out`, picking 16-bpp (delegates to `cmap_to_rgb565`)
/// or 32-bpp packing based on `screen_info.bits_per_pixel`. Honours
/// `fb_scaling` by writing each pixel that many times.
///
/// # Safety
///
/// `inp` must be readable for `in_pixels` bytes. `out` must be
/// writable for `in_pixels * fb_scaling * (bpp/8)` bytes.
unsafe fn cmap_to_fb(video: &VideoState, out: *mut u8, inp: *mut u8, in_pixels: i32) {
    let screen_info = &video.screen_info;
    let bpp = screen_info.bits_per_pixel;

    if bpp == 16 {
        cmap_to_rgb565(video, out, inp, in_pixels);
        return;
    }

    // FIXME: C i_video.c calls I_Error for any bpp other than 16/32.
    // This port silently treats any non-16 value as 32-bpp, which
    // would write garbage rather than crashing if `bits_per_pixel`
    // is corrupted.
    if bpp != 32 {
        // For safety, default to 32bpp path
    }

    let mut out_ptr = out;
    let mut inp_ptr = inp;

    for _ in 0..in_pixels {
        let idx = *inp_ptr as usize;
        let c = video.colors[idx];

        let pix = ((c.r as u32) << screen_info.red.offset)
            | ((c.g as u32) << screen_info.green.offset)
            | ((c.b as u32) << screen_info.blue.offset);

        for _ in 0..video.fb_scaling {
            (out_ptr as *mut u32).write_unaligned(pix);
            out_ptr = out_ptr.add(4);
        }

        inp_ptr = inp_ptr.add(1);
    }
}

/// Convert `in_pixels` palette indices at `inp` into RGB565
/// little-endian pixels at `out`. Each output pixel is written
/// `fb_scaling` times.
///
/// # Safety
///
/// Same preconditions as `cmap_to_fb` (with 2-byte pixels).
unsafe fn cmap_to_rgb565(video: &VideoState, out: *mut u8, inp: *mut u8, in_pixels: i32) {
    let mut out_ptr = out as *mut u16;
    let mut inp_ptr = inp;

    for _ in 0..in_pixels {
        let idx = *inp_ptr as usize;
        let c = video.colors[idx];

        let p = (((c.r as u16) & 0xF8) << 8) | (((c.g as u16) & 0xFC) << 3) | ((c.b as u16) >> 3);

        for _ in 0..video.fb_scaling {
            *out_ptr = p;
            out_ptr = out_ptr.add(1);
        }

        inp_ptr = inp_ptr.add(1);
    }
}

/// Initialise the video subsystem: pick the framebuffer pixel layout
/// (`-gfxmode rgba8888|rgb565`, default rgba8888), compute the
/// integer scaling factor (`-scaling <n>` or auto-fit), allocate the
/// 320x200 palette buffer, mark the screen visible, and start input.
///
/// Mirrors `I_InitGraphics` from `i_video.c`. The Rust version
/// reads `-scaling` digits manually (no `atoi`) and skips the C
/// `printf` diagnostics.
///
/// # Safety
///
/// Sets the video state (`screen_info`, `video_buffer`, `fb_scaling`,
/// `screenvisible`) and calls back into the zone
/// allocator. Must be called exactly once at startup before any
/// other I_* graphics function.
pub unsafe fn I_InitGraphics() {
    let mut screen_info = mem::zeroed::<FB_ScreenInfo>();
    screen_info.xres = crate::engine::platform::DOOMGENERIC_RESX as u32;
    screen_info.yres = crate::engine::platform::DOOMGENERIC_RESY as u32;
    screen_info.xres_virtual = screen_info.xres;
    screen_info.yres_virtual = screen_info.yres;

    // Default to rgba8888
    screen_info.bits_per_pixel = 32;
    screen_info.blue.length = 8;
    screen_info.green.length = 8;
    screen_info.red.length = 8;
    screen_info.transp.length = 8;
    screen_info.blue.offset = 0;
    screen_info.green.offset = 8;
    screen_info.red.offset = 16;
    screen_info.transp.offset = 24;

    // Check for -gfxmode arg
    let gfxmodeparm = M_CheckParmWithArgs("-gfxmode", 1);
    if gfxmodeparm != 0 {
        let mode = argument(gfxmodeparm + 1);
        if mode == "rgba8888" {
            screen_info.bits_per_pixel = 32;
            screen_info.blue.length = 8;
            screen_info.green.length = 8;
            screen_info.red.length = 8;
            screen_info.transp.length = 8;
            screen_info.blue.offset = 0;
            screen_info.green.offset = 8;
            screen_info.red.offset = 16;
            screen_info.transp.offset = 24;
        } else if mode == "rgb565" {
            screen_info.bits_per_pixel = 16;
            screen_info.blue.length = 5;
            screen_info.green.length = 6;
            screen_info.red.length = 5;
            screen_info.transp.length = 0;
            screen_info.blue.offset = 11;
            screen_info.green.offset = 5;
            screen_info.red.offset = 0;
            screen_info.transp.offset = 16;
        } else {
            i_error!("Unknown gfxmode value: {mode}\n");
        }
    }

    // Auto-scaling factor
    let scale_parm = M_CheckParmWithArgs("-scaling", 1);
    let fb_scaling = if scale_parm != 0 {
        // Leading digits only (no sign or whitespace), as the C loop reads.
        argument(scale_parm + 1)
            .bytes()
            .take_while(u8::is_ascii_digit)
            .fold(0, |scaling, digit| scaling * 10 + i32::from(digit - b'0'))
    } else {
        let x_scale = (screen_info.xres / SCREENWIDTH as u32) as i32;
        let y_scale = (screen_info.yres / SCREENHEIGHT as u32) as i32;
        x_scale.min(y_scale)
    };

    // Allocate video buffer
    let video_buffer = Z_Malloc(SCREENWIDTH * SCREENHEIGHT, PU_STATIC, ptr::null_mut()) as *mut u8;

    {
        let mut video = VIDEO.borrow_mut();
        video.screen_info = screen_info;
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
/// final framebuffer at `DG_ScreenBuffer`, centring it inside the
/// configured `screen_info.xres x screen_info.yres` window with `x_offset` and
/// `y_offset` padding bytes per row, repeating each Doom scanline
/// `fb_scaling` times to upscale. After the conversion, the platform
/// callback `DG_DrawFrame` is invoked to present.
///
/// The padding math saturates at zero (`.max(0)`) so a framebuffer
/// smaller than the scaled image still produces a valid pointer
/// arithmetic - corresponding C code performs unsigned subtraction
/// and would wrap around in that case.
///
/// # Safety
///
/// Reads `SCREENWIDTH * SCREENHEIGHT` bytes from `I_VideoBuffer` and
/// writes a region of `DG_ScreenBuffer` whose extent depends on the
/// framebuffer geometry. Caller must ensure both pointers are valid
/// and the framebuffer is large enough for the configured layout.
pub unsafe fn I_FinishUpdate() {
    let screen_buffer = crate::engine::platform::screen_buffer();
    {
        let video = VIDEO.borrow();
        let screen_info = &video.screen_info;
        let fb_scaling = video.fb_scaling;
        let bytes_per_pixel = screen_info.bits_per_pixel as i32 / 8;
        let y_offset = (((screen_info.yres as i32 - (SCREENHEIGHT * fb_scaling)) * bytes_per_pixel)
            / 2)
        .max(0) as u32;
        let x_offset = (((screen_info.xres as i32 - (SCREENWIDTH * fb_scaling)) * bytes_per_pixel)
            / 2)
        .max(0) as u32;
        let x_offset_end = ((screen_info.xres as i32 - (SCREENWIDTH * fb_scaling))
            * bytes_per_pixel
            - x_offset as i32)
            .max(0) as u32;

        let mut line_in = video.video_buffer;
        let screen_ptr = screen_buffer as *mut u8;
        let mut line_out = screen_ptr.add(y_offset as usize + x_offset as usize);

        for _ in 0..SCREENHEIGHT as usize {
            for _i in 0..fb_scaling {
                line_out = line_out.add(x_offset as usize);

                cmap_to_fb(&video, line_out, line_in, SCREENWIDTH);

                line_out = line_out.add(
                    (SCREENWIDTH * fb_scaling * bytes_per_pixel) as usize + x_offset_end as usize,
                );
            }
            line_in = line_in.add(SCREENWIDTH as usize);
        }
    }

    let frame =
        std::slice::from_raw_parts(screen_buffer, crate::engine::platform::DOOMGENERIC_PIXELS);
    crate::engine::platform::platform().draw_frame(frame);
}

/// Copy the entire palette-indexed screen buffer into `scr`. Used by
/// the wipe/melt screen-transition code. Mirrors `I_ReadScreen`.
///
/// # Safety
///
/// `scr` must be writable for `SCREENWIDTH * SCREENHEIGHT` bytes.
/// `I_VideoBuffer` must be initialised.
pub unsafe fn I_ReadScreen(scr: *mut u8) {
    std::ptr::copy(video_buffer(), scr, (SCREENWIDTH * SCREENHEIGHT) as usize);
}

/// Apply a new 768-byte (256 x R/G/B) PLAYPAL palette by
/// gamma-correcting each component through `gammatable[usegamma]`
/// and storing into `colors`. Mirrors `I_SetPalette` from `i_video.c`.
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
    let mut video = VIDEO.borrow_mut();
    let gamma = &gammatable[video.usegamma as usize];
    let (rgb_triples, _) = palette.as_chunks::<3>();
    for (color, &[red, green, blue]) in video.colors.iter_mut().zip(rgb_triples) {
        color.a = 0;
        color.r = gamma[red as usize];
        color.g = gamma[green as usize];
        color.b = gamma[blue as usize];
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
