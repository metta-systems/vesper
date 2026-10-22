use ::c2rust_bitfields;
extern "C" {
    static mut myargv: *mut *mut ::core::ffi::c_char;
    fn M_CheckParmWithArgs(
        check: *mut ::core::ffi::c_char,
        num_args: ::core::ffi::c_int,
    ) -> ::core::ffi::c_int;
    fn printf(__format: *const ::core::ffi::c_char, ...) -> ::core::ffi::c_int;
    fn snprintf(
        __s: *mut ::core::ffi::c_char,
        __maxlen: size_t,
        __format: *const ::core::ffi::c_char,
        ...
    ) -> ::core::ffi::c_int;
    fn memcpy(
        __dest: *mut ::core::ffi::c_void,
        __src: *const ::core::ffi::c_void,
        __n: size_t,
    ) -> *mut ::core::ffi::c_void;
    fn memset(
        __s: *mut ::core::ffi::c_void,
        __c: ::core::ffi::c_int,
        __n: size_t,
    ) -> *mut ::core::ffi::c_void;
    fn strcmp(
        __s1: *const ::core::ffi::c_char,
        __s2: *const ::core::ffi::c_char,
    ) -> ::core::ffi::c_int;
    fn I_ErrorV(msg: *const ::core::ffi::c_char);
    fn Z_Malloc(
        size: ::core::ffi::c_int,
        tag: ::core::ffi::c_int,
        ptr: *mut ::core::ffi::c_void,
    ) -> *mut ::core::ffi::c_void;
    fn Z_Free(ptr: *mut ::core::ffi::c_void);
    static gammatable: [[byte; 256]; 5];
    fn atoi(__nptr: *const ::core::ffi::c_char) -> ::core::ffi::c_int;
    static mut DG_ScreenBuffer: *mut pixel_t;
    fn DG_DrawFrame();
    fn DG_SetWindowTitle(title: *const ::core::ffi::c_char);
    fn I_GetEvent();
}
pub type size_t = usize;
pub type __uint8_t = u8;
pub type __uint16_t = u16;
pub type __uint32_t = u32;
pub type uint8_t = __uint8_t;
pub type uint16_t = __uint16_t;
pub type uint32_t = __uint32_t;
pub type boolean = ::core::ffi::c_uint;
pub type byte = uint8_t;
pub type grabmouse_callback_t = Option<unsafe extern "C" fn() -> boolean>;
pub const PU_STATIC: C2Rust_Unnamed = 1;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct FB_ScreenInfo {
    pub xres: uint32_t,
    pub yres: uint32_t,
    pub xres_virtual: uint32_t,
    pub yres_virtual: uint32_t,
    pub bits_per_pixel: uint32_t,
    pub red: FB_BitField,
    pub green: FB_BitField,
    pub blue: FB_BitField,
    pub transp: FB_BitField,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct FB_BitField {
    pub offset: uint32_t,
    pub length: uint32_t,
}
#[derive(Copy, Clone, ::c2rust_bitfields::BitfieldStruct)]
#[repr(C)]
pub struct color {
    #[bitfield(name = "b", ty = "uint32_t", bits = "0..=7")]
    #[bitfield(name = "g", ty = "uint32_t", bits = "8..=15")]
    #[bitfield(name = "r", ty = "uint32_t", bits = "16..=23")]
    #[bitfield(name = "a", ty = "uint32_t", bits = "24..=31")]
    pub b_g_r_a: [u8; 4],
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct col_t {
    pub r: byte,
    pub g: byte,
    pub b: byte,
}
pub type pixel_t = uint32_t;
pub type C2Rust_Unnamed = ::core::ffi::c_uint;
pub const PU_NUM_TAGS: C2Rust_Unnamed = 9;
pub const PU_CACHE: C2Rust_Unnamed = 8;
pub const PU_PURGELEVEL: C2Rust_Unnamed = 7;
pub const PU_LEVSPEC: C2Rust_Unnamed = 6;
pub const PU_LEVEL: C2Rust_Unnamed = 5;
pub const PU_FREE: C2Rust_Unnamed = 4;
pub const PU_MUSIC: C2Rust_Unnamed = 3;
pub const PU_SOUND: C2Rust_Unnamed = 2;
pub const INT_MAX: ::core::ffi::c_int = __INT_MAX__;
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
pub const SCREENWIDTH: ::core::ffi::c_int = 320 as ::core::ffi::c_int;
pub const SCREENHEIGHT: ::core::ffi::c_int = 200 as ::core::ffi::c_int;
pub const DOOMGENERIC_RESX: ::core::ffi::c_int = 640 as ::core::ffi::c_int;
pub const DOOMGENERIC_RESY: ::core::ffi::c_int = 400 as ::core::ffi::c_int;
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
#[no_mangle]
pub static mut fb_scaling: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
#[no_mangle]
pub static mut usemouse: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
static mut colors: [color; 256] = [color { b_g_r_a: [0; 4] }; 256];
#[no_mangle]
pub static mut I_VideoBuffer: *mut byte = ::core::ptr::null_mut::<byte>();
#[no_mangle]
pub static mut screensaver_mode: boolean = false_0 as boolean;
#[no_mangle]
pub static mut screenvisible: boolean = 0;
#[no_mangle]
pub static mut mouse_acceleration: ::core::ffi::c_float = 2.0f32;
#[no_mangle]
pub static mut mouse_threshold: ::core::ffi::c_int = 10 as ::core::ffi::c_int;
#[no_mangle]
pub static mut usegamma: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
static mut rgb565_palette: [uint16_t; 256] = [0; 256];
#[no_mangle]
pub unsafe extern "C" fn cmap_to_rgb565(
    mut out: *mut uint16_t,
    mut in_0: *mut uint8_t,
    mut in_pixels: ::core::ffi::c_int,
) {
    let mut i: ::core::ffi::c_int = 0;
    let mut j: ::core::ffi::c_int = 0;
    let mut c: color = color { b_g_r_a: [0; 4] };
    let mut r: uint16_t = 0;
    let mut g: uint16_t = 0;
    let mut b: uint16_t = 0;
    i = 0 as ::core::ffi::c_int;
    while i < in_pixels {
        c = colors[*in_0 as usize];
        r = (((c.r() as ::core::ffi::c_int >> 3 as ::core::ffi::c_int) as uint16_t
            as ::core::ffi::c_int)
            << 11 as ::core::ffi::c_int) as uint16_t;
        g = (((c.g() as ::core::ffi::c_int >> 2 as ::core::ffi::c_int) as uint16_t
            as ::core::ffi::c_int)
            << 5 as ::core::ffi::c_int) as uint16_t;
        b = (((c.b() as ::core::ffi::c_int >> 3 as ::core::ffi::c_int) as uint16_t
            as ::core::ffi::c_int)
            << 0 as ::core::ffi::c_int) as uint16_t;
        *out = (r as ::core::ffi::c_int | g as ::core::ffi::c_int | b as ::core::ffi::c_int)
            as uint16_t;
        in_0 = in_0.offset(1);
        j = 0 as ::core::ffi::c_int;
        while j < fb_scaling {
            out = out.offset(1);
            j += 1;
        }
        i += 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn cmap_to_fb(
    mut out: *mut uint8_t,
    mut in_0: *mut uint8_t,
    mut in_pixels: ::core::ffi::c_int,
) {
    let mut i: ::core::ffi::c_int = 0;
    let mut k: ::core::ffi::c_int = 0;
    let mut c: color = color { b_g_r_a: [0; 4] };
    let mut pix: uint32_t = 0;
    i = 0 as ::core::ffi::c_int;
    while i < in_pixels {
        c = colors[*in_0 as usize];
        if s_Fb.bits_per_pixel == 16 as uint32_t {
            let mut p: uint16_t = ((c.r() as ::core::ffi::c_int & 0xf8 as ::core::ffi::c_int)
                << 8 as ::core::ffi::c_int
                | (c.g() as ::core::ffi::c_int & 0xfc as ::core::ffi::c_int)
                    << 3 as ::core::ffi::c_int
                | c.b() as ::core::ffi::c_int >> 3 as ::core::ffi::c_int)
                as uint16_t;
            k = 0 as ::core::ffi::c_int;
            while k < fb_scaling {
                *(out as *mut uint16_t) = p;
                out = out.offset(2 as ::core::ffi::c_int as isize);
                k += 1;
            }
        } else if s_Fb.bits_per_pixel == 32 as uint32_t {
            pix = ((c.r() as ::core::ffi::c_int) << s_Fb.red.offset
                | (c.g() as ::core::ffi::c_int) << s_Fb.green.offset
                | (c.b() as ::core::ffi::c_int) << s_Fb.blue.offset) as uint32_t;
            k = 0 as ::core::ffi::c_int;
            while k < fb_scaling {
                *(out as *mut uint32_t) = pix;
                out = out.offset(4 as ::core::ffi::c_int as isize);
                k += 1;
            }
        } else {
            let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
            snprintf(
                &raw mut _i_err_buf as *mut ::core::ffi::c_char,
                ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
                b"No idea how to convert %d bpp pixels\0".as_ptr() as *const ::core::ffi::c_char,
                s_Fb.bits_per_pixel,
            );
            I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
        }
        in_0 = in_0.offset(1);
        i += 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn I_InitGraphics() {
    let mut i: ::core::ffi::c_int = 0;
    let mut gfxmodeparm: ::core::ffi::c_int = 0;
    let mut mode: *mut ::core::ffi::c_char = ::core::ptr::null_mut::<::core::ffi::c_char>();
    memset(
        &raw mut s_Fb as *mut ::core::ffi::c_void,
        0 as ::core::ffi::c_int,
        ::core::mem::size_of::<FB_ScreenInfo>() as size_t,
    );
    s_Fb.xres = DOOMGENERIC_RESX as uint32_t;
    s_Fb.yres = DOOMGENERIC_RESY as uint32_t;
    s_Fb.xres_virtual = s_Fb.xres;
    s_Fb.yres_virtual = s_Fb.yres;
    gfxmodeparm = M_CheckParmWithArgs(
        b"-gfxmode\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        1 as ::core::ffi::c_int,
    );
    if gfxmodeparm != 0 {
        mode = *myargv.offset((gfxmodeparm + 1 as ::core::ffi::c_int) as isize);
    } else {
        mode = b"rgba8888\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
    }
    if strcmp(mode, b"rgba8888\0".as_ptr() as *const ::core::ffi::c_char) == 0 as ::core::ffi::c_int
    {
        s_Fb.bits_per_pixel = 32 as uint32_t;
        s_Fb.blue.length = 8 as uint32_t;
        s_Fb.green.length = 8 as uint32_t;
        s_Fb.red.length = 8 as uint32_t;
        s_Fb.transp.length = 8 as uint32_t;
        s_Fb.blue.offset = 0 as uint32_t;
        s_Fb.green.offset = 8 as uint32_t;
        s_Fb.red.offset = 16 as uint32_t;
        s_Fb.transp.offset = 24 as uint32_t;
    } else if strcmp(mode, b"rgb565\0".as_ptr() as *const ::core::ffi::c_char)
        == 0 as ::core::ffi::c_int
    {
        s_Fb.bits_per_pixel = 16 as uint32_t;
        s_Fb.blue.length = 5 as uint32_t;
        s_Fb.green.length = 6 as uint32_t;
        s_Fb.red.length = 5 as uint32_t;
        s_Fb.transp.length = 0 as uint32_t;
        s_Fb.blue.offset = 11 as uint32_t;
        s_Fb.green.offset = 5 as uint32_t;
        s_Fb.red.offset = 0 as uint32_t;
        s_Fb.transp.offset = 16 as uint32_t;
    } else {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"Unknown gfxmode value: %s\n\0".as_ptr() as *const ::core::ffi::c_char,
            mode,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    printf(
        b"I_InitGraphics: framebuffer: x_res: %d, y_res: %d, x_virtual: %d, y_virtual: %d, bpp: %d\n\0"
            .as_ptr() as *const ::core::ffi::c_char,
        s_Fb.xres,
        s_Fb.yres,
        s_Fb.xres_virtual,
        s_Fb.yres_virtual,
        s_Fb.bits_per_pixel,
    );
    printf(
        b"I_InitGraphics: framebuffer: RGBA: %d%d%d%d, red_off: %d, green_off: %d, blue_off: %d, transp_off: %d\n\0"
            .as_ptr() as *const ::core::ffi::c_char,
        s_Fb.red.length,
        s_Fb.green.length,
        s_Fb.blue.length,
        s_Fb.transp.length,
        s_Fb.red.offset,
        s_Fb.green.offset,
        s_Fb.blue.offset,
        s_Fb.transp.offset,
    );
    printf(
        b"I_InitGraphics: DOOM screen size: w x h: %d x %d\n\0".as_ptr()
            as *const ::core::ffi::c_char,
        SCREENWIDTH,
        SCREENHEIGHT,
    );
    i = M_CheckParmWithArgs(
        b"-scaling\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        1 as ::core::ffi::c_int,
    );
    if i > 0 as ::core::ffi::c_int {
        i = atoi(*myargv.offset((i + 1 as ::core::ffi::c_int) as isize));
        fb_scaling = i;
        printf(
            b"I_InitGraphics: Scaling factor: %d\n\0".as_ptr() as *const ::core::ffi::c_char,
            fb_scaling,
        );
    } else {
        fb_scaling = s_Fb.xres.wrapping_div(SCREENWIDTH as uint32_t) as ::core::ffi::c_int;
        if s_Fb.yres.wrapping_div(SCREENHEIGHT as uint32_t) < fb_scaling as uint32_t {
            fb_scaling = s_Fb.yres.wrapping_div(SCREENHEIGHT as uint32_t) as ::core::ffi::c_int;
        }
        printf(
            b"I_InitGraphics: Auto-scaling factor: %d\n\0".as_ptr() as *const ::core::ffi::c_char,
            fb_scaling,
        );
    }
    I_VideoBuffer = Z_Malloc(
        SCREENWIDTH * SCREENHEIGHT,
        PU_STATIC as ::core::ffi::c_int,
        NULL,
    ) as *mut byte;
    screenvisible = true_0 as boolean;
    extern "C" {
        #[link_name = "I_InitInput"]
        fn I_InitInput_0();
    }
    I_InitInput_0();
}
#[no_mangle]
pub unsafe extern "C" fn I_ShutdownGraphics() {
    Z_Free(I_VideoBuffer as *mut ::core::ffi::c_void);
}
#[no_mangle]
pub unsafe extern "C" fn I_StartFrame() {}
#[no_mangle]
pub unsafe extern "C" fn I_StartTic() {
    I_GetEvent();
}
#[no_mangle]
pub unsafe extern "C" fn I_UpdateNoBlit() {}
#[no_mangle]
pub unsafe extern "C" fn I_FinishUpdate() {
    let mut y: ::core::ffi::c_int = 0;
    let mut x_offset: ::core::ffi::c_int = 0;
    let mut y_offset: ::core::ffi::c_int = 0;
    let mut x_offset_end: ::core::ffi::c_int = 0;
    let mut line_in: *mut ::core::ffi::c_uchar = ::core::ptr::null_mut::<::core::ffi::c_uchar>();
    let mut line_out: *mut ::core::ffi::c_uchar = ::core::ptr::null_mut::<::core::ffi::c_uchar>();
    y_offset = s_Fb
        .yres
        .wrapping_sub((SCREENHEIGHT * fb_scaling) as uint32_t)
        .wrapping_mul(s_Fb.bits_per_pixel)
        .wrapping_div(8 as uint32_t)
        .wrapping_div(2 as uint32_t) as ::core::ffi::c_int;
    x_offset = s_Fb
        .xres
        .wrapping_sub((SCREENWIDTH * fb_scaling) as uint32_t)
        .wrapping_mul(s_Fb.bits_per_pixel)
        .wrapping_div(8 as uint32_t)
        .wrapping_div(2 as uint32_t) as ::core::ffi::c_int;
    x_offset_end = s_Fb
        .xres
        .wrapping_sub((SCREENWIDTH * fb_scaling) as uint32_t)
        .wrapping_mul(s_Fb.bits_per_pixel)
        .wrapping_div(8 as uint32_t)
        .wrapping_sub(x_offset as uint32_t) as ::core::ffi::c_int;
    line_in = I_VideoBuffer as *mut ::core::ffi::c_uchar;
    line_out = DG_ScreenBuffer as *mut ::core::ffi::c_uchar;
    y = SCREENHEIGHT;
    loop {
        let c2rust_fresh3 = y;
        y = y - 1;
        if !(c2rust_fresh3 != 0) {
            break;
        }
        let mut i: ::core::ffi::c_int = 0;
        i = 0 as ::core::ffi::c_int;
        while i < fb_scaling {
            line_out = line_out.offset(x_offset as isize);
            cmap_to_fb(
                line_out as *mut ::core::ffi::c_void as *mut uint8_t,
                line_in as *mut ::core::ffi::c_void as *mut uint8_t,
                SCREENWIDTH,
            );
            line_out = line_out.offset(
                ((SCREENWIDTH * fb_scaling) as uint32_t)
                    .wrapping_mul(s_Fb.bits_per_pixel.wrapping_div(8 as uint32_t))
                    .wrapping_add(x_offset_end as uint32_t) as isize,
            );
            i += 1;
        }
        line_in = line_in.offset(SCREENWIDTH as isize);
    }
    DG_DrawFrame();
}
#[no_mangle]
pub unsafe extern "C" fn I_ReadScreen(mut scr: *mut byte) {
    memcpy(
        scr as *mut ::core::ffi::c_void,
        I_VideoBuffer as *const ::core::ffi::c_void,
        (SCREENWIDTH * SCREENHEIGHT) as size_t,
    );
}
#[no_mangle]
pub unsafe extern "C" fn I_SetPalette(mut palette: *mut byte) {
    let mut i: ::core::ffi::c_int = 0;
    i = 0 as ::core::ffi::c_int;
    while i < 256 as ::core::ffi::c_int {
        colors[i as usize].set_a(0 as uint32_t as uint32_t);
        let mut c2rust_rhs = {
            let c2rust_fresh0 = palette;
            palette = palette.offset(1);
            gammatable[usegamma as usize][*c2rust_fresh0 as usize] as uint32_t
        } as uint32_t;
        colors[i as usize].set_r(c2rust_rhs);
        let mut c2rust_rhs_0 = {
            let c2rust_fresh1 = palette;
            palette = palette.offset(1);
            gammatable[usegamma as usize][*c2rust_fresh1 as usize] as uint32_t
        } as uint32_t;
        colors[i as usize].set_g(c2rust_rhs_0);
        let mut c2rust_rhs_1 = {
            let c2rust_fresh2 = palette;
            palette = palette.offset(1);
            gammatable[usegamma as usize][*c2rust_fresh2 as usize] as uint32_t
        } as uint32_t;
        colors[i as usize].set_b(c2rust_rhs_1);
        i += 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn I_GetPaletteIndex(
    mut r: ::core::ffi::c_int,
    mut g: ::core::ffi::c_int,
    mut b: ::core::ffi::c_int,
) -> ::core::ffi::c_int {
    let mut best: ::core::ffi::c_int = 0;
    let mut best_diff: ::core::ffi::c_int = 0;
    let mut diff: ::core::ffi::c_int = 0;
    let mut i: ::core::ffi::c_int = 0;
    let mut color: col_t = col_t { r: 0, g: 0, b: 0 };
    printf(b"I_GetPaletteIndex\n\0".as_ptr() as *const ::core::ffi::c_char);
    best = 0 as ::core::ffi::c_int;
    best_diff = INT_MAX;
    i = 0 as ::core::ffi::c_int;
    while i < 256 as ::core::ffi::c_int {
        color.r = ((0xf800 as ::core::ffi::c_int
            & rgb565_palette[i as usize] as ::core::ffi::c_int)
            >> 11 as ::core::ffi::c_int) as byte;
        color.g = ((0x7e0 as ::core::ffi::c_int & rgb565_palette[i as usize] as ::core::ffi::c_int)
            >> 5 as ::core::ffi::c_int) as byte;
        color.b =
            (0x1f as ::core::ffi::c_int & rgb565_palette[i as usize] as ::core::ffi::c_int) as byte;
        diff = (r - color.r as ::core::ffi::c_int) * (r - color.r as ::core::ffi::c_int)
            + (g - color.g as ::core::ffi::c_int) * (g - color.g as ::core::ffi::c_int)
            + (b - color.b as ::core::ffi::c_int) * (b - color.b as ::core::ffi::c_int);
        if diff < best_diff {
            best = i;
            best_diff = diff;
        }
        if diff == 0 as ::core::ffi::c_int {
            break;
        }
        i += 1;
    }
    return best;
}
#[no_mangle]
pub unsafe extern "C" fn I_BeginRead() {}
#[no_mangle]
pub unsafe extern "C" fn I_EndRead() {}
#[no_mangle]
pub unsafe extern "C" fn I_SetWindowTitle(mut title: *mut ::core::ffi::c_char) {
    DG_SetWindowTitle(title);
}
#[no_mangle]
pub unsafe extern "C" fn I_GraphicsCheckCommandLine() {}
#[no_mangle]
pub unsafe extern "C" fn I_SetGrabMouseCallback(mut func: grabmouse_callback_t) {}
#[no_mangle]
pub unsafe extern "C" fn I_EnableLoadingDisk() {}
#[no_mangle]
pub unsafe extern "C" fn I_BindVideoVariables() {}
#[no_mangle]
pub unsafe extern "C" fn I_DisplayFPSDots(mut dots_on: boolean) {}
#[no_mangle]
pub unsafe extern "C" fn I_CheckIsScreensaver() {}
pub const __INT_MAX__: ::core::ffi::c_int = 2147483647 as ::core::ffi::c_int;
pub const true_0: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
pub const false_0: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
