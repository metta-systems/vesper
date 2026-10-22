//! Rust port of `vendor/doomgeneric/sounds.c` plus the `sounds.h` enums.
//!
//! Holds the static SFX (`S_sfx`) and music (`S_music`) tables consumed by
//! `s_sound.rs` and the low-level audio drivers, plus the matching id enums
//! (`Sfx`, `Mus`). Layout of `SfxInfo`/`MusicInfo` exactly mirrors
//! `sfxinfo_t`/`musicinfo_t` so the tables can be handed to C-linkage code
//! unchanged.
//!
//! Sound lump names live in the WAD as `DS<short_name>` (SFX) and
//! `D_<short_name>` (music); only the short part is stored here. Each
//! per-name byte array is exposed as a `const` so it has a stable address
//! the table can take a pointer to.
//!
//! Rust-vs-C differences:
//! - Names are built with the `const fn name::<N>` helper at compile time,
//!   producing fixed-size `[c_char; N]` arrays instead of C string literals.
//! - The C source initialises every `sfxinfo_t` field by hand. Here, the
//!   `SfxInfo::new` / `with_pitch` / `with_volume` builders fill defaults
//!   matching `S_sfx[]` in `sounds.c` (pitch = -1, volume = -1, etc.).

#![allow(non_upper_case_globals, non_snake_case)]

use std::ffi::{c_char, c_int, c_void};

/// Rust mirror of `sfxinfo_t` (`sounds.h`).
///
/// The layout is locked to the C struct so the table can be passed to driver
/// code via `#[no_mangle] S_sfx`. `name` is a fixed 9-byte slot (8 chars +
/// NUL) holding the short lump name; the engine prepends `DS` when looking
/// up the actual WAD lump. `link` lets two effects share underlying audio
/// (e.g. `sfx_chgun -> sfx_pistol`).
#[repr(C)]
pub struct SfxInfo {
    /// Tag name pointer (DEH support); unused at runtime, always null.
    pub tagname: *mut c_char,
    /// Short lump name (no `DS` prefix), NUL-terminated.
    pub name: [c_char; 9],
    /// Channel priority; higher values evict lower ones in `S_GetChannel`.
    pub priority: c_int,
    /// Optional alias to another `SfxInfo` so several effects share audio.
    pub link: *mut SfxInfo,
    /// Pitch override (-1 = use sound default). Only meaningful when `link`.
    pub pitch: c_int,
    /// Volume modifier in 0-127 range, added when the effect aliases via `link`.
    pub volume: c_int,
    /// Soft reference count for the cached lump; decremented on `S_StopChannel`.
    pub usefulness: c_int,
    /// Cached lump number, or -1 before the lump is first loaded.
    pub lumpnum: c_int,
    /// Maximum simultaneous channels (-1 = unlimited).
    pub numchannels: c_int,
    /// Pointer to driver-side decoded audio data.
    pub driver_data: *mut c_void,
}

/// Rust mirror of `musicinfo_t` (`sounds.h`).
///
/// Layout-compatible with the C struct so `S_music` can be exposed via
/// `#[no_mangle]`. `name` is the short lump name (the engine prepends `d_`),
/// `data` holds the cached lump, `handle` is the driver-side song handle.
#[repr(C)]
#[derive(Default)]
pub struct MusicInfo {
    /// Short lump name (no `d_` prefix), NUL-terminated. Null for the sentinel.
    pub name: *mut c_char,
    /// Cached lump number; 0 means "not yet looked up".
    pub lumpnum: c_int,
    /// Pointer to the cached MIDI/MUS lump in zone memory.
    pub data: *mut c_void,
    /// Driver-side song handle returned by `I_RegisterSong`.
    pub handle: *mut c_void,
}

/// Number of `Sfx` ids, matching `NUMSFX` (109) in `sounds.h`.
// sfxenum_t in sounds.h ends with NUMSFX = 109
pub const NUMSFX: usize = Sfx::Radio as usize + 1;
/// Number of `Mus` ids, matching `NUMMUSIC` (68) in `sounds.h`.
// musicenum_t in sounds.h ends with NUMMUSIC = 68
pub const NUMMUSIC: usize = Mus::Dm2int as usize + 1;

const _: () = assert!(
    std::mem::size_of::<Sfx>() == std::mem::size_of::<std::ffi::c_int>(),
    "Sfx must be the same size as c_int"
);
const _: () = assert!(
    std::mem::size_of::<Mus>() == std::mem::size_of::<std::ffi::c_int>(),
    "Mus must be the same size as c_int"
);

/// Sound effect IDs, matching the `sfxenum_t` C enum in `sounds.h`.
#[doc(alias = "sfxenum_t")]
#[repr(C)]
#[derive(Default, PartialEq, Clone, Copy)]
pub enum Sfx {
    #[default]
    /// Sentinel "no sound" id.
    #[doc(alias = "sfx_None")]
    None = 0,
    /// Sound effect `sfx_pistol`.
    #[doc(alias = "sfx_pistol")]
    Pistol = 1,
    /// Sound effect `sfx_shotgn`.
    #[doc(alias = "sfx_shotgn")]
    Shotgn = 2,
    /// Sound effect `sfx_sgcock`.
    #[doc(alias = "sfx_sgcock")]
    Sgcock = 3,
    /// Sound effect `sfx_dshtgn`.
    #[doc(alias = "sfx_dshtgn")]
    Dshtgn = 4,
    /// Sound effect `sfx_dbopn`.
    #[doc(alias = "sfx_dbopn")]
    Dbopn = 5,
    /// Sound effect `sfx_dbcls`.
    #[doc(alias = "sfx_dbcls")]
    Dbcls = 6,
    /// Sound effect `sfx_dbload`.
    #[doc(alias = "sfx_dbload")]
    Dbload = 7,
    /// Sound effect `sfx_plasma`.
    #[doc(alias = "sfx_plasma")]
    Plasma = 8,
    /// Sound effect `sfx_bfg`.
    #[doc(alias = "sfx_bfg")]
    Bfg = 9,
    /// Sound effect `sfx_sawup`.
    #[doc(alias = "sfx_sawup")]
    Sawup = 10,
    /// Sound effect `sfx_sawidl`.
    #[doc(alias = "sfx_sawidl")]
    Sawidl = 11,
    /// Sound effect `sfx_sawful`.
    #[doc(alias = "sfx_sawful")]
    Sawful = 12,
    /// Sound effect `sfx_sawhit`.
    #[doc(alias = "sfx_sawhit")]
    Sawhit = 13,
    /// Sound effect `sfx_rlaunc`.
    #[doc(alias = "sfx_rlaunc")]
    Rlaunc = 14,
    /// Sound effect `sfx_rxplod`.
    #[doc(alias = "sfx_rxplod")]
    Rxplod = 15,
    /// Sound effect `sfx_firsht`.
    #[doc(alias = "sfx_firsht")]
    Firsht = 16,
    /// Sound effect `sfx_firxpl`.
    #[doc(alias = "sfx_firxpl")]
    Firxpl = 17,
    /// Sound effect `sfx_pstart`.
    #[doc(alias = "sfx_pstart")]
    Pstart = 18,
    /// Sound effect `sfx_pstop`.
    #[doc(alias = "sfx_pstop")]
    Pstop = 19,
    /// Sound effect `sfx_doropn`.
    #[doc(alias = "sfx_doropn")]
    Doropn = 20,
    /// Sound effect `sfx_dorcls`.
    #[doc(alias = "sfx_dorcls")]
    Dorcls = 21,
    /// Sound effect `sfx_stnmov`.
    #[doc(alias = "sfx_stnmov")]
    Stnmov = 22,
    /// Sound effect `sfx_swtchn`.
    #[doc(alias = "sfx_swtchn")]
    Swtchn = 23,
    /// Sound effect `sfx_swtchx`.
    #[doc(alias = "sfx_swtchx")]
    Swtchx = 24,
    /// Sound effect `sfx_plpain`.
    #[doc(alias = "sfx_plpain")]
    Plpain = 25,
    /// Sound effect `sfx_dmpain`.
    #[doc(alias = "sfx_dmpain")]
    Dmpain = 26,
    /// Sound effect `sfx_popain`.
    #[doc(alias = "sfx_popain")]
    Popain = 27,
    /// Sound effect `sfx_vipain`.
    #[doc(alias = "sfx_vipain")]
    Vipain = 28,
    /// Sound effect `sfx_mnpain`.
    #[doc(alias = "sfx_mnpain")]
    Mnpain = 29,
    /// Sound effect `sfx_pepain`.
    #[doc(alias = "sfx_pepain")]
    Pepain = 30,
    /// Sound effect `sfx_slop`.
    #[doc(alias = "sfx_slop")]
    Slop = 31,
    /// Sound effect `sfx_itemup`.
    #[doc(alias = "sfx_itemup")]
    Itemup = 32,
    /// Sound effect `sfx_wpnup`.
    #[doc(alias = "sfx_wpnup")]
    Wpnup = 33,
    /// Sound effect `sfx_oof`.
    #[doc(alias = "sfx_oof")]
    Oof = 34,
    /// Sound effect `sfx_telept`.
    #[doc(alias = "sfx_telept")]
    Telept = 35,
    /// Sound effect `sfx_posit1`.
    #[doc(alias = "sfx_posit1")]
    Posit1 = 36,
    /// Sound effect `sfx_posit2`.
    #[doc(alias = "sfx_posit2")]
    Posit2 = 37,
    /// Sound effect `sfx_posit3`.
    #[doc(alias = "sfx_posit3")]
    Posit3 = 38,
    /// Sound effect `sfx_bgsit1`.
    #[doc(alias = "sfx_bgsit1")]
    Bgsit1 = 39,
    /// Sound effect `sfx_bgsit2`.
    #[doc(alias = "sfx_bgsit2")]
    Bgsit2 = 40,
    /// Sound effect `sfx_sgtsit`.
    #[doc(alias = "sfx_sgtsit")]
    Sgtsit = 41,
    /// Sound effect `sfx_cacsit`.
    #[doc(alias = "sfx_cacsit")]
    Cacsit = 42,
    /// Sound effect `sfx_brssit`.
    #[doc(alias = "sfx_brssit")]
    Brssit = 43,
    /// Sound effect `sfx_cybsit`.
    #[doc(alias = "sfx_cybsit")]
    Cybsit = 44,
    /// Sound effect `sfx_spisit`.
    #[doc(alias = "sfx_spisit")]
    Spisit = 45,
    /// Sound effect `sfx_bspsit`.
    #[doc(alias = "sfx_bspsit")]
    Bspsit = 46,
    /// Sound effect `sfx_kntsit`.
    #[doc(alias = "sfx_kntsit")]
    Kntsit = 47,
    /// Sound effect `sfx_vilsit`.
    #[doc(alias = "sfx_vilsit")]
    Vilsit = 48,
    /// Sound effect `sfx_mansit`.
    #[doc(alias = "sfx_mansit")]
    Mansit = 49,
    /// Sound effect `sfx_pesit`.
    #[doc(alias = "sfx_pesit")]
    Pesit = 50,
    /// Sound effect `sfx_sklatk`.
    #[doc(alias = "sfx_sklatk")]
    Sklatk = 51,
    /// Sound effect `sfx_sgtatk`.
    #[doc(alias = "sfx_sgtatk")]
    Sgtatk = 52,
    /// Sound effect `sfx_skepch`.
    #[doc(alias = "sfx_skepch")]
    Skepch = 53,
    /// Sound effect `sfx_vilatk`.
    #[doc(alias = "sfx_vilatk")]
    Vilatk = 54,
    /// Sound effect `sfx_claw`.
    #[doc(alias = "sfx_claw")]
    Claw = 55,
    /// Sound effect `sfx_skeswg`.
    #[doc(alias = "sfx_skeswg")]
    Skeswg = 56,
    /// Sound effect `sfx_pldeth`.
    #[doc(alias = "sfx_pldeth")]
    Pldeth = 57,
    /// Sound effect `sfx_pdiehi`.
    #[doc(alias = "sfx_pdiehi")]
    Pdiehi = 58,
    /// Sound effect `sfx_podth1`.
    #[doc(alias = "sfx_podth1")]
    Podth1 = 59,
    /// Sound effect `sfx_podth2`.
    #[doc(alias = "sfx_podth2")]
    Podth2 = 60,
    /// Sound effect `sfx_podth3`.
    #[doc(alias = "sfx_podth3")]
    Podth3 = 61,
    /// Sound effect `sfx_bgdth1`.
    #[doc(alias = "sfx_bgdth1")]
    Bgdth1 = 62,
    /// Sound effect `sfx_bgdth2`.
    #[doc(alias = "sfx_bgdth2")]
    Bgdth2 = 63,
    /// Sound effect `sfx_sgtdth`.
    #[doc(alias = "sfx_sgtdth")]
    Sgtdth = 64,
    /// Sound effect `sfx_cacdth`.
    #[doc(alias = "sfx_cacdth")]
    Cacdth = 65,
    /// Sound effect `sfx_skldth`.
    #[doc(alias = "sfx_skldth")]
    Skldth = 66,
    /// Sound effect `sfx_brsdth`.
    #[doc(alias = "sfx_brsdth")]
    Brsdth = 67,
    /// Sound effect `sfx_cybdth`.
    #[doc(alias = "sfx_cybdth")]
    Cybdth = 68,
    /// Sound effect `sfx_spidth`.
    #[doc(alias = "sfx_spidth")]
    Spidth = 69,
    /// Sound effect `sfx_bspdth`.
    #[doc(alias = "sfx_bspdth")]
    Bspdth = 70,
    /// Sound effect `sfx_vildth`.
    #[doc(alias = "sfx_vildth")]
    Vildth = 71,
    /// Sound effect `sfx_kntdth`.
    #[doc(alias = "sfx_kntdth")]
    Kntdth = 72,
    /// Sound effect `sfx_pedth`.
    #[doc(alias = "sfx_pedth")]
    Pedth = 73,
    /// Sound effect `sfx_skedth`.
    #[doc(alias = "sfx_skedth")]
    Skedth = 74,
    /// Sound effect `sfx_posact`.
    #[doc(alias = "sfx_posact")]
    Posact = 75,
    /// Sound effect `sfx_bgact`.
    #[doc(alias = "sfx_bgact")]
    Bgact = 76,
    /// Sound effect `sfx_dmact`.
    #[doc(alias = "sfx_dmact")]
    Dmact = 77,
    /// Sound effect `sfx_bspact`.
    #[doc(alias = "sfx_bspact")]
    Bspact = 78,
    /// Sound effect `sfx_bspwlk`.
    #[doc(alias = "sfx_bspwlk")]
    Bspwlk = 79,
    /// Sound effect `sfx_vilact`.
    #[doc(alias = "sfx_vilact")]
    Vilact = 80,
    /// Sound effect `sfx_noway`.
    #[doc(alias = "sfx_noway")]
    Noway = 81,
    /// Sound effect `sfx_barexp`.
    #[doc(alias = "sfx_barexp")]
    Barexp = 82,
    /// Sound effect `sfx_punch`.
    #[doc(alias = "sfx_punch")]
    Punch = 83,
    /// Sound effect `sfx_hoof`.
    #[doc(alias = "sfx_hoof")]
    Hoof = 84,
    /// Sound effect `sfx_metal`.
    #[doc(alias = "sfx_metal")]
    Metal = 85,
    /// Sound effect `sfx_chgun`.
    #[doc(alias = "sfx_chgun")]
    Chgun = 86,
    /// Sound effect `sfx_tink`.
    #[doc(alias = "sfx_tink")]
    Tink = 87,
    /// Sound effect `sfx_bdopn`.
    #[doc(alias = "sfx_bdopn")]
    Bdopn = 88,
    /// Sound effect `sfx_bdcls`.
    #[doc(alias = "sfx_bdcls")]
    Bdcls = 89,
    /// Sound effect `sfx_itmbk`.
    #[doc(alias = "sfx_itmbk")]
    Itmbk = 90,
    /// Sound effect `sfx_flame`.
    #[doc(alias = "sfx_flame")]
    Flame = 91,
    /// Sound effect `sfx_flamst`.
    #[doc(alias = "sfx_flamst")]
    Flamst = 92,
    /// Sound effect `sfx_getpow`.
    #[doc(alias = "sfx_getpow")]
    Getpow = 93,
    /// Sound effect `sfx_bospit`.
    #[doc(alias = "sfx_bospit")]
    Bospit = 94,
    /// Sound effect `sfx_boscub`.
    #[doc(alias = "sfx_boscub")]
    Boscub = 95,
    /// Sound effect `sfx_bossit`.
    #[doc(alias = "sfx_bossit")]
    Bossit = 96,
    /// Sound effect `sfx_bospn`.
    #[doc(alias = "sfx_bospn")]
    Bospn = 97,
    /// Sound effect `sfx_bosdth`.
    #[doc(alias = "sfx_bosdth")]
    Bosdth = 98,
    /// Sound effect `sfx_manatk`.
    #[doc(alias = "sfx_manatk")]
    Manatk = 99,
    /// Sound effect `sfx_mandth`.
    #[doc(alias = "sfx_mandth")]
    Mandth = 100,
    /// Sound effect `sfx_sssit`.
    #[doc(alias = "sfx_sssit")]
    Sssit = 101,
    /// Sound effect `sfx_ssdth`.
    #[doc(alias = "sfx_ssdth")]
    Ssdth = 102,
    /// Sound effect `sfx_keenpn`.
    #[doc(alias = "sfx_keenpn")]
    Keenpn = 103,
    /// Sound effect `sfx_keendt`.
    #[doc(alias = "sfx_keendt")]
    Keendt = 104,
    /// Sound effect `sfx_skeact`.
    #[doc(alias = "sfx_skeact")]
    Skeact = 105,
    /// Sound effect `sfx_skesit`.
    #[doc(alias = "sfx_skesit")]
    Skesit = 106,
    /// Sound effect `sfx_skeatk`.
    #[doc(alias = "sfx_skeatk")]
    Skeatk = 107,
    /// Sound effect `sfx_radio`.
    #[doc(alias = "sfx_radio")]
    Radio = 108,
}

/// Music track IDs, matching the `musicenum_t` C enum in `sounds.h`.
#[doc(alias = "musicenum_t")]
#[repr(C)]
#[derive(Default, PartialEq, Clone, Copy)]
pub enum Mus {
    #[default]
    /// Sentinel "no music" id.
    #[doc(alias = "mus_None")]
    None = 0,
    /// Music track `mus_e1m1`.
    #[doc(alias = "mus_e1m1")]
    E1m1 = 1,
    /// Music track `mus_e1m2`.
    #[doc(alias = "mus_e1m2")]
    E1m2 = 2,
    /// Music track `mus_e1m3`.
    #[doc(alias = "mus_e1m3")]
    E1m3 = 3,
    /// Music track `mus_e1m4`.
    #[doc(alias = "mus_e1m4")]
    E1m4 = 4,
    /// Music track `mus_e1m5`.
    #[doc(alias = "mus_e1m5")]
    E1m5 = 5,
    /// Music track `mus_e1m6`.
    #[doc(alias = "mus_e1m6")]
    E1m6 = 6,
    /// Music track `mus_e1m7`.
    #[doc(alias = "mus_e1m7")]
    E1m7 = 7,
    /// Music track `mus_e1m8`.
    #[doc(alias = "mus_e1m8")]
    E1m8 = 8,
    /// Music track `mus_e1m9`.
    #[doc(alias = "mus_e1m9")]
    E1m9 = 9,
    /// Music track `mus_e2m1`.
    #[doc(alias = "mus_e2m1")]
    E2m1 = 10,
    /// Music track `mus_e2m2`.
    #[doc(alias = "mus_e2m2")]
    E2m2 = 11,
    /// Music track `mus_e2m3`.
    #[doc(alias = "mus_e2m3")]
    E2m3 = 12,
    /// Music track `mus_e2m4`.
    #[doc(alias = "mus_e2m4")]
    E2m4 = 13,
    /// Music track `mus_e2m5`.
    #[doc(alias = "mus_e2m5")]
    E2m5 = 14,
    /// Music track `mus_e2m6`.
    #[doc(alias = "mus_e2m6")]
    E2m6 = 15,
    /// Music track `mus_e2m7`.
    #[doc(alias = "mus_e2m7")]
    E2m7 = 16,
    /// Music track `mus_e2m8`.
    #[doc(alias = "mus_e2m8")]
    E2m8 = 17,
    /// Music track `mus_e2m9`.
    #[doc(alias = "mus_e2m9")]
    E2m9 = 18,
    /// Music track `mus_e3m1`.
    #[doc(alias = "mus_e3m1")]
    E3m1 = 19,
    /// Music track `mus_e3m2`.
    #[doc(alias = "mus_e3m2")]
    E3m2 = 20,
    /// Music track `mus_e3m3`.
    #[doc(alias = "mus_e3m3")]
    E3m3 = 21,
    /// Music track `mus_e3m4`.
    #[doc(alias = "mus_e3m4")]
    E3m4 = 22,
    /// Music track `mus_e3m5`.
    #[doc(alias = "mus_e3m5")]
    E3m5 = 23,
    /// Music track `mus_e3m6`.
    #[doc(alias = "mus_e3m6")]
    E3m6 = 24,
    /// Music track `mus_e3m7`.
    #[doc(alias = "mus_e3m7")]
    E3m7 = 25,
    /// Music track `mus_e3m8`.
    #[doc(alias = "mus_e3m8")]
    E3m8 = 26,
    /// Music track `mus_e3m9`.
    #[doc(alias = "mus_e3m9")]
    E3m9 = 27,
    /// Music track `mus_inter`.
    #[doc(alias = "mus_inter")]
    Inter = 28,
    /// Music track `mus_intro`.
    #[doc(alias = "mus_intro")]
    Intro = 29,
    /// Music track `mus_bunny`.
    #[doc(alias = "mus_bunny")]
    Bunny = 30,
    /// Music track `mus_victor`.
    #[doc(alias = "mus_victor")]
    Victor = 31,
    /// Music track `mus_introa`.
    #[doc(alias = "mus_introa")]
    Introa = 32,
    /// Music track `mus_runnin`.
    #[doc(alias = "mus_runnin")]
    Runnin = 33,
    /// Music track `mus_stalks`.
    #[doc(alias = "mus_stalks")]
    Stalks = 34,
    /// Music track `mus_countd`.
    #[doc(alias = "mus_countd")]
    Countd = 35,
    /// Music track `mus_betwee`.
    #[doc(alias = "mus_betwee")]
    Betwee = 36,
    /// Music track `mus_doom`.
    #[doc(alias = "mus_doom")]
    Doom = 37,
    /// Music track `mus_the_da`.
    #[doc(alias = "mus_the_da")]
    TheDa = 38,
    /// Music track `mus_shawn`.
    #[doc(alias = "mus_shawn")]
    Shawn = 39,
    /// Music track `mus_ddtblu`.
    #[doc(alias = "mus_ddtblu")]
    Ddtblu = 40,
    /// Music track `mus_in_cit`.
    #[doc(alias = "mus_in_cit")]
    InCit = 41,
    /// Music track `mus_dead`.
    #[doc(alias = "mus_dead")]
    Dead = 42,
    /// Music track `mus_stlks2`.
    #[doc(alias = "mus_stlks2")]
    Stlks2 = 43,
    /// Music track `mus_theda2`.
    #[doc(alias = "mus_theda2")]
    Theda2 = 44,
    /// Music track `mus_doom2`.
    #[doc(alias = "mus_doom2")]
    Doom2 = 45,
    /// Music track `mus_ddtbl2`.
    #[doc(alias = "mus_ddtbl2")]
    Ddtbl2 = 46,
    /// Music track `mus_runni2`.
    #[doc(alias = "mus_runni2")]
    Runni2 = 47,
    /// Music track `mus_dead2`.
    #[doc(alias = "mus_dead2")]
    Dead2 = 48,
    /// Music track `mus_stlks3`.
    #[doc(alias = "mus_stlks3")]
    Stlks3 = 49,
    /// Music track `mus_romero`.
    #[doc(alias = "mus_romero")]
    Romero = 50,
    /// Music track `mus_shawn2`.
    #[doc(alias = "mus_shawn2")]
    Shawn2 = 51,
    /// Music track `mus_messag`.
    #[doc(alias = "mus_messag")]
    Messag = 52,
    /// Music track `mus_count2`.
    #[doc(alias = "mus_count2")]
    Count2 = 53,
    /// Music track `mus_ddtbl3`.
    #[doc(alias = "mus_ddtbl3")]
    Ddtbl3 = 54,
    /// Music track `mus_ampie`.
    #[doc(alias = "mus_ampie")]
    Ampie = 55,
    /// Music track `mus_theda3`.
    #[doc(alias = "mus_theda3")]
    Theda3 = 56,
    /// Music track `mus_adrian`.
    #[doc(alias = "mus_adrian")]
    Adrian = 57,
    /// Music track `mus_messg2`.
    #[doc(alias = "mus_messg2")]
    Messg2 = 58,
    /// Music track `mus_romer2`.
    #[doc(alias = "mus_romer2")]
    Romer2 = 59,
    /// Music track `mus_tense`.
    #[doc(alias = "mus_tense")]
    Tense = 60,
    /// Music track `mus_shawn3`.
    #[doc(alias = "mus_shawn3")]
    Shawn3 = 61,
    /// Music track `mus_openin`.
    #[doc(alias = "mus_openin")]
    Openin = 62,
    /// Music track `mus_evil`.
    #[doc(alias = "mus_evil")]
    Evil = 63,
    /// Music track `mus_ultima`.
    #[doc(alias = "mus_ultima")]
    Ultima = 64,
    /// Music track `mus_read_m`.
    #[doc(alias = "mus_read_m")]
    ReadM = 65,
    /// Music track `mus_dm2ttl`.
    #[doc(alias = "mus_dm2ttl")]
    Dm2ttl = 66,
    /// Music track `mus_dm2int`.
    #[doc(alias = "mus_dm2int")]
    Dm2int = 67,
}

/// Manual `Sync` for `SfxInfo`: the static table is mutated only at startup
/// (or by single-threaded driver callbacks), so sharing the static across
/// threads is sound under the engine's threading model.
unsafe impl Sync for SfxInfo {}
/// Manual `Sync` for `MusicInfo`: same justification as `SfxInfo` -
/// `S_music` is treated as effectively immutable after `S_Init`.
unsafe impl Sync for MusicInfo {}

/// Constructors used to build the static `S_sfx` table at compile time.
impl SfxInfo {
    /// Build a default `SfxInfo` for the given short lump name and priority.
    /// Pitch and volume default to -1 (driver default), `numchannels` to -1
    /// (unlimited); other fields are null/zero. Matches the C
    /// `S_sfx[]` initialiser pattern in `sounds.c`.
    const fn new(name: [c_char; 9], priority: c_int) -> Self {
        SfxInfo {
            tagname: std::ptr::null_mut(),
            name,
            priority,
            link: std::ptr::null_mut(),
            pitch: -1,
            volume: -1,
            usefulness: 0,
            lumpnum: 0,
            numchannels: -1,
            driver_data: std::ptr::null_mut(),
        }
    }

    /// Builder helper: set the `pitch` override and return `self`.
    const fn with_pitch(mut self, pitch: c_int) -> Self {
        self.pitch = pitch;
        self
    }

    /// Builder helper: set the `volume` modifier and return `self`.
    const fn with_volume(mut self, volume: c_int) -> Self {
        self.volume = volume;
        self
    }
}

/// Constructors used to build the static `S_music` table at compile time.
impl MusicInfo {
    /// `MusicInfo` for the slot-zero sentinel ("no music"); all fields null.
    const fn none() -> Self {
        MusicInfo {
            name: std::ptr::null_mut(),
            lumpnum: 0,
            data: std::ptr::null_mut(),
            handle: std::ptr::null_mut(),
        }
    }

    /// `MusicInfo` pointing at the given const name buffer. The buffer must
    /// have static lifetime so the recorded pointer remains valid.
    const fn new<const N: usize>(name: &'static [c_char; N]) -> Self {
        MusicInfo {
            name: name.as_ptr() as *mut c_char,
            lumpnum: 0,
            data: std::ptr::null_mut(),
            handle: std::ptr::null_mut(),
        }
    }
}

/// Compile-time helper: turn a Rust `&str` into a fixed `[c_char; N]` buffer,
/// NUL-padded. Panics at const-eval if `s` would not fit (must leave room for
/// the trailing NUL).
const fn name<const N: usize>(s: &str) -> [c_char; N] {
    let b = s.as_bytes();
    assert!(b.len() < N, "String is too long for the array");
    let mut a = [0i8; N];
    let mut i = 0;
    while i < b.len() {
        a[i] = b[i] as c_char;
        i += 1;
    }
    a
}

/// Short lump-name buffer for SFX `DSNONE` (`sfx_none`).
const N_none: [c_char; 9] = name("none");
/// Short lump-name buffer for SFX `DSPISTOL` (`sfx_pistol`).
const N_pistol: [c_char; 9] = name("pistol");
/// Short lump-name buffer for SFX `DSSHOTGN` (`sfx_shotgn`).
const N_shotgn: [c_char; 9] = name("shotgn");
/// Short lump-name buffer for SFX `DSSGCOCK` (`sfx_sgcock`).
const N_sgcock: [c_char; 9] = name("sgcock");
/// Short lump-name buffer for SFX `DSDSHTGN` (`sfx_dshtgn`).
const N_dshtgn: [c_char; 9] = name("dshtgn");
/// Short lump-name buffer for SFX `DSDBOPN` (`sfx_dbopn`).
const N_dbopn: [c_char; 9] = name("dbopn");
/// Short lump-name buffer for SFX `DSDBCLS` (`sfx_dbcls`).
const N_dbcls: [c_char; 9] = name("dbcls");
/// Short lump-name buffer for SFX `DSDBLOAD` (`sfx_dbload`).
const N_dbload: [c_char; 9] = name("dbload");
/// Short lump-name buffer for SFX `DSPLASMA` (`sfx_plasma`).
const N_plasma: [c_char; 9] = name("plasma");
/// Short lump-name buffer for SFX `DSBFG` (`sfx_bfg`).
const N_bfg: [c_char; 9] = name("bfg");
/// Short lump-name buffer for SFX `DSSAWUP` (`sfx_sawup`).
const N_sawup: [c_char; 9] = name("sawup");
/// Short lump-name buffer for SFX `DSSAWIDL` (`sfx_sawidl`).
const N_sawidl: [c_char; 9] = name("sawidl");
/// Short lump-name buffer for SFX `DSSAWFUL` (`sfx_sawful`).
const N_sawful: [c_char; 9] = name("sawful");
/// Short lump-name buffer for SFX `DSSAWHIT` (`sfx_sawhit`).
const N_sawhit: [c_char; 9] = name("sawhit");
/// Short lump-name buffer for SFX `DSRLAUNC` (`sfx_rlaunc`).
const N_rlaunc: [c_char; 9] = name("rlaunc");
/// Short lump-name buffer for SFX `DSRXPLOD` (`sfx_rxplod`).
const N_rxplod: [c_char; 9] = name("rxplod");
/// Short lump-name buffer for SFX `DSFIRSHT` (`sfx_firsht`).
const N_firsht: [c_char; 9] = name("firsht");
/// Short lump-name buffer for SFX `DSFIRXPL` (`sfx_firxpl`).
const N_firxpl: [c_char; 9] = name("firxpl");
/// Short lump-name buffer for SFX `DSPSTART` (`sfx_pstart`).
const N_pstart: [c_char; 9] = name("pstart");
/// Short lump-name buffer for SFX `DSPSTOP` (`sfx_pstop`).
const N_pstop: [c_char; 9] = name("pstop");
/// Short lump-name buffer for SFX `DSDOROPN` (`sfx_doropn`).
const N_doropn: [c_char; 9] = name("doropn");
/// Short lump-name buffer for SFX `DSDORCLS` (`sfx_dorcls`).
const N_dorcls: [c_char; 9] = name("dorcls");
/// Short lump-name buffer for SFX `DSSTNMOV` (`sfx_stnmov`).
const N_stnmov: [c_char; 9] = name("stnmov");
/// Short lump-name buffer for SFX `DSSWTCHN` (`sfx_swtchn`).
const N_swtchn: [c_char; 9] = name("swtchn");
/// Short lump-name buffer for SFX `DSSWTCHX` (`sfx_swtchx`).
const N_swtchx: [c_char; 9] = name("swtchx");
/// Short lump-name buffer for SFX `DSPLPAIN` (`sfx_plpain`).
const N_plpain: [c_char; 9] = name("plpain");
/// Short lump-name buffer for SFX `DSDMPAIN` (`sfx_dmpain`).
const N_dmpain: [c_char; 9] = name("dmpain");
/// Short lump-name buffer for SFX `DSPOPAIN` (`sfx_popain`).
const N_popain: [c_char; 9] = name("popain");
/// Short lump-name buffer for SFX `DSVIPAIN` (`sfx_vipain`).
const N_vipain: [c_char; 9] = name("vipain");
/// Short lump-name buffer for SFX `DSMNPAIN` (`sfx_mnpain`).
const N_mnpain: [c_char; 9] = name("mnpain");
/// Short lump-name buffer for SFX `DSPEPAIN` (`sfx_pepain`).
const N_pepain: [c_char; 9] = name("pepain");
/// Short lump-name buffer for SFX `DSSLOP` (`sfx_slop`).
const N_slop: [c_char; 9] = name("slop");
/// Short lump-name buffer for SFX `DSITEMUP` (`sfx_itemup`).
const N_itemup: [c_char; 9] = name("itemup");
/// Short lump-name buffer for SFX `DSWPNUP` (`sfx_wpnup`).
const N_wpnup: [c_char; 9] = name("wpnup");
/// Short lump-name buffer for SFX `DSOOF` (`sfx_oof`).
const N_oof: [c_char; 9] = name("oof");
/// Short lump-name buffer for SFX `DSTELEPT` (`sfx_telept`).
const N_telept: [c_char; 9] = name("telept");
/// Short lump-name buffer for SFX `DSPOSIT1` (`sfx_posit1`).
const N_posit1: [c_char; 9] = name("posit1");
/// Short lump-name buffer for SFX `DSPOSIT2` (`sfx_posit2`).
const N_posit2: [c_char; 9] = name("posit2");
/// Short lump-name buffer for SFX `DSPOSIT3` (`sfx_posit3`).
const N_posit3: [c_char; 9] = name("posit3");
/// Short lump-name buffer for SFX `DSBGSIT1` (`sfx_bgsit1`).
const N_bgsit1: [c_char; 9] = name("bgsit1");
/// Short lump-name buffer for SFX `DSBGSIT2` (`sfx_bgsit2`).
const N_bgsit2: [c_char; 9] = name("bgsit2");
/// Short lump-name buffer for SFX `DSSGTSIT` (`sfx_sgtsit`).
const N_sgtsit: [c_char; 9] = name("sgtsit");
/// Short lump-name buffer for SFX `DSCACSIT` (`sfx_cacsit`).
const N_cacsit: [c_char; 9] = name("cacsit");
/// Short lump-name buffer for SFX `DSBRSSIT` (`sfx_brssit`).
const N_brssit: [c_char; 9] = name("brssit");
/// Short lump-name buffer for SFX `DSCYBSIT` (`sfx_cybsit`).
const N_cybsit: [c_char; 9] = name("cybsit");
/// Short lump-name buffer for SFX `DSSPISIT` (`sfx_spisit`).
const N_spisit: [c_char; 9] = name("spisit");
/// Short lump-name buffer for SFX `DSBSPSIT` (`sfx_bspit`).
const N_bspit: [c_char; 9] = name("bspsit");
/// Short lump-name buffer for SFX `DSKNTSIT` (`sfx_kntsit`).
const N_kntsit: [c_char; 9] = name("kntsit");
/// Short lump-name buffer for SFX `DSVILSIT` (`sfx_vilsit`).
const N_vilsit: [c_char; 9] = name("vilsit");
/// Short lump-name buffer for SFX `DSMANSIT` (`sfx_mansit`).
const N_mansit: [c_char; 9] = name("mansit");
/// Short lump-name buffer for SFX `DSPESIT` (`sfx_pesit`).
const N_pesit: [c_char; 9] = name("pesit");
/// Short lump-name buffer for SFX `DSSKLATK` (`sfx_sklatk`).
const N_sklatk: [c_char; 9] = name("sklatk");
/// Short lump-name buffer for SFX `DSSGTATK` (`sfx_sgtatk`).
const N_sgtatk: [c_char; 9] = name("sgtatk");
/// Short lump-name buffer for SFX `DSSKEPCH` (`sfx_skepch`).
const N_skepch: [c_char; 9] = name("skepch");
/// Short lump-name buffer for SFX `DSVILATK` (`sfx_vilatk`).
const N_vilatk: [c_char; 9] = name("vilatk");
/// Short lump-name buffer for SFX `DSCLAW` (`sfx_claw`).
const N_claw: [c_char; 9] = name("claw");
/// Short lump-name buffer for SFX `DSSKESWG` (`sfx_skeswg`).
const N_skeswg: [c_char; 9] = name("skeswg");
/// Short lump-name buffer for SFX `DSPLDETH` (`sfx_pldeth`).
const N_pldeth: [c_char; 9] = name("pldeth");
/// Short lump-name buffer for SFX `DSPDIEHI` (`sfx_pdiehi`).
const N_pdiehi: [c_char; 9] = name("pdiehi");
/// Short lump-name buffer for SFX `DSPODTH1` (`sfx_podth1`).
const N_podth1: [c_char; 9] = name("podth1");
/// Short lump-name buffer for SFX `DSPODTH2` (`sfx_podth2`).
const N_podth2: [c_char; 9] = name("podth2");
/// Short lump-name buffer for SFX `DSPODTH3` (`sfx_podth3`).
const N_podth3: [c_char; 9] = name("podth3");
/// Short lump-name buffer for SFX `DSBGDTH1` (`sfx_bgdth1`).
const N_bgdth1: [c_char; 9] = name("bgdth1");
/// Short lump-name buffer for SFX `DSBGDTH2` (`sfx_bgdth2`).
const N_bgdth2: [c_char; 9] = name("bgdth2");
/// Short lump-name buffer for SFX `DSSGTDTH` (`sfx_sgtdth`).
const N_sgtdth: [c_char; 9] = name("sgtdth");
/// Short lump-name buffer for SFX `DSCACDTH` (`sfx_cacdth`).
const N_cacdth: [c_char; 9] = name("cacdth");
/// Short lump-name buffer for SFX `DSSKLDTH` (`sfx_skldth`).
const N_skldth: [c_char; 9] = name("skldth");
/// Short lump-name buffer for SFX `DSBRSDTH` (`sfx_brsdth`).
const N_brsdth: [c_char; 9] = name("brsdth");
/// Short lump-name buffer for SFX `DSCYBDTH` (`sfx_cybdth`).
const N_cybdth: [c_char; 9] = name("cybdth");
/// Short lump-name buffer for SFX `DSSPIDTH` (`sfx_spidth`).
const N_spidth: [c_char; 9] = name("spidth");
/// Short lump-name buffer for SFX `DSBSPDTH` (`sfx_bspdth`).
const N_bspdth: [c_char; 9] = name("bspdth");
/// Short lump-name buffer for SFX `DSVILDTH` (`sfx_vildth`).
const N_vildth: [c_char; 9] = name("vildth");
/// Short lump-name buffer for SFX `DSKNTDTH` (`sfx_kntdth`).
const N_kntdth: [c_char; 9] = name("kntdth");
/// Short lump-name buffer for SFX `DSPEDTH` (`sfx_pedth`).
const N_pedth: [c_char; 9] = name("pedth");
/// Short lump-name buffer for SFX `DSSKEDTH` (`sfx_skedth`).
const N_skedth: [c_char; 9] = name("skedth");
/// Short lump-name buffer for SFX `DSPOSACT` (`sfx_posact`).
const N_posact: [c_char; 9] = name("posact");
/// Short lump-name buffer for SFX `DSBGACT` (`sfx_bgact`).
const N_bgact: [c_char; 9] = name("bgact");
/// Short lump-name buffer for SFX `DSDMACT` (`sfx_dmact`).
const N_dmact: [c_char; 9] = name("dmact");
/// Short lump-name buffer for SFX `DSBSPACT` (`sfx_bspact`).
const N_bspact: [c_char; 9] = name("bspact");
/// Short lump-name buffer for SFX `DSBSPWLK` (`sfx_bspwlk`).
const N_bspwlk: [c_char; 9] = name("bspwlk");
/// Short lump-name buffer for SFX `DSVILACT` (`sfx_vilact`).
const N_vilact: [c_char; 9] = name("vilact");
/// Short lump-name buffer for SFX `DSNOWAY` (`sfx_noway`).
const N_noway: [c_char; 9] = name("noway");
/// Short lump-name buffer for SFX `DSBAREXP` (`sfx_barexp`).
const N_barexp: [c_char; 9] = name("barexp");
/// Short lump-name buffer for SFX `DSPUNCH` (`sfx_punch`).
const N_punch: [c_char; 9] = name("punch");
/// Short lump-name buffer for SFX `DSHOOF` (`sfx_hoof`).
const N_hoof: [c_char; 9] = name("hoof");
/// Short lump-name buffer for SFX `DSMETAL` (`sfx_metal`).
const N_metal: [c_char; 9] = name("metal");
/// Short lump-name buffer for SFX `DSCHGUN` (`sfx_chgun`).
const N_chgun: [c_char; 9] = name("chgun");
/// Short lump-name buffer for SFX `DSTINK` (`sfx_tink`).
const N_tink: [c_char; 9] = name("tink");
/// Short lump-name buffer for SFX `DSBDOPN` (`sfx_bdopn`).
const N_bdopn: [c_char; 9] = name("bdopn");
/// Short lump-name buffer for SFX `DSBDCLS` (`sfx_bdcls`).
const N_bdcls: [c_char; 9] = name("bdcls");
/// Short lump-name buffer for SFX `DSITMBK` (`sfx_itmbk`).
const N_itmbk: [c_char; 9] = name("itmbk");
/// Short lump-name buffer for SFX `DSFLAME` (`sfx_flame`).
const N_flame: [c_char; 9] = name("flame");
/// Short lump-name buffer for SFX `DSFLAMST` (`sfx_flamst`).
const N_flamst: [c_char; 9] = name("flamst");
/// Short lump-name buffer for SFX `DSGETPOW` (`sfx_getpow`).
const N_getpow: [c_char; 9] = name("getpow");
/// Short lump-name buffer for SFX `DSBOSPIT` (`sfx_bospit`).
const N_bospit: [c_char; 9] = name("bospit");
/// Short lump-name buffer for SFX `DSBOSCUB` (`sfx_boscub`).
const N_boscub: [c_char; 9] = name("boscub");
/// Short lump-name buffer for SFX `DSBOSSIT` (`sfx_bossit`).
const N_bossit: [c_char; 9] = name("bossit");
/// Short lump-name buffer for SFX `DSBOSPN` (`sfx_bospn`).
const N_bospn: [c_char; 9] = name("bospn");
/// Short lump-name buffer for SFX `DSBOSDTH` (`sfx_bosdth`).
const N_bosdth: [c_char; 9] = name("bosdth");
/// Short lump-name buffer for SFX `DSMANATK` (`sfx_manatk`).
const N_manatk: [c_char; 9] = name("manatk");
/// Short lump-name buffer for SFX `DSMANDTH` (`sfx_mandth`).
const N_mandth: [c_char; 9] = name("mandth");
/// Short lump-name buffer for SFX `DSSSSIT` (`sfx_sssit`).
const N_sssit: [c_char; 9] = name("sssit");
/// Short lump-name buffer for SFX `DSSSDTH` (`sfx_ssdth`).
const N_ssdth: [c_char; 9] = name("ssdth");
/// Short lump-name buffer for SFX `DSKEENPN` (`sfx_keenpn`).
const N_keenpn: [c_char; 9] = name("keenpn");
/// Short lump-name buffer for SFX `DSKEENDT` (`sfx_keendt`).
const N_keendt: [c_char; 9] = name("keendt");
/// Short lump-name buffer for SFX `DSSKEACT` (`sfx_skeact`).
const N_skeact: [c_char; 9] = name("skeact");
/// Short lump-name buffer for SFX `DSSKESIT` (`sfx_skesit`).
const N_skesit: [c_char; 9] = name("skesit");
/// Short lump-name buffer for SFX `DSSKEATK` (`sfx_skeatk`).
const N_skeatk: [c_char; 9] = name("skeatk");
/// Short lump-name buffer for SFX `DSRADIO` (`sfx_radio`).
const N_radio: [c_char; 9] = name("radio");

/// Short lump-name buffer for music `D_E1M1` (`mus_e1m1`).
const MUS_e1m1: [c_char; 5] = name("e1m1");
/// Short lump-name buffer for music `D_E1M2` (`mus_e1m2`).
const MUS_e1m2: [c_char; 5] = name("e1m2");
/// Short lump-name buffer for music `D_E1M3` (`mus_e1m3`).
const MUS_e1m3: [c_char; 5] = name("e1m3");
/// Short lump-name buffer for music `D_E1M4` (`mus_e1m4`).
const MUS_e1m4: [c_char; 5] = name("e1m4");
/// Short lump-name buffer for music `D_E1M5` (`mus_e1m5`).
const MUS_e1m5: [c_char; 5] = name("e1m5");
/// Short lump-name buffer for music `D_E1M6` (`mus_e1m6`).
const MUS_e1m6: [c_char; 5] = name("e1m6");
/// Short lump-name buffer for music `D_E1M7` (`mus_e1m7`).
const MUS_e1m7: [c_char; 5] = name("e1m7");
/// Short lump-name buffer for music `D_E1M8` (`mus_e1m8`).
const MUS_e1m8: [c_char; 5] = name("e1m8");
/// Short lump-name buffer for music `D_E1M9` (`mus_e1m9`).
const MUS_e1m9: [c_char; 5] = name("e1m9");
/// Short lump-name buffer for music `D_E2M1` (`mus_e2m1`).
const MUS_e2m1: [c_char; 5] = name("e2m1");
/// Short lump-name buffer for music `D_E2M2` (`mus_e2m2`).
const MUS_e2m2: [c_char; 5] = name("e2m2");
/// Short lump-name buffer for music `D_E2M3` (`mus_e2m3`).
const MUS_e2m3: [c_char; 5] = name("e2m3");
/// Short lump-name buffer for music `D_E2M4` (`mus_e2m4`).
const MUS_e2m4: [c_char; 5] = name("e2m4");
/// Short lump-name buffer for music `D_E2M5` (`mus_e2m5`).
const MUS_e2m5: [c_char; 5] = name("e2m5");
/// Short lump-name buffer for music `D_E2M6` (`mus_e2m6`).
const MUS_e2m6: [c_char; 5] = name("e2m6");
/// Short lump-name buffer for music `D_E2M7` (`mus_e2m7`).
const MUS_e2m7: [c_char; 5] = name("e2m7");
/// Short lump-name buffer for music `D_E2M8` (`mus_e2m8`).
const MUS_e2m8: [c_char; 5] = name("e2m8");
/// Short lump-name buffer for music `D_E2M9` (`mus_e2m9`).
const MUS_e2m9: [c_char; 5] = name("e2m9");
/// Short lump-name buffer for music `D_E3M1` (`mus_e3m1`).
const MUS_e3m1: [c_char; 5] = name("e3m1");
/// Short lump-name buffer for music `D_E3M2` (`mus_e3m2`).
const MUS_e3m2: [c_char; 5] = name("e3m2");
/// Short lump-name buffer for music `D_E3M3` (`mus_e3m3`).
const MUS_e3m3: [c_char; 5] = name("e3m3");
/// Short lump-name buffer for music `D_E3M4` (`mus_e3m4`).
const MUS_e3m4: [c_char; 5] = name("e3m4");
/// Short lump-name buffer for music `D_E3M5` (`mus_e3m5`).
const MUS_e3m5: [c_char; 5] = name("e3m5");
/// Short lump-name buffer for music `D_E3M6` (`mus_e3m6`).
const MUS_e3m6: [c_char; 5] = name("e3m6");
/// Short lump-name buffer for music `D_E3M7` (`mus_e3m7`).
const MUS_e3m7: [c_char; 5] = name("e3m7");
/// Short lump-name buffer for music `D_E3M8` (`mus_e3m8`).
const MUS_e3m8: [c_char; 5] = name("e3m8");
/// Short lump-name buffer for music `D_E3M9` (`mus_e3m9`).
const MUS_e3m9: [c_char; 5] = name("e3m9");
/// Short lump-name buffer for music `D_INTER` (`mus_inter`).
const MUS_inter: [c_char; 6] = name("inter");
/// Short lump-name buffer for music `D_INTRO` (`mus_intro`).
const MUS_intro: [c_char; 6] = name("intro");
/// Short lump-name buffer for music `D_BUNNY` (`mus_bunny`).
const MUS_bunny: [c_char; 6] = name("bunny");
/// Short lump-name buffer for music `D_VICTOR` (`mus_victor`).
const MUS_victor: [c_char; 7] = name("victor");
/// Short lump-name buffer for music `D_INTROA` (`mus_introa`).
const MUS_introa: [c_char; 7] = name("introa");
/// Short lump-name buffer for music `D_RUNNIN` (`mus_runnin`).
const MUS_runnin: [c_char; 7] = name("runnin");
/// Short lump-name buffer for music `D_STALKS` (`mus_stalks`).
const MUS_stalks: [c_char; 7] = name("stalks");
/// Short lump-name buffer for music `D_COUNTD` (`mus_countd`).
const MUS_countd: [c_char; 7] = name("countd");
/// Short lump-name buffer for music `D_BETWEE` (`mus_betwee`).
const MUS_betwee: [c_char; 7] = name("betwee");
/// Short lump-name buffer for music `D_DOOM` (`mus_doom`).
const MUS_doom: [c_char; 5] = name("doom");
/// Short lump-name buffer for music `D_THE_DA` (`mus_the_da`).
const MUS_the_da: [c_char; 7] = name("the_da");
/// Short lump-name buffer for music `D_SHAWN` (`mus_shawn`).
const MUS_shawn: [c_char; 6] = name("shawn");
/// Short lump-name buffer for music `D_DDTBLU` (`mus_ddtblu`).
const MUS_ddtblu: [c_char; 7] = name("ddtblu");
/// Short lump-name buffer for music `D_IN_CIT` (`mus_in_cit`).
const MUS_in_cit: [c_char; 7] = name("in_cit");
/// Short lump-name buffer for music `D_DEAD` (`mus_dead`).
const MUS_dead: [c_char; 5] = name("dead");
/// Short lump-name buffer for music `D_STLKS2` (`mus_stlks2`).
const MUS_stlks2: [c_char; 7] = name("stlks2");
/// Short lump-name buffer for music `D_THEDA2` (`mus_theda2`).
const MUS_theda2: [c_char; 7] = name("theda2");
/// Short lump-name buffer for music `D_DOOM2` (`mus_doom2`).
const MUS_doom2: [c_char; 6] = name("doom2");
/// Short lump-name buffer for music `D_DDTBL2` (`mus_ddtbl2`).
const MUS_ddtbl2: [c_char; 7] = name("ddtbl2");
/// Short lump-name buffer for music `D_RUNNI2` (`mus_runni2`).
const MUS_runni2: [c_char; 7] = name("runni2");
/// Short lump-name buffer for music `D_DEAD2` (`mus_dead2`).
const MUS_dead2: [c_char; 6] = name("dead2");
/// Short lump-name buffer for music `D_STLKS3` (`mus_stlks3`).
const MUS_stlks3: [c_char; 7] = name("stlks3");
/// Short lump-name buffer for music `D_ROMERO` (`mus_romero`).
const MUS_romero: [c_char; 7] = name("romero");
/// Short lump-name buffer for music `D_SHAWN2` (`mus_shawn2`).
const MUS_shawn2: [c_char; 7] = name("shawn2");
/// Short lump-name buffer for music `D_MESSAG` (`mus_messag`).
const MUS_messag: [c_char; 7] = name("messag");
/// Short lump-name buffer for music `D_COUNT2` (`mus_count2`).
const MUS_count2: [c_char; 7] = name("count2");
/// Short lump-name buffer for music `D_DDTBL3` (`mus_ddtbl3`).
const MUS_ddtbl3: [c_char; 7] = name("ddtbl3");
/// Short lump-name buffer for music `D_AMPIE` (`mus_ampie`).
const MUS_ampie: [c_char; 6] = name("ampie");
/// Short lump-name buffer for music `D_THEDA3` (`mus_theda3`).
const MUS_theda3: [c_char; 7] = name("theda3");
/// Short lump-name buffer for music `D_ADRIAN` (`mus_adrian`).
const MUS_adrian: [c_char; 7] = name("adrian");
/// Short lump-name buffer for music `D_MESSG2` (`mus_messg2`).
const MUS_messg2: [c_char; 7] = name("messg2");
/// Short lump-name buffer for music `D_ROMER2` (`mus_romer2`).
const MUS_romer2: [c_char; 7] = name("romer2");
/// Short lump-name buffer for music `D_TENSE` (`mus_tense`).
const MUS_tense: [c_char; 6] = name("tense");
/// Short lump-name buffer for music `D_SHAWN3` (`mus_shawn3`).
const MUS_shawn3: [c_char; 7] = name("shawn3");
/// Short lump-name buffer for music `D_OPENIN` (`mus_openin`).
const MUS_openin: [c_char; 7] = name("openin");
/// Short lump-name buffer for music `D_EVIL` (`mus_evil`).
const MUS_evil: [c_char; 5] = name("evil");
/// Short lump-name buffer for music `D_ULTIMA` (`mus_ultima`).
const MUS_ultima: [c_char; 7] = name("ultima");
/// Short lump-name buffer for music `D_READ_M` (`mus_read_m`).
const MUS_read_m: [c_char; 7] = name("read_m");
/// Short lump-name buffer for music `D_DM2TTL` (`mus_dm2ttl`).
const MUS_dm2ttl: [c_char; 7] = name("dm2ttl");
/// Short lump-name buffer for music `D_DM2INT` (`mus_dm2int`).
const MUS_dm2int: [c_char; 7] = name("dm2int");

/// Master SFX table indexed by `Sfx` ids. Mirrors `S_sfx[]` in `sounds.c`.
///
/// Built at compile time from the `N_*` short-name buffers and per-entry
/// priorities. Entry 0 is a "none" sentinel. `usefulness` is reset to -1 for
/// every non-sentinel entry by `S_Init`. Linked entries (currently only
/// `sfx_chgun -> sfx_pistol`) are wired up at runtime by `S_InitSfxLinks`.
///
/// `#[no_mangle]` so it is reachable from any remaining C-linkage callers
/// (the test harness verifies layout against the C `sizeof`).
#[no_mangle]
pub static mut S_sfx: [SfxInfo; NUMSFX] = [
    // [0] sfx_none
    SfxInfo::new(N_none, 0),
    // [1] sfx_pistol
    SfxInfo::new(N_pistol, 64),
    // [2] sfx_shotgn
    SfxInfo::new(N_shotgn, 64),
    // [3] sfx_sgcock
    SfxInfo::new(N_sgcock, 64),
    // [4] sfx_dshtgn
    SfxInfo::new(N_dshtgn, 64),
    // [5] sfx_dbopn
    SfxInfo::new(N_dbopn, 64),
    // [6] sfx_dbcls
    SfxInfo::new(N_dbcls, 64),
    // [7] sfx_dbload
    SfxInfo::new(N_dbload, 64),
    // [8] sfx_plasma
    SfxInfo::new(N_plasma, 64),
    // [9] sfx_bfg
    SfxInfo::new(N_bfg, 64),
    // [10] sfx_sawup
    SfxInfo::new(N_sawup, 64),
    // [11] sfx_sawidl
    SfxInfo::new(N_sawidl, 118),
    // [12] sfx_sawful
    SfxInfo::new(N_sawful, 64),
    // [13] sfx_sawhit
    SfxInfo::new(N_sawhit, 64),
    // [14] sfx_rlaunc
    SfxInfo::new(N_rlaunc, 64),
    // [15] sfx_rxplod
    SfxInfo::new(N_rxplod, 70),
    // [16] sfx_firsht
    SfxInfo::new(N_firsht, 70),
    // [17] sfx_firxpl
    SfxInfo::new(N_firxpl, 70),
    // [18] sfx_pstart
    SfxInfo::new(N_pstart, 100),
    // [19] sfx_pstop
    SfxInfo::new(N_pstop, 100),
    // [20] sfx_doropn
    SfxInfo::new(N_doropn, 100),
    // [21] sfx_dorcls
    SfxInfo::new(N_dorcls, 100),
    // [22] sfx_stnmov
    SfxInfo::new(N_stnmov, 119),
    // [23] sfx_swtchn
    SfxInfo::new(N_swtchn, 78),
    // [24] sfx_swtchx
    SfxInfo::new(N_swtchx, 78),
    // [25] sfx_plpain
    SfxInfo::new(N_plpain, 96),
    // [26] sfx_dmpain
    SfxInfo::new(N_dmpain, 96),
    // [27] sfx_popain
    SfxInfo::new(N_popain, 96),
    // [28] sfx_vipain
    SfxInfo::new(N_vipain, 96),
    // [29] sfx_mnpain
    SfxInfo::new(N_mnpain, 96),
    // [30] sfx_pepain
    SfxInfo::new(N_pepain, 96),
    // [31] sfx_slop
    SfxInfo::new(N_slop, 78),
    // [32] sfx_itemup
    SfxInfo::new(N_itemup, 78),
    // [33] sfx_wpnup
    SfxInfo::new(N_wpnup, 78),
    // [34] sfx_oof
    SfxInfo::new(N_oof, 96),
    // [35] sfx_telept
    SfxInfo::new(N_telept, 32),
    // [36] sfx_posit1
    SfxInfo::new(N_posit1, 98),
    // [37] sfx_posit2
    SfxInfo::new(N_posit2, 98),
    // [38] sfx_posit3
    SfxInfo::new(N_posit3, 98),
    // [39] sfx_bgsit1
    SfxInfo::new(N_bgsit1, 98),
    // [40] sfx_bgsit2
    SfxInfo::new(N_bgsit2, 98),
    // [41] sfx_sgtsit
    SfxInfo::new(N_sgtsit, 98),
    // [42] sfx_cacsit
    SfxInfo::new(N_cacsit, 98),
    // [43] sfx_brssit
    SfxInfo::new(N_brssit, 94),
    // [44] sfx_cybsit
    SfxInfo::new(N_cybsit, 92),
    // [45] sfx_spisit
    SfxInfo::new(N_spisit, 90),
    // [46] sfx_bspit
    SfxInfo::new(N_bspit, 90),
    // [47] sfx_kntsit
    SfxInfo::new(N_kntsit, 90),
    // [48] sfx_vilsit
    SfxInfo::new(N_vilsit, 90),
    // [49] sfx_mansit
    SfxInfo::new(N_mansit, 90),
    // [50] sfx_pesit
    SfxInfo::new(N_pesit, 90),
    // [51] sfx_sklatk
    SfxInfo::new(N_sklatk, 70),
    // [52] sfx_sgtatk
    SfxInfo::new(N_sgtatk, 70),
    // [53] sfx_skepch
    SfxInfo::new(N_skepch, 70),
    // [54] sfx_vilatk
    SfxInfo::new(N_vilatk, 70),
    // [55] sfx_claw
    SfxInfo::new(N_claw, 70),
    // [56] sfx_skeswg
    SfxInfo::new(N_skeswg, 70),
    // [57] sfx_pldeth
    SfxInfo::new(N_pldeth, 32),
    // [58] sfx_pdiehi
    SfxInfo::new(N_pdiehi, 32),
    // [59] sfx_podth1
    SfxInfo::new(N_podth1, 70),
    // [60] sfx_podth2
    SfxInfo::new(N_podth2, 70),
    // [61] sfx_podth3
    SfxInfo::new(N_podth3, 70),
    // [62] sfx_bgdth1
    SfxInfo::new(N_bgdth1, 70),
    // [63] sfx_bgdth2
    SfxInfo::new(N_bgdth2, 70),
    // [64] sfx_sgtdth
    SfxInfo::new(N_sgtdth, 70),
    // [65] sfx_cacdth
    SfxInfo::new(N_cacdth, 70),
    // [66] sfx_skldth
    SfxInfo::new(N_skldth, 70),
    // [67] sfx_brsdth
    SfxInfo::new(N_brsdth, 32),
    // [68] sfx_cybdth
    SfxInfo::new(N_cybdth, 32),
    // [69] sfx_spidth
    SfxInfo::new(N_spidth, 32),
    // [70] sfx_bspdth
    SfxInfo::new(N_bspdth, 32),
    // [71] sfx_vildth
    SfxInfo::new(N_vildth, 32),
    // [72] sfx_kntdth
    SfxInfo::new(N_kntdth, 32),
    // [73] sfx_pedth
    SfxInfo::new(N_pedth, 32),
    // [74] sfx_skedth
    SfxInfo::new(N_skedth, 32),
    // [75] sfx_posact
    SfxInfo::new(N_posact, 120),
    // [76] sfx_bgact
    SfxInfo::new(N_bgact, 120),
    // [77] sfx_dmact
    SfxInfo::new(N_dmact, 120),
    // [78] sfx_bspact
    SfxInfo::new(N_bspact, 100),
    // [79] sfx_bspwlk
    SfxInfo::new(N_bspwlk, 100),
    // [80] sfx_vilact
    SfxInfo::new(N_vilact, 100),
    // [81] sfx_noway
    SfxInfo::new(N_noway, 78),
    // [82] sfx_barexp
    SfxInfo::new(N_barexp, 60),
    // [83] sfx_punch
    SfxInfo::new(N_punch, 64),
    // [84] sfx_hoof
    SfxInfo::new(N_hoof, 70),
    // [85] sfx_metal
    SfxInfo::new(N_metal, 70),
    // [86] sfx_chgun
    SfxInfo::new(N_chgun, 64).with_volume(0).with_pitch(150),
    // [87] sfx_tink
    SfxInfo::new(N_tink, 60),
    // [88] sfx_bdopn
    SfxInfo::new(N_bdopn, 100),
    // [89] sfx_bdcls
    SfxInfo::new(N_bdcls, 100),
    // [90] sfx_itmbk
    SfxInfo::new(N_itmbk, 100),
    // [91] sfx_flame
    SfxInfo::new(N_flame, 32),
    // [92] sfx_flamst
    SfxInfo::new(N_flamst, 32),
    // [93] sfx_getpow
    SfxInfo::new(N_getpow, 60),
    // [94] sfx_bospit
    SfxInfo::new(N_bospit, 70),
    // [95] sfx_boscub
    SfxInfo::new(N_boscub, 70),
    // [96] sfx_bossit
    SfxInfo::new(N_bossit, 70),
    // [97] sfx_bospn
    SfxInfo::new(N_bospn, 70),
    // [98] sfx_bosdth
    SfxInfo::new(N_bosdth, 70),
    // [99] sfx_manatk
    SfxInfo::new(N_manatk, 70),
    // [100] sfx_mandth
    SfxInfo::new(N_mandth, 70),
    // [101] sfx_sssit
    SfxInfo::new(N_sssit, 70),
    // [102] sfx_ssdth
    SfxInfo::new(N_ssdth, 70),
    // [103] sfx_keenpn
    SfxInfo::new(N_keenpn, 70),
    // [104] sfx_keendt
    SfxInfo::new(N_keendt, 70),
    // [105] sfx_skeact
    SfxInfo::new(N_skeact, 70),
    // [106] sfx_skesit
    SfxInfo::new(N_skesit, 70),
    // [107] sfx_skeatk
    SfxInfo::new(N_skeatk, 70),
    // [108] sfx_radio
    SfxInfo::new(N_radio, 60),
];

/// Master music table indexed by `Mus` ids. Mirrors `S_music[]` in `sounds.c`.
///
/// Each non-sentinel entry stores a pointer into one of the `MUS_*` short-name
/// buffers, which `S_ChangeMusic` formats into the WAD lump name (prefixed
/// with `d_`) on first use. Entry 0 is the "no music" sentinel.
/// `#[no_mangle]` so the symbol matches the original C linkage.
#[no_mangle]
pub static mut S_music: [MusicInfo; NUMMUSIC] = [
    // [0] mus_none (sentinel)
    MusicInfo::none(),
    // [1] mus_e1m1
    MusicInfo::new(&MUS_e1m1),
    // [2] mus_e1m2
    MusicInfo::new(&MUS_e1m2),
    // [3] mus_e1m3
    MusicInfo::new(&MUS_e1m3),
    // [4] mus_e1m4
    MusicInfo::new(&MUS_e1m4),
    // [5] mus_e1m5
    MusicInfo::new(&MUS_e1m5),
    // [6] mus_e1m6
    MusicInfo::new(&MUS_e1m6),
    // [7] mus_e1m7
    MusicInfo::new(&MUS_e1m7),
    // [8] mus_e1m8
    MusicInfo::new(&MUS_e1m8),
    // [9] mus_e1m9
    MusicInfo::new(&MUS_e1m9),
    // [10] mus_e2m1
    MusicInfo::new(&MUS_e2m1),
    // [11] mus_e2m2
    MusicInfo::new(&MUS_e2m2),
    // [12] mus_e2m3
    MusicInfo::new(&MUS_e2m3),
    // [13] mus_e2m4
    MusicInfo::new(&MUS_e2m4),
    // [14] mus_e2m5
    MusicInfo::new(&MUS_e2m5),
    // [15] mus_e2m6
    MusicInfo::new(&MUS_e2m6),
    // [16] mus_e2m7
    MusicInfo::new(&MUS_e2m7),
    // [17] mus_e2m8
    MusicInfo::new(&MUS_e2m8),
    // [18] mus_e2m9
    MusicInfo::new(&MUS_e2m9),
    // [19] mus_e3m1
    MusicInfo::new(&MUS_e3m1),
    // [20] mus_e3m2
    MusicInfo::new(&MUS_e3m2),
    // [21] mus_e3m3
    MusicInfo::new(&MUS_e3m3),
    // [22] mus_e3m4
    MusicInfo::new(&MUS_e3m4),
    // [23] mus_e3m5
    MusicInfo::new(&MUS_e3m5),
    // [24] mus_e3m6
    MusicInfo::new(&MUS_e3m6),
    // [25] mus_e3m7
    MusicInfo::new(&MUS_e3m7),
    // [26] mus_e3m8
    MusicInfo::new(&MUS_e3m8),
    // [27] mus_e3m9
    MusicInfo::new(&MUS_e3m9),
    // [28] mus_inter
    MusicInfo::new(&MUS_inter),
    // [29] mus_intro
    MusicInfo::new(&MUS_intro),
    // [30] mus_bunny
    MusicInfo::new(&MUS_bunny),
    // [31] mus_victor
    MusicInfo::new(&MUS_victor),
    // [32] mus_introa
    MusicInfo::new(&MUS_introa),
    // [33] mus_runnin
    MusicInfo::new(&MUS_runnin),
    // [34] mus_stalks
    MusicInfo::new(&MUS_stalks),
    // [35] mus_countd
    MusicInfo::new(&MUS_countd),
    // [36] mus_betwee
    MusicInfo::new(&MUS_betwee),
    // [37] mus_doom
    MusicInfo::new(&MUS_doom),
    // [38] mus_the_da
    MusicInfo::new(&MUS_the_da),
    // [39] mus_shawn
    MusicInfo::new(&MUS_shawn),
    // [40] mus_ddtblu
    MusicInfo::new(&MUS_ddtblu),
    // [41] mus_in_cit
    MusicInfo::new(&MUS_in_cit),
    // [42] mus_dead
    MusicInfo::new(&MUS_dead),
    // [43] mus_stlks2
    MusicInfo::new(&MUS_stlks2),
    // [44] mus_theda2
    MusicInfo::new(&MUS_theda2),
    // [45] mus_doom2
    MusicInfo::new(&MUS_doom2),
    // [46] mus_ddtbl2
    MusicInfo::new(&MUS_ddtbl2),
    // [47] mus_runni2
    MusicInfo::new(&MUS_runni2),
    // [48] mus_dead2
    MusicInfo::new(&MUS_dead2),
    // [49] mus_stlks3
    MusicInfo::new(&MUS_stlks3),
    // [50] mus_romero
    MusicInfo::new(&MUS_romero),
    // [51] mus_shawn2
    MusicInfo::new(&MUS_shawn2),
    // [52] mus_messag
    MusicInfo::new(&MUS_messag),
    // [53] mus_count2
    MusicInfo::new(&MUS_count2),
    // [54] mus_ddtbl3
    MusicInfo::new(&MUS_ddtbl3),
    // [55] mus_ampie
    MusicInfo::new(&MUS_ampie),
    // [56] mus_theda3
    MusicInfo::new(&MUS_theda3),
    // [57] mus_adrian
    MusicInfo::new(&MUS_adrian),
    // [58] mus_messg2
    MusicInfo::new(&MUS_messg2),
    // [59] mus_romer2
    MusicInfo::new(&MUS_romer2),
    // [60] mus_tense
    MusicInfo::new(&MUS_tense),
    // [61] mus_shawn3
    MusicInfo::new(&MUS_shawn3),
    // [62] mus_openin
    MusicInfo::new(&MUS_openin),
    // [63] mus_evil
    MusicInfo::new(&MUS_evil),
    // [64] mus_ultima
    MusicInfo::new(&MUS_ultima),
    // [65] mus_read_m
    MusicInfo::new(&MUS_read_m),
    // [66] mus_dm2ttl
    MusicInfo::new(&MUS_dm2ttl),
    // [67] mus_dm2int
    MusicInfo::new(&MUS_dm2int),
];

/// Wire up the runtime cross-links in `S_sfx`.
///
/// Currently only `sfx_chgun -> sfx_pistol` (sharing the pistol sample). Must
/// be called before any `S_StartSound`. Called from `S_Init`. C origin:
/// `S_InitSfxLinks` in `sounds.c`.
#[no_mangle]
pub extern "C" fn S_InitSfxLinks() {
    unsafe {
        S_sfx[Sfx::Chgun as usize].link = &mut S_sfx[Sfx::Pistol as usize] as *mut SfxInfo;
    }
}

/// Unit tests pinning enum counts, table sizes, and struct layout to the C
/// definitions so silent drift cannot break the FFI surface.
#[cfg(test)]
mod tests {
    use super::*;

    /// `NUMSFX` must stay at 109, matching the C `sfxenum_t`.
    #[test]
    fn numsfx_matches_c_source() {
        assert_eq!(NUMSFX, 109);
    }

    /// `NUMMUSIC` must stay at 68, matching the C `musicenum_t`.
    #[test]
    fn nummusic_matches_c_source() {
        assert_eq!(NUMMUSIC, 68);
    }

    /// `S_sfx` length must equal `NUMSFX` (catches off-by-one in the table).
    #[test]
    fn sfx_table_has_correct_length() {
        // The static array is sized by NUMSFX; this confirms no off-by-one.
        unsafe {
            assert_eq!(S_sfx.len(), NUMSFX);
        }
    }

    /// `S_music` length must equal `NUMMUSIC`.
    #[test]
    fn music_table_has_correct_length() {
        unsafe {
            assert_eq!(S_music.len(), NUMMUSIC);
        }
    }

    /// Entry 0 is the "none" sentinel - priority and numchannels must be 0 / -1.
    #[test]
    fn sfx_entry_zero_is_none_sentinel() {
        unsafe {
            assert_eq!(S_sfx[0].priority, 0);
            assert_eq!(S_sfx[0].numchannels, -1);
            assert_eq!(S_sfx[0].lumpnum, 0);
            // tagname and link are null initially
            assert!(S_sfx[0].tagname.is_null());
            assert!(S_sfx[0].link.is_null());
        }
    }

    /// sfx_pistol is entry 1 with priority 64.
    #[test]
    fn sfx_pistol_is_entry_1_with_priority_64() {
        unsafe {
            let entry = &S_sfx[Sfx::Pistol as usize];
            assert_eq!(entry.priority, 64);
            // name must start with "pistol"
            let name_bytes: Vec<u8> = entry.name.iter().map(|&c| c as u8).collect();
            assert!(
                name_bytes.starts_with(b"pistol"),
                "name should start with 'pistol'"
            );
        }
    }

    /// sfx_chgun (entry 86) must link to sfx_pistol after S_InitSfxLinks.
    #[test]
    fn sfx_chgun_links_to_pistol_after_init() {
        unsafe {
            S_InitSfxLinks();
            let pistol_ptr = &S_sfx[Sfx::Pistol as usize] as *const SfxInfo;
            let chgun_link = S_sfx[Sfx::Chgun as usize].link as *const SfxInfo;
            assert_eq!(
                chgun_link, pistol_ptr,
                "sfx_chgun.link must point to sfx_pistol"
            );
        }
    }

    /// `sizeof(sfxinfo_t)` must be 64 on 64-bit, matching the C struct.
    #[test]
    fn sfx_info_size_matches_c() {
        // sizeof(sfxinfo_t) in C on 64-bit: tagname(8) + name[9](9) + pad(3)
        // + priority(4) + link(8) + pitch(4) + volume(4) + usefulness(4)
        // + lumpnum(4) + numchannels(4) + pad(4) + driver_data(8) = 64 bytes
        assert_eq!(std::mem::size_of::<SfxInfo>(), 64);
    }

    /// `sizeof(musicinfo_t)` must be 32 on 64-bit (four pointer-sized fields).
    #[test]
    fn music_info_size_matches_c() {
        // sizeof(musicinfo_t) is 32 on 64-bit (four pointer-sized fields).
        assert_eq!(std::mem::size_of::<MusicInfo>(), 32);
    }
}
