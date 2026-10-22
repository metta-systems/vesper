//! Rust port of vendor/doomgeneric/p_enemy.c.
//!
//! Implements monster AI, movement, and all action-pointer functions (`A_*`)
//! that the state machine invokes via function pointers stored in `info.rs`.
//! This covers monster pathfinding (`P_NewChaseDir`, `P_Move`, `P_TryWalk`),
//! target acquisition (`P_LookForPlayers`, `P_NoiseAlert`), melee/missile
//! range checks, individual monster attacks, boss-death special actions, and
//! the icon-of-sin (brain) spawn logic.
//!
//! Rust-vs-C differences: `dirtype_t` is a plain `c_int` alias rather than
//! a C enum; game-mode/version/skill constants are local `c_int` consts
//! rather than imported enums; `Boolean` replaces the C `boolean` typedef
//! throughout; raw pointer casts are explicit where C used implicit
//! `(mobj_t *)` coercions.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::c_void;
use std::os::raw::{c_int, c_uint};

use crate::doom::sounds::Sfx;
use crate::types::Boolean;
/// C `size_t` equivalent; used for buffer sizes matching the C ABI.
type size_t = usize;
/// Binary-angle measurement type (`u32`); a full circle is `2^32` units.
type angle_t = c_uint;
/// State index type mirroring C `statenum_t`; indexes the `states` table in `info.rs`.
type statenum_t = c_int;
/// C `short` integer type alias.
type c_short = i16;
/// Map-object type identifier; mirrors C `mobjtype_t` (index into `mobjinfo` table).
type mobjtype_t = c_int;
/// Alias for `MobjInfo`; mirrors C `mobjinfo_t` typedef.
type mobjinfo_t = MobjInfo;

/// Returns the absolute value of `x`; local substitute for `stdlib.h` `abs` used in `p_enemy.c`.
#[inline]
fn abs(x: c_int) -> c_int {
    if x < 0 {
        -x
    } else {
        x
    }
}

use crate::doom::c_ffi::{line_t, sector_t, vertex_t, LinedefFlag, MAPBLOCKSHIFT};
use crate::doom::d_loop::gametic;
use crate::doom::d_player::{players, PlayerT, PspdefT, MAXPLAYERS};
use crate::doom::doomstat::{gamemode, gameversion};
use crate::doom::info::{
    mobjinfo, MobjInfo, State, MF_AMBUSH, MF_CORPSE, MF_FLOAT, MF_INFLOAT, MF_JUSTATTACKED,
    MF_JUSTHIT, MF_SHADOW, MF_SHOOTABLE, MF_SKULLFLY, MF_SOLID, MT_ARACHPLAZ, MT_BABY,
    MT_BOSSTARGET, MT_BRUISER, MT_BRUISERSHOT, MT_CYBORG, MT_FATSHOT, MT_FATSO, MT_FIRE, MT_HEAD,
    MT_HEADSHOT, MT_KNIGHT, MT_PAIN, MT_ROCKET, MT_SERGEANT, MT_SHADOWS, MT_SKULL, MT_SMOKE,
    MT_SPAWNFIRE, MT_SPAWNSHOT, MT_SPIDER, MT_TRACER, MT_TROOP, MT_TROOPSHOT, MT_UNDEAD, MT_VILE,
    S_BRAINEXPLODE1, S_NULL, S_VILE_HEAL1,
};
use crate::doom::m_fixed::FRACUNIT;
use crate::doom::m_fixed::{fixed_t, FixedMul};
use crate::doom::m_random::P_Random;
use crate::doom::p_inter::P_DamageMobj;
use crate::doom::p_map::{
    floatok, numspechit, spechit, tmfloorz, P_AimLineAttack, P_CheckPosition, P_LineAttack,
    P_RadiusAttack, P_TeleportMove, P_TryMove,
};
use crate::doom::p_maputl::{
    openrange, P_AproxDistance, P_BlockThingsIterator, P_LineOpening, P_SetThingPosition,
    P_UnsetThingPosition,
};
use crate::doom::p_mobj::{
    P_MobjThinker, P_RemoveMobj, P_SetMobjState, P_SpawnMissile, P_SpawnMobj, P_SpawnPuff,
    P_SubstNullMobj,
};
use crate::doom::p_setup::{bmaporgx, bmaporgy, sides};
use crate::doom::p_sight::P_CheckSight;
use crate::doom::p_switch::P_UseSpecialLine;
use crate::doom::p_telept::mobj_t;
use crate::doom::tables::{ANG180, ANG270, ANG90, ANGLETOFINESHIFT};
use crate::i_error;
/// Alias for the `c_ffi`-module `mobj_t` used in pointer casts when calling C-facing functions
/// that expect that specific type definition.
type CffiMobj = crate::doom::c_ffi::mobj_t;
use crate::doom::p_tick::{thinker_t, thinkercap};
use crate::doom::r_main::{validcount, R_PointToAngle2};
use crate::doom::s_sound::S_StartSound;
use crate::doom::tables::{finecosine, finesine};

/// Maximum melee attack range in fixed-point units (64 map units); from `p_local.h`.
const MELEERANGE: c_int = 64 * FRACUNIT;
/// Maximum missile attack range in fixed-point units (2048 map units); from `p_local.h`.
const MISSILERANGE: c_int = 32 * 64 * FRACUNIT;
/// Vertical speed at which floating monsters adjust altitude each tic (4 units/tic).
const FLOATSPEED: c_int = FRACUNIT * 4;
/// Maximum radius of any map object (32 units), used as a blockmap search margin.
const MAXRADIUS: c_int = 32 * FRACUNIT;

/// Eight-direction movement type used by the monster pathfinding code; mirrors C `dirtype_t`.
type dirtype_t = c_int;
/// East cardinal direction index (positive X axis).
const DI_EAST: c_int = 0;
/// Northeast diagonal direction index.
const DI_NORTHEAST: c_int = 1;
/// North cardinal direction index (positive Y axis).
const DI_NORTH: c_int = 2;
/// Northwest diagonal direction index.
const DI_NORTHWEST: c_int = 3;
/// West cardinal direction index (negative X axis).
const DI_WEST: c_int = 4;
/// Southwest diagonal direction index.
const DI_SOUTHWEST: c_int = 5;
/// South cardinal direction index (negative Y axis).
const DI_SOUTH: c_int = 6;
/// Southeast diagonal direction index.
const DI_SOUTHEAST: c_int = 7;
/// Sentinel value meaning the monster has no current movement direction.
const DI_NODIR: c_int = 8;
/// Total number of direction values including `DI_NODIR`.
#[allow(dead_code)]
const NUMDIRS: c_int = 9;

/// Minimum game version that introduced Ultimate Doom episode logic; mirrors `exe_ultimate` from `doomfeatures.h`.
const exe_ultimate: c_int = 6;

/// Game mode value for Doom II (commercial); mirrors `commercial` from `doomdef.h`.
const commercial: c_int = 2;

/// Skill level index for Nightmare difficulty; mirrors `sk_nightmare` from `doomdef.h`.
const sk_nightmare: c_int = 4;
/// Skill level index for Easy (Hey Not Too Rough) difficulty; mirrors `sk_easy` from `doomdef.h`.
const sk_easy: c_int = 1;

/// `EV_DoFloor` floor type: lower floor to the lowest adjacent floor; mirrors `lowerFloorToLowest`.
const lowerFloorToLowest: c_int = 5;
/// `EV_DoFloor` floor type: raise floor to nearest texture height; mirrors `raiseToTexture`.
const raiseToTexture: c_int = 8;
/// `EV_DoDoor` door type: blaze-open (fast open); mirrors `vld_blazeOpen`.
const vld_blazeOpen: c_int = 5;
/// `EV_DoDoor` door type: normal open; mirrors `vld_open`.
const vld_open: c_int = 0;

/// `mobjtype_t` index for the player map object; mirrors `MT_PLAYER` from `info.h`.
const MT_PLAYER: c_int = 0;
/// Angular spread (ANG90/8) used between Mancubus fire balls; mirrors `FATSPREAD` from `p_enemy.c`.
const FATSPREAD: c_int = ANG90 as c_int / 8;
/// Lost Soul charge speed in fixed-point units per tic (20 map units/tic); mirrors `SKULLSPEED`.
const SKULLSPEED: c_int = 20 * FRACUNIT;

use crate::doom::d_main::fastparm;
use crate::doom::g_game::{gameepisode, gamemap, gameskill, netgame, playeringame, G_ExitLevel};
use crate::doom::p_doors::EV_DoDoor;
use crate::doom::p_floor::EV_DoFloor;
use crate::doom::p_pspr::A_ReFire;

/// Type alias used when casting a `line_t` pointer for `EV_DoDoor`/`EV_DoFloor` calls that
/// require the `p_lights` module's `line_t` definition; all `line_t` variants share an
/// identical `#[repr(C)]` layout so the cast is safe.
type PLineThing = crate::doom::p_lights::line_t;

/// Lookup table mapping each of the eight directions to its 180-degree opposite, with
/// `DI_NODIR` mapping to `DI_NODIR`; indexed by `dirtype_t` in `P_NewChaseDir`.
/// Has C linkage (`#[no_mangle]`); referenced from `p_enemy.c` (C test harness).
#[no_mangle]
pub static mut opposite: [dirtype_t; 9] = [
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
/// Has C linkage (`#[no_mangle]`); referenced from `p_enemy.c` (C test harness).
#[no_mangle]
pub static mut diags: [dirtype_t; 4] = [DI_NORTHWEST, DI_NORTHEAST, DI_SOUTHWEST, DI_SOUTHEAST];
/// The monster or player whose noise triggered the current `P_RecursiveSound` traversal;
/// written by `P_NoiseAlert`, read by `P_RecursiveSound` to stamp each sector.
/// Has C linkage (`#[no_mangle]`); referenced directly from `p_enemy.c`.
#[no_mangle]
pub static mut soundtarget: *mut mobj_t = std::ptr::null_mut::<mobj_t>();
/// Recursively floods sound through adjacent sectors, stamping each with `soundtarget`.
///
/// Traversal stops at sectors already visited this frame (`validcount`) and at
/// two-sided linedefs with the `ML_SOUNDBLOCK` flag - the block flag allows one
/// crossing (incrementing `soundblocks` from 0 to 1) but never two, so sound
/// cannot pass through two consecutive blocking walls.
///
/// Called by `P_NoiseAlert` and recursively by itself.
///
/// # Safety
///
/// `sec` must point to a valid, live `sector_t`. All sector/sidedef/linedef
/// pointers reachable from `sec` must also be valid. `soundtarget` must be null
/// or point to a valid `mobj_t`. This function is called from C.
#[no_mangle]
pub unsafe extern "C" fn P_RecursiveSound(sec: *mut sector_t, soundblocks: c_int) {
    let mut i: c_int;

    let mut check: *mut line_t;

    let mut other: *mut sector_t;

    if (*sec).validcount == validcount && (*sec).soundtraversed <= soundblocks + 1 as c_int {
        return;
    }
    (*sec).validcount = validcount;
    (*sec).soundtraversed = soundblocks + 1 as c_int;
    (*sec).soundtarget = soundtarget as *mut c_void;
    i = 0 as c_int;
    while i < (*sec).linecount {
        check = *(*sec).lines.offset(i as isize) as *mut line_t;
        if (*check).flags as c_int & LinedefFlag::TWOSIDED as c_int != 0 {
            P_LineOpening(check);
            if openrange > 0 as c_int {
                if std::ptr::eq(
                    (*sides.offset((*check).sidenum[0 as c_int as usize] as isize)).sector,
                    sec,
                ) {
                    other = (*sides.offset((*check).sidenum[1 as c_int as usize] as isize)).sector;
                } else {
                    other = (*sides.offset((*check).sidenum[0 as c_int as usize] as isize)).sector;
                }
                if (*check).flags as c_int & LinedefFlag::SOUNDBLOCK as c_int != 0 {
                    if soundblocks == 0 {
                        P_RecursiveSound(other, 1 as c_int);
                    }
                } else {
                    P_RecursiveSound(other, soundblocks);
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
/// `target` and `emmiter` must both be non-null, valid `mobj_t` pointers. `emmiter`
/// must have a valid `subsector` with a valid `sector`. This function is called from C.
#[no_mangle]
pub unsafe extern "C" fn P_NoiseAlert(target: *mut mobj_t, emmiter: *mut mobj_t) {
    soundtarget = target;
    validcount += 1;
    P_RecursiveSound((*(*emmiter).subsector).sector as *mut sector_t, 0 as c_int);
}
/// Returns `TRUE` when `actor`'s target is within melee striking range and line of sight.
///
/// Range test: approximate distance must be less than `MELEERANGE - 20 + target.radius`.
/// Returns `FALSE` immediately if `actor->target` is null, if the distance check fails,
/// or if `P_CheckSight` reports no clear line of sight.
///
/// # Safety
///
/// `actor` must be a valid, non-null `mobj_t`. If `actor->target` is non-null it must
/// also point to a valid `mobj_t` with a valid `info` pointer. Called from C.
#[no_mangle]
pub unsafe extern "C" fn P_CheckMeleeRange(actor: *mut mobj_t) -> Boolean {
    if (*actor).target.is_null() {
        return Boolean::FALSE;
    }
    let pl: *mut mobj_t = (*actor).target;
    let dist: fixed_t = P_AproxDistance((*pl).x - (*actor).x, (*pl).y - (*actor).y);
    if dist >= MELEERANGE - 20 as c_int * FRACUNIT + (*((*pl).info as *mut MobjInfo)).radius {
        return Boolean::FALSE;
    }
    if P_CheckSight(actor, (*actor).target) == 0 {
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
/// `actor` must be non-null and have a valid `target`, `info`, and `reactiontime`.
/// Called from C.
#[no_mangle]
pub unsafe extern "C" fn P_CheckMissileRange(actor: *mut mobj_t) -> Boolean {
    let mut dist: fixed_t;

    if P_CheckSight(actor, (*actor).target) == 0 {
        return Boolean::FALSE;
    }
    if (*actor).flags & MF_JUSTHIT as c_int != 0 {
        (*actor).flags &= !(MF_JUSTHIT as c_int);
        return Boolean::TRUE;
    }
    if (*actor).reactiontime != 0 {
        return Boolean::FALSE;
    }
    dist = (P_AproxDistance(
        (*actor).x - (*(*actor).target).x,
        (*actor).y - (*(*actor).target).y,
    ) as c_int
        - 64 as c_int * FRACUNIT) as fixed_t;
    if (*((*actor).info as *mut MobjInfo)).meleestate == 0 {
        dist -= 128 as c_int * FRACUNIT;
    }
    dist >>= 16 as c_int;
    if (*actor).mobjtype as c_uint == MT_VILE as c_int as c_uint && dist > 14 as c_int * 64 as c_int
    {
        return Boolean::FALSE;
    }
    if (*actor).mobjtype as c_uint == MT_UNDEAD as c_int as c_uint {
        if dist < 196 as c_int {
            return Boolean::FALSE;
        }
        dist >>= 1 as c_int;
    }
    if (*actor).mobjtype as c_uint == MT_CYBORG as c_int as c_uint
        || (*actor).mobjtype as c_uint == MT_SPIDER as c_int as c_uint
        || (*actor).mobjtype as c_uint == MT_SKULL as c_int as c_uint
    {
        dist >>= 1 as c_int;
    }
    if dist > 200 as c_int {
        dist = 200 as c_int as fixed_t;
    }
    if (*actor).mobjtype as c_uint == MT_CYBORG as c_int as c_uint && dist > 160 as c_int {
        dist = 160 as c_int as fixed_t;
    }
    if P_Random() < dist {
        return Boolean::FALSE;
    }
    Boolean::TRUE
}
/// Per-direction X-axis speed multiplier table indexed by `dirtype_t` (0=East..7=Southeast).
/// Each entry is a fixed-point unit (FRACUNIT or ~0.718*FRACUNIT for diagonals).
/// Has C linkage (`#[no_mangle]`); referenced from `p_enemy.c` (C test harness).
#[no_mangle]
pub static mut xspeed: [fixed_t; 8] = [
    FRACUNIT,
    47000 as c_int,
    0 as c_int,
    -47000 as c_int,
    -FRACUNIT,
    -47000 as c_int,
    0 as c_int,
    47000 as c_int,
];
/// Per-direction Y-axis speed multiplier table indexed by `dirtype_t` (0=East..7=Southeast).
/// Each entry is a fixed-point unit (FRACUNIT or ~0.718*FRACUNIT for diagonals).
/// Has C linkage (`#[no_mangle]`); referenced from `p_enemy.c` (C test harness).
#[no_mangle]
pub static mut yspeed: [fixed_t; 8] = [
    0 as c_int,
    47000 as c_int,
    FRACUNIT,
    47000 as c_int,
    0 as c_int,
    -47000 as c_int,
    -FRACUNIT,
    -47000 as c_int,
];
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
/// `actor` must be a non-null, valid `mobj_t` with valid `info`. All globals
/// `floatok`, `tmfloorz`, `numspechit`, and `spechit` must be consistent with the
/// most recent `P_TryMove` call. Called from C.
#[no_mangle]
pub unsafe extern "C" fn P_Move(actor: *mut mobj_t) -> Boolean {
    let mut ld: *mut line_t;

    let mut good: Boolean;

    if (*actor).movedir == DI_NODIR as c_int {
        return Boolean::FALSE;
    }
    if (*actor).movedir as c_uint >= 8 as c_uint {
        i_error!("Weird actor->movedir!");
    }
    let tryx: fixed_t = (*actor).x
        + (*((*actor).info as *mut MobjInfo)).speed as fixed_t * xspeed[(*actor).movedir as usize];
    let tryy: fixed_t = (*actor).y
        + (*((*actor).info as *mut MobjInfo)).speed as fixed_t * yspeed[(*actor).movedir as usize];
    let try_ok: Boolean = Boolean::from_raw(P_TryMove(actor as *mut CffiMobj, tryx, tryy));
    if try_ok.is_false() {
        if (*actor).flags & MF_FLOAT as c_int != 0 && floatok != 0 {
            if (*actor).z < tmfloorz {
                (*actor).z += FLOATSPEED;
            } else {
                (*actor).z -= FLOATSPEED;
            }
            (*actor).flags |= MF_INFLOAT as c_int;
            return Boolean::TRUE;
        }
        if numspechit == 0 {
            return Boolean::FALSE;
        }
        (*actor).movedir = DI_NODIR as c_int;
        good = Boolean::FALSE;
        loop {
            let c2rust_fresh0 = numspechit;
            numspechit -= 1;
            if c2rust_fresh0 == 0 {
                break;
            }
            ld = spechit[numspechit as usize];
            if P_UseSpecialLine(
                actor as *mut c_void,
                ld as *mut crate::doom::p_lights::line_t,
                0 as c_int,
            ) != 0
            {
                good = Boolean::TRUE;
            }
        }
        return good;
    } else {
        (*actor).flags &= !(MF_INFLOAT as c_int);
    }
    if (*actor).flags & MF_FLOAT as c_int == 0 {
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
/// `actor` must be a non-null, valid `mobj_t`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn P_TryWalk(actor: *mut mobj_t) -> Boolean {
    if P_Move(actor).is_false() {
        return Boolean::FALSE;
    }
    (*actor).movecount = P_Random() & 15 as c_int;
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
/// Panics (via `i_error!`) if `actor->target` is null.
///
/// # Safety
///
/// `actor` must be non-null with a valid, non-null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn P_NewChaseDir(actor: *mut mobj_t) {
    let mut d: [dirtype_t; 3] = [DI_EAST; 3];
    let mut tdir: c_int;

    if (*actor).target.is_null() {
        i_error!("P_NewChaseDir: called with no target");
    }
    let olddir: dirtype_t = (*actor).movedir as dirtype_t;
    let turnaround: dirtype_t = opposite[olddir as usize];
    let deltax: fixed_t = (*(*actor).target).x - (*actor).x;
    let deltay: fixed_t = (*(*actor).target).y - (*actor).y;
    if deltax > 10 as c_int * FRACUNIT {
        d[1 as c_int as usize] = DI_EAST;
    } else if deltax < -10 as c_int * FRACUNIT {
        d[1 as c_int as usize] = DI_WEST;
    } else {
        d[1 as c_int as usize] = DI_NODIR;
    }
    if deltay < -10 as c_int * FRACUNIT {
        d[2 as c_int as usize] = DI_SOUTH;
    } else if deltay > 10 as c_int * FRACUNIT {
        d[2 as c_int as usize] = DI_NORTH;
    } else {
        d[2 as c_int as usize] = DI_NODIR;
    }
    if d[1 as c_int as usize] as c_uint != DI_NODIR as c_int as c_uint
        && d[2 as c_int as usize] as c_uint != DI_NODIR as c_int as c_uint
    {
        (*actor).movedir = diags[((((deltay < 0 as c_int) as c_int) << 1 as c_int)
            + (deltax > 0 as c_int) as c_int) as usize] as c_int;
        if (*actor).movedir != turnaround as c_int && P_TryWalk(actor).is_truthy() {
            return;
        }
    }
    if P_Random() > 200 as c_int || (deltay as c_int).abs() > (deltax as c_int).abs() {
        tdir = d[1 as c_int as usize] as c_int;
        d[1 as c_int as usize] = d[2 as c_int as usize];
        d[2 as c_int as usize] = tdir as dirtype_t;
    }
    if d[1 as c_int as usize] as c_uint == turnaround as c_uint {
        d[1 as c_int as usize] = DI_NODIR;
    }
    if d[2 as c_int as usize] as c_uint == turnaround as c_uint {
        d[2 as c_int as usize] = DI_NODIR;
    }
    if d[1 as c_int as usize] as c_uint != DI_NODIR as c_int as c_uint {
        (*actor).movedir = d[1 as c_int as usize] as c_int;
        if P_TryWalk(actor).is_truthy() {
            return;
        }
    }
    if d[2 as c_int as usize] as c_uint != DI_NODIR as c_int as c_uint {
        (*actor).movedir = d[2 as c_int as usize] as c_int;
        if P_TryWalk(actor).is_truthy() {
            return;
        }
    }
    if olddir as c_uint != DI_NODIR as c_int as c_uint {
        (*actor).movedir = olddir as c_int;
        if P_TryWalk(actor).is_truthy() {
            return;
        }
    }
    if P_Random() & 1 as c_int != 0 {
        tdir = DI_EAST as c_int;
        while tdir <= DI_SOUTHEAST as c_int {
            if tdir != turnaround as c_int {
                (*actor).movedir = tdir;
                if P_TryWalk(actor).is_truthy() {
                    return;
                }
            }
            tdir += 1;
        }
    } else {
        tdir = DI_SOUTHEAST as c_int;
        while tdir != DI_EAST as c_int - 1 as c_int {
            if tdir != turnaround as c_int {
                (*actor).movedir = tdir;
                if P_TryWalk(actor).is_truthy() {
                    return;
                }
            }
            tdir -= 1;
        }
    }
    if turnaround as c_uint != DI_NODIR as c_int as c_uint {
        (*actor).movedir = turnaround as c_int;
        if P_TryWalk(actor).is_truthy() {
            return;
        }
    }
    (*actor).movedir = DI_NODIR as c_int;
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
/// `actor` must be a non-null, valid `mobj_t`. The global `players` array and
/// `playeringame` flags must be consistent. Called from C.
#[no_mangle]
pub unsafe extern "C" fn P_LookForPlayers(actor: *mut mobj_t, allaround: Boolean) -> Boolean {
    let mut c: c_int;

    let mut player: *mut PlayerT;

    let mut an: angle_t;

    let mut dist: fixed_t;

    c = 0 as c_int;
    let stop: c_int = ((*actor).lastlook - 1 as c_int) & 3 as c_int;
    loop {
        's_20: {
            if playeringame[(*actor).lastlook as usize] != 0 {
                let c2rust_fresh1 = c;
                c += 1;
                if c2rust_fresh1 == 2 as c_int || (*actor).lastlook == stop {
                    return Boolean::FALSE;
                }
                player = (&raw mut players as *mut PlayerT).offset((*actor).lastlook as isize);
                if (*player).health > 0 as c_int {
                    let sight = P_CheckSight(actor, (*player).mo as *mut mobj_t);
                    if sight != 0 {
                        if allaround.is_false() {
                            an = R_PointToAngle2(
                                (*actor).x,
                                (*actor).y,
                                (*((*player).mo as *mut mobj_t)).x,
                                (*((*player).mo as *mut mobj_t)).y,
                            )
                            .wrapping_sub((*actor).angle);
                            if an > ANG90 as angle_t && an < ANG270 {
                                dist = P_AproxDistance(
                                    (*((*player).mo as *mut mobj_t)).x - (*actor).x,
                                    (*((*player).mo as *mut mobj_t)).y - (*actor).y,
                                );
                                if dist > MELEERANGE {
                                    break 's_20;
                                }
                            }
                        }
                        (*actor).target = (*player).mo as *mut mobj_t;
                        return Boolean::TRUE;
                    }
                }
            }
        }
        (*actor).lastlook = ((*actor).lastlook + 1 as c_int) & 3 as c_int;
    }
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
/// `mo` must be a non-null, valid `mobj_t`. The thinker list (`thinkercap`) must be
/// consistent. Called from C via the state-machine action pointer.
#[no_mangle]
pub unsafe extern "C" fn A_KeenDie(mo: *mut mobj_t) {
    let mut th: *mut thinker_t;

    let mut mo2: *mut mobj_t;

    let mut junk: line_t = line_t {
        v1: std::ptr::null_mut::<vertex_t>(),
        v2: std::ptr::null_mut::<vertex_t>(),
        dx: 0,
        dy: 0,
        flags: 0,
        special: 0,
        tag: 0,
        sidenum: [0; 2],
        bbox: [0; 4],
        slopetype: 0,
        frontsector: std::ptr::null_mut::<c_void>(),
        backsector: std::ptr::null_mut::<c_void>(),
        validcount: 0,
        specialdata: std::ptr::null_mut::<c_void>(),
    };
    A_Fall(mo);
    th = thinkercap.next;
    while !std::ptr::eq(th, &raw const thinkercap) {
        if (*th).function.acp1
            == core::mem::transmute::<
                Option<unsafe extern "C" fn(*mut mobj_t) -> ()>,
                Option<unsafe extern "C" fn(*mut c_void) -> ()>,
            >(Some(
                P_MobjThinker as unsafe extern "C" fn(*mut mobj_t) -> (),
            ))
        {
            mo2 = th as *mut mobj_t;
            if mo2 != mo
                && (*mo2).mobjtype as c_uint == (*mo).mobjtype as c_uint
                && (*mo2).health > 0 as c_int
            {
                return;
            }
        }
        th = (*th).next;
    }
    junk.tag = 666 as c_short;
    EV_DoDoor(&mut junk as *mut line_t as *mut PLineThing, vld_open);
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
/// `actor` must be a non-null, valid `mobj_t` with valid `subsector`, `info`, and
/// `flags`. Called from C via state-machine action pointer.
#[no_mangle]
pub unsafe extern "C" fn A_Look(actor: *mut mobj_t) {
    (*actor).threshold = 0 as c_int;
    let targ: *mut mobj_t = (*(*(*actor).subsector).sector).soundtarget as *mut mobj_t;
    '_seeyou: {
        if !targ.is_null() && (*targ).flags & MF_SHOOTABLE as c_int != 0 {
            (*actor).target = targ;
            if (*actor).flags & MF_AMBUSH as c_int != 0 {
                if P_CheckSight(actor, (*actor).target) != 0 {
                    break '_seeyou;
                }
            } else {
                break '_seeyou;
            }
        }
        if P_LookForPlayers(actor, Boolean::FALSE).is_false() {
            return;
        }
    }
    if (*((*actor).info as *mut MobjInfo)).seesound != Sfx::None {
        let sound = match (*((*actor).info as *mut MobjInfo)).seesound {
            Sfx::Posit1 | Sfx::Posit2 | Sfx::Posit3 => {
                Sfx::Posit1 as c_int + P_Random() % 3 as c_int
            }
            Sfx::Bgsit1 | Sfx::Bgsit2 => Sfx::Bgsit1 as c_int + P_Random() % 2 as c_int,
            s => s as c_int,
        };
        if (*actor).mobjtype as c_uint == MT_SPIDER as c_int as c_uint
            || (*actor).mobjtype as c_uint == MT_CYBORG as c_int as c_uint
        {
            S_StartSound(std::ptr::null_mut::<c_void>(), sound);
        } else {
            S_StartSound(actor as *mut c_void, sound);
        }
    }
    P_SetMobjState(
        actor,
        (*((*actor).info as *mut MobjInfo)).seestate as statenum_t,
    );
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
/// `actor` must be a non-null, valid `mobj_t` with valid `info`. All referenced
/// globals (`gameskill`, `fastparm`, `netgame`) must be initialised. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_Chase(actor: *mut mobj_t) {
    let delta: c_int;

    if (*actor).reactiontime != 0 {
        (*actor).reactiontime -= 1;
    }
    if (*actor).threshold != 0 {
        if (*actor).target.is_null() || (*(*actor).target).health <= 0 as c_int {
            (*actor).threshold = 0 as c_int;
        } else {
            (*actor).threshold -= 1;
        }
    }
    if (*actor).movedir < 8 as c_int {
        (*actor).angle &= ((7 as c_int) << 29 as c_int) as angle_t;
        delta = (*actor)
            .angle
            .wrapping_sub(((*actor).movedir << 29 as c_int) as angle_t) as c_int;
        if delta > 0 as c_int {
            (*actor).angle = (*actor).angle.wrapping_sub((ANG90 / 2) as angle_t);
        } else if delta < 0 as c_int {
            (*actor).angle = (*actor).angle.wrapping_add((ANG90 / 2) as angle_t);
        }
    }
    if (*actor).target.is_null() || (*(*actor).target).flags & MF_SHOOTABLE as c_int == 0 {
        if P_LookForPlayers(actor, Boolean::TRUE).is_truthy() {
            return;
        }
        P_SetMobjState(
            actor,
            (*((*actor).info as *mut MobjInfo)).spawnstate as statenum_t,
        );
        return;
    }
    if (*actor).flags & MF_JUSTATTACKED as c_int != 0 {
        (*actor).flags &= !(MF_JUSTATTACKED as c_int);
        if gameskill as c_int != sk_nightmare as c_int && fastparm == 0 {
            P_NewChaseDir(actor);
        }
        return;
    }
    if (*((*actor).info as *mut MobjInfo)).meleestate != 0 && P_CheckMeleeRange(actor).is_truthy() {
        if (*((*actor).info as *mut MobjInfo)).attacksound != Sfx::None {
            S_StartSound(
                actor as *mut c_void,
                (*((*actor).info as *mut MobjInfo)).attacksound as c_int,
            );
        }
        P_SetMobjState(
            actor,
            (*((*actor).info as *mut MobjInfo)).meleestate as statenum_t,
        );
        return;
    }
    if (*((*actor).info as *mut MobjInfo)).missilestate != 0
        && !((gameskill as c_int) < sk_nightmare as c_int
            && fastparm == 0
            && (*actor).movecount != 0)
        && P_CheckMissileRange(actor).is_truthy()
    {
        P_SetMobjState(
            actor,
            (*((*actor).info as *mut MobjInfo)).missilestate as statenum_t,
        );
        (*actor).flags |= MF_JUSTATTACKED as c_int;
        return;
    }
    if netgame != 0
        && (*actor).threshold == 0
        && P_CheckSight(actor, (*actor).target) == 0
        && P_LookForPlayers(actor, Boolean::TRUE).is_truthy()
    {
        return;
    }
    (*actor).movecount -= 1;
    if (*actor).movecount < 0 as c_int || P_Move(actor).is_false() {
        P_NewChaseDir(actor);
    }
    if (*((*actor).info as *mut MobjInfo)).activesound != Sfx::None && P_Random() < 3 as c_int {
        S_StartSound(
            actor as *mut c_void,
            (*((*actor).info as *mut MobjInfo)).activesound as c_int,
        );
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
/// `actor` must be non-null with a valid or null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_FaceTarget(actor: *mut mobj_t) {
    if (*actor).target.is_null() {
        return;
    }
    (*actor).flags &= !(MF_AMBUSH as c_int);
    (*actor).angle = R_PointToAngle2(
        (*actor).x,
        (*actor).y,
        (*(*actor).target).x,
        (*(*actor).target).y,
    );
    if (*(*actor).target).flags & MF_SHADOW as c_int != 0 {
        (*actor).angle = (*actor)
            .angle
            .wrapping_add(((P_Random() - P_Random()) << 21 as c_int) as angle_t);
    }
}
/// Attack action for the Former Human (Zombieman): single hitscan shot.
///
/// Faces the target, aims with `P_AimLineAttack`, plays `sfx_pistol`, then fires one
/// hitscan ray with horizontal spread of ±20 bits and damage of `(rnd%5+1)*3` (3-15).
///
/// # Safety
///
/// `actor` must be non-null with a valid or null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_PosAttack(actor: *mut mobj_t) {
    let mut angle: c_int;

    if (*actor).target.is_null() {
        return;
    }
    A_FaceTarget(actor);
    angle = (*actor).angle as c_int;
    let slope: c_int =
        P_AimLineAttack(actor as *mut CffiMobj, angle as angle_t, MISSILERANGE) as c_int;
    S_StartSound(actor as *mut c_void, Sfx::Pistol as c_int);
    angle = angle.wrapping_add((P_Random() - P_Random()) << 20 as c_int);
    let damage: c_int = (P_Random() % 5 as c_int + 1 as c_int) * 3 as c_int;
    P_LineAttack(
        actor as *mut CffiMobj,
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
/// `actor` must be non-null with a valid or null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_SPosAttack(actor: *mut mobj_t) {
    let mut i: c_int;

    let mut angle: c_int;

    let mut damage: c_int;

    if (*actor).target.is_null() {
        return;
    }
    S_StartSound(actor as *mut c_void, Sfx::Shotgn as c_int);
    A_FaceTarget(actor);
    let bangle: c_int = (*actor).angle as c_int;
    let slope: c_int =
        P_AimLineAttack(actor as *mut CffiMobj, bangle as angle_t, MISSILERANGE) as c_int;
    i = 0 as c_int;
    while i < 3 as c_int {
        angle = bangle.wrapping_add((P_Random() - P_Random()) << 20 as c_int);
        damage = (P_Random() % 5 as c_int + 1 as c_int) * 3 as c_int;
        P_LineAttack(
            actor as *mut CffiMobj,
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
/// `actor` must be non-null with a valid or null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_CPosAttack(actor: *mut mobj_t) {
    if (*actor).target.is_null() {
        return;
    }
    S_StartSound(actor as *mut c_void, Sfx::Shotgn as c_int);
    A_FaceTarget(actor);
    let bangle: c_int = (*actor).angle as c_int;
    let slope: c_int =
        P_AimLineAttack(actor as *mut CffiMobj, bangle as angle_t, MISSILERANGE) as c_int;
    let angle: c_int = bangle.wrapping_add((P_Random() - P_Random()) << 20 as c_int);
    let damage: c_int = (P_Random() % 5 as c_int + 1 as c_int) * 3 as c_int;
    P_LineAttack(
        actor as *mut CffiMobj,
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
/// `actor` must be non-null with a valid `info`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_CPosRefire(actor: *mut mobj_t) {
    A_FaceTarget(actor);
    if P_Random() < 40 as c_int {
        return;
    }
    if (*actor).target.is_null()
        || (*(*actor).target).health <= 0 as c_int
        || P_CheckSight(actor, (*actor).target) == 0
    {
        P_SetMobjState(
            actor,
            (*((*actor).info as *mut MobjInfo)).seestate as statenum_t,
        );
    }
}
/// Refire check for the Spider Mastermind: keeps firing unless the target is gone or hidden.
///
/// Same logic as `A_CPosRefire` but with a lower 10/256 early-return chance, making the
/// Spider Mastermind more persistent.
///
/// # Safety
///
/// `actor` must be non-null with a valid `info`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_SpidRefire(actor: *mut mobj_t) {
    A_FaceTarget(actor);
    if P_Random() < 10 as c_int {
        return;
    }
    if (*actor).target.is_null()
        || (*(*actor).target).health <= 0 as c_int
        || P_CheckSight(actor, (*actor).target) == 0
    {
        P_SetMobjState(
            actor,
            (*((*actor).info as *mut MobjInfo)).seestate as statenum_t,
        );
    }
}
/// Attack action for the Arachnotron: launches one `MT_ARACHPLAZ` plasma ball.
///
/// # Safety
///
/// `actor` must be non-null with a valid or null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_BspiAttack(actor: *mut mobj_t) {
    if (*actor).target.is_null() {
        return;
    }
    A_FaceTarget(actor);
    P_SpawnMissile(actor, (*actor).target, MT_ARACHPLAZ);
}
/// Attack action for the Imp: claw swipe in melee range, fireball at distance.
///
/// If in melee range plays `sfx_claw` and deals `(rnd%8+1)*3` damage (3-24).
/// Otherwise launches a `MT_TROOPSHOT` fireball.
///
/// # Safety
///
/// `actor` must be non-null with a valid or null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_TroopAttack(actor: *mut mobj_t) {
    let damage: c_int;

    if (*actor).target.is_null() {
        return;
    }
    A_FaceTarget(actor);
    if P_CheckMeleeRange(actor).is_truthy() {
        S_StartSound(actor as *mut c_void, Sfx::Claw as c_int);
        damage = (P_Random() % 8 as c_int + 1 as c_int) * 3 as c_int;
        P_DamageMobj((*actor).target, actor, actor, damage);
        return;
    }
    P_SpawnMissile(actor, (*actor).target, MT_TROOPSHOT);
}
/// Attack action for the Demon (Sarg): melee-only bite dealing `(rnd%10+1)*4` damage (4-40).
///
/// Only damages the target when within melee range; no ranged fallback.
///
/// # Safety
///
/// `actor` must be non-null with a valid or null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_SargAttack(actor: *mut mobj_t) {
    let damage: c_int;

    if (*actor).target.is_null() {
        return;
    }
    A_FaceTarget(actor);
    if P_CheckMeleeRange(actor).is_truthy() {
        damage = (P_Random() % 10 as c_int + 1 as c_int) * 4 as c_int;
        P_DamageMobj((*actor).target, actor, actor, damage);
    }
}
/// Attack action for the Cacodemon: bite in melee range, fireball at distance.
///
/// Melee deals `(rnd%6+1)*10` damage (10-60). Ranged fires `MT_HEADSHOT`.
///
/// # Safety
///
/// `actor` must be non-null with a valid or null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_HeadAttack(actor: *mut mobj_t) {
    let damage: c_int;

    if (*actor).target.is_null() {
        return;
    }
    A_FaceTarget(actor);
    if P_CheckMeleeRange(actor).is_truthy() {
        damage = (P_Random() % 6 as c_int + 1 as c_int) * 10 as c_int;
        P_DamageMobj((*actor).target, actor, actor, damage);
        return;
    }
    P_SpawnMissile(actor, (*actor).target, MT_HEADSHOT);
}
/// Attack action for the Cyberdemon: launches one `MT_ROCKET`.
///
/// # Safety
///
/// `actor` must be non-null with a valid or null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_CyberAttack(actor: *mut mobj_t) {
    if (*actor).target.is_null() {
        return;
    }
    A_FaceTarget(actor);
    P_SpawnMissile(actor, (*actor).target, MT_ROCKET);
}
/// Attack action for the Baron of Hell / Hell Knight: claw in melee, plasma ball at distance.
///
/// Melee plays `sfx_claw` and deals `(rnd%8+1)*10` damage (10-80). Ranged fires
/// `MT_BRUISERSHOT`.
///
/// # Safety
///
/// `actor` must be non-null with a valid or null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_BruisAttack(actor: *mut mobj_t) {
    let damage: c_int;

    if (*actor).target.is_null() {
        return;
    }
    if P_CheckMeleeRange(actor).is_truthy() {
        S_StartSound(actor as *mut c_void, Sfx::Claw as c_int);
        damage = (P_Random() % 8 as c_int + 1 as c_int) * 10 as c_int;
        P_DamageMobj((*actor).target, actor, actor, damage);
        return;
    }
    P_SpawnMissile(actor, (*actor).target, MT_BRUISERSHOT);
}
/// Attack action for the Revenant: launches a homing `MT_TRACER` missile.
///
/// Temporarily raises the actor's Z by 16 units so the missile spawns at shoulder
/// height. After spawning, advances the missile one tic forward and stores the
/// current target in `tracer` so `A_Tracer` can home in.
///
/// # Safety
///
/// `actor` must be non-null with a valid or null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_SkelMissile(actor: *mut mobj_t) {
    if (*actor).target.is_null() {
        return;
    }
    A_FaceTarget(actor);
    (*actor).z += 16 as c_int * FRACUNIT;
    let mo: *mut mobj_t = P_SpawnMissile(actor, (*actor).target, MT_TRACER);
    (*actor).z -= 16 as c_int * FRACUNIT;
    (*mo).x += (*mo).momx;
    (*mo).y += (*mo).momy;
    (*mo).tracer = (*actor).target;
}
/// Maximum angular correction applied per active tic by `A_Tracer` (approx 11.25 degrees in BAM).
/// Has C linkage (`#[no_mangle]`); referenced from `p_enemy.c` (C test harness).
#[no_mangle]
pub static mut TRACEANGLE: c_int = 0xc000000 as c_int;
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
/// `actor` must be a non-null, valid `mobj_t` with valid `info`. If `actor->tracer` is
/// non-null it must point to a valid `mobj_t`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_Tracer(actor: *mut mobj_t) {
    let mut exact: angle_t;

    let mut dist: fixed_t;

    if gametic & 3 as c_int != 0 {
        return;
    }
    P_SpawnPuff((*actor).x, (*actor).y, (*actor).z);
    let th: *mut mobj_t = P_SpawnMobj(
        (*actor).x - (*actor).momx,
        (*actor).y - (*actor).momy,
        (*actor).z,
        MT_SMOKE,
    );
    (*th).momz = FRACUNIT as fixed_t;
    (*th).tics -= P_Random() & 3 as c_int;
    if (*th).tics < 1 as c_int {
        (*th).tics = 1 as c_int;
    }
    let dest: *mut mobj_t = (*actor).tracer;
    if dest.is_null() || (*dest).health <= 0 as c_int {
        return;
    }
    exact = R_PointToAngle2((*actor).x, (*actor).y, (*dest).x, (*dest).y);
    if exact != (*actor).angle {
        if exact.wrapping_sub((*actor).angle) > 0x80000000 as c_uint {
            (*actor).angle = (*actor).angle.wrapping_sub(TRACEANGLE as angle_t);
            if exact.wrapping_sub((*actor).angle) < 0x80000000 as c_uint {
                (*actor).angle = exact;
            }
        } else {
            (*actor).angle = (*actor).angle.wrapping_add(TRACEANGLE as angle_t);
            if exact.wrapping_sub((*actor).angle) > 0x80000000 as c_uint {
                (*actor).angle = exact;
            }
        }
    }
    exact = (*actor).angle >> ANGLETOFINESHIFT;
    (*actor).momx = FixedMul(
        (*((*actor).info as *mut MobjInfo)).speed as fixed_t,
        *finecosine.0.add(exact as usize),
    );
    (*actor).momy = FixedMul(
        (*((*actor).info as *mut MobjInfo)).speed as fixed_t,
        finesine[exact as usize],
    );
    dist = P_AproxDistance((*dest).x - (*actor).x, (*dest).y - (*actor).y);
    dist = (dist as c_int / (*((*actor).info as *mut MobjInfo)).speed) as fixed_t;
    if dist < 1 as c_int {
        dist = 1 as c_int as fixed_t;
    }
    let slope: fixed_t = ((*dest).z + 40 as fixed_t * FRACUNIT - (*actor).z) / dist;
    if slope < (*actor).momz {
        (*actor).momz -= FRACUNIT / 8 as c_int;
    } else {
        (*actor).momz += FRACUNIT / 8 as c_int;
    };
}
/// Revenant melee wind-up: faces the target and plays the whoosh sound.
///
/// # Safety
///
/// `actor` must be non-null with a valid or null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_SkelWhoosh(actor: *mut mobj_t) {
    if (*actor).target.is_null() {
        return;
    }
    A_FaceTarget(actor);
    S_StartSound(actor as *mut c_void, Sfx::Skeswg as c_int);
}
/// Revenant melee strike: deals `(rnd%10+1)*6` damage (6-60) when in melee range.
///
/// Plays `sfx_skepch` on a successful hit.
///
/// # Safety
///
/// `actor` must be non-null with a valid or null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_SkelFist(actor: *mut mobj_t) {
    let damage: c_int;

    if (*actor).target.is_null() {
        return;
    }
    A_FaceTarget(actor);
    if P_CheckMeleeRange(actor).is_truthy() {
        damage = (P_Random() % 10 as c_int + 1 as c_int) * 6 as c_int;
        S_StartSound(actor as *mut c_void, Sfx::Skepch as c_int);
        P_DamageMobj((*actor).target, actor, actor, damage);
    }
}
/// The corpse most recently selected by `PIT_VileCheck` for resurrection; written by
/// `PIT_VileCheck`, read and mutated by `A_VileChase`. Has C linkage.
#[no_mangle]
pub static mut corpsehit: *mut mobj_t = std::ptr::null_mut::<mobj_t>();
/// The Archvile actor currently searching for a corpse to raise; set by `A_VileChase`
/// before calling `P_BlockThingsIterator`. Has C linkage.
#[no_mangle]
pub static mut vileobj: *mut mobj_t = std::ptr::null_mut::<mobj_t>();
/// X coordinate of the position the Archvile is moving toward, used as the centre of the
/// corpse search radius in `PIT_VileCheck`. Has C linkage.
#[no_mangle]
pub static mut viletryx: fixed_t = 0;
/// Y coordinate of the position the Archvile is moving toward. Has C linkage.
#[no_mangle]
pub static mut viletryy: fixed_t = 0;
/// Blockmap iterator callback: tests whether `thing` is a raiseable corpse near the Archvile.
///
/// Returns `TRUE` (keep iterating) unless `thing` is a fully-settled corpse (`MF_CORPSE`,
/// `tics == -1`) with a valid `raisestate`, within `thing->radius + MT_VILE->radius` of
/// (`viletryx`, `viletryy`), and with enough headroom to stand at full height. If all
/// conditions are met, stores `thing` in `corpsehit` and returns `FALSE` to stop iteration.
///
/// # Safety
///
/// `thing` must be a non-null, valid `mobj_t`. `viletryx`/`viletryy` and `vileobj` must
/// have been set by `A_VileChase`. Called from C via `P_BlockThingsIterator`.
#[no_mangle]
pub unsafe extern "C" fn PIT_VileCheck(thing: *mut mobj_t) -> Boolean {
    if (*thing).flags & MF_CORPSE as c_int == 0 {
        return Boolean::TRUE;
    }
    if (*thing).tics != -1 as c_int {
        return Boolean::TRUE;
    }
    if (*((*thing).info as *mut MobjInfo)).raisestate == S_NULL as c_int {
        return Boolean::TRUE;
    }
    let maxdist: c_int =
        (*((*thing).info as *mut MobjInfo)).radius + mobjinfo[MT_VILE as c_int as usize].radius;
    if ((*thing).x as c_int - viletryx as c_int).abs() > maxdist
        || ((*thing).y as c_int - viletryy as c_int).abs() > maxdist
    {
        return Boolean::TRUE;
    }
    corpsehit = thing;
    (*corpsehit).momy = 0 as c_int as fixed_t;
    (*corpsehit).momx = (*corpsehit).momy;
    (*corpsehit).height <<= 2 as c_int;
    let check: Boolean = Boolean::from_raw(P_CheckPosition(
        corpsehit as *mut CffiMobj,
        (*corpsehit).x,
        (*corpsehit).y,
    ));
    (*corpsehit).height >>= 2 as c_int;
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
/// `actor` must be non-null with valid `info` and `movedir`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_VileChase(actor: *mut mobj_t) {
    let xl: c_int;

    let xh: c_int;

    let yl: c_int;

    let yh: c_int;

    let mut bx: c_int;

    let mut by: c_int;

    let info: *mut MobjInfo;

    let temp: *mut mobj_t;

    if (*actor).movedir != DI_NODIR as c_int {
        viletryx = (*actor).x
            + (*((*actor).info as *mut MobjInfo)).speed as fixed_t
                * xspeed[(*actor).movedir as usize];
        viletryy = (*actor).y
            + (*((*actor).info as *mut MobjInfo)).speed as fixed_t
                * yspeed[(*actor).movedir as usize];
        xl = (viletryx as c_int - bmaporgx as c_int - 32 as c_int * FRACUNIT * 2 as c_int)
            >> MAPBLOCKSHIFT;
        xh = (viletryx as c_int - bmaporgx as c_int + 32 as c_int * FRACUNIT * 2 as c_int)
            >> MAPBLOCKSHIFT;
        yl = (viletryy as c_int - bmaporgy as c_int - 32 as c_int * FRACUNIT * 2 as c_int)
            >> MAPBLOCKSHIFT;
        yh = (viletryy as c_int - bmaporgy as c_int + 32 as c_int * FRACUNIT * 2 as c_int)
            >> MAPBLOCKSHIFT;
        vileobj = actor;
        bx = xl;
        while bx <= xh {
            by = yl;
            while by <= yh {
                if P_BlockThingsIterator(
                    bx,
                    by,
                    Some(core::mem::transmute::<
                        unsafe extern "C" fn(*mut mobj_t) -> Boolean,
                        unsafe extern "C" fn(*mut CffiMobj) -> c_uint,
                    >(PIT_VileCheck)),
                ) == 0
                {
                    temp = (*actor).target;
                    (*actor).target = corpsehit;
                    A_FaceTarget(actor);
                    (*actor).target = temp;
                    P_SetMobjState(actor, S_VILE_HEAL1);
                    S_StartSound(corpsehit as *mut c_void, Sfx::Slop as c_int);
                    info = (*corpsehit).info as *mut MobjInfo;
                    P_SetMobjState(corpsehit, (*info).raisestate as statenum_t);
                    (*corpsehit).height <<= 2 as c_int;
                    (*corpsehit).flags = (*info).flags;
                    (*corpsehit).health = (*info).spawnhealth;
                    (*corpsehit).target = ::core::ptr::null_mut::<mobj_t>();
                    return;
                }
                by += 1;
            }
            bx += 1;
        }
    }
    A_Chase(actor);
}
/// Archvile attack wind-up: plays the attack start sound (`sfx_vilatk`).
///
/// # Safety
///
/// `actor` must be non-null. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_VileStart(actor: *mut mobj_t) {
    S_StartSound(actor as *mut c_void, Sfx::Vilatk as c_int);
}
/// Archvile fire start: plays the flame start sound then positions the fire object.
///
/// # Safety
///
/// `actor` must be non-null with valid `tracer` and `target` fields (or null). Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_StartFire(actor: *mut mobj_t) {
    S_StartSound(actor as *mut c_void, Sfx::Flamst as c_int);
    A_Fire(actor);
}
/// Archvile fire crackle: plays the sustained flame sound then repositions the fire object.
///
/// # Safety
///
/// `actor` must be non-null with valid `tracer` and `target` fields (or null). Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_FireCrackle(actor: *mut mobj_t) {
    S_StartSound(actor as *mut c_void, Sfx::Flame as c_int);
    A_Fire(actor);
}
/// Repositions the Archvile fire object 24 units in front of its tracer (the victim).
///
/// Does nothing if `actor->tracer` is null. Checks sight from the Archvile's `target`
/// to the tracer; if the vile lost sight of its victim, the fire does not move.
/// Uses `P_UnsetThingPosition`/`P_SetThingPosition` to maintain blockmap consistency.
///
/// # Safety
///
/// `actor` must be non-null. `actor->tracer` if non-null must be a valid `mobj_t`.
/// `actor->target` if non-null must be a valid `mobj_t`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_Fire(actor: *mut mobj_t) {
    let dest: *mut mobj_t = (*actor).tracer;
    if dest.is_null() {
        return;
    }
    let target: *mut mobj_t = P_SubstNullMobj((*actor).target);
    if P_CheckSight(target, dest) == 0 {
        return;
    }
    let an: c_uint = ((*dest).angle >> ANGLETOFINESHIFT) as c_uint;
    P_UnsetThingPosition(actor as *mut CffiMobj);
    (*actor).x = (*dest).x + FixedMul(24 as fixed_t * FRACUNIT, *finecosine.0.add(an as usize));
    (*actor).y = (*dest).y + FixedMul(24 as fixed_t * FRACUNIT, finesine[an as usize]);
    (*actor).z = (*dest).z;
    P_SetThingPosition(actor as *mut CffiMobj);
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
/// `actor` must be non-null with a valid or null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_VileTarget(actor: *mut mobj_t) {
    if (*actor).target.is_null() {
        return;
    }
    A_FaceTarget(actor);
    // FIXME: p_enemy.c passes `actor->target->x` for the Y argument instead of
    // `actor->target->y` - this is a vanilla Doom bug reproduced faithfully here.
    let fog: *mut mobj_t = P_SpawnMobj(
        (*(*actor).target).x,
        (*(*actor).target).x,
        (*(*actor).target).z,
        MT_FIRE,
    );
    (*actor).tracer = fog;
    (*fog).target = actor;
    (*fog).tracer = (*actor).target;
    A_Fire(fog);
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
/// `actor` must be non-null with a valid or null `target` and `tracer`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_VileAttack(actor: *mut mobj_t) {
    if (*actor).target.is_null() {
        return;
    }
    A_FaceTarget(actor);
    if P_CheckSight(actor, (*actor).target) == 0 {
        return;
    }
    S_StartSound(actor as *mut c_void, Sfx::Barexp as c_int);
    P_DamageMobj((*actor).target, actor, actor, 20 as c_int);
    (*(*actor).target).momz =
        (1000 as c_int * FRACUNIT / (*((*(*actor).target).info as *mut MobjInfo)).mass) as fixed_t;
    let an: c_int = ((*actor).angle >> ANGLETOFINESHIFT) as c_int;
    let fire: *mut mobj_t = (*actor).tracer;
    if fire.is_null() {
        return;
    }
    (*fire).x =
        (*(*actor).target).x - FixedMul(24 as fixed_t * FRACUNIT, *finecosine.0.add(an as usize));
    (*fire).y = (*(*actor).target).y - FixedMul(24 as fixed_t * FRACUNIT, finesine[an as usize]);
    P_RadiusAttack(fire as *mut CffiMobj, actor as *mut CffiMobj, 70 as c_int);
}
/// Mancubus raise/taunt: faces the target and plays the attack preparation sound.
///
/// # Safety
///
/// `actor` must be non-null with a valid or null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_FatRaise(actor: *mut mobj_t) {
    A_FaceTarget(actor);
    S_StartSound(actor as *mut c_void, Sfx::Manatk as c_int);
}
/// Mancubus attack phase 1: two `MT_FATSHOT` fireballs spread to the right.
///
/// Rotates the actor's angle by `+FATSPREAD` (ANG90/8), fires one shot directly,
/// then fires a second shot additionally rotated by `+FATSPREAD` with velocity
/// recomputed from the new angle.
///
/// # Safety
///
/// `actor` must be non-null with a valid or null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_FatAttack1(actor: *mut mobj_t) {
    A_FaceTarget(actor);
    (*actor).angle = (*actor).angle.wrapping_add(FATSPREAD as angle_t);
    let target: *mut mobj_t = P_SubstNullMobj((*actor).target);
    P_SpawnMissile(actor, target, MT_FATSHOT);
    let mo: *mut mobj_t = P_SpawnMissile(actor, target, MT_FATSHOT);
    (*mo).angle = (*mo).angle.wrapping_add(FATSPREAD as angle_t);
    let an: c_int = ((*mo).angle >> ANGLETOFINESHIFT) as c_int;
    (*mo).momx = FixedMul(
        (*((*mo).info as *mut MobjInfo)).speed as fixed_t,
        *finecosine.0.add(an as usize),
    );
    (*mo).momy = FixedMul(
        (*((*mo).info as *mut MobjInfo)).speed as fixed_t,
        finesine[an as usize],
    );
}
/// Mancubus attack phase 2: two `MT_FATSHOT` fireballs spread to the left.
///
/// Rotates the actor's angle by `-FATSPREAD`, fires one shot directly, then fires a
/// second shot additionally rotated by `-2*FATSPREAD` with velocity recomputed.
///
/// # Safety
///
/// `actor` must be non-null with a valid or null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_FatAttack2(actor: *mut mobj_t) {
    A_FaceTarget(actor);
    (*actor).angle = (*actor).angle.wrapping_sub(FATSPREAD as angle_t);
    let target: *mut mobj_t = P_SubstNullMobj((*actor).target);
    P_SpawnMissile(actor, target, MT_FATSHOT);
    let mo: *mut mobj_t = P_SpawnMissile(actor, target, MT_FATSHOT);
    (*mo).angle = (*mo)
        .angle
        .wrapping_sub((FATSPREAD * 2 as c_int) as angle_t);
    let an: c_int = ((*mo).angle >> ANGLETOFINESHIFT) as c_int;
    (*mo).momx = FixedMul(
        (*((*mo).info as *mut MobjInfo)).speed as fixed_t,
        *finecosine.0.add(an as usize),
    );
    (*mo).momy = FixedMul(
        (*((*mo).info as *mut MobjInfo)).speed as fixed_t,
        finesine[an as usize],
    );
}
/// Mancubus attack phase 3: two `MT_FATSHOT` fireballs spread symmetrically ±FATSPREAD/2.
///
/// Fires one shot rotated `-FATSPREAD/2` and a second at `+FATSPREAD/2`; velocities are
/// recomputed for each so they actually travel in the spread directions.
///
/// # Safety
///
/// `actor` must be non-null with a valid or null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_FatAttack3(actor: *mut mobj_t) {
    let mut mo: *mut mobj_t;

    let mut an: c_int;

    A_FaceTarget(actor);
    let target: *mut mobj_t = P_SubstNullMobj((*actor).target);
    mo = P_SpawnMissile(actor, target, MT_FATSHOT);
    (*mo).angle = (*mo)
        .angle
        .wrapping_sub((FATSPREAD / 2 as c_int) as angle_t);
    an = ((*mo).angle >> ANGLETOFINESHIFT) as c_int;
    (*mo).momx = FixedMul(
        (*((*mo).info as *mut MobjInfo)).speed as fixed_t,
        *finecosine.0.add(an as usize),
    );
    (*mo).momy = FixedMul(
        (*((*mo).info as *mut MobjInfo)).speed as fixed_t,
        finesine[an as usize],
    );
    mo = P_SpawnMissile(actor, target, MT_FATSHOT);
    (*mo).angle = (*mo)
        .angle
        .wrapping_add((FATSPREAD / 2 as c_int) as angle_t);
    an = ((*mo).angle >> ANGLETOFINESHIFT) as c_int;
    (*mo).momx = FixedMul(
        (*((*mo).info as *mut MobjInfo)).speed as fixed_t,
        *finecosine.0.add(an as usize),
    );
    (*mo).momy = FixedMul(
        (*((*mo).info as *mut MobjInfo)).speed as fixed_t,
        finesine[an as usize],
    );
}
/// Lost Soul charge attack: sets `MF_SKULLFLY` and launches the actor as a living missile.
///
/// Sets horizontal momentum from `SKULLSPEED` scaled by the angle's fine-trig values.
/// Computes vertical momentum to reach the midpoint of the target's height over the
/// flight time (`dist / SKULLSPEED` tics, minimum 1).
///
/// # Safety
///
/// `actor` must be non-null with a valid or null `target` and valid `info`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_SkullAttack(actor: *mut mobj_t) {
    let mut dist: c_int;

    if (*actor).target.is_null() {
        return;
    }
    let dest: *mut mobj_t = (*actor).target;
    (*actor).flags |= MF_SKULLFLY as c_int;
    S_StartSound(
        actor as *mut c_void,
        (*((*actor).info as *mut MobjInfo)).attacksound as c_int,
    );
    A_FaceTarget(actor);
    let an: angle_t = (*actor).angle >> ANGLETOFINESHIFT;
    (*actor).momx = FixedMul(SKULLSPEED, *finecosine.0.add(an as usize));
    (*actor).momy = FixedMul(SKULLSPEED, finesine[an as usize]);
    dist = P_AproxDistance((*dest).x - (*actor).x, (*dest).y - (*actor).y) as c_int;
    dist /= SKULLSPEED;
    if dist < 1 as c_int {
        dist = 1 as c_int;
    }
    (*actor).momz = (((*dest).z as c_int + ((*dest).height as c_int >> 1 as c_int)
        - (*actor).z as c_int)
        / dist) as fixed_t;
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
/// `actor` must be non-null with valid `info` and a valid or null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_PainShootSkull(actor: *mut mobj_t, angle: angle_t) {
    let mut count: c_int;

    let mut currentthinker: *mut thinker_t;

    count = 0 as c_int;
    currentthinker = thinkercap.next;
    while !std::ptr::eq(currentthinker, &raw const thinkercap) {
        if (*currentthinker).function.acp1
            == core::mem::transmute::<
                Option<unsafe extern "C" fn(*mut mobj_t) -> ()>,
                Option<unsafe extern "C" fn(*mut c_void) -> ()>,
            >(Some(
                P_MobjThinker as unsafe extern "C" fn(*mut mobj_t) -> (),
            ))
            && (*(currentthinker as *mut mobj_t)).mobjtype as c_uint == MT_SKULL as c_int as c_uint
        {
            count += 1;
        }
        currentthinker = (*currentthinker).next;
    }
    if count > 20 as c_int {
        return;
    }
    let an: angle_t = angle >> ANGLETOFINESHIFT;
    let prestep: c_int = 4 as c_int * FRACUNIT
        + 3 as c_int
            * ((*((*actor).info as *mut MobjInfo)).radius
                + mobjinfo[MT_SKULL as c_int as usize].radius)
            / 2 as c_int;
    let x: fixed_t = (*actor).x + FixedMul(prestep as fixed_t, *finecosine.0.add(an as usize));
    let y: fixed_t = (*actor).y + FixedMul(prestep as fixed_t, finesine[an as usize]);
    let z: fixed_t = ((*actor).z as c_int + 8 as c_int * FRACUNIT) as fixed_t;
    let newmobj: *mut mobj_t = P_SpawnMobj(x, y, z, MT_SKULL);
    if P_TryMove(newmobj as *mut CffiMobj, (*newmobj).x, (*newmobj).y) == 0 {
        P_DamageMobj(newmobj, actor, actor, 10000 as c_int);
        return;
    }
    (*newmobj).target = (*actor).target;
    A_SkullAttack(newmobj);
}
/// Pain Elemental attack: spawns a Lost Soul in the direction the actor is facing.
///
/// # Safety
///
/// `actor` must be non-null with a valid or null `target`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_PainAttack(actor: *mut mobj_t) {
    if (*actor).target.is_null() {
        return;
    }
    A_FaceTarget(actor);
    A_PainShootSkull(actor, (*actor).angle);
}
/// Pain Elemental death: drops to the ground and spits three Lost Souls in cardinal directions.
///
/// Calls `A_Fall` to clear `MF_SOLID`, then spawns skulls at `angle+90`, `angle+180`,
/// and `angle+270` relative to the current facing direction.
///
/// # Safety
///
/// `actor` must be non-null. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_PainDie(actor: *mut mobj_t) {
    A_Fall(actor);
    A_PainShootSkull(actor, (*actor).angle.wrapping_add(ANG90 as angle_t));
    A_PainShootSkull(actor, (*actor).angle.wrapping_add(ANG180));
    A_PainShootSkull(actor, (*actor).angle.wrapping_add(ANG270));
}
/// Plays the monster's death scream; randomises for multi-variant sound monsters.
///
/// Zombie Pig (Podth variants) picks randomly from three sounds; Demon (Bgdth variants)
/// from two. Cyberdemon and Spider Mastermind play at full (global) volume; all others
/// play positioned at the actor.
///
/// # Safety
///
/// `actor` must be non-null with a valid `info`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_Scream(actor: *mut mobj_t) {
    let sound = match (*((*actor).info as *mut MobjInfo)).deathsound {
        Sfx::None => return,
        Sfx::Podth1 | Sfx::Podth2 | Sfx::Podth3 => Sfx::Podth1 as c_int + P_Random() % 3 as c_int,
        Sfx::Bgdth1 | Sfx::Bgdth2 => Sfx::Bgdth1 as c_int + P_Random() % 2 as c_int,
        s => s as c_int,
    };
    if (*actor).mobjtype as c_uint == MT_SPIDER as c_int as c_uint
        || (*actor).mobjtype as c_uint == MT_CYBORG as c_int as c_uint
    {
        S_StartSound(std::ptr::null_mut::<c_void>(), sound);
    } else {
        S_StartSound(actor as *mut c_void, sound);
    };
}
/// Plays the player/monster gibbing scream (`sfx_slop`) positioned at the actor.
///
/// # Safety
///
/// `actor` must be non-null. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_XScream(actor: *mut mobj_t) {
    S_StartSound(actor as *mut c_void, Sfx::Slop as c_int);
}
/// Plays the monster's pain sound if it has one.
///
/// # Safety
///
/// `actor` must be non-null with a valid `info`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_Pain(actor: *mut mobj_t) {
    if (*((*actor).info as *mut MobjInfo)).painsound != Sfx::None {
        S_StartSound(
            actor as *mut c_void,
            (*((*actor).info as *mut MobjInfo)).painsound as c_int,
        );
    }
}
/// Clears `MF_SOLID` so the falling corpse can be walked over.
///
/// # Safety
///
/// `actor` must be non-null. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_Fall(actor: *mut mobj_t) {
    (*actor).flags &= !(MF_SOLID as c_int);
}
/// Triggers a 128-unit radius blast centred on `thingy`, sourced from `thingy->target`.
///
/// Used by rockets and barrel explosions. Calls `P_RadiusAttack` which damages all
/// shootable things within range proportional to distance.
///
/// # Safety
///
/// `actor` (`thingy`) must be non-null; `thingy->target` may be null (passed directly
/// to `P_RadiusAttack`). Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_Explode(thingy: *mut mobj_t) {
    P_RadiusAttack(
        thingy as *mut CffiMobj,
        (*thingy).target as *mut CffiMobj,
        128 as c_int,
    );
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
///
/// # Safety
///
/// Reads `gameversion`, `gameepisode`, and `gamemap` globals which must be initialised.
unsafe extern "C" fn CheckBossEnd(motype: mobjtype_t) -> Boolean {
    if (gameversion as c_uint) < exe_ultimate as c_int as c_uint {
        if gamemap != 8 as c_int {
            return Boolean::FALSE;
        }
        if motype == MT_BRUISER && gameepisode != 1 as c_int {
            return Boolean::FALSE;
        }
        Boolean::TRUE
    } else {
        match gameepisode {
            1 => Boolean::from(gamemap == 8 as c_int && motype == MT_BRUISER),
            2 => Boolean::from(gamemap == 8 as c_int && motype == MT_CYBORG),
            3 => Boolean::from(gamemap == 8 as c_int && motype == MT_SPIDER),
            4 => Boolean::from(
                gamemap == 6 as c_int && motype == MT_CYBORG
                    || gamemap == 8 as c_int && motype == MT_SPIDER,
            ),
            _ => Boolean::from(gamemap == 8 as c_int),
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
/// `mo` must be non-null with a valid `mobjtype`. The thinker list and game-state globals
/// must be consistent. Called from C via state-machine action pointer.
#[no_mangle]
pub unsafe extern "C" fn A_BossDeath(mo: *mut mobj_t) {
    let mut th: *mut thinker_t;

    let mut mo2: *mut mobj_t;

    let mut junk: line_t = line_t {
        v1: std::ptr::null_mut::<vertex_t>(),
        v2: std::ptr::null_mut::<vertex_t>(),
        dx: 0,
        dy: 0,
        flags: 0,
        special: 0,
        tag: 0,
        sidenum: [0; 2],
        bbox: [0; 4],
        slopetype: 0,
        frontsector: std::ptr::null_mut::<c_void>(),
        backsector: std::ptr::null_mut::<c_void>(),
        validcount: 0,
        specialdata: std::ptr::null_mut::<c_void>(),
    };
    let _i: c_int = 0;
    if gamemode as c_uint == commercial as c_int as c_uint {
        if gamemap != 7 as c_int {
            return;
        }
        if (*mo).mobjtype as c_uint != MT_FATSO as c_int as c_uint
            && (*mo).mobjtype as c_uint != MT_BABY as c_int as c_uint
        {
            return;
        }
    } else if CheckBossEnd((*mo).mobjtype).is_false() {
        return;
    }
    let mut i: usize = 0;
    while i < MAXPLAYERS {
        if playeringame[i] != 0 && players[i].health > 0 as c_int {
            break;
        }
        i += 1;
    }
    if i == MAXPLAYERS {
        return;
    }
    th = thinkercap.next;
    while !std::ptr::eq(th, &raw const thinkercap) {
        if (*th).function.acp1
            == core::mem::transmute::<
                Option<unsafe extern "C" fn(*mut mobj_t) -> ()>,
                Option<unsafe extern "C" fn(*mut c_void) -> ()>,
            >(Some(
                P_MobjThinker as unsafe extern "C" fn(*mut mobj_t) -> (),
            ))
        {
            mo2 = th as *mut mobj_t;
            if mo2 != mo
                && (*mo2).mobjtype as c_uint == (*mo).mobjtype as c_uint
                && (*mo2).health > 0 as c_int
            {
                return;
            }
        }
        th = (*th).next;
    }
    if gamemode as c_uint == commercial as c_int as c_uint {
        if gamemap == 7 as c_int {
            if (*mo).mobjtype as c_uint == MT_FATSO as c_int as c_uint {
                junk.tag = 666 as c_short;
                EV_DoFloor(
                    &mut junk as *mut line_t as *mut PLineThing,
                    lowerFloorToLowest,
                );
                return;
            }
            if (*mo).mobjtype as c_uint == MT_BABY as c_int as c_uint {
                junk.tag = 667 as c_short;
                EV_DoFloor(&mut junk as *mut line_t as *mut PLineThing, raiseToTexture);
                return;
            }
        }
    } else {
        match gameepisode {
            1 => {
                junk.tag = 666 as c_short;
                EV_DoFloor(
                    &mut junk as *mut line_t as *mut PLineThing,
                    lowerFloorToLowest,
                );
                return;
            }
            4 => match gamemap {
                6 => {
                    junk.tag = 666 as c_short;
                    EV_DoDoor(&mut junk as *mut line_t as *mut PLineThing, vld_blazeOpen);
                    return;
                }
                8 => {
                    junk.tag = 666 as c_short;
                    EV_DoFloor(
                        &mut junk as *mut line_t as *mut PLineThing,
                        lowerFloorToLowest,
                    );
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
/// `mo` must be non-null with a valid `info`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_Hoof(mo: *mut mobj_t) {
    S_StartSound(mo as *mut c_void, Sfx::Hoof as c_int);
    A_Chase(mo);
}
/// Spider Mastermind footstep: plays `sfx_metal` then advances via `A_Chase`.
///
/// # Safety
///
/// `mo` must be non-null with a valid `info`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_Metal(mo: *mut mobj_t) {
    S_StartSound(mo as *mut c_void, Sfx::Metal as c_int);
    A_Chase(mo);
}
/// Arachnotron footstep: plays `sfx_bspwlk` then advances via `A_Chase`.
///
/// # Safety
///
/// `mo` must be non-null with a valid `info`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_BabyMetal(mo: *mut mobj_t) {
    S_StartSound(mo as *mut c_void, Sfx::Bspwlk as c_int);
    A_Chase(mo);
}
/// Super Shotgun open action: plays the barrel-break sound (`sfx_dbopn`) for the player.
///
/// # Safety
///
/// `player` must be non-null with a valid `mo`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_OpenShotgun2(player: *mut PlayerT, _psp: *mut PspdefT) {
    S_StartSound((*player).mo as *mut c_void, Sfx::Dbopn as c_int);
}
/// Super Shotgun load action: plays the shell-load sound (`sfx_dbload`) for the player.
///
/// # Safety
///
/// `player` must be non-null with a valid `mo`. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_LoadShotgun2(player: *mut PlayerT, _psp: *mut PspdefT) {
    S_StartSound((*player).mo as *mut c_void, Sfx::Dbload as c_int);
}
/// Super Shotgun close action: plays the close sound (`sfx_dbcls`) then checks for refire.
///
/// # Safety
///
/// `player` must be non-null with a valid `mo`; `psp` must be non-null. Called from C.
#[no_mangle]
pub unsafe extern "C" fn A_CloseShotgun2(player: *mut PlayerT, psp: *mut PspdefT) {
    S_StartSound((*player).mo as *mut c_void, Sfx::Dbcls as c_int);
    A_ReFire(player, psp);
}
/// Array of `MT_BOSSTARGET` map objects (Icon of Sin target spots) populated by `A_BrainAwake`.
/// The brain cycles through these to choose spawn destinations. Has C linkage.
#[no_mangle]
pub static mut braintargets: [*mut mobj_t; 32] = [std::ptr::null_mut::<mobj_t>(); 32];
/// Count of valid entries in `braintargets`; set by `A_BrainAwake`. Has C linkage.
#[no_mangle]
pub static mut numbraintargets: c_int = 0;
/// Round-robin index into `braintargets` for the next cube launch; advanced by `A_BrainSpit`.
/// Has C linkage.
#[no_mangle]
pub static mut braintargeton: c_int = 0 as c_int;
/// Icon of Sin wake-up: scans the thinker list for `MT_BOSSTARGET` spots and plays `sfx_bossit`.
///
/// Populates `braintargets` and resets `numbraintargets`/`braintargeton` to 0.
/// The target spots are placed by the level designer in the map; the brain cycles
/// through them to choose where to spit monster cubes.
///
/// # Safety
///
/// The thinker list must be consistent. Called from C via state-machine action pointer.
#[no_mangle]
pub unsafe extern "C" fn A_BrainAwake(_mo: *mut mobj_t) {
    let mut thinker: *mut thinker_t;

    let mut m: *mut mobj_t;

    numbraintargets = 0 as c_int;
    braintargeton = 0 as c_int;
    thinker = thinkercap.next;
    while !std::ptr::eq(thinker, &raw const thinkercap) {
        if (*thinker).function.acp1
            == core::mem::transmute::<
                Option<unsafe extern "C" fn(*mut mobj_t) -> ()>,
                Option<unsafe extern "C" fn(*mut c_void) -> ()>,
            >(Some(
                P_MobjThinker as unsafe extern "C" fn(*mut mobj_t) -> (),
            ))
        {
            m = thinker as *mut mobj_t;
            if (*m).mobjtype as c_uint == MT_BOSSTARGET as c_int as c_uint {
                braintargets[numbraintargets as usize] = m;
                numbraintargets += 1;
            }
        }
        thinker = (*thinker).next;
    }
    S_StartSound(std::ptr::null_mut::<c_void>(), Sfx::Bossit as c_int);
}
/// Icon of Sin pain reaction: plays `sfx_bospn` at full (global) volume.
///
/// # Safety
///
/// Called from C. No pointer dereference; safe with any (even null) `_mo`.
#[no_mangle]
pub unsafe extern "C" fn A_BrainPain(_mo: *mut mobj_t) {
    S_StartSound(std::ptr::null_mut::<c_void>(), Sfx::Bospn as c_int);
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
/// `mo` must be non-null. Called from C via state-machine action pointer.
#[no_mangle]
pub unsafe extern "C" fn A_BrainScream(mo: *mut mobj_t) {
    let mut x: c_int;

    let mut y: c_int;

    let mut z: c_int;

    let mut th: *mut mobj_t;

    x = (*mo).x as c_int - 196 as c_int * FRACUNIT;
    while x < (*mo).x as c_int + 320 as c_int * FRACUNIT {
        y = (*mo).y as c_int - 320 as c_int * FRACUNIT;
        z = 128 as c_int + P_Random() * 2 as c_int * FRACUNIT;
        th = P_SpawnMobj(x as fixed_t, y as fixed_t, z as fixed_t, MT_ROCKET);
        (*th).momz = (P_Random() * 512 as c_int) as fixed_t;
        P_SetMobjState(th, S_BRAINEXPLODE1);
        (*th).tics -= P_Random() & 7 as c_int;
        if (*th).tics < 1 as c_int {
            (*th).tics = 1 as c_int;
        }
        x += FRACUNIT * 8 as c_int;
    }
    S_StartSound(std::ptr::null_mut::<c_void>(), Sfx::Bosdth as c_int);
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
/// `mo` must be non-null. Called from C via state-machine action pointer.
#[no_mangle]
pub unsafe extern "C" fn A_BrainExplode(mo: *mut mobj_t) {
    let x: c_int = (*mo).x as c_int + (P_Random() - P_Random()) * 2048 as c_int;
    let y: c_int = (*mo).y as c_int;
    let z: c_int = 128 as c_int + P_Random() * 2 as c_int * FRACUNIT;
    let th: *mut mobj_t = P_SpawnMobj(x as fixed_t, y as fixed_t, z as fixed_t, MT_ROCKET);
    (*th).momz = (P_Random() * 512 as c_int) as fixed_t;
    P_SetMobjState(th, S_BRAINEXPLODE1);
    (*th).tics -= P_Random() & 7 as c_int;
    if (*th).tics < 1 as c_int {
        (*th).tics = 1 as c_int;
    }
}
/// Icon of Sin death: ends the level via `G_ExitLevel`.
///
/// # Safety
///
/// Called from C. No pointer dereference beyond the ignored `_mo`.
#[no_mangle]
pub unsafe extern "C" fn A_BrainDie(_mo: *mut mobj_t) {
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
/// `mo` must be non-null. `braintargets` must have been populated by `A_BrainAwake` and
/// `numbraintargets` must be > 0. Called from C via state-machine action pointer.
#[no_mangle]
pub unsafe extern "C" fn A_BrainSpit(mo: *mut mobj_t) {
    static mut easy: c_int = 0 as c_int;
    easy ^= 1 as c_int;
    if gameskill as c_int <= sk_easy as c_int && easy == 0 {
        return;
    }
    let targ: *mut mobj_t = braintargets[braintargeton as usize];
    braintargeton = (braintargeton + 1 as c_int) % numbraintargets;
    let newmobj: *mut mobj_t = P_SpawnMissile(mo, targ, MT_SPAWNSHOT);
    (*newmobj).target = targ;
    (*newmobj).reactiontime = ((*targ).y as c_int - (*mo).y as c_int)
        / (*newmobj).momy as c_int
        / (*((*newmobj).state as *mut State)).tics;
    S_StartSound(std::ptr::null_mut::<c_void>(), Sfx::Bospit as c_int);
}
/// In-flight cube sound: plays `sfx_boscub` while the cube travels, then calls `A_SpawnFly`.
///
/// # Safety
///
/// `mo` must be non-null. Called from C via state-machine action pointer.
#[no_mangle]
pub unsafe extern "C" fn A_SpawnSound(mo: *mut mobj_t) {
    S_StartSound(mo as *mut c_void, Sfx::Boscub as c_int);
    A_SpawnFly(mo);
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
/// `mo` must be non-null with a valid `target`. `mo->target` must point to a valid
/// `mobj_t` (the boss target spot). Called from C via state-machine action pointer.
#[no_mangle]
pub unsafe extern "C" fn A_SpawnFly(mo: *mut mobj_t) {
    let type_0: mobjtype_t;

    (*mo).reactiontime -= 1;
    if (*mo).reactiontime != 0 {
        return;
    }
    let targ: *mut mobj_t = P_SubstNullMobj((*mo).target);
    let fog: *mut mobj_t = P_SpawnMobj((*targ).x, (*targ).y, (*targ).z, MT_SPAWNFIRE);
    S_StartSound(fog as *mut c_void, Sfx::Telept as c_int);
    let r: c_int = P_Random();
    if r < 50 as c_int {
        type_0 = MT_TROOP;
    } else if r < 90 as c_int {
        type_0 = MT_SERGEANT;
    } else if r < 120 as c_int {
        type_0 = MT_SHADOWS;
    } else if r < 130 as c_int {
        type_0 = MT_PAIN;
    } else if r < 160 as c_int {
        type_0 = MT_HEAD;
    } else if r < 162 as c_int {
        type_0 = MT_VILE;
    } else if r < 172 as c_int {
        type_0 = MT_UNDEAD;
    } else if r < 192 as c_int {
        type_0 = MT_BABY;
    } else if r < 222 as c_int {
        type_0 = MT_FATSO;
    } else if r < 246 as c_int {
        type_0 = MT_KNIGHT;
    } else {
        type_0 = MT_BRUISER;
    }
    let newmobj: *mut mobj_t = P_SpawnMobj((*targ).x, (*targ).y, (*targ).z, type_0);
    if P_LookForPlayers(newmobj, Boolean::TRUE).is_truthy() {
        P_SetMobjState(
            newmobj,
            (*((*newmobj).info as *mut MobjInfo)).seestate as statenum_t,
        );
    }
    P_TeleportMove(newmobj as *mut CffiMobj, (*newmobj).x, (*newmobj).y);
    P_RemoveMobj(mo);
}
/// Plays the player death scream; uses the gibbing scream in Doom II when health is below -50.
///
/// Default sound is `sfx_pldeth`. In commercial mode, if `mo->health < -50`, plays
/// `sfx_pdiehi` (the "squish" gib scream) instead.
///
/// # Safety
///
/// `mo` must be non-null. Called from C via state-machine action pointer.
#[no_mangle]
pub unsafe extern "C" fn A_PlayerScream(mo: *mut mobj_t) {
    let mut sound: c_int = Sfx::Pldeth as c_int;
    if gamemode as c_uint == commercial as c_int as c_uint && (*mo).health < -50 as c_int {
        sound = Sfx::Pdiehi as c_int;
    }
    S_StartSound(mo as *mut c_void, sound);
}
