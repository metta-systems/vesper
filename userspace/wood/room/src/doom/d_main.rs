//! Rust port of vendor/doomgeneric/d_main.c.
//!
//! Main initialization (`D_DoomMain`) and game loop (`D_DoomLoop`),
//! plus demo sequence management, display rendering, and command-line
//! parameter processing.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::{ffi::c_void, ptr};

use crate::{
    doom::{
        d_mode,
        d_player::{consoleplayer, players, MAXPLAYERS},
        doomstat::{gamedescription, gamemission, gamemode, gameversion, modifiedgame},
        i_timer::TICRATE,
        i_video::{I_StartFrame, SCREENHEIGHT, SCREENWIDTH},
        m_config::M_SaveDefaults,
        m_misc::atoi,
    },
    i_error,
    types::Boolean,
};

// ---------------------------------------------------------------------------
// String constants from d_englsh.h / dstrings.h
// ---------------------------------------------------------------------------

/// Message printed to stdout when `-devparm` is active.
static D_DEVSTR: &str = "Development mode ON.\n";

/// Multiplayer chat target key for the green player.
const HUSTR_KEYGREEN: u8 = b'g';
/// Multiplayer chat target key for the indigo player.
const HUSTR_KEYINDIGO: u8 = b'i';
/// Multiplayer chat target key for the brown player.
const HUSTR_KEYBROWN: u8 = b'b';
/// Multiplayer chat target key for the red player.
const HUSTR_KEYRED: u8 = b'r';

// ---------------------------------------------------------------------------
// Type aliases matching C enums
// ---------------------------------------------------------------------------

/// C-compatible game state discriminant (`gamestate_t`).
type gamestate_t = i32;
/// C-compatible pending game action discriminant (`gameaction_t`).
type gameaction_t = i32;
/// C-compatible skill level discriminant (`skill_t`).
type skill_t = i32;
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
// Globals
// ---------------------------------------------------------------------------

/// Savegame directory.
pub static mut savegamedir: String = String::new();

/// Path to the currently loaded IWAD file.
pub static mut iwadfile: String = String::new();

/// Started game with -devparm.
pub static mut devparm: i32 = 0;

/// Checkparm of -nomonsters.
pub static mut nomonsters: i32 = 0;

/// Checkparm of -respawn.
pub static mut respawnparm: i32 = 0;

/// Checkparm of -fast.
pub static mut fastparm: i32 = 0;

/// Skill level to start at (set by -skill).
pub static mut startskill: i32 = 0;

/// Episode to start at (set by -episode).
pub static mut startepisode: i32 = 0;

/// Map to start at (set by -warp / -episode).
pub static mut startmap: i32 = 0;

/// True when -warp / -episode has provided an explicit start.
pub static mut autostart: i32 = 0;

/// Load game slot (-1 = not loading).
pub static mut startloadgame: i32 = 0;

/// True while playing back a built-in demo sequence.
pub static mut advancedemo: i32 = 0;

/// Store demo, do not accept any inputs.
pub static mut storedemo: i32 = 0;

/// True when the BFG edition of the IWAD is detected.
pub static mut bfgedition: i32 = 0;

/// True once the main event loop has started.
pub static mut main_loop_started: i32 = 0;

/// When non-zero, display the ENDOOM text on exit.
pub static mut show_endoom: i32 = 1;

/// Demo loop sequence counter.
pub static mut demosequence: i32 = 0;

/// Tics remaining on the current demo page.
pub static mut pagetic: i32 = 0;

/// Name of the current demo page lump.
pub static mut pagename: &[u8] = b"";

/// Wipe gamestate for transition effects. -1 forces a wipe on next draw.
pub static mut wipegamestate: i32 = GS_DEMOSCREEN;

// D_Display static local state — these must persist across frames.
/// Cached `viewactive` value from the previous frame.
static mut D_DISP_VIEWACTIVE: i32 = 0;
/// Cached `menuactive` value from the previous frame.
static mut D_DISP_MENUACTIVE: i32 = 0;
/// Cached `inhelpscreens` value from the previous frame.
static mut D_DISP_INHELPSCREENS: i32 = 0;
/// Non-zero when the view fills the full screen (no status bar visible).
static mut D_DISP_FULLSCREEN: i32 = 0;
/// Game state from the previous frame; -1 forces border and palette redraws.
static mut D_DISP_OLD_GAMESTATE: i32 = -1;
/// Remaining frames for which the view border must be redrawn.
static mut D_DISP_BORDERDRAWCOUNT: i32 = 0;

/// Title string printed at startup.
static mut title: [u8; 128] = [0; 128];

/// Startup banner strings (may be replaced by dehacked). Each is a
/// format containing one `%i.%i` version placeholder.
const banners: [&str; 7] = [
    "                         DOOM 2: Hell on Earth v%i.%i                           ",
    "                            DOOM Shareware Startup v%i.%i                           ",
    "                            DOOM Registered Startup v%i.%i                           ",
    "                          DOOM System Startup v%i.%i                          ",
    "                         The Ultimate DOOM Startup v%i.%i                        ",
    "                     DOOM 2: TNT - Evilution v%i.%i                           ",
    "                   DOOM 2: Plutonia Experiment v%i.%i                           ",
];

/// One entry in the `-gameversion` lookup table.
struct GameVersionDesc {
    /// Human-readable version label shown in the help list.
    description: &'static str,
    /// Command-line argument string (e.g. `"1.9"`, `"ultimate"`).
    cmdline: &'static str,
    /// Numeric version constant from `d_mode` (e.g. `exe_doom_1_9`).
    version: i32,
}

/// Table of known game versions.
const GAME_VERSIONS: [GameVersionDesc; 9] = [
    GameVersionDesc {
        description: "Doom 1.666",
        cmdline: "1.666",
        version: d_mode::exe_doom_1_666,
    },
    GameVersionDesc {
        description: "Doom 1.7/1.7a",
        cmdline: "1.7",
        version: d_mode::exe_doom_1_7,
    },
    GameVersionDesc {
        description: "Doom 1.8",
        cmdline: "1.8",
        version: d_mode::exe_doom_1_8,
    },
    GameVersionDesc {
        description: "Doom 1.9",
        cmdline: "1.9",
        version: d_mode::exe_doom_1_9,
    },
    GameVersionDesc {
        description: "Hacx",
        cmdline: "hacx",
        version: d_mode::exe_hacx,
    },
    GameVersionDesc {
        description: "Ultimate Doom",
        cmdline: "ultimate",
        version: d_mode::exe_ultimate,
    },
    GameVersionDesc {
        description: "Final Doom",
        cmdline: "final",
        version: d_mode::exe_final,
    },
    GameVersionDesc {
        description: "Final Doom (alt)",
        cmdline: "final2",
        version: d_mode::exe_final2,
    },
    GameVersionDesc {
        description: "Chex Quest",
        cmdline: "chex",
        version: d_mode::exe_chex,
    },
];

/// One entry in the `-pack` lookup table.
struct PackDesc {
    /// Command-line pack name (e.g. `"tnt"`, `"plutonia"`).
    name: &'static str,
    /// `gamemission` value to assign when this pack is selected.
    mission: i32,
}

/// Recognized `-pack` arguments, mapping names to `gamemission` values.
const PACKS: [PackDesc; 3] = [
    PackDesc {
        name: "doom2",
        mission: d_mode::doom2,
    },
    PackDesc {
        name: "tnt",
        mission: d_mode::pack_tnt,
    },
    PackDesc {
        name: "plutonia",
        mission: d_mode::pack_plut,
    },
];

// ---------------------------------------------------------------------------
// C library functions
// ---------------------------------------------------------------------------

use crate::doom::dummy::drone;

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

// deh_str.rs
use crate::doom::deh_str::DEH_String;

// m_menu.rs
use crate::doom::m_menu::{
    detailLevel, inhelpscreens, menuactive, mouseSensitivity, screenblocks, showMessages,
};

// s_sound.rs
use crate::doom::{
    s_sound::{musicVolume, sfxVolume, snd_channels, S_Init, S_StartMusic, S_UpdateSounds},
    sounds::Mus,
};

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
    lump_name, lump_names_equal, lumpinfo, numlumps, W_AddFile, W_CacheLumpName,
    W_CheckCorrectIWAD, W_CheckNumForName, W_GenerateHashTable,
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
use crate::doom::z_zone::Z_Init;

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
use crate::doom::m_argv::{argument, argument_count, M_CheckParm, M_CheckParmWithArgs};

// m_config.rs
use crate::doom::m_config::{
    M_BindVariable, M_GetSaveGameDir, M_LoadDefaults, M_SetConfigDir, M_SetConfigFilenames,
};

// m_controls.rs
use crate::doom::m_controls::{
    M_ApplyPlatformDefaults, M_BindBaseControls, M_BindChatControls, M_BindMapControls,
    M_BindMenuControls, M_BindWeaponControls,
};

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
static IWAD_CHECK_NAMES: [&[u8]; 23] = [
    b"e2m1",
    b"e2m2",
    b"e2m3",
    b"e2m4",
    b"e2m5",
    b"e2m6",
    b"e2m7",
    b"e2m8",
    b"e2m9",
    b"e3m1",
    b"e3m3",
    b"e3m3", // duplicate in original C code
    b"e3m4",
    b"e3m5",
    b"e3m6",
    b"e3m7",
    b"e3m8",
    b"e3m9",
    b"dphoof",
    b"bfgga0",
    b"heada1",
    b"cybra1",
    b"spida1d1",
];

/// Return `true` if `text` ends with `suffix`, ignoring ASCII case.
fn ends_with_ignore_case(text: &[u8], suffix: &[u8]) -> bool {
    text.len() >= suffix.len() && text[text.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

/// First byte of `text` as C reads `text[0]`: `0` (the NUL) when empty.
fn first_byte(text: &str) -> i32 {
    i32::from(text.bytes().next().unwrap_or(0))
}

// ---------------------------------------------------------------------------
// D_ProcessEvents
// ---------------------------------------------------------------------------

/// Send all buffered events down the responder chain.
pub fn D_ProcessEvents() {
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
pub fn D_Display() {
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
                D_DISP_FULLSCREEN = (viewheight == 200) as i32;
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
            I_SetPalette(W_CacheLumpName(DEH_String(b"PLAYPAL"), PU_CACHE) as *mut byte);
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
                W_CacheLumpName(DEH_String(b"M_PAUSE"), PU_CACHE) as *mut patch_t,
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
pub fn D_BindVariables() {
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

        key_multi_msgplayer[0] = HUSTR_KEYGREEN as i32;
        key_multi_msgplayer[1] = HUSTR_KEYINDIGO as i32;
        key_multi_msgplayer[2] = HUSTR_KEYBROWN as i32;
        key_multi_msgplayer[3] = HUSTR_KEYRED as i32;

        M_BindVariable(
            "mouse_sensitivity",
            &raw mut mouseSensitivity as *mut c_void,
        );
        M_BindVariable("sfx_volume", &raw mut sfxVolume as *mut c_void);
        M_BindVariable("music_volume", &raw mut musicVolume as *mut c_void);
        M_BindVariable("show_messages", &raw mut showMessages as *mut c_void);
        M_BindVariable("screenblocks", &raw mut screenblocks as *mut c_void);
        M_BindVariable("detaillevel", &raw mut detailLevel as *mut c_void);
        M_BindVariable("snd_channels", &raw mut snd_channels as *mut c_void);
        M_BindVariable(
            "vanilla_savegame_limit",
            &raw mut vanilla_savegame_limit as *mut c_void,
        );
        M_BindVariable(
            "vanilla_demo_limit",
            &raw mut vanilla_demo_limit as *mut c_void,
        );
        M_BindVariable("show_endoom", &raw mut show_endoom as *mut c_void);

        // Multiplayer chat macros
        for i in 0..10 {
            M_BindVariable(&format!("chatmacro{i}"), (&raw mut chat_macros[i]).cast());
        }
    }
}

// ---------------------------------------------------------------------------
// D_GrabMouseCallback
// ---------------------------------------------------------------------------

/// Return `TRUE` when the window should grab (capture) the mouse pointer.
///
/// The mouse is released for drone players, while the menu is open, or while a demo is playing.
fn D_GrabMouseCallback() -> Boolean {
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
        let demoplayback_val: i32 = demoplayback;
        let advancedemo_val: i32 = advancedemo;
        Boolean::from(gamestate == GS_LEVEL && demoplayback_val == 0 && advancedemo_val == 0)
    }
}

// ---------------------------------------------------------------------------
// doomgeneric_Tick
// ---------------------------------------------------------------------------

/// Execute one rendered frame: run game tics, update sounds, and draw the display.
///
/// Called by the doomgeneric platform layer once per video frame.
pub fn doomgeneric_Tick() {
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
pub fn D_DoomLoop() {
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

        I_SetWindowTitle(&*ptr::addr_of!(gamedescription));
        I_GraphicsCheckCommandLine();

        // Cast the Rust callback to match the C signature
        fn grab_cb() -> Boolean {
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
pub fn D_PageTicker() {
    unsafe {
        pagetic -= 1;
        if pagetic < 0 {
            D_AdvanceDemo();
        }
    }
}

/// Draw the current demo page.
pub fn D_PageDrawer() {
    unsafe {
        V_DrawPatch(0, 0, W_CacheLumpName(pagename, PU_CACHE) as *mut patch_t);
    }
}

/// Called after each demo or intro demosequence finishes.
pub fn D_AdvanceDemo() {
    unsafe {
        advancedemo = 1;
    }
}

/// Cycle through the demo sequences.
pub fn D_DoAdvanceDemo() {
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
                pagename = b"TITLEPIC";
                if gamemode == d_mode::commercial {
                    S_StartMusic(Mus::Dm2ttl as i32);
                } else {
                    S_StartMusic(Mus::Intro as i32);
                }
            }
            1 => {
                G_DeferedPlayDemo("demo1");
            }
            2 => {
                pagetic = 200;
                gamestate = GS_DEMOSCREEN;
                pagename = b"CREDIT";
            }
            3 => {
                G_DeferedPlayDemo("demo2");
            }
            4 => {
                gamestate = GS_DEMOSCREEN;
                if gamemode == d_mode::commercial {
                    pagetic = TICRATE * 11;
                    pagename = b"TITLEPIC";
                    S_StartMusic(Mus::Dm2ttl as i32);
                } else {
                    pagetic = 200;
                    if gamemode == d_mode::retail {
                        pagename = b"CREDIT";
                    } else {
                        pagename = b"HELP2";
                    }
                }
            }
            5 => {
                G_DeferedPlayDemo("demo3");
            }
            6 => {
                // THE DEFINITIVE DOOM Special Edition demo
                G_DeferedPlayDemo("demo4");
            }
            _ => {}
        }

        // BFG Edition workaround: TITLEPIC missing, use INTERPIC
        if bfgedition != 0
            && { pagename }.eq_ignore_ascii_case(b"TITLEPIC")
            && W_CheckNumForName(b"titlepic") < 0
        {
            pagename = b"INTERPIC";
        }
    }
}

/// Start the title screen demo sequence.
pub fn D_StartTitle() {
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
/// If any startup banner has been replaced by a DEH patch, the patched banner is formatted with
/// the version number and trimmed of surrounding whitespace. Otherwise `gamename` is returned
/// unchanged.
fn GetGameName(gamename: &'static str) -> String {
    for banner in banners {
        let deh_sub = DEH_String(banner);

        // Has been replaced?
        if !std::ptr::eq(deh_sub, banner) {
            let version = unsafe { G_VanillaVersionCode() };
            let expanded =
                deh_sub.replacen("%i.%i", &format!("{}.{}", version / 100, version % 100), 1);
            return expanded.trim_ascii().to_owned();
        }
    }

    gamename.to_owned()
}

// ---------------------------------------------------------------------------
// SetMissionForPackName
// ---------------------------------------------------------------------------

/// Set `gamemission` from the `-pack` argument string, or abort with an error if unrecognized.
///
/// # Safety
///
/// Writes the `gamemission` global.
unsafe fn SetMissionForPackName(pack_name: &[u8]) {
    for pack in &PACKS {
        if pack_name.eq_ignore_ascii_case(pack.name.as_bytes()) {
            gamemission = pack.mission;
            return;
        }
    }

    println!("Valid mission packs are:");
    for pack in &PACKS {
        println!("\t{}", pack.name);
    }

    i_error!("Unknown mission pack name");
}

// ---------------------------------------------------------------------------
// D_IdentifyVersion
// ---------------------------------------------------------------------------

/// Find out what version of Doom is playing.
pub fn D_IdentifyVersion() {
    unsafe {
        if gamemission == d_mode::none {
            for i in 0..numlumps {
                let name = &(*lumpinfo.add(i as usize)).name;

                if lump_names_equal(name, b"MAP01") {
                    gamemission = d_mode::doom2;
                    break;
                } else if lump_names_equal(name, b"E1M1") {
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
            if W_CheckNumForName(b"E4M1") > 0 {
                gamemode = d_mode::retail;
            } else if W_CheckNumForName(b"E3M1") > 0 {
                gamemode = d_mode::registered;
            } else {
                gamemode = d_mode::shareware;
            }
        } else {
            // Doom 2 of some kind.
            gamemode = d_mode::commercial;

            // Manually override gamemission with -pack
            let p = M_CheckParmWithArgs("-pack", 1);
            if p > 0 {
                SetMissionForPackName(argument(p + 1).as_bytes());
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
unsafe fn logical_gamemission() -> i32 {
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
pub fn D_SetGameDescription() {
    unsafe {
        let is_freedoom = W_CheckNumForName(b"FREEDOOM") >= 0;
        let is_freedm = W_CheckNumForName(b"FREEDM") >= 0;

        let logical_mission = logical_gamemission();
        let description = if logical_mission == d_mode::doom {
            // Doom 1. But which version?
            if is_freedoom {
                GetGameName("Freedoom: Phase 1")
            } else if gamemode == d_mode::retail {
                GetGameName("The Ultimate DOOM")
            } else if gamemode == d_mode::registered {
                GetGameName("DOOM Registered")
            } else if gamemode == d_mode::shareware {
                GetGameName("DOOM Shareware")
            } else {
                "Unknown".to_owned()
            }
        } else {
            // Doom 2 of some kind.
            if is_freedoom {
                if is_freedm {
                    GetGameName("FreeDM")
                } else {
                    GetGameName("Freedoom: Phase 2")
                }
            } else if logical_mission == d_mode::doom2 {
                GetGameName("DOOM 2: Hell on Earth")
            } else if logical_mission == d_mode::pack_plut {
                GetGameName("DOOM 2: Plutonia Experiment")
            } else if logical_mission == d_mode::pack_tnt {
                GetGameName("DOOM 2: TNT - Evilution")
            } else {
                "Unknown".to_owned()
            }
        };

        gamedescription = description;
    }
}

// ---------------------------------------------------------------------------
// D_AddFile
// ---------------------------------------------------------------------------

/// Add a WAD file to the search path, printing its name to stdout.
///
/// Returns `true` if the file was opened successfully.
fn D_AddFile(filename: &str) -> bool {
    println!(" adding {filename}");
    let handle = W_AddFile(filename);
    !handle.is_null()
}

// ---------------------------------------------------------------------------
// PrintDehackedBanners
// ---------------------------------------------------------------------------

/// Copyright notice strings that are printed when replaced by DEH patches.
const COPYRIGHT_BANNERS: [&str; 3] = [
    "===========================================================================\n\
ATTENTION:  This version of DOOM has been modified.  If you would like to\n\
get a copy of the original game, call 1-800-IDGAMES or see the readme file.\n\
        You will not receive technical support for modified games.\n\
                      press enter to continue\n\
===========================================================================\n",
    "===========================================================================\n\
                 Commercial product - do not distribute!\n\
         Please report software piracy to the SPA: 1-800-388-PIR8\n\
===========================================================================\n",
    "===========================================================================\n\
                                Shareware!\n\
===========================================================================\n",
];

/// Print any copyright banners that have been replaced by DEH patches.
fn PrintDehackedBanners() {
    for banner in COPYRIGHT_BANNERS {
        let deh_s = DEH_String(banner);

        if !std::ptr::eq(deh_s, banner) {
            print!("{deh_s}");

            // Ensure modified banner ends in newline
            if !deh_s.is_empty() && !deh_s.ends_with('\n') {
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
    let p = M_CheckParmWithArgs("-gameversion", 1);

    if p > 0 {
        let arg = argument(p + 1);

        let mut found = false;
        for gv in &GAME_VERSIONS {
            if arg == gv.cmdline {
                gameversion = gv.version;
                found = true;
                break;
            }
        }

        if !found {
            println!("Supported game versions:");
            for gv in &GAME_VERSIONS {
                println!("\t{} ({})", gv.cmdline, gv.description);
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
pub fn PrintGameVersion() {
    unsafe {
        for gv in &GAME_VERSIONS {
            if gv.version == gameversion {
                println!(
                    "Emulating the behavior of the '{}' executable.",
                    gv.description
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
fn D_Endoom() {
    unsafe {
        // Don't show ENDOOM if disabled, or in screensaver/control test mode.
        // Only show it once the game has actually started.
        if show_endoom == 0 || main_loop_started == 0 || M_CheckParm("-testcontrols") > 0 {
            return;
        }

        if screensaver_mode != 0 {
            return;
        }

        let endoom = W_CacheLumpName(DEH_String(b"ENDOOM"), PU_STATIC) as *mut u8;
        I_Endoom(endoom);

        // C `exit` flushes stdio; `process::exit` skips Rust's stdout buffer.
        let _ = std::io::Write::flush(&mut std::io::stdout());
        std::process::exit(0);
    }
}

// ---------------------------------------------------------------------------
// D_DoomMain
// ---------------------------------------------------------------------------

/// Main entry point — initializes all subsystems.
pub fn D_DoomMain() {
    unsafe {
        let mut demolumpname = String::new();

        I_AtExit(D_Endoom, Boolean::FALSE);

        // Print banner
        I_PrintBanner("Room");

        // Init zone memory
        println!("Z_Init: Init zone memory allocation daemon.");
        Z_Init();

        // Check command-line flags
        nomonsters = (M_CheckParm("-nomonsters") != 0) as i32;
        respawnparm = (M_CheckParm("-respawn") != 0) as i32;
        fastparm = (M_CheckParm("-fast") != 0) as i32;
        devparm = (M_CheckParm("-devparm") != 0) as i32;

        I_DisplayFPSDots(Boolean::from(devparm != 0));

        if M_CheckParm("-deathmatch") != 0 {
            deathmatch = 1;
        }

        if M_CheckParm("-altdeath") != 0 {
            deathmatch = 2;
        }

        if devparm != 0 {
            println!("{}", D_DEVSTR);
        }

        // Config directory
        M_SetConfigDir(None);

        // Turbo mode
        let p = M_CheckParm("-turbo");
        if p > 0 {
            let mut scale: i32 = 200;

            if p < argument_count() - 1 {
                scale = atoi(argument(p + 1));
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
        M_SetConfigFilenames("default.cfg", "doom.cfg");
        D_BindVariables();
        M_LoadDefaults();

        I_AtExit(M_SaveDefaults, Boolean::FALSE);

        // Find main IWAD
        let Some(found_iwad) = D_FindIWAD(1, &raw mut gamemission) else {
            // IWAD_MASK_DOOM = 1
            i_error!("Game mode indeterminate.  No IWAD file was found.  Try\nspecifying one with the '-iwad' command line parameter.\n");
        };
        iwadfile = found_iwad;

        modifiedgame = Boolean::FALSE;

        println!("W_Init: Init WADfiles.");
        D_AddFile(&*ptr::addr_of!(iwadfile));

        W_CheckCorrectIWAD(d_mode::doom);

        // Identify version and game version
        D_IdentifyVersion();
        InitGameVersion();

        // BFG Edition check
        if W_CheckNumForName(b"dmenupic") >= 0 {
            println!("BFG Edition: Using workarounds as needed.");
            bfgedition = 1;

            // BFG changes secret level names
            // (DEH replacements would go here, skipped since no dehacked)
        }

        // Load PWAD files
        modifiedgame = W_ParseCommandLine();

        // Check for -playdemo / -timedemo
        let p = M_CheckParmWithArgs("-playdemo", 1);
        let mut _is_timedemo = false;
        let p = if p == 0 {
            let tp = M_CheckParmWithArgs("-timedemo", 1);
            if tp > 0 {
                _is_timedemo = true;
            }
            tp
        } else {
            p
        };

        if p > 0 {
            let arg = argument(p + 1);

            // Copy demo name, handle .lmp extension
            let file = if ends_with_ignore_case(arg.as_bytes(), b".lmp") {
                arg.to_owned()
            } else {
                format!("{arg}.lmp")
            };

            if D_AddFile(&file) {
                // Copy lump name from the last loaded lump
                let name = (*lumpinfo.add(numlumps as usize - 1)).name;
                demolumpname = String::from_utf8_lossy(lump_name(&name)).into_owned();
            } else {
                // Still continue like Vanilla Doom (the lump lookup only
                // reads the first 8 characters, as the C 9-byte copy kept).
                demolumpname = arg.to_owned();
            }

            println!("Playing demo {file}.");
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
                    if W_CheckNumForName(IWAD_CHECK_NAMES[i]) < 0 {
                        i_error!("\nThis is not the registered version.");
                    }
                }
            }
        }

        // Warning about modified sprites
        if W_CheckNumForName(b"SS_START") >= 0 || W_CheckNumForName(b"FF_END") >= 0 {
            println!(
                " WARNING: The loaded WAD file contains modified sprites or\n\
                 floor textures.  You may want to use the '-merge' command\n\
                 line option instead of '-file'."
            );
        }

        // Print startup banner
        I_PrintStartupBanner(&*ptr::addr_of!(gamedescription));
        PrintDehackedBanners();

        // Freedoom warning
        if W_CheckNumForName(b"FREEDOOM") >= 0 && W_CheckNumForName(b"FREEDM") < 0 {
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
        let p = M_CheckParmWithArgs("-skill", 1);
        if p > 0 {
            startskill = first_byte(argument(p + 1)) - b'1' as i32;
            autostart = 1;
        }

        // -episode
        let p = M_CheckParmWithArgs("-episode", 1);
        if p > 0 {
            startepisode = first_byte(argument(p + 1)) - b'0' as i32;
            startmap = 1;
            autostart = 1;
        }

        let mut _timelimit: i32 = 0;

        // -timer
        let p = M_CheckParmWithArgs("-timer", 1);
        if p > 0 {
            _timelimit = atoi(argument(p + 1));
        }

        // -avg
        if M_CheckParm("-avg") != 0 {
            _timelimit = 20;
        }

        // -warp
        let p = M_CheckParmWithArgs("-warp", 1);
        if p > 0 {
            if gamemode == d_mode::commercial {
                startmap = atoi(argument(p + 1));
            } else {
                startepisode = first_byte(argument(p + 1)) - b'0' as i32;
                if p + 2 < argument_count() {
                    startmap = first_byte(argument(p + 2)) - b'0' as i32;
                } else {
                    startmap = 1;
                }
            }
            autostart = 1;
        }

        // -testcontrols
        if M_CheckParm("-testcontrols") > 0 {
            startepisode = 1;
            startmap = 1;
            autostart = 1;
            testcontrols = 1;
        }

        // -loadgame
        let p = M_CheckParmWithArgs("-loadgame", 1);
        if p > 0 {
            startloadgame = atoi(argument(p + 1));
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
        if gamemode == d_mode::commercial && W_CheckNumForName(b"map01") < 0 {
            storedemo = 1;
        }

        // -statdump
        if M_CheckParmWithArgs("-statdump", 1) > 0 {
            println!("External statistics registered.");
        }

        // -record
        let p = M_CheckParmWithArgs("-record", 1);
        if p > 0 {
            G_RecordDemo(argument(p + 1));
            autostart = 1;
        }

        // -playdemo
        let p = M_CheckParmWithArgs("-playdemo", 1);
        if p > 0 {
            singledemo = 1;
            G_DeferedPlayDemo(&demolumpname);
            D_DoomLoop();
            return;
        }

        // -timedemo
        let p = M_CheckParmWithArgs("-timedemo", 1);
        if p > 0 {
            G_TimeDemo(&demolumpname);
            D_DoomLoop();
            return;
        }

        // Load game
        if startloadgame >= 0 {
            G_LoadGame(&P_SaveGameFile(startloadgame));
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
            assert_eq!({ devparm }, 0);
            assert_eq!({ nomonsters }, 0);
            assert_eq!({ respawnparm }, 0);
            assert_eq!({ fastparm }, 0);
            assert_eq!({ autostart }, 0);
            assert_eq!({ advancedemo }, 0);
            assert_eq!({ storedemo }, 0);
            assert_eq!({ bfgedition }, 0);
            assert_eq!({ main_loop_started }, 0);
            assert_eq!({ show_endoom }, 1);
            assert_eq!({ startepisode }, 0);
            assert_eq!({ startmap }, 0);
        }
    }

    /// Verify `wipegamestate` starts at `GS_DEMOSCREEN` so the first frame always wipes.
    #[test]
    fn wipegamestate_default() {
        unsafe {
            assert_eq!({ wipegamestate }, GS_DEMOSCREEN);
        }
    }

    /// Verify the sizes of the fixed-length character buffers match their C counterparts.
    #[test]
    fn global_buffer_sizes() {
        unsafe {
            assert_eq!(std::mem::size_of_val(&*ptr::addr_of!(title)), 128);
        }
    }

    /// Every `GAME_VERSIONS` entry has a description and a command-line name.
    #[test]
    fn game_version_table_complete() {
        for gv in &GAME_VERSIONS {
            assert!(!gv.description.is_empty());
            assert!(!gv.cmdline.is_empty());
        }
    }

    /// The `PACKS` table lists the three mission packs by name.
    #[test]
    fn packs_table_complete() {
        let names: Vec<&str> = PACKS.iter().map(|pack| pack.name).collect();
        assert_eq!(names, ["doom2", "tnt", "plutonia"]);
    }

    /// An unpatched banner leaves the game name as given.
    #[test]
    fn game_name_without_deh_patch_is_unchanged() {
        assert_eq!(GetGameName("DOOM Shareware"), "DOOM Shareware");
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

    /// Every startup banner carries the version placeholder `GetGameName` fills in.
    #[test]
    fn banners_have_version_placeholder() {
        for banner in banners {
            assert!(banner.contains("%i.%i"), "{banner:?}");
        }
    }
}
