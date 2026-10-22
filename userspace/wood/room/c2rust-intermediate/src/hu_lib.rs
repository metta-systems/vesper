extern "C" {
    fn toupper(__c: ::core::ffi::c_int) -> ::core::ffi::c_int;
    fn V_DrawPatchDirect(x: ::core::ffi::c_int, y: ::core::ffi::c_int, patch: *mut patch_t);
    static mut viewwidth: ::core::ffi::c_int;
    static mut viewheight: ::core::ffi::c_int;
    static mut viewwindowx: ::core::ffi::c_int;
    static mut viewwindowy: ::core::ffi::c_int;
    fn R_VideoErase(ofs: ::core::ffi::c_uint, count: ::core::ffi::c_int);
    static mut automapactive: boolean;
}
pub type boolean = ::core::ffi::c_uint;
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
pub struct hu_textline_t {
    pub x: ::core::ffi::c_int,
    pub y: ::core::ffi::c_int,
    pub f: *mut *mut patch_t,
    pub sc: ::core::ffi::c_int,
    pub l: [::core::ffi::c_char; 81],
    pub len: ::core::ffi::c_int,
    pub needsupdate: ::core::ffi::c_int,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct hu_stext_t {
    pub l: [hu_textline_t; 4],
    pub h: ::core::ffi::c_int,
    pub cl: ::core::ffi::c_int,
    pub on: *mut boolean,
    pub laston: boolean,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct hu_itext_t {
    pub l: hu_textline_t,
    pub lm: ::core::ffi::c_int,
    pub on: *mut boolean,
    pub laston: boolean,
}
pub const true_0: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
pub const false_0: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
pub const KEY_ENTER: ::core::ffi::c_int = 13 as ::core::ffi::c_int;
pub const KEY_BACKSPACE: ::core::ffi::c_int = 0x7f as ::core::ffi::c_int;
pub const SCREENWIDTH: ::core::ffi::c_int = 320 as ::core::ffi::c_int;
pub const HU_MAXLINELENGTH: ::core::ffi::c_int = 80 as ::core::ffi::c_int;
#[no_mangle]
pub unsafe extern "C" fn HUlib_init() {}
#[no_mangle]
pub unsafe extern "C" fn HUlib_clearTextLine(mut t: *mut hu_textline_t) {
    (*t).len = 0 as ::core::ffi::c_int;
    (*t).l[0 as ::core::ffi::c_int as usize] = 0 as ::core::ffi::c_char;
    (*t).needsupdate = true_0;
}
#[no_mangle]
pub unsafe extern "C" fn HUlib_initTextLine(
    mut t: *mut hu_textline_t,
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut f: *mut *mut patch_t,
    mut sc: ::core::ffi::c_int,
) {
    (*t).x = x;
    (*t).y = y;
    (*t).f = f;
    (*t).sc = sc;
    HUlib_clearTextLine(t);
}
#[no_mangle]
pub unsafe extern "C" fn HUlib_addCharToTextLine(
    mut t: *mut hu_textline_t,
    mut ch: ::core::ffi::c_char,
) -> boolean {
    if (*t).len == HU_MAXLINELENGTH {
        return false_0 as boolean;
    } else {
        let c2rust_fresh0 = (*t).len;
        (*t).len = (*t).len + 1;
        (*t).l[c2rust_fresh0 as usize] = ch;
        (*t).l[(*t).len as usize] = 0 as ::core::ffi::c_char;
        (*t).needsupdate = 4 as ::core::ffi::c_int;
        return true_0 as boolean;
    };
}
#[no_mangle]
pub unsafe extern "C" fn HUlib_delCharFromTextLine(mut t: *mut hu_textline_t) -> boolean {
    if (*t).len == 0 {
        return false_0 as boolean;
    } else {
        (*t).len -= 1;
        (*t).l[(*t).len as usize] = 0 as ::core::ffi::c_char;
        (*t).needsupdate = 4 as ::core::ffi::c_int;
        return true_0 as boolean;
    };
}
#[no_mangle]
pub unsafe extern "C" fn HUlib_drawTextLine(mut l: *mut hu_textline_t, mut drawcursor: boolean) {
    let mut i: ::core::ffi::c_int = 0;
    let mut w: ::core::ffi::c_int = 0;
    let mut x: ::core::ffi::c_int = 0;
    let mut c: ::core::ffi::c_uchar = 0;
    x = (*l).x;
    i = 0 as ::core::ffi::c_int;
    while i < (*l).len {
        c = toupper((*l).l[i as usize] as ::core::ffi::c_int) as ::core::ffi::c_uchar;
        if c as ::core::ffi::c_int != ' ' as ::core::ffi::c_int
            && c as ::core::ffi::c_int >= (*l).sc
            && c as ::core::ffi::c_int <= '_' as ::core::ffi::c_int
        {
            w = (**(*l).f.offset((c as ::core::ffi::c_int - (*l).sc) as isize)).width
                as ::core::ffi::c_int;
            if x + w > SCREENWIDTH {
                break;
            }
            V_DrawPatchDirect(
                x,
                (*l).y,
                *(*l).f.offset((c as ::core::ffi::c_int - (*l).sc) as isize),
            );
            x += w;
        } else {
            x += 4 as ::core::ffi::c_int;
            if x >= SCREENWIDTH {
                break;
            }
        }
        i += 1;
    }
    if drawcursor != 0
        && x + (**(*l)
            .f
            .offset(('_' as ::core::ffi::c_int - (*l).sc) as isize))
        .width as ::core::ffi::c_int
            <= SCREENWIDTH
    {
        V_DrawPatchDirect(
            x,
            (*l).y,
            *(*l)
                .f
                .offset(('_' as ::core::ffi::c_int - (*l).sc) as isize),
        );
    }
}
#[no_mangle]
pub unsafe extern "C" fn HUlib_eraseTextLine(mut l: *mut hu_textline_t) {
    let mut lh: ::core::ffi::c_int = 0;
    let mut y: ::core::ffi::c_int = 0;
    let mut yoffset: ::core::ffi::c_int = 0;
    if automapactive == 0 && viewwindowx != 0 && (*l).needsupdate != 0 {
        lh = (**(*l).f.offset(0 as ::core::ffi::c_int as isize)).height as ::core::ffi::c_int
            + 1 as ::core::ffi::c_int;
        y = (*l).y;
        yoffset = y * SCREENWIDTH;
        while y < (*l).y + lh {
            if y < viewwindowy || y >= viewwindowy + viewheight {
                R_VideoErase(yoffset as ::core::ffi::c_uint, SCREENWIDTH);
            } else {
                R_VideoErase(yoffset as ::core::ffi::c_uint, viewwindowx);
                R_VideoErase(
                    (yoffset + viewwindowx + viewwidth) as ::core::ffi::c_uint,
                    viewwindowx,
                );
            }
            y += 1;
            yoffset += SCREENWIDTH;
        }
    }
    if (*l).needsupdate != 0 {
        (*l).needsupdate -= 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn HUlib_initSText(
    mut s: *mut hu_stext_t,
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut h: ::core::ffi::c_int,
    mut font: *mut *mut patch_t,
    mut startchar: ::core::ffi::c_int,
    mut on: *mut boolean,
) {
    let mut i: ::core::ffi::c_int = 0;
    (*s).h = h;
    (*s).on = on;
    (*s).laston = true_0 as boolean;
    (*s).cl = 0 as ::core::ffi::c_int;
    i = 0 as ::core::ffi::c_int;
    while i < h {
        HUlib_initTextLine(
            (&raw mut (*s).l as *mut hu_textline_t).offset(i as isize) as *mut hu_textline_t,
            x,
            y - i
                * ((**font.offset(0 as ::core::ffi::c_int as isize)).height as ::core::ffi::c_int
                    + 1 as ::core::ffi::c_int),
            font,
            startchar,
        );
        i += 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn HUlib_addLineToSText(mut s: *mut hu_stext_t) {
    let mut i: ::core::ffi::c_int = 0;
    (*s).cl += 1;
    if (*s).cl == (*s).h {
        (*s).cl = 0 as ::core::ffi::c_int;
    }
    HUlib_clearTextLine(
        (&raw mut (*s).l as *mut hu_textline_t).offset((*s).cl as isize) as *mut hu_textline_t,
    );
    i = 0 as ::core::ffi::c_int;
    while i < (*s).h {
        (*s).l[i as usize].needsupdate = 4 as ::core::ffi::c_int;
        i += 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn HUlib_addMessageToSText(
    mut s: *mut hu_stext_t,
    mut prefix: *mut ::core::ffi::c_char,
    mut msg: *mut ::core::ffi::c_char,
) {
    HUlib_addLineToSText(s);
    if !prefix.is_null() {
        while *prefix != 0 {
            let c2rust_fresh1 = prefix;
            prefix = prefix.offset(1);
            HUlib_addCharToTextLine(
                (&raw mut (*s).l as *mut hu_textline_t).offset((*s).cl as isize)
                    as *mut hu_textline_t,
                *c2rust_fresh1,
            );
        }
    }
    while *msg != 0 {
        let c2rust_fresh2 = msg;
        msg = msg.offset(1);
        HUlib_addCharToTextLine(
            (&raw mut (*s).l as *mut hu_textline_t).offset((*s).cl as isize) as *mut hu_textline_t,
            *c2rust_fresh2,
        );
    }
}
#[no_mangle]
pub unsafe extern "C" fn HUlib_drawSText(mut s: *mut hu_stext_t) {
    let mut i: ::core::ffi::c_int = 0;
    let mut idx: ::core::ffi::c_int = 0;
    let mut l: *mut hu_textline_t = ::core::ptr::null_mut::<hu_textline_t>();
    if *(*s).on == 0 {
        return;
    }
    i = 0 as ::core::ffi::c_int;
    while i < (*s).h {
        idx = (*s).cl - i;
        if idx < 0 as ::core::ffi::c_int {
            idx += (*s).h;
        }
        l = (&raw mut (*s).l as *mut hu_textline_t).offset(idx as isize) as *mut hu_textline_t;
        HUlib_drawTextLine(l, false_0 as boolean);
        i += 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn HUlib_eraseSText(mut s: *mut hu_stext_t) {
    let mut i: ::core::ffi::c_int = 0;
    i = 0 as ::core::ffi::c_int;
    while i < (*s).h {
        if (*s).laston != 0 && *(*s).on == 0 {
            (*s).l[i as usize].needsupdate = 4 as ::core::ffi::c_int;
        }
        HUlib_eraseTextLine(
            (&raw mut (*s).l as *mut hu_textline_t).offset(i as isize) as *mut hu_textline_t
        );
        i += 1;
    }
    (*s).laston = *(*s).on;
}
#[no_mangle]
pub unsafe extern "C" fn HUlib_initIText(
    mut it: *mut hu_itext_t,
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut font: *mut *mut patch_t,
    mut startchar: ::core::ffi::c_int,
    mut on: *mut boolean,
) {
    (*it).lm = 0 as ::core::ffi::c_int;
    (*it).on = on;
    (*it).laston = true_0 as boolean;
    HUlib_initTextLine(&raw mut (*it).l, x, y, font, startchar);
}
#[no_mangle]
pub unsafe extern "C" fn HUlib_delCharFromIText(mut it: *mut hu_itext_t) {
    if (*it).l.len != (*it).lm {
        HUlib_delCharFromTextLine(&raw mut (*it).l);
    }
}
#[no_mangle]
pub unsafe extern "C" fn HUlib_eraseLineFromIText(mut it: *mut hu_itext_t) {
    while (*it).lm != (*it).l.len {
        HUlib_delCharFromTextLine(&raw mut (*it).l);
    }
}
#[no_mangle]
pub unsafe extern "C" fn HUlib_resetIText(mut it: *mut hu_itext_t) {
    (*it).lm = 0 as ::core::ffi::c_int;
    HUlib_clearTextLine(&raw mut (*it).l);
}
#[no_mangle]
pub unsafe extern "C" fn HUlib_addPrefixToIText(
    mut it: *mut hu_itext_t,
    mut str: *mut ::core::ffi::c_char,
) {
    while *str != 0 {
        let c2rust_fresh3 = str;
        str = str.offset(1);
        HUlib_addCharToTextLine(&raw mut (*it).l, *c2rust_fresh3);
    }
    (*it).lm = (*it).l.len;
}
#[no_mangle]
pub unsafe extern "C" fn HUlib_keyInIText(
    mut it: *mut hu_itext_t,
    mut ch: ::core::ffi::c_uchar,
) -> boolean {
    ch = toupper(ch as ::core::ffi::c_int) as ::core::ffi::c_uchar;
    if ch as ::core::ffi::c_int >= ' ' as ::core::ffi::c_int
        && ch as ::core::ffi::c_int <= '_' as ::core::ffi::c_int
    {
        HUlib_addCharToTextLine(&raw mut (*it).l, ch as ::core::ffi::c_char);
    } else if ch as ::core::ffi::c_int == KEY_BACKSPACE {
        HUlib_delCharFromIText(it);
    } else if ch as ::core::ffi::c_int != KEY_ENTER {
        return false_0 as boolean;
    }
    return true_0 as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn HUlib_drawIText(mut it: *mut hu_itext_t) {
    let mut l: *mut hu_textline_t = &raw mut (*it).l;
    if *(*it).on == 0 {
        return;
    }
    HUlib_drawTextLine(l, true_0 as boolean);
}
#[no_mangle]
pub unsafe extern "C" fn HUlib_eraseIText(mut it: *mut hu_itext_t) {
    if (*it).laston != 0 && *(*it).on == 0 {
        (*it).l.needsupdate = 4 as ::core::ffi::c_int;
    }
    HUlib_eraseTextLine(&raw mut (*it).l);
    (*it).laston = *(*it).on;
}
