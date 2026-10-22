//! Thinker management and the main per-tic game loop.
//!
//! Rust port of `vendor/doomgeneric/p_tick.c`.  Every game object that needs
//! to update once per tic (monsters, moving platforms, ceilings, …) embeds a
//! [`thinker_t`] as its first field and is linked into the global doubly-linked
//! list headed by [`thinkercap`].  [`P_RunThinkers`] walks that list every tic
//! and drives each object's logic.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::c_void;
use std::os::raw::c_int;

use crate::doom::d_player::{consoleplayer, players, MAXPLAYERS};
use crate::doom::g_game::{demoplayback, netgame, paused, playeringame};
use crate::doom::m_menu::menuactive;
use crate::doom::p_mobj::P_RespawnSpecials;
use crate::doom::p_spec::P_UpdateSpecials;
use crate::doom::p_user::P_PlayerThink;
use crate::doom::z_zone::Z_Free;

/// Union of the three C function-pointer variants stored in every thinker.
///
/// `acv` — a no-argument callback (also used to hold the sentinel value `-1`).
/// `acp1` — the common single-argument callback: `fn(*mut void)`.
/// `acp2` — a two-argument callback: `fn(*mut void, *mut void)`.
///
/// Callers cast the concrete thinker pointer (e.g. `*mut ceiling_t`) to
/// `*mut c_void` when invoking `acp1`.  The active variant is always `acp1`
/// during normal thinker execution; `acv` is only written by
/// [`P_RemoveThinker`] to store the sentinel.
#[repr(C)]
#[derive(Clone, Copy)]
pub union actionf_t {
    /// No-argument action; also used to carry the removal sentinel (`-1`).
    pub acv: Option<unsafe extern "C" fn()>,
    /// Single-argument action — the common thinker callback form.
    pub acp1: Option<unsafe extern "C" fn(*mut c_void)>,
    /// Two-argument action — used by a small number of thinker types.
    pub acp2: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
}

/// Linked-list node embedded at offset 0 of every thinker-based game object.
///
/// All thinkers must be allocated via `Z_Malloc` so that [`P_RunThinkers`] can
/// call `Z_Free` on removed nodes.  The concrete struct (e.g. `ceiling_t`,
/// `plat_t`, `mobj_t`) begins with this header, enabling safe pointer casts
/// between `*mut thinker_t` and the owning type.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct thinker_t {
    /// Pointer to the previous node in the circular thinker list.
    pub prev: *mut thinker_t,
    /// Pointer to the next node in the circular thinker list.
    pub next: *mut thinker_t,
    /// The per-tic callback, or the removal sentinel when set by
    /// [`P_RemoveThinker`].
    pub function: actionf_t,
}

/// Produce the sentinel value `(actionf_v)(-1)` that C uses to mark
/// thinkers for removal.  We build it at runtime so the compiler doesn't
/// try to validate the bit pattern as a real function pointer.
fn sentinel_ac() -> Option<unsafe extern "C" fn()> {
    // -1 as pointer-sized integer, reinterpreted as a function pointer.
    Some(unsafe { core::mem::transmute::<usize, unsafe extern "C" fn()>(usize::MAX) })
}

/// Return `true` if `f` holds the removal-sentinel value (`(actionf_v)(-1)`).
fn is_sentinel(f: actionf_t) -> bool {
    unsafe { f.acv == sentinel_ac() }
}

/// Current level time in tics, incremented once per [`P_Ticker`] call.
///
/// Exported as `leveltime` for C linkage.  Used throughout the engine to
/// throttle periodic events (e.g. crushing-ceiling sound plays every 8 tics).
#[no_mangle]
pub static mut leveltime: c_int = 0;

/// Sentinel head/tail node of the doubly-linked thinker list.
///
/// The list is circular: `thinkercap.next` is the first real thinker and
/// `thinkercap.prev` is the last.  When the list is empty both fields point
/// back to `thinkercap` itself (see [`P_InitThinkers`]).
///
/// Exported as `thinkercap` for C linkage.
#[no_mangle]
pub static mut thinkercap: thinker_t = thinker_t {
    prev: std::ptr::null_mut::<thinker_t>(),
    next: std::ptr::null_mut::<thinker_t>(),
    function: actionf_t { acv: None },
};

/// Reset the thinker list to empty by making [`thinkercap`] point to itself.
///
/// Must be called at the start of each level before any thinker is added.
/// Corresponds to `P_InitThinkers` in `p_tick.c`.
#[no_mangle]
pub extern "C" fn P_InitThinkers() {
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
/// `thinker` must be a valid, non-null pointer to a `thinker_t` that was
/// allocated via `Z_Malloc` and will not be freed by the caller — ownership
/// transfers to the thinker list, which frees it in [`P_RunThinkers`] once
/// the removal sentinel is detected.
#[no_mangle]
pub extern "C" fn P_AddThinker(thinker: *mut thinker_t) {
    unsafe {
        let cap = &raw mut thinkercap;
        (*(*cap).prev).next = thinker;
        (*thinker).next = cap;
        (*thinker).prev = (*cap).prev;
        (*cap).prev = thinker;
    }
}

/// Mark `thinker` for deferred removal by setting its function to the
/// sentinel value `(actionf_v)(-1)`.
///
/// The thinker is not immediately unlinked or freed; [`P_RunThinkers`]
/// performs the actual removal and deallocation on the next iteration.
/// This matches the "lazy deallocation" comment in `p_tick.c`.
///
/// # Safety
///
/// `thinker` must be a valid, non-null pointer to a thinker that is currently
/// linked into the global thinker list.
#[no_mangle]
pub extern "C" fn P_RemoveThinker(thinker: *mut thinker_t) {
    unsafe {
        (*thinker).function.acv = sentinel_ac();
    }
}

/// No-op stub matching the C `P_AllocateThinker` signature.
///
/// The original C function body is empty; this Rust port preserves that
/// to keep the symbol available for any C caller that references it.
#[no_mangle]
pub extern "C" fn P_AllocateThinker(_thinker: *mut thinker_t) {}

/// Iterate the thinker list, execute each thinker's callback, and free any
/// thinkers that have been marked for removal.
///
/// For each node in the list:
/// - If the node's `function.acv` equals the sentinel (`-1`), the node is
///   unlinked and freed via `Z_Free`.
/// - Otherwise, `function.acp1` is called with the node pointer cast to
///   `*mut c_void`.
///
/// The C original advances the cursor *after* `Z_Free`, reading from already-
/// freed memory.  This Rust port saves `next` before freeing to avoid the
/// undefined behaviour.
///
/// Corresponds to `P_RunThinkers` in `p_tick.c`.
#[no_mangle]
pub extern "C" fn P_RunThinkers() {
    unsafe {
        let cap = &raw mut thinkercap;
        let mut current = (*cap).next;

        while current != cap {
            if is_sentinel((*current).function) {
                let prev = (*current).prev;
                let next = (*current).next;
                (*prev).next = next;
                (*next).prev = prev;
                Z_Free(current as *mut c_void);
                current = next;
            } else {
                if let Some(fn_ptr) = (*current).function.acp1 {
                    fn_ptr(current as *mut c_void);
                }
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
#[no_mangle]
pub extern "C" fn P_Ticker() {
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
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    const THINKER_T_SIZEOF: usize = 24;

    #[test]
    fn thinker_t_size_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(
            std::mem::size_of::<thinker_t>(),
            THINKER_T_SIZEOF,
            "thinker_t size mismatch: Rust={}, expected={}",
            std::mem::size_of::<thinker_t>(),
            THINKER_T_SIZEOF,
        );
    }

    #[test]
    fn globals_default() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert_eq!(leveltime, 0);
        }
    }

    #[test]
    fn sentinel_is_all_ones() {
        let s = sentinel_ac();
        assert_eq!(usize::MAX, s.map(|f| f as usize).unwrap_or(0));
    }

    #[test]
    fn init_sets_self_pointers() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            thinkercap.prev = 0 as *mut thinker_t;
            thinkercap.next = 0 as *mut thinker_t;
            P_InitThinkers();
            assert_eq!(thinkercap.prev, &raw mut thinkercap as *mut thinker_t);
            assert_eq!(thinkercap.next, &raw mut thinkercap as *mut thinker_t);
        }
    }
}
