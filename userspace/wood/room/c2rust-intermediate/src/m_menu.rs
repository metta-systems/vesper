extern "C" {
    pub type _IO_wide_data;
    pub type _IO_codecvt;
    pub type _IO_marker;
    fn toupper(__c: ::core::ffi::c_int) -> ::core::ffi::c_int;
    static mut stderr: *mut FILE;
    fn fclose(__stream: *mut FILE) -> ::core::ffi::c_int;
    fn fopen(
        __filename: *const ::core::ffi::c_char,
        __modes: *const ::core::ffi::c_char,
    ) -> *mut FILE;
    fn fprintf(
        __stream: *mut FILE,
        __format: *const ::core::ffi::c_char,
        ...
    ) -> ::core::ffi::c_int;
    fn snprintf(
        __s: *mut ::core::ffi::c_char,
        __maxlen: size_t,
        __format: *const ::core::ffi::c_char,
        ...
    ) -> ::core::ffi::c_int;
    fn fread(
        __ptr: *mut ::core::ffi::c_void,
        __size: size_t,
        __n: size_t,
        __stream: *mut FILE,
    ) -> ::core::ffi::c_ulong;
    fn strcmp(
        __s1: *const ::core::ffi::c_char,
        __s2: *const ::core::ffi::c_char,
    ) -> ::core::ffi::c_int;
    fn strlen(__s: *const ::core::ffi::c_char) -> size_t;
    fn I_GetTime() -> ::core::ffi::c_int;
    fn I_WaitVBL(count: ::core::ffi::c_int);
    static mut doom1_endmsg: [*mut ::core::ffi::c_char; 0];
    static mut doom2_endmsg: [*mut ::core::ffi::c_char; 0];
    fn D_StartTitle();
    fn I_Quit();
    fn I_ErrorV(msg: *const ::core::ffi::c_char);
    fn I_SetPalette(palette: *mut byte);
    static mut vanilla_keyboard_mapping: ::core::ffi::c_int;
    static mut usegamma: ::core::ffi::c_int;
    fn M_StringCopy(
        dest: *mut ::core::ffi::c_char,
        src: *const ::core::ffi::c_char,
        dest_size: size_t,
    ) -> boolean;
    fn V_DrawPatchDirect(x: ::core::ffi::c_int, y: ::core::ffi::c_int, patch: *mut patch_t);
    fn W_CacheLumpName(
        name: *mut ::core::ffi::c_char,
        tag: ::core::ffi::c_int,
    ) -> *mut ::core::ffi::c_void;
    fn R_SetViewSize(blocks: ::core::ffi::c_int, detail_0: ::core::ffi::c_int);
    fn G_DeferedInitNew(skill: skill_t, episode: ::core::ffi::c_int, map: ::core::ffi::c_int);
    fn G_LoadGame(name: *mut ::core::ffi::c_char);
    fn G_SaveGame(slot: ::core::ffi::c_int, description: *mut ::core::ffi::c_char);
    fn G_ScreenShot();
    static mut key_menu_activate: ::core::ffi::c_int;
    static mut key_menu_up: ::core::ffi::c_int;
    static mut key_menu_down: ::core::ffi::c_int;
    static mut key_menu_left: ::core::ffi::c_int;
    static mut key_menu_right: ::core::ffi::c_int;
    static mut key_menu_back: ::core::ffi::c_int;
    static mut key_menu_forward: ::core::ffi::c_int;
    static mut key_menu_confirm: ::core::ffi::c_int;
    static mut key_menu_abort: ::core::ffi::c_int;
    static mut key_menu_help: ::core::ffi::c_int;
    static mut key_menu_save: ::core::ffi::c_int;
    static mut key_menu_load: ::core::ffi::c_int;
    static mut key_menu_volume: ::core::ffi::c_int;
    static mut key_menu_detail: ::core::ffi::c_int;
    static mut key_menu_qsave: ::core::ffi::c_int;
    static mut key_menu_endgame: ::core::ffi::c_int;
    static mut key_menu_messages: ::core::ffi::c_int;
    static mut key_menu_qload: ::core::ffi::c_int;
    static mut key_menu_quit: ::core::ffi::c_int;
    static mut key_menu_gamma: ::core::ffi::c_int;
    static mut key_menu_incscreen: ::core::ffi::c_int;
    static mut key_menu_decscreen: ::core::ffi::c_int;
    static mut key_menu_screenshot: ::core::ffi::c_int;
    static mut joybmenu: ::core::ffi::c_int;
    fn P_SaveGameFile(slot: ::core::ffi::c_int) -> *mut ::core::ffi::c_char;
    fn S_StartSound(origin: *mut ::core::ffi::c_void, sound_id: ::core::ffi::c_int);
    fn S_SetMusicVolume(volume: ::core::ffi::c_int);
    fn S_SetSfxVolume(volume: ::core::ffi::c_int);
    static mut gametic: ::core::ffi::c_int;
    static mut devparm: boolean;
    static mut gamemode: GameMode_t;
    static mut gamemission: GameMission_t;
    static mut gameversion: GameVersion_t;
    static mut netgame: boolean;
    static mut sfxVolume: ::core::ffi::c_int;
    static mut musicVolume: ::core::ffi::c_int;
    static mut automapactive: boolean;
    static mut testcontrols: boolean;
    static mut consoleplayer: ::core::ffi::c_int;
    static mut usergame: boolean;
    static mut demoplayback: boolean;
    static mut gamestate: gamestate_t;
    static mut players: [player_t; 4];
    static mut hu_font: [*mut patch_t; 63];
    static mut message_dontfuckwithme: boolean;
    static mut chat_on: boolean;
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
pub type skill_t = ::core::ffi::c_int;
pub const sk_nightmare: skill_t = 4;
pub const sk_hard: skill_t = 3;
pub const sk_medium: skill_t = 2;
pub const sk_easy: skill_t = 1;
pub const sk_baby: skill_t = 0;
pub const sk_noitems: skill_t = -1;
pub type gamestate_t = ::core::ffi::c_uint;
pub const GS_DEMOSCREEN: gamestate_t = 3;
pub const GS_FINALE: gamestate_t = 2;
pub const GS_INTERMISSION: gamestate_t = 1;
pub const GS_LEVEL: gamestate_t = 0;
pub type weapontype_t = ::core::ffi::c_uint;
pub const wp_nochange: weapontype_t = 10;
pub const NUMWEAPONS: weapontype_t = 9;
pub const wp_supershotgun: weapontype_t = 8;
pub const wp_chainsaw: weapontype_t = 7;
pub const wp_bfg: weapontype_t = 6;
pub const wp_plasma: weapontype_t = 5;
pub const wp_missile: weapontype_t = 4;
pub const wp_chaingun: weapontype_t = 3;
pub const wp_shotgun: weapontype_t = 2;
pub const wp_pistol: weapontype_t = 1;
pub const wp_fist: weapontype_t = 0;
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
pub type evtype_t = ::core::ffi::c_uint;
pub const ev_quit: evtype_t = 4;
pub const ev_joystick: evtype_t = 3;
pub const ev_mouse: evtype_t = 2;
pub const ev_keyup: evtype_t = 1;
pub const ev_keydown: evtype_t = 0;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct event_t {
    pub type_0: evtype_t,
    pub data1: ::core::ffi::c_int,
    pub data2: ::core::ffi::c_int,
    pub data3: ::core::ffi::c_int,
    pub data4: ::core::ffi::c_int,
}
#[derive(Copy, Clone)]
#[repr(C, packed)]
pub struct patch_t {
    pub width: ::core::ffi::c_short,
    pub height: ::core::ffi::c_short,
    pub leftoffset: ::core::ffi::c_short,
    pub topoffset: ::core::ffi::c_short,
    pub columnofs: [::core::ffi::c_int; 8],
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
pub type fixed_t = ::core::ffi::c_int;
pub type angle_t = ::core::ffi::c_uint;
pub type actionf_v = Option<unsafe extern "C" fn() -> ()>;
pub type actionf_p1 = Option<unsafe extern "C" fn(*mut ::core::ffi::c_void) -> ()>;
pub type actionf_p2 =
    Option<unsafe extern "C" fn(*mut ::core::ffi::c_void, *mut ::core::ffi::c_void) -> ()>;
#[derive(Copy, Clone)]
#[repr(C)]
pub union actionf_t {
    pub acv: actionf_v,
    pub acp1: actionf_p1,
    pub acp2: actionf_p2,
}
pub type think_t = actionf_t;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct thinker_s {
    pub prev: *mut thinker_s,
    pub next: *mut thinker_s,
    pub function: think_t,
}
pub type thinker_t = thinker_s;
#[derive(Copy, Clone)]
#[repr(C, packed)]
pub struct mapthing_t {
    pub x: ::core::ffi::c_short,
    pub y: ::core::ffi::c_short,
    pub angle: ::core::ffi::c_short,
    pub type_0: ::core::ffi::c_short,
    pub options: ::core::ffi::c_short,
}
pub type spritenum_t = ::core::ffi::c_uint;
pub const NUMSPRITES: spritenum_t = 138;
pub const SPR_TLP2: spritenum_t = 137;
pub const SPR_TLMP: spritenum_t = 136;
pub const SPR_BRS1: spritenum_t = 135;
pub const SPR_POB2: spritenum_t = 134;
pub const SPR_POB1: spritenum_t = 133;
pub const SPR_HDB6: spritenum_t = 132;
pub const SPR_HDB5: spritenum_t = 131;
pub const SPR_HDB4: spritenum_t = 130;
pub const SPR_HDB3: spritenum_t = 129;
pub const SPR_HDB2: spritenum_t = 128;
pub const SPR_HDB1: spritenum_t = 127;
pub const SPR_SMRT: spritenum_t = 126;
pub const SPR_SMGT: spritenum_t = 125;
pub const SPR_SMBT: spritenum_t = 124;
pub const SPR_TRED: spritenum_t = 123;
pub const SPR_TGRN: spritenum_t = 122;
pub const SPR_TBLU: spritenum_t = 121;
pub const SPR_COL5: spritenum_t = 120;
pub const SPR_FSKU: spritenum_t = 119;
pub const SPR_CEYE: spritenum_t = 118;
pub const SPR_ELEC: spritenum_t = 117;
pub const SPR_TRE2: spritenum_t = 116;
pub const SPR_TRE1: spritenum_t = 115;
pub const SPR_COL6: spritenum_t = 114;
pub const SPR_CBRA: spritenum_t = 113;
pub const SPR_CAND: spritenum_t = 112;
pub const SPR_COL4: spritenum_t = 111;
pub const SPR_COL3: spritenum_t = 110;
pub const SPR_COL2: spritenum_t = 109;
pub const SPR_COL1: spritenum_t = 108;
pub const SPR_SMIT: spritenum_t = 107;
pub const SPR_GOR5: spritenum_t = 106;
pub const SPR_GOR4: spritenum_t = 105;
pub const SPR_GOR3: spritenum_t = 104;
pub const SPR_GOR2: spritenum_t = 103;
pub const SPR_POL6: spritenum_t = 102;
pub const SPR_POL1: spritenum_t = 101;
pub const SPR_POL3: spritenum_t = 100;
pub const SPR_POL4: spritenum_t = 99;
pub const SPR_POL5: spritenum_t = 98;
pub const SPR_POL2: spritenum_t = 97;
pub const SPR_GOR1: spritenum_t = 96;
pub const SPR_SMT2: spritenum_t = 95;
pub const SPR_COLU: spritenum_t = 94;
pub const SPR_SGN2: spritenum_t = 93;
pub const SPR_SHOT: spritenum_t = 92;
pub const SPR_PLAS: spritenum_t = 91;
pub const SPR_LAUN: spritenum_t = 90;
pub const SPR_CSAW: spritenum_t = 89;
pub const SPR_MGUN: spritenum_t = 88;
pub const SPR_BFUG: spritenum_t = 87;
pub const SPR_BPAK: spritenum_t = 86;
pub const SPR_SBOX: spritenum_t = 85;
pub const SPR_SHEL: spritenum_t = 84;
pub const SPR_CELP: spritenum_t = 83;
pub const SPR_CELL: spritenum_t = 82;
pub const SPR_BROK: spritenum_t = 81;
pub const SPR_ROCK: spritenum_t = 80;
pub const SPR_AMMO: spritenum_t = 79;
pub const SPR_CLIP: spritenum_t = 78;
pub const SPR_PVIS: spritenum_t = 77;
pub const SPR_PMAP: spritenum_t = 76;
pub const SPR_SUIT: spritenum_t = 75;
pub const SPR_MEGA: spritenum_t = 74;
pub const SPR_PINS: spritenum_t = 73;
pub const SPR_PSTR: spritenum_t = 72;
pub const SPR_PINV: spritenum_t = 71;
pub const SPR_SOUL: spritenum_t = 70;
pub const SPR_MEDI: spritenum_t = 69;
pub const SPR_STIM: spritenum_t = 68;
pub const SPR_YSKU: spritenum_t = 67;
pub const SPR_RSKU: spritenum_t = 66;
pub const SPR_BSKU: spritenum_t = 65;
pub const SPR_YKEY: spritenum_t = 64;
pub const SPR_RKEY: spritenum_t = 63;
pub const SPR_BKEY: spritenum_t = 62;
pub const SPR_BON2: spritenum_t = 61;
pub const SPR_BON1: spritenum_t = 60;
pub const SPR_FCAN: spritenum_t = 59;
pub const SPR_BEXP: spritenum_t = 58;
pub const SPR_BAR1: spritenum_t = 57;
pub const SPR_ARM2: spritenum_t = 56;
pub const SPR_ARM1: spritenum_t = 55;
pub const SPR_BOSF: spritenum_t = 54;
pub const SPR_BBRN: spritenum_t = 53;
pub const SPR_KEEN: spritenum_t = 52;
pub const SPR_SSWV: spritenum_t = 51;
pub const SPR_PAIN: spritenum_t = 50;
pub const SPR_CYBR: spritenum_t = 49;
pub const SPR_APBX: spritenum_t = 48;
pub const SPR_APLS: spritenum_t = 47;
pub const SPR_BSPI: spritenum_t = 46;
pub const SPR_SPID: spritenum_t = 45;
pub const SPR_SKUL: spritenum_t = 44;
pub const SPR_BOS2: spritenum_t = 43;
pub const SPR_BOSS: spritenum_t = 42;
pub const SPR_BAL7: spritenum_t = 41;
pub const SPR_HEAD: spritenum_t = 40;
pub const SPR_SARG: spritenum_t = 39;
pub const SPR_CPOS: spritenum_t = 38;
pub const SPR_FATT: spritenum_t = 37;
pub const SPR_MANF: spritenum_t = 36;
pub const SPR_SKEL: spritenum_t = 35;
pub const SPR_FBXP: spritenum_t = 34;
pub const SPR_FATB: spritenum_t = 33;
pub const SPR_FIRE: spritenum_t = 32;
pub const SPR_VILE: spritenum_t = 31;
pub const SPR_SPOS: spritenum_t = 30;
pub const SPR_POSS: spritenum_t = 29;
pub const SPR_PLAY: spritenum_t = 28;
pub const SPR_IFOG: spritenum_t = 27;
pub const SPR_TFOG: spritenum_t = 26;
pub const SPR_BFE2: spritenum_t = 25;
pub const SPR_BFE1: spritenum_t = 24;
pub const SPR_BFS1: spritenum_t = 23;
pub const SPR_MISL: spritenum_t = 22;
pub const SPR_PLSE: spritenum_t = 21;
pub const SPR_PLSS: spritenum_t = 20;
pub const SPR_BAL2: spritenum_t = 19;
pub const SPR_BAL1: spritenum_t = 18;
pub const SPR_PUFF: spritenum_t = 17;
pub const SPR_BLUD: spritenum_t = 16;
pub const SPR_BFGF: spritenum_t = 15;
pub const SPR_BFGG: spritenum_t = 14;
pub const SPR_PLSF: spritenum_t = 13;
pub const SPR_PLSG: spritenum_t = 12;
pub const SPR_SAWG: spritenum_t = 11;
pub const SPR_MISF: spritenum_t = 10;
pub const SPR_MISG: spritenum_t = 9;
pub const SPR_CHGF: spritenum_t = 8;
pub const SPR_CHGG: spritenum_t = 7;
pub const SPR_SHT2: spritenum_t = 6;
pub const SPR_SHTF: spritenum_t = 5;
pub const SPR_PISF: spritenum_t = 4;
pub const SPR_PISG: spritenum_t = 3;
pub const SPR_PUNG: spritenum_t = 2;
pub const SPR_SHTG: spritenum_t = 1;
pub const SPR_TROO: spritenum_t = 0;
pub type statenum_t = ::core::ffi::c_uint;
pub const NUMSTATES: statenum_t = 967;
pub const S_TECH2LAMP4: statenum_t = 966;
pub const S_TECH2LAMP3: statenum_t = 965;
pub const S_TECH2LAMP2: statenum_t = 964;
pub const S_TECH2LAMP: statenum_t = 963;
pub const S_TECHLAMP4: statenum_t = 962;
pub const S_TECHLAMP3: statenum_t = 961;
pub const S_TECHLAMP2: statenum_t = 960;
pub const S_TECHLAMP: statenum_t = 959;
pub const S_BRAINSTEM: statenum_t = 958;
pub const S_SMALLPOOL: statenum_t = 957;
pub const S_COLONGIBS: statenum_t = 956;
pub const S_HANGTNOBRAIN: statenum_t = 955;
pub const S_HANGTLOOKUP: statenum_t = 954;
pub const S_HANGTSKULL: statenum_t = 953;
pub const S_HANGTLOOKDN: statenum_t = 952;
pub const S_HANGBNOBRAIN: statenum_t = 951;
pub const S_HANGNOGUTS: statenum_t = 950;
pub const S_RTORCHSHRT4: statenum_t = 949;
pub const S_RTORCHSHRT3: statenum_t = 948;
pub const S_RTORCHSHRT2: statenum_t = 947;
pub const S_RTORCHSHRT: statenum_t = 946;
pub const S_GTORCHSHRT4: statenum_t = 945;
pub const S_GTORCHSHRT3: statenum_t = 944;
pub const S_GTORCHSHRT2: statenum_t = 943;
pub const S_GTORCHSHRT: statenum_t = 942;
pub const S_BTORCHSHRT4: statenum_t = 941;
pub const S_BTORCHSHRT3: statenum_t = 940;
pub const S_BTORCHSHRT2: statenum_t = 939;
pub const S_BTORCHSHRT: statenum_t = 938;
pub const S_REDTORCH4: statenum_t = 937;
pub const S_REDTORCH3: statenum_t = 936;
pub const S_REDTORCH2: statenum_t = 935;
pub const S_REDTORCH: statenum_t = 934;
pub const S_GREENTORCH4: statenum_t = 933;
pub const S_GREENTORCH3: statenum_t = 932;
pub const S_GREENTORCH2: statenum_t = 931;
pub const S_GREENTORCH: statenum_t = 930;
pub const S_BLUETORCH4: statenum_t = 929;
pub const S_BLUETORCH3: statenum_t = 928;
pub const S_BLUETORCH2: statenum_t = 927;
pub const S_BLUETORCH: statenum_t = 926;
pub const S_HEARTCOL2: statenum_t = 925;
pub const S_HEARTCOL: statenum_t = 924;
pub const S_FLOATSKULL3: statenum_t = 923;
pub const S_FLOATSKULL2: statenum_t = 922;
pub const S_FLOATSKULL: statenum_t = 921;
pub const S_EVILEYE4: statenum_t = 920;
pub const S_EVILEYE3: statenum_t = 919;
pub const S_EVILEYE2: statenum_t = 918;
pub const S_EVILEYE: statenum_t = 917;
pub const S_TECHPILLAR: statenum_t = 916;
pub const S_BIGTREE: statenum_t = 915;
pub const S_TORCHTREE: statenum_t = 914;
pub const S_SKULLCOL: statenum_t = 913;
pub const S_CANDELABRA: statenum_t = 912;
pub const S_CANDLESTIK: statenum_t = 911;
pub const S_SHRTREDCOL: statenum_t = 910;
pub const S_TALLREDCOL: statenum_t = 909;
pub const S_SHRTGRNCOL: statenum_t = 908;
pub const S_TALLGRNCOL: statenum_t = 907;
pub const S_STALAGTITE: statenum_t = 906;
pub const S_MEAT5: statenum_t = 905;
pub const S_MEAT4: statenum_t = 904;
pub const S_MEAT3: statenum_t = 903;
pub const S_MEAT2: statenum_t = 902;
pub const S_LIVESTICK2: statenum_t = 901;
pub const S_LIVESTICK: statenum_t = 900;
pub const S_DEADSTICK: statenum_t = 899;
pub const S_HEADCANDLES2: statenum_t = 898;
pub const S_HEADCANDLES: statenum_t = 897;
pub const S_HEADONASTICK: statenum_t = 896;
pub const S_GIBS: statenum_t = 895;
pub const S_HEADSONSTICK: statenum_t = 894;
pub const S_DEADBOTTOM: statenum_t = 893;
pub const S_DEADTORSO: statenum_t = 892;
pub const S_BLOODYTWITCH4: statenum_t = 891;
pub const S_BLOODYTWITCH3: statenum_t = 890;
pub const S_BLOODYTWITCH2: statenum_t = 889;
pub const S_BLOODYTWITCH: statenum_t = 888;
pub const S_STALAG: statenum_t = 887;
pub const S_COLU: statenum_t = 886;
pub const S_SHOT2: statenum_t = 885;
pub const S_SHOT: statenum_t = 884;
pub const S_PLAS: statenum_t = 883;
pub const S_LAUN: statenum_t = 882;
pub const S_CSAW: statenum_t = 881;
pub const S_MGUN: statenum_t = 880;
pub const S_BFUG: statenum_t = 879;
pub const S_BPAK: statenum_t = 878;
pub const S_SBOX: statenum_t = 877;
pub const S_SHEL: statenum_t = 876;
pub const S_CELP: statenum_t = 875;
pub const S_CELL: statenum_t = 874;
pub const S_BROK: statenum_t = 873;
pub const S_ROCK: statenum_t = 872;
pub const S_AMMO: statenum_t = 871;
pub const S_CLIP: statenum_t = 870;
pub const S_PVIS2: statenum_t = 869;
pub const S_PVIS: statenum_t = 868;
pub const S_PMAP6: statenum_t = 867;
pub const S_PMAP5: statenum_t = 866;
pub const S_PMAP4: statenum_t = 865;
pub const S_PMAP3: statenum_t = 864;
pub const S_PMAP2: statenum_t = 863;
pub const S_PMAP: statenum_t = 862;
pub const S_SUIT: statenum_t = 861;
pub const S_MEGA4: statenum_t = 860;
pub const S_MEGA3: statenum_t = 859;
pub const S_MEGA2: statenum_t = 858;
pub const S_MEGA: statenum_t = 857;
pub const S_PINS4: statenum_t = 856;
pub const S_PINS3: statenum_t = 855;
pub const S_PINS2: statenum_t = 854;
pub const S_PINS: statenum_t = 853;
pub const S_PSTR: statenum_t = 852;
pub const S_PINV4: statenum_t = 851;
pub const S_PINV3: statenum_t = 850;
pub const S_PINV2: statenum_t = 849;
pub const S_PINV: statenum_t = 848;
pub const S_SOUL6: statenum_t = 847;
pub const S_SOUL5: statenum_t = 846;
pub const S_SOUL4: statenum_t = 845;
pub const S_SOUL3: statenum_t = 844;
pub const S_SOUL2: statenum_t = 843;
pub const S_SOUL: statenum_t = 842;
pub const S_MEDI: statenum_t = 841;
pub const S_STIM: statenum_t = 840;
pub const S_YSKULL2: statenum_t = 839;
pub const S_YSKULL: statenum_t = 838;
pub const S_RSKULL2: statenum_t = 837;
pub const S_RSKULL: statenum_t = 836;
pub const S_BSKULL2: statenum_t = 835;
pub const S_BSKULL: statenum_t = 834;
pub const S_YKEY2: statenum_t = 833;
pub const S_YKEY: statenum_t = 832;
pub const S_RKEY2: statenum_t = 831;
pub const S_RKEY: statenum_t = 830;
pub const S_BKEY2: statenum_t = 829;
pub const S_BKEY: statenum_t = 828;
pub const S_BON2E: statenum_t = 827;
pub const S_BON2D: statenum_t = 826;
pub const S_BON2C: statenum_t = 825;
pub const S_BON2B: statenum_t = 824;
pub const S_BON2A: statenum_t = 823;
pub const S_BON2: statenum_t = 822;
pub const S_BON1E: statenum_t = 821;
pub const S_BON1D: statenum_t = 820;
pub const S_BON1C: statenum_t = 819;
pub const S_BON1B: statenum_t = 818;
pub const S_BON1A: statenum_t = 817;
pub const S_BON1: statenum_t = 816;
pub const S_BBAR3: statenum_t = 815;
pub const S_BBAR2: statenum_t = 814;
pub const S_BBAR1: statenum_t = 813;
pub const S_BEXP5: statenum_t = 812;
pub const S_BEXP4: statenum_t = 811;
pub const S_BEXP3: statenum_t = 810;
pub const S_BEXP2: statenum_t = 809;
pub const S_BEXP: statenum_t = 808;
pub const S_BAR2: statenum_t = 807;
pub const S_BAR1: statenum_t = 806;
pub const S_ARM2A: statenum_t = 805;
pub const S_ARM2: statenum_t = 804;
pub const S_ARM1A: statenum_t = 803;
pub const S_ARM1: statenum_t = 802;
pub const S_BRAINEXPLODE3: statenum_t = 801;
pub const S_BRAINEXPLODE2: statenum_t = 800;
pub const S_BRAINEXPLODE1: statenum_t = 799;
pub const S_SPAWNFIRE8: statenum_t = 798;
pub const S_SPAWNFIRE7: statenum_t = 797;
pub const S_SPAWNFIRE6: statenum_t = 796;
pub const S_SPAWNFIRE5: statenum_t = 795;
pub const S_SPAWNFIRE4: statenum_t = 794;
pub const S_SPAWNFIRE3: statenum_t = 793;
pub const S_SPAWNFIRE2: statenum_t = 792;
pub const S_SPAWNFIRE1: statenum_t = 791;
pub const S_SPAWN4: statenum_t = 790;
pub const S_SPAWN3: statenum_t = 789;
pub const S_SPAWN2: statenum_t = 788;
pub const S_SPAWN1: statenum_t = 787;
pub const S_BRAINEYE1: statenum_t = 786;
pub const S_BRAINEYESEE: statenum_t = 785;
pub const S_BRAINEYE: statenum_t = 784;
pub const S_BRAIN_DIE4: statenum_t = 783;
pub const S_BRAIN_DIE3: statenum_t = 782;
pub const S_BRAIN_DIE2: statenum_t = 781;
pub const S_BRAIN_DIE1: statenum_t = 780;
pub const S_BRAIN_PAIN: statenum_t = 779;
pub const S_BRAIN: statenum_t = 778;
pub const S_KEENPAIN2: statenum_t = 777;
pub const S_KEENPAIN: statenum_t = 776;
pub const S_COMMKEEN12: statenum_t = 775;
pub const S_COMMKEEN11: statenum_t = 774;
pub const S_COMMKEEN10: statenum_t = 773;
pub const S_COMMKEEN9: statenum_t = 772;
pub const S_COMMKEEN8: statenum_t = 771;
pub const S_COMMKEEN7: statenum_t = 770;
pub const S_COMMKEEN6: statenum_t = 769;
pub const S_COMMKEEN5: statenum_t = 768;
pub const S_COMMKEEN4: statenum_t = 767;
pub const S_COMMKEEN3: statenum_t = 766;
pub const S_COMMKEEN2: statenum_t = 765;
pub const S_COMMKEEN: statenum_t = 764;
pub const S_KEENSTND: statenum_t = 763;
pub const S_SSWV_RAISE5: statenum_t = 762;
pub const S_SSWV_RAISE4: statenum_t = 761;
pub const S_SSWV_RAISE3: statenum_t = 760;
pub const S_SSWV_RAISE2: statenum_t = 759;
pub const S_SSWV_RAISE1: statenum_t = 758;
pub const S_SSWV_XDIE9: statenum_t = 757;
pub const S_SSWV_XDIE8: statenum_t = 756;
pub const S_SSWV_XDIE7: statenum_t = 755;
pub const S_SSWV_XDIE6: statenum_t = 754;
pub const S_SSWV_XDIE5: statenum_t = 753;
pub const S_SSWV_XDIE4: statenum_t = 752;
pub const S_SSWV_XDIE3: statenum_t = 751;
pub const S_SSWV_XDIE2: statenum_t = 750;
pub const S_SSWV_XDIE1: statenum_t = 749;
pub const S_SSWV_DIE5: statenum_t = 748;
pub const S_SSWV_DIE4: statenum_t = 747;
pub const S_SSWV_DIE3: statenum_t = 746;
pub const S_SSWV_DIE2: statenum_t = 745;
pub const S_SSWV_DIE1: statenum_t = 744;
pub const S_SSWV_PAIN2: statenum_t = 743;
pub const S_SSWV_PAIN: statenum_t = 742;
pub const S_SSWV_ATK6: statenum_t = 741;
pub const S_SSWV_ATK5: statenum_t = 740;
pub const S_SSWV_ATK4: statenum_t = 739;
pub const S_SSWV_ATK3: statenum_t = 738;
pub const S_SSWV_ATK2: statenum_t = 737;
pub const S_SSWV_ATK1: statenum_t = 736;
pub const S_SSWV_RUN8: statenum_t = 735;
pub const S_SSWV_RUN7: statenum_t = 734;
pub const S_SSWV_RUN6: statenum_t = 733;
pub const S_SSWV_RUN5: statenum_t = 732;
pub const S_SSWV_RUN4: statenum_t = 731;
pub const S_SSWV_RUN3: statenum_t = 730;
pub const S_SSWV_RUN2: statenum_t = 729;
pub const S_SSWV_RUN1: statenum_t = 728;
pub const S_SSWV_STND2: statenum_t = 727;
pub const S_SSWV_STND: statenum_t = 726;
pub const S_PAIN_RAISE6: statenum_t = 725;
pub const S_PAIN_RAISE5: statenum_t = 724;
pub const S_PAIN_RAISE4: statenum_t = 723;
pub const S_PAIN_RAISE3: statenum_t = 722;
pub const S_PAIN_RAISE2: statenum_t = 721;
pub const S_PAIN_RAISE1: statenum_t = 720;
pub const S_PAIN_DIE6: statenum_t = 719;
pub const S_PAIN_DIE5: statenum_t = 718;
pub const S_PAIN_DIE4: statenum_t = 717;
pub const S_PAIN_DIE3: statenum_t = 716;
pub const S_PAIN_DIE2: statenum_t = 715;
pub const S_PAIN_DIE1: statenum_t = 714;
pub const S_PAIN_PAIN2: statenum_t = 713;
pub const S_PAIN_PAIN: statenum_t = 712;
pub const S_PAIN_ATK4: statenum_t = 711;
pub const S_PAIN_ATK3: statenum_t = 710;
pub const S_PAIN_ATK2: statenum_t = 709;
pub const S_PAIN_ATK1: statenum_t = 708;
pub const S_PAIN_RUN6: statenum_t = 707;
pub const S_PAIN_RUN5: statenum_t = 706;
pub const S_PAIN_RUN4: statenum_t = 705;
pub const S_PAIN_RUN3: statenum_t = 704;
pub const S_PAIN_RUN2: statenum_t = 703;
pub const S_PAIN_RUN1: statenum_t = 702;
pub const S_PAIN_STND: statenum_t = 701;
pub const S_CYBER_DIE10: statenum_t = 700;
pub const S_CYBER_DIE9: statenum_t = 699;
pub const S_CYBER_DIE8: statenum_t = 698;
pub const S_CYBER_DIE7: statenum_t = 697;
pub const S_CYBER_DIE6: statenum_t = 696;
pub const S_CYBER_DIE5: statenum_t = 695;
pub const S_CYBER_DIE4: statenum_t = 694;
pub const S_CYBER_DIE3: statenum_t = 693;
pub const S_CYBER_DIE2: statenum_t = 692;
pub const S_CYBER_DIE1: statenum_t = 691;
pub const S_CYBER_PAIN: statenum_t = 690;
pub const S_CYBER_ATK6: statenum_t = 689;
pub const S_CYBER_ATK5: statenum_t = 688;
pub const S_CYBER_ATK4: statenum_t = 687;
pub const S_CYBER_ATK3: statenum_t = 686;
pub const S_CYBER_ATK2: statenum_t = 685;
pub const S_CYBER_ATK1: statenum_t = 684;
pub const S_CYBER_RUN8: statenum_t = 683;
pub const S_CYBER_RUN7: statenum_t = 682;
pub const S_CYBER_RUN6: statenum_t = 681;
pub const S_CYBER_RUN5: statenum_t = 680;
pub const S_CYBER_RUN4: statenum_t = 679;
pub const S_CYBER_RUN3: statenum_t = 678;
pub const S_CYBER_RUN2: statenum_t = 677;
pub const S_CYBER_RUN1: statenum_t = 676;
pub const S_CYBER_STND2: statenum_t = 675;
pub const S_CYBER_STND: statenum_t = 674;
pub const S_ARACH_PLEX5: statenum_t = 673;
pub const S_ARACH_PLEX4: statenum_t = 672;
pub const S_ARACH_PLEX3: statenum_t = 671;
pub const S_ARACH_PLEX2: statenum_t = 670;
pub const S_ARACH_PLEX: statenum_t = 669;
pub const S_ARACH_PLAZ2: statenum_t = 668;
pub const S_ARACH_PLAZ: statenum_t = 667;
pub const S_BSPI_RAISE7: statenum_t = 666;
pub const S_BSPI_RAISE6: statenum_t = 665;
pub const S_BSPI_RAISE5: statenum_t = 664;
pub const S_BSPI_RAISE4: statenum_t = 663;
pub const S_BSPI_RAISE3: statenum_t = 662;
pub const S_BSPI_RAISE2: statenum_t = 661;
pub const S_BSPI_RAISE1: statenum_t = 660;
pub const S_BSPI_DIE7: statenum_t = 659;
pub const S_BSPI_DIE6: statenum_t = 658;
pub const S_BSPI_DIE5: statenum_t = 657;
pub const S_BSPI_DIE4: statenum_t = 656;
pub const S_BSPI_DIE3: statenum_t = 655;
pub const S_BSPI_DIE2: statenum_t = 654;
pub const S_BSPI_DIE1: statenum_t = 653;
pub const S_BSPI_PAIN2: statenum_t = 652;
pub const S_BSPI_PAIN: statenum_t = 651;
pub const S_BSPI_ATK4: statenum_t = 650;
pub const S_BSPI_ATK3: statenum_t = 649;
pub const S_BSPI_ATK2: statenum_t = 648;
pub const S_BSPI_ATK1: statenum_t = 647;
pub const S_BSPI_RUN12: statenum_t = 646;
pub const S_BSPI_RUN11: statenum_t = 645;
pub const S_BSPI_RUN10: statenum_t = 644;
pub const S_BSPI_RUN9: statenum_t = 643;
pub const S_BSPI_RUN8: statenum_t = 642;
pub const S_BSPI_RUN7: statenum_t = 641;
pub const S_BSPI_RUN6: statenum_t = 640;
pub const S_BSPI_RUN5: statenum_t = 639;
pub const S_BSPI_RUN4: statenum_t = 638;
pub const S_BSPI_RUN3: statenum_t = 637;
pub const S_BSPI_RUN2: statenum_t = 636;
pub const S_BSPI_RUN1: statenum_t = 635;
pub const S_BSPI_SIGHT: statenum_t = 634;
pub const S_BSPI_STND2: statenum_t = 633;
pub const S_BSPI_STND: statenum_t = 632;
pub const S_SPID_DIE11: statenum_t = 631;
pub const S_SPID_DIE10: statenum_t = 630;
pub const S_SPID_DIE9: statenum_t = 629;
pub const S_SPID_DIE8: statenum_t = 628;
pub const S_SPID_DIE7: statenum_t = 627;
pub const S_SPID_DIE6: statenum_t = 626;
pub const S_SPID_DIE5: statenum_t = 625;
pub const S_SPID_DIE4: statenum_t = 624;
pub const S_SPID_DIE3: statenum_t = 623;
pub const S_SPID_DIE2: statenum_t = 622;
pub const S_SPID_DIE1: statenum_t = 621;
pub const S_SPID_PAIN2: statenum_t = 620;
pub const S_SPID_PAIN: statenum_t = 619;
pub const S_SPID_ATK4: statenum_t = 618;
pub const S_SPID_ATK3: statenum_t = 617;
pub const S_SPID_ATK2: statenum_t = 616;
pub const S_SPID_ATK1: statenum_t = 615;
pub const S_SPID_RUN12: statenum_t = 614;
pub const S_SPID_RUN11: statenum_t = 613;
pub const S_SPID_RUN10: statenum_t = 612;
pub const S_SPID_RUN9: statenum_t = 611;
pub const S_SPID_RUN8: statenum_t = 610;
pub const S_SPID_RUN7: statenum_t = 609;
pub const S_SPID_RUN6: statenum_t = 608;
pub const S_SPID_RUN5: statenum_t = 607;
pub const S_SPID_RUN4: statenum_t = 606;
pub const S_SPID_RUN3: statenum_t = 605;
pub const S_SPID_RUN2: statenum_t = 604;
pub const S_SPID_RUN1: statenum_t = 603;
pub const S_SPID_STND2: statenum_t = 602;
pub const S_SPID_STND: statenum_t = 601;
pub const S_SKULL_DIE6: statenum_t = 600;
pub const S_SKULL_DIE5: statenum_t = 599;
pub const S_SKULL_DIE4: statenum_t = 598;
pub const S_SKULL_DIE3: statenum_t = 597;
pub const S_SKULL_DIE2: statenum_t = 596;
pub const S_SKULL_DIE1: statenum_t = 595;
pub const S_SKULL_PAIN2: statenum_t = 594;
pub const S_SKULL_PAIN: statenum_t = 593;
pub const S_SKULL_ATK4: statenum_t = 592;
pub const S_SKULL_ATK3: statenum_t = 591;
pub const S_SKULL_ATK2: statenum_t = 590;
pub const S_SKULL_ATK1: statenum_t = 589;
pub const S_SKULL_RUN2: statenum_t = 588;
pub const S_SKULL_RUN1: statenum_t = 587;
pub const S_SKULL_STND2: statenum_t = 586;
pub const S_SKULL_STND: statenum_t = 585;
pub const S_BOS2_RAISE7: statenum_t = 584;
pub const S_BOS2_RAISE6: statenum_t = 583;
pub const S_BOS2_RAISE5: statenum_t = 582;
pub const S_BOS2_RAISE4: statenum_t = 581;
pub const S_BOS2_RAISE3: statenum_t = 580;
pub const S_BOS2_RAISE2: statenum_t = 579;
pub const S_BOS2_RAISE1: statenum_t = 578;
pub const S_BOS2_DIE7: statenum_t = 577;
pub const S_BOS2_DIE6: statenum_t = 576;
pub const S_BOS2_DIE5: statenum_t = 575;
pub const S_BOS2_DIE4: statenum_t = 574;
pub const S_BOS2_DIE3: statenum_t = 573;
pub const S_BOS2_DIE2: statenum_t = 572;
pub const S_BOS2_DIE1: statenum_t = 571;
pub const S_BOS2_PAIN2: statenum_t = 570;
pub const S_BOS2_PAIN: statenum_t = 569;
pub const S_BOS2_ATK3: statenum_t = 568;
pub const S_BOS2_ATK2: statenum_t = 567;
pub const S_BOS2_ATK1: statenum_t = 566;
pub const S_BOS2_RUN8: statenum_t = 565;
pub const S_BOS2_RUN7: statenum_t = 564;
pub const S_BOS2_RUN6: statenum_t = 563;
pub const S_BOS2_RUN5: statenum_t = 562;
pub const S_BOS2_RUN4: statenum_t = 561;
pub const S_BOS2_RUN3: statenum_t = 560;
pub const S_BOS2_RUN2: statenum_t = 559;
pub const S_BOS2_RUN1: statenum_t = 558;
pub const S_BOS2_STND2: statenum_t = 557;
pub const S_BOS2_STND: statenum_t = 556;
pub const S_BOSS_RAISE7: statenum_t = 555;
pub const S_BOSS_RAISE6: statenum_t = 554;
pub const S_BOSS_RAISE5: statenum_t = 553;
pub const S_BOSS_RAISE4: statenum_t = 552;
pub const S_BOSS_RAISE3: statenum_t = 551;
pub const S_BOSS_RAISE2: statenum_t = 550;
pub const S_BOSS_RAISE1: statenum_t = 549;
pub const S_BOSS_DIE7: statenum_t = 548;
pub const S_BOSS_DIE6: statenum_t = 547;
pub const S_BOSS_DIE5: statenum_t = 546;
pub const S_BOSS_DIE4: statenum_t = 545;
pub const S_BOSS_DIE3: statenum_t = 544;
pub const S_BOSS_DIE2: statenum_t = 543;
pub const S_BOSS_DIE1: statenum_t = 542;
pub const S_BOSS_PAIN2: statenum_t = 541;
pub const S_BOSS_PAIN: statenum_t = 540;
pub const S_BOSS_ATK3: statenum_t = 539;
pub const S_BOSS_ATK2: statenum_t = 538;
pub const S_BOSS_ATK1: statenum_t = 537;
pub const S_BOSS_RUN8: statenum_t = 536;
pub const S_BOSS_RUN7: statenum_t = 535;
pub const S_BOSS_RUN6: statenum_t = 534;
pub const S_BOSS_RUN5: statenum_t = 533;
pub const S_BOSS_RUN4: statenum_t = 532;
pub const S_BOSS_RUN3: statenum_t = 531;
pub const S_BOSS_RUN2: statenum_t = 530;
pub const S_BOSS_RUN1: statenum_t = 529;
pub const S_BOSS_STND2: statenum_t = 528;
pub const S_BOSS_STND: statenum_t = 527;
pub const S_BRBALLX3: statenum_t = 526;
pub const S_BRBALLX2: statenum_t = 525;
pub const S_BRBALLX1: statenum_t = 524;
pub const S_BRBALL2: statenum_t = 523;
pub const S_BRBALL1: statenum_t = 522;
pub const S_HEAD_RAISE6: statenum_t = 521;
pub const S_HEAD_RAISE5: statenum_t = 520;
pub const S_HEAD_RAISE4: statenum_t = 519;
pub const S_HEAD_RAISE3: statenum_t = 518;
pub const S_HEAD_RAISE2: statenum_t = 517;
pub const S_HEAD_RAISE1: statenum_t = 516;
pub const S_HEAD_DIE6: statenum_t = 515;
pub const S_HEAD_DIE5: statenum_t = 514;
pub const S_HEAD_DIE4: statenum_t = 513;
pub const S_HEAD_DIE3: statenum_t = 512;
pub const S_HEAD_DIE2: statenum_t = 511;
pub const S_HEAD_DIE1: statenum_t = 510;
pub const S_HEAD_PAIN3: statenum_t = 509;
pub const S_HEAD_PAIN2: statenum_t = 508;
pub const S_HEAD_PAIN: statenum_t = 507;
pub const S_HEAD_ATK3: statenum_t = 506;
pub const S_HEAD_ATK2: statenum_t = 505;
pub const S_HEAD_ATK1: statenum_t = 504;
pub const S_HEAD_RUN1: statenum_t = 503;
pub const S_HEAD_STND: statenum_t = 502;
pub const S_SARG_RAISE6: statenum_t = 501;
pub const S_SARG_RAISE5: statenum_t = 500;
pub const S_SARG_RAISE4: statenum_t = 499;
pub const S_SARG_RAISE3: statenum_t = 498;
pub const S_SARG_RAISE2: statenum_t = 497;
pub const S_SARG_RAISE1: statenum_t = 496;
pub const S_SARG_DIE6: statenum_t = 495;
pub const S_SARG_DIE5: statenum_t = 494;
pub const S_SARG_DIE4: statenum_t = 493;
pub const S_SARG_DIE3: statenum_t = 492;
pub const S_SARG_DIE2: statenum_t = 491;
pub const S_SARG_DIE1: statenum_t = 490;
pub const S_SARG_PAIN2: statenum_t = 489;
pub const S_SARG_PAIN: statenum_t = 488;
pub const S_SARG_ATK3: statenum_t = 487;
pub const S_SARG_ATK2: statenum_t = 486;
pub const S_SARG_ATK1: statenum_t = 485;
pub const S_SARG_RUN8: statenum_t = 484;
pub const S_SARG_RUN7: statenum_t = 483;
pub const S_SARG_RUN6: statenum_t = 482;
pub const S_SARG_RUN5: statenum_t = 481;
pub const S_SARG_RUN4: statenum_t = 480;
pub const S_SARG_RUN3: statenum_t = 479;
pub const S_SARG_RUN2: statenum_t = 478;
pub const S_SARG_RUN1: statenum_t = 477;
pub const S_SARG_STND2: statenum_t = 476;
pub const S_SARG_STND: statenum_t = 475;
pub const S_TROO_RAISE5: statenum_t = 474;
pub const S_TROO_RAISE4: statenum_t = 473;
pub const S_TROO_RAISE3: statenum_t = 472;
pub const S_TROO_RAISE2: statenum_t = 471;
pub const S_TROO_RAISE1: statenum_t = 470;
pub const S_TROO_XDIE8: statenum_t = 469;
pub const S_TROO_XDIE7: statenum_t = 468;
pub const S_TROO_XDIE6: statenum_t = 467;
pub const S_TROO_XDIE5: statenum_t = 466;
pub const S_TROO_XDIE4: statenum_t = 465;
pub const S_TROO_XDIE3: statenum_t = 464;
pub const S_TROO_XDIE2: statenum_t = 463;
pub const S_TROO_XDIE1: statenum_t = 462;
pub const S_TROO_DIE5: statenum_t = 461;
pub const S_TROO_DIE4: statenum_t = 460;
pub const S_TROO_DIE3: statenum_t = 459;
pub const S_TROO_DIE2: statenum_t = 458;
pub const S_TROO_DIE1: statenum_t = 457;
pub const S_TROO_PAIN2: statenum_t = 456;
pub const S_TROO_PAIN: statenum_t = 455;
pub const S_TROO_ATK3: statenum_t = 454;
pub const S_TROO_ATK2: statenum_t = 453;
pub const S_TROO_ATK1: statenum_t = 452;
pub const S_TROO_RUN8: statenum_t = 451;
pub const S_TROO_RUN7: statenum_t = 450;
pub const S_TROO_RUN6: statenum_t = 449;
pub const S_TROO_RUN5: statenum_t = 448;
pub const S_TROO_RUN4: statenum_t = 447;
pub const S_TROO_RUN3: statenum_t = 446;
pub const S_TROO_RUN2: statenum_t = 445;
pub const S_TROO_RUN1: statenum_t = 444;
pub const S_TROO_STND2: statenum_t = 443;
pub const S_TROO_STND: statenum_t = 442;
pub const S_CPOS_RAISE7: statenum_t = 441;
pub const S_CPOS_RAISE6: statenum_t = 440;
pub const S_CPOS_RAISE5: statenum_t = 439;
pub const S_CPOS_RAISE4: statenum_t = 438;
pub const S_CPOS_RAISE3: statenum_t = 437;
pub const S_CPOS_RAISE2: statenum_t = 436;
pub const S_CPOS_RAISE1: statenum_t = 435;
pub const S_CPOS_XDIE6: statenum_t = 434;
pub const S_CPOS_XDIE5: statenum_t = 433;
pub const S_CPOS_XDIE4: statenum_t = 432;
pub const S_CPOS_XDIE3: statenum_t = 431;
pub const S_CPOS_XDIE2: statenum_t = 430;
pub const S_CPOS_XDIE1: statenum_t = 429;
pub const S_CPOS_DIE7: statenum_t = 428;
pub const S_CPOS_DIE6: statenum_t = 427;
pub const S_CPOS_DIE5: statenum_t = 426;
pub const S_CPOS_DIE4: statenum_t = 425;
pub const S_CPOS_DIE3: statenum_t = 424;
pub const S_CPOS_DIE2: statenum_t = 423;
pub const S_CPOS_DIE1: statenum_t = 422;
pub const S_CPOS_PAIN2: statenum_t = 421;
pub const S_CPOS_PAIN: statenum_t = 420;
pub const S_CPOS_ATK4: statenum_t = 419;
pub const S_CPOS_ATK3: statenum_t = 418;
pub const S_CPOS_ATK2: statenum_t = 417;
pub const S_CPOS_ATK1: statenum_t = 416;
pub const S_CPOS_RUN8: statenum_t = 415;
pub const S_CPOS_RUN7: statenum_t = 414;
pub const S_CPOS_RUN6: statenum_t = 413;
pub const S_CPOS_RUN5: statenum_t = 412;
pub const S_CPOS_RUN4: statenum_t = 411;
pub const S_CPOS_RUN3: statenum_t = 410;
pub const S_CPOS_RUN2: statenum_t = 409;
pub const S_CPOS_RUN1: statenum_t = 408;
pub const S_CPOS_STND2: statenum_t = 407;
pub const S_CPOS_STND: statenum_t = 406;
pub const S_FATT_RAISE8: statenum_t = 405;
pub const S_FATT_RAISE7: statenum_t = 404;
pub const S_FATT_RAISE6: statenum_t = 403;
pub const S_FATT_RAISE5: statenum_t = 402;
pub const S_FATT_RAISE4: statenum_t = 401;
pub const S_FATT_RAISE3: statenum_t = 400;
pub const S_FATT_RAISE2: statenum_t = 399;
pub const S_FATT_RAISE1: statenum_t = 398;
pub const S_FATT_DIE10: statenum_t = 397;
pub const S_FATT_DIE9: statenum_t = 396;
pub const S_FATT_DIE8: statenum_t = 395;
pub const S_FATT_DIE7: statenum_t = 394;
pub const S_FATT_DIE6: statenum_t = 393;
pub const S_FATT_DIE5: statenum_t = 392;
pub const S_FATT_DIE4: statenum_t = 391;
pub const S_FATT_DIE3: statenum_t = 390;
pub const S_FATT_DIE2: statenum_t = 389;
pub const S_FATT_DIE1: statenum_t = 388;
pub const S_FATT_PAIN2: statenum_t = 387;
pub const S_FATT_PAIN: statenum_t = 386;
pub const S_FATT_ATK10: statenum_t = 385;
pub const S_FATT_ATK9: statenum_t = 384;
pub const S_FATT_ATK8: statenum_t = 383;
pub const S_FATT_ATK7: statenum_t = 382;
pub const S_FATT_ATK6: statenum_t = 381;
pub const S_FATT_ATK5: statenum_t = 380;
pub const S_FATT_ATK4: statenum_t = 379;
pub const S_FATT_ATK3: statenum_t = 378;
pub const S_FATT_ATK2: statenum_t = 377;
pub const S_FATT_ATK1: statenum_t = 376;
pub const S_FATT_RUN12: statenum_t = 375;
pub const S_FATT_RUN11: statenum_t = 374;
pub const S_FATT_RUN10: statenum_t = 373;
pub const S_FATT_RUN9: statenum_t = 372;
pub const S_FATT_RUN8: statenum_t = 371;
pub const S_FATT_RUN7: statenum_t = 370;
pub const S_FATT_RUN6: statenum_t = 369;
pub const S_FATT_RUN5: statenum_t = 368;
pub const S_FATT_RUN4: statenum_t = 367;
pub const S_FATT_RUN3: statenum_t = 366;
pub const S_FATT_RUN2: statenum_t = 365;
pub const S_FATT_RUN1: statenum_t = 364;
pub const S_FATT_STND2: statenum_t = 363;
pub const S_FATT_STND: statenum_t = 362;
pub const S_FATSHOTX3: statenum_t = 361;
pub const S_FATSHOTX2: statenum_t = 360;
pub const S_FATSHOTX1: statenum_t = 359;
pub const S_FATSHOT2: statenum_t = 358;
pub const S_FATSHOT1: statenum_t = 357;
pub const S_SKEL_RAISE6: statenum_t = 356;
pub const S_SKEL_RAISE5: statenum_t = 355;
pub const S_SKEL_RAISE4: statenum_t = 354;
pub const S_SKEL_RAISE3: statenum_t = 353;
pub const S_SKEL_RAISE2: statenum_t = 352;
pub const S_SKEL_RAISE1: statenum_t = 351;
pub const S_SKEL_DIE6: statenum_t = 350;
pub const S_SKEL_DIE5: statenum_t = 349;
pub const S_SKEL_DIE4: statenum_t = 348;
pub const S_SKEL_DIE3: statenum_t = 347;
pub const S_SKEL_DIE2: statenum_t = 346;
pub const S_SKEL_DIE1: statenum_t = 345;
pub const S_SKEL_PAIN2: statenum_t = 344;
pub const S_SKEL_PAIN: statenum_t = 343;
pub const S_SKEL_MISS4: statenum_t = 342;
pub const S_SKEL_MISS3: statenum_t = 341;
pub const S_SKEL_MISS2: statenum_t = 340;
pub const S_SKEL_MISS1: statenum_t = 339;
pub const S_SKEL_FIST4: statenum_t = 338;
pub const S_SKEL_FIST3: statenum_t = 337;
pub const S_SKEL_FIST2: statenum_t = 336;
pub const S_SKEL_FIST1: statenum_t = 335;
pub const S_SKEL_RUN12: statenum_t = 334;
pub const S_SKEL_RUN11: statenum_t = 333;
pub const S_SKEL_RUN10: statenum_t = 332;
pub const S_SKEL_RUN9: statenum_t = 331;
pub const S_SKEL_RUN8: statenum_t = 330;
pub const S_SKEL_RUN7: statenum_t = 329;
pub const S_SKEL_RUN6: statenum_t = 328;
pub const S_SKEL_RUN5: statenum_t = 327;
pub const S_SKEL_RUN4: statenum_t = 326;
pub const S_SKEL_RUN3: statenum_t = 325;
pub const S_SKEL_RUN2: statenum_t = 324;
pub const S_SKEL_RUN1: statenum_t = 323;
pub const S_SKEL_STND2: statenum_t = 322;
pub const S_SKEL_STND: statenum_t = 321;
pub const S_TRACEEXP3: statenum_t = 320;
pub const S_TRACEEXP2: statenum_t = 319;
pub const S_TRACEEXP1: statenum_t = 318;
pub const S_TRACER2: statenum_t = 317;
pub const S_TRACER: statenum_t = 316;
pub const S_SMOKE5: statenum_t = 315;
pub const S_SMOKE4: statenum_t = 314;
pub const S_SMOKE3: statenum_t = 313;
pub const S_SMOKE2: statenum_t = 312;
pub const S_SMOKE1: statenum_t = 311;
pub const S_FIRE30: statenum_t = 310;
pub const S_FIRE29: statenum_t = 309;
pub const S_FIRE28: statenum_t = 308;
pub const S_FIRE27: statenum_t = 307;
pub const S_FIRE26: statenum_t = 306;
pub const S_FIRE25: statenum_t = 305;
pub const S_FIRE24: statenum_t = 304;
pub const S_FIRE23: statenum_t = 303;
pub const S_FIRE22: statenum_t = 302;
pub const S_FIRE21: statenum_t = 301;
pub const S_FIRE20: statenum_t = 300;
pub const S_FIRE19: statenum_t = 299;
pub const S_FIRE18: statenum_t = 298;
pub const S_FIRE17: statenum_t = 297;
pub const S_FIRE16: statenum_t = 296;
pub const S_FIRE15: statenum_t = 295;
pub const S_FIRE14: statenum_t = 294;
pub const S_FIRE13: statenum_t = 293;
pub const S_FIRE12: statenum_t = 292;
pub const S_FIRE11: statenum_t = 291;
pub const S_FIRE10: statenum_t = 290;
pub const S_FIRE9: statenum_t = 289;
pub const S_FIRE8: statenum_t = 288;
pub const S_FIRE7: statenum_t = 287;
pub const S_FIRE6: statenum_t = 286;
pub const S_FIRE5: statenum_t = 285;
pub const S_FIRE4: statenum_t = 284;
pub const S_FIRE3: statenum_t = 283;
pub const S_FIRE2: statenum_t = 282;
pub const S_FIRE1: statenum_t = 281;
pub const S_VILE_DIE10: statenum_t = 280;
pub const S_VILE_DIE9: statenum_t = 279;
pub const S_VILE_DIE8: statenum_t = 278;
pub const S_VILE_DIE7: statenum_t = 277;
pub const S_VILE_DIE6: statenum_t = 276;
pub const S_VILE_DIE5: statenum_t = 275;
pub const S_VILE_DIE4: statenum_t = 274;
pub const S_VILE_DIE3: statenum_t = 273;
pub const S_VILE_DIE2: statenum_t = 272;
pub const S_VILE_DIE1: statenum_t = 271;
pub const S_VILE_PAIN2: statenum_t = 270;
pub const S_VILE_PAIN: statenum_t = 269;
pub const S_VILE_HEAL3: statenum_t = 268;
pub const S_VILE_HEAL2: statenum_t = 267;
pub const S_VILE_HEAL1: statenum_t = 266;
pub const S_VILE_ATK11: statenum_t = 265;
pub const S_VILE_ATK10: statenum_t = 264;
pub const S_VILE_ATK9: statenum_t = 263;
pub const S_VILE_ATK8: statenum_t = 262;
pub const S_VILE_ATK7: statenum_t = 261;
pub const S_VILE_ATK6: statenum_t = 260;
pub const S_VILE_ATK5: statenum_t = 259;
pub const S_VILE_ATK4: statenum_t = 258;
pub const S_VILE_ATK3: statenum_t = 257;
pub const S_VILE_ATK2: statenum_t = 256;
pub const S_VILE_ATK1: statenum_t = 255;
pub const S_VILE_RUN12: statenum_t = 254;
pub const S_VILE_RUN11: statenum_t = 253;
pub const S_VILE_RUN10: statenum_t = 252;
pub const S_VILE_RUN9: statenum_t = 251;
pub const S_VILE_RUN8: statenum_t = 250;
pub const S_VILE_RUN7: statenum_t = 249;
pub const S_VILE_RUN6: statenum_t = 248;
pub const S_VILE_RUN5: statenum_t = 247;
pub const S_VILE_RUN4: statenum_t = 246;
pub const S_VILE_RUN3: statenum_t = 245;
pub const S_VILE_RUN2: statenum_t = 244;
pub const S_VILE_RUN1: statenum_t = 243;
pub const S_VILE_STND2: statenum_t = 242;
pub const S_VILE_STND: statenum_t = 241;
pub const S_SPOS_RAISE5: statenum_t = 240;
pub const S_SPOS_RAISE4: statenum_t = 239;
pub const S_SPOS_RAISE3: statenum_t = 238;
pub const S_SPOS_RAISE2: statenum_t = 237;
pub const S_SPOS_RAISE1: statenum_t = 236;
pub const S_SPOS_XDIE9: statenum_t = 235;
pub const S_SPOS_XDIE8: statenum_t = 234;
pub const S_SPOS_XDIE7: statenum_t = 233;
pub const S_SPOS_XDIE6: statenum_t = 232;
pub const S_SPOS_XDIE5: statenum_t = 231;
pub const S_SPOS_XDIE4: statenum_t = 230;
pub const S_SPOS_XDIE3: statenum_t = 229;
pub const S_SPOS_XDIE2: statenum_t = 228;
pub const S_SPOS_XDIE1: statenum_t = 227;
pub const S_SPOS_DIE5: statenum_t = 226;
pub const S_SPOS_DIE4: statenum_t = 225;
pub const S_SPOS_DIE3: statenum_t = 224;
pub const S_SPOS_DIE2: statenum_t = 223;
pub const S_SPOS_DIE1: statenum_t = 222;
pub const S_SPOS_PAIN2: statenum_t = 221;
pub const S_SPOS_PAIN: statenum_t = 220;
pub const S_SPOS_ATK3: statenum_t = 219;
pub const S_SPOS_ATK2: statenum_t = 218;
pub const S_SPOS_ATK1: statenum_t = 217;
pub const S_SPOS_RUN8: statenum_t = 216;
pub const S_SPOS_RUN7: statenum_t = 215;
pub const S_SPOS_RUN6: statenum_t = 214;
pub const S_SPOS_RUN5: statenum_t = 213;
pub const S_SPOS_RUN4: statenum_t = 212;
pub const S_SPOS_RUN3: statenum_t = 211;
pub const S_SPOS_RUN2: statenum_t = 210;
pub const S_SPOS_RUN1: statenum_t = 209;
pub const S_SPOS_STND2: statenum_t = 208;
pub const S_SPOS_STND: statenum_t = 207;
pub const S_POSS_RAISE4: statenum_t = 206;
pub const S_POSS_RAISE3: statenum_t = 205;
pub const S_POSS_RAISE2: statenum_t = 204;
pub const S_POSS_RAISE1: statenum_t = 203;
pub const S_POSS_XDIE9: statenum_t = 202;
pub const S_POSS_XDIE8: statenum_t = 201;
pub const S_POSS_XDIE7: statenum_t = 200;
pub const S_POSS_XDIE6: statenum_t = 199;
pub const S_POSS_XDIE5: statenum_t = 198;
pub const S_POSS_XDIE4: statenum_t = 197;
pub const S_POSS_XDIE3: statenum_t = 196;
pub const S_POSS_XDIE2: statenum_t = 195;
pub const S_POSS_XDIE1: statenum_t = 194;
pub const S_POSS_DIE5: statenum_t = 193;
pub const S_POSS_DIE4: statenum_t = 192;
pub const S_POSS_DIE3: statenum_t = 191;
pub const S_POSS_DIE2: statenum_t = 190;
pub const S_POSS_DIE1: statenum_t = 189;
pub const S_POSS_PAIN2: statenum_t = 188;
pub const S_POSS_PAIN: statenum_t = 187;
pub const S_POSS_ATK3: statenum_t = 186;
pub const S_POSS_ATK2: statenum_t = 185;
pub const S_POSS_ATK1: statenum_t = 184;
pub const S_POSS_RUN8: statenum_t = 183;
pub const S_POSS_RUN7: statenum_t = 182;
pub const S_POSS_RUN6: statenum_t = 181;
pub const S_POSS_RUN5: statenum_t = 180;
pub const S_POSS_RUN4: statenum_t = 179;
pub const S_POSS_RUN3: statenum_t = 178;
pub const S_POSS_RUN2: statenum_t = 177;
pub const S_POSS_RUN1: statenum_t = 176;
pub const S_POSS_STND2: statenum_t = 175;
pub const S_POSS_STND: statenum_t = 174;
pub const S_PLAY_XDIE9: statenum_t = 173;
pub const S_PLAY_XDIE8: statenum_t = 172;
pub const S_PLAY_XDIE7: statenum_t = 171;
pub const S_PLAY_XDIE6: statenum_t = 170;
pub const S_PLAY_XDIE5: statenum_t = 169;
pub const S_PLAY_XDIE4: statenum_t = 168;
pub const S_PLAY_XDIE3: statenum_t = 167;
pub const S_PLAY_XDIE2: statenum_t = 166;
pub const S_PLAY_XDIE1: statenum_t = 165;
pub const S_PLAY_DIE7: statenum_t = 164;
pub const S_PLAY_DIE6: statenum_t = 163;
pub const S_PLAY_DIE5: statenum_t = 162;
pub const S_PLAY_DIE4: statenum_t = 161;
pub const S_PLAY_DIE3: statenum_t = 160;
pub const S_PLAY_DIE2: statenum_t = 159;
pub const S_PLAY_DIE1: statenum_t = 158;
pub const S_PLAY_PAIN2: statenum_t = 157;
pub const S_PLAY_PAIN: statenum_t = 156;
pub const S_PLAY_ATK2: statenum_t = 155;
pub const S_PLAY_ATK1: statenum_t = 154;
pub const S_PLAY_RUN4: statenum_t = 153;
pub const S_PLAY_RUN3: statenum_t = 152;
pub const S_PLAY_RUN2: statenum_t = 151;
pub const S_PLAY_RUN1: statenum_t = 150;
pub const S_PLAY: statenum_t = 149;
pub const S_IFOG5: statenum_t = 148;
pub const S_IFOG4: statenum_t = 147;
pub const S_IFOG3: statenum_t = 146;
pub const S_IFOG2: statenum_t = 145;
pub const S_IFOG02: statenum_t = 144;
pub const S_IFOG01: statenum_t = 143;
pub const S_IFOG: statenum_t = 142;
pub const S_TFOG10: statenum_t = 141;
pub const S_TFOG9: statenum_t = 140;
pub const S_TFOG8: statenum_t = 139;
pub const S_TFOG7: statenum_t = 138;
pub const S_TFOG6: statenum_t = 137;
pub const S_TFOG5: statenum_t = 136;
pub const S_TFOG4: statenum_t = 135;
pub const S_TFOG3: statenum_t = 134;
pub const S_TFOG2: statenum_t = 133;
pub const S_TFOG02: statenum_t = 132;
pub const S_TFOG01: statenum_t = 131;
pub const S_TFOG: statenum_t = 130;
pub const S_EXPLODE3: statenum_t = 129;
pub const S_EXPLODE2: statenum_t = 128;
pub const S_EXPLODE1: statenum_t = 127;
pub const S_BFGEXP4: statenum_t = 126;
pub const S_BFGEXP3: statenum_t = 125;
pub const S_BFGEXP2: statenum_t = 124;
pub const S_BFGEXP: statenum_t = 123;
pub const S_BFGLAND6: statenum_t = 122;
pub const S_BFGLAND5: statenum_t = 121;
pub const S_BFGLAND4: statenum_t = 120;
pub const S_BFGLAND3: statenum_t = 119;
pub const S_BFGLAND2: statenum_t = 118;
pub const S_BFGLAND: statenum_t = 117;
pub const S_BFGSHOT2: statenum_t = 116;
pub const S_BFGSHOT: statenum_t = 115;
pub const S_ROCKET: statenum_t = 114;
pub const S_PLASEXP5: statenum_t = 113;
pub const S_PLASEXP4: statenum_t = 112;
pub const S_PLASEXP3: statenum_t = 111;
pub const S_PLASEXP2: statenum_t = 110;
pub const S_PLASEXP: statenum_t = 109;
pub const S_PLASBALL2: statenum_t = 108;
pub const S_PLASBALL: statenum_t = 107;
pub const S_RBALLX3: statenum_t = 106;
pub const S_RBALLX2: statenum_t = 105;
pub const S_RBALLX1: statenum_t = 104;
pub const S_RBALL2: statenum_t = 103;
pub const S_RBALL1: statenum_t = 102;
pub const S_TBALLX3: statenum_t = 101;
pub const S_TBALLX2: statenum_t = 100;
pub const S_TBALLX1: statenum_t = 99;
pub const S_TBALL2: statenum_t = 98;
pub const S_TBALL1: statenum_t = 97;
pub const S_PUFF4: statenum_t = 96;
pub const S_PUFF3: statenum_t = 95;
pub const S_PUFF2: statenum_t = 94;
pub const S_PUFF1: statenum_t = 93;
pub const S_BLOOD3: statenum_t = 92;
pub const S_BLOOD2: statenum_t = 91;
pub const S_BLOOD1: statenum_t = 90;
pub const S_BFGFLASH2: statenum_t = 89;
pub const S_BFGFLASH1: statenum_t = 88;
pub const S_BFG4: statenum_t = 87;
pub const S_BFG3: statenum_t = 86;
pub const S_BFG2: statenum_t = 85;
pub const S_BFG1: statenum_t = 84;
pub const S_BFGUP: statenum_t = 83;
pub const S_BFGDOWN: statenum_t = 82;
pub const S_BFG: statenum_t = 81;
pub const S_PLASMAFLASH2: statenum_t = 80;
pub const S_PLASMAFLASH1: statenum_t = 79;
pub const S_PLASMA2: statenum_t = 78;
pub const S_PLASMA1: statenum_t = 77;
pub const S_PLASMAUP: statenum_t = 76;
pub const S_PLASMADOWN: statenum_t = 75;
pub const S_PLASMA: statenum_t = 74;
pub const S_SAW3: statenum_t = 73;
pub const S_SAW2: statenum_t = 72;
pub const S_SAW1: statenum_t = 71;
pub const S_SAWUP: statenum_t = 70;
pub const S_SAWDOWN: statenum_t = 69;
pub const S_SAWB: statenum_t = 68;
pub const S_SAW: statenum_t = 67;
pub const S_MISSILEFLASH4: statenum_t = 66;
pub const S_MISSILEFLASH3: statenum_t = 65;
pub const S_MISSILEFLASH2: statenum_t = 64;
pub const S_MISSILEFLASH1: statenum_t = 63;
pub const S_MISSILE3: statenum_t = 62;
pub const S_MISSILE2: statenum_t = 61;
pub const S_MISSILE1: statenum_t = 60;
pub const S_MISSILEUP: statenum_t = 59;
pub const S_MISSILEDOWN: statenum_t = 58;
pub const S_MISSILE: statenum_t = 57;
pub const S_CHAINFLASH2: statenum_t = 56;
pub const S_CHAINFLASH1: statenum_t = 55;
pub const S_CHAIN3: statenum_t = 54;
pub const S_CHAIN2: statenum_t = 53;
pub const S_CHAIN1: statenum_t = 52;
pub const S_CHAINUP: statenum_t = 51;
pub const S_CHAINDOWN: statenum_t = 50;
pub const S_CHAIN: statenum_t = 49;
pub const S_DSGUNFLASH2: statenum_t = 48;
pub const S_DSGUNFLASH1: statenum_t = 47;
pub const S_DSNR2: statenum_t = 46;
pub const S_DSNR1: statenum_t = 45;
pub const S_DSGUN10: statenum_t = 44;
pub const S_DSGUN9: statenum_t = 43;
pub const S_DSGUN8: statenum_t = 42;
pub const S_DSGUN7: statenum_t = 41;
pub const S_DSGUN6: statenum_t = 40;
pub const S_DSGUN5: statenum_t = 39;
pub const S_DSGUN4: statenum_t = 38;
pub const S_DSGUN3: statenum_t = 37;
pub const S_DSGUN2: statenum_t = 36;
pub const S_DSGUN1: statenum_t = 35;
pub const S_DSGUNUP: statenum_t = 34;
pub const S_DSGUNDOWN: statenum_t = 33;
pub const S_DSGUN: statenum_t = 32;
pub const S_SGUNFLASH2: statenum_t = 31;
pub const S_SGUNFLASH1: statenum_t = 30;
pub const S_SGUN9: statenum_t = 29;
pub const S_SGUN8: statenum_t = 28;
pub const S_SGUN7: statenum_t = 27;
pub const S_SGUN6: statenum_t = 26;
pub const S_SGUN5: statenum_t = 25;
pub const S_SGUN4: statenum_t = 24;
pub const S_SGUN3: statenum_t = 23;
pub const S_SGUN2: statenum_t = 22;
pub const S_SGUN1: statenum_t = 21;
pub const S_SGUNUP: statenum_t = 20;
pub const S_SGUNDOWN: statenum_t = 19;
pub const S_SGUN: statenum_t = 18;
pub const S_PISTOLFLASH: statenum_t = 17;
pub const S_PISTOL4: statenum_t = 16;
pub const S_PISTOL3: statenum_t = 15;
pub const S_PISTOL2: statenum_t = 14;
pub const S_PISTOL1: statenum_t = 13;
pub const S_PISTOLUP: statenum_t = 12;
pub const S_PISTOLDOWN: statenum_t = 11;
pub const S_PISTOL: statenum_t = 10;
pub const S_PUNCH5: statenum_t = 9;
pub const S_PUNCH4: statenum_t = 8;
pub const S_PUNCH3: statenum_t = 7;
pub const S_PUNCH2: statenum_t = 6;
pub const S_PUNCH1: statenum_t = 5;
pub const S_PUNCHUP: statenum_t = 4;
pub const S_PUNCHDOWN: statenum_t = 3;
pub const S_PUNCH: statenum_t = 2;
pub const S_LIGHTDONE: statenum_t = 1;
pub const S_NULL: statenum_t = 0;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct state_t {
    pub sprite: spritenum_t,
    pub frame: ::core::ffi::c_int,
    pub tics: ::core::ffi::c_int,
    pub action: actionf_t,
    pub nextstate: statenum_t,
    pub misc1: ::core::ffi::c_int,
    pub misc2: ::core::ffi::c_int,
}
pub type mobjtype_t = ::core::ffi::c_uint;
pub const NUMMOBJTYPES: mobjtype_t = 137;
pub const MT_MISC86: mobjtype_t = 136;
pub const MT_MISC85: mobjtype_t = 135;
pub const MT_MISC84: mobjtype_t = 134;
pub const MT_MISC83: mobjtype_t = 133;
pub const MT_MISC82: mobjtype_t = 132;
pub const MT_MISC81: mobjtype_t = 131;
pub const MT_MISC80: mobjtype_t = 130;
pub const MT_MISC79: mobjtype_t = 129;
pub const MT_MISC78: mobjtype_t = 128;
pub const MT_MISC77: mobjtype_t = 127;
pub const MT_MISC76: mobjtype_t = 126;
pub const MT_MISC75: mobjtype_t = 125;
pub const MT_MISC74: mobjtype_t = 124;
pub const MT_MISC73: mobjtype_t = 123;
pub const MT_MISC72: mobjtype_t = 122;
pub const MT_MISC71: mobjtype_t = 121;
pub const MT_MISC70: mobjtype_t = 120;
pub const MT_MISC69: mobjtype_t = 119;
pub const MT_MISC68: mobjtype_t = 118;
pub const MT_MISC67: mobjtype_t = 117;
pub const MT_MISC66: mobjtype_t = 116;
pub const MT_MISC65: mobjtype_t = 115;
pub const MT_MISC64: mobjtype_t = 114;
pub const MT_MISC63: mobjtype_t = 113;
pub const MT_MISC62: mobjtype_t = 112;
pub const MT_MISC61: mobjtype_t = 111;
pub const MT_MISC60: mobjtype_t = 110;
pub const MT_MISC59: mobjtype_t = 109;
pub const MT_MISC58: mobjtype_t = 108;
pub const MT_MISC57: mobjtype_t = 107;
pub const MT_MISC56: mobjtype_t = 106;
pub const MT_MISC55: mobjtype_t = 105;
pub const MT_MISC54: mobjtype_t = 104;
pub const MT_MISC53: mobjtype_t = 103;
pub const MT_MISC52: mobjtype_t = 102;
pub const MT_MISC51: mobjtype_t = 101;
pub const MT_MISC50: mobjtype_t = 100;
pub const MT_MISC49: mobjtype_t = 99;
pub const MT_MISC48: mobjtype_t = 98;
pub const MT_MISC47: mobjtype_t = 97;
pub const MT_MISC46: mobjtype_t = 96;
pub const MT_MISC45: mobjtype_t = 95;
pub const MT_MISC44: mobjtype_t = 94;
pub const MT_MISC43: mobjtype_t = 93;
pub const MT_MISC42: mobjtype_t = 92;
pub const MT_MISC41: mobjtype_t = 91;
pub const MT_MISC40: mobjtype_t = 90;
pub const MT_MISC39: mobjtype_t = 89;
pub const MT_MISC38: mobjtype_t = 88;
pub const MT_MISC37: mobjtype_t = 87;
pub const MT_MISC36: mobjtype_t = 86;
pub const MT_MISC35: mobjtype_t = 85;
pub const MT_MISC34: mobjtype_t = 84;
pub const MT_MISC33: mobjtype_t = 83;
pub const MT_MISC32: mobjtype_t = 82;
pub const MT_MISC31: mobjtype_t = 81;
pub const MT_MISC30: mobjtype_t = 80;
pub const MT_MISC29: mobjtype_t = 79;
pub const MT_SUPERSHOTGUN: mobjtype_t = 78;
pub const MT_SHOTGUN: mobjtype_t = 77;
pub const MT_MISC28: mobjtype_t = 76;
pub const MT_MISC27: mobjtype_t = 75;
pub const MT_MISC26: mobjtype_t = 74;
pub const MT_CHAINGUN: mobjtype_t = 73;
pub const MT_MISC25: mobjtype_t = 72;
pub const MT_MISC24: mobjtype_t = 71;
pub const MT_MISC23: mobjtype_t = 70;
pub const MT_MISC22: mobjtype_t = 69;
pub const MT_MISC21: mobjtype_t = 68;
pub const MT_MISC20: mobjtype_t = 67;
pub const MT_MISC19: mobjtype_t = 66;
pub const MT_MISC18: mobjtype_t = 65;
pub const MT_MISC17: mobjtype_t = 64;
pub const MT_CLIP: mobjtype_t = 63;
pub const MT_MEGA: mobjtype_t = 62;
pub const MT_MISC16: mobjtype_t = 61;
pub const MT_MISC15: mobjtype_t = 60;
pub const MT_MISC14: mobjtype_t = 59;
pub const MT_INS: mobjtype_t = 58;
pub const MT_MISC13: mobjtype_t = 57;
pub const MT_INV: mobjtype_t = 56;
pub const MT_MISC12: mobjtype_t = 55;
pub const MT_MISC11: mobjtype_t = 54;
pub const MT_MISC10: mobjtype_t = 53;
pub const MT_MISC9: mobjtype_t = 52;
pub const MT_MISC8: mobjtype_t = 51;
pub const MT_MISC7: mobjtype_t = 50;
pub const MT_MISC6: mobjtype_t = 49;
pub const MT_MISC5: mobjtype_t = 48;
pub const MT_MISC4: mobjtype_t = 47;
pub const MT_MISC3: mobjtype_t = 46;
pub const MT_MISC2: mobjtype_t = 45;
pub const MT_MISC1: mobjtype_t = 44;
pub const MT_MISC0: mobjtype_t = 43;
pub const MT_EXTRABFG: mobjtype_t = 42;
pub const MT_TELEPORTMAN: mobjtype_t = 41;
pub const MT_IFOG: mobjtype_t = 40;
pub const MT_TFOG: mobjtype_t = 39;
pub const MT_BLOOD: mobjtype_t = 38;
pub const MT_PUFF: mobjtype_t = 37;
pub const MT_ARACHPLAZ: mobjtype_t = 36;
pub const MT_BFG: mobjtype_t = 35;
pub const MT_PLASMA: mobjtype_t = 34;
pub const MT_ROCKET: mobjtype_t = 33;
pub const MT_HEADSHOT: mobjtype_t = 32;
pub const MT_TROOPSHOT: mobjtype_t = 31;
pub const MT_BARREL: mobjtype_t = 30;
pub const MT_SPAWNFIRE: mobjtype_t = 29;
pub const MT_SPAWNSHOT: mobjtype_t = 28;
pub const MT_BOSSTARGET: mobjtype_t = 27;
pub const MT_BOSSSPIT: mobjtype_t = 26;
pub const MT_BOSSBRAIN: mobjtype_t = 25;
pub const MT_KEEN: mobjtype_t = 24;
pub const MT_WOLFSS: mobjtype_t = 23;
pub const MT_PAIN: mobjtype_t = 22;
pub const MT_CYBORG: mobjtype_t = 21;
pub const MT_BABY: mobjtype_t = 20;
pub const MT_SPIDER: mobjtype_t = 19;
pub const MT_SKULL: mobjtype_t = 18;
pub const MT_KNIGHT: mobjtype_t = 17;
pub const MT_BRUISERSHOT: mobjtype_t = 16;
pub const MT_BRUISER: mobjtype_t = 15;
pub const MT_HEAD: mobjtype_t = 14;
pub const MT_SHADOWS: mobjtype_t = 13;
pub const MT_SERGEANT: mobjtype_t = 12;
pub const MT_TROOP: mobjtype_t = 11;
pub const MT_CHAINGUY: mobjtype_t = 10;
pub const MT_FATSHOT: mobjtype_t = 9;
pub const MT_FATSO: mobjtype_t = 8;
pub const MT_SMOKE: mobjtype_t = 7;
pub const MT_TRACER: mobjtype_t = 6;
pub const MT_UNDEAD: mobjtype_t = 5;
pub const MT_FIRE: mobjtype_t = 4;
pub const MT_VILE: mobjtype_t = 3;
pub const MT_SHOTGUY: mobjtype_t = 2;
pub const MT_POSSESSED: mobjtype_t = 1;
pub const MT_PLAYER: mobjtype_t = 0;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct mobjinfo_t {
    pub doomednum: ::core::ffi::c_int,
    pub spawnstate: ::core::ffi::c_int,
    pub spawnhealth: ::core::ffi::c_int,
    pub seestate: ::core::ffi::c_int,
    pub seesound: ::core::ffi::c_int,
    pub reactiontime: ::core::ffi::c_int,
    pub attacksound: ::core::ffi::c_int,
    pub painstate: ::core::ffi::c_int,
    pub painchance: ::core::ffi::c_int,
    pub painsound: ::core::ffi::c_int,
    pub meleestate: ::core::ffi::c_int,
    pub missilestate: ::core::ffi::c_int,
    pub deathstate: ::core::ffi::c_int,
    pub xdeathstate: ::core::ffi::c_int,
    pub deathsound: ::core::ffi::c_int,
    pub speed: ::core::ffi::c_int,
    pub radius: ::core::ffi::c_int,
    pub height: ::core::ffi::c_int,
    pub mass: ::core::ffi::c_int,
    pub damage: ::core::ffi::c_int,
    pub activesound: ::core::ffi::c_int,
    pub flags: ::core::ffi::c_int,
    pub raisestate: ::core::ffi::c_int,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct mobj_s {
    pub thinker: thinker_t,
    pub x: fixed_t,
    pub y: fixed_t,
    pub z: fixed_t,
    pub snext: *mut mobj_s,
    pub sprev: *mut mobj_s,
    pub angle: angle_t,
    pub sprite: spritenum_t,
    pub frame: ::core::ffi::c_int,
    pub bnext: *mut mobj_s,
    pub bprev: *mut mobj_s,
    pub subsector: *mut subsector_s,
    pub floorz: fixed_t,
    pub ceilingz: fixed_t,
    pub radius: fixed_t,
    pub height: fixed_t,
    pub momx: fixed_t,
    pub momy: fixed_t,
    pub momz: fixed_t,
    pub validcount: ::core::ffi::c_int,
    pub type_0: mobjtype_t,
    pub info: *mut mobjinfo_t,
    pub tics: ::core::ffi::c_int,
    pub state: *mut state_t,
    pub flags: ::core::ffi::c_int,
    pub health: ::core::ffi::c_int,
    pub movedir: ::core::ffi::c_int,
    pub movecount: ::core::ffi::c_int,
    pub target: *mut mobj_s,
    pub reactiontime: ::core::ffi::c_int,
    pub threshold: ::core::ffi::c_int,
    pub player: *mut player_s,
    pub lastlook: ::core::ffi::c_int,
    pub spawnpoint: mapthing_t,
    pub tracer: *mut mobj_s,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct player_s {
    pub mo: *mut mobj_t,
    pub playerstate: playerstate_t,
    pub cmd: ticcmd_t,
    pub viewz: fixed_t,
    pub viewheight: fixed_t,
    pub deltaviewheight: fixed_t,
    pub bob: fixed_t,
    pub health: ::core::ffi::c_int,
    pub armorpoints: ::core::ffi::c_int,
    pub armortype: ::core::ffi::c_int,
    pub powers: [::core::ffi::c_int; 6],
    pub cards: [boolean; 6],
    pub backpack: boolean,
    pub frags: [::core::ffi::c_int; 4],
    pub readyweapon: weapontype_t,
    pub pendingweapon: weapontype_t,
    pub weaponowned: [boolean; 9],
    pub ammo: [::core::ffi::c_int; 4],
    pub maxammo: [::core::ffi::c_int; 4],
    pub attackdown: ::core::ffi::c_int,
    pub usedown: ::core::ffi::c_int,
    pub cheats: ::core::ffi::c_int,
    pub refire: ::core::ffi::c_int,
    pub killcount: ::core::ffi::c_int,
    pub itemcount: ::core::ffi::c_int,
    pub secretcount: ::core::ffi::c_int,
    pub message: *mut ::core::ffi::c_char,
    pub damagecount: ::core::ffi::c_int,
    pub bonuscount: ::core::ffi::c_int,
    pub attacker: *mut mobj_t,
    pub extralight: ::core::ffi::c_int,
    pub fixedcolormap: ::core::ffi::c_int,
    pub colormap: ::core::ffi::c_int,
    pub psprites: [pspdef_t; 2],
    pub didsecret: boolean,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct pspdef_t {
    pub state: *mut state_t,
    pub tics: ::core::ffi::c_int,
    pub sx: fixed_t,
    pub sy: fixed_t,
}
pub type mobj_t = mobj_s;
pub type playerstate_t = ::core::ffi::c_uint;
pub const PST_REBORN: playerstate_t = 2;
pub const PST_DEAD: playerstate_t = 1;
pub const PST_LIVE: playerstate_t = 0;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct subsector_s {
    pub sector: *mut sector_t,
    pub numlines: ::core::ffi::c_short,
    pub firstline: ::core::ffi::c_short,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct sector_t {
    pub floorheight: fixed_t,
    pub ceilingheight: fixed_t,
    pub floorpic: ::core::ffi::c_short,
    pub ceilingpic: ::core::ffi::c_short,
    pub lightlevel: ::core::ffi::c_short,
    pub special: ::core::ffi::c_short,
    pub tag: ::core::ffi::c_short,
    pub soundtraversed: ::core::ffi::c_int,
    pub soundtarget: *mut mobj_t,
    pub blockbox: [::core::ffi::c_int; 4],
    pub soundorg: degenmobj_t,
    pub validcount: ::core::ffi::c_int,
    pub thinglist: *mut mobj_t,
    pub specialdata: *mut ::core::ffi::c_void,
    pub linecount: ::core::ffi::c_int,
    pub lines: *mut *mut line_s,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct line_s {
    pub v1: *mut vertex_t,
    pub v2: *mut vertex_t,
    pub dx: fixed_t,
    pub dy: fixed_t,
    pub flags: ::core::ffi::c_short,
    pub special: ::core::ffi::c_short,
    pub tag: ::core::ffi::c_short,
    pub sidenum: [::core::ffi::c_short; 2],
    pub bbox: [fixed_t; 4],
    pub slopetype: slopetype_t,
    pub frontsector: *mut sector_t,
    pub backsector: *mut sector_t,
    pub validcount: ::core::ffi::c_int,
    pub specialdata: *mut ::core::ffi::c_void,
}
pub type slopetype_t = ::core::ffi::c_uint;
pub const ST_NEGATIVE: slopetype_t = 3;
pub const ST_POSITIVE: slopetype_t = 2;
pub const ST_VERTICAL: slopetype_t = 1;
pub const ST_HORIZONTAL: slopetype_t = 0;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct vertex_t {
    pub x: fixed_t,
    pub y: fixed_t,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct degenmobj_t {
    pub thinker: thinker_t,
    pub x: fixed_t,
    pub y: fixed_t,
    pub z: fixed_t,
}
pub type player_t = player_s;
pub type C2Rust_Unnamed_0 = ::core::ffi::c_uint;
pub const NUMSFX: C2Rust_Unnamed_0 = 109;
pub const sfx_radio: C2Rust_Unnamed_0 = 108;
pub const sfx_skeatk: C2Rust_Unnamed_0 = 107;
pub const sfx_skesit: C2Rust_Unnamed_0 = 106;
pub const sfx_skeact: C2Rust_Unnamed_0 = 105;
pub const sfx_keendt: C2Rust_Unnamed_0 = 104;
pub const sfx_keenpn: C2Rust_Unnamed_0 = 103;
pub const sfx_ssdth: C2Rust_Unnamed_0 = 102;
pub const sfx_sssit: C2Rust_Unnamed_0 = 101;
pub const sfx_mandth: C2Rust_Unnamed_0 = 100;
pub const sfx_manatk: C2Rust_Unnamed_0 = 99;
pub const sfx_bosdth: C2Rust_Unnamed_0 = 98;
pub const sfx_bospn: C2Rust_Unnamed_0 = 97;
pub const sfx_bossit: C2Rust_Unnamed_0 = 96;
pub const sfx_boscub: C2Rust_Unnamed_0 = 95;
pub const sfx_bospit: C2Rust_Unnamed_0 = 94;
pub const sfx_getpow: C2Rust_Unnamed_0 = 93;
pub const sfx_flamst: C2Rust_Unnamed_0 = 92;
pub const sfx_flame: C2Rust_Unnamed_0 = 91;
pub const sfx_itmbk: C2Rust_Unnamed_0 = 90;
pub const sfx_bdcls: C2Rust_Unnamed_0 = 89;
pub const sfx_bdopn: C2Rust_Unnamed_0 = 88;
pub const sfx_tink: C2Rust_Unnamed_0 = 87;
pub const sfx_chgun: C2Rust_Unnamed_0 = 86;
pub const sfx_metal: C2Rust_Unnamed_0 = 85;
pub const sfx_hoof: C2Rust_Unnamed_0 = 84;
pub const sfx_punch: C2Rust_Unnamed_0 = 83;
pub const sfx_barexp: C2Rust_Unnamed_0 = 82;
pub const sfx_noway: C2Rust_Unnamed_0 = 81;
pub const sfx_vilact: C2Rust_Unnamed_0 = 80;
pub const sfx_bspwlk: C2Rust_Unnamed_0 = 79;
pub const sfx_bspact: C2Rust_Unnamed_0 = 78;
pub const sfx_dmact: C2Rust_Unnamed_0 = 77;
pub const sfx_bgact: C2Rust_Unnamed_0 = 76;
pub const sfx_posact: C2Rust_Unnamed_0 = 75;
pub const sfx_skedth: C2Rust_Unnamed_0 = 74;
pub const sfx_pedth: C2Rust_Unnamed_0 = 73;
pub const sfx_kntdth: C2Rust_Unnamed_0 = 72;
pub const sfx_vildth: C2Rust_Unnamed_0 = 71;
pub const sfx_bspdth: C2Rust_Unnamed_0 = 70;
pub const sfx_spidth: C2Rust_Unnamed_0 = 69;
pub const sfx_cybdth: C2Rust_Unnamed_0 = 68;
pub const sfx_brsdth: C2Rust_Unnamed_0 = 67;
pub const sfx_skldth: C2Rust_Unnamed_0 = 66;
pub const sfx_cacdth: C2Rust_Unnamed_0 = 65;
pub const sfx_sgtdth: C2Rust_Unnamed_0 = 64;
pub const sfx_bgdth2: C2Rust_Unnamed_0 = 63;
pub const sfx_bgdth1: C2Rust_Unnamed_0 = 62;
pub const sfx_podth3: C2Rust_Unnamed_0 = 61;
pub const sfx_podth2: C2Rust_Unnamed_0 = 60;
pub const sfx_podth1: C2Rust_Unnamed_0 = 59;
pub const sfx_pdiehi: C2Rust_Unnamed_0 = 58;
pub const sfx_pldeth: C2Rust_Unnamed_0 = 57;
pub const sfx_skeswg: C2Rust_Unnamed_0 = 56;
pub const sfx_claw: C2Rust_Unnamed_0 = 55;
pub const sfx_vilatk: C2Rust_Unnamed_0 = 54;
pub const sfx_skepch: C2Rust_Unnamed_0 = 53;
pub const sfx_sgtatk: C2Rust_Unnamed_0 = 52;
pub const sfx_sklatk: C2Rust_Unnamed_0 = 51;
pub const sfx_pesit: C2Rust_Unnamed_0 = 50;
pub const sfx_mansit: C2Rust_Unnamed_0 = 49;
pub const sfx_vilsit: C2Rust_Unnamed_0 = 48;
pub const sfx_kntsit: C2Rust_Unnamed_0 = 47;
pub const sfx_bspsit: C2Rust_Unnamed_0 = 46;
pub const sfx_spisit: C2Rust_Unnamed_0 = 45;
pub const sfx_cybsit: C2Rust_Unnamed_0 = 44;
pub const sfx_brssit: C2Rust_Unnamed_0 = 43;
pub const sfx_cacsit: C2Rust_Unnamed_0 = 42;
pub const sfx_sgtsit: C2Rust_Unnamed_0 = 41;
pub const sfx_bgsit2: C2Rust_Unnamed_0 = 40;
pub const sfx_bgsit1: C2Rust_Unnamed_0 = 39;
pub const sfx_posit3: C2Rust_Unnamed_0 = 38;
pub const sfx_posit2: C2Rust_Unnamed_0 = 37;
pub const sfx_posit1: C2Rust_Unnamed_0 = 36;
pub const sfx_telept: C2Rust_Unnamed_0 = 35;
pub const sfx_oof: C2Rust_Unnamed_0 = 34;
pub const sfx_wpnup: C2Rust_Unnamed_0 = 33;
pub const sfx_itemup: C2Rust_Unnamed_0 = 32;
pub const sfx_slop: C2Rust_Unnamed_0 = 31;
pub const sfx_pepain: C2Rust_Unnamed_0 = 30;
pub const sfx_mnpain: C2Rust_Unnamed_0 = 29;
pub const sfx_vipain: C2Rust_Unnamed_0 = 28;
pub const sfx_popain: C2Rust_Unnamed_0 = 27;
pub const sfx_dmpain: C2Rust_Unnamed_0 = 26;
pub const sfx_plpain: C2Rust_Unnamed_0 = 25;
pub const sfx_swtchx: C2Rust_Unnamed_0 = 24;
pub const sfx_swtchn: C2Rust_Unnamed_0 = 23;
pub const sfx_stnmov: C2Rust_Unnamed_0 = 22;
pub const sfx_dorcls: C2Rust_Unnamed_0 = 21;
pub const sfx_doropn: C2Rust_Unnamed_0 = 20;
pub const sfx_pstop: C2Rust_Unnamed_0 = 19;
pub const sfx_pstart: C2Rust_Unnamed_0 = 18;
pub const sfx_firxpl: C2Rust_Unnamed_0 = 17;
pub const sfx_firsht: C2Rust_Unnamed_0 = 16;
pub const sfx_rxplod: C2Rust_Unnamed_0 = 15;
pub const sfx_rlaunc: C2Rust_Unnamed_0 = 14;
pub const sfx_sawhit: C2Rust_Unnamed_0 = 13;
pub const sfx_sawful: C2Rust_Unnamed_0 = 12;
pub const sfx_sawidl: C2Rust_Unnamed_0 = 11;
pub const sfx_sawup: C2Rust_Unnamed_0 = 10;
pub const sfx_bfg: C2Rust_Unnamed_0 = 9;
pub const sfx_plasma: C2Rust_Unnamed_0 = 8;
pub const sfx_dbload: C2Rust_Unnamed_0 = 7;
pub const sfx_dbcls: C2Rust_Unnamed_0 = 6;
pub const sfx_dbopn: C2Rust_Unnamed_0 = 5;
pub const sfx_dshtgn: C2Rust_Unnamed_0 = 4;
pub const sfx_sgcock: C2Rust_Unnamed_0 = 3;
pub const sfx_shotgn: C2Rust_Unnamed_0 = 2;
pub const sfx_pistol: C2Rust_Unnamed_0 = 1;
pub const sfx_None: C2Rust_Unnamed_0 = 0;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct menuitem_t {
    pub status: ::core::ffi::c_short,
    pub name: [::core::ffi::c_char; 10],
    pub routine: Option<unsafe extern "C" fn(::core::ffi::c_int) -> ()>,
    pub alphaKey: ::core::ffi::c_char,
}
pub type menu_t = menu_s;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct menu_s {
    pub numitems: ::core::ffi::c_short,
    pub prevMenu: *mut menu_s,
    pub menuitems: *mut menuitem_t,
    pub routine: Option<unsafe extern "C" fn() -> ()>,
    pub x: ::core::ffi::c_short,
    pub y: ::core::ffi::c_short,
    pub lastOn: ::core::ffi::c_short,
}
pub const read2_end: C2Rust_Unnamed_6 = 1;
pub const read1_end: C2Rust_Unnamed_5 = 1;
pub const load_end: C2Rust_Unnamed_8 = 6;
pub const scrnsize: C2Rust_Unnamed_4 = 3;
pub const mousesens: C2Rust_Unnamed_4 = 5;
pub const messages: C2Rust_Unnamed_4 = 1;
pub const detail: C2Rust_Unnamed_4 = 2;
pub const music_vol: C2Rust_Unnamed_7 = 2;
pub const sfx_vol: C2Rust_Unnamed_7 = 0;
pub const sound_end: C2Rust_Unnamed_7 = 4;
pub const opt_end: C2Rust_Unnamed_4 = 8;
pub const ep1: C2Rust_Unnamed_2 = 0;
pub const hurtme: C2Rust_Unnamed_3 = 2;
pub const nightmare: C2Rust_Unnamed_3 = 4;
pub const newg_end: C2Rust_Unnamed_3 = 5;
pub const ep_end: C2Rust_Unnamed_2 = 4;
pub const main_end: C2Rust_Unnamed_1 = 6;
pub const quitdoom: C2Rust_Unnamed_1 = 5;
pub const readthis: C2Rust_Unnamed_1 = 4;
pub type C2Rust_Unnamed_1 = ::core::ffi::c_uint;
pub const savegame: C2Rust_Unnamed_1 = 3;
pub const loadgame: C2Rust_Unnamed_1 = 2;
pub const options: C2Rust_Unnamed_1 = 1;
pub const newgame: C2Rust_Unnamed_1 = 0;
pub type C2Rust_Unnamed_2 = ::core::ffi::c_uint;
pub const ep4: C2Rust_Unnamed_2 = 3;
pub const ep3: C2Rust_Unnamed_2 = 2;
pub const ep2: C2Rust_Unnamed_2 = 1;
pub type C2Rust_Unnamed_3 = ::core::ffi::c_uint;
pub const violence: C2Rust_Unnamed_3 = 3;
pub const toorough: C2Rust_Unnamed_3 = 1;
pub const killthings: C2Rust_Unnamed_3 = 0;
pub type C2Rust_Unnamed_4 = ::core::ffi::c_uint;
pub const soundvol: C2Rust_Unnamed_4 = 7;
pub const option_empty2: C2Rust_Unnamed_4 = 6;
pub const option_empty1: C2Rust_Unnamed_4 = 4;
pub const endgame: C2Rust_Unnamed_4 = 0;
pub type C2Rust_Unnamed_5 = ::core::ffi::c_uint;
pub const rdthsempty1: C2Rust_Unnamed_5 = 0;
pub type C2Rust_Unnamed_6 = ::core::ffi::c_uint;
pub const rdthsempty2: C2Rust_Unnamed_6 = 0;
pub type C2Rust_Unnamed_7 = ::core::ffi::c_uint;
pub const sfx_empty2: C2Rust_Unnamed_7 = 3;
pub const sfx_empty1: C2Rust_Unnamed_7 = 1;
pub type C2Rust_Unnamed_8 = ::core::ffi::c_uint;
pub const load6: C2Rust_Unnamed_8 = 5;
pub const load5: C2Rust_Unnamed_8 = 4;
pub const load4: C2Rust_Unnamed_8 = 3;
pub const load3: C2Rust_Unnamed_8 = 2;
pub const load2: C2Rust_Unnamed_8 = 1;
pub const load1: C2Rust_Unnamed_8 = 0;
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
pub const true_0: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
pub const false_0: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
pub const KEY_ESCAPE: ::core::ffi::c_int = 27 as ::core::ffi::c_int;
pub const KEY_ENTER: ::core::ffi::c_int = 13;
pub const KEY_BACKSPACE: ::core::ffi::c_int = 127;
pub const KEY_PAUSE: ::core::ffi::c_int = 0xff as ::core::ffi::c_int;
pub const KEY_CAPSLOCK: ::core::ffi::c_int =
    0x80 as ::core::ffi::c_int + 0x3a as ::core::ffi::c_int;
pub const KEY_NUMLOCK: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x45 as ::core::ffi::c_int;
pub const KEY_SCRLCK: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int + 0x46 as ::core::ffi::c_int;
pub const GAMMALVL0: [::core::ffi::c_char; 26] = unsafe {
    ::core::mem::transmute::<[u8; 26], [::core::ffi::c_char; 26]>(
        *b"Gamma correction OFF\0\0\0\0\0\0",
    )
};
pub const GAMMALVL1: [::core::ffi::c_char; 26] = unsafe {
    ::core::mem::transmute::<[u8; 26], [::core::ffi::c_char; 26]>(*b"Gamma correction level 1\0\0")
};
pub const GAMMALVL2: [::core::ffi::c_char; 26] = unsafe {
    ::core::mem::transmute::<[u8; 26], [::core::ffi::c_char; 26]>(*b"Gamma correction level 2\0\0")
};
pub const GAMMALVL3: [::core::ffi::c_char; 26] = unsafe {
    ::core::mem::transmute::<[u8; 26], [::core::ffi::c_char; 26]>(*b"Gamma correction level 3\0\0")
};
pub const GAMMALVL4: [::core::ffi::c_char; 26] = unsafe {
    ::core::mem::transmute::<[u8; 26], [::core::ffi::c_char; 26]>(*b"Gamma correction level 4\0\0")
};
pub const EMPTYSTRING: [::core::ffi::c_char; 11] =
    unsafe { ::core::mem::transmute::<[u8; 11], [::core::ffi::c_char; 11]>(*b"empty slot\0") };
pub const NUM_QUITMESSAGES: ::core::ffi::c_int = 8 as ::core::ffi::c_int;
pub const SCREENWIDTH: ::core::ffi::c_int = 320 as ::core::ffi::c_int;
pub const SCREENHEIGHT: ::core::ffi::c_int = 200 as ::core::ffi::c_int;
pub const HU_FONTSTART: ::core::ffi::c_int = '!' as ::core::ffi::c_int;
pub const HU_FONTEND: ::core::ffi::c_int = '_' as ::core::ffi::c_int;
pub const HU_FONTSIZE: ::core::ffi::c_int = HU_FONTEND - HU_FONTSTART + 1 as ::core::ffi::c_int;
pub const SAVESTRINGSIZE: ::core::ffi::c_int = 24 as ::core::ffi::c_int;
#[no_mangle]
pub static mut mouseSensitivity: ::core::ffi::c_int = 5 as ::core::ffi::c_int;
#[no_mangle]
pub static mut showMessages: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
#[no_mangle]
pub static mut detailLevel: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
#[no_mangle]
pub static mut screenblocks: ::core::ffi::c_int = 10 as ::core::ffi::c_int;
#[no_mangle]
pub static mut screenSize: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut quickSaveSlot: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut messageToPrint: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut messageString: *mut ::core::ffi::c_char =
    ::core::ptr::null_mut::<::core::ffi::c_char>();
#[no_mangle]
pub static mut messx: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut messy: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut messageLastMenuActive: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut messageNeedsInput: boolean = 0;
#[no_mangle]
pub static mut messageRoutine: Option<unsafe extern "C" fn(::core::ffi::c_int) -> ()> = None;
#[no_mangle]
pub static mut gammamsg: [[::core::ffi::c_char; 26]; 5] =
    [GAMMALVL0, GAMMALVL1, GAMMALVL2, GAMMALVL3, GAMMALVL4];
#[no_mangle]
pub static mut saveStringEnter: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut saveSlot: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut saveCharIndex: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut saveOldString: [::core::ffi::c_char; 24] = [0; 24];
#[no_mangle]
pub static mut inhelpscreens: boolean = 0;
#[no_mangle]
pub static mut menuactive: boolean = 0;
pub const SKULLXOFF: ::core::ffi::c_int = -32 as ::core::ffi::c_int;
pub const LINEHEIGHT: ::core::ffi::c_int = 16 as ::core::ffi::c_int;
#[no_mangle]
pub static mut savegamestrings: [[::core::ffi::c_char; 24]; 10] = [[0; 24]; 10];
#[no_mangle]
pub static mut endstring: [::core::ffi::c_char; 160] = [0; 160];
#[no_mangle]
pub static mut itemOn: ::core::ffi::c_short = 0;
#[no_mangle]
pub static mut skullAnimCounter: ::core::ffi::c_short = 0;
#[no_mangle]
pub static mut whichSkull: ::core::ffi::c_short = 0;
#[no_mangle]
pub static mut skullName: [*mut ::core::ffi::c_char; 2] = [
    b"M_SKULL1\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
    b"M_SKULL2\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
];
#[no_mangle]
pub static mut currentMenu: *mut menu_t = ::core::ptr::null_mut::<menu_t>();
#[no_mangle]
pub static mut main_e: C2Rust_Unnamed_1 = newgame;
#[no_mangle]
pub static mut MainMenu: [menuitem_t; 6] = unsafe {
    [
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_NGAME\0\0\0"),
            routine: Some(M_NewGame as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 'n' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_OPTION\0\0"),
            routine: Some(M_Options as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 'o' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_LOADG\0\0\0"),
            routine: Some(M_LoadGame as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 'l' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_SAVEG\0\0\0"),
            routine: Some(M_SaveGame as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 's' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_RDTHIS\0\0"),
            routine: Some(M_ReadThis as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 'r' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_QUITG\0\0\0"),
            routine: Some(M_QuitDOOM as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 'q' as ::core::ffi::c_char,
        },
    ]
};
#[no_mangle]
pub static mut MainDef: menu_t = unsafe {
    menu_s {
        numitems: main_end as ::core::ffi::c_int as ::core::ffi::c_short,
        prevMenu: ::core::ptr::null_mut::<menu_s>(),
        menuitems: &raw const MainMenu as *mut menuitem_t,
        routine: Some(M_DrawMainMenu as unsafe extern "C" fn() -> ()),
        x: 97 as ::core::ffi::c_short,
        y: 64 as ::core::ffi::c_short,
        lastOn: 0 as ::core::ffi::c_short,
    }
};
#[no_mangle]
pub static mut episodes_e: C2Rust_Unnamed_2 = ep1;
#[no_mangle]
pub static mut EpisodeMenu: [menuitem_t; 4] = unsafe {
    [
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_EPI1\0\0\0\0"),
            routine: Some(M_Episode as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 'k' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_EPI2\0\0\0\0"),
            routine: Some(M_Episode as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 't' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_EPI3\0\0\0\0"),
            routine: Some(M_Episode as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 'i' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_EPI4\0\0\0\0"),
            routine: Some(M_Episode as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 't' as ::core::ffi::c_char,
        },
    ]
};
#[no_mangle]
pub static mut EpiDef: menu_t = unsafe {
    menu_s {
        numitems: ep_end as ::core::ffi::c_int as ::core::ffi::c_short,
        prevMenu: &raw const MainDef as *mut menu_s,
        menuitems: &raw const EpisodeMenu as *mut menuitem_t,
        routine: Some(M_DrawEpisode as unsafe extern "C" fn() -> ()),
        x: 48 as ::core::ffi::c_short,
        y: 63 as ::core::ffi::c_short,
        lastOn: ep1 as ::core::ffi::c_int as ::core::ffi::c_short,
    }
};
#[no_mangle]
pub static mut newgame_e: C2Rust_Unnamed_3 = killthings;
#[no_mangle]
pub static mut NewGameMenu: [menuitem_t; 5] = unsafe {
    [
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_JKILL\0\0\0"),
            routine: Some(M_ChooseSkill as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 'i' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_ROUGH\0\0\0"),
            routine: Some(M_ChooseSkill as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 'h' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_HURT\0\0\0\0"),
            routine: Some(M_ChooseSkill as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 'h' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_ULTRA\0\0\0"),
            routine: Some(M_ChooseSkill as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 'u' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_NMARE\0\0\0"),
            routine: Some(M_ChooseSkill as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 'n' as ::core::ffi::c_char,
        },
    ]
};
#[no_mangle]
pub static mut NewDef: menu_t = unsafe {
    menu_s {
        numitems: newg_end as ::core::ffi::c_int as ::core::ffi::c_short,
        prevMenu: &raw const EpiDef as *mut menu_s,
        menuitems: &raw const NewGameMenu as *mut menuitem_t,
        routine: Some(M_DrawNewGame as unsafe extern "C" fn() -> ()),
        x: 48 as ::core::ffi::c_short,
        y: 63 as ::core::ffi::c_short,
        lastOn: hurtme as ::core::ffi::c_int as ::core::ffi::c_short,
    }
};
#[no_mangle]
pub static mut options_e: C2Rust_Unnamed_4 = endgame;
#[no_mangle]
pub static mut OptionsMenu: [menuitem_t; 8] = unsafe {
    [
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_ENDGAM\0\0"),
            routine: Some(M_EndGame as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 'e' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_MESSG\0\0\0"),
            routine: Some(M_ChangeMessages as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 'm' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_DETAIL\0\0"),
            routine: Some(M_ChangeDetail as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 'g' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 2 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_SCRNSZ\0\0"),
            routine: Some(M_SizeDisplay as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 's' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: -1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(
                *b"\0\0\0\0\0\0\0\0\0\0",
            ),
            routine: None,
            alphaKey: '\0' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 2 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_MSENS\0\0\0"),
            routine: Some(M_ChangeSensitivity as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 'm' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: -1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(
                *b"\0\0\0\0\0\0\0\0\0\0",
            ),
            routine: None,
            alphaKey: '\0' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_SVOL\0\0\0\0"),
            routine: Some(M_Sound as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 's' as ::core::ffi::c_char,
        },
    ]
};
#[no_mangle]
pub static mut OptionsDef: menu_t = unsafe {
    menu_s {
        numitems: opt_end as ::core::ffi::c_int as ::core::ffi::c_short,
        prevMenu: &raw const MainDef as *mut menu_s,
        menuitems: &raw const OptionsMenu as *mut menuitem_t,
        routine: Some(M_DrawOptions as unsafe extern "C" fn() -> ()),
        x: 60 as ::core::ffi::c_short,
        y: 37 as ::core::ffi::c_short,
        lastOn: 0 as ::core::ffi::c_short,
    }
};
#[no_mangle]
pub static mut read_e: C2Rust_Unnamed_5 = rdthsempty1;
#[no_mangle]
pub static mut ReadMenu1: [menuitem_t; 1] = unsafe {
    [menuitem_t {
        status: 1 as ::core::ffi::c_short,
        name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(
            *b"\0\0\0\0\0\0\0\0\0\0",
        ),
        routine: Some(M_ReadThis2 as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
        alphaKey: 0 as ::core::ffi::c_char,
    }]
};
#[no_mangle]
pub static mut ReadDef1: menu_t = unsafe {
    menu_s {
        numitems: read1_end as ::core::ffi::c_int as ::core::ffi::c_short,
        prevMenu: &raw const MainDef as *mut menu_s,
        menuitems: &raw const ReadMenu1 as *mut menuitem_t,
        routine: Some(M_DrawReadThis1 as unsafe extern "C" fn() -> ()),
        x: 280 as ::core::ffi::c_short,
        y: 185 as ::core::ffi::c_short,
        lastOn: 0 as ::core::ffi::c_short,
    }
};
#[no_mangle]
pub static mut read_e2: C2Rust_Unnamed_6 = rdthsempty2;
#[no_mangle]
pub static mut ReadMenu2: [menuitem_t; 1] = unsafe {
    [menuitem_t {
        status: 1 as ::core::ffi::c_short,
        name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(
            *b"\0\0\0\0\0\0\0\0\0\0",
        ),
        routine: Some(M_FinishReadThis as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
        alphaKey: 0 as ::core::ffi::c_char,
    }]
};
#[no_mangle]
pub static mut ReadDef2: menu_t = unsafe {
    menu_s {
        numitems: read2_end as ::core::ffi::c_int as ::core::ffi::c_short,
        prevMenu: &raw const ReadDef1 as *mut menu_s,
        menuitems: &raw const ReadMenu2 as *mut menuitem_t,
        routine: Some(M_DrawReadThis2 as unsafe extern "C" fn() -> ()),
        x: 330 as ::core::ffi::c_short,
        y: 175 as ::core::ffi::c_short,
        lastOn: 0 as ::core::ffi::c_short,
    }
};
#[no_mangle]
pub static mut sound_e: C2Rust_Unnamed_7 = sfx_vol;
#[no_mangle]
pub static mut SoundMenu: [menuitem_t; 4] = unsafe {
    [
        menuitem_t {
            status: 2 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_SFXVOL\0\0"),
            routine: Some(M_SfxVol as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 's' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: -1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(
                *b"\0\0\0\0\0\0\0\0\0\0",
            ),
            routine: None,
            alphaKey: '\0' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 2 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(*b"M_MUSVOL\0\0"),
            routine: Some(M_MusicVol as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: 'm' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: -1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(
                *b"\0\0\0\0\0\0\0\0\0\0",
            ),
            routine: None,
            alphaKey: '\0' as ::core::ffi::c_char,
        },
    ]
};
#[no_mangle]
pub static mut SoundDef: menu_t = unsafe {
    menu_s {
        numitems: sound_end as ::core::ffi::c_int as ::core::ffi::c_short,
        prevMenu: &raw const OptionsDef as *mut menu_s,
        menuitems: &raw const SoundMenu as *mut menuitem_t,
        routine: Some(M_DrawSound as unsafe extern "C" fn() -> ()),
        x: 80 as ::core::ffi::c_short,
        y: 64 as ::core::ffi::c_short,
        lastOn: 0 as ::core::ffi::c_short,
    }
};
#[no_mangle]
pub static mut load_e: C2Rust_Unnamed_8 = load1;
#[no_mangle]
pub static mut LoadMenu: [menuitem_t; 6] = unsafe {
    [
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(
                *b"\0\0\0\0\0\0\0\0\0\0",
            ),
            routine: Some(M_LoadSelect as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: '1' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(
                *b"\0\0\0\0\0\0\0\0\0\0",
            ),
            routine: Some(M_LoadSelect as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: '2' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(
                *b"\0\0\0\0\0\0\0\0\0\0",
            ),
            routine: Some(M_LoadSelect as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: '3' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(
                *b"\0\0\0\0\0\0\0\0\0\0",
            ),
            routine: Some(M_LoadSelect as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: '4' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(
                *b"\0\0\0\0\0\0\0\0\0\0",
            ),
            routine: Some(M_LoadSelect as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: '5' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(
                *b"\0\0\0\0\0\0\0\0\0\0",
            ),
            routine: Some(M_LoadSelect as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: '6' as ::core::ffi::c_char,
        },
    ]
};
#[no_mangle]
pub static mut LoadDef: menu_t = unsafe {
    menu_s {
        numitems: load_end as ::core::ffi::c_int as ::core::ffi::c_short,
        prevMenu: &raw const MainDef as *mut menu_s,
        menuitems: &raw const LoadMenu as *mut menuitem_t,
        routine: Some(M_DrawLoad as unsafe extern "C" fn() -> ()),
        x: 80 as ::core::ffi::c_short,
        y: 54 as ::core::ffi::c_short,
        lastOn: 0 as ::core::ffi::c_short,
    }
};
#[no_mangle]
pub static mut SaveMenu: [menuitem_t; 6] = unsafe {
    [
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(
                *b"\0\0\0\0\0\0\0\0\0\0",
            ),
            routine: Some(M_SaveSelect as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: '1' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(
                *b"\0\0\0\0\0\0\0\0\0\0",
            ),
            routine: Some(M_SaveSelect as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: '2' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(
                *b"\0\0\0\0\0\0\0\0\0\0",
            ),
            routine: Some(M_SaveSelect as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: '3' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(
                *b"\0\0\0\0\0\0\0\0\0\0",
            ),
            routine: Some(M_SaveSelect as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: '4' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(
                *b"\0\0\0\0\0\0\0\0\0\0",
            ),
            routine: Some(M_SaveSelect as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: '5' as ::core::ffi::c_char,
        },
        menuitem_t {
            status: 1 as ::core::ffi::c_short,
            name: ::core::mem::transmute::<[u8; 10], [::core::ffi::c_char; 10]>(
                *b"\0\0\0\0\0\0\0\0\0\0",
            ),
            routine: Some(M_SaveSelect as unsafe extern "C" fn(::core::ffi::c_int) -> ()),
            alphaKey: '6' as ::core::ffi::c_char,
        },
    ]
};
#[no_mangle]
pub static mut SaveDef: menu_t = unsafe {
    menu_s {
        numitems: load_end as ::core::ffi::c_int as ::core::ffi::c_short,
        prevMenu: &raw const MainDef as *mut menu_s,
        menuitems: &raw const SaveMenu as *mut menuitem_t,
        routine: Some(M_DrawSave as unsafe extern "C" fn() -> ()),
        x: 80 as ::core::ffi::c_short,
        y: 54 as ::core::ffi::c_short,
        lastOn: 0 as ::core::ffi::c_short,
    }
};
#[no_mangle]
pub unsafe extern "C" fn M_ReadSaveStrings() {
    let mut handle: *mut FILE = ::core::ptr::null_mut::<FILE>();
    let mut i: ::core::ffi::c_int = 0;
    let mut name: [::core::ffi::c_char; 256] = [0; 256];
    i = 0 as ::core::ffi::c_int;
    while i < load_end as ::core::ffi::c_int {
        M_StringCopy(
            &raw mut name as *mut ::core::ffi::c_char,
            P_SaveGameFile(i),
            ::core::mem::size_of::<[::core::ffi::c_char; 256]>() as size_t,
        );
        handle = fopen(
            &raw mut name as *mut ::core::ffi::c_char,
            b"rb\0".as_ptr() as *const ::core::ffi::c_char,
        ) as *mut FILE;
        if handle.is_null() {
            M_StringCopy(
                &raw mut *(&raw mut savegamestrings as *mut [::core::ffi::c_char; 24])
                    .offset(i as isize) as *mut ::core::ffi::c_char,
                EMPTYSTRING.as_ptr(),
                SAVESTRINGSIZE as size_t,
            );
            LoadMenu[i as usize].status = 0 as ::core::ffi::c_short;
        } else {
            fread(
                (&raw mut savegamestrings as *mut [::core::ffi::c_char; 24]).offset(i as isize)
                    as *mut [::core::ffi::c_char; 24] as *mut ::core::ffi::c_void,
                1 as size_t,
                SAVESTRINGSIZE as size_t,
                handle,
            );
            fclose(handle);
            LoadMenu[i as usize].status = 1 as ::core::ffi::c_short;
        }
        i += 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn M_DrawLoad() {
    let mut i: ::core::ffi::c_int = 0;
    V_DrawPatchDirect(
        72 as ::core::ffi::c_int,
        28 as ::core::ffi::c_int,
        W_CacheLumpName(
            b"M_LOADG\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
    i = 0 as ::core::ffi::c_int;
    while i < load_end as ::core::ffi::c_int {
        M_DrawSaveLoadBorder(
            LoadDef.x as ::core::ffi::c_int,
            LoadDef.y as ::core::ffi::c_int + LINEHEIGHT * i,
        );
        M_WriteText(
            LoadDef.x as ::core::ffi::c_int,
            LoadDef.y as ::core::ffi::c_int + LINEHEIGHT * i,
            &raw mut *(&raw mut savegamestrings as *mut [::core::ffi::c_char; 24])
                .offset(i as isize) as *mut ::core::ffi::c_char,
        );
        i += 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn M_DrawSaveLoadBorder(
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
) {
    let mut i: ::core::ffi::c_int = 0;
    V_DrawPatchDirect(
        x - 8 as ::core::ffi::c_int,
        y + 7 as ::core::ffi::c_int,
        W_CacheLumpName(
            b"M_LSLEFT\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
    i = 0 as ::core::ffi::c_int;
    while i < 24 as ::core::ffi::c_int {
        V_DrawPatchDirect(
            x,
            y + 7 as ::core::ffi::c_int,
            W_CacheLumpName(
                b"M_LSCNTR\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
                PU_CACHE as ::core::ffi::c_int,
            ) as *mut patch_t,
        );
        x += 8 as ::core::ffi::c_int;
        i += 1;
    }
    V_DrawPatchDirect(
        x,
        y + 7 as ::core::ffi::c_int,
        W_CacheLumpName(
            b"M_LSRGHT\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_LoadSelect(mut choice: ::core::ffi::c_int) {
    let mut name: [::core::ffi::c_char; 256] = [0; 256];
    M_StringCopy(
        &raw mut name as *mut ::core::ffi::c_char,
        P_SaveGameFile(choice),
        ::core::mem::size_of::<[::core::ffi::c_char; 256]>() as size_t,
    );
    G_LoadGame(&raw mut name as *mut ::core::ffi::c_char);
    M_ClearMenus();
}
#[no_mangle]
pub unsafe extern "C" fn M_LoadGame(mut choice: ::core::ffi::c_int) {
    if netgame != 0 {
        M_StartMessage(
            b"you can't do load while in a net game!\n\npress a key.\0".as_ptr()
                as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            NULL,
            false_0 as boolean,
        );
        return;
    }
    M_SetupNextMenu(&raw mut LoadDef);
    M_ReadSaveStrings();
}
#[no_mangle]
pub unsafe extern "C" fn M_DrawSave() {
    let mut i: ::core::ffi::c_int = 0;
    V_DrawPatchDirect(
        72 as ::core::ffi::c_int,
        28 as ::core::ffi::c_int,
        W_CacheLumpName(
            b"M_SAVEG\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
    i = 0 as ::core::ffi::c_int;
    while i < load_end as ::core::ffi::c_int {
        M_DrawSaveLoadBorder(
            LoadDef.x as ::core::ffi::c_int,
            LoadDef.y as ::core::ffi::c_int + LINEHEIGHT * i,
        );
        M_WriteText(
            LoadDef.x as ::core::ffi::c_int,
            LoadDef.y as ::core::ffi::c_int + LINEHEIGHT * i,
            &raw mut *(&raw mut savegamestrings as *mut [::core::ffi::c_char; 24])
                .offset(i as isize) as *mut ::core::ffi::c_char,
        );
        i += 1;
    }
    if saveStringEnter != 0 {
        i = M_StringWidth(
            &raw mut *(&raw mut savegamestrings as *mut [::core::ffi::c_char; 24])
                .offset(saveSlot as isize) as *mut ::core::ffi::c_char,
        );
        M_WriteText(
            LoadDef.x as ::core::ffi::c_int + i,
            LoadDef.y as ::core::ffi::c_int + LINEHEIGHT * saveSlot,
            b"_\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        );
    }
}
#[no_mangle]
pub unsafe extern "C" fn M_DoSave(mut slot: ::core::ffi::c_int) {
    G_SaveGame(
        slot,
        &raw mut *(&raw mut savegamestrings as *mut [::core::ffi::c_char; 24]).offset(slot as isize)
            as *mut ::core::ffi::c_char,
    );
    M_ClearMenus();
    if quickSaveSlot == -2 as ::core::ffi::c_int {
        quickSaveSlot = slot;
    }
}
#[no_mangle]
pub unsafe extern "C" fn M_SaveSelect(mut choice: ::core::ffi::c_int) {
    saveStringEnter = 1 as ::core::ffi::c_int;
    saveSlot = choice;
    M_StringCopy(
        &raw mut saveOldString as *mut ::core::ffi::c_char,
        &raw mut *(&raw mut savegamestrings as *mut [::core::ffi::c_char; 24])
            .offset(choice as isize) as *mut ::core::ffi::c_char,
        SAVESTRINGSIZE as size_t,
    );
    if strcmp(
        &raw mut *(&raw mut savegamestrings as *mut [::core::ffi::c_char; 24])
            .offset(choice as isize) as *mut ::core::ffi::c_char,
        EMPTYSTRING.as_ptr(),
    ) == 0
    {
        savegamestrings[choice as usize][0 as ::core::ffi::c_int as usize] =
            0 as ::core::ffi::c_char;
    }
    saveCharIndex = strlen(
        &raw mut *(&raw mut savegamestrings as *mut [::core::ffi::c_char; 24])
            .offset(choice as isize) as *mut ::core::ffi::c_char,
    ) as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn M_SaveGame(mut choice: ::core::ffi::c_int) {
    if usergame == 0 {
        M_StartMessage(
            b"you can't save if you aren't playing!\n\npress a key.\0".as_ptr()
                as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            NULL,
            false_0 as boolean,
        );
        return;
    }
    if gamestate as ::core::ffi::c_uint != GS_LEVEL as ::core::ffi::c_int as ::core::ffi::c_uint {
        return;
    }
    M_SetupNextMenu(&raw mut SaveDef);
    M_ReadSaveStrings();
}
#[no_mangle]
pub static mut tempstring: [::core::ffi::c_char; 80] = [0; 80];
#[no_mangle]
pub unsafe extern "C" fn M_QuickSaveResponse(mut key: ::core::ffi::c_int) {
    if key == key_menu_confirm {
        M_DoSave(quickSaveSlot);
        S_StartSound(NULL, sfx_swtchx as ::core::ffi::c_int);
    }
}
#[no_mangle]
pub unsafe extern "C" fn M_QuickSave() {
    if usergame == 0 {
        S_StartSound(NULL, sfx_oof as ::core::ffi::c_int);
        return;
    }
    if gamestate as ::core::ffi::c_uint != GS_LEVEL as ::core::ffi::c_int as ::core::ffi::c_uint {
        return;
    }
    if quickSaveSlot < 0 as ::core::ffi::c_int {
        M_StartControlPanel();
        M_ReadSaveStrings();
        M_SetupNextMenu(&raw mut SaveDef);
        quickSaveSlot = -2 as ::core::ffi::c_int;
        return;
    }
    snprintf(
        &raw mut tempstring as *mut ::core::ffi::c_char,
        80 as size_t,
        b"quicksave over your game named\n\n'%s'?\n\npress y or n.\0".as_ptr()
            as *const ::core::ffi::c_char,
        &raw mut *(&raw mut savegamestrings as *mut [::core::ffi::c_char; 24])
            .offset(quickSaveSlot as isize) as *mut ::core::ffi::c_char,
    );
    M_StartMessage(
        &raw mut tempstring as *mut ::core::ffi::c_char,
        ::core::mem::transmute::<
            Option<unsafe extern "C" fn(::core::ffi::c_int) -> ()>,
            *mut ::core::ffi::c_void,
        >(Some(
            M_QuickSaveResponse as unsafe extern "C" fn(::core::ffi::c_int) -> (),
        )),
        true_0 as boolean,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_QuickLoadResponse(mut key: ::core::ffi::c_int) {
    if key == key_menu_confirm {
        M_LoadSelect(quickSaveSlot);
        S_StartSound(NULL, sfx_swtchx as ::core::ffi::c_int);
    }
}
#[no_mangle]
pub unsafe extern "C" fn M_QuickLoad() {
    if netgame != 0 {
        M_StartMessage(
            b"you can't quickload during a netgame!\n\npress a key.\0".as_ptr()
                as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            NULL,
            false_0 as boolean,
        );
        return;
    }
    if quickSaveSlot < 0 as ::core::ffi::c_int {
        M_StartMessage(
            b"you haven't picked a quicksave slot yet!\n\npress a key.\0".as_ptr()
                as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            NULL,
            false_0 as boolean,
        );
        return;
    }
    snprintf(
        &raw mut tempstring as *mut ::core::ffi::c_char,
        80 as size_t,
        b"do you want to quickload the game named\n\n'%s'?\n\npress y or n.\0".as_ptr()
            as *const ::core::ffi::c_char,
        &raw mut *(&raw mut savegamestrings as *mut [::core::ffi::c_char; 24])
            .offset(quickSaveSlot as isize) as *mut ::core::ffi::c_char,
    );
    M_StartMessage(
        &raw mut tempstring as *mut ::core::ffi::c_char,
        ::core::mem::transmute::<
            Option<unsafe extern "C" fn(::core::ffi::c_int) -> ()>,
            *mut ::core::ffi::c_void,
        >(Some(
            M_QuickLoadResponse as unsafe extern "C" fn(::core::ffi::c_int) -> (),
        )),
        true_0 as boolean,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_DrawReadThis1() {
    let mut lumpname: *mut ::core::ffi::c_char =
        b"CREDIT\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
    let mut skullx: ::core::ffi::c_int = 330 as ::core::ffi::c_int;
    let mut skully: ::core::ffi::c_int = 175 as ::core::ffi::c_int;
    inhelpscreens = true_0 as boolean;
    match gameversion as ::core::ffi::c_uint {
        1 | 2 | 3 | 4 | 5 => {
            if gamemode as ::core::ffi::c_uint
                == commercial as ::core::ffi::c_int as ::core::ffi::c_uint
            {
                lumpname =
                    b"HELP\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
                skullx = 330 as ::core::ffi::c_int;
                skully = 165 as ::core::ffi::c_int;
            } else {
                lumpname =
                    b"HELP2\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
                skullx = 280 as ::core::ffi::c_int;
                skully = 185 as ::core::ffi::c_int;
            }
        }
        6 | 9 => {
            lumpname =
                b"HELP1\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        }
        7 | 8 => {
            lumpname = b"HELP\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        }
        _ => {
            let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
            snprintf(
                &raw mut _i_err_buf as *mut ::core::ffi::c_char,
                ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
                b"Unhandled game version\0".as_ptr() as *const ::core::ffi::c_char,
            );
            I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
        }
    }
    lumpname = lumpname;
    V_DrawPatchDirect(
        0 as ::core::ffi::c_int,
        0 as ::core::ffi::c_int,
        W_CacheLumpName(lumpname, PU_CACHE as ::core::ffi::c_int) as *mut patch_t,
    );
    ReadDef1.x = skullx as ::core::ffi::c_short;
    ReadDef1.y = skully as ::core::ffi::c_short;
}
#[no_mangle]
pub unsafe extern "C" fn M_DrawReadThis2() {
    inhelpscreens = true_0 as boolean;
    V_DrawPatchDirect(
        0 as ::core::ffi::c_int,
        0 as ::core::ffi::c_int,
        W_CacheLumpName(
            b"HELP1\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_DrawSound() {
    V_DrawPatchDirect(
        60 as ::core::ffi::c_int,
        38 as ::core::ffi::c_int,
        W_CacheLumpName(
            b"M_SVOL\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
    M_DrawThermo(
        SoundDef.x as ::core::ffi::c_int,
        SoundDef.y as ::core::ffi::c_int
            + LINEHEIGHT * (sfx_vol as ::core::ffi::c_int + 1 as ::core::ffi::c_int),
        16 as ::core::ffi::c_int,
        sfxVolume,
    );
    M_DrawThermo(
        SoundDef.x as ::core::ffi::c_int,
        SoundDef.y as ::core::ffi::c_int
            + LINEHEIGHT * (music_vol as ::core::ffi::c_int + 1 as ::core::ffi::c_int),
        16 as ::core::ffi::c_int,
        musicVolume,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_Sound(mut choice: ::core::ffi::c_int) {
    M_SetupNextMenu(&raw mut SoundDef);
}
#[no_mangle]
pub unsafe extern "C" fn M_SfxVol(mut choice: ::core::ffi::c_int) {
    match choice {
        0 => {
            if sfxVolume != 0 {
                sfxVolume -= 1;
            }
        }
        1 => {
            if sfxVolume < 15 as ::core::ffi::c_int {
                sfxVolume += 1;
            }
        }
        _ => {}
    }
    S_SetSfxVolume(sfxVolume * 8 as ::core::ffi::c_int);
}
#[no_mangle]
pub unsafe extern "C" fn M_MusicVol(mut choice: ::core::ffi::c_int) {
    match choice {
        0 => {
            if musicVolume != 0 {
                musicVolume -= 1;
            }
        }
        1 => {
            if musicVolume < 15 as ::core::ffi::c_int {
                musicVolume += 1;
            }
        }
        _ => {}
    }
    S_SetMusicVolume(musicVolume * 8 as ::core::ffi::c_int);
}
#[no_mangle]
pub unsafe extern "C" fn M_DrawMainMenu() {
    V_DrawPatchDirect(
        94 as ::core::ffi::c_int,
        2 as ::core::ffi::c_int,
        W_CacheLumpName(
            b"M_DOOM\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_DrawNewGame() {
    V_DrawPatchDirect(
        96 as ::core::ffi::c_int,
        14 as ::core::ffi::c_int,
        W_CacheLumpName(
            b"M_NEWG\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
    V_DrawPatchDirect(
        54 as ::core::ffi::c_int,
        38 as ::core::ffi::c_int,
        W_CacheLumpName(
            b"M_SKILL\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_NewGame(mut choice: ::core::ffi::c_int) {
    if netgame != 0 && demoplayback == 0 {
        M_StartMessage(
            b"you can't start a new game\nwhile in a network game.\n\npress a key.\0".as_ptr()
                as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            NULL,
            false_0 as boolean,
        );
        return;
    }
    if gamemode as ::core::ffi::c_uint == commercial as ::core::ffi::c_int as ::core::ffi::c_uint
        || gameversion as ::core::ffi::c_uint
            == exe_chex as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        M_SetupNextMenu(&raw mut NewDef);
    } else {
        M_SetupNextMenu(&raw mut EpiDef);
    };
}
#[no_mangle]
pub static mut epi: ::core::ffi::c_int = 0;
#[no_mangle]
pub unsafe extern "C" fn M_DrawEpisode() {
    V_DrawPatchDirect(
        54 as ::core::ffi::c_int,
        38 as ::core::ffi::c_int,
        W_CacheLumpName(
            b"M_EPISOD\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_VerifyNightmare(mut key: ::core::ffi::c_int) {
    if key != key_menu_confirm {
        return;
    }
    G_DeferedInitNew(
        nightmare as ::core::ffi::c_int as skill_t,
        epi + 1 as ::core::ffi::c_int,
        1 as ::core::ffi::c_int,
    );
    M_ClearMenus();
}
#[no_mangle]
pub unsafe extern "C" fn M_ChooseSkill(mut choice: ::core::ffi::c_int) {
    if choice == nightmare as ::core::ffi::c_int {
        M_StartMessage(
            b"are you sure? this skill level\nisn't even remotely fair.\n\npress y or n.\0".as_ptr()
                as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            ::core::mem::transmute::<
                Option<unsafe extern "C" fn(::core::ffi::c_int) -> ()>,
                *mut ::core::ffi::c_void,
            >(Some(
                M_VerifyNightmare as unsafe extern "C" fn(::core::ffi::c_int) -> (),
            )),
            true_0 as boolean,
        );
        return;
    }
    G_DeferedInitNew(
        choice as skill_t,
        epi + 1 as ::core::ffi::c_int,
        1 as ::core::ffi::c_int,
    );
    M_ClearMenus();
}
#[no_mangle]
pub unsafe extern "C" fn M_Episode(mut choice: ::core::ffi::c_int) {
    if gamemode as ::core::ffi::c_uint == shareware as ::core::ffi::c_int as ::core::ffi::c_uint
        && choice != 0
    {
        M_StartMessage(
            b"this is the shareware version of doom.\n\nyou need to order the entire trilogy.\n\npress a key.\0"
                .as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            NULL,
            false_0 as boolean,
        );
        M_SetupNextMenu(&raw mut ReadDef1);
        return;
    }
    if gamemode as ::core::ffi::c_uint == registered as ::core::ffi::c_int as ::core::ffi::c_uint
        && choice > 2 as ::core::ffi::c_int
    {
        fprintf(
            stderr,
            b"M_Episode: 4th episode requires UltimateDOOM\n\0".as_ptr()
                as *const ::core::ffi::c_char,
        );
        choice = 0 as ::core::ffi::c_int;
    }
    epi = choice;
    M_SetupNextMenu(&raw mut NewDef);
}
static mut detailNames: [*mut ::core::ffi::c_char; 2] = [
    b"M_GDHIGH\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
    b"M_GDLOW\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
];
static mut msgNames: [*mut ::core::ffi::c_char; 2] = [
    b"M_MSGOFF\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
    b"M_MSGON\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
];
#[no_mangle]
pub unsafe extern "C" fn M_DrawOptions() {
    V_DrawPatchDirect(
        108 as ::core::ffi::c_int,
        15 as ::core::ffi::c_int,
        W_CacheLumpName(
            b"M_OPTTTL\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
    V_DrawPatchDirect(
        OptionsDef.x as ::core::ffi::c_int + 175 as ::core::ffi::c_int,
        OptionsDef.y as ::core::ffi::c_int + LINEHEIGHT * detail as ::core::ffi::c_int,
        W_CacheLumpName(
            detailNames[detailLevel as usize],
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
    V_DrawPatchDirect(
        OptionsDef.x as ::core::ffi::c_int + 120 as ::core::ffi::c_int,
        OptionsDef.y as ::core::ffi::c_int + LINEHEIGHT * messages as ::core::ffi::c_int,
        W_CacheLumpName(
            msgNames[showMessages as usize],
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
    M_DrawThermo(
        OptionsDef.x as ::core::ffi::c_int,
        OptionsDef.y as ::core::ffi::c_int
            + LINEHEIGHT * (mousesens as ::core::ffi::c_int + 1 as ::core::ffi::c_int),
        10 as ::core::ffi::c_int,
        mouseSensitivity,
    );
    M_DrawThermo(
        OptionsDef.x as ::core::ffi::c_int,
        OptionsDef.y as ::core::ffi::c_int
            + LINEHEIGHT * (scrnsize as ::core::ffi::c_int + 1 as ::core::ffi::c_int),
        9 as ::core::ffi::c_int,
        screenSize,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_Options(mut choice: ::core::ffi::c_int) {
    M_SetupNextMenu(&raw mut OptionsDef);
}
#[no_mangle]
pub unsafe extern "C" fn M_ChangeMessages(mut choice: ::core::ffi::c_int) {
    choice = 0 as ::core::ffi::c_int;
    showMessages = 1 as ::core::ffi::c_int - showMessages;
    if showMessages == 0 {
        players[consoleplayer as usize].message =
            b"Messages OFF\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
    } else {
        players[consoleplayer as usize].message =
            b"Messages ON\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
    }
    message_dontfuckwithme = true_0 as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn M_EndGameResponse(mut key: ::core::ffi::c_int) {
    if key != key_menu_confirm {
        return;
    }
    (*currentMenu).lastOn = itemOn;
    M_ClearMenus();
    D_StartTitle();
}
#[no_mangle]
pub unsafe extern "C" fn M_EndGame(mut choice: ::core::ffi::c_int) {
    choice = 0 as ::core::ffi::c_int;
    if usergame == 0 {
        S_StartSound(NULL, sfx_oof as ::core::ffi::c_int);
        return;
    }
    if netgame != 0 {
        M_StartMessage(
            b"you can't end a netgame!\n\npress a key.\0".as_ptr() as *const ::core::ffi::c_char
                as *mut ::core::ffi::c_char,
            NULL,
            false_0 as boolean,
        );
        return;
    }
    M_StartMessage(
        b"are you sure you want to end the game?\n\npress y or n.\0".as_ptr()
            as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        ::core::mem::transmute::<
            Option<unsafe extern "C" fn(::core::ffi::c_int) -> ()>,
            *mut ::core::ffi::c_void,
        >(Some(
            M_EndGameResponse as unsafe extern "C" fn(::core::ffi::c_int) -> (),
        )),
        true_0 as boolean,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_ReadThis(mut choice: ::core::ffi::c_int) {
    choice = 0 as ::core::ffi::c_int;
    M_SetupNextMenu(&raw mut ReadDef1);
}
#[no_mangle]
pub unsafe extern "C" fn M_ReadThis2(mut choice: ::core::ffi::c_int) {
    if gameversion as ::core::ffi::c_uint
        <= exe_doom_1_9 as ::core::ffi::c_int as ::core::ffi::c_uint
        && gamemode as ::core::ffi::c_uint
            != commercial as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        choice = 0 as ::core::ffi::c_int;
        M_SetupNextMenu(&raw mut ReadDef2);
    } else {
        M_FinishReadThis(0 as ::core::ffi::c_int);
    };
}
#[no_mangle]
pub unsafe extern "C" fn M_FinishReadThis(mut choice: ::core::ffi::c_int) {
    choice = 0 as ::core::ffi::c_int;
    M_SetupNextMenu(&raw mut MainDef);
}
#[no_mangle]
pub static mut quitsounds: [::core::ffi::c_int; 8] = [
    sfx_pldeth as ::core::ffi::c_int,
    sfx_dmpain as ::core::ffi::c_int,
    sfx_popain as ::core::ffi::c_int,
    sfx_slop as ::core::ffi::c_int,
    sfx_telept as ::core::ffi::c_int,
    sfx_posit1 as ::core::ffi::c_int,
    sfx_posit3 as ::core::ffi::c_int,
    sfx_sgtatk as ::core::ffi::c_int,
];
#[no_mangle]
pub static mut quitsounds2: [::core::ffi::c_int; 8] = [
    sfx_vilact as ::core::ffi::c_int,
    sfx_getpow as ::core::ffi::c_int,
    sfx_boscub as ::core::ffi::c_int,
    sfx_slop as ::core::ffi::c_int,
    sfx_skeswg as ::core::ffi::c_int,
    sfx_kntdth as ::core::ffi::c_int,
    sfx_bspact as ::core::ffi::c_int,
    sfx_sgtatk as ::core::ffi::c_int,
];
#[no_mangle]
pub unsafe extern "C" fn M_QuitResponse(mut key: ::core::ffi::c_int) {
    if key != key_menu_confirm {
        return;
    }
    if netgame == 0 {
        if gamemode as ::core::ffi::c_uint
            == commercial as ::core::ffi::c_int as ::core::ffi::c_uint
        {
            S_StartSound(
                NULL,
                quitsounds2
                    [(gametic >> 2 as ::core::ffi::c_int & 7 as ::core::ffi::c_int) as usize],
            );
        } else {
            S_StartSound(
                NULL,
                quitsounds[(gametic >> 2 as ::core::ffi::c_int & 7 as ::core::ffi::c_int) as usize],
            );
        }
        I_WaitVBL(105 as ::core::ffi::c_int);
    }
    I_Quit();
}
unsafe extern "C" fn M_SelectEndMessage() -> *mut ::core::ffi::c_char {
    let mut endmsg: *mut *mut ::core::ffi::c_char =
        ::core::ptr::null_mut::<*mut ::core::ffi::c_char>();
    if (if gamemission as ::core::ffi::c_uint
        == pack_chex as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        doom as ::core::ffi::c_int as ::core::ffi::c_uint
    } else {
        (if gamemission as ::core::ffi::c_uint
            == pack_hacx as ::core::ffi::c_int as ::core::ffi::c_uint
        {
            doom2 as ::core::ffi::c_int as ::core::ffi::c_uint
        } else {
            gamemission as ::core::ffi::c_uint
        })
    }) == doom as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        endmsg = &raw mut doom1_endmsg as *mut *mut ::core::ffi::c_char;
    } else {
        endmsg = &raw mut doom2_endmsg as *mut *mut ::core::ffi::c_char;
    }
    return *endmsg.offset((gametic % NUM_QUITMESSAGES) as isize);
}
#[no_mangle]
pub unsafe extern "C" fn M_QuitDOOM(mut choice: ::core::ffi::c_int) {
    snprintf(
        &raw mut endstring as *mut ::core::ffi::c_char,
        ::core::mem::size_of::<[::core::ffi::c_char; 160]>() as size_t,
        b"%s\n\n(press y to quit to dos.)\0".as_ptr() as *const ::core::ffi::c_char,
        M_SelectEndMessage(),
    );
    M_StartMessage(
        &raw mut endstring as *mut ::core::ffi::c_char,
        ::core::mem::transmute::<
            Option<unsafe extern "C" fn(::core::ffi::c_int) -> ()>,
            *mut ::core::ffi::c_void,
        >(Some(
            M_QuitResponse as unsafe extern "C" fn(::core::ffi::c_int) -> (),
        )),
        true_0 as boolean,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_ChangeSensitivity(mut choice: ::core::ffi::c_int) {
    match choice {
        0 => {
            if mouseSensitivity != 0 {
                mouseSensitivity -= 1;
            }
        }
        1 => {
            if mouseSensitivity < 9 as ::core::ffi::c_int {
                mouseSensitivity += 1;
            }
        }
        _ => {}
    };
}
#[no_mangle]
pub unsafe extern "C" fn M_ChangeDetail(mut choice: ::core::ffi::c_int) {
    choice = 0 as ::core::ffi::c_int;
    detailLevel = 1 as ::core::ffi::c_int - detailLevel;
    R_SetViewSize(screenblocks, detailLevel);
    if detailLevel == 0 {
        players[consoleplayer as usize].message =
            b"High detail\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
    } else {
        players[consoleplayer as usize].message =
            b"Low detail\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
    };
}
#[no_mangle]
pub unsafe extern "C" fn M_SizeDisplay(mut choice: ::core::ffi::c_int) {
    match choice {
        0 => {
            if screenSize > 0 as ::core::ffi::c_int {
                screenblocks -= 1;
                screenSize -= 1;
            }
        }
        1 => {
            if screenSize < 8 as ::core::ffi::c_int {
                screenblocks += 1;
                screenSize += 1;
            }
        }
        _ => {}
    }
    R_SetViewSize(screenblocks, detailLevel);
}
#[no_mangle]
pub unsafe extern "C" fn M_DrawThermo(
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut thermWidth: ::core::ffi::c_int,
    mut thermDot: ::core::ffi::c_int,
) {
    let mut xx: ::core::ffi::c_int = 0;
    let mut i: ::core::ffi::c_int = 0;
    xx = x;
    V_DrawPatchDirect(
        xx,
        y,
        W_CacheLumpName(
            b"M_THERML\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
    xx += 8 as ::core::ffi::c_int;
    i = 0 as ::core::ffi::c_int;
    while i < thermWidth {
        V_DrawPatchDirect(
            xx,
            y,
            W_CacheLumpName(
                b"M_THERMM\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
                PU_CACHE as ::core::ffi::c_int,
            ) as *mut patch_t,
        );
        xx += 8 as ::core::ffi::c_int;
        i += 1;
    }
    V_DrawPatchDirect(
        xx,
        y,
        W_CacheLumpName(
            b"M_THERMR\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
    V_DrawPatchDirect(
        x + 8 as ::core::ffi::c_int + thermDot * 8 as ::core::ffi::c_int,
        y,
        W_CacheLumpName(
            b"M_THERMO\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_DrawEmptyCell(mut menu: *mut menu_t, mut item: ::core::ffi::c_int) {
    V_DrawPatchDirect(
        (*menu).x as ::core::ffi::c_int - 10 as ::core::ffi::c_int,
        (*menu).y as ::core::ffi::c_int + item * LINEHEIGHT - 1 as ::core::ffi::c_int,
        W_CacheLumpName(
            b"M_CELL1\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_DrawSelCell(mut menu: *mut menu_t, mut item: ::core::ffi::c_int) {
    V_DrawPatchDirect(
        (*menu).x as ::core::ffi::c_int - 10 as ::core::ffi::c_int,
        (*menu).y as ::core::ffi::c_int + item * LINEHEIGHT - 1 as ::core::ffi::c_int,
        W_CacheLumpName(
            b"M_CELL2\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_StartMessage(
    mut string: *mut ::core::ffi::c_char,
    mut routine: *mut ::core::ffi::c_void,
    mut input: boolean,
) {
    messageLastMenuActive = menuactive as ::core::ffi::c_int;
    messageToPrint = 1 as ::core::ffi::c_int;
    messageString = string;
    messageRoutine = ::core::mem::transmute::<
        *mut ::core::ffi::c_void,
        Option<unsafe extern "C" fn(::core::ffi::c_int) -> ()>,
    >(routine);
    messageNeedsInput = input;
    menuactive = true_0 as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn M_StopMessage() {
    menuactive = messageLastMenuActive as boolean;
    messageToPrint = 0 as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn M_StringWidth(mut string: *mut ::core::ffi::c_char) -> ::core::ffi::c_int {
    let mut i: size_t = 0;
    let mut w: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
    let mut c: ::core::ffi::c_int = 0;
    i = 0 as size_t;
    while i < strlen(string) {
        c = toupper(*string.offset(i as isize) as ::core::ffi::c_int) - HU_FONTSTART;
        if c < 0 as ::core::ffi::c_int || c >= HU_FONTSIZE {
            w += 4 as ::core::ffi::c_int;
        } else {
            w += (*hu_font[c as usize]).width as ::core::ffi::c_int;
        }
        i = i.wrapping_add(1);
    }
    return w;
}
#[no_mangle]
pub unsafe extern "C" fn M_StringHeight(
    mut string: *mut ::core::ffi::c_char,
) -> ::core::ffi::c_int {
    let mut i: size_t = 0;
    let mut h: ::core::ffi::c_int = 0;
    let mut height: ::core::ffi::c_int =
        (*hu_font[0 as ::core::ffi::c_int as usize]).height as ::core::ffi::c_int;
    h = height;
    i = 0 as size_t;
    while i < strlen(string) {
        if *string.offset(i as isize) as ::core::ffi::c_int == '\n' as ::core::ffi::c_int {
            h += height;
        }
        i = i.wrapping_add(1);
    }
    return h;
}
#[no_mangle]
pub unsafe extern "C" fn M_WriteText(
    mut x: ::core::ffi::c_int,
    mut y: ::core::ffi::c_int,
    mut string: *mut ::core::ffi::c_char,
) {
    let mut w: ::core::ffi::c_int = 0;
    let mut ch: *mut ::core::ffi::c_char = ::core::ptr::null_mut::<::core::ffi::c_char>();
    let mut c: ::core::ffi::c_int = 0;
    let mut cx: ::core::ffi::c_int = 0;
    let mut cy: ::core::ffi::c_int = 0;
    ch = string;
    cx = x;
    cy = y;
    loop {
        let c2rust_fresh1 = ch;
        ch = ch.offset(1);
        c = *c2rust_fresh1 as ::core::ffi::c_int;
        if c == 0 {
            break;
        }
        if c == '\n' as ::core::ffi::c_int {
            cx = x;
            cy += 12 as ::core::ffi::c_int;
        } else {
            c = toupper(c) - HU_FONTSTART;
            if c < 0 as ::core::ffi::c_int || c >= HU_FONTSIZE {
                cx += 4 as ::core::ffi::c_int;
            } else {
                w = (*hu_font[c as usize]).width as ::core::ffi::c_int;
                if cx + w > SCREENWIDTH {
                    break;
                }
                V_DrawPatchDirect(cx, cy, hu_font[c as usize]);
                cx += w;
            }
        }
    }
}
unsafe extern "C" fn IsNullKey(mut key: ::core::ffi::c_int) -> boolean {
    return (key == KEY_PAUSE || key == KEY_CAPSLOCK || key == KEY_SCRLCK || key == KEY_NUMLOCK)
        as ::core::ffi::c_int as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn M_Responder(mut ev: *mut event_t) -> boolean {
    let mut ch: ::core::ffi::c_int = 0;
    let mut key: ::core::ffi::c_int = 0;
    let mut i: ::core::ffi::c_int = 0;
    static mut joywait: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
    static mut mousewait: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
    static mut mousey: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
    static mut lasty: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
    static mut mousex: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
    static mut lastx: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
    if testcontrols != 0 {
        if (*ev).type_0 as ::core::ffi::c_uint
            == ev_quit as ::core::ffi::c_int as ::core::ffi::c_uint
            || (*ev).type_0 as ::core::ffi::c_uint
                == ev_keydown as ::core::ffi::c_int as ::core::ffi::c_uint
                && ((*ev).data1 == key_menu_activate || (*ev).data1 == key_menu_quit)
        {
            I_Quit();
            return true_0 as boolean;
        }
        return false_0 as boolean;
    }
    if (*ev).type_0 as ::core::ffi::c_uint == ev_quit as ::core::ffi::c_int as ::core::ffi::c_uint {
        if menuactive != 0
            && messageToPrint != 0
            && messageRoutine
                == Some(M_QuitResponse as unsafe extern "C" fn(::core::ffi::c_int) -> ())
        {
            M_QuitResponse(key_menu_confirm);
        } else {
            S_StartSound(NULL, sfx_swtchn as ::core::ffi::c_int);
            M_QuitDOOM(0 as ::core::ffi::c_int);
        }
        return true_0 as boolean;
    }
    ch = 0 as ::core::ffi::c_int;
    key = -1 as ::core::ffi::c_int;
    if (*ev).type_0 as ::core::ffi::c_uint
        == ev_joystick as ::core::ffi::c_int as ::core::ffi::c_uint
        && joywait < I_GetTime()
    {
        if (*ev).data3 < 0 as ::core::ffi::c_int {
            key = key_menu_up;
            joywait = I_GetTime() + 5 as ::core::ffi::c_int;
        } else if (*ev).data3 > 0 as ::core::ffi::c_int {
            key = key_menu_down;
            joywait = I_GetTime() + 5 as ::core::ffi::c_int;
        }
        if (*ev).data2 < 0 as ::core::ffi::c_int {
            key = key_menu_left;
            joywait = I_GetTime() + 2 as ::core::ffi::c_int;
        } else if (*ev).data2 > 0 as ::core::ffi::c_int {
            key = key_menu_right;
            joywait = I_GetTime() + 2 as ::core::ffi::c_int;
        }
        if (*ev).data1 & 1 as ::core::ffi::c_int != 0 {
            key = key_menu_forward;
            joywait = I_GetTime() + 5 as ::core::ffi::c_int;
        }
        if (*ev).data1 & 2 as ::core::ffi::c_int != 0 {
            key = key_menu_back;
            joywait = I_GetTime() + 5 as ::core::ffi::c_int;
        }
        if joybmenu >= 0 as ::core::ffi::c_int
            && (*ev).data1 & (1 as ::core::ffi::c_int) << joybmenu != 0 as ::core::ffi::c_int
        {
            key = key_menu_activate;
            joywait = I_GetTime() + 5 as ::core::ffi::c_int;
        }
    } else if (*ev).type_0 as ::core::ffi::c_uint
        == ev_mouse as ::core::ffi::c_int as ::core::ffi::c_uint
        && mousewait < I_GetTime()
    {
        mousey += (*ev).data3;
        if mousey < lasty - 30 as ::core::ffi::c_int {
            key = key_menu_down;
            mousewait = I_GetTime() + 5 as ::core::ffi::c_int;
            lasty -= 30 as ::core::ffi::c_int;
            mousey = lasty;
        } else if mousey > lasty + 30 as ::core::ffi::c_int {
            key = key_menu_up;
            mousewait = I_GetTime() + 5 as ::core::ffi::c_int;
            lasty += 30 as ::core::ffi::c_int;
            mousey = lasty;
        }
        mousex += (*ev).data2;
        if mousex < lastx - 30 as ::core::ffi::c_int {
            key = key_menu_left;
            mousewait = I_GetTime() + 5 as ::core::ffi::c_int;
            lastx -= 30 as ::core::ffi::c_int;
            mousex = lastx;
        } else if mousex > lastx + 30 as ::core::ffi::c_int {
            key = key_menu_right;
            mousewait = I_GetTime() + 5 as ::core::ffi::c_int;
            lastx += 30 as ::core::ffi::c_int;
            mousex = lastx;
        }
        if (*ev).data1 & 1 as ::core::ffi::c_int != 0 {
            key = key_menu_forward;
            mousewait = I_GetTime() + 15 as ::core::ffi::c_int;
        }
        if (*ev).data1 & 2 as ::core::ffi::c_int != 0 {
            key = key_menu_back;
            mousewait = I_GetTime() + 15 as ::core::ffi::c_int;
        }
    } else if (*ev).type_0 as ::core::ffi::c_uint
        == ev_keydown as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        key = (*ev).data1;
        ch = (*ev).data2;
    }
    if key == -1 as ::core::ffi::c_int {
        return false_0 as boolean;
    }
    if saveStringEnter != 0 {
        match key {
            KEY_BACKSPACE => {
                if saveCharIndex > 0 as ::core::ffi::c_int {
                    saveCharIndex -= 1;
                    savegamestrings[saveSlot as usize][saveCharIndex as usize] =
                        0 as ::core::ffi::c_char;
                }
            }
            KEY_ESCAPE => {
                saveStringEnter = 0 as ::core::ffi::c_int;
                M_StringCopy(
                    &raw mut *(&raw mut savegamestrings as *mut [::core::ffi::c_char; 24])
                        .offset(saveSlot as isize) as *mut ::core::ffi::c_char,
                    &raw mut saveOldString as *mut ::core::ffi::c_char,
                    SAVESTRINGSIZE as size_t,
                );
            }
            KEY_ENTER => {
                saveStringEnter = 0 as ::core::ffi::c_int;
                if savegamestrings[saveSlot as usize][0 as ::core::ffi::c_int as usize] != 0 {
                    M_DoSave(saveSlot);
                }
            }
            _ => {
                if vanilla_keyboard_mapping != 0 {
                    ch = key;
                }
                ch = toupper(ch);
                if !(ch != ' ' as ::core::ffi::c_int
                    && (ch - HU_FONTSTART < 0 as ::core::ffi::c_int
                        || ch - HU_FONTSTART >= HU_FONTSIZE))
                {
                    if ch >= 32 as ::core::ffi::c_int
                        && ch <= 127 as ::core::ffi::c_int
                        && saveCharIndex < SAVESTRINGSIZE - 1 as ::core::ffi::c_int
                        && M_StringWidth(
                            &raw mut *(&raw mut savegamestrings as *mut [::core::ffi::c_char; 24])
                                .offset(saveSlot as isize)
                                as *mut ::core::ffi::c_char,
                        ) < (SAVESTRINGSIZE - 2 as ::core::ffi::c_int) * 8 as ::core::ffi::c_int
                    {
                        let c2rust_fresh0 = saveCharIndex;
                        saveCharIndex = saveCharIndex + 1;
                        savegamestrings[saveSlot as usize][c2rust_fresh0 as usize] =
                            ch as ::core::ffi::c_char;
                        savegamestrings[saveSlot as usize][saveCharIndex as usize] =
                            0 as ::core::ffi::c_char;
                    }
                }
            }
        }
        return true_0 as boolean;
    }
    if messageToPrint != 0 {
        if messageNeedsInput != 0 {
            if key != ' ' as ::core::ffi::c_int
                && key != KEY_ESCAPE
                && key != key_menu_confirm
                && key != key_menu_abort
            {
                return false_0 as boolean;
            }
        }
        menuactive = messageLastMenuActive as boolean;
        messageToPrint = 0 as ::core::ffi::c_int;
        if messageRoutine.is_some() {
            messageRoutine.expect("non-null function pointer")(key);
        }
        menuactive = false_0 as boolean;
        S_StartSound(NULL, sfx_swtchx as ::core::ffi::c_int);
        return true_0 as boolean;
    }
    if devparm != 0 && key == key_menu_help
        || key != 0 as ::core::ffi::c_int && key == key_menu_screenshot
    {
        G_ScreenShot();
        return true_0 as boolean;
    }
    if menuactive == 0 {
        if key == key_menu_decscreen {
            if automapactive != 0 || chat_on != 0 {
                return false_0 as boolean;
            }
            M_SizeDisplay(0 as ::core::ffi::c_int);
            S_StartSound(NULL, sfx_stnmov as ::core::ffi::c_int);
            return true_0 as boolean;
        } else if key == key_menu_incscreen {
            if automapactive != 0 || chat_on != 0 {
                return false_0 as boolean;
            }
            M_SizeDisplay(1 as ::core::ffi::c_int);
            S_StartSound(NULL, sfx_stnmov as ::core::ffi::c_int);
            return true_0 as boolean;
        } else if key == key_menu_help {
            M_StartControlPanel();
            if gamemode as ::core::ffi::c_uint
                == retail as ::core::ffi::c_int as ::core::ffi::c_uint
            {
                currentMenu = &raw mut ReadDef2;
            } else {
                currentMenu = &raw mut ReadDef1;
            }
            itemOn = 0 as ::core::ffi::c_short;
            S_StartSound(NULL, sfx_swtchn as ::core::ffi::c_int);
            return true_0 as boolean;
        } else if key == key_menu_save {
            M_StartControlPanel();
            S_StartSound(NULL, sfx_swtchn as ::core::ffi::c_int);
            M_SaveGame(0 as ::core::ffi::c_int);
            return true_0 as boolean;
        } else if key == key_menu_load {
            M_StartControlPanel();
            S_StartSound(NULL, sfx_swtchn as ::core::ffi::c_int);
            M_LoadGame(0 as ::core::ffi::c_int);
            return true_0 as boolean;
        } else if key == key_menu_volume {
            M_StartControlPanel();
            currentMenu = &raw mut SoundDef;
            itemOn = sfx_vol as ::core::ffi::c_int as ::core::ffi::c_short;
            S_StartSound(NULL, sfx_swtchn as ::core::ffi::c_int);
            return true_0 as boolean;
        } else if key == key_menu_detail {
            M_ChangeDetail(0 as ::core::ffi::c_int);
            S_StartSound(NULL, sfx_swtchn as ::core::ffi::c_int);
            return true_0 as boolean;
        } else if key == key_menu_qsave {
            S_StartSound(NULL, sfx_swtchn as ::core::ffi::c_int);
            M_QuickSave();
            return true_0 as boolean;
        } else if key == key_menu_endgame {
            S_StartSound(NULL, sfx_swtchn as ::core::ffi::c_int);
            M_EndGame(0 as ::core::ffi::c_int);
            return true_0 as boolean;
        } else if key == key_menu_messages {
            M_ChangeMessages(0 as ::core::ffi::c_int);
            S_StartSound(NULL, sfx_swtchn as ::core::ffi::c_int);
            return true_0 as boolean;
        } else if key == key_menu_qload {
            S_StartSound(NULL, sfx_swtchn as ::core::ffi::c_int);
            M_QuickLoad();
            return true_0 as boolean;
        } else if key == key_menu_quit {
            S_StartSound(NULL, sfx_swtchn as ::core::ffi::c_int);
            M_QuitDOOM(0 as ::core::ffi::c_int);
            return true_0 as boolean;
        } else if key == key_menu_gamma {
            usegamma += 1;
            if usegamma > 4 as ::core::ffi::c_int {
                usegamma = 0 as ::core::ffi::c_int;
            }
            players[consoleplayer as usize].message =
                &raw mut *(&raw mut gammamsg as *mut [::core::ffi::c_char; 26])
                    .offset(usegamma as isize) as *mut ::core::ffi::c_char;
            I_SetPalette(W_CacheLumpName(
                b"PLAYPAL\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
                PU_CACHE as ::core::ffi::c_int,
            ) as *mut byte);
            return true_0 as boolean;
        }
    }
    if menuactive == 0 {
        if key == key_menu_activate {
            M_StartControlPanel();
            S_StartSound(NULL, sfx_swtchn as ::core::ffi::c_int);
            return true_0 as boolean;
        }
        return false_0 as boolean;
    }
    if key == key_menu_down {
        loop {
            if itemOn as ::core::ffi::c_int + 1 as ::core::ffi::c_int
                > (*currentMenu).numitems as ::core::ffi::c_int - 1 as ::core::ffi::c_int
            {
                itemOn = 0 as ::core::ffi::c_short;
            } else {
                itemOn += 1;
            }
            S_StartSound(NULL, sfx_pstop as ::core::ffi::c_int);
            if !((*(*currentMenu).menuitems.offset(itemOn as isize)).status as ::core::ffi::c_int
                == -1 as ::core::ffi::c_int)
            {
                break;
            }
        }
        return true_0 as boolean;
    } else if key == key_menu_up {
        loop {
            if itemOn == 0 {
                itemOn = ((*currentMenu).numitems as ::core::ffi::c_int - 1 as ::core::ffi::c_int)
                    as ::core::ffi::c_short;
            } else {
                itemOn -= 1;
            }
            S_StartSound(NULL, sfx_pstop as ::core::ffi::c_int);
            if !((*(*currentMenu).menuitems.offset(itemOn as isize)).status as ::core::ffi::c_int
                == -1 as ::core::ffi::c_int)
            {
                break;
            }
        }
        return true_0 as boolean;
    } else if key == key_menu_left {
        if (*(*currentMenu).menuitems.offset(itemOn as isize))
            .routine
            .is_some()
            && (*(*currentMenu).menuitems.offset(itemOn as isize)).status as ::core::ffi::c_int
                == 2 as ::core::ffi::c_int
        {
            S_StartSound(NULL, sfx_stnmov as ::core::ffi::c_int);
            (*(*currentMenu).menuitems.offset(itemOn as isize))
                .routine
                .expect("non-null function pointer")(0 as ::core::ffi::c_int);
        }
        return true_0 as boolean;
    } else if key == key_menu_right {
        if (*(*currentMenu).menuitems.offset(itemOn as isize))
            .routine
            .is_some()
            && (*(*currentMenu).menuitems.offset(itemOn as isize)).status as ::core::ffi::c_int
                == 2 as ::core::ffi::c_int
        {
            S_StartSound(NULL, sfx_stnmov as ::core::ffi::c_int);
            (*(*currentMenu).menuitems.offset(itemOn as isize))
                .routine
                .expect("non-null function pointer")(1 as ::core::ffi::c_int);
        }
        return true_0 as boolean;
    } else if key == key_menu_forward {
        if (*(*currentMenu).menuitems.offset(itemOn as isize))
            .routine
            .is_some()
            && (*(*currentMenu).menuitems.offset(itemOn as isize)).status as ::core::ffi::c_int != 0
        {
            (*currentMenu).lastOn = itemOn;
            if (*(*currentMenu).menuitems.offset(itemOn as isize)).status as ::core::ffi::c_int
                == 2 as ::core::ffi::c_int
            {
                (*(*currentMenu).menuitems.offset(itemOn as isize))
                    .routine
                    .expect("non-null function pointer")(1 as ::core::ffi::c_int);
                S_StartSound(NULL, sfx_stnmov as ::core::ffi::c_int);
            } else {
                (*(*currentMenu).menuitems.offset(itemOn as isize))
                    .routine
                    .expect("non-null function pointer")(
                    itemOn as ::core::ffi::c_int
                );
                S_StartSound(NULL, sfx_pistol as ::core::ffi::c_int);
            }
        }
        return true_0 as boolean;
    } else if key == key_menu_activate {
        (*currentMenu).lastOn = itemOn;
        M_ClearMenus();
        S_StartSound(NULL, sfx_swtchx as ::core::ffi::c_int);
        return true_0 as boolean;
    } else if key == key_menu_back {
        (*currentMenu).lastOn = itemOn;
        if !(*currentMenu).prevMenu.is_null() {
            currentMenu = (*currentMenu).prevMenu as *mut menu_t;
            itemOn = (*currentMenu).lastOn;
            S_StartSound(NULL, sfx_swtchn as ::core::ffi::c_int);
        }
        return true_0 as boolean;
    } else if ch != 0 as ::core::ffi::c_int || IsNullKey(key) != 0 {
        i = itemOn as ::core::ffi::c_int + 1 as ::core::ffi::c_int;
        while i < (*currentMenu).numitems as ::core::ffi::c_int {
            if (*(*currentMenu).menuitems.offset(i as isize)).alphaKey as ::core::ffi::c_int == ch {
                itemOn = i as ::core::ffi::c_short;
                S_StartSound(NULL, sfx_pstop as ::core::ffi::c_int);
                return true_0 as boolean;
            }
            i += 1;
        }
        i = 0 as ::core::ffi::c_int;
        while i <= itemOn as ::core::ffi::c_int {
            if (*(*currentMenu).menuitems.offset(i as isize)).alphaKey as ::core::ffi::c_int == ch {
                itemOn = i as ::core::ffi::c_short;
                S_StartSound(NULL, sfx_pstop as ::core::ffi::c_int);
                return true_0 as boolean;
            }
            i += 1;
        }
    }
    return false_0 as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn M_StartControlPanel() {
    if menuactive != 0 {
        return;
    }
    menuactive = 1 as boolean;
    currentMenu = &raw mut MainDef;
    itemOn = (*currentMenu).lastOn;
}
#[no_mangle]
pub unsafe extern "C" fn M_Drawer() {
    static mut x: ::core::ffi::c_short = 0;
    static mut y: ::core::ffi::c_short = 0;
    let mut i: ::core::ffi::c_uint = 0;
    let mut max: ::core::ffi::c_uint = 0;
    let mut string: [::core::ffi::c_char; 80] = [0; 80];
    let mut name: *mut ::core::ffi::c_char = ::core::ptr::null_mut::<::core::ffi::c_char>();
    let mut start: ::core::ffi::c_int = 0;
    inhelpscreens = false_0 as boolean;
    if messageToPrint != 0 {
        start = 0 as ::core::ffi::c_int;
        y = (SCREENHEIGHT / 2 as ::core::ffi::c_int
            - M_StringHeight(messageString) / 2 as ::core::ffi::c_int)
            as ::core::ffi::c_short;
        while *messageString.offset(start as isize) as ::core::ffi::c_int
            != '\0' as ::core::ffi::c_int
        {
            let mut foundnewline: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
            i = 0 as ::core::ffi::c_uint;
            while (i as size_t) < strlen(messageString.offset(start as isize)) {
                if *messageString.offset((start as ::core::ffi::c_uint).wrapping_add(i) as isize)
                    as ::core::ffi::c_int
                    == '\n' as ::core::ffi::c_int
                {
                    M_StringCopy(
                        &raw mut string as *mut ::core::ffi::c_char,
                        messageString.offset(start as isize),
                        ::core::mem::size_of::<[::core::ffi::c_char; 80]>() as size_t,
                    );
                    if (i as usize) < ::core::mem::size_of::<[::core::ffi::c_char; 80]>() as usize {
                        string[i as usize] = '\0' as ::core::ffi::c_char;
                    }
                    foundnewline = 1 as ::core::ffi::c_int;
                    start = (start as ::core::ffi::c_uint)
                        .wrapping_add(i.wrapping_add(1 as ::core::ffi::c_uint))
                        as ::core::ffi::c_int;
                    break;
                } else {
                    i = i.wrapping_add(1);
                }
            }
            if foundnewline == 0 {
                M_StringCopy(
                    &raw mut string as *mut ::core::ffi::c_char,
                    messageString.offset(start as isize),
                    ::core::mem::size_of::<[::core::ffi::c_char; 80]>() as size_t,
                );
                start = (start as size_t)
                    .wrapping_add(strlen(&raw mut string as *mut ::core::ffi::c_char))
                    as ::core::ffi::c_int;
            }
            x = (SCREENWIDTH / 2 as ::core::ffi::c_int
                - M_StringWidth(&raw mut string as *mut ::core::ffi::c_char)
                    / 2 as ::core::ffi::c_int) as ::core::ffi::c_short;
            M_WriteText(
                x as ::core::ffi::c_int,
                y as ::core::ffi::c_int,
                &raw mut string as *mut ::core::ffi::c_char,
            );
            y = (y as ::core::ffi::c_int
                + (*hu_font[0 as ::core::ffi::c_int as usize]).height as ::core::ffi::c_int)
                as ::core::ffi::c_short;
        }
        return;
    }
    if menuactive == 0 {
        return;
    }
    if (*currentMenu).routine.is_some() {
        ::core::mem::transmute::<_, fn()>(
            (*currentMenu).routine.expect("non-null function pointer"),
        )();
    }
    x = (*currentMenu).x;
    y = (*currentMenu).y;
    max = (*currentMenu).numitems as ::core::ffi::c_uint;
    i = 0 as ::core::ffi::c_uint;
    while i < max {
        name = &raw mut (*(*currentMenu).menuitems.offset(i as isize)).name
            as *mut ::core::ffi::c_char;
        if *name.offset(0 as ::core::ffi::c_int as isize) != 0 {
            V_DrawPatchDirect(
                x as ::core::ffi::c_int,
                y as ::core::ffi::c_int,
                W_CacheLumpName(name, PU_CACHE as ::core::ffi::c_int) as *mut patch_t,
            );
        }
        y = (y as ::core::ffi::c_int + LINEHEIGHT) as ::core::ffi::c_short;
        i = i.wrapping_add(1);
    }
    V_DrawPatchDirect(
        x as ::core::ffi::c_int + SKULLXOFF,
        (*currentMenu).y as ::core::ffi::c_int - 5 as ::core::ffi::c_int
            + itemOn as ::core::ffi::c_int * LINEHEIGHT,
        W_CacheLumpName(
            skullName[whichSkull as usize],
            PU_CACHE as ::core::ffi::c_int,
        ) as *mut patch_t,
    );
}
#[no_mangle]
pub unsafe extern "C" fn M_ClearMenus() {
    menuactive = 0 as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn M_SetupNextMenu(mut menudef: *mut menu_t) {
    currentMenu = menudef;
    itemOn = (*currentMenu).lastOn;
}
#[no_mangle]
pub unsafe extern "C" fn M_Ticker() {
    skullAnimCounter -= 1;
    if skullAnimCounter as ::core::ffi::c_int <= 0 as ::core::ffi::c_int {
        whichSkull =
            (whichSkull as ::core::ffi::c_int ^ 1 as ::core::ffi::c_int) as ::core::ffi::c_short;
        skullAnimCounter = 8 as ::core::ffi::c_short;
    }
}
#[no_mangle]
pub unsafe extern "C" fn M_Init() {
    currentMenu = &raw mut MainDef;
    menuactive = 0 as boolean;
    itemOn = (*currentMenu).lastOn;
    whichSkull = 0 as ::core::ffi::c_short;
    skullAnimCounter = 10 as ::core::ffi::c_short;
    screenSize = screenblocks - 3 as ::core::ffi::c_int;
    messageToPrint = 0 as ::core::ffi::c_int;
    messageString = ::core::ptr::null_mut::<::core::ffi::c_char>();
    messageLastMenuActive = menuactive as ::core::ffi::c_int;
    quickSaveSlot = -1 as ::core::ffi::c_int;
    match gamemode as ::core::ffi::c_uint {
        2 => {
            MainMenu[readthis as ::core::ffi::c_int as usize] =
                MainMenu[quitdoom as ::core::ffi::c_int as usize];
            MainDef.numitems -= 1;
            MainDef.y =
                (MainDef.y as ::core::ffi::c_int + 8 as ::core::ffi::c_int) as ::core::ffi::c_short;
            NewDef.prevMenu = &raw mut MainDef as *mut menu_s;
        }
        0 => {}
        1 | 3 | _ => {}
    }
    if (gameversion as ::core::ffi::c_uint)
        < exe_ultimate as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        EpiDef.numitems -= 1;
    }
}
