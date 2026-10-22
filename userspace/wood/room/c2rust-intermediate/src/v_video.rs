extern "C" {
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
    fn fabs(__x: ::core::ffi::c_double) -> ::core::ffi::c_double;
    fn I_ErrorV(msg: *const ::core::ffi::c_char);
    fn I_GetPaletteIndex(
        r: ::core::ffi::c_int,
        g: ::core::ffi::c_int,
        b: ::core::ffi::c_int,
    ) -> ::core::ffi::c_int;
    static mut mouse_acceleration: ::core::ffi::c_float;
    static mut mouse_threshold: ::core::ffi::c_int;
    static mut I_VideoBuffer: *mut byte;
    fn M_AddToBox(box_0: *mut fixed_t, x: fixed_t, y: fixed_t);
    fn M_WriteFile(
        name: *mut ::core::ffi::c_char,
        source: *mut ::core::ffi::c_void,
        length: ::core::ffi::c_int,
    ) -> boolean;
    fn M_FileExists(file: *mut ::core::ffi::c_char) -> boolean;
    fn M_snprintf_clamp(
        buf: *mut ::core::ffi::c_char,
        len: size_t,
        result: ::core::ffi::c_int,
    ) -> ::core::ffi::c_int;
    fn Z_Malloc(
        size: ::core::ffi::c_int,
        tag: ::core::ffi::c_int,
        ptr: *mut ::core::ffi::c_void,
    ) -> *mut ::core::ffi::c_void;
    fn Z_Free(ptr: *mut ::core::ffi::c_void);
    fn W_CacheLumpName(
        name: *mut ::core::ffi::c_char,
        tag: ::core::ffi::c_int,
    ) -> *mut ::core::ffi::c_void;
}
pub type size_t = usize;
pub type __uint8_t = u8;
pub type uint8_t = __uint8_t;
pub type boolean = ::core::ffi::c_uint;
pub type byte = uint8_t;
pub type fixed_t = ::core::ffi::c_int;
#[derive(Copy, Clone)]
#[repr(C, packed)]
pub struct patch_t {
    pub width: ::core::ffi::c_short,
    pub height: ::core::ffi::c_short,
    pub leftoffset: ::core::ffi::c_short,
    pub topoffset: ::core::ffi::c_short,
    pub columnofs: [::core::ffi::c_int; 8],
}
#[derive(Copy, Clone)]
#[repr(C, packed)]
pub struct post_t {
    pub topdelta: byte,
    pub length: byte,
}
pub type column_t = post_t;
pub type vpatchclipfunc_t =
    Option<unsafe extern "C" fn(*mut patch_t, ::core::ffi::c_int, ::core::ffi::c_int) -> boolean>;
pub const PU_CACHE: C2Rust_Unnamed = 8;
#[derive(Copy, Clone)]
#[repr(C, packed)]
pub struct pcx_t {
    pub manufacturer: ::core::ffi::c_char,
    pub version: ::core::ffi::c_char,
    pub encoding: ::core::ffi::c_char,
    pub bits_per_pixel: ::core::ffi::c_char,
    pub xmin: ::core::ffi::c_ushort,
    pub ymin: ::core::ffi::c_ushort,
    pub xmax: ::core::ffi::c_ushort,
    pub ymax: ::core::ffi::c_ushort,
    pub hres: ::core::ffi::c_ushort,
    pub vres: ::core::ffi::c_ushort,
    pub palette: [::core::ffi::c_uchar; 48],
    pub reserved: ::core::ffi::c_char,
    pub color_planes: ::core::ffi::c_char,
    pub bytes_per_line: ::core::ffi::c_ushort,
    pub palette_type: ::core::ffi::c_ushort,
    pub filler: [::core::ffi::c_char; 58],
    pub data: ::core::ffi::c_uchar,
}
pub const PU_STATIC: C2Rust_Unnamed = 1;
pub type C2Rust_Unnamed = ::core::ffi::c_uint;
pub const PU_NUM_TAGS: C2Rust_Unnamed = 9;
pub const PU_PURGELEVEL: C2Rust_Unnamed = 7;
pub const PU_LEVSPEC: C2Rust_Unnamed = 6;
pub const PU_LEVEL: C2Rust_Unnamed = 5;
pub const PU_FREE: C2Rust_Unnamed = 4;
pub const PU_MUSIC: C2Rust_Unnamed = 3;
pub const PU_SOUND: C2Rust_Unnamed = 2;
pub const SCREENWIDTH: ::core::ffi::c_int = 320 as ::core::ffi::c_int;
pub const SCREENHEIGHT: ::core::ffi::c_int = 200 as ::core::ffi::c_int;
#[no_mangle]
pub static mut tinttable: *mut byte = ::core::ptr::null_mut::<byte>();
#[no_mangle]
pub static mut xlatab: *mut byte = ::core::ptr::null_mut::<byte>();
static mut dest_screen: *mut byte = ::core::ptr::null_mut::<byte>();
#[no_mangle]
pub static mut dirtybox: [::core::ffi::c_int; 4] = [0; 4];
static mut patchclip_callback: vpatchclipfunc_t = None;
#[no_mangle]
pub unsafe extern "C" fn V_MarkRect(
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut width: ::core::ffi::c_int,
    mut height: ::core::ffi::c_int,
) {
    if dest_screen == I_VideoBuffer {
        M_AddToBox(
            &raw mut dirtybox as *mut fixed_t,
            x as fixed_t,
            y as fixed_t,
        );
        M_AddToBox(
            &raw mut dirtybox as *mut fixed_t,
            x as fixed_t + width as fixed_t - 1 as fixed_t,
            y as fixed_t + height as fixed_t - 1 as fixed_t,
        );
    }
}
#[no_mangle]
pub unsafe extern "C" fn V_CopyRect(
    mut srcx: ::core::ffi::c_int,
    mut srcy: ::core::ffi::c_int,
    mut source: *mut byte,
    mut width: ::core::ffi::c_int,
    mut height: ::core::ffi::c_int,
    mut destx: ::core::ffi::c_int,
    mut desty: ::core::ffi::c_int,
) {
    let mut src: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut dest: *mut byte = ::core::ptr::null_mut::<byte>();
    if srcx < 0 as ::core::ffi::c_int
        || srcx + width > SCREENWIDTH
        || srcy < 0 as ::core::ffi::c_int
        || srcy + height > SCREENHEIGHT
        || destx < 0 as ::core::ffi::c_int
        || destx + width > SCREENWIDTH
        || desty < 0 as ::core::ffi::c_int
        || desty + height > SCREENHEIGHT
    {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"Bad V_CopyRect\0".as_ptr() as *const ::core::ffi::c_char,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    V_MarkRect(destx, desty, width, height);
    src = source
        .offset((SCREENWIDTH * srcy) as isize)
        .offset(srcx as isize);
    dest = dest_screen
        .offset((SCREENWIDTH * desty) as isize)
        .offset(destx as isize);
    while height > 0 as ::core::ffi::c_int {
        memcpy(
            dest as *mut ::core::ffi::c_void,
            src as *const ::core::ffi::c_void,
            width as size_t,
        );
        src = src.offset(SCREENWIDTH as isize);
        dest = dest.offset(SCREENWIDTH as isize);
        height -= 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn V_SetPatchClipCallback(mut func: vpatchclipfunc_t) {
    patchclip_callback = func;
}
#[no_mangle]
pub unsafe extern "C" fn V_DrawPatch(
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut patch: *mut patch_t,
) {
    let mut count: ::core::ffi::c_int = 0;
    let mut col: ::core::ffi::c_int = 0;
    let mut column: *mut column_t = ::core::ptr::null_mut::<column_t>();
    let mut desttop: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut dest: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut source: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut w: ::core::ffi::c_int = 0;
    y -= (*patch).topoffset as ::core::ffi::c_int;
    x -= (*patch).leftoffset as ::core::ffi::c_int;
    if patchclip_callback.is_some() {
        if patchclip_callback.expect("non-null function pointer")(patch, x, y) == 0 {
            return;
        }
    }
    if x < 0 as ::core::ffi::c_int
        || x + (*patch).width as ::core::ffi::c_int > SCREENWIDTH
        || y < 0 as ::core::ffi::c_int
        || y + (*patch).height as ::core::ffi::c_int > SCREENHEIGHT
    {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"Bad V_DrawPatch x=%i y=%i patch.width=%i patch.height=%i topoffset=%i leftoffset=%i\0"
                .as_ptr() as *const ::core::ffi::c_char,
            x,
            y,
            (*patch).width as ::core::ffi::c_int,
            (*patch).height as ::core::ffi::c_int,
            (*patch).topoffset as ::core::ffi::c_int,
            (*patch).leftoffset as ::core::ffi::c_int,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    V_MarkRect(
        x,
        y,
        (*patch).width as ::core::ffi::c_int,
        (*patch).height as ::core::ffi::c_int,
    );
    col = 0 as ::core::ffi::c_int;
    desttop = dest_screen
        .offset((y * SCREENWIDTH) as isize)
        .offset(x as isize);
    w = (*patch).width as ::core::ffi::c_int;
    while col < w {
        column =
            (patch as *mut byte).offset((*patch).columnofs[col as usize] as isize) as *mut column_t;
        while (*column).topdelta as ::core::ffi::c_int != 0xff as ::core::ffi::c_int {
            source = (column as *mut byte).offset(3 as ::core::ffi::c_int as isize);
            dest =
                desttop.offset(((*column).topdelta as ::core::ffi::c_int * SCREENWIDTH) as isize);
            count = (*column).length as ::core::ffi::c_int;
            loop {
                let c2rust_fresh0 = count;
                count = count - 1;
                if !(c2rust_fresh0 != 0) {
                    break;
                }
                let c2rust_fresh1 = source;
                source = source.offset(1);
                *dest = *c2rust_fresh1;
                dest = dest.offset(SCREENWIDTH as isize);
            }
            column = (column as *mut byte)
                .offset((*column).length as ::core::ffi::c_int as isize)
                .offset(4 as ::core::ffi::c_int as isize) as *mut column_t;
        }
        x += 1;
        col += 1;
        desttop = desttop.offset(1);
    }
}
#[no_mangle]
pub unsafe extern "C" fn V_DrawPatchFlipped(
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut patch: *mut patch_t,
) {
    let mut count: ::core::ffi::c_int = 0;
    let mut col: ::core::ffi::c_int = 0;
    let mut column: *mut column_t = ::core::ptr::null_mut::<column_t>();
    let mut desttop: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut dest: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut source: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut w: ::core::ffi::c_int = 0;
    y -= (*patch).topoffset as ::core::ffi::c_int;
    x -= (*patch).leftoffset as ::core::ffi::c_int;
    if patchclip_callback.is_some() {
        if patchclip_callback.expect("non-null function pointer")(patch, x, y) == 0 {
            return;
        }
    }
    if x < 0 as ::core::ffi::c_int
        || x + (*patch).width as ::core::ffi::c_int > SCREENWIDTH
        || y < 0 as ::core::ffi::c_int
        || y + (*patch).height as ::core::ffi::c_int > SCREENHEIGHT
    {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"Bad V_DrawPatchFlipped\0".as_ptr() as *const ::core::ffi::c_char,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    V_MarkRect(
        x,
        y,
        (*patch).width as ::core::ffi::c_int,
        (*patch).height as ::core::ffi::c_int,
    );
    col = 0 as ::core::ffi::c_int;
    desttop = dest_screen
        .offset((y * SCREENWIDTH) as isize)
        .offset(x as isize);
    w = (*patch).width as ::core::ffi::c_int;
    while col < w {
        column = (patch as *mut byte)
            .offset((*patch).columnofs[(w - 1 as ::core::ffi::c_int - col) as usize] as isize)
            as *mut column_t;
        while (*column).topdelta as ::core::ffi::c_int != 0xff as ::core::ffi::c_int {
            source = (column as *mut byte).offset(3 as ::core::ffi::c_int as isize);
            dest =
                desttop.offset(((*column).topdelta as ::core::ffi::c_int * SCREENWIDTH) as isize);
            count = (*column).length as ::core::ffi::c_int;
            loop {
                let c2rust_fresh2 = count;
                count = count - 1;
                if !(c2rust_fresh2 != 0) {
                    break;
                }
                let c2rust_fresh3 = source;
                source = source.offset(1);
                *dest = *c2rust_fresh3;
                dest = dest.offset(SCREENWIDTH as isize);
            }
            column = (column as *mut byte)
                .offset((*column).length as ::core::ffi::c_int as isize)
                .offset(4 as ::core::ffi::c_int as isize) as *mut column_t;
        }
        x += 1;
        col += 1;
        desttop = desttop.offset(1);
    }
}
#[no_mangle]
pub unsafe extern "C" fn V_DrawPatchDirect(
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut patch: *mut patch_t,
) {
    V_DrawPatch(x, y, patch);
}
#[no_mangle]
pub unsafe extern "C" fn V_DrawTLPatch(
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut patch: *mut patch_t,
) {
    let mut count: ::core::ffi::c_int = 0;
    let mut col: ::core::ffi::c_int = 0;
    let mut column: *mut column_t = ::core::ptr::null_mut::<column_t>();
    let mut desttop: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut dest: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut source: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut w: ::core::ffi::c_int = 0;
    y -= (*patch).topoffset as ::core::ffi::c_int;
    x -= (*patch).leftoffset as ::core::ffi::c_int;
    if x < 0 as ::core::ffi::c_int
        || x + (*patch).width as ::core::ffi::c_int > SCREENWIDTH
        || y < 0 as ::core::ffi::c_int
        || y + (*patch).height as ::core::ffi::c_int > SCREENHEIGHT
    {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"Bad V_DrawTLPatch\0".as_ptr() as *const ::core::ffi::c_char,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    col = 0 as ::core::ffi::c_int;
    desttop = dest_screen
        .offset((y * SCREENWIDTH) as isize)
        .offset(x as isize);
    w = (*patch).width as ::core::ffi::c_int;
    while col < w {
        column =
            (patch as *mut byte).offset((*patch).columnofs[col as usize] as isize) as *mut column_t;
        while (*column).topdelta as ::core::ffi::c_int != 0xff as ::core::ffi::c_int {
            source = (column as *mut byte).offset(3 as ::core::ffi::c_int as isize);
            dest =
                desttop.offset(((*column).topdelta as ::core::ffi::c_int * SCREENWIDTH) as isize);
            count = (*column).length as ::core::ffi::c_int;
            loop {
                let c2rust_fresh4 = count;
                count = count - 1;
                if !(c2rust_fresh4 != 0) {
                    break;
                }
                let c2rust_fresh5 = source;
                source = source.offset(1);
                *dest = *tinttable.offset(
                    (((*dest as ::core::ffi::c_int) << 8 as ::core::ffi::c_int)
                        + *c2rust_fresh5 as ::core::ffi::c_int) as isize,
                );
                dest = dest.offset(SCREENWIDTH as isize);
            }
            column = (column as *mut byte)
                .offset((*column).length as ::core::ffi::c_int as isize)
                .offset(4 as ::core::ffi::c_int as isize) as *mut column_t;
        }
        x += 1;
        col += 1;
        desttop = desttop.offset(1);
    }
}
#[no_mangle]
pub unsafe extern "C" fn V_DrawXlaPatch(
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut patch: *mut patch_t,
) {
    let mut count: ::core::ffi::c_int = 0;
    let mut col: ::core::ffi::c_int = 0;
    let mut column: *mut column_t = ::core::ptr::null_mut::<column_t>();
    let mut desttop: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut dest: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut source: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut w: ::core::ffi::c_int = 0;
    y -= (*patch).topoffset as ::core::ffi::c_int;
    x -= (*patch).leftoffset as ::core::ffi::c_int;
    if patchclip_callback.is_some() {
        if patchclip_callback.expect("non-null function pointer")(patch, x, y) == 0 {
            return;
        }
    }
    col = 0 as ::core::ffi::c_int;
    desttop = dest_screen
        .offset((y * SCREENWIDTH) as isize)
        .offset(x as isize);
    w = (*patch).width as ::core::ffi::c_int;
    while col < w {
        column =
            (patch as *mut byte).offset((*patch).columnofs[col as usize] as isize) as *mut column_t;
        while (*column).topdelta as ::core::ffi::c_int != 0xff as ::core::ffi::c_int {
            source = (column as *mut byte).offset(3 as ::core::ffi::c_int as isize);
            dest =
                desttop.offset(((*column).topdelta as ::core::ffi::c_int * SCREENWIDTH) as isize);
            count = (*column).length as ::core::ffi::c_int;
            loop {
                let c2rust_fresh10 = count;
                count = count - 1;
                if !(c2rust_fresh10 != 0) {
                    break;
                }
                *dest = *xlatab.offset(
                    (*dest as ::core::ffi::c_int
                        + ((*source as ::core::ffi::c_int) << 8 as ::core::ffi::c_int))
                        as isize,
                );
                source = source.offset(1);
                dest = dest.offset(SCREENWIDTH as isize);
            }
            column = (column as *mut byte)
                .offset((*column).length as ::core::ffi::c_int as isize)
                .offset(4 as ::core::ffi::c_int as isize) as *mut column_t;
        }
        x += 1;
        col += 1;
        desttop = desttop.offset(1);
    }
}
#[no_mangle]
pub unsafe extern "C" fn V_DrawAltTLPatch(
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut patch: *mut patch_t,
) {
    let mut count: ::core::ffi::c_int = 0;
    let mut col: ::core::ffi::c_int = 0;
    let mut column: *mut column_t = ::core::ptr::null_mut::<column_t>();
    let mut desttop: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut dest: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut source: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut w: ::core::ffi::c_int = 0;
    y -= (*patch).topoffset as ::core::ffi::c_int;
    x -= (*patch).leftoffset as ::core::ffi::c_int;
    if x < 0 as ::core::ffi::c_int
        || x + (*patch).width as ::core::ffi::c_int > SCREENWIDTH
        || y < 0 as ::core::ffi::c_int
        || y + (*patch).height as ::core::ffi::c_int > SCREENHEIGHT
    {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"Bad V_DrawAltTLPatch\0".as_ptr() as *const ::core::ffi::c_char,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    col = 0 as ::core::ffi::c_int;
    desttop = dest_screen
        .offset((y * SCREENWIDTH) as isize)
        .offset(x as isize);
    w = (*patch).width as ::core::ffi::c_int;
    while col < w {
        column =
            (patch as *mut byte).offset((*patch).columnofs[col as usize] as isize) as *mut column_t;
        while (*column).topdelta as ::core::ffi::c_int != 0xff as ::core::ffi::c_int {
            source = (column as *mut byte).offset(3 as ::core::ffi::c_int as isize);
            dest =
                desttop.offset(((*column).topdelta as ::core::ffi::c_int * SCREENWIDTH) as isize);
            count = (*column).length as ::core::ffi::c_int;
            loop {
                let c2rust_fresh6 = count;
                count = count - 1;
                if !(c2rust_fresh6 != 0) {
                    break;
                }
                let c2rust_fresh7 = source;
                source = source.offset(1);
                *dest = *tinttable.offset(
                    (((*dest as ::core::ffi::c_int) << 8 as ::core::ffi::c_int)
                        + *c2rust_fresh7 as ::core::ffi::c_int) as isize,
                );
                dest = dest.offset(SCREENWIDTH as isize);
            }
            column = (column as *mut byte)
                .offset((*column).length as ::core::ffi::c_int as isize)
                .offset(4 as ::core::ffi::c_int as isize) as *mut column_t;
        }
        x += 1;
        col += 1;
        desttop = desttop.offset(1);
    }
}
#[no_mangle]
pub unsafe extern "C" fn V_DrawShadowedPatch(
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut patch: *mut patch_t,
) {
    let mut count: ::core::ffi::c_int = 0;
    let mut col: ::core::ffi::c_int = 0;
    let mut column: *mut column_t = ::core::ptr::null_mut::<column_t>();
    let mut desttop: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut dest: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut source: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut desttop2: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut dest2: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut w: ::core::ffi::c_int = 0;
    y -= (*patch).topoffset as ::core::ffi::c_int;
    x -= (*patch).leftoffset as ::core::ffi::c_int;
    if x < 0 as ::core::ffi::c_int
        || x + (*patch).width as ::core::ffi::c_int > SCREENWIDTH
        || y < 0 as ::core::ffi::c_int
        || y + (*patch).height as ::core::ffi::c_int > SCREENHEIGHT
    {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"Bad V_DrawShadowedPatch\0".as_ptr() as *const ::core::ffi::c_char,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    col = 0 as ::core::ffi::c_int;
    desttop = dest_screen
        .offset((y * SCREENWIDTH) as isize)
        .offset(x as isize);
    desttop2 = dest_screen
        .offset(((y + 2 as ::core::ffi::c_int) * SCREENWIDTH) as isize)
        .offset(x as isize)
        .offset(2 as ::core::ffi::c_int as isize);
    w = (*patch).width as ::core::ffi::c_int;
    while col < w {
        column =
            (patch as *mut byte).offset((*patch).columnofs[col as usize] as isize) as *mut column_t;
        while (*column).topdelta as ::core::ffi::c_int != 0xff as ::core::ffi::c_int {
            source = (column as *mut byte).offset(3 as ::core::ffi::c_int as isize);
            dest =
                desttop.offset(((*column).topdelta as ::core::ffi::c_int * SCREENWIDTH) as isize);
            dest2 =
                desttop2.offset(((*column).topdelta as ::core::ffi::c_int * SCREENWIDTH) as isize);
            count = (*column).length as ::core::ffi::c_int;
            loop {
                let c2rust_fresh8 = count;
                count = count - 1;
                if !(c2rust_fresh8 != 0) {
                    break;
                }
                *dest2 = *tinttable
                    .offset(((*dest2 as ::core::ffi::c_int) << 8 as ::core::ffi::c_int) as isize);
                dest2 = dest2.offset(SCREENWIDTH as isize);
                let c2rust_fresh9 = source;
                source = source.offset(1);
                *dest = *c2rust_fresh9;
                dest = dest.offset(SCREENWIDTH as isize);
            }
            column = (column as *mut byte)
                .offset((*column).length as ::core::ffi::c_int as isize)
                .offset(4 as ::core::ffi::c_int as isize) as *mut column_t;
        }
        x += 1;
        col += 1;
        desttop = desttop.offset(1);
        desttop2 = desttop2.offset(1);
    }
}
#[no_mangle]
pub unsafe extern "C" fn V_LoadTintTable() {
    tinttable = W_CacheLumpName(
        b"TINTTAB\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        PU_STATIC as ::core::ffi::c_int,
    ) as *mut byte;
}
#[no_mangle]
pub unsafe extern "C" fn V_LoadXlaTable() {
    xlatab = W_CacheLumpName(
        b"XLATAB\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        PU_STATIC as ::core::ffi::c_int,
    ) as *mut byte;
}
#[no_mangle]
pub unsafe extern "C" fn V_DrawBlock(
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut width: ::core::ffi::c_int,
    mut height: ::core::ffi::c_int,
    mut src: *mut byte,
) {
    let mut dest: *mut byte = ::core::ptr::null_mut::<byte>();
    if x < 0 as ::core::ffi::c_int
        || x + width > SCREENWIDTH
        || y < 0 as ::core::ffi::c_int
        || y + height > SCREENHEIGHT
    {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"Bad V_DrawBlock\0".as_ptr() as *const ::core::ffi::c_char,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    V_MarkRect(x, y, width, height);
    dest = dest_screen
        .offset((y * SCREENWIDTH) as isize)
        .offset(x as isize);
    loop {
        let c2rust_fresh11 = height;
        height = height - 1;
        if !(c2rust_fresh11 != 0) {
            break;
        }
        memcpy(
            dest as *mut ::core::ffi::c_void,
            src as *const ::core::ffi::c_void,
            width as size_t,
        );
        src = src.offset(width as isize);
        dest = dest.offset(SCREENWIDTH as isize);
    }
}
#[no_mangle]
pub unsafe extern "C" fn V_DrawFilledBox(
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut w: ::core::ffi::c_int,
    mut h: ::core::ffi::c_int,
    mut c: ::core::ffi::c_int,
) {
    let mut buf: *mut uint8_t = ::core::ptr::null_mut::<uint8_t>();
    let mut buf1: *mut uint8_t = ::core::ptr::null_mut::<uint8_t>();
    let mut x1: ::core::ffi::c_int = 0;
    let mut y1: ::core::ffi::c_int = 0;
    buf = I_VideoBuffer
        .offset((SCREENWIDTH * y) as isize)
        .offset(x as isize) as *mut uint8_t;
    y1 = 0 as ::core::ffi::c_int;
    while y1 < h {
        buf1 = buf;
        x1 = 0 as ::core::ffi::c_int;
        while x1 < w {
            let c2rust_fresh12 = buf1;
            buf1 = buf1.offset(1);
            *c2rust_fresh12 = c as uint8_t;
            x1 += 1;
        }
        buf = buf.offset(SCREENWIDTH as isize);
        y1 += 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn V_DrawHorizLine(
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut w: ::core::ffi::c_int,
    mut c: ::core::ffi::c_int,
) {
    let mut buf: *mut uint8_t = ::core::ptr::null_mut::<uint8_t>();
    let mut x1: ::core::ffi::c_int = 0;
    buf = I_VideoBuffer
        .offset((SCREENWIDTH * y) as isize)
        .offset(x as isize) as *mut uint8_t;
    x1 = 0 as ::core::ffi::c_int;
    while x1 < w {
        let c2rust_fresh13 = buf;
        buf = buf.offset(1);
        *c2rust_fresh13 = c as uint8_t;
        x1 += 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn V_DrawVertLine(
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut h: ::core::ffi::c_int,
    mut c: ::core::ffi::c_int,
) {
    let mut buf: *mut uint8_t = ::core::ptr::null_mut::<uint8_t>();
    let mut y1: ::core::ffi::c_int = 0;
    buf = I_VideoBuffer
        .offset((SCREENWIDTH * y) as isize)
        .offset(x as isize) as *mut uint8_t;
    y1 = 0 as ::core::ffi::c_int;
    while y1 < h {
        *buf = c as uint8_t;
        buf = buf.offset(SCREENWIDTH as isize);
        y1 += 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn V_DrawBox(
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut w: ::core::ffi::c_int,
    mut h: ::core::ffi::c_int,
    mut c: ::core::ffi::c_int,
) {
    V_DrawHorizLine(x, y, w, c);
    V_DrawHorizLine(x, y + h - 1 as ::core::ffi::c_int, w, c);
    V_DrawVertLine(x, y, h, c);
    V_DrawVertLine(x + w - 1 as ::core::ffi::c_int, y, h, c);
}
#[no_mangle]
pub unsafe extern "C" fn V_DrawRawScreen(mut raw: *mut byte) {
    memcpy(
        dest_screen as *mut ::core::ffi::c_void,
        raw as *const ::core::ffi::c_void,
        (SCREENWIDTH * SCREENHEIGHT) as size_t,
    );
}
#[no_mangle]
pub unsafe extern "C" fn V_Init() {}
#[no_mangle]
pub unsafe extern "C" fn V_UseBuffer(mut buffer: *mut byte) {
    dest_screen = buffer;
}
#[no_mangle]
pub unsafe extern "C" fn V_RestoreBuffer() {
    dest_screen = I_VideoBuffer;
}
#[no_mangle]
pub unsafe extern "C" fn WritePCXfile(
    mut filename: *mut ::core::ffi::c_char,
    mut data: *mut byte,
    mut width: ::core::ffi::c_int,
    mut height: ::core::ffi::c_int,
    mut palette: *mut byte,
) {
    let mut i: ::core::ffi::c_int = 0;
    let mut length: ::core::ffi::c_int = 0;
    let mut pcx: *mut pcx_t = ::core::ptr::null_mut::<pcx_t>();
    let mut pack: *mut byte = ::core::ptr::null_mut::<byte>();
    pcx = Z_Malloc(
        width * height * 2 as ::core::ffi::c_int + 1000 as ::core::ffi::c_int,
        PU_STATIC as ::core::ffi::c_int,
        NULL,
    ) as *mut pcx_t;
    (*pcx).manufacturer = 0xa as ::core::ffi::c_char;
    (*pcx).version = 5 as ::core::ffi::c_char;
    (*pcx).encoding = 1 as ::core::ffi::c_char;
    (*pcx).bits_per_pixel = 8 as ::core::ffi::c_char;
    (*pcx).xmin = 0 as ::core::ffi::c_ushort;
    (*pcx).ymin = 0 as ::core::ffi::c_ushort;
    (*pcx).xmax =
        (width - 1 as ::core::ffi::c_int) as ::core::ffi::c_short as ::core::ffi::c_ushort;
    (*pcx).ymax =
        (height - 1 as ::core::ffi::c_int) as ::core::ffi::c_short as ::core::ffi::c_ushort;
    (*pcx).hres = width as ::core::ffi::c_short as ::core::ffi::c_ushort;
    (*pcx).vres = height as ::core::ffi::c_short as ::core::ffi::c_ushort;
    memset(
        &raw mut (*pcx).palette as *mut ::core::ffi::c_uchar as *mut ::core::ffi::c_void,
        0 as ::core::ffi::c_int,
        ::core::mem::size_of::<[::core::ffi::c_uchar; 48]>() as size_t,
    );
    (*pcx).color_planes = 1 as ::core::ffi::c_char;
    (*pcx).bytes_per_line = width as ::core::ffi::c_short as ::core::ffi::c_ushort;
    (*pcx).palette_type = 2 as ::core::ffi::c_int as ::core::ffi::c_short as ::core::ffi::c_ushort;
    memset(
        &raw mut (*pcx).filler as *mut ::core::ffi::c_char as *mut ::core::ffi::c_void,
        0 as ::core::ffi::c_int,
        ::core::mem::size_of::<[::core::ffi::c_char; 58]>() as size_t,
    );
    pack = &raw mut (*pcx).data as *mut byte;
    i = 0 as ::core::ffi::c_int;
    while i < width * height {
        if *data as ::core::ffi::c_int & 0xc0 as ::core::ffi::c_int != 0xc0 as ::core::ffi::c_int {
            let c2rust_fresh14 = data;
            data = data.offset(1);
            let c2rust_fresh15 = pack;
            pack = pack.offset(1);
            *c2rust_fresh15 = *c2rust_fresh14;
        } else {
            let c2rust_fresh16 = pack;
            pack = pack.offset(1);
            *c2rust_fresh16 = 0xc1 as byte;
            let c2rust_fresh17 = data;
            data = data.offset(1);
            let c2rust_fresh18 = pack;
            pack = pack.offset(1);
            *c2rust_fresh18 = *c2rust_fresh17;
        }
        i += 1;
    }
    let c2rust_fresh19 = pack;
    pack = pack.offset(1);
    *c2rust_fresh19 = 0xc as byte;
    i = 0 as ::core::ffi::c_int;
    while i < 768 as ::core::ffi::c_int {
        let c2rust_fresh20 = palette;
        palette = palette.offset(1);
        let c2rust_fresh21 = pack;
        pack = pack.offset(1);
        *c2rust_fresh21 = *c2rust_fresh20;
        i += 1;
    }
    length = pack.offset_from(pcx as *mut byte) as ::core::ffi::c_long as ::core::ffi::c_int;
    M_WriteFile(filename, pcx as *mut ::core::ffi::c_void, length);
    Z_Free(pcx as *mut ::core::ffi::c_void);
}
#[no_mangle]
pub unsafe extern "C" fn V_ScreenShot(mut format: *mut ::core::ffi::c_char) {
    let mut i: ::core::ffi::c_int = 0;
    let mut lbmname: [::core::ffi::c_char; 16] = [0; 16];
    let mut ext: *mut ::core::ffi::c_char = ::core::ptr::null_mut::<::core::ffi::c_char>();
    ext = b"pcx\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
    i = 0 as ::core::ffi::c_int;
    while i <= 99 as ::core::ffi::c_int {
        M_snprintf_clamp(
            &raw mut lbmname as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 16]>() as size_t,
            snprintf(
                &raw mut lbmname as *mut ::core::ffi::c_char,
                ::core::mem::size_of::<[::core::ffi::c_char; 16]>() as size_t,
                format,
                i,
                ext,
            ),
        );
        if M_FileExists(&raw mut lbmname as *mut ::core::ffi::c_char) == 0 {
            break;
        }
        i += 1;
    }
    if i == 100 as ::core::ffi::c_int {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"V_ScreenShot: Couldn't create a PCX\0".as_ptr() as *const ::core::ffi::c_char,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    WritePCXfile(
        &raw mut lbmname as *mut ::core::ffi::c_char,
        I_VideoBuffer,
        SCREENWIDTH,
        SCREENHEIGHT,
        W_CacheLumpName(
            b"PLAYPAL\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut byte,
    );
}
pub const MOUSE_SPEED_BOX_WIDTH: ::core::ffi::c_int = 120 as ::core::ffi::c_int;
pub const MOUSE_SPEED_BOX_HEIGHT: ::core::ffi::c_int = 9 as ::core::ffi::c_int;
#[no_mangle]
pub unsafe extern "C" fn V_DrawMouseSpeedBox(mut speed: ::core::ffi::c_int) {
    extern "C" {
        static mut usemouse: ::core::ffi::c_int;
    }
    let mut bgcolor: ::core::ffi::c_int = 0;
    let mut bordercolor: ::core::ffi::c_int = 0;
    let mut red: ::core::ffi::c_int = 0;
    let mut black: ::core::ffi::c_int = 0;
    let mut white: ::core::ffi::c_int = 0;
    let mut yellow: ::core::ffi::c_int = 0;
    let mut box_x: ::core::ffi::c_int = 0;
    let mut box_y: ::core::ffi::c_int = 0;
    let mut original_speed: ::core::ffi::c_int = 0;
    let mut redline_x: ::core::ffi::c_int = 0;
    let mut linelen: ::core::ffi::c_int = 0;
    bgcolor = I_GetPaletteIndex(
        0x77 as ::core::ffi::c_int,
        0x77 as ::core::ffi::c_int,
        0x77 as ::core::ffi::c_int,
    );
    bordercolor = I_GetPaletteIndex(
        0x55 as ::core::ffi::c_int,
        0x55 as ::core::ffi::c_int,
        0x55 as ::core::ffi::c_int,
    );
    red = I_GetPaletteIndex(
        0xff as ::core::ffi::c_int,
        0 as ::core::ffi::c_int,
        0 as ::core::ffi::c_int,
    );
    black = I_GetPaletteIndex(
        0 as ::core::ffi::c_int,
        0 as ::core::ffi::c_int,
        0 as ::core::ffi::c_int,
    );
    yellow = I_GetPaletteIndex(
        0xff as ::core::ffi::c_int,
        0xff as ::core::ffi::c_int,
        0 as ::core::ffi::c_int,
    );
    white = I_GetPaletteIndex(
        0xff as ::core::ffi::c_int,
        0xff as ::core::ffi::c_int,
        0xff as ::core::ffi::c_int,
    );
    if usemouse == 0
        || fabs(
            (mouse_acceleration - 1 as ::core::ffi::c_int as ::core::ffi::c_float)
                as ::core::ffi::c_double,
        ) < 0.01f64
    {
        return;
    }
    box_x = SCREENWIDTH - MOUSE_SPEED_BOX_WIDTH - 10 as ::core::ffi::c_int;
    box_y = 15 as ::core::ffi::c_int;
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
    redline_x = MOUSE_SPEED_BOX_WIDTH / 3 as ::core::ffi::c_int;
    if speed < mouse_threshold {
        original_speed = speed;
    } else {
        original_speed = speed - mouse_threshold;
        original_speed =
            (original_speed as ::core::ffi::c_float / mouse_acceleration) as ::core::ffi::c_int;
        original_speed += mouse_threshold;
    }
    linelen = original_speed * redline_x / mouse_threshold;
    if linelen > MOUSE_SPEED_BOX_WIDTH - 1 as ::core::ffi::c_int {
        linelen = MOUSE_SPEED_BOX_WIDTH - 1 as ::core::ffi::c_int;
    }
    V_DrawHorizLine(
        box_x + 1 as ::core::ffi::c_int,
        box_y + 4 as ::core::ffi::c_int,
        MOUSE_SPEED_BOX_WIDTH - 2 as ::core::ffi::c_int,
        black,
    );
    if linelen < redline_x {
        V_DrawHorizLine(
            box_x + 1 as ::core::ffi::c_int,
            box_y + MOUSE_SPEED_BOX_HEIGHT / 2 as ::core::ffi::c_int,
            linelen,
            white,
        );
    } else {
        V_DrawHorizLine(
            box_x + 1 as ::core::ffi::c_int,
            box_y + MOUSE_SPEED_BOX_HEIGHT / 2 as ::core::ffi::c_int,
            redline_x,
            white,
        );
        V_DrawHorizLine(
            box_x + redline_x,
            box_y + MOUSE_SPEED_BOX_HEIGHT / 2 as ::core::ffi::c_int,
            linelen - redline_x,
            yellow,
        );
    }
    V_DrawVertLine(
        box_x + redline_x,
        box_y + 1 as ::core::ffi::c_int,
        MOUSE_SPEED_BOX_HEIGHT - 2 as ::core::ffi::c_int,
        red,
    );
}
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
