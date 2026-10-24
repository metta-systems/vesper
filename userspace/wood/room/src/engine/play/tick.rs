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
    global::Global,
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
/// [`TickState::thinkers`], so only the think function remains.
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

/// Put a zeroed `T` in its arena, as C's `Z_Malloc` of a thinker does; the
/// caller fills it in and lists it with [`P_AddThinker`].
pub fn new_thinker<T: Pooled + ZeroValid>() -> Handle<T> {
    T::arena().borrow_mut().insert(T::zeroed())
}

/// Declare the arena of each thinker type and its [`Pooled`] impl.
macro_rules! thinker_arenas {
    ($($arena:ident: $kind:ty),+ $(,)?) => {
        $(
            #[doc = concat!("Every `", stringify!($kind), "` of the current level.")]
            pub static $arena: Global<Arena<$kind>> = Global::new(Arena::new());

            impl Pooled for $kind {
                const NAME: &'static str = stringify!($kind);

                fn arena() -> &'static Global<Arena<Self>> {
                    &$arena
                }
            }

            // SAFETY: the thinker structs hold plain numbers, typed level
            // and table indices, optional handles and `ThinkerKind` (whose
            // zero is `None`), all valid when zero.
            unsafe impl ZeroValid for $kind {}
        )+

        /// Free every object of every thinker type: the level is over.
        fn clear_thinker_arenas() {
            $($arena.borrow_mut().clear();)+
        }
    };
}

thinker_arenas!(
    MOBJ_ARENA: mobj_t,
    CEILING_ARENA: ceiling_t,
    DOOR_ARENA: vldoor_t,
    FLOOR_ARENA: floormove_t,
    PLAT_ARENA: plat_t,
    LIGHT_FLASH_ARENA: lightflash_t,
    STROBE_ARENA: strobe_t,
    GLOW_ARENA: glow_t,
    FIRE_FLICKER_ARENA: fireflicker_t,
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
    /// Pointer to the object's embedded thinker state.
    #[track_caller]
    pub fn thinker(self) -> *mut thinker_t {
        // SAFETY (all arms): field projection on an arena pointer, which is
        // valid for the rest of the program; no reference is made.
        unsafe {
            match self {
                ThinkerId::Mobj(id) => &raw mut (*id.ptr()).thinker,
                ThinkerId::Ceiling(id) => &raw mut (*id.ptr()).thinker,
                ThinkerId::Door(id) => &raw mut (*id.ptr()).thinker,
                ThinkerId::Floor(id) => &raw mut (*id.ptr()).thinker,
                ThinkerId::Plat(id) => &raw mut (*id.ptr()).thinker,
                ThinkerId::LightFlash(id) => &raw mut (*id.ptr()).thinker,
                ThinkerId::Strobe(id) => &raw mut (*id.ptr()).thinker,
                ThinkerId::Glow(id) => &raw mut (*id.ptr()).thinker,
                ThinkerId::FireFlicker(id) => &raw mut (*id.ptr()).thinker,
            }
        }
    }

    /// The thinker's current kind.
    #[track_caller]
    pub fn kind(self) -> ThinkerKind {
        // SAFETY: a read through a valid arena pointer; no reference to the
        // thinker state outlives this read.
        unsafe { (*self.thinker()).kind }
    }

    /// Free the object in its arena; its handles go stale.
    fn free(self) {
        match self {
            ThinkerId::Mobj(id) => MOBJ_ARENA.borrow_mut().free(id),
            ThinkerId::Ceiling(id) => CEILING_ARENA.borrow_mut().free(id),
            ThinkerId::Door(id) => DOOR_ARENA.borrow_mut().free(id),
            ThinkerId::Floor(id) => FLOOR_ARENA.borrow_mut().free(id),
            ThinkerId::Plat(id) => PLAT_ARENA.borrow_mut().free(id),
            ThinkerId::LightFlash(id) => LIGHT_FLASH_ARENA.borrow_mut().free(id),
            ThinkerId::Strobe(id) => STROBE_ARENA.borrow_mut().free(id),
            ThinkerId::Glow(id) => GLOW_ARENA.borrow_mut().free(id),
            ThinkerId::FireFlicker(id) => FIRE_FLICKER_ARENA.borrow_mut().free(id),
        }
    }
}

/// The thinker list (`p_tick.c` global; the level clock `leveltime` is in
/// [`World`]).
pub struct TickState {
    /// Every thinker in run order: C's circular list from `thinkercap.next`
    /// to `thinkercap.prev`. [`P_AddThinker`] appends; [`P_RunThinkers`]
    /// drops removed thinkers when it reaches them.
    thinkers: Vec<ThinkerId>,
}

impl TickState {
    /// The state at program start: no thinkers.
    pub const fn new() -> Self {
        Self {
            thinkers: Vec::new(),
        }
    }
}

impl Default for TickState {
    fn default() -> Self {
        Self::new()
    }
}

/// The thinker list.
pub static TICK: Global<TickState> = Global::new(TickState::new());

/// The thinker at `position` in the list, if the list is that long.
///
/// Walks of the list read one entry at a time, so the thinker can be acted
/// on (spawning or removing others) without a borrow of the list.
pub fn thinker_at(position: usize) -> Option<ThinkerId> {
    TICK.borrow().thinkers.get(position).copied()
}

/// Iterate the thinker list in run order, as C's walks from
/// `thinkercap.next` do.
///
/// Each step reads the list afresh, so thinkers appended during the walk
/// are reached too.
pub fn thinkers() -> impl Iterator<Item = ThinkerId> {
    (0..).map_while(thinker_at)
}

/// Empty the thinker list and free every thinker of the level.
///
/// Must be called at the start of each level before any thinker is added.
/// Corresponds to `P_InitThinkers` in `p_tick.c`; C's thinkers are freed with
/// the level's zone memory just before it.
pub fn P_InitThinkers() {
    TICK.borrow_mut().thinkers.clear();
    clear_thinker_arenas();
}

/// Append `thinker` to the end of the thinker list.
///
/// Corresponds to `P_AddThinker` in `p_tick.c`. The list owns the object
/// from now on: it is freed in [`P_RunThinkers`] once it is marked
/// [`ThinkerKind::Removed`].
pub fn P_AddThinker(thinker: ThinkerId) {
    TICK.borrow_mut().thinkers.push(thinker);
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
///
/// # Safety
///
/// The thinker's think function must be sound to run in the current game
/// state (a level is loaded).
unsafe fn run_thinker(world: &mut World, thinker: ThinkerId) {
    unsafe {
        match (thinker, thinker.kind()) {
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
}

/// Walk the thinker list once: free each removed thinker when the walk
/// reaches it, and hand every other one to `think`.
///
/// Thinkers appended while the walk runs are reached in the same walk, and a
/// thinker removed ahead of the cursor is freed when the cursor gets there,
/// as with C's linked list.
fn run_thinker_list(world: &mut World, mut think: impl FnMut(&mut World, ThinkerId)) {
    let mut position = 0;
    // `thinker_at` releases its borrow of the list before the body runs.
    while let Some(thinker) = thinker_at(position) {
        if thinker.kind().is_removed() {
            TICK.borrow_mut().thinkers.remove(position);
            thinker.free();
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
    // SAFETY: the ticker runs only while a level is loaded.
    run_thinker_list(world, |world, thinker| unsafe {
        run_thinker(world, thinker)
    });
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

/// Serialises the tests that use the thinker list and arenas.
#[cfg(test)]
pub static THINKER_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globals_default() {
        assert!(TickState::new().thinkers.is_empty());
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
    fn add_glow(level: i32) -> GlowId {
        let id = new_thinker::<glow_t>();
        let glow = unsafe { &mut *id.ptr() };
        glow.minlight = level;
        glow.thinker.kind = ThinkerKind::Glow;
        P_AddThinker(ThinkerId::Glow(id));
        id
    }

    fn glow_level(thinker: ThinkerId) -> i32 {
        let ThinkerId::Glow(id) = thinker else {
            panic!("{thinker:?} is not a glow");
        };
        unsafe { (*id.ptr()).minlight }
    }

    /// Thinkers run in insertion order; one removed ahead of the cursor is
    /// skipped and freed when the walk reaches it, one added during the walk
    /// runs in the same walk, at the tail.
    #[test]
    fn walk_keeps_order_across_add_and_remove() {
        let _guard = THINKER_TEST_LOCK.lock().unwrap();
        P_InitThinkers();
        let first = add_glow(1);
        let second = add_glow(2);
        let third = add_glow(3);

        let mut world = World::new();
        let mut ran = Vec::new();
        run_thinker_list(&mut world, |_world, thinker| {
            let level = glow_level(thinker);
            ran.push(level);
            if level == 1 {
                // Remove the next thinker before the walk reaches it.
                P_RemoveThinker(unsafe { &mut (*second.ptr()).thinker });
                add_glow(4);
            }
        });
        assert_eq!(ran, [1, 3, 4]);
        assert!(first.is_live() && third.is_live());
        assert!(!second.is_live(), "freed when the walk reached it");
        let order: Vec<i32> = thinkers().map(glow_level).collect();
        assert_eq!(order, [1, 3, 4]);
        P_InitThinkers();
    }

    /// A removed thinker stays listed and readable until the walk frees it;
    /// after that its handle is stale but still reads the old data.
    #[test]
    fn deferred_removal_keeps_the_slot_readable() {
        let _guard = THINKER_TEST_LOCK.lock().unwrap();
        P_InitThinkers();
        let glow = add_glow(5);
        P_RemoveThinker(unsafe { &mut (*glow.ptr()).thinker });
        assert!(glow.is_live(), "removal is deferred");
        assert_eq!(thinkers().count(), 1);

        let mut world = World::new();
        run_thinker_list(&mut world, |_world, thinker| {
            panic!("{thinker:?} is removed and must not run")
        });
        assert!(!glow.is_live());
        assert_eq!(thinkers().count(), 0);
        assert_eq!(unsafe { (*glow.ptr()).minlight }, 5);
        P_InitThinkers();
    }

    #[test]
    fn init_empties_the_list_and_frees_the_level() {
        let _guard = THINKER_TEST_LOCK.lock().unwrap();
        let glow = add_glow(6);
        P_InitThinkers();
        assert_eq!(thinkers().count(), 0);
        assert!(!glow.is_live());
    }
}
