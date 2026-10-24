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
    doom::player::{MAXPLAYERS, PlayerT},
    global::Global,
};

/// The play simulation's state.
///
/// Holds the player slots; the rest of the state still lives in the
/// subsystem `Global`s. It is to hold, as `pub` fields so callers can split
/// borrows:
///
/// - `level`: the loaded level (`LEVEL`, spawn spots, player starts);
/// - `mobjs`: the map object arena (`MOBJ_ARENA`);
/// - `thinkers`: the thinker list and the other thinker arenas (`TICK`,
///   `CEILING_ARENA` ... `FIRE_FLICKER_ARENA`);
/// - `leveltime`;
/// - `session`: the game settings play reads (skill, deathmatch, netgame,
///   respawn and fast monsters, map and episode, console player, players in
///   game, level totals, the body queue), copied from `GAME`;
/// - `random`: the play random index (`prndindex` of `RANDOM`);
/// - `active`: the active ceilings and platforms, the switch buttons and
///   the item respawn queue (`CEILINGS`, `PLATS`, `SWITCHES`, `MOBJ`);
/// - `scratch`: the per-call scratch state of play (`MAP`, `MAPUTL`,
///   `INTERCEPTS`, `SIGHT`, `ENEMY`, `PSPR`, `USER`, `VALIDCOUNT`).
pub struct World {
    /// The player slots (C `players[MAXPLAYERS]`), indexed by `PlayerId`.
    pub players: [PlayerT; MAXPLAYERS],
}

impl World {
    /// The state at program start: every player slot empty.
    pub const fn new() -> Self {
        Self {
            // SAFETY: an all-zero `PlayerT` is a valid empty player slot.
            players: unsafe { std::mem::zeroed() },
        }
    }
}

impl Default for World {
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
