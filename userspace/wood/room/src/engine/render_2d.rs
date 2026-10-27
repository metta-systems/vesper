//! Rust port of vendor/doomgeneric/v_video.c.
//!
//! Screen buffer management, patch drawing, block copying, and screenshots.
//!
//! The patch drawers take a checked [`Patch`] view and write through the
//! destination buffer as a slice of its recorded length. Each column is
//! clipped to the buffer's width and each post to its rows, so a patch
//! placed partly or wholly off the buffer draws only its visible part
//! instead of writing out of bounds as C did (C's `RANGECHECK` made that an
//! `I_Error`); patches that fit draw exactly as in C.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use {
    std::{mem::size_of, ptr},
    zerocopy::{IntoBytes, little_endian::U16},
};

use crate::{
    engine::{
        global::Global,
        interface::video::{
            I_GetPaletteIndex, SCREENHEIGHT, SCREENWIDTH, mouse_acceleration, mouse_threshold,
            usemouse, video_buffer,
        },
        math::bbox::M_AddToBox,
        misc::{M_FileExists, M_WriteFile},
        wad::{
            W_CacheLumpName, W_GetNumForName, cache_lump_bytes,
            format::{Column, Patch, pcx_t},
        },
        zone::{PU_CACHE, PU_STATIC},
    },
    i_error,
};

/// Clipping callback consulted before drawing a patch at the given
/// top-left corner; returning 0 skips the patch.
pub type vpatchclipfunc_t = Option<fn(Patch<'_>, i32, i32) -> i32>;

/// 2D drawing state (C `v_video.c` globals).
#[derive(Clone, Copy)]
pub struct Render2dState {
    /// Translucency lookup table (`TINTTAB` lump), C `tinttable`.
    tinttable: Option<&'static [u8]>,
    /// Alternate translucency table (`XLATAB` lump), C `xlatab`.
    xlatab: Option<&'static [u8]>,
    /// Bounding box of the screen area drawn since the last update,
    /// C `dirtybox`.
    dirtybox: [i32; 4],
    /// Buffer the drawing functions write to, C `dest_screen`.
    dest_screen: *mut u8,
    /// Length of `dest_screen` in bytes: whole `SCREENWIDTH` rows, as set
    /// by `V_UseBuffer` / `V_RestoreBuffer`. The patch drawers clip to it.
    dest_screen_length: usize,
    /// Clipping callback consulted before drawing a patch, C
    /// `patchclip_callback`.
    patchclip_callback: vpatchclipfunc_t,
}

impl Render2dState {
    /// No tables loaded, no destination buffer.
    pub const fn new() -> Self {
        Self {
            tinttable: None,
            xlatab: None,
            dirtybox: [0; 4],
            dest_screen: ptr::null_mut(),
            dest_screen_length: 0,
            patchclip_callback: None,
        }
    }
}

impl Default for Render2dState {
    fn default() -> Self {
        Self::new()
    }
}

/// The 2D drawing state.
pub static RENDER_2D: Global<Render2dState> = Global::new(Render2dState::new());

pub fn V_MarkRect(x: i32, y: i32, width: i32, height: i32) {
    let mut render = RENDER_2D.borrow_mut();
    if render.dest_screen == video_buffer() {
        M_AddToBox(&mut render.dirtybox, x, y);
        M_AddToBox(&mut render.dirtybox, x + width - 1, y + height - 1);
    }
}

/// Copy the `width` x `height` rectangle at (`srcx`, `srcy`) of `source`, a
/// buffer of `SCREENWIDTH`-wide rows, to (`destx`, `desty`) of the
/// destination buffer, and mark it dirty.
///
/// # Panics
/// If either rectangle does not fit its buffer, or `source` overlaps the
/// destination buffer.
pub fn V_CopyRect(
    srcx: i32,
    srcy: i32,
    source: &[u8],
    width: i32,
    height: i32,
    destx: i32,
    desty: i32,
) {
    V_MarkRect(destx, desty, width, height);

    let render = RENDER_2D.get();
    let destination_start = render.dest_screen as usize;
    let destination_end = destination_start + render.dest_screen_length;
    let source_range = source.as_ptr_range();
    assert!(
        source_range.end as usize <= destination_start
            || destination_end <= source_range.start as usize,
        "V_CopyRect: the source overlaps the destination buffer"
    );
    // SAFETY: the slice lives for this copy only, and `source`, the only
    // other reference used meanwhile, does not overlap it.
    let screen = unsafe { destination_screen(&render) };

    let width = width as usize;
    let mut source_offset = (SCREENWIDTH * srcy + srcx) as usize;
    let mut destination_offset = (SCREENWIDTH * desty + destx) as usize;
    for _ in 0..height {
        screen[destination_offset..destination_offset + width]
            .copy_from_slice(&source[source_offset..source_offset + width]);
        source_offset += SCREENWIDTH as usize;
        destination_offset += SCREENWIDTH as usize;
    }
}

pub fn V_SetPatchClipCallback(func: vpatchclipfunc_t) {
    RENDER_2D.borrow_mut().patchclip_callback = func;
}

/// The destination buffer as a slice of its recorded length.
///
/// Panics if no buffer is set (before `V_RestoreBuffer` first runs).
///
/// # Safety
/// No other reference into the destination buffer may be live while the
/// slice is in use. The 2D drawers take it for the length of one draw and
/// call nothing that touches the buffer meanwhile.
unsafe fn destination_screen<'a>(render: &Render2dState) -> &'a mut [u8] {
    assert!(
        !render.dest_screen.is_null(),
        "2D drawing before a destination buffer is set"
    );
    // SAFETY: `V_UseBuffer` / `V_RestoreBuffer` record a buffer writable
    // for `dest_screen_length` bytes that outlives its use as destination;
    // the caller rules out other references into it.
    unsafe { std::slice::from_raw_parts_mut(render.dest_screen, render.dest_screen_length) }
}

/// Write one post's `pixels` into column `screen_x` of `screen` (rows
/// `SCREENWIDTH` wide), starting at row `top`, with `plot(destination,
/// source)` per pixel. Rows outside the buffer are skipped.
#[inline(always)]
fn draw_post(
    screen: &mut [u8],
    screen_x: usize,
    top: i32,
    pixels: &[u8],
    mut plot: impl FnMut(&mut u8, u8),
) {
    let rows = (screen.len() / SCREENWIDTH as usize) as i32;
    let visible_top = top.max(0);
    let visible_bottom = (top + pixels.len() as i32).min(rows);
    if visible_bottom <= visible_top {
        return;
    }
    let skipped = (visible_top - top) as usize;
    let count = (visible_bottom - visible_top) as usize;
    let start = visible_top as usize * SCREENWIDTH as usize + screen_x;
    let destinations = screen[start..].iter_mut().step_by(SCREENWIDTH as usize);
    for (destination, &source) in destinations.zip(&pixels[skipped..skipped + count]) {
        plot(destination, source);
    }
}

/// Draw every column of `patch` with its top-left corner at (`x`, `y`) of
/// `screen`, mirrored left to right if `flipped`, with `plot(destination,
/// source)` per pixel. Columns outside the screen width are skipped.
#[inline(always)]
fn draw_patch_columns(
    screen: &mut [u8],
    x: i32,
    y: i32,
    patch: Patch<'_>,
    flipped: bool,
    mut plot: impl FnMut(&mut u8, u8),
) {
    let width = patch.width();
    for column_index in 0..width {
        let screen_x = x + column_index;
        if !(0..SCREENWIDTH).contains(&screen_x) {
            continue;
        }
        let source_column = if flipped {
            width - 1 - column_index
        } else {
            column_index
        };
        for post in patch.column(source_column as usize) {
            draw_post(
                screen,
                screen_x as usize,
                y + post.topdelta as i32,
                post.pixels,
                &mut plot,
            );
        }
    }
}

/// Draw `patch` at (`x`, `y`) less its offsets, mirrored if `flipped`:
/// the shared body of `V_DrawPatch` and `V_DrawPatchFlipped`.
fn draw_patch_opaque(x: i32, y: i32, patch: Patch<'_>, flipped: bool) {
    let render = RENDER_2D.get();
    let y = y - patch.topoffset();
    let x = x - patch.leftoffset();

    if let Some(callback) = render.patchclip_callback
        && callback(patch, x, y) == 0
    {
        return;
    }

    V_MarkRect(x, y, patch.width(), patch.height());

    // SAFETY: the slice lives for this draw only, which touches no other
    // reference into the buffer.
    let screen = unsafe { destination_screen(&render) };
    draw_patch_columns(screen, x, y, patch, flipped, |destination, source| {
        *destination = source;
    });
}

/// Draw the posts of `column` into column `x` of the video buffer itself
/// (not the `V_UseBuffer` destination), each post at its `topdelta` row,
/// as C `F_DrawPatchCol` did. An `x` outside the screen draws nothing.
pub fn draw_column_to_video_buffer(x: i32, column: Column<'_>) {
    let video = video_buffer();
    assert!(!video.is_null(), "drawing before I_InitGraphics");
    if !(0..SCREENWIDTH).contains(&x) {
        return;
    }
    // SAFETY: `I_InitGraphics` allocates the video buffer as one screen,
    // and no other reference into it is live while this column is drawn.
    let screen =
        unsafe { std::slice::from_raw_parts_mut(video, (SCREENWIDTH * SCREENHEIGHT) as usize) };
    for post in column {
        draw_post(
            screen,
            x as usize,
            post.topdelta as i32,
            post.pixels,
            |destination, source| *destination = source,
        );
    }
}

/// Draw `patch` into the destination buffer with its origin at (`x`, `y`),
/// clipped to the buffer.
pub fn V_DrawPatch(x: i32, y: i32, patch: Patch<'_>) {
    draw_patch_opaque(x, y, patch, false);
}

/// Draw `patch` mirrored left to right, as `V_DrawPatch` otherwise.
pub fn V_DrawPatchFlipped(x: i32, y: i32, patch: Patch<'_>) {
    draw_patch_opaque(x, y, patch, true);
}

/// Draw `patch` as `V_DrawPatch` does.
pub fn V_DrawPatchDirect(x: i32, y: i32, patch: Patch<'_>) {
    V_DrawPatch(x, y, patch);
}

/// The translucency table loaded by `V_LoadTintTable`. Panics if it was
/// not loaded, where C would read through a null pointer.
fn loaded_tinttable(render: &Render2dState) -> &'static [u8] {
    render
        .tinttable
        .expect("translucent patch drawn before V_LoadTintTable")
}

/// Draw `patch` translucently: each pixel becomes
/// `tinttable[destination << 8 | source]`.
pub fn V_DrawTLPatch(x: i32, y: i32, patch: Patch<'_>) {
    let render = RENDER_2D.get();
    let tinttable = loaded_tinttable(&render);
    let y = y - patch.topoffset();
    let x = x - patch.leftoffset();

    // SAFETY: the slice lives for this draw only, which touches no other
    // reference into the buffer.
    let screen = unsafe { destination_screen(&render) };
    draw_patch_columns(screen, x, y, patch, false, |destination, source| {
        *destination = tinttable[((*destination as usize) << 8) + source as usize];
    });
}

/// Draw `patch` through the alternate translucency table: each pixel
/// becomes `xlatab[source << 8 | destination]`.
pub fn V_DrawXlaPatch(x: i32, y: i32, patch: Patch<'_>) {
    let render = RENDER_2D.get();
    let xlatab = render
        .xlatab
        .expect("translucent patch drawn before V_LoadXlaTable");
    let y = y - patch.topoffset();
    let x = x - patch.leftoffset();

    if let Some(callback) = render.patchclip_callback
        && callback(patch, x, y) == 0
    {
        return;
    }

    // SAFETY: the slice lives for this draw only, which touches no other
    // reference into the buffer.
    let screen = unsafe { destination_screen(&render) };
    draw_patch_columns(screen, x, y, patch, false, |destination, source| {
        *destination = xlatab[*destination as usize + ((source as usize) << 8)];
    });
}

/// Draw `patch` translucently, as `V_DrawTLPatch`.
pub fn V_DrawAltTLPatch(x: i32, y: i32, patch: Patch<'_>) {
    V_DrawTLPatch(x, y, patch);
}

/// Draw `patch` with a shadow: its shape darkened through `tinttable`
/// two pixels right and down, then the patch itself.
pub fn V_DrawShadowedPatch(x: i32, y: i32, patch: Patch<'_>) {
    let render = RENDER_2D.get();
    let tinttable = loaded_tinttable(&render);
    let y = y - patch.topoffset();
    let x = x - patch.leftoffset();

    // SAFETY: the slice lives for this draw only, which touches no other
    // reference into the buffer.
    let screen = unsafe { destination_screen(&render) };
    // C interleaves the shadow and the patch pixel by pixel. A column's
    // shadow lands two columns right, which only that later column draws
    // over, so all shadows first and then the patch gives the same image.
    draw_patch_columns(screen, x + 2, y + 2, patch, false, |destination, _| {
        *destination = tinttable[(*destination as usize) << 8];
    });
    draw_patch_columns(screen, x, y, patch, false, |destination, source| {
        *destination = source;
    });
}

pub fn V_LoadTintTable() {
    // SAFETY: the table is cached at PU_STATIC and never released.
    let tinttable = unsafe { cache_lump_bytes(W_GetNumForName(b"TINTTAB"), PU_STATIC) };
    RENDER_2D.borrow_mut().tinttable = Some(tinttable);
}

pub fn V_LoadXlaTable() {
    // SAFETY: the table is cached at PU_STATIC and never released.
    let xlatab = unsafe { cache_lump_bytes(W_GetNumForName(b"XLATAB"), PU_STATIC) };
    RENDER_2D.borrow_mut().xlatab = Some(xlatab);
}

/// # Safety
/// `src` must hold `width * height` readable bytes, and the rectangle must
/// fit the screen.
pub unsafe fn V_DrawBlock(x: i32, y: i32, width: i32, height: i32, src: *mut u8) {
    unsafe {
        V_MarkRect(x, y, width, height);

        let mut dest = RENDER_2D
            .get()
            .dest_screen
            .add((y * SCREENWIDTH + x) as usize);
        let mut source = src;
        let mut h = height;

        while h > 0 {
            ptr::copy_nonoverlapping(source, dest, width as usize);
            source = source.add(width as usize);
            dest = dest.add(SCREENWIDTH as usize);
            h -= 1;
        }
    }
}

pub fn V_DrawFilledBox(x: i32, y: i32, w: i32, h: i32, c: i32) {
    unsafe {
        let mut buf = video_buffer().add((SCREENWIDTH * y + x) as usize);

        let mut y1 = 0;
        while y1 < h {
            let mut buf1 = buf;
            let mut x1 = 0;
            while x1 < w {
                *buf1 = c as u8;
                buf1 = buf1.add(1);
                x1 += 1;
            }
            buf = buf.add(SCREENWIDTH as usize);
            y1 += 1;
        }
    }
}

pub fn V_DrawHorizLine(x: i32, y: i32, w: i32, c: i32) {
    unsafe {
        let mut buf = video_buffer().add((SCREENWIDTH * y + x) as usize);
        let mut x1 = 0;
        while x1 < w {
            *buf = c as u8;
            buf = buf.add(1);
            x1 += 1;
        }
    }
}

pub fn V_DrawVertLine(x: i32, y: i32, h: i32, c: i32) {
    unsafe {
        let mut buf = video_buffer().add((SCREENWIDTH * y + x) as usize);
        let mut y1 = 0;
        while y1 < h {
            *buf = c as u8;
            buf = buf.add(SCREENWIDTH as usize);
            y1 += 1;
        }
    }
}

pub fn V_DrawBox(x: i32, y: i32, w: i32, h: i32, c: i32) {
    V_DrawHorizLine(x, y, w, c);
    V_DrawHorizLine(x, y + h - 1, w, c);
    V_DrawVertLine(x, y, h, c);
    V_DrawVertLine(x + w - 1, y, h, c);
}

/// # Safety
/// `raw` must hold `SCREENWIDTH * SCREENHEIGHT` readable bytes.
pub unsafe fn V_DrawRawScreen(raw: *mut u8) {
    unsafe {
        ptr::copy_nonoverlapping(
            raw,
            RENDER_2D.get().dest_screen,
            (SCREENWIDTH * SCREENHEIGHT) as usize,
        );
    }
}

pub fn V_Init() {
    // no-op
}

/// Redirect the drawing functions to `buffer`, `length` bytes of whole
/// `SCREENWIDTH` rows, until `V_RestoreBuffer`.
///
/// # Safety
/// `buffer` must be writable for `length` bytes, overlap no lump the
/// drawers read, and stay valid until `V_RestoreBuffer` is called: the
/// drawing functions write through it.
pub unsafe fn V_UseBuffer(buffer: *mut u8, length: usize) {
    let mut render = RENDER_2D.borrow_mut();
    render.dest_screen = buffer;
    render.dest_screen_length = length;
}

pub fn V_RestoreBuffer() {
    let screen = video_buffer();
    let mut render = RENDER_2D.borrow_mut();
    render.dest_screen = screen;
    render.dest_screen_length = if screen.is_null() {
        0
    } else {
        (SCREENWIDTH * SCREENHEIGHT) as usize
    };
}

/// Write `width * height` pixels from `data` with the 768-byte `palette` as
/// a PCX file. Panics if either slice is shorter than that.
pub fn WritePCXfile(filename: &str, data: &[u8], width: i32, height: i32, palette: &[u8]) {
    let header = pcx_t {
        manufacturer: 0x0a,
        version: 5,
        encoding: 1,
        bits_per_pixel: 8,
        xmin: U16::ZERO,
        ymin: U16::ZERO,
        xmax: U16::new((width - 1) as u16),
        ymax: U16::new((height - 1) as u16),
        hres: U16::new(width as u16),
        vres: U16::new(height as u16),
        palette: [0; 48],
        reserved: 0,
        color_planes: 1,
        bytes_per_line: U16::new(width as u16),
        palette_type: U16::new(2),
        filler: [0; 58],
    };

    let total = (width * height) as usize;
    let mut pcx = Vec::with_capacity(size_of::<pcx_t>() + total * 2 + 1 + 768);
    pcx.extend_from_slice(header.as_bytes());

    for &byte in &data[..total] {
        if (byte & 0xc0) != 0xc0 {
            pcx.push(byte);
        } else {
            pcx.extend_from_slice(&[0xc1, byte]);
        }
    }

    // write palette
    pcx.push(0x0c);
    pcx.extend_from_slice(&palette[..768]);

    M_WriteFile(filename, &pcx);
}

/// Build a screenshot filename for index `i`: `"DOOM{i:02}.pcx"`, the
/// `"DOOM%02i.%s"` format the C caller passed.
fn screenshot_filename(i: i32) -> String {
    format!("DOOM{i:02}.pcx")
}

pub fn V_ScreenShot() {
    let mut i = 0;
    while i <= 99 {
        if !M_FileExists(screenshot_filename(i)) {
            break;
        }
        i += 1;
    }

    if i == 100 {
        i_error!("V_ScreenShot: Couldn't create a PCX");
    }

    // SAFETY: the video buffer is a full screen and PLAYPAL starts with a
    // 768-byte palette; both outlive the call.
    let (screen, palette) = unsafe {
        (
            std::slice::from_raw_parts(video_buffer(), (SCREENWIDTH * SCREENHEIGHT) as usize),
            std::slice::from_raw_parts(W_CacheLumpName(b"PLAYPAL", PU_CACHE) as *const u8, 768),
        )
    };
    WritePCXfile(
        &screenshot_filename(i),
        screen,
        SCREENWIDTH,
        SCREENHEIGHT,
        palette,
    );
}

const MOUSE_SPEED_BOX_WIDTH: i32 = 120;
const MOUSE_SPEED_BOX_HEIGHT: i32 = 9;

pub fn V_DrawMouseSpeedBox(speed: i32) {
    let bgcolor = I_GetPaletteIndex(0x77, 0x77, 0x77);
    let bordercolor = I_GetPaletteIndex(0x55, 0x55, 0x55);
    let red = I_GetPaletteIndex(0xff, 0x00, 0x00);
    let black = I_GetPaletteIndex(0x00, 0x00, 0x00);
    let yellow = I_GetPaletteIndex(0xff, 0xff, 0x00);
    let white = I_GetPaletteIndex(0xff, 0xff, 0xff);

    if usemouse == 0 || (mouse_acceleration - 1.0).abs() < 0.01 {
        return;
    }

    let box_x = SCREENWIDTH - MOUSE_SPEED_BOX_WIDTH - 10;
    let box_y = 15;

    V_DrawFilledBox(
        box_x,
        box_y,
        MOUSE_SPEED_BOX_WIDTH,
        MOUSE_SPEED_BOX_HEIGHT,
        bgcolor,
    );
    V_DrawBox(
        box_x,
        box_y,
        MOUSE_SPEED_BOX_WIDTH,
        MOUSE_SPEED_BOX_HEIGHT,
        bordercolor,
    );

    let redline_x = MOUSE_SPEED_BOX_WIDTH / 3;

    let original_speed = if speed < mouse_threshold {
        speed
    } else {
        let mut s = speed - mouse_threshold;
        s = (s as f32 / mouse_acceleration) as i32;
        s + mouse_threshold
    };

    let mut linelen = (original_speed * redline_x) / mouse_threshold;
    if linelen > MOUSE_SPEED_BOX_WIDTH - 1 {
        linelen = MOUSE_SPEED_BOX_WIDTH - 1;
    }

    V_DrawHorizLine(box_x + 1, box_y + 4, MOUSE_SPEED_BOX_WIDTH - 2, black);

    if linelen < redline_x {
        V_DrawHorizLine(
            box_x + 1,
            box_y + MOUSE_SPEED_BOX_HEIGHT / 2,
            linelen,
            white,
        );
    } else {
        V_DrawHorizLine(
            box_x + 1,
            box_y + MOUSE_SPEED_BOX_HEIGHT / 2,
            redline_x,
            white,
        );
        V_DrawHorizLine(
            box_x + redline_x,
            box_y + MOUSE_SPEED_BOX_HEIGHT / 2,
            linelen - redline_x,
            yellow,
        );
    }

    V_DrawVertLine(
        box_x + redline_x,
        box_y + 1,
        MOUSE_SPEED_BOX_HEIGHT - 2,
        red,
    );
}

#[cfg(test)]
mod tests {
    use {super::*, crate::engine::wad::format::tests::build_patch_lump};

    const WIDTH: usize = SCREENWIDTH as usize;

    /// Draw `lump` opaquely into a zeroed 4-row buffer at (`x`, `y`).
    fn draw_into_rows(lump: &[u8], x: i32, y: i32, flipped: bool) -> Vec<u8> {
        let mut screen = vec![0u8; WIDTH * 4];
        let patch = Patch::from_bytes(lump).unwrap();
        draw_patch_columns(&mut screen, x, y, patch, flipped, |destination, source| {
            *destination = source;
        });
        screen
    }

    /// A patch that fits is copied post by post at its rows; flipping
    /// mirrors the columns.
    #[test]
    fn patch_draws_posts_at_their_rows() {
        let lump = build_patch_lump(4, 0, 0, &[&[(0, &[1, 2]), (3, &[3])], &[(1, &[4])]]);

        let screen = draw_into_rows(&lump, 10, 0, false);
        assert_eq!(
            [0, 1, 2, 3].map(|row| screen[row * WIDTH + 10]),
            [1, 2, 0, 3]
        );
        assert_eq!(
            [0, 1, 2, 3].map(|row| screen[row * WIDTH + 11]),
            [0, 4, 0, 0]
        );
        assert_eq!(screen.iter().filter(|&&pixel| pixel != 0).count(), 4);

        let flipped = draw_into_rows(&lump, 10, 0, true);
        assert_eq!(flipped[WIDTH + 10], 4);
        assert_eq!(flipped[11], 1);
        assert_eq!(flipped[3 * WIDTH + 11], 3);
    }

    /// Rows above and below the buffer and columns past either side are
    /// clipped instead of written out of bounds or wrapped to another row.
    #[test]
    fn patch_clips_to_buffer() {
        let lump = build_patch_lump(6, 0, 0, &[&[(0, &[1, 2, 3, 4, 5, 6])], &[(0, &[7])]]);

        // Column 0 at the last screen column from row -1: rows 0-3 get
        // pixels 2-5, pixel 6 falls below. Column 1 is off the right edge
        // and must not wrap into column 0 of the next row.
        let screen = draw_into_rows(&lump, WIDTH as i32 - 1, -1, false);
        assert_eq!(
            [0, 1, 2, 3].map(|row| screen[row * WIDTH + WIDTH - 1]),
            [2, 3, 4, 5]
        );
        assert_eq!(screen.iter().filter(|&&pixel| pixel != 0).count(), 4);

        // Column 0 off the left edge; column 1 lands in screen column 0.
        let screen = draw_into_rows(&lump, -1, 0, false);
        assert_eq!(screen[0], 7);
        assert_eq!(screen.iter().filter(|&&pixel| pixel != 0).count(), 1);

        // Wholly below or above the buffer: nothing drawn.
        assert!(
            draw_into_rows(&lump, 0, 4, false)
                .iter()
                .all(|&pixel| pixel == 0)
        );
        assert!(
            draw_into_rows(&lump, 0, -6, false)
                .iter()
                .all(|&pixel| pixel == 0)
        );
    }

    #[test]
    fn screenshot_filename_zero_padded() {
        assert_eq!(screenshot_filename(0), "DOOM00.pcx");
    }

    #[test]
    fn screenshot_filename_two_digit() {
        assert_eq!(screenshot_filename(42), "DOOM42.pcx");
    }

    #[test]
    fn screenshot_filename_max() {
        assert_eq!(screenshot_filename(99), "DOOM99.pcx");
    }
}
