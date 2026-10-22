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
    fn I_ErrorV(msg: *const ::core::ffi::c_char);
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
    static mut I_VideoBuffer: *mut byte;
    static mut colormaps: *mut lighttable_t;
    static mut centery: ::core::ffi::c_int;
    fn V_DrawPatch(x: ::core::ffi::c_int, y: ::core::ffi::c_int, patch: *mut patch_t);
    fn V_MarkRect(
        x: ::core::ffi::c_int,
        y: ::core::ffi::c_int,
        width: ::core::ffi::c_int,
        height: ::core::ffi::c_int,
    );
    fn V_UseBuffer(buffer: *mut byte);
    fn V_RestoreBuffer();
    static mut gamemode: GameMode_t;
}
pub type size_t = usize;
pub type __uint8_t = u8;
pub type uint8_t = __uint8_t;
pub type byte = uint8_t;
pub type GameMode_t = ::core::ffi::c_uint;
pub const indetermined: GameMode_t = 4;
pub const retail: GameMode_t = 3;
pub const commercial: GameMode_t = 2;
pub const registered: GameMode_t = 1;
pub const shareware: GameMode_t = 0;
pub type C2Rust_Unnamed = ::core::ffi::c_uint;
pub const PU_NUM_TAGS: C2Rust_Unnamed = 9;
pub const PU_CACHE: C2Rust_Unnamed = 8;
pub const PU_PURGELEVEL: C2Rust_Unnamed = 7;
pub const PU_LEVSPEC: C2Rust_Unnamed = 6;
pub const PU_LEVEL: C2Rust_Unnamed = 5;
pub const PU_FREE: C2Rust_Unnamed = 4;
pub const PU_MUSIC: C2Rust_Unnamed = 3;
pub const PU_SOUND: C2Rust_Unnamed = 2;
pub const PU_STATIC: C2Rust_Unnamed = 1;
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
pub type lighttable_t = byte;
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
pub const FRACBITS: ::core::ffi::c_int = 16 as ::core::ffi::c_int;
pub const SCREENWIDTH: ::core::ffi::c_int = 320 as ::core::ffi::c_int;
pub const SCREENHEIGHT: ::core::ffi::c_int = 200 as ::core::ffi::c_int;
pub const SBARHEIGHT: ::core::ffi::c_int = 32 as ::core::ffi::c_int;
#[no_mangle]
pub static mut viewimage: *mut byte = ::core::ptr::null_mut::<byte>();
#[no_mangle]
pub static mut viewwidth: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut scaledviewwidth: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut viewheight: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut viewwindowx: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut viewwindowy: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut ylookup: [*mut byte; 832] = [::core::ptr::null_mut::<byte>(); 832];
#[no_mangle]
pub static mut columnofs: [::core::ffi::c_int; 1120] = [0; 1120];
#[no_mangle]
pub static mut translations: [[byte; 256]; 3] = [[0; 256]; 3];
static mut background_buffer: *mut byte = ::core::ptr::null_mut::<byte>();
#[no_mangle]
pub static mut dc_colormap: *mut lighttable_t = ::core::ptr::null_mut::<lighttable_t>();
#[no_mangle]
pub static mut dc_x: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut dc_yl: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut dc_yh: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut dc_iscale: fixed_t = 0;
#[no_mangle]
pub static mut dc_texturemid: fixed_t = 0;
#[no_mangle]
pub static mut dc_source: *mut byte = ::core::ptr::null_mut::<byte>();
#[no_mangle]
pub static mut dccount: ::core::ffi::c_int = 0;
#[no_mangle]
pub unsafe extern "C" fn R_DrawColumn() {
    let mut count: ::core::ffi::c_int = 0;
    let mut dest: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut frac: fixed_t = 0;
    let mut fracstep: fixed_t = 0;
    count = dc_yh - dc_yl;
    if count < 0 as ::core::ffi::c_int {
        return;
    }
    if dc_x as ::core::ffi::c_uint >= SCREENWIDTH as ::core::ffi::c_uint
        || dc_yl < 0 as ::core::ffi::c_int
        || dc_yh >= SCREENHEIGHT
    {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"R_DrawColumn: %i to %i at %i\0".as_ptr() as *const ::core::ffi::c_char,
            dc_yl,
            dc_yh,
            dc_x,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    dest = ylookup[dc_yl as usize].offset(columnofs[dc_x as usize] as isize);
    fracstep = dc_iscale;
    frac = dc_texturemid + (dc_yl as fixed_t - centery as fixed_t) * fracstep;
    loop {
        *dest =
            *dc_colormap.offset(*dc_source.offset(
                (frac as ::core::ffi::c_int >> FRACBITS & 127 as ::core::ffi::c_int) as isize,
            ) as isize) as byte;
        dest = dest.offset(SCREENWIDTH as isize);
        frac += fracstep;
        let c2rust_fresh0 = count;
        count = count - 1;
        if !(c2rust_fresh0 != 0) {
            break;
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn R_DrawColumnLow() {
    let mut count: ::core::ffi::c_int = 0;
    let mut dest: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut dest2: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut frac: fixed_t = 0;
    let mut fracstep: fixed_t = 0;
    let mut x: ::core::ffi::c_int = 0;
    count = dc_yh - dc_yl;
    if count < 0 as ::core::ffi::c_int {
        return;
    }
    if dc_x as ::core::ffi::c_uint >= SCREENWIDTH as ::core::ffi::c_uint
        || dc_yl < 0 as ::core::ffi::c_int
        || dc_yh >= SCREENHEIGHT
    {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"R_DrawColumn: %i to %i at %i\0".as_ptr() as *const ::core::ffi::c_char,
            dc_yl,
            dc_yh,
            dc_x,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    x = dc_x << 1 as ::core::ffi::c_int;
    dest = ylookup[dc_yl as usize].offset(columnofs[x as usize] as isize);
    dest2 =
        ylookup[dc_yl as usize].offset(columnofs[(x + 1 as ::core::ffi::c_int) as usize] as isize);
    fracstep = dc_iscale;
    frac = dc_texturemid + (dc_yl as fixed_t - centery as fixed_t) * fracstep;
    loop {
        *dest =
            *dc_colormap.offset(*dc_source.offset(
                (frac as ::core::ffi::c_int >> FRACBITS & 127 as ::core::ffi::c_int) as isize,
            ) as isize) as byte;
        *dest2 = *dest;
        dest = dest.offset(SCREENWIDTH as isize);
        dest2 = dest2.offset(SCREENWIDTH as isize);
        frac += fracstep;
        let c2rust_fresh1 = count;
        count = count - 1;
        if !(c2rust_fresh1 != 0) {
            break;
        }
    }
}
pub const FUZZTABLE: ::core::ffi::c_int = 50 as ::core::ffi::c_int;
pub const FUZZOFF: ::core::ffi::c_int = 320 as ::core::ffi::c_int;
#[no_mangle]
pub static mut fuzzoffset: [::core::ffi::c_int; 50] = [
    FUZZOFF, -FUZZOFF, FUZZOFF, -FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF,
    FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF, FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF, -FUZZOFF, -FUZZOFF,
    -FUZZOFF, FUZZOFF, -FUZZOFF, -FUZZOFF, FUZZOFF, FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF, FUZZOFF,
    -FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF, -FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF, -FUZZOFF, -FUZZOFF,
    -FUZZOFF, FUZZOFF, FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF, FUZZOFF, FUZZOFF, -FUZZOFF, FUZZOFF,
];
#[no_mangle]
pub static mut fuzzpos: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
#[no_mangle]
pub unsafe extern "C" fn R_DrawFuzzColumn() {
    let mut count: ::core::ffi::c_int = 0;
    let mut dest: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut frac: fixed_t = 0;
    let mut fracstep: fixed_t = 0;
    if dc_yl == 0 {
        dc_yl = 1 as ::core::ffi::c_int;
    }
    if dc_yh == viewheight - 1 as ::core::ffi::c_int {
        dc_yh = viewheight - 2 as ::core::ffi::c_int;
    }
    count = dc_yh - dc_yl;
    if count < 0 as ::core::ffi::c_int {
        return;
    }
    if dc_x as ::core::ffi::c_uint >= SCREENWIDTH as ::core::ffi::c_uint
        || dc_yl < 0 as ::core::ffi::c_int
        || dc_yh >= SCREENHEIGHT
    {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"R_DrawFuzzColumn: %i to %i at %i\0".as_ptr() as *const ::core::ffi::c_char,
            dc_yl,
            dc_yh,
            dc_x,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    dest = ylookup[dc_yl as usize].offset(columnofs[dc_x as usize] as isize);
    fracstep = dc_iscale;
    frac = dc_texturemid + (dc_yl as fixed_t - centery as fixed_t) * fracstep;
    loop {
        *dest = *colormaps.offset(
            (6 as ::core::ffi::c_int * 256 as ::core::ffi::c_int
                + *dest.offset(fuzzoffset[fuzzpos as usize] as isize) as ::core::ffi::c_int)
                as isize,
        ) as byte;
        fuzzpos += 1;
        if fuzzpos == FUZZTABLE {
            fuzzpos = 0 as ::core::ffi::c_int;
        }
        dest = dest.offset(SCREENWIDTH as isize);
        frac += fracstep;
        let c2rust_fresh2 = count;
        count = count - 1;
        if !(c2rust_fresh2 != 0) {
            break;
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn R_DrawFuzzColumnLow() {
    let mut count: ::core::ffi::c_int = 0;
    let mut dest: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut dest2: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut frac: fixed_t = 0;
    let mut fracstep: fixed_t = 0;
    let mut x: ::core::ffi::c_int = 0;
    if dc_yl == 0 {
        dc_yl = 1 as ::core::ffi::c_int;
    }
    if dc_yh == viewheight - 1 as ::core::ffi::c_int {
        dc_yh = viewheight - 2 as ::core::ffi::c_int;
    }
    count = dc_yh - dc_yl;
    if count < 0 as ::core::ffi::c_int {
        return;
    }
    x = dc_x << 1 as ::core::ffi::c_int;
    if x as ::core::ffi::c_uint >= SCREENWIDTH as ::core::ffi::c_uint
        || dc_yl < 0 as ::core::ffi::c_int
        || dc_yh >= SCREENHEIGHT
    {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"R_DrawFuzzColumn: %i to %i at %i\0".as_ptr() as *const ::core::ffi::c_char,
            dc_yl,
            dc_yh,
            dc_x,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    dest = ylookup[dc_yl as usize].offset(columnofs[x as usize] as isize);
    dest2 =
        ylookup[dc_yl as usize].offset(columnofs[(x + 1 as ::core::ffi::c_int) as usize] as isize);
    fracstep = dc_iscale;
    frac = dc_texturemid + (dc_yl as fixed_t - centery as fixed_t) * fracstep;
    loop {
        *dest = *colormaps.offset(
            (6 as ::core::ffi::c_int * 256 as ::core::ffi::c_int
                + *dest.offset(fuzzoffset[fuzzpos as usize] as isize) as ::core::ffi::c_int)
                as isize,
        ) as byte;
        *dest2 = *colormaps.offset(
            (6 as ::core::ffi::c_int * 256 as ::core::ffi::c_int
                + *dest2.offset(fuzzoffset[fuzzpos as usize] as isize) as ::core::ffi::c_int)
                as isize,
        ) as byte;
        fuzzpos += 1;
        if fuzzpos == FUZZTABLE {
            fuzzpos = 0 as ::core::ffi::c_int;
        }
        dest = dest.offset(SCREENWIDTH as isize);
        dest2 = dest2.offset(SCREENWIDTH as isize);
        frac += fracstep;
        let c2rust_fresh3 = count;
        count = count - 1;
        if !(c2rust_fresh3 != 0) {
            break;
        }
    }
}
#[no_mangle]
pub static mut dc_translation: *mut byte = ::core::ptr::null_mut::<byte>();
#[no_mangle]
pub static mut translationtables: *mut byte = ::core::ptr::null_mut::<byte>();
#[no_mangle]
pub unsafe extern "C" fn R_DrawTranslatedColumn() {
    let mut count: ::core::ffi::c_int = 0;
    let mut dest: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut frac: fixed_t = 0;
    let mut fracstep: fixed_t = 0;
    count = dc_yh - dc_yl;
    if count < 0 as ::core::ffi::c_int {
        return;
    }
    if dc_x as ::core::ffi::c_uint >= SCREENWIDTH as ::core::ffi::c_uint
        || dc_yl < 0 as ::core::ffi::c_int
        || dc_yh >= SCREENHEIGHT
    {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"R_DrawColumn: %i to %i at %i\0".as_ptr() as *const ::core::ffi::c_char,
            dc_yl,
            dc_yh,
            dc_x,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    dest = ylookup[dc_yl as usize].offset(columnofs[dc_x as usize] as isize);
    fracstep = dc_iscale;
    frac = dc_texturemid + (dc_yl as fixed_t - centery as fixed_t) * fracstep;
    loop {
        *dest = *dc_colormap.offset(
            *dc_translation.offset(*dc_source.offset((frac >> FRACBITS) as isize) as isize)
                as isize,
        ) as byte;
        dest = dest.offset(SCREENWIDTH as isize);
        frac += fracstep;
        let c2rust_fresh4 = count;
        count = count - 1;
        if !(c2rust_fresh4 != 0) {
            break;
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn R_DrawTranslatedColumnLow() {
    let mut count: ::core::ffi::c_int = 0;
    let mut dest: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut dest2: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut frac: fixed_t = 0;
    let mut fracstep: fixed_t = 0;
    let mut x: ::core::ffi::c_int = 0;
    count = dc_yh - dc_yl;
    if count < 0 as ::core::ffi::c_int {
        return;
    }
    x = dc_x << 1 as ::core::ffi::c_int;
    if x as ::core::ffi::c_uint >= SCREENWIDTH as ::core::ffi::c_uint
        || dc_yl < 0 as ::core::ffi::c_int
        || dc_yh >= SCREENHEIGHT
    {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"R_DrawColumn: %i to %i at %i\0".as_ptr() as *const ::core::ffi::c_char,
            dc_yl,
            dc_yh,
            x,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    dest = ylookup[dc_yl as usize].offset(columnofs[x as usize] as isize);
    dest2 =
        ylookup[dc_yl as usize].offset(columnofs[(x + 1 as ::core::ffi::c_int) as usize] as isize);
    fracstep = dc_iscale;
    frac = dc_texturemid + (dc_yl as fixed_t - centery as fixed_t) * fracstep;
    loop {
        *dest = *dc_colormap.offset(
            *dc_translation.offset(*dc_source.offset((frac >> FRACBITS) as isize) as isize)
                as isize,
        ) as byte;
        *dest2 = *dc_colormap.offset(
            *dc_translation.offset(*dc_source.offset((frac >> FRACBITS) as isize) as isize)
                as isize,
        ) as byte;
        dest = dest.offset(SCREENWIDTH as isize);
        dest2 = dest2.offset(SCREENWIDTH as isize);
        frac += fracstep;
        let c2rust_fresh5 = count;
        count = count - 1;
        if !(c2rust_fresh5 != 0) {
            break;
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn R_InitTranslationTables() {
    let mut i: ::core::ffi::c_int = 0;
    translationtables = Z_Malloc(
        256 as ::core::ffi::c_int * 3 as ::core::ffi::c_int,
        PU_STATIC as ::core::ffi::c_int,
        ::core::ptr::null_mut::<::core::ffi::c_void>(),
    ) as *mut byte;
    i = 0 as ::core::ffi::c_int;
    while i < 256 as ::core::ffi::c_int {
        if i >= 0x70 as ::core::ffi::c_int && i <= 0x7f as ::core::ffi::c_int {
            *translationtables.offset(i as isize) =
                (0x60 as ::core::ffi::c_int + (i & 0xf as ::core::ffi::c_int)) as byte;
            *translationtables.offset((i + 256 as ::core::ffi::c_int) as isize) =
                (0x40 as ::core::ffi::c_int + (i & 0xf as ::core::ffi::c_int)) as byte;
            *translationtables.offset((i + 512 as ::core::ffi::c_int) as isize) =
                (0x20 as ::core::ffi::c_int + (i & 0xf as ::core::ffi::c_int)) as byte;
        } else {
            let ref mut c2rust_fresh11 =
                *translationtables.offset((i + 512 as ::core::ffi::c_int) as isize);
            *c2rust_fresh11 = i as byte;
            let ref mut c2rust_fresh12 =
                *translationtables.offset((i + 256 as ::core::ffi::c_int) as isize);
            *c2rust_fresh12 = *c2rust_fresh11;
            *translationtables.offset(i as isize) = *c2rust_fresh12;
        }
        i += 1;
    }
}
#[no_mangle]
pub static mut ds_y: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut ds_x1: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut ds_x2: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut ds_colormap: *mut lighttable_t = ::core::ptr::null_mut::<lighttable_t>();
#[no_mangle]
pub static mut ds_xfrac: fixed_t = 0;
#[no_mangle]
pub static mut ds_yfrac: fixed_t = 0;
#[no_mangle]
pub static mut ds_xstep: fixed_t = 0;
#[no_mangle]
pub static mut ds_ystep: fixed_t = 0;
#[no_mangle]
pub static mut ds_source: *mut byte = ::core::ptr::null_mut::<byte>();
#[no_mangle]
pub static mut dscount: ::core::ffi::c_int = 0;
#[no_mangle]
pub unsafe extern "C" fn R_DrawSpan() {
    let mut position: ::core::ffi::c_uint = 0;
    let mut step: ::core::ffi::c_uint = 0;
    let mut dest: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut count: ::core::ffi::c_int = 0;
    let mut spot: ::core::ffi::c_int = 0;
    let mut xtemp: ::core::ffi::c_uint = 0;
    let mut ytemp: ::core::ffi::c_uint = 0;
    if ds_x2 < ds_x1
        || ds_x1 < 0 as ::core::ffi::c_int
        || ds_x2 >= SCREENWIDTH
        || ds_y as ::core::ffi::c_uint > SCREENHEIGHT as ::core::ffi::c_uint
    {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"R_DrawSpan: %i to %i at %i\0".as_ptr() as *const ::core::ffi::c_char,
            ds_x1,
            ds_x2,
            ds_y,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    position = (ds_xfrac << 10 as ::core::ffi::c_int) as ::core::ffi::c_uint
        & 0xffff0000 as ::core::ffi::c_uint
        | (ds_yfrac as ::core::ffi::c_int >> 6 as ::core::ffi::c_int & 0xffff as ::core::ffi::c_int)
            as ::core::ffi::c_uint;
    step = (ds_xstep << 10 as ::core::ffi::c_int) as ::core::ffi::c_uint
        & 0xffff0000 as ::core::ffi::c_uint
        | (ds_ystep as ::core::ffi::c_int >> 6 as ::core::ffi::c_int & 0xffff as ::core::ffi::c_int)
            as ::core::ffi::c_uint;
    dest = ylookup[ds_y as usize].offset(columnofs[ds_x1 as usize] as isize);
    count = ds_x2 - ds_x1;
    loop {
        ytemp = position >> 4 as ::core::ffi::c_int & 0xfc0 as ::core::ffi::c_uint;
        xtemp = position >> 26 as ::core::ffi::c_int;
        spot = (xtemp | ytemp) as ::core::ffi::c_int;
        let c2rust_fresh6 = dest;
        dest = dest.offset(1);
        *c2rust_fresh6 = *ds_colormap.offset(*ds_source.offset(spot as isize) as isize) as byte;
        position = position.wrapping_add(step);
        let c2rust_fresh7 = count;
        count = count - 1;
        if !(c2rust_fresh7 != 0) {
            break;
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn R_DrawSpanLow() {
    let mut position: ::core::ffi::c_uint = 0;
    let mut step: ::core::ffi::c_uint = 0;
    let mut xtemp: ::core::ffi::c_uint = 0;
    let mut ytemp: ::core::ffi::c_uint = 0;
    let mut dest: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut count: ::core::ffi::c_int = 0;
    let mut spot: ::core::ffi::c_int = 0;
    if ds_x2 < ds_x1
        || ds_x1 < 0 as ::core::ffi::c_int
        || ds_x2 >= SCREENWIDTH
        || ds_y as ::core::ffi::c_uint > SCREENHEIGHT as ::core::ffi::c_uint
    {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"R_DrawSpan: %i to %i at %i\0".as_ptr() as *const ::core::ffi::c_char,
            ds_x1,
            ds_x2,
            ds_y,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    position = (ds_xfrac << 10 as ::core::ffi::c_int) as ::core::ffi::c_uint
        & 0xffff0000 as ::core::ffi::c_uint
        | (ds_yfrac as ::core::ffi::c_int >> 6 as ::core::ffi::c_int & 0xffff as ::core::ffi::c_int)
            as ::core::ffi::c_uint;
    step = (ds_xstep << 10 as ::core::ffi::c_int) as ::core::ffi::c_uint
        & 0xffff0000 as ::core::ffi::c_uint
        | (ds_ystep as ::core::ffi::c_int >> 6 as ::core::ffi::c_int & 0xffff as ::core::ffi::c_int)
            as ::core::ffi::c_uint;
    count = ds_x2 - ds_x1;
    ds_x1 <<= 1 as ::core::ffi::c_int;
    ds_x2 <<= 1 as ::core::ffi::c_int;
    dest = ylookup[ds_y as usize].offset(columnofs[ds_x1 as usize] as isize);
    loop {
        ytemp = position >> 4 as ::core::ffi::c_int & 0xfc0 as ::core::ffi::c_uint;
        xtemp = position >> 26 as ::core::ffi::c_int;
        spot = (xtemp | ytemp) as ::core::ffi::c_int;
        let c2rust_fresh8 = dest;
        dest = dest.offset(1);
        *c2rust_fresh8 = *ds_colormap.offset(*ds_source.offset(spot as isize) as isize) as byte;
        let c2rust_fresh9 = dest;
        dest = dest.offset(1);
        *c2rust_fresh9 = *ds_colormap.offset(*ds_source.offset(spot as isize) as isize) as byte;
        position = position.wrapping_add(step);
        let c2rust_fresh10 = count;
        count = count - 1;
        if !(c2rust_fresh10 != 0) {
            break;
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn R_InitBuffer(
    mut width: ::core::ffi::c_int,
    mut height: ::core::ffi::c_int,
) {
    let mut i: ::core::ffi::c_int = 0;
    viewwindowx = SCREENWIDTH - width >> 1 as ::core::ffi::c_int;
    i = 0 as ::core::ffi::c_int;
    while i < width {
        columnofs[i as usize] = viewwindowx + i;
        i += 1;
    }
    if width == SCREENWIDTH {
        viewwindowy = 0 as ::core::ffi::c_int;
    } else {
        viewwindowy = SCREENHEIGHT - SBARHEIGHT - height >> 1 as ::core::ffi::c_int;
    }
    i = 0 as ::core::ffi::c_int;
    while i < height {
        ylookup[i as usize] = I_VideoBuffer.offset(((i + viewwindowy) * SCREENWIDTH) as isize);
        i += 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn R_FillBackScreen() {
    let mut src: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut dest: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut x: ::core::ffi::c_int = 0;
    let mut y: ::core::ffi::c_int = 0;
    let mut patch: *mut patch_t = ::core::ptr::null_mut::<patch_t>();
    let mut name1: *mut ::core::ffi::c_char =
        b"FLOOR7_2\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
    let mut name2: *mut ::core::ffi::c_char =
        b"GRNROCK\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
    let mut name: *mut ::core::ffi::c_char = ::core::ptr::null_mut::<::core::ffi::c_char>();
    if scaledviewwidth == SCREENWIDTH {
        if !background_buffer.is_null() {
            Z_Free(background_buffer as *mut ::core::ffi::c_void);
            background_buffer = ::core::ptr::null_mut::<byte>();
        }
        return;
    }
    if background_buffer.is_null() {
        background_buffer = Z_Malloc(
            SCREENWIDTH * (SCREENHEIGHT - SBARHEIGHT),
            PU_STATIC as ::core::ffi::c_int,
            NULL,
        ) as *mut byte;
    }
    if gamemode as ::core::ffi::c_uint == commercial as ::core::ffi::c_int as ::core::ffi::c_uint {
        name = name2;
    } else {
        name = name1;
    }
    src = W_CacheLumpName(name, PU_CACHE as ::core::ffi::c_int) as *mut byte;
    dest = background_buffer;
    y = 0 as ::core::ffi::c_int;
    while y < SCREENHEIGHT - SBARHEIGHT {
        x = 0 as ::core::ffi::c_int;
        while x < SCREENWIDTH / 64 as ::core::ffi::c_int {
            memcpy(
                dest as *mut ::core::ffi::c_void,
                src.offset(((y & 63 as ::core::ffi::c_int) << 6 as ::core::ffi::c_int) as isize)
                    as *const ::core::ffi::c_void,
                64 as size_t,
            );
            dest = dest.offset(64 as ::core::ffi::c_int as isize);
            x += 1;
        }
        if SCREENWIDTH & 63 as ::core::ffi::c_int != 0 {
            memcpy(
                dest as *mut ::core::ffi::c_void,
                src.offset(((y & 63 as ::core::ffi::c_int) << 6 as ::core::ffi::c_int) as isize)
                    as *const ::core::ffi::c_void,
                (SCREENWIDTH & 63 as ::core::ffi::c_int) as size_t,
            );
            dest = dest.offset((SCREENWIDTH & 63 as ::core::ffi::c_int) as isize);
        }
        y += 1;
    }
    V_UseBuffer(background_buffer);
    patch = W_CacheLumpName(
        b"brdr_t\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        PU_CACHE as ::core::ffi::c_int,
    ) as *mut patch_t;
    x = 0 as ::core::ffi::c_int;
    while x < scaledviewwidth {
        V_DrawPatch(
            viewwindowx + x,
            viewwindowy - 8 as ::core::ffi::c_int,
            patch,
        );
        x += 8 as ::core::ffi::c_int;
    }
    patch = W_CacheLumpName(
        b"brdr_b\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        PU_CACHE as ::core::ffi::c_int,
    ) as *mut patch_t;
    x = 0 as ::core::ffi::c_int;
    while x < scaledviewwidth {
        V_DrawPatch(viewwindowx + x, viewwindowy + viewheight, patch);
        x += 8 as ::core::ffi::c_int;
    }
    patch = W_CacheLumpName(
        b"brdr_l\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        PU_CACHE as ::core::ffi::c_int,
    ) as *mut patch_t;
    y = 0 as ::core::ffi::c_int;
    while y < viewheight {
        V_DrawPatch(
            viewwindowx - 8 as ::core::ffi::c_int,
            viewwindowy + y,
            patch,
        );
        y += 8 as ::core::ffi::c_int;
    }
    patch = W_CacheLumpName(
        b"brdr_r\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        PU_CACHE as ::core::ffi::c_int,
    ) as *mut patch_t;
    y = 0 as ::core::ffi::c_int;
    while y < viewheight {
        V_DrawPatch(viewwindowx + scaledviewwidth, viewwindowy + y, patch);
        y += 8 as ::core::ffi::c_int;
    }
    V_DrawPatch(
        viewwindowx - 8 as ::core::ffi::c_int,
        viewwindowy - 8 as ::core::ffi::c_int,
        W_CacheLumpName(
            b"brdr_tl\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
    V_DrawPatch(
        viewwindowx + scaledviewwidth,
        viewwindowy - 8 as ::core::ffi::c_int,
        W_CacheLumpName(
            b"brdr_tr\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
    V_DrawPatch(
        viewwindowx - 8 as ::core::ffi::c_int,
        viewwindowy + viewheight,
        W_CacheLumpName(
            b"brdr_bl\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
    V_DrawPatch(
        viewwindowx + scaledviewwidth,
        viewwindowy + viewheight,
        W_CacheLumpName(
            b"brdr_br\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
    V_RestoreBuffer();
}
#[no_mangle]
pub unsafe extern "C" fn R_VideoErase(mut ofs: ::core::ffi::c_uint, mut count: ::core::ffi::c_int) {
    if !background_buffer.is_null() {
        memcpy(
            I_VideoBuffer.offset(ofs as isize) as *mut ::core::ffi::c_void,
            background_buffer.offset(ofs as isize) as *const ::core::ffi::c_void,
            count as size_t,
        );
    }
}
#[no_mangle]
pub unsafe extern "C" fn R_DrawViewBorder() {
    let mut top: ::core::ffi::c_int = 0;
    let mut side: ::core::ffi::c_int = 0;
    let mut ofs: ::core::ffi::c_int = 0;
    let mut i: ::core::ffi::c_int = 0;
    if scaledviewwidth == SCREENWIDTH {
        return;
    }
    top = (SCREENHEIGHT - SBARHEIGHT - viewheight) / 2 as ::core::ffi::c_int;
    side = (SCREENWIDTH - scaledviewwidth) / 2 as ::core::ffi::c_int;
    R_VideoErase(0 as ::core::ffi::c_uint, top * SCREENWIDTH + side);
    ofs = (viewheight + top) * SCREENWIDTH - side;
    R_VideoErase(ofs as ::core::ffi::c_uint, top * SCREENWIDTH + side);
    ofs = top * SCREENWIDTH + SCREENWIDTH - side;
    side <<= 1 as ::core::ffi::c_int;
    i = 1 as ::core::ffi::c_int;
    while i < viewheight {
        R_VideoErase(ofs as ::core::ffi::c_uint, side);
        ofs += SCREENWIDTH;
        i += 1;
    }
    V_MarkRect(
        0 as ::core::ffi::c_int,
        0 as ::core::ffi::c_int,
        SCREENWIDTH,
        SCREENHEIGHT - SBARHEIGHT,
    );
}
