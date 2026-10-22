extern "C" {
    fn snprintf(
        __s: *mut ::core::ffi::c_char,
        __maxlen: size_t,
        __format: *const ::core::ffi::c_char,
        ...
    ) -> ::core::ffi::c_int;
    fn I_SetPalette(palette: *mut byte);
    fn Z_Malloc(
        size: ::core::ffi::c_int,
        tag: ::core::ffi::c_int,
        ptr: *mut ::core::ffi::c_void,
    ) -> *mut ::core::ffi::c_void;
    fn M_snprintf_clamp(
        buf: *mut ::core::ffi::c_char,
        len: size_t,
        result: ::core::ffi::c_int,
    ) -> ::core::ffi::c_int;
    fn M_Random() -> ::core::ffi::c_int;
    fn W_GetNumForName(name: *mut ::core::ffi::c_char) -> ::core::ffi::c_int;
    fn W_CacheLumpNum(
        lump: ::core::ffi::c_int,
        tag: ::core::ffi::c_int,
    ) -> *mut ::core::ffi::c_void;
    fn W_CacheLumpName(
        name: *mut ::core::ffi::c_char,
        tag: ::core::ffi::c_int,
    ) -> *mut ::core::ffi::c_void;
    fn W_ReleaseLumpName(name: *mut ::core::ffi::c_char);
    fn G_DeferedInitNew(skill: skill_t, episode: ::core::ffi::c_int, map: ::core::ffi::c_int);
    fn cht_CheckCheat(cht: *mut cheatseq_t, key: ::core::ffi::c_char) -> ::core::ffi::c_int;
    fn cht_GetParam(cht: *mut cheatseq_t, buffer: *mut ::core::ffi::c_char);
    fn STlib_init();
    fn STlib_initNum(
        n: *mut st_number_t,
        x: ::core::ffi::c_int,
        y: ::core::ffi::c_int,
        pl: *mut *mut patch_t,
        num: *mut ::core::ffi::c_int,
        on: *mut boolean,
        width: ::core::ffi::c_int,
    );
    fn STlib_updateNum(n: *mut st_number_t, refresh: boolean);
    fn STlib_initPercent(
        p: *mut st_percent_t,
        x: ::core::ffi::c_int,
        y: ::core::ffi::c_int,
        pl: *mut *mut patch_t,
        num: *mut ::core::ffi::c_int,
        on: *mut boolean,
        percent: *mut patch_t,
    );
    fn STlib_updatePercent(per: *mut st_percent_t, refresh: ::core::ffi::c_int);
    fn STlib_initMultIcon(
        mi: *mut st_multicon_t,
        x: ::core::ffi::c_int,
        y: ::core::ffi::c_int,
        il: *mut *mut patch_t,
        inum: *mut ::core::ffi::c_int,
        on: *mut boolean,
    );
    fn STlib_updateMultIcon(mi: *mut st_multicon_t, refresh: boolean);
    fn STlib_initBinIcon(
        b: *mut st_binicon_t,
        x: ::core::ffi::c_int,
        y: ::core::ffi::c_int,
        i: *mut patch_t,
        val: *mut boolean,
        on: *mut boolean,
    );
    fn STlib_updateBinIcon(bi: *mut st_binicon_t, refresh: boolean);
    static mut weaponinfo: [weaponinfo_t; 9];
    fn R_PointToAngle2(x1: fixed_t, y1: fixed_t, x2: fixed_t, y2: fixed_t) -> angle_t;
    fn P_GivePower(_: *mut player_t, _: ::core::ffi::c_int) -> boolean;
    fn S_ChangeMusic(music_id: ::core::ffi::c_int, looping: ::core::ffi::c_int);
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
    fn V_UseBuffer(buffer: *mut byte);
    fn V_RestoreBuffer();
    static mut gamemode: GameMode_t;
    static mut gamemission: GameMission_t;
    static mut gameversion: GameVersion_t;
    static mut gameskill: skill_t;
    static mut netgame: boolean;
    static mut deathmatch: ::core::ffi::c_int;
    static mut automapactive: boolean;
    static mut consoleplayer: ::core::ffi::c_int;
    static mut players: [player_t; 4];
}
pub type size_t = usize;
pub type __uint8_t = u8;
pub type uint8_t = __uint8_t;
pub type boolean = ::core::ffi::c_uint;
pub type byte = uint8_t;
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
pub type C2Rust_Unnamed_0 = ::core::ffi::c_uint;
pub const NUMCARDS: C2Rust_Unnamed_0 = 6;
pub const it_redskull: C2Rust_Unnamed_0 = 5;
pub const it_yellowskull: C2Rust_Unnamed_0 = 4;
pub const it_blueskull: C2Rust_Unnamed_0 = 3;
pub const it_redcard: C2Rust_Unnamed_0 = 2;
pub const it_yellowcard: C2Rust_Unnamed_0 = 1;
pub const it_bluecard: C2Rust_Unnamed_0 = 0;
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
pub type ammotype_t = ::core::ffi::c_uint;
pub const am_noammo: ammotype_t = 5;
pub const NUMAMMO: ammotype_t = 4;
pub const am_misl: ammotype_t = 3;
pub const am_cell: ammotype_t = 2;
pub const am_shell: ammotype_t = 1;
pub const am_clip: ammotype_t = 0;
pub type C2Rust_Unnamed_1 = ::core::ffi::c_uint;
pub const NUMPOWERS: C2Rust_Unnamed_1 = 6;
pub const pw_infrared: C2Rust_Unnamed_1 = 5;
pub const pw_allmap: C2Rust_Unnamed_1 = 4;
pub const pw_ironfeet: C2Rust_Unnamed_1 = 3;
pub const pw_invisibility: C2Rust_Unnamed_1 = 2;
pub const pw_strength: C2Rust_Unnamed_1 = 1;
pub const pw_invulnerability: C2Rust_Unnamed_1 = 0;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct cheatseq_t {
    pub sequence: [::core::ffi::c_char; 25],
    pub sequence_len: size_t,
    pub parameter_chars: ::core::ffi::c_int,
    pub chars_read: size_t,
    pub param_chars_read: ::core::ffi::c_int,
    pub parameter_buf: [::core::ffi::c_char; 5],
}
pub type player_t = player_s;
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
pub type fixed_t = ::core::ffi::c_int;
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
pub union actionf_t {
    pub acv: actionf_v,
    pub acp1: actionf_p1,
    pub acp2: actionf_p2,
}
pub type actionf_p2 =
    Option<unsafe extern "C" fn(*mut ::core::ffi::c_void, *mut ::core::ffi::c_void) -> ()>;
pub type actionf_p1 = Option<unsafe extern "C" fn(*mut ::core::ffi::c_void) -> ()>;
pub type actionf_v = Option<unsafe extern "C" fn() -> ()>;
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
pub type mobj_t = mobj_s;
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
pub type thinker_t = thinker_s;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct thinker_s {
    pub prev: *mut thinker_s,
    pub next: *mut thinker_s,
    pub function: think_t,
}
pub type think_t = actionf_t;
pub type angle_t = ::core::ffi::c_uint;
pub type playerstate_t = ::core::ffi::c_uint;
pub const PST_REBORN: playerstate_t = 2;
pub const PST_DEAD: playerstate_t = 1;
pub const PST_LIVE: playerstate_t = 0;
pub const CF_NOCLIP: C2Rust_Unnamed_2 = 1;
pub const mus_e1m1: C2Rust_Unnamed_3 = 1;
pub const mus_runnin: C2Rust_Unnamed_3 = 33;
pub const CF_GODMODE: C2Rust_Unnamed_2 = 2;
pub type st_stateenum_t = ::core::ffi::c_uint;
pub const FirstPersonState: st_stateenum_t = 1;
pub const AutomapState: st_stateenum_t = 0;
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
pub struct weaponinfo_t {
    pub ammo: ammotype_t,
    pub upstate: ::core::ffi::c_int,
    pub downstate: ::core::ffi::c_int,
    pub readystate: ::core::ffi::c_int,
    pub atkstate: ::core::ffi::c_int,
    pub flashstate: ::core::ffi::c_int,
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
#[derive(Copy, Clone)]
#[repr(C)]
pub struct st_percent_t {
    pub n: st_number_t,
    pub p: *mut patch_t,
}
pub type st_chatstateenum_t = ::core::ffi::c_uint;
pub const GetChatState: st_chatstateenum_t = 2;
pub const WaitDestState: st_chatstateenum_t = 1;
pub const StartChatState: st_chatstateenum_t = 0;
pub type load_callback_t =
    Option<unsafe extern "C" fn(*mut ::core::ffi::c_char, *mut *mut patch_t) -> ()>;
pub type C2Rust_Unnamed_2 = ::core::ffi::c_uint;
pub const CF_NOMOMENTUM: C2Rust_Unnamed_2 = 4;
pub type C2Rust_Unnamed_3 = ::core::ffi::c_uint;
pub const NUMMUSIC: C2Rust_Unnamed_3 = 68;
pub const mus_dm2int: C2Rust_Unnamed_3 = 67;
pub const mus_dm2ttl: C2Rust_Unnamed_3 = 66;
pub const mus_read_m: C2Rust_Unnamed_3 = 65;
pub const mus_ultima: C2Rust_Unnamed_3 = 64;
pub const mus_evil: C2Rust_Unnamed_3 = 63;
pub const mus_openin: C2Rust_Unnamed_3 = 62;
pub const mus_shawn3: C2Rust_Unnamed_3 = 61;
pub const mus_tense: C2Rust_Unnamed_3 = 60;
pub const mus_romer2: C2Rust_Unnamed_3 = 59;
pub const mus_messg2: C2Rust_Unnamed_3 = 58;
pub const mus_adrian: C2Rust_Unnamed_3 = 57;
pub const mus_theda3: C2Rust_Unnamed_3 = 56;
pub const mus_ampie: C2Rust_Unnamed_3 = 55;
pub const mus_ddtbl3: C2Rust_Unnamed_3 = 54;
pub const mus_count2: C2Rust_Unnamed_3 = 53;
pub const mus_messag: C2Rust_Unnamed_3 = 52;
pub const mus_shawn2: C2Rust_Unnamed_3 = 51;
pub const mus_romero: C2Rust_Unnamed_3 = 50;
pub const mus_stlks3: C2Rust_Unnamed_3 = 49;
pub const mus_dead2: C2Rust_Unnamed_3 = 48;
pub const mus_runni2: C2Rust_Unnamed_3 = 47;
pub const mus_ddtbl2: C2Rust_Unnamed_3 = 46;
pub const mus_doom2: C2Rust_Unnamed_3 = 45;
pub const mus_theda2: C2Rust_Unnamed_3 = 44;
pub const mus_stlks2: C2Rust_Unnamed_3 = 43;
pub const mus_dead: C2Rust_Unnamed_3 = 42;
pub const mus_in_cit: C2Rust_Unnamed_3 = 41;
pub const mus_ddtblu: C2Rust_Unnamed_3 = 40;
pub const mus_shawn: C2Rust_Unnamed_3 = 39;
pub const mus_the_da: C2Rust_Unnamed_3 = 38;
pub const mus_doom: C2Rust_Unnamed_3 = 37;
pub const mus_betwee: C2Rust_Unnamed_3 = 36;
pub const mus_countd: C2Rust_Unnamed_3 = 35;
pub const mus_stalks: C2Rust_Unnamed_3 = 34;
pub const mus_introa: C2Rust_Unnamed_3 = 32;
pub const mus_victor: C2Rust_Unnamed_3 = 31;
pub const mus_bunny: C2Rust_Unnamed_3 = 30;
pub const mus_intro: C2Rust_Unnamed_3 = 29;
pub const mus_inter: C2Rust_Unnamed_3 = 28;
pub const mus_e3m9: C2Rust_Unnamed_3 = 27;
pub const mus_e3m8: C2Rust_Unnamed_3 = 26;
pub const mus_e3m7: C2Rust_Unnamed_3 = 25;
pub const mus_e3m6: C2Rust_Unnamed_3 = 24;
pub const mus_e3m5: C2Rust_Unnamed_3 = 23;
pub const mus_e3m4: C2Rust_Unnamed_3 = 22;
pub const mus_e3m3: C2Rust_Unnamed_3 = 21;
pub const mus_e3m2: C2Rust_Unnamed_3 = 20;
pub const mus_e3m1: C2Rust_Unnamed_3 = 19;
pub const mus_e2m9: C2Rust_Unnamed_3 = 18;
pub const mus_e2m8: C2Rust_Unnamed_3 = 17;
pub const mus_e2m7: C2Rust_Unnamed_3 = 16;
pub const mus_e2m6: C2Rust_Unnamed_3 = 15;
pub const mus_e2m5: C2Rust_Unnamed_3 = 14;
pub const mus_e2m4: C2Rust_Unnamed_3 = 13;
pub const mus_e2m3: C2Rust_Unnamed_3 = 12;
pub const mus_e2m2: C2Rust_Unnamed_3 = 11;
pub const mus_e2m1: C2Rust_Unnamed_3 = 10;
pub const mus_e1m9: C2Rust_Unnamed_3 = 9;
pub const mus_e1m8: C2Rust_Unnamed_3 = 8;
pub const mus_e1m7: C2Rust_Unnamed_3 = 7;
pub const mus_e1m6: C2Rust_Unnamed_3 = 6;
pub const mus_e1m5: C2Rust_Unnamed_3 = 5;
pub const mus_e1m4: C2Rust_Unnamed_3 = 4;
pub const mus_e1m3: C2Rust_Unnamed_3 = 3;
pub const mus_e1m2: C2Rust_Unnamed_3 = 2;
pub const mus_None: C2Rust_Unnamed_3 = 0;
pub const true_0: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
pub const false_0: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
pub const SCREENWIDTH: ::core::ffi::c_int = 320 as ::core::ffi::c_int;
pub const SCREENHEIGHT: ::core::ffi::c_int = 200 as ::core::ffi::c_int;
pub const DEH_DEFAULT_GOD_MODE_HEALTH: ::core::ffi::c_int = 100 as ::core::ffi::c_int;
pub const DEH_DEFAULT_IDFA_ARMOR: ::core::ffi::c_int = 200 as ::core::ffi::c_int;
pub const DEH_DEFAULT_IDFA_ARMOR_CLASS: ::core::ffi::c_int = 2 as ::core::ffi::c_int;
pub const DEH_DEFAULT_IDKFA_ARMOR: ::core::ffi::c_int = 200 as ::core::ffi::c_int;
pub const DEH_DEFAULT_IDKFA_ARMOR_CLASS: ::core::ffi::c_int = 2 as ::core::ffi::c_int;
pub const deh_god_mode_health: ::core::ffi::c_int = DEH_DEFAULT_GOD_MODE_HEALTH;
pub const deh_idfa_armor: ::core::ffi::c_int = DEH_DEFAULT_IDFA_ARMOR;
pub const deh_idfa_armor_class: ::core::ffi::c_int = DEH_DEFAULT_IDFA_ARMOR_CLASS;
pub const deh_idkfa_armor: ::core::ffi::c_int = DEH_DEFAULT_IDKFA_ARMOR;
pub const deh_idkfa_armor_class: ::core::ffi::c_int = DEH_DEFAULT_IDKFA_ARMOR_CLASS;
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
pub const TICRATE: ::core::ffi::c_int = 35 as ::core::ffi::c_int;
pub const MAXPLAYERS: ::core::ffi::c_int = 4 as ::core::ffi::c_int;
pub const ST_HEIGHT: ::core::ffi::c_int = 32 as ::core::ffi::c_int;
pub const ST_WIDTH: ::core::ffi::c_int = SCREENWIDTH;
pub const ST_Y: ::core::ffi::c_int = SCREENHEIGHT - ST_HEIGHT;
pub const ANG45: ::core::ffi::c_int = 0x20000000 as ::core::ffi::c_int;
pub const ANG180: ::core::ffi::c_uint = 0x80000000 as ::core::ffi::c_uint;
pub const AM_MSGHEADER: ::core::ffi::c_int = (('a' as ::core::ffi::c_int)
    << 24 as ::core::ffi::c_int)
    + (('m' as ::core::ffi::c_int) << 16 as ::core::ffi::c_int);
pub const AM_MSGENTERED: ::core::ffi::c_int = 1634559232;
pub const AM_MSGEXITED: ::core::ffi::c_int = 1634564096;
pub const STARTREDPALS: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
pub const STARTBONUSPALS: ::core::ffi::c_int = 9 as ::core::ffi::c_int;
pub const NUMREDPALS: ::core::ffi::c_int = 8 as ::core::ffi::c_int;
pub const NUMBONUSPALS: ::core::ffi::c_int = 4 as ::core::ffi::c_int;
pub const RADIATIONPAL: ::core::ffi::c_int = 13 as ::core::ffi::c_int;
pub const ST_X: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
pub const ST_FX: ::core::ffi::c_int = 143 as ::core::ffi::c_int;
pub const ST_NUMPAINFACES: ::core::ffi::c_int = 5 as ::core::ffi::c_int;
pub const ST_NUMSTRAIGHTFACES: ::core::ffi::c_int = 3 as ::core::ffi::c_int;
pub const ST_NUMTURNFACES: ::core::ffi::c_int = 2 as ::core::ffi::c_int;
pub const ST_NUMSPECIALFACES: ::core::ffi::c_int = 3 as ::core::ffi::c_int;
pub const ST_FACESTRIDE: ::core::ffi::c_int =
    ST_NUMSTRAIGHTFACES + ST_NUMTURNFACES + ST_NUMSPECIALFACES;
pub const ST_TURNOFFSET: ::core::ffi::c_int = 3 as ::core::ffi::c_int;
pub const ST_OUCHOFFSET: ::core::ffi::c_int = ST_TURNOFFSET + ST_NUMTURNFACES;
pub const ST_EVILGRINOFFSET: ::core::ffi::c_int = ST_OUCHOFFSET + 1 as ::core::ffi::c_int;
pub const ST_RAMPAGEOFFSET: ::core::ffi::c_int = ST_EVILGRINOFFSET + 1 as ::core::ffi::c_int;
pub const ST_GODFACE: ::core::ffi::c_int = ST_NUMPAINFACES * ST_FACESTRIDE;
pub const ST_DEADFACE: ::core::ffi::c_int = ST_GODFACE + 1 as ::core::ffi::c_int;
pub const ST_FACESX: ::core::ffi::c_int = 143 as ::core::ffi::c_int;
pub const ST_FACESY: ::core::ffi::c_int = 168 as ::core::ffi::c_int;
pub const ST_EVILGRINCOUNT: ::core::ffi::c_int = 2 as ::core::ffi::c_int * TICRATE;
pub const ST_STRAIGHTFACECOUNT: ::core::ffi::c_int = TICRATE / 2 as ::core::ffi::c_int;
pub const ST_TURNCOUNT: ::core::ffi::c_int = 1 as ::core::ffi::c_int * TICRATE;
pub const ST_RAMPAGEDELAY: ::core::ffi::c_int = 2 as ::core::ffi::c_int * TICRATE;
pub const ST_MUCHPAIN: ::core::ffi::c_int = 20 as ::core::ffi::c_int;
pub const ST_AMMOWIDTH: ::core::ffi::c_int = 3 as ::core::ffi::c_int;
pub const ST_AMMOX: ::core::ffi::c_int = 44 as ::core::ffi::c_int;
pub const ST_AMMOY: ::core::ffi::c_int = 171 as ::core::ffi::c_int;
pub const ST_HEALTHX: ::core::ffi::c_int = 90 as ::core::ffi::c_int;
pub const ST_HEALTHY: ::core::ffi::c_int = 171 as ::core::ffi::c_int;
pub const ST_ARMSX: ::core::ffi::c_int = 111 as ::core::ffi::c_int;
pub const ST_ARMSY: ::core::ffi::c_int = 172 as ::core::ffi::c_int;
pub const ST_ARMSBGX: ::core::ffi::c_int = 104 as ::core::ffi::c_int;
pub const ST_ARMSBGY: ::core::ffi::c_int = 168 as ::core::ffi::c_int;
pub const ST_ARMSXSPACE: ::core::ffi::c_int = 12 as ::core::ffi::c_int;
pub const ST_ARMSYSPACE: ::core::ffi::c_int = 10 as ::core::ffi::c_int;
pub const ST_FRAGSX: ::core::ffi::c_int = 138 as ::core::ffi::c_int;
pub const ST_FRAGSY: ::core::ffi::c_int = 171 as ::core::ffi::c_int;
pub const ST_FRAGSWIDTH: ::core::ffi::c_int = 2 as ::core::ffi::c_int;
pub const ST_ARMORX: ::core::ffi::c_int = 221 as ::core::ffi::c_int;
pub const ST_ARMORY: ::core::ffi::c_int = 171 as ::core::ffi::c_int;
pub const ST_KEY0X: ::core::ffi::c_int = 239 as ::core::ffi::c_int;
pub const ST_KEY0Y: ::core::ffi::c_int = 171 as ::core::ffi::c_int;
pub const ST_KEY1X: ::core::ffi::c_int = 239 as ::core::ffi::c_int;
pub const ST_KEY1Y: ::core::ffi::c_int = 181 as ::core::ffi::c_int;
pub const ST_KEY2X: ::core::ffi::c_int = 239 as ::core::ffi::c_int;
pub const ST_KEY2Y: ::core::ffi::c_int = 191 as ::core::ffi::c_int;
pub const ST_AMMO0WIDTH: ::core::ffi::c_int = 3 as ::core::ffi::c_int;
pub const ST_AMMO0X: ::core::ffi::c_int = 288 as ::core::ffi::c_int;
pub const ST_AMMO0Y: ::core::ffi::c_int = 173 as ::core::ffi::c_int;
pub const ST_AMMO1WIDTH: ::core::ffi::c_int = ST_AMMO0WIDTH;
pub const ST_AMMO1X: ::core::ffi::c_int = 288 as ::core::ffi::c_int;
pub const ST_AMMO1Y: ::core::ffi::c_int = 179 as ::core::ffi::c_int;
pub const ST_AMMO2WIDTH: ::core::ffi::c_int = ST_AMMO0WIDTH;
pub const ST_AMMO2X: ::core::ffi::c_int = 288 as ::core::ffi::c_int;
pub const ST_AMMO2Y: ::core::ffi::c_int = 191 as ::core::ffi::c_int;
pub const ST_AMMO3WIDTH: ::core::ffi::c_int = ST_AMMO0WIDTH;
pub const ST_AMMO3X: ::core::ffi::c_int = 288 as ::core::ffi::c_int;
pub const ST_AMMO3Y: ::core::ffi::c_int = 185 as ::core::ffi::c_int;
pub const ST_MAXAMMO0WIDTH: ::core::ffi::c_int = 3 as ::core::ffi::c_int;
pub const ST_MAXAMMO0X: ::core::ffi::c_int = 314 as ::core::ffi::c_int;
pub const ST_MAXAMMO0Y: ::core::ffi::c_int = 173 as ::core::ffi::c_int;
pub const ST_MAXAMMO1WIDTH: ::core::ffi::c_int = ST_MAXAMMO0WIDTH;
pub const ST_MAXAMMO1X: ::core::ffi::c_int = 314 as ::core::ffi::c_int;
pub const ST_MAXAMMO1Y: ::core::ffi::c_int = 179 as ::core::ffi::c_int;
pub const ST_MAXAMMO2WIDTH: ::core::ffi::c_int = ST_MAXAMMO0WIDTH;
pub const ST_MAXAMMO2X: ::core::ffi::c_int = 314 as ::core::ffi::c_int;
pub const ST_MAXAMMO2Y: ::core::ffi::c_int = 191 as ::core::ffi::c_int;
pub const ST_MAXAMMO3WIDTH: ::core::ffi::c_int = ST_MAXAMMO0WIDTH;
pub const ST_MAXAMMO3X: ::core::ffi::c_int = 314 as ::core::ffi::c_int;
pub const ST_MAXAMMO3Y: ::core::ffi::c_int = 185 as ::core::ffi::c_int;
#[no_mangle]
pub static mut st_backing_screen: *mut byte = ::core::ptr::null_mut::<byte>();
static mut plyr: *mut player_t = ::core::ptr::null_mut::<player_t>();
static mut st_firsttime: boolean = 0;
static mut lu_palette: ::core::ffi::c_int = 0;
static mut st_clock: ::core::ffi::c_uint = 0;
static mut st_msgcounter: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
static mut st_chatstate: st_chatstateenum_t = StartChatState;
static mut st_gamestate: st_stateenum_t = AutomapState;
static mut st_statusbaron: boolean = 0;
static mut st_chat: boolean = 0;
static mut st_oldchat: boolean = 0;
static mut st_cursoron: boolean = 0;
static mut st_notdeathmatch: boolean = 0;
static mut st_armson: boolean = 0;
static mut st_fragson: boolean = 0;
static mut sbar: *mut patch_t = ::core::ptr::null_mut::<patch_t>();
static mut tallnum: [*mut patch_t; 10] = [::core::ptr::null_mut::<patch_t>(); 10];
static mut tallpercent: *mut patch_t = ::core::ptr::null_mut::<patch_t>();
static mut shortnum: [*mut patch_t; 10] = [::core::ptr::null_mut::<patch_t>(); 10];
static mut keys: [*mut patch_t; 6] = [::core::ptr::null_mut::<patch_t>(); 6];
static mut faces: [*mut patch_t; 42] = [::core::ptr::null_mut::<patch_t>(); 42];
static mut faceback: *mut patch_t = ::core::ptr::null_mut::<patch_t>();
static mut armsbg: *mut patch_t = ::core::ptr::null_mut::<patch_t>();
static mut arms: [[*mut patch_t; 2]; 6] = [[::core::ptr::null_mut::<patch_t>(); 2]; 6];
static mut w_ready: st_number_t = st_number_t {
    x: 0,
    y: 0,
    width: 0,
    oldnum: 0,
    num: ::core::ptr::null_mut::<::core::ffi::c_int>(),
    on: ::core::ptr::null_mut::<boolean>(),
    p: ::core::ptr::null_mut::<*mut patch_t>(),
    data: 0,
};
static mut w_frags: st_number_t = st_number_t {
    x: 0,
    y: 0,
    width: 0,
    oldnum: 0,
    num: ::core::ptr::null_mut::<::core::ffi::c_int>(),
    on: ::core::ptr::null_mut::<boolean>(),
    p: ::core::ptr::null_mut::<*mut patch_t>(),
    data: 0,
};
static mut w_health: st_percent_t = st_percent_t {
    n: st_number_t {
        x: 0,
        y: 0,
        width: 0,
        oldnum: 0,
        num: ::core::ptr::null_mut::<::core::ffi::c_int>(),
        on: ::core::ptr::null_mut::<boolean>(),
        p: ::core::ptr::null_mut::<*mut patch_t>(),
        data: 0,
    },
    p: ::core::ptr::null_mut::<patch_t>(),
};
static mut w_armsbg: st_binicon_t = st_binicon_t {
    x: 0,
    y: 0,
    oldval: 0,
    val: ::core::ptr::null_mut::<boolean>(),
    on: ::core::ptr::null_mut::<boolean>(),
    p: ::core::ptr::null_mut::<patch_t>(),
    data: 0,
};
static mut w_arms: [st_multicon_t; 6] = [st_multicon_t {
    x: 0,
    y: 0,
    oldinum: 0,
    inum: ::core::ptr::null_mut::<::core::ffi::c_int>(),
    on: ::core::ptr::null_mut::<boolean>(),
    p: ::core::ptr::null_mut::<*mut patch_t>(),
    data: 0,
}; 6];
static mut w_faces: st_multicon_t = st_multicon_t {
    x: 0,
    y: 0,
    oldinum: 0,
    inum: ::core::ptr::null_mut::<::core::ffi::c_int>(),
    on: ::core::ptr::null_mut::<boolean>(),
    p: ::core::ptr::null_mut::<*mut patch_t>(),
    data: 0,
};
static mut w_keyboxes: [st_multicon_t; 3] = [st_multicon_t {
    x: 0,
    y: 0,
    oldinum: 0,
    inum: ::core::ptr::null_mut::<::core::ffi::c_int>(),
    on: ::core::ptr::null_mut::<boolean>(),
    p: ::core::ptr::null_mut::<*mut patch_t>(),
    data: 0,
}; 3];
static mut w_armor: st_percent_t = st_percent_t {
    n: st_number_t {
        x: 0,
        y: 0,
        width: 0,
        oldnum: 0,
        num: ::core::ptr::null_mut::<::core::ffi::c_int>(),
        on: ::core::ptr::null_mut::<boolean>(),
        p: ::core::ptr::null_mut::<*mut patch_t>(),
        data: 0,
    },
    p: ::core::ptr::null_mut::<patch_t>(),
};
static mut w_ammo: [st_number_t; 4] = [st_number_t {
    x: 0,
    y: 0,
    width: 0,
    oldnum: 0,
    num: ::core::ptr::null_mut::<::core::ffi::c_int>(),
    on: ::core::ptr::null_mut::<boolean>(),
    p: ::core::ptr::null_mut::<*mut patch_t>(),
    data: 0,
}; 4];
static mut w_maxammo: [st_number_t; 4] = [st_number_t {
    x: 0,
    y: 0,
    width: 0,
    oldnum: 0,
    num: ::core::ptr::null_mut::<::core::ffi::c_int>(),
    on: ::core::ptr::null_mut::<boolean>(),
    p: ::core::ptr::null_mut::<*mut patch_t>(),
    data: 0,
}; 4];
static mut st_fragscount: ::core::ffi::c_int = 0;
static mut st_oldhealth: ::core::ffi::c_int = -1 as ::core::ffi::c_int;
static mut oldweaponsowned: [boolean; 9] = [0; 9];
static mut st_facecount: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
static mut st_faceindex: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
static mut keyboxes: [::core::ffi::c_int; 3] = [0; 3];
static mut st_randomnumber: ::core::ffi::c_int = 0;
#[no_mangle]
pub static mut cheat_mus: cheatseq_t = cheatseq_t {
    sequence: [0; 25],
    sequence_len: 0,
    parameter_chars: 0,
    chars_read: 0,
    param_chars_read: 0,
    parameter_buf: [0; 5],
};
#[no_mangle]
pub static mut cheat_god: cheatseq_t = cheatseq_t {
    sequence: [0; 25],
    sequence_len: 0,
    parameter_chars: 0,
    chars_read: 0,
    param_chars_read: 0,
    parameter_buf: [0; 5],
};
#[no_mangle]
pub static mut cheat_ammo: cheatseq_t = cheatseq_t {
    sequence: [0; 25],
    sequence_len: 0,
    parameter_chars: 0,
    chars_read: 0,
    param_chars_read: 0,
    parameter_buf: [0; 5],
};
#[no_mangle]
pub static mut cheat_ammonokey: cheatseq_t = cheatseq_t {
    sequence: [0; 25],
    sequence_len: 0,
    parameter_chars: 0,
    chars_read: 0,
    param_chars_read: 0,
    parameter_buf: [0; 5],
};
#[no_mangle]
pub static mut cheat_noclip: cheatseq_t = cheatseq_t {
    sequence: [0; 25],
    sequence_len: 0,
    parameter_chars: 0,
    chars_read: 0,
    param_chars_read: 0,
    parameter_buf: [0; 5],
};
#[no_mangle]
pub static mut cheat_commercial_noclip: cheatseq_t = cheatseq_t {
    sequence: [0; 25],
    sequence_len: 0,
    parameter_chars: 0,
    chars_read: 0,
    param_chars_read: 0,
    parameter_buf: [0; 5],
};
#[no_mangle]
pub static mut cheat_powerup: [cheatseq_t; 7] = [cheatseq_t {
    sequence: [0; 25],
    sequence_len: 0,
    parameter_chars: 0,
    chars_read: 0,
    param_chars_read: 0,
    parameter_buf: [0; 5],
}; 7];
#[no_mangle]
pub static mut cheat_choppers: cheatseq_t = cheatseq_t {
    sequence: [0; 25],
    sequence_len: 0,
    parameter_chars: 0,
    chars_read: 0,
    param_chars_read: 0,
    parameter_buf: [0; 5],
};
#[no_mangle]
pub static mut cheat_clev: cheatseq_t = cheatseq_t {
    sequence: [0; 25],
    sequence_len: 0,
    parameter_chars: 0,
    chars_read: 0,
    param_chars_read: 0,
    parameter_buf: [0; 5],
};
#[no_mangle]
pub static mut cheat_mypos: cheatseq_t = cheatseq_t {
    sequence: [0; 25],
    sequence_len: 0,
    parameter_chars: 0,
    chars_read: 0,
    param_chars_read: 0,
    parameter_buf: [0; 5],
};
#[no_mangle]
pub unsafe extern "C" fn ST_refreshBackground() {
    if st_statusbaron != 0 {
        V_UseBuffer(st_backing_screen);
        V_DrawPatch(ST_X, 0 as ::core::ffi::c_int, sbar);
        if netgame != 0 {
            V_DrawPatch(ST_FX, 0 as ::core::ffi::c_int, faceback);
        }
        V_RestoreBuffer();
        V_CopyRect(
            ST_X,
            0 as ::core::ffi::c_int,
            st_backing_screen,
            ST_WIDTH,
            ST_HEIGHT,
            ST_X,
            ST_Y,
        );
    }
}
#[no_mangle]
pub unsafe extern "C" fn ST_Responder(mut ev: *mut event_t) -> boolean {
    let mut i: ::core::ffi::c_int = 0;
    if (*ev).type_0 as ::core::ffi::c_uint == ev_keyup as ::core::ffi::c_int as ::core::ffi::c_uint
        && (*ev).data1 as ::core::ffi::c_uint & 0xffff0000 as ::core::ffi::c_uint
            == AM_MSGHEADER as ::core::ffi::c_uint
    {
        match (*ev).data1 {
            AM_MSGENTERED => {
                st_gamestate = AutomapState;
                st_firsttime = true_0 as boolean;
            }
            AM_MSGEXITED => {
                st_gamestate = FirstPersonState;
            }
            _ => {}
        }
    } else if (*ev).type_0 as ::core::ffi::c_uint
        == ev_keydown as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        if netgame == 0 && gameskill as ::core::ffi::c_int != sk_nightmare as ::core::ffi::c_int {
            if cht_CheckCheat(&raw mut cheat_god, (*ev).data2 as ::core::ffi::c_char) != 0 {
                (*plyr).cheats ^= CF_GODMODE as ::core::ffi::c_int;
                if (*plyr).cheats & CF_GODMODE as ::core::ffi::c_int != 0 {
                    if !(*plyr).mo.is_null() {
                        (*(*plyr).mo).health = 100 as ::core::ffi::c_int;
                    }
                    (*plyr).health = deh_god_mode_health;
                    (*plyr).message = b"Degreelessness Mode On\0".as_ptr()
                        as *const ::core::ffi::c_char
                        as *mut ::core::ffi::c_char;
                } else {
                    (*plyr).message = b"Degreelessness Mode Off\0".as_ptr()
                        as *const ::core::ffi::c_char
                        as *mut ::core::ffi::c_char;
                }
            } else if cht_CheckCheat(&raw mut cheat_ammonokey, (*ev).data2 as ::core::ffi::c_char)
                != 0
            {
                (*plyr).armorpoints = deh_idfa_armor;
                (*plyr).armortype = deh_idfa_armor_class;
                i = 0 as ::core::ffi::c_int;
                while i < NUMWEAPONS as ::core::ffi::c_int {
                    (*plyr).weaponowned[i as usize] = true_0 as boolean;
                    i += 1;
                }
                i = 0 as ::core::ffi::c_int;
                while i < NUMAMMO as ::core::ffi::c_int {
                    (*plyr).ammo[i as usize] = (*plyr).maxammo[i as usize];
                    i += 1;
                }
                (*plyr).message = b"Ammo (no keys) Added\0".as_ptr() as *const ::core::ffi::c_char
                    as *mut ::core::ffi::c_char;
            } else if cht_CheckCheat(&raw mut cheat_ammo, (*ev).data2 as ::core::ffi::c_char) != 0 {
                (*plyr).armorpoints = deh_idkfa_armor;
                (*plyr).armortype = deh_idkfa_armor_class;
                i = 0 as ::core::ffi::c_int;
                while i < NUMWEAPONS as ::core::ffi::c_int {
                    (*plyr).weaponowned[i as usize] = true_0 as boolean;
                    i += 1;
                }
                i = 0 as ::core::ffi::c_int;
                while i < NUMAMMO as ::core::ffi::c_int {
                    (*plyr).ammo[i as usize] = (*plyr).maxammo[i as usize];
                    i += 1;
                }
                i = 0 as ::core::ffi::c_int;
                while i < NUMCARDS as ::core::ffi::c_int {
                    (*plyr).cards[i as usize] = true_0 as boolean;
                    i += 1;
                }
                (*plyr).message = b"Very Happy Ammo Added\0".as_ptr() as *const ::core::ffi::c_char
                    as *mut ::core::ffi::c_char;
            } else if cht_CheckCheat(&raw mut cheat_mus, (*ev).data2 as ::core::ffi::c_char) != 0 {
                let mut buf: [::core::ffi::c_char; 3] = [0; 3];
                let mut musnum: ::core::ffi::c_int = 0;
                (*plyr).message = b"Music Change\0".as_ptr() as *const ::core::ffi::c_char
                    as *mut ::core::ffi::c_char;
                cht_GetParam(&raw mut cheat_mus, &raw mut buf as *mut ::core::ffi::c_char);
                if gamemode as ::core::ffi::c_uint
                    == commercial as ::core::ffi::c_int as ::core::ffi::c_uint
                    || (gameversion as ::core::ffi::c_uint)
                        < exe_ultimate as ::core::ffi::c_int as ::core::ffi::c_uint
                {
                    musnum = mus_runnin as ::core::ffi::c_int
                        + (buf[0 as ::core::ffi::c_int as usize] as ::core::ffi::c_int
                            - '0' as ::core::ffi::c_int)
                            * 10 as ::core::ffi::c_int
                        + buf[1 as ::core::ffi::c_int as usize] as ::core::ffi::c_int
                        - '0' as ::core::ffi::c_int
                        - 1 as ::core::ffi::c_int;
                    if (buf[0 as ::core::ffi::c_int as usize] as ::core::ffi::c_int
                        - '0' as ::core::ffi::c_int)
                        * 10 as ::core::ffi::c_int
                        + buf[1 as ::core::ffi::c_int as usize] as ::core::ffi::c_int
                        - '0' as ::core::ffi::c_int
                        > 35 as ::core::ffi::c_int
                    {
                        (*plyr).message = b"IMPOSSIBLE SELECTION\0".as_ptr()
                            as *const ::core::ffi::c_char
                            as *mut ::core::ffi::c_char;
                    } else {
                        S_ChangeMusic(musnum, 1 as ::core::ffi::c_int);
                    }
                } else {
                    musnum = mus_e1m1 as ::core::ffi::c_int
                        + (buf[0 as ::core::ffi::c_int as usize] as ::core::ffi::c_int
                            - '1' as ::core::ffi::c_int)
                            * 9 as ::core::ffi::c_int
                        + (buf[1 as ::core::ffi::c_int as usize] as ::core::ffi::c_int
                            - '1' as ::core::ffi::c_int);
                    if (buf[0 as ::core::ffi::c_int as usize] as ::core::ffi::c_int
                        - '1' as ::core::ffi::c_int)
                        * 9 as ::core::ffi::c_int
                        + buf[1 as ::core::ffi::c_int as usize] as ::core::ffi::c_int
                        - '1' as ::core::ffi::c_int
                        > 31 as ::core::ffi::c_int
                    {
                        (*plyr).message = b"IMPOSSIBLE SELECTION\0".as_ptr()
                            as *const ::core::ffi::c_char
                            as *mut ::core::ffi::c_char;
                    } else {
                        S_ChangeMusic(musnum, 1 as ::core::ffi::c_int);
                    }
                }
            } else if (if gamemission as ::core::ffi::c_uint
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
                && cht_CheckCheat(&raw mut cheat_noclip, (*ev).data2 as ::core::ffi::c_char) != 0
                || (if gamemission as ::core::ffi::c_uint
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
                }) != doom as ::core::ffi::c_int as ::core::ffi::c_uint
                    && cht_CheckCheat(
                        &raw mut cheat_commercial_noclip,
                        (*ev).data2 as ::core::ffi::c_char,
                    ) != 0
            {
                (*plyr).cheats ^= CF_NOCLIP as ::core::ffi::c_int;
                if (*plyr).cheats & CF_NOCLIP as ::core::ffi::c_int != 0 {
                    (*plyr).message = b"No Clipping Mode ON\0".as_ptr()
                        as *const ::core::ffi::c_char
                        as *mut ::core::ffi::c_char;
                } else {
                    (*plyr).message = b"No Clipping Mode OFF\0".as_ptr()
                        as *const ::core::ffi::c_char
                        as *mut ::core::ffi::c_char;
                }
            }
            i = 0 as ::core::ffi::c_int;
            while i < 6 as ::core::ffi::c_int {
                if cht_CheckCheat(
                    (&raw mut cheat_powerup as *mut cheatseq_t).offset(i as isize)
                        as *mut cheatseq_t,
                    (*ev).data2 as ::core::ffi::c_char,
                ) != 0
                {
                    if (*plyr).powers[i as usize] == 0 {
                        P_GivePower(plyr, i);
                    } else if i != pw_strength as ::core::ffi::c_int {
                        (*plyr).powers[i as usize] = 1 as ::core::ffi::c_int;
                    } else {
                        (*plyr).powers[i as usize] = 0 as ::core::ffi::c_int;
                    }
                    (*plyr).message = b"Power-up Toggled\0".as_ptr() as *const ::core::ffi::c_char
                        as *mut ::core::ffi::c_char;
                }
                i += 1;
            }
            if cht_CheckCheat(
                (&raw mut cheat_powerup as *mut cheatseq_t).offset(6 as ::core::ffi::c_int as isize)
                    as *mut cheatseq_t,
                (*ev).data2 as ::core::ffi::c_char,
            ) != 0
            {
                (*plyr).message = b"inVuln, Str, Inviso, Rad, Allmap, or Lite-amp\0".as_ptr()
                    as *const ::core::ffi::c_char
                    as *mut ::core::ffi::c_char;
            } else if cht_CheckCheat(&raw mut cheat_choppers, (*ev).data2 as ::core::ffi::c_char)
                != 0
            {
                (*plyr).weaponowned[wp_chainsaw as ::core::ffi::c_int as usize] = true_0 as boolean;
                (*plyr).powers[pw_invulnerability as ::core::ffi::c_int as usize] = true_0;
                (*plyr).message = b"... doesn't suck - GM\0".as_ptr() as *const ::core::ffi::c_char
                    as *mut ::core::ffi::c_char;
            } else if cht_CheckCheat(&raw mut cheat_mypos, (*ev).data2 as ::core::ffi::c_char) != 0
            {
                static mut buf_0: [::core::ffi::c_char; 52] = [0; 52];
                M_snprintf_clamp(
                    &raw mut buf_0 as *mut ::core::ffi::c_char,
                    ::core::mem::size_of::<[::core::ffi::c_char; 52]>() as size_t,
                    snprintf(
                        &raw mut buf_0 as *mut ::core::ffi::c_char,
                        ::core::mem::size_of::<[::core::ffi::c_char; 52]>() as size_t,
                        b"ang=0x%x;x,y=(0x%x,0x%x)\0".as_ptr() as *const ::core::ffi::c_char,
                        (*players[consoleplayer as usize].mo).angle,
                        (*players[consoleplayer as usize].mo).x,
                        (*players[consoleplayer as usize].mo).y,
                    ),
                );
                (*plyr).message = &raw mut buf_0 as *mut ::core::ffi::c_char;
            }
        }
        if netgame == 0
            && cht_CheckCheat(&raw mut cheat_clev, (*ev).data2 as ::core::ffi::c_char) != 0
        {
            let mut buf_1: [::core::ffi::c_char; 3] = [0; 3];
            let mut epsd: ::core::ffi::c_int = 0;
            let mut map: ::core::ffi::c_int = 0;
            cht_GetParam(
                &raw mut cheat_clev,
                &raw mut buf_1 as *mut ::core::ffi::c_char,
            );
            if gamemode as ::core::ffi::c_uint
                == commercial as ::core::ffi::c_int as ::core::ffi::c_uint
            {
                epsd = 1 as ::core::ffi::c_int;
                map = (buf_1[0 as ::core::ffi::c_int as usize] as ::core::ffi::c_int
                    - '0' as ::core::ffi::c_int)
                    * 10 as ::core::ffi::c_int
                    + buf_1[1 as ::core::ffi::c_int as usize] as ::core::ffi::c_int
                    - '0' as ::core::ffi::c_int;
            } else {
                epsd = buf_1[0 as ::core::ffi::c_int as usize] as ::core::ffi::c_int
                    - '0' as ::core::ffi::c_int;
                map = buf_1[1 as ::core::ffi::c_int as usize] as ::core::ffi::c_int
                    - '0' as ::core::ffi::c_int;
            }
            if gameversion as ::core::ffi::c_uint
                == exe_chex as ::core::ffi::c_int as ::core::ffi::c_uint
            {
                epsd = 1 as ::core::ffi::c_int;
            }
            if epsd < 1 as ::core::ffi::c_int {
                return false_0 as boolean;
            }
            if map < 1 as ::core::ffi::c_int {
                return false_0 as boolean;
            }
            if gamemode as ::core::ffi::c_uint
                == retail as ::core::ffi::c_int as ::core::ffi::c_uint
                && (epsd > 4 as ::core::ffi::c_int || map > 9 as ::core::ffi::c_int)
            {
                return false_0 as boolean;
            }
            if gamemode as ::core::ffi::c_uint
                == registered as ::core::ffi::c_int as ::core::ffi::c_uint
                && (epsd > 3 as ::core::ffi::c_int || map > 9 as ::core::ffi::c_int)
            {
                return false_0 as boolean;
            }
            if gamemode as ::core::ffi::c_uint
                == shareware as ::core::ffi::c_int as ::core::ffi::c_uint
                && (epsd > 1 as ::core::ffi::c_int || map > 9 as ::core::ffi::c_int)
            {
                return false_0 as boolean;
            }
            if gamemode as ::core::ffi::c_uint
                == commercial as ::core::ffi::c_int as ::core::ffi::c_uint
                && (epsd > 1 as ::core::ffi::c_int || map > 40 as ::core::ffi::c_int)
            {
                return false_0 as boolean;
            }
            (*plyr).message = b"Changing Level...\0".as_ptr() as *const ::core::ffi::c_char
                as *mut ::core::ffi::c_char;
            G_DeferedInitNew(gameskill, epsd, map);
        }
    }
    return false_0 as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn ST_calcPainOffset() -> ::core::ffi::c_int {
    let mut health: ::core::ffi::c_int = 0;
    static mut lastcalc: ::core::ffi::c_int = 0;
    static mut oldhealth: ::core::ffi::c_int = -1 as ::core::ffi::c_int;
    health = if (*plyr).health > 100 as ::core::ffi::c_int {
        100 as ::core::ffi::c_int
    } else {
        (*plyr).health
    };
    if health != oldhealth {
        lastcalc = ST_FACESTRIDE
            * ((100 as ::core::ffi::c_int - health) * ST_NUMPAINFACES / 101 as ::core::ffi::c_int);
        oldhealth = health;
    }
    return lastcalc;
}
#[no_mangle]
pub unsafe extern "C" fn ST_updateFaceWidget() {
    let mut i: ::core::ffi::c_int = 0;
    let mut badguyangle: angle_t = 0;
    let mut diffang: angle_t = 0;
    static mut lastattackdown: ::core::ffi::c_int = -1 as ::core::ffi::c_int;
    static mut priority: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
    let mut doevilgrin: boolean = 0;
    if priority < 10 as ::core::ffi::c_int {
        if (*plyr).health == 0 {
            priority = 9 as ::core::ffi::c_int;
            st_faceindex = ST_DEADFACE;
            st_facecount = 1 as ::core::ffi::c_int;
        }
    }
    if priority < 9 as ::core::ffi::c_int {
        if (*plyr).bonuscount != 0 {
            doevilgrin = false_0 as boolean;
            i = 0 as ::core::ffi::c_int;
            while i < NUMWEAPONS as ::core::ffi::c_int {
                if oldweaponsowned[i as usize] != (*plyr).weaponowned[i as usize] {
                    doevilgrin = true_0 as boolean;
                    oldweaponsowned[i as usize] = (*plyr).weaponowned[i as usize];
                }
                i += 1;
            }
            if doevilgrin != 0 {
                priority = 8 as ::core::ffi::c_int;
                st_facecount = ST_EVILGRINCOUNT;
                st_faceindex = ST_calcPainOffset() + ST_EVILGRINOFFSET;
            }
        }
    }
    if priority < 8 as ::core::ffi::c_int {
        if (*plyr).damagecount != 0 && !(*plyr).attacker.is_null() && (*plyr).attacker != (*plyr).mo
        {
            priority = 7 as ::core::ffi::c_int;
            if (*plyr).health - st_oldhealth > ST_MUCHPAIN {
                st_facecount = ST_TURNCOUNT;
                st_faceindex = ST_calcPainOffset() + ST_OUCHOFFSET;
            } else {
                badguyangle = R_PointToAngle2(
                    (*(*plyr).mo).x,
                    (*(*plyr).mo).y,
                    (*(*plyr).attacker).x,
                    (*(*plyr).attacker).y,
                );
                if badguyangle > (*(*plyr).mo).angle {
                    diffang = badguyangle.wrapping_sub((*(*plyr).mo).angle);
                    i = (diffang > ANG180) as ::core::ffi::c_int;
                } else {
                    diffang = (*(*plyr).mo).angle.wrapping_sub(badguyangle);
                    i = (diffang <= ANG180) as ::core::ffi::c_int;
                }
                st_facecount = ST_TURNCOUNT;
                st_faceindex = ST_calcPainOffset();
                if diffang < ANG45 as angle_t {
                    st_faceindex += ST_RAMPAGEOFFSET;
                } else if i != 0 {
                    st_faceindex += ST_TURNOFFSET;
                } else {
                    st_faceindex += ST_TURNOFFSET + 1 as ::core::ffi::c_int;
                }
            }
        }
    }
    if priority < 7 as ::core::ffi::c_int {
        if (*plyr).damagecount != 0 {
            if (*plyr).health - st_oldhealth > ST_MUCHPAIN {
                priority = 7 as ::core::ffi::c_int;
                st_facecount = ST_TURNCOUNT;
                st_faceindex = ST_calcPainOffset() + ST_OUCHOFFSET;
            } else {
                priority = 6 as ::core::ffi::c_int;
                st_facecount = ST_TURNCOUNT;
                st_faceindex = ST_calcPainOffset() + ST_RAMPAGEOFFSET;
            }
        }
    }
    if priority < 6 as ::core::ffi::c_int {
        if (*plyr).attackdown != 0 {
            if lastattackdown == -1 as ::core::ffi::c_int {
                lastattackdown = ST_RAMPAGEDELAY;
            } else {
                lastattackdown -= 1;
                if lastattackdown == 0 {
                    priority = 5 as ::core::ffi::c_int;
                    st_faceindex = ST_calcPainOffset() + ST_RAMPAGEOFFSET;
                    st_facecount = 1 as ::core::ffi::c_int;
                    lastattackdown = 1 as ::core::ffi::c_int;
                }
            }
        } else {
            lastattackdown = -1 as ::core::ffi::c_int;
        }
    }
    if priority < 5 as ::core::ffi::c_int {
        if (*plyr).cheats & CF_GODMODE as ::core::ffi::c_int != 0
            || (*plyr).powers[pw_invulnerability as ::core::ffi::c_int as usize] != 0
        {
            priority = 4 as ::core::ffi::c_int;
            st_faceindex = ST_GODFACE;
            st_facecount = 1 as ::core::ffi::c_int;
        }
    }
    if st_facecount == 0 {
        st_faceindex = ST_calcPainOffset() + st_randomnumber % 3 as ::core::ffi::c_int;
        st_facecount = ST_STRAIGHTFACECOUNT;
        priority = 0 as ::core::ffi::c_int;
    }
    st_facecount -= 1;
}
#[no_mangle]
pub unsafe extern "C" fn ST_updateWidgets() {
    static mut largeammo: ::core::ffi::c_int = 1994 as ::core::ffi::c_int;
    let mut i: ::core::ffi::c_int = 0;
    if weaponinfo[(*plyr).readyweapon as usize].ammo as ::core::ffi::c_uint
        == am_noammo as ::core::ffi::c_int as ::core::ffi::c_uint
    {
        w_ready.num = &raw mut largeammo;
    } else {
        w_ready.num = (&raw mut (*plyr).ammo as *mut ::core::ffi::c_int).offset(
            (*(&raw mut weaponinfo as *mut weaponinfo_t).offset((*plyr).readyweapon as isize)).ammo
                as isize,
        ) as *mut ::core::ffi::c_int;
    }
    w_ready.data = (*plyr).readyweapon as ::core::ffi::c_int;
    i = 0 as ::core::ffi::c_int;
    while i < 3 as ::core::ffi::c_int {
        keyboxes[i as usize] = if (*plyr).cards[i as usize] != 0 {
            i
        } else {
            -1 as ::core::ffi::c_int
        };
        if (*plyr).cards[(i + 3 as ::core::ffi::c_int) as usize] != 0 {
            keyboxes[i as usize] = i + 3 as ::core::ffi::c_int;
        }
        i += 1;
    }
    ST_updateFaceWidget();
    st_notdeathmatch = (deathmatch == 0) as ::core::ffi::c_int as boolean;
    st_armson = (st_statusbaron != 0 && deathmatch == 0) as ::core::ffi::c_int as boolean;
    st_fragson = (deathmatch != 0 && st_statusbaron != 0) as ::core::ffi::c_int as boolean;
    st_fragscount = 0 as ::core::ffi::c_int;
    i = 0 as ::core::ffi::c_int;
    while i < MAXPLAYERS {
        if i != consoleplayer {
            st_fragscount += (*plyr).frags[i as usize];
        } else {
            st_fragscount -= (*plyr).frags[i as usize];
        }
        i += 1;
    }
    st_msgcounter -= 1;
    if st_msgcounter == 0 {
        st_chat = st_oldchat;
    }
}
#[no_mangle]
pub unsafe extern "C" fn ST_Ticker() {
    st_clock = st_clock.wrapping_add(1);
    st_randomnumber = M_Random();
    ST_updateWidgets();
    st_oldhealth = (*plyr).health;
}
static mut st_palette: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
#[no_mangle]
pub unsafe extern "C" fn ST_doPaletteStuff() {
    let mut palette: ::core::ffi::c_int = 0;
    let mut pal: *mut byte = ::core::ptr::null_mut::<byte>();
    let mut cnt: ::core::ffi::c_int = 0;
    let mut bzc: ::core::ffi::c_int = 0;
    cnt = (*plyr).damagecount;
    if (*plyr).powers[pw_strength as ::core::ffi::c_int as usize] != 0 {
        bzc = 12 as ::core::ffi::c_int
            - ((*plyr).powers[pw_strength as ::core::ffi::c_int as usize]
                >> 6 as ::core::ffi::c_int);
        if bzc > cnt {
            cnt = bzc;
        }
    }
    if cnt != 0 {
        palette = cnt + 7 as ::core::ffi::c_int >> 3 as ::core::ffi::c_int;
        if palette >= NUMREDPALS {
            palette = NUMREDPALS - 1 as ::core::ffi::c_int;
        }
        palette += STARTREDPALS;
    } else if (*plyr).bonuscount != 0 {
        palette = (*plyr).bonuscount + 7 as ::core::ffi::c_int >> 3 as ::core::ffi::c_int;
        if palette >= NUMBONUSPALS {
            palette = NUMBONUSPALS - 1 as ::core::ffi::c_int;
        }
        palette += STARTBONUSPALS;
    } else if (*plyr).powers[pw_ironfeet as ::core::ffi::c_int as usize]
        > 4 as ::core::ffi::c_int * 32 as ::core::ffi::c_int
        || (*plyr).powers[pw_ironfeet as ::core::ffi::c_int as usize] & 8 as ::core::ffi::c_int != 0
    {
        palette = RADIATIONPAL;
    } else {
        palette = 0 as ::core::ffi::c_int;
    }
    if gameversion as ::core::ffi::c_uint == exe_chex as ::core::ffi::c_int as ::core::ffi::c_uint
        && palette >= STARTREDPALS
        && palette < STARTREDPALS + NUMREDPALS
    {
        palette = RADIATIONPAL;
    }
    if palette != st_palette {
        st_palette = palette;
        pal = (W_CacheLumpNum(lu_palette, PU_CACHE as ::core::ffi::c_int) as *mut byte)
            .offset((palette * 768 as ::core::ffi::c_int) as isize);
        I_SetPalette(pal);
    }
}
#[no_mangle]
pub unsafe extern "C" fn ST_drawWidgets(mut refresh: boolean) {
    let mut i: ::core::ffi::c_int = 0;
    st_armson = (st_statusbaron != 0 && deathmatch == 0) as ::core::ffi::c_int as boolean;
    st_fragson = (deathmatch != 0 && st_statusbaron != 0) as ::core::ffi::c_int as boolean;
    STlib_updateNum(&raw mut w_ready, refresh);
    i = 0 as ::core::ffi::c_int;
    while i < 4 as ::core::ffi::c_int {
        STlib_updateNum(
            (&raw mut w_ammo as *mut st_number_t).offset(i as isize) as *mut st_number_t,
            refresh,
        );
        STlib_updateNum(
            (&raw mut w_maxammo as *mut st_number_t).offset(i as isize) as *mut st_number_t,
            refresh,
        );
        i += 1;
    }
    STlib_updatePercent(&raw mut w_health, refresh as ::core::ffi::c_int);
    STlib_updatePercent(&raw mut w_armor, refresh as ::core::ffi::c_int);
    STlib_updateBinIcon(&raw mut w_armsbg, refresh);
    i = 0 as ::core::ffi::c_int;
    while i < 6 as ::core::ffi::c_int {
        STlib_updateMultIcon(
            (&raw mut w_arms as *mut st_multicon_t).offset(i as isize) as *mut st_multicon_t,
            refresh,
        );
        i += 1;
    }
    STlib_updateMultIcon(&raw mut w_faces, refresh);
    i = 0 as ::core::ffi::c_int;
    while i < 3 as ::core::ffi::c_int {
        STlib_updateMultIcon(
            (&raw mut w_keyboxes as *mut st_multicon_t).offset(i as isize) as *mut st_multicon_t,
            refresh,
        );
        i += 1;
    }
    STlib_updateNum(&raw mut w_frags, refresh);
}
#[no_mangle]
pub unsafe extern "C" fn ST_doRefresh() {
    st_firsttime = false_0 as boolean;
    ST_refreshBackground();
    ST_drawWidgets(true_0 as boolean);
}
#[no_mangle]
pub unsafe extern "C" fn ST_diffDraw() {
    ST_drawWidgets(false_0 as boolean);
}
#[no_mangle]
pub unsafe extern "C" fn ST_Drawer(mut fullscreen: boolean, mut refresh: boolean) {
    st_statusbaron = (fullscreen == 0 || automapactive != 0) as ::core::ffi::c_int as boolean;
    st_firsttime = (st_firsttime != 0 || refresh != 0) as ::core::ffi::c_int as boolean;
    ST_doPaletteStuff();
    if st_firsttime != 0 {
        ST_doRefresh();
    } else {
        ST_diffDraw();
    };
}
unsafe extern "C" fn ST_loadUnloadGraphics(mut callback: load_callback_t) {
    let mut i: ::core::ffi::c_int = 0;
    let mut j: ::core::ffi::c_int = 0;
    let mut facenum: ::core::ffi::c_int = 0;
    let mut namebuf: [::core::ffi::c_char; 9] = [0; 9];
    i = 0 as ::core::ffi::c_int;
    while i < 10 as ::core::ffi::c_int {
        snprintf(
            &raw mut namebuf as *mut ::core::ffi::c_char,
            9 as size_t,
            b"STTNUM%d\0".as_ptr() as *const ::core::ffi::c_char,
            i,
        );
        callback.expect("non-null function pointer")(
            &raw mut namebuf as *mut ::core::ffi::c_char,
            (&raw mut tallnum as *mut *mut patch_t).offset(i as isize) as *mut *mut patch_t,
        );
        snprintf(
            &raw mut namebuf as *mut ::core::ffi::c_char,
            9 as size_t,
            b"STYSNUM%d\0".as_ptr() as *const ::core::ffi::c_char,
            i,
        );
        callback.expect("non-null function pointer")(
            &raw mut namebuf as *mut ::core::ffi::c_char,
            (&raw mut shortnum as *mut *mut patch_t).offset(i as isize) as *mut *mut patch_t,
        );
        i += 1;
    }
    callback.expect("non-null function pointer")(
        b"STTPRCNT\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut tallpercent,
    );
    i = 0 as ::core::ffi::c_int;
    while i < NUMCARDS as ::core::ffi::c_int {
        snprintf(
            &raw mut namebuf as *mut ::core::ffi::c_char,
            9 as size_t,
            b"STKEYS%d\0".as_ptr() as *const ::core::ffi::c_char,
            i,
        );
        callback.expect("non-null function pointer")(
            &raw mut namebuf as *mut ::core::ffi::c_char,
            (&raw mut keys as *mut *mut patch_t).offset(i as isize) as *mut *mut patch_t,
        );
        i += 1;
    }
    callback.expect("non-null function pointer")(
        b"STARMS\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut armsbg,
    );
    i = 0 as ::core::ffi::c_int;
    while i < 6 as ::core::ffi::c_int {
        snprintf(
            &raw mut namebuf as *mut ::core::ffi::c_char,
            9 as size_t,
            b"STGNUM%d\0".as_ptr() as *const ::core::ffi::c_char,
            i + 2 as ::core::ffi::c_int,
        );
        callback.expect("non-null function pointer")(
            &raw mut namebuf as *mut ::core::ffi::c_char,
            (&raw mut *(&raw mut arms as *mut [*mut patch_t; 2]).offset(i as isize)
                as *mut *mut patch_t)
                .offset(0 as ::core::ffi::c_int as isize) as *mut *mut patch_t,
        );
        arms[i as usize][1 as ::core::ffi::c_int as usize] =
            shortnum[(i + 2 as ::core::ffi::c_int) as usize];
        i += 1;
    }
    snprintf(
        &raw mut namebuf as *mut ::core::ffi::c_char,
        9 as size_t,
        b"STFB%d\0".as_ptr() as *const ::core::ffi::c_char,
        consoleplayer,
    );
    callback.expect("non-null function pointer")(
        &raw mut namebuf as *mut ::core::ffi::c_char,
        &raw mut faceback,
    );
    callback.expect("non-null function pointer")(
        b"STBAR\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut sbar,
    );
    facenum = 0 as ::core::ffi::c_int;
    i = 0 as ::core::ffi::c_int;
    while i < ST_NUMPAINFACES {
        j = 0 as ::core::ffi::c_int;
        while j < ST_NUMSTRAIGHTFACES {
            snprintf(
                &raw mut namebuf as *mut ::core::ffi::c_char,
                9 as size_t,
                b"STFST%d%d\0".as_ptr() as *const ::core::ffi::c_char,
                i,
                j,
            );
            callback.expect("non-null function pointer")(
                &raw mut namebuf as *mut ::core::ffi::c_char,
                (&raw mut faces as *mut *mut patch_t).offset(facenum as isize) as *mut *mut patch_t,
            );
            facenum += 1;
            j += 1;
        }
        snprintf(
            &raw mut namebuf as *mut ::core::ffi::c_char,
            9 as size_t,
            b"STFTR%d0\0".as_ptr() as *const ::core::ffi::c_char,
            i,
        );
        callback.expect("non-null function pointer")(
            &raw mut namebuf as *mut ::core::ffi::c_char,
            (&raw mut faces as *mut *mut patch_t).offset(facenum as isize) as *mut *mut patch_t,
        );
        facenum += 1;
        snprintf(
            &raw mut namebuf as *mut ::core::ffi::c_char,
            9 as size_t,
            b"STFTL%d0\0".as_ptr() as *const ::core::ffi::c_char,
            i,
        );
        callback.expect("non-null function pointer")(
            &raw mut namebuf as *mut ::core::ffi::c_char,
            (&raw mut faces as *mut *mut patch_t).offset(facenum as isize) as *mut *mut patch_t,
        );
        facenum += 1;
        snprintf(
            &raw mut namebuf as *mut ::core::ffi::c_char,
            9 as size_t,
            b"STFOUCH%d\0".as_ptr() as *const ::core::ffi::c_char,
            i,
        );
        callback.expect("non-null function pointer")(
            &raw mut namebuf as *mut ::core::ffi::c_char,
            (&raw mut faces as *mut *mut patch_t).offset(facenum as isize) as *mut *mut patch_t,
        );
        facenum += 1;
        snprintf(
            &raw mut namebuf as *mut ::core::ffi::c_char,
            9 as size_t,
            b"STFEVL%d\0".as_ptr() as *const ::core::ffi::c_char,
            i,
        );
        callback.expect("non-null function pointer")(
            &raw mut namebuf as *mut ::core::ffi::c_char,
            (&raw mut faces as *mut *mut patch_t).offset(facenum as isize) as *mut *mut patch_t,
        );
        facenum += 1;
        snprintf(
            &raw mut namebuf as *mut ::core::ffi::c_char,
            9 as size_t,
            b"STFKILL%d\0".as_ptr() as *const ::core::ffi::c_char,
            i,
        );
        callback.expect("non-null function pointer")(
            &raw mut namebuf as *mut ::core::ffi::c_char,
            (&raw mut faces as *mut *mut patch_t).offset(facenum as isize) as *mut *mut patch_t,
        );
        facenum += 1;
        i += 1;
    }
    callback.expect("non-null function pointer")(
        b"STFGOD0\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        (&raw mut faces as *mut *mut patch_t).offset(facenum as isize) as *mut *mut patch_t,
    );
    facenum += 1;
    callback.expect("non-null function pointer")(
        b"STFDEAD0\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        (&raw mut faces as *mut *mut patch_t).offset(facenum as isize) as *mut *mut patch_t,
    );
    facenum += 1;
}
unsafe extern "C" fn ST_loadCallback(
    mut lumpname: *mut ::core::ffi::c_char,
    mut variable: *mut *mut patch_t,
) {
    *variable = W_CacheLumpName(lumpname, PU_STATIC as ::core::ffi::c_int) as *mut patch_t;
}
#[no_mangle]
pub unsafe extern "C" fn ST_loadGraphics() {
    ST_loadUnloadGraphics(Some(
        ST_loadCallback as unsafe extern "C" fn(*mut ::core::ffi::c_char, *mut *mut patch_t) -> (),
    ));
}
#[no_mangle]
pub unsafe extern "C" fn ST_loadData() {
    lu_palette = W_GetNumForName(
        b"PLAYPAL\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char
    );
    ST_loadGraphics();
}
unsafe extern "C" fn ST_unloadCallback(
    mut lumpname: *mut ::core::ffi::c_char,
    mut variable: *mut *mut patch_t,
) {
    W_ReleaseLumpName(lumpname);
    *variable = ::core::ptr::null_mut::<patch_t>();
}
#[no_mangle]
pub unsafe extern "C" fn ST_unloadGraphics() {
    ST_loadUnloadGraphics(Some(
        ST_unloadCallback
            as unsafe extern "C" fn(*mut ::core::ffi::c_char, *mut *mut patch_t) -> (),
    ));
}
#[no_mangle]
pub unsafe extern "C" fn ST_unloadData() {
    ST_unloadGraphics();
}
#[no_mangle]
pub unsafe extern "C" fn ST_initData() {
    let mut i: ::core::ffi::c_int = 0;
    st_firsttime = true_0 as boolean;
    plyr = (&raw mut players as *mut player_t).offset(consoleplayer as isize) as *mut player_t;
    st_clock = 0 as ::core::ffi::c_uint;
    st_chatstate = StartChatState;
    st_gamestate = FirstPersonState;
    st_statusbaron = true_0 as boolean;
    st_chat = false_0 as boolean;
    st_oldchat = st_chat;
    st_cursoron = false_0 as boolean;
    st_faceindex = 0 as ::core::ffi::c_int;
    st_palette = -1 as ::core::ffi::c_int;
    st_oldhealth = -1 as ::core::ffi::c_int;
    i = 0 as ::core::ffi::c_int;
    while i < NUMWEAPONS as ::core::ffi::c_int {
        oldweaponsowned[i as usize] = (*plyr).weaponowned[i as usize];
        i += 1;
    }
    i = 0 as ::core::ffi::c_int;
    while i < 3 as ::core::ffi::c_int {
        keyboxes[i as usize] = -1 as ::core::ffi::c_int;
        i += 1;
    }
    STlib_init();
}
#[no_mangle]
pub unsafe extern "C" fn ST_createWidgets() {
    let mut i: ::core::ffi::c_int = 0;
    STlib_initNum(
        &raw mut w_ready,
        ST_AMMOX,
        ST_AMMOY,
        &raw mut tallnum as *mut *mut patch_t,
        (&raw mut (*plyr).ammo as *mut ::core::ffi::c_int).offset(
            (*(&raw mut weaponinfo as *mut weaponinfo_t).offset((*plyr).readyweapon as isize)).ammo
                as isize,
        ) as *mut ::core::ffi::c_int,
        &raw mut st_statusbaron,
        ST_AMMOWIDTH,
    );
    w_ready.data = (*plyr).readyweapon as ::core::ffi::c_int;
    STlib_initPercent(
        &raw mut w_health,
        ST_HEALTHX,
        ST_HEALTHY,
        &raw mut tallnum as *mut *mut patch_t,
        &raw mut (*plyr).health,
        &raw mut st_statusbaron,
        tallpercent,
    );
    STlib_initBinIcon(
        &raw mut w_armsbg,
        ST_ARMSBGX,
        ST_ARMSBGY,
        armsbg,
        &raw mut st_notdeathmatch,
        &raw mut st_statusbaron,
    );
    i = 0 as ::core::ffi::c_int;
    while i < 6 as ::core::ffi::c_int {
        STlib_initMultIcon(
            (&raw mut w_arms as *mut st_multicon_t).offset(i as isize) as *mut st_multicon_t,
            ST_ARMSX + i % 3 as ::core::ffi::c_int * ST_ARMSXSPACE,
            ST_ARMSY + i / 3 as ::core::ffi::c_int * ST_ARMSYSPACE,
            &raw mut *(&raw mut arms as *mut [*mut patch_t; 2]).offset(i as isize)
                as *mut *mut patch_t,
            (&raw mut (*plyr).weaponowned as *mut boolean)
                .offset((i + 1 as ::core::ffi::c_int) as isize) as *mut boolean
                as *mut ::core::ffi::c_int,
            &raw mut st_armson,
        );
        i += 1;
    }
    STlib_initNum(
        &raw mut w_frags,
        ST_FRAGSX,
        ST_FRAGSY,
        &raw mut tallnum as *mut *mut patch_t,
        &raw mut st_fragscount,
        &raw mut st_fragson,
        ST_FRAGSWIDTH,
    );
    STlib_initMultIcon(
        &raw mut w_faces,
        ST_FACESX,
        ST_FACESY,
        &raw mut faces as *mut *mut patch_t,
        &raw mut st_faceindex,
        &raw mut st_statusbaron,
    );
    STlib_initPercent(
        &raw mut w_armor,
        ST_ARMORX,
        ST_ARMORY,
        &raw mut tallnum as *mut *mut patch_t,
        &raw mut (*plyr).armorpoints,
        &raw mut st_statusbaron,
        tallpercent,
    );
    STlib_initMultIcon(
        (&raw mut w_keyboxes as *mut st_multicon_t).offset(0 as ::core::ffi::c_int as isize)
            as *mut st_multicon_t,
        ST_KEY0X,
        ST_KEY0Y,
        &raw mut keys as *mut *mut patch_t,
        (&raw mut keyboxes as *mut ::core::ffi::c_int).offset(0 as ::core::ffi::c_int as isize)
            as *mut ::core::ffi::c_int,
        &raw mut st_statusbaron,
    );
    STlib_initMultIcon(
        (&raw mut w_keyboxes as *mut st_multicon_t).offset(1 as ::core::ffi::c_int as isize)
            as *mut st_multicon_t,
        ST_KEY1X,
        ST_KEY1Y,
        &raw mut keys as *mut *mut patch_t,
        (&raw mut keyboxes as *mut ::core::ffi::c_int).offset(1 as ::core::ffi::c_int as isize)
            as *mut ::core::ffi::c_int,
        &raw mut st_statusbaron,
    );
    STlib_initMultIcon(
        (&raw mut w_keyboxes as *mut st_multicon_t).offset(2 as ::core::ffi::c_int as isize)
            as *mut st_multicon_t,
        ST_KEY2X,
        ST_KEY2Y,
        &raw mut keys as *mut *mut patch_t,
        (&raw mut keyboxes as *mut ::core::ffi::c_int).offset(2 as ::core::ffi::c_int as isize)
            as *mut ::core::ffi::c_int,
        &raw mut st_statusbaron,
    );
    STlib_initNum(
        (&raw mut w_ammo as *mut st_number_t).offset(0 as ::core::ffi::c_int as isize)
            as *mut st_number_t,
        ST_AMMO0X,
        ST_AMMO0Y,
        &raw mut shortnum as *mut *mut patch_t,
        (&raw mut (*plyr).ammo as *mut ::core::ffi::c_int).offset(0 as ::core::ffi::c_int as isize)
            as *mut ::core::ffi::c_int,
        &raw mut st_statusbaron,
        ST_AMMO0WIDTH,
    );
    STlib_initNum(
        (&raw mut w_ammo as *mut st_number_t).offset(1 as ::core::ffi::c_int as isize)
            as *mut st_number_t,
        ST_AMMO1X,
        ST_AMMO1Y,
        &raw mut shortnum as *mut *mut patch_t,
        (&raw mut (*plyr).ammo as *mut ::core::ffi::c_int).offset(1 as ::core::ffi::c_int as isize)
            as *mut ::core::ffi::c_int,
        &raw mut st_statusbaron,
        ST_AMMO1WIDTH,
    );
    STlib_initNum(
        (&raw mut w_ammo as *mut st_number_t).offset(2 as ::core::ffi::c_int as isize)
            as *mut st_number_t,
        ST_AMMO2X,
        ST_AMMO2Y,
        &raw mut shortnum as *mut *mut patch_t,
        (&raw mut (*plyr).ammo as *mut ::core::ffi::c_int).offset(2 as ::core::ffi::c_int as isize)
            as *mut ::core::ffi::c_int,
        &raw mut st_statusbaron,
        ST_AMMO2WIDTH,
    );
    STlib_initNum(
        (&raw mut w_ammo as *mut st_number_t).offset(3 as ::core::ffi::c_int as isize)
            as *mut st_number_t,
        ST_AMMO3X,
        ST_AMMO3Y,
        &raw mut shortnum as *mut *mut patch_t,
        (&raw mut (*plyr).ammo as *mut ::core::ffi::c_int).offset(3 as ::core::ffi::c_int as isize)
            as *mut ::core::ffi::c_int,
        &raw mut st_statusbaron,
        ST_AMMO3WIDTH,
    );
    STlib_initNum(
        (&raw mut w_maxammo as *mut st_number_t).offset(0 as ::core::ffi::c_int as isize)
            as *mut st_number_t,
        ST_MAXAMMO0X,
        ST_MAXAMMO0Y,
        &raw mut shortnum as *mut *mut patch_t,
        (&raw mut (*plyr).maxammo as *mut ::core::ffi::c_int)
            .offset(0 as ::core::ffi::c_int as isize) as *mut ::core::ffi::c_int,
        &raw mut st_statusbaron,
        ST_MAXAMMO0WIDTH,
    );
    STlib_initNum(
        (&raw mut w_maxammo as *mut st_number_t).offset(1 as ::core::ffi::c_int as isize)
            as *mut st_number_t,
        ST_MAXAMMO1X,
        ST_MAXAMMO1Y,
        &raw mut shortnum as *mut *mut patch_t,
        (&raw mut (*plyr).maxammo as *mut ::core::ffi::c_int)
            .offset(1 as ::core::ffi::c_int as isize) as *mut ::core::ffi::c_int,
        &raw mut st_statusbaron,
        ST_MAXAMMO1WIDTH,
    );
    STlib_initNum(
        (&raw mut w_maxammo as *mut st_number_t).offset(2 as ::core::ffi::c_int as isize)
            as *mut st_number_t,
        ST_MAXAMMO2X,
        ST_MAXAMMO2Y,
        &raw mut shortnum as *mut *mut patch_t,
        (&raw mut (*plyr).maxammo as *mut ::core::ffi::c_int)
            .offset(2 as ::core::ffi::c_int as isize) as *mut ::core::ffi::c_int,
        &raw mut st_statusbaron,
        ST_MAXAMMO2WIDTH,
    );
    STlib_initNum(
        (&raw mut w_maxammo as *mut st_number_t).offset(3 as ::core::ffi::c_int as isize)
            as *mut st_number_t,
        ST_MAXAMMO3X,
        ST_MAXAMMO3Y,
        &raw mut shortnum as *mut *mut patch_t,
        (&raw mut (*plyr).maxammo as *mut ::core::ffi::c_int)
            .offset(3 as ::core::ffi::c_int as isize) as *mut ::core::ffi::c_int,
        &raw mut st_statusbaron,
        ST_MAXAMMO3WIDTH,
    );
}
static mut st_stopped: boolean = true_0 as boolean;
#[no_mangle]
pub unsafe extern "C" fn ST_Start() {
    if st_stopped == 0 {
        ST_Stop();
    }
    ST_initData();
    ST_createWidgets();
    st_stopped = false_0 as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn ST_Stop() {
    if st_stopped != 0 {
        return;
    }
    I_SetPalette(W_CacheLumpNum(lu_palette, PU_CACHE as ::core::ffi::c_int) as *mut byte);
    st_stopped = true_0 as boolean;
}
#[no_mangle]
pub unsafe extern "C" fn ST_Init() {
    ST_loadData();
    st_backing_screen = Z_Malloc(
        ST_WIDTH * ST_HEIGHT,
        PU_STATIC as ::core::ffi::c_int,
        ::core::ptr::null_mut::<::core::ffi::c_void>(),
    ) as *mut byte;
}
unsafe extern "C" fn c2rust_run_static_initializers() {
    cheat_clev = cheatseq_t {
        sequence: ::core::mem::transmute::<[u8; 25], [::core::ffi::c_char; 25]>(
            *b"idclev\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
        ),
        sequence_len: (::core::mem::size_of::<[::core::ffi::c_char; 7]>() as size_t)
            .wrapping_sub(1 as size_t),
        parameter_chars: 2 as ::core::ffi::c_int,
        chars_read: 0 as size_t,
        param_chars_read: 0 as ::core::ffi::c_int,
        parameter_buf: ::core::mem::transmute::<[u8; 5], [::core::ffi::c_char; 5]>(*b"\0\0\0\0\0"),
    };
    cheat_mypos = cheatseq_t {
        sequence: ::core::mem::transmute::<[u8; 25], [::core::ffi::c_char; 25]>(
            *b"idmypos\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
        ),
        sequence_len: (::core::mem::size_of::<[::core::ffi::c_char; 8]>() as size_t)
            .wrapping_sub(1 as size_t),
        parameter_chars: 0 as ::core::ffi::c_int,
        chars_read: 0 as size_t,
        param_chars_read: 0 as ::core::ffi::c_int,
        parameter_buf: ::core::mem::transmute::<[u8; 5], [::core::ffi::c_char; 5]>(*b"\0\0\0\0\0"),
    };
    cheat_choppers = cheatseq_t {
        sequence: ::core::mem::transmute::<[u8; 25], [::core::ffi::c_char; 25]>(
            *b"idchoppers\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
        ),
        sequence_len: (::core::mem::size_of::<[::core::ffi::c_char; 11]>() as size_t)
            .wrapping_sub(1 as size_t),
        parameter_chars: 0 as ::core::ffi::c_int,
        chars_read: 0 as size_t,
        param_chars_read: 0 as ::core::ffi::c_int,
        parameter_buf: ::core::mem::transmute::<[u8; 5], [::core::ffi::c_char; 5]>(*b"\0\0\0\0\0"),
    };
    cheat_powerup = [
        cheatseq_t {
            sequence: ::core::mem::transmute::<[u8; 25], [::core::ffi::c_char; 25]>(
                *b"idbeholdv\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
            ),
            sequence_len: (::core::mem::size_of::<[::core::ffi::c_char; 10]>() as size_t)
                .wrapping_sub(1 as size_t),
            parameter_chars: 0 as ::core::ffi::c_int,
            chars_read: 0 as size_t,
            param_chars_read: 0 as ::core::ffi::c_int,
            parameter_buf: ::core::mem::transmute::<[u8; 5], [::core::ffi::c_char; 5]>(
                *b"\0\0\0\0\0",
            ),
        },
        cheatseq_t {
            sequence: ::core::mem::transmute::<[u8; 25], [::core::ffi::c_char; 25]>(
                *b"idbeholds\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
            ),
            sequence_len: (::core::mem::size_of::<[::core::ffi::c_char; 10]>() as size_t)
                .wrapping_sub(1 as size_t),
            parameter_chars: 0 as ::core::ffi::c_int,
            chars_read: 0 as size_t,
            param_chars_read: 0 as ::core::ffi::c_int,
            parameter_buf: ::core::mem::transmute::<[u8; 5], [::core::ffi::c_char; 5]>(
                *b"\0\0\0\0\0",
            ),
        },
        cheatseq_t {
            sequence: ::core::mem::transmute::<[u8; 25], [::core::ffi::c_char; 25]>(
                *b"idbeholdi\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
            ),
            sequence_len: (::core::mem::size_of::<[::core::ffi::c_char; 10]>() as size_t)
                .wrapping_sub(1 as size_t),
            parameter_chars: 0 as ::core::ffi::c_int,
            chars_read: 0 as size_t,
            param_chars_read: 0 as ::core::ffi::c_int,
            parameter_buf: ::core::mem::transmute::<[u8; 5], [::core::ffi::c_char; 5]>(
                *b"\0\0\0\0\0",
            ),
        },
        cheatseq_t {
            sequence: ::core::mem::transmute::<[u8; 25], [::core::ffi::c_char; 25]>(
                *b"idbeholdr\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
            ),
            sequence_len: (::core::mem::size_of::<[::core::ffi::c_char; 10]>() as size_t)
                .wrapping_sub(1 as size_t),
            parameter_chars: 0 as ::core::ffi::c_int,
            chars_read: 0 as size_t,
            param_chars_read: 0 as ::core::ffi::c_int,
            parameter_buf: ::core::mem::transmute::<[u8; 5], [::core::ffi::c_char; 5]>(
                *b"\0\0\0\0\0",
            ),
        },
        cheatseq_t {
            sequence: ::core::mem::transmute::<[u8; 25], [::core::ffi::c_char; 25]>(
                *b"idbeholda\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
            ),
            sequence_len: (::core::mem::size_of::<[::core::ffi::c_char; 10]>() as size_t)
                .wrapping_sub(1 as size_t),
            parameter_chars: 0 as ::core::ffi::c_int,
            chars_read: 0 as size_t,
            param_chars_read: 0 as ::core::ffi::c_int,
            parameter_buf: ::core::mem::transmute::<[u8; 5], [::core::ffi::c_char; 5]>(
                *b"\0\0\0\0\0",
            ),
        },
        cheatseq_t {
            sequence: ::core::mem::transmute::<[u8; 25], [::core::ffi::c_char; 25]>(
                *b"idbeholdl\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
            ),
            sequence_len: (::core::mem::size_of::<[::core::ffi::c_char; 10]>() as size_t)
                .wrapping_sub(1 as size_t),
            parameter_chars: 0 as ::core::ffi::c_int,
            chars_read: 0 as size_t,
            param_chars_read: 0 as ::core::ffi::c_int,
            parameter_buf: ::core::mem::transmute::<[u8; 5], [::core::ffi::c_char; 5]>(
                *b"\0\0\0\0\0",
            ),
        },
        cheatseq_t {
            sequence: ::core::mem::transmute::<[u8; 25], [::core::ffi::c_char; 25]>(
                *b"idbehold\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
            ),
            sequence_len: (::core::mem::size_of::<[::core::ffi::c_char; 9]>() as size_t)
                .wrapping_sub(1 as size_t),
            parameter_chars: 0 as ::core::ffi::c_int,
            chars_read: 0 as size_t,
            param_chars_read: 0 as ::core::ffi::c_int,
            parameter_buf: ::core::mem::transmute::<[u8; 5], [::core::ffi::c_char; 5]>(
                *b"\0\0\0\0\0",
            ),
        },
    ];
    cheat_commercial_noclip = cheatseq_t {
        sequence: ::core::mem::transmute::<[u8; 25], [::core::ffi::c_char; 25]>(
            *b"idclip\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
        ),
        sequence_len: (::core::mem::size_of::<[::core::ffi::c_char; 7]>() as size_t)
            .wrapping_sub(1 as size_t),
        parameter_chars: 0 as ::core::ffi::c_int,
        chars_read: 0 as size_t,
        param_chars_read: 0 as ::core::ffi::c_int,
        parameter_buf: ::core::mem::transmute::<[u8; 5], [::core::ffi::c_char; 5]>(*b"\0\0\0\0\0"),
    };
    cheat_noclip = cheatseq_t {
        sequence: ::core::mem::transmute::<[u8; 25], [::core::ffi::c_char; 25]>(
            *b"idspispopd\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
        ),
        sequence_len: (::core::mem::size_of::<[::core::ffi::c_char; 11]>() as size_t)
            .wrapping_sub(1 as size_t),
        parameter_chars: 0 as ::core::ffi::c_int,
        chars_read: 0 as size_t,
        param_chars_read: 0 as ::core::ffi::c_int,
        parameter_buf: ::core::mem::transmute::<[u8; 5], [::core::ffi::c_char; 5]>(*b"\0\0\0\0\0"),
    };
    cheat_mus = cheatseq_t {
        sequence: ::core::mem::transmute::<[u8; 25], [::core::ffi::c_char; 25]>(
            *b"idmus\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
        ),
        sequence_len: (::core::mem::size_of::<[::core::ffi::c_char; 6]>() as size_t)
            .wrapping_sub(1 as size_t),
        parameter_chars: 2 as ::core::ffi::c_int,
        chars_read: 0 as size_t,
        param_chars_read: 0 as ::core::ffi::c_int,
        parameter_buf: ::core::mem::transmute::<[u8; 5], [::core::ffi::c_char; 5]>(*b"\0\0\0\0\0"),
    };
    cheat_ammo = cheatseq_t {
        sequence: ::core::mem::transmute::<[u8; 25], [::core::ffi::c_char; 25]>(
            *b"idkfa\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
        ),
        sequence_len: (::core::mem::size_of::<[::core::ffi::c_char; 6]>() as size_t)
            .wrapping_sub(1 as size_t),
        parameter_chars: 0 as ::core::ffi::c_int,
        chars_read: 0 as size_t,
        param_chars_read: 0 as ::core::ffi::c_int,
        parameter_buf: ::core::mem::transmute::<[u8; 5], [::core::ffi::c_char; 5]>(*b"\0\0\0\0\0"),
    };
    cheat_ammonokey = cheatseq_t {
        sequence: ::core::mem::transmute::<[u8; 25], [::core::ffi::c_char; 25]>(
            *b"idfa\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
        ),
        sequence_len: (::core::mem::size_of::<[::core::ffi::c_char; 5]>() as size_t)
            .wrapping_sub(1 as size_t),
        parameter_chars: 0 as ::core::ffi::c_int,
        chars_read: 0 as size_t,
        param_chars_read: 0 as ::core::ffi::c_int,
        parameter_buf: ::core::mem::transmute::<[u8; 5], [::core::ffi::c_char; 5]>(*b"\0\0\0\0\0"),
    };
    cheat_god = cheatseq_t {
        sequence: ::core::mem::transmute::<[u8; 25], [::core::ffi::c_char; 25]>(
            *b"iddqd\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
        ),
        sequence_len: (::core::mem::size_of::<[::core::ffi::c_char; 6]>() as size_t)
            .wrapping_sub(1 as size_t),
        parameter_chars: 0 as ::core::ffi::c_int,
        chars_read: 0 as size_t,
        param_chars_read: 0 as ::core::ffi::c_int,
        parameter_buf: ::core::mem::transmute::<[u8; 5], [::core::ffi::c_char; 5]>(*b"\0\0\0\0\0"),
    };
}
#[used]
#[cfg_attr(target_os = "linux", link_section = ".init_array")]
#[cfg_attr(target_os = "windows", link_section = ".CRT$XIB")]
#[cfg_attr(target_os = "macos", link_section = "__DATA,__mod_init_func")]
static INIT_ARRAY: [unsafe extern "C" fn(); 1] = [c2rust_run_static_initializers];
