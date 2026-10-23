//! Thinker management and the main per-tic game loop.
//!
//! Rust port of `vendor/doomgeneric/p_tick.c`.  Every game object that needs
//! to update once per tic (monsters, moving platforms, ceilings, …) embeds a
//! [`thinker_t`] and is linked into the global doubly-linked list headed by
//! [`thinkercap`].  [`P_RunThinkers`] walks that list every tic and drives
//! each object's logic according to the node's [`ThinkerKind`].

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::c_void;

use crate::engine::{
    doom::player::{consoleplayer, players, MAXPLAYERS},
    game::{demoplayback, netgame, paused, playeringame},
    misc::menu::menuactive,
    play::{
        ceilng::{ceiling_t, T_MoveCeiling},
        doors::{vldoor_t, T_VerticalDoor},
        floor::{floormove_t, T_MoveFloor},
        lights::{
            fireflicker_t, glow_t, lightflash_t, strobe_t, T_FireFlicker, T_Glow, T_LightFlash,
            T_StrobeFlash,
        },
        mobj::{mobj_t, P_MobjThinker, P_RespawnSpecials},
        plats::{plat_t, T_PlatRaise},
        spec::P_UpdateSpecials,
        user::P_PlayerThink,
    },
    zone::Z_Free,
};

/// Which struct embeds a [`thinker_t`], i.e. which `Z_Malloc` allocation the
/// node lives in.
///
/// Carried by [`ThinkerKind::Removed`] so that [`P_RunThinkers`] can find the
/// start of the allocation to free after the node's think function is gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThinkerClass {
    /// [`mobj_t`].
    Mobj,
    /// [`ceiling_t`].
    Ceiling,
    /// [`vldoor_t`].
    Door,
    /// [`floormove_t`].
    Floor,
    /// [`plat_t`].
    Plat,
    /// [`lightflash_t`].
    LightFlash,
    /// [`strobe_t`].
    Strobe,
    /// [`glow_t`].
    Glow,
    /// [`fireflicker_t`].
    FireFlicker,
}

/// What a thinker does each tic: the typed replacement for the C
/// `thinker_t.function` pointer.
///
/// Each active variant names the think function [`P_RunThinkers`] calls
/// (`Mobj` → [`P_MobjThinker`], `Ceiling` → [`T_MoveCeiling`], …).
///
/// `repr(u8)` with `None = 0` so that a freshly zeroed `Z_Malloc` block holds
/// a valid `None`, the equivalent of C's null function pointer.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThinkerKind {
    /// No think function: the [`thinkercap`] head, or a node whose kind has
    /// not been set yet. Skipped by [`P_RunThinkers`].
    None = 0,
    /// A map object; runs [`P_MobjThinker`].
    Mobj,
    /// A moving ceiling; runs [`T_MoveCeiling`].
    Ceiling,
    /// A crushing ceiling stopped by `EV_CeilingCrushStop`: C nulls its
    /// function, so it stays linked but does not think until
    /// `P_ActivateInStasisCeiling` sets it back to [`ThinkerKind::Ceiling`].
    CeilingInStasis,
    /// A vertical door; runs [`T_VerticalDoor`].
    Door,
    /// A moving floor; runs [`T_MoveFloor`].
    Floor,
    /// A platform; runs [`T_PlatRaise`].
    Plat,
    /// A platform stopped by `EV_StopPlat`: C nulls its function, so it stays
    /// linked but does not think until `P_ActivateInStasis` sets it back to
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
    /// [`P_RunThinkers`] unlinks and frees the node when it reaches it.
    Removed(ThinkerClass),
}

impl ThinkerKind {
    /// The struct that embeds a thinker of this kind, or `None` for
    /// [`ThinkerKind::None`], which has no owning struct.
    pub fn class(self) -> Option<ThinkerClass> {
        match self {
            ThinkerKind::None => None,
            ThinkerKind::Mobj => Some(ThinkerClass::Mobj),
            ThinkerKind::Ceiling | ThinkerKind::CeilingInStasis => Some(ThinkerClass::Ceiling),
            ThinkerKind::Door => Some(ThinkerClass::Door),
            ThinkerKind::Floor => Some(ThinkerClass::Floor),
            ThinkerKind::Plat | ThinkerKind::PlatInStasis => Some(ThinkerClass::Plat),
            ThinkerKind::LightFlash => Some(ThinkerClass::LightFlash),
            ThinkerKind::Strobe => Some(ThinkerClass::Strobe),
            ThinkerKind::Glow => Some(ThinkerClass::Glow),
            ThinkerKind::FireFlicker => Some(ThinkerClass::FireFlicker),
            ThinkerKind::Removed(class) => Some(class),
        }
    }

    /// `true` once [`P_RemoveThinker`] has marked the node for freeing.
    pub fn is_removed(self) -> bool {
        matches!(self, ThinkerKind::Removed(_))
    }
}

/// Linked-list node embedded in every thinker-based game object.
///
/// All thinkers must be allocated via `Z_Malloc` so that [`P_RunThinkers`] can
/// call `Z_Free` on removed nodes.  The owning struct (e.g. `ceiling_t`,
/// `plat_t`, `mobj_t`) implements [`ThinkerContainer`], which maps a
/// `*mut thinker_t` back to the struct that embeds it.
#[derive(Clone, Copy)]
pub struct thinker_t {
    /// Pointer to the previous node in the circular thinker list.
    pub prev: *mut thinker_t,
    /// Pointer to the next node in the circular thinker list.
    pub next: *mut thinker_t,
    /// What this thinker does each tic, or [`ThinkerKind::Removed`] once
    /// [`P_RemoveThinker`] has marked it.
    pub kind: ThinkerKind,
}

/// A struct that embeds a [`thinker_t`] in its `thinker` field.
///
/// # Safety
///
/// `THINKER_OFFSET` must be `offset_of!(Self, thinker)`.
pub unsafe trait ThinkerContainer: Sized {
    /// Byte offset of the embedded `thinker` field.
    const THINKER_OFFSET: usize;

    /// The struct whose `thinker` field `thinker` points at.
    ///
    /// # Safety
    ///
    /// `thinker` must point at the `thinker` field of a live `Self`.
    unsafe fn from_thinker(thinker: *mut thinker_t) -> *mut Self {
        unsafe { thinker.byte_sub(Self::THINKER_OFFSET).cast::<Self>() }
    }
}

/// Implement [`ThinkerContainer`] for structs with a `thinker: thinker_t` field.
macro_rules! thinker_container {
    ($($container:ty),+ $(,)?) => {
        $(
            // SAFETY: the offset is computed by the compiler for this type.
            unsafe impl ThinkerContainer for $container {
                const THINKER_OFFSET: usize = std::mem::offset_of!($container, thinker);
            }
        )+
    };
}

thinker_container!(
    mobj_t,
    ceiling_t,
    vldoor_t,
    floormove_t,
    plat_t,
    lightflash_t,
    strobe_t,
    glow_t,
    fireflicker_t,
);

/// Start of the `Z_Malloc` block holding `thinker`, which is embedded in a
/// struct of type `class`.
///
/// # Safety
///
/// `thinker` must be the `thinker` field of a live struct of type `class`.
pub unsafe fn thinker_allocation(thinker: *mut thinker_t, class: ThinkerClass) -> *mut c_void {
    unsafe {
        match class {
            ThinkerClass::Mobj => mobj_t::from_thinker(thinker).cast(),
            ThinkerClass::Ceiling => ceiling_t::from_thinker(thinker).cast(),
            ThinkerClass::Door => vldoor_t::from_thinker(thinker).cast(),
            ThinkerClass::Floor => floormove_t::from_thinker(thinker).cast(),
            ThinkerClass::Plat => plat_t::from_thinker(thinker).cast(),
            ThinkerClass::LightFlash => lightflash_t::from_thinker(thinker).cast(),
            ThinkerClass::Strobe => strobe_t::from_thinker(thinker).cast(),
            ThinkerClass::Glow => glow_t::from_thinker(thinker).cast(),
            ThinkerClass::FireFlicker => fireflicker_t::from_thinker(thinker).cast(),
        }
    }
}

/// Current level time in tics, incremented once per [`P_Ticker`] call.
///
/// Used throughout the engine to
/// throttle periodic events (e.g. crushing-ceiling sound plays every 8 tics).
pub static mut leveltime: i32 = 0;

/// Sentinel head/tail node of the doubly-linked thinker list.
///
/// The list is circular: `thinkercap.next` is the first real thinker and
/// `thinkercap.prev` is the last.  When the list is empty both fields point
/// back to `thinkercap` itself (see [`P_InitThinkers`]).
pub static mut thinkercap: thinker_t = thinker_t {
    prev: std::ptr::null_mut::<thinker_t>(),
    next: std::ptr::null_mut::<thinker_t>(),
    kind: ThinkerKind::None,
};

/// Reset the thinker list to empty by making [`thinkercap`] point to itself.
///
/// Must be called at the start of each level before any thinker is added.
/// Corresponds to `P_InitThinkers` in `p_tick.c`.
pub fn P_InitThinkers() {
    unsafe {
        thinkercap.prev = &raw mut thinkercap;
        thinkercap.next = &raw mut thinkercap;
    }
}

/// Append `thinker` to the end of the global thinker list.
///
/// The new node is inserted before [`thinkercap`], making it the logical
/// tail of the list.  Corresponds to `P_AddThinker` in `p_tick.c`.
///
/// # Safety
///
/// `thinker` must be a valid, non-null pointer to the `thinker` field of a
/// [`ThinkerContainer`] that was allocated via `Z_Malloc` and will not be
/// freed by the caller — ownership transfers to the thinker list, which frees
/// it in [`P_RunThinkers`] once it is marked [`ThinkerKind::Removed`].
pub fn P_AddThinker(thinker: *mut thinker_t) {
    unsafe {
        let cap = &raw mut thinkercap;
        (*(*cap).prev).next = thinker;
        (*thinker).next = cap;
        (*thinker).prev = (*cap).prev;
        (*cap).prev = thinker;
    }
}

/// Mark `thinker` for deferred removal by setting its kind to
/// [`ThinkerKind::Removed`], keeping the class of its owning struct.
///
/// The thinker is not immediately unlinked or freed; [`P_RunThinkers`]
/// performs the actual removal and deallocation on the next iteration.
/// This matches the "lazy deallocation" comment in `p_tick.c`.
///
/// # Safety
///
/// `thinker` must be a valid, non-null pointer to a thinker that is currently
/// linked into the global thinker list and whose kind has been set.
pub fn P_RemoveThinker(thinker: *mut thinker_t) {
    unsafe {
        let Some(class) = (*thinker).kind.class() else {
            panic!("P_RemoveThinker: thinker has no kind");
        };
        (*thinker).kind = ThinkerKind::Removed(class);
    }
}

/// No-op stub matching the C `P_AllocateThinker` signature.
///
/// The original C function body is empty; this Rust port preserves that
/// to keep the symbol available for any C caller that references it.
pub fn P_AllocateThinker(_thinker: *mut thinker_t) {}

/// Run the think function of one linked thinker, by kind.
///
/// # Safety
///
/// `thinker` must be the `thinker` field of a live struct of the class its
/// kind names.
unsafe fn run_thinker(thinker: *mut thinker_t) {
    unsafe {
        match (*thinker).kind {
            ThinkerKind::Mobj => P_MobjThinker(mobj_t::from_thinker(thinker)),
            ThinkerKind::Ceiling => T_MoveCeiling(ceiling_t::from_thinker(thinker)),
            ThinkerKind::Door => T_VerticalDoor(vldoor_t::from_thinker(thinker)),
            ThinkerKind::Floor => T_MoveFloor(floormove_t::from_thinker(thinker)),
            ThinkerKind::Plat => T_PlatRaise(plat_t::from_thinker(thinker)),
            ThinkerKind::LightFlash => T_LightFlash(lightflash_t::from_thinker(thinker)),
            ThinkerKind::Strobe => T_StrobeFlash(strobe_t::from_thinker(thinker)),
            ThinkerKind::Glow => T_Glow(glow_t::from_thinker(thinker)),
            ThinkerKind::FireFlicker => T_FireFlicker(fireflicker_t::from_thinker(thinker)),
            ThinkerKind::None
            | ThinkerKind::CeilingInStasis
            | ThinkerKind::PlatInStasis
            | ThinkerKind::Removed(_) => {}
        }
    }
}

/// Iterate the thinker list, execute each thinker's think function, and free
/// any thinkers that have been marked for removal.
///
/// For each node in the list:
/// - If the node is [`ThinkerKind::Removed`], it is unlinked and its owning
///   struct is freed via `Z_Free` (the allocation starts at the struct, not at
///   the embedded thinker, so the pointer is mapped back through
///   [`thinker_allocation`]).
/// - Otherwise, the think function its kind names is called; kinds without
///   one (C's null function: stasis, the list head) are skipped.
///
/// The C original advances the cursor *after* `Z_Free`, reading from already-
/// freed memory.  This Rust port saves `next` before freeing to avoid the
/// undefined behaviour.
///
/// Corresponds to `P_RunThinkers` in `p_tick.c`.
pub fn P_RunThinkers() {
    unsafe {
        let cap = &raw mut thinkercap;
        let mut current = (*cap).next;

        while current != cap {
            if let ThinkerKind::Removed(class) = (*current).kind {
                let prev = (*current).prev;
                let next = (*current).next;
                (*prev).next = next;
                (*next).prev = prev;
                Z_Free(thinker_allocation(current, class));
                current = next;
            } else {
                run_thinker(current);
                current = (*current).next;
            }
        }
    }
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
pub fn P_Ticker() {
    unsafe {
        if paused != 0 {
            return;
        }

        if netgame == 0
            && menuactive != 0
            && demoplayback == 0
            && (*std::ptr::addr_of!(players[0]).offset(consoleplayer as isize)).viewz != 1
        {
            return;
        }

        for i in 0..MAXPLAYERS {
            if playeringame[i] != 0 {
                P_PlayerThink(&mut players[i]);
            }
        }

        P_RunThinkers();
        P_UpdateSpecials();
        P_RespawnSpecials();

        leveltime += 1;
    }
}

#[cfg(test)]
mod tests {
    use {super::*, std::sync::Mutex};

    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn globals_default() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert_eq!({ leveltime }, 0);
        }
    }

    #[test]
    fn zeroed_kind_is_none() {
        // Z_Malloc zeroes new blocks; a thinker read before its kind is set
        // must be the C null function, not some other kind.
        let zeroed: ThinkerKind = unsafe { std::mem::zeroed() };
        assert_eq!(zeroed, ThinkerKind::None);
    }

    #[test]
    fn removal_keeps_the_owning_class() {
        let mut thinker = thinker_t {
            prev: std::ptr::null_mut(),
            next: std::ptr::null_mut(),
            kind: ThinkerKind::CeilingInStasis,
        };
        P_RemoveThinker(&raw mut thinker);
        assert_eq!(thinker.kind, ThinkerKind::Removed(ThinkerClass::Ceiling));
        assert!(thinker.kind.is_removed());
        P_RemoveThinker(&raw mut thinker);
        assert_eq!(thinker.kind, ThinkerKind::Removed(ThinkerClass::Ceiling));
    }

    #[test]
    fn from_thinker_returns_the_container() {
        let mut glow = std::mem::MaybeUninit::<glow_t>::zeroed();
        let container = glow.as_mut_ptr();
        let thinker = unsafe { &raw mut (*container).thinker };
        assert_eq!(unsafe { glow_t::from_thinker(thinker) }, container);
        assert_eq!(
            unsafe { thinker_allocation(thinker, ThinkerClass::Glow) },
            container.cast::<c_void>()
        );

        let mut mobj = std::mem::MaybeUninit::<mobj_t>::zeroed();
        let container = mobj.as_mut_ptr();
        let thinker = unsafe { &raw mut (*container).thinker };
        assert_eq!(
            unsafe { thinker_allocation(thinker, ThinkerClass::Mobj) },
            container.cast::<c_void>()
        );
    }

    #[test]
    fn init_sets_self_pointers() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            thinkercap.prev = std::ptr::null_mut();
            thinkercap.next = std::ptr::null_mut();
            P_InitThinkers();
            assert_eq!({ thinkercap.prev }, &raw mut thinkercap);
            assert_eq!({ thinkercap.next }, &raw mut thinkercap);
        }
    }
}
