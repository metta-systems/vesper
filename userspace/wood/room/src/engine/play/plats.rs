//! Plats (elevator platforms): raising and lowering floor sectors.
//!
//! Rust port of `vendor/doomgeneric/p_plats.c`.  Manages the per-tic update
//! logic for moving platforms ([`T_PlatRaise`]), linedef-triggered platform
//! activation ([`EV_DoPlat`] / [`EV_StopPlat`]), and the fixed-size active
//! platform table ([`activeplats`]).

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::engine::sound::info::Sfx;

use crate::{
    engine::{
        interface::timer::TICRATE,
        map::{line_t, sector_t},
        math::fixed::{fixed_t, FRACUNIT},
        misc::random::P_Random,
        play::{
            floor::T_MovePlane,
            setup::{sectors, sides},
            spec::{
                P_FindHighestFloorSurrounding, P_FindLowestFloorSurrounding,
                P_FindNextHighestFloor, P_FindSectorFromLineTag,
            },
            tick::{leveltime, thinker_t, P_AddThinker, P_RemoveThinker, ThinkerKind},
        },
        sound::S_StartSectorSound,
        zone::{Z_Malloc, PU_LEVSPEC},
    },
    i_error,
};

/// Default platform movement speed: 1 fixed-point unit per tic.
///
/// Individual platform types multiply or divide this value:
/// `downWaitUpStay` uses `PLATSPEED * 4`, `blazeDWUS` uses `PLATSPEED * 8`,
/// and `raiseAndChange`/`raiseToNearestAndChange` use `PLATSPEED / 2`.
pub const PLATSPEED: fixed_t = FRACUNIT;

/// Unitless platform wait count; multiplied by `TICRATE` at call sites to give
/// the actual wait duration in tics (3 × 35 = 105 tics ≈ 3 s at 35 Hz).
pub const PLATWAIT: i32 = 3;

/// Maximum number of simultaneously active platform thinkers.
///
/// Matches `MAXPLATS` in `p_local.h`.  If all slots are full,
/// [`P_AddActivePlat`] calls `I_Error` and the engine aborts.
pub const MAXPLATS: usize = 30;

// result_e enum values returned by T_MovePlane.
/// `T_MovePlane` result: the floor moved without incident.
const result_ok: i32 = 0;
/// `T_MovePlane` result: the floor crushed something while moving.
const result_crushed: i32 = 1;
/// `T_MovePlane` result: the floor reached its destination height.
const result_pastdest: i32 = 2;

// plat_e enum values — current platform motion state.
/// Platform is moving upward toward `high`.
const up: i32 = 0;
/// Platform is moving downward toward `low`.
const down: i32 = 1;
/// Platform has reached a target and is counting down before reversing.
const waiting: i32 = 2;
/// Platform has been suspended; its thinker callback is suppressed.
const in_stasis: i32 = 3;

// plattype_e enum values — platform behaviour types.
/// Perpetually bounces between the lowest and highest surrounding floor heights.
const perpetualRaise: i32 = 0;
/// Lowers to the lowest surrounding floor, waits, then rises back and stops.
const downWaitUpStay: i32 = 1;
/// Rises by `amount` units while matching the front sidedef's floor texture,
/// then stops.
const raiseAndChange: i32 = 2;
/// Rises to the next higher surrounding floor while matching texture, then
/// stops.
const raiseToNearestAndChange: i32 = 3;
/// Like [`downWaitUpStay`] but at twice the speed (`PLATSPEED * 8`).
const blazeDWUS: i32 = 4;

/// Per-sector thinker for an active platform mover.
///
/// Allocated via `Z_Malloc` and linked into the thinker list through its
/// embedded `thinker`.  The fields are those of the C `plat_t` struct.
#[derive(Clone, Copy)]
pub struct plat_t {
    /// Thinker list node.
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
    pub wait: i32,
    /// Remaining tics in the current wait period; decremented each tic.
    pub count: i32,
    /// Current motion state: one of `up`, `down`, `waiting`, `in_stasis`.
    pub status: i32,
    /// Saved status used to resume after stasis.
    pub oldstatus: i32,
    /// Non-zero if the platform damages things it crushes (unused for plats).
    pub crush: i32,
    /// Linedef tag that activated this platform.
    pub tag: i32,
    /// Platform behaviour type (one of the `plattype_e` integer constants).
    pub kind: i32,
}

/// Table of all currently active platform thinkers.
///
/// Null entries represent free slots.  [`P_AddActivePlat`] fills the first
/// null slot; [`P_RemoveActivePlat`] nulls the matching slot.  Unlike ceilings,
/// overflowing this table is a fatal error.
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
pub unsafe fn T_PlatRaise(plat: *mut plat_t) {
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

            if ((*plat).kind == raiseAndChange || (*plat).kind == raiseToNearestAndChange)
                && (leveltime & 7) == 0
            {
                S_StartSectorSound(&raw mut (*(*plat).sector).soundorg, Sfx::Stnmov as i32);
            }

            if res == result_crushed && (*plat).crush == 0 {
                (*plat).count = (*plat).wait;
                (*plat).status = down;
                S_StartSectorSound(&raw mut (*(*plat).sector).soundorg, Sfx::Pstart as i32);
            } else if res == result_pastdest {
                (*plat).count = (*plat).wait;
                (*plat).status = waiting;
                S_StartSectorSound(&raw mut (*(*plat).sector).soundorg, Sfx::Pstop as i32);

                match (*plat).kind {
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
                S_StartSectorSound(&raw mut (*(*plat).sector).soundorg, Sfx::Pstop as i32);
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
                S_StartSectorSound(&raw mut (*(*plat).sector).soundorg, Sfx::Pstart as i32);
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
pub unsafe fn EV_DoPlat(line: *mut line_t, plattype: i32, amount: i32) -> i32 {
    let mut secnum: i32 = -1;
    let mut rtn: i32 = 0;

    match plattype {
        x if x == perpetualRaise => {
            P_ActivateInStasis((*line).tag as i32);
        }
        _ => {}
    }

    while {
        secnum = P_FindSectorFromLineTag(line, secnum);
        secnum
    } >= 0
    {
        let sec = sectors.add(secnum as usize);

        if !(*sec).specialdata.is_null() {
            continue;
        }

        rtn = 1;
        let plat = Z_Malloc(
            std::mem::size_of::<plat_t>() as i32,
            PU_LEVSPEC,
            std::ptr::null_mut(),
        ) as *mut plat_t;
        P_AddThinker(&raw mut (*plat).thinker);

        (*plat).kind = plattype;
        (*plat).sector = sec;
        (*plat).sector.as_mut().unwrap().specialdata = &raw mut (*plat).thinker;
        (*plat).thinker.kind = ThinkerKind::Plat;
        (*plat).crush = 0;
        (*plat).tag = (*line).tag as i32;

        match plattype {
            x if x == raiseToNearestAndChange => {
                (*plat).speed = PLATSPEED / 2;
                let sidenum = (*line).sidenum[0] as isize;
                (*sec).floorpic = (*sides.offset(sidenum)).sector.as_mut().unwrap().floorpic;
                (*plat).high = P_FindNextHighestFloor(sec, (*sec).floorheight);
                (*plat).wait = 0;
                (*plat).status = up;
                (*sec).special = 0;
                S_StartSectorSound(&raw mut (*sec).soundorg, Sfx::Stnmov as i32);
            }
            x if x == raiseAndChange => {
                (*plat).speed = PLATSPEED / 2;
                let sidenum = (*line).sidenum[0] as isize;
                (*sec).floorpic = (*sides.offset(sidenum)).sector.as_mut().unwrap().floorpic;
                (*plat).high = (*sec).floorheight + amount * FRACUNIT;
                (*plat).wait = 0;
                (*plat).status = up;
                S_StartSectorSound(&raw mut (*sec).soundorg, Sfx::Stnmov as i32);
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
                S_StartSectorSound(&raw mut (*sec).soundorg, Sfx::Pstart as i32);
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
                S_StartSectorSound(&raw mut (*sec).soundorg, Sfx::Pstart as i32);
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
                S_StartSectorSound(&raw mut (*sec).soundorg, Sfx::Pstart as i32);
            }
            _ => {}
        }
        P_AddActivePlat(plat);
    }
    rtn
}

/// Restart all in-stasis platforms whose tag equals `tag`.
///
/// Restores `status` from `oldstatus` and sets the thinker kind back to
/// [`ThinkerKind::Plat`] so `T_PlatRaise` runs again.  Called by
/// [`EV_DoPlat`] for the `perpetualRaise` type.
///
/// Corresponds to `P_ActivateInStasis` in `p_plats.c`.
pub fn P_ActivateInStasis(tag: i32) {
    unsafe {
        for i in 0..MAXPLATS {
            if !activeplats[i].is_null()
                && (*activeplats[i]).tag == tag
                && (*activeplats[i]).status == in_stasis
            {
                (*activeplats[i]).status = (*activeplats[i]).oldstatus;
                (*activeplats[i]).thinker.kind = ThinkerKind::Plat;
            }
        }
    }
}

/// Suspend all active platforms whose tag matches `line->tag`.
///
/// Saves `status` to `oldstatus`, sets `status` to `in_stasis`, and sets the
/// thinker kind to [`ThinkerKind::PlatInStasis`] so `T_PlatRaise` is skipped.  The platform
/// remains in [`activeplats`] and can be resumed by [`P_ActivateInStasis`].
///
/// Corresponds to `EV_StopPlat` in `p_plats.c`.
///
/// # Safety
///
/// `line` must be a valid, non-null pointer.
pub fn EV_StopPlat(line: *mut line_t) {
    unsafe {
        for j in 0..MAXPLATS {
            if !activeplats[j].is_null()
                && (*activeplats[j]).status != in_stasis
                && (*activeplats[j]).tag == (*line).tag as i32
            {
                (*activeplats[j]).oldstatus = (*activeplats[j]).status;
                (*activeplats[j]).status = in_stasis;
                (*activeplats[j]).thinker.kind = ThinkerKind::PlatInStasis;
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
pub fn P_AddActivePlat(plat: *mut plat_t) {
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
pub fn P_RemoveActivePlat(plat: *mut plat_t) {
    unsafe {
        for i in 0..MAXPLATS {
            if plat == activeplats[i] {
                (*activeplats[i]).sector.as_mut().unwrap().specialdata = std::ptr::null_mut();
                P_RemoveThinker(&raw mut (*activeplats[i]).thinker);
                activeplats[i] = std::ptr::null_mut();
                return;
            }
        }
        i_error!("P_RemoveActivePlat: can't find plat!");
    }
}
