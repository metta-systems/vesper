//! Plats (elevator platforms): raising and lowering floor sectors.
//!
//! Rust port of `vendor/doomgeneric/p_plats.c`.  Manages the per-tic update
//! logic for moving platforms ([`T_PlatRaise`]), linedef-triggered platform
//! activation ([`EV_DoPlat`] / [`EV_StopPlat`]), and the fixed-size active
//! platform table ([`activeplats`]).

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::doom::sounds::Sfx;
use std::ffi::c_void;
use std::os::raw::c_int;

use crate::doom::c_ffi as cffi;
use crate::doom::i_timer::TICRATE;
use crate::doom::m_fixed::{fixed_t, FRACUNIT};
use crate::doom::m_random::P_Random;
use crate::doom::p_floor::T_MovePlane;
use crate::doom::p_lights::{line_t, sector_t};
use crate::doom::p_setup::{sectors, sides};
use crate::doom::p_spec::{
    P_FindHighestFloorSurrounding, P_FindLowestFloorSurrounding, P_FindNextHighestFloor,
    P_FindSectorFromLineTag,
};
use crate::doom::p_tick::{leveltime, thinker_t, P_AddThinker, P_RemoveThinker};
use crate::doom::s_sound::S_StartSound;
use crate::doom::z_zone::{Z_Malloc, PU_LEVSPEC};
use crate::i_error;

/// Default platform movement speed: 1 fixed-point unit per tic.
///
/// Individual platform types multiply or divide this value:
/// `downWaitUpStay` uses `PLATSPEED * 4`, `blazeDWUS` uses `PLATSPEED * 8`,
/// and `raiseAndChange`/`raiseToNearestAndChange` use `PLATSPEED / 2`.
const PLATSPEED: fixed_t = FRACUNIT;

/// Unitless platform wait count; multiplied by `TICRATE` at call sites to give
/// the actual wait duration in tics (3 × 35 = 105 tics ≈ 3 s at 35 Hz).
const PLATWAIT: c_int = 3;

/// Maximum number of simultaneously active platform thinkers.
///
/// Matches `MAXPLATS` in `p_local.h`.  If all slots are full,
/// [`P_AddActivePlat`] calls `I_Error` and the engine aborts.
const MAXPLATS: usize = 30;

// result_e enum values returned by T_MovePlane.
/// `T_MovePlane` result: the floor moved without incident.
const result_ok: c_int = 0;
/// `T_MovePlane` result: the floor crushed something while moving.
const result_crushed: c_int = 1;
/// `T_MovePlane` result: the floor reached its destination height.
const result_pastdest: c_int = 2;

// plat_e enum values — current platform motion state.
/// Platform is moving upward toward `high`.
const up: c_int = 0;
/// Platform is moving downward toward `low`.
const down: c_int = 1;
/// Platform has reached a target and is counting down before reversing.
const waiting: c_int = 2;
/// Platform has been suspended; its thinker callback is suppressed.
const in_stasis: c_int = 3;

// plattype_e enum values — platform behaviour types.
/// Perpetually bounces between the lowest and highest surrounding floor heights.
const perpetualRaise: c_int = 0;
/// Lowers to the lowest surrounding floor, waits, then rises back and stops.
const downWaitUpStay: c_int = 1;
/// Rises by `amount` units while matching the front sidedef's floor texture,
/// then stops.
const raiseAndChange: c_int = 2;
/// Rises to the next higher surrounding floor while matching texture, then
/// stops.
const raiseToNearestAndChange: c_int = 3;
/// Like [`downWaitUpStay`] but at twice the speed (`PLATSPEED * 8`).
const blazeDWUS: c_int = 4;

/// Per-sector thinker for an active platform mover.
///
/// Allocated via `Z_Malloc` and linked into the thinker list.  The `thinker`
/// field must be at offset 0 (verified by the compile-time assertions in
/// `layout_checks`) so that a `*mut plat_t` can be cast safely to
/// `*mut thinker_t`.
///
/// Layout matches the C `plat_t` struct.  The overall size is 72 bytes on
/// 64-bit targets.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct plat_t {
    /// Thinker header — must be at offset 0.
    pub thinker: thinker_t,
    /// The sector whose floor this thinker is moving.
    pub sector: *mut sector_t,
    /// Current movement speed in fixed-point units per tic.
    pub speed: fixed_t,
    /// Lower target height (destination when moving down).
    pub low: fixed_t,
    /// Upper target height (destination when moving up).
    pub high: fixed_t,
    /// Total tics to wait at a target before reversing (set from `PLATWAIT`).
    pub wait: c_int,
    /// Remaining tics in the current wait period; decremented each tic.
    pub count: c_int,
    /// Current motion state: one of `up`, `down`, `waiting`, `in_stasis`.
    pub status: c_int,
    /// Saved status used to resume after stasis.
    pub oldstatus: c_int,
    /// Non-zero if the platform damages things it crushes (unused for plats).
    pub crush: c_int,
    /// Linedef tag that activated this platform.
    pub tag: c_int,
    /// Platform behaviour type (one of the `plattype_e` integer constants).
    pub r#type: c_int,
}

#[cfg(target_pointer_width = "64")]
mod layout_checks {
    use super::*;
    const _: () = assert!(std::mem::size_of::<plat_t>() == 72);
    const _: () = assert!(std::mem::offset_of!(plat_t, thinker) == 0);
    const _: () = assert!(std::mem::offset_of!(plat_t, sector) == 24);
    const _: () = assert!(std::mem::offset_of!(plat_t, speed) == 32);
    const _: () = assert!(std::mem::offset_of!(plat_t, low) == 36);
    const _: () = assert!(std::mem::offset_of!(plat_t, high) == 40);
    const _: () = assert!(std::mem::offset_of!(plat_t, wait) == 44);
    const _: () = assert!(std::mem::offset_of!(plat_t, count) == 48);
    const _: () = assert!(std::mem::offset_of!(plat_t, status) == 52);
    const _: () = assert!(std::mem::offset_of!(plat_t, oldstatus) == 56);
    const _: () = assert!(std::mem::offset_of!(plat_t, crush) == 60);
    const _: () = assert!(std::mem::offset_of!(plat_t, tag) == 64);
    const _: () = assert!(std::mem::offset_of!(plat_t, r#type) == 68);
}

/// Table of all currently active platform thinkers.
///
/// Null entries represent free slots.  [`P_AddActivePlat`] fills the first
/// null slot; [`P_RemoveActivePlat`] nulls the matching slot.  Unlike ceilings,
/// overflowing this table is a fatal error.
///
/// Exported as `activeplats` for C linkage.
#[no_mangle]
pub static mut activeplats: [*mut plat_t; MAXPLATS] = [std::ptr::null_mut(); MAXPLATS];

/// Per-tic update for a moving platform thinker.
///
/// Dispatches on `plat->status`:
///
/// - **`up`**: calls `T_MovePlane` toward `high`.
///   - If the result is `result_crushed` and `crush == 0`, resets the count,
///     reverses direction to `down`, and plays the start sound.
///   - If the result is `result_pastDest`, switches to `waiting`, plays the
///     stop sound, and for one-shot types (`downWaitUpStay`, `blazeDWUS`,
///     `raiseAndChange`, `raiseToNearestAndChange`) removes the platform.
///   - For `raiseAndChange`/`raiseToNearestAndChange`, plays the movement
///     sound every 8 tics during the rise.
/// - **`down`**: calls `T_MovePlane` toward `low` (no crush).
///   - On `result_pastDest`, switches to `waiting` and plays the stop sound.
/// - **`waiting`**: decrements `count`; when it reaches 0, determines the next
///   direction by comparing the current floor height to `low`, plays the start
///   sound, and transitions to `up` or `down`.
/// - **`in_stasis`**: no-op — the thinker function is nulled by
///   [`EV_StopPlat`] in practice, but this arm handles the edge case where it
///   is not.
///
/// Corresponds to `T_PlatRaise` in `p_plats.c`.
///
/// # Safety
///
/// `plat` must be a valid, non-null pointer to a `plat_t` that is currently
/// linked in the thinker list and whose `sector` pointer is valid.
#[no_mangle]
pub unsafe extern "C" fn T_PlatRaise(plat: *mut plat_t) {
    match (*plat).status {
        x if x == up => {
            let res = T_MovePlane(
                (*plat).sector,
                (*plat).speed,
                (*plat).high,
                (*plat).crush,
                0,
                1,
            );

            if ((*plat).r#type == raiseAndChange || (*plat).r#type == raiseToNearestAndChange)
                && (leveltime & 7) == 0
            {
                S_StartSound(
                    &(*(*plat).sector).soundorg as *const [u8; 40] as *mut c_void,
                    Sfx::Stnmov as c_int,
                );
            }

            if res == result_crushed && (*plat).crush == 0 {
                (*plat).count = (*plat).wait;
                (*plat).status = down;
                S_StartSound(
                    &(*(*plat).sector).soundorg as *const [u8; 40] as *mut c_void,
                    Sfx::Pstart as c_int,
                );
            } else if res == result_pastdest {
                (*plat).count = (*plat).wait;
                (*plat).status = waiting;
                S_StartSound(
                    &(*(*plat).sector).soundorg as *const [u8; 40] as *mut c_void,
                    Sfx::Pstop as c_int,
                );

                match (*plat).r#type {
                    x if x == blazeDWUS
                        || x == downWaitUpStay
                        || x == raiseAndChange
                        || x == raiseToNearestAndChange =>
                    {
                        P_RemoveActivePlat(plat);
                    }
                    _ => {}
                }
            }
        }
        x if x == down => {
            let res = T_MovePlane((*plat).sector, (*plat).speed, (*plat).low, 0, 0, -1);

            if res == result_pastdest {
                (*plat).count = (*plat).wait;
                (*plat).status = waiting;
                S_StartSound(
                    &(*(*plat).sector).soundorg as *const [u8; 40] as *mut c_void,
                    Sfx::Pstop as c_int,
                );
            }
        }
        x if x == waiting => {
            (*plat).count -= 1;
            if (*plat).count == 0 {
                if (*(*plat).sector).floorheight == (*plat).low {
                    (*plat).status = up;
                } else {
                    (*plat).status = down;
                }
                S_StartSound(
                    &(*(*plat).sector).soundorg as *const [u8; 40] as *mut c_void,
                    Sfx::Pstart as c_int,
                );
            }
        }
        x if x == in_stasis => {}
        _ => {}
    }
}

/// Activate a platform mover on every sector whose tag matches `line->tag`.
///
/// For `perpetualRaise`, first reactivates any in-stasis platforms with the
/// same tag via [`P_ActivateInStasis`].
///
/// For each eligible sector (no existing special data):
/// 1. Allocates and links a new `plat_t` thinker.
/// 2. Initialises its parameters (speed, height targets, wait time, initial
///    status) based on `plattype`.
/// 3. Registers it in [`activeplats`].
///
/// The `amount` parameter is only used by `raiseAndChange` to set the target
/// height to `floorheight + amount * FRACUNIT`.
///
/// Returns `1` if at least one platform was activated, `0` otherwise.
///
/// Corresponds to `EV_DoPlat` in `p_plats.c`.
///
/// # Safety
///
/// `line` must be a valid, non-null pointer.  The global `sectors` and `sides`
/// arrays must be initialised for the current level.
#[no_mangle]
pub unsafe extern "C" fn EV_DoPlat(line: *mut line_t, plattype: c_int, amount: c_int) -> c_int {
    let mut secnum: c_int = -1;
    let mut rtn: c_int = 0;

    match plattype {
        x if x == perpetualRaise => {
            P_ActivateInStasis((*line).tag as c_int);
        }
        _ => {}
    }

    while {
        secnum = P_FindSectorFromLineTag(line as *mut cffi::line_t, secnum);
        secnum
    } >= 0
    {
        let sec = sectors.add(secnum as usize);

        if !(*sec).specialdata.is_null() {
            continue;
        }

        rtn = 1;
        let plat = Z_Malloc(
            std::mem::size_of::<plat_t>() as c_int,
            PU_LEVSPEC,
            std::ptr::null_mut(),
        ) as *mut plat_t;
        P_AddThinker(&mut (*plat).thinker);

        (*plat).r#type = plattype;
        (*plat).sector = sec as *mut sector_t;
        (*plat).sector.as_mut().unwrap().specialdata = plat as *mut c_void;
        (*plat).thinker.function.acp1 = Some(core::mem::transmute::<
            unsafe extern "C" fn(*mut plat_t),
            unsafe extern "C" fn(*mut c_void),
        >(T_PlatRaise));
        (*plat).crush = 0;
        (*plat).tag = (*line).tag as c_int;

        match plattype {
            x if x == raiseToNearestAndChange => {
                (*plat).speed = PLATSPEED / 2;
                let sidenum = (*line).sidenum[0] as isize;
                (*sec).floorpic = (*sides.offset(sidenum)).sector.as_mut().unwrap().floorpic;
                (*plat).high = P_FindNextHighestFloor(sec, (*sec).floorheight);
                (*plat).wait = 0;
                (*plat).status = up;
                (*sec).special = 0;
                S_StartSound(
                    &(*sec).soundorg as *const [u8; 40] as *mut c_void,
                    Sfx::Stnmov as c_int,
                );
            }
            x if x == raiseAndChange => {
                (*plat).speed = PLATSPEED / 2;
                let sidenum = (*line).sidenum[0] as isize;
                (*sec).floorpic = (*sides.offset(sidenum)).sector.as_mut().unwrap().floorpic;
                (*plat).high = (*sec).floorheight + amount * FRACUNIT;
                (*plat).wait = 0;
                (*plat).status = up;
                S_StartSound(
                    &(*sec).soundorg as *const [u8; 40] as *mut c_void,
                    Sfx::Stnmov as c_int,
                );
            }
            x if x == downWaitUpStay => {
                (*plat).speed = PLATSPEED * 4;
                (*plat).low = P_FindLowestFloorSurrounding(sec);
                if (*plat).low > (*sec).floorheight {
                    (*plat).low = (*sec).floorheight;
                }
                (*plat).high = (*sec).floorheight;
                (*plat).wait = TICRATE * PLATWAIT;
                (*plat).status = down;
                S_StartSound(
                    &(*sec).soundorg as *const [u8; 40] as *mut c_void,
                    Sfx::Pstart as c_int,
                );
            }
            x if x == blazeDWUS => {
                (*plat).speed = PLATSPEED * 8;
                (*plat).low = P_FindLowestFloorSurrounding(sec);
                if (*plat).low > (*sec).floorheight {
                    (*plat).low = (*sec).floorheight;
                }
                (*plat).high = (*sec).floorheight;
                (*plat).wait = TICRATE * PLATWAIT;
                (*plat).status = down;
                S_StartSound(
                    &(*sec).soundorg as *const [u8; 40] as *mut c_void,
                    Sfx::Pstart as c_int,
                );
            }
            x if x == perpetualRaise => {
                (*plat).speed = PLATSPEED;
                (*plat).low = P_FindLowestFloorSurrounding(sec);
                if (*plat).low > (*sec).floorheight {
                    (*plat).low = (*sec).floorheight;
                }
                (*plat).high = P_FindHighestFloorSurrounding(sec);
                if (*plat).high < (*sec).floorheight {
                    (*plat).high = (*sec).floorheight;
                }
                (*plat).wait = TICRATE * PLATWAIT;
                (*plat).status = P_Random() & 1;
                S_StartSound(
                    &(*sec).soundorg as *const [u8; 40] as *mut c_void,
                    Sfx::Pstart as c_int,
                );
            }
            _ => {}
        }
        P_AddActivePlat(plat);
    }
    rtn
}

/// Restart all in-stasis platforms whose tag equals `tag`.
///
/// Restores `status` from `oldstatus` and reinstates `T_PlatRaise` as the
/// thinker callback.  Called by [`EV_DoPlat`] for the `perpetualRaise` type.
///
/// Corresponds to `P_ActivateInStasis` in `p_plats.c`.
#[no_mangle]
pub extern "C" fn P_ActivateInStasis(tag: c_int) {
    unsafe {
        for i in 0..MAXPLATS {
            if !activeplats[i].is_null()
                && (*activeplats[i]).tag == tag
                && (*activeplats[i]).status == in_stasis
            {
                (*activeplats[i]).status = (*activeplats[i]).oldstatus;
                (*activeplats[i]).thinker.function.acp1 = Some(core::mem::transmute::<
                    unsafe extern "C" fn(*mut plat_t),
                    unsafe extern "C" fn(*mut c_void),
                >(T_PlatRaise));
            }
        }
    }
}

/// Suspend all active platforms whose tag matches `line->tag`.
///
/// Saves `status` to `oldstatus`, sets `status` to `in_stasis`, and sets the
/// thinker function to `None` so the callback is skipped.  The platform
/// remains in [`activeplats`] and can be resumed by [`P_ActivateInStasis`].
///
/// Corresponds to `EV_StopPlat` in `p_plats.c`.
///
/// # Safety
///
/// `line` must be a valid, non-null pointer.
#[no_mangle]
pub extern "C" fn EV_StopPlat(line: *mut line_t) {
    unsafe {
        for j in 0..MAXPLATS {
            if !activeplats[j].is_null()
                && (*activeplats[j]).status != in_stasis
                && (*activeplats[j]).tag == (*line).tag as c_int
            {
                (*activeplats[j]).oldstatus = (*activeplats[j]).status;
                (*activeplats[j]).status = in_stasis;
                (*activeplats[j]).thinker.function.acv = None;
            }
        }
    }
}

/// Register `plat` in the first available slot of [`activeplats`].
///
/// If all `MAXPLATS` (30) slots are occupied, calls `I_Error` and aborts — unlike
/// `P_AddActiveCeiling` which silently discards overflows.
///
/// Corresponds to `P_AddActivePlat` in `p_plats.c`.
#[no_mangle]
pub extern "C" fn P_AddActivePlat(plat: *mut plat_t) {
    unsafe {
        for i in 0..MAXPLATS {
            if activeplats[i].is_null() {
                activeplats[i] = plat;
                return;
            }
        }
        i_error!("P_AddActivePlat: no more plats!");
    }
}

/// Unlink and schedule deallocation of the active platform `plat`.
///
/// Clears `sector->specialdata`, calls [`P_RemoveThinker`] (which marks the
/// thinker for deferred `Z_Free`), and nulls the matching slot in
/// [`activeplats`].  Calls `I_Error` if the platform is not found.
///
/// Corresponds to `P_RemoveActivePlat` in `p_plats.c`.
#[no_mangle]
pub extern "C" fn P_RemoveActivePlat(plat: *mut plat_t) {
    unsafe {
        for i in 0..MAXPLATS {
            if plat == activeplats[i] {
                (*activeplats[i]).sector.as_mut().unwrap().specialdata = std::ptr::null_mut();
                P_RemoveThinker(&mut (*activeplats[i]).thinker);
                activeplats[i] = std::ptr::null_mut();
                return;
            }
        }
        i_error!("P_RemoveActivePlat: can't find plat!");
    }
}

/// Link anchor that prevents dead-code elimination of exported platform symbols.
///
/// Referenced from the engine initialisation path so the linker keeps every
/// platform function in the final binary even if no Rust caller uses them
/// directly.
#[no_mangle]
pub extern "C" fn P_Plats_Link_Anchor() {
    let _ = T_PlatRaise as *const () as usize;
    let _ = EV_DoPlat as *const () as usize;
    let _ = P_ActivateInStasis as *const () as usize;
    let _ = EV_StopPlat as *const () as usize;
    let _ = P_AddActivePlat as *const () as usize;
    let _ = P_RemoveActivePlat as *const () as usize;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    const PLAT_T_SIZEOF: usize = 72;
    const PLAT_T_THINKER: usize = 0;
    const PLAT_T_SECTOR: usize = 24;
    const PLAT_T_SPEED: usize = 32;
    const PLAT_T_LOW: usize = 36;
    const PLAT_T_HIGH: usize = 40;
    const PLAT_T_WAIT: usize = 44;
    const PLAT_T_COUNT: usize = 48;
    const PLAT_T_STATUS: usize = 52;
    const PLAT_T_OLDSTATUS: usize = 56;
    const PLAT_T_CRUSH: usize = 60;
    const PLAT_T_TAG: usize = 64;
    const PLAT_T_TYPE: usize = 68;

    #[test]
    fn plat_t_layout_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(std::mem::size_of::<plat_t>(), PLAT_T_SIZEOF);
        assert_eq!(std::mem::offset_of!(plat_t, thinker), PLAT_T_THINKER);
        assert_eq!(std::mem::offset_of!(plat_t, sector), PLAT_T_SECTOR);
        assert_eq!(std::mem::offset_of!(plat_t, speed), PLAT_T_SPEED);
        assert_eq!(std::mem::offset_of!(plat_t, low), PLAT_T_LOW);
        assert_eq!(std::mem::offset_of!(plat_t, high), PLAT_T_HIGH);
        assert_eq!(std::mem::offset_of!(plat_t, wait), PLAT_T_WAIT);
        assert_eq!(std::mem::offset_of!(plat_t, count), PLAT_T_COUNT);
        assert_eq!(std::mem::offset_of!(plat_t, status), PLAT_T_STATUS);
        assert_eq!(std::mem::offset_of!(plat_t, oldstatus), PLAT_T_OLDSTATUS);
        assert_eq!(std::mem::offset_of!(plat_t, crush), PLAT_T_CRUSH);
        assert_eq!(std::mem::offset_of!(plat_t, tag), PLAT_T_TAG);
        assert_eq!(std::mem::offset_of!(plat_t, r#type), PLAT_T_TYPE);
    }
}
