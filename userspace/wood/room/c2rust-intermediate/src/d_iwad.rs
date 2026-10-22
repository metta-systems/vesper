extern "C" {
    fn printf(__format: *const ::core::ffi::c_char, ...) -> ::core::ffi::c_int;
    fn snprintf(
        __s: *mut ::core::ffi::c_char,
        __maxlen: size_t,
        __format: *const ::core::ffi::c_char,
        ...
    ) -> ::core::ffi::c_int;
    fn malloc(__size: size_t) -> *mut ::core::ffi::c_void;
    fn free(__ptr: *mut ::core::ffi::c_void);
    fn strcmp(
        __s1: *const ::core::ffi::c_char,
        __s2: *const ::core::ffi::c_char,
    ) -> ::core::ffi::c_int;
    fn strdup(__s: *const ::core::ffi::c_char) -> *mut ::core::ffi::c_char;
    fn strrchr(
        __s: *const ::core::ffi::c_char,
        __c: ::core::ffi::c_int,
    ) -> *mut ::core::ffi::c_char;
    fn strlen(__s: *const ::core::ffi::c_char) -> size_t;
    fn strcasecmp(
        __s1: *const ::core::ffi::c_char,
        __s2: *const ::core::ffi::c_char,
    ) -> ::core::ffi::c_int;
    fn I_ErrorV(msg: *const ::core::ffi::c_char);
    static mut myargv: *mut *mut ::core::ffi::c_char;
    fn M_CheckParmWithArgs(
        check: *mut ::core::ffi::c_char,
        num_args: ::core::ffi::c_int,
    ) -> ::core::ffi::c_int;
    fn M_FileExists(file: *mut ::core::ffi::c_char) -> boolean;
    fn M_StringJoinA(strs: *const *const ::core::ffi::c_char) -> *mut ::core::ffi::c_char;
}
pub type size_t = usize;
pub type boolean = ::core::ffi::c_uint;
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
#[derive(Copy, Clone)]
#[repr(C)]
pub struct iwad_t {
    pub name: *mut ::core::ffi::c_char,
    pub mission: GameMission_t,
    pub mode: GameMode_t,
    pub description: *mut ::core::ffi::c_char,
}
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
pub const FILES_DIR: [::core::ffi::c_char; 2] =
    unsafe { ::core::mem::transmute::<[u8; 2], [::core::ffi::c_char; 2]>(*b".\0") };
pub const true_0: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
pub const false_0: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
pub const DIR_SEPARATOR: ::core::ffi::c_int = '/' as ::core::ffi::c_int;
static mut iwads: [iwad_t; 14] = [
    iwad_t {
        name: b"doom2.wad\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        mission: doom2,
        mode: commercial,
        description: b"Doom II\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
    },
    iwad_t {
        name: b"plutonia.wad\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        mission: pack_plut,
        mode: commercial,
        description: b"Final Doom: Plutonia Experiment\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
    },
    iwad_t {
        name: b"tnt.wad\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        mission: pack_tnt,
        mode: commercial,
        description: b"Final Doom: TNT: Evilution\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
    },
    iwad_t {
        name: b"doom.wad\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        mission: doom,
        mode: retail,
        description: b"Doom\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
    },
    iwad_t {
        name: b"doom1.wad\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        mission: doom,
        mode: shareware,
        description: b"Doom Shareware\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
    },
    iwad_t {
        name: b"chex.wad\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        mission: pack_chex,
        mode: shareware,
        description: b"Chex Quest\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
    },
    iwad_t {
        name: b"hacx.wad\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        mission: pack_hacx,
        mode: commercial,
        description: b"Hacx\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
    },
    iwad_t {
        name: b"freedm.wad\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        mission: doom2,
        mode: commercial,
        description: b"FreeDM\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
    },
    iwad_t {
        name: b"freedoom2.wad\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        mission: doom2,
        mode: commercial,
        description: b"Freedoom: Phase 2\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
    },
    iwad_t {
        name: b"freedoom1.wad\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        mission: doom,
        mode: retail,
        description: b"Freedoom: Phase 1\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
    },
    iwad_t {
        name: b"heretic.wad\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        mission: heretic,
        mode: retail,
        description: b"Heretic\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
    },
    iwad_t {
        name: b"heretic1.wad\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        mission: heretic,
        mode: shareware,
        description: b"Heretic Shareware\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char,
    },
    iwad_t {
        name: b"hexen.wad\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        mission: hexen,
        mode: commercial,
        description: b"Hexen\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
    },
    iwad_t {
        name: b"strife1.wad\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        mission: strife,
        mode: commercial,
        description: b"Strife\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
    },
];
pub const MAX_IWAD_DIRS: ::core::ffi::c_int = 128 as ::core::ffi::c_int;
static mut iwad_dirs_built: boolean = false_0 as boolean;
static mut iwad_dirs: [*mut ::core::ffi::c_char; 128] =
    [::core::ptr::null_mut::<::core::ffi::c_char>(); 128];
static mut num_iwad_dirs: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
unsafe extern "C" fn AddIWADDir(mut dir: *mut ::core::ffi::c_char) {
    if num_iwad_dirs < MAX_IWAD_DIRS {
        iwad_dirs[num_iwad_dirs as usize] = dir;
        num_iwad_dirs += 1;
    }
}
unsafe extern "C" fn DirIsFile(
    mut path: *mut ::core::ffi::c_char,
    mut filename: *mut ::core::ffi::c_char,
) -> boolean {
    let mut path_len: size_t = 0;
    let mut filename_len: size_t = 0;
    path_len = strlen(path);
    filename_len = strlen(filename);
    return (path_len >= filename_len.wrapping_add(1 as size_t)
        && *path.offset(
            path_len
                .wrapping_sub(filename_len)
                .wrapping_sub(1 as size_t) as isize,
        ) as ::core::ffi::c_int
            == DIR_SEPARATOR
        && strcasecmp(
            path.offset(path_len.wrapping_sub(filename_len) as isize) as *mut ::core::ffi::c_char,
            filename,
        ) == 0) as ::core::ffi::c_int as boolean;
}
unsafe extern "C" fn CheckDirectoryHasIWAD(
    mut dir: *mut ::core::ffi::c_char,
    mut iwadname: *mut ::core::ffi::c_char,
) -> *mut ::core::ffi::c_char {
    let mut filename: *mut ::core::ffi::c_char = ::core::ptr::null_mut::<::core::ffi::c_char>();
    if DirIsFile(dir, iwadname) != 0 && M_FileExists(dir) != 0 {
        return strdup(dir);
    }
    if strcmp(dir, b".\0".as_ptr() as *const ::core::ffi::c_char) == 0 {
        filename = strdup(iwadname);
    } else {
        let c2rust_fresh1: [*const ::core::ffi::c_char; 4] = [
            dir as *const ::core::ffi::c_char,
            b"/\0".as_ptr() as *const ::core::ffi::c_char,
            iwadname as *const ::core::ffi::c_char,
            ::core::ptr::null::<::core::ffi::c_char>(),
        ];
        filename = M_StringJoinA(&raw const c2rust_fresh1 as *const *const ::core::ffi::c_char);
    }
    printf(
        b"Trying IWAD file:%s\n\0".as_ptr() as *const ::core::ffi::c_char,
        filename,
    );
    if M_FileExists(filename) != 0 {
        return filename;
    }
    free(filename as *mut ::core::ffi::c_void);
    return ::core::ptr::null_mut::<::core::ffi::c_char>();
}
unsafe extern "C" fn SearchDirectoryForIWAD(
    mut dir: *mut ::core::ffi::c_char,
    mut mask: ::core::ffi::c_int,
    mut mission: *mut GameMission_t,
) -> *mut ::core::ffi::c_char {
    let mut filename: *mut ::core::ffi::c_char = ::core::ptr::null_mut::<::core::ffi::c_char>();
    let mut i: size_t = 0;
    i = 0 as size_t;
    while i
        < (::core::mem::size_of::<[iwad_t; 14]>() as usize)
            .wrapping_div(::core::mem::size_of::<iwad_t>() as usize)
    {
        if !((1 as ::core::ffi::c_int) << iwads[i as usize].mission as ::core::ffi::c_uint & mask
            == 0 as ::core::ffi::c_int)
        {
            filename = CheckDirectoryHasIWAD(dir, iwads[i as usize].name);
            if !filename.is_null() {
                *mission = iwads[i as usize].mission;
                return filename;
            }
        }
        i = i.wrapping_add(1);
    }
    return ::core::ptr::null_mut::<::core::ffi::c_char>();
}
unsafe extern "C" fn IdentifyIWADByName(
    mut name: *mut ::core::ffi::c_char,
    mut mask: ::core::ffi::c_int,
) -> GameMission_t {
    let mut i: size_t = 0;
    let mut mission: GameMission_t = doom;
    let mut p: *mut ::core::ffi::c_char = ::core::ptr::null_mut::<::core::ffi::c_char>();
    p = strrchr(name, DIR_SEPARATOR);
    if !p.is_null() {
        name = p.offset(1 as ::core::ffi::c_int as isize);
    }
    mission = none;
    i = 0 as size_t;
    while i
        < (::core::mem::size_of::<[iwad_t; 14]>() as usize)
            .wrapping_div(::core::mem::size_of::<iwad_t>() as usize)
    {
        if !((1 as ::core::ffi::c_int) << iwads[i as usize].mission as ::core::ffi::c_uint & mask
            == 0 as ::core::ffi::c_int)
        {
            if strcasecmp(name, iwads[i as usize].name) == 0 {
                mission = iwads[i as usize].mission;
                break;
            }
        }
        i = i.wrapping_add(1);
    }
    return mission;
}
unsafe extern "C" fn BuildIWADDirList() {
    AddIWADDir(FILES_DIR.as_ptr() as *mut ::core::ffi::c_char);
    iwad_dirs_built = true_0 as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn D_FindWADByName(
    mut name: *mut ::core::ffi::c_char,
) -> *mut ::core::ffi::c_char {
    let mut path: *mut ::core::ffi::c_char = ::core::ptr::null_mut::<::core::ffi::c_char>();
    let mut i: ::core::ffi::c_int = 0;
    if M_FileExists(name) != 0 {
        return name;
    }
    BuildIWADDirList();
    i = 0 as ::core::ffi::c_int;
    while i < num_iwad_dirs {
        if DirIsFile(iwad_dirs[i as usize], name) != 0 && M_FileExists(iwad_dirs[i as usize]) != 0 {
            return strdup(iwad_dirs[i as usize]);
        }
        let c2rust_fresh0: [*const ::core::ffi::c_char; 4] = [
            iwad_dirs[i as usize] as *const ::core::ffi::c_char,
            b"/\0".as_ptr() as *const ::core::ffi::c_char,
            name as *const ::core::ffi::c_char,
            ::core::ptr::null::<::core::ffi::c_char>(),
        ];
        path = M_StringJoinA(&raw const c2rust_fresh0 as *const *const ::core::ffi::c_char);
        if M_FileExists(path) != 0 {
            return path;
        }
        free(path as *mut ::core::ffi::c_void);
        i += 1;
    }
    return ::core::ptr::null_mut::<::core::ffi::c_char>();
}
#[no_mangle]
pub unsafe extern "C" fn D_TryFindWADByName(
    mut filename: *mut ::core::ffi::c_char,
) -> *mut ::core::ffi::c_char {
    let mut result: *mut ::core::ffi::c_char = ::core::ptr::null_mut::<::core::ffi::c_char>();
    result = D_FindWADByName(filename);
    if !result.is_null() {
        return result;
    } else {
        return filename;
    };
}
#[no_mangle]
pub unsafe extern "C" fn D_FindIWAD(
    mut mask: ::core::ffi::c_int,
    mut mission: *mut GameMission_t,
) -> *mut ::core::ffi::c_char {
    let mut result: *mut ::core::ffi::c_char = ::core::ptr::null_mut::<::core::ffi::c_char>();
    let mut iwadfile: *mut ::core::ffi::c_char = ::core::ptr::null_mut::<::core::ffi::c_char>();
    let mut iwadparm: ::core::ffi::c_int = 0;
    let mut i: ::core::ffi::c_int = 0;
    iwadparm = M_CheckParmWithArgs(
        b"-iwad\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        1 as ::core::ffi::c_int,
    );
    if iwadparm != 0 {
        iwadfile = *myargv.offset((iwadparm + 1 as ::core::ffi::c_int) as isize);
        result = D_FindWADByName(iwadfile);
        if result.is_null() {
            let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
            snprintf(
                &raw mut _i_err_buf as *mut ::core::ffi::c_char,
                ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
                b"IWAD file '%s' not found!\0".as_ptr() as *const ::core::ffi::c_char,
                iwadfile,
            );
            I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
        }
        *mission = IdentifyIWADByName(result, mask);
    } else {
        printf(b"-iwad not specified, trying a few iwad names\n\0".as_ptr()
            as *const ::core::ffi::c_char);
        result = ::core::ptr::null_mut::<::core::ffi::c_char>();
        BuildIWADDirList();
        i = 0 as ::core::ffi::c_int;
        while result.is_null() && i < num_iwad_dirs {
            result = SearchDirectoryForIWAD(iwad_dirs[i as usize], mask, mission);
            i += 1;
        }
    }
    return result;
}
#[no_mangle]
pub unsafe extern "C" fn D_FindAllIWADs(mut mask: ::core::ffi::c_int) -> *mut *const iwad_t {
    let mut result: *mut *const iwad_t = ::core::ptr::null_mut::<*const iwad_t>();
    let mut result_len: ::core::ffi::c_int = 0;
    let mut filename: *mut ::core::ffi::c_char = ::core::ptr::null_mut::<::core::ffi::c_char>();
    let mut i: ::core::ffi::c_int = 0;
    result = malloc(
        (::core::mem::size_of::<*mut iwad_t>() as size_t).wrapping_mul(
            (::core::mem::size_of::<[iwad_t; 14]>() as size_t)
                .wrapping_div(::core::mem::size_of::<iwad_t>() as size_t)
                .wrapping_add(1 as size_t),
        ),
    ) as *mut *const iwad_t;
    result_len = 0 as ::core::ffi::c_int;
    i = 0 as ::core::ffi::c_int;
    while (i as usize)
        < (::core::mem::size_of::<[iwad_t; 14]>() as usize)
            .wrapping_div(::core::mem::size_of::<iwad_t>() as usize)
    {
        if !((1 as ::core::ffi::c_int) << iwads[i as usize].mission as ::core::ffi::c_uint & mask
            == 0 as ::core::ffi::c_int)
        {
            filename = D_FindWADByName(iwads[i as usize].name);
            if !filename.is_null() {
                let ref mut c2rust_fresh2 = *result.offset(result_len as isize);
                *c2rust_fresh2 =
                    (&raw const iwads as *const iwad_t).offset(i as isize) as *const iwad_t;
                result_len += 1;
            }
        }
        i += 1;
    }
    let ref mut c2rust_fresh3 = *result.offset(result_len as isize);
    *c2rust_fresh3 = ::core::ptr::null::<iwad_t>();
    return result;
}
#[no_mangle]
pub unsafe extern "C" fn D_SaveGameIWADName(
    mut gamemission: GameMission_t,
) -> *mut ::core::ffi::c_char {
    let mut i: size_t = 0;
    i = 0 as size_t;
    while i
        < (::core::mem::size_of::<[iwad_t; 14]>() as usize)
            .wrapping_div(::core::mem::size_of::<iwad_t>() as usize)
    {
        if gamemission as ::core::ffi::c_uint == iwads[i as usize].mission as ::core::ffi::c_uint {
            return iwads[i as usize].name;
        }
        i = i.wrapping_add(1);
    }
    return b"unknown.wad\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
}
#[no_mangle]
pub unsafe extern "C" fn D_SuggestIWADName(
    mut mission: GameMission_t,
    mut mode: GameMode_t,
) -> *mut ::core::ffi::c_char {
    let mut i: ::core::ffi::c_int = 0;
    i = 0 as ::core::ffi::c_int;
    while (i as usize)
        < (::core::mem::size_of::<[iwad_t; 14]>() as usize)
            .wrapping_div(::core::mem::size_of::<iwad_t>() as usize)
    {
        if iwads[i as usize].mission as ::core::ffi::c_uint == mission as ::core::ffi::c_uint
            && iwads[i as usize].mode as ::core::ffi::c_uint == mode as ::core::ffi::c_uint
        {
            return iwads[i as usize].name;
        }
        i += 1;
    }
    return b"unknown.wad\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
}
#[no_mangle]
pub unsafe extern "C" fn D_SuggestGameName(
    mut mission: GameMission_t,
    mut mode: GameMode_t,
) -> *mut ::core::ffi::c_char {
    let mut i: ::core::ffi::c_int = 0;
    i = 0 as ::core::ffi::c_int;
    while (i as usize)
        < (::core::mem::size_of::<[iwad_t; 14]>() as usize)
            .wrapping_div(::core::mem::size_of::<iwad_t>() as usize)
    {
        if iwads[i as usize].mission as ::core::ffi::c_uint == mission as ::core::ffi::c_uint
            && (mode as ::core::ffi::c_uint
                == indetermined as ::core::ffi::c_int as ::core::ffi::c_uint
                || iwads[i as usize].mode as ::core::ffi::c_uint == mode as ::core::ffi::c_uint)
        {
            return iwads[i as usize].description;
        }
        i += 1;
    }
    return b"Unknown game?\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
}
