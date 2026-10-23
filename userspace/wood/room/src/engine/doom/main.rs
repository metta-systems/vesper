//! Rust port of vendor/doomgeneric/d_main.c.
//!
//! Main initialization (`D_DoomMain`) and game loop (`D_DoomLoop`),
//! plus demo sequence management, display rendering, and command-line
//! parameter processing.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::{
    engine::{
        doom,
        doom::{
            player::{MAXPLAYERS, consoleplayer},
            stat::{STAT, gamemission, gamemode, gameversion, modifiedgame},
        },
        global::Global,
        interface::{
            timer::TICRATE,
            video::{I_StartFrame, SCREENHEIGHT, SCREENWIDTH},
        },
        misc::{atoi, config::M_SaveDefaults},
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

use crate::engine::zone::{PU_CACHE, PU_STATIC};

// ---------------------------------------------------------------------------
// Globals
// ---------------------------------------------------------------------------

/// Startup and frame state (C `d_main.c` globals).
pub struct DoomMainState {
    /// Savegame directory.
    pub savegamedir: String,
    /// Path to the currently loaded IWAD file.
    pub iwadfile: String,
    /// Started game with -devparm.
    pub devparm: i32,
    /// Checkparm of -nomonsters.
    pub nomonsters: i32,
    /// Checkparm of -respawn.
    pub respawnparm: i32,
    /// Checkparm of -fast.
    pub fastparm: i32,
    /// Skill level to start at (set by -skill).
    pub startskill: i32,
    /// Episode to start at (set by -episode).
    pub startepisode: i32,
    /// Map to start at (set by -warp / -episode).
    pub startmap: i32,
    /// True when -warp / -episode has provided an explicit start.
    pub autostart: i32,
    /// Load game slot (-1 = not loading).
    pub startloadgame: i32,
    /// True while playing back a built-in demo sequence.
    pub advancedemo: i32,
    /// Store demo, do not accept any inputs.
    pub storedemo: i32,
    /// True when the BFG edition of the IWAD is detected.
    pub bfgedition: i32,
    /// True once the main event loop has started.
    pub main_loop_started: i32,
    /// When non-zero, display the ENDOOM text on exit. Bound to the
    /// `show_endoom` config variable by address.
    pub show_endoom: i32,
    /// Demo loop sequence counter.
    pub demosequence: i32,
    /// Tics remaining on the current demo page.
    pub pagetic: i32,
    /// Name of the current demo page lump.
    pub pagename: &'static [u8],
    /// Wipe gamestate for transition effects. -1 forces a wipe on next draw.
    pub wipegamestate: i32,
    // D_Display static locals: these must persist across frames.
    /// Cached `viewactive` value from the previous frame.
    display_viewactive: i32,
    /// Cached `menuactive` value from the previous frame.
    display_menuactive: i32,
    /// Cached `inhelpscreens` value from the previous frame.
    display_inhelpscreens: i32,
    /// Non-zero when the view fills the full screen (no status bar visible).
    display_fullscreen: i32,
    /// Game state from the previous frame; -1 forces border and palette
    /// redraws.
    display_old_gamestate: i32,
    /// Remaining frames for which the view border must be redrawn.
    display_borderdrawcount: i32,
}

impl DoomMainState {
    /// The state at program start, before the command line is parsed.
    pub const fn new() -> Self {
        Self {
            savegamedir: String::new(),
            iwadfile: String::new(),
            devparm: 0,
            nomonsters: 0,
            respawnparm: 0,
            fastparm: 0,
            startskill: 0,
            startepisode: 0,
            startmap: 0,
            autostart: 0,
            startloadgame: 0,
            advancedemo: 0,
            storedemo: 0,
            bfgedition: 0,
            main_loop_started: 0,
            show_endoom: 1,
            demosequence: 0,
            pagetic: 0,
            pagename: b"",
            wipegamestate: GS_DEMOSCREEN,
            display_viewactive: 0,
            display_menuactive: 0,
            display_inhelpscreens: 0,
            display_fullscreen: 0,
            display_old_gamestate: -1,
            display_borderdrawcount: 0,
        }
    }
}

impl Default for DoomMainState {
    fn default() -> Self {
        Self::new()
    }
}

/// The startup and frame state.
pub static DOOM_MAIN: Global<DoomMainState> = Global::new(DoomMainState::new());

/// Checkparm of -nomonsters (C `nomonsters`).
pub fn nomonsters() -> i32 {
    DOOM_MAIN.borrow().nomonsters
}

/// Checkparm of -respawn (C `respawnparm`).
pub fn respawnparm() -> i32 {
    DOOM_MAIN.borrow().respawnparm
}

/// Checkparm of -fast (C `fastparm`).
pub fn fastparm() -> i32 {
    DOOM_MAIN.borrow().fastparm
}

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
        version: doom::mode::exe_doom_1_666,
    },
    GameVersionDesc {
        description: "Doom 1.7/1.7a",
        cmdline: "1.7",
        version: doom::mode::exe_doom_1_7,
    },
    GameVersionDesc {
        description: "Doom 1.8",
        cmdline: "1.8",
        version: doom::mode::exe_doom_1_8,
    },
    GameVersionDesc {
        description: "Doom 1.9",
        cmdline: "1.9",
        version: doom::mode::exe_doom_1_9,
    },
    GameVersionDesc {
        description: "Hacx",
        cmdline: "hacx",
        version: doom::mode::exe_hacx,
    },
    GameVersionDesc {
        description: "Ultimate Doom",
        cmdline: "ultimate",
        version: doom::mode::exe_ultimate,
    },
    GameVersionDesc {
        description: "Final Doom",
        cmdline: "final",
        version: doom::mode::exe_final,
    },
    GameVersionDesc {
        description: "Final Doom (alt)",
        cmdline: "final2",
        version: doom::mode::exe_final2,
    },
    GameVersionDesc {
        description: "Chex Quest",
        cmdline: "chex",
        version: doom::mode::exe_chex,
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
        mission: doom::mode::doom2,
    },
    PackDesc {
        name: "tnt",
        mission: doom::mode::pack_tnt,
    },
    PackDesc {
        name: "plutonia",
        mission: doom::mode::pack_plut,
    },
];

// ---------------------------------------------------------------------------
// C library functions
// ---------------------------------------------------------------------------

use crate::engine::doom::net::drone;

// g_game.rs
use crate::engine::game::{
    G_BeginRecording, G_DeferedPlayDemo, G_InitNew, G_LoadGame, G_RecordDemo, G_Responder,
    G_TimeDemo, G_VanillaVersionCode, GAME, demoplayback, displayplayer, gameaction, gamestate,
    netgame, paused, player,
};

// am_map.rs
use crate::engine::automap::{AM_Drawer, automap_active};

// deh_str.rs
use crate::engine::dehacked::DEH_String;

// m_menu.rs
use crate::engine::misc::menu::{MENU, menu_active};

// s_sound.rs
use crate::engine::sound::{S_Init, S_StartMusic, S_UpdateSounds, SOUND, SoundState, info::Mus};

// r_main.rs
use crate::engine::render_3d::main::{R_ExecuteSetViewSize, R_Init, R_RenderPlayerView, VIEW};

// r_draw.rs
use crate::engine::render_3d::draw::{DRAW, R_DrawViewBorder, R_FillBackScreen};

// i_video.rs
use crate::engine::interface::video::{VIDEO, screensaver_mode};

// w_wad.rs
use crate::engine::wad::{
    W_AddFile, W_CacheLumpName, W_CheckCorrectIWAD, W_CheckNumForName, W_GenerateHashTable,
    W_NumLumps, lump_name, lump_names_equal, lumpinfo,
};

// hu_stuff.rs
use crate::engine::hud::stuff::{HU_Drawer, HU_Init, HUD};

// m_controls.rs
use crate::engine::misc::controls::CONTROLS;

// wi_stuff.rs
use crate::engine::intermission::WI_Drawer;

// st_stuff.rs
use crate::engine::status::stuff::{ST_Drawer, ST_Init};

// p_setup.rs
use crate::engine::play::setup::P_Init;

// p_saveg.rs
use crate::engine::play::saveg::P_SaveGameFile;

// statdump.rs

// d_net.rs
use crate::engine::doom::net::{D_CheckNetGame, D_ConnectNetGame};

// d_event.rs
use crate::engine::doom::event::D_PopEvent;

// d_loop.rs
use crate::engine::doom::main_loop::{D_StartGameLoop, NetUpdate, TryRunTics, gametic};

// d_iwad.rs
use crate::engine::doom::iwad::{D_FindIWAD, D_SaveGameIWADName};

// z_zone.rs
use crate::engine::zone::Z_Init;

// w_main.rs
use crate::engine::wad::{format::patch_t, main::W_ParseCommandLine};

// v_video.rs
use crate::engine::render_2d::{
    V_DrawMouseSpeedBox, V_DrawPatch, V_DrawPatchDirect, V_Init, V_RestoreBuffer,
};

// f_finale.rs
use crate::engine::finale::F_Drawer;

// f_wipe.rs
use crate::engine::finale::wipe::{wipe_EndScreen, wipe_ScreenWipe, wipe_StartScreen};

// m_argv.rs
use crate::engine::misc::argv::{M_CheckParm, M_CheckParmWithArgs, argument, argument_count};

// m_config.rs
use crate::engine::misc::config::{
    M_BindVariable, M_GetSaveGameDir, M_LoadDefaults, M_SetConfigDir, M_SetConfigFilenames,
};

// m_controls.rs
use crate::engine::misc::controls::{
    M_ApplyPlatformDefaults, M_BindBaseControls, M_BindChatControls, M_BindMapControls,
    M_BindMenuControls, M_BindWeaponControls,
};

// m_menu.rs
use crate::engine::misc::menu::{M_Drawer, M_Init, M_Responder};

// i_timer.rs
use crate::engine::interface::timer::I_GetTime;

// i_system.rs
use crate::engine::interface::system::{
    I_AtExit, I_PrintBanner, I_PrintDivider, I_PrintStartupBanner,
};

// i_sound.rs
use crate::engine::interface::sound::{I_BindSoundVariables, I_InitMusic, I_InitSound};

// i_video.rs
use crate::engine::interface::video::{
    I_BindVideoVariables, I_CheckIsScreensaver, I_DisplayFPSDots, I_EnableLoadingDisk,
    I_FinishUpdate, I_GraphicsCheckCommandLine, I_InitGraphics, I_SetGrabMouseCallback,
    I_SetPalette, I_SetWindowTitle, I_UpdateNoBlit,
};

// i_timer.rs
use crate::engine::interface::timer::{I_InitTimer, I_Sleep};

// i_joystick.rs
use crate::engine::interface::joystick::{I_BindJoystickVariables, I_InitJoystick};

// i_endoom.rs
use crate::engine::interface::endoom::I_Endoom;

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
        if DOOM_MAIN.borrow().storedemo != 0 {
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
        let (nodrawers, gamestate) = {
            let game = GAME.borrow();
            (game.nodrawers, game.gamestate)
        };
        if nodrawers != 0 {
            return;
        }

        let mut redrawsbar = false;

        // Change the view size if needed
        if VIEW.borrow().setsizeneeded.is_truthy() {
            R_ExecuteSetViewSize();
            let mut main = DOOM_MAIN.borrow_mut();
            main.display_old_gamestate = -1; // force background redraw
            main.display_borderdrawcount = 3;
        }

        // Save the current screen if about to wipe
        let wipe = gamestate != DOOM_MAIN.borrow().wipegamestate;
        if wipe {
            wipe_StartScreen(0, 0, SCREENWIDTH, SCREENHEIGHT);
        }

        // Buffered drawing based on game state
        if gamestate == GS_LEVEL {
            if gametic() == 0 {
                // Skip level rendering
            } else {
                if automap_active() {
                    AM_Drawer();
                }
                let (fullscreen, previous_inhelpscreens) = {
                    let main = DOOM_MAIN.borrow();
                    (main.display_fullscreen, main.display_inhelpscreens)
                };
                let viewheight = DRAW.borrow().viewheight;
                if wipe || (viewheight != 200 && fullscreen != 0) {
                    redrawsbar = true;
                }
                if previous_inhelpscreens != 0 && MENU.borrow().inhelpscreens == 0 {
                    redrawsbar = true;
                }
                ST_Drawer(Boolean::from(viewheight == 200), Boolean::from(redrawsbar));
                DOOM_MAIN.borrow_mut().display_fullscreen = (viewheight == 200) as i32;
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
        if gamestate == GS_LEVEL && !automap_active() && gametic() != 0 {
            let dp = displayplayer() as usize;
            if dp < MAXPLAYERS {
                R_RenderPlayerView(player(dp));
            }
            HU_Drawer();
        }

        // Set palette on state change (non-level states)
        let old_gamestate = DOOM_MAIN.borrow().display_old_gamestate;
        if gamestate != GS_LEVEL && gamestate != old_gamestate {
            I_SetPalette(W_CacheLumpName(DEH_String(b"PLAYPAL"), PU_CACHE) as *mut byte);
        }

        // See if the border needs to be initially drawn
        if gamestate == GS_LEVEL && old_gamestate != GS_LEVEL {
            DOOM_MAIN.borrow_mut().display_viewactive = 0;
            R_FillBackScreen();
        }

        // See if the border needs to be updated
        if gamestate == GS_LEVEL && !automap_active() && DRAW.borrow().scaledviewwidth != 320 {
            let border_needs_redraw = {
                let mut main = DOOM_MAIN.borrow_mut();
                if menu_active() || main.display_menuactive != 0 || main.display_viewactive == 0 {
                    main.display_borderdrawcount = 3;
                }
                main.display_borderdrawcount != 0
            };
            if border_needs_redraw {
                R_DrawViewBorder();
                DOOM_MAIN.borrow_mut().display_borderdrawcount -= 1;
            }
        }

        // Test controls: show mouse speed box
        let (testcontrols, testcontrols_mousespeed) = {
            let game = GAME.borrow();
            (game.testcontrols, game.testcontrols_mousespeed)
        };
        if testcontrols != 0 {
            V_DrawMouseSpeedBox(testcontrols_mousespeed);
        }

        // Update static state trackers
        {
            let (menuactive, inhelpscreens) = {
                let menu = MENU.borrow();
                (menu.menuactive, menu.inhelpscreens)
            };
            let viewactive = GAME.borrow().viewactive;
            let mut main = DOOM_MAIN.borrow_mut();
            main.display_menuactive = menuactive;
            main.display_viewactive = viewactive;
            main.display_inhelpscreens = inhelpscreens;
            main.display_old_gamestate = gamestate;
            main.wipegamestate = gamestate;
        }

        // Draw pause pic
        if paused() != 0 {
            let (viewwindowx, viewwindowy, scaledviewwidth) = {
                let draw = DRAW.borrow();
                (draw.viewwindowx, draw.viewwindowy, draw.scaledviewwidth)
            };
            let y = if automap_active() { 4 } else { viewwindowy + 4 };
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

        {
            let mut controls = CONTROLS.borrow_mut();
            controls.key_multi_msgplayer[0] = HUSTR_KEYGREEN as i32;
            controls.key_multi_msgplayer[1] = HUSTR_KEYINDIGO as i32;
            controls.key_multi_msgplayer[2] = HUSTR_KEYBROWN as i32;
            controls.key_multi_msgplayer[3] = HUSTR_KEYRED as i32;
        }

        // The config table keeps these pointers, so they take the stable
        // addresses of the owning states' fields.
        let menu = MENU.as_ptr();
        let sound = SOUND.as_ptr();
        M_BindVariable(
            "mouse_sensitivity",
            (&raw mut (*menu).mouseSensitivity).cast(),
        );
        M_BindVariable("sfx_volume", (&raw mut (*sound).sfxVolume).cast());
        M_BindVariable("music_volume", (&raw mut (*sound).musicVolume).cast());
        M_BindVariable("show_messages", (&raw mut (*menu).showMessages).cast());
        M_BindVariable("screenblocks", (&raw mut (*menu).screenblocks).cast());
        M_BindVariable("detaillevel", (&raw mut (*menu).detailLevel).cast());
        M_BindVariable("snd_channels", (&raw mut (*sound).snd_channels).cast());
        let game = GAME.as_ptr();
        M_BindVariable(
            "vanilla_savegame_limit",
            (&raw mut (*game).vanilla_savegame_limit).cast(),
        );
        M_BindVariable(
            "vanilla_demo_limit",
            (&raw mut (*game).vanilla_demo_limit).cast(),
        );
        M_BindVariable(
            "show_endoom",
            (&raw mut (*DOOM_MAIN.as_ptr()).show_endoom).cast(),
        );

        // Multiplayer chat macros
        for i in 0..10 {
            // The config table keeps this pointer, so it takes the stable address.
            M_BindVariable(
                &format!("chatmacro{i}"),
                (&raw mut (*HUD.as_ptr()).chat_macros[i]).cast(),
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
fn D_GrabMouseCallback() -> Boolean {
    // Drone players don't need mouse focus
    if drone != 0 {
        return Boolean::FALSE;
    }

    // When menu is active or game is paused, release the mouse
    if menu_active() || paused() != 0 {
        return Boolean::FALSE;
    }

    // Only grab mouse when playing levels (but not demos)
    let demoplayback_val: i32 = demoplayback();
    let advancedemo_val: i32 = DOOM_MAIN.borrow().advancedemo;
    Boolean::from(gamestate() == GS_LEVEL && demoplayback_val == 0 && advancedemo_val == 0)
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
        let console = consoleplayer();
        let mo = if console >= 0 && (console as usize) < MAXPLAYERS {
            (*player(console as usize)).mo
        } else {
            std::ptr::null_mut()
        };
        S_UpdateSounds(mo);

        // Update display
        if VIDEO.borrow().screenvisible != 0 {
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
        if DOOM_MAIN.borrow().bfgedition != 0 {
            let is_recording = GAME.borrow().demorecording;
            let is_playdemo = gameaction() == ga_playdemo;
            if is_recording != 0 || is_playdemo || netgame() != 0 {
                eprintln!(
                    " WARNING: You are playing using one of the Doom Classic\n\
                     IWAD files shipped with the Doom 3: BFG Edition. These are\n\
                     known to be incompatible with the regular IWAD files and\n\
                     may cause demos and network games to get out of sync."
                );
            }
        }

        if GAME.borrow().demorecording != 0 {
            G_BeginRecording();
        }

        DOOM_MAIN.borrow_mut().main_loop_started = 1;

        TryRunTics();

        let gamedescription = STAT.borrow().gamedescription.clone();
        I_SetWindowTitle(&gamedescription);
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

        if GAME.borrow().testcontrols != 0 {
            DOOM_MAIN.borrow_mut().wipegamestate = gamestate();
        }

        doomgeneric_Tick();
    }
}

// ---------------------------------------------------------------------------
// Demo sequence management
// ---------------------------------------------------------------------------

/// Handle timing for demo projection.
pub fn D_PageTicker() {
    let pagetic = {
        let mut main = DOOM_MAIN.borrow_mut();
        main.pagetic -= 1;
        main.pagetic
    };
    if pagetic < 0 {
        D_AdvanceDemo();
    }
}

/// Draw the current demo page.
pub fn D_PageDrawer() {
    let pagename = DOOM_MAIN.borrow().pagename;
    unsafe { V_DrawPatch(0, 0, W_CacheLumpName(pagename, PU_CACHE) as *mut patch_t) };
}

/// Called after each demo or intro demosequence finishes.
pub fn D_AdvanceDemo() {
    DOOM_MAIN.borrow_mut().advancedemo = 1;
}

/// Show demo page `pagename` for `pagetic` tics.
fn set_page(pagetic: i32, pagename: &'static [u8]) {
    let mut main = DOOM_MAIN.borrow_mut();
    main.pagetic = pagetic;
    main.pagename = pagename;
}

/// Cycle through the demo sequences.
pub fn D_DoAdvanceDemo() {
    unsafe {
        // Set player state to live (PST_LIVE = 1)
        let cp = consoleplayer();
        if cp >= 0 && (cp as usize) < MAXPLAYERS {
            (*player(cp as usize)).playerstate = 1;
        }

        DOOM_MAIN.borrow_mut().advancedemo = 0;
        {
            let mut game = GAME.borrow_mut();
            game.usergame = 0;
            game.paused = 0;
            game.gameaction = ga_nothing;
        }

        // Demo sequence: 7 for ultimate/final, 6 for others
        let max_seq = {
            let gameversion = gameversion();
            if gameversion == doom::mode::exe_ultimate || gameversion == doom::mode::exe_final {
                7
            } else {
                6
            }
        };
        let demosequence = {
            let mut main = DOOM_MAIN.borrow_mut();
            main.demosequence = (main.demosequence + 1) % max_seq;
            main.demosequence
        };

        match demosequence {
            0 => {
                let pagetic = if gamemode() == doom::mode::commercial {
                    TICRATE * 11
                } else {
                    170
                };
                GAME.borrow_mut().gamestate = GS_DEMOSCREEN;
                set_page(pagetic, b"TITLEPIC");
                if gamemode() == doom::mode::commercial {
                    S_StartMusic(Mus::Dm2ttl as i32);
                } else {
                    S_StartMusic(Mus::Intro as i32);
                }
            }
            1 => {
                G_DeferedPlayDemo("demo1");
            }
            2 => {
                GAME.borrow_mut().gamestate = GS_DEMOSCREEN;
                set_page(200, b"CREDIT");
            }
            3 => {
                G_DeferedPlayDemo("demo2");
            }
            4 => {
                GAME.borrow_mut().gamestate = GS_DEMOSCREEN;
                if gamemode() == doom::mode::commercial {
                    set_page(TICRATE * 11, b"TITLEPIC");
                    S_StartMusic(Mus::Dm2ttl as i32);
                } else if gamemode() == doom::mode::retail {
                    set_page(200, b"CREDIT");
                } else {
                    set_page(200, b"HELP2");
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
        let (bfgedition, pagename) = {
            let main = DOOM_MAIN.borrow();
            (main.bfgedition, main.pagename)
        };
        if bfgedition != 0
            && pagename.eq_ignore_ascii_case(b"TITLEPIC")
            && W_CheckNumForName(b"titlepic") < 0
        {
            DOOM_MAIN.borrow_mut().pagename = b"INTERPIC";
        }
    }
}

/// Start the title screen demo sequence.
pub fn D_StartTitle() {
    GAME.borrow_mut().gameaction = ga_nothing;
    DOOM_MAIN.borrow_mut().demosequence = -1;
    D_AdvanceDemo();
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
            let version = G_VanillaVersionCode();
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
fn SetMissionForPackName(pack_name: &[u8]) {
    for pack in &PACKS {
        if pack_name.eq_ignore_ascii_case(pack.name.as_bytes()) {
            STAT.borrow_mut().gamemission = pack.mission;
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
        if gamemission() == doom::mode::none {
            for i in 0..W_NumLumps() {
                let name = &(*lumpinfo().add(i as usize)).name;

                if lump_names_equal(name, b"MAP01") {
                    STAT.borrow_mut().gamemission = doom::mode::doom2;
                    break;
                } else if lump_names_equal(name, b"E1M1") {
                    STAT.borrow_mut().gamemission = doom::mode::doom;
                    break;
                }
            }

            if gamemission() == doom::mode::none {
                i_error!("Unknown or invalid IWAD file.");
            }
        }

        // Make sure gamemode is set up correctly
        let logical_mission = logical_gamemission();
        if logical_mission == doom::mode::doom {
            // Doom 1. But which version?
            let gamemode = if W_CheckNumForName(b"E4M1") > 0 {
                doom::mode::retail
            } else if W_CheckNumForName(b"E3M1") > 0 {
                doom::mode::registered
            } else {
                doom::mode::shareware
            };
            STAT.borrow_mut().gamemode = gamemode;
        } else {
            // Doom 2 of some kind.
            STAT.borrow_mut().gamemode = doom::mode::commercial;

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
fn logical_gamemission() -> i32 {
    let gamemission = gamemission();
    if gamemission == doom::mode::pack_chex {
        doom::mode::doom
    } else if gamemission == doom::mode::pack_hacx {
        doom::mode::doom2
    } else {
        gamemission
    }
}

// ---------------------------------------------------------------------------
// D_SetGameDescription
// ---------------------------------------------------------------------------

/// Set the gamedescription string.
pub fn D_SetGameDescription() {
    let is_freedoom = W_CheckNumForName(b"FREEDOOM") >= 0;
    let is_freedm = W_CheckNumForName(b"FREEDM") >= 0;

    let logical_mission = logical_gamemission();
    let description = if logical_mission == doom::mode::doom {
        // Doom 1. But which version?
        if is_freedoom {
            GetGameName("Freedoom: Phase 1")
        } else if gamemode() == doom::mode::retail {
            GetGameName("The Ultimate DOOM")
        } else if gamemode() == doom::mode::registered {
            GetGameName("DOOM Registered")
        } else if gamemode() == doom::mode::shareware {
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
        } else if logical_mission == doom::mode::doom2 {
            GetGameName("DOOM 2: Hell on Earth")
        } else if logical_mission == doom::mode::pack_plut {
            GetGameName("DOOM 2: Plutonia Experiment")
        } else if logical_mission == doom::mode::pack_tnt {
            GetGameName("DOOM 2: TNT - Evilution")
        } else {
            "Unknown".to_owned()
        }
    };

    STAT.borrow_mut().gamedescription = description;
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
                STAT.borrow_mut().gameversion = gv.version;
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
        let mut stat = STAT.borrow_mut();
        if stat.gamemission == doom::mode::pack_chex {
            stat.gameversion = doom::mode::exe_chex;
        } else if stat.gamemission == doom::mode::pack_hacx {
            stat.gameversion = doom::mode::exe_hacx;
        } else if stat.gamemode == doom::mode::shareware || stat.gamemode == doom::mode::registered
        {
            stat.gameversion = doom::mode::exe_doom_1_9;
        } else if stat.gamemode == doom::mode::retail {
            stat.gameversion = doom::mode::exe_ultimate;
        } else if stat.gamemode == doom::mode::commercial {
            if stat.gamemission == doom::mode::doom2 {
                stat.gameversion = doom::mode::exe_doom_1_9;
            } else {
                // Final Doom: tnt or plutonia
                // Defaults to emulating the first Final Doom executable
                stat.gameversion = doom::mode::exe_final;
            }
        }
    }

    let mut stat = STAT.borrow_mut();

    // Original exe does not support retail - 4th episode not supported
    if stat.gameversion < doom::mode::exe_ultimate && stat.gamemode == doom::mode::retail {
        stat.gamemode = doom::mode::registered;
    }

    // EXEs prior to Final Doom do not support Final Doom
    if stat.gameversion < doom::mode::exe_final
        && stat.gamemode == doom::mode::commercial
        && (stat.gamemission == doom::mode::pack_tnt || stat.gamemission == doom::mode::pack_plut)
    {
        stat.gamemission = doom::mode::doom2;
    }
}

// ---------------------------------------------------------------------------
// PrintGameVersion
// ---------------------------------------------------------------------------

/// Print a startup message identifying which original executable version is being emulated.
pub fn PrintGameVersion() {
    let gameversion = gameversion();
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

// ---------------------------------------------------------------------------
// D_Endoom
// ---------------------------------------------------------------------------

/// `I_AtExit` callback: display the ENDOOM lump and exit when the game shuts down.
///
/// Skipped when `show_endoom` is 0, when the main loop never started, or in
/// screensaver / test-controls mode.
fn D_Endoom() {
    // Don't show ENDOOM if disabled, or in screensaver/control test mode.
    // Only show it once the game has actually started.
    let (show_endoom, main_loop_started) = {
        let main = DOOM_MAIN.borrow();
        (main.show_endoom, main.main_loop_started)
    };
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
        let devparm = (M_CheckParm("-devparm") != 0) as i32;
        {
            let mut main = DOOM_MAIN.borrow_mut();
            main.nomonsters = (M_CheckParm("-nomonsters") != 0) as i32;
            main.respawnparm = (M_CheckParm("-respawn") != 0) as i32;
            main.fastparm = (M_CheckParm("-fast") != 0) as i32;
            main.devparm = devparm;
        }

        I_DisplayFPSDots(Boolean::from(devparm != 0));

        if M_CheckParm("-deathmatch") != 0 {
            GAME.borrow_mut().deathmatch = 1;
        }

        if M_CheckParm("-altdeath") != 0 {
            GAME.borrow_mut().deathmatch = 2;
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

            let mut game = GAME.borrow_mut();
            game.forwardmove[0] = game.forwardmove[0] * scale / 100;
            game.forwardmove[1] = game.forwardmove[1] * scale / 100;
            game.sidemove[0] = game.sidemove[0] * scale / 100;
            game.sidemove[1] = game.sidemove[1] * scale / 100;
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
        let mut mission = gamemission();
        let found_iwad = D_FindIWAD(1, &mut mission); // IWAD_MASK_DOOM = 1
        STAT.borrow_mut().gamemission = mission;
        let Some(found_iwad) = found_iwad else {
            i_error!(
                "Game mode indeterminate.  No IWAD file was found.  Try\nspecifying one with the '-iwad' command line parameter.\n"
            );
        };
        DOOM_MAIN.borrow_mut().iwadfile = found_iwad.clone();

        STAT.borrow_mut().modifiedgame = Boolean::FALSE;

        println!("W_Init: Init WADfiles.");
        D_AddFile(&found_iwad);

        W_CheckCorrectIWAD(doom::mode::doom);

        // Identify version and game version
        D_IdentifyVersion();
        InitGameVersion();

        // BFG Edition check
        if W_CheckNumForName(b"dmenupic") >= 0 {
            println!("BFG Edition: Using workarounds as needed.");
            DOOM_MAIN.borrow_mut().bfgedition = 1;

            // BFG changes secret level names
            // (DEH replacements would go here, skipped since no dehacked)
        }

        // Load PWAD files
        let pwads_loaded = W_ParseCommandLine();
        STAT.borrow_mut().modifiedgame = pwads_loaded;

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
                let name = (*lumpinfo().add(W_NumLumps() as usize - 1)).name;
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
        let savegamedir = M_GetSaveGameDir(D_SaveGameIWADName(gamemission()));
        DOOM_MAIN.borrow_mut().savegamedir = savegamedir;

        // Check for -file in shareware
        if modifiedgame().is_truthy() {
            if gamemode() == doom::mode::shareware {
                i_error!("\nYou cannot -file with the shareware version. Register!");
            }

            // Check for fake IWAD
            if gamemode() == doom::mode::registered {
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
        let gamedescription = STAT.borrow().gamedescription.clone();
        I_PrintStartupBanner(&gamedescription);
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
        let mut startskill = sk_medium;
        let mut startepisode = 1;
        let mut startmap = 1;
        let mut autostart = 0;

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
            if gamemode() == doom::mode::commercial {
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
            GAME.borrow_mut().testcontrols = 1;
        }

        // -loadgame
        let p = M_CheckParmWithArgs("-loadgame", 1);
        let startloadgame = if p > 0 { atoi(argument(p + 1)) } else { -1 };

        {
            let mut main = DOOM_MAIN.borrow_mut();
            main.startskill = startskill;
            main.startepisode = startepisode;
            main.startmap = startmap;
            main.autostart = autostart;
            main.startloadgame = startloadgame;
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
        let SoundState {
            sfxVolume,
            musicVolume,
            ..
        } = SOUND.get();
        S_Init(sfxVolume * 8, musicVolume * 8);

        println!("D_CheckNetGame: Checking network game status.");
        D_CheckNetGame();

        PrintGameVersion();

        println!("HU_Init: Setting up heads up display.");
        HU_Init();

        println!("ST_Init: Init status bar.");
        ST_Init();

        // Store demo check
        if gamemode() == doom::mode::commercial && W_CheckNumForName(b"map01") < 0 {
            DOOM_MAIN.borrow_mut().storedemo = 1;
        }

        // -statdump
        if M_CheckParmWithArgs("-statdump", 1) > 0 {
            println!("External statistics registered.");
        }

        // -record
        let p = M_CheckParmWithArgs("-record", 1);
        if p > 0 {
            G_RecordDemo(argument(p + 1));
            DOOM_MAIN.borrow_mut().autostart = 1;
        }

        // -playdemo
        let p = M_CheckParmWithArgs("-playdemo", 1);
        if p > 0 {
            GAME.borrow_mut().singledemo = 1;
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
        let startloadgame = DOOM_MAIN.borrow().startloadgame;
        if startloadgame >= 0 {
            G_LoadGame(&P_SaveGameFile(startloadgame));
        }

        // Start new game or title screen
        if gameaction() != ga_loadgame {
            let (autostart, startskill, startepisode, startmap) = {
                let main = DOOM_MAIN.borrow();
                (
                    main.autostart,
                    main.startskill,
                    main.startepisode,
                    main.startmap,
                )
            };
            if autostart != 0 || netgame() != 0 {
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

    /// Verify all startup flags start zeroed (except `show_endoom`).
    #[test]
    fn startup_flags_defaults() {
        let main = DoomMainState::new();
        assert_eq!(main.devparm, 0);
        assert_eq!(main.nomonsters, 0);
        assert_eq!(main.respawnparm, 0);
        assert_eq!(main.fastparm, 0);
        assert_eq!(main.autostart, 0);
        assert_eq!(main.advancedemo, 0);
        assert_eq!(main.storedemo, 0);
        assert_eq!(main.bfgedition, 0);
        assert_eq!(main.main_loop_started, 0);
        assert_eq!(main.show_endoom, 1);
        assert_eq!(main.startepisode, 0);
        assert_eq!(main.startmap, 0);
    }

    /// Verify `wipegamestate` starts at `GS_DEMOSCREEN` so the first frame always wipes,
    /// and the first `D_Display` forces border and palette redraws.
    #[test]
    fn wipegamestate_default() {
        let main = DoomMainState::new();
        assert_eq!(main.wipegamestate, GS_DEMOSCREEN);
        assert_eq!(main.display_old_gamestate, -1);
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
