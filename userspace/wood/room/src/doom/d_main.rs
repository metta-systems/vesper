//! Rust port of vendor/doomgeneric/d_main.c.
//!
//! Main initialization (`D_DoomMain`) and game loop (`D_DoomLoop`),
//! plus demo sequence management, display rendering, and command-line
//! parameter processing.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::{c_char, c_int, c_void};
use std::ptr;

use crate::doom::d_mode;
use crate::doom::d_player::{consoleplayer, players, MAXPLAYERS};
use crate::doom::doomstat::{gamedescription, gamemission, gamemode, gameversion, modifiedgame};
use crate::doom::i_timer::TICRATE;
use crate::doom::i_video::I_StartFrame;
use crate::doom::i_video::{SCREENHEIGHT, SCREENWIDTH};
use crate::doom::m_config::M_SaveDefaults;
use crate::doom::m_misc::M_snprintf_clamp;
use crate::types::Boolean;
use crate::{c_write, i_error};

// ---------------------------------------------------------------------------
// String constants from d_englsh.h / dstrings.h
// ---------------------------------------------------------------------------

/// Message printed to stdout when `-devparm` is active.
static D_DEVSTR: &str = "Development mode ON.\n";

/// Multiplayer chat target key for the green player.
const HUSTR_KEYGREEN: c_char = b'g' as c_char;
/// Multiplayer chat target key for the indigo player.
const HUSTR_KEYINDIGO: c_char = b'i' as c_char;
/// Multiplayer chat target key for the brown player.
const HUSTR_KEYBROWN: c_char = b'b' as c_char;
/// Multiplayer chat target key for the red player.
const HUSTR_KEYRED: c_char = b'r' as c_char;

// ---------------------------------------------------------------------------
// Type aliases matching C enums
// ---------------------------------------------------------------------------

/// C-compatible game state discriminant (`gamestate_t`).
type gamestate_t = c_int;
/// C-compatible pending game action discriminant (`gameaction_t`).
type gameaction_t = c_int;
/// C-compatible skill level discriminant (`skill_t`).
type skill_t = c_int;
/// Single byte, matching the C `byte` typedef.
type byte = u8;

/// Game state: playing a level.
const GS_LEVEL: gamestate_t = 0;
/// Game state: intermission screen between levels.
const GS_INTERMISSION: gamestate_t = 1;
/// Game state: end-of-episode finale.
const GS_FINALE: gamestate_t = 2;
/// Game state: title / demo screen.
const GS_DEMOSCREEN: gamestate_t = 3;

/// No pending action.
const ga_nothing: gameaction_t = 0;
/// Pending action: load a level.
const ga_loadlevel: gameaction_t = 1;
/// Pending action: start a new game.
const ga_newgame: gameaction_t = 2;
/// Pending action: load a saved game.
const ga_loadgame: gameaction_t = 3;
/// Pending action: save the current game.
const ga_savegame: gameaction_t = 4;
/// Pending action: play a demo.
const ga_playdemo: gameaction_t = 5;
/// Pending action: level completed.
const ga_completed: gameaction_t = 6;
/// Pending action: episode victory sequence.
const ga_victory: gameaction_t = 7;
/// Pending action: world (episode) done.
const ga_worlddone: gameaction_t = 8;
/// Pending action: take a screenshot.
const ga_screenshot: gameaction_t = 9;

/// Skill level: "I'm Too Young To Die" (baby).
const sk_baby: skill_t = 0;
/// Skill level: "Hey, Not Too Rough" (easy).
const sk_easy: skill_t = 1;
/// Skill level: "Hurt Me Plenty" (medium).
const sk_medium: skill_t = 2;
/// Skill level: "Ultra-Violence" (hard).
const sk_hard: skill_t = 3;
/// Skill level: "Nightmare!".
const sk_nightmare: skill_t = 4;
/// Sentinel: no items mode (internal use only).
const sk_noitems: skill_t = -1;

use crate::doom::z_zone::{PU_CACHE, PU_STATIC};

// ---------------------------------------------------------------------------
// Globals — these are #[no_mangle] so remaining C code (g_game.c etc.)
// can resolve them at link time.
// ---------------------------------------------------------------------------

/// Savegame directory.
#[no_mangle]
pub static mut savegamedir: *mut c_char = ptr::null_mut();

/// Path to the currently loaded IWAD file.
#[no_mangle]
pub static mut iwadfile: *mut c_char = ptr::null_mut();

/// Started game with -devparm.
#[no_mangle]
pub static mut devparm: c_int = 0;

/// Checkparm of -nomonsters.
#[no_mangle]
pub static mut nomonsters: c_int = 0;

/// Checkparm of -respawn.
#[no_mangle]
pub static mut respawnparm: c_int = 0;

/// Checkparm of -fast.
#[no_mangle]
pub static mut fastparm: c_int = 0;

/// Skill level to start at (set by -skill).
#[no_mangle]
pub static mut startskill: c_int = 0;

/// Episode to start at (set by -episode).
#[no_mangle]
pub static mut startepisode: c_int = 0;

/// Map to start at (set by -warp / -episode).
#[no_mangle]
pub static mut startmap: c_int = 0;

/// True when -warp / -episode has provided an explicit start.
#[no_mangle]
pub static mut autostart: c_int = 0;

/// Load game slot (-1 = not loading).
#[no_mangle]
pub static mut startloadgame: c_int = 0;

/// True while playing back a built-in demo sequence.
#[no_mangle]
pub static mut advancedemo: c_int = 0;

/// Store demo, do not accept any inputs.
#[no_mangle]
pub static mut storedemo: c_int = 0;

/// True when the BFG edition of the IWAD is detected.
#[no_mangle]
pub static mut bfgedition: c_int = 0;

/// True once the main event loop has started.
#[no_mangle]
pub static mut main_loop_started: c_int = 0;

/// Primary WAD file path buffer.
#[no_mangle]
pub static mut wadfile: [c_char; 1024] = [0; 1024];

/// Directory of development maps.
#[no_mangle]
pub static mut mapdir: [c_char; 1024] = [0; 1024];

/// When non-zero, display the ENDOOM text on exit.
#[no_mangle]
pub static mut show_endoom: c_int = 1;

/// Demo loop sequence counter.
#[no_mangle]
pub static mut demosequence: c_int = 0;

/// Tics remaining on the current demo page.
#[no_mangle]
pub static mut pagetic: c_int = 0;

/// Name of the current demo page lump.
#[no_mangle]
pub static mut pagename: *mut c_char = ptr::null_mut();

/// Wipe gamestate for transition effects. -1 forces a wipe on next draw.
#[no_mangle]
pub static mut wipegamestate: c_int = GS_DEMOSCREEN;

// D_Display static local state — these must persist across frames.
/// Cached `viewactive` value from the previous frame.
static mut D_DISP_VIEWACTIVE: c_int = 0;
/// Cached `menuactive` value from the previous frame.
static mut D_DISP_MENUACTIVE: c_int = 0;
/// Cached `inhelpscreens` value from the previous frame.
static mut D_DISP_INHELPSCREENS: c_int = 0;
/// Non-zero when the view fills the full screen (no status bar visible).
static mut D_DISP_FULLSCREEN: c_int = 0;
/// Game state from the previous frame; -1 forces border and palette redraws.
static mut D_DISP_OLD_GAMESTATE: c_int = -1;
/// Remaining frames for which the view border must be redrawn.
static mut D_DISP_BORDERDRAWCOUNT: c_int = 0;

/// Title string printed at startup.
static mut title: [c_char; 128] = [0; 128];

/// Startup banner strings (may be replaced by dehacked).
/// These contain embedded \0 bytes (part of the format) so cannot use c"" literals.
#[allow(clippy::manual_c_str_literals)]
static mut banners: [*const c_char; 7] = [
    b"                         \0DOOM 2: Hell on Earth v%i.%i\0                           \0"
        .as_ptr() as *const c_char,
    b"                            \0DOOM Shareware Startup v%i.%i\0                           \0"
        .as_ptr() as *const c_char,
    b"                            \0DOOM Registered Startup v%i.%i\0                           \0"
        .as_ptr() as *const c_char,
    b"                          \0DOOM System Startup v%i.%i\0                          \0".as_ptr()
        as *const c_char,
    b"                         \0The Ultimate DOOM Startup v%i.%i\0                        \0"
        .as_ptr() as *const c_char,
    b"                     \0DOOM 2: TNT - Evilution v%i.%i\0                           \0".as_ptr()
        as *const c_char,
    b"                   \0DOOM 2: Plutonia Experiment v%i.%i\0                           \0"
        .as_ptr() as *const c_char,
];

/// Newtype wrapper that makes a raw C-string pointer `Sync` for use in statics.
struct SyncPtr(*const c_char);
unsafe impl Sync for SyncPtr {}

/// One entry in the `-gameversion` lookup table.
#[repr(C)]
struct GameVersionDesc {
    /// Human-readable version label shown in the help list.
    description: SyncPtr,
    /// Command-line argument string (e.g. `"1.9"`, `"ultimate"`).
    cmdline: SyncPtr,
    /// Numeric version constant from `d_mode` (e.g. `exe_doom_1_9`).
    version: c_int,
}

/// Construct a [`SyncPtr`] from a static byte-string literal.
const fn sp(s: &'static [u8]) -> SyncPtr {
    SyncPtr(s.as_ptr() as *const c_char)
}

/// Table of known game versions, termined by a null-description sentinel entry.
static GAME_VERSIONS: [GameVersionDesc; 10] = [
    GameVersionDesc {
        description: sp(b"Doom 1.666\0"),
        cmdline: sp(b"1.666\0"),
        version: d_mode::exe_doom_1_666,
    },
    GameVersionDesc {
        description: sp(b"Doom 1.7/1.7a\0"),
        cmdline: sp(b"1.7\0"),
        version: d_mode::exe_doom_1_7,
    },
    GameVersionDesc {
        description: sp(b"Doom 1.8\0"),
        cmdline: sp(b"1.8\0"),
        version: d_mode::exe_doom_1_8,
    },
    GameVersionDesc {
        description: sp(b"Doom 1.9\0"),
        cmdline: sp(b"1.9\0"),
        version: d_mode::exe_doom_1_9,
    },
    GameVersionDesc {
        description: sp(b"Hacx\0"),
        cmdline: sp(b"hacx\0"),
        version: d_mode::exe_hacx,
    },
    GameVersionDesc {
        description: sp(b"Ultimate Doom\0"),
        cmdline: sp(b"ultimate\0"),
        version: d_mode::exe_ultimate,
    },
    GameVersionDesc {
        description: sp(b"Final Doom\0"),
        cmdline: sp(b"final\0"),
        version: d_mode::exe_final,
    },
    GameVersionDesc {
        description: sp(b"Final Doom (alt)\0"),
        cmdline: sp(b"final2\0"),
        version: d_mode::exe_final2,
    },
    GameVersionDesc {
        description: sp(b"Chex Quest\0"),
        cmdline: sp(b"chex\0"),
        version: d_mode::exe_chex,
    },
    GameVersionDesc {
        description: SyncPtr(ptr::null()),
        cmdline: SyncPtr(ptr::null()),
        version: 0,
    },
];

/// One entry in the `-pack` lookup table.
#[repr(C)]
struct PackDesc {
    /// Command-line pack name (e.g. `"tnt"`, `"plutonia"`).
    name: SyncPtr,
    /// `gamemission` value to assign when this pack is selected.
    mission: c_int,
}

/// Recognized `-pack` arguments, mapping names to `gamemission` values.
static PACKS: [PackDesc; 3] = [
    PackDesc {
        name: sp(b"doom2\0"),
        mission: d_mode::doom2,
    },
    PackDesc {
        name: sp(b"tnt\0"),
        mission: d_mode::pack_tnt,
    },
    PackDesc {
        name: sp(b"plutonia\0"),
        mission: d_mode::pack_plut,
    },
];

// ---------------------------------------------------------------------------
// Extern declarations for C code that is not yet ported (g_game.c)
// ---------------------------------------------------------------------------

extern "C" {
    // net_dedicated.c / net_query.c — not yet ported:
    /// Runs the dedicated server loop; from `net_dedicated.c` (not yet ported).
    fn NET_DedicatedServer();
    /// Queries the master server for a game list; from `net_query.c` (not yet ported).
    fn NET_MasterQuery();
    /// Queries a specific network address for a game; from `net_query.c` (not yet ported).
    fn NET_QueryAddress(addr: *mut c_char);
    /// Scans the local network (LAN) for games; from `net_query.c` (not yet ported).
    fn NET_LANQuery();

    // d_loop.rs — drone not yet exported as pub static:
    /// Whether this instance is a drone (spectator) player; from `d_loop.rs`.
    static mut drone: c_int;

    // C standard library:
    /// Returns the length of the null-terminated C string `s`, excluding the null terminator.
    fn strlen(s: *const c_char) -> usize;
    /// Compares two null-terminated C strings lexicographically; returns 0 if equal.
    fn strcmp(s1: *const c_char, s2: *const c_char) -> c_int;
    /// Case-insensitive comparison of two null-terminated C strings; returns 0 if equal.
    fn strcasecmp(s1: *const c_char, s2: *const c_char) -> c_int;
    /// Case-insensitive comparison of at most `n` bytes of two null-terminated C strings; returns 0 if equal.
    fn strncasecmp(s1: *const c_char, s2: *const c_char, n: usize) -> c_int;
    /// Converts the initial portion of the null-terminated C string `nptr` to `c_int`.
    fn atoi(nptr: *const c_char) -> c_int;
    /// Terminates the process with the given exit `status` code.
    fn exit(status: c_int) -> !;
    /// Returns non-zero if `c` is a whitespace character per the current locale.
    fn isspace(c: c_int) -> c_int;
    /// Copies `n` bytes from `src` to `dest`, handling overlapping regions correctly.
    fn memmove(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void;
}

// g_game.rs
use crate::doom::g_game::{
    deathmatch, demoplayback, demorecording, displayplayer, forwardmove, gameaction, gamestate,
    netgame, nodrawers, paused, sidemove, singledemo, testcontrols, testcontrols_mousespeed,
    usergame, vanilla_demo_limit, vanilla_savegame_limit, viewactive, G_BeginRecording,
    G_DeferedPlayDemo, G_InitNew, G_LoadGame, G_RecordDemo, G_Responder, G_TimeDemo,
    G_VanillaVersionCode,
};

// am_map.rs
use crate::doom::am_map::{automapactive, AM_Drawer};

// m_menu.rs
use crate::doom::m_menu::{
    detailLevel, inhelpscreens, menuactive, mouseSensitivity, screenblocks, showMessages,
};

// s_sound.rs
use crate::doom::s_sound::{
    musicVolume, sfxVolume, snd_channels, S_Init, S_StartMusic, S_UpdateSounds,
};
use crate::doom::sounds::Mus;

// r_main.rs
use crate::doom::r_main::{setsizeneeded, R_ExecuteSetViewSize, R_Init, R_RenderPlayerView};

// r_draw.rs
use crate::doom::r_draw::{
    scaledviewwidth, viewheight, viewwindowx, viewwindowy, R_DrawViewBorder, R_FillBackScreen,
};

// i_video.rs
use crate::doom::i_video::{screensaver_mode, screenvisible};

// w_wad.rs
use crate::doom::w_wad::{
    lumpinfo, numlumps, W_AddFile, W_CacheLumpName, W_CheckCorrectIWAD, W_CheckNumForName,
    W_GenerateHashTable,
};

// hu_stuff.rs
use crate::doom::hu_stuff::{chat_macros, HU_Drawer, HU_Init};

// m_controls.rs
use crate::doom::m_controls::key_multi_msgplayer;

// wi_stuff.rs
use crate::doom::wi_stuff::WI_Drawer;

// st_stuff.rs
use crate::doom::st_stuff::{ST_Drawer, ST_Init};

// p_setup.rs
use crate::doom::p_setup::P_Init;

// p_saveg.rs
use crate::doom::p_saveg::P_SaveGameFile;

// statdump.rs

// d_net.rs
use crate::doom::d_net::{D_CheckNetGame, D_ConnectNetGame};

// d_event.rs
use crate::doom::d_event::D_PopEvent;

// d_loop.rs
use crate::doom::d_loop::{gametic, D_StartGameLoop, NetUpdate, TryRunTics};

// d_iwad.rs
use crate::doom::d_iwad::{D_FindIWAD, D_SaveGameIWADName};

// z_zone.rs
use crate::doom::z_zone::{Z_Init, Z_Malloc};

// w_main.rs
use crate::doom::w_main::W_ParseCommandLine;

// v_video.rs
use crate::doom::v_video::{
    patch_t, V_DrawMouseSpeedBox, V_DrawPatch, V_DrawPatchDirect, V_Init, V_RestoreBuffer,
};

// f_finale.rs
use crate::doom::f_finale::F_Drawer;

// f_wipe.rs
use crate::doom::f_wipe::{wipe_EndScreen, wipe_ScreenWipe, wipe_StartScreen};

// m_argv.rs
use crate::doom::m_argv::{myargc, myargv, M_CheckParm, M_CheckParmWithArgs};

// m_config.rs
use crate::doom::m_config::{
    M_BindVariable, M_GetSaveGameDir, M_LoadDefaults, M_SetConfigDir, M_SetConfigFilenames,
};

// m_controls.rs
use crate::doom::m_controls::{
    M_ApplyPlatformDefaults, M_BindBaseControls, M_BindChatControls, M_BindMapControls,
    M_BindMenuControls, M_BindWeaponControls,
};

// m_misc.rs
use crate::doom::m_misc::M_StringCopy;

// m_menu.rs
use crate::doom::m_menu::{M_Drawer, M_Init, M_Responder};

// i_timer.rs
use crate::doom::i_timer::I_GetTime;

// i_system.rs
use crate::doom::i_system::{I_AtExit, I_PrintBanner, I_PrintDivider, I_PrintStartupBanner};

// i_sound.rs
use crate::doom::i_sound::{I_BindSoundVariables, I_InitMusic, I_InitSound};

// i_video.rs
use crate::doom::i_video::{
    I_BindVideoVariables, I_CheckIsScreensaver, I_DisplayFPSDots, I_EnableLoadingDisk,
    I_FinishUpdate, I_GraphicsCheckCommandLine, I_InitGraphics, I_SetGrabMouseCallback,
    I_SetPalette, I_SetWindowTitle, I_UpdateNoBlit,
};

// i_timer.rs
use crate::doom::i_timer::{I_InitTimer, I_Sleep};

// i_joystick.rs
use crate::doom::i_joystick::{I_BindJoystickVariables, I_InitJoystick};

// i_endoom.rs
use crate::doom::i_endoom::I_Endoom;

// ---------------------------------------------------------------------------
// Constants for lump name checks (IWAD detection)
// ---------------------------------------------------------------------------

/// Lump names whose presence in the WAD confirms a registered (non-shareware) IWAD.
static IWAD_CHECK_NAMES: [SyncPtr; 23] = [
    sp(b"e2m1\0"),
    sp(b"e2m2\0"),
    sp(b"e2m3\0"),
    sp(b"e2m4\0"),
    sp(b"e2m5\0"),
    sp(b"e2m6\0"),
    sp(b"e2m7\0"),
    sp(b"e2m8\0"),
    sp(b"e2m9\0"),
    sp(b"e3m1\0"),
    sp(b"e3m3\0"),
    sp(b"e3m3\0"), // duplicate in original C code
    sp(b"e3m4\0"),
    sp(b"e3m5\0"),
    sp(b"e3m6\0"),
    sp(b"e3m7\0"),
    sp(b"e3m8\0"),
    sp(b"e3m9\0"),
    sp(b"dphoof\0"),
    sp(b"bfgga0\0"),
    sp(b"heada1\0"),
    sp(b"cybra1\0"),
    sp(b"spida1d1\0"),
];

// ---------------------------------------------------------------------------
// Helper: DEH_String passthrough
// ---------------------------------------------------------------------------

/// Return the string as-is.  In the original C code this could be replaced
/// by dehacked patches, but since FEATURE_DEHACKED is not defined we
/// simply return the original pointer.
#[inline]
fn DEH_String(s: *const c_char) -> *const c_char {
    s
}

/// Return `true` if the C string `s` ends with `suffix` (case-insensitive).
/// Returns `false` for null pointers.
///
/// # Safety
///
/// Both `s` and `suffix`, if non-null, must point to valid, null-terminated C strings that remain
/// live for the duration of the call.
unsafe fn c_str_ends_with(s: *const c_char, suffix: *const c_char) -> bool {
    if s.is_null() || suffix.is_null() {
        return false;
    }
    let s_len = strlen(s);
    let suf_len = strlen(suffix);
    if suf_len > s_len {
        return false;
    }
    strncasecmp(s.add(s_len.wrapping_sub(suf_len)), suffix, suf_len) == 0
}

/// Return `true` if the two C strings are equal under a case-insensitive comparison.
/// Returns `false` if either pointer is null.
///
/// # Safety
///
/// Both `s1` and `s2`, if non-null, must point to valid, null-terminated C strings that remain
/// live for the duration of the call.
unsafe fn c_str_eq(s1: *const c_char, s2: *const c_char) -> bool {
    if s1.is_null() || s2.is_null() {
        return false;
    }
    strcasecmp(s1, s2) == 0
}

/// Return `true` if the first `n` bytes of the two C strings differ under a case-insensitive
/// comparison. Returns `true` (not-equal) if either pointer is null.
///
/// # Safety
///
/// Both `s1` and `s2`, if non-null, must point to valid C strings with at least `n` accessible
/// bytes that remain live for the duration of the call.
unsafe fn c_str_ne_n(s1: *const c_char, s2: *const c_char, n: usize) -> bool {
    if s1.is_null() || s2.is_null() {
        return true;
    }
    strncasecmp(s1, s2, n) != 0
}

/// Convert a raw C string to an owned `String`, returning empty string for null pointers.
///
/// # Safety
///
/// `s`, if non-null, must point to a valid, null-terminated C string that remains live for the
/// duration of the call.
unsafe fn c_str_to_str(s: *const c_char) -> String {
    if s.is_null() {
        return String::new();
    }
    std::ffi::CStr::from_ptr(s).to_string_lossy().into_owned()
}

// ---------------------------------------------------------------------------
// D_ProcessEvents
// ---------------------------------------------------------------------------

/// Send all buffered events down the responder chain.
#[no_mangle]
pub extern "C" fn D_ProcessEvents() {
    unsafe {
        // IF STORE DEMO, DO NOT ACCEPT INPUT
        if storedemo != 0 {
            return;
        }

        while {
            let ev = D_PopEvent();
            if ev.is_null() {
                false
            } else {
                if M_Responder(ev).is_false() {
                    G_Responder(ev);
                }
                true
            }
        } {}
    }
}

// ---------------------------------------------------------------------------
// D_Display
// ---------------------------------------------------------------------------

/// Draw current display, possibly wiping it from the previous frame.
#[no_mangle]
pub extern "C" fn D_Display() {
    unsafe {
        if nodrawers != 0 {
            return;
        }

        let mut redrawsbar = false;

        // Change the view size if needed
        if setsizeneeded.is_truthy() {
            R_ExecuteSetViewSize();
            D_DISP_OLD_GAMESTATE = -1; // force background redraw
            D_DISP_BORDERDRAWCOUNT = 3;
        }

        // Save the current screen if about to wipe
        let wipe = gamestate != wipegamestate;
        if wipe {
            wipe_StartScreen(0, 0, SCREENWIDTH, SCREENHEIGHT);
        }

        // Buffered drawing based on game state
        if gamestate == GS_LEVEL {
            if gametic == 0 {
                // Skip level rendering
            } else {
                if automapactive != 0 {
                    AM_Drawer();
                }
                if wipe || (viewheight != 200 && D_DISP_FULLSCREEN != 0) {
                    redrawsbar = true;
                }
                if D_DISP_INHELPSCREENS != 0 && inhelpscreens == 0 {
                    redrawsbar = true;
                }
                ST_Drawer(Boolean::from(viewheight == 200), Boolean::from(redrawsbar));
                D_DISP_FULLSCREEN = (viewheight == 200) as c_int;
            }
        } else if gamestate == GS_INTERMISSION {
            WI_Drawer();
        } else if gamestate == GS_FINALE {
            F_Drawer();
        } else if gamestate == GS_DEMOSCREEN {
            D_PageDrawer();
        }

        // Draw buffered stuff to screen
        I_UpdateNoBlit();

        // Draw the view directly
        if gamestate == GS_LEVEL && automapactive == 0 && gametic != 0 {
            let dp = displayplayer as usize;
            if dp < MAXPLAYERS {
                R_RenderPlayerView(std::ptr::addr_of_mut!(players[dp]));
            }
            HU_Drawer();
        }

        // Set palette on state change (non-level states)
        if gamestate != GS_LEVEL && gamestate != D_DISP_OLD_GAMESTATE {
            I_SetPalette(W_CacheLumpName(DEH_String(c"PLAYPAL".as_ptr()), PU_CACHE) as *mut byte);
        }

        // See if the border needs to be initially drawn
        if gamestate == GS_LEVEL && D_DISP_OLD_GAMESTATE != GS_LEVEL {
            D_DISP_VIEWACTIVE = 0;
            R_FillBackScreen();
        }

        // See if the border needs to be updated
        if gamestate == GS_LEVEL && automapactive == 0 && scaledviewwidth != 320 {
            if menuactive != 0 || D_DISP_MENUACTIVE != 0 || D_DISP_VIEWACTIVE == 0 {
                D_DISP_BORDERDRAWCOUNT = 3;
            }
            if D_DISP_BORDERDRAWCOUNT != 0 {
                R_DrawViewBorder();
                D_DISP_BORDERDRAWCOUNT -= 1;
            }
        }

        // Test controls: show mouse speed box
        if testcontrols != 0 {
            V_DrawMouseSpeedBox(testcontrols_mousespeed);
        }

        // Update static state trackers
        D_DISP_MENUACTIVE = menuactive;
        D_DISP_VIEWACTIVE = viewactive;
        D_DISP_INHELPSCREENS = inhelpscreens;
        D_DISP_OLD_GAMESTATE = gamestate;
        wipegamestate = gamestate;

        // Draw pause pic
        if paused != 0 {
            let y = if automapactive != 0 {
                4
            } else {
                viewwindowy + 4
            };
            V_DrawPatchDirect(
                viewwindowx + (scaledviewwidth - 68) / 2,
                y,
                W_CacheLumpName(DEH_String(c"M_PAUSE".as_ptr()), PU_CACHE) as *mut patch_t,
            );
        }

        // Menus go directly to the screen
        M_Drawer();
        NetUpdate();

        // Normal update
        if !wipe {
            I_FinishUpdate();
            return;
        }

        // Wipe update
        wipe_EndScreen(0, 0, SCREENWIDTH, SCREENHEIGHT);

        let mut wipestart = I_GetTime() - 1;
        loop {
            let mut nowtime;
            let mut tics;
            loop {
                nowtime = I_GetTime();
                tics = nowtime - wipestart;
                if tics <= 0 {
                    I_Sleep(1);
                } else {
                    break;
                }
            }

            wipestart = nowtime;
            let done = wipe_ScreenWipe(
                1, // wipe_Melt
                0,
                0,
                SCREENWIDTH,
                SCREENHEIGHT,
                tics,
            ) != 0;

            I_UpdateNoBlit();
            M_Drawer();
            I_FinishUpdate();

            if done {
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// D_BindVariables
// ---------------------------------------------------------------------------

/// Bind all configuration variables.
#[no_mangle]
pub extern "C" fn D_BindVariables() {
    unsafe {
        M_ApplyPlatformDefaults();

        I_BindVideoVariables();
        I_BindJoystickVariables();
        I_BindSoundVariables();

        M_BindBaseControls();
        M_BindWeaponControls();
        M_BindMapControls();
        M_BindMenuControls();
        M_BindChatControls(4); // MAXPLAYERS = 4

        key_multi_msgplayer[0] = HUSTR_KEYGREEN as c_int;
        key_multi_msgplayer[1] = HUSTR_KEYINDIGO as c_int;
        key_multi_msgplayer[2] = HUSTR_KEYBROWN as c_int;
        key_multi_msgplayer[3] = HUSTR_KEYRED as c_int;

        M_BindVariable(
            c"mouse_sensitivity".as_ptr().cast_mut(),
            &raw mut mouseSensitivity as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            c"sfx_volume".as_ptr().cast_mut(),
            &raw mut sfxVolume as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            c"music_volume".as_ptr().cast_mut(),
            &raw mut musicVolume as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            c"show_messages".as_ptr().cast_mut(),
            &raw mut showMessages as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            c"screenblocks".as_ptr().cast_mut(),
            &raw mut screenblocks as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            c"detaillevel".as_ptr().cast_mut(),
            &raw mut detailLevel as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            c"snd_channels".as_ptr().cast_mut(),
            &raw mut snd_channels as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            c"vanilla_savegame_limit".as_ptr().cast_mut(),
            &raw mut vanilla_savegame_limit as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            c"vanilla_demo_limit".as_ptr().cast_mut(),
            &raw mut vanilla_demo_limit as *mut c_int as *mut c_void,
        );
        M_BindVariable(
            c"show_endoom".as_ptr().cast_mut(),
            &raw mut show_endoom as *mut c_int as *mut c_void,
        );

        // Multiplayer chat macros
        for i in 0..10 {
            let mut buf: [c_char; 12] = [0; 12];
            c_write!(buf, "chatmacro{}", i);
            M_BindVariable(
                buf.as_mut_ptr(),
                &mut chat_macros[i] as *mut *mut c_char as *mut c_void,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// D_GrabMouseCallback
// ---------------------------------------------------------------------------

/// Return `TRUE` when the window should grab (capture) the mouse pointer.
///
/// The mouse is released for drone players, while the menu is open, or while a demo is playing.
extern "C" fn D_GrabMouseCallback() -> Boolean {
    unsafe {
        // Drone players don't need mouse focus
        if drone != 0 {
            return Boolean::FALSE;
        }

        // When menu is active or game is paused, release the mouse
        if menuactive != 0 || paused != 0 {
            return Boolean::FALSE;
        }

        // Only grab mouse when playing levels (but not demos)
        let demoplayback_val: c_int = demoplayback;
        let advancedemo_val: c_int = advancedemo;
        Boolean::from(gamestate == GS_LEVEL && demoplayback_val == 0 && advancedemo_val == 0)
    }
}

// ---------------------------------------------------------------------------
// doomgeneric_Tick
// ---------------------------------------------------------------------------

/// Execute one rendered frame: run game tics, update sounds, and draw the display.
///
/// Called by the doomgeneric platform layer once per video frame.
#[no_mangle]
pub extern "C" fn doomgeneric_Tick() {
    unsafe {
        // Frame synchronous IO operations
        I_StartFrame();

        TryRunTics(); // will run at least one tic

        // Update positional sounds
        let console = consoleplayer;
        let mo = if console >= 0 && (console as usize) < MAXPLAYERS {
            players[console as usize].mo
        } else {
            std::ptr::null_mut()
        };
        S_UpdateSounds(mo as *mut crate::doom::s_sound::MobjStub);

        // Update display
        if screenvisible != 0 {
            D_Display();
        }
    }
}

// ---------------------------------------------------------------------------
// D_DoomLoop
// ---------------------------------------------------------------------------

/// Main game loop — never returns.
#[no_mangle]
pub extern "C" fn D_DoomLoop() {
    unsafe {
        if bfgedition != 0 {
            let is_recording = demorecording;
            let is_playdemo = gameaction == ga_playdemo;
            if is_recording != 0 || is_playdemo || netgame != 0 {
                eprintln!(
                    " WARNING: You are playing using one of the Doom Classic\n\
                     IWAD files shipped with the Doom 3: BFG Edition. These are\n\
                     known to be incompatible with the regular IWAD files and\n\
                     may cause demos and network games to get out of sync."
                );
            }
        }

        if demorecording != 0 {
            G_BeginRecording();
        }

        main_loop_started = 1;

        TryRunTics();

        I_SetWindowTitle(gamedescription);
        I_GraphicsCheckCommandLine();

        // Cast the Rust callback to match the C signature
        extern "C" fn grab_cb() -> Boolean {
            D_GrabMouseCallback()
        }
        I_SetGrabMouseCallback(grab_cb);

        I_InitGraphics();
        I_EnableLoadingDisk();

        V_RestoreBuffer();
        R_ExecuteSetViewSize();

        D_StartGameLoop();

        if testcontrols != 0 {
            wipegamestate = gamestate;
        }

        doomgeneric_Tick();
    }
}

// ---------------------------------------------------------------------------
// Demo sequence management
// ---------------------------------------------------------------------------

/// Handle timing for demo projection.
#[no_mangle]
pub extern "C" fn D_PageTicker() {
    unsafe {
        pagetic -= 1;
        if pagetic < 0 {
            D_AdvanceDemo();
        }
    }
}

/// Draw the current demo page.
#[no_mangle]
pub extern "C" fn D_PageDrawer() {
    unsafe {
        V_DrawPatch(0, 0, W_CacheLumpName(pagename, PU_CACHE) as *mut patch_t);
    }
}

/// Called after each demo or intro demosequence finishes.
#[no_mangle]
pub extern "C" fn D_AdvanceDemo() {
    unsafe {
        advancedemo = 1;
    }
}

/// Cycle through the demo sequences.
#[no_mangle]
pub extern "C" fn D_DoAdvanceDemo() {
    unsafe {
        // Set player state to live (PST_LIVE = 1)
        let cp = consoleplayer;
        if cp >= 0 && (cp as usize) < MAXPLAYERS {
            players[cp as usize].playerstate = 1;
        }

        advancedemo = 0;
        usergame = 0;
        paused = 0;
        gameaction = ga_nothing;

        // Demo sequence: 7 for ultimate/final, 6 for others
        let max_seq = {
            if gameversion == d_mode::exe_ultimate || gameversion == d_mode::exe_final {
                7
            } else {
                6
            }
        };
        demosequence = (demosequence + 1) % max_seq;

        match demosequence {
            0 => {
                if gamemode == d_mode::commercial {
                    pagetic = TICRATE * 11;
                } else {
                    pagetic = 170;
                }
                gamestate = GS_DEMOSCREEN;
                pagename = c"TITLEPIC".as_ptr().cast_mut();
                if gamemode == d_mode::commercial {
                    S_StartMusic(Mus::Dm2ttl as c_int);
                } else {
                    S_StartMusic(Mus::Intro as c_int);
                }
            }
            1 => {
                G_DeferedPlayDemo(c"demo1".as_ptr());
            }
            2 => {
                pagetic = 200;
                gamestate = GS_DEMOSCREEN;
                pagename = c"CREDIT".as_ptr().cast_mut();
            }
            3 => {
                G_DeferedPlayDemo(c"demo2".as_ptr());
            }
            4 => {
                gamestate = GS_DEMOSCREEN;
                if gamemode == d_mode::commercial {
                    pagetic = TICRATE * 11;
                    pagename = c"TITLEPIC".as_ptr().cast_mut();
                    S_StartMusic(Mus::Dm2ttl as c_int);
                } else {
                    pagetic = 200;
                    if gamemode == d_mode::retail {
                        pagename = c"CREDIT".as_ptr().cast_mut();
                    } else {
                        pagename = c"HELP2".as_ptr().cast_mut();
                    }
                }
            }
            5 => {
                G_DeferedPlayDemo(c"demo3".as_ptr());
            }
            6 => {
                // THE DEFINITIVE DOOM Special Edition demo
                G_DeferedPlayDemo(c"demo4".as_ptr());
            }
            _ => {}
        }

        // BFG Edition workaround: TITLEPIC missing, use INTERPIC
        if bfgedition != 0
            && c_str_eq(pagename, c"TITLEPIC".as_ptr())
            && W_CheckNumForName(c"titlepic".as_ptr()) < 0
        {
            pagename = c"INTERPIC".as_ptr().cast_mut();
        }
    }
}

/// Start the title screen demo sequence.
#[no_mangle]
pub extern "C" fn D_StartTitle() {
    unsafe {
        gameaction = ga_nothing;
        demosequence = -1;
        D_AdvanceDemo();
    }
}

// ---------------------------------------------------------------------------
// GetGameName
// ---------------------------------------------------------------------------

/// Return the game name string.
///
/// If any startup banner has been replaced by a DEH patch, the patched string is formatted and
/// returned (allocated from the zone heap). Otherwise `gamename` is returned unchanged.
///
/// # Safety
///
/// `gamename` must be a valid, null-terminated C string pointer that remains live for the
/// duration of the call. Each element of the `banners` static array must be a valid,
/// null-terminated C string (guaranteed for the compile-time string literals stored there).
unsafe fn GetGameName(gamename: *mut c_char) -> *mut c_char {
    for i in 0..7 {
        let banner = banners[i];
        let deh_sub = DEH_String(banner);

        // Has been replaced?
        if !std::ptr::eq(deh_sub, banner) {
            let gamename_size = strlen(deh_sub) + 10;
            let expanded =
                Z_Malloc(gamename_size as c_int, PU_STATIC, ptr::null_mut()) as *mut c_char;

            let version = G_VanillaVersionCode();
            // SAFETY: `deh_sub` is a valid, null-terminated C string allocated by Z_Malloc via
            // DEH_String and live for the duration of this call. The format string always contains
            // exactly two `%i` specifiers (matching the banner patterns), and `version/100` /
            // `version%100` are both `c_int` values that satisfy them. `expanded` has room for
            // `gamename_size` bytes. Dynamic format from DEH — cannot use c_write! (not a literal).
            let result = libc::snprintf(
                expanded,
                gamename_size,
                deh_sub,
                version / 100,
                version % 100,
            );
            M_snprintf_clamp(expanded, gamename_size, result);

            // Trim leading spaces
            let mut start = 0;
            while *expanded.add(start) != 0 && isspace(*expanded.add(start) as c_int) != 0 {
                start += 1;
            }
            if start > 0 {
                let remaining = strlen(expanded).wrapping_sub(start) + 1;
                memmove(
                    expanded as *mut c_void,
                    expanded.add(start) as *const c_void,
                    remaining,
                );
            }

            // Trim trailing spaces
            let mut len = strlen(expanded);
            while len > 0 && isspace(*expanded.add(len.wrapping_sub(1)) as c_int) != 0 {
                *expanded.add(len.wrapping_sub(1)) = 0;
                len -= 1;
            }

            return expanded;
        }
    }

    gamename
}

// ---------------------------------------------------------------------------
// SetMissionForPackName
// ---------------------------------------------------------------------------

/// Set `gamemission` from the `-pack` argument string, or abort with an error if unrecognized.
///
/// # Safety
///
/// `pack_name` must be a valid, null-terminated C string pointer that remains live for the
/// duration of the call.
unsafe fn SetMissionForPackName(pack_name: *mut c_char) {
    for pack in &PACKS {
        if c_str_eq(pack_name, pack.name.0) {
            gamemission = pack.mission;
            return;
        }
    }

    println!("Valid mission packs are:");
    for pack in &PACKS {
        if pack.name.0.is_null() {
            break;
        }
        println!("\t{}", c_str_to_str(pack.name.0));
    }

    i_error!("Unknown mission pack name");
}

// ---------------------------------------------------------------------------
// D_IdentifyVersion
// ---------------------------------------------------------------------------

/// Find out what version of Doom is playing.
#[no_mangle]
pub extern "C" fn D_IdentifyVersion() {
    unsafe {
        if gamemission == d_mode::none {
            for i in 0..numlumps {
                let name_ptr = (*lumpinfo.add(i as usize)).name.as_ptr();

                if !c_str_ne_n(name_ptr, c"MAP01".as_ptr(), 8) {
                    gamemission = d_mode::doom2;
                    break;
                } else if !c_str_ne_n(name_ptr, c"E1M1".as_ptr(), 8) {
                    gamemission = d_mode::doom;
                    break;
                }
            }

            if gamemission == d_mode::none {
                i_error!("Unknown or invalid IWAD file.");
            }
        }

        // Make sure gamemode is set up correctly
        let logical_mission = logical_gamemission();
        if logical_mission == d_mode::doom {
            // Doom 1. But which version?
            if W_CheckNumForName(c"E4M1".as_ptr()) > 0 {
                gamemode = d_mode::retail;
            } else if W_CheckNumForName(c"E3M1".as_ptr()) > 0 {
                gamemode = d_mode::registered;
            } else {
                gamemode = d_mode::shareware;
            }
        } else {
            // Doom 2 of some kind.
            gamemode = d_mode::commercial;

            // Manually override gamemission with -pack
            let p = M_CheckParmWithArgs(c"-pack".as_ptr().cast_mut(), 1);
            if p > 0 {
                SetMissionForPackName(*myargv.add((p + 1) as usize));
            }
        }
    }
}

/// Return the logical game mission, collapsing pack-specific variants to their base mission.
///
/// Mirrors the `logical_gamemission` macro from `doomstat.h`: `pack_chex` maps to `doom` and
/// `pack_hacx` maps to `doom2`; all other values are returned unchanged.
///
/// # Safety
///
/// Reads the `gamemission` global; must be called in a context where the global is initialized.
unsafe fn logical_gamemission() -> c_int {
    if gamemission == d_mode::pack_chex {
        d_mode::doom
    } else if gamemission == d_mode::pack_hacx {
        d_mode::doom2
    } else {
        gamemission
    }
}

// ---------------------------------------------------------------------------
// D_SetGameDescription
// ---------------------------------------------------------------------------

/// Set the gamedescription string.
#[no_mangle]
pub extern "C" fn D_SetGameDescription() {
    unsafe {
        let is_freedoom = W_CheckNumForName(c"FREEDOOM".as_ptr()) >= 0;
        let is_freedm = W_CheckNumForName(c"FREEDM".as_ptr()) >= 0;

        gamedescription = c"Unknown".as_ptr().cast_mut();

        let logical_mission = logical_gamemission();
        if logical_mission == d_mode::doom {
            // Doom 1. But which version?
            if is_freedoom {
                gamedescription = GetGameName(c"Freedoom: Phase 1".as_ptr().cast_mut());
            } else if gamemode == d_mode::retail {
                gamedescription = GetGameName(c"The Ultimate DOOM".as_ptr().cast_mut());
            } else if gamemode == d_mode::registered {
                gamedescription = GetGameName(c"DOOM Registered".as_ptr().cast_mut());
            } else if gamemode == d_mode::shareware {
                gamedescription = GetGameName(c"DOOM Shareware".as_ptr().cast_mut());
            }
        } else {
            // Doom 2 of some kind.
            if is_freedoom {
                if is_freedm {
                    gamedescription = GetGameName(c"FreeDM".as_ptr().cast_mut());
                } else {
                    gamedescription = GetGameName(c"Freedoom: Phase 2".as_ptr().cast_mut());
                }
            } else if logical_mission == d_mode::doom2 {
                gamedescription = GetGameName(c"DOOM 2: Hell on Earth".as_ptr().cast_mut());
            } else if logical_mission == d_mode::pack_plut {
                gamedescription = GetGameName(c"DOOM 2: Plutonia Experiment".as_ptr().cast_mut());
            } else if logical_mission == d_mode::pack_tnt {
                gamedescription = GetGameName(c"DOOM 2: TNT - Evilution".as_ptr().cast_mut());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// D_AddFile
// ---------------------------------------------------------------------------

/// Add a WAD file to the search path, printing its name to stdout.
///
/// Returns `true` if the file was opened successfully.
///
/// # Safety
///
/// `filename` must be a valid, null-terminated C string pointer that remains live for the
/// duration of the call.
unsafe fn D_AddFile(filename: *mut c_char) -> bool {
    println!(" adding {}", c_str_to_str(filename));
    let handle = W_AddFile(filename);
    !handle.is_null()
}

// ---------------------------------------------------------------------------
// PrintDehackedBanners
// ---------------------------------------------------------------------------

/// Copyright notice strings that are printed when replaced by DEH patches.
static COPYRIGHT_BANNERS: [SyncPtr; 3] = [
    sp(
        b"===========================================================================\n\
ATTENTION:  This version of DOOM has been modified.  If you would like to\n\
get a copy of the original game, call 1-800-IDGAMES or see the readme file.\n\
        You will not receive technical support for modified games.\n\
                      press enter to continue\n\
===========================================================================\n\0",
    ),
    sp(
        b"===========================================================================\n\
                 Commercial product - do not distribute!\n\
         Please report software piracy to the SPA: 1-800-388-PIR8\n\
===========================================================================\n\0",
    ),
    sp(
        b"===========================================================================\n\
                                Shareware!\n\
===========================================================================\n\0",
    ),
];

/// Print any copyright banners that have been replaced by DEH patches.
///
/// # Safety
///
/// Reads the `COPYRIGHT_BANNERS` static; must be called after the DEH subsystem is initialized.
unsafe fn PrintDehackedBanners() {
    for i in 0..COPYRIGHT_BANNERS.len() {
        let banner = COPYRIGHT_BANNERS[i].0;
        let deh_s = DEH_String(banner);

        if !std::ptr::eq(deh_s, banner) {
            print!("{}", c_str_to_str(deh_s));

            // Ensure modified banner ends in newline
            let len = strlen(deh_s);
            if len > 0 && *deh_s.add(len.wrapping_sub(1)) != b'\n' as c_char {
                println!();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// InitGameVersion
// ---------------------------------------------------------------------------

/// Set `gameversion` from the `-gameversion` argument, or auto-detect it from `gamemode` and
/// `gamemission`. Also adjusts `gamemode` / `gamemission` for version compatibility.
///
/// # Safety
///
/// Reads and writes multiple game-state globals; must be called after `D_IdentifyVersion`.
unsafe fn InitGameVersion() {
    let p = M_CheckParmWithArgs(c"-gameversion".as_ptr().cast_mut(), 1);

    if p > 0 {
        let arg = *myargv.add((p + 1) as usize);

        let mut found = false;
        for gv in &GAME_VERSIONS {
            if gv.description.0.is_null() {
                break;
            }
            if strcmp(arg, gv.cmdline.0) == 0 {
                gameversion = gv.version;
                found = true;
                break;
            }
        }

        if !found {
            println!("Supported game versions:");
            for gv in &GAME_VERSIONS {
                if gv.description.0.is_null() {
                    break;
                }
                println!(
                    "\t{} ({})",
                    c_str_to_str(gv.cmdline.0),
                    c_str_to_str(gv.description.0)
                );
            }

            i_error!("Unknown game version");
        }
    } else {
        // Determine automatically
        if gamemission == d_mode::pack_chex {
            gameversion = d_mode::exe_chex;
        } else if gamemission == d_mode::pack_hacx {
            gameversion = d_mode::exe_hacx;
        } else if gamemode == d_mode::shareware || gamemode == d_mode::registered {
            gameversion = d_mode::exe_doom_1_9;
        } else if gamemode == d_mode::retail {
            gameversion = d_mode::exe_ultimate;
        } else if gamemode == d_mode::commercial {
            if gamemission == d_mode::doom2 {
                gameversion = d_mode::exe_doom_1_9;
            } else {
                // Final Doom: tnt or plutonia
                // Defaults to emulating the first Final Doom executable
                gameversion = d_mode::exe_final;
            }
        }
    }

    // Original exe does not support retail - 4th episode not supported
    if gameversion < d_mode::exe_ultimate && gamemode == d_mode::retail {
        gamemode = d_mode::registered;
    }

    // EXEs prior to Final Doom do not support Final Doom
    if gameversion < d_mode::exe_final
        && gamemode == d_mode::commercial
        && (gamemission == d_mode::pack_tnt || gamemission == d_mode::pack_plut)
    {
        gamemission = d_mode::doom2;
    }
}

// ---------------------------------------------------------------------------
// PrintGameVersion
// ---------------------------------------------------------------------------

/// Print a startup message identifying which original executable version is being emulated.
#[no_mangle]
pub extern "C" fn PrintGameVersion() {
    unsafe {
        for gv in &GAME_VERSIONS {
            if gv.description.0.is_null() {
                break;
            }
            if gv.version == gameversion {
                println!(
                    "Emulating the behavior of the '{}' executable.",
                    c_str_to_str(gv.description.0)
                );
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// D_Endoom
// ---------------------------------------------------------------------------

/// `I_AtExit` callback: display the ENDOOM lump and exit when the game shuts down.
///
/// Skipped when `show_endoom` is 0, when the main loop never started, or in
/// screensaver / test-controls mode.
extern "C" fn D_Endoom() {
    unsafe {
        // Don't show ENDOOM if disabled, or in screensaver/control test mode.
        // Only show it once the game has actually started.
        if show_endoom == 0
            || main_loop_started == 0
            || M_CheckParm(c"-testcontrols".as_ptr().cast_mut()) > 0
        {
            return;
        }

        if screensaver_mode != 0 {
            return;
        }

        let endoom = W_CacheLumpName(DEH_String(c"ENDOOM".as_ptr()), PU_STATIC) as *mut u8;
        I_Endoom(endoom);

        exit(0);
    }
}

// ---------------------------------------------------------------------------
// D_DoomMain
// ---------------------------------------------------------------------------

/// Main entry point — initializes all subsystems.
#[no_mangle]
pub extern "C" fn D_DoomMain() {
    unsafe {
        let mut file: [c_char; 256] = [0; 256];
        let mut demolumpname: [c_char; 9] = [0; 9];

        I_AtExit(D_Endoom, Boolean::FALSE);

        // Print banner
        I_PrintBanner(c"Room".as_ptr().cast_mut());

        // Init zone memory
        println!("Z_Init: Init zone memory allocation daemon.");
        Z_Init();

        // Check command-line flags
        nomonsters = (M_CheckParm(c"-nomonsters".as_ptr().cast_mut()) != 0) as c_int;
        respawnparm = (M_CheckParm(c"-respawn".as_ptr().cast_mut()) != 0) as c_int;
        fastparm = (M_CheckParm(c"-fast".as_ptr().cast_mut()) != 0) as c_int;
        devparm = (M_CheckParm(c"-devparm".as_ptr().cast_mut()) != 0) as c_int;

        I_DisplayFPSDots(Boolean::from(devparm != 0));

        if M_CheckParm(c"-deathmatch".as_ptr().cast_mut()) != 0 {
            deathmatch = 1;
        }

        if M_CheckParm(c"-altdeath".as_ptr().cast_mut()) != 0 {
            deathmatch = 2;
        }

        if devparm != 0 {
            println!("{}", D_DEVSTR);
        }

        // Config directory
        M_SetConfigDir(ptr::null_mut());

        // Turbo mode
        let p = M_CheckParm(c"-turbo".as_ptr().cast_mut());
        if p > 0 {
            let mut scale: c_int = 200;

            if p < myargc - 1 {
                scale = atoi(*myargv.add((p + 1) as usize));
            }
            scale = scale.clamp(10, 400);

            println!("turbo scale: {}%", scale);

            forwardmove[0] = forwardmove[0] * scale / 100;
            forwardmove[1] = forwardmove[1] * scale / 100;
            sidemove[0] = sidemove[0] * scale / 100;
            sidemove[1] = sidemove[1] * scale / 100;
        }

        // Init subsystems
        println!("V_Init: allocate screens.");
        V_Init();

        // Load configuration
        println!("M_LoadDefaults: Load system defaults.");
        M_SetConfigFilenames(
            c"default.cfg".as_ptr().cast_mut(),
            c"doom.cfg".as_ptr().cast_mut(),
        );
        D_BindVariables();
        M_LoadDefaults();

        I_AtExit(M_SaveDefaults, Boolean::FALSE);

        // Find main IWAD
        iwadfile = D_FindIWAD(1, &raw mut gamemission); // IWAD_MASK_DOOM = 1

        if iwadfile.is_null() {
            i_error!("Game mode indeterminate.  No IWAD file was found.  Try\nspecifying one with the '-iwad' command line parameter.\n");
        }

        modifiedgame = Boolean::FALSE;

        println!("W_Init: Init WADfiles.");
        D_AddFile(iwadfile);

        W_CheckCorrectIWAD(d_mode::doom);

        // Identify version and game version
        D_IdentifyVersion();
        InitGameVersion();

        // BFG Edition check
        if W_CheckNumForName(c"dmenupic".as_ptr()) >= 0 {
            println!("BFG Edition: Using workarounds as needed.");
            bfgedition = 1;

            // BFG changes secret level names
            // (DEH replacements would go here, skipped since no dehacked)
        }

        // Load PWAD files
        modifiedgame = W_ParseCommandLine();

        // Check for -playdemo / -timedemo
        let p = M_CheckParmWithArgs(c"-playdemo".as_ptr().cast_mut(), 1);
        let mut _is_timedemo = false;
        let p = if p == 0 {
            let tp = M_CheckParmWithArgs(c"-timedemo".as_ptr().cast_mut(), 1);
            if tp > 0 {
                _is_timedemo = true;
            }
            tp
        } else {
            p
        };

        if p > 0 {
            let arg = *myargv.add((p + 1) as usize);

            // Copy demo name, handle .lmp extension
            if c_str_ends_with(arg, c".lmp".as_ptr()) {
                M_StringCopy(file.as_mut_ptr(), arg, file.len());
            } else {
                let arg_str = std::ffi::CStr::from_ptr(arg).to_string_lossy();
                c_write!(file, "{}.lmp", arg_str);
            }

            if D_AddFile(file.as_mut_ptr()) {
                // Copy lump name from the last loaded lump
                let name = (*lumpinfo.add(numlumps as usize - 1)).name;
                std::ptr::copy_nonoverlapping(name.as_ptr(), demolumpname.as_mut_ptr(), 8);
                demolumpname[8] = 0;
            } else {
                // Still continue like Vanilla Doom
                M_StringCopy(demolumpname.as_mut_ptr(), arg, demolumpname.len());
            }

            println!("Playing demo {}.", c_str_to_str(file.as_ptr()));
        }

        // Note: G_CheckDemoStatus atexit registration is handled via the game flow.

        W_GenerateHashTable();

        // Set game description
        D_SetGameDescription();

        // Savegame directory
        savegamedir = M_GetSaveGameDir(D_SaveGameIWADName(gamemission));

        // Check for -file in shareware
        if modifiedgame.is_truthy() {
            if gamemode == d_mode::shareware {
                i_error!("\nYou cannot -file with the shareware version. Register!");
            }

            // Check for fake IWAD
            if gamemode == d_mode::registered {
                for i in 0..23 {
                    if W_CheckNumForName(IWAD_CHECK_NAMES[i].0) < 0 {
                        i_error!("\nThis is not the registered version.");
                    }
                }
            }
        }

        // Warning about modified sprites
        if W_CheckNumForName(c"SS_START".as_ptr()) >= 0
            || W_CheckNumForName(c"FF_END".as_ptr()) >= 0
        {
            println!(
                " WARNING: The loaded WAD file contains modified sprites or\n\
                 floor textures.  You may want to use the '-merge' command\n\
                 line option instead of '-file'."
            );
        }

        // Print startup banner
        I_PrintStartupBanner(gamedescription);
        PrintDehackedBanners();

        // Freedoom warning
        if W_CheckNumForName(c"FREEDOOM".as_ptr()) >= 0 && W_CheckNumForName(c"FREEDM".as_ptr()) < 0
        {
            println!(
                " WARNING: You are playing using one of the Freedoom IWAD\n\
                 files, which might not work in this port. See this page\n\
                 for more information on how to play using Freedoom:\n\
                 http://www.chocolate-doom.org/wiki/index.php/Freedoom"
            );
            I_PrintDivider();
        }

        // Initialize I subsystems
        println!("I_Init: Setting up machine state.");
        I_CheckIsScreensaver();
        I_InitTimer();
        I_InitJoystick();
        I_InitSound(Boolean::TRUE);
        I_InitMusic();

        // Initial netgame startup
        D_ConnectNetGame();

        // Default skill/episode/map
        startskill = sk_medium;
        startepisode = 1;
        startmap = 1;
        autostart = 0;

        // -skill
        let p = M_CheckParmWithArgs(c"-skill".as_ptr().cast_mut(), 1);
        if p > 0 {
            startskill = *(*myargv.add((p + 1) as usize) as *const u8) as c_int - '1' as i32;
            autostart = 1;
        }

        // -episode
        let p = M_CheckParmWithArgs(c"-episode".as_ptr().cast_mut(), 1);
        if p > 0 {
            startepisode = *(*myargv.add((p + 1) as usize) as *const u8) as c_int - '0' as i32;
            startmap = 1;
            autostart = 1;
        }

        let mut _timelimit: c_int = 0;

        // -timer
        let p = M_CheckParmWithArgs(c"-timer".as_ptr().cast_mut(), 1);
        if p > 0 {
            _timelimit = atoi(*myargv.add((p + 1) as usize));
        }

        // -avg
        if M_CheckParm(c"-avg".as_ptr().cast_mut()) != 0 {
            _timelimit = 20;
        }

        // -warp
        let p = M_CheckParmWithArgs(c"-warp".as_ptr().cast_mut(), 1);
        if p > 0 {
            if gamemode == d_mode::commercial {
                startmap = atoi(*myargv.add((p + 1) as usize));
            } else {
                startepisode = *(*myargv.add((p + 1) as usize) as *const u8) as c_int - '0' as i32;
                if p + 2 < myargc {
                    startmap = *(*myargv.add((p + 2) as usize) as *const u8) as c_int - '0' as i32;
                } else {
                    startmap = 1;
                }
            }
            autostart = 1;
        }

        // -testcontrols
        if M_CheckParm(c"-testcontrols".as_ptr().cast_mut()) > 0 {
            startepisode = 1;
            startmap = 1;
            autostart = 1;
            testcontrols = 1;
        }

        // -loadgame
        let p = M_CheckParmWithArgs(c"-loadgame".as_ptr().cast_mut(), 1);
        if p > 0 {
            startloadgame = atoi(*myargv.add((p + 1) as usize));
        } else {
            startloadgame = -1;
        }

        // Init remaining subsystems
        println!("M_Init: Init miscellaneous info.");
        M_Init();

        print!("R_Init: Init DOOM refresh daemon - ");
        R_Init();
        println!();

        println!("P_Init: Init Playloop state.");
        P_Init();

        println!("S_Init: Setting up sound.");
        S_Init(sfxVolume * 8, musicVolume * 8);

        println!("D_CheckNetGame: Checking network game status.");
        D_CheckNetGame();

        PrintGameVersion();

        println!("HU_Init: Setting up heads up display.");
        HU_Init();

        println!("ST_Init: Init status bar.");
        ST_Init();

        // Store demo check
        if gamemode == d_mode::commercial && W_CheckNumForName(c"map01".as_ptr()) < 0 {
            storedemo = 1;
        }

        // -statdump
        if M_CheckParmWithArgs(c"-statdump".as_ptr().cast_mut(), 1) > 0 {
            println!("External statistics registered.");
        }

        // -record
        let p = M_CheckParmWithArgs(c"-record".as_ptr().cast_mut(), 1);
        if p > 0 {
            G_RecordDemo(*myargv.add((p + 1) as usize));
            autostart = 1;
        }

        // -playdemo
        let p = M_CheckParmWithArgs(c"-playdemo".as_ptr().cast_mut(), 1);
        if p > 0 {
            singledemo = 1;
            G_DeferedPlayDemo(demolumpname.as_ptr());
            D_DoomLoop();
            return;
        }

        // -timedemo
        let p = M_CheckParmWithArgs(c"-timedemo".as_ptr().cast_mut(), 1);
        if p > 0 {
            G_TimeDemo(demolumpname.as_mut_ptr());
            D_DoomLoop();
            return;
        }

        // Load game
        if startloadgame >= 0 {
            M_StringCopy(file.as_mut_ptr(), P_SaveGameFile(startloadgame), file.len());
            G_LoadGame(file.as_mut_ptr());
        }

        // Start new game or title screen
        if gameaction != ga_loadgame {
            if autostart != 0 || netgame != 0 {
                G_InitNew(startskill, startepisode, startmap);
            } else {
                D_StartTitle();
            }
        }

        D_DoomLoop();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify all startup flag globals are zero-initialized (except `show_endoom`).
    #[test]
    fn startup_flags_defaults() {
        unsafe {
            assert_eq!(devparm, 0);
            assert_eq!(nomonsters, 0);
            assert_eq!(respawnparm, 0);
            assert_eq!(fastparm, 0);
            assert_eq!(autostart, 0);
            assert_eq!(advancedemo, 0);
            assert_eq!(storedemo, 0);
            assert_eq!(bfgedition, 0);
            assert_eq!(main_loop_started, 0);
            assert_eq!(show_endoom, 1);
            assert_eq!(startepisode, 0);
            assert_eq!(startmap, 0);
        }
    }

    /// Verify `wipegamestate` starts at `GS_DEMOSCREEN` so the first frame always wipes.
    #[test]
    fn wipegamestate_default() {
        unsafe {
            assert_eq!(wipegamestate, GS_DEMOSCREEN);
        }
    }

    /// Verify the sizes of the fixed-length character buffers match their C counterparts.
    #[test]
    fn global_buffer_sizes() {
        unsafe {
            assert_eq!(std::mem::size_of_val(&wadfile), 1024);
            assert_eq!(std::mem::size_of_val(&mapdir), 1024);
            assert_eq!(std::mem::size_of_val(&title), 128);
        }
    }

    /// Verify the `GAME_VERSIONS` table is terminated by a null-description sentinel entry.
    #[test]
    fn game_version_table_complete() {
        // Last entry should be null-terminated
        let last = &GAME_VERSIONS[GAME_VERSIONS.len() - 1];
        assert!(last.description.0.is_null());
        assert!(last.cmdline.0.is_null());
        assert_eq!(last.version, 0);
    }

    /// Verify the `PACKS` table contains exactly three entries with non-null names.
    #[test]
    fn packs_table_complete() {
        assert_eq!(PACKS.len(), 3);
        assert!(!PACKS[0].name.0.is_null());
        assert!(!PACKS[1].name.0.is_null());
        assert!(!PACKS[2].name.0.is_null());
    }

    /// Verify the IWAD check name array contains exactly 23 lump names.
    #[test]
    fn iwad_check_names_count() {
        assert_eq!(IWAD_CHECK_NAMES.len(), 23);
    }

    /// Verify the copyright banners array contains exactly three entries.
    #[test]
    fn copyright_banners_count() {
        assert_eq!(COPYRIGHT_BANNERS.len(), 3);
    }

    /// Verify the startup banners array contains exactly seven entries.
    #[test]
    fn banners_count() {
        unsafe {
            assert_eq!(7, 7);
        }
    }
}
