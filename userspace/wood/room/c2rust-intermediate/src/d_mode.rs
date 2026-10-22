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
pub type GameVersion_t = ::core::ffi::c_uint;
pub const exe_strife_1_31: GameVersion_t = 13;
pub const exe_strife_1_2: GameVersion_t = 12;
pub const exe_hexen_1_1: GameVersion_t = 11;
pub const exe_heretic_1_3: GameVersion_t = 10;
pub const exe_chex: GameVersion_t = 9;
pub const exe_final2: GameVersion_t = 8;
pub const exe_final: GameVersion_t = 7;
pub const exe_ultimate: GameVersion_t = 6;
pub const exe_hacx: GameVersion_t = 5;
pub const exe_doom_1_9: GameVersion_t = 4;
pub const exe_doom_1_8: GameVersion_t = 3;
pub const exe_doom_1_7: GameVersion_t = 2;
pub const exe_doom_1_666: GameVersion_t = 1;
pub const exe_doom_1_2: GameVersion_t = 0;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct C2Rust_Unnamed {
    pub mission: GameMission_t,
    pub mode: GameMode_t,
    pub episode: ::core::ffi::c_int,
    pub map: ::core::ffi::c_int,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct C2Rust_Unnamed_0 {
    pub mission: GameMission_t,
    pub version: GameVersion_t,
}
pub const true_0: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
pub const false_0: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
static mut valid_modes: [C2Rust_Unnamed; 13] = [
    C2Rust_Unnamed {
        mission: pack_chex,
        mode: shareware,
        episode: 1 as ::core::ffi::c_int,
        map: 5 as ::core::ffi::c_int,
    },
    C2Rust_Unnamed {
        mission: doom,
        mode: shareware,
        episode: 1 as ::core::ffi::c_int,
        map: 9 as ::core::ffi::c_int,
    },
    C2Rust_Unnamed {
        mission: doom,
        mode: registered,
        episode: 3 as ::core::ffi::c_int,
        map: 9 as ::core::ffi::c_int,
    },
    C2Rust_Unnamed {
        mission: doom,
        mode: retail,
        episode: 4 as ::core::ffi::c_int,
        map: 9 as ::core::ffi::c_int,
    },
    C2Rust_Unnamed {
        mission: doom2,
        mode: commercial,
        episode: 1 as ::core::ffi::c_int,
        map: 32 as ::core::ffi::c_int,
    },
    C2Rust_Unnamed {
        mission: pack_tnt,
        mode: commercial,
        episode: 1 as ::core::ffi::c_int,
        map: 32 as ::core::ffi::c_int,
    },
    C2Rust_Unnamed {
        mission: pack_plut,
        mode: commercial,
        episode: 1 as ::core::ffi::c_int,
        map: 32 as ::core::ffi::c_int,
    },
    C2Rust_Unnamed {
        mission: pack_hacx,
        mode: commercial,
        episode: 1 as ::core::ffi::c_int,
        map: 32 as ::core::ffi::c_int,
    },
    C2Rust_Unnamed {
        mission: heretic,
        mode: shareware,
        episode: 1 as ::core::ffi::c_int,
        map: 9 as ::core::ffi::c_int,
    },
    C2Rust_Unnamed {
        mission: heretic,
        mode: registered,
        episode: 3 as ::core::ffi::c_int,
        map: 9 as ::core::ffi::c_int,
    },
    C2Rust_Unnamed {
        mission: heretic,
        mode: retail,
        episode: 5 as ::core::ffi::c_int,
        map: 9 as ::core::ffi::c_int,
    },
    C2Rust_Unnamed {
        mission: hexen,
        mode: commercial,
        episode: 1 as ::core::ffi::c_int,
        map: 60 as ::core::ffi::c_int,
    },
    C2Rust_Unnamed {
        mission: strife,
        mode: commercial,
        episode: 1 as ::core::ffi::c_int,
        map: 34 as ::core::ffi::c_int,
    },
];
#[no_mangle]
pub unsafe extern "C" fn D_ValidGameMode(
    mut mission: GameMission_t,
    mut mode: GameMode_t,
) -> boolean {
    let mut i: ::core::ffi::c_int = 0;
    i = 0 as ::core::ffi::c_int;
    while (i as usize)
        < (::core::mem::size_of::<[C2Rust_Unnamed; 13]>() as usize)
            .wrapping_div(::core::mem::size_of::<C2Rust_Unnamed>() as usize)
    {
        if valid_modes[i as usize].mode as ::core::ffi::c_uint == mode as ::core::ffi::c_uint
            && valid_modes[i as usize].mission as ::core::ffi::c_uint
                == mission as ::core::ffi::c_uint
        {
            return true_0 as boolean;
        }
        i += 1;
    }
    return false_0 as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn D_ValidEpisodeMap(
    mut mission: GameMission_t,
    mut mode: GameMode_t,
    mut episode: ::core::ffi::c_int,
    mut map: ::core::ffi::c_int,
) -> boolean {
    let mut i: ::core::ffi::c_int = 0;
    if mission as ::core::ffi::c_uint == heretic as ::core::ffi::c_int as ::core::ffi::c_uint {
        if mode as ::core::ffi::c_uint == retail as ::core::ffi::c_int as ::core::ffi::c_uint
            && episode == 6 as ::core::ffi::c_int
        {
            return (map >= 1 as ::core::ffi::c_int && map <= 3 as ::core::ffi::c_int)
                as ::core::ffi::c_int as boolean;
        } else if mode as ::core::ffi::c_uint
            == registered as ::core::ffi::c_int as ::core::ffi::c_uint
            && episode == 4 as ::core::ffi::c_int
        {
            return (map == 1 as ::core::ffi::c_int) as ::core::ffi::c_int as boolean;
        }
    }
    i = 0 as ::core::ffi::c_int;
    while (i as usize)
        < (::core::mem::size_of::<[C2Rust_Unnamed; 13]>() as usize)
            .wrapping_div(::core::mem::size_of::<C2Rust_Unnamed>() as usize)
    {
        if mission as ::core::ffi::c_uint == valid_modes[i as usize].mission as ::core::ffi::c_uint
            && mode as ::core::ffi::c_uint == valid_modes[i as usize].mode as ::core::ffi::c_uint
        {
            return (episode >= 1 as ::core::ffi::c_int
                && episode <= valid_modes[i as usize].episode
                && map >= 1 as ::core::ffi::c_int
                && map <= valid_modes[i as usize].map) as ::core::ffi::c_int
                as boolean;
        }
        i += 1;
    }
    return false_0 as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn D_GetNumEpisodes(
    mut mission: GameMission_t,
    mut mode: GameMode_t,
) -> ::core::ffi::c_int {
    let mut episode: ::core::ffi::c_int = 0;
    episode = 1 as ::core::ffi::c_int;
    while D_ValidEpisodeMap(mission, mode, episode, 1 as ::core::ffi::c_int) != 0 {
        episode += 1;
    }
    return episode - 1 as ::core::ffi::c_int;
}
static mut valid_versions: [C2Rust_Unnamed_0; 10] = [
    C2Rust_Unnamed_0 {
        mission: doom,
        version: exe_doom_1_9,
    },
    C2Rust_Unnamed_0 {
        mission: doom,
        version: exe_hacx,
    },
    C2Rust_Unnamed_0 {
        mission: doom,
        version: exe_ultimate,
    },
    C2Rust_Unnamed_0 {
        mission: doom,
        version: exe_final,
    },
    C2Rust_Unnamed_0 {
        mission: doom,
        version: exe_final2,
    },
    C2Rust_Unnamed_0 {
        mission: doom,
        version: exe_chex,
    },
    C2Rust_Unnamed_0 {
        mission: heretic,
        version: exe_heretic_1_3,
    },
    C2Rust_Unnamed_0 {
        mission: hexen,
        version: exe_hexen_1_1,
    },
    C2Rust_Unnamed_0 {
        mission: strife,
        version: exe_strife_1_2,
    },
    C2Rust_Unnamed_0 {
        mission: strife,
        version: exe_strife_1_31,
    },
];
#[no_mangle]
pub unsafe extern "C" fn D_ValidGameVersion(
    mut mission: GameMission_t,
    mut version: GameVersion_t,
) -> boolean {
    let mut i: ::core::ffi::c_int = 0;
    if mission as ::core::ffi::c_uint == doom2 as ::core::ffi::c_int as ::core::ffi::c_uint
        || mission as ::core::ffi::c_uint == pack_plut as ::core::ffi::c_int as ::core::ffi::c_uint
        || mission as ::core::ffi::c_uint == pack_tnt as ::core::ffi::c_int as ::core::ffi::c_uint
        || mission as ::core::ffi::c_uint == pack_hacx as ::core::ffi::c_int as ::core::ffi::c_uint
        || mission as ::core::ffi::c_uint == pack_chex as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        mission = doom;
    }
    i = 0 as ::core::ffi::c_int;
    while (i as usize)
        < (::core::mem::size_of::<[C2Rust_Unnamed_0; 10]>() as usize)
            .wrapping_div(::core::mem::size_of::<C2Rust_Unnamed_0>() as usize)
    {
        if valid_versions[i as usize].mission as ::core::ffi::c_uint
            == mission as ::core::ffi::c_uint
            && valid_versions[i as usize].version as ::core::ffi::c_uint
                == version as ::core::ffi::c_uint
        {
            return true_0 as boolean;
        }
        i += 1;
    }
    return false_0 as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn D_IsEpisodeMap(mut mission: GameMission_t) -> boolean {
    match mission as ::core::ffi::c_uint {
        0 | 6 | 4 => return true_0 as boolean,
        9 | 7 | 1 | 5 | 2 | 3 | 8 | _ => return false_0 as boolean,
    };
}
#[no_mangle]
pub unsafe extern "C" fn D_GameMissionString(
    mut mission: GameMission_t,
) -> *mut ::core::ffi::c_char {
    match mission as ::core::ffi::c_uint {
        0 => {
            return b"doom\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        }
        1 => {
            return b"doom2\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        }
        2 => {
            return b"tnt\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        }
        3 => {
            return b"plutonia\0".as_ptr() as *const ::core::ffi::c_char
                as *mut ::core::ffi::c_char;
        }
        5 => {
            return b"hacx\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        }
        4 => {
            return b"chex\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        }
        6 => {
            return b"heretic\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        }
        7 => {
            return b"hexen\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        }
        8 => {
            return b"strife\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        }
        9 | _ => {
            return b"none\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        }
    };
}
