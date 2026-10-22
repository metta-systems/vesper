//! Rust port of vendor/doomgeneric/v_video.c.
//!
//! Screen buffer management, patch drawing, block copying, and screenshots.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use {
    std::{mem::size_of, ptr},
    zerocopy::{little_endian::U16, IntoBytes},
};

use crate::{
    engine::{
        interface::video::{
            mouse_acceleration, mouse_threshold, usemouse, I_GetPaletteIndex, I_VideoBuffer,
            SCREENHEIGHT, SCREENWIDTH,
        },
        math::bbox::M_AddToBox,
        misc::{M_FileExists, M_WriteFile},
        wad::{
            format::{column_t, patch_t, pcx_t},
            W_CacheLumpName,
        },
        zone::{PU_CACHE, PU_STATIC},
    },
    i_error,
};

pub type vpatchclipfunc_t = Option<fn(*mut patch_t, i32, i32) -> i32>;

pub static mut tinttable: *mut u8 = ptr::null_mut();

pub static mut xlatab: *mut u8 = ptr::null_mut();

pub static mut dirtybox: [i32; 4] = [0; 4];

static mut dest_screen: *mut u8 = ptr::null_mut();
static mut patchclip_callback: vpatchclipfunc_t = None;

pub fn V_MarkRect(x: i32, y: i32, width: i32, height: i32) {
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

pub fn V_CopyRect(
    srcx: i32,
    srcy: i32,
    source: *mut u8,
    width: i32,
    height: i32,
    destx: i32,
    desty: i32,
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

pub fn V_SetPatchClipCallback(func: vpatchclipfunc_t) {
    unsafe {
        patchclip_callback = func;
    }
}

pub fn V_DrawPatch(x: i32, y: i32, patch: *mut patch_t) {
    unsafe {
        let y = y - (*patch).topoffset.get() as i32;
        let x = x - (*patch).leftoffset.get() as i32;

        if let Some(cb) = patchclip_callback {
            if cb(patch, x, y) == 0 {
                return;
            }
        }

        V_MarkRect(
            x,
            y,
            (*patch).width.get() as i32,
            (*patch).height.get() as i32,
        );

        let w = (*patch).width.get() as i32;
        let mut desttop = dest_screen.add((y * SCREENWIDTH + x) as usize);

        let mut col = 0;
        while col < w {
            let ofs = patch_t::column_offset(patch, col as usize);
            let column = (patch as *mut u8).add(ofs) as *mut column_t;

            let mut col_ptr = column;
            while (*col_ptr).topdelta != 0xff {
                let mut source = (col_ptr as *mut u8).add(3);
                let mut dest = desttop.add((*col_ptr).topdelta as usize * SCREENWIDTH as usize);
                let mut count = (*col_ptr).length as i32;

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

pub fn V_DrawPatchFlipped(x: i32, y: i32, patch: *mut patch_t) {
    unsafe {
        let y = y - (*patch).topoffset.get() as i32;
        let x = x - (*patch).leftoffset.get() as i32;

        if let Some(cb) = patchclip_callback {
            if cb(patch, x, y) == 0 {
                return;
            }
        }

        V_MarkRect(
            x,
            y,
            (*patch).width.get() as i32,
            (*patch).height.get() as i32,
        );

        let w = (*patch).width.get() as i32;
        let mut desttop = dest_screen.add((y * SCREENWIDTH + x) as usize);

        let mut col = 0;
        while col < w {
            let ofs = patch_t::column_offset(patch, (w - 1 - col) as usize);
            let column = (patch as *mut u8).add(ofs) as *mut column_t;

            let mut col_ptr = column;
            while (*col_ptr).topdelta != 0xff {
                let mut source = (col_ptr as *mut u8).add(3);
                let mut dest = desttop.add((*col_ptr).topdelta as usize * SCREENWIDTH as usize);
                let mut count = (*col_ptr).length as i32;

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

pub fn V_DrawPatchDirect(x: i32, y: i32, patch: *mut patch_t) {
    V_DrawPatch(x, y, patch);
}

pub fn V_DrawTLPatch(x: i32, y: i32, patch: *mut patch_t) {
    unsafe {
        let y = y - (*patch).topoffset.get() as i32;
        let x = x - (*patch).leftoffset.get() as i32;

        let w = (*patch).width.get() as i32;
        let mut desttop = dest_screen.add((y * SCREENWIDTH + x) as usize);

        let mut col = 0;
        while col < w {
            let ofs = patch_t::column_offset(patch, col as usize);
            let column = (patch as *mut u8).add(ofs) as *mut column_t;

            let mut col_ptr = column;
            while (*col_ptr).topdelta != 0xff {
                let mut source = (col_ptr as *mut u8).add(3);
                let mut dest = desttop.add((*col_ptr).topdelta as usize * SCREENWIDTH as usize);
                let mut count = (*col_ptr).length as i32;

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

pub fn V_DrawXlaPatch(x: i32, y: i32, patch: *mut patch_t) {
    unsafe {
        let y = y - (*patch).topoffset.get() as i32;
        let x = x - (*patch).leftoffset.get() as i32;

        if let Some(cb) = patchclip_callback {
            if cb(patch, x, y) == 0 {
                return;
            }
        }

        let w = (*patch).width.get() as i32;
        let mut desttop = dest_screen.add((y * SCREENWIDTH + x) as usize);

        let mut col = 0;
        while col < w {
            let ofs = patch_t::column_offset(patch, col as usize);
            let column = (patch as *mut u8).add(ofs) as *mut column_t;

            let mut col_ptr = column;
            while (*col_ptr).topdelta != 0xff {
                let mut source = (col_ptr as *mut u8).add(3);
                let mut dest = desttop.add((*col_ptr).topdelta as usize * SCREENWIDTH as usize);
                let mut count = (*col_ptr).length as i32;

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

pub fn V_DrawAltTLPatch(x: i32, y: i32, patch: *mut patch_t) {
    unsafe {
        let y = y - (*patch).topoffset.get() as i32;
        let x = x - (*patch).leftoffset.get() as i32;

        let w = (*patch).width.get() as i32;
        let mut desttop = dest_screen.add((y * SCREENWIDTH + x) as usize);

        let mut col = 0;
        while col < w {
            let ofs = patch_t::column_offset(patch, col as usize);
            let column = (patch as *mut u8).add(ofs) as *mut column_t;

            let mut col_ptr = column;
            while (*col_ptr).topdelta != 0xff {
                let mut source = (col_ptr as *mut u8).add(3);
                let mut dest = desttop.add((*col_ptr).topdelta as usize * SCREENWIDTH as usize);
                let mut count = (*col_ptr).length as i32;

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

pub fn V_DrawShadowedPatch(x: i32, y: i32, patch: *mut patch_t) {
    unsafe {
        let y = y - (*patch).topoffset.get() as i32;
        let x = x - (*patch).leftoffset.get() as i32;

        let w = (*patch).width.get() as i32;
        let mut desttop = dest_screen.add((y * SCREENWIDTH + x) as usize);
        let mut desttop2 = dest_screen.add(((y + 2) * SCREENWIDTH + (x + 2)) as usize);

        let mut col = 0;
        while col < w {
            let ofs = patch_t::column_offset(patch, col as usize);
            let column = (patch as *mut u8).add(ofs) as *mut column_t;

            let mut col_ptr = column;
            while (*col_ptr).topdelta != 0xff {
                let mut source = (col_ptr as *mut u8).add(3);
                let mut dest = desttop.add((*col_ptr).topdelta as usize * SCREENWIDTH as usize);
                let mut dest2 = desttop2.add((*col_ptr).topdelta as usize * SCREENWIDTH as usize);
                let mut count = (*col_ptr).length as i32;

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

pub fn V_LoadTintTable() {
    unsafe {
        tinttable = W_CacheLumpName(b"TINTTAB", PU_STATIC) as *mut u8;
    }
}

pub fn V_LoadXlaTable() {
    unsafe {
        xlatab = W_CacheLumpName(b"XLATAB", PU_STATIC) as *mut u8;
    }
}

pub fn V_DrawBlock(x: i32, y: i32, width: i32, height: i32, src: *mut u8) {
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

pub fn V_DrawFilledBox(x: i32, y: i32, w: i32, h: i32, c: i32) {
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

pub fn V_DrawHorizLine(x: i32, y: i32, w: i32, c: i32) {
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

pub fn V_DrawVertLine(x: i32, y: i32, h: i32, c: i32) {
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

pub fn V_DrawBox(x: i32, y: i32, w: i32, h: i32, c: i32) {
    V_DrawHorizLine(x, y, w, c);
    V_DrawHorizLine(x, y + h - 1, w, c);
    V_DrawVertLine(x, y, h, c);
    V_DrawVertLine(x + w - 1, y, h, c);
}

pub fn V_DrawRawScreen(raw: *mut u8) {
    unsafe {
        ptr::copy_nonoverlapping(raw, dest_screen, (SCREENWIDTH * SCREENHEIGHT) as usize);
    }
}

pub fn V_Init() {
    // no-op
}

pub fn V_UseBuffer(buffer: *mut u8) {
    unsafe {
        dest_screen = buffer;
    }
}

pub fn V_RestoreBuffer() {
    unsafe {
        dest_screen = I_VideoBuffer;
    }
}

pub fn WritePCXfile(filename: &str, data: *mut u8, width: i32, height: i32, palette: *mut u8) {
    unsafe {
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

        for &byte in std::slice::from_raw_parts(data, total) {
            if (byte & 0xc0) != 0xc0 {
                pcx.push(byte);
            } else {
                pcx.extend_from_slice(&[0xc1, byte]);
            }
        }

        // write palette
        pcx.push(0x0c);
        pcx.extend_from_slice(std::slice::from_raw_parts(palette, 768));

        M_WriteFile(filename, &pcx);
    }
}

/// Build a screenshot filename for index `i`: `"DOOM{i:02}.pcx"`, the
/// `"DOOM%02i.%s"` format the C caller passed.
fn screenshot_filename(i: i32) -> String {
    format!("DOOM{i:02}.pcx")
}

pub fn V_ScreenShot() {
    unsafe {
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

        WritePCXfile(
            &screenshot_filename(i),
            I_VideoBuffer,
            SCREENWIDTH,
            SCREENHEIGHT,
            W_CacheLumpName(b"PLAYPAL", PU_CACHE) as *mut u8,
        );
    }
}

const MOUSE_SPEED_BOX_WIDTH: i32 = 120;
const MOUSE_SPEED_BOX_HEIGHT: i32 = 9;

pub fn V_DrawMouseSpeedBox(speed: i32) {
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
