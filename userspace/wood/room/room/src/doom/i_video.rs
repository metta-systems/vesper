//! Rust port of vendor/doomgeneric/i_video.c.
//!
//! Video output shim: allocates the palette-indexed `I_VideoBuffer`,
//! expands it through the gamma-corrected palette into BGRA
//! `DG_ScreenBuffer` on `I_FinishUpdate`, and forwards calls to the
//! platform layer.
//!
//! The C source supports two compile-time paths: an 8-bit `CMAP256`
//! direct-blit and a 16/32-bit framebuffer path. Only the latter is
//! ported here. The `s_Fb` structure tracks pixel layout chosen at
//! init time via `-gfxmode rgba8888|rgb565`; `cmap_to_fb` /
//! `cmap_to_rgb565` perform the conversion from palette indices to
//! the chosen format. Many of the stub entry points
//! (`I_BeginRead`, `I_BindVideoVariables`, ...) exist purely to
//! satisfy the engine's call sites without doing any work on the
//! generic platform.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::doom::i_input::{I_GetEvent, I_InitInput};
use crate::doom::m_argv::{myargv, M_CheckParmWithArgs};
use crate::doom::tables::gammatable;
use crate::doom::z_zone::{Z_Free, Z_Malloc};
use crate::i_error;
use std::ffi::{c_char, c_float, c_int, c_void};

use crate::types::Boolean;
use std::mem;
use std::ptr;

use super::z_zone::PU_STATIC;

/// Doom's logical screen width in palette-indexed pixels. The
/// renderer always paints into a `SCREENWIDTH * SCREENHEIGHT`
/// buffer; `I_FinishUpdate` scales/letterboxes from there.
pub const SCREENWIDTH: c_int = 320;

/// Doom's logical screen height in palette-indexed pixels.
pub const SCREENHEIGHT: c_int = 200;

/// 32-bit pixel as the framebuffer expects it: BGRA with alpha last.
///
/// Mirrors `struct color` in `i_video.c`. `#[repr(C)]` because
/// pointers into this layout cross the FFI boundary (the
/// `DG_ScreenBuffer` is written byte by byte but interpreted as
/// 32-bit pixels by the host platform's display code).
#[repr(C)]
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

/// Pointer to the palette-indexed Doom screen buffer. Allocated in
/// the zone heap (`PU_STATIC`) during `I_InitGraphics`. Mirrors
/// the C global `I_VideoBuffer`.
#[no_mangle]
pub static mut I_VideoBuffer: *mut u8 = ptr::null_mut();

/// Non-zero when the screen is visible; the C source sets this to
/// `true` after init and never clears it. Mirrors `screenvisible`.
#[no_mangle]
pub static mut screenvisible: c_int = 0;

/// Non-zero if the engine is running as a screensaver. Always 0 in
/// this port. Mirrors `screensaver_mode`.
#[no_mangle]
pub static mut screensaver_mode: c_int = 0;

/// Gamma-correction level index into `gammatable`. Mirrors `usegamma`.
/// Bound via `m_config` (in the C source).
#[no_mangle]
pub static mut usegamma: c_int = 0;

/// DOS-style mouse acceleration factor. Movement above
/// `mouse_threshold` is multiplied by this. Mirrors `mouse_acceleration`.
#[no_mangle]
pub static mut mouse_acceleration: c_float = 2.0;

/// Mouse-movement threshold above which acceleration kicks in.
/// Mirrors `mouse_threshold`.
#[no_mangle]
pub static mut mouse_threshold: c_int = 10;

/// Integer upscaling factor used by `I_FinishUpdate` to enlarge the
/// 320x200 Doom buffer to fit the framebuffer. Picked automatically
/// in `I_InitGraphics` or overridden with `-scaling <n>`.
#[no_mangle]
pub static mut fb_scaling: c_int = 1;

/// Non-zero to enable mouse input. Always 0 in this port unless set
/// by config. Mirrors `usemouse`.
#[no_mangle]
pub static mut usemouse: c_int = 0;

/// Active 256-entry gamma-corrected palette. Filled from the WAD
/// PLAYPAL by `I_SetPalette` and consumed by `cmap_to_fb` /
/// `cmap_to_rgb565`. Mirrors the C `colors[256]` static array.
static mut COLORS: [Color; 256] = [Color {
    b: 0,
    g: 0,
    r: 0,
    a: 0,
}; 256];

/// Active framebuffer layout. Initialised by `I_InitGraphics`,
/// consumed by `cmap_to_fb` / `I_FinishUpdate`. Mirrors the C file
/// static `s_Fb`.
static mut s_Fb: FB_ScreenInfo = FB_ScreenInfo {
    xres: 0,
    yres: 0,
    xres_virtual: 0,
    yres_virtual: 0,
    bits_per_pixel: 0,
    red: FB_BitField {
        offset: 0,
        length: 0,
    },
    green: FB_BitField {
        offset: 0,
        length: 0,
    },
    blue: FB_BitField {
        offset: 0,
        length: 0,
    },
    transp: FB_BitField {
        offset: 0,
        length: 0,
    },
};

extern "C" {
    /// Platform hook called once per frame after `I_FinishUpdate` has
    /// filled `DG_ScreenBuffer`. Implemented by the host front-end
    /// (GPU / headless / Wayland backend).
    fn DG_DrawFrame();
    /// Platform hook called from `I_SetWindowTitle` to update the
    /// host window title with a NUL-terminated C string.
    fn DG_SetWindowTitle(title: *const c_char);
}

/// Convert `in_pixels` palette indices at `inp` into framebuffer
/// pixels at `out`, picking 16-bpp (delegates to `cmap_to_rgb565`)
/// or 32-bpp packing based on `s_Fb.bits_per_pixel`. Honours
/// `fb_scaling` by writing each pixel that many times.
///
/// # Safety
///
/// `inp` must be readable for `in_pixels` bytes. `out` must be
/// writable for `in_pixels * fb_scaling * (bpp/8)` bytes. The
/// caller is responsible for keeping `s_Fb` and `COLORS` valid.
unsafe fn cmap_to_fb(out: *mut u8, inp: *mut u8, in_pixels: c_int) {
    let bpp = s_Fb.bits_per_pixel;

    if bpp == 16 {
        cmap_to_rgb565(out, inp, in_pixels);
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
        let c = COLORS[idx];

        let pix = ((c.r as u32) << s_Fb.red.offset)
            | ((c.g as u32) << s_Fb.green.offset)
            | ((c.b as u32) << s_Fb.blue.offset);

        for _ in 0..fb_scaling {
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
unsafe fn cmap_to_rgb565(out: *mut u8, inp: *mut u8, in_pixels: c_int) {
    let mut out_ptr = out as *mut u16;
    let mut inp_ptr = inp;

    for _ in 0..in_pixels {
        let idx = *inp_ptr as usize;
        let c = COLORS[idx];

        let p = (((c.r as u16) & 0xF8) << 8) | (((c.g as u16) & 0xFC) << 3) | ((c.b as u16) >> 3);

        for _ in 0..fb_scaling {
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
/// Touches a number of mutable statics (`s_Fb`, `I_VideoBuffer`,
/// `fb_scaling`, `screenvisible`) and calls back into the zone
/// allocator. Must be called exactly once at startup before any
/// other I_* graphics function.
#[no_mangle]
pub unsafe extern "C" fn I_InitGraphics() {
    s_Fb = mem::zeroed::<FB_ScreenInfo>();
    s_Fb.xres = super::doomgeneric::DOOMGENERIC_RESX as u32;
    s_Fb.yres = super::doomgeneric::DOOMGENERIC_RESY as u32;
    s_Fb.xres_virtual = s_Fb.xres;
    s_Fb.yres_virtual = s_Fb.yres;

    // Default to rgba8888
    s_Fb.bits_per_pixel = 32;
    s_Fb.blue.length = 8;
    s_Fb.green.length = 8;
    s_Fb.red.length = 8;
    s_Fb.transp.length = 8;
    s_Fb.blue.offset = 0;
    s_Fb.green.offset = 8;
    s_Fb.red.offset = 16;
    s_Fb.transp.offset = 24;

    // Check for -gfxmode arg
    let gfxmodeparm = M_CheckParmWithArgs(c"-gfxmode".as_ptr().cast_mut(), 1);
    if gfxmodeparm != 0 {
        let mode = *myargv.add((gfxmodeparm + 1) as usize);
        if !mode.is_null() {
            let mode_str = std::ffi::CStr::from_ptr(mode);
            if mode_str.to_bytes() == b"rgba8888" {
                s_Fb.bits_per_pixel = 32;
                s_Fb.blue.length = 8;
                s_Fb.green.length = 8;
                s_Fb.red.length = 8;
                s_Fb.transp.length = 8;
                s_Fb.blue.offset = 0;
                s_Fb.green.offset = 8;
                s_Fb.red.offset = 16;
                s_Fb.transp.offset = 24;
            } else if mode_str.to_bytes() == b"rgb565" {
                s_Fb.bits_per_pixel = 16;
                s_Fb.blue.length = 5;
                s_Fb.green.length = 6;
                s_Fb.red.length = 5;
                s_Fb.transp.length = 0;
                s_Fb.blue.offset = 11;
                s_Fb.green.offset = 5;
                s_Fb.red.offset = 0;
                s_Fb.transp.offset = 16;
            } else {
                i_error!(
                    "Unknown gfxmode value: {}\n",
                    std::ffi::CStr::from_ptr(mode).to_string_lossy()
                );
            }
        }
    }

    // Auto-scaling factor
    let scale_parm = M_CheckParmWithArgs(c"-scaling".as_ptr().cast_mut(), 1);
    if scale_parm != 0 {
        let val = *myargv.add((scale_parm + 1) as usize);
        if !val.is_null() {
            // Simple atoi-like conversion
            let mut n: c_int = 0;
            let mut p = val;
            while *p >= b'0' as c_char && *p <= b'9' as c_char {
                n = n * 10 + (*p as c_int - b'0' as c_int);
                p = p.add(1);
            }
            fb_scaling = n;
        }
    } else {
        fb_scaling = (s_Fb.xres / SCREENWIDTH as u32) as c_int;
        let y_scale = (s_Fb.yres / SCREENHEIGHT as u32) as c_int;
        if y_scale < fb_scaling {
            fb_scaling = y_scale;
        }
    }

    // Allocate video buffer
    I_VideoBuffer = Z_Malloc(SCREENWIDTH * SCREENHEIGHT, PU_STATIC, ptr::null_mut()) as *mut u8;

    screenvisible = 1;

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
#[no_mangle]
pub unsafe extern "C" fn I_ShutdownGraphics() {
    Z_Free(I_VideoBuffer as *mut c_void);
    I_VideoBuffer = ptr::null_mut();
}

/// Per-frame "start" hook. No-op in this port (the C source also
/// does nothing here for the generic backend).
///
/// # Safety
///
/// Trivially safe; declared `unsafe extern "C"` to match the
/// engine's expected signature.
#[no_mangle]
pub unsafe extern "C" fn I_StartFrame() {}

/// Per-tic "start" hook. Pumps one input event via `I_GetEvent`.
/// Mirrors `I_StartTic` from `i_video.c`.
///
/// # Safety
///
/// Forwards to `I_GetEvent`, which dereferences input-queue globals.
#[no_mangle]
pub unsafe extern "C" fn I_StartTic() {
    I_GetEvent();
}

/// Stub that originally allowed an SDL "no blit" intermediate stage.
/// Always a no-op for the generic backend.
///
/// # Safety
///
/// Trivially safe.
#[no_mangle]
pub unsafe extern "C" fn I_UpdateNoBlit() {}

/// Blit and present one frame.
///
/// Converts the 320x200 palette buffer at `I_VideoBuffer` into the
/// final framebuffer at `DG_ScreenBuffer`, centring it inside the
/// configured `s_Fb.xres x s_Fb.yres` window with `x_offset` and
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
#[no_mangle]
pub unsafe extern "C" fn I_FinishUpdate() {
    let y_offset = (((s_Fb.yres as i32 - (SCREENHEIGHT as c_int * fb_scaling))
        * (s_Fb.bits_per_pixel as c_int / 8))
        / 2)
    .max(0) as u32;
    let x_offset = (((s_Fb.xres as i32 - (SCREENWIDTH as c_int * fb_scaling))
        * (s_Fb.bits_per_pixel as c_int / 8))
        / 2)
    .max(0) as u32;
    let x_offset_end = ((s_Fb.xres as i32 - (SCREENWIDTH as c_int * fb_scaling))
        * (s_Fb.bits_per_pixel as c_int / 8)
        - x_offset as i32)
        .max(0) as u32;

    let mut line_in = I_VideoBuffer;
    let screen_ptr = super::doomgeneric::DG_ScreenBuffer as *mut u8;
    let mut line_out = screen_ptr.add(y_offset as usize + x_offset as usize);

    for _ in 0..SCREENHEIGHT as usize {
        for _i in 0..fb_scaling {
            line_out = line_out.add(x_offset as usize);

            cmap_to_fb(line_out, line_in, SCREENWIDTH as c_int);

            line_out = line_out.add(
                (SCREENWIDTH as c_int * fb_scaling * (s_Fb.bits_per_pixel as c_int / 8)) as usize
                    + x_offset_end as usize,
            );
        }
        line_in = line_in.add(SCREENWIDTH as usize);
    }

    DG_DrawFrame();
}

/// Copy the entire palette-indexed screen buffer into `scr`. Used by
/// the wipe/melt screen-transition code. Mirrors `I_ReadScreen`.
///
/// # Safety
///
/// `scr` must be writable for `SCREENWIDTH * SCREENHEIGHT` bytes.
/// `I_VideoBuffer` must be initialised.
#[no_mangle]
pub unsafe extern "C" fn I_ReadScreen(scr: *mut u8) {
    std::ptr::copy(I_VideoBuffer, scr, (SCREENWIDTH * SCREENHEIGHT) as usize);
}

/// Apply a new 768-byte (256 x R/G/B) PLAYPAL palette by
/// gamma-correcting each component through `gammatable[usegamma]`
/// and storing into `COLORS`. Mirrors `I_SetPalette` from `i_video.c`.
///
/// # Safety
///
/// `palette` must point to at least `256 * 3` readable bytes. The
/// function trusts `usegamma` to index `gammatable` in range.
#[no_mangle]
pub unsafe extern "C" fn I_SetPalette(palette: *mut u8) {
    let mut p = palette;
    for i in 0..256 {
        COLORS[i].a = 0;
        COLORS[i].r = gammatable[usegamma as usize][*p as usize];
        p = p.add(1);
        COLORS[i].g = gammatable[usegamma as usize][*p as usize];
        p = p.add(1);
        COLORS[i].b = gammatable[usegamma as usize][*p as usize];
        p = p.add(1);
    }
}

/// Return the palette index closest to the given RGB triple.
///
/// Mirrors `I_GetPaletteIndex` in `i_video.c`: linear scan over the
/// 256-entry active palette, returning the index with the smallest
/// squared RGB distance. Exits early on the first exact match. The C
/// source searches the gamma-corrected `rgb565_palette`; this port
/// searches the gamma-corrected `COLORS` array directly (no 5/6/5
/// quantization because the Rust framebuffer is 32 bpp).
///
/// The diff is accumulated in `i64` so out-of-range `r`/`g`/`b` values
/// (the C ABI accepts any `int`) cannot overflow even though all real
/// callers pass values in `0..=255`.
///
/// # Safety
///
/// Reads `static mut COLORS`. The caller must have run `I_SetPalette`
/// at least once and must not race a concurrent writer.
#[no_mangle]
pub unsafe extern "C" fn I_GetPaletteIndex(r: c_int, g: c_int, b: c_int) -> c_int {
    let r = r as i64;
    let g = g as i64;
    let b = b as i64;
    // `best = 0` is a sound default only because the palette is
    // always 256 entries and the loop visits all of them.
    let mut best: c_int = 0;
    let mut best_diff: i64 = i64::MAX;
    let colors = ptr::addr_of!(COLORS) as *const Color;
    for i in 0..256 {
        let color = colors.add(i).read();
        let dr = r - color.r as i64;
        let dg = g - color.g as i64;
        let db = b - color.b as i64;
        let diff = dr * dr + dg * dg + db * db;
        if diff < best_diff {
            best = i as c_int;
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
///
/// # Safety
///
/// Trivially safe.
#[no_mangle]
pub unsafe extern "C" fn I_BeginRead() {}

/// SDL-era hook to signal the end of a disk read. No-op here.
///
/// # Safety
///
/// Trivially safe.
#[no_mangle]
pub unsafe extern "C" fn I_EndRead() {}

/// Set the host-window title via the platform `DG_SetWindowTitle`
/// callback. Mirrors `I_SetWindowTitle` from `i_video.c`.
///
/// # Safety
///
/// `title` must be a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn I_SetWindowTitle(title: *mut c_char) {
    DG_SetWindowTitle(title);
}

/// Parse video-related command-line switches. No-op in this port;
/// the SDL backend in chocolate-doom handles things like `-window`
/// here.
///
/// # Safety
///
/// Trivially safe.
#[no_mangle]
pub unsafe extern "C" fn I_GraphicsCheckCommandLine() {}

/// Register a callback that returns whether the mouse should be
/// grabbed by the window. No-op in this port (no mouse capture).
///
/// # Safety
///
/// Trivially safe; the callback argument is stored nowhere.
#[no_mangle]
pub unsafe extern "C" fn I_SetGrabMouseCallback(_func: extern "C" fn() -> Boolean) {}

/// Enable the disk-activity icon overlay. No-op in this port.
///
/// # Safety
///
/// Trivially safe.
#[no_mangle]
pub unsafe extern "C" fn I_EnableLoadingDisk() {}

/// Bind video-related `m_config` variables. No-op in this port;
/// chocolate-doom binds things like `fullscreen`, `aspect_ratio_correct`,
/// etc. here.
///
/// # Safety
///
/// Trivially safe.
#[no_mangle]
pub unsafe extern "C" fn I_BindVideoVariables() {}

/// Toggle the on-screen FPS dot indicator. No-op in this port.
///
/// # Safety
///
/// Trivially safe.
#[no_mangle]
pub unsafe extern "C" fn I_DisplayFPSDots(_dots_on: Boolean) {}

/// Detect whether the engine was launched as a screensaver. No-op
/// in this port; `screensaver_mode` stays 0.
///
/// # Safety
///
/// Trivially safe.
#[no_mangle]
pub unsafe extern "C" fn I_CheckIsScreensaver() {}

/// Link anchor referencing every public C symbol in this module so
/// the linker keeps them all. Not part of the original Doom API.
///
/// # Safety
///
/// Passes null pointers everywhere and would crash if called.
/// Treat as link-only.
#[no_mangle]
pub unsafe extern "C" fn I_Video_Link_Anchor() {
    I_InitGraphics();
    I_ShutdownGraphics();
    I_StartFrame();
    I_StartTic();
    I_UpdateNoBlit();
    I_FinishUpdate();
    I_ReadScreen(ptr::null_mut());
    I_SetPalette(ptr::null_mut());
    I_GetPaletteIndex(0, 0, 0);
    I_BeginRead();
    I_EndRead();
    I_SetWindowTitle(ptr::null_mut());
    I_GraphicsCheckCommandLine();
    /// Inert mouse-grab callback used only to take a function pointer for
    /// `I_SetGrabMouseCallback`; never invoked at runtime.
    extern "C" fn _grab_anchor() -> Boolean {
        Boolean::FALSE
    }
    I_SetGrabMouseCallback(_grab_anchor);
    I_EnableLoadingDisk();
    I_BindVideoVariables();
    I_DisplayFPSDots(Boolean::FALSE);
    I_CheckIsScreensaver();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// `COLORS` is a `static mut` shared across the whole crate; the
    /// tests below mutate it, so they must run serially.
    static PALETTE_LOCK: Mutex<()> = Mutex::new(());

    /// RAII guard that owns the palette lock for the duration of a
    /// test and restores the `COLORS` snapshot captured at acquisition
    /// time on drop, even on panic. Keeps the global palette
    /// invisible to subsequent tests in the same binary.
    struct PaletteGuard {
        _lock: MutexGuard<'static, ()>,
        saved: [Color; 256],
    }

    impl PaletteGuard {
        fn acquire() -> Self {
            let lock = PALETTE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let saved = unsafe { ptr::addr_of!(COLORS).read() };
            Self { _lock: lock, saved }
        }
    }

    impl Drop for PaletteGuard {
        fn drop(&mut self) {
            unsafe { ptr::addr_of_mut!(COLORS).write(self.saved) };
        }
    }

    fn set_test_palette(entries: &[(u8, u8, u8)]) {
        let colors = ptr::addr_of_mut!(COLORS) as *mut Color;
        unsafe {
            for i in 0..256 {
                let (r, g, b) = entries.get(i).copied().unwrap_or((0, 0, 0));
                colors.add(i).write(Color { b, g, r, a: 0 });
            }
        }
    }

    #[test]
    fn get_palette_index_nearest_rgb() {
        let _guard = PaletteGuard::acquire();
        let palette: [(u8, u8, u8); 4] = [(0, 0, 0), (255, 0, 0), (0, 255, 0), (0, 0, 255)];
        set_test_palette(&palette);

        unsafe {
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
            assert_eq!(I_GetPaletteIndex(c_int::MAX, 0, 0), 1, "saturate to red");
            assert_eq!(I_GetPaletteIndex(c_int::MIN, 0, 0), 0, "saturate to black");
        }
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

        unsafe {
            assert_eq!(I_GetPaletteIndex(42, 0, 0), 42);
        }
    }
}
