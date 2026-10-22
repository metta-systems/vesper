extern "C" {
    fn toupper(__c: ::core::ffi::c_int) -> ::core::ffi::c_int;
    fn printf(__format: *const ::core::ffi::c_char, ...) -> ::core::ffi::c_int;
    fn snprintf(
        __s: *mut ::core::ffi::c_char,
        __maxlen: size_t,
        __format: *const ::core::ffi::c_char,
        ...
    ) -> ::core::ffi::c_int;
    fn calloc(__nmemb: size_t, __size: size_t) -> *mut ::core::ffi::c_void;
    fn free(__ptr: *mut ::core::ffi::c_void);
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
    fn strncpy(
        __dest: *mut ::core::ffi::c_char,
        __src: *const ::core::ffi::c_char,
        __n: size_t,
    ) -> *mut ::core::ffi::c_char;
    fn strncmp(
        __s1: *const ::core::ffi::c_char,
        __s2: *const ::core::ffi::c_char,
        __n: size_t,
    ) -> ::core::ffi::c_int;
    fn strlen(__s: *const ::core::ffi::c_char) -> size_t;
    fn strcasecmp(
        __s1: *const ::core::ffi::c_char,
        __s2: *const ::core::ffi::c_char,
    ) -> ::core::ffi::c_int;
    fn strncasecmp(
        __s1: *const ::core::ffi::c_char,
        __s2: *const ::core::ffi::c_char,
        __n: size_t,
    ) -> ::core::ffi::c_int;
    fn D_GameMissionString(mission: GameMission_t) -> *mut ::core::ffi::c_char;
    fn D_SuggestGameName(mission: GameMission_t, mode: GameMode_t) -> *mut ::core::ffi::c_char;
    fn I_ErrorV(msg: *const ::core::ffi::c_char);
    fn I_BeginRead();
    fn I_EndRead();
    fn M_ExtractFileBase(path: *mut ::core::ffi::c_char, dest: *mut ::core::ffi::c_char);
    fn Z_Malloc(
        size: ::core::ffi::c_int,
        tag: ::core::ffi::c_int,
        ptr: *mut ::core::ffi::c_void,
    ) -> *mut ::core::ffi::c_void;
    fn Z_Free(ptr: *mut ::core::ffi::c_void);
    fn Z_ChangeTag2(
        ptr: *mut ::core::ffi::c_void,
        tag: ::core::ffi::c_int,
        file: *mut ::core::ffi::c_char,
        line: ::core::ffi::c_int,
    );
    fn Z_ChangeUser(ptr: *mut ::core::ffi::c_void, user: *mut *mut ::core::ffi::c_void);
    fn W_OpenFile(path: *mut ::core::ffi::c_char) -> *mut wad_file_t;
    fn W_Read(
        wad: *mut wad_file_t,
        offset: ::core::ffi::c_uint,
        buffer: *mut ::core::ffi::c_void,
        buffer_len: size_t,
    ) -> size_t;
}
pub type __uint8_t = u8;
pub type size_t = usize;
pub type uint8_t = __uint8_t;
pub type byte = uint8_t;
pub type GameMission_t = ::core::ffi::c_uint;
pub const none: GameMission_t = 9;
pub const strife: GameMission_t = 8;
pub const hexen: GameMission_t = 7;
pub const heretic: GameMission_t = 6;
pub const pack_hacx: GameMission_t = 5;
pub const pack_chex: GameMission_t = 4;
pub const pack_plut: GameMission_t = 3;
pub const pack_tnt: GameMission_t = 2;
pub const doom2: GameMission_t = 1;
pub const doom: GameMission_t = 0;
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
#[derive(Copy, Clone)]
#[repr(C)]
pub struct _wad_file_s {
    pub file_class: *mut wad_file_class_t,
    pub mapped: *mut byte,
    pub length: ::core::ffi::c_uint,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct wad_file_class_t {
    pub OpenFile: Option<unsafe extern "C" fn(*mut ::core::ffi::c_char) -> *mut wad_file_t>,
    pub CloseFile: Option<unsafe extern "C" fn(*mut wad_file_t) -> ()>,
    pub Read: Option<
        unsafe extern "C" fn(
            *mut wad_file_t,
            ::core::ffi::c_uint,
            *mut ::core::ffi::c_void,
            size_t,
        ) -> size_t,
    >,
}
pub type wad_file_t = _wad_file_s;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct lumpinfo_s {
    pub name: [::core::ffi::c_char; 8],
    pub wad_file: *mut wad_file_t,
    pub position: ::core::ffi::c_int,
    pub size: ::core::ffi::c_int,
    pub cache: *mut ::core::ffi::c_void,
    pub next: *mut lumpinfo_t,
}
pub type lumpinfo_t = lumpinfo_s;
#[derive(Copy, Clone)]
#[repr(C, packed)]
pub struct filelump_t {
    pub filepos: ::core::ffi::c_int,
    pub size: ::core::ffi::c_int,
    pub name: [::core::ffi::c_char; 8],
}
#[derive(Copy, Clone)]
#[repr(C, packed)]
pub struct wadinfo_t {
    pub identification: [::core::ffi::c_char; 4],
    pub numlumps: ::core::ffi::c_int,
    pub infotableofs: ::core::ffi::c_int,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct C2Rust_Unnamed_0 {
    pub mission: GameMission_t,
    pub lumpname: *mut ::core::ffi::c_char,
}
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
pub const PROGRAM_PREFIX: [::core::ffi::c_char; 12] =
    unsafe { ::core::mem::transmute::<[u8; 12], [::core::ffi::c_char; 12]>(*b"doomgeneric\0") };
#[no_mangle]
pub static mut lumpinfo: *mut lumpinfo_t = ::core::ptr::null_mut::<lumpinfo_t>();
#[no_mangle]
pub static mut numlumps: ::core::ffi::c_uint = 0 as ::core::ffi::c_uint;
static mut lumphash: *mut *mut lumpinfo_t = ::core::ptr::null_mut::<*mut lumpinfo_t>();
#[no_mangle]
pub unsafe extern "C" fn W_LumpNameHash(mut s: *const ::core::ffi::c_char) -> ::core::ffi::c_uint {
    let mut result: ::core::ffi::c_uint = 5381 as ::core::ffi::c_uint;
    let mut i: ::core::ffi::c_uint = 0;
    i = 0 as ::core::ffi::c_uint;
    while i < 8 as ::core::ffi::c_uint
        && *s.offset(i as isize) as ::core::ffi::c_int != '\0' as ::core::ffi::c_int
    {
        result = result << 5 as ::core::ffi::c_int
            ^ result
            ^ toupper(*s.offset(i as isize) as ::core::ffi::c_int) as ::core::ffi::c_uint;
        i = i.wrapping_add(1);
    }
    return result;
}
unsafe extern "C" fn ExtendLumpInfo(mut newnumlumps: ::core::ffi::c_int) {
    let mut newlumpinfo: *mut lumpinfo_t = ::core::ptr::null_mut::<lumpinfo_t>();
    let mut i: ::core::ffi::c_uint = 0;
    newlumpinfo = calloc(
        newnumlumps as size_t,
        ::core::mem::size_of::<lumpinfo_t>() as size_t,
    ) as *mut lumpinfo_t;
    if newlumpinfo.is_null() {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"Couldn't realloc lumpinfo\0".as_ptr() as *const ::core::ffi::c_char,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    i = 0 as ::core::ffi::c_uint;
    while i < numlumps && i < newnumlumps as ::core::ffi::c_uint {
        memcpy(
            newlumpinfo.offset(i as isize) as *mut lumpinfo_t as *mut ::core::ffi::c_void,
            lumpinfo.offset(i as isize) as *mut lumpinfo_t as *const ::core::ffi::c_void,
            ::core::mem::size_of::<lumpinfo_t>() as size_t,
        );
        if !(*newlumpinfo.offset(i as isize)).cache.is_null() {
            Z_ChangeUser(
                (*newlumpinfo.offset(i as isize)).cache,
                &raw mut (*newlumpinfo.offset(i as isize)).cache,
            );
        }
        if !(*lumpinfo.offset(i as isize)).next.is_null() {
            let mut nextlumpnum: ::core::ffi::c_int =
                (*lumpinfo.offset(i as isize)).next.offset_from(lumpinfo) as ::core::ffi::c_long
                    as ::core::ffi::c_int;
            let ref mut c2rust_fresh0 = (*newlumpinfo.offset(i as isize)).next;
            *c2rust_fresh0 = newlumpinfo.offset(nextlumpnum as isize) as *mut lumpinfo_t;
        }
        i = i.wrapping_add(1);
    }
    free(lumpinfo as *mut ::core::ffi::c_void);
    lumpinfo = newlumpinfo;
    numlumps = newnumlumps as ::core::ffi::c_uint;
}
#[no_mangle]
pub unsafe extern "C" fn W_AddFile(mut filename: *mut ::core::ffi::c_char) -> *mut wad_file_t {
    let mut header: wadinfo_t = wadinfo_t {
        identification: [0; 4],
        numlumps: 0,
        infotableofs: 0,
    };
    let mut lump_p: *mut lumpinfo_t = ::core::ptr::null_mut::<lumpinfo_t>();
    let mut i: ::core::ffi::c_uint = 0;
    let mut wad_file: *mut wad_file_t = ::core::ptr::null_mut::<wad_file_t>();
    let mut length: ::core::ffi::c_int = 0;
    let mut startlump: ::core::ffi::c_int = 0;
    let mut fileinfo: *mut filelump_t = ::core::ptr::null_mut::<filelump_t>();
    let mut filerover: *mut filelump_t = ::core::ptr::null_mut::<filelump_t>();
    let mut newnumlumps: ::core::ffi::c_int = 0;
    wad_file = W_OpenFile(filename);
    if wad_file.is_null() {
        printf(
            b" couldn't open %s\n\0".as_ptr() as *const ::core::ffi::c_char,
            filename,
        );
        return ::core::ptr::null_mut::<wad_file_t>();
    }
    newnumlumps = numlumps as ::core::ffi::c_int;
    if strcasecmp(
        filename
            .offset(strlen(filename) as isize)
            .offset(-(3 as ::core::ffi::c_int as isize)),
        b"wad\0".as_ptr() as *const ::core::ffi::c_char,
    ) != 0
    {
        fileinfo = Z_Malloc(
            ::core::mem::size_of::<filelump_t>() as ::core::ffi::c_int,
            PU_STATIC as ::core::ffi::c_int,
            ::core::ptr::null_mut::<::core::ffi::c_void>(),
        ) as *mut filelump_t;
        (*fileinfo).filepos = 0 as ::core::ffi::c_int;
        (*fileinfo).size = (*wad_file).length as ::core::ffi::c_int;
        M_ExtractFileBase(
            filename,
            &raw mut (*fileinfo).name as *mut ::core::ffi::c_char,
        );
        newnumlumps += 1;
    } else {
        W_Read(
            wad_file,
            0 as ::core::ffi::c_uint,
            &raw mut header as *mut ::core::ffi::c_void,
            ::core::mem::size_of::<wadinfo_t>() as size_t,
        );
        if strncmp(
            &raw mut header.identification as *mut ::core::ffi::c_char,
            b"IWAD\0".as_ptr() as *const ::core::ffi::c_char,
            4 as size_t,
        ) != 0
        {
            if strncmp(
                &raw mut header.identification as *mut ::core::ffi::c_char,
                b"PWAD\0".as_ptr() as *const ::core::ffi::c_char,
                4 as size_t,
            ) != 0
            {
                let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
                snprintf(
                    &raw mut _i_err_buf as *mut ::core::ffi::c_char,
                    ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
                    b"Wad file %s doesn't have IWAD or PWAD id\n\0".as_ptr()
                        as *const ::core::ffi::c_char,
                    filename,
                );
                I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
            }
        }
        header.numlumps = header.numlumps;
        header.infotableofs = header.infotableofs;
        length = (header.numlumps as usize)
            .wrapping_mul(::core::mem::size_of::<filelump_t>() as usize)
            as ::core::ffi::c_int;
        fileinfo = Z_Malloc(
            length,
            PU_STATIC as ::core::ffi::c_int,
            ::core::ptr::null_mut::<::core::ffi::c_void>(),
        ) as *mut filelump_t;
        W_Read(
            wad_file,
            header.infotableofs as ::core::ffi::c_uint,
            fileinfo as *mut ::core::ffi::c_void,
            length as size_t,
        );
        newnumlumps += header.numlumps;
    }
    startlump = numlumps as ::core::ffi::c_int;
    ExtendLumpInfo(newnumlumps);
    lump_p = lumpinfo.offset(startlump as isize) as *mut lumpinfo_t;
    filerover = fileinfo;
    i = startlump as ::core::ffi::c_uint;
    while i < numlumps {
        (*lump_p).wad_file = wad_file;
        (*lump_p).position = (*filerover).filepos;
        (*lump_p).size = (*filerover).size;
        (*lump_p).cache = NULL;
        strncpy(
            &raw mut (*lump_p).name as *mut ::core::ffi::c_char,
            &raw mut (*filerover).name as *mut ::core::ffi::c_char,
            8 as size_t,
        );
        lump_p = lump_p.offset(1);
        filerover = filerover.offset(1);
        i = i.wrapping_add(1);
    }
    Z_Free(fileinfo as *mut ::core::ffi::c_void);
    if !lumphash.is_null() {
        Z_Free(lumphash as *mut ::core::ffi::c_void);
        lumphash = ::core::ptr::null_mut::<*mut lumpinfo_t>();
    }
    return wad_file;
}
#[no_mangle]
pub unsafe extern "C" fn W_NumLumps() -> ::core::ffi::c_int {
    return numlumps as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn W_CheckNumForName(
    mut name: *mut ::core::ffi::c_char,
) -> ::core::ffi::c_int {
    let mut lump_p: *mut lumpinfo_t = ::core::ptr::null_mut::<lumpinfo_t>();
    let mut i: ::core::ffi::c_int = 0;
    if !lumphash.is_null() {
        let mut hash: ::core::ffi::c_int = 0;
        hash = W_LumpNameHash(name).wrapping_rem(numlumps) as ::core::ffi::c_int;
        lump_p = *lumphash.offset(hash as isize);
        while !lump_p.is_null() {
            if strncasecmp(
                &raw mut (*lump_p).name as *mut ::core::ffi::c_char,
                name,
                8 as size_t,
            ) == 0
            {
                return lump_p.offset_from(lumpinfo) as ::core::ffi::c_long as ::core::ffi::c_int;
            }
            lump_p = (*lump_p).next;
        }
    } else {
        i = numlumps.wrapping_sub(1 as ::core::ffi::c_uint) as ::core::ffi::c_int;
        while i >= 0 as ::core::ffi::c_int {
            if strncasecmp(
                &raw mut (*lumpinfo.offset(i as isize)).name as *mut ::core::ffi::c_char,
                name,
                8 as size_t,
            ) == 0
            {
                return i;
            }
            i -= 1;
        }
    }
    return -1 as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn W_GetNumForName(mut name: *mut ::core::ffi::c_char) -> ::core::ffi::c_int {
    let mut i: ::core::ffi::c_int = 0;
    i = W_CheckNumForName(name);
    if i < 0 as ::core::ffi::c_int {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"W_GetNumForName: %s not found!\0".as_ptr() as *const ::core::ffi::c_char,
            name,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    return i;
}
#[no_mangle]
pub unsafe extern "C" fn W_LumpLength(mut lump: ::core::ffi::c_uint) -> ::core::ffi::c_int {
    if lump >= numlumps {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"W_LumpLength: %i >= numlumps\0".as_ptr() as *const ::core::ffi::c_char,
            lump,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    return (*lumpinfo.offset(lump as isize)).size;
}
#[no_mangle]
pub unsafe extern "C" fn W_ReadLump(
    mut lump: ::core::ffi::c_uint,
    mut dest: *mut ::core::ffi::c_void,
) {
    let mut c: ::core::ffi::c_int = 0;
    let mut l: *mut lumpinfo_t = ::core::ptr::null_mut::<lumpinfo_t>();
    if lump >= numlumps {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"W_ReadLump: %i >= numlumps\0".as_ptr() as *const ::core::ffi::c_char,
            lump,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    l = lumpinfo.offset(lump as isize);
    I_BeginRead();
    c = W_Read(
        (*l).wad_file,
        (*l).position as ::core::ffi::c_uint,
        dest,
        (*l).size as size_t,
    ) as ::core::ffi::c_int;
    if c < (*l).size {
        let mut _i_err_buf_0: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf_0 as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"W_ReadLump: only read %i of %i on lump %i\0".as_ptr() as *const ::core::ffi::c_char,
            c,
            (*l).size,
            lump,
        );
        I_ErrorV(&raw mut _i_err_buf_0 as *mut ::core::ffi::c_char);
    }
    I_EndRead();
}
#[no_mangle]
pub unsafe extern "C" fn W_CacheLumpNum(
    mut lumpnum: ::core::ffi::c_int,
    mut tag: ::core::ffi::c_int,
) -> *mut ::core::ffi::c_void {
    let mut result: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut lump: *mut lumpinfo_t = ::core::ptr::null_mut::<lumpinfo_t>();
    if lumpnum as ::core::ffi::c_uint >= numlumps {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"W_CacheLumpNum: %i >= numlumps\0".as_ptr() as *const ::core::ffi::c_char,
            lumpnum,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    lump = lumpinfo.offset(lumpnum as isize) as *mut lumpinfo_t;
    if !(*(*lump).wad_file).mapped.is_null() {
        result = (*(*lump).wad_file).mapped.offset((*lump).position as isize);
    } else if !(*lump).cache.is_null() {
        result = (*lump).cache as *mut byte;
        Z_ChangeTag2(
            (*lump).cache,
            tag,
            b"vendor/doomgeneric/w_wad.c\0".as_ptr() as *const ::core::ffi::c_char
                as *mut ::core::ffi::c_char,
            410 as ::core::ffi::c_int,
        );
    } else {
        (*lump).cache = Z_Malloc(
            W_LumpLength(lumpnum as ::core::ffi::c_uint),
            tag,
            &raw mut (*lump).cache as *mut ::core::ffi::c_void,
        );
        W_ReadLump(lumpnum as ::core::ffi::c_uint, (*lump).cache);
        result = (*lump).cache as *mut byte;
    }
    return result as *mut ::core::ffi::c_void;
}
#[no_mangle]
pub unsafe extern "C" fn W_CacheLumpName(
    mut name: *mut ::core::ffi::c_char,
    mut tag: ::core::ffi::c_int,
) -> *mut ::core::ffi::c_void {
    return W_CacheLumpNum(W_GetNumForName(name), tag);
}
#[no_mangle]
pub unsafe extern "C" fn W_ReleaseLumpNum(mut lumpnum: ::core::ffi::c_int) {
    let mut lump: *mut lumpinfo_t = ::core::ptr::null_mut::<lumpinfo_t>();
    if lumpnum as ::core::ffi::c_uint >= numlumps {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"W_ReleaseLumpNum: %i >= numlumps\0".as_ptr() as *const ::core::ffi::c_char,
            lumpnum,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    lump = lumpinfo.offset(lumpnum as isize) as *mut lumpinfo_t;
    if (*(*lump).wad_file).mapped.is_null() {
        Z_ChangeTag2(
            (*lump).cache,
            PU_CACHE as ::core::ffi::c_int,
            b"vendor/doomgeneric/w_wad.c\0".as_ptr() as *const ::core::ffi::c_char
                as *mut ::core::ffi::c_char,
            461 as ::core::ffi::c_int,
        );
    }
}
#[no_mangle]
pub unsafe extern "C" fn W_ReleaseLumpName(mut name: *mut ::core::ffi::c_char) {
    W_ReleaseLumpNum(W_GetNumForName(name));
}
#[no_mangle]
pub unsafe extern "C" fn W_GenerateHashTable() {
    let mut i: ::core::ffi::c_uint = 0;
    if !lumphash.is_null() {
        Z_Free(lumphash as *mut ::core::ffi::c_void);
    }
    if numlumps > 0 as ::core::ffi::c_uint {
        lumphash = Z_Malloc(
            (::core::mem::size_of::<*mut lumpinfo_t>() as usize).wrapping_mul(numlumps as usize)
                as ::core::ffi::c_int,
            PU_STATIC as ::core::ffi::c_int,
            NULL,
        ) as *mut *mut lumpinfo_t;
        memset(
            lumphash as *mut ::core::ffi::c_void,
            0 as ::core::ffi::c_int,
            (::core::mem::size_of::<*mut lumpinfo_t>() as size_t).wrapping_mul(numlumps as size_t),
        );
        i = 0 as ::core::ffi::c_uint;
        while i < numlumps {
            let mut hash: ::core::ffi::c_uint = 0;
            hash = W_LumpNameHash(
                &raw mut (*lumpinfo.offset(i as isize)).name as *mut ::core::ffi::c_char,
            )
            .wrapping_rem(numlumps);
            let ref mut c2rust_fresh1 = (*lumpinfo.offset(i as isize)).next;
            *c2rust_fresh1 = *lumphash.offset(hash as isize);
            let ref mut c2rust_fresh2 = *lumphash.offset(hash as isize);
            *c2rust_fresh2 = lumpinfo.offset(i as isize) as *mut lumpinfo_t;
            i = i.wrapping_add(1);
        }
    }
}
static mut unique_lumps: [C2Rust_Unnamed_0; 4] = [
    C2Rust_Unnamed_0 {
        mission: doom,
        lumpname: b"POSSA1\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
    },
    C2Rust_Unnamed_0 {
        mission: heretic,
        lumpname: b"IMPXA1\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
    },
    C2Rust_Unnamed_0 {
        mission: hexen,
        lumpname: b"ETTNA1\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
    },
    C2Rust_Unnamed_0 {
        mission: strife,
        lumpname: b"AGRDA1\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
    },
];
#[no_mangle]
pub unsafe extern "C" fn W_CheckCorrectIWAD(mut mission: GameMission_t) {
    let mut i: ::core::ffi::c_int = 0;
    let mut lumpnum: ::core::ffi::c_int = 0;
    i = 0 as ::core::ffi::c_int;
    while (i as usize)
        < (::core::mem::size_of::<[C2Rust_Unnamed_0; 4]>() as usize)
            .wrapping_div(::core::mem::size_of::<C2Rust_Unnamed_0>() as usize)
    {
        if mission as ::core::ffi::c_uint != unique_lumps[i as usize].mission as ::core::ffi::c_uint
        {
            lumpnum = W_CheckNumForName(unique_lumps[i as usize].lumpname);
            if lumpnum >= 0 as ::core::ffi::c_int {
                let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
                snprintf(
                    &raw mut _i_err_buf as *mut ::core::ffi::c_char,
                    ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
                    b"\nYou are trying to use a %s IWAD file with the %s%s binary.\nThis isn't going to work.\nYou probably want to use the %s%s binary.\0"
                        .as_ptr() as *const ::core::ffi::c_char,
                    D_SuggestGameName(unique_lumps[i as usize].mission, indetermined),
                    PROGRAM_PREFIX.as_ptr(),
                    D_GameMissionString(mission),
                    PROGRAM_PREFIX.as_ptr(),
                    D_GameMissionString(unique_lumps[i as usize].mission),
                );
                I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
            }
        }
        i += 1;
    }
}
