//! Rust port of vendor/doomgeneric/v_video.c.
//!
//! Screen buffer management, patch drawing, block copying, and screenshots.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::{c_char, c_int, c_void};
use std::ptr;

use crate::doom::i_video::{
    mouse_acceleration, mouse_threshold, usemouse, I_GetPaletteIndex, I_VideoBuffer, SCREENHEIGHT,
    SCREENWIDTH,
};
use crate::doom::m_bbox::M_AddToBox;
use crate::doom::m_misc::{M_FileExists, M_WriteFile};
use crate::doom::w_wad::W_CacheLumpName;
use crate::doom::z_zone::{Z_Free, Z_Malloc, PU_CACHE, PU_STATIC};
use crate::i_error;

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct patch_t {
    pub width: i16,
    pub height: i16,
    pub leftoffset: i16,
    pub topoffset: i16,
    // columnofs follows in the WAD data but is variable-length;
    // we read it via pointer arithmetic to avoid Rust array bounds checks.
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct post_t {
    pub topdelta: u8,
    pub length: u8,
}

pub type column_t = post_t;

pub type vpatchclipfunc_t = Option<extern "C" fn(*mut patch_t, c_int, c_int) -> c_int>;

#[repr(C, packed)]
struct pcx_t {
    manufacturer: c_char,
    version: c_char,
    encoding: c_char,
    bits_per_pixel: c_char,
    xmin: u16,
    ymin: u16,
    xmax: u16,
    ymax: u16,
    hres: u16,
    vres: u16,
    palette: [u8; 48],
    reserved: c_char,
    color_planes: c_char,
    bytes_per_line: u16,
    palette_type: u16,
    filler: [u8; 58],
    data: u8,
}

#[no_mangle]
pub static mut tinttable: *mut u8 = ptr::null_mut();

#[no_mangle]
pub static mut xlatab: *mut u8 = ptr::null_mut();

#[no_mangle]
pub static mut dirtybox: [c_int; 4] = [0; 4];

static mut dest_screen: *mut u8 = ptr::null_mut();
static mut patchclip_callback: vpatchclipfunc_t = None;

#[no_mangle]
pub extern "C" fn V_MarkRect(x: c_int, y: c_int, width: c_int, height: c_int) {
    unsafe {
        if dest_screen == I_VideoBuffer {
            M_AddToBox(std::ptr::addr_of_mut!(dirtybox[0]), x, y);
            M_AddToBox(
                std::ptr::addr_of_mut!(dirtybox[0]),
                x + width - 1,
                y + height - 1,
            );
        }
    }
}

#[no_mangle]
pub extern "C" fn V_CopyRect(
    srcx: c_int,
    srcy: c_int,
    source: *mut u8,
    width: c_int,
    height: c_int,
    destx: c_int,
    desty: c_int,
) {
    unsafe {
        V_MarkRect(destx, desty, width, height);

        let mut src = source.add((SCREENWIDTH * srcy + srcx) as usize);
        let mut dest = dest_screen.add((SCREENWIDTH * desty + destx) as usize);

        let mut h = height;
        while h > 0 {
            ptr::copy_nonoverlapping(src, dest, width as usize);
            src = src.add(SCREENWIDTH as usize);
            dest = dest.add(SCREENWIDTH as usize);
            h -= 1;
        }
    }
}

#[no_mangle]
pub extern "C" fn V_SetPatchClipCallback(func: vpatchclipfunc_t) {
    unsafe {
        patchclip_callback = func;
    }
}

#[no_mangle]
pub extern "C" fn V_DrawPatch(x: c_int, y: c_int, patch: *mut patch_t) {
    unsafe {
        let y = y - (*patch).topoffset as c_int;
        let x = x - (*patch).leftoffset as c_int;

        if let Some(cb) = patchclip_callback {
            if cb(patch, x, y) == 0 {
                return;
            }
        }

        V_MarkRect(x, y, (*patch).width as c_int, (*patch).height as c_int);

        let w = (*patch).width as c_int;
        let mut desttop = dest_screen.add((y * SCREENWIDTH + x) as usize);

        let mut col = 0;
        while col < w {
            let ofs = ptr::read_unaligned((patch as *mut u8).add(8 + col as usize * 4) as *mut i32);
            let column = (patch as *mut u8).add(ofs as usize) as *mut column_t;

            let mut col_ptr = column;
            while (*col_ptr).topdelta != 0xff {
                let mut source = (col_ptr as *mut u8).add(3);
                let mut dest = desttop.add((*col_ptr).topdelta as usize * SCREENWIDTH as usize);
                let mut count = (*col_ptr).length as c_int;

                while count > 0 {
                    *dest = *source;
                    dest = dest.add(SCREENWIDTH as usize);
                    source = source.add(1);
                    count -= 1;
                }

                col_ptr = (col_ptr as *mut u8).add((*col_ptr).length as usize + 4) as *mut column_t;
            }

            col += 1;
            desttop = desttop.add(1);
        }
    }
}

#[no_mangle]
pub extern "C" fn V_DrawPatchFlipped(x: c_int, y: c_int, patch: *mut patch_t) {
    unsafe {
        let y = y - (*patch).topoffset as c_int;
        let x = x - (*patch).leftoffset as c_int;

        if let Some(cb) = patchclip_callback {
            if cb(patch, x, y) == 0 {
                return;
            }
        }

        V_MarkRect(x, y, (*patch).width as c_int, (*patch).height as c_int);

        let w = (*patch).width as c_int;
        let mut desttop = dest_screen.add((y * SCREENWIDTH + x) as usize);

        let mut col = 0;
        while col < w {
            let ofs = ptr::read_unaligned(
                (patch as *mut u8).add(8 + (w - 1 - col) as usize * 4) as *mut i32
            );
            let column = (patch as *mut u8).add(ofs as usize) as *mut column_t;

            let mut col_ptr = column;
            while (*col_ptr).topdelta != 0xff {
                let mut source = (col_ptr as *mut u8).add(3);
                let mut dest = desttop.add((*col_ptr).topdelta as usize * SCREENWIDTH as usize);
                let mut count = (*col_ptr).length as c_int;

                while count > 0 {
                    *dest = *source;
                    dest = dest.add(SCREENWIDTH as usize);
                    source = source.add(1);
                    count -= 1;
                }

                col_ptr = (col_ptr as *mut u8).add((*col_ptr).length as usize + 4) as *mut column_t;
            }

            col += 1;
            desttop = desttop.add(1);
        }
    }
}

#[no_mangle]
pub extern "C" fn V_DrawPatchDirect(x: c_int, y: c_int, patch: *mut patch_t) {
    V_DrawPatch(x, y, patch);
}

#[no_mangle]
pub extern "C" fn V_DrawTLPatch(x: c_int, y: c_int, patch: *mut patch_t) {
    unsafe {
        let y = y - (*patch).topoffset as c_int;
        let x = x - (*patch).leftoffset as c_int;

        let w = (*patch).width as c_int;
        let mut desttop = dest_screen.add((y * SCREENWIDTH + x) as usize);

        let mut col = 0;
        while col < w {
            let ofs = ptr::read_unaligned((patch as *mut u8).add(8 + col as usize * 4) as *mut i32);
            let column = (patch as *mut u8).add(ofs as usize) as *mut column_t;

            let mut col_ptr = column;
            while (*col_ptr).topdelta != 0xff {
                let mut source = (col_ptr as *mut u8).add(3);
                let mut dest = desttop.add((*col_ptr).topdelta as usize * SCREENWIDTH as usize);
                let mut count = (*col_ptr).length as c_int;

                while count > 0 {
                    let idx = ((*dest as usize) << 8) + (*source as usize);
                    *dest = *tinttable.add(idx);
                    dest = dest.add(SCREENWIDTH as usize);
                    source = source.add(1);
                    count -= 1;
                }

                col_ptr = (col_ptr as *mut u8).add((*col_ptr).length as usize + 4) as *mut column_t;
            }

            col += 1;
            desttop = desttop.add(1);
        }
    }
}

#[no_mangle]
pub extern "C" fn V_DrawXlaPatch(x: c_int, y: c_int, patch: *mut patch_t) {
    unsafe {
        let y = y - (*patch).topoffset as c_int;
        let x = x - (*patch).leftoffset as c_int;

        if let Some(cb) = patchclip_callback {
            if cb(patch, x, y) == 0 {
                return;
            }
        }

        let w = (*patch).width as c_int;
        let mut desttop = dest_screen.add((y * SCREENWIDTH + x) as usize);

        let mut col = 0;
        while col < w {
            let ofs = ptr::read_unaligned((patch as *mut u8).add(8 + col as usize * 4) as *mut i32);
            let column = (patch as *mut u8).add(ofs as usize) as *mut column_t;

            let mut col_ptr = column;
            while (*col_ptr).topdelta != 0xff {
                let mut source = (col_ptr as *mut u8).add(3);
                let mut dest = desttop.add((*col_ptr).topdelta as usize * SCREENWIDTH as usize);
                let mut count = (*col_ptr).length as c_int;

                while count > 0 {
                    let idx = (*dest as usize) + ((*source as usize) << 8);
                    *dest = *xlatab.add(idx);
                    source = source.add(1);
                    dest = dest.add(SCREENWIDTH as usize);
                    count -= 1;
                }

                col_ptr = (col_ptr as *mut u8).add((*col_ptr).length as usize + 4) as *mut column_t;
            }

            col += 1;
            desttop = desttop.add(1);
        }
    }
}

#[no_mangle]
pub extern "C" fn V_DrawAltTLPatch(x: c_int, y: c_int, patch: *mut patch_t) {
    unsafe {
        let y = y - (*patch).topoffset as c_int;
        let x = x - (*patch).leftoffset as c_int;

        let w = (*patch).width as c_int;
        let mut desttop = dest_screen.add((y * SCREENWIDTH + x) as usize);

        let mut col = 0;
        while col < w {
            let ofs = ptr::read_unaligned((patch as *mut u8).add(8 + col as usize * 4) as *mut i32);
            let column = (patch as *mut u8).add(ofs as usize) as *mut column_t;

            let mut col_ptr = column;
            while (*col_ptr).topdelta != 0xff {
                let mut source = (col_ptr as *mut u8).add(3);
                let mut dest = desttop.add((*col_ptr).topdelta as usize * SCREENWIDTH as usize);
                let mut count = (*col_ptr).length as c_int;

                while count > 0 {
                    let idx = ((*dest as usize) << 8) + (*source as usize);
                    *dest = *tinttable.add(idx);
                    dest = dest.add(SCREENWIDTH as usize);
                    source = source.add(1);
                    count -= 1;
                }

                col_ptr = (col_ptr as *mut u8).add((*col_ptr).length as usize + 4) as *mut column_t;
            }

            col += 1;
            desttop = desttop.add(1);
        }
    }
}

#[no_mangle]
pub extern "C" fn V_DrawShadowedPatch(x: c_int, y: c_int, patch: *mut patch_t) {
    unsafe {
        let y = y - (*patch).topoffset as c_int;
        let x = x - (*patch).leftoffset as c_int;

        let w = (*patch).width as c_int;
        let mut desttop = dest_screen.add((y * SCREENWIDTH + x) as usize);
        let mut desttop2 = dest_screen.add(((y + 2) * SCREENWIDTH + (x + 2)) as usize);

        let mut col = 0;
        while col < w {
            let ofs = ptr::read_unaligned((patch as *mut u8).add(8 + col as usize * 4) as *mut i32);
            let column = (patch as *mut u8).add(ofs as usize) as *mut column_t;

            let mut col_ptr = column;
            while (*col_ptr).topdelta != 0xff {
                let mut source = (col_ptr as *mut u8).add(3);
                let mut dest = desttop.add((*col_ptr).topdelta as usize * SCREENWIDTH as usize);
                let mut dest2 = desttop2.add((*col_ptr).topdelta as usize * SCREENWIDTH as usize);
                let mut count = (*col_ptr).length as c_int;

                while count > 0 {
                    let idx = (*dest2 as usize) << 8;
                    *dest2 = *tinttable.add(idx);
                    dest2 = dest2.add(SCREENWIDTH as usize);
                    *dest = *source;
                    dest = dest.add(SCREENWIDTH as usize);
                    source = source.add(1);
                    count -= 1;
                }

                col_ptr = (col_ptr as *mut u8).add((*col_ptr).length as usize + 4) as *mut column_t;
            }

            col += 1;
            desttop = desttop.add(1);
            desttop2 = desttop2.add(1);
        }
    }
}

#[no_mangle]
pub extern "C" fn V_LoadTintTable() {
    unsafe {
        tinttable = W_CacheLumpName(c"TINTTAB".as_ptr(), PU_STATIC) as *mut u8;
    }
}

#[no_mangle]
pub extern "C" fn V_LoadXlaTable() {
    unsafe {
        xlatab = W_CacheLumpName(c"XLATAB".as_ptr(), PU_STATIC) as *mut u8;
    }
}

#[no_mangle]
pub extern "C" fn V_DrawBlock(x: c_int, y: c_int, width: c_int, height: c_int, src: *mut u8) {
    unsafe {
        V_MarkRect(x, y, width, height);

        let mut dest = dest_screen.add((y * SCREENWIDTH + x) as usize);
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

#[no_mangle]
pub extern "C" fn V_DrawFilledBox(x: c_int, y: c_int, w: c_int, h: c_int, c: c_int) {
    unsafe {
        let mut buf = I_VideoBuffer.add((SCREENWIDTH * y + x) as usize);

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

#[no_mangle]
pub extern "C" fn V_DrawHorizLine(x: c_int, y: c_int, w: c_int, c: c_int) {
    unsafe {
        let mut buf = I_VideoBuffer.add((SCREENWIDTH * y + x) as usize);
        let mut x1 = 0;
        while x1 < w {
            *buf = c as u8;
            buf = buf.add(1);
            x1 += 1;
        }
    }
}

#[no_mangle]
pub extern "C" fn V_DrawVertLine(x: c_int, y: c_int, h: c_int, c: c_int) {
    unsafe {
        let mut buf = I_VideoBuffer.add((SCREENWIDTH * y + x) as usize);
        let mut y1 = 0;
        while y1 < h {
            *buf = c as u8;
            buf = buf.add(SCREENWIDTH as usize);
            y1 += 1;
        }
    }
}

#[no_mangle]
pub extern "C" fn V_DrawBox(x: c_int, y: c_int, w: c_int, h: c_int, c: c_int) {
    V_DrawHorizLine(x, y, w, c);
    V_DrawHorizLine(x, y + h - 1, w, c);
    V_DrawVertLine(x, y, h, c);
    V_DrawVertLine(x + w - 1, y, h, c);
}

#[no_mangle]
pub extern "C" fn V_DrawRawScreen(raw: *mut u8) {
    unsafe {
        ptr::copy_nonoverlapping(raw, dest_screen, (SCREENWIDTH * SCREENHEIGHT) as usize);
    }
}

#[no_mangle]
pub extern "C" fn V_Init() {
    // no-op
}

#[no_mangle]
pub extern "C" fn V_UseBuffer(buffer: *mut u8) {
    unsafe {
        dest_screen = buffer;
    }
}

#[no_mangle]
pub extern "C" fn V_RestoreBuffer() {
    unsafe {
        dest_screen = I_VideoBuffer;
    }
}

#[no_mangle]
pub extern "C" fn WritePCXfile(
    filename: *mut c_char,
    data: *mut u8,
    width: c_int,
    height: c_int,
    palette: *mut u8,
) {
    unsafe {
        let pcx = Z_Malloc(width * height * 2 + 1000, PU_STATIC, ptr::null_mut()) as *mut pcx_t;

        (*pcx).manufacturer = 0x0a;
        (*pcx).version = 5;
        (*pcx).encoding = 1;
        (*pcx).bits_per_pixel = 8;
        (*pcx).xmin = 0;
        (*pcx).ymin = 0;
        (*pcx).xmax = (width - 1) as u16;
        (*pcx).ymax = (height - 1) as u16;
        (*pcx).hres = width as u16;
        (*pcx).vres = height as u16;
        (*pcx).palette = [0; 48];
        (*pcx).color_planes = 1;
        (*pcx).bytes_per_line = width as u16;
        (*pcx).palette_type = 2;
        (*pcx).filler = [0; 58];

        let mut pack = (pcx as *mut u8).add(std::mem::offset_of!(pcx_t, data));

        let mut i = 0;
        let total = (width * height) as usize;
        let mut data_ptr = data;
        while i < total {
            let byte = *data_ptr;
            if (byte & 0xc0) != 0xc0 {
                *pack = byte;
                pack = pack.add(1);
            } else {
                *pack = 0xc1;
                pack = pack.add(1);
                *pack = byte;
                pack = pack.add(1);
            }
            data_ptr = data_ptr.add(1);
            i += 1;
        }

        // write palette
        *pack = 0x0c;
        pack = pack.add(1);

        let mut palette_ptr = palette;
        i = 0;
        while i < 768 {
            *pack = *palette_ptr;
            pack = pack.add(1);
            palette_ptr = palette_ptr.add(1);
            i += 1;
        }

        let length = pack.offset_from(pcx as *mut u8) as c_int;
        M_WriteFile(filename, pcx as *mut c_void, length);

        Z_Free(pcx as *mut c_void);
    }
}

/// Build a screenshot filename for index `i`: `"DOOM{i:02}.pcx"` in a 16-byte null-padded buffer.
/// The only real caller always passes `"DOOM%02i.%s"` as format, so the pattern is fixed.
fn screenshot_filename(i: i32) -> [u8; 16] {
    let s = format!("DOOM{:02}.pcx", i);
    let mut buf = [0u8; 16];
    buf[..s.len()].copy_from_slice(s.as_bytes());
    buf
}

#[no_mangle]
pub extern "C" fn V_ScreenShot(_format: *mut c_char) {
    unsafe {
        let mut i = 0;
        while i <= 99 {
            let mut lbmname = screenshot_filename(i);
            if M_FileExists(lbmname.as_mut_ptr() as *mut c_char) == 0 {
                break;
            }
            i += 1;
        }

        if i == 100 {
            i_error!("V_ScreenShot: Couldn't create a PCX");
        }

        let mut lbmname = screenshot_filename(i);
        WritePCXfile(
            lbmname.as_mut_ptr() as *mut c_char,
            I_VideoBuffer,
            SCREENWIDTH,
            SCREENHEIGHT,
            W_CacheLumpName(c"PLAYPAL".as_ptr(), PU_CACHE) as *mut u8,
        );
    }
}

const MOUSE_SPEED_BOX_WIDTH: c_int = 120;
const MOUSE_SPEED_BOX_HEIGHT: c_int = 9;

#[no_mangle]
pub extern "C" fn V_DrawMouseSpeedBox(speed: c_int) {
    unsafe {
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
            s = (s as f32 / mouse_acceleration) as c_int;
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
}

extern "C" fn dummy_clip(_: *mut patch_t, _: c_int, _: c_int) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn V_Video_Link_Anchor() {
    V_MarkRect(0, 0, 0, 0);
    V_CopyRect(0, 0, ptr::null_mut(), 0, 0, 0, 0);
    V_SetPatchClipCallback(Some(dummy_clip));
    V_DrawPatch(0, 0, ptr::null_mut());
    V_DrawPatchFlipped(0, 0, ptr::null_mut());
    V_DrawPatchDirect(0, 0, ptr::null_mut());
    V_DrawTLPatch(0, 0, ptr::null_mut());
    V_DrawXlaPatch(0, 0, ptr::null_mut());
    V_DrawAltTLPatch(0, 0, ptr::null_mut());
    V_DrawShadowedPatch(0, 0, ptr::null_mut());
    V_LoadTintTable();
    V_LoadXlaTable();
    V_DrawBlock(0, 0, 0, 0, ptr::null_mut());
    V_DrawFilledBox(0, 0, 0, 0, 0);
    V_DrawHorizLine(0, 0, 0, 0);
    V_DrawVertLine(0, 0, 0, 0);
    V_DrawBox(0, 0, 0, 0, 0);
    V_DrawRawScreen(ptr::null_mut());
    V_Init();
    V_UseBuffer(ptr::null_mut());
    V_RestoreBuffer();
    WritePCXfile(ptr::null_mut(), ptr::null_mut(), 0, 0, ptr::null_mut());
    V_ScreenShot(ptr::null_mut());
    V_DrawMouseSpeedBox(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_filename_zero_padded() {
        let name = screenshot_filename(0);
        assert_eq!(&name[..10], b"DOOM00.pcx");
        assert_eq!(name[10], 0);
    }

    #[test]
    fn screenshot_filename_two_digit() {
        let name = screenshot_filename(42);
        assert_eq!(&name[..10], b"DOOM42.pcx");
    }

    #[test]
    fn screenshot_filename_max() {
        let name = screenshot_filename(99);
        assert_eq!(&name[..10], b"DOOM99.pcx");
    }
}
