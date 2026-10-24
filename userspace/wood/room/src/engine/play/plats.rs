//! Plats (elevator platforms): raising and lowering floor sectors.
//!
//! Rust port of `vendor/doomgeneric/p_plats.c`.  Manages the per-tic update
//! logic for moving platforms ([`T_PlatRaise`]), linedef-triggered platform
//! activation ([`EV_DoPlat`] / [`EV_StopPlat`]), and the fixed-size active
//! platform table ([`PlatState::activeplats`]).

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::engine::sound::info::Sfx;

use crate::{
    engine::{
        interface::timer::TICRATE,
        map::{SectorId, line_t},
        math::fixed::{FRACUNIT, fixed_t},
        misc::random::P_Random,
        play::{
            floor::T_MovePlane,
            spec::{
                P_FindHighestFloorSurrounding, P_FindLowestFloorSurrounding,
                P_FindNextHighestFloor, P_FindSectorFromLineTag,
            },
            tick::{
                P_AddThinker, P_RemoveThinker, PlatId, ThinkerId, ThinkerKind, new_thinker,
                thinker_t,
            },
        },
        sound::S_StartSectorSound,
        world::World,
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
/// Lives in the platform arena and is listed in the thinker list as
/// [`ThinkerId::Plat`].  The fields are those of the C `plat_t` struct.
#[derive(Clone, Copy)]
pub struct plat_t {
    /// Thinker state: what the platform does each tic.
    pub thinker: thinker_t,
    /// The sector whose floor this thinker is moving.
    pub sector: SectorId,
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

/// The active-platform table (`p_plats.c` global), kept in
/// [`ActiveSpecials`](crate::engine::world::ActiveSpecials).
pub struct PlatState {
    /// Table of all currently active platform thinkers.
    ///
    /// `None` entries represent free slots.  [`P_AddActivePlat`] fills the
    /// first free slot; [`P_RemoveActivePlat`] clears the matching slot.
    /// Unlike ceilings, overflowing this table is a fatal error.
    pub activeplats: [Option<PlatId>; MAXPLATS],
}

impl PlatState {
    /// No platform active.
    pub const fn new() -> Self {
        Self {
            activeplats: [None; MAXPLATS],
        }
    }
}

impl Default for PlatState {
    fn default() -> Self {
        Self::new()
    }
}

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
/// `plat_id` must name a platform in the thinker list whose `sector`
/// belongs to the current map.
pub unsafe fn T_PlatRaise(world: &mut World, plat_id: PlatId) {
    let plat = plat_id.ptr();
    match (*plat).status {
        x if x == up => {
            let res = T_MovePlane(
                world,
                (*plat).sector,
                (*plat).speed,
                (*plat).high,
                (*plat).crush,
                0,
                1,
            );

            if ((*plat).kind == raiseAndChange || (*plat).kind == raiseToNearestAndChange)
                && (world.leveltime & 7) == 0
            {
                S_StartSectorSound(world, (*plat).sector, Sfx::Stnmov as i32);
            }

            if res == result_crushed && (*plat).crush == 0 {
                (*plat).count = (*plat).wait;
                (*plat).status = down;
                S_StartSectorSound(world, (*plat).sector, Sfx::Pstart as i32);
            } else if res == result_pastdest {
                (*plat).count = (*plat).wait;
                (*plat).status = waiting;
                S_StartSectorSound(world, (*plat).sector, Sfx::Pstop as i32);

                match (*plat).kind {
                    x if x == blazeDWUS
                        || x == downWaitUpStay
                        || x == raiseAndChange
                        || x == raiseToNearestAndChange =>
                    {
                        P_RemoveActivePlat(world, plat_id);
                    }
                    _ => {}
                }
            }
        }
        x if x == down => {
            let res = T_MovePlane(world, (*plat).sector, (*plat).speed, (*plat).low, 0, 0, -1);

            if res == result_pastdest {
                (*plat).count = (*plat).wait;
                (*plat).status = waiting;
                S_StartSectorSound(world, (*plat).sector, Sfx::Pstop as i32);
            }
        }
        x if x == waiting => {
            (*plat).count -= 1;
            if (*plat).count == 0 {
                if world.level.sectors[(*plat).sector].floorheight == (*plat).low {
                    (*plat).status = up;
                } else {
                    (*plat).status = down;
                }
                S_StartSectorSound(world, (*plat).sector, Sfx::Pstart as i32);
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
/// 3. Registers it in `activeplats`.
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
/// The thinker list must be initialised and a level loaded.
pub unsafe fn EV_DoPlat(world: &mut World, line: &line_t, plattype: i32, amount: i32) -> i32 {
    let mut secnum: i32 = -1;
    let mut rtn: i32 = 0;

    match plattype {
        x if x == perpetualRaise => {
            P_ActivateInStasis(world, line.tag as i32);
        }
        _ => {}
    }

    while {
        secnum = P_FindSectorFromLineTag(&world.level, line, secnum);
        secnum
    } >= 0
    {
        let sec = SectorId::new(secnum as usize);

        if world.level.sectors[sec].specialdata.is_some() {
            continue;
        }

        rtn = 1;
        let plat_id = new_thinker::<plat_t>();
        let plat = plat_id.ptr();
        P_AddThinker(ThinkerId::Plat(plat_id));

        // The platform's sound starts once it is set up.
        let sound = {
            let level = &mut world.level;
            (*plat).kind = plattype;
            (*plat).sector = sec;
            level.sectors[sec].specialdata = Some(ThinkerId::Plat(plat_id));
            (*plat).thinker.kind = ThinkerKind::Plat;
            (*plat).crush = 0;
            (*plat).tag = line.tag as i32;
            let floorheight = level.sectors[sec].floorheight;

            match plattype {
                x if x == raiseToNearestAndChange => {
                    (*plat).speed = PLATSPEED / 2;
                    let sidenum = line.sidenum[0] as usize;
                    level.sectors[sec].floorpic =
                        level.sectors[level.sides[sidenum].sector].floorpic;
                    (*plat).high = P_FindNextHighestFloor(level, sec, floorheight);
                    (*plat).wait = 0;
                    (*plat).status = up;
                    level.sectors[sec].special = 0;
                    Some(Sfx::Stnmov)
                }
                x if x == raiseAndChange => {
                    (*plat).speed = PLATSPEED / 2;
                    let sidenum = line.sidenum[0] as usize;
                    level.sectors[sec].floorpic =
                        level.sectors[level.sides[sidenum].sector].floorpic;
                    (*plat).high = floorheight + amount * FRACUNIT;
                    (*plat).wait = 0;
                    (*plat).status = up;
                    Some(Sfx::Stnmov)
                }
                x if x == downWaitUpStay => {
                    (*plat).speed = PLATSPEED * 4;
                    (*plat).low = P_FindLowestFloorSurrounding(level, sec);
                    if (*plat).low > floorheight {
                        (*plat).low = floorheight;
                    }
                    (*plat).high = floorheight;
                    (*plat).wait = TICRATE * PLATWAIT;
                    (*plat).status = down;
                    Some(Sfx::Pstart)
                }
                x if x == blazeDWUS => {
                    (*plat).speed = PLATSPEED * 8;
                    (*plat).low = P_FindLowestFloorSurrounding(level, sec);
                    if (*plat).low > floorheight {
                        (*plat).low = floorheight;
                    }
                    (*plat).high = floorheight;
                    (*plat).wait = TICRATE * PLATWAIT;
                    (*plat).status = down;
                    Some(Sfx::Pstart)
                }
                x if x == perpetualRaise => {
                    (*plat).speed = PLATSPEED;
                    (*plat).low = P_FindLowestFloorSurrounding(level, sec);
                    if (*plat).low > floorheight {
                        (*plat).low = floorheight;
                    }
                    (*plat).high = P_FindHighestFloorSurrounding(level, sec);
                    if (*plat).high < floorheight {
                        (*plat).high = floorheight;
                    }
                    (*plat).wait = TICRATE * PLATWAIT;
                    (*plat).status = P_Random(world) & 1;
                    Some(Sfx::Pstart)
                }
                _ => None,
            }
        };
        if let Some(sound) = sound {
            S_StartSectorSound(world, sec, sound as i32);
        }
        P_AddActivePlat(world, plat_id);
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
pub fn P_ActivateInStasis(world: &mut World, tag: i32) {
    for plat in world.active.plats.activeplats.into_iter().flatten() {
        // SAFETY: platforms are only reached through arena pointers, and no
        // reference to one is live while a line special runs.
        let plat = unsafe { &mut *plat.ptr() };
        if plat.tag == tag && plat.status == in_stasis {
            plat.status = plat.oldstatus;
            plat.thinker.kind = ThinkerKind::Plat;
        }
    }
}

/// Suspend all active platforms whose tag matches `line->tag`.
///
/// Saves `status` to `oldstatus`, sets `status` to `in_stasis`, and sets the
/// thinker kind to [`ThinkerKind::PlatInStasis`] so `T_PlatRaise` is skipped.  The platform
/// remains in `activeplats` and can be resumed by [`P_ActivateInStasis`].
///
/// Corresponds to `EV_StopPlat` in `p_plats.c`.
pub fn EV_StopPlat(world: &mut World, line: &line_t) {
    for plat in world.active.plats.activeplats.into_iter().flatten() {
        // SAFETY: platforms are only reached through arena pointers, and no
        // reference to one is live while a line special runs.
        let plat = unsafe { &mut *plat.ptr() };
        if plat.status != in_stasis && plat.tag == line.tag as i32 {
            plat.oldstatus = plat.status;
            plat.status = in_stasis;
            plat.thinker.kind = ThinkerKind::PlatInStasis;
        }
    }
}

/// Register `plat` in the first available slot of `activeplats`.
///
/// If all `MAXPLATS` (30) slots are occupied, calls `I_Error` and aborts — unlike
/// `P_AddActiveCeiling` which silently discards overflows. The platform's
/// `sector` must belong to the loaded level; it stays registered until
/// `P_RemoveActivePlat` or the next level load clears the table.
///
/// Corresponds to `P_AddActivePlat` in `p_plats.c`.
pub fn P_AddActivePlat(world: &mut World, plat: PlatId) {
    for slot in world.active.plats.activeplats.iter_mut() {
        if slot.is_none() {
            *slot = Some(plat);
            return;
        }
    }
    i_error!("P_AddActivePlat: no more plats!");
}

/// Unlink and schedule deallocation of the active platform `plat`.
///
/// Clears `sector->specialdata`, calls [`P_RemoveThinker`] (which marks the
/// thinker for deferred freeing), and clears the matching slot in
/// `activeplats`.  Calls `I_Error` if the platform is not found.
///
/// Corresponds to `P_RemoveActivePlat` in `p_plats.c`.
pub fn P_RemoveActivePlat(world: &mut World, plat: PlatId) {
    let World { level, active, .. } = world;
    for slot in active.plats.activeplats.iter_mut() {
        if *slot == Some(plat) {
            // SAFETY: platforms are only reached through arena pointers, and
            // no reference to one is live while it is removed.
            let plat = unsafe { &mut *plat.ptr() };
            level.sectors[plat.sector].specialdata = None;
            P_RemoveThinker(&mut plat.thinker);
            *slot = None;
            return;
        }
    }
    i_error!("P_RemoveActivePlat: can't find plat!");
}
