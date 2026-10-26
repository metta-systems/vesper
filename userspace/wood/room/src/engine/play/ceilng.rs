//! Ceiling animation: lowering, crushing, and raising.
//!
//! Rust port of `vendor/doomgeneric/p_ceilng.c`.  Manages the per-tic update
//! logic for moving ceilings ([`T_MoveCeiling`]), linedef-triggered ceiling
//! activation ([`EV_DoCeiling`] / [`EV_CeilingCrushStop`]), and the fixed-size
//! active-ceiling table ([`CeilingState::activeceilings`]).

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::engine::sound::info::Sfx;

use crate::engine::{
    map::{SectorId, line_t},
    math::fixed::{FRACUNIT, fixed_t},
    play::{
        floor::T_MovePlane,
        spec::{P_FindHighestCeilingSurrounding, P_FindSectorFromLineTag},
        tick::{
            CeilingId, P_AddThinker, P_RemoveThinker, ThinkerId, ThinkerKind, new_thinker,
            thinker_t,
        },
    },
    sound::S_StartSectorSound,
    world::World,
};

/// Default ceiling movement speed: 1 fixed-point unit per tic (`FRACUNIT`).
///
/// Crusher ceilings slow to `CEILSPEED / 8` when they hit something, and fast
/// crushers move at `CEILSPEED * 2`.  Matches `CEILSPEED` in `p_ceilng.c`.
pub const CEILSPEED: fixed_t = FRACUNIT;

/// Ceiling movement wait time in tics (`CEILWAIT` in `p_spec.h` = 150).
pub const CEILWAIT: i32 = 150;

/// Maximum number of simultaneously active ceiling movers.
///
/// Matches `MAXCEILINGS` in `p_local.h`.  If all slots are occupied,
/// [`P_AddActiveCeiling`] silently discards new ceilings.
pub const MAXCEILINGS: usize = 30;

// result_e enum values returned by T_MovePlane.
/// `T_MovePlane` result: the plane moved without incident.
const result_ok: i32 = 0;
/// `T_MovePlane` result: the plane crushed something while moving.
const result_crushed: i32 = 1;
/// `T_MovePlane` result: the plane reached its destination height.
const result_pastdest: i32 = 2;

// ceiling_e enum values — ceiling mover types.
/// Lower ceiling until it reaches the floor height of the sector.
const lowerToFloor: i32 = 0;
/// Raise ceiling to the highest surrounding ceiling height.
const raiseToHighest: i32 = 1;
/// Lower ceiling to 8 units above the floor, crushing things in the way.
const lowerAndCrush: i32 = 2;
/// Crusher that bounces between floor+8 and its original height; normal speed.
const crushAndRaise: i32 = 3;
/// Crusher that bounces at twice the normal speed.
const fastCrushAndRaise: i32 = 4;
/// Like [`crushAndRaise`] but does not play the movement sound each tic.
const silentCrushAndRaise: i32 = 5;

/// Per-sector thinker for an active ceiling mover.
///
/// Lives in the ceiling arena and is listed in the thinker list as
/// [`ThinkerId::Ceiling`].
#[derive(Clone, Copy)]
pub struct ceiling_t {
    /// Thinker state: what the ceiling does each tic.
    pub thinker: thinker_t,
    /// Ceiling type (one of the `ceiling_e` integer constants).
    pub kind: i32,
    /// The sector whose ceiling this thinker is moving.
    pub sector: SectorId,
    /// Target height when the ceiling is moving downward.
    pub bottomheight: fixed_t,
    /// Target height when the ceiling is moving upward.
    pub topheight: fixed_t,
    /// Current movement speed in fixed-point units per tic.
    pub speed: fixed_t,
    /// Non-zero if the ceiling damages things it crushes.
    pub crush: i32,
    /// Current movement direction: `1` = up, `-1` = down, `0` = in stasis.
    pub direction: i32,
    /// Linedef tag that activated this ceiling (used for crush-stop matching).
    pub tag: i32,
    /// Saved direction used to resume a ceiling that was put into stasis.
    pub olddirection: i32,
}

/// The active-ceiling table (`p_ceilng.c` global), kept in
/// [`ActiveSpecials`](crate::engine::world::ActiveSpecials).
pub struct CeilingState {
    /// Table of all currently active ceiling thinkers.
    ///
    /// `None` entries represent free slots.  [`P_AddActiveCeiling`] fills the
    /// first free slot; [`P_RemoveActiveCeiling`] clears the matching slot.
    pub activeceilings: [Option<CeilingId>; MAXCEILINGS],
}

impl CeilingState {
    /// No ceiling active.
    pub const fn new() -> Self {
        Self {
            activeceilings: [None; MAXCEILINGS],
        }
    }
}

impl Default for CeilingState {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-tic update for a moving ceiling thinker.
///
/// Calls `T_MovePlane` with the appropriate target height (determined by
/// `ceiling->direction`), then reacts to the result:
///
/// - **`result_pastdest`**: the ceiling reached its target height.
///   - `raiseToHighest`: remove and free the thinker.
///   - `silentCrushAndRaise` (downward): play the stop sound, then reverse
///     direction upward.
///   - `fastCrushAndRaise` / `crushAndRaise` (downward): reverse direction.
///   - `lowerAndCrush` / `lowerToFloor` (downward): remove and free the
///     thinker.
/// - **`result_crushed`**: the ceiling hit something while moving down.
///   - `silentCrushAndRaise` / `crushAndRaise` / `lowerAndCrush`: reduce
///     speed to `CEILSPEED / 8` to avoid shaking the victim rapidly.
///
/// Every 8 tics, a movement sound (`sfx_stnmov`) is played at the sector's
/// sound origin, except for `silentCrushAndRaise` which suppresses it.
///
/// # FIXME
/// The C original uses a `switch(direction)` with explicit `case 0` (stasis,
/// no-op), `case 1` (up), and `case -1` (down) branches.  The direction
/// controls which target height is passed to `T_MovePlane` and which
/// `pastdest` actions apply.  This Rust port passes `direction` to
/// `T_MovePlane` but does not gate the `pastdest`/`crushed` handling on the
/// current direction, so those match arms may fire for `direction == 0`
/// (stasis) or the wrong direction — a minor behavioral divergence from C.
///
/// # FIXME
/// The C `case silentCrushAndRaise` (downward) falls through to
/// `crushAndRaise`/`fastCrushAndRaise`, which restores speed to `CEILSPEED`
/// and reverses direction.  This Rust port handles `silentCrushAndRaise`
/// separately and only plays the stop sound; it does not restore speed or
/// reverse direction for that variant.
///
/// Corresponds to `T_MoveCeiling` in `p_ceilng.c`.
///
/// # Safety
///
/// The ceiling's `sector` must belong to the current map, and every thing
/// linked into the blockmap must be live (see `T_MovePlane`).
pub unsafe fn T_MoveCeiling(world: &mut World, ceiling_id: CeilingId) {
    let ceiling = world.thinkers.ceilings[ceiling_id];
    let res = T_MovePlane(
        world,
        ceiling.sector,
        ceiling.speed,
        if ceiling.direction == 1 {
            ceiling.topheight
        } else {
            ceiling.bottomheight
        },
        ceiling.crush,
        1,
        ceiling.direction,
    );
    // `T_MovePlane` can run play code: read the ceiling afresh, as C does
    // through its pointer.
    let ceiling = world.thinkers.ceilings[ceiling_id];

    if (world.leveltime & 7) == 0 {
        match ceiling.kind {
            x if x == silentCrushAndRaise => {}
            _ => {
                S_StartSectorSound(world, ceiling.sector, Sfx::Stnmov as i32);
            }
        }
    }

    if res == result_pastdest {
        match ceiling.kind {
            x if x == raiseToHighest => {
                P_RemoveActiveCeiling(world, ceiling_id);
            }
            x if x == silentCrushAndRaise => {
                S_StartSectorSound(world, ceiling.sector, Sfx::Pstop as i32);
            }
            x if x == fastCrushAndRaise || x == crushAndRaise => {
                world.thinkers.ceilings[ceiling_id].direction = -1;
            }
            _ => {}
        }
    } else if res == result_crushed {
        match ceiling.kind {
            x if x == silentCrushAndRaise || x == crushAndRaise || x == lowerAndCrush => {
                world.thinkers.ceilings[ceiling_id].speed = CEILSPEED / 8;
            }
            _ => {}
        }
    }
}

/// Activate a ceiling mover on every sector whose tag matches `line->tag`.
///
/// For crusher types (`fastCrushAndRaise`, `silentCrushAndRaise`,
/// `crushAndRaise`), first reactivates any in-stasis ceilings with the same
/// tag via [`P_ActivateInStasisCeiling`].
///
/// For each eligible sector (no existing special data):
/// 1. Allocates and links a new `ceiling_t` thinker.
/// 2. Sets its parameters based on `ceilingtype` (height targets, speed,
///    crush flag, direction).  `silentCrushAndRaise` and `crushAndRaise` only
///    initialise `crush` and `topheight`; `bottomheight` and `direction` keep
///    their zero-initialised defaults and are set dynamically by `T_MoveCeiling`.
/// 3. Registers it in `activeceilings`.
///
/// Returns `1` if at least one ceiling was activated, `0` otherwise.
///
/// Corresponds to `EV_DoCeiling` in `p_ceilng.c`.
pub fn EV_DoCeiling(world: &mut World, line: &line_t, ceilingtype: i32) -> i32 {
    let mut secnum: i32 = -1;
    let mut rtn: i32 = 0;

    // Reactivate in-stasis ceilings for certain types.
    match ceilingtype {
        x if x == fastCrushAndRaise || x == silentCrushAndRaise || x == crushAndRaise => {
            P_ActivateInStasisCeiling(world, line);
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
        let ceiling_id = new_thinker::<ceiling_t>(world);
        P_AddThinker(world, ThinkerId::Ceiling(ceiling_id));
        {
            let level = &mut world.level;
            let ceilings = &mut world.thinkers.ceilings;
            let ceiling = &mut ceilings[ceiling_id];
            level.sectors[sec].specialdata = Some(ThinkerId::Ceiling(ceiling_id));
            ceiling.thinker.kind = ThinkerKind::Ceiling;
            ceiling.sector = sec;
            ceiling.crush = 0;

            let (ceilingheight, floorheight) = (
                level.sectors[sec].ceilingheight,
                level.sectors[sec].floorheight,
            );
            match ceilingtype {
                x if x == fastCrushAndRaise => {
                    ceiling.crush = 1;
                    ceiling.topheight = ceilingheight;
                    ceiling.bottomheight = floorheight + (8 * FRACUNIT);
                    ceiling.direction = -1;
                    ceiling.speed = CEILSPEED * 2;
                }
                x if x == silentCrushAndRaise || x == crushAndRaise => {
                    ceiling.crush = 1;
                    ceiling.topheight = ceilingheight;
                }
                x if x == lowerAndCrush || x == lowerToFloor => {
                    ceiling.bottomheight = floorheight;
                    if ceilingtype != lowerToFloor {
                        ceiling.bottomheight += 8 * FRACUNIT;
                    }
                    ceiling.direction = -1;
                    ceiling.speed = CEILSPEED;
                }
                x if x == raiseToHighest => {
                    ceiling.topheight = P_FindHighestCeilingSurrounding(level, sec);
                    ceiling.direction = 1;
                    ceiling.speed = CEILSPEED;
                }
                _ => {}
            }

            ceiling.tag = level.sectors[sec].tag as i32;
            ceiling.kind = ceilingtype;
        }
        P_AddActiveCeiling(world, ceiling_id);
    }
    rtn
}

/// Register `c` in the first available slot of `activeceilings`.
///
/// If all [`MAXCEILINGS`] slots are occupied the function returns silently,
/// discarding the ceiling — matching the C behaviour which has no overflow
/// guard. The ceiling's `sector` must belong to the loaded level; it stays
/// registered until `P_RemoveActiveCeiling` or the next level load clears
/// the table.
///
/// Corresponds to `P_AddActiveCeiling` in `p_ceilng.c`.
pub fn P_AddActiveCeiling(world: &mut World, c: CeilingId) {
    for slot in world.active.ceilings.activeceilings.iter_mut() {
        if slot.is_none() {
            *slot = Some(c);
            return;
        }
    }
}

/// Unlink and schedule deallocation of the active ceiling `c`.
///
/// Clears `sector->specialdata`, calls [`P_RemoveThinker`] (which marks the
/// thinker for deferred freeing), and clears the matching slot in
/// `activeceilings`.  Does nothing if `c` is not found in the table.
///
/// Corresponds to `P_RemoveActiveCeiling` in `p_ceilng.c`.
pub fn P_RemoveActiveCeiling(world: &mut World, c: CeilingId) {
    let World { level, active, .. } = world;
    for slot in active.ceilings.activeceilings.iter_mut() {
        if *slot == Some(c) {
            let ceilings = &mut world.thinkers.ceilings;
            let ceiling = &mut ceilings[c];
            level.sectors[ceiling.sector].specialdata = None;
            P_RemoveThinker(&mut ceiling.thinker);
            *slot = None;
            break;
        }
    }
}

/// Restart any in-stasis ceilings whose tag matches `line->tag`.
///
/// A ceiling is in stasis when `direction == 0`.  This function restores
/// `direction` from `olddirection` and sets the thinker kind back to
/// [`ThinkerKind::Ceiling`] so `T_MoveCeiling` runs again.
///
/// Called by [`EV_DoCeiling`] for crusher types, and corresponds to
/// `P_ActivateInStasisCeiling` in `p_ceilng.c`.
pub fn P_ActivateInStasisCeiling(world: &mut World, line: &line_t) {
    let ceilings = &mut world.thinkers.ceilings;
    for ceiling_id in world.active.ceilings.activeceilings.into_iter().flatten() {
        let ceiling = &mut ceilings[ceiling_id];
        if ceiling.tag == line.tag as i32 && ceiling.direction == 0 {
            ceiling.direction = ceiling.olddirection;
            ceiling.thinker.kind = ThinkerKind::Ceiling;
        }
    }
}

/// Stop all active crusher ceilings whose tag matches `line->tag`.
///
/// Each matching ceiling has its `direction` saved to `olddirection`, its
/// `direction` set to `0` (stasis), and its thinker kind set to
/// [`ThinkerKind::CeilingInStasis`] so `T_MoveCeiling` is skipped each tic.  The ceiling is not removed from
/// `activeceilings`; it can be restarted by [`P_ActivateInStasisCeiling`].
///
/// Returns `1` if at least one ceiling was stopped, `0` otherwise.
///
/// Corresponds to `EV_CeilingCrushStop` in `p_ceilng.c`.
pub fn EV_CeilingCrushStop(world: &mut World, line: &line_t) -> i32 {
    let mut rtn: i32 = 0;
    let ceilings = &mut world.thinkers.ceilings;
    for ceiling_id in world.active.ceilings.activeceilings.into_iter().flatten() {
        let ceiling = &mut ceilings[ceiling_id];
        if ceiling.tag == line.tag as i32 && ceiling.direction != 0 {
            ceiling.olddirection = ceiling.direction;
            ceiling.thinker.kind = ThinkerKind::CeilingInStasis;
            ceiling.direction = 0;
            rtn = 1;
        }
    }
    rtn
}
