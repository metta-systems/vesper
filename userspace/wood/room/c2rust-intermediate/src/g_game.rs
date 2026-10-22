extern "C" {
    pub type _IO_wide_data;
    pub type _IO_codecvt;
    pub type _IO_marker;
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
    fn strlen(__s: *const ::core::ffi::c_char) -> size_t;
    fn atoi(__nptr: *const ::core::ffi::c_char) -> ::core::ffi::c_int;
    fn abs(__x: ::core::ffi::c_int) -> ::core::ffi::c_int;
    fn remove(__filename: *const ::core::ffi::c_char) -> ::core::ffi::c_int;
    fn rename(
        __old: *const ::core::ffi::c_char,
        __new: *const ::core::ffi::c_char,
    ) -> ::core::ffi::c_int;
    fn fclose(__stream: *mut FILE) -> ::core::ffi::c_int;
    fn fopen(
        __filename: *const ::core::ffi::c_char,
        __modes: *const ::core::ffi::c_char,
    ) -> *mut FILE;
    fn printf(__format: *const ::core::ffi::c_char, ...) -> ::core::ffi::c_int;
    fn snprintf(
        __s: *mut ::core::ffi::c_char,
        __maxlen: size_t,
        __format: *const ::core::ffi::c_char,
        ...
    ) -> ::core::ffi::c_int;
    fn ftell(__stream: *mut FILE) -> ::core::ffi::c_long;
    fn I_GetTime() -> ::core::ffi::c_int;
    static mut singletics: boolean;
    static mut gametic: ::core::ffi::c_int;
    static mut ticdup: ::core::ffi::c_int;
    static finesine: [fixed_t; 10240];
    static mut finecosine: *const fixed_t;
    static finetangent: [fixed_t; 4096];
    static mut states: [state_t; 967];
    static mut mobjinfo: [mobjinfo_t; 137];
    static mut nomonsters: boolean;
    static mut respawnparm: boolean;
    static mut fastparm: boolean;
    static mut gamemode: GameMode_t;
    static mut gamemission: GameMission_t;
    static mut gameversion: GameVersion_t;
    static mut automapactive: boolean;
    static mut leveltime: ::core::ffi::c_int;
    static mut deathmatchstarts: [mapthing_t; 10];
    static mut deathmatch_p: *mut mapthing_t;
    static mut playerstarts: [mapthing_t; 4];
    static mut wipegamestate: gamestate_t;
    static mut mouseSensitivity: ::core::ffi::c_int;
    static mut skyflatnum: ::core::ffi::c_int;
    static mut rndindex: ::core::ffi::c_int;
    static mut netcmds: *mut ticcmd_t;
    fn P_SpawnPlayer(mthing: *mut mapthing_t);
    static mut setsizeneeded: boolean;
    fn R_ExecuteSetViewSize();
    fn Z_Malloc(
        size: ::core::ffi::c_int,
        tag: ::core::ffi::c_int,
        ptr: *mut ::core::ffi::c_void,
    ) -> *mut ::core::ffi::c_void;
    fn Z_Free(ptr: *mut ::core::ffi::c_void);
    fn Z_CheckHeap();
    static mut myargv: *mut *mut ::core::ffi::c_char;
    fn M_CheckParm(check: *mut ::core::ffi::c_char) -> ::core::ffi::c_int;
    fn M_CheckParmWithArgs(
        check: *mut ::core::ffi::c_char,
        num_args: ::core::ffi::c_int,
    ) -> ::core::ffi::c_int;
    static mut key_right: ::core::ffi::c_int;
    static mut key_left: ::core::ffi::c_int;
    static mut key_up: ::core::ffi::c_int;
    static mut key_down: ::core::ffi::c_int;
    static mut key_strafeleft: ::core::ffi::c_int;
    static mut key_straferight: ::core::ffi::c_int;
    static mut key_fire: ::core::ffi::c_int;
    static mut key_use: ::core::ffi::c_int;
    static mut key_strafe: ::core::ffi::c_int;
    static mut key_speed: ::core::ffi::c_int;
    static mut key_pause: ::core::ffi::c_int;
    static mut key_weapon1: ::core::ffi::c_int;
    static mut key_weapon2: ::core::ffi::c_int;
    static mut key_weapon3: ::core::ffi::c_int;
    static mut key_weapon4: ::core::ffi::c_int;
    static mut key_weapon5: ::core::ffi::c_int;
    static mut key_weapon6: ::core::ffi::c_int;
    static mut key_weapon7: ::core::ffi::c_int;
    static mut key_weapon8: ::core::ffi::c_int;
    static mut key_demo_quit: ::core::ffi::c_int;
    static mut key_spy: ::core::ffi::c_int;
    static mut key_prevweapon: ::core::ffi::c_int;
    static mut key_nextweapon: ::core::ffi::c_int;
    static mut mousebfire: ::core::ffi::c_int;
    static mut mousebstrafe: ::core::ffi::c_int;
    static mut mousebforward: ::core::ffi::c_int;
    static mut mousebstrafeleft: ::core::ffi::c_int;
    static mut mousebstraferight: ::core::ffi::c_int;
    static mut mousebbackward: ::core::ffi::c_int;
    static mut mousebuse: ::core::ffi::c_int;
    static mut mousebprevweapon: ::core::ffi::c_int;
    static mut mousebnextweapon: ::core::ffi::c_int;
    static mut joybfire: ::core::ffi::c_int;
    static mut joybstrafe: ::core::ffi::c_int;
    static mut joybuse: ::core::ffi::c_int;
    static mut joybspeed: ::core::ffi::c_int;
    static mut joybstrafeleft: ::core::ffi::c_int;
    static mut joybstraferight: ::core::ffi::c_int;
    static mut joybprevweapon: ::core::ffi::c_int;
    static mut joybnextweapon: ::core::ffi::c_int;
    static mut dclick_use: ::core::ffi::c_int;
    fn M_WriteFile(
        name: *mut ::core::ffi::c_char,
        source: *mut ::core::ffi::c_void,
        length: ::core::ffi::c_int,
    ) -> boolean;
    fn M_TempFile(s: *mut ::core::ffi::c_char) -> *mut ::core::ffi::c_char;
    fn M_StringCopy(
        dest: *mut ::core::ffi::c_char,
        src: *const ::core::ffi::c_char,
        dest_size: size_t,
    ) -> boolean;
    fn M_snprintf_clamp(
        buf: *mut ::core::ffi::c_char,
        len: size_t,
        result: ::core::ffi::c_int,
    ) -> ::core::ffi::c_int;
    fn M_StartControlPanel();
    fn P_Random() -> ::core::ffi::c_int;
    fn M_ClearRandom();
    fn I_Quit();
    fn I_ErrorV(msg: *const ::core::ffi::c_char);
    fn P_SetupLevel(
        episode: ::core::ffi::c_int,
        map: ::core::ffi::c_int,
        playermask: ::core::ffi::c_int,
        skill: skill_t,
    );
    fn P_TempSaveGameFile() -> *mut ::core::ffi::c_char;
    fn P_SaveGameFile(slot: ::core::ffi::c_int) -> *mut ::core::ffi::c_char;
    fn P_ReadSaveGameHeader() -> boolean;
    fn P_WriteSaveGameHeader(description: *mut ::core::ffi::c_char);
    fn P_ReadSaveGameEOF() -> boolean;
    fn P_WriteSaveGameEOF();
    fn P_ArchivePlayers();
    fn P_UnArchivePlayers();
    fn P_ArchiveWorld();
    fn P_UnArchiveWorld();
    fn P_ArchiveThinkers();
    fn P_UnArchiveThinkers();
    fn P_ArchiveSpecials();
    fn P_UnArchiveSpecials();
    static mut save_stream: *mut FILE;
    static mut savegame_error: boolean;
    fn P_Ticker();
    fn D_PageTicker();
    fn D_AdvanceDemo();
    fn WI_Ticker();
    fn WI_Start(wbstartstruct: *mut wbstartstruct_t);
    fn WI_End();
    fn HU_Responder(ev: *mut event_t) -> boolean;
    fn HU_Ticker();
    fn HU_dequeueChatChar() -> ::core::ffi::c_char;
    fn ST_Responder(ev: *mut event_t) -> boolean;
    fn ST_Ticker();
    fn AM_Responder(ev: *mut event_t) -> boolean;
    fn AM_Ticker();
    fn AM_Stop();
    fn StatCopy(stats: *mut wbstartstruct_t);
    fn V_ScreenShot(format: *mut ::core::ffi::c_char);
    fn W_CheckNumForName(name: *mut ::core::ffi::c_char) -> ::core::ffi::c_int;
    fn W_CacheLumpName(
        name: *mut ::core::ffi::c_char,
        tag: ::core::ffi::c_int,
    ) -> *mut ::core::ffi::c_void;
    fn W_ReleaseLumpName(name: *mut ::core::ffi::c_char);
    fn R_FlatNumForName(name: *mut ::core::ffi::c_char) -> ::core::ffi::c_int;
    fn R_TextureNumForName(name: *mut ::core::ffi::c_char) -> ::core::ffi::c_int;
    fn R_PointInSubsector(x: fixed_t, y: fixed_t) -> *mut subsector_t;
    fn R_FillBackScreen();
    fn P_SpawnMobj(x: fixed_t, y: fixed_t, z: fixed_t, type_0: mobjtype_t) -> *mut mobj_t;
    fn P_RemoveMobj(th: *mut mobj_t);
    fn P_CheckPosition(thing: *mut mobj_t, x: fixed_t, y: fixed_t) -> boolean;
    static mut maxammo: [::core::ffi::c_int; 4];
    fn S_StartSound(origin: *mut ::core::ffi::c_void, sound_id: ::core::ffi::c_int);
    fn S_PauseSound();
    fn S_ResumeSound();
    static mut skytexture: ::core::ffi::c_int;
    fn F_Responder(ev: *mut event_t) -> boolean;
    fn F_Ticker();
    fn F_StartFinale();
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
pub type gameaction_t = ::core::ffi::c_uint;
pub const ga_screenshot: gameaction_t = 9;
pub const ga_worlddone: gameaction_t = 8;
pub const ga_victory: gameaction_t = 7;
pub const ga_completed: gameaction_t = 6;
pub const ga_playdemo: gameaction_t = 5;
pub const ga_savegame: gameaction_t = 4;
pub const ga_loadgame: gameaction_t = 3;
pub const ga_newgame: gameaction_t = 2;
pub const ga_loadlevel: gameaction_t = 1;
pub const ga_nothing: gameaction_t = 0;
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
pub type C2Rust_Unnamed = ::core::ffi::c_uint;
pub const am_noammo: C2Rust_Unnamed = 5;
pub const NUMAMMO: C2Rust_Unnamed = 4;
pub const am_misl: C2Rust_Unnamed = 3;
pub const am_cell: C2Rust_Unnamed = 2;
pub const am_shell: C2Rust_Unnamed = 1;
pub const am_clip: C2Rust_Unnamed = 0;
pub type C2Rust_Unnamed_0 = ::core::ffi::c_uint;
pub const NUMPOWERS: C2Rust_Unnamed_0 = 6;
pub const pw_infrared: C2Rust_Unnamed_0 = 5;
pub const pw_allmap: C2Rust_Unnamed_0 = 4;
pub const pw_ironfeet: C2Rust_Unnamed_0 = 3;
pub const pw_invisibility: C2Rust_Unnamed_0 = 2;
pub const pw_strength: C2Rust_Unnamed_0 = 1;
pub const pw_invulnerability: C2Rust_Unnamed_0 = 0;
#[derive(Copy, Clone)]
#[repr(C, packed)]
pub struct mapthing_t {
    pub x: ::core::ffi::c_short,
    pub y: ::core::ffi::c_short,
    pub angle: ::core::ffi::c_short,
    pub type_0: ::core::ffi::c_short,
    pub options: ::core::ffi::c_short,
}
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
pub struct pspdef_t {
    pub state: *mut state_t,
    pub tics: ::core::ffi::c_int,
    pub sx: fixed_t,
    pub sy: fixed_t,
}
pub type C2Rust_Unnamed_1 = ::core::ffi::c_uint;
pub const MF_TRANSSHIFT: C2Rust_Unnamed_1 = 26;
pub const MF_TRANSLATION: C2Rust_Unnamed_1 = 201326592;
pub const MF_NOTDMATCH: C2Rust_Unnamed_1 = 33554432;
pub const MF_SKULLFLY: C2Rust_Unnamed_1 = 16777216;
pub const MF_COUNTITEM: C2Rust_Unnamed_1 = 8388608;
pub const MF_COUNTKILL: C2Rust_Unnamed_1 = 4194304;
pub const MF_INFLOAT: C2Rust_Unnamed_1 = 2097152;
pub const MF_CORPSE: C2Rust_Unnamed_1 = 1048576;
pub const MF_NOBLOOD: C2Rust_Unnamed_1 = 524288;
pub const MF_SHADOW: C2Rust_Unnamed_1 = 262144;
pub const MF_DROPPED: C2Rust_Unnamed_1 = 131072;
pub const MF_MISSILE: C2Rust_Unnamed_1 = 65536;
pub const MF_TELEPORT: C2Rust_Unnamed_1 = 32768;
pub const MF_FLOAT: C2Rust_Unnamed_1 = 16384;
pub const MF_SLIDE: C2Rust_Unnamed_1 = 8192;
pub const MF_NOCLIP: C2Rust_Unnamed_1 = 4096;
pub const MF_PICKUP: C2Rust_Unnamed_1 = 2048;
pub const MF_DROPOFF: C2Rust_Unnamed_1 = 1024;
pub const MF_NOGRAVITY: C2Rust_Unnamed_1 = 512;
pub const MF_SPAWNCEILING: C2Rust_Unnamed_1 = 256;
pub const MF_JUSTATTACKED: C2Rust_Unnamed_1 = 128;
pub const MF_JUSTHIT: C2Rust_Unnamed_1 = 64;
pub const MF_AMBUSH: C2Rust_Unnamed_1 = 32;
pub const MF_NOBLOCKMAP: C2Rust_Unnamed_1 = 16;
pub const MF_NOSECTOR: C2Rust_Unnamed_1 = 8;
pub const MF_SHOOTABLE: C2Rust_Unnamed_1 = 4;
pub const MF_SOLID: C2Rust_Unnamed_1 = 2;
pub const MF_SPECIAL: C2Rust_Unnamed_1 = 1;
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
#[derive(Copy, Clone)]
#[repr(C)]
pub struct wbplayerstruct_t {
    pub in_0: boolean,
    pub skills: ::core::ffi::c_int,
    pub sitems: ::core::ffi::c_int,
    pub ssecret: ::core::ffi::c_int,
    pub stime: ::core::ffi::c_int,
    pub frags: [::core::ffi::c_int; 4],
    pub score: ::core::ffi::c_int,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct wbstartstruct_t {
    pub epsd: ::core::ffi::c_int,
    pub didsecret: boolean,
    pub last: ::core::ffi::c_int,
    pub next: ::core::ffi::c_int,
    pub maxkills: ::core::ffi::c_int,
    pub maxitems: ::core::ffi::c_int,
    pub maxsecret: ::core::ffi::c_int,
    pub maxfrags: ::core::ffi::c_int,
    pub partime: ::core::ffi::c_int,
    pub pnum: ::core::ffi::c_int,
    pub plyr: [wbplayerstruct_t; 4],
}
pub type C2Rust_Unnamed_2 = ::core::ffi::c_uint;
pub const PU_NUM_TAGS: C2Rust_Unnamed_2 = 9;
pub const PU_CACHE: C2Rust_Unnamed_2 = 8;
pub const PU_PURGELEVEL: C2Rust_Unnamed_2 = 7;
pub const PU_LEVSPEC: C2Rust_Unnamed_2 = 6;
pub const PU_LEVEL: C2Rust_Unnamed_2 = 5;
pub const PU_FREE: C2Rust_Unnamed_2 = 4;
pub const PU_MUSIC: C2Rust_Unnamed_2 = 3;
pub const PU_SOUND: C2Rust_Unnamed_2 = 2;
pub const PU_STATIC: C2Rust_Unnamed_2 = 1;
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
pub type C2Rust_Unnamed_3 = ::core::ffi::c_uint;
pub const BTS_SAVESHIFT: C2Rust_Unnamed_3 = 2;
pub const BTS_SAVEMASK: C2Rust_Unnamed_3 = 28;
pub const BTS_SAVEGAME: C2Rust_Unnamed_3 = 2;
pub const BTS_PAUSE: C2Rust_Unnamed_3 = 1;
pub const BT_WEAPONSHIFT: C2Rust_Unnamed_3 = 3;
pub const BT_WEAPONMASK: C2Rust_Unnamed_3 = 56;
pub const BT_CHANGE: C2Rust_Unnamed_3 = 4;
pub const BT_SPECIALMASK: C2Rust_Unnamed_3 = 3;
pub const BT_SPECIAL: C2Rust_Unnamed_3 = 128;
pub const BT_USE: C2Rust_Unnamed_3 = 2;
pub const BT_ATTACK: C2Rust_Unnamed_3 = 1;
pub type subsector_t = subsector_s;
pub type C2Rust_Unnamed_4 = ::core::ffi::c_uint;
pub const NUMSFX: C2Rust_Unnamed_4 = 109;
pub const sfx_radio: C2Rust_Unnamed_4 = 108;
pub const sfx_skeatk: C2Rust_Unnamed_4 = 107;
pub const sfx_skesit: C2Rust_Unnamed_4 = 106;
pub const sfx_skeact: C2Rust_Unnamed_4 = 105;
pub const sfx_keendt: C2Rust_Unnamed_4 = 104;
pub const sfx_keenpn: C2Rust_Unnamed_4 = 103;
pub const sfx_ssdth: C2Rust_Unnamed_4 = 102;
pub const sfx_sssit: C2Rust_Unnamed_4 = 101;
pub const sfx_mandth: C2Rust_Unnamed_4 = 100;
pub const sfx_manatk: C2Rust_Unnamed_4 = 99;
pub const sfx_bosdth: C2Rust_Unnamed_4 = 98;
pub const sfx_bospn: C2Rust_Unnamed_4 = 97;
pub const sfx_bossit: C2Rust_Unnamed_4 = 96;
pub const sfx_boscub: C2Rust_Unnamed_4 = 95;
pub const sfx_bospit: C2Rust_Unnamed_4 = 94;
pub const sfx_getpow: C2Rust_Unnamed_4 = 93;
pub const sfx_flamst: C2Rust_Unnamed_4 = 92;
pub const sfx_flame: C2Rust_Unnamed_4 = 91;
pub const sfx_itmbk: C2Rust_Unnamed_4 = 90;
pub const sfx_bdcls: C2Rust_Unnamed_4 = 89;
pub const sfx_bdopn: C2Rust_Unnamed_4 = 88;
pub const sfx_tink: C2Rust_Unnamed_4 = 87;
pub const sfx_chgun: C2Rust_Unnamed_4 = 86;
pub const sfx_metal: C2Rust_Unnamed_4 = 85;
pub const sfx_hoof: C2Rust_Unnamed_4 = 84;
pub const sfx_punch: C2Rust_Unnamed_4 = 83;
pub const sfx_barexp: C2Rust_Unnamed_4 = 82;
pub const sfx_noway: C2Rust_Unnamed_4 = 81;
pub const sfx_vilact: C2Rust_Unnamed_4 = 80;
pub const sfx_bspwlk: C2Rust_Unnamed_4 = 79;
pub const sfx_bspact: C2Rust_Unnamed_4 = 78;
pub const sfx_dmact: C2Rust_Unnamed_4 = 77;
pub const sfx_bgact: C2Rust_Unnamed_4 = 76;
pub const sfx_posact: C2Rust_Unnamed_4 = 75;
pub const sfx_skedth: C2Rust_Unnamed_4 = 74;
pub const sfx_pedth: C2Rust_Unnamed_4 = 73;
pub const sfx_kntdth: C2Rust_Unnamed_4 = 72;
pub const sfx_vildth: C2Rust_Unnamed_4 = 71;
pub const sfx_bspdth: C2Rust_Unnamed_4 = 70;
pub const sfx_spidth: C2Rust_Unnamed_4 = 69;
pub const sfx_cybdth: C2Rust_Unnamed_4 = 68;
pub const sfx_brsdth: C2Rust_Unnamed_4 = 67;
pub const sfx_skldth: C2Rust_Unnamed_4 = 66;
pub const sfx_cacdth: C2Rust_Unnamed_4 = 65;
pub const sfx_sgtdth: C2Rust_Unnamed_4 = 64;
pub const sfx_bgdth2: C2Rust_Unnamed_4 = 63;
pub const sfx_bgdth1: C2Rust_Unnamed_4 = 62;
pub const sfx_podth3: C2Rust_Unnamed_4 = 61;
pub const sfx_podth2: C2Rust_Unnamed_4 = 60;
pub const sfx_podth1: C2Rust_Unnamed_4 = 59;
pub const sfx_pdiehi: C2Rust_Unnamed_4 = 58;
pub const sfx_pldeth: C2Rust_Unnamed_4 = 57;
pub const sfx_skeswg: C2Rust_Unnamed_4 = 56;
pub const sfx_claw: C2Rust_Unnamed_4 = 55;
pub const sfx_vilatk: C2Rust_Unnamed_4 = 54;
pub const sfx_skepch: C2Rust_Unnamed_4 = 53;
pub const sfx_sgtatk: C2Rust_Unnamed_4 = 52;
pub const sfx_sklatk: C2Rust_Unnamed_4 = 51;
pub const sfx_pesit: C2Rust_Unnamed_4 = 50;
pub const sfx_mansit: C2Rust_Unnamed_4 = 49;
pub const sfx_vilsit: C2Rust_Unnamed_4 = 48;
pub const sfx_kntsit: C2Rust_Unnamed_4 = 47;
pub const sfx_bspsit: C2Rust_Unnamed_4 = 46;
pub const sfx_spisit: C2Rust_Unnamed_4 = 45;
pub const sfx_cybsit: C2Rust_Unnamed_4 = 44;
pub const sfx_brssit: C2Rust_Unnamed_4 = 43;
pub const sfx_cacsit: C2Rust_Unnamed_4 = 42;
pub const sfx_sgtsit: C2Rust_Unnamed_4 = 41;
pub const sfx_bgsit2: C2Rust_Unnamed_4 = 40;
pub const sfx_bgsit1: C2Rust_Unnamed_4 = 39;
pub const sfx_posit3: C2Rust_Unnamed_4 = 38;
pub const sfx_posit2: C2Rust_Unnamed_4 = 37;
pub const sfx_posit1: C2Rust_Unnamed_4 = 36;
pub const sfx_telept: C2Rust_Unnamed_4 = 35;
pub const sfx_oof: C2Rust_Unnamed_4 = 34;
pub const sfx_wpnup: C2Rust_Unnamed_4 = 33;
pub const sfx_itemup: C2Rust_Unnamed_4 = 32;
pub const sfx_slop: C2Rust_Unnamed_4 = 31;
pub const sfx_pepain: C2Rust_Unnamed_4 = 30;
pub const sfx_mnpain: C2Rust_Unnamed_4 = 29;
pub const sfx_vipain: C2Rust_Unnamed_4 = 28;
pub const sfx_popain: C2Rust_Unnamed_4 = 27;
pub const sfx_dmpain: C2Rust_Unnamed_4 = 26;
pub const sfx_plpain: C2Rust_Unnamed_4 = 25;
pub const sfx_swtchx: C2Rust_Unnamed_4 = 24;
pub const sfx_swtchn: C2Rust_Unnamed_4 = 23;
pub const sfx_stnmov: C2Rust_Unnamed_4 = 22;
pub const sfx_dorcls: C2Rust_Unnamed_4 = 21;
pub const sfx_doropn: C2Rust_Unnamed_4 = 20;
pub const sfx_pstop: C2Rust_Unnamed_4 = 19;
pub const sfx_pstart: C2Rust_Unnamed_4 = 18;
pub const sfx_firxpl: C2Rust_Unnamed_4 = 17;
pub const sfx_firsht: C2Rust_Unnamed_4 = 16;
pub const sfx_rxplod: C2Rust_Unnamed_4 = 15;
pub const sfx_rlaunc: C2Rust_Unnamed_4 = 14;
pub const sfx_sawhit: C2Rust_Unnamed_4 = 13;
pub const sfx_sawful: C2Rust_Unnamed_4 = 12;
pub const sfx_sawidl: C2Rust_Unnamed_4 = 11;
pub const sfx_sawup: C2Rust_Unnamed_4 = 10;
pub const sfx_bfg: C2Rust_Unnamed_4 = 9;
pub const sfx_plasma: C2Rust_Unnamed_4 = 8;
pub const sfx_dbload: C2Rust_Unnamed_4 = 7;
pub const sfx_dbcls: C2Rust_Unnamed_4 = 6;
pub const sfx_dbopn: C2Rust_Unnamed_4 = 5;
pub const sfx_dshtgn: C2Rust_Unnamed_4 = 4;
pub const sfx_sgcock: C2Rust_Unnamed_4 = 3;
pub const sfx_shotgn: C2Rust_Unnamed_4 = 2;
pub const sfx_pistol: C2Rust_Unnamed_4 = 1;
pub const sfx_None: C2Rust_Unnamed_4 = 0;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct C2Rust_Unnamed_5 {
    pub weapon: weapontype_t,
    pub weapon_num: weapontype_t,
}
pub const DEH_DEFAULT_INITIAL_HEALTH: ::core::ffi::c_int = 100 as ::core::ffi::c_int;
pub const DEH_DEFAULT_INITIAL_BULLETS: ::core::ffi::c_int = 50 as ::core::ffi::c_int;
pub const deh_initial_health: ::core::ffi::c_int = DEH_DEFAULT_INITIAL_HEALTH;
pub const deh_initial_bullets: ::core::ffi::c_int = DEH_DEFAULT_INITIAL_BULLETS;
pub const TICRATE: ::core::ffi::c_int = 35 as ::core::ffi::c_int;
pub const DOOM_191_VERSION: ::core::ffi::c_int = 111 as ::core::ffi::c_int;
pub const MAXPLAYERS: ::core::ffi::c_int = 4 as ::core::ffi::c_int;
pub const BACKUPTICS: ::core::ffi::c_int = 128 as ::core::ffi::c_int;
pub const FRACBITS: ::core::ffi::c_int = 16 as ::core::ffi::c_int;
pub const FRACUNIT: ::core::ffi::c_int = (1 as ::core::ffi::c_int) << FRACBITS;
pub const ANGLETOFINESHIFT: ::core::ffi::c_int = 19 as ::core::ffi::c_int;
pub const ANG45: ::core::ffi::c_int = 0x20000000 as ::core::ffi::c_int;
pub const SAVEGAMESIZE: ::core::ffi::c_int = 0x2c000 as ::core::ffi::c_int;
#[no_mangle]
pub static mut oldgamestate: gamestate_t = GS_LEVEL;
#[no_mangle]
pub static mut gameaction: gameaction_t = ga_nothing;
#[no_mangle]
pub static mut gamestate: gamestate_t = GS_LEVEL;
#[no_mangle]
pub static mut gameskill: skill_t = sk_baby;
#[no_mangle]
pub static mut respawnmonsters: boolean = 0;
#[no_mangle]
pub static mut gameepisode: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut gamemap: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut timelimit: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut paused: boolean = 0;
#[no_mangle]
pub static mut sendpause: boolean = 0;
#[no_mangle]
pub static mut sendsave: boolean = 0;
#[no_mangle]
pub static mut usergame: boolean = 0;
#[no_mangle]
pub static mut timingdemo: boolean = 0;
#[no_mangle]
pub static mut nodrawers: boolean = 0;
#[no_mangle]
pub static mut starttime: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut viewactive: boolean = 0;
#[no_mangle]
pub static mut deathmatch: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut netgame: boolean = 0;
#[no_mangle]
pub static mut playeringame: [boolean; 4] = [0; 4];
#[no_mangle]
pub static mut players: [player_t; 4] = [player_t {
    mo: ::core::ptr::null_mut::<mobj_t>(),
    playerstate: PST_LIVE,
    cmd: ticcmd_t {
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
    },
    viewz: 0,
    viewheight: 0,
    deltaviewheight: 0,
    bob: 0,
    health: 0,
    armorpoints: 0,
    armortype: 0,
    powers: [0; 6],
    cards: [0; 6],
    backpack: 0,
    frags: [0; 4],
    readyweapon: wp_fist,
    pendingweapon: wp_fist,
    weaponowned: [0; 9],
    ammo: [0; 4],
    maxammo: [0; 4],
    attackdown: 0,
    usedown: 0,
    cheats: 0,
    refire: 0,
    killcount: 0,
    itemcount: 0,
    secretcount: 0,
    message: ::core::ptr::null_mut::<::core::ffi::c_char>(),
    damagecount: 0,
    bonuscount: 0,
    attacker: ::core::ptr::null_mut::<mobj_t>(),
    extralight: 0,
    fixedcolormap: 0,
    colormap: 0,
    psprites: [pspdef_t {
        state: ::core::ptr::null_mut::<state_t>(),
        tics: 0,
        sx: 0,
        sy: 0,
    }; 2],
    didsecret: 0,
}; 4];
#[no_mangle]
pub static mut turbodetected: [boolean; 4] = [0; 4];
#[no_mangle]
pub static mut consoleplayer: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut displayplayer: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut levelstarttic: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut totalkills: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut totalitems: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut totalsecret: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut demoname: *mut ::core::ffi::c_char = ::core::ptr::null_mut::<::core::ffi::c_char>();
#[no_mangle]
pub static mut demorecording: boolean = 0;
#[no_mangle]
pub static mut longtics: boolean = 0;
#[no_mangle]
pub static mut lowres_turn: boolean = 0;
#[no_mangle]
pub static mut demoplayback: boolean = 0;
#[no_mangle]
pub static mut netdemo: boolean = 0;
#[no_mangle]
pub static mut demobuffer: *mut byte = ::core::ptr::null_mut::<byte>();
#[no_mangle]
pub static mut demo_p: *mut byte = ::core::ptr::null_mut::<byte>();
#[no_mangle]
pub static mut demoend: *mut byte = ::core::ptr::null_mut::<byte>();
#[no_mangle]
pub static mut singledemo: boolean = 0;
#[no_mangle]
pub static mut precache: boolean = true_0 as boolean;
#[no_mangle]
pub static mut testcontrols: boolean = false_0 as boolean;
#[no_mangle]
pub static mut testcontrols_mousespeed: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut wminfo: wbstartstruct_t = wbstartstruct_t {
    epsd: 0,
    didsecret: 0,
    last: 0,
    next: 0,
    maxkills: 0,
    maxitems: 0,
    maxsecret: 0,
    maxfrags: 0,
    partime: 0,
    pnum: 0,
    plyr: [wbplayerstruct_t {
        in_0: 0,
        skills: 0,
        sitems: 0,
        ssecret: 0,
        stime: 0,
        frags: [0; 4],
        score: 0,
    }; 4],
};
#[no_mangle]
pub static mut consistancy: [[byte; 128]; 4] = [[0; 128]; 4];
pub const TURBOTHRESHOLD: ::core::ffi::c_int = 0x32 as ::core::ffi::c_int;
#[no_mangle]
pub static mut forwardmove: [fixed_t; 2] = [0x19 as ::core::ffi::c_int, 0x32 as ::core::ffi::c_int];
#[no_mangle]
pub static mut sidemove: [fixed_t; 2] = [0x18 as ::core::ffi::c_int, 0x28 as ::core::ffi::c_int];
#[no_mangle]
pub static mut angleturn: [fixed_t; 3] = [
    640 as ::core::ffi::c_int,
    1280 as ::core::ffi::c_int,
    320 as ::core::ffi::c_int,
];
static mut weapon_keys: [*mut ::core::ffi::c_int; 8] = unsafe {
    [
        &raw const key_weapon1 as *mut ::core::ffi::c_int,
        &raw const key_weapon2 as *mut ::core::ffi::c_int,
        &raw const key_weapon3 as *mut ::core::ffi::c_int,
        &raw const key_weapon4 as *mut ::core::ffi::c_int,
        &raw const key_weapon5 as *mut ::core::ffi::c_int,
        &raw const key_weapon6 as *mut ::core::ffi::c_int,
        &raw const key_weapon7 as *mut ::core::ffi::c_int,
        &raw const key_weapon8 as *mut ::core::ffi::c_int,
    ]
};
static mut next_weapon: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
static mut weapon_order_table: [C2Rust_Unnamed_5; 9] = [
    C2Rust_Unnamed_5 {
        weapon: wp_fist,
        weapon_num: wp_fist,
    },
    C2Rust_Unnamed_5 {
        weapon: wp_chainsaw,
        weapon_num: wp_fist,
    },
    C2Rust_Unnamed_5 {
        weapon: wp_pistol,
        weapon_num: wp_pistol,
    },
    C2Rust_Unnamed_5 {
        weapon: wp_shotgun,
        weapon_num: wp_shotgun,
    },
    C2Rust_Unnamed_5 {
        weapon: wp_supershotgun,
        weapon_num: wp_shotgun,
    },
    C2Rust_Unnamed_5 {
        weapon: wp_chaingun,
        weapon_num: wp_chaingun,
    },
    C2Rust_Unnamed_5 {
        weapon: wp_missile,
        weapon_num: wp_missile,
    },
    C2Rust_Unnamed_5 {
        weapon: wp_plasma,
        weapon_num: wp_plasma,
    },
    C2Rust_Unnamed_5 {
        weapon: wp_bfg,
        weapon_num: wp_bfg,
    },
];
pub const SLOWTURNTICS: ::core::ffi::c_int = 6 as ::core::ffi::c_int;
pub const NUMKEYS: ::core::ffi::c_int = 256 as ::core::ffi::c_int;
pub const MAX_JOY_BUTTONS: ::core::ffi::c_int = 20 as ::core::ffi::c_int;
static mut gamekeydown: [boolean; 256] = [0; 256];
static mut turnheld: ::core::ffi::c_int = 0;
static mut mousearray: [boolean; 9] = [0; 9];
static mut mousebuttons: *mut boolean = ::core::ptr::null_mut::<boolean>();
#[no_mangle]
pub static mut mousex: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut mousey: ::core::ffi::c_int = 0;
static mut dclicktime: ::core::ffi::c_int = 0;
static mut dclickstate: boolean = 0;
static mut dclicks: ::core::ffi::c_int = 0;
static mut dclicktime2: ::core::ffi::c_int = 0;
static mut dclickstate2: boolean = 0;
static mut dclicks2: ::core::ffi::c_int = 0;
static mut joyxmove: ::core::ffi::c_int = 0;
static mut joyymove: ::core::ffi::c_int = 0;
static mut joystrafemove: ::core::ffi::c_int = 0;
static mut joyarray: [boolean; 21] = [0; 21];
static mut joybuttons: *mut boolean = ::core::ptr::null_mut::<boolean>();
static mut savegameslot: ::core::ffi::c_int = 0;
static mut savedescription: [::core::ffi::c_char; 32] = [0; 32];
pub const BODYQUESIZE: ::core::ffi::c_int = 32 as ::core::ffi::c_int;
#[no_mangle]
pub static mut bodyque: [*mut mobj_t; 32] = [::core::ptr::null_mut::<mobj_t>(); 32];
#[no_mangle]
pub static mut bodyqueslot: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut vanilla_savegame_limit: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
#[no_mangle]
pub static mut vanilla_demo_limit: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
#[no_mangle]
pub unsafe extern "C" fn G_CmdChecksum(mut cmd: *mut ticcmd_t) -> ::core::ffi::c_int {
    let mut i: size_t = 0;
    let mut sum: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
    i = 0 as size_t;
    while i
        < (::core::mem::size_of::<ticcmd_t>() as usize)
            .wrapping_div(4 as usize)
            .wrapping_sub(1 as usize)
    {
        sum += *(cmd as *mut ::core::ffi::c_int).offset(i as isize);
        i = i.wrapping_add(1);
    }
    return sum;
}
unsafe extern "C" fn WeaponSelectable(mut weapon: weapontype_t) -> boolean {
    if weapon as ::core::ffi::c_uint == wp_supershotgun as ::core::ffi::c_int as ::core::ffi::c_uint
        && (if gamemission as ::core::ffi::c_uint
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
        return false_0 as boolean;
    }
    if (weapon as ::core::ffi::c_uint == wp_plasma as ::core::ffi::c_int as ::core::ffi::c_uint
        || weapon as ::core::ffi::c_uint == wp_bfg as ::core::ffi::c_int as ::core::ffi::c_uint)
        && gamemission as ::core::ffi::c_uint == doom as ::core::ffi::c_int as ::core::ffi::c_uint
        && gamemode as ::core::ffi::c_uint == shareware as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        return false_0 as boolean;
    }
    if players[consoleplayer as usize].weaponowned[weapon as usize] == 0 {
        return false_0 as boolean;
    }
    if weapon as ::core::ffi::c_uint == wp_fist as ::core::ffi::c_int as ::core::ffi::c_uint
        && players[consoleplayer as usize].weaponowned[wp_chainsaw as ::core::ffi::c_int as usize]
            != 0
        && players[consoleplayer as usize].powers[pw_strength as ::core::ffi::c_int as usize] == 0
    {
        return false_0 as boolean;
    }
    return true_0 as boolean;
}
unsafe extern "C" fn G_NextWeapon(mut direction: ::core::ffi::c_int) -> ::core::ffi::c_int {
    let mut weapon: weapontype_t = wp_fist;
    let mut start_i: ::core::ffi::c_int = 0;
    let mut i: ::core::ffi::c_int = 0;
    if players[consoleplayer as usize].pendingweapon as ::core::ffi::c_uint
        == wp_nochange as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        weapon = players[consoleplayer as usize].readyweapon;
    } else {
        weapon = players[consoleplayer as usize].pendingweapon;
    }
    i = 0 as ::core::ffi::c_int;
    while (i as usize)
        < (::core::mem::size_of::<[C2Rust_Unnamed_5; 9]>() as usize)
            .wrapping_div(::core::mem::size_of::<C2Rust_Unnamed_5>() as usize)
    {
        if weapon_order_table[i as usize].weapon as ::core::ffi::c_uint
            == weapon as ::core::ffi::c_uint
        {
            break;
        }
        i += 1;
    }
    start_i = i;
    loop {
        i += direction;
        i = (i as usize)
            .wrapping_add(
                (::core::mem::size_of::<[C2Rust_Unnamed_5; 9]>() as usize)
                    .wrapping_div(::core::mem::size_of::<C2Rust_Unnamed_5>() as usize),
            )
            .wrapping_rem(
                (::core::mem::size_of::<[C2Rust_Unnamed_5; 9]>() as usize)
                    .wrapping_div(::core::mem::size_of::<C2Rust_Unnamed_5>() as usize),
            ) as ::core::ffi::c_int;
        if !(i != start_i && WeaponSelectable(weapon_order_table[i as usize].weapon) == 0) {
            break;
        }
    }
    return weapon_order_table[i as usize].weapon_num as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn G_BuildTiccmd(mut cmd: *mut ticcmd_t, mut maketic: ::core::ffi::c_int) {
    let mut i: ::core::ffi::c_int = 0;
    let mut strafe: boolean = 0;
    let mut bstrafe: boolean = 0;
    let mut speed: ::core::ffi::c_int = 0;
    let mut tspeed: ::core::ffi::c_int = 0;
    let mut forward: ::core::ffi::c_int = 0;
    let mut side: ::core::ffi::c_int = 0;
    memset(
        cmd as *mut ::core::ffi::c_void,
        0 as ::core::ffi::c_int,
        ::core::mem::size_of::<ticcmd_t>() as size_t,
    );
    (*cmd).consistancy = consistancy[consoleplayer as usize][(maketic % BACKUPTICS) as usize];
    strafe = (gamekeydown[key_strafe as usize] != 0
        || *mousebuttons.offset(mousebstrafe as isize) != 0
        || *joybuttons.offset(joybstrafe as isize) != 0) as ::core::ffi::c_int
        as boolean;
    speed = (key_speed >= NUMKEYS
        || joybspeed >= MAX_JOY_BUTTONS
        || gamekeydown[key_speed as usize] != 0
        || *joybuttons.offset(joybspeed as isize) != 0) as ::core::ffi::c_int;
    side = 0 as ::core::ffi::c_int;
    forward = side;
    if joyxmove < 0 as ::core::ffi::c_int
        || joyxmove > 0 as ::core::ffi::c_int
        || gamekeydown[key_right as usize] != 0
        || gamekeydown[key_left as usize] != 0
    {
        turnheld += ticdup;
    } else {
        turnheld = 0 as ::core::ffi::c_int;
    }
    if turnheld < SLOWTURNTICS {
        tspeed = 2 as ::core::ffi::c_int;
    } else {
        tspeed = speed;
    }
    if strafe != 0 {
        if gamekeydown[key_right as usize] != 0 {
            side += sidemove[speed as usize] as ::core::ffi::c_int;
        }
        if gamekeydown[key_left as usize] != 0 {
            side -= sidemove[speed as usize] as ::core::ffi::c_int;
        }
        if joyxmove > 0 as ::core::ffi::c_int {
            side += sidemove[speed as usize] as ::core::ffi::c_int;
        }
        if joyxmove < 0 as ::core::ffi::c_int {
            side -= sidemove[speed as usize] as ::core::ffi::c_int;
        }
    } else {
        if gamekeydown[key_right as usize] != 0 {
            (*cmd).angleturn = ((*cmd).angleturn as ::core::ffi::c_int
                - angleturn[tspeed as usize] as ::core::ffi::c_int)
                as ::core::ffi::c_short;
        }
        if gamekeydown[key_left as usize] != 0 {
            (*cmd).angleturn = ((*cmd).angleturn as ::core::ffi::c_int
                + angleturn[tspeed as usize] as ::core::ffi::c_int)
                as ::core::ffi::c_short;
        }
        if joyxmove > 0 as ::core::ffi::c_int {
            (*cmd).angleturn = ((*cmd).angleturn as ::core::ffi::c_int
                - angleturn[tspeed as usize] as ::core::ffi::c_int)
                as ::core::ffi::c_short;
        }
        if joyxmove < 0 as ::core::ffi::c_int {
            (*cmd).angleturn = ((*cmd).angleturn as ::core::ffi::c_int
                + angleturn[tspeed as usize] as ::core::ffi::c_int)
                as ::core::ffi::c_short;
        }
    }
    if gamekeydown[key_up as usize] != 0 {
        forward += forwardmove[speed as usize] as ::core::ffi::c_int;
    }
    if gamekeydown[key_down as usize] != 0 {
        forward -= forwardmove[speed as usize] as ::core::ffi::c_int;
    }
    if joyymove < 0 as ::core::ffi::c_int {
        forward += forwardmove[speed as usize] as ::core::ffi::c_int;
    }
    if joyymove > 0 as ::core::ffi::c_int {
        forward -= forwardmove[speed as usize] as ::core::ffi::c_int;
    }
    if gamekeydown[key_strafeleft as usize] != 0
        || *joybuttons.offset(joybstrafeleft as isize) != 0
        || *mousebuttons.offset(mousebstrafeleft as isize) != 0
        || joystrafemove < 0 as ::core::ffi::c_int
    {
        side -= sidemove[speed as usize] as ::core::ffi::c_int;
    }
    if gamekeydown[key_straferight as usize] != 0
        || *joybuttons.offset(joybstraferight as isize) != 0
        || *mousebuttons.offset(mousebstraferight as isize) != 0
        || joystrafemove > 0 as ::core::ffi::c_int
    {
        side += sidemove[speed as usize] as ::core::ffi::c_int;
    }
    (*cmd).chatchar = HU_dequeueChatChar() as byte;
    if gamekeydown[key_fire as usize] != 0
        || *mousebuttons.offset(mousebfire as isize) != 0
        || *joybuttons.offset(joybfire as isize) != 0
    {
        (*cmd).buttons =
            ((*cmd).buttons as ::core::ffi::c_int | BT_ATTACK as ::core::ffi::c_int) as byte;
    }
    if gamekeydown[key_use as usize] != 0
        || *joybuttons.offset(joybuse as isize) != 0
        || *mousebuttons.offset(mousebuse as isize) != 0
    {
        (*cmd).buttons =
            ((*cmd).buttons as ::core::ffi::c_int | BT_USE as ::core::ffi::c_int) as byte;
        dclicks = 0 as ::core::ffi::c_int;
    }
    if gamestate as ::core::ffi::c_uint == GS_LEVEL as ::core::ffi::c_int as ::core::ffi::c_uint
        && next_weapon != 0 as ::core::ffi::c_int
    {
        i = G_NextWeapon(next_weapon);
        (*cmd).buttons =
            ((*cmd).buttons as ::core::ffi::c_int | BT_CHANGE as ::core::ffi::c_int) as byte;
        (*cmd).buttons = ((*cmd).buttons as ::core::ffi::c_int
            | i << BT_WEAPONSHIFT as ::core::ffi::c_int) as byte;
    } else {
        i = 0 as ::core::ffi::c_int;
        while (i as usize)
            < (::core::mem::size_of::<[*mut ::core::ffi::c_int; 8]>() as usize)
                .wrapping_div(::core::mem::size_of::<*mut ::core::ffi::c_int>() as usize)
        {
            let mut key: ::core::ffi::c_int = *weapon_keys[i as usize];
            if gamekeydown[key as usize] != 0 {
                (*cmd).buttons = ((*cmd).buttons as ::core::ffi::c_int
                    | BT_CHANGE as ::core::ffi::c_int) as byte;
                (*cmd).buttons = ((*cmd).buttons as ::core::ffi::c_int
                    | i << BT_WEAPONSHIFT as ::core::ffi::c_int)
                    as byte;
                break;
            } else {
                i += 1;
            }
        }
    }
    next_weapon = 0 as ::core::ffi::c_int;
    if *mousebuttons.offset(mousebforward as isize) != 0 {
        forward += forwardmove[speed as usize] as ::core::ffi::c_int;
    }
    if *mousebuttons.offset(mousebbackward as isize) != 0 {
        forward -= forwardmove[speed as usize] as ::core::ffi::c_int;
    }
    if dclick_use != 0 {
        if *mousebuttons.offset(mousebforward as isize) != dclickstate
            && dclicktime > 1 as ::core::ffi::c_int
        {
            dclickstate = *mousebuttons.offset(mousebforward as isize);
            if dclickstate != 0 {
                dclicks += 1;
            }
            if dclicks == 2 as ::core::ffi::c_int {
                (*cmd).buttons =
                    ((*cmd).buttons as ::core::ffi::c_int | BT_USE as ::core::ffi::c_int) as byte;
                dclicks = 0 as ::core::ffi::c_int;
            } else {
                dclicktime = 0 as ::core::ffi::c_int;
            }
        } else {
            dclicktime += ticdup;
            if dclicktime > 20 as ::core::ffi::c_int {
                dclicks = 0 as ::core::ffi::c_int;
                dclickstate = 0 as boolean;
            }
        }
        bstrafe = (*mousebuttons.offset(mousebstrafe as isize) != 0
            || *joybuttons.offset(joybstrafe as isize) != 0) as ::core::ffi::c_int
            as boolean;
        if bstrafe != dclickstate2 && dclicktime2 > 1 as ::core::ffi::c_int {
            dclickstate2 = bstrafe;
            if dclickstate2 != 0 {
                dclicks2 += 1;
            }
            if dclicks2 == 2 as ::core::ffi::c_int {
                (*cmd).buttons =
                    ((*cmd).buttons as ::core::ffi::c_int | BT_USE as ::core::ffi::c_int) as byte;
                dclicks2 = 0 as ::core::ffi::c_int;
            } else {
                dclicktime2 = 0 as ::core::ffi::c_int;
            }
        } else {
            dclicktime2 += ticdup;
            if dclicktime2 > 20 as ::core::ffi::c_int {
                dclicks2 = 0 as ::core::ffi::c_int;
                dclickstate2 = 0 as boolean;
            }
        }
    }
    forward += mousey;
    if strafe != 0 {
        side += mousex * 2 as ::core::ffi::c_int;
    } else {
        (*cmd).angleturn = ((*cmd).angleturn as ::core::ffi::c_int
            - mousex * 0x8 as ::core::ffi::c_int)
            as ::core::ffi::c_short;
    }
    if mousex == 0 as ::core::ffi::c_int {
        testcontrols_mousespeed = 0 as ::core::ffi::c_int;
    }
    mousey = 0 as ::core::ffi::c_int;
    mousex = mousey;
    if forward > forwardmove[1 as ::core::ffi::c_int as usize] {
        forward = forwardmove[1 as ::core::ffi::c_int as usize] as ::core::ffi::c_int;
    } else if forward < -forwardmove[1 as ::core::ffi::c_int as usize] {
        forward = -forwardmove[1 as ::core::ffi::c_int as usize] as ::core::ffi::c_int;
    }
    if side > forwardmove[1 as ::core::ffi::c_int as usize] {
        side = forwardmove[1 as ::core::ffi::c_int as usize] as ::core::ffi::c_int;
    } else if side < -forwardmove[1 as ::core::ffi::c_int as usize] {
        side = -forwardmove[1 as ::core::ffi::c_int as usize] as ::core::ffi::c_int;
    }
    (*cmd).forwardmove =
        ((*cmd).forwardmove as ::core::ffi::c_int + forward) as ::core::ffi::c_schar;
    (*cmd).sidemove = ((*cmd).sidemove as ::core::ffi::c_int + side) as ::core::ffi::c_schar;
    if sendpause != 0 {
        sendpause = false_0 as boolean;
        (*cmd).buttons =
            (BT_SPECIAL as ::core::ffi::c_int | BTS_PAUSE as ::core::ffi::c_int) as byte;
    }
    if sendsave != 0 {
        sendsave = false_0 as boolean;
        (*cmd).buttons = (BT_SPECIAL as ::core::ffi::c_int
            | BTS_SAVEGAME as ::core::ffi::c_int
            | savegameslot << BTS_SAVESHIFT as ::core::ffi::c_int) as byte;
    }
    if lowres_turn != 0 {
        static mut carry: ::core::ffi::c_short = 0 as ::core::ffi::c_short;
        let mut desired_angleturn: ::core::ffi::c_short = 0;
        desired_angleturn = ((*cmd).angleturn as ::core::ffi::c_int + carry as ::core::ffi::c_int)
            as ::core::ffi::c_short;
        (*cmd).angleturn = (desired_angleturn as ::core::ffi::c_int + 128 as ::core::ffi::c_int
            & 0xff00 as ::core::ffi::c_int) as ::core::ffi::c_short;
        carry = (desired_angleturn as ::core::ffi::c_int - (*cmd).angleturn as ::core::ffi::c_int)
            as ::core::ffi::c_short;
    }
}
#[no_mangle]
pub unsafe extern "C" fn G_DoLoadLevel() {
    let mut i: ::core::ffi::c_int = 0;
    skyflatnum = R_FlatNumForName(
        b"F_SKY1\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char
    );
    if gamemode as ::core::ffi::c_uint == commercial as ::core::ffi::c_int as ::core::ffi::c_uint
        && (gameversion as ::core::ffi::c_uint
            == exe_final2 as ::core::ffi::c_int as ::core::ffi::c_uint
            || gameversion as ::core::ffi::c_uint
                == exe_chex as ::core::ffi::c_int as ::core::ffi::c_uint)
    {
        let mut skytexturename: *mut ::core::ffi::c_char =
            ::core::ptr::null_mut::<::core::ffi::c_char>();
        if gamemap < 12 as ::core::ffi::c_int {
            skytexturename =
                b"SKY1\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        } else if gamemap < 21 as ::core::ffi::c_int {
            skytexturename =
                b"SKY2\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        } else {
            skytexturename =
                b"SKY3\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        }
        skytexturename = skytexturename;
        skytexture = R_TextureNumForName(skytexturename);
    }
    levelstarttic = gametic;
    if wipegamestate as ::core::ffi::c_uint == GS_LEVEL as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        wipegamestate = 4294967295 as gamestate_t;
    }
    gamestate = GS_LEVEL;
    i = 0 as ::core::ffi::c_int;
    while i < MAXPLAYERS {
        turbodetected[i as usize] = false_0 as boolean;
        if playeringame[i as usize] != 0
            && players[i as usize].playerstate as ::core::ffi::c_uint
                == PST_DEAD as ::core::ffi::c_int as ::core::ffi::c_uint
        {
            players[i as usize].playerstate = PST_REBORN;
        }
        memset(
            &raw mut (*(&raw mut players as *mut player_t).offset(i as isize)).frags
                as *mut ::core::ffi::c_int as *mut ::core::ffi::c_void,
            0 as ::core::ffi::c_int,
            ::core::mem::size_of::<[::core::ffi::c_int; 4]>() as size_t,
        );
        i += 1;
    }
    P_SetupLevel(gameepisode, gamemap, 0 as ::core::ffi::c_int, gameskill);
    displayplayer = consoleplayer;
    gameaction = ga_nothing;
    Z_CheckHeap();
    memset(
        &raw mut gamekeydown as *mut boolean as *mut ::core::ffi::c_void,
        0 as ::core::ffi::c_int,
        ::core::mem::size_of::<[boolean; 256]>() as size_t,
    );
    joystrafemove = 0 as ::core::ffi::c_int;
    joyymove = joystrafemove;
    joyxmove = joyymove;
    mousey = 0 as ::core::ffi::c_int;
    mousex = mousey;
    paused = false_0 as boolean;
    sendsave = paused;
    sendpause = sendsave;
    memset(
        &raw mut mousearray as *mut boolean as *mut ::core::ffi::c_void,
        0 as ::core::ffi::c_int,
        ::core::mem::size_of::<[boolean; 9]>() as size_t,
    );
    memset(
        &raw mut joyarray as *mut boolean as *mut ::core::ffi::c_void,
        0 as ::core::ffi::c_int,
        ::core::mem::size_of::<[boolean; 21]>() as size_t,
    );
    if testcontrols != 0 {
        players[consoleplayer as usize].message = b"Press escape to quit.\0".as_ptr()
            as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char;
    }
}
unsafe extern "C" fn SetJoyButtons(mut buttons_mask: ::core::ffi::c_uint) {
    let mut i: ::core::ffi::c_int = 0;
    i = 0 as ::core::ffi::c_int;
    while i < MAX_JOY_BUTTONS {
        let mut button_on: ::core::ffi::c_int =
            (buttons_mask & ((1 as ::core::ffi::c_int) << i) as ::core::ffi::c_uint
                != 0 as ::core::ffi::c_uint) as ::core::ffi::c_int;
        if *joybuttons.offset(i as isize) == 0 && button_on != 0 {
            if i == joybprevweapon {
                next_weapon = -1 as ::core::ffi::c_int;
            } else if i == joybnextweapon {
                next_weapon = 1 as ::core::ffi::c_int;
            }
        }
        *joybuttons.offset(i as isize) = button_on as boolean;
        i += 1;
    }
}
unsafe extern "C" fn SetMouseButtons(mut buttons_mask: ::core::ffi::c_uint) {
    let mut i: ::core::ffi::c_int = 0;
    i = 0 as ::core::ffi::c_int;
    while i < MAX_MOUSE_BUTTONS {
        let mut button_on: ::core::ffi::c_uint =
            (buttons_mask & ((1 as ::core::ffi::c_int) << i) as ::core::ffi::c_uint
                != 0 as ::core::ffi::c_uint) as ::core::ffi::c_int
                as ::core::ffi::c_uint;
        if *mousebuttons.offset(i as isize) == 0 && button_on != 0 {
            if i == mousebprevweapon {
                next_weapon = -1 as ::core::ffi::c_int;
            } else if i == mousebnextweapon {
                next_weapon = 1 as ::core::ffi::c_int;
            }
        }
        *mousebuttons.offset(i as isize) = button_on as boolean;
        i += 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn G_Responder(mut ev: *mut event_t) -> boolean {
    if gamestate as ::core::ffi::c_uint == GS_LEVEL as ::core::ffi::c_int as ::core::ffi::c_uint
        && (*ev).type_0 as ::core::ffi::c_uint
            == ev_keydown as ::core::ffi::c_int as ::core::ffi::c_uint
        && (*ev).data1 == key_spy
        && (singledemo != 0 || deathmatch == 0)
    {
        loop {
            displayplayer += 1;
            if displayplayer == MAXPLAYERS {
                displayplayer = 0 as ::core::ffi::c_int;
            }
            if !(playeringame[displayplayer as usize] == 0 && displayplayer != consoleplayer) {
                break;
            }
        }
        return true_0 as boolean;
    }
    if gameaction as ::core::ffi::c_uint == ga_nothing as ::core::ffi::c_int as ::core::ffi::c_uint
        && singledemo == 0
        && (demoplayback != 0
            || gamestate as ::core::ffi::c_uint
                == GS_DEMOSCREEN as ::core::ffi::c_int as ::core::ffi::c_uint)
    {
        if (*ev).type_0 as ::core::ffi::c_uint
            == ev_keydown as ::core::ffi::c_int as ::core::ffi::c_uint
            || (*ev).type_0 as ::core::ffi::c_uint
                == ev_mouse as ::core::ffi::c_int as ::core::ffi::c_uint
                && (*ev).data1 != 0
            || (*ev).type_0 as ::core::ffi::c_uint
                == ev_joystick as ::core::ffi::c_int as ::core::ffi::c_uint
                && (*ev).data1 != 0
        {
            M_StartControlPanel();
            return true_0 as boolean;
        }
        return false_0 as boolean;
    }
    if gamestate as ::core::ffi::c_uint == GS_LEVEL as ::core::ffi::c_int as ::core::ffi::c_uint {
        if HU_Responder(ev) != 0 {
            return true_0 as boolean;
        }
        if ST_Responder(ev) != 0 {
            return true_0 as boolean;
        }
        if AM_Responder(ev) != 0 {
            return true_0 as boolean;
        }
    }
    if gamestate as ::core::ffi::c_uint == GS_FINALE as ::core::ffi::c_int as ::core::ffi::c_uint {
        if F_Responder(ev) != 0 {
            return true_0 as boolean;
        }
    }
    if testcontrols != 0
        && (*ev).type_0 as ::core::ffi::c_uint
            == ev_mouse as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        testcontrols_mousespeed = abs((*ev).data2);
    }
    if (*ev).type_0 as ::core::ffi::c_uint
        == ev_keydown as ::core::ffi::c_int as ::core::ffi::c_uint
        && (*ev).data1 == key_prevweapon
    {
        next_weapon = -1 as ::core::ffi::c_int;
    } else if (*ev).type_0 as ::core::ffi::c_uint
        == ev_keydown as ::core::ffi::c_int as ::core::ffi::c_uint
        && (*ev).data1 == key_nextweapon
    {
        next_weapon = 1 as ::core::ffi::c_int;
    }
    match (*ev).type_0 as ::core::ffi::c_uint {
        0 => {
            if (*ev).data1 == key_pause {
                sendpause = true_0 as boolean;
            } else if (*ev).data1 < NUMKEYS {
                gamekeydown[(*ev).data1 as usize] = true_0 as boolean;
            }
            return true_0 as boolean;
        }
        1 => {
            if (*ev).data1 < NUMKEYS {
                gamekeydown[(*ev).data1 as usize] = false_0 as boolean;
            }
            return false_0 as boolean;
        }
        2 => {
            SetMouseButtons((*ev).data1 as ::core::ffi::c_uint);
            mousex = (*ev).data2 * (mouseSensitivity + 5 as ::core::ffi::c_int)
                / 10 as ::core::ffi::c_int;
            mousey = (*ev).data3 * (mouseSensitivity + 5 as ::core::ffi::c_int)
                / 10 as ::core::ffi::c_int;
            return true_0 as boolean;
        }
        3 => {
            SetJoyButtons((*ev).data1 as ::core::ffi::c_uint);
            joyxmove = (*ev).data2;
            joyymove = (*ev).data3;
            joystrafemove = (*ev).data4;
            return true_0 as boolean;
        }
        _ => {}
    }
    return false_0 as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn G_Ticker() {
    let mut i: ::core::ffi::c_int = 0;
    let mut buf: ::core::ffi::c_int = 0;
    let mut cmd: *mut ticcmd_t = ::core::ptr::null_mut::<ticcmd_t>();
    i = 0 as ::core::ffi::c_int;
    while i < MAXPLAYERS {
        if playeringame[i as usize] != 0
            && players[i as usize].playerstate as ::core::ffi::c_uint
                == PST_REBORN as ::core::ffi::c_int as ::core::ffi::c_uint
        {
            G_DoReborn(i);
        }
        i += 1;
    }
    while gameaction as ::core::ffi::c_uint
        != ga_nothing as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        match gameaction as ::core::ffi::c_uint {
            1 => {
                G_DoLoadLevel();
            }
            2 => {
                G_DoNewGame();
            }
            3 => {
                G_DoLoadGame();
            }
            4 => {
                G_DoSaveGame();
            }
            5 => {
                G_DoPlayDemo();
            }
            6 => {
                G_DoCompleted();
            }
            7 => {
                F_StartFinale();
            }
            8 => {
                G_DoWorldDone();
            }
            9 => {
                V_ScreenShot(b"DOOM%02i.%s\0".as_ptr() as *const ::core::ffi::c_char
                    as *mut ::core::ffi::c_char);
                players[consoleplayer as usize].message = b"screen shot\0".as_ptr()
                    as *const ::core::ffi::c_char
                    as *mut ::core::ffi::c_char;
                gameaction = ga_nothing;
            }
            0 | _ => {}
        }
    }
    buf = gametic / ticdup % BACKUPTICS;
    i = 0 as ::core::ffi::c_int;
    while i < MAXPLAYERS {
        if playeringame[i as usize] != 0 {
            cmd = &raw mut (*(&raw mut players as *mut player_t).offset(i as isize)).cmd;
            memcpy(
                cmd as *mut ::core::ffi::c_void,
                netcmds.offset(i as isize) as *mut ticcmd_t as *const ::core::ffi::c_void,
                ::core::mem::size_of::<ticcmd_t>() as size_t,
            );
            if demoplayback != 0 {
                G_ReadDemoTiccmd(cmd);
            }
            if demorecording != 0 {
                G_WriteDemoTiccmd(cmd);
            }
            if (*cmd).forwardmove as ::core::ffi::c_int > TURBOTHRESHOLD {
                turbodetected[i as usize] = true_0 as boolean;
            }
            if gametic & 31 as ::core::ffi::c_int == 0 as ::core::ffi::c_int
                && (gametic >> 5 as ::core::ffi::c_int) % MAXPLAYERS == i
                && turbodetected[i as usize] != 0
            {
                static mut turbomessage: [::core::ffi::c_char; 80] = [0; 80];
                extern "C" {
                    static mut player_names: [*mut ::core::ffi::c_char; 4];
                }
                M_snprintf_clamp(
                    &raw mut turbomessage as *mut ::core::ffi::c_char,
                    ::core::mem::size_of::<[::core::ffi::c_char; 80]>() as size_t,
                    snprintf(
                        &raw mut turbomessage as *mut ::core::ffi::c_char,
                        ::core::mem::size_of::<[::core::ffi::c_char; 80]>() as size_t,
                        b"%s is turbo!\0".as_ptr() as *const ::core::ffi::c_char,
                        player_names[i as usize],
                    ),
                );
                players[consoleplayer as usize].message =
                    &raw mut turbomessage as *mut ::core::ffi::c_char;
                turbodetected[i as usize] = false_0 as boolean;
            }
            if netgame != 0 && netdemo == 0 && gametic % ticdup == 0 {
                if gametic > BACKUPTICS
                    && consistancy[i as usize][buf as usize] as ::core::ffi::c_int
                        != (*cmd).consistancy as ::core::ffi::c_int
                {
                    let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
                    snprintf(
                        &raw mut _i_err_buf as *mut ::core::ffi::c_char,
                        ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
                        b"consistency failure (%i should be %i)\0".as_ptr()
                            as *const ::core::ffi::c_char,
                        (*cmd).consistancy as ::core::ffi::c_int,
                        consistancy[i as usize][buf as usize] as ::core::ffi::c_int,
                    );
                    I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
                }
                if !players[i as usize].mo.is_null() {
                    consistancy[i as usize][buf as usize] = (*players[i as usize].mo).x as byte;
                } else {
                    consistancy[i as usize][buf as usize] = rndindex as byte;
                }
            }
        }
        i += 1;
    }
    i = 0 as ::core::ffi::c_int;
    while i < MAXPLAYERS {
        if playeringame[i as usize] != 0 {
            if players[i as usize].cmd.buttons as ::core::ffi::c_int
                & BT_SPECIAL as ::core::ffi::c_int
                != 0
            {
                match players[i as usize].cmd.buttons as ::core::ffi::c_int
                    & BT_SPECIALMASK as ::core::ffi::c_int
                {
                    1 => {
                        paused ^= 1 as boolean;
                        if paused != 0 {
                            S_PauseSound();
                        } else {
                            S_ResumeSound();
                        }
                    }
                    2 => {
                        if savedescription[0 as ::core::ffi::c_int as usize] == 0 {
                            M_StringCopy(
                                &raw mut savedescription as *mut ::core::ffi::c_char,
                                b"NET GAME\0".as_ptr() as *const ::core::ffi::c_char,
                                ::core::mem::size_of::<[::core::ffi::c_char; 32]>() as size_t,
                            );
                        }
                        savegameslot = (players[i as usize].cmd.buttons as ::core::ffi::c_int
                            & BTS_SAVEMASK as ::core::ffi::c_int)
                            >> BTS_SAVESHIFT as ::core::ffi::c_int;
                        gameaction = ga_savegame;
                    }
                    _ => {}
                }
            }
        }
        i += 1;
    }
    if oldgamestate as ::core::ffi::c_uint
        == GS_INTERMISSION as ::core::ffi::c_int as ::core::ffi::c_uint
        && gamestate as ::core::ffi::c_uint
            != GS_INTERMISSION as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        WI_End();
    }
    oldgamestate = gamestate;
    match gamestate as ::core::ffi::c_uint {
        0 => {
            P_Ticker();
            ST_Ticker();
            AM_Ticker();
            HU_Ticker();
        }
        1 => {
            WI_Ticker();
        }
        2 => {
            F_Ticker();
        }
        3 => {
            D_PageTicker();
        }
        _ => {}
    };
}
#[no_mangle]
pub unsafe extern "C" fn G_InitPlayer(mut player: ::core::ffi::c_int) {
    G_PlayerReborn(player);
}
#[no_mangle]
pub unsafe extern "C" fn G_PlayerFinishLevel(mut player: ::core::ffi::c_int) {
    let mut p: *mut player_t = ::core::ptr::null_mut::<player_t>();
    p = (&raw mut players as *mut player_t).offset(player as isize) as *mut player_t;
    memset(
        &raw mut (*p).powers as *mut ::core::ffi::c_int as *mut ::core::ffi::c_void,
        0 as ::core::ffi::c_int,
        ::core::mem::size_of::<[::core::ffi::c_int; 6]>() as size_t,
    );
    memset(
        &raw mut (*p).cards as *mut boolean as *mut ::core::ffi::c_void,
        0 as ::core::ffi::c_int,
        ::core::mem::size_of::<[boolean; 6]>() as size_t,
    );
    (*(*p).mo).flags &= !(MF_SHADOW as ::core::ffi::c_int);
    (*p).extralight = 0 as ::core::ffi::c_int;
    (*p).fixedcolormap = 0 as ::core::ffi::c_int;
    (*p).damagecount = 0 as ::core::ffi::c_int;
    (*p).bonuscount = 0 as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn G_PlayerReborn(mut player: ::core::ffi::c_int) {
    let mut p: *mut player_t = ::core::ptr::null_mut::<player_t>();
    let mut i: ::core::ffi::c_int = 0;
    let mut frags: [::core::ffi::c_int; 4] = [0; 4];
    let mut killcount: ::core::ffi::c_int = 0;
    let mut itemcount: ::core::ffi::c_int = 0;
    let mut secretcount: ::core::ffi::c_int = 0;
    memcpy(
        &raw mut frags as *mut ::core::ffi::c_int as *mut ::core::ffi::c_void,
        &raw mut (*(&raw mut players as *mut player_t).offset(player as isize)).frags
            as *mut ::core::ffi::c_int as *const ::core::ffi::c_void,
        ::core::mem::size_of::<[::core::ffi::c_int; 4]>() as size_t,
    );
    killcount = players[player as usize].killcount;
    itemcount = players[player as usize].itemcount;
    secretcount = players[player as usize].secretcount;
    p = (&raw mut players as *mut player_t).offset(player as isize) as *mut player_t;
    memset(
        p as *mut ::core::ffi::c_void,
        0 as ::core::ffi::c_int,
        ::core::mem::size_of::<player_t>() as size_t,
    );
    memcpy(
        &raw mut (*(&raw mut players as *mut player_t).offset(player as isize)).frags
            as *mut ::core::ffi::c_int as *mut ::core::ffi::c_void,
        &raw mut frags as *mut ::core::ffi::c_int as *const ::core::ffi::c_void,
        ::core::mem::size_of::<[::core::ffi::c_int; 4]>() as size_t,
    );
    players[player as usize].killcount = killcount;
    players[player as usize].itemcount = itemcount;
    players[player as usize].secretcount = secretcount;
    (*p).attackdown = true_0;
    (*p).usedown = (*p).attackdown;
    (*p).playerstate = PST_LIVE;
    (*p).health = deh_initial_health;
    (*p).pendingweapon = wp_pistol;
    (*p).readyweapon = (*p).pendingweapon;
    (*p).weaponowned[wp_fist as ::core::ffi::c_int as usize] = true_0 as boolean;
    (*p).weaponowned[wp_pistol as ::core::ffi::c_int as usize] = true_0 as boolean;
    (*p).ammo[am_clip as ::core::ffi::c_int as usize] = deh_initial_bullets;
    i = 0 as ::core::ffi::c_int;
    while i < NUMAMMO as ::core::ffi::c_int {
        (*p).maxammo[i as usize] = maxammo[i as usize];
        i += 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn G_CheckSpot(
    mut playernum: ::core::ffi::c_int,
    mut mthing: *mut mapthing_t,
) -> boolean {
    let mut x: fixed_t = 0;
    let mut y: fixed_t = 0;
    let mut ss: *mut subsector_t = ::core::ptr::null_mut::<subsector_t>();
    let mut mo: *mut mobj_t = ::core::ptr::null_mut::<mobj_t>();
    let mut i: ::core::ffi::c_int = 0;
    if players[playernum as usize].mo.is_null() {
        i = 0 as ::core::ffi::c_int;
        while i < playernum {
            if (*players[i as usize].mo).x == ((*mthing).x as ::core::ffi::c_int) << FRACBITS
                && (*players[i as usize].mo).y == ((*mthing).y as ::core::ffi::c_int) << FRACBITS
            {
                return false_0 as boolean;
            }
            i += 1;
        }
        return true_0 as boolean;
    }
    x = (((*mthing).x as ::core::ffi::c_int) << FRACBITS) as fixed_t;
    y = (((*mthing).y as ::core::ffi::c_int) << FRACBITS) as fixed_t;
    if P_CheckPosition(players[playernum as usize].mo, x, y) == 0 {
        return false_0 as boolean;
    }
    if bodyqueslot >= BODYQUESIZE {
        P_RemoveMobj(bodyque[(bodyqueslot % BODYQUESIZE) as usize]);
    }
    bodyque[(bodyqueslot % BODYQUESIZE) as usize] = players[playernum as usize].mo;
    bodyqueslot += 1;
    ss = R_PointInSubsector(x, y);
    let mut xa: fixed_t = 0;
    let mut ya: fixed_t = 0;
    let mut an: ::core::ffi::c_int = 0;
    an = (ANG45 >> ANGLETOFINESHIFT)
        * ((*mthing).angle as ::core::ffi::c_int / 45 as ::core::ffi::c_int);
    match an {
        4096 => {
            xa = finetangent[2048 as ::core::ffi::c_int as usize];
            ya = finetangent[0 as ::core::ffi::c_int as usize];
        }
        5120 => {
            xa = finetangent[3072 as ::core::ffi::c_int as usize];
            ya = finetangent[1024 as ::core::ffi::c_int as usize];
        }
        6144 => {
            xa = finesine[0 as ::core::ffi::c_int as usize];
            ya = finetangent[2048 as ::core::ffi::c_int as usize];
        }
        7168 => {
            xa = finesine[1024 as ::core::ffi::c_int as usize];
            ya = finetangent[3072 as ::core::ffi::c_int as usize];
        }
        0 | 1024 | 2048 | 3072 => {
            xa = *finecosine.offset(an as isize);
            ya = finesine[an as usize];
        }
        _ => {
            let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
            snprintf(
                &raw mut _i_err_buf as *mut ::core::ffi::c_char,
                ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
                b"G_CheckSpot: unexpected angle %d\n\0".as_ptr() as *const ::core::ffi::c_char,
                an,
            );
            I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
            ya = 0 as ::core::ffi::c_int as fixed_t;
            xa = ya;
        }
    }
    mo = P_SpawnMobj(
        x + 20 as fixed_t * xa,
        y + 20 as fixed_t * ya,
        (*(*ss).sector).floorheight,
        MT_TFOG,
    );
    if players[consoleplayer as usize].viewz != 1 as ::core::ffi::c_int {
        S_StartSound(
            mo as *mut ::core::ffi::c_void,
            sfx_telept as ::core::ffi::c_int,
        );
    }
    return true_0 as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn G_DeathMatchSpawnPlayer(mut playernum: ::core::ffi::c_int) {
    let mut i: ::core::ffi::c_int = 0;
    let mut j: ::core::ffi::c_int = 0;
    let mut selections: ::core::ffi::c_int = 0;
    selections = deathmatch_p.offset_from(&raw mut deathmatchstarts as *mut mapthing_t)
        as ::core::ffi::c_long as ::core::ffi::c_int;
    if selections < 4 as ::core::ffi::c_int {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"Only %i deathmatch spots, 4 required\0".as_ptr() as *const ::core::ffi::c_char,
            selections,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    j = 0 as ::core::ffi::c_int;
    while j < 20 as ::core::ffi::c_int {
        i = P_Random() % selections;
        if G_CheckSpot(
            playernum,
            (&raw mut deathmatchstarts as *mut mapthing_t).offset(i as isize) as *mut mapthing_t,
        ) != 0
        {
            deathmatchstarts[i as usize].type_0 =
                (playernum + 1 as ::core::ffi::c_int) as ::core::ffi::c_short;
            P_SpawnPlayer(
                (&raw mut deathmatchstarts as *mut mapthing_t).offset(i as isize)
                    as *mut mapthing_t,
            );
            return;
        }
        j += 1;
    }
    P_SpawnPlayer(
        (&raw mut playerstarts as *mut mapthing_t).offset(playernum as isize) as *mut mapthing_t,
    );
}
#[no_mangle]
pub unsafe extern "C" fn G_DoReborn(mut playernum: ::core::ffi::c_int) {
    let mut i: ::core::ffi::c_int = 0;
    if netgame == 0 {
        gameaction = ga_loadlevel;
    } else {
        (*players[playernum as usize].mo).player = ::core::ptr::null_mut::<player_s>();
        if deathmatch != 0 {
            G_DeathMatchSpawnPlayer(playernum);
            return;
        }
        if G_CheckSpot(
            playernum,
            (&raw mut playerstarts as *mut mapthing_t).offset(playernum as isize)
                as *mut mapthing_t,
        ) != 0
        {
            P_SpawnPlayer(
                (&raw mut playerstarts as *mut mapthing_t).offset(playernum as isize)
                    as *mut mapthing_t,
            );
            return;
        }
        i = 0 as ::core::ffi::c_int;
        while i < MAXPLAYERS {
            if G_CheckSpot(
                playernum,
                (&raw mut playerstarts as *mut mapthing_t).offset(i as isize) as *mut mapthing_t,
            ) != 0
            {
                playerstarts[i as usize].type_0 =
                    (playernum + 1 as ::core::ffi::c_int) as ::core::ffi::c_short;
                P_SpawnPlayer(
                    (&raw mut playerstarts as *mut mapthing_t).offset(i as isize)
                        as *mut mapthing_t,
                );
                playerstarts[i as usize].type_0 =
                    (i + 1 as ::core::ffi::c_int) as ::core::ffi::c_short;
                return;
            }
            i += 1;
        }
        P_SpawnPlayer(
            (&raw mut playerstarts as *mut mapthing_t).offset(playernum as isize)
                as *mut mapthing_t,
        );
    };
}
#[no_mangle]
pub unsafe extern "C" fn G_ScreenShot() {
    gameaction = ga_screenshot;
}
#[no_mangle]
pub static mut pars: [[::core::ffi::c_int; 10]; 4] = [
    [0 as ::core::ffi::c_int; 10],
    [
        0 as ::core::ffi::c_int,
        30 as ::core::ffi::c_int,
        75 as ::core::ffi::c_int,
        120 as ::core::ffi::c_int,
        90 as ::core::ffi::c_int,
        165 as ::core::ffi::c_int,
        180 as ::core::ffi::c_int,
        180 as ::core::ffi::c_int,
        30 as ::core::ffi::c_int,
        165 as ::core::ffi::c_int,
    ],
    [
        0 as ::core::ffi::c_int,
        90 as ::core::ffi::c_int,
        90 as ::core::ffi::c_int,
        90 as ::core::ffi::c_int,
        120 as ::core::ffi::c_int,
        90 as ::core::ffi::c_int,
        360 as ::core::ffi::c_int,
        240 as ::core::ffi::c_int,
        30 as ::core::ffi::c_int,
        170 as ::core::ffi::c_int,
    ],
    [
        0 as ::core::ffi::c_int,
        90 as ::core::ffi::c_int,
        45 as ::core::ffi::c_int,
        90 as ::core::ffi::c_int,
        150 as ::core::ffi::c_int,
        90 as ::core::ffi::c_int,
        90 as ::core::ffi::c_int,
        165 as ::core::ffi::c_int,
        30 as ::core::ffi::c_int,
        135 as ::core::ffi::c_int,
    ],
];
#[no_mangle]
pub static mut cpars: [::core::ffi::c_int; 32] = [
    30 as ::core::ffi::c_int,
    90 as ::core::ffi::c_int,
    120 as ::core::ffi::c_int,
    120 as ::core::ffi::c_int,
    90 as ::core::ffi::c_int,
    150 as ::core::ffi::c_int,
    120 as ::core::ffi::c_int,
    120 as ::core::ffi::c_int,
    270 as ::core::ffi::c_int,
    90 as ::core::ffi::c_int,
    210 as ::core::ffi::c_int,
    150 as ::core::ffi::c_int,
    150 as ::core::ffi::c_int,
    150 as ::core::ffi::c_int,
    210 as ::core::ffi::c_int,
    150 as ::core::ffi::c_int,
    420 as ::core::ffi::c_int,
    150 as ::core::ffi::c_int,
    210 as ::core::ffi::c_int,
    150 as ::core::ffi::c_int,
    240 as ::core::ffi::c_int,
    150 as ::core::ffi::c_int,
    180 as ::core::ffi::c_int,
    150 as ::core::ffi::c_int,
    150 as ::core::ffi::c_int,
    300 as ::core::ffi::c_int,
    330 as ::core::ffi::c_int,
    420 as ::core::ffi::c_int,
    300 as ::core::ffi::c_int,
    180 as ::core::ffi::c_int,
    120 as ::core::ffi::c_int,
    30 as ::core::ffi::c_int,
];
#[no_mangle]
pub static mut secretexit: boolean = 0;
#[no_mangle]
pub unsafe extern "C" fn G_ExitLevel() {
    secretexit = false_0 as boolean;
    gameaction = ga_completed;
}
#[no_mangle]
pub unsafe extern "C" fn G_SecretExitLevel() {
    if gamemode as ::core::ffi::c_uint == commercial as ::core::ffi::c_int as ::core::ffi::c_uint
        && W_CheckNumForName(
            b"map31\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char
        ) < 0 as ::core::ffi::c_int
    {
        secretexit = false_0 as boolean;
    } else {
        secretexit = true_0 as boolean;
    }
    gameaction = ga_completed;
}
#[no_mangle]
pub unsafe extern "C" fn G_DoCompleted() {
    let mut i: ::core::ffi::c_int = 0;
    gameaction = ga_nothing;
    i = 0 as ::core::ffi::c_int;
    while i < MAXPLAYERS {
        if playeringame[i as usize] != 0 {
            G_PlayerFinishLevel(i);
        }
        i += 1;
    }
    if automapactive != 0 {
        AM_Stop();
    }
    if gamemode as ::core::ffi::c_uint != commercial as ::core::ffi::c_int as ::core::ffi::c_uint {
        if gameversion as ::core::ffi::c_uint
            == exe_chex as ::core::ffi::c_int as ::core::ffi::c_uint
        {
            if gamemap == 5 as ::core::ffi::c_int {
                gameaction = ga_victory;
                return;
            }
        } else {
            match gamemap {
                8 => {
                    gameaction = ga_victory;
                    return;
                }
                9 => {
                    i = 0 as ::core::ffi::c_int;
                    while i < MAXPLAYERS {
                        players[i as usize].didsecret = true_0 as boolean;
                        i += 1;
                    }
                }
                _ => {}
            }
        }
    }
    if gamemap == 8 as ::core::ffi::c_int
        && gamemode as ::core::ffi::c_uint
            != commercial as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        gameaction = ga_victory;
        return;
    }
    if gamemap == 9 as ::core::ffi::c_int
        && gamemode as ::core::ffi::c_uint
            != commercial as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        i = 0 as ::core::ffi::c_int;
        while i < MAXPLAYERS {
            players[i as usize].didsecret = true_0 as boolean;
            i += 1;
        }
    }
    wminfo.didsecret = players[consoleplayer as usize].didsecret;
    wminfo.epsd = gameepisode - 1 as ::core::ffi::c_int;
    wminfo.last = gamemap - 1 as ::core::ffi::c_int;
    if gamemode as ::core::ffi::c_uint == commercial as ::core::ffi::c_int as ::core::ffi::c_uint {
        if secretexit != 0 {
            match gamemap {
                15 => {
                    wminfo.next = 30 as ::core::ffi::c_int;
                }
                31 => {
                    wminfo.next = 31 as ::core::ffi::c_int;
                }
                _ => {}
            }
        } else {
            match gamemap {
                31 | 32 => {
                    wminfo.next = 15 as ::core::ffi::c_int;
                }
                _ => {
                    wminfo.next = gamemap;
                }
            }
        }
    } else if secretexit != 0 {
        wminfo.next = 8 as ::core::ffi::c_int;
    } else if gamemap == 9 as ::core::ffi::c_int {
        match gameepisode {
            1 => {
                wminfo.next = 3 as ::core::ffi::c_int;
            }
            2 => {
                wminfo.next = 5 as ::core::ffi::c_int;
            }
            3 => {
                wminfo.next = 6 as ::core::ffi::c_int;
            }
            4 => {
                wminfo.next = 2 as ::core::ffi::c_int;
            }
            _ => {}
        }
    } else {
        wminfo.next = gamemap;
    }
    wminfo.maxkills = totalkills;
    wminfo.maxitems = totalitems;
    wminfo.maxsecret = totalsecret;
    wminfo.maxfrags = 0 as ::core::ffi::c_int;
    if gamemode as ::core::ffi::c_uint == commercial as ::core::ffi::c_int as ::core::ffi::c_uint {
        wminfo.partime = TICRATE * cpars[(gamemap - 1 as ::core::ffi::c_int) as usize];
    } else if gameepisode < 4 as ::core::ffi::c_int {
        wminfo.partime = TICRATE * pars[gameepisode as usize][gamemap as usize];
    } else {
        wminfo.partime = TICRATE * cpars[gamemap as usize];
    }
    wminfo.pnum = consoleplayer;
    i = 0 as ::core::ffi::c_int;
    while i < MAXPLAYERS {
        wminfo.plyr[i as usize].in_0 = playeringame[i as usize];
        wminfo.plyr[i as usize].skills = players[i as usize].killcount;
        wminfo.plyr[i as usize].sitems = players[i as usize].itemcount;
        wminfo.plyr[i as usize].ssecret = players[i as usize].secretcount;
        wminfo.plyr[i as usize].stime = leveltime;
        memcpy(
            &raw mut (*(&raw mut wminfo.plyr as *mut wbplayerstruct_t).offset(i as isize)).frags
                as *mut ::core::ffi::c_int as *mut ::core::ffi::c_void,
            &raw mut (*(&raw mut players as *mut player_t).offset(i as isize)).frags
                as *mut ::core::ffi::c_int as *const ::core::ffi::c_void,
            ::core::mem::size_of::<[::core::ffi::c_int; 4]>() as size_t,
        );
        i += 1;
    }
    gamestate = GS_INTERMISSION;
    viewactive = false_0 as boolean;
    automapactive = false_0 as boolean;
    StatCopy(&raw mut wminfo);
    WI_Start(&raw mut wminfo);
}
#[no_mangle]
pub unsafe extern "C" fn G_WorldDone() {
    gameaction = ga_worlddone;
    if secretexit != 0 {
        players[consoleplayer as usize].didsecret = true_0 as boolean;
    }
    if gamemode as ::core::ffi::c_uint == commercial as ::core::ffi::c_int as ::core::ffi::c_uint {
        's_40: {
            match gamemap {
                15 | 31 => {
                    if secretexit == 0 {
                        break 's_40;
                    }
                }
                6 | 11 | 20 | 30 => {}
                _ => {
                    break 's_40;
                }
            }
            F_StartFinale();
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn G_DoWorldDone() {
    gamestate = GS_LEVEL;
    gamemap = wminfo.next + 1 as ::core::ffi::c_int;
    G_DoLoadLevel();
    gameaction = ga_nothing;
    viewactive = true_0 as boolean;
}
#[no_mangle]
pub static mut savename: [::core::ffi::c_char; 256] = [0; 256];
#[no_mangle]
pub unsafe extern "C" fn G_LoadGame(mut name: *mut ::core::ffi::c_char) {
    M_StringCopy(
        &raw mut savename as *mut ::core::ffi::c_char,
        name,
        ::core::mem::size_of::<[::core::ffi::c_char; 256]>() as size_t,
    );
    gameaction = ga_loadgame;
}
#[no_mangle]
pub unsafe extern "C" fn G_DoLoadGame() {
    let mut savedleveltime: ::core::ffi::c_int = 0;
    gameaction = ga_nothing;
    save_stream = fopen(
        &raw mut savename as *mut ::core::ffi::c_char,
        b"rb\0".as_ptr() as *const ::core::ffi::c_char,
    ) as *mut FILE;
    if save_stream.is_null() {
        return;
    }
    savegame_error = false_0 as boolean;
    if P_ReadSaveGameHeader() == 0 {
        fclose(save_stream);
        return;
    }
    savedleveltime = leveltime;
    G_InitNew(gameskill, gameepisode, gamemap);
    leveltime = savedleveltime;
    P_UnArchivePlayers();
    P_UnArchiveWorld();
    P_UnArchiveThinkers();
    P_UnArchiveSpecials();
    if P_ReadSaveGameEOF() == 0 {
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"Bad savegame\0".as_ptr() as *const ::core::ffi::c_char,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    fclose(save_stream);
    if setsizeneeded != 0 {
        R_ExecuteSetViewSize();
    }
    R_FillBackScreen();
}
#[no_mangle]
pub unsafe extern "C" fn G_SaveGame(
    mut slot: ::core::ffi::c_int,
    mut description: *mut ::core::ffi::c_char,
) {
    savegameslot = slot;
    M_StringCopy(
        &raw mut savedescription as *mut ::core::ffi::c_char,
        description,
        ::core::mem::size_of::<[::core::ffi::c_char; 32]>() as size_t,
    );
    sendsave = true_0 as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn G_DoSaveGame() {
    let mut savegame_file: *mut ::core::ffi::c_char =
        ::core::ptr::null_mut::<::core::ffi::c_char>();
    let mut temp_savegame_file: *mut ::core::ffi::c_char =
        ::core::ptr::null_mut::<::core::ffi::c_char>();
    let mut recovery_savegame_file: *mut ::core::ffi::c_char =
        ::core::ptr::null_mut::<::core::ffi::c_char>();
    recovery_savegame_file = ::core::ptr::null_mut::<::core::ffi::c_char>();
    temp_savegame_file = P_TempSaveGameFile();
    savegame_file = P_SaveGameFile(savegameslot);
    save_stream = fopen(
        temp_savegame_file,
        b"wb\0".as_ptr() as *const ::core::ffi::c_char,
    ) as *mut FILE;
    if save_stream.is_null() {
        recovery_savegame_file =
            M_TempFile(b"recovery.dsg\0".as_ptr() as *const ::core::ffi::c_char
                as *mut ::core::ffi::c_char);
        save_stream = fopen(
            recovery_savegame_file,
            b"wb\0".as_ptr() as *const ::core::ffi::c_char,
        ) as *mut FILE;
        if save_stream.is_null() {
            let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
            snprintf(
                &raw mut _i_err_buf as *mut ::core::ffi::c_char,
                ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
                b"Failed to open either '%s' or '%s' to write savegame.\0".as_ptr()
                    as *const ::core::ffi::c_char,
                temp_savegame_file,
                recovery_savegame_file,
            );
            I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
        }
    }
    savegame_error = false_0 as boolean;
    P_WriteSaveGameHeader(&raw mut savedescription as *mut ::core::ffi::c_char);
    P_ArchivePlayers();
    P_ArchiveWorld();
    P_ArchiveThinkers();
    P_ArchiveSpecials();
    P_WriteSaveGameEOF();
    if vanilla_savegame_limit != 0 && ftell(save_stream) > SAVEGAMESIZE as ::core::ffi::c_long {
        let mut _i_err_buf_0: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf_0 as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"Savegame buffer overrun\0".as_ptr() as *const ::core::ffi::c_char,
        );
        I_ErrorV(&raw mut _i_err_buf_0 as *mut ::core::ffi::c_char);
    }
    fclose(save_stream);
    if !recovery_savegame_file.is_null() {
        let mut _i_err_buf_1: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf_1 as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"Failed to open savegame file '%s' for writing.\nBut your game has been saved to '%s' for recovery.\0"
                .as_ptr() as *const ::core::ffi::c_char,
            temp_savegame_file,
            recovery_savegame_file,
        );
        I_ErrorV(&raw mut _i_err_buf_1 as *mut ::core::ffi::c_char);
    }
    remove(savegame_file);
    rename(temp_savegame_file, savegame_file);
    gameaction = ga_nothing;
    M_StringCopy(
        &raw mut savedescription as *mut ::core::ffi::c_char,
        b"\0".as_ptr() as *const ::core::ffi::c_char,
        ::core::mem::size_of::<[::core::ffi::c_char; 32]>() as size_t,
    );
    players[consoleplayer as usize].message =
        b"game saved.\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
    R_FillBackScreen();
}
#[no_mangle]
pub static mut d_skill: skill_t = sk_baby;
#[no_mangle]
pub static mut d_episode: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut d_map: ::core::ffi::c_int = 0;
#[no_mangle]
pub unsafe extern "C" fn G_DeferedInitNew(
    mut skill: skill_t,
    mut episode: ::core::ffi::c_int,
    mut map: ::core::ffi::c_int,
) {
    d_skill = skill;
    d_episode = episode;
    d_map = map;
    gameaction = ga_newgame;
}
#[no_mangle]
pub unsafe extern "C" fn G_DoNewGame() {
    demoplayback = false_0 as boolean;
    netdemo = false_0 as boolean;
    netgame = false_0 as boolean;
    deathmatch = false_0;
    playeringame[3 as ::core::ffi::c_int as usize] = 0 as boolean;
    playeringame[2 as ::core::ffi::c_int as usize] = playeringame[3 as ::core::ffi::c_int as usize];
    playeringame[1 as ::core::ffi::c_int as usize] = playeringame[2 as ::core::ffi::c_int as usize];
    respawnparm = false_0 as boolean;
    fastparm = false_0 as boolean;
    nomonsters = false_0 as boolean;
    consoleplayer = 0 as ::core::ffi::c_int;
    G_InitNew(d_skill, d_episode, d_map);
    gameaction = ga_nothing;
}
#[no_mangle]
pub unsafe extern "C" fn G_InitNew(
    mut skill: skill_t,
    mut episode: ::core::ffi::c_int,
    mut map: ::core::ffi::c_int,
) {
    let mut skytexturename: *mut ::core::ffi::c_char =
        ::core::ptr::null_mut::<::core::ffi::c_char>();
    let mut i: ::core::ffi::c_int = 0;
    if paused != 0 {
        paused = false_0 as boolean;
        S_ResumeSound();
    }
    if skill as ::core::ffi::c_int > sk_nightmare as ::core::ffi::c_int {
        skill = sk_nightmare;
    }
    if gameversion as ::core::ffi::c_uint
        >= exe_ultimate as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        if episode == 0 as ::core::ffi::c_int {
            episode = 4 as ::core::ffi::c_int;
        }
    } else {
        if episode < 1 as ::core::ffi::c_int {
            episode = 1 as ::core::ffi::c_int;
        }
        if episode > 3 as ::core::ffi::c_int {
            episode = 3 as ::core::ffi::c_int;
        }
    }
    if episode > 1 as ::core::ffi::c_int
        && gamemode as ::core::ffi::c_uint == shareware as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        episode = 1 as ::core::ffi::c_int;
    }
    if map < 1 as ::core::ffi::c_int {
        map = 1 as ::core::ffi::c_int;
    }
    if map > 9 as ::core::ffi::c_int
        && gamemode as ::core::ffi::c_uint
            != commercial as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        map = 9 as ::core::ffi::c_int;
    }
    M_ClearRandom();
    if skill as ::core::ffi::c_int == sk_nightmare as ::core::ffi::c_int || respawnparm != 0 {
        respawnmonsters = true_0 as boolean;
    } else {
        respawnmonsters = false_0 as boolean;
    }
    if fastparm != 0
        || skill as ::core::ffi::c_int == sk_nightmare as ::core::ffi::c_int
            && gameskill as ::core::ffi::c_int != sk_nightmare as ::core::ffi::c_int
    {
        i = S_SARG_RUN1 as ::core::ffi::c_int;
        while i <= S_SARG_PAIN2 as ::core::ffi::c_int {
            states[i as usize].tics >>= 1 as ::core::ffi::c_int;
            i += 1;
        }
        mobjinfo[MT_BRUISERSHOT as ::core::ffi::c_int as usize].speed =
            20 as ::core::ffi::c_int * FRACUNIT;
        mobjinfo[MT_HEADSHOT as ::core::ffi::c_int as usize].speed =
            20 as ::core::ffi::c_int * FRACUNIT;
        mobjinfo[MT_TROOPSHOT as ::core::ffi::c_int as usize].speed =
            20 as ::core::ffi::c_int * FRACUNIT;
    } else if skill as ::core::ffi::c_int != sk_nightmare as ::core::ffi::c_int
        && gameskill as ::core::ffi::c_int == sk_nightmare as ::core::ffi::c_int
    {
        i = S_SARG_RUN1 as ::core::ffi::c_int;
        while i <= S_SARG_PAIN2 as ::core::ffi::c_int {
            states[i as usize].tics <<= 1 as ::core::ffi::c_int;
            i += 1;
        }
        mobjinfo[MT_BRUISERSHOT as ::core::ffi::c_int as usize].speed =
            15 as ::core::ffi::c_int * FRACUNIT;
        mobjinfo[MT_HEADSHOT as ::core::ffi::c_int as usize].speed =
            10 as ::core::ffi::c_int * FRACUNIT;
        mobjinfo[MT_TROOPSHOT as ::core::ffi::c_int as usize].speed =
            10 as ::core::ffi::c_int * FRACUNIT;
    }
    i = 0 as ::core::ffi::c_int;
    while i < MAXPLAYERS {
        players[i as usize].playerstate = PST_REBORN;
        i += 1;
    }
    usergame = true_0 as boolean;
    paused = false_0 as boolean;
    demoplayback = false_0 as boolean;
    automapactive = false_0 as boolean;
    viewactive = true_0 as boolean;
    gameepisode = episode;
    gamemap = map;
    gameskill = skill;
    viewactive = true_0 as boolean;
    if gamemode as ::core::ffi::c_uint == commercial as ::core::ffi::c_int as ::core::ffi::c_uint {
        if gamemap < 12 as ::core::ffi::c_int {
            skytexturename =
                b"SKY1\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        } else if gamemap < 21 as ::core::ffi::c_int {
            skytexturename =
                b"SKY2\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        } else {
            skytexturename =
                b"SKY3\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        }
    } else {
        match gameepisode {
            2 => {
                skytexturename =
                    b"SKY2\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
            }
            3 => {
                skytexturename =
                    b"SKY3\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
            }
            4 => {
                skytexturename =
                    b"SKY4\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
            }
            1 | _ => {
                skytexturename =
                    b"SKY1\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
            }
        }
    }
    skytexturename = skytexturename;
    skytexture = R_TextureNumForName(skytexturename);
    G_DoLoadLevel();
}
pub const DEMOMARKER: ::core::ffi::c_int = 0x80 as ::core::ffi::c_int;
#[no_mangle]
pub unsafe extern "C" fn G_ReadDemoTiccmd(mut cmd: *mut ticcmd_t) {
    if *demo_p as ::core::ffi::c_int == DEMOMARKER {
        G_CheckDemoStatus();
        return;
    }
    let c2rust_fresh18 = demo_p;
    demo_p = demo_p.offset(1);
    (*cmd).forwardmove = *c2rust_fresh18 as ::core::ffi::c_schar;
    let c2rust_fresh19 = demo_p;
    demo_p = demo_p.offset(1);
    (*cmd).sidemove = *c2rust_fresh19 as ::core::ffi::c_schar;
    if longtics != 0 {
        let c2rust_fresh20 = demo_p;
        demo_p = demo_p.offset(1);
        (*cmd).angleturn = *c2rust_fresh20 as ::core::ffi::c_short;
        let c2rust_fresh21 = demo_p;
        demo_p = demo_p.offset(1);
        (*cmd).angleturn = ((*cmd).angleturn as ::core::ffi::c_int
            | (*c2rust_fresh21 as ::core::ffi::c_int) << 8 as ::core::ffi::c_int)
            as ::core::ffi::c_short;
    } else {
        let c2rust_fresh22 = demo_p;
        demo_p = demo_p.offset(1);
        (*cmd).angleturn = ((*c2rust_fresh22 as ::core::ffi::c_uchar as ::core::ffi::c_int)
            << 8 as ::core::ffi::c_int) as ::core::ffi::c_short;
    }
    let c2rust_fresh23 = demo_p;
    demo_p = demo_p.offset(1);
    (*cmd).buttons = *c2rust_fresh23 as ::core::ffi::c_uchar as byte;
}
unsafe extern "C" fn IncreaseDemoBuffer() {
    let mut current_length: ::core::ffi::c_int = 0;
    let mut new_demobuffer: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut new_demop: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut new_length: ::core::ffi::c_int = 0;
    current_length = demoend.offset_from(demobuffer) as ::core::ffi::c_long as ::core::ffi::c_int;
    new_length = current_length * 2 as ::core::ffi::c_int;
    new_demobuffer = Z_Malloc(
        new_length,
        PU_STATIC as ::core::ffi::c_int,
        ::core::ptr::null_mut::<::core::ffi::c_void>(),
    ) as *mut byte;
    new_demop =
        new_demobuffer.offset(demo_p.offset_from(demobuffer) as ::core::ffi::c_long as isize);
    memcpy(
        new_demobuffer as *mut ::core::ffi::c_void,
        demobuffer as *const ::core::ffi::c_void,
        current_length as size_t,
    );
    Z_Free(demobuffer as *mut ::core::ffi::c_void);
    demobuffer = new_demobuffer;
    demo_p = new_demop;
    demoend = demobuffer.offset(new_length as isize);
}
#[no_mangle]
pub unsafe extern "C" fn G_WriteDemoTiccmd(mut cmd: *mut ticcmd_t) {
    let mut demo_start: *mut byte = ::core::ptr::null_mut::<byte>();
    if gamekeydown[key_demo_quit as usize] != 0 {
        G_CheckDemoStatus();
    }
    demo_start = demo_p;
    let c2rust_fresh12 = demo_p;
    demo_p = demo_p.offset(1);
    *c2rust_fresh12 = (*cmd).forwardmove as byte;
    let c2rust_fresh13 = demo_p;
    demo_p = demo_p.offset(1);
    *c2rust_fresh13 = (*cmd).sidemove as byte;
    if longtics != 0 {
        let c2rust_fresh14 = demo_p;
        demo_p = demo_p.offset(1);
        *c2rust_fresh14 =
            ((*cmd).angleturn as ::core::ffi::c_int & 0xff as ::core::ffi::c_int) as byte;
        let c2rust_fresh15 = demo_p;
        demo_p = demo_p.offset(1);
        *c2rust_fresh15 = ((*cmd).angleturn as ::core::ffi::c_int >> 8 as ::core::ffi::c_int
            & 0xff as ::core::ffi::c_int) as byte;
    } else {
        let c2rust_fresh16 = demo_p;
        demo_p = demo_p.offset(1);
        *c2rust_fresh16 =
            ((*cmd).angleturn as ::core::ffi::c_int >> 8 as ::core::ffi::c_int) as byte;
    }
    let c2rust_fresh17 = demo_p;
    demo_p = demo_p.offset(1);
    *c2rust_fresh17 = (*cmd).buttons;
    demo_p = demo_start;
    if demo_p > demoend.offset(-(16 as ::core::ffi::c_int as isize)) {
        if vanilla_demo_limit != 0 {
            G_CheckDemoStatus();
            return;
        } else {
            IncreaseDemoBuffer();
        }
    }
    G_ReadDemoTiccmd(cmd);
}
#[no_mangle]
pub unsafe extern "C" fn G_RecordDemo(mut name: *mut ::core::ffi::c_char) {
    let mut demoname_size: size_t = 0;
    let mut i: ::core::ffi::c_int = 0;
    let mut maxsize: ::core::ffi::c_int = 0;
    usergame = false_0 as boolean;
    demoname_size = strlen(name).wrapping_add(5 as size_t);
    demoname = Z_Malloc(
        demoname_size as ::core::ffi::c_int,
        PU_STATIC as ::core::ffi::c_int,
        NULL,
    ) as *mut ::core::ffi::c_char;
    M_snprintf_clamp(
        demoname,
        demoname_size,
        snprintf(
            demoname,
            demoname_size,
            b"%s.lmp\0".as_ptr() as *const ::core::ffi::c_char,
            name,
        ),
    );
    maxsize = 0x20000 as ::core::ffi::c_int;
    i = M_CheckParmWithArgs(
        b"-maxdemo\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        1 as ::core::ffi::c_int,
    );
    if i != 0 {
        maxsize = atoi(*myargv.offset((i + 1 as ::core::ffi::c_int) as isize))
            * 1024 as ::core::ffi::c_int;
    }
    demobuffer = Z_Malloc(maxsize, PU_STATIC as ::core::ffi::c_int, NULL) as *mut byte;
    demoend = demobuffer.offset(maxsize as isize);
    demorecording = true_0 as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn G_VanillaVersionCode() -> ::core::ffi::c_int {
    match gameversion as ::core::ffi::c_uint {
        0 => {
            let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
            snprintf(
                &raw mut _i_err_buf as *mut ::core::ffi::c_char,
                ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
                b"Doom 1.2 does not have a version code!\0".as_ptr() as *const ::core::ffi::c_char,
            );
            I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
        }
        1 => {}
        2 => return 107 as ::core::ffi::c_int,
        3 => return 108 as ::core::ffi::c_int,
        4 | _ => return 109 as ::core::ffi::c_int,
    }
    return 106 as ::core::ffi::c_int;
}
#[no_mangle]
pub unsafe extern "C" fn G_BeginRecording() {
    let mut i: ::core::ffi::c_int = 0;
    longtics = (M_CheckParm(
        b"-longtics\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char
    ) != 0 as ::core::ffi::c_int) as ::core::ffi::c_int as boolean;
    lowres_turn = (longtics == 0) as ::core::ffi::c_int as boolean;
    demo_p = demobuffer;
    if longtics != 0 {
        let c2rust_fresh0 = demo_p;
        demo_p = demo_p.offset(1);
        *c2rust_fresh0 = DOOM_191_VERSION as byte;
    } else {
        let c2rust_fresh1 = demo_p;
        demo_p = demo_p.offset(1);
        *c2rust_fresh1 = G_VanillaVersionCode() as byte;
    }
    let c2rust_fresh2 = demo_p;
    demo_p = demo_p.offset(1);
    *c2rust_fresh2 = gameskill as byte;
    let c2rust_fresh3 = demo_p;
    demo_p = demo_p.offset(1);
    *c2rust_fresh3 = gameepisode as byte;
    let c2rust_fresh4 = demo_p;
    demo_p = demo_p.offset(1);
    *c2rust_fresh4 = gamemap as byte;
    let c2rust_fresh5 = demo_p;
    demo_p = demo_p.offset(1);
    *c2rust_fresh5 = deathmatch as byte;
    let c2rust_fresh6 = demo_p;
    demo_p = demo_p.offset(1);
    *c2rust_fresh6 = respawnparm as byte;
    let c2rust_fresh7 = demo_p;
    demo_p = demo_p.offset(1);
    *c2rust_fresh7 = fastparm as byte;
    let c2rust_fresh8 = demo_p;
    demo_p = demo_p.offset(1);
    *c2rust_fresh8 = nomonsters as byte;
    let c2rust_fresh9 = demo_p;
    demo_p = demo_p.offset(1);
    *c2rust_fresh9 = consoleplayer as byte;
    i = 0 as ::core::ffi::c_int;
    while i < MAXPLAYERS {
        let c2rust_fresh10 = demo_p;
        demo_p = demo_p.offset(1);
        *c2rust_fresh10 = playeringame[i as usize] as byte;
        i += 1;
    }
}
#[no_mangle]
pub static mut defdemoname: *mut ::core::ffi::c_char =
    ::core::ptr::null_mut::<::core::ffi::c_char>();
#[no_mangle]
pub unsafe extern "C" fn G_DeferedPlayDemo(mut name: *mut ::core::ffi::c_char) {
    defdemoname = name;
    gameaction = ga_playdemo;
}
unsafe extern "C" fn DemoVersionDescription(
    mut version: ::core::ffi::c_int,
) -> *mut ::core::ffi::c_char {
    static mut resultbuf: [::core::ffi::c_char; 16] = [0; 16];
    match version {
        104 => {
            return b"v1.4\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        }
        105 => {
            return b"v1.5\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        }
        106 => {
            return b"v1.6/v1.666\0".as_ptr() as *const ::core::ffi::c_char
                as *mut ::core::ffi::c_char;
        }
        107 => {
            return b"v1.7/v1.7a\0".as_ptr() as *const ::core::ffi::c_char
                as *mut ::core::ffi::c_char;
        }
        108 => {
            return b"v1.8\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        }
        109 => {
            return b"v1.9\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        }
        _ => {}
    }
    if version >= 0 as ::core::ffi::c_int && version <= 4 as ::core::ffi::c_int {
        return b"v1.0/v1.1/v1.2\0".as_ptr() as *const ::core::ffi::c_char
            as *mut ::core::ffi::c_char;
    } else {
        M_snprintf_clamp(
            &raw mut resultbuf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 16]>() as size_t,
            snprintf(
                &raw mut resultbuf as *mut ::core::ffi::c_char,
                ::core::mem::size_of::<[::core::ffi::c_char; 16]>() as size_t,
                b"%i.%i (unknown)\0".as_ptr() as *const ::core::ffi::c_char,
                version / 100 as ::core::ffi::c_int,
                version % 100 as ::core::ffi::c_int,
            ),
        );
        return &raw mut resultbuf as *mut ::core::ffi::c_char;
    };
}
#[no_mangle]
pub unsafe extern "C" fn G_DoPlayDemo() {
    let mut skill: skill_t = sk_baby;
    let mut i: ::core::ffi::c_int = 0;
    let mut episode: ::core::ffi::c_int = 0;
    let mut map: ::core::ffi::c_int = 0;
    let mut demoversion: ::core::ffi::c_int = 0;
    gameaction = ga_nothing;
    demo_p = W_CacheLumpName(defdemoname, PU_STATIC as ::core::ffi::c_int) as *mut byte;
    demobuffer = demo_p;
    let c2rust_fresh24 = demo_p;
    demo_p = demo_p.offset(1);
    demoversion = *c2rust_fresh24 as ::core::ffi::c_int;
    if demoversion == G_VanillaVersionCode() {
        longtics = false_0 as boolean;
    } else if demoversion == DOOM_191_VERSION {
        longtics = true_0 as boolean;
    } else {
        let mut message: *mut ::core::ffi::c_char = b"Demo is from a different game version!\n(read %i, should be %i)\n\n*** You may need to upgrade your version of Doom to v1.9. ***\n    See: https://www.doomworld.com/classicdoom/info/patches.php\n    This appears to be %s.\0"
            .as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
        printf(
            message,
            demoversion,
            G_VanillaVersionCode(),
            DemoVersionDescription(demoversion),
        );
    }
    let c2rust_fresh25 = demo_p;
    demo_p = demo_p.offset(1);
    skill = *c2rust_fresh25 as skill_t;
    let c2rust_fresh26 = demo_p;
    demo_p = demo_p.offset(1);
    episode = *c2rust_fresh26 as ::core::ffi::c_int;
    let c2rust_fresh27 = demo_p;
    demo_p = demo_p.offset(1);
    map = *c2rust_fresh27 as ::core::ffi::c_int;
    let c2rust_fresh28 = demo_p;
    demo_p = demo_p.offset(1);
    deathmatch = *c2rust_fresh28 as ::core::ffi::c_int;
    let c2rust_fresh29 = demo_p;
    demo_p = demo_p.offset(1);
    respawnparm = *c2rust_fresh29 as boolean;
    let c2rust_fresh30 = demo_p;
    demo_p = demo_p.offset(1);
    fastparm = *c2rust_fresh30 as boolean;
    let c2rust_fresh31 = demo_p;
    demo_p = demo_p.offset(1);
    nomonsters = *c2rust_fresh31 as boolean;
    let c2rust_fresh32 = demo_p;
    demo_p = demo_p.offset(1);
    consoleplayer = *c2rust_fresh32 as ::core::ffi::c_int;
    i = 0 as ::core::ffi::c_int;
    while i < MAXPLAYERS {
        let c2rust_fresh33 = demo_p;
        demo_p = demo_p.offset(1);
        playeringame[i as usize] = *c2rust_fresh33 as boolean;
        i += 1;
    }
    if playeringame[1 as ::core::ffi::c_int as usize] != 0
        || M_CheckParm(
            b"-solo-net\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char
        ) > 0 as ::core::ffi::c_int
        || M_CheckParm(
            b"-netdemo\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char
        ) > 0 as ::core::ffi::c_int
    {
        netgame = true_0 as boolean;
        netdemo = true_0 as boolean;
    }
    precache = false_0 as boolean;
    G_InitNew(skill, episode, map);
    precache = true_0 as boolean;
    starttime = I_GetTime();
    usergame = false_0 as boolean;
    demoplayback = true_0 as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn G_TimeDemo(mut name: *mut ::core::ffi::c_char) {
    nodrawers = M_CheckParm(
        b"-nodraw\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char
    ) as boolean;
    timingdemo = true_0 as boolean;
    singletics = true_0 as boolean;
    defdemoname = name;
    gameaction = ga_playdemo;
}
#[no_mangle]
pub unsafe extern "C" fn G_CheckDemoStatus() -> boolean {
    let mut endtime: ::core::ffi::c_int = 0;
    if timingdemo != 0 {
        let mut fps: ::core::ffi::c_float = 0.;
        let mut realtics: ::core::ffi::c_int = 0;
        endtime = I_GetTime();
        realtics = endtime - starttime;
        fps = gametic as ::core::ffi::c_float * TICRATE as ::core::ffi::c_float
            / realtics as ::core::ffi::c_float;
        timingdemo = false_0 as boolean;
        demoplayback = false_0 as boolean;
        let mut _i_err_buf: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"timed %i gametics in %i realtics (%f fps)\0".as_ptr() as *const ::core::ffi::c_char,
            gametic,
            realtics,
            fps as ::core::ffi::c_double,
        );
        I_ErrorV(&raw mut _i_err_buf as *mut ::core::ffi::c_char);
    }
    if demoplayback != 0 {
        W_ReleaseLumpName(defdemoname);
        demoplayback = false_0 as boolean;
        netdemo = false_0 as boolean;
        netgame = false_0 as boolean;
        deathmatch = false_0;
        playeringame[3 as ::core::ffi::c_int as usize] = 0 as boolean;
        playeringame[2 as ::core::ffi::c_int as usize] =
            playeringame[3 as ::core::ffi::c_int as usize];
        playeringame[1 as ::core::ffi::c_int as usize] =
            playeringame[2 as ::core::ffi::c_int as usize];
        respawnparm = false_0 as boolean;
        fastparm = false_0 as boolean;
        nomonsters = false_0 as boolean;
        consoleplayer = 0 as ::core::ffi::c_int;
        if singledemo != 0 {
            I_Quit();
        } else {
            D_AdvanceDemo();
        }
        return true_0 as boolean;
    }
    if demorecording != 0 {
        let c2rust_fresh11 = demo_p;
        demo_p = demo_p.offset(1);
        *c2rust_fresh11 = DEMOMARKER as byte;
        M_WriteFile(
            demoname,
            demobuffer as *mut ::core::ffi::c_void,
            demo_p.offset_from(demobuffer) as ::core::ffi::c_long as ::core::ffi::c_int,
        );
        Z_Free(demobuffer as *mut ::core::ffi::c_void);
        demorecording = false_0 as boolean;
        let mut _i_err_buf_0: [::core::ffi::c_char; 512] = [0; 512];
        snprintf(
            &raw mut _i_err_buf_0 as *mut ::core::ffi::c_char,
            ::core::mem::size_of::<[::core::ffi::c_char; 512]>() as size_t,
            b"Demo %s recorded\0".as_ptr() as *const ::core::ffi::c_char,
            demoname,
        );
        I_ErrorV(&raw mut _i_err_buf_0 as *mut ::core::ffi::c_char);
    }
    return false_0 as boolean;
}
pub const MAX_MOUSE_BUTTONS: ::core::ffi::c_int = 8 as ::core::ffi::c_int;
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
pub const true_0: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
pub const false_0: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
unsafe extern "C" fn c2rust_run_static_initializers() {
    joybuttons = (&raw mut joyarray as *mut boolean).offset(1 as ::core::ffi::c_int as isize)
        as *mut boolean;
    mousebuttons = (&raw mut mousearray as *mut boolean).offset(1 as ::core::ffi::c_int as isize)
        as *mut boolean;
}
#[used]
#[cfg_attr(target_os = "linux", link_section = ".init_array")]
#[cfg_attr(target_os = "windows", link_section = ".CRT$XIB")]
#[cfg_attr(target_os = "macos", link_section = "__DATA,__mod_init_func")]
static INIT_ARRAY: [unsafe extern "C" fn(); 1] = [c2rust_run_static_initializers];
