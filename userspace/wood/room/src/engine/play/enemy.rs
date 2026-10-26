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
//! `mobj_t *`, and each action reads and writes them through the borrowed
//! mobj arena, copying fields out before a call that runs play code and
//! reading them afresh after it, as C sees them through its pointers.

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
            player::{MAXPLAYERS, PlayerId},
            stat::{gamemode, gameversion},
        },
        map::{LineId, SectorId, VertexId, line_t, slopetype_t},
        math::{
            fixed::{FRACUNIT, FixedMul, fixed_t},
            tables::{ANG90, ANG180, ANG270, ANGLETOFINESHIFT, FINEANGLES, finesine},
        },
        misc::random::P_Random,
        play::{
            info::{
                MF_AMBUSH, MF_CORPSE, MF_FLOAT, MF_INFLOAT, MF_JUSTATTACKED, MF_JUSTHIT, MF_SHADOW,
                MF_SHOOTABLE, MF_SKULLFLY, MF_SOLID, MOBJINFO, MT_ARACHPLAZ, MT_BABY,
                MT_BOSSTARGET, MT_BRUISER, MT_BRUISERSHOT, MT_CYBORG, MT_FATSHOT, MT_FATSO,
                MT_FIRE, MT_HEAD, MT_HEADSHOT, MT_KNIGHT, MT_PAIN, MT_ROCKET, MT_SERGEANT,
                MT_SHADOWS, MT_SKULL, MT_SMOKE, MT_SPAWNFIRE, MT_SPAWNSHOT, MT_SPIDER, MT_TRACER,
                MT_TROOP, MT_TROOPSHOT, MT_UNDEAD, MT_VILE, S_BRAINEXPLODE1, S_NULL, S_VILE_HEAL1,
            },
            inter::P_DamageMobj,
            map::{
                P_AimLineAttack, P_CheckPosition, P_LineAttack, P_RadiusAttack, P_TeleportMove,
                P_TryMove,
            },
            maputl::{
                MAPBLOCKSHIFT, P_AproxDistance, P_BlockThingsIterator, P_LineOpening,
                P_SetThingPosition, P_UnsetThingPosition,
            },
            mobj::{
                MobjId, P_RemoveMobj, P_SetMobjState, P_SpawnMissile, P_SpawnMobj, P_SpawnPuff,
                P_SubstNullMobj, mobj_t,
            },
            setup::LevelState,
            sight::P_CheckSight,
            switch::P_UseSpecialLine,
            tick::{MOBJ_ARENA, ThinkerId, ThinkerKind, thinkers},
        },
        render_3d::main::R_PointToAngle2,
        sound::{S_StartNonPositionalSound, S_StartSound},
        wad::format::LinedefFlag,
        world::{PlayScratch, Session, World},
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
    game::G_ExitLevel,
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
/// Monster AI scratch state (`p_enemy.c` globals), kept in
/// [`PlayScratch`].
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

/// Recursively floods sound through adjacent sectors, stamping each with `soundtarget`.
///
/// Traversal stops at sectors already visited this frame (`validcount`) and at
/// two-sided linedefs with the `ML_SOUNDBLOCK` flag - the block flag allows one
/// crossing (incrementing `soundblocks` from 0 to 1) but never two, so sound
/// cannot pass through two consecutive blocking walls.
///
/// Called by `P_NoiseAlert`, which splits the world into the level and the
/// scratch for the whole flood, and recursively by itself. Reads `validcount` and `soundtarget` from
/// `scratch` and leaves the last line opening in it.
pub fn P_RecursiveSound(
    scratch: &mut PlayScratch,
    level: &mut LevelState,
    sec: SectorId,
    soundblocks: i32,
) {
    let validcount = scratch.validcount;
    {
        let sector = &mut level.sectors[sec];
        if sector.validcount == validcount && sector.soundtraversed <= soundblocks + 1 {
            return;
        }
        sector.validcount = validcount;
        sector.soundtraversed = soundblocks + 1;
        sector.soundtarget = scratch.enemy.soundtarget;
    }
    let mut i = 0;
    while i < level.sectors[sec].linecount {
        let check = level.sector_lines(sec)[i as usize];
        let line = level.lines[check];
        if line.flags as i32 & LinedefFlag::TWOSIDED as i32 != 0 {
            P_LineOpening(&mut scratch.maputl, level, check);
            if scratch.maputl.openrange > 0 {
                let other = if level.sides[line.sidenum[0] as usize].sector == sec {
                    level.sides[line.sidenum[1] as usize].sector
                } else {
                    level.sides[line.sidenum[0] as usize].sector
                };
                if line.flags as i32 & LinedefFlag::SOUNDBLOCK as i32 != 0 {
                    if soundblocks == 0 {
                        P_RecursiveSound(scratch, level, other, 1);
                    }
                } else {
                    P_RecursiveSound(scratch, level, other, soundblocks);
                }
            }
        }
        i += 1;
    }
}
/// Alerts monsters in earshot that a target (typically the player) has made noise.
///
/// Sets the scratch `soundtarget`, increments `validcount` to mark a new traversal
/// frame, then calls `P_RecursiveSound` starting from the sector containing `emmiter`.
/// Called by `p_inter.rs` and `p_map.rs` whenever a shot, explosion, or door fires.
///
/// # Panics
///
/// If `emmiter` is not in a subsector of the loaded level.
pub fn P_NoiseAlert(world: &mut World, target: MobjId, emmiter: MobjId) {
    let World { level, scratch, .. } = world;
    scratch.enemy.soundtarget = Some(target);
    scratch.validcount += 1;
    let sector = level.subsectors[MOBJ_ARENA.borrow()[emmiter].subsector].sector;
    P_RecursiveSound(scratch, level, sector, 0);
}
/// The target of `actor`, which C dereferences without a null check: the
/// callers have checked it, or C would crash too.
///
/// # Panics
///
/// If `actor` has no target.
#[track_caller]
fn target_of(actor: &mobj_t) -> MobjId {
    actor.target.expect("actor has no target")
}
/// The current target of map object `actor_id`, read afresh (see
/// [`target_of`]).
///
/// # Panics
///
/// If the actor has no target.
#[track_caller]
fn current_target(actor_id: MobjId) -> MobjId {
    target_of(&MOBJ_ARENA.borrow()[actor_id])
}
/// `true` if map object `actor_id` has a target now (C's
/// `actor->target != NULL`).
#[track_caller]
fn has_target(actor_id: MobjId) -> bool {
    MOBJ_ARENA.borrow()[actor_id].target.is_some()
}
/// Returns `TRUE` when `actor`'s target is within melee striking range and line of sight.
///
/// Range test: approximate distance must be less than `MELEERANGE - 20 + target.radius`.
/// Returns `FALSE` immediately if `actor->target` is `None`, if the distance check fails,
/// or if `P_CheckSight` reports no clear line of sight.
///
pub fn P_CheckMeleeRange(world: &mut World, actor_id: MobjId) -> Boolean {
    {
        let mobjs = MOBJ_ARENA.borrow();
        let actor = &mobjs[actor_id];
        let Some(target) = actor.target else {
            return Boolean::FALSE;
        };
        let pl = &mobjs[target];
        let dist: fixed_t = P_AproxDistance(pl.x - actor.x, pl.y - actor.y);
        if dist >= MELEERANGE - 20 * FRACUNIT + pl.info.get().radius {
            return Boolean::FALSE;
        }
    }
    if P_CheckSight(world, actor_id, current_target(actor_id)) == 0 {
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
/// # Panics
///
/// If the actor has no target.
pub fn P_CheckMissileRange(world: &mut World, actor_id: MobjId) -> Boolean {
    let mut dist: fixed_t;

    if P_CheckSight(world, actor_id, current_target(actor_id)) == 0 {
        return Boolean::FALSE;
    }
    let mut mobjs = MOBJ_ARENA.borrow_mut();
    let actor = &mut mobjs[actor_id];
    if actor.flags & MF_JUSTHIT != 0 {
        actor.flags &= !(MF_JUSTHIT);
        return Boolean::TRUE;
    }
    if actor.reactiontime != 0 {
        return Boolean::FALSE;
    }
    let (actor_x, actor_y, actor_kind, actor_info) = (actor.x, actor.y, actor.kind, actor.info);
    let target = &mobjs[target_of(&mobjs[actor_id])];
    dist =
        (P_AproxDistance(actor_x - target.x, actor_y - target.y) as i32 - 64 * FRACUNIT) as fixed_t;
    drop(mobjs);
    if actor_info.get().meleestate == 0 {
        dist -= 128 * FRACUNIT;
    }
    dist >>= 16;
    if actor_kind as u32 == MT_VILE as u32 && dist > 14 * 64 {
        return Boolean::FALSE;
    }
    if actor_kind as u32 == MT_UNDEAD as u32 {
        if dist < 196 {
            return Boolean::FALSE;
        }
        dist >>= 1;
    }
    if actor_kind as u32 == MT_CYBORG as u32
        || actor_kind as u32 == MT_SPIDER as u32
        || actor_kind as u32 == MT_SKULL as u32
    {
        dist >>= 1;
    }
    if dist > 200 {
        dist = 200 as fixed_t;
    }
    if actor_kind as u32 == MT_CYBORG as u32 && dist > 160 {
        dist = 160 as fixed_t;
    }
    if P_Random(world) < dist {
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
/// `actor_id` must name a map object of the level; the step runs
/// [`P_TryMove`] and the specials of blocking lines.
pub unsafe fn P_Move(world: &mut World, actor_id: MobjId) -> Boolean {
    let mut ld: LineId;

    let mut good: Boolean;

    let (tryx, tryy) = {
        let mobjs = MOBJ_ARENA.borrow();
        let actor = &mobjs[actor_id];
        if actor.movedir == DI_NODIR {
            return Boolean::FALSE;
        }
        if actor.movedir as u32 >= 8 {
            i_error!("Weird actor->movedir!");
        }
        let speed = actor.info.get().speed as fixed_t;
        (
            actor.x + speed * xspeed[actor.movedir as usize],
            actor.y + speed * yspeed[actor.movedir as usize],
        )
    };
    let try_ok: Boolean = Boolean::from_raw(P_TryMove(world, actor_id, tryx, tryy));
    // The move runs play code; the actor is read afresh after it.
    if try_ok.is_false() {
        {
            let mut mobjs = MOBJ_ARENA.borrow_mut();
            let actor = &mut mobjs[actor_id];
            let map = &world.scratch.map;
            if actor.flags & MF_FLOAT != 0 && map.floatok != 0 {
                if actor.z < map.tmfloorz {
                    actor.z += FLOATSPEED;
                } else {
                    actor.z -= FLOATSPEED;
                }
                actor.flags |= MF_INFLOAT;
                return Boolean::TRUE;
            }
            if map.numspechit == 0 {
                return Boolean::FALSE;
            }
            actor.movedir = DI_NODIR;
        }
        good = Boolean::FALSE;
        // A used line can move things and refill the list, so the count is
        // re-read before every pop, as in C.
        loop {
            ld = {
                let map = &mut world.scratch.map;
                let c2rust_fresh0 = map.numspechit;
                map.numspechit -= 1;
                if c2rust_fresh0 == 0 {
                    break;
                }
                map.spechit[map.numspechit as usize]
            };
            if P_UseSpecialLine(world, actor_id, ld, 0) != 0 {
                good = Boolean::TRUE;
            }
        }
        return good;
    }
    let mut mobjs = MOBJ_ARENA.borrow_mut();
    let actor = &mut mobjs[actor_id];
    actor.flags &= !(MF_INFLOAT);
    if actor.flags & MF_FLOAT == 0 {
        actor.z = actor.floorz;
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
pub unsafe fn P_TryWalk(world: &mut World, actor_id: MobjId) -> Boolean {
    if P_Move(world, actor_id).is_false() {
        return Boolean::FALSE;
    }
    let movecount = P_Random(world) & 15;
    MOBJ_ARENA.borrow_mut()[actor_id].movecount = movecount;
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
pub unsafe fn P_NewChaseDir(world: &mut World, actor_id: MobjId) {
    let mut d: [dirtype_t; 3] = [DI_EAST; 3];
    let mut tdir: i32;

    // Each walk attempt runs play code, so the direction is written just
    // before it.
    let set_movedir = |movedir: i32| MOBJ_ARENA.borrow_mut()[actor_id].movedir = movedir;
    let (olddir, deltax, deltay): (dirtype_t, fixed_t, fixed_t) = {
        let mobjs = MOBJ_ARENA.borrow();
        let actor = &mobjs[actor_id];
        let Some(target) = actor.target else {
            i_error!("P_NewChaseDir: called with no target");
        };
        let target = &mobjs[target];
        (
            actor.movedir as dirtype_t,
            target.x - actor.x,
            target.y - actor.y,
        )
    };
    let turnaround: dirtype_t = opposite[olddir as usize];
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
        let movedir: i32 = diags[((((deltay < 0) as i32) << 1) + (deltax > 0) as i32) as usize];
        set_movedir(movedir);
        if movedir != turnaround as i32 && P_TryWalk(world, actor_id).is_truthy() {
            return;
        }
    }
    if P_Random(world) > 200 || deltay.abs() > deltax.abs() {
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
        set_movedir(d[1]);
        if P_TryWalk(world, actor_id).is_truthy() {
            return;
        }
    }
    if d[2] as u32 != DI_NODIR as u32 {
        set_movedir(d[2]);
        if P_TryWalk(world, actor_id).is_truthy() {
            return;
        }
    }
    if olddir as u32 != DI_NODIR as u32 {
        set_movedir(olddir);
        if P_TryWalk(world, actor_id).is_truthy() {
            return;
        }
    }
    if P_Random(world) & 1 != 0 {
        tdir = DI_EAST;
        while tdir <= DI_SOUTHEAST {
            if tdir != turnaround as i32 {
                set_movedir(tdir);
                if P_TryWalk(world, actor_id).is_truthy() {
                    return;
                }
            }
            tdir += 1;
        }
    } else {
        tdir = DI_SOUTHEAST;
        while tdir != DI_EAST - 1 {
            if tdir != turnaround as i32 {
                set_movedir(tdir);
                if P_TryWalk(world, actor_id).is_truthy() {
                    return;
                }
            }
            tdir -= 1;
        }
    }
    if turnaround as u32 != DI_NODIR as u32 {
        set_movedir(turnaround as i32);
        if P_TryWalk(world, actor_id).is_truthy() {
            return;
        }
    }
    set_movedir(DI_NODIR);
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
/// # Panics
///
/// If a player in the game has no map object.
pub fn P_LookForPlayers(world: &mut World, actor_id: MobjId, allaround: Boolean) -> Boolean {
    let mut c: i32;

    let mut an: angle_t;

    let mut dist: fixed_t;

    // The sight check runs no play code; `lastlook` is still read through
    // the arena at each use, as C reads it through its pointer.
    let lastlook = || MOBJ_ARENA.borrow()[actor_id].lastlook;
    c = 0;
    let stop: i32 = (lastlook() - 1) & 3;
    loop {
        's_20: {
            if world.session.in_game(lastlook() as usize) {
                let c2rust_fresh1 = c;
                c += 1;
                if c2rust_fresh1 == 2 || lastlook() == stop {
                    return Boolean::FALSE;
                }
                let player = PlayerId::new(lastlook() as usize);
                if world.players[player].health > 0 {
                    let player_mo = world.players[player].mo();
                    let sight = P_CheckSight(world, actor_id, player_mo);
                    if sight != 0 {
                        let mut mobjs = MOBJ_ARENA.borrow_mut();
                        if allaround.is_false() {
                            let (actor, player_mo) = (&mobjs[actor_id], &mobjs[player_mo]);
                            an = R_PointToAngle2(actor.x, actor.y, player_mo.x, player_mo.y)
                                .wrapping_sub(actor.angle);
                            if an > ANG90 as angle_t && an < ANG270 {
                                dist =
                                    P_AproxDistance(player_mo.x - actor.x, player_mo.y - actor.y);
                                if dist > MELEERANGE {
                                    break 's_20;
                                }
                            }
                        }
                        mobjs[actor_id].target = Some(player_mo);
                        return Boolean::TRUE;
                    }
                }
            }
        }
        let mut mobjs = MOBJ_ARENA.borrow_mut();
        let actor = &mut mobjs[actor_id];
        actor.lastlook = (actor.lastlook + 1) & 3;
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
fn another_alive_of_same_kind(mo_id: MobjId) -> bool {
    let mobjs = MOBJ_ARENA.borrow();
    let kind = mobjs[mo_id].kind;
    thinkers().any(|thinker| {
        let ThinkerId::Mobj(other_id) = thinker else {
            return false;
        };
        if thinker.kind() != ThinkerKind::Mobj || other_id == mo_id {
            return false;
        }
        let other = &mobjs[other_id];
        other.kind as u32 == kind as u32 && other.health > 0
    })
}
/// Action function for Commander Keen's death (Doom II map 32 special).
///
/// Calls `A_Fall` to make the corpse non-solid, then scans all thinkers; if any other
/// Keen of the same type is still alive the function returns early. When the last Keen
/// dies it synthesises a `line_t` with `tag = 666` and calls `EV_DoDoor` with
/// `vld_open` to open the tagged door, allowing exit from the secret level.
pub fn A_KeenDie(world: &mut World, mo_id: MobjId) {
    let mut junk: line_t = JUNK_LINE;
    A_Fall(world, mo_id);
    if another_alive_of_same_kind(mo_id) {
        return;
    }
    junk.tag = 666;
    EV_DoDoor(world, &junk, vld_open);
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
pub unsafe fn A_Look(world: &mut World, actor_id: MobjId) {
    // `true` when the sector's sound target is seen (C's `goto seeyou`).
    let seen = 'seeyou: {
        let ambush = {
            let mut mobjs = MOBJ_ARENA.borrow_mut();
            let actor = &mut mobjs[actor_id];
            actor.threshold = 0;
            let targ: Option<MobjId> = world.level.subsector_sector(actor.subsector).soundtarget;
            let Some(targ) = targ.filter(|&targ| mobjs[targ].flags & MF_SHOOTABLE != 0) else {
                break 'seeyou false;
            };
            let actor = &mut mobjs[actor_id];
            actor.target = Some(targ);
            actor.flags & MF_AMBUSH != 0
        };
        !ambush || P_CheckSight(world, actor_id, current_target(actor_id)) != 0
    };
    if !seen && P_LookForPlayers(world, actor_id, Boolean::FALSE).is_false() {
        return;
    }
    let (info, kind) = {
        let mobjs = MOBJ_ARENA.borrow();
        (mobjs[actor_id].info, mobjs[actor_id].kind)
    };
    if info.get().seesound != Sfx::None {
        let sound = match info.get().seesound {
            Sfx::Posit1 | Sfx::Posit2 | Sfx::Posit3 => Sfx::Posit1 as i32 + P_Random(world) % 3,
            Sfx::Bgsit1 | Sfx::Bgsit2 => Sfx::Bgsit1 as i32 + P_Random(world) % 2,
            s => s as i32,
        };
        if kind as u32 == MT_SPIDER as u32 || kind as u32 == MT_CYBORG as u32 {
            S_StartNonPositionalSound(sound);
        } else {
            S_StartSound(world, actor_id, sound);
        }
    }
    // Sound runs no play code; the actor's type is as read above.
    P_SetMobjState(world, actor_id, info.get().seestate as statenum_t);
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
/// `actor_id` must name a map object with valid `info`. The session
/// settings (`gameskill`, `fastparm`, `netgame`) must be initialised.
pub unsafe fn A_Chase(world: &mut World, actor_id: MobjId) {
    let delta: i32;

    // The actor's type is set at spawn and never changes, so its info is
    // read once; every other field is read afresh after each call that
    // runs play code.
    let (info, has_shootable_target, justattacked) = {
        let mut mobjs = MOBJ_ARENA.borrow_mut();
        let target_live = mobjs[actor_id].target.map(|target| {
            (
                mobjs[target].health > 0,
                mobjs[target].flags & MF_SHOOTABLE != 0,
            )
        });
        let actor = &mut mobjs[actor_id];
        if actor.reactiontime != 0 {
            actor.reactiontime -= 1;
        }
        if actor.threshold != 0 {
            if target_live.is_none_or(|(alive, _)| !alive) {
                actor.threshold = 0;
            } else {
                actor.threshold -= 1;
            }
        }
        if actor.movedir < 8 {
            actor.angle &= ((7) << 29) as angle_t;
            delta = actor.angle.wrapping_sub((actor.movedir << 29) as angle_t) as i32;
            if delta > 0 {
                actor.angle = actor.angle.wrapping_sub((ANG90 / 2) as angle_t);
            } else if delta < 0 {
                actor.angle = actor.angle.wrapping_add((ANG90 / 2) as angle_t);
            }
        }
        let has_shootable_target = target_live.is_some_and(|(_, shootable)| shootable);
        let justattacked = has_shootable_target && actor.flags & MF_JUSTATTACKED != 0;
        if justattacked {
            actor.flags &= !(MF_JUSTATTACKED);
        }
        (actor.info, has_shootable_target, justattacked)
    };
    let info = info.get();
    if !has_shootable_target {
        if P_LookForPlayers(world, actor_id, Boolean::TRUE).is_truthy() {
            return;
        }
        P_SetMobjState(world, actor_id, info.spawnstate as statenum_t);
        return;
    }
    if justattacked {
        if world.session.gameskill != sk_nightmare && world.session.fastparm == 0 {
            P_NewChaseDir(world, actor_id);
        }
        return;
    }
    if info.meleestate != 0 && P_CheckMeleeRange(world, actor_id).is_truthy() {
        if info.attacksound != Sfx::None {
            S_StartSound(world, actor_id, info.attacksound as i32);
        }
        P_SetMobjState(world, actor_id, info.meleestate as statenum_t);
        return;
    }
    if info.missilestate != 0
        && !(world.session.gameskill < sk_nightmare
            && world.session.fastparm == 0
            && MOBJ_ARENA.borrow()[actor_id].movecount != 0)
        && P_CheckMissileRange(world, actor_id).is_truthy()
    {
        P_SetMobjState(world, actor_id, info.missilestate as statenum_t);
        MOBJ_ARENA.borrow_mut()[actor_id].flags |= MF_JUSTATTACKED;
        return;
    }
    if world.session.netgame != 0
        && MOBJ_ARENA.borrow()[actor_id].threshold == 0
        && P_CheckSight(world, actor_id, current_target(actor_id)) == 0
        && P_LookForPlayers(world, actor_id, Boolean::TRUE).is_truthy()
    {
        return;
    }
    let movecount = {
        let mut mobjs = MOBJ_ARENA.borrow_mut();
        let actor = &mut mobjs[actor_id];
        actor.movecount -= 1;
        actor.movecount
    };
    if movecount < 0 || P_Move(world, actor_id).is_false() {
        P_NewChaseDir(world, actor_id);
    }
    if info.activesound != Sfx::None && P_Random(world) < 3 {
        S_StartSound(world, actor_id, info.activesound as i32);
    }
}
/// Turns `actor` to face its `target` and clears the `MF_AMBUSH` flag.
///
/// If the target has `MF_SHADOW` (partial invisibility), the facing angle is
/// perturbed by a random ±21-bit BAM value to simulate aim confusion.
/// Returns immediately if `actor->target` is null.
pub fn A_FaceTarget(world: &mut World, actor_id: MobjId) {
    let shadow = {
        let mut mobjs = MOBJ_ARENA.borrow_mut();
        let Some(target) = mobjs[actor_id].target else {
            return;
        };
        let (target_x, target_y, target_flags) =
            (mobjs[target].x, mobjs[target].y, mobjs[target].flags);
        let actor = &mut mobjs[actor_id];
        actor.flags &= !(MF_AMBUSH);
        actor.angle = R_PointToAngle2(actor.x, actor.y, target_x, target_y);
        target_flags & MF_SHADOW != 0
    };
    if shadow {
        let spread = ((P_Random(world) - P_Random(world)) << 21) as angle_t;
        let mut mobjs = MOBJ_ARENA.borrow_mut();
        let actor = &mut mobjs[actor_id];
        actor.angle = actor.angle.wrapping_add(spread);
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
pub unsafe fn A_PosAttack(world: &mut World, actor_id: MobjId) {
    let mut angle: i32;

    if !has_target(actor_id) {
        return;
    }
    A_FaceTarget(world, actor_id);
    angle = MOBJ_ARENA.borrow()[actor_id].angle as i32;
    let slope: i32 = P_AimLineAttack(world, actor_id, angle as angle_t, MISSILERANGE) as i32;
    S_StartSound(world, actor_id, Sfx::Pistol as i32);
    angle = angle.wrapping_add((P_Random(world) - P_Random(world)) << 20);
    let damage: i32 = (P_Random(world) % 5 + 1) * 3;
    P_LineAttack(
        world,
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
pub unsafe fn A_SPosAttack(world: &mut World, actor_id: MobjId) {
    let mut i: i32;

    let mut angle: i32;

    let mut damage: i32;

    if !has_target(actor_id) {
        return;
    }
    S_StartSound(world, actor_id, Sfx::Shotgn as i32);
    A_FaceTarget(world, actor_id);
    let bangle: i32 = MOBJ_ARENA.borrow()[actor_id].angle as i32;
    let slope: i32 = P_AimLineAttack(world, actor_id, bangle as angle_t, MISSILERANGE) as i32;
    i = 0;
    while i < 3 {
        angle = bangle.wrapping_add((P_Random(world) - P_Random(world)) << 20);
        damage = (P_Random(world) % 5 + 1) * 3;
        P_LineAttack(
            world,
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
pub unsafe fn A_CPosAttack(world: &mut World, actor_id: MobjId) {
    if !has_target(actor_id) {
        return;
    }
    S_StartSound(world, actor_id, Sfx::Shotgn as i32);
    A_FaceTarget(world, actor_id);
    let bangle: i32 = MOBJ_ARENA.borrow()[actor_id].angle as i32;
    let slope: i32 = P_AimLineAttack(world, actor_id, bangle as angle_t, MISSILERANGE) as i32;
    let angle: i32 = bangle.wrapping_add((P_Random(world) - P_Random(world)) << 20);
    let damage: i32 = (P_Random(world) % 5 + 1) * 3;
    P_LineAttack(
        world,
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
pub unsafe fn A_CPosRefire(world: &mut World, actor_id: MobjId) {
    A_FaceTarget(world, actor_id);
    if P_Random(world) < 40 {
        return;
    }
    stop_firing_unless_target_seen(world, actor_id);
}
/// The refire test shared by `A_CPosRefire` and `A_SpidRefire`: go back to
/// the see state if the target is gone, dead or out of sight.
///
/// # Safety
///
/// Runs [`P_SetMobjState`] on `actor_id`.
unsafe fn stop_firing_unless_target_seen(world: &mut World, actor_id: MobjId) {
    let target_dead = {
        let mobjs = MOBJ_ARENA.borrow();
        mobjs[actor_id]
            .target
            .is_none_or(|target| mobjs[target].health <= 0)
    };
    if target_dead || P_CheckSight(world, actor_id, current_target(actor_id)) == 0 {
        let seestate = MOBJ_ARENA.borrow()[actor_id].info.get().seestate;
        P_SetMobjState(world, actor_id, seestate as statenum_t);
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
pub unsafe fn A_SpidRefire(world: &mut World, actor_id: MobjId) {
    A_FaceTarget(world, actor_id);
    if P_Random(world) < 10 {
        return;
    }
    stop_firing_unless_target_seen(world, actor_id);
}
/// Attack action for the Arachnotron: launches one `MT_ARACHPLAZ` plasma ball.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_BspiAttack(world: &mut World, actor_id: MobjId) {
    if !has_target(actor_id) {
        return;
    }
    A_FaceTarget(world, actor_id);
    P_SpawnMissile(world, actor_id, current_target(actor_id), MT_ARACHPLAZ);
}
/// Attack action for the Imp: claw swipe in melee range, fireball at distance.
///
/// If in melee range plays `sfx_claw` and deals `(rnd%8+1)*3` damage (3-24).
/// Otherwise launches a `MT_TROOPSHOT` fireball.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_TroopAttack(world: &mut World, actor_id: MobjId) {
    let damage: i32;

    if !has_target(actor_id) {
        return;
    }
    A_FaceTarget(world, actor_id);
    if P_CheckMeleeRange(world, actor_id).is_truthy() {
        S_StartSound(world, actor_id, Sfx::Claw as i32);
        damage = (P_Random(world) % 8 + 1) * 3;
        P_DamageMobj(
            world,
            current_target(actor_id),
            Some(actor_id),
            Some(actor_id),
            damage,
        );
        return;
    }
    P_SpawnMissile(world, actor_id, current_target(actor_id), MT_TROOPSHOT);
}
/// Attack action for the Demon (Sarg): melee-only bite dealing `(rnd%10+1)*4` damage (4-40).
///
/// Only damages the target when within melee range; no ranged fallback.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_SargAttack(world: &mut World, actor_id: MobjId) {
    let damage: i32;

    if !has_target(actor_id) {
        return;
    }
    A_FaceTarget(world, actor_id);
    if P_CheckMeleeRange(world, actor_id).is_truthy() {
        damage = (P_Random(world) % 10 + 1) * 4;
        P_DamageMobj(
            world,
            current_target(actor_id),
            Some(actor_id),
            Some(actor_id),
            damage,
        );
    }
}
/// Attack action for the Cacodemon: bite in melee range, fireball at distance.
///
/// Melee deals `(rnd%6+1)*10` damage (10-60). Ranged fires `MT_HEADSHOT`.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_HeadAttack(world: &mut World, actor_id: MobjId) {
    let damage: i32;

    if !has_target(actor_id) {
        return;
    }
    A_FaceTarget(world, actor_id);
    if P_CheckMeleeRange(world, actor_id).is_truthy() {
        damage = (P_Random(world) % 6 + 1) * 10;
        P_DamageMobj(
            world,
            current_target(actor_id),
            Some(actor_id),
            Some(actor_id),
            damage,
        );
        return;
    }
    P_SpawnMissile(world, actor_id, current_target(actor_id), MT_HEADSHOT);
}
/// Attack action for the Cyberdemon: launches one `MT_ROCKET`.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_CyberAttack(world: &mut World, actor_id: MobjId) {
    if !has_target(actor_id) {
        return;
    }
    A_FaceTarget(world, actor_id);
    P_SpawnMissile(world, actor_id, current_target(actor_id), MT_ROCKET);
}
/// Attack action for the Baron of Hell / Hell Knight: claw in melee, plasma ball at distance.
///
/// Melee plays `sfx_claw` and deals `(rnd%8+1)*10` damage (10-80). Ranged fires
/// `MT_BRUISERSHOT`.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_BruisAttack(world: &mut World, actor_id: MobjId) {
    let damage: i32;

    if !has_target(actor_id) {
        return;
    }
    if P_CheckMeleeRange(world, actor_id).is_truthy() {
        S_StartSound(world, actor_id, Sfx::Claw as i32);
        damage = (P_Random(world) % 8 + 1) * 10;
        P_DamageMobj(
            world,
            current_target(actor_id),
            Some(actor_id),
            Some(actor_id),
            damage,
        );
        return;
    }
    P_SpawnMissile(world, actor_id, current_target(actor_id), MT_BRUISERSHOT);
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
pub unsafe fn A_SkelMissile(world: &mut World, actor_id: MobjId) {
    if !has_target(actor_id) {
        return;
    }
    A_FaceTarget(world, actor_id);
    MOBJ_ARENA.borrow_mut()[actor_id].z += 16 * FRACUNIT;
    let mo_id = P_SpawnMissile(world, actor_id, current_target(actor_id), MT_TRACER);
    let mut mobjs = MOBJ_ARENA.borrow_mut();
    mobjs[actor_id].z -= 16 * FRACUNIT;
    let actor_target = mobjs[actor_id].target;
    let mo = &mut mobjs[mo_id];
    mo.x += mo.momx;
    mo.y += mo.momy;
    mo.tracer = actor_target;
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
pub unsafe fn A_Tracer(world: &mut World, actor_id: MobjId) {
    let mut exact: angle_t;

    let mut dist: fixed_t;

    if gametic() & 3 != 0 {
        return;
    }
    let (x, y, z) = {
        let mobjs = MOBJ_ARENA.borrow();
        (mobjs[actor_id].x, mobjs[actor_id].y, mobjs[actor_id].z)
    };
    P_SpawnPuff(world, x, y, z);
    // The puff's spawn runs play code; the actor is read afresh.
    let (x, y, z) = {
        let mobjs = MOBJ_ARENA.borrow();
        let actor = &mobjs[actor_id];
        (actor.x - actor.momx, actor.y - actor.momy, actor.z)
    };
    let th_id = P_SpawnMobj(world, x, y, z, MT_SMOKE);
    let tic_spread = P_Random(world) & 3;
    let mut mobjs = MOBJ_ARENA.borrow_mut();
    let th = &mut mobjs[th_id];
    th.momz = FRACUNIT as fixed_t;
    th.tics -= tic_spread;
    if th.tics < 1 {
        th.tics = 1;
    }
    // The tracer may be stale: C reads the dangling pointer's last data.
    let Some(dest_id) = mobjs[actor_id].tracer else {
        return;
    };
    let dest = &mobjs[dest_id];
    if dest.health <= 0 {
        return;
    }
    let (dest_x, dest_y, dest_z) = (dest.x, dest.y, dest.z);
    let actor = &mut mobjs[actor_id];
    exact = R_PointToAngle2(actor.x, actor.y, dest_x, dest_y);
    if exact != actor.angle {
        if exact.wrapping_sub(actor.angle) > 0x80000000 {
            actor.angle = actor.angle.wrapping_sub(TRACEANGLE as angle_t);
            if exact.wrapping_sub(actor.angle) < 0x80000000 {
                actor.angle = exact;
            }
        } else {
            actor.angle = actor.angle.wrapping_add(TRACEANGLE as angle_t);
            if exact.wrapping_sub(actor.angle) > 0x80000000 {
                actor.angle = exact;
            }
        }
    }
    exact = actor.angle >> ANGLETOFINESHIFT;
    let speed = actor.info.get().speed;
    // `finecosine[exact]`: the cosine table is the sine table a quarter
    // turn on.
    actor.momx = FixedMul(speed as fixed_t, finesine[exact as usize + FINEANGLES / 4]);
    actor.momy = FixedMul(speed as fixed_t, finesine[exact as usize]);
    dist = P_AproxDistance(dest_x - actor.x, dest_y - actor.y);
    dist = (dist as i32 / speed) as fixed_t;
    if dist < 1 {
        dist = 1 as fixed_t;
    }
    let slope: fixed_t = (dest_z + 40 as fixed_t * FRACUNIT - actor.z) / dist;
    if slope < actor.momz {
        actor.momz -= FRACUNIT / 8;
    } else {
        actor.momz += FRACUNIT / 8;
    };
}
/// Revenant melee wind-up: faces the target and plays the whoosh sound.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_SkelWhoosh(world: &mut World, actor_id: MobjId) {
    if !has_target(actor_id) {
        return;
    }
    A_FaceTarget(world, actor_id);
    S_StartSound(world, actor_id, Sfx::Skeswg as i32);
}
/// Revenant melee strike: deals `(rnd%10+1)*6` damage (6-60) when in melee range.
///
/// Plays `sfx_skepch` on a successful hit.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_SkelFist(world: &mut World, actor_id: MobjId) {
    let damage: i32;

    if !has_target(actor_id) {
        return;
    }
    A_FaceTarget(world, actor_id);
    if P_CheckMeleeRange(world, actor_id).is_truthy() {
        damage = (P_Random(world) % 10 + 1) * 6;
        S_StartSound(world, actor_id, Sfx::Skepch as i32);
        P_DamageMobj(
            world,
            current_target(actor_id),
            Some(actor_id),
            Some(actor_id),
            damage,
        );
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
/// `viletryx`/`viletryy` and `vileobj` must have been set by `A_VileChase`.
/// Called through `P_BlockThingsIterator`; the fit test runs
/// [`P_CheckPosition`].
pub unsafe fn PIT_VileCheck(world: &mut World, thing_id: MobjId) -> Boolean {
    let (x, y) = {
        let mut mobjs = MOBJ_ARENA.borrow_mut();
        let thing = &mut mobjs[thing_id];
        if thing.flags & MF_CORPSE == 0 {
            return Boolean::TRUE;
        }
        if thing.tics != -1 {
            return Boolean::TRUE;
        }
        if thing.info.get().raisestate == S_NULL {
            return Boolean::TRUE;
        }
        let maxdist: i32 = thing.info.get().radius + MOBJINFO.borrow()[MT_VILE as usize].radius;
        let (viletryx, viletryy) = (world.scratch.enemy.viletryx, world.scratch.enemy.viletryy);
        if (thing.x - viletryx).abs() > maxdist || (thing.y - viletryy).abs() > maxdist {
            return Boolean::TRUE;
        }
        world.scratch.enemy.corpsehit = Some(thing_id);
        let corpsehit = thing;
        corpsehit.momy = 0 as fixed_t;
        corpsehit.momx = corpsehit.momy;
        corpsehit.height <<= 2;
        (corpsehit.x, corpsehit.y)
    };
    let check: Boolean = Boolean::from_raw(P_CheckPosition(world, thing_id, x, y));
    MOBJ_ARENA.borrow_mut()[thing_id].height >>= 2;
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
pub unsafe fn A_VileChase(world: &mut World, actor_id: MobjId) {
    let xl: i32;

    let xh: i32;

    let yl: i32;

    let yh: i32;

    let mut bx: i32;

    let mut by: i32;

    let temp: Option<MobjId>;

    let look_ahead = {
        let mobjs = MOBJ_ARENA.borrow();
        let actor = &mobjs[actor_id];
        (actor.movedir != DI_NODIR).then(|| {
            let speed = actor.info.get().speed as fixed_t;
            (
                actor.x + speed * xspeed[actor.movedir as usize],
                actor.y + speed * yspeed[actor.movedir as usize],
            )
        })
    };
    if let Some((viletryx, viletryy)) = look_ahead {
        let (bmaporgx, bmaporgy) = world.level.blockmap_origin();
        xl = (viletryx - bmaporgx - 32 * FRACUNIT * 2) >> MAPBLOCKSHIFT;
        xh = (viletryx - bmaporgx + 32 * FRACUNIT * 2) >> MAPBLOCKSHIFT;
        yl = (viletryy - bmaporgy - 32 * FRACUNIT * 2) >> MAPBLOCKSHIFT;
        yh = (viletryy - bmaporgy + 32 * FRACUNIT * 2) >> MAPBLOCKSHIFT;
        {
            let enemy = &mut world.scratch.enemy;
            enemy.viletryx = viletryx;
            enemy.viletryy = viletryy;
            enemy.vileobj = Some(actor_id);
        }
        bx = xl;
        while bx <= xh {
            by = yl;
            while by <= yh {
                // SAFETY: `viletryx`/`viletryy` are set above.
                if !P_BlockThingsIterator(world, bx, by, |world, thing| unsafe {
                    PIT_VileCheck(world, thing).is_truthy()
                }) {
                    let corpsehit_id = world
                        .scratch
                        .enemy
                        .corpsehit
                        .expect("the corpse search stopped without a corpse");
                    {
                        let actor = &mut MOBJ_ARENA.borrow_mut()[actor_id];
                        temp = actor.target;
                        actor.target = Some(corpsehit_id);
                    }
                    A_FaceTarget(world, actor_id);
                    MOBJ_ARENA.borrow_mut()[actor_id].target = temp;
                    P_SetMobjState(world, actor_id, S_VILE_HEAL1);
                    S_StartSound(world, corpsehit_id, Sfx::Slop as i32);
                    let info = MOBJ_ARENA.borrow()[corpsehit_id].info.get();
                    P_SetMobjState(world, corpsehit_id, info.raisestate as statenum_t);
                    let mut mobjs = MOBJ_ARENA.borrow_mut();
                    let corpsehit = &mut mobjs[corpsehit_id];
                    corpsehit.height <<= 2;
                    corpsehit.flags = info.flags;
                    corpsehit.health = info.spawnhealth;
                    corpsehit.target = None;
                    return;
                }
                by += 1;
            }
            bx += 1;
        }
    }
    A_Chase(world, actor_id);
}
/// Archvile attack wind-up: plays the attack start sound (`sfx_vilatk`).
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_VileStart(world: &mut World, actor_id: MobjId) {
    S_StartSound(world, actor_id, Sfx::Vilatk as i32);
}
/// Archvile fire start: plays the flame start sound then positions the fire object.
///
/// # Safety
///
/// `actor_id` must name a map object of the level (see [`A_Fire`]).
pub unsafe fn A_StartFire(world: &mut World, actor_id: MobjId) {
    S_StartSound(world, actor_id, Sfx::Flamst as i32);
    A_Fire(world, actor_id);
}
/// Archvile fire crackle: plays the sustained flame sound then repositions the fire object.
///
/// # Safety
///
/// `actor_id` must name a map object of the level (see [`A_Fire`]).
pub unsafe fn A_FireCrackle(world: &mut World, actor_id: MobjId) {
    S_StartSound(world, actor_id, Sfx::Flame as i32);
    A_Fire(world, actor_id);
}
/// Repositions the Archvile fire object 24 units in front of its tracer (the victim).
///
/// Does nothing if `actor->tracer` is `None`. Checks sight from the Archvile's `target`
/// (a dummy when `None`, see [`P_SubstNullMobj`]) to the tracer; if the vile lost
/// sight of its victim, the fire does not move.
/// Uses `P_UnsetThingPosition`/`P_SetThingPosition` to maintain blockmap consistency.
pub fn A_Fire(world: &mut World, actor_id: MobjId) {
    let (tracer, target) = {
        let mobjs = MOBJ_ARENA.borrow();
        (mobjs[actor_id].tracer, mobjs[actor_id].target)
    };
    let Some(dest_id) = tracer else {
        return;
    };
    let target = P_SubstNullMobj(world, target);
    if P_CheckSight(world, target, dest_id) == 0 {
        return;
    }
    P_UnsetThingPosition(world, actor_id);
    {
        let mut mobjs = MOBJ_ARENA.borrow_mut();
        let dest = &mobjs[dest_id];
        let an = (dest.angle >> ANGLETOFINESHIFT) as usize;
        // `finecosine[an]`: the cosine table is the sine table a quarter
        // turn on.
        let x = dest.x + FixedMul(24 as fixed_t * FRACUNIT, finesine[an + FINEANGLES / 4]);
        let y = dest.y + FixedMul(24 as fixed_t * FRACUNIT, finesine[an]);
        let z = dest.z;
        let actor = &mut mobjs[actor_id];
        actor.x = x;
        actor.y = y;
        actor.z = z;
    }
    P_SetThingPosition(world, actor_id);
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
pub unsafe fn A_VileTarget(world: &mut World, actor_id: MobjId) {
    if !has_target(actor_id) {
        return;
    }
    A_FaceTarget(world, actor_id);
    // FIXME: p_enemy.c passes `actor->target->x` for the Y argument instead of
    // `actor->target->y` - this is a vanilla Doom bug reproduced faithfully here.
    let (target_x, target_z) = {
        let mobjs = MOBJ_ARENA.borrow();
        let target = &mobjs[target_of(&mobjs[actor_id])];
        (target.x, target.z)
    };
    let fog_id = P_SpawnMobj(world, target_x, target_x, target_z, MT_FIRE);
    {
        let mut mobjs = MOBJ_ARENA.borrow_mut();
        mobjs[actor_id].tracer = Some(fog_id);
        mobjs[fog_id].target = Some(actor_id);
        mobjs[fog_id].tracer = mobjs[actor_id].target;
    }
    A_Fire(world, fog_id);
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
pub unsafe fn A_VileAttack(world: &mut World, actor_id: MobjId) {
    if !has_target(actor_id) {
        return;
    }
    A_FaceTarget(world, actor_id);
    if P_CheckSight(world, actor_id, current_target(actor_id)) == 0 {
        return;
    }
    S_StartSound(world, actor_id, Sfx::Barexp as i32);
    P_DamageMobj(
        world,
        current_target(actor_id),
        Some(actor_id),
        Some(actor_id),
        20,
    );
    // Nothing from here on changes `actor->target`, so C's repeated reads
    // after the damage are one read here.
    let fire_id = {
        let mut mobjs = MOBJ_ARENA.borrow_mut();
        let target_id = target_of(&mobjs[actor_id]);
        let target = &mut mobjs[target_id];
        target.momz = (1000 * FRACUNIT / target.info.get().mass) as fixed_t;
        let (target_x, target_y) = (target.x, target.y);
        let actor = &mobjs[actor_id];
        let an = (actor.angle >> ANGLETOFINESHIFT) as usize;
        let Some(fire_id) = actor.tracer else {
            return;
        };
        let fire = &mut mobjs[fire_id];
        // `finecosine[an]`: the cosine table is the sine table a quarter
        // turn on.
        fire.x = target_x - FixedMul(24 as fixed_t * FRACUNIT, finesine[an + FINEANGLES / 4]);
        fire.y = target_y - FixedMul(24 as fixed_t * FRACUNIT, finesine[an]);
        fire_id
    };
    P_RadiusAttack(world, fire_id, Some(actor_id), 70);
}
/// Mancubus raise/taunt: faces the target and plays the attack preparation sound.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_FatRaise(world: &mut World, actor_id: MobjId) {
    A_FaceTarget(world, actor_id);
    S_StartSound(world, actor_id, Sfx::Manatk as i32);
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
pub unsafe fn A_FatAttack1(world: &mut World, actor_id: MobjId) {
    A_FaceTarget(world, actor_id);
    let target = {
        let actor = &mut MOBJ_ARENA.borrow_mut()[actor_id];
        actor.angle = actor.angle.wrapping_add(FATSPREAD as angle_t);
        actor.target
    };
    let target = P_SubstNullMobj(world, target);
    P_SpawnMissile(world, actor_id, target, MT_FATSHOT);
    let mo_id = P_SpawnMissile(world, actor_id, target, MT_FATSHOT);
    turn_missile(mo_id, FATSPREAD as angle_t);
}
/// Turn missile `mo_id` by `turn` (wrapping) and point its momentum along
/// the new angle at its type's speed: the second Mancubus fireball's
/// spread.
fn turn_missile(mo_id: MobjId, turn: angle_t) {
    let mut mobjs = MOBJ_ARENA.borrow_mut();
    let mo = &mut mobjs[mo_id];
    mo.angle = mo.angle.wrapping_add(turn);
    let an = (mo.angle >> ANGLETOFINESHIFT) as usize;
    let speed = mo.info.get().speed as fixed_t;
    // `finecosine[an]`: the cosine table is the sine table a quarter turn
    // on.
    mo.momx = FixedMul(speed, finesine[an + FINEANGLES / 4]);
    mo.momy = FixedMul(speed, finesine[an]);
}
/// Mancubus attack phase 2: two `MT_FATSHOT` fireballs spread to the left.
///
/// Rotates the actor's angle by `-FATSPREAD`, fires one shot directly, then fires a
/// second shot additionally rotated by `-2*FATSPREAD` with velocity recomputed.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_FatAttack2(world: &mut World, actor_id: MobjId) {
    A_FaceTarget(world, actor_id);
    let target = {
        let actor = &mut MOBJ_ARENA.borrow_mut()[actor_id];
        actor.angle = actor.angle.wrapping_sub(FATSPREAD as angle_t);
        actor.target
    };
    let target = P_SubstNullMobj(world, target);
    P_SpawnMissile(world, actor_id, target, MT_FATSHOT);
    let mo_id = P_SpawnMissile(world, actor_id, target, MT_FATSHOT);
    turn_missile(mo_id, ((FATSPREAD * 2) as angle_t).wrapping_neg());
}
/// Mancubus attack phase 3: two `MT_FATSHOT` fireballs spread symmetrically ±FATSPREAD/2.
///
/// Fires one shot rotated `-FATSPREAD/2` and a second at `+FATSPREAD/2`; velocities are
/// recomputed for each so they actually travel in the spread directions.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_FatAttack3(world: &mut World, actor_id: MobjId) {
    A_FaceTarget(world, actor_id);
    let target = MOBJ_ARENA.borrow()[actor_id].target;
    let target = P_SubstNullMobj(world, target);
    let mo_id = P_SpawnMissile(world, actor_id, target, MT_FATSHOT);
    turn_missile(mo_id, ((FATSPREAD / 2) as angle_t).wrapping_neg());
    let mo_id = P_SpawnMissile(world, actor_id, target, MT_FATSHOT);
    turn_missile(mo_id, (FATSPREAD / 2) as angle_t);
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
pub unsafe fn A_SkullAttack(world: &mut World, actor_id: MobjId) {
    let mut dist: i32;

    let (dest_id, attacksound) = {
        let mut mobjs = MOBJ_ARENA.borrow_mut();
        let actor = &mut mobjs[actor_id];
        let Some(dest_id) = actor.target else {
            return;
        };
        actor.flags |= MF_SKULLFLY;
        (dest_id, actor.info.get().attacksound)
    };
    S_StartSound(world, actor_id, attacksound as i32);
    A_FaceTarget(world, actor_id);
    let mut mobjs = MOBJ_ARENA.borrow_mut();
    let actor = &mut mobjs[actor_id];
    let an = (actor.angle >> ANGLETOFINESHIFT) as usize;
    // `finecosine[an]`: the cosine table is the sine table a quarter turn
    // on.
    actor.momx = FixedMul(SKULLSPEED, finesine[an + FINEANGLES / 4]);
    actor.momy = FixedMul(SKULLSPEED, finesine[an]);
    let (actor_x, actor_y, actor_z) = (actor.x, actor.y, actor.z);
    let dest = &mobjs[dest_id];
    dist = P_AproxDistance(dest.x - actor_x, dest.y - actor_y) as i32;
    dist /= SKULLSPEED;
    if dist < 1 {
        dist = 1;
    }
    let momz = ((dest.z + (dest.height >> 1) - actor_z) / dist) as fixed_t;
    mobjs[actor_id].momz = momz;
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
pub unsafe fn A_PainShootSkull(world: &mut World, actor_id: MobjId, angle: angle_t) {
    let (x, y, z) = {
        let mobjs = MOBJ_ARENA.borrow();
        let mut count: i32 = 0;
        for thinker in thinkers() {
            if let ThinkerId::Mobj(mobj) = thinker
                && thinker.kind() == ThinkerKind::Mobj
                && mobjs[mobj].kind as u32 == MT_SKULL as u32
            {
                count += 1;
            }
        }
        if count > 20 {
            return;
        }
        let actor = &mobjs[actor_id];
        let an = (angle >> ANGLETOFINESHIFT) as usize;
        let prestep: i32 = 4 * FRACUNIT
            + 3 * (actor.info.get().radius + MOBJINFO.borrow()[MT_SKULL as usize].radius) / 2;
        // `finecosine[an]`: the cosine table is the sine table a quarter
        // turn on.
        (
            actor.x + FixedMul(prestep as fixed_t, finesine[an + FINEANGLES / 4]),
            actor.y + FixedMul(prestep as fixed_t, finesine[an]),
            (actor.z + 8 * FRACUNIT) as fixed_t,
        )
    };
    let newmobj_id = P_SpawnMobj(world, x, y, z, MT_SKULL);
    let (newmobj_x, newmobj_y) = {
        let mobjs = MOBJ_ARENA.borrow();
        (mobjs[newmobj_id].x, mobjs[newmobj_id].y)
    };
    if P_TryMove(world, newmobj_id, newmobj_x, newmobj_y) == 0 {
        P_DamageMobj(world, newmobj_id, Some(actor_id), Some(actor_id), 10000);
        return;
    }
    {
        let mut mobjs = MOBJ_ARENA.borrow_mut();
        mobjs[newmobj_id].target = mobjs[actor_id].target;
    }
    A_SkullAttack(world, newmobj_id);
}
/// Pain Elemental attack: spawns a Lost Soul in the direction the actor is facing.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_PainAttack(world: &mut World, actor_id: MobjId) {
    if !has_target(actor_id) {
        return;
    }
    A_FaceTarget(world, actor_id);
    let angle = MOBJ_ARENA.borrow()[actor_id].angle;
    A_PainShootSkull(world, actor_id, angle);
}
/// Pain Elemental death: drops to the ground and spits three Lost Souls in cardinal directions.
///
/// Calls `A_Fall` to clear `MF_SOLID`, then spawns skulls at `angle+90`, `angle+180`,
/// and `angle+270` relative to the current facing direction.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_PainDie(world: &mut World, actor_id: MobjId) {
    A_Fall(world, actor_id);
    // Each skull's spawn runs play code, so the angle is read afresh.
    let angle = || MOBJ_ARENA.borrow()[actor_id].angle;
    A_PainShootSkull(world, actor_id, angle().wrapping_add(ANG90 as angle_t));
    A_PainShootSkull(world, actor_id, angle().wrapping_add(ANG180));
    A_PainShootSkull(world, actor_id, angle().wrapping_add(ANG270));
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
pub unsafe fn A_Scream(world: &mut World, actor_id: MobjId) {
    let (info, kind) = {
        let mobjs = MOBJ_ARENA.borrow();
        (mobjs[actor_id].info, mobjs[actor_id].kind)
    };
    let sound = match info.get().deathsound {
        Sfx::None => return,
        Sfx::Podth1 | Sfx::Podth2 | Sfx::Podth3 => Sfx::Podth1 as i32 + P_Random(world) % 3,
        Sfx::Bgdth1 | Sfx::Bgdth2 => Sfx::Bgdth1 as i32 + P_Random(world) % 2,
        s => s as i32,
    };
    if kind as u32 == MT_SPIDER as u32 || kind as u32 == MT_CYBORG as u32 {
        S_StartNonPositionalSound(sound);
    } else {
        S_StartSound(world, actor_id, sound);
    };
}
/// Plays the player/monster gibbing scream (`sfx_slop`) positioned at the actor.
///
/// # Safety
///
/// `actor_id` must name a map object of the level.
pub unsafe fn A_XScream(world: &mut World, actor_id: MobjId) {
    S_StartSound(world, actor_id, Sfx::Slop as i32);
}
/// Plays the monster's pain sound if it has one.
///
/// # Safety
///
/// `actor_id` must name a map object with a valid `info`.
pub unsafe fn A_Pain(world: &mut World, actor_id: MobjId) {
    let painsound = MOBJ_ARENA.borrow()[actor_id].info.get().painsound;
    if painsound != Sfx::None {
        S_StartSound(world, actor_id, painsound as i32);
    }
}
/// Clears `MF_SOLID` so the falling corpse can be walked over.
pub fn A_Fall(_world: &mut World, actor_id: MobjId) {
    MOBJ_ARENA.borrow_mut()[actor_id].flags &= !(MF_SOLID);
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
pub unsafe fn A_Explode(world: &mut World, thingy: MobjId) {
    let target = MOBJ_ARENA.borrow()[thingy].target;
    P_RadiusAttack(world, thingy, target, 128);
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
fn CheckBossEnd(session: &Session, motype: mobjtype_t) -> Boolean {
    let (gameepisode, gamemap) = (session.gameepisode, session.gamemap);
    if (gameversion() as u32) < exe_ultimate as u32 {
        if gamemap != 8 {
            return Boolean::FALSE;
        }
        if motype == MT_BRUISER && gameepisode != 1 {
            return Boolean::FALSE;
        }
        Boolean::TRUE
    } else {
        match gameepisode {
            1 => Boolean::from(gamemap == 8 && motype == MT_BRUISER),
            2 => Boolean::from(gamemap == 8 && motype == MT_CYBORG),
            3 => Boolean::from(gamemap == 8 && motype == MT_SPIDER),
            4 => Boolean::from(
                gamemap == 6 && motype == MT_CYBORG || gamemap == 8 && motype == MT_SPIDER,
            ),
            _ => Boolean::from(gamemap == 8),
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
pub fn A_BossDeath(world: &mut World, mo_id: MobjId) {
    // The type is set at spawn and never changes; the specials below
    // cannot change it either.
    let kind = MOBJ_ARENA.borrow()[mo_id].kind;
    let mut junk: line_t = JUNK_LINE;
    let _i: i32 = 0;
    if gamemode() as u32 == commercial as u32 {
        if world.session.gamemap != 7 {
            return;
        }
        if kind as u32 != MT_FATSO as u32 && kind as u32 != MT_BABY as u32 {
            return;
        }
    } else if CheckBossEnd(&world.session, kind).is_false() {
        return;
    }
    let mut i: usize = 0;
    while i < MAXPLAYERS {
        if world.session.in_game(i) && world.players[i].health > 0 {
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
        if world.session.gamemap == 7 {
            if kind as u32 == MT_FATSO as u32 {
                junk.tag = 666;
                EV_DoFloor(world, &junk, lowerFloorToLowest);
                return;
            }
            if kind as u32 == MT_BABY as u32 {
                junk.tag = 667;
                EV_DoFloor(world, &junk, raiseToTexture);
                return;
            }
        }
    } else {
        match world.session.gameepisode {
            1 => {
                junk.tag = 666;
                EV_DoFloor(world, &junk, lowerFloorToLowest);
                return;
            }
            4 => match world.session.gamemap {
                6 => {
                    junk.tag = 666;
                    EV_DoDoor(world, &junk, vld_blazeOpen);
                    return;
                }
                8 => {
                    junk.tag = 666;
                    EV_DoFloor(world, &junk, lowerFloorToLowest);
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
pub unsafe fn A_Hoof(world: &mut World, mo_id: MobjId) {
    S_StartSound(world, mo_id, Sfx::Hoof as i32);
    A_Chase(world, mo_id);
}
/// Spider Mastermind footstep: plays `sfx_metal` then advances via `A_Chase`.
///
/// # Safety
///
/// `mo_id` must name a map object with a valid `info`.
pub unsafe fn A_Metal(world: &mut World, mo_id: MobjId) {
    S_StartSound(world, mo_id, Sfx::Metal as i32);
    A_Chase(world, mo_id);
}
/// Arachnotron footstep: plays `sfx_bspwlk` then advances via `A_Chase`.
///
/// # Safety
///
/// `mo_id` must name a map object with a valid `info`.
pub unsafe fn A_BabyMetal(world: &mut World, mo_id: MobjId) {
    S_StartSound(world, mo_id, Sfx::Bspwlk as i32);
    A_Chase(world, mo_id);
}
/// Super Shotgun open action: plays the barrel-break sound (`sfx_dbopn`) for the player.
///
/// # Safety
///
/// `player` must be spawned in the level.
pub unsafe fn A_OpenShotgun2(world: &mut World, player: PlayerId, _psprite: usize) {
    S_StartSound(world, world.players[player].mo(), Sfx::Dbopn as i32);
}
/// Super Shotgun load action: plays the shell-load sound (`sfx_dbload`) for the player.
///
/// # Safety
///
/// `player` must be spawned in the level.
pub unsafe fn A_LoadShotgun2(world: &mut World, player: PlayerId, _psprite: usize) {
    S_StartSound(world, world.players[player].mo(), Sfx::Dbload as i32);
}
/// Super Shotgun close action: plays the close sound (`sfx_dbcls`) then checks for refire.
///
/// # Safety
///
/// `player` must be spawned in the level.
pub unsafe fn A_CloseShotgun2(world: &mut World, player: PlayerId, psprite: usize) {
    S_StartSound(world, world.players[player].mo(), Sfx::Dbcls as i32);
    A_ReFire(world, player, psprite);
}
/// Icon of Sin wake-up: scans the thinker list for `MT_BOSSTARGET` spots and plays `sfx_bossit`.
///
/// Populates `braintargets` and resets `numbraintargets`/`braintargeton` to 0.
/// The target spots are placed by the level designer in the map; the brain cycles
/// through them to choose where to spit monster cubes.
///
pub fn A_BrainAwake(world: &mut World, _mo: MobjId) {
    {
        let enemy = &mut world.scratch.enemy;
        enemy.numbraintargets = 0;
        enemy.braintargeton = 0;
        let mobjs = MOBJ_ARENA.borrow();
        for thinker in thinkers() {
            if let ThinkerId::Mobj(mobj) = thinker
                && thinker.kind() == ThinkerKind::Mobj
                && mobjs[mobj].kind as u32 == MT_BOSSTARGET as u32
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
pub fn A_BrainPain(_world: &mut World, _mo: MobjId) {
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
pub unsafe fn A_BrainScream(world: &mut World, mo_id: MobjId) {
    let mut x: i32;

    let mut y: i32;

    let mut z: i32;

    // Each rocket's spawn and state change run play code, so the brain's
    // position is read afresh for every one, as C reads it.
    let position = || {
        let mobjs = MOBJ_ARENA.borrow();
        (mobjs[mo_id].x, mobjs[mo_id].y)
    };
    x = position().0 - 196 * FRACUNIT;
    while x < position().0 + 320 * FRACUNIT {
        y = position().1 - 320 * FRACUNIT;
        z = 128 + P_Random(world) * 2 * FRACUNIT;
        let th_id = P_SpawnMobj(world, x as fixed_t, y as fixed_t, z as fixed_t, MT_ROCKET);
        let momz = (P_Random(world) * 512) as fixed_t;
        MOBJ_ARENA.borrow_mut()[th_id].momz = momz;
        P_SetMobjState(world, th_id, S_BRAINEXPLODE1);
        randomise_explosion_tics(world, th_id);
        x += FRACUNIT * 8;
    }
    S_StartNonPositionalSound(Sfx::Bosdth as i32);
}
/// Shorten brain-explosion rocket `th_id`'s first state by a random 0..=7
/// tics, keeping at least one (the tail of `A_BrainScream` and
/// `A_BrainExplode`).
fn randomise_explosion_tics(world: &mut World, th_id: MobjId) {
    let tic_spread = P_Random(world) & 7;
    let mut mobjs = MOBJ_ARENA.borrow_mut();
    let th = &mut mobjs[th_id];
    th.tics -= tic_spread;
    if th.tics < 1 {
        th.tics = 1;
    }
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
pub unsafe fn A_BrainExplode(world: &mut World, mo_id: MobjId) {
    let (mo_x, mo_y) = {
        let mobjs = MOBJ_ARENA.borrow();
        (mobjs[mo_id].x, mobjs[mo_id].y)
    };
    let x: i32 = mo_x + (P_Random(world) - P_Random(world)) * 2048;
    let y: i32 = mo_y;
    let z: i32 = 128 + P_Random(world) * 2 * FRACUNIT;
    let th_id = P_SpawnMobj(world, x as fixed_t, y as fixed_t, z as fixed_t, MT_ROCKET);
    let momz = (P_Random(world) * 512) as fixed_t;
    MOBJ_ARENA.borrow_mut()[th_id].momz = momz;
    P_SetMobjState(world, th_id, S_BRAINEXPLODE1);
    randomise_explosion_tics(world, th_id);
}
/// Icon of Sin death: ends the level via `G_ExitLevel`.
pub fn A_BrainDie(_world: &mut World, _mo: MobjId) {
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
pub unsafe fn A_BrainSpit(world: &mut World, mo_id: MobjId) {
    let targ_id = {
        let gameskill = world.session.gameskill;
        let enemy = &mut world.scratch.enemy;
        enemy.brainspit_easy ^= 1;
        if gameskill <= sk_easy && enemy.brainspit_easy == 0 {
            return;
        }
        let targ = enemy.braintargets[enemy.braintargeton as usize];
        enemy.braintargeton = (enemy.braintargeton + 1) % enemy.numbraintargets;
        targ.expect("brain spit at a missing target spot")
    };
    let newmobj_id = P_SpawnMissile(world, mo_id, targ_id, MT_SPAWNSHOT);
    {
        let mut mobjs = MOBJ_ARENA.borrow_mut();
        mobjs[newmobj_id].target = Some(targ_id);
        let travel = mobjs[targ_id].y - mobjs[mo_id].y;
        let newmobj = &mut mobjs[newmobj_id];
        newmobj.reactiontime = travel / newmobj.momy / newmobj.state.get().tics;
    }
    S_StartNonPositionalSound(Sfx::Bospit as i32);
}
/// In-flight cube sound: plays `sfx_boscub` while the cube travels, then calls `A_SpawnFly`.
///
/// # Safety
///
/// `mo_id` must name a map object of the level. Called through the state-machine
/// action pointer.
pub unsafe fn A_SpawnSound(world: &mut World, mo_id: MobjId) {
    S_StartSound(world, mo_id, Sfx::Boscub as i32);
    A_SpawnFly(world, mo_id);
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
pub unsafe fn A_SpawnFly(world: &mut World, mo_id: MobjId) {
    let type_0: mobjtype_t;

    let target = {
        let mut mobjs = MOBJ_ARENA.borrow_mut();
        let mo = &mut mobjs[mo_id];
        mo.reactiontime -= 1;
        if mo.reactiontime != 0 {
            return;
        }
        mo.target
    };
    let targ_id = P_SubstNullMobj(world, target);
    // The spot's position, read afresh after each spawn (which runs play
    // code), as C reads it through its pointer.
    let targ_position = || {
        let mobjs = MOBJ_ARENA.borrow();
        let targ = &mobjs[targ_id];
        (targ.x, targ.y, targ.z)
    };
    let (x, y, z) = targ_position();
    let fog = P_SpawnMobj(world, x, y, z, MT_SPAWNFIRE);
    S_StartSound(world, fog, Sfx::Telept as i32);
    let r: i32 = P_Random(world);
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
    let (x, y, z) = targ_position();
    let newmobj_id = P_SpawnMobj(world, x, y, z, type_0);
    if P_LookForPlayers(world, newmobj_id, Boolean::TRUE).is_truthy() {
        let seestate = MOBJ_ARENA.borrow()[newmobj_id].info.get().seestate;
        P_SetMobjState(world, newmobj_id, seestate as statenum_t);
    }
    let (x, y) = {
        let mobjs = MOBJ_ARENA.borrow();
        (mobjs[newmobj_id].x, mobjs[newmobj_id].y)
    };
    P_TeleportMove(world, newmobj_id, x, y);
    P_RemoveMobj(world, mo_id);
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
pub unsafe fn A_PlayerScream(world: &mut World, mo_id: MobjId) {
    let mut sound: i32 = Sfx::Pldeth as i32;
    if gamemode() as u32 == commercial as u32 && MOBJ_ARENA.borrow()[mo_id].health < -50 {
        sound = Sfx::Pdiehi as i32;
    }
    S_StartSound(world, mo_id, sound);
}
