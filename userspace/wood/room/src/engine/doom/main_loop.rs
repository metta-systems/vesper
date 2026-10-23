//! Rust port of vendor/doomgeneric/d_loop.c.
//!
//! Abstract main game-loop layer. This module drives the tic-based game loop:
//! it collects player input, builds `TiccmdT` command records, runs game tics
//! via registered callbacks, and maintains timing state. The loop advances in
//! fixed increments of 1/35 second (one tic = one invocation of the `RunTic` callback).
//!
//! In the original Chocolate Doom, this file also contains the networking
//! layer, but the doomgeneric fork compiles with `FEATURE_MULTIPLAYER`
//! undefined so all network-specific paths are stripped out. The Rust port
//! faithfully reflects this simplified build: `D_InitNetGame`,
//! `D_StartNetGame`, and `D_QuitNetGame` are stubs, and `D_ReceiveTic` only
//! handles only the `null` net client branch (the multiplayer branch is compiled out).
//!
//! The C file-scope variables live in [`MainLoopState`], held in the
//! [`MAIN_LOOP`] global. The loop callbacks re-enter the engine (and read
//! [`gametic`] / [`ticdup`]), so no guard is held while one runs.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::{i_error, types::Boolean};

use crate::engine::{
    doom::{
        net::{LoopInterfaceT, NetConnectDataT, NetGameSettingsT},
        player::TiccmdT,
    },
    global::Global,
    interface::timer::TICRATE,
    math::fixed::FRACUNIT,
};

/// Maximum number of players in a multiplayer session.
///
/// Mirrors `NET_MAXPLAYERS` from `net_defs.h`. Even in single-player mode,
/// arrays are sized by this constant to keep the data layout compatible with
/// the C original.
const NET_MAXPLAYERS: usize = 8;

/// Number of tic slots kept in the circular `ticdata` ring buffer.
///
/// Exported as `pub` because other modules (e.g. `d_loop_c.rs`) read it.
/// Corresponds to `BACKUPTICS` in `d_loop.h`.
pub const BACKUPTICS: usize = 128;

/// A complete set of player commands and participation flags for one tic.
///
/// One `TiccmdSetT` occupies a slot in `ticdata`. The `cmds` array holds
/// one input record per potential player; `ingame` tracks which player slots
/// are actually active for this tic.
///
/// Corresponds to `ticcmd_set_t` in `d_loop.c`.
struct TiccmdSetT {
    /// Per-player input commands for this tic.
    cmds: [TiccmdT; NET_MAXPLAYERS],
    /// Non-zero for each player slot that is active this tic (`boolean` in C).
    ingame: [i32; NET_MAXPLAYERS],
}

/// Index of the local player in the `NET_MAXPLAYERS` arrays.
///
/// Always `0` in single-player mode. Corresponds to `localplayer` in
/// `d_loop.c`.
const LOCALPLAYER: i32 = 0;

/// Millisecond offset applied to the timer when `new_sync` is active.
///
/// Allows the network client to nudge the local clock to stay in sync with the
/// server. Stored as a `fixed_t` (i.e. scaled by [`FRACUNIT`]).
/// Corresponds to `offsetms` in `d_loop.c`.
pub const offsetms: i32 = 0; // fixed_t

/// Game-loop state (C `d_loop.c` globals).
pub struct MainLoopState {
    /// Circular ring buffer of tic command sets, indexed by
    /// `tic % BACKUPTICS`.
    ///
    /// `maketic` selects where new local commands are written; `recvtic`
    /// tracks how many complete tic sets have been received from the network
    /// (single-player: always equals `maketic`). `TryRunTics` hands the
    /// `RunTic` callback pointers into it, so it stays in place inside
    /// [`MAIN_LOOP`]. Corresponds to `ticdata[]` in `d_loop.c`.
    ticdata: [TiccmdSetT; BACKUPTICS],
    /// Index of the next tic for which a local `TiccmdT` has not yet been
    /// built.
    ///
    /// Incremented by `build_new_tic`. Corresponds to `maketic` in
    /// `d_loop.c`.
    maketic: i32,
    /// Number of complete tic command sets received from the server.
    ///
    /// In single-player mode this advances in lock-step with `maketic` via
    /// [`D_ReceiveTic`]. Corresponds to `recvtic` in `d_loop.c`.
    recvtic: i32,
    /// Index of the tic currently being run (or about to be run); read
    /// through [`gametic`].
    ///
    /// Corresponds to `gametic` in `d_loop.c`.
    pub gametic: i32,
    /// When non-zero, exactly one tic is built and run per [`TryRunTics`]
    /// call. Set by `-timedemo` mode.
    /// Corresponds to `singletics` in `d_loop.c` (type `boolean`).
    pub singletics: i32,
    /// Number of tics to skip building during the next [`NetUpdate`] call.
    ///
    /// Used by the old net-sync algorithm to slow input generation when a
    /// follower player is too far ahead of the key player. Corresponds to
    /// `skiptics` in `d_loop.c`.
    skiptics: i32,
    /// Tic duplication factor: every `ticdup`-th input sample is sent over
    /// the network, reducing bandwidth at the cost of input resolution; read
    /// through [`ticdup`].
    ///
    /// Always `1` in single-player mode.
    /// Corresponds to `ticdup` in `d_loop.c`.
    pub ticdup: i32,
    /// Whether to use the new client synchronisation algorithm.
    ///
    /// When non-zero, [`TryRunTics`] runs exactly `availabletics` tics per
    /// frame and `build_new_tic` applies tighter buffering limits.
    /// Always `0` in this single-player build (hardcoded in
    /// [`D_StartNetGame`]).
    /// Corresponds to `new_sync` in `d_loop.c` (type `boolean`).
    new_sync: i32,
    /// Pointer to the registered loop callback table.
    ///
    /// Set by [`D_RegisterLoopCallbacks`] before the game loop starts.
    /// Corresponds to `loop_interface` in `d_loop.c`.
    loop_interface: *const LoopInterfaceT,
    /// Per-player activity flags as last seen from the network layer.
    ///
    /// Distinct from the `playeringame[]` array used by the game logic, which
    /// may be modified during demo playback. Corresponds to
    /// `local_playeringame[]` in `d_loop.c`.
    local_playeringame: [i32; NET_MAXPLAYERS],
    /// Player class sent to the server at connection time.
    ///
    /// Stored here and copied into `NetGameSettingsT` by
    /// [`D_StartNetGame`]. Corresponds to `player_class` in `d_loop.c`.
    player_class: i32,
    /// Adjusted time (in tics) at the start of the previous [`NetUpdate`]
    /// call.
    ///
    /// Used to compute `newtics = nowtime - lasttime`. Corresponds to
    /// `lasttime` in `d_loop.c`.
    lasttime: i32,
    /// Frame counter used by the old net-sync algorithm.
    ///
    /// Corresponds to `frameon` in `d_loop.c`.
    frameon: i32,
    /// Rolling window of four booleans used to detect consistent lag in the
    /// old net-sync algorithm. When all four entries are non-zero,
    /// `skiptics` is set.
    ///
    /// Corresponds to `frameskip[4]` in `d_loop.c`.
    frameskip: [i32; 4],
    /// `maketic` value recorded at the previous old-net-sync frame.
    ///
    /// Compared against `recvtic` to determine whether the local player is
    /// ahead of the network. Corresponds to `oldnettics` in `d_loop.c`.
    oldnettics: i32,
    /// Entry time (in tics) of the previous [`TryRunTics`] call (static
    /// local `oldentertics` in C `TryRunTics`).
    oldentertics: i32,
}

impl MainLoopState {
    /// The state at program start: no tics built or run.
    pub const fn new() -> Self {
        Self {
            // SAFETY: an all-zero `TiccmdSetT` is a valid empty tic set.
            ticdata: unsafe { std::mem::zeroed() },
            maketic: 0,
            recvtic: 0,
            gametic: 0,
            singletics: 0,
            skiptics: 0,
            ticdup: 0,
            new_sync: 0,
            loop_interface: std::ptr::null(),
            local_playeringame: [0; NET_MAXPLAYERS],
            player_class: 0,
            lasttime: 0,
            frameon: 0,
            frameskip: [0; 4],
            oldnettics: 0,
            oldentertics: 0,
        }
    }
}

impl Default for MainLoopState {
    fn default() -> Self {
        Self::new()
    }
}

/// The game-loop state.
pub static MAIN_LOOP: Global<MainLoopState> = Global::new(MainLoopState::new());

/// The tic currently being run (C `gametic`).
pub fn gametic() -> i32 {
    MAIN_LOOP.borrow().gametic
}

/// The tic duplication factor (C `ticdup`).
pub fn ticdup() -> i32 {
    MAIN_LOOP.borrow().ticdup
}

use crate::engine::{
    doom::net::{drone, net_client_connected},
    interface::{
        system::I_AtExit,
        timer::{I_GetTime, I_GetTimeMS, I_Sleep},
        video::I_StartTic,
    },
};

/// Returns the current game time in tics, optionally adjusted by [`offsetms`].
///
/// When `new_sync` is active, the raw millisecond clock is nudged by
/// `offsetms / FRACUNIT` before conversion to tics. Corresponds to
/// `GetAdjustedTime` in `d_loop.c`.
fn get_adjusted_time() -> i32 {
    let mut time_ms = I_GetTimeMS();
    if MAIN_LOOP.borrow().new_sync != 0 {
        time_ms += offsetms / FRACUNIT;
    }
    (time_ms * TICRATE) / 1000
}

/// Attempts to build one new ticcmd for the local player at `maketic`.
///
/// Polls for OS input, dispatches events, and runs the menu. Skips building a
/// command (returning `false`) in drone mode or when the command buffer is too
/// far ahead of the consumed tic counter. On success, stores the command in
/// `ticdata` and increments `maketic`. Corresponds to `BuildNewTic` in
/// `d_loop.c`.
///
/// Returns `true` if a new tic was successfully queued, `false` otherwise.
///
/// # Safety
/// `loop_interface` must have been initialised by a prior call to
/// [`D_RegisterLoopCallbacks`] and all four function pointers it contains
/// (`ProcessEvents`, `RunMenu`, `BuildTiccmd`, and `RunTic`) must be non-null.
unsafe fn build_new_tic() -> bool {
    let (gameticdiv, loop_interface) = {
        let main_loop = MAIN_LOOP.borrow();
        (
            main_loop.gametic / main_loop.ticdup,
            main_loop.loop_interface,
        )
    };

    I_StartTic();

    let iface = &*loop_interface;
    iface.ProcessEvents.unwrap()();

    // Always run the menu
    iface.RunMenu.unwrap()();

    if drone != 0 {
        return false;
    }

    let (maketic, new_sync) = {
        let main_loop = MAIN_LOOP.borrow();
        (main_loop.maketic, main_loop.new_sync)
    };
    if new_sync != 0 {
        if net_client_connected == 0 && maketic - gameticdiv > 2 {
            return false;
        }
        if maketic - gameticdiv > 8 {
            return false;
        }
    } else {
        if maketic - gameticdiv >= 5 {
            return false;
        }
    }

    let mut cmd = std::mem::zeroed::<TiccmdT>();
    iface.BuildTiccmd.unwrap()(&mut cmd, maketic);

    let mut main_loop = MAIN_LOOP.borrow_mut();
    let slot = (maketic as usize) % BACKUPTICS;
    main_loop.ticdata[slot].cmds[LOCALPLAYER as usize] = cmd;
    main_loop.ticdata[slot].ingame[LOCALPLAYER as usize] = 1;

    main_loop.maketic += 1;

    true
}

/// Builds new ticcmds for the console player and (in networked play) sends
/// them to peers.
///
/// Computes the number of new tics elapsed since the last call, applies any
/// pending skip, and calls `build_new_tic` for each new tic. In
/// `singletics` mode this function returns immediately (the caller is
/// expected to call `build_new_tic` directly). Corresponds to `NetUpdate` in
/// `d_loop.c`. Called from `d_main.c` and `r_main.c`.
pub fn NetUpdate() {
    if MAIN_LOOP.borrow().singletics != 0 {
        return;
    }

    let nowtime = get_adjusted_time() / ticdup();
    let newtics = {
        let mut main_loop = MAIN_LOOP.borrow_mut();
        let mut newtics = nowtime - main_loop.lasttime;
        main_loop.lasttime = nowtime;

        if main_loop.skiptics <= newtics {
            newtics -= main_loop.skiptics;
            main_loop.skiptics = 0;
        } else {
            main_loop.skiptics -= newtics;
            newtics = 0;
        }
        newtics
    };

    for _ in 0..newtics {
        if !unsafe { build_new_tic() } {
            break;
        }
    }
}

/// Handles a network disconnection event.
///
/// In drone mode, disconnection is fatal and calls `I_Error`. In normal play
/// the C original prints a diagnostic message; this port omits the `printf`
/// but preserves the drone abort. Corresponds to `D_Disconnected` in
/// `d_loop.c`.
fn d_disconnected() {
    if drone != 0 {
        i_error!("Disconnected from server in drone mode.");
    }
}

/// Receives a completed set of ticcmds from the network layer for `recvtic`.
///
/// If both `ticcmds` and `players_mask` are null, the connection has been
/// lost and `d_disconnected` is called. Otherwise, the supplied commands are
/// written into `ticdata`, skipping the local player's slot (to preserve
/// locally-generated input). Increments `recvtic` on success.
///
/// The `FEATURE_MULTIPLAYER` net client calls this function when a server
/// packet is processed.
///
/// # Safety
/// When non-null, `ticcmds` must point to an array of at least
/// `NET_MAXPLAYERS` `TiccmdT` elements, and `players_mask` must point to an
/// array of at least `NET_MAXPLAYERS` `i32` values.
pub unsafe fn D_ReceiveTic(ticcmds: *mut TiccmdT, players_mask: *mut i32) {
    if ticcmds.is_null() && players_mask.is_null() {
        d_disconnected();
        return;
    }

    let mut main_loop = MAIN_LOOP.borrow_mut();
    let slot = (main_loop.recvtic as usize) % BACKUPTICS;
    for i in 0..NET_MAXPLAYERS {
        if drone == 0 && i == LOCALPLAYER as usize {
            // This is us.  Don't overwrite it.
        } else {
            unsafe {
                main_loop.ticdata[slot].cmds[i] = *ticcmds.add(i);
                main_loop.ticdata[slot].ingame[i] = *players_mask.add(i);
            }
        }
    }

    main_loop.recvtic += 1;
}

/// Initialises the loop timer to the current adjusted time.
///
/// Must be called after the screen is set up but before the first call to
/// [`TryRunTics`], so that the initial `newtics` calculation in [`NetUpdate`]
/// is `0` rather than an arbitrary large value. Called from `d_main.c`.
/// Corresponds to `D_StartGameLoop` in `d_loop.c`.
pub fn D_StartGameLoop() {
    let lasttime = get_adjusted_time() / ticdup();
    MAIN_LOOP.borrow_mut().lasttime = lasttime;
}

/// Configures game settings for a single-player session and updates globals.
///
/// Sets the console player to `0`, forces `num_players = 1`, disables new
/// sync, sets `extratics = 1`, and sets `ticdup = 1`. Copies `ticdup` and
/// `new_sync` back into the loop state.
///
/// The `callback` parameter (used in networked play to report lobby readiness)
/// is ignored. Called from `d_net.c`.
pub fn D_StartNetGame(settings: &mut NetGameSettingsT, _callback: *const ()) {
    let mut main_loop = MAIN_LOOP.borrow_mut();
    settings.consoleplayer = 0;
    settings.num_players = 1;
    settings.player_classes[0] = main_loop.player_class;
    settings.new_sync = 0;
    settings.extratics = 1;
    settings.ticdup = 1;

    main_loop.ticdup = settings.ticdup;
    main_loop.new_sync = settings.new_sync;
}

/// Initialises networking and registers [`D_QuitNetGame`] as an at-exit
/// handler.
///
/// In the full Chocolate Doom build, this function connects to a multiplayer
/// server when `-connect` / `-server` / `-autojoin` flags are present. In
/// this single-player port, all multiplayer logic is compiled out; the
/// function only reads the player class from `connect_data` and registers the
/// exit handler. Always returns `0` (false = not connected to a server).
/// Called from `d_net.c`.
pub fn D_InitNetGame(connect_data: &NetConnectDataT) -> i32 {
    I_AtExit(D_QuitNetGame, Boolean::TRUE);
    MAIN_LOOP.borrow_mut().player_class = connect_data.player_class;
    0 // false
}

/// Shuts down networking before the process exits.
///
/// In the full build this disconnects the net client and shuts down the
/// server. In this single-player port the function is a no-op. It is
/// registered via `I_AtExit` from [`D_InitNetGame`] and called automatically
/// on both clean exit and error exit.
pub fn D_QuitNetGame() {
    // No-op when FEATURE_MULTIPLAYER is not defined.
}

/// Computes the lowest tic that all players have sent input for.
///
/// In single-player mode this is simply `maketic`. In networked play it is
/// `min(maketic, recvtic)` to prevent the game from running ahead of
/// unconfirmed network tics. Corresponds to `GetLowTic` in `d_loop.c`.
fn get_low_tic() -> i32 {
    let main_loop = MAIN_LOOP.borrow();
    let mut lowtic = main_loop.maketic;
    if net_client_connected != 0 && (drone != 0 || main_loop.recvtic < lowtic) {
        lowtic = main_loop.recvtic;
    }
    lowtic
}

/// Applies the classic (pre-new-sync) network timing adjustment.
///
/// Increments the frame counter, finds the key player (lowest active index),
/// and if the local player is a follower: nudges `lasttime` downward when
/// ahead of received tics, records a frameskip entry, and sets
/// `skiptics = 1` if all four recent frames indicate skipping. Corresponds to
/// `OldNetSync` in `d_loop.c`.
fn old_net_sync() {
    let mut main_loop = MAIN_LOOP.borrow_mut();
    main_loop.frameon += 1;

    let mut keyplayer = -1;
    for i in 0..NET_MAXPLAYERS {
        if main_loop.local_playeringame[i] != 0 {
            keyplayer = i as i32;
            break;
        }
    }

    if keyplayer < 0 {
        return;
    }

    if LOCALPLAYER == keyplayer {
        // the key player does not adapt
    } else {
        if main_loop.maketic <= main_loop.recvtic {
            main_loop.lasttime -= 1;
        }

        let frame = (main_loop.frameon & 3) as usize;
        main_loop.frameskip[frame] = (main_loop.oldnettics > main_loop.recvtic) as i32;
        main_loop.oldnettics = main_loop.maketic;

        if main_loop.frameskip.iter().all(|&skipped| skipped != 0) {
            main_loop.skiptics = 1;
        }
    }
}

/// Returns `true` if there is at least one active player that can drive the
/// game forward.
///
/// In networked mode, checks `local_playeringame`. In single-player mode
/// (drone == 0 and not net-connected) always returns `true`. Corresponds to
/// `PlayersInGame` in `d_loop.c`.
fn players_in_game() -> bool {
    let mut result = false;
    if net_client_connected != 0 {
        let main_loop = MAIN_LOOP.borrow();
        for i in 0..NET_MAXPLAYERS {
            result = result || main_loop.local_playeringame[i] != 0;
        }
    }
    if drone == 0 {
        result = true;
    }
    result
}

/// Clears one-shot fields from each player's ticcmd in a duplicated tic set.
///
/// When `ticdup > 1`, each group of `ticdup` tics shares the same `TiccmdSetT`
/// slot. Chat characters and `BT_SPECIAL` button events must be cleared after
/// the first tic of the group to avoid repeating them. `BT_SPECIAL` is the
/// high bit (`0x80`) of the `buttons` field. Corresponds to `TicdupSquash` in
/// `d_loop.c`.
///
/// # Safety
/// `set` must be a valid, non-null pointer to a `TiccmdSetT`.
unsafe fn ticdup_squash(set: *mut TiccmdSetT) {
    const BT_SPECIAL: u8 = 128;
    for i in 0..NET_MAXPLAYERS {
        let cmd = &mut (*set).cmds[i];
        cmd.chatchar = 0;
        if cmd.buttons & BT_SPECIAL != 0 {
            cmd.buttons = 0;
        }
    }
}

/// Marks all non-local player slots as inactive in a tic set.
///
/// Used in single-player mode to ensure that only the local player's slot
/// drives the game tick. Corresponds to `SinglePlayerClear` in `d_loop.c`.
///
/// # Safety
/// `set` must be a valid, non-null pointer to a `TiccmdSetT`.
unsafe fn single_player_clear(set: *mut TiccmdSetT) {
    for i in 0..NET_MAXPLAYERS {
        if i != LOCALPLAYER as usize {
            (*set).ingame[i] = 0;
        }
    }
}

/// Attempts to advance the game by as many tics as time permits.
///
/// This is the heart of the game loop. Each frame the renderer calls this
/// function, which:
/// 1. Computes the elapsed real tics since the last call.
/// 2. Builds new ticcmds (or calls `build_new_tic` once in singletics mode).
/// 3. Determines how many tics to run (`counts`), capped so the game does not
///    outrun available input or real time.
/// 4. Waits (with 1 ms sleeps) until enough input is available, returning
///    early if real time has advanced.
/// 5. For each count: copies `ingame` flags, calls `RunTic` via the loop
///    interface, increments `gametic`, and squashes one-shot fields for the
///    next `ticdup` sub-tic.
///
/// Called from `d_main.c`. Corresponds to `TryRunTics` in `d_loop.c`.
pub fn TryRunTics() {
    unsafe {
        let entertic = I_GetTime() / ticdup();
        let (realtics, singletics) = {
            let mut main_loop = MAIN_LOOP.borrow_mut();
            let realtics = entertic - main_loop.oldentertics;
            main_loop.oldentertics = entertic;
            (realtics, main_loop.singletics)
        };

        if singletics != 0 {
            build_new_tic();
        } else {
            NetUpdate();
        }

        let mut lowtic = get_low_tic();
        let availabletics = lowtic - gametic() / ticdup();

        let counts: i32;
        if MAIN_LOOP.borrow().new_sync != 0 {
            counts = availabletics;
        } else {
            let mut c: i32;
            if realtics < availabletics - 1 {
                c = realtics + 1;
            } else if realtics < availabletics {
                c = realtics;
            } else {
                c = availabletics;
            }

            if c < 1 {
                c = 1;
            }

            if net_client_connected != 0 {
                old_net_sync();
            }

            counts = c;
        }

        let mut counts = if counts < 1 { 1 } else { counts };

        while !players_in_game() || lowtic < gametic() / ticdup() + counts {
            NetUpdate();
            lowtic = get_low_tic();

            if lowtic < gametic() / ticdup() {
                i_error!("TryRunTics: lowtic < gametic");
            }

            if I_GetTime() / ticdup() - entertic > 0 {
                return;
            }

            I_Sleep(1);
        }

        while counts > 0 {
            counts -= 1;

            if !players_in_game() {
                return;
            }

            // `RunTic` receives pointers into this slot, so it is taken from
            // the stable address of `ticdata` inside `MAIN_LOOP`.
            let slot = ((gametic() / ticdup()) as usize) % BACKUPTICS;
            let set = &raw mut (*MAIN_LOOP.as_ptr()).ticdata[slot];

            if net_client_connected == 0 {
                single_player_clear(set);
            }

            for _ in 0..ticdup() {
                if gametic() / ticdup() > lowtic {
                    i_error!("gametic>lowtic");
                }

                let ingame = (*set).ingame;
                let loop_interface = {
                    let mut main_loop = MAIN_LOOP.borrow_mut();
                    main_loop.local_playeringame = ingame;
                    main_loop.loop_interface
                };

                let iface = &*loop_interface;
                iface.RunTic.unwrap()((*set).cmds.as_mut_ptr(), (*set).ingame.as_mut_ptr());
                MAIN_LOOP.borrow_mut().gametic += 1;

                ticdup_squash(set);
            }

            NetUpdate();
        }
    }
}

/// Registers the loop callback table used by all main-loop operations.
///
/// Must be called before [`D_StartGameLoop`] or [`TryRunTics`]. The `i`
/// pointer is stored directly; the caller must ensure the pointed-to
/// `LoopInterfaceT` remains valid for the lifetime of the game loop. Called
/// from `d_net.c`.
///
/// # Safety
/// `i` must be a valid, non-null pointer to a fully-initialised
/// `LoopInterfaceT` with all four function pointers set to non-null values,
/// and it must stay valid for as long as the game loop runs.
pub unsafe fn D_RegisterLoopCallbacks(i: *const LoopInterfaceT) {
    MAIN_LOOP.borrow_mut().loop_interface = i;
}
