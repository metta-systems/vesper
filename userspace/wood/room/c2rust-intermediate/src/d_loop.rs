extern "C" {
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
    fn printf(__format: *const ::core::ffi::c_char, ...) -> ::core::ffi::c_int;
    fn snprintf(
        __s: *mut ::core::ffi::c_char,
        __maxlen: size_t,
        __format: *const ::core::ffi::c_char,
        ...
    ) -> ::core::ffi::c_int;
    fn I_ErrorV(msg: *const ::core::ffi::c_char);
    fn I_AtExit(func: atexit_func_t, run_if_error: boolean);
    fn I_GetTime() -> ::core::ffi::c_int;
    fn I_GetTimeMS() -> ::core::ffi::c_int;
    fn I_Sleep(ms: ::core::ffi::c_int);
    fn I_StartTic();
    static mut net_client_connected: boolean;
    static mut drone: boolean;
}
pub type size_t = usize;
pub type __uint8_t = u8;
pub type uint8_t = __uint8_t;
pub type boolean = ::core::ffi::c_uint;
pub type byte = uint8_t;
pub type C2Rust_Unnamed = ::core::ffi::c_uint;
pub const BTS_SAVESHIFT: C2Rust_Unnamed = 2;
pub const BTS_SAVEMASK: C2Rust_Unnamed = 28;
pub const BTS_SAVEGAME: C2Rust_Unnamed = 2;
pub const BTS_PAUSE: C2Rust_Unnamed = 1;
pub const BT_WEAPONSHIFT: C2Rust_Unnamed = 3;
pub const BT_WEAPONMASK: C2Rust_Unnamed = 56;
pub const BT_CHANGE: C2Rust_Unnamed = 4;
pub const BT_SPECIALMASK: C2Rust_Unnamed = 3;
pub const BT_SPECIAL: C2Rust_Unnamed = 128;
pub const BT_USE: C2Rust_Unnamed = 2;
pub const BT_ATTACK: C2Rust_Unnamed = 1;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct ticcmd_t {
    pub forwardmove: ::core::ffi::c_schar,
    pub sidemove: ::core::ffi::c_schar,
    pub angleturn: ::core::ffi::c_short,
    pub chatchar: byte,
    pub buttons: byte,
    pub consistancy: byte,
    pub buttons2: byte,
    pub inventory: ::core::ffi::c_int,
    pub lookfly: byte,
    pub arti: byte,
}
pub type sha1_digest_t = [byte; 20];
#[derive(Copy, Clone)]
#[repr(C)]
pub struct net_connect_data_t {
    pub gamemode: ::core::ffi::c_int,
    pub gamemission: ::core::ffi::c_int,
    pub lowres_turn: ::core::ffi::c_int,
    pub drone: ::core::ffi::c_int,
    pub max_players: ::core::ffi::c_int,
    pub is_freedoom: ::core::ffi::c_int,
    pub wad_sha1sum: sha1_digest_t,
    pub deh_sha1sum: sha1_digest_t,
    pub player_class: ::core::ffi::c_int,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct net_gamesettings_t {
    pub ticdup: ::core::ffi::c_int,
    pub extratics: ::core::ffi::c_int,
    pub deathmatch: ::core::ffi::c_int,
    pub episode: ::core::ffi::c_int,
    pub nomonsters: ::core::ffi::c_int,
    pub fast_monsters: ::core::ffi::c_int,
    pub respawn_monsters: ::core::ffi::c_int,
    pub map: ::core::ffi::c_int,
    pub skill: ::core::ffi::c_int,
    pub gameversion: ::core::ffi::c_int,
    pub lowres_turn: ::core::ffi::c_int,
    pub new_sync: ::core::ffi::c_int,
    pub timelimit: ::core::ffi::c_int,
    pub loadgame: ::core::ffi::c_int,
    pub random: ::core::ffi::c_int,
    pub num_players: ::core::ffi::c_int,
    pub consoleplayer: ::core::ffi::c_int,
    pub player_classes: [::core::ffi::c_int; 8],
}
pub type netgame_startup_callback_t =
    Option<unsafe extern "C" fn(::core::ffi::c_int, ::core::ffi::c_int) -> boolean>;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct loop_interface_t {
    pub ProcessEvents: Option<unsafe extern "C" fn() -> ()>,
    pub BuildTiccmd: Option<unsafe extern "C" fn(*mut ticcmd_t, ::core::ffi::c_int) -> ()>,
    pub RunTic: Option<unsafe extern "C" fn(*mut ticcmd_t, *mut boolean) -> ()>,
    pub RunMenu: Option<unsafe extern "C" fn() -> ()>,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct ticcmd_set_t {
    pub cmds: [ticcmd_t; 8],
    pub ingame: [boolean; 8],
}
pub type fixed_t = ::core::ffi::c_int;
pub type atexit_func_t = Option<unsafe extern "C" fn() -> ()>;
pub const true_0: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
pub const false_0: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
pub const NET_MAXPLAYERS: ::core::ffi::c_int = 8 as ::core::ffi::c_int;
pub const BACKUPTICS: ::core::ffi::c_int = 128 as ::core::ffi::c_int;
pub const TICRATE: ::core::ffi::c_int = 35 as ::core::ffi::c_int;
pub const FRACBITS: ::core::ffi::c_int = 16 as ::core::ffi::c_int;
pub const FRACUNIT: ::core::ffi::c_int = (1 as ::core::ffi::c_int) << FRACBITS;
static mut ticdata: [ticcmd_set_t; 128] = [ticcmd_set_t {
    cmds: [ticcmd_t {
        forwardmove: 0,
        sidemove: 0,
        angleturn: 0,
        chatchar: 0,
        buttons: 0,
        consistancy: 0,
        buttons2: 0,
        inventory: 0,
        lookfly: 0,
        arti: 0,
    }; 8],
    ingame: [0; 8],
}; 128];
static mut maketic: ::core::ffi::c_int = 0;
static mut recvtic: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut gametic: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut singletics: boolean = false_0 as boolean;
static mut localplayer: ::core::ffi::c_int = 0;
static mut skiptics: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
#[no_mangle]
pub static mut ticdup: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut offsetms: fixed_t = 0;
static mut new_sync: boolean = true_0 as boolean;
static mut loop_interface: *mut loop_interface_t = ::core::ptr::null_mut::<loop_interface_t>();
static mut local_playeringame: [boolean; 8] = [0; 8];
static mut player_class: ::core::ffi::c_int = 0;
unsafe extern "C" fn GetAdjustedTime() -> ::core::ffi::c_int {
    let mut time_ms: ::core::ffi::c_int = 0;
    time_ms = I_GetTimeMS();
    if new_sync != 0 {
        time_ms += offsetms as ::core::ffi::c_int / FRACUNIT;
    }
    return time_ms * TICRATE / 1000 as ::core::ffi::c_int;
}
unsafe extern "C" fn BuildNewTic() -> boolean {
    let mut gameticdiv: ::core::ffi::c_int = 0;
    let mut cmd: ticcmd_t = ticcmd_t {
        forwardmove: 0,
        sidemove: 0,
        angleturn: 0,
        chatchar: 0,
        buttons: 0,
        consistancy: 0,
        buttons2: 0,
        inventory: 0,
        lookfly: 0,
        arti: 0,
    };
    gameticdiv = gametic / ticdup;
    I_StartTic();
    ::core::mem::transmute::<_, fn()>(
        (*loop_interface)
            .ProcessEvents
            .expect("non-null function pointer"),
    )();
    ::core::mem::transmute::<_, fn()>(
        (*loop_interface)
            .RunMenu
            .expect("non-null function pointer"),
    )();
    if drone != 0 {
        return false_0 as boolean;
    }
    if new_sync != 0 {
        if net_client_connected == 0 && maketic - gameticdiv > 2 as ::core::ffi::c_int {
            return false_0 as boolean;
        }
        if maketic - gameticdiv > 8 as ::core::ffi::c_int {
            return false_0 as boolean;
        }
    } else if maketic - gameticdiv >= 5 as ::core::ffi::c_int {
        return false_0 as boolean;
    }
    memset(
        &raw mut cmd as *mut ::core::ffi::c_void,
        0 as ::core::ffi::c_int,
        ::core::mem::size_of::<ticcmd_t>() as size_t,
    );
    (*loop_interface)
        .BuildTiccmd
        .expect("non-null function pointer")(&raw mut cmd, maketic);
    ticdata[(maketic % BACKUPTICS) as usize].cmds[localplayer as usize] = cmd;
    ticdata[(maketic % BACKUPTICS) as usize].ingame[localplayer as usize] = true_0 as boolean;
    maketic += 1;
    return true_0 as boolean;
}
#[no_mangle]
pub static mut lasttime: ::core::ffi::c_int = 0;
#[no_mangle]
pub unsafe extern "C" fn NetUpdate() {
    let mut nowtime: ::core::ffi::c_int = 0;
    let mut newtics: ::core::ffi::c_int = 0;
    let mut i: ::core::ffi::c_int = 0;
    if singletics != 0 {
        return;
    }
    nowtime = GetAdjustedTime() / ticdup;
    newtics = nowtime - lasttime;
    lasttime = nowtime;
    if skiptics <= newtics {
        newtics -= skiptics;
        skiptics = 0 as ::core::ffi::c_int;
    } else {
        skiptics -= newtics;
        newtics = 0 as ::core::ffi::c_int;
    }
    i = 0 as ::core::ffi::c_int;
    while i < newtics {
        if BuildNewTic() == 0 {
            break;
        }
        i += 1;
    }
}
unsafe extern "C" fn D_Disconnected() {
    if drone != 0 {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"Disconnected from server in drone mode.\0".as_ptr() as *const ::core::ffi::c_char,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    printf(b"Disconnected from server.\n\0".as_ptr() as *const ::core::ffi::c_char);
}
#[no_mangle]
pub unsafe extern "C" fn D_ReceiveTic(mut ticcmds: *mut ticcmd_t, mut players_mask: *mut boolean) {
    let mut i: ::core::ffi::c_int = 0;
    if ticcmds.is_null() && players_mask.is_null() {
        D_Disconnected();
        return;
    }
    i = 0 as ::core::ffi::c_int;
    while i < NET_MAXPLAYERS {
        if !(drone == 0 && i == localplayer) {
            ticdata[(recvtic % BACKUPTICS) as usize].cmds[i as usize] = *ticcmds.offset(i as isize);
            ticdata[(recvtic % BACKUPTICS) as usize].ingame[i as usize] =
                *players_mask.offset(i as isize);
        }
        i += 1;
    }
    recvtic += 1;
}
#[no_mangle]
pub unsafe extern "C" fn D_StartGameLoop() {
    lasttime = GetAdjustedTime() / ticdup;
}
#[no_mangle]
pub unsafe extern "C" fn D_StartNetGame(
    mut settings: *mut net_gamesettings_t,
    mut callback: netgame_startup_callback_t,
) {
    (*settings).consoleplayer = 0 as ::core::ffi::c_int;
    (*settings).num_players = 1 as ::core::ffi::c_int;
    (*settings).player_classes[0 as ::core::ffi::c_int as usize] = player_class;
    (*settings).new_sync = 0 as ::core::ffi::c_int;
    (*settings).extratics = 1 as ::core::ffi::c_int;
    (*settings).ticdup = 1 as ::core::ffi::c_int;
    ticdup = (*settings).ticdup;
    new_sync = (*settings).new_sync as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn D_InitNetGame(mut connect_data: *mut net_connect_data_t) -> boolean {
    let mut result: boolean = false_0 as boolean;
    I_AtExit(
        Some(D_QuitNetGame as unsafe extern "C" fn() -> ()),
        true_0 as boolean,
    );
    player_class = (*connect_data).player_class;
    return result;
}
#[no_mangle]
pub unsafe extern "C" fn D_QuitNetGame() {}
unsafe extern "C" fn GetLowTic() -> ::core::ffi::c_int {
    let mut lowtic: ::core::ffi::c_int = 0;
    lowtic = maketic;
    return lowtic;
}
static mut frameon: ::core::ffi::c_int = 0;
static mut frameskip: [::core::ffi::c_int; 4] = [0; 4];
static mut oldnettics: ::core::ffi::c_int = 0;
unsafe extern "C" fn OldNetSync() {
    let mut i: ::core::ffi::c_uint = 0;
    let mut keyplayer: ::core::ffi::c_int = -1 as ::core::ffi::c_int;
    frameon += 1;
    i = 0 as ::core::ffi::c_uint;
    while i < NET_MAXPLAYERS as ::core::ffi::c_uint {
        if local_playeringame[i as usize] != 0 {
            keyplayer = i as ::core::ffi::c_int;
            break;
        } else {
            i = i.wrapping_add(1);
        }
    }
    if keyplayer < 0 as ::core::ffi::c_int {
        return;
    }
    if !(localplayer == keyplayer) {
        if maketic <= recvtic {
            lasttime -= 1;
        }
        frameskip[(frameon & 3 as ::core::ffi::c_int) as usize] =
            (oldnettics > recvtic) as ::core::ffi::c_int;
        oldnettics = maketic;
        if frameskip[0 as ::core::ffi::c_int as usize] != 0
            && frameskip[1 as ::core::ffi::c_int as usize] != 0
            && frameskip[2 as ::core::ffi::c_int as usize] != 0
            && frameskip[3 as ::core::ffi::c_int as usize] != 0
        {
            skiptics = 1 as ::core::ffi::c_int;
        }
    }
}
unsafe extern "C" fn PlayersInGame() -> boolean {
    let mut result: boolean = false_0 as boolean;
    let mut i: ::core::ffi::c_uint = 0;
    if net_client_connected != 0 {
        i = 0 as ::core::ffi::c_uint;
        while i < NET_MAXPLAYERS as ::core::ffi::c_uint {
            result = (result != 0 || local_playeringame[i as usize] != 0) as ::core::ffi::c_int
                as boolean;
            i = i.wrapping_add(1);
        }
    }
    if drone == 0 {
        result = true_0 as boolean;
    }
    return result;
}
unsafe extern "C" fn TicdupSquash(mut set: *mut ticcmd_set_t) {
    let mut cmd: *mut ticcmd_t = ::core::ptr::null_mut::<ticcmd_t>();
    let mut i: ::core::ffi::c_uint = 0;
    i = 0 as ::core::ffi::c_uint;
    while i < NET_MAXPLAYERS as ::core::ffi::c_uint {
        cmd = (&raw mut (*set).cmds as *mut ticcmd_t).offset(i as isize) as *mut ticcmd_t;
        (*cmd).chatchar = 0 as byte;
        if (*cmd).buttons as ::core::ffi::c_int & BT_SPECIAL as ::core::ffi::c_int != 0 {
            (*cmd).buttons = 0 as byte;
        }
        i = i.wrapping_add(1);
    }
}
unsafe extern "C" fn SinglePlayerClear(mut set: *mut ticcmd_set_t) {
    let mut i: ::core::ffi::c_uint = 0;
    i = 0 as ::core::ffi::c_uint;
    while i < NET_MAXPLAYERS as ::core::ffi::c_uint {
        if i != localplayer as ::core::ffi::c_uint {
            (*set).ingame[i as usize] = false_0 as boolean;
        }
        i = i.wrapping_add(1);
    }
}
#[no_mangle]
pub unsafe extern "C" fn TryRunTics() {
    let mut i: ::core::ffi::c_int = 0;
    let mut lowtic: ::core::ffi::c_int = 0;
    let mut entertic: ::core::ffi::c_int = 0;
    static mut oldentertics: ::core::ffi::c_int = 0;
    let mut realtics: ::core::ffi::c_int = 0;
    let mut availabletics: ::core::ffi::c_int = 0;
    let mut counts: ::core::ffi::c_int = 0;
    entertic = I_GetTime() / ticdup;
    realtics = entertic - oldentertics;
    oldentertics = entertic;
    if singletics != 0 {
        BuildNewTic();
    } else {
        NetUpdate();
    }
    lowtic = GetLowTic();
    availabletics = lowtic - gametic / ticdup;
    if new_sync != 0 {
        counts = availabletics;
    } else {
        if realtics < availabletics - 1 as ::core::ffi::c_int {
            counts = realtics + 1 as ::core::ffi::c_int;
        } else if realtics < availabletics {
            counts = realtics;
        } else {
            counts = availabletics;
        }
        if counts < 1 as ::core::ffi::c_int {
            counts = 1 as ::core::ffi::c_int;
        }
        if net_client_connected != 0 {
            OldNetSync();
        }
    }
    if counts < 1 as ::core::ffi::c_int {
        counts = 1 as ::core::ffi::c_int;
    }
    while PlayersInGame() == 0 || lowtic < gametic / ticdup + counts {
        NetUpdate();
        lowtic = GetLowTic();
        if lowtic < gametic / ticdup {
            let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
            snprintf(
                &raw mut _i_err_buf as *mut ::core::ffi::c_char,
                ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
                b"TryRunTics: lowtic < gametic\0".as_ptr() as *const ::core::ffi::c_char,
            );
            I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
        }
        if I_GetTime() / ticdup - entertic > 0 as ::core::ffi::c_int {
            return;
        }
        I_Sleep(1 as ::core::ffi::c_int);
    }
    loop {
        let c2rust_fresh0 = counts;
        counts = counts - 1;
        if !(c2rust_fresh0 != 0) {
            break;
        }
        let mut set: *mut ticcmd_set_t = ::core::ptr::null_mut::<ticcmd_set_t>();
        if PlayersInGame() == 0 {
            return;
        }
        set = (&raw mut ticdata as *mut ticcmd_set_t)
            .offset((gametic / ticdup % BACKUPTICS) as isize) as *mut ticcmd_set_t;
        if net_client_connected == 0 {
            SinglePlayerClear(set);
        }
        i = 0 as ::core::ffi::c_int;
        while i < ticdup {
            if gametic / ticdup > lowtic {
                let mut _i_err_buf_0: [::core::ffi::c_char; 512] = [0; 512];
                snprintf(
                    &raw mut _i_err_buf_0 as *mut ::core::ffi::c_char,
                    ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
                    b"gametic>lowtic\0".as_ptr() as *const ::core::ffi::c_char,
                );
                I_ErrorV(&raw mut _i_err_buf_0 as *mut ::core::ffi::c_char);
            }
            memcpy(
                &raw mut local_playeringame as *mut boolean as *mut ::core::ffi::c_void,
                &raw mut (*set).ingame as *mut boolean as *const ::core::ffi::c_void,
                ::core::mem::size_of::<[boolean; 8]>() as size_t,
            );
            (*loop_interface).RunTic.expect("non-null function pointer")(
                &raw mut (*set).cmds as *mut ticcmd_t,
                &raw mut (*set).ingame as *mut boolean,
            );
            gametic += 1;
            TicdupSquash(set);
            i += 1;
        }
        NetUpdate();
    }
}
#[no_mangle]
pub unsafe extern "C" fn D_RegisterLoopCallbacks(mut i: *mut loop_interface_t) {
    loop_interface = i;
}
