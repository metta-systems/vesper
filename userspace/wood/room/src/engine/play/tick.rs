//! Thinker management and the main per-tic game loop.
//!
//! Rust port of `vendor/doomgeneric/p_tick.c`.  Every game object that needs
//! to update once per tic (monsters, moving platforms, ceilings, …) lives in
//! the [`Arena`] of its type and is listed, by [`ThinkerId`], in the ordered
//! thinker list that replaces C's doubly-linked list headed by `thinkercap`.
//! [`P_RunThinkers`] walks that list every tic and drives each object's logic
//! according to its [`ThinkerKind`].

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::engine::{
    arena::{Arena, Handle, Pooled},
    doom::player::PlayerId,
    game::{demoplayback, paused},
    misc::menu::menu_active,
    play::{
        ceilng::{T_MoveCeiling, ceiling_t},
        doors::{T_VerticalDoor, vldoor_t},
        floor::{T_MoveFloor, floormove_t},
        lights::{
            T_FireFlicker, T_Glow, T_LightFlash, T_StrobeFlash, fireflicker_t, glow_t,
            lightflash_t, strobe_t,
        },
        mobj::{MobjId, P_MobjThinker, P_RespawnSpecials, mobj_t},
        plats::{T_PlatRaise, plat_t},
        spec::P_UpdateSpecials,
        user::P_PlayerThink,
    },
    world::World,
};

/// What a thinker does each tic: the typed replacement for the C
/// `thinker_t.function` pointer.
///
/// Each active variant names the think function [`P_RunThinkers`] calls
/// (`Mobj` → [`P_MobjThinker`], `Ceiling` → [`T_MoveCeiling`], …).
///
/// `repr(u8)` with `None = 0` so that a zeroed thinker holds a valid `None`,
/// the equivalent of C's null function pointer.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThinkerKind {
    /// No think function: a node whose kind has not been set yet. Skipped by
    /// [`P_RunThinkers`].
    None = 0,
    /// A map object; runs [`P_MobjThinker`].
    Mobj,
    /// A moving ceiling; runs [`T_MoveCeiling`].
    Ceiling,
    /// A crushing ceiling stopped by `EV_CeilingCrushStop`: C nulls its
    /// function, so it stays listed but does not think until
    /// `P_ActivateInStasisCeiling` sets it back to [`ThinkerKind::Ceiling`].
    CeilingInStasis,
    /// A vertical door; runs [`T_VerticalDoor`].
    Door,
    /// A moving floor; runs [`T_MoveFloor`].
    Floor,
    /// A platform; runs [`T_PlatRaise`].
    Plat,
    /// A platform stopped by `EV_StopPlat`: C nulls its function, so it stays
    /// listed but does not think until `P_ActivateInStasis` sets it back to
    /// [`ThinkerKind::Plat`].
    PlatInStasis,
    /// A random light flash; runs [`T_LightFlash`].
    LightFlash,
    /// A strobing light; runs [`T_StrobeFlash`].
    Strobe,
    /// A glowing light; runs [`T_Glow`].
    Glow,
    /// A fire flicker; runs [`T_FireFlicker`].
    FireFlicker,
    /// Marked by [`P_RemoveThinker`] (C's `(actionf_v)(-1)` sentinel);
    /// [`P_RunThinkers`] unlists and frees the object when it reaches it.
    Removed,
}

impl ThinkerKind {
    /// `true` once [`P_RemoveThinker`] has marked the node for freeing.
    pub fn is_removed(self) -> bool {
        self == ThinkerKind::Removed
    }
}

/// The thinker state embedded in every thinker-based game object.
///
/// C's `thinker_t` also holds the list links; here the list is
/// [`Thinkers::list`], so only the think function remains.
#[derive(Clone, Copy)]
pub struct thinker_t {
    /// What this thinker does each tic, or [`ThinkerKind::Removed`] once
    /// [`P_RemoveThinker`] has marked it.
    pub kind: ThinkerKind,
}

/// A moving ceiling (`ceiling_t`) in its arena.
pub type CeilingId = Handle<ceiling_t>;
/// A vertical door (`vldoor_t`) in its arena.
pub type DoorId = Handle<vldoor_t>;
/// A moving floor (`floormove_t`) in its arena.
pub type FloorId = Handle<floormove_t>;
/// A platform (`plat_t`) in its arena.
pub type PlatId = Handle<plat_t>;
/// A light flash (`lightflash_t`) in its arena.
pub type LightFlashId = Handle<lightflash_t>;
/// A strobing light (`strobe_t`) in its arena.
pub type StrobeId = Handle<strobe_t>;
/// A glowing light (`glow_t`) in its arena.
pub type GlowId = Handle<glow_t>;
/// A fire flicker (`fireflicker_t`) in its arena.
pub type FireFlickerId = Handle<fireflicker_t>;

/// A thinker type whose all-zero value is valid, as the zeroed block C's
/// `Z_Malloc` hands out is a valid (inert) thinker.
///
/// # Safety
///
/// All-zero bytes must be a valid value of the type: no references, no
/// non-optional handles, no enums without a zero variant.
pub unsafe trait ZeroValid: Sized {
    /// The all-zero value.
    fn zeroed() -> Self {
        // SAFETY: guaranteed by the implementer.
        unsafe { std::mem::zeroed() }
    }
}

/// A thinker type and where its arena lives in the [`World`].
pub trait ThinkerType: Pooled + ZeroValid {
    /// The arena holding every object of this type.
    fn arena_mut(world: &mut World) -> &mut Arena<Self>;
}

/// Put a zeroed `T` in its arena, as C's `Z_Malloc` of a thinker does; the
/// caller fills it in and lists it with [`P_AddThinker`].
pub fn new_thinker<T: ThinkerType>(world: &mut World) -> Handle<T> {
    T::arena_mut(world).insert(T::zeroed())
}

/// Declare where the arena of each thinker type lives in the [`World`],
/// and its [`Pooled`] impl.
macro_rules! thinker_arenas {
    ($($kind:ty => $($field:ident).+),+ $(,)?) => {
        $(
            impl Pooled for $kind {
                const NAME: &'static str = stringify!($kind);
            }

            impl ThinkerType for $kind {
                fn arena_mut(world: &mut World) -> &mut Arena<Self> {
                    &mut world.$($field).+
                }
            }

            // SAFETY: the thinker structs hold plain numbers, typed level
            // and table indices, optional handles and `ThinkerKind` (whose
            // zero is `None`), all valid when zero.
            unsafe impl ZeroValid for $kind {}
        )+

        /// Free every object of every thinker type: the level is over.
        fn clear_thinker_arenas(world: &mut World) {
            $(world.$($field).+.clear();)+
        }
    };
}

thinker_arenas!(
    mobj_t => mobjs,
    ceiling_t => thinkers.ceilings,
    vldoor_t => thinkers.doors,
    floormove_t => thinkers.floors,
    plat_t => thinkers.plats,
    lightflash_t => thinkers.light_flashes,
    strobe_t => thinkers.strobes,
    glow_t => thinkers.glows,
    fireflicker_t => thinkers.fire_flickers,
);

/// A thinker in the thinker list: its type and its handle in that type's
/// arena. Replaces C's `thinker_t *`, whose type was told by its function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThinkerId {
    /// A map object.
    Mobj(MobjId),
    /// A moving ceiling.
    Ceiling(CeilingId),
    /// A vertical door.
    Door(DoorId),
    /// A moving floor.
    Floor(FloorId),
    /// A platform.
    Plat(PlatId),
    /// A light flash.
    LightFlash(LightFlashId),
    /// A strobing light.
    Strobe(StrobeId),
    /// A glowing light.
    Glow(GlowId),
    /// A fire flicker.
    FireFlicker(FireFlickerId),
}

impl ThinkerId {
    /// The thinker's current kind.
    #[track_caller]
    pub fn kind(self, world: &World) -> ThinkerKind {
        let thinkers = &world.thinkers;
        match self {
            ThinkerId::Mobj(id) => world.mobjs[id].thinker.kind,
            ThinkerId::Ceiling(id) => thinkers.ceilings[id].thinker.kind,
            ThinkerId::Door(id) => thinkers.doors[id].thinker.kind,
            ThinkerId::Floor(id) => thinkers.floors[id].thinker.kind,
            ThinkerId::Plat(id) => thinkers.plats[id].thinker.kind,
            ThinkerId::LightFlash(id) => thinkers.light_flashes[id].thinker.kind,
            ThinkerId::Strobe(id) => thinkers.strobes[id].thinker.kind,
            ThinkerId::Glow(id) => thinkers.glows[id].thinker.kind,
            ThinkerId::FireFlicker(id) => thinkers.fire_flickers[id].thinker.kind,
        }
    }

    /// Free the object in its arena; its handles go stale.
    fn free(self, world: &mut World) {
        let thinkers = &mut world.thinkers;
        match self {
            ThinkerId::Mobj(id) => world.mobjs.free(id),
            ThinkerId::Ceiling(id) => thinkers.ceilings.free(id),
            ThinkerId::Door(id) => thinkers.doors.free(id),
            ThinkerId::Floor(id) => thinkers.floors.free(id),
            ThinkerId::Plat(id) => thinkers.plats.free(id),
            ThinkerId::LightFlash(id) => thinkers.light_flashes.free(id),
            ThinkerId::Strobe(id) => thinkers.strobes.free(id),
            ThinkerId::Glow(id) => thinkers.glows.free(id),
            ThinkerId::FireFlicker(id) => thinkers.fire_flickers.free(id),
        }
    }
}

/// The thinker list and the arenas of the thinkers other than map objects
/// (`p_tick.c`'s `thinkercap` and the zone blocks of the specials; the map
/// objects are [`World::mobjs`], the level clock is [`World::leveltime`]).
pub struct Thinkers {
    /// Every thinker in run order: C's circular list from `thinkercap.next`
    /// to `thinkercap.prev`. [`P_AddThinker`] appends; [`P_RunThinkers`]
    /// drops removed thinkers when it reaches them.
    pub list: Vec<ThinkerId>,
    /// Every moving ceiling of the level.
    pub ceilings: Arena<ceiling_t>,
    /// Every vertical door of the level.
    pub doors: Arena<vldoor_t>,
    /// Every moving floor of the level.
    pub floors: Arena<floormove_t>,
    /// Every platform of the level.
    pub plats: Arena<plat_t>,
    /// Every light flash of the level.
    pub light_flashes: Arena<lightflash_t>,
    /// Every strobing light of the level.
    pub strobes: Arena<strobe_t>,
    /// Every glowing light of the level.
    pub glows: Arena<glow_t>,
    /// Every fire flicker of the level.
    pub fire_flickers: Arena<fireflicker_t>,
}

impl Thinkers {
    /// The state at program start: no thinkers.
    pub const fn new() -> Self {
        Self {
            list: Vec::new(),
            ceilings: Arena::new(),
            doors: Arena::new(),
            floors: Arena::new(),
            plats: Arena::new(),
            light_flashes: Arena::new(),
            strobes: Arena::new(),
            glows: Arena::new(),
            fire_flickers: Arena::new(),
        }
    }
}

impl Default for Thinkers {
    fn default() -> Self {
        Self::new()
    }
}

/// The thinker at `position` in the list, if the list is that long.
///
/// Walks of the list that act on the thinkers (spawning or removing others)
/// read one entry at a time with it, holding no borrow of the world between
/// steps; thinkers appended during such a walk are reached too, as with C's
/// walks from `thinkercap.next`.
pub fn thinker_at(world: &World, position: usize) -> Option<ThinkerId> {
    world.thinkers.list.get(position).copied()
}

/// Empty the thinker list and free every thinker of the level.
///
/// Must be called at the start of each level before any thinker is added.
/// Corresponds to `P_InitThinkers` in `p_tick.c`; C's thinkers are freed with
/// the level's zone memory just before it.
pub fn P_InitThinkers(world: &mut World) {
    world.thinkers.list.clear();
    clear_thinker_arenas(world);
}

/// Append `thinker` to the end of the thinker list.
///
/// Corresponds to `P_AddThinker` in `p_tick.c`. The list owns the object
/// from now on: it is freed in [`P_RunThinkers`] once it is marked
/// [`ThinkerKind::Removed`].
pub fn P_AddThinker(world: &mut World, thinker: ThinkerId) {
    world.thinkers.list.push(thinker);
}

/// Mark `thinker` for deferred removal by setting its kind to
/// [`ThinkerKind::Removed`].
///
/// The thinker is not immediately unlisted or freed; [`P_RunThinkers`]
/// performs the actual removal and deallocation when it reaches it.
/// This matches the "lazy deallocation" comment in `p_tick.c`.
///
/// # Panics
///
/// If the thinker's kind was never set.
pub fn P_RemoveThinker(thinker: &mut thinker_t) {
    assert!(
        thinker.kind != ThinkerKind::None,
        "P_RemoveThinker: thinker has no kind"
    );
    thinker.kind = ThinkerKind::Removed;
}

/// Run the think function of one listed thinker, by kind.
fn run_thinker(world: &mut World, thinker: ThinkerId) {
    match (thinker, thinker.kind(world)) {
        (ThinkerId::Mobj(id), ThinkerKind::Mobj) => P_MobjThinker(world, id),
        (ThinkerId::Ceiling(id), ThinkerKind::Ceiling) => T_MoveCeiling(world, id),
        (ThinkerId::Door(id), ThinkerKind::Door) => T_VerticalDoor(world, id),
        (ThinkerId::Floor(id), ThinkerKind::Floor) => T_MoveFloor(world, id),
        (ThinkerId::Plat(id), ThinkerKind::Plat) => T_PlatRaise(world, id),
        (ThinkerId::LightFlash(id), ThinkerKind::LightFlash) => T_LightFlash(world, id),
        (ThinkerId::Strobe(id), ThinkerKind::Strobe) => T_StrobeFlash(world, id),
        (ThinkerId::Glow(id), ThinkerKind::Glow) => T_Glow(world, id),
        (ThinkerId::FireFlicker(id), ThinkerKind::FireFlicker) => T_FireFlicker(world, id),
        // C's null function (stasis, not yet set) and removed thinkers
        // do not think.
        _ => {}
    }
}

/// Walk the thinker list once: free each removed thinker when the walk
/// reaches it, and hand every other one to `think`.
///
/// Thinkers appended while the walk runs are reached in the same walk, and a
/// thinker removed ahead of the cursor is freed when the cursor gets there,
/// as with C's linked list.
fn run_thinker_list(world: &mut World, mut think: impl FnMut(&mut World, ThinkerId)) {
    let mut position = 0;
    // `thinker_at` copies the entry out, so no borrow of the list is held
    // while the thinker runs.
    while let Some(thinker) = thinker_at(world, position) {
        if thinker.kind(world).is_removed() {
            world.thinkers.list.remove(position);
            thinker.free(world);
        } else {
            think(world, thinker);
            position += 1;
        }
    }
}

/// Iterate the thinker list, execute each thinker's think function, and free
/// any thinkers that have been marked for removal.
///
/// For each thinker in the list:
/// - If it is [`ThinkerKind::Removed`], it is unlisted and freed in its
///   arena; stale handles to it still read its data (see
///   [`crate::engine::arena`]).
/// - Otherwise, the think function its kind names is called; kinds without
///   one (C's null function: stasis) are skipped.
///
/// Corresponds to `P_RunThinkers` in `p_tick.c`.
pub fn P_RunThinkers(world: &mut World) {
    run_thinker_list(world, run_thinker);
}

/// Run one game tic: advance all players, thinkers, specials, and the level
/// timer.
///
/// Returns early if the game is paused, or if in single-player mode with the
/// menu open and the view already initialised (i.e. at least one tic has run).
/// The `viewz == 1` sentinel is the initial value set before any tic; once the
/// player has been processed once the check is false and menus pause the game.
///
/// Corresponds to `P_Ticker` in `p_tick.c`.
pub fn P_Ticker(world: &mut World) {
    unsafe {
        if paused() != 0 {
            return;
        }

        if world.session.netgame == 0
            && menu_active()
            && demoplayback() == 0
            && world.players[world.session.console()].viewz != 1
        {
            return;
        }

        for player in PlayerId::all() {
            if world.session.in_game(player.index()) {
                P_PlayerThink(world, player);
            }
        }

        P_RunThinkers(world);
        P_UpdateSpecials(world);
        P_RespawnSpecials(world);

        world.leveltime += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globals_default() {
        assert!(Thinkers::new().list.is_empty());
        assert_eq!(World::new().leveltime, 0);
    }

    #[test]
    fn zeroed_kind_is_none() {
        // A zeroed thinker read before its kind is set must be the C null
        // function, not some other kind.
        let zeroed: ThinkerKind = unsafe { std::mem::zeroed() };
        assert_eq!(zeroed, ThinkerKind::None);
    }

    #[test]
    fn removal_marks_the_thinker() {
        let mut thinker = thinker_t {
            kind: ThinkerKind::CeilingInStasis,
        };
        P_RemoveThinker(&mut thinker);
        assert_eq!(thinker.kind, ThinkerKind::Removed);
        assert!(thinker.kind.is_removed());
        P_RemoveThinker(&mut thinker);
        assert_eq!(thinker.kind, ThinkerKind::Removed);
    }

    /// A listed glow with the given light level, for telling thinkers apart.
    fn add_glow(world: &mut World, level: i32) -> GlowId {
        let id = new_thinker::<glow_t>(world);
        let glow = &mut world.thinkers.glows[id];
        glow.minlight = level;
        glow.thinker.kind = ThinkerKind::Glow;
        P_AddThinker(world, ThinkerId::Glow(id));
        id
    }

    fn glow_level(world: &World, thinker: ThinkerId) -> i32 {
        let ThinkerId::Glow(id) = thinker else {
            panic!("{thinker:?} is not a glow");
        };
        world.thinkers.glows[id].minlight
    }

    fn levels(world: &World) -> Vec<i32> {
        world
            .thinkers
            .list
            .iter()
            .map(|&thinker| glow_level(world, thinker))
            .collect()
    }

    /// Thinkers run in insertion order; one removed ahead of the cursor is
    /// skipped and freed when the walk reaches it, one added during the walk
    /// runs in the same walk, at the tail.
    #[test]
    fn walk_keeps_order_across_add_and_remove() {
        let mut world = World::new();
        let first = add_glow(&mut world, 1);
        let second = add_glow(&mut world, 2);
        let third = add_glow(&mut world, 3);

        let mut ran = Vec::new();
        run_thinker_list(&mut world, |world, thinker| {
            let level = glow_level(world, thinker);
            ran.push(level);
            if level == 1 {
                // Remove the next thinker before the walk reaches it.
                P_RemoveThinker(&mut world.thinkers.glows[second].thinker);
                add_glow(world, 4);
            }
        });
        assert_eq!(ran, [1, 3, 4]);
        let glows = &world.thinkers.glows;
        assert!(glows.is_live(first) && glows.is_live(third));
        assert!(!glows.is_live(second), "freed when the walk reached it");
        assert_eq!(levels(&world), [1, 3, 4]);
    }

    /// A removed thinker stays listed and readable until the walk frees it;
    /// after that its handle is stale but still reads the old data.
    #[test]
    fn deferred_removal_keeps_the_slot_readable() {
        let mut world = World::new();
        let glow = add_glow(&mut world, 5);
        P_RemoveThinker(&mut world.thinkers.glows[glow].thinker);
        assert!(world.thinkers.glows.is_live(glow), "removal is deferred");
        assert_eq!(world.thinkers.list.len(), 1);

        run_thinker_list(&mut world, |_world, thinker| {
            panic!("{thinker:?} is removed and must not run")
        });
        assert!(!world.thinkers.glows.is_live(glow));
        assert!(world.thinkers.list.is_empty());
        assert_eq!(world.thinkers.glows[glow].minlight, 5);
    }

    /// The kind of each listed thinker is read from its own arena, map
    /// objects included, and a removed map object is freed in
    /// [`World::mobjs`].
    #[test]
    fn kind_reads_the_thinkers_own_arena() {
        let mut world = World::new();
        let mobj = new_thinker::<mobj_t>(&mut world);
        world.mobjs[mobj].thinker.kind = ThinkerKind::Mobj;
        P_AddThinker(&mut world, ThinkerId::Mobj(mobj));
        let ceiling = new_thinker::<ceiling_t>(&mut world);
        world.thinkers.ceilings[ceiling].thinker.kind = ThinkerKind::CeilingInStasis;
        P_AddThinker(&mut world, ThinkerId::Ceiling(ceiling));
        assert_eq!(ThinkerId::Mobj(mobj).kind(&world), ThinkerKind::Mobj);
        assert_eq!(
            ThinkerId::Ceiling(ceiling).kind(&world),
            ThinkerKind::CeilingInStasis
        );

        P_RemoveThinker(&mut world.mobjs[mobj].thinker);
        let mut ran = Vec::new();
        run_thinker_list(&mut world, |_world, thinker| ran.push(thinker));
        assert_eq!(ran, [ThinkerId::Ceiling(ceiling)]);
        assert!(!world.mobjs.is_live(mobj));
        assert!(world.thinkers.ceilings.is_live(ceiling));
    }

    #[test]
    fn init_empties_the_list_and_frees_the_level() {
        let mut world = World::new();
        let glow = add_glow(&mut world, 6);
        let mobj = new_thinker::<mobj_t>(&mut world);
        P_InitThinkers(&mut world);
        assert!(world.thinkers.list.is_empty());
        assert!(!world.thinkers.glows.is_live(glow));
        assert!(!world.mobjs.is_live(mobj));
    }
}
