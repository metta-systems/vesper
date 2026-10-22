extern "C" {
    pub type _IO_wide_data;
    pub type _IO_codecvt;
    pub type _IO_marker;
    static mut stdout: *mut FILE;
    fn fflush(__stream: *mut FILE) -> ::core::ffi::c_int;
    fn printf(__format: *const ::core::ffi::c_char, ...) -> ::core::ffi::c_int;
    fn puts(__s: *const ::core::ffi::c_char) -> ::core::ffi::c_int;
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
    fn M_CheckParm(check: *mut ::core::ffi::c_char) -> ::core::ffi::c_int;
    fn Z_Malloc(
        size: ::core::ffi::c_int,
        tag: ::core::ffi::c_int,
        ptr: *mut ::core::ffi::c_void,
    ) -> *mut ::core::ffi::c_void;
    fn Z_Free(ptr: *mut ::core::ffi::c_void);
}
pub type size_t = usize;
pub type __uint8_t = u8;
pub type __off_t = ::core::ffi::c_long;
pub type __off64_t = ::core::ffi::c_long;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct _IO_FILE {
    pub _flags: ::core::ffi::c_int,
    pub _IO_read_ptr: *mut ::core::ffi::c_char,
    pub _IO_read_end: *mut ::core::ffi::c_char,
    pub _IO_read_base: *mut ::core::ffi::c_char,
    pub _IO_write_base: *mut ::core::ffi::c_char,
    pub _IO_write_ptr: *mut ::core::ffi::c_char,
    pub _IO_write_end: *mut ::core::ffi::c_char,
    pub _IO_buf_base: *mut ::core::ffi::c_char,
    pub _IO_buf_end: *mut ::core::ffi::c_char,
    pub _IO_save_base: *mut ::core::ffi::c_char,
    pub _IO_backup_base: *mut ::core::ffi::c_char,
    pub _IO_save_end: *mut ::core::ffi::c_char,
    pub _markers: *mut _IO_marker,
    pub _chain: *mut _IO_FILE,
    pub _fileno: ::core::ffi::c_int,
    pub _flags2: ::core::ffi::c_int,
    pub _old_offset: __off_t,
    pub _cur_column: ::core::ffi::c_ushort,
    pub _vtable_offset: ::core::ffi::c_schar,
    pub _shortbuf: [::core::ffi::c_char; 1],
    pub _lock: *mut ::core::ffi::c_void,
    pub _offset: __off64_t,
    pub _codecvt: *mut _IO_codecvt,
    pub _wide_data: *mut _IO_wide_data,
    pub _freeres_list: *mut _IO_FILE,
    pub _freeres_buf: *mut ::core::ffi::c_void,
    pub __pad5: size_t,
    pub _mode: ::core::ffi::c_int,
    pub _unused2: [::core::ffi::c_char; 20],
}
pub type _IO_lock_t = ();
pub type FILE = _IO_FILE;
pub type uint8_t = __uint8_t;
pub type boolean = ::core::ffi::c_uint;
pub type byte = uint8_t;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct screen_mode_t {
    pub width: ::core::ffi::c_int,
    pub height: ::core::ffi::c_int,
    pub InitMode: Option<unsafe extern "C" fn(*mut byte) -> ()>,
    pub DrawScreen: Option<
        unsafe extern "C" fn(
            ::core::ffi::c_int,
            ::core::ffi::c_int,
            ::core::ffi::c_int,
            ::core::ffi::c_int,
        ) -> boolean,
    >,
    pub poor_quality: boolean,
}
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
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
pub const true_0: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
pub const false_0: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
pub const INT_MAX: ::core::ffi::c_int = __INT_MAX__;
pub const SCREENWIDTH: ::core::ffi::c_int = 320 as ::core::ffi::c_int;
pub const SCREENHEIGHT: ::core::ffi::c_int = 200 as ::core::ffi::c_int;
pub const SCREENWIDTH_4_3: ::core::ffi::c_int = 256 as ::core::ffi::c_int;
pub const SCREENHEIGHT_4_3: ::core::ffi::c_int = 240 as ::core::ffi::c_int;
static mut src_buffer: *mut byte = ::core::ptr::null_mut::<byte>();
static mut dest_buffer: *mut byte = ::core::ptr::null_mut::<byte>();
static mut dest_pitch: ::core::ffi::c_int = 0;
static mut stretch_tables: [*mut byte; 2] = [
    ::core::ptr::null_mut::<byte>(),
    ::core::ptr::null_mut::<byte>(),
];
static mut half_stretch_table: *mut byte = ::core::ptr::null_mut::<byte>();
#[no_mangle]
pub unsafe extern "C" fn I_InitScale(
    mut _src_buffer: *mut byte,
    mut _dest_buffer: *mut byte,
    mut _dest_pitch: ::core::ffi::c_int,
) {
    src_buffer = _src_buffer;
    dest_buffer = _dest_buffer;
    dest_pitch = _dest_pitch;
}
unsafe extern "C" fn I_Scale1x(
    mut x1: ::core::ffi::c_int,
    mut y1: ::core::ffi::c_int,
    mut x2: ::core::ffi::c_int,
    mut y2: ::core::ffi::c_int,
) -> boolean {
    let mut bufp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut y: ::core::ffi::c_int = 0;
    let mut w: ::core::ffi::c_int = x2 - x1;
    bufp = src_buffer
        .offset((y1 * SCREENWIDTH) as isize)
        .offset(x1 as isize);
    screenp = dest_buffer
        .offset((y1 * dest_pitch) as isize)
        .offset(x1 as isize);
    y = y1;
    while y < y2 {
        memcpy(
            screenp as *mut ::core::ffi::c_void,
            bufp as *const ::core::ffi::c_void,
            w as size_t,
        );
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        y += 1;
    }
    return true_0 as boolean;
}
#[no_mangle]
pub static mut mode_scale_1x: screen_mode_t = screen_mode_t {
    width: SCREENWIDTH,
    height: SCREENHEIGHT,
    InitMode: None,
    DrawScreen: Some(
        I_Scale1x
            as unsafe extern "C" fn(
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
            ) -> boolean,
    ),
    poor_quality: false_0 as boolean,
};
unsafe extern "C" fn I_Scale2x(
    mut x1: ::core::ffi::c_int,
    mut y1: ::core::ffi::c_int,
    mut x2: ::core::ffi::c_int,
    mut y2: ::core::ffi::c_int,
) -> boolean {
    let mut bufp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp2: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut x: ::core::ffi::c_int = 0;
    let mut y: ::core::ffi::c_int = 0;
    let mut multi_pitch: ::core::ffi::c_int = 0;
    multi_pitch = dest_pitch * 2 as ::core::ffi::c_int;
    bufp = src_buffer
        .offset((y1 * SCREENWIDTH) as isize)
        .offset(x1 as isize);
    screenp = dest_buffer.offset(((y1 * dest_pitch + x1) * 2 as ::core::ffi::c_int) as isize);
    screenp2 = screenp.offset(dest_pitch as isize);
    y = y1;
    while y < y2 {
        let mut sp: *mut byte = ::core::ptr::null_mut::<byte>();
        let mut sp2: *mut byte = ::core::ptr::null_mut::<byte>();
        let mut bp: *mut byte = ::core::ptr::null_mut::<byte>();
        sp = screenp;
        sp2 = screenp2;
        bp = bufp;
        x = x1;
        while x < x2 {
            let c2rust_fresh0 = sp;
            sp = sp.offset(1);
            *c2rust_fresh0 = *bp;
            let c2rust_fresh1 = sp;
            sp = sp.offset(1);
            *c2rust_fresh1 = *bp;
            let c2rust_fresh2 = sp2;
            sp2 = sp2.offset(1);
            *c2rust_fresh2 = *bp;
            let c2rust_fresh3 = sp2;
            sp2 = sp2.offset(1);
            *c2rust_fresh3 = *bp;
            bp = bp.offset(1);
            x += 1;
        }
        screenp = screenp.offset(multi_pitch as isize);
        screenp2 = screenp2.offset(multi_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        y += 1;
    }
    return true_0 as boolean;
}
#[no_mangle]
pub static mut mode_scale_2x: screen_mode_t = screen_mode_t {
    width: SCREENWIDTH * 2 as ::core::ffi::c_int,
    height: SCREENHEIGHT * 2 as ::core::ffi::c_int,
    InitMode: None,
    DrawScreen: Some(
        I_Scale2x
            as unsafe extern "C" fn(
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
            ) -> boolean,
    ),
    poor_quality: false_0 as boolean,
};
unsafe extern "C" fn I_Scale3x(
    mut x1: ::core::ffi::c_int,
    mut y1: ::core::ffi::c_int,
    mut x2: ::core::ffi::c_int,
    mut y2: ::core::ffi::c_int,
) -> boolean {
    let mut bufp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp2: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp3: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut x: ::core::ffi::c_int = 0;
    let mut y: ::core::ffi::c_int = 0;
    let mut multi_pitch: ::core::ffi::c_int = 0;
    multi_pitch = dest_pitch * 3 as ::core::ffi::c_int;
    bufp = src_buffer
        .offset((y1 * SCREENWIDTH) as isize)
        .offset(x1 as isize);
    screenp = dest_buffer.offset(((y1 * dest_pitch + x1) * 3 as ::core::ffi::c_int) as isize);
    screenp2 = screenp.offset(dest_pitch as isize);
    screenp3 = screenp.offset((dest_pitch * 2 as ::core::ffi::c_int) as isize);
    y = y1;
    while y < y2 {
        let mut sp: *mut byte = ::core::ptr::null_mut::<byte>();
        let mut sp2: *mut byte = ::core::ptr::null_mut::<byte>();
        let mut sp3: *mut byte = ::core::ptr::null_mut::<byte>();
        let mut bp: *mut byte = ::core::ptr::null_mut::<byte>();
        sp = screenp;
        sp2 = screenp2;
        sp3 = screenp3;
        bp = bufp;
        x = x1;
        while x < x2 {
            let c2rust_fresh4 = sp;
            sp = sp.offset(1);
            *c2rust_fresh4 = *bp;
            let c2rust_fresh5 = sp;
            sp = sp.offset(1);
            *c2rust_fresh5 = *bp;
            let c2rust_fresh6 = sp;
            sp = sp.offset(1);
            *c2rust_fresh6 = *bp;
            let c2rust_fresh7 = sp2;
            sp2 = sp2.offset(1);
            *c2rust_fresh7 = *bp;
            let c2rust_fresh8 = sp2;
            sp2 = sp2.offset(1);
            *c2rust_fresh8 = *bp;
            let c2rust_fresh9 = sp2;
            sp2 = sp2.offset(1);
            *c2rust_fresh9 = *bp;
            let c2rust_fresh10 = sp3;
            sp3 = sp3.offset(1);
            *c2rust_fresh10 = *bp;
            let c2rust_fresh11 = sp3;
            sp3 = sp3.offset(1);
            *c2rust_fresh11 = *bp;
            let c2rust_fresh12 = sp3;
            sp3 = sp3.offset(1);
            *c2rust_fresh12 = *bp;
            bp = bp.offset(1);
            x += 1;
        }
        screenp = screenp.offset(multi_pitch as isize);
        screenp2 = screenp2.offset(multi_pitch as isize);
        screenp3 = screenp3.offset(multi_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        y += 1;
    }
    return true_0 as boolean;
}
#[no_mangle]
pub static mut mode_scale_3x: screen_mode_t = screen_mode_t {
    width: SCREENWIDTH * 3 as ::core::ffi::c_int,
    height: SCREENHEIGHT * 3 as ::core::ffi::c_int,
    InitMode: None,
    DrawScreen: Some(
        I_Scale3x
            as unsafe extern "C" fn(
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
            ) -> boolean,
    ),
    poor_quality: false_0 as boolean,
};
unsafe extern "C" fn I_Scale4x(
    mut x1: ::core::ffi::c_int,
    mut y1: ::core::ffi::c_int,
    mut x2: ::core::ffi::c_int,
    mut y2: ::core::ffi::c_int,
) -> boolean {
    let mut bufp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp2: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp3: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp4: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut x: ::core::ffi::c_int = 0;
    let mut y: ::core::ffi::c_int = 0;
    let mut multi_pitch: ::core::ffi::c_int = 0;
    multi_pitch = dest_pitch * 4 as ::core::ffi::c_int;
    bufp = src_buffer
        .offset((y1 * SCREENWIDTH) as isize)
        .offset(x1 as isize);
    screenp = dest_buffer.offset(((y1 * dest_pitch + x1) * 4 as ::core::ffi::c_int) as isize);
    screenp2 = screenp.offset(dest_pitch as isize);
    screenp3 = screenp.offset((dest_pitch * 2 as ::core::ffi::c_int) as isize);
    screenp4 = screenp.offset((dest_pitch * 3 as ::core::ffi::c_int) as isize);
    y = y1;
    while y < y2 {
        let mut sp: *mut byte = ::core::ptr::null_mut::<byte>();
        let mut sp2: *mut byte = ::core::ptr::null_mut::<byte>();
        let mut sp3: *mut byte = ::core::ptr::null_mut::<byte>();
        let mut sp4: *mut byte = ::core::ptr::null_mut::<byte>();
        let mut bp: *mut byte = ::core::ptr::null_mut::<byte>();
        sp = screenp;
        sp2 = screenp2;
        sp3 = screenp3;
        sp4 = screenp4;
        bp = bufp;
        x = x1;
        while x < x2 {
            let c2rust_fresh13 = sp;
            sp = sp.offset(1);
            *c2rust_fresh13 = *bp;
            let c2rust_fresh14 = sp;
            sp = sp.offset(1);
            *c2rust_fresh14 = *bp;
            let c2rust_fresh15 = sp;
            sp = sp.offset(1);
            *c2rust_fresh15 = *bp;
            let c2rust_fresh16 = sp;
            sp = sp.offset(1);
            *c2rust_fresh16 = *bp;
            let c2rust_fresh17 = sp2;
            sp2 = sp2.offset(1);
            *c2rust_fresh17 = *bp;
            let c2rust_fresh18 = sp2;
            sp2 = sp2.offset(1);
            *c2rust_fresh18 = *bp;
            let c2rust_fresh19 = sp2;
            sp2 = sp2.offset(1);
            *c2rust_fresh19 = *bp;
            let c2rust_fresh20 = sp2;
            sp2 = sp2.offset(1);
            *c2rust_fresh20 = *bp;
            let c2rust_fresh21 = sp3;
            sp3 = sp3.offset(1);
            *c2rust_fresh21 = *bp;
            let c2rust_fresh22 = sp3;
            sp3 = sp3.offset(1);
            *c2rust_fresh22 = *bp;
            let c2rust_fresh23 = sp3;
            sp3 = sp3.offset(1);
            *c2rust_fresh23 = *bp;
            let c2rust_fresh24 = sp3;
            sp3 = sp3.offset(1);
            *c2rust_fresh24 = *bp;
            let c2rust_fresh25 = sp4;
            sp4 = sp4.offset(1);
            *c2rust_fresh25 = *bp;
            let c2rust_fresh26 = sp4;
            sp4 = sp4.offset(1);
            *c2rust_fresh26 = *bp;
            let c2rust_fresh27 = sp4;
            sp4 = sp4.offset(1);
            *c2rust_fresh27 = *bp;
            let c2rust_fresh28 = sp4;
            sp4 = sp4.offset(1);
            *c2rust_fresh28 = *bp;
            bp = bp.offset(1);
            x += 1;
        }
        screenp = screenp.offset(multi_pitch as isize);
        screenp2 = screenp2.offset(multi_pitch as isize);
        screenp3 = screenp3.offset(multi_pitch as isize);
        screenp4 = screenp4.offset(multi_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        y += 1;
    }
    return true_0 as boolean;
}
#[no_mangle]
pub static mut mode_scale_4x: screen_mode_t = screen_mode_t {
    width: SCREENWIDTH * 4 as ::core::ffi::c_int,
    height: SCREENHEIGHT * 4 as ::core::ffi::c_int,
    InitMode: None,
    DrawScreen: Some(
        I_Scale4x
            as unsafe extern "C" fn(
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
            ) -> boolean,
    ),
    poor_quality: false_0 as boolean,
};
unsafe extern "C" fn I_Scale5x(
    mut x1: ::core::ffi::c_int,
    mut y1: ::core::ffi::c_int,
    mut x2: ::core::ffi::c_int,
    mut y2: ::core::ffi::c_int,
) -> boolean {
    let mut bufp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp2: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp3: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp4: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp5: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut x: ::core::ffi::c_int = 0;
    let mut y: ::core::ffi::c_int = 0;
    let mut multi_pitch: ::core::ffi::c_int = 0;
    multi_pitch = dest_pitch * 5 as ::core::ffi::c_int;
    bufp = src_buffer
        .offset((y1 * SCREENWIDTH) as isize)
        .offset(x1 as isize);
    screenp = dest_buffer.offset(((y1 * dest_pitch + x1) * 5 as ::core::ffi::c_int) as isize);
    screenp2 = screenp.offset(dest_pitch as isize);
    screenp3 = screenp.offset((dest_pitch * 2 as ::core::ffi::c_int) as isize);
    screenp4 = screenp.offset((dest_pitch * 3 as ::core::ffi::c_int) as isize);
    screenp5 = screenp.offset((dest_pitch * 4 as ::core::ffi::c_int) as isize);
    y = y1;
    while y < y2 {
        let mut sp: *mut byte = ::core::ptr::null_mut::<byte>();
        let mut sp2: *mut byte = ::core::ptr::null_mut::<byte>();
        let mut sp3: *mut byte = ::core::ptr::null_mut::<byte>();
        let mut sp4: *mut byte = ::core::ptr::null_mut::<byte>();
        let mut sp5: *mut byte = ::core::ptr::null_mut::<byte>();
        let mut bp: *mut byte = ::core::ptr::null_mut::<byte>();
        sp = screenp;
        sp2 = screenp2;
        sp3 = screenp3;
        sp4 = screenp4;
        sp5 = screenp5;
        bp = bufp;
        x = x1;
        while x < x2 {
            let c2rust_fresh29 = sp;
            sp = sp.offset(1);
            *c2rust_fresh29 = *bp;
            let c2rust_fresh30 = sp;
            sp = sp.offset(1);
            *c2rust_fresh30 = *bp;
            let c2rust_fresh31 = sp;
            sp = sp.offset(1);
            *c2rust_fresh31 = *bp;
            let c2rust_fresh32 = sp;
            sp = sp.offset(1);
            *c2rust_fresh32 = *bp;
            let c2rust_fresh33 = sp;
            sp = sp.offset(1);
            *c2rust_fresh33 = *bp;
            let c2rust_fresh34 = sp2;
            sp2 = sp2.offset(1);
            *c2rust_fresh34 = *bp;
            let c2rust_fresh35 = sp2;
            sp2 = sp2.offset(1);
            *c2rust_fresh35 = *bp;
            let c2rust_fresh36 = sp2;
            sp2 = sp2.offset(1);
            *c2rust_fresh36 = *bp;
            let c2rust_fresh37 = sp2;
            sp2 = sp2.offset(1);
            *c2rust_fresh37 = *bp;
            let c2rust_fresh38 = sp2;
            sp2 = sp2.offset(1);
            *c2rust_fresh38 = *bp;
            let c2rust_fresh39 = sp3;
            sp3 = sp3.offset(1);
            *c2rust_fresh39 = *bp;
            let c2rust_fresh40 = sp3;
            sp3 = sp3.offset(1);
            *c2rust_fresh40 = *bp;
            let c2rust_fresh41 = sp3;
            sp3 = sp3.offset(1);
            *c2rust_fresh41 = *bp;
            let c2rust_fresh42 = sp3;
            sp3 = sp3.offset(1);
            *c2rust_fresh42 = *bp;
            let c2rust_fresh43 = sp3;
            sp3 = sp3.offset(1);
            *c2rust_fresh43 = *bp;
            let c2rust_fresh44 = sp4;
            sp4 = sp4.offset(1);
            *c2rust_fresh44 = *bp;
            let c2rust_fresh45 = sp4;
            sp4 = sp4.offset(1);
            *c2rust_fresh45 = *bp;
            let c2rust_fresh46 = sp4;
            sp4 = sp4.offset(1);
            *c2rust_fresh46 = *bp;
            let c2rust_fresh47 = sp4;
            sp4 = sp4.offset(1);
            *c2rust_fresh47 = *bp;
            let c2rust_fresh48 = sp4;
            sp4 = sp4.offset(1);
            *c2rust_fresh48 = *bp;
            let c2rust_fresh49 = sp5;
            sp5 = sp5.offset(1);
            *c2rust_fresh49 = *bp;
            let c2rust_fresh50 = sp5;
            sp5 = sp5.offset(1);
            *c2rust_fresh50 = *bp;
            let c2rust_fresh51 = sp5;
            sp5 = sp5.offset(1);
            *c2rust_fresh51 = *bp;
            let c2rust_fresh52 = sp5;
            sp5 = sp5.offset(1);
            *c2rust_fresh52 = *bp;
            let c2rust_fresh53 = sp5;
            sp5 = sp5.offset(1);
            *c2rust_fresh53 = *bp;
            bp = bp.offset(1);
            x += 1;
        }
        screenp = screenp.offset(multi_pitch as isize);
        screenp2 = screenp2.offset(multi_pitch as isize);
        screenp3 = screenp3.offset(multi_pitch as isize);
        screenp4 = screenp4.offset(multi_pitch as isize);
        screenp5 = screenp5.offset(multi_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        y += 1;
    }
    return true_0 as boolean;
}
#[no_mangle]
pub static mut mode_scale_5x: screen_mode_t = screen_mode_t {
    width: SCREENWIDTH * 5 as ::core::ffi::c_int,
    height: SCREENHEIGHT * 5 as ::core::ffi::c_int,
    InitMode: None,
    DrawScreen: Some(
        I_Scale5x
            as unsafe extern "C" fn(
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
            ) -> boolean,
    ),
    poor_quality: false_0 as boolean,
};
unsafe extern "C" fn FindNearestColor(
    mut palette: *mut byte,
    mut r: ::core::ffi::c_int,
    mut g: ::core::ffi::c_int,
    mut b: ::core::ffi::c_int,
) -> ::core::ffi::c_int {
    let mut col: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut best: ::core::ffi::c_int = 0;
    let mut best_diff: ::core::ffi::c_int = 0;
    let mut diff: ::core::ffi::c_int = 0;
    let mut i: ::core::ffi::c_int = 0;
    best = 0 as ::core::ffi::c_int;
    best_diff = INT_MAX;
    i = 0 as ::core::ffi::c_int;
    while i < 256 as ::core::ffi::c_int {
        col = palette.offset((i * 3 as ::core::ffi::c_int) as isize);
        diff = (r - *col.offset(0 as ::core::ffi::c_int as isize) as ::core::ffi::c_int)
            * (r - *col.offset(0 as ::core::ffi::c_int as isize) as ::core::ffi::c_int)
            + (g - *col.offset(1 as ::core::ffi::c_int as isize) as ::core::ffi::c_int)
                * (g - *col.offset(1 as ::core::ffi::c_int as isize) as ::core::ffi::c_int)
            + (b - *col.offset(2 as ::core::ffi::c_int as isize) as ::core::ffi::c_int)
                * (b - *col.offset(2 as ::core::ffi::c_int as isize) as ::core::ffi::c_int);
        if diff == 0 as ::core::ffi::c_int {
            return i;
        } else if diff < best_diff {
            best = i;
            best_diff = diff;
        }
        i += 1;
    }
    return best;
}
unsafe extern "C" fn GenerateStretchTable(
    mut palette: *mut byte,
    mut pct: ::core::ffi::c_int,
) -> *mut byte {
    let mut result: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut x: ::core::ffi::c_int = 0;
    let mut y: ::core::ffi::c_int = 0;
    let mut r: ::core::ffi::c_int = 0;
    let mut g: ::core::ffi::c_int = 0;
    let mut b: ::core::ffi::c_int = 0;
    let mut col1: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut col2: *mut byte = ::core::ptr::null_mut::<byte>();
    result = Z_Malloc(
        256 as ::core::ffi::c_int * 256 as ::core::ffi::c_int,
        PU_STATIC as ::core::ffi::c_int,
        NULL,
    ) as *mut byte;
    x = 0 as ::core::ffi::c_int;
    while x < 256 as ::core::ffi::c_int {
        y = 0 as ::core::ffi::c_int;
        while y < 256 as ::core::ffi::c_int {
            col1 = palette.offset((x * 3 as ::core::ffi::c_int) as isize);
            col2 = palette.offset((y * 3 as ::core::ffi::c_int) as isize);
            r = (*col1.offset(0 as ::core::ffi::c_int as isize) as ::core::ffi::c_int * pct
                + *col2.offset(0 as ::core::ffi::c_int as isize) as ::core::ffi::c_int
                    * (100 as ::core::ffi::c_int - pct))
                / 100 as ::core::ffi::c_int;
            g = (*col1.offset(1 as ::core::ffi::c_int as isize) as ::core::ffi::c_int * pct
                + *col2.offset(1 as ::core::ffi::c_int as isize) as ::core::ffi::c_int
                    * (100 as ::core::ffi::c_int - pct))
                / 100 as ::core::ffi::c_int;
            b = (*col1.offset(2 as ::core::ffi::c_int as isize) as ::core::ffi::c_int * pct
                + *col2.offset(2 as ::core::ffi::c_int as isize) as ::core::ffi::c_int
                    * (100 as ::core::ffi::c_int - pct))
                / 100 as ::core::ffi::c_int;
            *result.offset((x * 256 as ::core::ffi::c_int + y) as isize) =
                FindNearestColor(palette, r, g, b) as byte;
            y += 1;
        }
        x += 1;
    }
    return result;
}
unsafe extern "C" fn I_InitStretchTables(mut palette: *mut byte) {
    if !stretch_tables[0 as ::core::ffi::c_int as usize].is_null() {
        return;
    }
    printf(
        b"I_InitStretchTables: Generating lookup tables..\0".as_ptr() as *const ::core::ffi::c_char,
    );
    fflush(stdout);
    stretch_tables[0 as ::core::ffi::c_int as usize] =
        GenerateStretchTable(palette, 20 as ::core::ffi::c_int);
    printf(b"..\0".as_ptr() as *const ::core::ffi::c_char);
    fflush(stdout);
    stretch_tables[1 as ::core::ffi::c_int as usize] =
        GenerateStretchTable(palette, 40 as ::core::ffi::c_int);
    puts(b"\0".as_ptr() as *const ::core::ffi::c_char);
}
unsafe extern "C" fn I_InitSquashTable(mut palette: *mut byte) {
    if !half_stretch_table.is_null() {
        return;
    }
    printf(b"I_InitSquashTable: Generating lookup table..\0".as_ptr() as *const ::core::ffi::c_char);
    fflush(stdout);
    half_stretch_table = GenerateStretchTable(palette, 50 as ::core::ffi::c_int);
    puts(b"\0".as_ptr() as *const ::core::ffi::c_char);
}
#[no_mangle]
pub unsafe extern "C" fn I_ResetScaleTables(mut palette: *mut byte) {
    if !stretch_tables[0 as ::core::ffi::c_int as usize].is_null() {
        Z_Free(stretch_tables[0 as ::core::ffi::c_int as usize] as *mut ::core::ffi::c_void);
        Z_Free(stretch_tables[1 as ::core::ffi::c_int as usize] as *mut ::core::ffi::c_void);
        printf(
            b"I_ResetScaleTables: Regenerating lookup tables..\n\0".as_ptr()
                as *const ::core::ffi::c_char,
        );
        stretch_tables[0 as ::core::ffi::c_int as usize] =
            GenerateStretchTable(palette, 20 as ::core::ffi::c_int);
        stretch_tables[1 as ::core::ffi::c_int as usize] =
            GenerateStretchTable(palette, 40 as ::core::ffi::c_int);
    }
    if !half_stretch_table.is_null() {
        Z_Free(half_stretch_table as *mut ::core::ffi::c_void);
        printf(
            b"I_ResetScaleTables: Regenerating lookup table..\n\0".as_ptr()
                as *const ::core::ffi::c_char,
        );
        half_stretch_table = GenerateStretchTable(palette, 50 as ::core::ffi::c_int);
    }
}
#[inline]
unsafe extern "C" fn WriteBlendedLine1x(
    mut dest: *mut byte,
    mut src1: *mut byte,
    mut src2: *mut byte,
    mut stretch_table: *mut byte,
) {
    let mut x: ::core::ffi::c_int = 0;
    x = 0 as ::core::ffi::c_int;
    while x < SCREENWIDTH {
        *dest = *stretch_table.offset(
            (*src1 as ::core::ffi::c_int * 256 as ::core::ffi::c_int + *src2 as ::core::ffi::c_int)
                as isize,
        );
        dest = dest.offset(1);
        src1 = src1.offset(1);
        src2 = src2.offset(1);
        x += 1;
    }
}
unsafe extern "C" fn I_Stretch1x(
    mut x1: ::core::ffi::c_int,
    mut y1: ::core::ffi::c_int,
    mut x2: ::core::ffi::c_int,
    mut y2: ::core::ffi::c_int,
) -> boolean {
    let mut bufp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut y: ::core::ffi::c_int = 0;
    if x1 != 0 as ::core::ffi::c_int
        || y1 != 0 as ::core::ffi::c_int
        || x2 != SCREENWIDTH
        || y2 != SCREENHEIGHT
    {
        return false_0 as boolean;
    }
    bufp = src_buffer
        .offset((y1 * SCREENWIDTH) as isize)
        .offset(x1 as isize);
    screenp = dest_buffer
        .offset((y1 * dest_pitch) as isize)
        .offset(x1 as isize);
    y = 0 as ::core::ffi::c_int;
    while y < SCREENHEIGHT {
        memcpy(
            screenp as *mut ::core::ffi::c_void,
            bufp as *const ::core::ffi::c_void,
            SCREENWIDTH as size_t,
        );
        screenp = screenp.offset(dest_pitch as isize);
        WriteBlendedLine1x(
            screenp,
            bufp,
            bufp.offset(SCREENWIDTH as isize),
            stretch_tables[0 as ::core::ffi::c_int as usize],
        );
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        WriteBlendedLine1x(
            screenp,
            bufp,
            bufp.offset(SCREENWIDTH as isize),
            stretch_tables[1 as ::core::ffi::c_int as usize],
        );
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        WriteBlendedLine1x(
            screenp,
            bufp.offset(SCREENWIDTH as isize),
            bufp,
            stretch_tables[1 as ::core::ffi::c_int as usize],
        );
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        WriteBlendedLine1x(
            screenp,
            bufp.offset(SCREENWIDTH as isize),
            bufp,
            stretch_tables[0 as ::core::ffi::c_int as usize],
        );
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        memcpy(
            screenp as *mut ::core::ffi::c_void,
            bufp as *const ::core::ffi::c_void,
            SCREENWIDTH as size_t,
        );
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        y += 5 as ::core::ffi::c_int;
    }
    return true_0 as boolean;
}
#[no_mangle]
pub static mut mode_stretch_1x: screen_mode_t = screen_mode_t {
    width: SCREENWIDTH,
    height: SCREENHEIGHT_4_3,
    InitMode: Some(I_InitStretchTables as unsafe extern "C" fn(*mut byte) -> ()),
    DrawScreen: Some(
        I_Stretch1x
            as unsafe extern "C" fn(
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
            ) -> boolean,
    ),
    poor_quality: true_0 as boolean,
};
#[inline]
unsafe extern "C" fn WriteLine2x(mut dest: *mut byte, mut src: *mut byte) {
    let mut x: ::core::ffi::c_int = 0;
    x = 0 as ::core::ffi::c_int;
    while x < SCREENWIDTH {
        *dest.offset(0 as ::core::ffi::c_int as isize) = *src;
        *dest.offset(1 as ::core::ffi::c_int as isize) = *src;
        dest = dest.offset(2 as ::core::ffi::c_int as isize);
        src = src.offset(1);
        x += 1;
    }
}
#[inline]
unsafe extern "C" fn WriteBlendedLine2x(
    mut dest: *mut byte,
    mut src1: *mut byte,
    mut src2: *mut byte,
    mut stretch_table: *mut byte,
) {
    let mut x: ::core::ffi::c_int = 0;
    let mut val: ::core::ffi::c_int = 0;
    x = 0 as ::core::ffi::c_int;
    while x < SCREENWIDTH {
        val = *stretch_table.offset(
            (*src1 as ::core::ffi::c_int * 256 as ::core::ffi::c_int + *src2 as ::core::ffi::c_int)
                as isize,
        ) as ::core::ffi::c_int;
        *dest.offset(0 as ::core::ffi::c_int as isize) = val as byte;
        *dest.offset(1 as ::core::ffi::c_int as isize) = val as byte;
        dest = dest.offset(2 as ::core::ffi::c_int as isize);
        src1 = src1.offset(1);
        src2 = src2.offset(1);
        x += 1;
    }
}
unsafe extern "C" fn I_Stretch2x(
    mut x1: ::core::ffi::c_int,
    mut y1: ::core::ffi::c_int,
    mut x2: ::core::ffi::c_int,
    mut y2: ::core::ffi::c_int,
) -> boolean {
    let mut bufp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut y: ::core::ffi::c_int = 0;
    if x1 != 0 as ::core::ffi::c_int
        || y1 != 0 as ::core::ffi::c_int
        || x2 != SCREENWIDTH
        || y2 != SCREENHEIGHT
    {
        return false_0 as boolean;
    }
    bufp = src_buffer
        .offset((y1 * SCREENWIDTH) as isize)
        .offset(x1 as isize);
    screenp = dest_buffer
        .offset((y1 * dest_pitch) as isize)
        .offset(x1 as isize);
    y = 0 as ::core::ffi::c_int;
    while y < SCREENHEIGHT {
        WriteLine2x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine2x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteBlendedLine2x(
            screenp,
            bufp,
            bufp.offset(SCREENWIDTH as isize),
            stretch_tables[1 as ::core::ffi::c_int as usize],
        );
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        WriteLine2x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteBlendedLine2x(
            screenp,
            bufp.offset(SCREENWIDTH as isize),
            bufp,
            stretch_tables[0 as ::core::ffi::c_int as usize],
        );
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        WriteLine2x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine2x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteBlendedLine2x(
            screenp,
            bufp,
            bufp.offset(SCREENWIDTH as isize),
            stretch_tables[0 as ::core::ffi::c_int as usize],
        );
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        WriteLine2x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteBlendedLine2x(
            screenp,
            bufp.offset(SCREENWIDTH as isize),
            bufp,
            stretch_tables[1 as ::core::ffi::c_int as usize],
        );
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        WriteLine2x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine2x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        y += 5 as ::core::ffi::c_int;
    }
    return true_0 as boolean;
}
#[no_mangle]
pub static mut mode_stretch_2x: screen_mode_t = screen_mode_t {
    width: SCREENWIDTH * 2 as ::core::ffi::c_int,
    height: SCREENHEIGHT_4_3 * 2 as ::core::ffi::c_int,
    InitMode: Some(I_InitStretchTables as unsafe extern "C" fn(*mut byte) -> ()),
    DrawScreen: Some(
        I_Stretch2x
            as unsafe extern "C" fn(
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
            ) -> boolean,
    ),
    poor_quality: false_0 as boolean,
};
#[inline]
unsafe extern "C" fn WriteLine3x(mut dest: *mut byte, mut src: *mut byte) {
    let mut x: ::core::ffi::c_int = 0;
    x = 0 as ::core::ffi::c_int;
    while x < SCREENWIDTH {
        *dest.offset(0 as ::core::ffi::c_int as isize) = *src;
        *dest.offset(1 as ::core::ffi::c_int as isize) = *src;
        *dest.offset(2 as ::core::ffi::c_int as isize) = *src;
        dest = dest.offset(3 as ::core::ffi::c_int as isize);
        src = src.offset(1);
        x += 1;
    }
}
#[inline]
unsafe extern "C" fn WriteBlendedLine3x(
    mut dest: *mut byte,
    mut src1: *mut byte,
    mut src2: *mut byte,
    mut stretch_table: *mut byte,
) {
    let mut x: ::core::ffi::c_int = 0;
    let mut val: ::core::ffi::c_int = 0;
    x = 0 as ::core::ffi::c_int;
    while x < SCREENWIDTH {
        val = *stretch_table.offset(
            (*src1 as ::core::ffi::c_int * 256 as ::core::ffi::c_int + *src2 as ::core::ffi::c_int)
                as isize,
        ) as ::core::ffi::c_int;
        *dest.offset(0 as ::core::ffi::c_int as isize) = val as byte;
        *dest.offset(1 as ::core::ffi::c_int as isize) = val as byte;
        *dest.offset(2 as ::core::ffi::c_int as isize) = val as byte;
        dest = dest.offset(3 as ::core::ffi::c_int as isize);
        src1 = src1.offset(1);
        src2 = src2.offset(1);
        x += 1;
    }
}
unsafe extern "C" fn I_Stretch3x(
    mut x1: ::core::ffi::c_int,
    mut y1: ::core::ffi::c_int,
    mut x2: ::core::ffi::c_int,
    mut y2: ::core::ffi::c_int,
) -> boolean {
    let mut bufp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut y: ::core::ffi::c_int = 0;
    if x1 != 0 as ::core::ffi::c_int
        || y1 != 0 as ::core::ffi::c_int
        || x2 != SCREENWIDTH
        || y2 != SCREENHEIGHT
    {
        return false_0 as boolean;
    }
    bufp = src_buffer
        .offset((y1 * SCREENWIDTH) as isize)
        .offset(x1 as isize);
    screenp = dest_buffer
        .offset((y1 * dest_pitch) as isize)
        .offset(x1 as isize);
    y = 0 as ::core::ffi::c_int;
    while y < SCREENHEIGHT {
        WriteLine3x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine3x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine3x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteBlendedLine3x(
            screenp,
            bufp.offset(SCREENWIDTH as isize),
            bufp,
            stretch_tables[1 as ::core::ffi::c_int as usize],
        );
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        WriteLine3x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine3x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine3x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteBlendedLine3x(
            screenp,
            bufp,
            bufp.offset(SCREENWIDTH as isize),
            stretch_tables[0 as ::core::ffi::c_int as usize],
        );
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        WriteLine3x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine3x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteBlendedLine3x(
            screenp,
            bufp.offset(SCREENWIDTH as isize),
            bufp,
            stretch_tables[0 as ::core::ffi::c_int as usize],
        );
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        WriteLine3x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine3x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine3x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteBlendedLine3x(
            screenp,
            bufp,
            bufp.offset(SCREENWIDTH as isize),
            stretch_tables[1 as ::core::ffi::c_int as usize],
        );
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        WriteLine3x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine3x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine3x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        y += 5 as ::core::ffi::c_int;
    }
    return true_0 as boolean;
}
#[no_mangle]
pub static mut mode_stretch_3x: screen_mode_t = screen_mode_t {
    width: SCREENWIDTH * 3 as ::core::ffi::c_int,
    height: SCREENHEIGHT_4_3 * 3 as ::core::ffi::c_int,
    InitMode: Some(I_InitStretchTables as unsafe extern "C" fn(*mut byte) -> ()),
    DrawScreen: Some(
        I_Stretch3x
            as unsafe extern "C" fn(
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
            ) -> boolean,
    ),
    poor_quality: false_0 as boolean,
};
#[inline]
unsafe extern "C" fn WriteLine4x(mut dest: *mut byte, mut src: *mut byte) {
    let mut x: ::core::ffi::c_int = 0;
    x = 0 as ::core::ffi::c_int;
    while x < SCREENWIDTH {
        *dest.offset(0 as ::core::ffi::c_int as isize) = *src;
        *dest.offset(1 as ::core::ffi::c_int as isize) = *src;
        *dest.offset(2 as ::core::ffi::c_int as isize) = *src;
        *dest.offset(3 as ::core::ffi::c_int as isize) = *src;
        dest = dest.offset(4 as ::core::ffi::c_int as isize);
        src = src.offset(1);
        x += 1;
    }
}
#[inline]
unsafe extern "C" fn WriteBlendedLine4x(
    mut dest: *mut byte,
    mut src1: *mut byte,
    mut src2: *mut byte,
    mut stretch_table: *mut byte,
) {
    let mut x: ::core::ffi::c_int = 0;
    let mut val: ::core::ffi::c_int = 0;
    x = 0 as ::core::ffi::c_int;
    while x < SCREENWIDTH {
        val = *stretch_table.offset(
            (*src1 as ::core::ffi::c_int * 256 as ::core::ffi::c_int + *src2 as ::core::ffi::c_int)
                as isize,
        ) as ::core::ffi::c_int;
        *dest.offset(0 as ::core::ffi::c_int as isize) = val as byte;
        *dest.offset(1 as ::core::ffi::c_int as isize) = val as byte;
        *dest.offset(2 as ::core::ffi::c_int as isize) = val as byte;
        *dest.offset(3 as ::core::ffi::c_int as isize) = val as byte;
        dest = dest.offset(4 as ::core::ffi::c_int as isize);
        src1 = src1.offset(1);
        src2 = src2.offset(1);
        x += 1;
    }
}
unsafe extern "C" fn I_Stretch4x(
    mut x1: ::core::ffi::c_int,
    mut y1: ::core::ffi::c_int,
    mut x2: ::core::ffi::c_int,
    mut y2: ::core::ffi::c_int,
) -> boolean {
    let mut bufp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut y: ::core::ffi::c_int = 0;
    if x1 != 0 as ::core::ffi::c_int
        || y1 != 0 as ::core::ffi::c_int
        || x2 != SCREENWIDTH
        || y2 != SCREENHEIGHT
    {
        return false_0 as boolean;
    }
    bufp = src_buffer
        .offset((y1 * SCREENWIDTH) as isize)
        .offset(x1 as isize);
    screenp = dest_buffer
        .offset((y1 * dest_pitch) as isize)
        .offset(x1 as isize);
    y = 0 as ::core::ffi::c_int;
    while y < SCREENHEIGHT {
        WriteLine4x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine4x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine4x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine4x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteBlendedLine4x(
            screenp,
            bufp.offset(SCREENWIDTH as isize),
            bufp,
            stretch_tables[0 as ::core::ffi::c_int as usize],
        );
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        WriteLine4x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine4x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine4x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine4x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteBlendedLine4x(
            screenp,
            bufp.offset(SCREENWIDTH as isize),
            bufp,
            stretch_tables[1 as ::core::ffi::c_int as usize],
        );
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        WriteLine4x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine4x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine4x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine4x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteBlendedLine4x(
            screenp,
            bufp,
            bufp.offset(SCREENWIDTH as isize),
            stretch_tables[1 as ::core::ffi::c_int as usize],
        );
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        WriteLine4x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine4x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine4x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine4x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteBlendedLine4x(
            screenp,
            bufp,
            bufp.offset(SCREENWIDTH as isize),
            stretch_tables[0 as ::core::ffi::c_int as usize],
        );
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        WriteLine4x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine4x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine4x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine4x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        y += 5 as ::core::ffi::c_int;
    }
    return true_0 as boolean;
}
#[no_mangle]
pub static mut mode_stretch_4x: screen_mode_t = screen_mode_t {
    width: SCREENWIDTH * 4 as ::core::ffi::c_int,
    height: SCREENHEIGHT_4_3 * 4 as ::core::ffi::c_int,
    InitMode: Some(I_InitStretchTables as unsafe extern "C" fn(*mut byte) -> ()),
    DrawScreen: Some(
        I_Stretch4x
            as unsafe extern "C" fn(
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
            ) -> boolean,
    ),
    poor_quality: false_0 as boolean,
};
#[inline]
unsafe extern "C" fn WriteLine5x(mut dest: *mut byte, mut src: *mut byte) {
    let mut x: ::core::ffi::c_int = 0;
    x = 0 as ::core::ffi::c_int;
    while x < SCREENWIDTH {
        *dest.offset(0 as ::core::ffi::c_int as isize) = *src;
        *dest.offset(1 as ::core::ffi::c_int as isize) = *src;
        *dest.offset(2 as ::core::ffi::c_int as isize) = *src;
        *dest.offset(3 as ::core::ffi::c_int as isize) = *src;
        *dest.offset(4 as ::core::ffi::c_int as isize) = *src;
        dest = dest.offset(5 as ::core::ffi::c_int as isize);
        src = src.offset(1);
        x += 1;
    }
}
unsafe extern "C" fn I_Stretch5x(
    mut x1: ::core::ffi::c_int,
    mut y1: ::core::ffi::c_int,
    mut x2: ::core::ffi::c_int,
    mut y2: ::core::ffi::c_int,
) -> boolean {
    let mut bufp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut y: ::core::ffi::c_int = 0;
    if x1 != 0 as ::core::ffi::c_int
        || y1 != 0 as ::core::ffi::c_int
        || x2 != SCREENWIDTH
        || y2 != SCREENHEIGHT
    {
        return false_0 as boolean;
    }
    bufp = src_buffer
        .offset((y1 * SCREENWIDTH) as isize)
        .offset(x1 as isize);
    screenp = dest_buffer
        .offset((y1 * dest_pitch) as isize)
        .offset(x1 as isize);
    y = 0 as ::core::ffi::c_int;
    while y < SCREENHEIGHT {
        WriteLine5x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine5x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine5x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine5x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine5x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        WriteLine5x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        y += 1 as ::core::ffi::c_int;
    }
    if M_CheckParm(b"-scanline\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char)
        > 0 as ::core::ffi::c_int
    {
        screenp = dest_buffer.offset((2 as ::core::ffi::c_int * dest_pitch) as isize);
        y = 0 as ::core::ffi::c_int;
        while y < 1198 as ::core::ffi::c_int {
            memset(
                screenp as *mut ::core::ffi::c_void,
                0 as ::core::ffi::c_int,
                1600 as size_t,
            );
            screenp = screenp.offset((dest_pitch * 3 as ::core::ffi::c_int) as isize);
            y += 3 as ::core::ffi::c_int;
        }
    }
    return true_0 as boolean;
}
#[no_mangle]
pub static mut mode_stretch_5x: screen_mode_t = screen_mode_t {
    width: SCREENWIDTH * 5 as ::core::ffi::c_int,
    height: SCREENHEIGHT_4_3 * 5 as ::core::ffi::c_int,
    InitMode: Some(I_InitStretchTables as unsafe extern "C" fn(*mut byte) -> ()),
    DrawScreen: Some(
        I_Stretch5x
            as unsafe extern "C" fn(
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
            ) -> boolean,
    ),
    poor_quality: false_0 as boolean,
};
#[inline]
unsafe extern "C" fn WriteSquashedLine1x(mut dest: *mut byte, mut src: *mut byte) {
    let mut x: ::core::ffi::c_int = 0;
    x = 0 as ::core::ffi::c_int;
    while x < SCREENWIDTH {
        let c2rust_fresh54 = dest;
        dest = dest.offset(1);
        *c2rust_fresh54 = *stretch_tables[0 as ::core::ffi::c_int as usize].offset(
            (*src.offset(1 as ::core::ffi::c_int as isize) as ::core::ffi::c_int
                * 256 as ::core::ffi::c_int
                + *src.offset(0 as ::core::ffi::c_int as isize) as ::core::ffi::c_int)
                as isize,
        );
        let c2rust_fresh55 = dest;
        dest = dest.offset(1);
        *c2rust_fresh55 = *stretch_tables[1 as ::core::ffi::c_int as usize].offset(
            (*src.offset(2 as ::core::ffi::c_int as isize) as ::core::ffi::c_int
                * 256 as ::core::ffi::c_int
                + *src.offset(1 as ::core::ffi::c_int as isize) as ::core::ffi::c_int)
                as isize,
        );
        let c2rust_fresh56 = dest;
        dest = dest.offset(1);
        *c2rust_fresh56 = *stretch_tables[1 as ::core::ffi::c_int as usize].offset(
            (*src.offset(2 as ::core::ffi::c_int as isize) as ::core::ffi::c_int
                * 256 as ::core::ffi::c_int
                + *src.offset(3 as ::core::ffi::c_int as isize) as ::core::ffi::c_int)
                as isize,
        );
        let c2rust_fresh57 = dest;
        dest = dest.offset(1);
        *c2rust_fresh57 = *stretch_tables[0 as ::core::ffi::c_int as usize].offset(
            (*src.offset(3 as ::core::ffi::c_int as isize) as ::core::ffi::c_int
                * 256 as ::core::ffi::c_int
                + *src.offset(4 as ::core::ffi::c_int as isize) as ::core::ffi::c_int)
                as isize,
        );
        x += 5 as ::core::ffi::c_int;
        src = src.offset(5 as ::core::ffi::c_int as isize);
    }
}
unsafe extern "C" fn I_Squash1x(
    mut x1: ::core::ffi::c_int,
    mut y1: ::core::ffi::c_int,
    mut x2: ::core::ffi::c_int,
    mut y2: ::core::ffi::c_int,
) -> boolean {
    let mut bufp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut y: ::core::ffi::c_int = 0;
    if x1 != 0 as ::core::ffi::c_int
        || y1 != 0 as ::core::ffi::c_int
        || x2 != SCREENWIDTH
        || y2 != SCREENHEIGHT
    {
        return false_0 as boolean;
    }
    bufp = src_buffer;
    screenp = dest_buffer;
    y = 0 as ::core::ffi::c_int;
    while y < SCREENHEIGHT {
        WriteSquashedLine1x(screenp, bufp);
        screenp = screenp.offset(dest_pitch as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        y += 1;
    }
    return true_0 as boolean;
}
#[no_mangle]
pub static mut mode_squash_1x: screen_mode_t = screen_mode_t {
    width: SCREENWIDTH_4_3,
    height: SCREENHEIGHT,
    InitMode: Some(I_InitStretchTables as unsafe extern "C" fn(*mut byte) -> ()),
    DrawScreen: Some(
        I_Squash1x
            as unsafe extern "C" fn(
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
            ) -> boolean,
    ),
    poor_quality: true_0 as boolean,
};
#[inline]
unsafe extern "C" fn WriteSquashedLine2x(mut dest: *mut byte, mut src: *mut byte) {
    let mut dest2: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut x: ::core::ffi::c_int = 0;
    let mut c: ::core::ffi::c_int = 0;
    dest2 = dest.offset(dest_pitch as isize);
    x = 0 as ::core::ffi::c_int;
    while x < SCREENWIDTH {
        c = *src.offset(0 as ::core::ffi::c_int as isize) as ::core::ffi::c_int;
        let c2rust_fresh58 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh58 = c as byte;
        let c2rust_fresh59 = dest;
        dest = dest.offset(1);
        *c2rust_fresh59 = *c2rust_fresh58;
        c = *stretch_tables[1 as ::core::ffi::c_int as usize].offset(
            (*src.offset(1 as ::core::ffi::c_int as isize) as ::core::ffi::c_int
                * 256 as ::core::ffi::c_int
                + *src.offset(0 as ::core::ffi::c_int as isize) as ::core::ffi::c_int)
                as isize,
        ) as ::core::ffi::c_int;
        let c2rust_fresh60 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh60 = c as byte;
        let c2rust_fresh61 = dest;
        dest = dest.offset(1);
        *c2rust_fresh61 = *c2rust_fresh60;
        c = *src.offset(1 as ::core::ffi::c_int as isize) as ::core::ffi::c_int;
        let c2rust_fresh62 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh62 = c as byte;
        let c2rust_fresh63 = dest;
        dest = dest.offset(1);
        *c2rust_fresh63 = *c2rust_fresh62;
        c = *stretch_tables[0 as ::core::ffi::c_int as usize].offset(
            (*src.offset(1 as ::core::ffi::c_int as isize) as ::core::ffi::c_int
                * 256 as ::core::ffi::c_int
                + *src.offset(2 as ::core::ffi::c_int as isize) as ::core::ffi::c_int)
                as isize,
        ) as ::core::ffi::c_int;
        let c2rust_fresh64 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh64 = c as byte;
        let c2rust_fresh65 = dest;
        dest = dest.offset(1);
        *c2rust_fresh65 = *c2rust_fresh64;
        c = *stretch_tables[0 as ::core::ffi::c_int as usize].offset(
            (*src.offset(3 as ::core::ffi::c_int as isize) as ::core::ffi::c_int
                * 256 as ::core::ffi::c_int
                + *src.offset(2 as ::core::ffi::c_int as isize) as ::core::ffi::c_int)
                as isize,
        ) as ::core::ffi::c_int;
        let c2rust_fresh66 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh66 = c as byte;
        let c2rust_fresh67 = dest;
        dest = dest.offset(1);
        *c2rust_fresh67 = *c2rust_fresh66;
        c = *src.offset(3 as ::core::ffi::c_int as isize) as ::core::ffi::c_int;
        let c2rust_fresh68 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh68 = c as byte;
        let c2rust_fresh69 = dest;
        dest = dest.offset(1);
        *c2rust_fresh69 = *c2rust_fresh68;
        c = *stretch_tables[1 as ::core::ffi::c_int as usize].offset(
            (*src.offset(3 as ::core::ffi::c_int as isize) as ::core::ffi::c_int
                * 256 as ::core::ffi::c_int
                + *src.offset(4 as ::core::ffi::c_int as isize) as ::core::ffi::c_int)
                as isize,
        ) as ::core::ffi::c_int;
        let c2rust_fresh70 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh70 = c as byte;
        let c2rust_fresh71 = dest;
        dest = dest.offset(1);
        *c2rust_fresh71 = *c2rust_fresh70;
        c = *src.offset(4 as ::core::ffi::c_int as isize) as ::core::ffi::c_int;
        let c2rust_fresh72 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh72 = c as byte;
        let c2rust_fresh73 = dest;
        dest = dest.offset(1);
        *c2rust_fresh73 = *c2rust_fresh72;
        x += 5 as ::core::ffi::c_int;
        src = src.offset(5 as ::core::ffi::c_int as isize);
    }
}
unsafe extern "C" fn I_Squash2x(
    mut x1: ::core::ffi::c_int,
    mut y1: ::core::ffi::c_int,
    mut x2: ::core::ffi::c_int,
    mut y2: ::core::ffi::c_int,
) -> boolean {
    let mut bufp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut y: ::core::ffi::c_int = 0;
    if x1 != 0 as ::core::ffi::c_int
        || y1 != 0 as ::core::ffi::c_int
        || x2 != SCREENWIDTH
        || y2 != SCREENHEIGHT
    {
        return false_0 as boolean;
    }
    bufp = src_buffer;
    screenp = dest_buffer;
    y = 0 as ::core::ffi::c_int;
    while y < SCREENHEIGHT {
        WriteSquashedLine2x(screenp, bufp);
        screenp = screenp.offset((dest_pitch * 2 as ::core::ffi::c_int) as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        y += 1;
    }
    return true_0 as boolean;
}
#[no_mangle]
pub static mut mode_squash_2x: screen_mode_t = screen_mode_t {
    width: SCREENWIDTH_4_3 * 2 as ::core::ffi::c_int,
    height: SCREENHEIGHT * 2 as ::core::ffi::c_int,
    InitMode: Some(I_InitStretchTables as unsafe extern "C" fn(*mut byte) -> ()),
    DrawScreen: Some(
        I_Squash2x
            as unsafe extern "C" fn(
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
            ) -> boolean,
    ),
    poor_quality: false_0 as boolean,
};
#[inline]
unsafe extern "C" fn WriteSquashedLine3x(mut dest: *mut byte, mut src: *mut byte) {
    let mut dest2: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut dest3: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut x: ::core::ffi::c_int = 0;
    let mut c: ::core::ffi::c_int = 0;
    dest2 = dest.offset(dest_pitch as isize);
    dest3 = dest.offset((dest_pitch * 2 as ::core::ffi::c_int) as isize);
    x = 0 as ::core::ffi::c_int;
    while x < SCREENWIDTH {
        c = *src.offset(0 as ::core::ffi::c_int as isize) as ::core::ffi::c_int;
        let c2rust_fresh74 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh74 = c as byte;
        let c2rust_fresh75 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh75 = *c2rust_fresh74;
        let c2rust_fresh76 = dest;
        dest = dest.offset(1);
        *c2rust_fresh76 = *c2rust_fresh75;
        let c2rust_fresh77 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh77 = c as byte;
        let c2rust_fresh78 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh78 = *c2rust_fresh77;
        let c2rust_fresh79 = dest;
        dest = dest.offset(1);
        *c2rust_fresh79 = *c2rust_fresh78;
        c = *half_stretch_table.offset(
            (*src.offset(0 as ::core::ffi::c_int as isize) as ::core::ffi::c_int
                * 256 as ::core::ffi::c_int
                + *src.offset(1 as ::core::ffi::c_int as isize) as ::core::ffi::c_int)
                as isize,
        ) as ::core::ffi::c_int;
        let c2rust_fresh80 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh80 = c as byte;
        let c2rust_fresh81 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh81 = *c2rust_fresh80;
        let c2rust_fresh82 = dest;
        dest = dest.offset(1);
        *c2rust_fresh82 = *c2rust_fresh81;
        c = *src.offset(1 as ::core::ffi::c_int as isize) as ::core::ffi::c_int;
        let c2rust_fresh83 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh83 = c as byte;
        let c2rust_fresh84 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh84 = *c2rust_fresh83;
        let c2rust_fresh85 = dest;
        dest = dest.offset(1);
        *c2rust_fresh85 = *c2rust_fresh84;
        let c2rust_fresh86 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh86 = c as byte;
        let c2rust_fresh87 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh87 = *c2rust_fresh86;
        let c2rust_fresh88 = dest;
        dest = dest.offset(1);
        *c2rust_fresh88 = *c2rust_fresh87;
        x += 2 as ::core::ffi::c_int;
        src = src.offset(2 as ::core::ffi::c_int as isize);
    }
}
unsafe extern "C" fn I_Squash3x(
    mut x1: ::core::ffi::c_int,
    mut y1: ::core::ffi::c_int,
    mut x2: ::core::ffi::c_int,
    mut y2: ::core::ffi::c_int,
) -> boolean {
    let mut bufp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut y: ::core::ffi::c_int = 0;
    if x1 != 0 as ::core::ffi::c_int
        || y1 != 0 as ::core::ffi::c_int
        || x2 != SCREENWIDTH
        || y2 != SCREENHEIGHT
    {
        return false_0 as boolean;
    }
    bufp = src_buffer;
    screenp = dest_buffer;
    y = 0 as ::core::ffi::c_int;
    while y < SCREENHEIGHT {
        WriteSquashedLine3x(screenp, bufp);
        screenp = screenp.offset((dest_pitch * 3 as ::core::ffi::c_int) as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        y += 1;
    }
    return true_0 as boolean;
}
#[no_mangle]
pub static mut mode_squash_3x: screen_mode_t = screen_mode_t {
    width: 800 as ::core::ffi::c_int,
    height: 600 as ::core::ffi::c_int,
    InitMode: Some(I_InitSquashTable as unsafe extern "C" fn(*mut byte) -> ()),
    DrawScreen: Some(
        I_Squash3x
            as unsafe extern "C" fn(
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
            ) -> boolean,
    ),
    poor_quality: false_0 as boolean,
};
#[inline]
unsafe extern "C" fn WriteSquashedLine4x(mut dest: *mut byte, mut src: *mut byte) {
    let mut x: ::core::ffi::c_int = 0;
    let mut c: ::core::ffi::c_int = 0;
    let mut dest2: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut dest3: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut dest4: *mut byte = ::core::ptr::null_mut::<byte>();
    dest2 = dest.offset(dest_pitch as isize);
    dest3 = dest.offset((dest_pitch * 2 as ::core::ffi::c_int) as isize);
    dest4 = dest.offset((dest_pitch * 3 as ::core::ffi::c_int) as isize);
    x = 0 as ::core::ffi::c_int;
    while x < SCREENWIDTH {
        c = *src.offset(0 as ::core::ffi::c_int as isize) as ::core::ffi::c_int;
        let c2rust_fresh89 = dest4;
        dest4 = dest4.offset(1);
        *c2rust_fresh89 = c as byte;
        let c2rust_fresh90 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh90 = *c2rust_fresh89;
        let c2rust_fresh91 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh91 = *c2rust_fresh90;
        let c2rust_fresh92 = dest;
        dest = dest.offset(1);
        *c2rust_fresh92 = *c2rust_fresh91;
        let c2rust_fresh93 = dest4;
        dest4 = dest4.offset(1);
        *c2rust_fresh93 = c as byte;
        let c2rust_fresh94 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh94 = *c2rust_fresh93;
        let c2rust_fresh95 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh95 = *c2rust_fresh94;
        let c2rust_fresh96 = dest;
        dest = dest.offset(1);
        *c2rust_fresh96 = *c2rust_fresh95;
        let c2rust_fresh97 = dest4;
        dest4 = dest4.offset(1);
        *c2rust_fresh97 = c as byte;
        let c2rust_fresh98 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh98 = *c2rust_fresh97;
        let c2rust_fresh99 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh99 = *c2rust_fresh98;
        let c2rust_fresh100 = dest;
        dest = dest.offset(1);
        *c2rust_fresh100 = *c2rust_fresh99;
        c = *stretch_tables[0 as ::core::ffi::c_int as usize].offset(
            (*src.offset(0 as ::core::ffi::c_int as isize) as ::core::ffi::c_int
                * 256 as ::core::ffi::c_int
                + *src.offset(1 as ::core::ffi::c_int as isize) as ::core::ffi::c_int)
                as isize,
        ) as ::core::ffi::c_int;
        let c2rust_fresh101 = dest4;
        dest4 = dest4.offset(1);
        *c2rust_fresh101 = c as byte;
        let c2rust_fresh102 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh102 = *c2rust_fresh101;
        let c2rust_fresh103 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh103 = *c2rust_fresh102;
        let c2rust_fresh104 = dest;
        dest = dest.offset(1);
        *c2rust_fresh104 = *c2rust_fresh103;
        c = *src.offset(1 as ::core::ffi::c_int as isize) as ::core::ffi::c_int;
        let c2rust_fresh105 = dest4;
        dest4 = dest4.offset(1);
        *c2rust_fresh105 = c as byte;
        let c2rust_fresh106 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh106 = *c2rust_fresh105;
        let c2rust_fresh107 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh107 = *c2rust_fresh106;
        let c2rust_fresh108 = dest;
        dest = dest.offset(1);
        *c2rust_fresh108 = *c2rust_fresh107;
        let c2rust_fresh109 = dest4;
        dest4 = dest4.offset(1);
        *c2rust_fresh109 = c as byte;
        let c2rust_fresh110 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh110 = *c2rust_fresh109;
        let c2rust_fresh111 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh111 = *c2rust_fresh110;
        let c2rust_fresh112 = dest;
        dest = dest.offset(1);
        *c2rust_fresh112 = *c2rust_fresh111;
        c = *stretch_tables[1 as ::core::ffi::c_int as usize].offset(
            (*src.offset(1 as ::core::ffi::c_int as isize) as ::core::ffi::c_int
                * 256 as ::core::ffi::c_int
                + *src.offset(2 as ::core::ffi::c_int as isize) as ::core::ffi::c_int)
                as isize,
        ) as ::core::ffi::c_int;
        let c2rust_fresh113 = dest4;
        dest4 = dest4.offset(1);
        *c2rust_fresh113 = c as byte;
        let c2rust_fresh114 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh114 = *c2rust_fresh113;
        let c2rust_fresh115 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh115 = *c2rust_fresh114;
        let c2rust_fresh116 = dest;
        dest = dest.offset(1);
        *c2rust_fresh116 = *c2rust_fresh115;
        c = *src.offset(2 as ::core::ffi::c_int as isize) as ::core::ffi::c_int;
        let c2rust_fresh117 = dest4;
        dest4 = dest4.offset(1);
        *c2rust_fresh117 = c as byte;
        let c2rust_fresh118 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh118 = *c2rust_fresh117;
        let c2rust_fresh119 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh119 = *c2rust_fresh118;
        let c2rust_fresh120 = dest;
        dest = dest.offset(1);
        *c2rust_fresh120 = *c2rust_fresh119;
        let c2rust_fresh121 = dest4;
        dest4 = dest4.offset(1);
        *c2rust_fresh121 = c as byte;
        let c2rust_fresh122 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh122 = *c2rust_fresh121;
        let c2rust_fresh123 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh123 = *c2rust_fresh122;
        let c2rust_fresh124 = dest;
        dest = dest.offset(1);
        *c2rust_fresh124 = *c2rust_fresh123;
        c = *stretch_tables[1 as ::core::ffi::c_int as usize].offset(
            (*src.offset(3 as ::core::ffi::c_int as isize) as ::core::ffi::c_int
                * 256 as ::core::ffi::c_int
                + *src.offset(2 as ::core::ffi::c_int as isize) as ::core::ffi::c_int)
                as isize,
        ) as ::core::ffi::c_int;
        let c2rust_fresh125 = dest4;
        dest4 = dest4.offset(1);
        *c2rust_fresh125 = c as byte;
        let c2rust_fresh126 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh126 = *c2rust_fresh125;
        let c2rust_fresh127 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh127 = *c2rust_fresh126;
        let c2rust_fresh128 = dest;
        dest = dest.offset(1);
        *c2rust_fresh128 = *c2rust_fresh127;
        c = *src.offset(3 as ::core::ffi::c_int as isize) as ::core::ffi::c_int;
        let c2rust_fresh129 = dest4;
        dest4 = dest4.offset(1);
        *c2rust_fresh129 = c as byte;
        let c2rust_fresh130 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh130 = *c2rust_fresh129;
        let c2rust_fresh131 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh131 = *c2rust_fresh130;
        let c2rust_fresh132 = dest;
        dest = dest.offset(1);
        *c2rust_fresh132 = *c2rust_fresh131;
        let c2rust_fresh133 = dest4;
        dest4 = dest4.offset(1);
        *c2rust_fresh133 = c as byte;
        let c2rust_fresh134 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh134 = *c2rust_fresh133;
        let c2rust_fresh135 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh135 = *c2rust_fresh134;
        let c2rust_fresh136 = dest;
        dest = dest.offset(1);
        *c2rust_fresh136 = *c2rust_fresh135;
        c = *stretch_tables[0 as ::core::ffi::c_int as usize].offset(
            (*src.offset(4 as ::core::ffi::c_int as isize) as ::core::ffi::c_int
                * 256 as ::core::ffi::c_int
                + *src.offset(3 as ::core::ffi::c_int as isize) as ::core::ffi::c_int)
                as isize,
        ) as ::core::ffi::c_int;
        let c2rust_fresh137 = dest4;
        dest4 = dest4.offset(1);
        *c2rust_fresh137 = c as byte;
        let c2rust_fresh138 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh138 = *c2rust_fresh137;
        let c2rust_fresh139 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh139 = *c2rust_fresh138;
        let c2rust_fresh140 = dest;
        dest = dest.offset(1);
        *c2rust_fresh140 = *c2rust_fresh139;
        c = *src.offset(4 as ::core::ffi::c_int as isize) as ::core::ffi::c_int;
        let c2rust_fresh141 = dest4;
        dest4 = dest4.offset(1);
        *c2rust_fresh141 = c as byte;
        let c2rust_fresh142 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh142 = *c2rust_fresh141;
        let c2rust_fresh143 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh143 = *c2rust_fresh142;
        let c2rust_fresh144 = dest;
        dest = dest.offset(1);
        *c2rust_fresh144 = *c2rust_fresh143;
        let c2rust_fresh145 = dest4;
        dest4 = dest4.offset(1);
        *c2rust_fresh145 = c as byte;
        let c2rust_fresh146 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh146 = *c2rust_fresh145;
        let c2rust_fresh147 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh147 = *c2rust_fresh146;
        let c2rust_fresh148 = dest;
        dest = dest.offset(1);
        *c2rust_fresh148 = *c2rust_fresh147;
        let c2rust_fresh149 = dest4;
        dest4 = dest4.offset(1);
        *c2rust_fresh149 = c as byte;
        let c2rust_fresh150 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh150 = *c2rust_fresh149;
        let c2rust_fresh151 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh151 = *c2rust_fresh150;
        let c2rust_fresh152 = dest;
        dest = dest.offset(1);
        *c2rust_fresh152 = *c2rust_fresh151;
        x += 5 as ::core::ffi::c_int;
        src = src.offset(5 as ::core::ffi::c_int as isize);
    }
}
unsafe extern "C" fn I_Squash4x(
    mut x1: ::core::ffi::c_int,
    mut y1: ::core::ffi::c_int,
    mut x2: ::core::ffi::c_int,
    mut y2: ::core::ffi::c_int,
) -> boolean {
    let mut bufp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut y: ::core::ffi::c_int = 0;
    if x1 != 0 as ::core::ffi::c_int
        || y1 != 0 as ::core::ffi::c_int
        || x2 != SCREENWIDTH
        || y2 != SCREENHEIGHT
    {
        return false_0 as boolean;
    }
    bufp = src_buffer;
    screenp = dest_buffer;
    y = 0 as ::core::ffi::c_int;
    while y < SCREENHEIGHT {
        WriteSquashedLine4x(screenp, bufp);
        screenp = screenp.offset((dest_pitch * 4 as ::core::ffi::c_int) as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        y += 1;
    }
    return true_0 as boolean;
}
#[no_mangle]
pub static mut mode_squash_4x: screen_mode_t = screen_mode_t {
    width: SCREENWIDTH_4_3 * 4 as ::core::ffi::c_int,
    height: SCREENHEIGHT * 4 as ::core::ffi::c_int,
    InitMode: Some(I_InitStretchTables as unsafe extern "C" fn(*mut byte) -> ()),
    DrawScreen: Some(
        I_Squash4x
            as unsafe extern "C" fn(
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
            ) -> boolean,
    ),
    poor_quality: false_0 as boolean,
};
#[inline]
unsafe extern "C" fn WriteSquashedLine5x(mut dest: *mut byte, mut src: *mut byte) {
    let mut x: ::core::ffi::c_int = 0;
    let mut c: ::core::ffi::c_int = 0;
    let mut dest2: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut dest3: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut dest4: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut dest5: *mut byte = ::core::ptr::null_mut::<byte>();
    dest2 = dest.offset(dest_pitch as isize);
    dest3 = dest.offset((dest_pitch * 2 as ::core::ffi::c_int) as isize);
    dest4 = dest.offset((dest_pitch * 3 as ::core::ffi::c_int) as isize);
    dest5 = dest.offset((dest_pitch * 4 as ::core::ffi::c_int) as isize);
    x = 0 as ::core::ffi::c_int;
    while x < SCREENWIDTH {
        let c2rust_fresh153 = src;
        src = src.offset(1);
        c = *c2rust_fresh153 as ::core::ffi::c_int;
        let c2rust_fresh154 = dest5;
        dest5 = dest5.offset(1);
        *c2rust_fresh154 = c as byte;
        let c2rust_fresh155 = dest4;
        dest4 = dest4.offset(1);
        *c2rust_fresh155 = *c2rust_fresh154;
        let c2rust_fresh156 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh156 = *c2rust_fresh155;
        let c2rust_fresh157 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh157 = *c2rust_fresh156;
        let c2rust_fresh158 = dest;
        dest = dest.offset(1);
        *c2rust_fresh158 = *c2rust_fresh157;
        let c2rust_fresh159 = dest5;
        dest5 = dest5.offset(1);
        *c2rust_fresh159 = c as byte;
        let c2rust_fresh160 = dest4;
        dest4 = dest4.offset(1);
        *c2rust_fresh160 = *c2rust_fresh159;
        let c2rust_fresh161 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh161 = *c2rust_fresh160;
        let c2rust_fresh162 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh162 = *c2rust_fresh161;
        let c2rust_fresh163 = dest;
        dest = dest.offset(1);
        *c2rust_fresh163 = *c2rust_fresh162;
        let c2rust_fresh164 = dest5;
        dest5 = dest5.offset(1);
        *c2rust_fresh164 = c as byte;
        let c2rust_fresh165 = dest4;
        dest4 = dest4.offset(1);
        *c2rust_fresh165 = *c2rust_fresh164;
        let c2rust_fresh166 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh166 = *c2rust_fresh165;
        let c2rust_fresh167 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh167 = *c2rust_fresh166;
        let c2rust_fresh168 = dest;
        dest = dest.offset(1);
        *c2rust_fresh168 = *c2rust_fresh167;
        let c2rust_fresh169 = dest5;
        dest5 = dest5.offset(1);
        *c2rust_fresh169 = c as byte;
        let c2rust_fresh170 = dest4;
        dest4 = dest4.offset(1);
        *c2rust_fresh170 = *c2rust_fresh169;
        let c2rust_fresh171 = dest3;
        dest3 = dest3.offset(1);
        *c2rust_fresh171 = *c2rust_fresh170;
        let c2rust_fresh172 = dest2;
        dest2 = dest2.offset(1);
        *c2rust_fresh172 = *c2rust_fresh171;
        let c2rust_fresh173 = dest;
        dest = dest.offset(1);
        *c2rust_fresh173 = *c2rust_fresh172;
        x += 1;
    }
}
unsafe extern "C" fn I_Squash5x(
    mut x1: ::core::ffi::c_int,
    mut y1: ::core::ffi::c_int,
    mut x2: ::core::ffi::c_int,
    mut y2: ::core::ffi::c_int,
) -> boolean {
    let mut bufp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut screenp: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut y: ::core::ffi::c_int = 0;
    if x1 != 0 as ::core::ffi::c_int
        || y1 != 0 as ::core::ffi::c_int
        || x2 != SCREENWIDTH
        || y2 != SCREENHEIGHT
    {
        return false_0 as boolean;
    }
    bufp = src_buffer;
    screenp = dest_buffer;
    y = 0 as ::core::ffi::c_int;
    while y < SCREENHEIGHT {
        WriteSquashedLine5x(screenp, bufp);
        screenp = screenp.offset((dest_pitch * 5 as ::core::ffi::c_int) as isize);
        bufp = bufp.offset(SCREENWIDTH as isize);
        y += 1;
    }
    return true_0 as boolean;
}
#[no_mangle]
pub static mut mode_squash_5x: screen_mode_t = screen_mode_t {
    width: SCREENWIDTH_4_3 * 5 as ::core::ffi::c_int,
    height: SCREENHEIGHT * 5 as ::core::ffi::c_int,
    InitMode: Some(I_InitStretchTables as unsafe extern "C" fn(*mut byte) -> ()),
    DrawScreen: Some(
        I_Squash5x
            as unsafe extern "C" fn(
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
                ::core::ffi::c_int,
            ) -> boolean,
    ),
    poor_quality: false_0 as boolean,
};
pub const __INT_MAX__: ::core::ffi::c_int = 2147483647 as ::core::ffi::c_int;
