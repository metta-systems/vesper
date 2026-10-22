extern "C" {
    fn memcpy(
        __dest: *mut ::core::ffi::c_void,
        __src: *const ::core::ffi::c_void,
        __n: size_t,
    ) -> *mut ::core::ffi::c_void;
    fn Z_Malloc(
        size: ::core::ffi::c_int,
        tag: ::core::ffi::c_int,
        ptr: *mut ::core::ffi::c_void,
    ) -> *mut ::core::ffi::c_void;
    fn Z_Free(ptr: *mut ::core::ffi::c_void);
    fn I_ReadScreen(scr: *mut byte);
    static mut I_VideoBuffer: *mut byte;
    fn V_DrawBlock(
        x: ::core::ffi::c_int,
        y_0: ::core::ffi::c_int,
        width: ::core::ffi::c_int,
        height: ::core::ffi::c_int,
        src: *mut byte,
    );
    fn V_MarkRect(
        x: ::core::ffi::c_int,
        y_0: ::core::ffi::c_int,
        width: ::core::ffi::c_int,
        height: ::core::ffi::c_int,
    );
    fn M_Random() -> ::core::ffi::c_int;
}
pub type size_t = usize;
pub type __uint8_t = u8;
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
pub type uint8_t = __uint8_t;
pub type boolean = ::core::ffi::c_uint;
pub type byte = uint8_t;
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
pub const true_0: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
pub const false_0: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
pub const SCREENWIDTH: ::core::ffi::c_int = 320 as ::core::ffi::c_int;
pub const SCREENHEIGHT: ::core::ffi::c_int = 200 as ::core::ffi::c_int;
static mut go: boolean = 0 as boolean;
static mut wipe_scr_start: *mut byte = ::core::ptr::null_mut::<byte>();
static mut wipe_scr_end: *mut byte = ::core::ptr::null_mut::<byte>();
static mut wipe_scr: *mut byte = ::core::ptr::null_mut::<byte>();
#[no_mangle]
pub unsafe extern "C" fn wipe_shittyColMajorXform(
    mut array: *mut ::core::ffi::c_short,
    mut width: ::core::ffi::c_int,
    mut height: ::core::ffi::c_int,
) {
    let mut x: ::core::ffi::c_int = 0;
    let mut y_0: ::core::ffi::c_int = 0;
    let mut dest: *mut ::core::ffi::c_short = ::core::ptr::null_mut::<::core::ffi::c_short>();
    dest = Z_Malloc(
        width * height * 2 as ::core::ffi::c_int,
        PU_STATIC as ::core::ffi::c_int,
        ::core::ptr::null_mut::<::core::ffi::c_void>(),
    ) as *mut ::core::ffi::c_short;
    y_0 = 0 as ::core::ffi::c_int;
    while y_0 < height {
        x = 0 as ::core::ffi::c_int;
        while x < width {
            *dest.offset((x * height + y_0) as isize) = *array.offset((y_0 * width + x) as isize);
            x += 1;
        }
        y_0 += 1;
    }
    memcpy(
        array as *mut ::core::ffi::c_void,
        dest as *const ::core::ffi::c_void,
        (width * height * 2 as ::core::ffi::c_int) as size_t,
    );
    Z_Free(dest as *mut ::core::ffi::c_void);
}
#[no_mangle]
pub unsafe extern "C" fn wipe_initColorXForm(
    mut width: ::core::ffi::c_int,
    mut height: ::core::ffi::c_int,
    mut ticks: ::core::ffi::c_int,
) -> ::core::ffi::c_int {
    memcpy(
        wipe_scr as *mut ::core::ffi::c_void,
        wipe_scr_start as *const ::core::ffi::c_void,
        (width * height) as size_t,
    );
    return 0 as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn wipe_doColorXForm(
    mut width: ::core::ffi::c_int,
    mut height: ::core::ffi::c_int,
    mut ticks: ::core::ffi::c_int,
) -> ::core::ffi::c_int {
    let mut changed: boolean = 0;
    let mut w: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut e: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut newval: ::core::ffi::c_int = 0;
    changed = false_0 as boolean;
    w = wipe_scr;
    e = wipe_scr_end;
    while w != wipe_scr.offset((width * height) as isize) {
        if *w as ::core::ffi::c_int != *e as ::core::ffi::c_int {
            if *w as ::core::ffi::c_int > *e as ::core::ffi::c_int {
                newval = *w as ::core::ffi::c_int - ticks;
                if newval < *e as ::core::ffi::c_int {
                    *w = *e;
                } else {
                    *w = newval as byte;
                }
                changed = true_0 as boolean;
            } else if (*w as ::core::ffi::c_int) < *e as ::core::ffi::c_int {
                newval = *w as ::core::ffi::c_int + ticks;
                if newval > *e as ::core::ffi::c_int {
                    *w = *e;
                } else {
                    *w = newval as byte;
                }
                changed = true_0 as boolean;
            }
        }
        w = w.offset(1);
        e = e.offset(1);
    }
    return (changed == 0) as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn wipe_exitColorXForm(
    mut width: ::core::ffi::c_int,
    mut height: ::core::ffi::c_int,
    mut ticks: ::core::ffi::c_int,
) -> ::core::ffi::c_int {
    return 0 as ::core::ffi::c_int;
}
static mut y: *mut ::core::ffi::c_int = ::core::ptr::null_mut::<::core::ffi::c_int>();
#[no_mangle]
pub unsafe extern "C" fn wipe_initMelt(
    mut width: ::core::ffi::c_int,
    mut height: ::core::ffi::c_int,
    mut ticks: ::core::ffi::c_int,
) -> ::core::ffi::c_int {
    let mut i: ::core::ffi::c_int = 0;
    let mut r: ::core::ffi::c_int = 0;
    memcpy(
        wipe_scr as *mut ::core::ffi::c_void,
        wipe_scr_start as *const ::core::ffi::c_void,
        (width * height) as size_t,
    );
    wipe_shittyColMajorXform(
        wipe_scr_start as *mut ::core::ffi::c_short,
        width / 2 as ::core::ffi::c_int,
        height,
    );
    wipe_shittyColMajorXform(
        wipe_scr_end as *mut ::core::ffi::c_short,
        width / 2 as ::core::ffi::c_int,
        height,
    );
    y = Z_Malloc(
        (width as usize).wrapping_mul(::core::mem::size_of::<::core::ffi::c_int>() as usize)
            as ::core::ffi::c_int,
        PU_STATIC as ::core::ffi::c_int,
        ::core::ptr::null_mut::<::core::ffi::c_void>(),
    ) as *mut ::core::ffi::c_int;
    *y.offset(0 as ::core::ffi::c_int as isize) = -(M_Random() % 16 as ::core::ffi::c_int);
    i = 1 as ::core::ffi::c_int;
    while i < width {
        r = M_Random() % 3 as ::core::ffi::c_int - 1 as ::core::ffi::c_int;
        *y.offset(i as isize) = *y.offset((i - 1 as ::core::ffi::c_int) as isize) + r;
        if *y.offset(i as isize) > 0 as ::core::ffi::c_int {
            *y.offset(i as isize) = 0 as ::core::ffi::c_int;
        } else if *y.offset(i as isize) == -16 as ::core::ffi::c_int {
            *y.offset(i as isize) = -15 as ::core::ffi::c_int;
        }
        i += 1;
    }
    return 0 as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn wipe_doMelt(
    mut width: ::core::ffi::c_int,
    mut height: ::core::ffi::c_int,
    mut ticks: ::core::ffi::c_int,
) -> ::core::ffi::c_int {
    let mut i: ::core::ffi::c_int = 0;
    let mut j: ::core::ffi::c_int = 0;
    let mut dy: ::core::ffi::c_int = 0;
    let mut idx: ::core::ffi::c_int = 0;
    let mut s: *mut ::core::ffi::c_short = ::core::ptr::null_mut::<::core::ffi::c_short>();
    let mut d: *mut ::core::ffi::c_short = ::core::ptr::null_mut::<::core::ffi::c_short>();
    let mut done: boolean = true_0 as boolean;
    width /= 2 as ::core::ffi::c_int;
    loop {
        let c2rust_fresh0 = ticks;
        ticks = ticks - 1;
        if !(c2rust_fresh0 != 0) {
            break;
        }
        i = 0 as ::core::ffi::c_int;
        while i < width {
            if *y.offset(i as isize) < 0 as ::core::ffi::c_int {
                let ref mut c2rust_fresh1 = *y.offset(i as isize);
                *c2rust_fresh1 += 1;
                done = false_0 as boolean;
            } else if *y.offset(i as isize) < height {
                dy = if *y.offset(i as isize) < 16 as ::core::ffi::c_int {
                    *y.offset(i as isize) + 1 as ::core::ffi::c_int
                } else {
                    8 as ::core::ffi::c_int
                };
                if *y.offset(i as isize) + dy >= height {
                    dy = height - *y.offset(i as isize);
                }
                s = (wipe_scr_end as *mut ::core::ffi::c_short)
                    .offset((i * height + *y.offset(i as isize)) as isize)
                    as *mut ::core::ffi::c_short;
                d = (wipe_scr as *mut ::core::ffi::c_short)
                    .offset((*y.offset(i as isize) * width + i) as isize)
                    as *mut ::core::ffi::c_short;
                idx = 0 as ::core::ffi::c_int;
                j = dy;
                while j != 0 {
                    let c2rust_fresh2 = s;
                    s = s.offset(1);
                    *d.offset(idx as isize) = *c2rust_fresh2;
                    idx += width;
                    j -= 1;
                }
                *y.offset(i as isize) += dy;
                s = (wipe_scr_start as *mut ::core::ffi::c_short).offset((i * height) as isize)
                    as *mut ::core::ffi::c_short;
                d = (wipe_scr as *mut ::core::ffi::c_short)
                    .offset((*y.offset(i as isize) * width + i) as isize)
                    as *mut ::core::ffi::c_short;
                idx = 0 as ::core::ffi::c_int;
                j = height - *y.offset(i as isize);
                while j != 0 {
                    let c2rust_fresh3 = s;
                    s = s.offset(1);
                    *d.offset(idx as isize) = *c2rust_fresh3;
                    idx += width;
                    j -= 1;
                }
                done = false_0 as boolean;
            }
            i += 1;
        }
    }
    return done as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn wipe_exitMelt(
    mut width: ::core::ffi::c_int,
    mut height: ::core::ffi::c_int,
    mut ticks: ::core::ffi::c_int,
) -> ::core::ffi::c_int {
    Z_Free(y as *mut ::core::ffi::c_void);
    Z_Free(wipe_scr_start as *mut ::core::ffi::c_void);
    Z_Free(wipe_scr_end as *mut ::core::ffi::c_void);
    return 0 as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn wipe_StartScreen(
    mut x: ::core::ffi::c_int,
    mut y_0: ::core::ffi::c_int,
    mut width: ::core::ffi::c_int,
    mut height: ::core::ffi::c_int,
) -> ::core::ffi::c_int {
    wipe_scr_start = Z_Malloc(
        SCREENWIDTH * SCREENHEIGHT,
        PU_STATIC as ::core::ffi::c_int,
        NULL,
    ) as *mut byte;
    I_ReadScreen(wipe_scr_start);
    return 0 as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn wipe_EndScreen(
    mut x: ::core::ffi::c_int,
    mut y_0: ::core::ffi::c_int,
    mut width: ::core::ffi::c_int,
    mut height: ::core::ffi::c_int,
) -> ::core::ffi::c_int {
    wipe_scr_end = Z_Malloc(
        SCREENWIDTH * SCREENHEIGHT,
        PU_STATIC as ::core::ffi::c_int,
        NULL,
    ) as *mut byte;
    I_ReadScreen(wipe_scr_end);
    V_DrawBlock(x, y_0, width, height, wipe_scr_start);
    return 0 as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn wipe_ScreenWipe(
    mut wipeno: ::core::ffi::c_int,
    mut x: ::core::ffi::c_int,
    mut y_0: ::core::ffi::c_int,
    mut width: ::core::ffi::c_int,
    mut height: ::core::ffi::c_int,
    mut ticks: ::core::ffi::c_int,
) -> ::core::ffi::c_int {
    let mut rc: ::core::ffi::c_int = 0;
    static mut wipes: [Option<
        unsafe extern "C" fn(
            ::core::ffi::c_int,
            ::core::ffi::c_int,
            ::core::ffi::c_int,
        ) -> ::core::ffi::c_int,
    >; 6] = [
        Some(
            wipe_initColorXForm
                as unsafe extern "C" fn(
                    ::core::ffi::c_int,
                    ::core::ffi::c_int,
                    ::core::ffi::c_int,
                ) -> ::core::ffi::c_int,
        ),
        Some(
            wipe_doColorXForm
                as unsafe extern "C" fn(
                    ::core::ffi::c_int,
                    ::core::ffi::c_int,
                    ::core::ffi::c_int,
                ) -> ::core::ffi::c_int,
        ),
        Some(
            wipe_exitColorXForm
                as unsafe extern "C" fn(
                    ::core::ffi::c_int,
                    ::core::ffi::c_int,
                    ::core::ffi::c_int,
                ) -> ::core::ffi::c_int,
        ),
        Some(
            wipe_initMelt
                as unsafe extern "C" fn(
                    ::core::ffi::c_int,
                    ::core::ffi::c_int,
                    ::core::ffi::c_int,
                ) -> ::core::ffi::c_int,
        ),
        Some(
            wipe_doMelt
                as unsafe extern "C" fn(
                    ::core::ffi::c_int,
                    ::core::ffi::c_int,
                    ::core::ffi::c_int,
                ) -> ::core::ffi::c_int,
        ),
        Some(
            wipe_exitMelt
                as unsafe extern "C" fn(
                    ::core::ffi::c_int,
                    ::core::ffi::c_int,
                    ::core::ffi::c_int,
                ) -> ::core::ffi::c_int,
        ),
    ];
    if go == 0 {
        go = 1 as boolean;
        wipe_scr = I_VideoBuffer;
        Some(
            (*(&raw mut wipes
                as *mut Option<
                    unsafe extern "C" fn(
                        ::core::ffi::c_int,
                        ::core::ffi::c_int,
                        ::core::ffi::c_int,
                    ) -> ::core::ffi::c_int,
                >)
                .offset((wipeno * 3 as ::core::ffi::c_int) as isize))
            .expect("non-null function pointer"),
        )
        .expect("non-null function pointer")(width, height, ticks);
    }
    V_MarkRect(
        0 as ::core::ffi::c_int,
        0 as ::core::ffi::c_int,
        width,
        height,
    );
    rc = Some(
        (*(&raw mut wipes
            as *mut Option<
                unsafe extern "C" fn(
                    ::core::ffi::c_int,
                    ::core::ffi::c_int,
                    ::core::ffi::c_int,
                ) -> ::core::ffi::c_int,
            >)
            .offset((wipeno * 3 as ::core::ffi::c_int + 1 as ::core::ffi::c_int) as isize))
        .expect("non-null function pointer"),
    )
    .expect("non-null function pointer")(width, height, ticks);
    if rc != 0 {
        go = 0 as boolean;
        Some(
            (*(&raw mut wipes
                as *mut Option<
                    unsafe extern "C" fn(
                        ::core::ffi::c_int,
                        ::core::ffi::c_int,
                        ::core::ffi::c_int,
                    ) -> ::core::ffi::c_int,
                >)
                .offset((wipeno * 3 as ::core::ffi::c_int + 2 as ::core::ffi::c_int) as isize))
            .expect("non-null function pointer"),
        )
        .expect("non-null function pointer")(width, height, ticks);
    }
    return (go == 0) as ::core::ffi::c_int;
}
