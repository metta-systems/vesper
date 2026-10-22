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

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::os::raw::c_int;

use crate::i_error;
use crate::types::Boolean;

use crate::doom::d_net::{LoopInterfaceT, NetConnectDataT, NetGameSettingsT};
use crate::doom::d_player::TiccmdT;
use crate::doom::i_timer::TICRATE;
use crate::doom::m_fixed::FRACUNIT;

/// Maximum number of players in a multiplayer session.
///
/// Mirrors `NET_MAXPLAYERS` from `net_defs.h`. Even in single-player mode,
/// arrays are sized by this constant to keep the data layout compatible with
/// the C original.
const NET_MAXPLAYERS: usize = 8;

/// Number of tic slots kept in the circular `TICDATA` ring buffer.
///
/// Exported as `pub` because other modules (e.g. `d_loop_c.rs`) read it.
/// Corresponds to `BACKUPTICS` in `d_loop.h`.
pub const BACKUPTICS: usize = 128;

/// A complete set of player commands and participation flags for one tic.
///
/// One `TiccmdSetT` occupies a slot in `TICDATA`. The `cmds` array holds
/// one input record per potential player; `ingame` tracks which player slots
/// are actually active for this tic.
///
/// Corresponds to `ticcmd_set_t` in `d_loop.c`.
#[repr(C)]
struct TiccmdSetT {
    /// Per-player input commands for this tic.
    cmds: [TiccmdT; NET_MAXPLAYERS],
    /// Non-zero for each player slot that is active this tic (`boolean` in C).
    ingame: [c_int; NET_MAXPLAYERS],
}

/// Circular ring buffer of tic command sets, indexed by `tic % BACKUPTICS`.
///
/// `MAKETIC` selects where new local commands are written; `RECVTIC` tracks
/// how many complete tic sets have been received from the network (single-
/// player: always equals `MAKETIC`). Corresponds to `ticdata[]` in `d_loop.c`.
static mut TICDATA: [TiccmdSetT; BACKUPTICS] = unsafe { std::mem::zeroed() };

/// Index of the next tic for which a local `TiccmdT` has not yet been built.
///
/// Incremented by `build_new_tic`. Corresponds to `maketic` in `d_loop.c`.
static mut MAKETIC: c_int = 0;

/// Number of complete tic command sets received from the server.
///
/// In single-player mode this advances in lock-step with `MAKETIC` via
/// [`D_ReceiveTic`]. Corresponds to `recvtic` in `d_loop.c`.
static mut RECVTIC: c_int = 0;

/// Index of the tic currently being run (or about to be run).
///
/// Exported as `#[no_mangle]` so C code can read it directly.
/// Corresponds to `gametic` in `d_loop.c`.
#[no_mangle]
pub static mut gametic: c_int = 0;

/// When non-zero, exactly one tic is built and run per [`TryRunTics`] call.
/// Set by `-timedemo` mode. Exported as `#[no_mangle]` for C access.
/// Corresponds to `singletics` in `d_loop.c` (type `boolean`).
#[no_mangle]
pub static mut singletics: c_int = 0; // boolean

/// Index of the local player in the `NET_MAXPLAYERS` arrays.
///
/// Always `0` in single-player mode. Corresponds to `localplayer` in
/// `d_loop.c`.
static mut LOCALPLAYER: c_int = 0;

/// Number of tics to skip building during the next [`NetUpdate`] call.
///
/// Used by the old net-sync algorithm to slow input generation when a follower
/// player is too far ahead of the key player. Corresponds to `skiptics` in
/// `d_loop.c`.
static mut SKIPTICS: c_int = 0;

/// Tic duplication factor: every `ticdup`-th input sample is sent over the
/// network, reducing bandwidth at the cost of input resolution.
///
/// Always `1` in single-player mode. Exported as `#[no_mangle]` for C access.
/// Corresponds to `ticdup` in `d_loop.c`.
#[no_mangle]
pub static mut ticdup: c_int = 0;

/// Millisecond offset applied to the timer when `NEW_SYNC` is active.
///
/// Allows the network client to nudge the local clock to stay in sync with the
/// server. Stored as a `fixed_t` (i.e. scaled by [`FRACUNIT`]). Exported as
/// `#[no_mangle]` for C access. Corresponds to `offsetms` in `d_loop.c`.
#[no_mangle]
pub static mut offsetms: c_int = 0; // fixed_t

/// Whether to use the new client synchronisation algorithm.
///
/// When non-zero, [`TryRunTics`] runs exactly `availabletics` tics per frame
/// and `build_new_tic` applies tighter buffering limits.
/// Always `0` in this single-player build (hardcoded in [`D_StartNetGame`]).
/// Corresponds to `new_sync` in `d_loop.c` (type `boolean`).
static mut NEW_SYNC: c_int = 0; // boolean

/// Pointer to the registered loop callback table.
///
/// Set by [`D_RegisterLoopCallbacks`] before the game loop starts.
/// Corresponds to `loop_interface` in `d_loop.c`.
static mut LOOP_INTERFACE: *mut LoopInterfaceT = std::ptr::null_mut();

/// Per-player activity flags as last seen from the network layer.
///
/// Distinct from the `playeringame[]` array used by the game logic, which may
/// be modified during demo playback. Corresponds to `local_playeringame[]` in
/// `d_loop.c`.
static mut LOCAL_PLAYERINGAME: [c_int; NET_MAXPLAYERS] = [0; NET_MAXPLAYERS];

/// Player class sent to the server at connection time.
///
/// Stored here and copied into `NetGameSettingsT` by [`D_StartNetGame`].
/// Corresponds to `player_class` in `d_loop.c`.
static mut PLAYER_CLASS: c_int = 0;

/// Adjusted time (in tics) at the start of the previous [`NetUpdate`] call.
///
/// Used to compute `newtics = nowtime - LASTTIME`. Corresponds to `lasttime`
/// in `d_loop.c`.
static mut LASTTIME: c_int = 0;

/// Frame counter used by the old net-sync algorithm.
///
/// Corresponds to `frameon` in `d_loop.c`.
static mut FRAMEON: c_int = 0;

/// Rolling window of four booleans used to detect consistent lag in the old
/// net-sync algorithm. When all four entries are non-zero, `SKIPTICS` is set.
///
/// Corresponds to `frameskip[4]` in `d_loop.c`.
static mut FRAMESKIP: [c_int; 4] = [0; 4];

/// `MAKETIC` value recorded at the previous old-net-sync frame.
///
/// Compared against `RECVTIC` to determine whether the local player is ahead
/// of the network. Corresponds to `oldnettics` in `d_loop.c`.
static mut OLDNETTICS: c_int = 0;

extern "C" {
    /// Non-zero when running as a network drone (spectator only, no input).
    static mut drone: c_int; // boolean
    /// Non-zero when a network client connection is active.
    static mut net_client_connected: c_int; // boolean

    /// Returns the current time in milliseconds since startup.
    fn I_GetTimeMS() -> c_int;
    /// Returns the current time in tics (1/35 second units) since startup.
    fn I_GetTime() -> c_int;
    /// Polls the OS for new input events and queues them.
    fn I_StartTic();
    /// Sleeps for approximately `ms` milliseconds.
    fn I_Sleep(ms: c_int);
    /// Registers `func` to be called at clean exit; `run_on_error` controls
    /// whether it also runs when `I_Error` is invoked.
    fn I_AtExit(func: extern "C" fn(), run_on_error: Boolean);

    /// Drains the event queue and dispatches events to handlers.
    fn D_ProcessEvents();
    /// Fills `cmd` with input state for the local player at tic `maketic`.
    fn G_BuildTiccmd(cmd: *mut TiccmdT, maketic: c_int);
    /// Advances the menu state by one tic.
    fn M_Ticker();
}

/// Returns the current game time in tics, optionally adjusted by [`offsetms`].
///
/// When `NEW_SYNC` is active, the raw millisecond clock is nudged by
/// `offsetms / FRACUNIT` before conversion to tics. Corresponds to
/// `GetAdjustedTime` in `d_loop.c`.
///
/// # Safety
/// Must be called from the single-threaded game loop only. The function reads
/// the mutable globals `NEW_SYNC` and `offsetms` without synchronisation, and
/// calls the C FFI function `I_GetTimeMS`.
unsafe fn get_adjusted_time() -> c_int {
    let mut time_ms = I_GetTimeMS();
    if NEW_SYNC != 0 {
        time_ms += offsetms / FRACUNIT;
    }
    (time_ms * TICRATE) / 1000
}

/// Attempts to build one new ticcmd for the local player at `MAKETIC`.
///
/// Polls for OS input, dispatches events, and runs the menu. Skips building a
/// command (returning `false`) in drone mode or when the command buffer is too
/// far ahead of the consumed tic counter. On success, stores the command in
/// `TICDATA` and increments `MAKETIC`. Corresponds to `BuildNewTic` in
/// `d_loop.c`.
///
/// Returns `true` if a new tic was successfully queued, `false` otherwise.
///
/// # Safety
/// `LOOP_INTERFACE` must have been initialised by a prior call to
/// [`D_RegisterLoopCallbacks`] and all four function pointers it contains
/// (`ProcessEvents`, `RunMenu`, `BuildTiccmd`, and `RunTic`) must be non-null.
/// Must be called from the single-threaded game loop only, as it reads and
/// writes the mutable globals `gametic`, `ticdup`, `drone`, `NEW_SYNC`,
/// `net_client_connected`, `MAKETIC`, `TICDATA`, and `LOCALPLAYER` without
/// synchronisation.
unsafe fn build_new_tic() -> bool {
    let gameticdiv = gametic / ticdup;

    I_StartTic();

    let iface = &*LOOP_INTERFACE;
    iface.ProcessEvents.unwrap()();

    // Always run the menu
    iface.RunMenu.unwrap()();

    if drone != 0 {
        return false;
    }

    if NEW_SYNC != 0 {
        if net_client_connected == 0 && MAKETIC - gameticdiv > 2 {
            return false;
        }
        if MAKETIC - gameticdiv > 8 {
            return false;
        }
    } else {
        if MAKETIC - gameticdiv >= 5 {
            return false;
        }
    }

    let mut cmd = std::mem::zeroed::<TiccmdT>();
    iface.BuildTiccmd.unwrap()(&mut cmd, MAKETIC);

    let slot = (MAKETIC as usize) % BACKUPTICS;
    TICDATA[slot].cmds[LOCALPLAYER as usize] = cmd;
    TICDATA[slot].ingame[LOCALPLAYER as usize] = 1;

    MAKETIC += 1;

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
#[no_mangle]
pub extern "C" fn NetUpdate() {
    unsafe {
        if singletics != 0 {
            return;
        }

        let nowtime = get_adjusted_time() / ticdup;
        let mut newtics = nowtime - LASTTIME;
        LASTTIME = nowtime;

        if SKIPTICS <= newtics {
            newtics -= SKIPTICS;
            SKIPTICS = 0;
        } else {
            SKIPTICS -= newtics;
            newtics = 0;
        }

        for _ in 0..newtics {
            if !build_new_tic() {
                break;
            }
        }
    }
}

/// Handles a network disconnection event.
///
/// In drone mode, disconnection is fatal and calls `I_Error`. In normal play
/// the C original prints a diagnostic message; this port omits the `printf`
/// but preserves the drone abort. Corresponds to `D_Disconnected` in
/// `d_loop.c`.
///
/// # Safety
/// Must be called from the single-threaded game loop only. The function reads
/// the mutable global `drone` without synchronisation.
unsafe fn d_disconnected() {
    if drone != 0 {
        i_error!("Disconnected from server in drone mode.");
    }
}

/// Receives a completed set of ticcmds from the network layer for `RECVTIC`.
///
/// If both `ticcmds` and `players_mask` are null, the connection has been
/// lost and `d_disconnected` is called. Otherwise, the supplied commands are
/// written into `TICDATA`, skipping the local player's slot (to preserve
/// locally-generated input). Increments `RECVTIC` on success.
///
/// Exported as `#[no_mangle]` for C callers. The `FEATURE_MULTIPLAYER` net
/// client calls this function when a server packet is processed.
///
/// # Safety
/// When non-null, `ticcmds` must point to an array of at least
/// `NET_MAXPLAYERS` `TiccmdT` elements, and `players_mask` must point to an
/// array of at least `NET_MAXPLAYERS` `c_int` values.
#[no_mangle]
pub extern "C" fn D_ReceiveTic(ticcmds: *mut TiccmdT, players_mask: *mut c_int) {
    unsafe {
        if ticcmds.is_null() && players_mask.is_null() {
            d_disconnected();
            return;
        }

        for i in 0..NET_MAXPLAYERS {
            if drone == 0 && i == LOCALPLAYER as usize {
                // This is us.  Don't overwrite it.
            } else {
                TICDATA[(RECVTIC as usize) % BACKUPTICS].cmds[i] = *ticcmds.add(i);
                TICDATA[(RECVTIC as usize) % BACKUPTICS].ingame[i] = *players_mask.add(i);
            }
        }

        RECVTIC += 1;
    }
}

/// Initialises the loop timer to the current adjusted time.
///
/// Must be called after the screen is set up but before the first call to
/// [`TryRunTics`], so that the initial `newtics` calculation in [`NetUpdate`]
/// is `0` rather than an arbitrary large value. Called from `d_main.c`.
/// Corresponds to `D_StartGameLoop` in `d_loop.c`.
#[no_mangle]
pub extern "C" fn D_StartGameLoop() {
    unsafe {
        LASTTIME = get_adjusted_time() / ticdup;
    }
}

/// Configures game settings for a single-player session and updates globals.
///
/// Sets the console player to `0`, forces `num_players = 1`, disables new
/// sync, sets `extratics = 1`, and sets `ticdup = 1`. Copies `ticdup` and
/// `new_sync` back into the module-level statics used by the loop.
///
/// The `callback` parameter (used in networked play to report lobby readiness)
/// is ignored. Called from `d_net.c`.
///
/// # Safety
/// `settings` must be a valid, non-null pointer to a `NetGameSettingsT`.
#[no_mangle]
pub extern "C" fn D_StartNetGame(settings: *mut NetGameSettingsT, _callback: *const ()) {
    unsafe {
        (*settings).consoleplayer = 0;
        (*settings).num_players = 1;
        (*settings).player_classes[0] = PLAYER_CLASS;
        (*settings).new_sync = 0;
        (*settings).extratics = 1;
        (*settings).ticdup = 1;

        ticdup = (*settings).ticdup;
        NEW_SYNC = (*settings).new_sync;
    }
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
///
/// # Safety
/// `connect_data` must be a valid, non-null pointer to a `NetConnectDataT`.
#[no_mangle]
pub extern "C" fn D_InitNetGame(connect_data: *mut NetConnectDataT) -> c_int {
    unsafe {
        I_AtExit(D_QuitNetGame, Boolean::TRUE);
        PLAYER_CLASS = (*connect_data).player_class;
    }
    0 // false
}

/// Shuts down networking before the process exits.
///
/// In the full build this disconnects the net client and shuts down the
/// server. In this single-player port the function is a no-op. It is
/// registered via `I_AtExit` from [`D_InitNetGame`] and called automatically
/// on both clean exit and error exit.
#[no_mangle]
pub extern "C" fn D_QuitNetGame() {
    // No-op when FEATURE_MULTIPLAYER is not defined.
}

/// Computes the lowest tic that all players have sent input for.
///
/// In single-player mode this is simply `MAKETIC`. In networked play it is
/// `min(MAKETIC, RECVTIC)` to prevent the game from running ahead of
/// unconfirmed network tics. Corresponds to `GetLowTic` in `d_loop.c`.
///
/// # Safety
/// Must be called from the single-threaded game loop only. The function reads
/// the mutable globals `MAKETIC`, `RECVTIC`, `net_client_connected`, and
/// `drone` without synchronisation.
unsafe fn get_low_tic() -> c_int {
    let mut lowtic = MAKETIC;
    if net_client_connected != 0 && (drone != 0 || RECVTIC < lowtic) {
        lowtic = RECVTIC;
    }
    lowtic
}

/// Applies the classic (pre-new-sync) network timing adjustment.
///
/// Increments the frame counter, finds the key player (lowest active index),
/// and if the local player is a follower: nudges `LASTTIME` downward when
/// ahead of received tics, records a frameskip entry, and sets `SKIPTICS = 1`
/// if all four recent frames indicate skipping. Corresponds to `OldNetSync`
/// in `d_loop.c`.
///
/// # Safety
/// Must be called from the single-threaded game loop only. The function reads
/// and writes the mutable globals `FRAMEON`, `LOCAL_PLAYERINGAME`,
/// `LOCALPLAYER`, `MAKETIC`, `RECVTIC`, `LASTTIME`, `FRAMESKIP`,
/// `OLDNETTICS`, and `SKIPTICS` without synchronisation.
unsafe fn old_net_sync() {
    FRAMEON += 1;

    let mut keyplayer = -1;
    for i in 0..NET_MAXPLAYERS {
        if LOCAL_PLAYERINGAME[i] != 0 {
            keyplayer = i as c_int;
            break;
        }
    }

    if keyplayer < 0 {
        return;
    }

    if LOCALPLAYER == keyplayer {
        // the key player does not adapt
    } else {
        if MAKETIC <= RECVTIC {
            LASTTIME -= 1;
        }

        FRAMESKIP[(FRAMEON & 3) as usize] = (OLDNETTICS > RECVTIC) as c_int;
        OLDNETTICS = MAKETIC;

        if FRAMESKIP[0] != 0 && FRAMESKIP[1] != 0 && FRAMESKIP[2] != 0 && FRAMESKIP[3] != 0 {
            SKIPTICS = 1;
        }
    }
}

/// Returns `true` if there is at least one active player that can drive the
/// game forward.
///
/// In networked mode, checks `LOCAL_PLAYERINGAME`. In single-player mode
/// (drone == 0 and not net-connected) always returns `true`. Corresponds to
/// `PlayersInGame` in `d_loop.c`.
///
/// # Safety
/// Must be called from the single-threaded game loop only. The function reads
/// the mutable globals `net_client_connected`, `drone`, and
/// `LOCAL_PLAYERINGAME` without synchronisation.
unsafe fn players_in_game() -> bool {
    let mut result = false;
    if net_client_connected != 0 {
        for i in 0..NET_MAXPLAYERS {
            result = result || LOCAL_PLAYERINGAME[i] != 0;
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
#[no_mangle]
pub extern "C" fn TryRunTics() {
    unsafe {
        let entertic = I_GetTime() / ticdup;
        static mut OLDENTERTICS: c_int = 0;
        let realtics = entertic - OLDENTERTICS;
        OLDENTERTICS = entertic;

        if singletics != 0 {
            build_new_tic();
        } else {
            NetUpdate();
        }

        let mut lowtic = get_low_tic();
        let availabletics = lowtic - gametic / ticdup;

        let counts: c_int;
        if NEW_SYNC != 0 {
            counts = availabletics;
        } else {
            let mut c: c_int;
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

        while !players_in_game() || lowtic < gametic / ticdup + counts {
            NetUpdate();
            lowtic = get_low_tic();

            if lowtic < gametic / ticdup {
                i_error!("TryRunTics: lowtic < gametic");
            }

            if I_GetTime() / ticdup - entertic > 0 {
                return;
            }

            I_Sleep(1);
        }

        while counts > 0 {
            counts -= 1;

            if !players_in_game() {
                return;
            }

            let set = &mut TICDATA[((gametic / ticdup) as usize) % BACKUPTICS] as *mut TiccmdSetT;

            if net_client_connected == 0 {
                single_player_clear(set);
            }

            for _ in 0..ticdup {
                if gametic / ticdup > lowtic {
                    i_error!("gametic>lowtic");
                }

                for i in 0..NET_MAXPLAYERS {
                    LOCAL_PLAYERINGAME[i] = (*set).ingame[i];
                }

                let iface = &*LOOP_INTERFACE;
                iface.RunTic.unwrap()((*set).cmds.as_mut_ptr(), (*set).ingame.as_mut_ptr());
                gametic += 1;

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
/// `LoopInterfaceT` with all four function pointers set to non-null values.
#[no_mangle]
pub extern "C" fn D_RegisterLoopCallbacks(i: *mut LoopInterfaceT) {
    unsafe {
        LOOP_INTERFACE = i;
    }
}
