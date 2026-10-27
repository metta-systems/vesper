//! Rust port of vendor/doomgeneric/d_net.c.
//!
//! Network game communication and protocol layer, OS-independent parts.
//!
//! In the original Doom, `d_net.c` implements tic-synchronisation over a
//! network connection. In this single-player port, `FEATURE_MULTIPLAYER` is
//! never defined, so the networking stack is replaced by stub implementations
//! provided by the loop interface defined in `d_loop.h`. The primary responsibilities that remain are:
//!
//! * Connecting and starting a "net game" via the loop back-end (`D_ConnectNetGame`,
//!   `D_CheckNetGame`).
//! * Registering the per-tic callback table (`DOOM_LOOP_INTERFACE`) so that the
//!   loop layer can call `D_ProcessEvents`, `G_BuildTiccmd`, `RunTic`, and
//!   `M_Ticker` at the right moments.
//! * Propagating game settings between the global variables (`deathmatch`,
//!   `startepisode`, etc.) and the `NetGameSettingsT` structure that the loop
//!   layer reads/writes.
//! * Handling player-quit events during multi-player (skipped when `demoplayback`
//!   is active, mirroring the original guard).
//!
//! Notable Rust-vs-C differences:
//! * `PlayerQuitGame` takes a player index (`usize`) instead of a `player_t *`
//!   pointer; pointer arithmetic is replaced by array indexing.
//! * `DEH_String` is the identity hook from [`crate::engine::dehacked`]
//!   (DeHackEd support is not implemented).
//! * The `DEH_printf` diagnostic prints present in `D_CheckNetGame` in C are
//!   omitted in this port.
//! * `deh_sha1sum` in `InitConnectData` is left zeroed (the C original called
//!   `DEH_Checksum`), guarded by `#if ORIGCODE` in the vendor source.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::engine::{
    doom::player::{MAXPLAYERS, PlayerId, TiccmdT},
    world::WORLD,
};

/// Maximum number of players supported by the networking code.
///
/// This is the upper bound used by the net layer, which may be larger than the
/// per-game `MAXPLAYERS` constant (e.g., 4 for Doom). Corresponds to the C
/// macro `NET_MAXPLAYERS` in `net_defs.h`.
const NET_MAXPLAYERS: usize = 8;

/// Byte length of a SHA-1 message digest.
///
/// Used to size the `wad_sha1sum` and `deh_sha1sum` fields in
/// [`NetConnectDataT`]. Corresponds to `SHA1_DIGEST_SIZE` in `sha1.h`.
const SHA1_DIGEST_SIZE: usize = 20;

/// Data sent by a client to the server when establishing a net connection.
///
/// Maps to `net_connect_data_t` in `net_defs.h`. The fields describe the
/// client's game configuration so the server can verify compatibility before
/// starting a game. In this single-player port the struct is populated by
/// `InitConnectData` and passed to the stub `D_InitNetGame`.
#[derive(Clone, Copy)]
pub struct NetConnectDataT {
    /// Integer encoding of `GameMode_t` (shareware, registered, commercial, …).
    pub gamemode: i32,
    /// Integer encoding of `GameMission_t` (doom, doom2, heretic, …).
    pub gamemission: i32,
    /// Non-zero when recording a demo without `-longtics`; limits turn
    /// resolution to 8-bit (Vanilla compatibility).
    pub lowres_turn: i32,
    /// Non-zero when this client is a drone (left/right screen in 3-screen
    /// setup). Set by `-left` or `-right` command-line parameters.
    pub drone: i32,
    /// Maximum number of players for this game; normally `MAXPLAYERS`.
    pub max_players: i32,
    /// Non-zero when the loaded IWAD is Freedoom (detected by the `FREEDOOM`
    /// lump name).
    pub is_freedoom: i32,
    /// SHA-1 digest of the WAD directory, used for net-consistency checks.
    pub wad_sha1sum: [u8; SHA1_DIGEST_SIZE],
    /// SHA-1 digest of any loaded DeHackEd patches. Always zeroed in this
    /// port; the C original called `DEH_Checksum` (guarded by `#if ORIGCODE`).
    pub deh_sha1sum: [u8; SHA1_DIGEST_SIZE],
    /// Player class (used by Hexen). Always 0 for Doom/Heretic.
    pub player_class: i32,
}

/// Session-wide game settings exchanged between the server and all clients.
///
/// Maps to `net_gamesettings_t` in `net_defs.h`. `SaveGameSettings` copies
/// global state into this struct before calling `D_StartNetGame`; afterwards
/// `LoadGameSettings` applies the (possibly server-modified) values back to
/// the globals.
#[derive(Clone, Copy)]
pub struct NetGameSettingsT {
    /// Number of game tics to duplicate per network packet (for lag
    /// compensation). Always 1 in single-player.
    pub ticdup: i32,
    /// Number of extra tics to send ahead of time for smoother play.
    pub extratics: i32,
    /// Deathmatch mode: 0 = cooperative, 1 = deathmatch, 2 = altdeath.
    pub deathmatch: i32,
    /// Starting episode number (Doom/Heretic episode-based games).
    pub episode: i32,
    /// Non-zero when monsters are disabled (`-nomonsters`).
    pub nomonsters: i32,
    /// Non-zero when fast monsters are enabled (`-fast`).
    pub fast_monsters: i32,
    /// Non-zero when monsters respawn (`-respawn`).
    pub respawn_monsters: i32,
    /// Starting map number within the episode.
    pub map: i32,
    /// Starting skill level (0 = baby … 4 = nightmare).
    pub skill: i32,
    /// `GameVersion_t` integer identifying which executable to emulate.
    pub gameversion: i32,
    /// Non-zero when turn resolution is limited to 8-bit (Vanilla demo compat).
    pub lowres_turn: i32,
    /// Non-zero when the new network sync protocol is in use.
    pub new_sync: i32,
    /// Per-level time limit in minutes for deathmatch (0 = no limit).
    pub timelimit: i32,
    /// Save-game slot to load at startup (-1 = no load).
    pub loadgame: i32,
    /// Random seed (Strife only).
    pub random: i32,
    /// Number of human players in this session.
    pub num_players: i32,
    /// Index of the local player (0-based) within the player array.
    pub consoleplayer: i32,
    /// Per-player class array, indexed by player number (Hexen only).
    pub player_classes: [i32; NET_MAXPLAYERS],
}

/// Callback table registered with the game loop layer.
///
/// Maps to `loop_interface_t` in `d_loop.h`. The loop layer (`d_loop.c`) calls
/// these function pointers at the appropriate points during each tic. All
/// fields are `Option<unsafe fn(…)>` because the C struct uses raw
/// function pointers that may theoretically be null, though in practice all
/// four are always set before use.
#[derive(Clone, Copy)]
pub struct LoopInterfaceT {
    /// Called once per tic to drain the event queue. Bound to `D_ProcessEvents`.
    pub ProcessEvents: Option<unsafe fn()>,
    /// Called to build a new [`TiccmdT`] from current input state. Bound to
    /// `G_BuildTiccmd`. The second argument is `maketic`, the tic number being
    /// constructed.
    pub BuildTiccmd: Option<unsafe fn(*mut TiccmdT, i32)>,
    /// Called to advance the game by one tic with the given player commands.
    /// The `ingame` array has one boolean per player indicating whether that
    /// player is still connected. Bound to the Rust [`RunTic`] function.
    pub RunTic: Option<unsafe fn(*mut TiccmdT, *mut i32)>,
    /// Called once per tic to run the menu system. Bound to `M_Ticker`.
    pub RunMenu: Option<unsafe fn()>,
}

/// Net game state (C `d_net.c` globals).
pub struct NetState {
    /// Pointer to the array of per-player tic commands for the current tic.
    ///
    /// Set by `RunTic` at the start of each tic, pointing into the buffer
    /// provided by the loop layer. Downstream code (primarily `g_game.c`)
    /// reads `netcmds[consoleplayer]` through [`netcmds`] to obtain the local
    /// player's input. Corresponds to `ticcmd_t *netcmds` in `d_net.c`.
    netcmds: *mut TiccmdT,
}

impl NetState {
    /// No tic commands yet.
    pub const fn new() -> Self {
        Self {
            netcmds: std::ptr::null_mut(),
        }
    }
}

impl Default for NetState {
    fn default() -> Self {
        Self::new()
    }
}

/// The net game state.
pub static NET: Global<NetState> = Global::new(NetState::new());

/// The per-player tic commands of the current tic (C `netcmds`).
pub fn netcmds() -> *mut TiccmdT {
    NET.borrow().netcmds
}

/// Whether the client is connected to a network game session.
///
/// Always false: the network client is not ported, so nothing sets it.
/// Corresponds to `net_client_connected` in `dummy.c`.
pub const net_client_connected: u32 = 0;

/// Whether this instance is a "drone" (observer) client, which joins a
/// network game without controlling a player.
///
/// Always false: the network client is not ported, so nothing sets it.
/// Corresponds to `drone` in `dummy.c`.
pub const drone: u32 = 0;

use crate::engine::{
    dehacked::DEH_String,
    doom::{
        main::{D_DoAdvanceDemo, D_ProcessEvents, DOOM_MAIN},
        main_loop::{D_InitNetGame, D_RegisterLoopCallbacks, D_StartNetGame},
        stat::{gamemission, gamemode, gameversion},
    },
    game::{
        G_BuildTiccmd, G_CheckDemoStatus, G_Ticker, GAME, deathmatch, demoplayback, netgame,
        playeringame,
    },
    global::Global,
    misc::{argv::M_CheckParm, menu::M_Ticker},
    render_3d::RENDERER,
    wad::{W_CheckNumForName, checksum::W_Checksum},
};

/// Notify the game that a player has disconnected.
///
/// Formats a "Player N left the game" message into a static 80-byte buffer,
/// marks `playeringame[player_idx]` as false, sets the console player's
/// message pointer to that buffer, and (if demo recording is active) stops the
/// demo via `G_CheckDemoStatus`.
///
/// The message template [`EXIT_MSG`] is DeHackEd-patchable in the original C
/// via `DEH_String`; the Rust `DEH_String` stub is a no-op, so the message is
/// always the default string. The player number is encoded by incrementing
/// `exitmsg[7]` (the `'1'` digit in `"Player 1 left the game"`).
///
/// Corresponds to the static `PlayerQuitGame(player_t *player)` in `d_net.c`,
/// where `player_num = player - players` replaces the explicit index argument.
///
/// # Safety
///
/// Caller must ensure `player_idx < MAXPLAYERS` and that the global state
/// (`players`, `playeringame`, `consoleplayer`, `demorecording`) is valid.
unsafe fn PlayerQuitGame(player_idx: usize) {
    // "Player 1 left the game" with the digit bumped to this player.
    let mut exit_message = DEH_String(EXIT_MSG).as_bytes().to_vec();
    exit_message[7] += player_idx as u8;

    GAME.borrow_mut().playeringame[player_idx] = 0;
    // Runs from `NetUpdate`, outside any world borrow. Play reads the
    // session's copy of `playeringame`, which changes here mid-level.
    let mut world = WORLD.borrow_mut();
    world.session.playeringame[player_idx] = 0;
    world.players[PlayerId::console()].set_message(exit_message);
    drop(world);

    if GAME.borrow().demorecording != 0 {
        G_CheckDemoStatus();
    }
}

/// Advance the game by one tic using the provided player commands.
///
/// Called by the loop layer once per tic. Checks whether any player has
/// dropped (present in `playeringame` but absent from `ingame`), notifying via
/// `PlayerQuitGame` when that happens - but only when not in demo playback,
/// mirroring the original guard. Then stores `cmds` in `netcmds`, optionally
/// advances the demo sequence via `D_DoAdvanceDemo`, and runs one game tic via
/// `G_Ticker`.
///
/// Registered as the `RunTic` slot in `DOOM_LOOP_INTERFACE`. Corresponds to
/// the static `RunTic(ticcmd_t *cmds, boolean *ingame)` in `d_net.c`.
///
/// # Safety
///
/// `cmds` must point to a valid array of at least `MAXPLAYERS` [`TiccmdT`]
/// values. `ingame` must point to a valid array of at least `MAXPLAYERS`
/// `i32` values. All global game state must be consistently initialised.
unsafe fn RunTic(cmds: *mut TiccmdT, ingame: *mut i32) {
    for i in 0..MAXPLAYERS {
        if demoplayback() == 0 && playeringame(i) != 0 && *ingame.add(i) == 0 {
            PlayerQuitGame(i);
        }
    }

    NET.borrow_mut().netcmds = cmds;

    if DOOM_MAIN.borrow().advancedemo != 0 {
        D_DoAdvanceDemo();
    }

    G_Ticker();
}

/// Apply settings from a `NetGameSettingsT` to the global game variables.
///
/// Called after `D_StartNetGame` returns so that the (possibly server-adjusted)
/// settings are visible to the rest of the engine. Only the fields that have
/// corresponding globals are applied; `ticdup`, `extratics`, `new_sync`, and
/// `random` are intentionally ignored in this single-player build.
///
/// In the original C a `printf` diagnostic is emitted when `lowres_turn` is
/// non-zero; this port omits that print.
///
/// Corresponds to the static `LoadGameSettings(net_gamesettings_t *settings)`
/// in `d_net.c`.
///
/// # Safety
///
/// `settings` must point to a fully initialised `NetGameSettingsT`. All
/// destination globals must be safely mutable at the call site.
unsafe fn LoadGameSettings(settings: *mut NetGameSettingsT) {
    let settings = &*settings;
    GAME.borrow_mut().deathmatch = settings.deathmatch;
    {
        let mut main = DOOM_MAIN.borrow_mut();
        main.startepisode = settings.episode;
        main.startmap = settings.map;
        main.startskill = settings.skill;
        main.startloadgame = settings.loadgame;
        main.nomonsters = settings.nomonsters;
        main.fastparm = settings.fast_monsters;
        main.respawnparm = settings.respawn_monsters;
    }
    let mut game = GAME.borrow_mut();
    game.lowres_turn = settings.lowres_turn;
    game.timelimit = settings.timelimit;
    game.consoleplayer = settings.consoleplayer;

    for i in 0..MAXPLAYERS {
        game.playeringame[i] = if i < settings.num_players as usize {
            1
        } else {
            0
        };
    }
}

/// Snapshot the global game variables into a `NetGameSettingsT`.
///
/// Called before `D_StartNetGame` so that the server (or stub) receives the
/// locally-configured session parameters. The `lowres_turn` field is derived
/// from the command-line: it is set when `-record` is active but `-longtics`
/// is not, matching Vanilla demo resolution.
///
/// Corresponds to the static `SaveGameSettings(net_gamesettings_t *settings)`
/// in `d_net.c`.
///
/// # Safety
///
/// `settings` must point to a writable `NetGameSettingsT`. All source
/// globals must be in a valid state.
unsafe fn SaveGameSettings(settings: *mut NetGameSettingsT) {
    (*settings).deathmatch = deathmatch();
    {
        let main = DOOM_MAIN.borrow();
        (*settings).episode = main.startepisode;
        (*settings).map = main.startmap;
        (*settings).skill = main.startskill;
        (*settings).loadgame = main.startloadgame;
        (*settings).nomonsters = main.nomonsters;
        (*settings).fast_monsters = main.fastparm;
        (*settings).respawn_monsters = main.respawnparm;
    }
    (*settings).gameversion = gameversion();
    (*settings).timelimit = GAME.borrow().timelimit;
    (*settings).lowres_turn = if M_CheckParm("-record") > 0 && M_CheckParm("-longtics") == 0 {
        1
    } else {
        0
    };
}

/// Populate a `NetConnectDataT` from the current engine state and command line.
///
/// Fills in game mode/mission, drone flags, low-resolution turn mode, the WAD
/// SHA-1 checksum, and the Freedoom detection flag. The `deh_sha1sum` field is
/// left zeroed; in the original C it was filled by `DEH_Checksum`, which is
/// guarded by `#if ORIGCODE` in the vendor source.
///
/// The `-left` flag sets `viewangleoffset` to 90 degrees (ANG90 =
/// `0x40000000`) and marks the client as a drone; `-right` sets it to 270
/// degrees (ANG270 = `0xC0000000`).
///
/// Corresponds to the static `InitConnectData(net_connect_data_t *connect_data)`
/// in `d_net.c`.
///
/// # Safety
///
/// `connect_data` must point to a writable `NetConnectDataT`. Global state
/// (`gamemode`, `gamemission`, `viewangleoffset`) must be valid.
unsafe fn InitConnectData(connect_data: *mut NetConnectDataT) {
    (*connect_data).max_players = MAXPLAYERS as i32;
    (*connect_data).drone = 0;

    if M_CheckParm("-left") > 0 {
        RENDERER.borrow_mut().view.viewangleoffset = 0x40000000u32 as i32;
        (*connect_data).drone = 1;
    }

    if M_CheckParm("-right") > 0 {
        RENDERER.borrow_mut().view.viewangleoffset = 0xC0000000u32 as i32;
        (*connect_data).drone = 1;
    }

    (*connect_data).gamemode = gamemode();
    (*connect_data).gamemission = gamemission();

    (*connect_data).lowres_turn = if M_CheckParm("-record") > 0 && M_CheckParm("-longtics") == 0 {
        1
    } else {
        0
    };

    W_Checksum(&raw mut (*connect_data).wad_sha1sum);

    (*connect_data).is_freedoom = if W_CheckNumForName(b"FREEDOOM") >= 0 {
        1
    } else {
        0
    };
}

/// Default "Player N left the game" message template.
///
/// The `'1'` at index 7 is incremented in `PlayerQuitGame` to encode the
/// player number. Matches the C literal `"Player 1 left the game"`
/// passed to `DEH_String` in `d_net.c`.
const EXIT_MSG: &str = "Player 1 left the game";

/// The loop callback table registered with `d_loop.c`.
///
/// The static table wires the four per-tic callbacks that the loop layer
/// (`D_RegisterLoopCallbacks`) needs. Nothing writes it, so it is a plain
/// `static` whose address the loop layer keeps. Corresponds to
/// `doom_loop_interface` in `d_net.c`.
static DOOM_LOOP_INTERFACE: LoopInterfaceT = LoopInterfaceT {
    ProcessEvents: Some(D_ProcessEvents),
    BuildTiccmd: Some(G_BuildTiccmd),
    RunTic: Some(RunTic),
    RunMenu: Some(M_Ticker),
};

/// Initialise the network connection and determine whether a net game is active.
///
/// Builds connect data from the current engine state, passes it to the loop
/// layer's `D_InitNetGame`, and stores the result in `netgame`. If the
/// `-solo-net` command-line parameter is present, `netgame` is forced to true
/// so that the engine behaves as if in a single-player net game (useful for
/// playing back net-game demos).
///
/// Called from `D_DoomMain` during startup. Corresponds to `D_ConnectNetGame`
/// in `d_net.c`.
pub fn D_ConnectNetGame() {
    unsafe {
        let mut connect_data: NetConnectDataT = std::mem::zeroed();
        InitConnectData(&mut connect_data);
        let netgame = D_InitNetGame(&connect_data);
        GAME.borrow_mut().netgame = netgame;

        if M_CheckParm("-solo-net") > 0 {
            GAME.borrow_mut().netgame = 1;
        }
    }
}

/// Finalise net-game setup and synchronise settings with the loop layer.
///
/// If a net game is active, sets `autostart` so the game begins immediately
/// without waiting for a keypress. Registers `DOOM_LOOP_INTERFACE` with the
/// loop layer, then exchanges game settings via `SaveGameSettings` /
/// `D_StartNetGame` / `LoadGameSettings` so that both sides agree on map,
/// skill, player count, and so on.
///
/// The diagnostic `DEH_printf` calls present in the C original are omitted in
/// this port.
///
/// Called from `D_DoomMain` after `D_ConnectNetGame`. Corresponds to
/// `D_CheckNetGame` in `d_net.c`.
pub fn D_CheckNetGame() {
    unsafe {
        if netgame() != 0 {
            DOOM_MAIN.borrow_mut().autostart = 1;
        }

        D_RegisterLoopCallbacks(&DOOM_LOOP_INTERFACE);

        let mut settings: NetGameSettingsT = std::mem::zeroed();
        SaveGameSettings(&mut settings);
        D_StartNetGame(&mut settings, std::ptr::null());
        LoadGameSettings(&mut settings);
    }
}
