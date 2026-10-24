//! Rust port of vendor/doomgeneric/p_enemy.c.
//!
//! Implements monster AI, movement, and all action-pointer functions (`A_*`)
//! that the state machine invokes via function pointers stored in `info.rs`.
//! This covers monster pathfinding (`P_NewChaseDir`, `P_Move`, `P_TryWalk`),
//! target acquisition (`P_LookForPlayers`, `P_NoiseAlert`), melee/missile
//! range checks, individual monster attacks, boss-death special actions, and
//! the icon-of-sin (brain) spawn logic.
//!
//! Rust-vs-C differences: `dirtype_t` is a plain `i32` alias rather than
//! a C enum; game-mode/version/skill constants are local `i32` consts
//! rather than imported enums; `Boolean` replaces the C `boolean` typedef
//! throughout; map objects are named by [`MobjId`] handles where C passes
//! `mobj_t *`, and each action resolves its actor's handle once on entry.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::{engine::sound::info::Sfx, types::Boolean};
/// Binary-angle measurement type (`u32`); a full circle is `2^32` units.
type angle_t = u32;
/// State index type mirroring C `statenum_t`; indexes the `states` table in `info.rs`.
type statenum_t = i32;
/// Map-object type identifier; mirrors C `mobjtype_t` (index into `mobjinfo` table).
type mobjtype_t = i32;

/// Returns the absolute value of `x`; local substitute for `stdlib.h` `abs` used in `p_enemy.c`.
#[inline]
fn abs(x: i32) -> i32 {
    if x < 0 { -x } else { x }
}

use crate::{
    engine::{
        doom::{
            main_loop::gametic,
            player::{MAXPLAYERS, PlayerId, PspdefT},
            stat::{gamemode, gameversion},
        },
        map::{LineId, SectorId, VertexId, line_t, slopetype_t},
        math::{
            fixed::{FRACUNIT, FixedMul, fixed_t},
            tables::{ANG90, ANG180, ANG270, ANGLETOFINESHIFT, finecosine, finesine},
        },
        misc::random::P_Random,
        play::{
            info::{
                MF_AMBUSH, MF_CORPSE, MF_FLOAT, MF_INFLOAT, MF_JUSTATTACKED, MF_JUSTHIT, MF_SHADOW,
                MF_SHOOTABLE, MF_SKULLFLY, MF_SOLID, MOBJINFO, MT_ARACHPLAZ, MT_BABY,
                MT_BOSSTARGET, MT_BRUISER, MT_BRUISERSHOT, MT_CYBORG, MT_FATSHOT, MT_FATSO,
                MT_FIRE, MT_HEAD, MT_HEADSHOT, MT_KNIGHT, MT_PAIN, MT_ROCKET, MT_SERGEANT,
                MT_SHADOWS, MT_SKULL, MT_SMOKE, MT_SPAWNFIRE, MT_SPAWNSHOT, MT_SPIDER, MT_TRACER,
                MT_TROOP, MT_TROOPSHOT, MT_UNDEAD, MT_VILE, MobjInfo, S_BRAINEXPLODE1, S_NULL,
                S_VILE_HEAL1,
            },
            inter::P_DamageMobj,
            map::{
                MAP, P_AimLineAttack, P_CheckPosition, P_LineAttack, P_RadiusAttack,
                P_TeleportMove, P_TryMove, floatok, tmfloorz,
            },
            maputl::{
                MAPBLOCKSHIFT, P_AproxDistance, P_BlockThingsIterator, P_LineOpening,
                P_SetThingPosition, P_UnsetThingPosition, openrange,
            },
            mobj::{
                MobjId, P_RemoveMobj, P_SetMobjState, P_SpawnMissile, P_SpawnMobj, P_SpawnPuff,
                P_SubstNullMobj, mobj_or_null, mobj_t,
            },
            setup::{LEVEL, LevelState, blockmap_origin},
            sight::P_CheckSight,
            switch::P_UseSpecialLine,
            tick::{ThinkerId, ThinkerKind, thinkers},
        },
        render_3d::main::{R_PointToAngle2, VALIDCOUNT},
        sound::{S_StartNonPositionalSound, S_StartSound},
        wad::format::LinedefFlag,
    },
    i_error,
};

/// Maximum melee attack range in fixed-point units (64 map units); from `p_local.h`.
const MELEERANGE: i32 = 64 * FRACUNIT;
/// Maximum missile attack range in fixed-point units (2048 map units); from `p_local.h`.
const MISSILERANGE: i32 = 32 * 64 * FRACUNIT;
/// Vertical speed at which floating monsters adjust altitude each tic (4 units/tic).
const FLOATSPEED: i32 = FRACUNIT * 4;
/// Maximum radius of any map object (32 units), used as a blockmap search margin.
const MAXRADIUS: i32 = 32 * FRACUNIT;

/// Eight-direction movement type used by the monster pathfinding code; mirrors C `dirtype_t`.
type dirtype_t = i32;
/// East cardinal direction index (positive X axis).
const DI_EAST: i32 = 0;
/// Northeast diagonal direction index.
const DI_NORTHEAST: i32 = 1;
/// North cardinal direction index (positive Y axis).
const DI_NORTH: i32 = 2;
/// Northwest diagonal direction index.
const DI_NORTHWEST: i32 = 3;
/// West cardinal direction index (negative X axis).
const DI_WEST: i32 = 4;
/// Southwest diagonal direction index.
const DI_SOUTHWEST: i32 = 5;
/// South cardinal direction index (negative Y axis).
const DI_SOUTH: i32 = 6;
/// Southeast diagonal direction index.
const DI_SOUTHEAST: i32 = 7;
/// Sentinel value meaning the monster has no current movement direction.
const DI_NODIR: i32 = 8;
/// Total number of direction values including `DI_NODIR`.
#[allow(dead_code)]
const NUMDIRS: i32 = 9;

/// Minimum game version that introduced Ultimate Doom episode logic; mirrors `exe_ultimate` from `doomfeatures.h`.
const exe_ultimate: i32 = 6;

/// Game mode value for Doom II (commercial); mirrors `commercial` from `doomdef.h`.
const commercial: i32 = 2;

/// Skill level index for Nightmare difficulty; mirrors `sk_nightmare` from `doomdef.h`.
const sk_nightmare: i32 = 4;
/// Skill level index for Easy (Hey Not Too Rough) difficulty; mirrors `sk_easy` from `doomdef.h`.
const sk_easy: i32 = 1;

/// `EV_DoFloor` floor type: lower floor to the lowest adjacent floor; mirrors `lowerFloorToLowest`.
const lowerFloorToLowest: i32 = 5;
/// `EV_DoFloor` floor type: raise floor to nearest texture height; mirrors `raiseToTexture`.
const raiseToTexture: i32 = 8;
/// `EV_DoDoor` door type: blaze-open (fast open); mirrors `vld_blazeOpen`.
const vld_blazeOpen: i32 = 5;
/// `EV_DoDoor` door type: normal open; mirrors `vld_open`.
const vld_open: i32 = 0;

/// `mobjtype_t` index for the player map object; mirrors `MT_PLAYER` from `info.h`.
const MT_PLAYER: i32 = 0;
/// Angular spread (ANG90/8) used between Mancubus fire balls; mirrors `FATSPREAD` from `p_enemy.c`.
pub const FATSPREAD: i32 = ANG90 as i32 / 8;
/// Lost Soul charge speed in fixed-point units per tic (20 map units/tic); mirrors `SKULLSPEED`.
pub const SKULLSPEED: i32 = 20 * FRACUNIT;

use crate::engine::{
    doom::main::fastparm,
    game::{G_ExitLevel, gameepisode, gamemap, gameskill, netgame, playeringame},
    global::Global,
    play::{doors::EV_DoDoor, floor::EV_DoFloor, pspr::A_ReFire},
};

/// Lookup table mapping each of the eight directions to its 180-degree opposite, with
/// `DI_NODIR` mapping to `DI_NODIR`; indexed by `dirtype_t` in `P_NewChaseDir`.
pub static opposite: [dirtype_t; 9] = [
    DI_WEST,
    DI_SOUTHWEST,
    DI_SOUTH,
    DI_SOUTHEAST,
    DI_EAST,
    DI_NORTHEAST,
    DI_NORTH,
    DI_NORTHWEST,
    DI_NODIR,
];
/// Lookup table mapping the two-bit index `(deltay<0)<<1 | (deltax>0)` to a diagonal
/// `dirtype_t`; used by `P_NewChaseDir` to prefer diagonal movement toward the target.
pub static diags: [dirtype_t; 4] = [DI_NORTHWEST, DI_NORTHEAST, DI_SOUTHWEST, DI_SOUTHEAST];
/// Monster AI scratch state (`p_enemy.c` globals).
pub struct EnemyState {
    /// The monster or player whose noise triggered the current `P_RecursiveSound` traversal;
    /// written by `P_NoiseAlert`, read by `P_RecursiveSound` to stamp each sector.
    pub soundtarget: Option<MobjId>,
    /// The corpse found by `PIT_VileCheck` for the Arch-vile to resurrect.
    pub corpsehit: Option<MobjId>,
    /// The Arch-vile currently searching for a corpse (`A_VileChase`).
    pub vileobj: Option<MobjId>,
    /// X coordinate the Arch-vile is about to move to; corpses are searched
    /// around it.
    pub viletryx: fixed_t,
    /// Y coordinate the Arch-vile is about to move to.
    pub viletryy: fixed_t,
    /// Boss-brain spawn targets collected by `A_BrainAwake`.
    pub braintargets: [Option<MobjId>; 32],
    /// Number of valid entries in `braintargets`.
    pub numbraintargets: i32,
    /// Index of the next spawn target `A_BrainSpit` aims at.
    pub braintargeton: i32,
    /// Toggled on each `A_BrainSpit` so easy skills spit every other time
    /// (static local `easy` in C).
    brainspit_easy: i32,
}

impl EnemyState {
    /// The state at program start: no noise, no corpse search, no brain.
    pub const fn new() -> Self {
        Self {
            soundtarget: None,
            corpsehit: None,
            vileobj: None,
            viletryx: 0,
            viletryy: 0,
            braintargets: [None; 32],
            numbraintargets: 0,
            braintargeton: 0,
            brainspit_easy: 0,
        }
    }
}

impl Default for EnemyState {
    fn default() -> Self {
        Self::new()
    }
}

/// Monster AI scratch state.
pub static ENEMY: Global<EnemyState> = Global::new(EnemyState::new());
/// Recursively floods sound through adjacent sectors, stamping each with `soundtarget`.
///
/// Traversal stops at sectors already visited this frame (`validcount`) and at
/// two-sided linedefs with the `ML_SOUNDBLOCK` flag - the block flag allows one
/// crossing (incrementing `soundblocks` from 0 to 1) but never two, so sound
/// cannot pass through two consecutive blocking walls.
///
/// Called by `P_NoiseAlert`, which borrows the level for the whole flood,
/// and recursively by itself.
pub fn P_RecursiveSound(level: &mut LevelState, sec: SectorId, soundblocks: i32) {
    let validcount = VALIDCOUNT.get();
    {
        let sector = &mut level.sectors[sec];
        if sector.validcount == validcount && sector.soundtraversed <= soundblocks + 1 {
            return;
        }
        sector.validcount = validcount;
        sector.soundtraversed = soundblocks + 1;
        sector.soundtarget = ENEMY.borrow().soundtarget;
    }
    let mut i = 0;
    while i < level.sectors[sec].linecount {
        let check = level.sector_lines(sec)[i as usize];
        let line = level.lines[check];
        if line.flags as i32 & LinedefFlag::TWOSIDED as i32 != 0 {
            P_LineOpening(level, check);
            if openrange() > 0 {
                let other = if level.sides[line.sidenum[0] as usize].sector == sec {
                    level.sides[line.sidenum[1] as usize].sector
                } else {
                    level.sides[line.sidenum[0] as usize].sector
                };
                if line.flags as i32 & LinedefFlag::SOUNDBLOCK as i32 != 0 {
                    if soundblocks == 0 {
                        P_RecursiveSound(level, other, 1);
                    }
                } else {
                    P_RecursiveSound(level, other, soundblocks);
                }
            }
        }
        i += 1;
    }
}
/// Alerts monsters in earshot that a target (typically the player) has made noise.
///
/// Sets the global `soundtarget`, increments `validcount` to mark a new traversal
/// frame, then calls `P_RecursiveSound` starting from the sector containing `emmiter`.
/// Called by `p_inter.rs` and `p_map.rs` whenever a shot, explosion, or door fires.
///
/// # Safety
///
/// `target` and `emmiter` must both name map objects of the level. `emmiter`
/// must have a valid `subsector` with a valid `sector`.
pub unsafe fn P_NoiseAlert(target: MobjId, emmiter: MobjId) {
    ENEMY.borrow_mut().soundtarget = Some(target);
    VALIDCOUNT.set(VALIDCOUNT.get() + 1);
    let mut level = LEVEL.borrow_mut();
    let sector = level.subsectors[(*emmiter.ptr()).subsector].sector;
    P_RecursiveSound(&mut level, sector, 0);
}
/// The target of `actor`, which C dereferences without a null check: the
/// callers have checked it, or C would crash too.
///
/// # Safety
///
/// `actor` must point at a map object.
///
/// # Panics
///
/// If `actor` has no target.
#[track_caller]
unsafe fn target_of(actor: *mut mobj_t) -> MobjId {
    (*actor).target.expect("actor has no target")
}
/// Returns `TRUE` when `actor`'s target is within melee striking range and line of sight.
///
/// Range test: approximate distance must be less than `MELEERANGE - 20 + target.radius`.
/// Returns `FALSE` immediately if `actor->target` is `None`, if the distance check fails,
/// or if `P_CheckSight` reports no clear line of sight.
///
/// # Safety
///
/// `actor_id` must name a map object of the level; its target, if any, must
/// have a valid `info` pointer.
pub unsafe fn P_CheckMeleeRange(actor_id: MobjId) -> Boolean {
    let actor = actor_id.ptr();
    let Some(target) = (*actor).target else {
        return Boolean::FALSE;
    };
    let pl: *mut mobj_t = target.ptr();
    let dist: fixed_t = P_AproxDistance((*pl).x - (*actor).x, (*pl).y - (*actor).y);
    if dist >= MELEERANGE - 20 * FRACUNIT + (*(*pl).info).radius {
        return Boolean::FALSE;
    }
    if P_CheckSight(actor_id, target_of(actor)) == 0 {
        return Boolean::FALSE;
    }
    Boolean::TRUE
}
/// Returns `TRUE` when `actor` is permitted to fire a missile at its current target.
///
/// Checks line of sight first; immediately grants permission if the actor was just hit
/// (`MF_JUSTHIT`, clears the flag). Blocks attacks during `reactiontime` countdown.
/// Distance is scaled to a 0-200 range and compared against a random threshold so
/// closer targets are more reliably hit. Per-monster special cases:
/// - Archvile (`MT_VILE`): blocked beyond 14*64 units.
/// - Revenant (`MT_UNDEAD`): blocked below 196 units (prefers melee); range halved.
/// - Cyberdemon, Spider Mastermind, Lost Soul: effective range halved and capped at 160
///   (Cyberdemon) or 200 (others).
///
/// # Safety
///
/// `actor_id` must name a map object with a target and a valid `info`.
pub unsafe fn P_CheckMissileRange(actor_id: MobjId) -> Boolean {
    let actor = actor_id.ptr();
    let mut dist: fixed_t;

    if P_CheckSight(actor_id, target_of(actor)) == 0 {
        return Boolean::FALSE;
    }
    if (*actor).flags & MF_JUSTHIT != 0 {
        (*actor).flags &= !(MF_JUSTHIT);
        return Boolean::TRUE;
    }
    if (*actor).reactiontime != 0 {
        return Boolean::FALSE;
    }
    let target = target_of(actor).ptr();
    dist = (P_AproxDistance((*actor).x - (*target).x, (*actor).y - (*target).y) as i32
        - 64 * FRACUNIT) as fixed_t;
    if (*(*actor).info).meleestate == 0 {
        dist -= 128 * FRACUNIT;
    }
    dist >>= 16;
    if (*actor).kind as u32 == MT_VILE as u32 && dist > 14 * 64 {
        return Boolean::FALSE;
    }
    if (*actor).kind as u32 == MT_UNDEAD as u32 {
        if dist < 196 {
            return Boolean::FALSE;
        }
        dist >>= 1;
    }
    if (*actor).kind as u32 == MT_CYBORG as u32
        || (*actor).kind as u32 == MT_SPIDER as u32
        || (*actor).kind as u32 == MT_SKULL as u32
    {
        dist >>= 1;
    }
    if dist > 200 {
        dist = 200 as fixed_t;
    }
    if (*actor).kind as u32 == MT_CYBORG as u32 && dist > 160 {
        dist = 160 as fixed_t;
    }
    if P_Random() < dist {
        return Boolean::FALSE;
    }
    Boolean::TRUE
}
/// Per-direction X-axis speed multiplier table indexed by `dirtype_t` (0=East..7=Southeast).
/// Each entry is a fixed-point unit (FRACUNIT or ~0.718*FRACUNIT for diagonals).
pub static xspeed: [fixed_t; 8] = [FRACUNIT, 47000, 0, -47000, -FRACUNIT, -47000, 0, 47000];
/// Per-direction Y-axis speed multiplier table indexed by `dirtype_t` (0=East..7=Southeast).
/// Each entry is a fixed-point unit (FRACUNIT or ~0.718*FRACUNIT for diagonals).
pub static yspeed: [fixed_t; 8] = [0, 47000, FRACUNIT, 47000, 0, -47000, -FRACUNIT, -47000];
/// Attempts to advance `actor` one step in its current `movedir`.
///
/// Returns `FALSE` if `movedir` is `DI_NODIR`. Computes the target position using
/// `xspeed`/`yspeed` scaled by the actor's `info->speed`, then calls `P_TryMove`.
/// On success, clears `MF_INFLOAT` and snaps non-floating actors to the floor.
/// On failure:
/// - If the actor has `MF_FLOAT` and `floatok` is set, adjusts Z by `FLOATSPEED`
///   toward `tmfloorz` and sets `MF_INFLOAT`, returning `TRUE`.
/// - Otherwise iterates `spechit` in reverse, calling `P_UseSpecialLine` on each;
///   returns `TRUE` if any special line was successfully activated.
///
/// # Safety
///
/// `actor_id` must name a map object with valid `info`. All globals
/// `floatok`, `tmfloorz`, `numspechit`, and `spechit` must be consistent with the
/// most recent `P_TryMove` call.
pub unsafe fn P_Move(actor_id: MobjId) -> Boolean {
    let actor = actor_id.ptr();
    let mut ld: LineId;

    let mut good: Boolean;

    if (*actor).movedir == DI_NODIR {
        return Boolean::FALSE;
    }
    if (*actor).movedir as u32 >= 8 {
        i_error!("Weird actor->movedir!");
    }
    let tryx: fixed_t =
        (*actor).x + (*(*actor).info).speed as fixed_t * xspeed[(*actor).movedir as usize];
    let tryy: fixed_t =
        (*actor).y + (*(*actor).info).speed as fixed_t * yspeed[(*actor).movedir as usize];
    let try_ok: Boolean = Boolean::from_raw(P_TryMove(actor_id, tryx, tryy));
    if try_ok.is_false() {
        if (*actor).flags & MF_FLOAT != 0 && floatok() != 0 {
            if (*actor).z < tmfloorz() {
                (*actor).z += FLOATSPEED;
            } else {
                (*actor).z -= FLOATSPEED;
            }
            (*actor).flags |= MF_INFLOAT;
            return Boolean::TRUE;
        }
        if MAP.borrow().numspechit == 0 {
            return Boolean::FALSE;
        }
        (*actor).movedir = DI_NODIR;
        good = Boolean::FALSE;
        // A used line can move things and refill the list, so the count is
        // re-read before every pop, as in C.
        loop {
            ld = {
                let mut map = MAP.borrow_mut();
                let c2rust_fresh0 = map.numspechit;
                map.numspechit -= 1;
                if c2rust_fresh0 == 0 {
                    break;
                }
                map.spechit[map.numspechit as usize]
            };
            if P_UseSpecialLine(actor_id, ld, 0) != 0 {
                good = Boolean::TRUE;
            }
        }
        return good;
    } else {
        (*actor).flags &= !(MF_INFLOAT);
    }
    if (*actor).flags & MF_FLOAT == 0 {
        (*actor).z = (*actor).floorz;
    }
    Boolean::TRUE
}
/// Attempts to move `actor` in its current direction; on success randomises `movecount`.
///
/// Calls `P_Move`; returns `FALSE` immediately if blocked. On success sets
/// `actor->movecount` to a random value in 0..=15, giving the monster a random
/// number of tics before it reconsiders its direction.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn P_TryWalk(actor_id: MobjId) -> Boolean {
    if P_Move(actor_id).is_false() {
        return Boolean::FALSE;
    }
    (*actor_id.ptr()).movecount = P_Random() & 15;
    Boolean::TRUE
}
/// Selects a new movement direction for `actor` based on the vector to its target.
///
/// Algorithm (mirrors `P_NewChaseDir` in `p_enemy.c`):
/// 1. Compute axis-aligned directions toward the target (`d[1]`, `d[2]`).
/// 2. Prefer the diagonal that combines both axes; try it first if not a U-turn.
/// 3. With 20% probability (or if |dy|>|dx|) swap x/y preference.
/// 4. Filter out the reverse direction and try each axis individually.
/// 5. Fall back to continuing the previous direction.
/// 6. Last resort: sweep all eight directions (randomly forward or backward).
/// 7. If still blocked, set `movedir = DI_NODIR`.
///
/// Panics (via `i_error!`) if `actor->target` is `None`.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn P_NewChaseDir(actor_id: MobjId) {
    let actor = actor_id.ptr();
    let mut d: [dirtype_t; 3] = [DI_EAST; 3];
    let mut tdir: i32;

    if (*actor).target.is_none() {
        i_error!("P_NewChaseDir: called with no target");
    }
    let olddir: dirtype_t = (*actor).movedir as dirtype_t;
    let turnaround: dirtype_t = opposite[olddir as usize];
    let target = target_of(actor).ptr();
    let deltax: fixed_t = (*target).x - (*actor).x;
    let deltay: fixed_t = (*target).y - (*actor).y;
    if deltax > 10 * FRACUNIT {
        d[1] = DI_EAST;
    } else if deltax < -10 * FRACUNIT {
        d[1] = DI_WEST;
    } else {
        d[1] = DI_NODIR;
    }
    if deltay < -10 * FRACUNIT {
        d[2] = DI_SOUTH;
    } else if deltay > 10 * FRACUNIT {
        d[2] = DI_NORTH;
    } else {
        d[2] = DI_NODIR;
    }
    if d[1] as u32 != DI_NODIR as u32 && d[2] as u32 != DI_NODIR as u32 {
        (*actor).movedir =
            diags[((((deltay < 0) as i32) << 1) + (deltax > 0) as i32) as usize] as i32;
        if (*actor).movedir != turnaround as i32 && P_TryWalk(actor_id).is_truthy() {
            return;
        }
    }
    if P_Random() > 200 || (deltay as i32).abs() > (deltax as i32).abs() {
        tdir = d[1];
        d[1] = d[2];
        d[2] = tdir as dirtype_t;
    }
    if d[1] as u32 == turnaround as u32 {
        d[1] = DI_NODIR;
    }
    if d[2] as u32 == turnaround as u32 {
        d[2] = DI_NODIR;
    }
    if d[1] as u32 != DI_NODIR as u32 {
        (*actor).movedir = d[1];
        if P_TryWalk(actor_id).is_truthy() {
            return;
        }
    }
    if d[2] as u32 != DI_NODIR as u32 {
        (*actor).movedir = d[2];
        if P_TryWalk(actor_id).is_truthy() {
            return;
        }
    }
    if olddir as u32 != DI_NODIR as u32 {
        (*actor).movedir = olddir as i32;
        if P_TryWalk(actor_id).is_truthy() {
            return;
        }
    }
    if P_Random() & 1 != 0 {
        tdir = DI_EAST;
        while tdir <= DI_SOUTHEAST {
            if tdir != turnaround as i32 {
                (*actor).movedir = tdir;
                if P_TryWalk(actor_id).is_truthy() {
                    return;
                }
            }
            tdir += 1;
        }
    } else {
        tdir = DI_SOUTHEAST;
        while tdir != DI_EAST - 1 {
            if tdir != turnaround as i32 {
                (*actor).movedir = tdir;
                if P_TryWalk(actor_id).is_truthy() {
                    return;
                }
            }
            tdir -= 1;
        }
    }
    if turnaround as u32 != DI_NODIR as u32 {
        (*actor).movedir = turnaround as i32;
        if P_TryWalk(actor_id).is_truthy() {
            return;
        }
    }
    (*actor).movedir = DI_NODIR;
}
/// Scans active players to find a visible target for `actor`; returns `TRUE` if one is found.
///
/// Iterates up to four player slots starting from `actor->lastlook`, cycling with `& 3`.
/// Stops after examining two live players or looping back to the starting slot.
/// Skips dead players and players with no line of sight.
/// If `allaround` is `FALSE`, also skips players that are more than 90 degrees behind
/// the actor (angle difference in ANG90..ANG270) unless they are within melee range.
/// On success sets `actor->target` to the found player's map object.
///
/// # Safety
///
/// `actor_id` must name a map object of the level. The global `players` array and
/// `playeringame` flags must be consistent.
pub unsafe fn P_LookForPlayers(actor_id: MobjId, allaround: Boolean) -> Boolean {
    let actor = actor_id.ptr();
    let mut c: i32;

    let mut an: angle_t;

    let mut dist: fixed_t;

    c = 0;
    let stop: i32 = ((*actor).lastlook - 1) & 3;
    loop {
        's_20: {
            if playeringame((*actor).lastlook as usize) != 0 {
                let c2rust_fresh1 = c;
                c += 1;
                if c2rust_fresh1 == 2 || (*actor).lastlook == stop {
                    return Boolean::FALSE;
                }
                let player = PlayerId::new((*actor).lastlook as usize);
                if player.read(|player| player.health) > 0 {
                    let player_mo = player.mo();
                    let sight = P_CheckSight(actor_id, player_mo);
                    if sight != 0 {
                        if allaround.is_false() {
                            let player_mo = player_mo.ptr();
                            an = R_PointToAngle2(
                                (*actor).x,
                                (*actor).y,
                                (*player_mo).x,
                                (*player_mo).y,
                            )
                            .wrapping_sub((*actor).angle);
                            if an > ANG90 as angle_t && an < ANG270 {
                                dist = P_AproxDistance(
                                    (*player_mo).x - (*actor).x,
                                    (*player_mo).y - (*actor).y,
                                );
                                if dist > MELEERANGE {
                                    break 's_20;
                                }
                            }
                        }
                        (*actor).target = Some(player_mo);
                        return Boolean::TRUE;
                    }
                }
            }
        }
        (*actor).lastlook = ((*actor).lastlook + 1) & 3;
    }
}
/// The synthetic line record the boss deaths pass to the tag-based
/// specials, which read only its `tag` (C's zeroed `line_t junk`). It is not
/// part of the level, so its ids are never followed.
const JUNK_LINE: line_t = line_t {
    v1: VertexId::new(0),
    v2: VertexId::new(0),
    dx: 0,
    dy: 0,
    flags: 0,
    special: 0,
    tag: 0,
    sidenum: [0; 2],
    bbox: [0; 4],
    slopetype: slopetype_t::ST_HORIZONTAL,
    frontsector: None,
    backsector: None,
    validcount: 0,
    specialdata: None,
};
/// `true` if another map object of `mo`'s type is still alive: the thinker
/// walk shared by `A_KeenDie` and `A_BossDeath` (C compares each mobj
/// thinker's type and health).
///
/// # Safety
///
/// `mo_id` must name a map object of the level.
unsafe fn another_alive_of_same_kind(mo_id: MobjId) -> bool {
    let kind = (*mo_id.ptr()).kind;
    thinkers().any(|thinker| {
        let ThinkerId::Mobj(other_id) = thinker else {
            return false;
        };
        if thinker.kind() != ThinkerKind::Mobj || other_id == mo_id {
            return false;
        }
        let other = other_id.ptr();
        (*other).kind as u32 == kind as u32 && (*other).health > 0
    })
}
/// Action function for Commander Keen's death (Doom II map 32 special).
///
/// Calls `A_Fall` to make the corpse non-solid, then scans all thinkers; if any other
/// Keen of the same type is still alive the function returns early. When the last Keen
/// dies it synthesises a `line_t` with `tag = 666` and calls `EV_DoDoor` with
/// `vld_open` to open the tagged door, allowing exit from the secret level.
///
/// # Safety
///
/// `mo_id` must name a map object of the level, and the thinker list must be
/// consistent. Called through the state-machine action pointer.
pub unsafe fn A_KeenDie(mo_id: MobjId) {
    let mut junk: line_t = JUNK_LINE;
    A_Fall(mo_id);
    if another_alive_of_same_kind(mo_id) {
        return;
    }
    junk.tag = 666;
    EV_DoDoor(&junk, vld_open);
}
/// State-machine action: monster idle look, waiting to spot a player.
///
/// Resets `threshold` to 0 so any hit will wake it. First checks the sector's
/// `soundtarget`; if a shootable target is present the monster wakes immediately
/// (ambush monsters additionally require line of sight). Falls back to
/// `P_LookForPlayers`. On waking, plays the monster's `seesound` (randomised for
/// Former Human and Demon variants) and transitions to `seestate`.
///
/// # Safety
///
/// `actor_id` must name a map object with valid `subsector` and `info`. Called
/// through the state-machine action pointer.
pub unsafe fn A_Look(actor_id: MobjId) {
    let actor = actor_id.ptr();
    (*actor).threshold = 0;
    let targ: Option<MobjId> = LEVEL
        .borrow()
        .subsector_sector((*actor).subsector)
        .soundtarget;
    '_seeyou: {
        if let Some(targ) = targ
            && (*targ.ptr()).flags & MF_SHOOTABLE != 0
        {
            (*actor).target = Some(targ);
            if (*actor).flags & MF_AMBUSH != 0 {
                if P_CheckSight(actor_id, target_of(actor)) != 0 {
                    break '_seeyou;
                }
            } else {
                break '_seeyou;
            }
        }
        if P_LookForPlayers(actor_id, Boolean::FALSE).is_false() {
            return;
        }
    }
    if (*(*actor).info).seesound != Sfx::None {
        let sound = match (*(*actor).info).seesound {
            Sfx::Posit1 | Sfx::Posit2 | Sfx::Posit3 => Sfx::Posit1 as i32 + P_Random() % 3,
            Sfx::Bgsit1 | Sfx::Bgsit2 => Sfx::Bgsit1 as i32 + P_Random() % 2,
            s => s as i32,
        };
        if (*actor).kind as u32 == MT_SPIDER as u32 || (*actor).kind as u32 == MT_CYBORG as u32 {
            S_StartNonPositionalSound(sound);
        } else {
            S_StartSound(actor_id, sound);
        }
    }
    P_SetMobjState(actor_id, (*(*actor).info).seestate as statenum_t);
}
/// State-machine action: monster actively chases its target and attacks when able.
///
/// Each call:
/// 1. Decrements `reactiontime` if non-zero (initial delay after spawning).
/// 2. Decrements `threshold` toward zero (persistence on current target).
/// 3. Snaps `angle` toward the current `movedir` (±ANG90/2 per tic).
/// 4. If the target is gone/dead, searches for a new player; falls back to `spawnstate`.
/// 5. If `MF_JUSTATTACKED` is set, clears it and redirects movement (except Nightmare).
/// 6. Triggers melee attack if in range (`meleestate`).
/// 7. Triggers missile attack if in range and the cooldown permits (`missilestate`).
/// 8. In netgame, may switch targets if the current one is out of sight.
/// 9. Advances movement; calls `P_NewChaseDir` if blocked or `movecount` expired.
/// 10. Occasionally plays `activesound` (random < 3).
///
/// # Safety
///
/// `actor_id` must name a map object with valid `info`. All referenced
/// globals (`gameskill`, `fastparm`, `netgame`) must be initialised.
pub unsafe fn A_Chase(actor_id: MobjId) {
    let actor = actor_id.ptr();
    let delta: i32;

    if (*actor).reactiontime != 0 {
        (*actor).reactiontime -= 1;
    }
    if (*actor).threshold != 0 {
        if (*actor).target.is_none() || (*target_of(actor).ptr()).health <= 0 {
            (*actor).threshold = 0;
        } else {
            (*actor).threshold -= 1;
        }
    }
    if (*actor).movedir < 8 {
        (*actor).angle &= ((7) << 29) as angle_t;
        delta = (*actor)
            .angle
            .wrapping_sub(((*actor).movedir << 29) as angle_t) as i32;
        if delta > 0 {
            (*actor).angle = (*actor).angle.wrapping_sub((ANG90 / 2) as angle_t);
        } else if delta < 0 {
            (*actor).angle = (*actor).angle.wrapping_add((ANG90 / 2) as angle_t);
        }
    }
    if (*actor).target.is_none() || (*target_of(actor).ptr()).flags & MF_SHOOTABLE == 0 {
        if P_LookForPlayers(actor_id, Boolean::TRUE).is_truthy() {
            return;
        }
        P_SetMobjState(actor_id, (*(*actor).info).spawnstate as statenum_t);
        return;
    }
    if (*actor).flags & MF_JUSTATTACKED != 0 {
        (*actor).flags &= !(MF_JUSTATTACKED);
        if gameskill() != sk_nightmare && fastparm() == 0 {
            P_NewChaseDir(actor_id);
        }
        return;
    }
    if (*(*actor).info).meleestate != 0 && P_CheckMeleeRange(actor_id).is_truthy() {
        if (*(*actor).info).attacksound != Sfx::None {
            S_StartSound(actor_id, (*(*actor).info).attacksound as i32);
        }
        P_SetMobjState(actor_id, (*(*actor).info).meleestate as statenum_t);
        return;
    }
    if (*(*actor).info).missilestate != 0
        && !(gameskill() < sk_nightmare && fastparm() == 0 && (*actor).movecount != 0)
        && P_CheckMissileRange(actor_id).is_truthy()
    {
        P_SetMobjState(actor_id, (*(*actor).info).missilestate as statenum_t);
        (*actor).flags |= MF_JUSTATTACKED;
        return;
    }
    if netgame() != 0
        && (*actor).threshold == 0
        && P_CheckSight(actor_id, target_of(actor)) == 0
        && P_LookForPlayers(actor_id, Boolean::TRUE).is_truthy()
    {
        return;
    }
    (*actor).movecount -= 1;
    if (*actor).movecount < 0 || P_Move(actor_id).is_false() {
        P_NewChaseDir(actor_id);
    }
    if (*(*actor).info).activesound != Sfx::None && P_Random() < 3 {
        S_StartSound(actor_id, (*(*actor).info).activesound as i32);
    }
}
/// Turns `actor` to face its `target` and clears the `MF_AMBUSH` flag.
///
/// If the target has `MF_SHADOW` (partial invisibility), the facing angle is
/// perturbed by a random ±21-bit BAM value to simulate aim confusion.
/// Returns immediately if `actor->target` is null.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_FaceTarget(actor_id: MobjId) {
    let actor = actor_id.ptr();
    if (*actor).target.is_none() {
        return;
    }
    (*actor).flags &= !(MF_AMBUSH);
    (*actor).angle = R_PointToAngle2(
        (*actor).x,
        (*actor).y,
        (*target_of(actor).ptr()).x,
        (*target_of(actor).ptr()).y,
    );
    if (*target_of(actor).ptr()).flags & MF_SHADOW != 0 {
        (*actor).angle = (*actor)
            .angle
            .wrapping_add(((P_Random() - P_Random()) << 21) as angle_t);
    }
}
/// Attack action for the Former Human (Zombieman): single hitscan shot.
///
/// Faces the target, aims with `P_AimLineAttack`, plays `sfx_pistol`, then fires one
/// hitscan ray with horizontal spread of ±20 bits and damage of `(rnd%5+1)*3` (3-15).
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_PosAttack(actor_id: MobjId) {
    let actor = actor_id.ptr();
    let mut angle: i32;

    if (*actor).target.is_none() {
        return;
    }
    A_FaceTarget(actor_id);
    angle = (*actor).angle as i32;
    let slope: i32 = P_AimLineAttack(actor_id, angle as angle_t, MISSILERANGE) as i32;
    S_StartSound(actor_id, Sfx::Pistol as i32);
    angle = angle.wrapping_add((P_Random() - P_Random()) << 20);
    let damage: i32 = (P_Random() % 5 + 1) * 3;
    P_LineAttack(
        actor_id,
        angle as angle_t,
        MISSILERANGE,
        slope as fixed_t,
        damage,
    );
}
/// Attack action for the Shotgun Guy (Sergeant): three-pellet spread shot.
///
/// Plays `sfx_shotgn`, faces the target, aims once, then fires three independent
/// hitscan rays each with ±20-bit spread and `(rnd%5+1)*3` damage.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_SPosAttack(actor_id: MobjId) {
    let actor = actor_id.ptr();
    let mut i: i32;

    let mut angle: i32;

    let mut damage: i32;

    if (*actor).target.is_none() {
        return;
    }
    S_StartSound(actor_id, Sfx::Shotgn as i32);
    A_FaceTarget(actor_id);
    let bangle: i32 = (*actor).angle as i32;
    let slope: i32 = P_AimLineAttack(actor_id, bangle as angle_t, MISSILERANGE) as i32;
    i = 0;
    while i < 3 {
        angle = bangle.wrapping_add((P_Random() - P_Random()) << 20);
        damage = (P_Random() % 5 + 1) * 3;
        P_LineAttack(
            actor_id,
            angle as angle_t,
            MISSILERANGE,
            slope as fixed_t,
            damage,
        );
        i += 1;
    }
}
/// Attack action for the Heavy Weapon Dude (Chaingunner): single-pellet burst fire.
///
/// Plays `sfx_shotgn`, faces the target, aims once, then fires one hitscan ray with
/// ±20-bit spread and `(rnd%5+1)*3` damage. The state machine calls this repeatedly
/// each tic to simulate chaingun fire; `A_CPosRefire` decides when to stop.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_CPosAttack(actor_id: MobjId) {
    let actor = actor_id.ptr();
    if (*actor).target.is_none() {
        return;
    }
    S_StartSound(actor_id, Sfx::Shotgn as i32);
    A_FaceTarget(actor_id);
    let bangle: i32 = (*actor).angle as i32;
    let slope: i32 = P_AimLineAttack(actor_id, bangle as angle_t, MISSILERANGE) as i32;
    let angle: i32 = bangle.wrapping_add((P_Random() - P_Random()) << 20);
    let damage: i32 = (P_Random() % 5 + 1) * 3;
    P_LineAttack(
        actor_id,
        angle as angle_t,
        MISSILERANGE,
        slope as fixed_t,
        damage,
    );
}
/// Refire check for the Chaingunner: keeps firing unless the target is gone or hidden.
///
/// Faces the target. With a 40/256 chance returns early (keeps firing regardless).
/// Otherwise, if the target is null, dead, or out of sight, transitions back to
/// `seestate` to stop the burst.
///
/// # Safety
///
/// `actor_id` must name a map object with a valid `info`.
pub unsafe fn A_CPosRefire(actor_id: MobjId) {
    let actor = actor_id.ptr();
    A_FaceTarget(actor_id);
    if P_Random() < 40 {
        return;
    }
    if (*actor).target.is_none()
        || (*target_of(actor).ptr()).health <= 0
        || P_CheckSight(actor_id, target_of(actor)) == 0
    {
        P_SetMobjState(actor_id, (*(*actor).info).seestate as statenum_t);
    }
}
/// Refire check for the Spider Mastermind: keeps firing unless the target is gone or hidden.
///
/// Same logic as `A_CPosRefire` but with a lower 10/256 early-return chance, making the
/// Spider Mastermind more persistent.
///
/// # Safety
///
/// `actor_id` must name a map object with a valid `info`.
pub unsafe fn A_SpidRefire(actor_id: MobjId) {
    let actor = actor_id.ptr();
    A_FaceTarget(actor_id);
    if P_Random() < 10 {
        return;
    }
    if (*actor).target.is_none()
        || (*target_of(actor).ptr()).health <= 0
        || P_CheckSight(actor_id, target_of(actor)) == 0
    {
        P_SetMobjState(actor_id, (*(*actor).info).seestate as statenum_t);
    }
}
/// Attack action for the Arachnotron: launches one `MT_ARACHPLAZ` plasma ball.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_BspiAttack(actor_id: MobjId) {
    let actor = actor_id.ptr();
    if (*actor).target.is_none() {
        return;
    }
    A_FaceTarget(actor_id);
    P_SpawnMissile(actor_id, target_of(actor), MT_ARACHPLAZ);
}
/// Attack action for the Imp: claw swipe in melee range, fireball at distance.
///
/// If in melee range plays `sfx_claw` and deals `(rnd%8+1)*3` damage (3-24).
/// Otherwise launches a `MT_TROOPSHOT` fireball.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_TroopAttack(actor_id: MobjId) {
    let actor = actor_id.ptr();
    let damage: i32;

    if (*actor).target.is_none() {
        return;
    }
    A_FaceTarget(actor_id);
    if P_CheckMeleeRange(actor_id).is_truthy() {
        S_StartSound(actor_id, Sfx::Claw as i32);
        damage = (P_Random() % 8 + 1) * 3;
        P_DamageMobj(target_of(actor), Some(actor_id), Some(actor_id), damage);
        return;
    }
    P_SpawnMissile(actor_id, target_of(actor), MT_TROOPSHOT);
}
/// Attack action for the Demon (Sarg): melee-only bite dealing `(rnd%10+1)*4` damage (4-40).
///
/// Only damages the target when within melee range; no ranged fallback.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_SargAttack(actor_id: MobjId) {
    let actor = actor_id.ptr();
    let damage: i32;

    if (*actor).target.is_none() {
        return;
    }
    A_FaceTarget(actor_id);
    if P_CheckMeleeRange(actor_id).is_truthy() {
        damage = (P_Random() % 10 + 1) * 4;
        P_DamageMobj(target_of(actor), Some(actor_id), Some(actor_id), damage);
    }
}
/// Attack action for the Cacodemon: bite in melee range, fireball at distance.
///
/// Melee deals `(rnd%6+1)*10` damage (10-60). Ranged fires `MT_HEADSHOT`.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_HeadAttack(actor_id: MobjId) {
    let actor = actor_id.ptr();
    let damage: i32;

    if (*actor).target.is_none() {
        return;
    }
    A_FaceTarget(actor_id);
    if P_CheckMeleeRange(actor_id).is_truthy() {
        damage = (P_Random() % 6 + 1) * 10;
        P_DamageMobj(target_of(actor), Some(actor_id), Some(actor_id), damage);
        return;
    }
    P_SpawnMissile(actor_id, target_of(actor), MT_HEADSHOT);
}
/// Attack action for the Cyberdemon: launches one `MT_ROCKET`.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_CyberAttack(actor_id: MobjId) {
    let actor = actor_id.ptr();
    if (*actor).target.is_none() {
        return;
    }
    A_FaceTarget(actor_id);
    P_SpawnMissile(actor_id, target_of(actor), MT_ROCKET);
}
/// Attack action for the Baron of Hell / Hell Knight: claw in melee, plasma ball at distance.
///
/// Melee plays `sfx_claw` and deals `(rnd%8+1)*10` damage (10-80). Ranged fires
/// `MT_BRUISERSHOT`.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_BruisAttack(actor_id: MobjId) {
    let actor = actor_id.ptr();
    let damage: i32;

    if (*actor).target.is_none() {
        return;
    }
    if P_CheckMeleeRange(actor_id).is_truthy() {
        S_StartSound(actor_id, Sfx::Claw as i32);
        damage = (P_Random() % 8 + 1) * 10;
        P_DamageMobj(target_of(actor), Some(actor_id), Some(actor_id), damage);
        return;
    }
    P_SpawnMissile(actor_id, target_of(actor), MT_BRUISERSHOT);
}
/// Attack action for the Revenant: launches a homing `MT_TRACER` missile.
///
/// Temporarily raises the actor's Z by 16 units so the missile spawns at shoulder
/// height. After spawning, advances the missile one tic forward and stores the
/// current target in `tracer` so `A_Tracer` can home in.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_SkelMissile(actor_id: MobjId) {
    let actor = actor_id.ptr();
    if (*actor).target.is_none() {
        return;
    }
    A_FaceTarget(actor_id);
    (*actor).z += 16 * FRACUNIT;
    let mo: *mut mobj_t = P_SpawnMissile(actor_id, target_of(actor), MT_TRACER).ptr();
    (*actor).z -= 16 * FRACUNIT;
    (*mo).x += (*mo).momx;
    (*mo).y += (*mo).momy;
    (*mo).tracer = (*actor).target;
}
/// Maximum angular correction applied per active tic by `A_Tracer` (approx 11.25 degrees in BAM).
pub const TRACEANGLE: i32 = 0xc000000;
/// Per-tic homing update for the Revenant's tracer missile.
///
/// Only executes on tics where `gametic & 3 == 0` (every fourth tic). Each active tic:
/// 1. Spawns a smoke puff at the current position and a `MT_SMOKE` particle behind it.
/// 2. Steers the missile angle toward `tracer` by at most `TRACEANGLE` per tic, snapping
///    exactly when the correction would overshoot.
/// 3. Recomputes `momx`/`momy` from the new angle and the missile's `info->speed`.
/// 4. Adjusts `momz` by ±`FRACUNIT/8` to converge on `tracer->z + 40` units.
/// Returns immediately if `tracer` is null or dead.
///
/// # Safety
///
/// `actor_id` must name a map object with valid `info`; its `tracer`, if
/// any, may be stale (C reads a dangling pointer there too).
pub unsafe fn A_Tracer(actor_id: MobjId) {
    let actor = actor_id.ptr();
    let mut exact: angle_t;

    let mut dist: fixed_t;

    if gametic() & 3 != 0 {
        return;
    }
    P_SpawnPuff((*actor).x, (*actor).y, (*actor).z);
    let th: *mut mobj_t = P_SpawnMobj(
        (*actor).x - (*actor).momx,
        (*actor).y - (*actor).momy,
        (*actor).z,
        MT_SMOKE,
    )
    .ptr();
    (*th).momz = FRACUNIT as fixed_t;
    (*th).tics -= P_Random() & 3;
    if (*th).tics < 1 {
        (*th).tics = 1;
    }
    let dest: *mut mobj_t = mobj_or_null((*actor).tracer);
    if dest.is_null() || (*dest).health <= 0 {
        return;
    }
    exact = R_PointToAngle2((*actor).x, (*actor).y, (*dest).x, (*dest).y);
    if exact != (*actor).angle {
        if exact.wrapping_sub((*actor).angle) > 0x80000000 {
            (*actor).angle = (*actor).angle.wrapping_sub(TRACEANGLE as angle_t);
            if exact.wrapping_sub((*actor).angle) < 0x80000000 {
                (*actor).angle = exact;
            }
        } else {
            (*actor).angle = (*actor).angle.wrapping_add(TRACEANGLE as angle_t);
            if exact.wrapping_sub((*actor).angle) > 0x80000000 {
                (*actor).angle = exact;
            }
        }
    }
    exact = (*actor).angle >> ANGLETOFINESHIFT;
    (*actor).momx = FixedMul(
        (*(*actor).info).speed as fixed_t,
        *finecosine.0.add(exact as usize),
    );
    (*actor).momy = FixedMul((*(*actor).info).speed as fixed_t, finesine[exact as usize]);
    dist = P_AproxDistance((*dest).x - (*actor).x, (*dest).y - (*actor).y);
    dist = (dist as i32 / (*(*actor).info).speed) as fixed_t;
    if dist < 1 {
        dist = 1 as fixed_t;
    }
    let slope: fixed_t = ((*dest).z + 40 as fixed_t * FRACUNIT - (*actor).z) / dist;
    if slope < (*actor).momz {
        (*actor).momz -= FRACUNIT / 8;
    } else {
        (*actor).momz += FRACUNIT / 8;
    };
}
/// Revenant melee wind-up: faces the target and plays the whoosh sound.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_SkelWhoosh(actor_id: MobjId) {
    let actor = actor_id.ptr();
    if (*actor).target.is_none() {
        return;
    }
    A_FaceTarget(actor_id);
    S_StartSound(actor_id, Sfx::Skeswg as i32);
}
/// Revenant melee strike: deals `(rnd%10+1)*6` damage (6-60) when in melee range.
///
/// Plays `sfx_skepch` on a successful hit.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_SkelFist(actor_id: MobjId) {
    let actor = actor_id.ptr();
    let damage: i32;

    if (*actor).target.is_none() {
        return;
    }
    A_FaceTarget(actor_id);
    if P_CheckMeleeRange(actor_id).is_truthy() {
        damage = (P_Random() % 10 + 1) * 6;
        S_StartSound(actor_id, Sfx::Skepch as i32);
        P_DamageMobj(target_of(actor), Some(actor_id), Some(actor_id), damage);
    }
}
/// Blockmap iterator callback: tests whether `thing` is a raiseable corpse near the Archvile.
///
/// Returns `TRUE` (keep iterating) unless `thing` is a fully-settled corpse (`MF_CORPSE`,
/// `tics == -1`) with a valid `raisestate`, within `thing->radius + MT_VILE->radius` of
/// (`viletryx`, `viletryy`), and with enough headroom to stand at full height. If all
/// conditions are met, stores `thing` in `corpsehit` and returns `FALSE` to stop iteration.
///
/// # Safety
///
/// `thing_id` must name a map object of the level. `viletryx`/`viletryy` and `vileobj`
/// must have been set by `A_VileChase`. Called through `P_BlockThingsIterator`.
pub unsafe fn PIT_VileCheck(thing_id: MobjId) -> Boolean {
    let thing = thing_id.ptr();
    if (*thing).flags & MF_CORPSE == 0 {
        return Boolean::TRUE;
    }
    if (*thing).tics != -1 {
        return Boolean::TRUE;
    }
    if (*(*thing).info).raisestate == S_NULL {
        return Boolean::TRUE;
    }
    let maxdist: i32 = (*(*thing).info).radius + MOBJINFO.borrow()[MT_VILE as usize].radius;
    let (viletryx, viletryy) = {
        let enemy = ENEMY.borrow();
        (enemy.viletryx, enemy.viletryy)
    };
    if ((*thing).x - viletryx).abs() > maxdist || ((*thing).y - viletryy).abs() > maxdist {
        return Boolean::TRUE;
    }
    ENEMY.borrow_mut().corpsehit = Some(thing_id);
    let corpsehit = thing;
    (*corpsehit).momy = 0 as fixed_t;
    (*corpsehit).momx = (*corpsehit).momy;
    (*corpsehit).height <<= 2;
    let check: Boolean =
        Boolean::from_raw(P_CheckPosition(thing_id, (*corpsehit).x, (*corpsehit).y));
    (*corpsehit).height >>= 2;
    if check.is_false() {
        return Boolean::TRUE;
    }
    Boolean::FALSE
}
/// Chase action for the Archvile: hunts for corpses to resurrect while pursuing the player.
///
/// If the actor has a movement direction, computes a look-ahead position and searches the
/// surrounding blockmap with `PIT_VileCheck`. When a raiseable corpse is found, the Archvile
/// faces it, enters `S_VILE_HEAL1`, plays the resurrection sound, restores the corpse's
/// flags/health/height, and clears its `target`. Then falls through to `A_Chase`.
///
/// # Safety
///
/// `actor_id` must name a map object with valid `info` and `movedir`.
pub unsafe fn A_VileChase(actor_id: MobjId) {
    let actor = actor_id.ptr();
    let xl: i32;

    let xh: i32;

    let yl: i32;

    let yh: i32;

    let mut bx: i32;

    let mut by: i32;

    let info: *mut MobjInfo;

    let temp: Option<MobjId>;

    if (*actor).movedir != DI_NODIR {
        let viletryx =
            (*actor).x + (*(*actor).info).speed as fixed_t * xspeed[(*actor).movedir as usize];
        let viletryy =
            (*actor).y + (*(*actor).info).speed as fixed_t * yspeed[(*actor).movedir as usize];
        let (bmaporgx, bmaporgy) = blockmap_origin();
        xl = (viletryx - bmaporgx - 32 * FRACUNIT * 2) >> MAPBLOCKSHIFT;
        xh = (viletryx - bmaporgx + 32 * FRACUNIT * 2) >> MAPBLOCKSHIFT;
        yl = (viletryy - bmaporgy - 32 * FRACUNIT * 2) >> MAPBLOCKSHIFT;
        yh = (viletryy - bmaporgy + 32 * FRACUNIT * 2) >> MAPBLOCKSHIFT;
        {
            let mut enemy = ENEMY.borrow_mut();
            enemy.viletryx = viletryx;
            enemy.viletryy = viletryy;
            enemy.vileobj = Some(actor_id);
        }
        bx = xl;
        while bx <= xh {
            by = yl;
            while by <= yh {
                if P_BlockThingsIterator(
                    bx,
                    by,
                    Some(core::mem::transmute::<
                        unsafe fn(MobjId) -> Boolean,
                        unsafe fn(MobjId) -> u32,
                    >(PIT_VileCheck)),
                ) == 0
                {
                    let corpsehit_id = ENEMY
                        .borrow()
                        .corpsehit
                        .expect("the corpse search stopped without a corpse");
                    temp = (*actor).target;
                    (*actor).target = Some(corpsehit_id);
                    A_FaceTarget(actor_id);
                    (*actor).target = temp;
                    P_SetMobjState(actor_id, S_VILE_HEAL1);
                    S_StartSound(corpsehit_id, Sfx::Slop as i32);
                    let corpsehit = corpsehit_id.ptr();
                    info = (*corpsehit).info;
                    P_SetMobjState(corpsehit_id, (*info).raisestate as statenum_t);
                    (*corpsehit).height <<= 2;
                    (*corpsehit).flags = (*info).flags;
                    (*corpsehit).health = (*info).spawnhealth;
                    (*corpsehit).target = None;
                    return;
                }
                by += 1;
            }
            bx += 1;
        }
    }
    A_Chase(actor_id);
}
/// Archvile attack wind-up: plays the attack start sound (`sfx_vilatk`).
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_VileStart(actor_id: MobjId) {
    S_StartSound(actor_id, Sfx::Vilatk as i32);
}
/// Archvile fire start: plays the flame start sound then positions the fire object.
///
/// # Safety
///
/// `actor_id` must name a map object of the level (see [`A_Fire`]).
pub unsafe fn A_StartFire(actor_id: MobjId) {
    S_StartSound(actor_id, Sfx::Flamst as i32);
    A_Fire(actor_id);
}
/// Archvile fire crackle: plays the sustained flame sound then repositions the fire object.
///
/// # Safety
///
/// `actor_id` must name a map object of the level (see [`A_Fire`]).
pub unsafe fn A_FireCrackle(actor_id: MobjId) {
    S_StartSound(actor_id, Sfx::Flame as i32);
    A_Fire(actor_id);
}
/// Repositions the Archvile fire object 24 units in front of its tracer (the victim).
///
/// Does nothing if `actor->tracer` is `None`. Checks sight from the Archvile's `target`
/// (a dummy when `None`, see [`P_SubstNullMobj`]) to the tracer; if the vile lost
/// sight of its victim, the fire does not move.
/// Uses `P_UnsetThingPosition`/`P_SetThingPosition` to maintain blockmap consistency.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_Fire(actor_id: MobjId) {
    let actor = actor_id.ptr();
    let Some(dest_id) = (*actor).tracer else {
        return;
    };
    let target = P_SubstNullMobj((*actor).target);
    if P_CheckSight(target, dest_id) == 0 {
        return;
    }
    let dest = dest_id.ptr();
    let an: u32 = (*dest).angle >> ANGLETOFINESHIFT;
    P_UnsetThingPosition(actor_id);
    (*actor).x = (*dest).x + FixedMul(24 as fixed_t * FRACUNIT, *finecosine.0.add(an as usize));
    (*actor).y = (*dest).y + FixedMul(24 as fixed_t * FRACUNIT, finesine[an as usize]);
    (*actor).z = (*dest).z;
    P_SetThingPosition(actor_id);
}
/// Archvile attack setup: spawns the `MT_FIRE` hellfire object at the target's location.
///
/// Links the fire into the Archvile/target triangle: `actor->tracer = fire`,
/// `fire->target = actor`, `fire->tracer = actor->target`, then calls `A_Fire` to
/// position it immediately.
///
/// Note: the C source passes `actor->target->x` for both X and Y of `P_SpawnMobj`,
/// which is a vanilla Doom bug (Y should be `actor->target->y`). This port
/// faithfully reproduces the original behavior.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_VileTarget(actor_id: MobjId) {
    let actor = actor_id.ptr();
    if (*actor).target.is_none() {
        return;
    }
    A_FaceTarget(actor_id);
    // FIXME: p_enemy.c passes `actor->target->x` for the Y argument instead of
    // `actor->target->y` - this is a vanilla Doom bug reproduced faithfully here.
    let target = target_of(actor).ptr();
    let fog_id = P_SpawnMobj((*target).x, (*target).x, (*target).z, MT_FIRE);
    let fog = fog_id.ptr();
    (*actor).tracer = Some(fog_id);
    (*fog).target = Some(actor_id);
    (*fog).tracer = (*actor).target;
    A_Fire(fog_id);
}
/// Archvile attack payload: direct damage plus radius explosion via the fire object.
///
/// Faces the target; aborts if line of sight is lost. Deals 20 direct damage and
/// applies an upward momentum impulse of `1000*FRACUNIT / target->mass` to the target.
/// Repositions the fire object 24 units behind the target (opposite the Archvile's angle)
/// and calls `P_RadiusAttack` with radius 70 to inflict blast damage in the area.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_VileAttack(actor_id: MobjId) {
    let actor = actor_id.ptr();
    if (*actor).target.is_none() {
        return;
    }
    A_FaceTarget(actor_id);
    if P_CheckSight(actor_id, target_of(actor)) == 0 {
        return;
    }
    S_StartSound(actor_id, Sfx::Barexp as i32);
    P_DamageMobj(target_of(actor), Some(actor_id), Some(actor_id), 20);
    // Nothing from here on changes `actor->target`, so C's repeated reads
    // after the damage are one read here.
    let target = target_of(actor).ptr();
    (*target).momz = (1000 * FRACUNIT / (*(*target).info).mass) as fixed_t;
    let an: i32 = ((*actor).angle >> ANGLETOFINESHIFT) as i32;
    let Some(fire_id) = (*actor).tracer else {
        return;
    };
    let fire = fire_id.ptr();
    (*fire).x = (*target).x - FixedMul(24 as fixed_t * FRACUNIT, *finecosine.0.add(an as usize));
    (*fire).y = (*target).y - FixedMul(24 as fixed_t * FRACUNIT, finesine[an as usize]);
    P_RadiusAttack(fire_id, Some(actor_id), 70);
}
/// Mancubus raise/taunt: faces the target and plays the attack preparation sound.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_FatRaise(actor_id: MobjId) {
    A_FaceTarget(actor_id);
    S_StartSound(actor_id, Sfx::Manatk as i32);
}
/// Mancubus attack phase 1: two `MT_FATSHOT` fireballs spread to the right.
///
/// Rotates the actor's angle by `+FATSPREAD` (ANG90/8), fires one shot directly,
/// then fires a second shot additionally rotated by `+FATSPREAD` with velocity
/// recomputed from the new angle.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_FatAttack1(actor_id: MobjId) {
    let actor = actor_id.ptr();
    A_FaceTarget(actor_id);
    (*actor).angle = (*actor).angle.wrapping_add(FATSPREAD as angle_t);
    let target = P_SubstNullMobj((*actor).target);
    P_SpawnMissile(actor_id, target, MT_FATSHOT);
    let mo: *mut mobj_t = P_SpawnMissile(actor_id, target, MT_FATSHOT).ptr();
    (*mo).angle = (*mo).angle.wrapping_add(FATSPREAD as angle_t);
    let an: i32 = ((*mo).angle >> ANGLETOFINESHIFT) as i32;
    (*mo).momx = FixedMul(
        (*(*mo).info).speed as fixed_t,
        *finecosine.0.add(an as usize),
    );
    (*mo).momy = FixedMul((*(*mo).info).speed as fixed_t, finesine[an as usize]);
}
/// Mancubus attack phase 2: two `MT_FATSHOT` fireballs spread to the left.
///
/// Rotates the actor's angle by `-FATSPREAD`, fires one shot directly, then fires a
/// second shot additionally rotated by `-2*FATSPREAD` with velocity recomputed.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_FatAttack2(actor_id: MobjId) {
    let actor = actor_id.ptr();
    A_FaceTarget(actor_id);
    (*actor).angle = (*actor).angle.wrapping_sub(FATSPREAD as angle_t);
    let target = P_SubstNullMobj((*actor).target);
    P_SpawnMissile(actor_id, target, MT_FATSHOT);
    let mo: *mut mobj_t = P_SpawnMissile(actor_id, target, MT_FATSHOT).ptr();
    (*mo).angle = (*mo).angle.wrapping_sub((FATSPREAD * 2) as angle_t);
    let an: i32 = ((*mo).angle >> ANGLETOFINESHIFT) as i32;
    (*mo).momx = FixedMul(
        (*(*mo).info).speed as fixed_t,
        *finecosine.0.add(an as usize),
    );
    (*mo).momy = FixedMul((*(*mo).info).speed as fixed_t, finesine[an as usize]);
}
/// Mancubus attack phase 3: two `MT_FATSHOT` fireballs spread symmetrically ±FATSPREAD/2.
///
/// Fires one shot rotated `-FATSPREAD/2` and a second at `+FATSPREAD/2`; velocities are
/// recomputed for each so they actually travel in the spread directions.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_FatAttack3(actor_id: MobjId) {
    let actor = actor_id.ptr();
    let mut mo: *mut mobj_t;

    let mut an: i32;

    A_FaceTarget(actor_id);
    let target = P_SubstNullMobj((*actor).target);
    mo = P_SpawnMissile(actor_id, target, MT_FATSHOT).ptr();
    (*mo).angle = (*mo).angle.wrapping_sub((FATSPREAD / 2) as angle_t);
    an = ((*mo).angle >> ANGLETOFINESHIFT) as i32;
    (*mo).momx = FixedMul(
        (*(*mo).info).speed as fixed_t,
        *finecosine.0.add(an as usize),
    );
    (*mo).momy = FixedMul((*(*mo).info).speed as fixed_t, finesine[an as usize]);
    mo = P_SpawnMissile(actor_id, target, MT_FATSHOT).ptr();
    (*mo).angle = (*mo).angle.wrapping_add((FATSPREAD / 2) as angle_t);
    an = ((*mo).angle >> ANGLETOFINESHIFT) as i32;
    (*mo).momx = FixedMul(
        (*(*mo).info).speed as fixed_t,
        *finecosine.0.add(an as usize),
    );
    (*mo).momy = FixedMul((*(*mo).info).speed as fixed_t, finesine[an as usize]);
}
/// Lost Soul charge attack: sets `MF_SKULLFLY` and launches the actor as a living missile.
///
/// Sets horizontal momentum from `SKULLSPEED` scaled by the angle's fine-trig values.
/// Computes vertical momentum to reach the midpoint of the target's height over the
/// flight time (`dist / SKULLSPEED` tics, minimum 1).
///
/// # Safety
///
/// `actor_id` must name a map object with valid `info`.
pub unsafe fn A_SkullAttack(actor_id: MobjId) {
    let actor = actor_id.ptr();
    let mut dist: i32;

    if (*actor).target.is_none() {
        return;
    }
    let dest: *mut mobj_t = target_of(actor).ptr();
    (*actor).flags |= MF_SKULLFLY;
    S_StartSound(actor_id, (*(*actor).info).attacksound as i32);
    A_FaceTarget(actor_id);
    let an: angle_t = (*actor).angle >> ANGLETOFINESHIFT;
    (*actor).momx = FixedMul(SKULLSPEED, *finecosine.0.add(an as usize));
    (*actor).momy = FixedMul(SKULLSPEED, finesine[an as usize]);
    dist = P_AproxDistance((*dest).x - (*actor).x, (*dest).y - (*actor).y) as i32;
    dist /= SKULLSPEED;
    if dist < 1 {
        dist = 1;
    }
    (*actor).momz = (((*dest).z + ((*dest).height >> 1) - (*actor).z) / dist) as fixed_t;
}
/// Spawns a Lost Soul (`MT_SKULL`) at `angle` from `actor` and launches it at the target.
///
/// Counts all live `MT_SKULL` thinkers; if more than 20 already exist, does nothing.
/// Computes the spawn point `prestep` units ahead (4 + 1.5 * combined radii) to avoid
/// spawning inside the Pain Elemental. If the skull cannot move at its spawn point
/// (`P_TryMove` fails), it is instantly killed with 10000 damage instead. On success,
/// the skull inherits `actor->target` and immediately calls `A_SkullAttack`.
///
/// # Safety
///
/// `actor_id` must name a map object with valid `info`.
pub unsafe fn A_PainShootSkull(actor_id: MobjId, angle: angle_t) {
    let actor = actor_id.ptr();
    let mut count: i32 = 0;
    for thinker in thinkers() {
        if let ThinkerId::Mobj(mobj) = thinker
            && thinker.kind() == ThinkerKind::Mobj
            && (*mobj.ptr()).kind as u32 == MT_SKULL as u32
        {
            count += 1;
        }
    }
    if count > 20 {
        return;
    }
    let an: angle_t = angle >> ANGLETOFINESHIFT;
    let prestep: i32 = 4 * FRACUNIT
        + 3 * ((*(*actor).info).radius + MOBJINFO.borrow()[MT_SKULL as usize].radius) / 2;
    let x: fixed_t = (*actor).x + FixedMul(prestep as fixed_t, *finecosine.0.add(an as usize));
    let y: fixed_t = (*actor).y + FixedMul(prestep as fixed_t, finesine[an as usize]);
    let z: fixed_t = ((*actor).z + 8 * FRACUNIT) as fixed_t;
    let newmobj_id = P_SpawnMobj(x, y, z, MT_SKULL);
    let newmobj = newmobj_id.ptr();
    if P_TryMove(newmobj_id, (*newmobj).x, (*newmobj).y) == 0 {
        P_DamageMobj(newmobj_id, Some(actor_id), Some(actor_id), 10000);
        return;
    }
    (*newmobj).target = (*actor).target;
    A_SkullAttack(newmobj_id);
}
/// Pain Elemental attack: spawns a Lost Soul in the direction the actor is facing.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_PainAttack(actor_id: MobjId) {
    let actor = actor_id.ptr();
    if (*actor).target.is_none() {
        return;
    }
    A_FaceTarget(actor_id);
    A_PainShootSkull(actor_id, (*actor).angle);
}
/// Pain Elemental death: drops to the ground and spits three Lost Souls in cardinal directions.
///
/// Calls `A_Fall` to clear `MF_SOLID`, then spawns skulls at `angle+90`, `angle+180`,
/// and `angle+270` relative to the current facing direction.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_PainDie(actor_id: MobjId) {
    let actor = actor_id.ptr();
    A_Fall(actor_id);
    A_PainShootSkull(actor_id, (*actor).angle.wrapping_add(ANG90 as angle_t));
    A_PainShootSkull(actor_id, (*actor).angle.wrapping_add(ANG180));
    A_PainShootSkull(actor_id, (*actor).angle.wrapping_add(ANG270));
}
/// Plays the monster's death scream; randomises for multi-variant sound monsters.
///
/// Zombie Pig (Podth variants) picks randomly from three sounds; Demon (Bgdth variants)
/// from two. Cyberdemon and Spider Mastermind play at full (global) volume; all others
/// play positioned at the actor.
///
/// # Safety
///
/// `actor_id` must name a map object with a valid `info`.
pub unsafe fn A_Scream(actor_id: MobjId) {
    let actor = actor_id.ptr();
    let sound = match (*(*actor).info).deathsound {
        Sfx::None => return,
        Sfx::Podth1 | Sfx::Podth2 | Sfx::Podth3 => Sfx::Podth1 as i32 + P_Random() % 3,
        Sfx::Bgdth1 | Sfx::Bgdth2 => Sfx::Bgdth1 as i32 + P_Random() % 2,
        s => s as i32,
    };
    if (*actor).kind as u32 == MT_SPIDER as u32 || (*actor).kind as u32 == MT_CYBORG as u32 {
        S_StartNonPositionalSound(sound);
    } else {
        S_StartSound(actor_id, sound);
    };
}
/// Plays the player/monster gibbing scream (`sfx_slop`) positioned at the actor.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_XScream(actor_id: MobjId) {
    S_StartSound(actor_id, Sfx::Slop as i32);
}
/// Plays the monster's pain sound if it has one.
///
/// # Safety
///
/// `actor_id` must name a map object with a valid `info`.
pub unsafe fn A_Pain(actor_id: MobjId) {
    let actor = actor_id.ptr();
    if (*(*actor).info).painsound != Sfx::None {
        S_StartSound(actor_id, (*(*actor).info).painsound as i32);
    }
}
/// Clears `MF_SOLID` so the falling corpse can be walked over.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_Fall(actor_id: MobjId) {
    let actor = actor_id.ptr();
    (*actor).flags &= !(MF_SOLID);
}
/// Triggers a 128-unit radius blast centred on `thingy`, sourced from `thingy->target`.
///
/// Used by rockets and barrel explosions. Calls `P_RadiusAttack` which damages all
/// shootable things within range proportional to distance.
///
/// # Safety
///
/// `thingy` must name a map object of the level; its `target` may be `None`
/// or stale (passed directly to `P_RadiusAttack`).
pub unsafe fn A_Explode(thingy: MobjId) {
    P_RadiusAttack(thingy, (*thingy.ptr()).target, 128);
}
/// Determines whether the death of a monster of type `motype` should trigger episode-end effects.
///
/// Pre-v1.9 (before `exe_ultimate`): only triggers on map 8; Barons on episodes 2+ are ignored.
/// `exe_ultimate` and later: episode-specific logic introduced with Ultimate Doom episode 4 support:
/// - Ep 1 map 8: Barons.
/// - Ep 2 map 8: Cyberdemon.
/// - Ep 3 map 8: Spider Mastermind.
/// - Ep 4 map 6: Cyberdemon; map 8: Spider Mastermind.
/// - All other episodes: map 8 unconditionally.
fn CheckBossEnd(motype: mobjtype_t) -> Boolean {
    if (gameversion() as u32) < exe_ultimate as u32 {
        if gamemap() != 8 {
            return Boolean::FALSE;
        }
        if motype == MT_BRUISER && gameepisode() != 1 {
            return Boolean::FALSE;
        }
        Boolean::TRUE
    } else {
        match gameepisode() {
            1 => Boolean::from(gamemap() == 8 && motype == MT_BRUISER),
            2 => Boolean::from(gamemap() == 8 && motype == MT_CYBORG),
            3 => Boolean::from(gamemap() == 8 && motype == MT_SPIDER),
            4 => Boolean::from(
                gamemap() == 6 && motype == MT_CYBORG || gamemap() == 8 && motype == MT_SPIDER,
            ),
            _ => Boolean::from(gamemap() == 8),
        }
    }
}
/// Triggers map-special effects when a boss monster dies (if all bosses of the same type are dead).
///
/// Dispatch logic:
/// - Doom II (`commercial`), map 7: Mancubus death lowers floor tag 666;
///   Arachnotron death raises floor tag 667.
/// - Doom I episodes: calls `CheckBossEnd`; on success triggers `EV_DoFloor`/`EV_DoDoor`
///   with a synthetic `line_t` (tag 666) or falls through to `G_ExitLevel`.
/// Returns early if any player is dead, or if another live boss of the same type exists.
///
/// # Safety
///
/// `mo_id` must name a map object of the level. The thinker list and game-state globals
/// must be consistent. Called through the state-machine action pointer.
pub unsafe fn A_BossDeath(mo_id: MobjId) {
    let mo = mo_id.ptr();
    let mut junk: line_t = JUNK_LINE;
    let _i: i32 = 0;
    if gamemode() as u32 == commercial as u32 {
        if gamemap() != 7 {
            return;
        }
        if (*mo).kind as u32 != MT_FATSO as u32 && (*mo).kind as u32 != MT_BABY as u32 {
            return;
        }
    } else if CheckBossEnd((*mo).kind).is_false() {
        return;
    }
    let mut i: usize = 0;
    while i < MAXPLAYERS {
        if playeringame(i) != 0 && PlayerId::new(i).read(|player| player.health) > 0 {
            break;
        }
        i += 1;
    }
    if i == MAXPLAYERS {
        return;
    }
    if another_alive_of_same_kind(mo_id) {
        return;
    }
    if gamemode() as u32 == commercial as u32 {
        if gamemap() == 7 {
            if (*mo).kind as u32 == MT_FATSO as u32 {
                junk.tag = 666;
                EV_DoFloor(&junk, lowerFloorToLowest);
                return;
            }
            if (*mo).kind as u32 == MT_BABY as u32 {
                junk.tag = 667;
                EV_DoFloor(&junk, raiseToTexture);
                return;
            }
        }
    } else {
        match gameepisode() {
            1 => {
                junk.tag = 666;
                EV_DoFloor(&junk, lowerFloorToLowest);
                return;
            }
            4 => match gamemap() {
                6 => {
                    junk.tag = 666;
                    EV_DoDoor(&junk, vld_blazeOpen);
                    return;
                }
                8 => {
                    junk.tag = 666;
                    EV_DoFloor(&junk, lowerFloorToLowest);
                    return;
                }
                _ => {}
            },
            _ => {}
        }
    }
    G_ExitLevel();
}
/// Cyberdemon footstep: plays `sfx_hoof` then advances via `A_Chase`.
///
/// # Safety
///
/// `mo_id` must name a map object with a valid `info`.
pub unsafe fn A_Hoof(mo_id: MobjId) {
    S_StartSound(mo_id, Sfx::Hoof as i32);
    A_Chase(mo_id);
}
/// Spider Mastermind footstep: plays `sfx_metal` then advances via `A_Chase`.
///
/// # Safety
///
/// `mo_id` must name a map object with a valid `info`.
pub unsafe fn A_Metal(mo_id: MobjId) {
    S_StartSound(mo_id, Sfx::Metal as i32);
    A_Chase(mo_id);
}
/// Arachnotron footstep: plays `sfx_bspwlk` then advances via `A_Chase`.
///
/// # Safety
///
/// `mo_id` must name a map object with a valid `info`.
pub unsafe fn A_BabyMetal(mo_id: MobjId) {
    S_StartSound(mo_id, Sfx::Bspwlk as i32);
    A_Chase(mo_id);
}
/// Super Shotgun open action: plays the barrel-break sound (`sfx_dbopn`) for the player.
///
/// # Safety
///
/// `player` must be spawned in the level.
pub unsafe fn A_OpenShotgun2(player: PlayerId, _psp: *mut PspdefT) {
    S_StartSound(player.mo(), Sfx::Dbopn as i32);
}
/// Super Shotgun load action: plays the shell-load sound (`sfx_dbload`) for the player.
///
/// # Safety
///
/// `player` must be spawned in the level.
pub unsafe fn A_LoadShotgun2(player: PlayerId, _psp: *mut PspdefT) {
    S_StartSound(player.mo(), Sfx::Dbload as i32);
}
/// Super Shotgun close action: plays the close sound (`sfx_dbcls`) then checks for refire.
///
/// # Safety
///
/// `player` must be spawned in the level, and no reference into its slot
/// may be live while this runs; `psp` must be non-null.
pub unsafe fn A_CloseShotgun2(player: PlayerId, psp: *mut PspdefT) {
    S_StartSound(player.mo(), Sfx::Dbcls as i32);
    A_ReFire(player, psp);
}
/// Icon of Sin wake-up: scans the thinker list for `MT_BOSSTARGET` spots and plays `sfx_bossit`.
///
/// Populates `braintargets` and resets `numbraintargets`/`braintargeton` to 0.
/// The target spots are placed by the level designer in the map; the brain cycles
/// through them to choose where to spit monster cubes.
///
/// # Safety
///
/// The thinker list must be consistent. Called through the state-machine action pointer.
pub unsafe fn A_BrainAwake(_mo: MobjId) {
    {
        let mut enemy = ENEMY.borrow_mut();
        enemy.numbraintargets = 0;
        enemy.braintargeton = 0;
        for thinker in thinkers() {
            if let ThinkerId::Mobj(mobj) = thinker
                && thinker.kind() == ThinkerKind::Mobj
                && (*mobj.ptr()).kind as u32 == MT_BOSSTARGET as u32
            {
                let index = enemy.numbraintargets as usize;
                enemy.braintargets[index] = Some(mobj);
                enemy.numbraintargets += 1;
            }
        }
    }
    S_StartNonPositionalSound(Sfx::Bossit as i32);
}
/// Icon of Sin pain reaction: plays `sfx_bospn` at full (global) volume.
pub fn A_BrainPain(_mo: MobjId) {
    S_StartNonPositionalSound(Sfx::Bospn as i32);
}
/// Icon of Sin death scream: spawns a row of exploding rockets across the brain's width.
///
/// Spawns `MT_ROCKET` objects every 8 units from `mo->x - 196` to `mo->x + 320`,
/// each at a random height and with a random upward `momz`. Each rocket is immediately
/// set to `S_BRAINEXPLODE1` with a randomised tic count for visual variety.
/// Plays `sfx_bosdth` at full volume to conclude.
///
/// # Safety
///
/// `mo_id` must name a map object of the level. Called through the state-machine
/// action pointer.
pub unsafe fn A_BrainScream(mo_id: MobjId) {
    let mo = mo_id.ptr();
    let mut x: i32;

    let mut y: i32;

    let mut z: i32;

    x = (*mo).x - 196 * FRACUNIT;
    while x < (*mo).x + 320 * FRACUNIT {
        y = (*mo).y - 320 * FRACUNIT;
        z = 128 + P_Random() * 2 * FRACUNIT;
        let th_id = P_SpawnMobj(x as fixed_t, y as fixed_t, z as fixed_t, MT_ROCKET);
        let th = th_id.ptr();
        (*th).momz = (P_Random() * 512) as fixed_t;
        P_SetMobjState(th_id, S_BRAINEXPLODE1);
        (*th).tics -= P_Random() & 7;
        if (*th).tics < 1 {
            (*th).tics = 1;
        }
        x += FRACUNIT * 8;
    }
    S_StartNonPositionalSound(Sfx::Bosdth as i32);
}
/// Individual brain explosion particle: spawns one `MT_ROCKET` at a random horizontal offset.
///
/// Called repeatedly by the `S_BRAINEXPLODE` state chain. Each invocation spawns a rocket
/// with random X offset (±`P_Random*2048`) at the same Y as `mo`, at a random height
/// (128 + `P_Random*2*FRACUNIT`). The rocket transitions immediately to `S_BRAINEXPLODE1`
/// with a randomised tic count.
///
/// # Safety
///
/// `mo_id` must name a map object of the level. Called through the state-machine
/// action pointer.
pub unsafe fn A_BrainExplode(mo_id: MobjId) {
    let mo = mo_id.ptr();
    let x: i32 = (*mo).x + (P_Random() - P_Random()) * 2048;
    let y: i32 = (*mo).y;
    let z: i32 = 128 + P_Random() * 2 * FRACUNIT;
    let th_id = P_SpawnMobj(x as fixed_t, y as fixed_t, z as fixed_t, MT_ROCKET);
    let th = th_id.ptr();
    (*th).momz = (P_Random() * 512) as fixed_t;
    P_SetMobjState(th_id, S_BRAINEXPLODE1);
    (*th).tics -= P_Random() & 7;
    if (*th).tics < 1 {
        (*th).tics = 1;
    }
}
/// Icon of Sin death: ends the level via `G_ExitLevel`.
pub fn A_BrainDie(_mo: MobjId) {
    G_ExitLevel();
}
/// Icon of Sin attack: launches a monster cube (`MT_SPAWNSHOT`) toward the next target spot.
///
/// Alternates with a static `easy` flag so on easy skill every other attack is skipped.
/// Picks `braintargets[braintargeton]` as the destination and advances `braintargeton`
/// modulo `numbraintargets`. Sets the cube's `reactiontime` to the travel time (in state
/// tics) so `A_SpawnFly` knows when to materialize the monster. Plays `sfx_bospit` globally.
///
/// # Safety
///
/// `mo_id` must name a map object of the level. `braintargets` must have been
/// populated by `A_BrainAwake` and `numbraintargets` must be > 0. Called through the
/// state-machine action pointer.
///
/// # Panics
///
/// If the brain has no target spots (C reads a null target there).
pub unsafe fn A_BrainSpit(mo_id: MobjId) {
    let mo = mo_id.ptr();
    let targ_id = {
        let mut enemy = ENEMY.borrow_mut();
        enemy.brainspit_easy ^= 1;
        if gameskill() <= sk_easy && enemy.brainspit_easy == 0 {
            return;
        }
        let targ = enemy.braintargets[enemy.braintargeton as usize];
        enemy.braintargeton = (enemy.braintargeton + 1) % enemy.numbraintargets;
        targ.expect("brain spit at a missing target spot")
    };
    let newmobj: *mut mobj_t = P_SpawnMissile(mo_id, targ_id, MT_SPAWNSHOT).ptr();
    (*newmobj).target = Some(targ_id);
    let targ = targ_id.ptr();
    (*newmobj).reactiontime = ((*targ).y - (*mo).y) / (*newmobj).momy / (*(*newmobj).state).tics;
    S_StartNonPositionalSound(Sfx::Bospit as i32);
}
/// In-flight cube sound: plays `sfx_boscub` while the cube travels, then calls `A_SpawnFly`.
///
/// # Safety
///
/// `mo_id` must name a map object of the level. Called through the state-machine
/// action pointer.
pub unsafe fn A_SpawnSound(mo_id: MobjId) {
    S_StartSound(mo_id, Sfx::Boscub as i32);
    A_SpawnFly(mo_id);
}
/// Monster cube arrival: materializes a random monster at the target spot when `reactiontime` hits 0.
///
/// Decrements `reactiontime` each tic; returns immediately while still > 0. On arrival:
/// 1. Spawns a `MT_SPAWNFIRE` teleport fog with `sfx_telept` at the target's location.
/// 2. Picks a random monster type via weighted probability table (Imp 50/256 through
///    Baron of Hell for top range).
/// 3. Spawns the monster, calls `P_LookForPlayers` to awaken it.
/// 4. Calls `P_TeleportMove` to telefrág anything at the spawn point.
/// 5. Removes the cube (`mo`) via `P_RemoveMobj`.
///
/// # Safety
///
/// `mo_id` must name a map object whose `target` is the boss target spot (a dummy
/// stands in for `None`, see [`P_SubstNullMobj`]). Called through the state-machine
/// action pointer.
pub unsafe fn A_SpawnFly(mo_id: MobjId) {
    let mo = mo_id.ptr();
    let type_0: mobjtype_t;

    (*mo).reactiontime -= 1;
    if (*mo).reactiontime != 0 {
        return;
    }
    let targ: *mut mobj_t = P_SubstNullMobj((*mo).target).ptr();
    let fog = P_SpawnMobj((*targ).x, (*targ).y, (*targ).z, MT_SPAWNFIRE);
    S_StartSound(fog, Sfx::Telept as i32);
    let r: i32 = P_Random();
    if r < 50 {
        type_0 = MT_TROOP;
    } else if r < 90 {
        type_0 = MT_SERGEANT;
    } else if r < 120 {
        type_0 = MT_SHADOWS;
    } else if r < 130 {
        type_0 = MT_PAIN;
    } else if r < 160 {
        type_0 = MT_HEAD;
    } else if r < 162 {
        type_0 = MT_VILE;
    } else if r < 172 {
        type_0 = MT_UNDEAD;
    } else if r < 192 {
        type_0 = MT_BABY;
    } else if r < 222 {
        type_0 = MT_FATSO;
    } else if r < 246 {
        type_0 = MT_KNIGHT;
    } else {
        type_0 = MT_BRUISER;
    }
    let newmobj_id = P_SpawnMobj((*targ).x, (*targ).y, (*targ).z, type_0);
    let newmobj = newmobj_id.ptr();
    if P_LookForPlayers(newmobj_id, Boolean::TRUE).is_truthy() {
        P_SetMobjState(newmobj_id, (*(*newmobj).info).seestate as statenum_t);
    }
    P_TeleportMove(newmobj_id, (*newmobj).x, (*newmobj).y);
    P_RemoveMobj(mo_id);
}
/// Plays the player death scream; uses the gibbing scream in Doom II when health is below -50.
///
/// Default sound is `sfx_pldeth`. In commercial mode, if `mo->health < -50`, plays
/// `sfx_pdiehi` (the "squish" gib scream) instead.
///
/// # Safety
///
/// `mo_id` must name a map object of the level. Called through the state-machine
/// action pointer.
pub unsafe fn A_PlayerScream(mo_id: MobjId) {
    let mo = mo_id.ptr();
    let mut sound: i32 = Sfx::Pldeth as i32;
    if gamemode() as u32 == commercial as u32 && (*mo).health < -50 {
        sound = Sfx::Pdiehi as i32;
    }
    S_StartSound(mo_id, sound);
}
