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
//! * `DEH_String` is the identity hook from [`crate::doom::deh_str`]
//!   (DeHackEd support is not implemented).
//! * The `DEH_printf` diagnostic prints present in `D_CheckNetGame` in C are
//!   omitted in this port.
//! * `deh_sha1sum` in `InitConnectData` is left zeroed (the C original called
//!   `DEH_Checksum`), guarded by `#if ORIGCODE` in the vendor source.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::doom::d_player::{consoleplayer, players, TiccmdT, MAXPLAYERS};

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
///
/// Layout must match the C struct exactly; enforced by the size test in the
/// `tests` module.
#[repr(C)]
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
///
/// Layout must match the C struct exactly; enforced by the size test in the
/// `tests` module.
#[repr(C)]
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
///
/// Layout must match the C struct exactly; enforced by the size test in the
/// `tests` module.
#[repr(C)]
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

/// Pointer to the array of per-player tic commands for the current tic.
///
/// Set by `RunTic` at the start of each tic, pointing into the buffer
/// provided by the loop layer. Downstream code (primarily `g_game.c`) reads
/// `netcmds[consoleplayer]` to obtain the local player's input.
/// Corresponds to `ticcmd_t *netcmds` in `d_net.c`.
pub static mut netcmds: *mut TiccmdT = std::ptr::null_mut();

use crate::doom::{
    d_loop::{D_InitNetGame, D_RegisterLoopCallbacks, D_StartNetGame},
    deh_str::DEH_String,
    d_main::{
        advancedemo, autostart, fastparm, nomonsters, respawnparm, startepisode, startloadgame,
        startmap, startskill, D_DoAdvanceDemo, D_ProcessEvents,
    },
    doomstat::{gamemission, gamemode, gameversion},
    g_game::{
        deathmatch, demoplayback, demorecording, lowres_turn, netgame, playeringame, timelimit,
        G_BuildTiccmd, G_CheckDemoStatus, G_Ticker,
    },
    m_argv::M_CheckParm,
    m_menu::M_Ticker,
    m_misc::M_StringCopy,
    r_main::viewangleoffset,
    w_checksum::W_Checksum,
    w_wad::W_CheckNumForName,
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
    /// Capacity of [`EXITMSG`], named so `M_StringCopy` does not have to take
    /// a shared reference to the `static mut` to read its length.
    const EXITMSG_LEN: usize = 80;

    /// 80-byte scratch buffer holding the formatted "Player N left the game"
    /// message; its address is passed to the console-player's message pointer.
    static mut EXITMSG: [u8; EXITMSG_LEN] = [0; EXITMSG_LEN];

    M_StringCopy(
        std::ptr::addr_of_mut!(EXITMSG[0]),
        DEH_String(EXIT_MSG.as_ptr()),
        EXITMSG_LEN,
    );

    EXITMSG[7] += player_idx as u8;

    playeringame[player_idx] = 0;
    (*std::ptr::addr_of_mut!(players[0]).offset(consoleplayer as isize)).message =
        std::ptr::addr_of_mut!(EXITMSG[0]);

    if demorecording != 0 {
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
        if demoplayback == 0 && playeringame[i] != 0 && *ingame.add(i) == 0 {
            PlayerQuitGame(i);
        }
    }

    netcmds = cmds;

    if advancedemo != 0 {
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
    deathmatch = (*settings).deathmatch;
    startepisode = (*settings).episode;
    startmap = (*settings).map;
    startskill = (*settings).skill;
    startloadgame = (*settings).loadgame;
    lowres_turn = (*settings).lowres_turn;
    nomonsters = (*settings).nomonsters;
    fastparm = (*settings).fast_monsters;
    respawnparm = (*settings).respawn_monsters;
    timelimit = (*settings).timelimit;
    consoleplayer = (*settings).consoleplayer;

    for i in 0..MAXPLAYERS {
        playeringame[i] = if i < (*settings).num_players as usize {
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
    (*settings).deathmatch = deathmatch;
    (*settings).episode = startepisode;
    (*settings).map = startmap;
    (*settings).skill = startskill;
    (*settings).loadgame = startloadgame;
    (*settings).gameversion = gameversion;
    (*settings).nomonsters = nomonsters;
    (*settings).fast_monsters = fastparm;
    (*settings).respawn_monsters = respawnparm;
    (*settings).timelimit = timelimit;
    (*settings).lowres_turn = if M_CheckParm(c"-record".as_ptr().cast()) > 0
        && M_CheckParm(c"-longtics".as_ptr().cast()) == 0
    {
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

    if M_CheckParm(c"-left".as_ptr().cast()) > 0 {
        viewangleoffset = 0x40000000u32 as i32;
        (*connect_data).drone = 1;
    }

    if M_CheckParm(c"-right".as_ptr().cast()) > 0 {
        viewangleoffset = 0xC0000000u32 as i32;
        (*connect_data).drone = 1;
    }

    (*connect_data).gamemode = gamemode;
    (*connect_data).gamemission = gamemission;

    (*connect_data).lowres_turn = if M_CheckParm(c"-record".as_ptr().cast()) > 0
        && M_CheckParm(c"-longtics".as_ptr().cast()) == 0
    {
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
/// player number. The string is NUL-terminated and matches the C literal
/// `"Player 1 left the game"` passed to `DEH_String` in `d_net.c`.
static EXIT_MSG: [u8; 23] = *b"Player 1 left the game\0";

/// The loop callback table registered with `d_loop.c`.
///
/// The static table wires the four per-tic callbacks that the loop layer
/// (`D_RegisterLoopCallbacks`) needs. It must remain `mut` because the C ABI
/// requires a non-const pointer in `D_RegisterLoopCallbacks`. Corresponds to
/// `doom_loop_interface` in `d_net.c`.
static mut DOOM_LOOP_INTERFACE: LoopInterfaceT = LoopInterfaceT {
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
        netgame = D_InitNetGame(&mut connect_data);

        if M_CheckParm(c"-solo-net".as_ptr().cast()) > 0 {
            netgame = 1;
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
        if netgame != 0 {
            autostart = 1;
        }

        D_RegisterLoopCallbacks(&raw mut DOOM_LOOP_INTERFACE);

        let mut settings: NetGameSettingsT = std::mem::zeroed();
        SaveGameSettings(&mut settings);
        D_StartNetGame(&mut settings, std::ptr::null());
        LoadGameSettings(&mut settings);
    }
}

#[cfg(test)]
mod tests {
    use {super::*, std::sync::Mutex};

    static LOCK: Mutex<()> = Mutex::new(());

    const NET_CONNECT_DATA_T_SIZEOF: usize = 68;
    const NET_GAMESETTINGS_T_SIZEOF: usize = 100;
    const LOOP_INTERFACE_T_SIZEOF: usize = 32;

    #[test]
    fn net_connect_data_t_size_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(
            std::mem::size_of::<NetConnectDataT>(),
            NET_CONNECT_DATA_T_SIZEOF,
            "NetConnectDataT size mismatch: Rust={}, expected={}",
            std::mem::size_of::<NetConnectDataT>(),
            NET_CONNECT_DATA_T_SIZEOF,
        );
    }

    #[test]
    fn net_gamesettings_t_size_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(
            std::mem::size_of::<NetGameSettingsT>(),
            NET_GAMESETTINGS_T_SIZEOF,
            "NetGameSettingsT size mismatch: Rust={}, expected={}",
            std::mem::size_of::<NetGameSettingsT>(),
            NET_GAMESETTINGS_T_SIZEOF,
        );
    }

    #[test]
    fn loop_interface_t_size_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(
            std::mem::size_of::<LoopInterfaceT>(),
            LOOP_INTERFACE_T_SIZEOF,
            "LoopInterfaceT size mismatch: Rust={}, expected={}",
            std::mem::size_of::<LoopInterfaceT>(),
            LOOP_INTERFACE_T_SIZEOF,
        );
    }
}
