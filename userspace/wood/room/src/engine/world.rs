//! The play simulation's state, borrowed explicitly instead of reached
//! through `Global`s and raw pointers.
//!
//! [`World`] is everything the play code reads or writes during a tic and
//! that a save game captures. It lives in [`WORLD`]; each top-level entry
//! point (`G_Ticker`'s phases, the game actions that load, save or set up a
//! level, the cheat responders) borrows it once and passes `&mut World`
//! down the play call graph. No guard is held across `TryRunTics` or
//! `NetUpdate`, which reach those entry points again; the debug borrow
//! counter of [`Global`] panics on such a nested borrow.

use crate::engine::{
    doom::player::{MAXPLAYERS, PlayerId, PlayerT},
    game::BODYQUESIZE,
    global::Global,
    map::LineId,
    misc::random::PlayRandom,
    play::{
        ceilng::CeilingState,
        enemy::EnemyState,
        map::MapState,
        maputl::{InterceptHit, MAXINTERCEPTS, MapUtlState, intercept_t},
        mobj::{MobjId, MobjState},
        plats::PlatState,
        pspr::PsprState,
        sight::SightState,
        switch::{MAXBUTTONS, button_t},
        user::UserState,
    },
};

/// The play simulation's state.
///
/// The level, the map object arena and the thinkers still live in their
/// subsystem `Global`s. They are to join as `pub` fields too, so callers
/// can split borrows:
///
/// - `level`: the loaded level (`LEVEL`, spawn spots, player starts);
/// - `mobjs`: the map object arena (`MOBJ_ARENA`);
/// - `thinkers`: the thinker list and the other thinker arenas (`TICK`,
///   `CEILING_ARENA` ... `FIRE_FLICKER_ARENA`).
pub struct World {
    /// The player slots (C `players[MAXPLAYERS]`), indexed by `PlayerId`.
    pub players: [PlayerT; MAXPLAYERS],
    /// Current level time in tics (C `leveltime`), incremented once per
    /// `P_Ticker` and saved in the save game header.
    pub leveltime: i32,
    /// The game settings play reads, copied from the game controller.
    pub session: Session,
    /// The play random cursor (C `prndindex`).
    pub random: PlayRandom,
    /// The active ceilings and platforms, the pressed buttons and the item
    /// respawn queue.
    pub active: ActiveSpecials,
    /// The per-call scratch state of play.
    pub scratch: PlayScratch,
}

impl World {
    /// The state at program start: every player slot empty, no level.
    pub const fn new() -> Self {
        Self {
            // SAFETY: an all-zero `PlayerT` is a valid empty player slot.
            players: unsafe { std::mem::zeroed() },
            leveltime: 0,
            session: Session::new(),
            random: PlayRandom::new(),
            active: ActiveSpecials::new(),
            scratch: PlayScratch::new(),
        }
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

/// The game settings play reads (`g_game.c` and `d_main.c` globals).
///
/// The settings are owned by the game controller (`GAME`, `DOOM_MAIN`),
/// which copies them in when it loads a level (`G_DoLoadLevel`, also
/// reached from a new game and a loaded save) and when a player leaves a
/// net game. The level totals and the body queue are play state kept only
/// here.
pub struct Session {
    /// Skill level (C `gameskill`, `sk_baby` .. `sk_nightmare`).
    pub gameskill: i32,
    /// Deathmatch mode (C `deathmatch`): 0 co-op, 1 deathmatch, 2 altdeath.
    pub deathmatch: i32,
    /// Non-zero in a network game (C `netgame`).
    pub netgame: i32,
    /// Non-zero when monsters respawn (C `respawnmonsters`).
    pub respawnmonsters: i32,
    /// Non-zero with `-fast` (C `fastparm`).
    pub fastparm: i32,
    /// Non-zero with `-nomonsters` (C `nomonsters`).
    pub nomonsters: i32,
    /// The loaded map (C `gamemap`).
    pub gamemap: i32,
    /// The loaded episode (C `gameepisode`).
    pub gameepisode: i32,
    /// The local player's slot (C `consoleplayer`).
    pub consoleplayer: i32,
    /// Per-slot presence flags (C `playeringame`).
    pub playeringame: [i32; MAXPLAYERS],
    /// Monsters counting toward the kill total on this level (C
    /// `totalkills`), counted as they spawn.
    pub totalkills: i32,
    /// Items counting toward the item total on this level (C `totalitems`).
    pub totalitems: i32,
    /// Secret sectors on this level (C `totalsecret`).
    pub totalsecret: i32,
    /// Bodies queued so far (C `bodyqueslot`); the next one goes into slot
    /// `bodyqueslot % BODYQUESIZE`.
    pub bodyqueslot: i32,
    /// Circular queue of the latest player corpses (C `bodyque`); the oldest
    /// is removed when the queue wraps (see `G_CheckSpot`).
    pub bodyque: [Option<MobjId>; BODYQUESIZE],
}

impl Session {
    /// The settings at program start: no game.
    pub const fn new() -> Self {
        Self {
            gameskill: 0,
            deathmatch: 0,
            netgame: 0,
            respawnmonsters: 0,
            fastparm: 0,
            nomonsters: 0,
            gamemap: 0,
            gameepisode: 0,
            consoleplayer: 0,
            playeringame: [0; MAXPLAYERS],
            totalkills: 0,
            totalitems: 0,
            totalsecret: 0,
            bodyqueslot: 0,
            bodyque: [None; BODYQUESIZE],
        }
    }

    /// The console player's id (C `&players[consoleplayer]`).
    #[track_caller]
    pub fn console(&self) -> PlayerId {
        PlayerId::new(self.consoleplayer as usize)
    }

    /// Whether player slot `player` is in the game (C
    /// `playeringame[player]`).
    pub fn in_game(&self, player: usize) -> bool {
        self.playeringame[player] != 0
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

/// The level's active specials (`p_ceilng.c`, `p_plats.c`, `p_switch.c`
/// and `p_mobj.c` globals).
pub struct ActiveSpecials {
    /// The active-ceiling table (C `activeceilings`).
    pub ceilings: CeilingState,
    /// The active-platform table (C `activeplats`).
    pub plats: PlatState,
    /// The pressed switches waiting to pop back (C `buttonlist`).
    pub buttons: [button_t; MAXBUTTONS],
    /// The item respawn queue and the null-object stand-in.
    pub mobj: MobjState,
}

impl ActiveSpecials {
    /// Nothing active.
    pub const fn new() -> Self {
        Self {
            ceilings: CeilingState::new(),
            plats: PlatState::new(),
            buttons: [button_t::EMPTY; MAXBUTTONS],
            mobj: MobjState::new(),
        }
    }
}

impl Default for ActiveSpecials {
    fn default() -> Self {
        Self::new()
    }
}

/// The per-call scratch state of play (`p_map.c`, `p_maputl.c`,
/// `p_sight.c`, `p_enemy.c`, `p_pspr.c` and `p_user.c` globals).
///
/// Play code re-enters itself (a blockmap or path callback damages an
/// object whose death actions move, spawn and traverse again), overwriting
/// this state exactly as nested calls overwrote the C globals. Values
/// needed after such a call are copied out before it and re-read after it.
pub struct PlayScratch {
    /// Movement, slide, hitscan, use, splash and crush parameters and
    /// results.
    pub map: MapState,
    /// The last line opening and the current path traversal.
    pub maputl: MapUtlState,
    /// The intercepts collected by the current path traversal, filled up to
    /// `maputl.intercept_count`.
    pub intercepts: [intercept_t; MAXINTERCEPTS],
    /// The sight check's ray and slopes (shared with auto-aim).
    pub sight: SightState,
    /// Monster AI scratch (noise target, Arch-vile corpse search, boss
    /// brain targets).
    pub enemy: EnemyState,
    /// Weapon bob and bullet slope.
    pub pspr: PsprState,
    /// Whether the moving player is on the ground.
    pub user: UserState,
    /// Search stamp (C `validcount`): incremented before each search, and
    /// sectors and lines visited by it are marked with it. The renderer
    /// stamps the same sectors with it, so it is one counter for both.
    pub validcount: i32,
}

impl PlayScratch {
    /// The state at program start: nothing searched or traversed.
    pub const fn new() -> Self {
        Self {
            map: MapState::new(),
            maputl: MapUtlState::new(),
            intercepts: [intercept_t {
                frac: 0,
                hit: InterceptHit::Line(LineId::new(0)),
            }; MAXINTERCEPTS],
            sight: SightState::new(),
            enemy: EnemyState::new(),
            pspr: PsprState::new(),
            user: UserState::new(),
            validcount: 1,
        }
    }
}

impl Default for PlayScratch {
    fn default() -> Self {
        Self::new()
    }
}

/// The play simulation's state, borrowed once per entry point.
pub static WORLD: Global<World> = Global::new(World::new());

#[cfg(test)]
mod tests {
    use super::*;

    /// An entry point reached while another one holds the world, the
    /// re-entrance the borrow counter exists to catch. Uses its own cell so
    /// that tests borrowing [`WORLD`] in parallel are not disturbed.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "Global already borrowed")]
    fn nested_entry_borrow_panics() {
        static ENTRY_WORLD: Global<World> = Global::new(World::new());
        let _entry = ENTRY_WORLD.borrow_mut();
        let _nested = ENTRY_WORLD.borrow_mut();
    }
}
