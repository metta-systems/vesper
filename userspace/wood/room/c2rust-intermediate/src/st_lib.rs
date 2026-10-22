extern "C" {
    fn snprintf(
        __s: *mut ::core::ffi::c_char,
        __maxlen: size_t,
        __format: *const ::core::ffi::c_char,
        ...
    ) -> ::core::ffi::c_int;
    fn V_CopyRect(
        srcx: ::core::ffi::c_int,
        srcy: ::core::ffi::c_int,
        source: *mut byte,
        width: ::core::ffi::c_int,
        height: ::core::ffi::c_int,
        destx: ::core::ffi::c_int,
        desty: ::core::ffi::c_int,
    );
    fn V_DrawPatch(x: ::core::ffi::c_int, y: ::core::ffi::c_int, patch: *mut patch_t);
    fn I_ErrorV(msg: *const ::core::ffi::c_char);
    fn W_CacheLumpName(
        name: *mut ::core::ffi::c_char,
        tag: ::core::ffi::c_int,
    ) -> *mut ::core::ffi::c_void;
    static mut st_backing_screen: *mut byte;
}
pub type size_t = usize;
pub type __uint8_t = u8;
pub type uint8_t = __uint8_t;
pub type boolean = ::core::ffi::c_uint;
pub type byte = uint8_t;
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
#[repr(C)]
pub struct st_number_t {
    pub x: ::core::ffi::c_int,
    pub y: ::core::ffi::c_int,
    pub width: ::core::ffi::c_int,
    pub oldnum: ::core::ffi::c_int,
    pub num: *mut ::core::ffi::c_int,
    pub on: *mut boolean,
    pub p: *mut *mut patch_t,
    pub data: ::core::ffi::c_int,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct st_percent_t {
    pub n: st_number_t,
    pub p: *mut patch_t,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct st_multicon_t {
    pub x: ::core::ffi::c_int,
    pub y: ::core::ffi::c_int,
    pub oldinum: ::core::ffi::c_int,
    pub inum: *mut ::core::ffi::c_int,
    pub on: *mut boolean,
    pub p: *mut *mut patch_t,
    pub data: ::core::ffi::c_int,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct st_binicon_t {
    pub x: ::core::ffi::c_int,
    pub y: ::core::ffi::c_int,
    pub oldval: boolean,
    pub val: *mut boolean,
    pub on: *mut boolean,
    pub p: *mut patch_t,
    pub data: ::core::ffi::c_int,
}
pub const false_0: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
pub const ST_HEIGHT: ::core::ffi::c_int = 32 as ::core::ffi::c_int;
pub const ST_Y: ::core::ffi::c_int = SCREENHEIGHT - ST_HEIGHT;
pub const SCREENHEIGHT: ::core::ffi::c_int = 200 as ::core::ffi::c_int;
#[no_mangle]
pub static mut sttminus: *mut patch_t = ::core::ptr::null_mut::<patch_t>();
#[no_mangle]
pub unsafe extern "C" fn STlib_init() {
    sttminus = W_CacheLumpName(
        b"STTMINUS\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        PU_STATIC as ::core::ffi::c_int,
    ) as *mut patch_t;
}
#[no_mangle]
pub unsafe extern "C" fn STlib_initNum(
    mut n: *mut st_number_t,
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut pl: *mut *mut patch_t,
    mut num: *mut ::core::ffi::c_int,
    mut on: *mut boolean,
    mut width: ::core::ffi::c_int,
) {
    (*n).x = x;
    (*n).y = y;
    (*n).oldnum = 0 as ::core::ffi::c_int;
    (*n).width = width;
    (*n).num = num;
    (*n).on = on;
    (*n).p = pl;
}
#[no_mangle]
pub unsafe extern "C" fn STlib_drawNum(mut n: *mut st_number_t, mut refresh: boolean) {
    let mut numdigits: ::core::ffi::c_int = (*n).width;
    let mut num: ::core::ffi::c_int = *(*n).num;
    let mut w: ::core::ffi::c_int =
        (**(*n).p.offset(0 as ::core::ffi::c_int as isize)).width as ::core::ffi::c_int;
    let mut h: ::core::ffi::c_int =
        (**(*n).p.offset(0 as ::core::ffi::c_int as isize)).height as ::core::ffi::c_int;
    let mut x: ::core::ffi::c_int = (*n).x;
    let mut neg: ::core::ffi::c_int = 0;
    (*n).oldnum = *(*n).num;
    neg = (num < 0 as ::core::ffi::c_int) as ::core::ffi::c_int;
    if neg != 0 {
        if numdigits == 2 as ::core::ffi::c_int && num < -9 as ::core::ffi::c_int {
            num = -9 as ::core::ffi::c_int;
        } else if numdigits == 3 as ::core::ffi::c_int && num < -99 as ::core::ffi::c_int {
            num = -99 as ::core::ffi::c_int;
        }
        num = -num;
    }
    x = (*n).x - numdigits * w;
    if (*n).y - ST_Y < 0 as ::core::ffi::c_int {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"drawNum: n->y - ST_Y < 0\0".as_ptr() as *const ::core::ffi::c_char,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    V_CopyRect(
        x,
        (*n).y - ST_Y,
        st_backing_screen,
        w * numdigits,
        h,
        x,
        (*n).y,
    );
    if num == 1994 as ::core::ffi::c_int {
        return;
    }
    x = (*n).x;
    if num == 0 {
        V_DrawPatch(
            x - w,
            (*n).y,
            *(*n).p.offset(0 as ::core::ffi::c_int as isize),
        );
    }
    while num != 0 && {
        let c2rust_fresh0 = numdigits;
        numdigits = numdigits - 1;
        c2rust_fresh0 != 0
    } {
        x -= w;
        V_DrawPatch(
            x,
            (*n).y,
            *(*n).p.offset((num % 10 as ::core::ffi::c_int) as isize),
        );
        num /= 10 as ::core::ffi::c_int;
    }
    if neg != 0 {
        V_DrawPatch(x - 8 as ::core::ffi::c_int, (*n).y, sttminus);
    }
}
#[no_mangle]
pub unsafe extern "C" fn STlib_updateNum(mut n: *mut st_number_t, mut refresh: boolean) {
    if *(*n).on != 0 {
        STlib_drawNum(n, refresh);
    }
}
#[no_mangle]
pub unsafe extern "C" fn STlib_initPercent(
    mut p: *mut st_percent_t,
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut pl: *mut *mut patch_t,
    mut num: *mut ::core::ffi::c_int,
    mut on: *mut boolean,
    mut percent: *mut patch_t,
) {
    STlib_initNum(&raw mut (*p).n, x, y, pl, num, on, 3 as ::core::ffi::c_int);
    (*p).p = percent;
}
#[no_mangle]
pub unsafe extern "C" fn STlib_updatePercent(
    mut per: *mut st_percent_t,
    mut refresh: ::core::ffi::c_int,
) {
    if refresh != 0 && *(*per).n.on != 0 {
        V_DrawPatch((*per).n.x, (*per).n.y, (*per).p);
    }
    STlib_updateNum(&raw mut (*per).n, refresh as boolean);
}
#[no_mangle]
pub unsafe extern "C" fn STlib_initMultIcon(
    mut i: *mut st_multicon_t,
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut il: *mut *mut patch_t,
    mut inum: *mut ::core::ffi::c_int,
    mut on: *mut boolean,
) {
    (*i).x = x;
    (*i).y = y;
    (*i).oldinum = -1 as ::core::ffi::c_int;
    (*i).inum = inum;
    (*i).on = on;
    (*i).p = il;
}
#[no_mangle]
pub unsafe extern "C" fn STlib_updateMultIcon(mut mi: *mut st_multicon_t, mut refresh: boolean) {
    let mut w: ::core::ffi::c_int = 0;
    let mut h: ::core::ffi::c_int = 0;
    let mut x: ::core::ffi::c_int = 0;
    let mut y: ::core::ffi::c_int = 0;
    if *(*mi).on != 0
        && ((*mi).oldinum != *(*mi).inum || refresh != 0)
        && *(*mi).inum != -1 as ::core::ffi::c_int
    {
        if (*mi).oldinum != -1 as ::core::ffi::c_int {
            x = (*mi).x
                - (**(*mi).p.offset((*mi).oldinum as isize)).leftoffset as ::core::ffi::c_int;
            y = (*mi).y
                - (**(*mi).p.offset((*mi).oldinum as isize)).topoffset as ::core::ffi::c_int;
            w = (**(*mi).p.offset((*mi).oldinum as isize)).width as ::core::ffi::c_int;
            h = (**(*mi).p.offset((*mi).oldinum as isize)).height as ::core::ffi::c_int;
            if y - ST_Y < 0 as ::core::ffi::c_int {
                let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
                snprintf(
                    &raw mut _i_err_buf as *mut ::core::ffi::c_char,
                    ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
                    b"updateMultIcon: y - ST_Y < 0\0".as_ptr() as *const ::core::ffi::c_char,
                );
                I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
            }
            V_CopyRect(x, y - ST_Y, st_backing_screen, w, h, x, y);
        }
        V_DrawPatch((*mi).x, (*mi).y, *(*mi).p.offset(*(*mi).inum as isize));
        (*mi).oldinum = *(*mi).inum;
    }
}
#[no_mangle]
pub unsafe extern "C" fn STlib_initBinIcon(
    mut b: *mut st_binicon_t,
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut i: *mut patch_t,
    mut val: *mut boolean,
    mut on: *mut boolean,
) {
    (*b).x = x;
    (*b).y = y;
    (*b).oldval = false_0 as boolean;
    (*b).val = val;
    (*b).on = on;
    (*b).p = i;
}
#[no_mangle]
pub unsafe extern "C" fn STlib_updateBinIcon(mut bi: *mut st_binicon_t, mut refresh: boolean) {
    let mut x: ::core::ffi::c_int = 0;
    let mut y: ::core::ffi::c_int = 0;
    let mut w: ::core::ffi::c_int = 0;
    let mut h: ::core::ffi::c_int = 0;
    if *(*bi).on != 0 && ((*bi).oldval != *(*bi).val || refresh != 0) {
        x = (*bi).x - (*(*bi).p).leftoffset as ::core::ffi::c_int;
        y = (*bi).y - (*(*bi).p).topoffset as ::core::ffi::c_int;
        w = (*(*bi).p).width as ::core::ffi::c_int;
        h = (*(*bi).p).height as ::core::ffi::c_int;
        if y - ST_Y < 0 as ::core::ffi::c_int {
            let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
            snprintf(
                &raw mut _i_err_buf as *mut ::core::ffi::c_char,
                ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
                b"updateBinIcon: y - ST_Y < 0\0".as_ptr() as *const ::core::ffi::c_char,
            );
            I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
        }
        if *(*bi).val != 0 {
            V_DrawPatch((*bi).x, (*bi).y, (*bi).p);
        } else {
            V_CopyRect(x, y - ST_Y, st_backing_screen, w, h, x, y);
        }
        (*bi).oldval = *(*bi).val;
    }
}
