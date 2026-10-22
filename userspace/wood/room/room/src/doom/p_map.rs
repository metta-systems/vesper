//! Movement, collision handling, shooting, and aiming for the Doom engine.
//!
//! Rust port of `vendor/doomgeneric/p_map.c`. Implements position checking
//! (`P_CheckPosition`, `P_TryMove`), slide movement (`P_SlideMove`), hitscan
//! attacks (`P_AimLineAttack`, `P_LineAttack`), the Use action
//! (`P_UseLines`), radius splash damage (`P_RadiusAttack`), and sector-height
//! change propagation (`P_ChangeSector`).
//!
//! ## Rust-vs-C differences
//!
//! The `spechit` array is declared with 20 slots (`MAXSPECIALCROSS_OVERFLOW`)
//! rather than the vanilla Doom value of 8 (`MAXSPECIALCROSS`). `SpechitOverrun`
//! emulates the original buffer-overrun behavior when more than 8 specials are
//! crossed in one move, writing into adjacent globals in the same order as the
//! vanilla binary - controlled by the `-spechit` command-line argument.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::{c_int, c_uint, c_void};
use std::ptr;

use crate::doom::c_ffi::{
    intercept_t, line_t, mobj_t, sector_t, subsector_t, LinedefFlag, DEFAULT_SPECHIT_MAGIC,
    MAPBLOCKSHIFT,
};
use crate::doom::info::MobjInfo;
use crate::doom::m_bbox::BBox;
use crate::doom::m_fixed::{fixed_t, FixedDiv, FixedMul, FRACBITS, FRACUNIT};
use crate::doom::m_random::P_Random;
use crate::doom::p_maputl::{
    lowfloor, openbottom, openrange, opentop, P_AproxDistance, P_BlockLinesIterator,
    P_BlockThingsIterator, P_BoxOnLineSide, P_LineOpening, P_PathTraverse, P_PointOnLineSide,
    P_SetThingPosition, P_UnsetThingPosition, PT_ADDLINES, PT_ADDTHINGS,
};
use crate::doom::p_setup::{bmaporgx, bmaporgy, lines};
use crate::doom::p_sight::{bottomslope, topslope, P_CheckSight};
use crate::doom::r_main::{validcount, R_PointInSubsector, R_PointToAngle2};
use crate::doom::tables::{finecosine, finesine, ANG180, ANGLETOFINESHIFT};
use crate::i_error;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of special lines that can be crossed during a single move.
///
/// The Rust port uses 20 slots (matching `MAXSPECIALCROSS` in the C source)
/// while the original vanilla limit was 8 (`MAXSPECIALCROSS_ORIGINAL`).
const MAXSPECIALCROSS: usize = 20;

/// Original vanilla Doom limit for special lines crossed per move.
///
/// Crossings beyond this count trigger [`SpechitOverrun`] to emulate the
/// vanilla memory-corruption behaviour that some demos depend on.
const MAXSPECIALCROSS_ORIGINAL: c_int = 8;

/// Maximum radius of any map object, in map units (fixed-point).
///
/// Used to expand blockmap queries so that objects whose origin lies in an
/// adjacent block but overlaps the query region are still found.
const MAXRADIUS: c_int = 32 * FRACUNIT;

/// Forward reach of the player's Use action, in map units (fixed-point).
const USERANGE: c_int = 64 * FRACUNIT;

/// Map-object flag: the thing is a special pickup item.
const MF_SPECIAL: c_int = 1;
/// Map-object flag: the thing blocks movement (solid).
const MF_SOLID: c_int = 2;
/// Map-object flag: the thing can be damaged by hitscan or projectile attacks.
const MF_SHOOTABLE: c_int = 4;
/// Map-object flag: the thing is not added to the sector thing list.
const MF_NOSECTOR: c_int = 8;
/// Map-object flag: the thing is not added to the blockmap.
const MF_NOBLOCKMAP: c_int = 16;
/// Map-object flag: the thing is a projectile (missile).
const MF_MISSILE: c_int = 65536;
/// Map-object flag: the thing can walk off ledges.
const MF_DROPOFF: c_int = 1024;
/// Map-object flag: the thing ignores clipping (no-clip cheat).
const MF_NOCLIP: c_int = 4096;
/// Map-object flag: the thing can fly vertically (floats in air).
const MF_FLOAT: c_int = 16384;
/// Map-object flag: the thing is mid-teleport and bypasses step/dropoff checks.
const MF_TELEPORT: c_int = 32768;
/// Map-object flag: a Lost Soul currently in charge-flight mode.
const MF_SKULLFLY: c_int = 16777216;
/// Map-object flag: the thing picks up items on contact.
const MF_PICKUP: c_int = 2048;
/// Map-object flag: the thing does not bleed when damaged (spawns puff instead).
const MF_NOBLOOD: c_int = 524288;
/// Map-object flag: the thing was dropped by a monster and should be removed when crushed.
const MF_DROPPED: c_int = 131072;

/// Thing-type index for the player.
const MT_PLAYER: c_int = 0;
/// Thing-type index for the Hell Knight.
const MT_KNIGHT: c_int = 17;
/// Thing-type index for the Baron of Hell.
const MT_BRUISER: c_int = 15;
/// Thing-type index for the Cyberdemon (immune to splash damage).
const MT_CYBORG: c_int = 21;
/// Thing-type index for the Spider Mastermind (immune to splash damage).
const MT_SPIDER: c_int = 19;
/// Thing-type index for the blood splat particle spawned during crushing.
const MT_BLOOD: c_int = 38;

/// State index for the "gibs" sprite, used when corpses are crushed.
const S_GIBS: c_int = 895;

/// Line slope type: perfectly horizontal (dy == 0).
const ST_HORIZONTAL: c_int = 0;
/// Line slope type: perfectly vertical (dx == 0).
const ST_VERTICAL: c_int = 1;

/// Default value for the DEH species-infighting flag (disabled).
///
/// When 0, monsters of the same species cannot hurt each other with projectiles.
/// A DeHackEd patch can override this.
const DEH_DEFAULT_SPECIES_INFIGHTING: c_int = 0;

// ---------------------------------------------------------------------------
// Extern functions from other modules
// ---------------------------------------------------------------------------

use crate::doom::g_game::gamemap;
use crate::doom::m_argv::{myargv, M_CheckParmWithArgs};
use crate::doom::m_misc::M_StrToInt;
use crate::doom::p_inter::{P_DamageMobj, P_TouchSpecialThing};
use crate::doom::p_mobj::{
    P_RemoveMobj, P_SetMobjState, P_SpawnBlood, P_SpawnMobj, P_SpawnPuff, P_SubstNullMobj,
};
use crate::doom::p_spec::{P_CrossSpecialLine, P_ShootSpecialLine};
use crate::doom::p_switch::P_UseSpecialLine;
use crate::doom::p_tick::leveltime;
use crate::doom::r_sky::skyflatnum;
use crate::doom::s_sound::S_StartSound;
use crate::doom::sounds::Sfx;

/// Type alias used when calling functions in `p_telept` that expect its own
/// local `mobj_t` definition (all `#[repr(C)]` layouts are identical).
type TeleptMobj = crate::doom::p_telept::mobj_t;

// ---------------------------------------------------------------------------
// Movement scratchpad globals
// ---------------------------------------------------------------------------

/// Axis-aligned bounding box of the thing currently being position-checked.
///
/// Set by [`P_CheckPosition`] / [`P_TeleportMove`] before iterating over
/// blockmap cells. Indices follow [`BBox`] (TOP, BOTTOM, LEFT, RIGHT).
#[no_mangle]
pub static mut tmbbox: [fixed_t; 4] = [0; 4];

/// Pointer to the map object currently being tested by [`P_CheckPosition`].
///
/// Used by the blockmap iterator callbacks ([`PIT_CheckThing`],
/// [`PIT_CheckLine`]) to access the moving object without passing it through
/// the C-style function-pointer interface.
#[no_mangle]
pub static mut tmthing: *mut mobj_t = ptr::null_mut();

/// Cached copy of `tmthing->flags` for the current position check.
///
/// Avoids repeated pointer dereferences inside tight blockmap loops.
#[no_mangle]
pub static mut tmflags: c_int = 0;

/// Destination x-coordinate being tested in the current position check.
#[no_mangle]
pub static mut tmx: fixed_t = 0;

/// Destination y-coordinate being tested in the current position check.
#[no_mangle]
pub static mut tmy: fixed_t = 0;

/// Set to non-zero by [`P_TryMove`] when the gap between floor and ceiling
/// is large enough for the thing to fit, even if other constraints still
/// block the move.
///
/// Callers (e.g. floating monsters) read this to decide whether to keep
/// trying to ascend or descend rather than giving up entirely.
#[no_mangle]
pub static mut floatok: c_int = 0; // boolean

/// Highest floor height touched during the current position check.
///
/// Updated by [`PIT_CheckLine`] as two-sided linedefs are crossed.
/// After [`P_CheckPosition`] returns, this is the floor the thing would
/// stand on at the tested position.
#[no_mangle]
pub static mut tmfloorz: fixed_t = 0;

/// Lowest ceiling height encountered during the current position check.
///
/// Updated by [`PIT_CheckLine`]. After [`P_CheckPosition`] returns, this
/// is the ceiling height at the tested position.
#[no_mangle]
pub static mut tmceilingz: fixed_t = 0;

/// Lowest floor height seen across all contacted sectors during the check.
///
/// Monsters will not move to a position where `tmfloorz - tmdropoffz`
/// exceeds 24 map units unless they have `MF_DROPOFF` or `MF_FLOAT`.
#[no_mangle]
pub static mut tmdropoffz: fixed_t = 0;

/// The linedef that produced the current value of [`tmceilingz`].
///
/// Missiles use this to avoid exploding against "sky hack" walls: if the
/// ceiling line's front sector has the sky flat, the missile silently
/// disappears instead of spawning a puff.
#[no_mangle]
pub static mut ceilingline: *mut line_t = ptr::null_mut();

/// Array of special linedefs crossed during the current move attempt.
///
/// Filled by [`PIT_CheckLine`]; processed by [`P_TryMove`] once the move
/// is confirmed valid. Kept separate so specials are not triggered for
/// moves that ultimately fail.
#[no_mangle]
pub static mut spechit: [*mut line_t; MAXSPECIALCROSS] = [ptr::null_mut(); MAXSPECIALCROSS];

/// Number of valid entries currently in [`spechit`].
#[no_mangle]
pub static mut numspechit: c_int = 0;

// ---------------------------------------------------------------------------
// TELEPORT MOVE
// ---------------------------------------------------------------------------

/// Blockmap iterator callback used by [`P_TeleportMove`] to stomp any thing
/// occupying the teleport destination.
///
/// Returns 1 (true) to continue iteration in most cases; returns 0 (false)
/// only when a non-boss monster would need to stomp something it is not
/// allowed to stomp, aborting the teleport.
///
/// # Safety
///
/// `thing` must be a valid, non-null pointer to an initialised [`mobj_t`].
/// [`tmthing`] and [`tmx`] / [`tmy`] must have been set by the caller
/// ([`P_TeleportMove`]) before this callback is invoked.
#[no_mangle]
pub unsafe extern "C" fn PIT_StompThing(thing: *mut mobj_t) -> c_uint {
    let thing = &*thing;
    if thing.flags & MF_SHOOTABLE == 0 {
        return 1;
    }
    let blockdist = thing.radius + (*tmthing).radius;
    if (thing.x - tmx).wrapping_abs() >= blockdist || (thing.y - tmy).wrapping_abs() >= blockdist {
        return 1;
    }
    if std::ptr::eq(thing, tmthing) {
        return 1;
    }
    if (*tmthing).player.is_null() && gamemap != 30 {
        return 0;
    }
    P_DamageMobj(
        thing as *const _ as *mut TeleptMobj,
        tmthing as *mut TeleptMobj,
        tmthing as *mut TeleptMobj,
        10000,
    );
    1
}

/// Teleport a thing to `(x, y)`, stomping any occupant at the destination.
///
/// Unlike [`P_TryMove`], this function does not check line-of-sight,
/// step height, or dropoff constraints — it is intended for teleporter
/// effects and spawn placement. Returns 1 on success, 0 if a non-boss
/// monster cannot stomp the occupant.
///
/// # Safety
///
/// `thing` must be a valid, non-null pointer to a live [`mobj_t`] that is
/// already linked into the map (sector list and blockmap). The blockmap and
/// sector structures must be fully initialised.
#[no_mangle]
pub unsafe extern "C" fn P_TeleportMove(thing: *mut mobj_t, x: fixed_t, y: fixed_t) -> c_uint {
    tmthing = thing;
    tmflags = (*thing).flags;
    tmx = x;
    tmy = y;

    tmbbox[BBox::TOP] = y + (*tmthing).radius;
    tmbbox[BBox::BOTTOM] = y - (*tmthing).radius;
    tmbbox[BBox::RIGHT] = x + (*tmthing).radius;
    tmbbox[BBox::LEFT] = x - (*tmthing).radius;

    let newsubsec = R_PointInSubsector(x, y) as *mut subsector_t;
    ceilingline = ptr::null_mut();
    let sec = (*newsubsec).sector as *mut sector_t;
    tmfloorz = (*sec).floorheight;
    tmdropoffz = tmfloorz;
    tmceilingz = (*sec).ceilingheight;

    validcount = validcount.wrapping_add(1);
    numspechit = 0;

    let xl = (tmbbox[BBox::LEFT] - bmaporgx - MAXRADIUS) >> MAPBLOCKSHIFT;
    let xh = (tmbbox[BBox::RIGHT] - bmaporgx + MAXRADIUS) >> MAPBLOCKSHIFT;
    let yl = (tmbbox[BBox::BOTTOM] - bmaporgy - MAXRADIUS) >> MAPBLOCKSHIFT;
    let yh = (tmbbox[BBox::TOP] - bmaporgy + MAXRADIUS) >> MAPBLOCKSHIFT;

    for bx in xl..=xh {
        for by in yl..=yh {
            if P_BlockThingsIterator(bx, by, Some(PIT_StompThing)) == 0 {
                return 0;
            }
        }
    }

    P_UnsetThingPosition(thing);
    (*thing).floorz = tmfloorz;
    (*thing).ceilingz = tmceilingz;
    (*thing).x = x;
    (*thing).y = y;
    P_SetThingPosition(thing);

    1
}

// ---------------------------------------------------------------------------
// MOVEMENT ITERATOR FUNCTIONS
// ---------------------------------------------------------------------------

/// Blockmap linedef iterator callback for [`P_CheckPosition`].
///
/// Tests whether linedef `ld` blocks the current move. If the line is
/// two-sided and passable, [`tmfloorz`], [`tmceilingz`], and
/// [`tmdropoffz`] are updated to reflect the opening. Special linedefs
/// are recorded in [`spechit`] for later processing by [`P_TryMove`].
///
/// Returns 1 to continue iteration, 0 to abort (line blocks the move).
///
/// # Safety
///
/// `ld` must be a valid, non-null pointer to an initialised [`line_t`].
/// [`tmthing`], [`tmbbox`], [`tmfloorz`], [`tmceilingz`], and
/// [`tmdropoffz`] must have been initialised by [`P_CheckPosition`] before
/// this callback is invoked.
#[no_mangle]
pub unsafe extern "C" fn PIT_CheckLine(ld: *mut line_t) -> c_uint {
    let ld = &*ld;
    if tmbbox[BBox::RIGHT] <= ld.bbox[BBox::LEFT]
        || tmbbox[BBox::LEFT] >= ld.bbox[BBox::RIGHT]
        || tmbbox[BBox::TOP] <= ld.bbox[BBox::BOTTOM]
        || tmbbox[BBox::BOTTOM] >= ld.bbox[BBox::TOP]
    {
        return 1;
    }
    if P_BoxOnLineSide(std::ptr::addr_of_mut!(tmbbox[0]), ld as *const _ as *mut _) != -1 {
        return 1;
    }
    if ld.backsector.is_null() {
        return 0;
    }
    if (*tmthing).flags & MF_MISSILE == 0 {
        if (ld.flags as c_int) & (LinedefFlag::BLOCKING as c_int) != 0 {
            return 0;
        }
        if (*tmthing).player.is_null()
            && (ld.flags as c_int) & (LinedefFlag::BLOCKMONSTERS as c_int) != 0
        {
            return 0;
        }
    }
    P_LineOpening(ld as *const _ as *mut _);
    if opentop < tmceilingz {
        tmceilingz = opentop;
        ceilingline = ld as *const _ as *mut _;
    }
    if openbottom > tmfloorz {
        tmfloorz = openbottom;
    }
    if lowfloor < tmdropoffz {
        tmdropoffz = lowfloor;
    }
    if ld.special != 0 {
        spechit[numspechit as usize] = ld as *const _ as *mut _;
        numspechit += 1;
        if numspechit > MAXSPECIALCROSS_ORIGINAL {
            SpechitOverrun(ld as *const _ as *mut _);
        }
    }
    1
}

/// Blockmap thing iterator callback for [`P_CheckPosition`].
///
/// Tests whether `thing` blocks or interacts with the moving object
/// (`tmthing`). Handles skull-fly collision damage, missile detonation,
/// same-species missile pass-through, and special item pickup. Returns 1
/// to continue blockmap iteration; returns 0 to stop (collision confirmed
/// or skull charge resolved).
///
/// # Safety
///
/// `thing` must be a valid, non-null pointer to an initialised [`mobj_t`].
/// [`tmthing`], [`tmx`], [`tmy`], and [`tmflags`] must have been set by
/// [`P_CheckPosition`] before this callback is invoked.
#[no_mangle]
pub unsafe extern "C" fn PIT_CheckThing(thing: *mut mobj_t) -> c_uint {
    let thing = &*thing;
    if thing.flags & (MF_SOLID | MF_SPECIAL | MF_SHOOTABLE) == 0 {
        return 1;
    }
    let blockdist = thing.radius + (*tmthing).radius;
    if (thing.x - tmx).wrapping_abs() >= blockdist || (thing.y - tmy).wrapping_abs() >= blockdist {
        return 1;
    }
    if std::ptr::eq(thing, tmthing) {
        return 1;
    }

    // check for skulls slamming into things
    if (*tmthing).flags & MF_SKULLFLY != 0 {
        let damage = ((P_Random() % 8) + 1) * (*((*tmthing).info as *mut MobjInfo)).damage;
        P_DamageMobj(
            thing as *const _ as *mut TeleptMobj,
            tmthing as *mut TeleptMobj,
            tmthing as *mut TeleptMobj,
            damage,
        );
        (*tmthing).flags &= !MF_SKULLFLY;
        (*tmthing).momx = 0;
        (*tmthing).momy = 0;
        (*tmthing).momz = 0;
        P_SetMobjState(
            tmthing as *mut TeleptMobj,
            (*((*tmthing).info as *mut MobjInfo)).spawnstate,
        );
        return 0;
    }

    // missiles can hit other things
    if (*tmthing).flags & MF_MISSILE != 0 {
        if (*tmthing).z > thing.z + thing.height {
            return 1;
        }
        if (*tmthing).z + (*tmthing).height < thing.z {
            return 1;
        }
        if !(*tmthing).target.is_null() {
            let target = (*tmthing).target as *mut mobj_t;
            let target_type = (*target).type_;
            let thing_type = thing.type_;
            if target_type == thing_type
                || (target_type == MT_KNIGHT && thing_type == MT_BRUISER)
                || (target_type == MT_BRUISER && thing_type == MT_KNIGHT)
            {
                if std::ptr::eq(thing, target) {
                    return 1;
                }
                if thing_type != MT_PLAYER && DEH_DEFAULT_SPECIES_INFIGHTING == 0 {
                    return 0;
                }
            }
        }
        if thing.flags & MF_SHOOTABLE == 0 {
            return (thing.flags & MF_SOLID == 0) as c_uint;
        }
        let damage = ((P_Random() % 8) + 1) * (*((*tmthing).info as *mut MobjInfo)).damage;
        let target = (*tmthing).target as *mut TeleptMobj;
        P_DamageMobj(
            thing as *const _ as *mut TeleptMobj,
            tmthing as *mut TeleptMobj,
            target,
            damage,
        );
        return 0;
    }

    // check for special pickup
    if thing.flags & MF_SPECIAL != 0 {
        let solid = thing.flags & MF_SOLID;
        if tmflags & MF_PICKUP != 0 {
            P_TouchSpecialThing(
                thing as *const _ as *mut TeleptMobj,
                tmthing as *mut TeleptMobj,
            );
        }
        return (solid == 0) as c_uint;
    }

    (thing.flags & MF_SOLID == 0) as c_uint
}

// ---------------------------------------------------------------------------
// MOVEMENT CLIPPING
// ---------------------------------------------------------------------------

/// Test whether `thing` can occupy position `(x, y)` without clipping.
///
/// This is a pure query — it does not move the thing. As a side effect it
/// sets [`tmfloorz`], [`tmceilingz`], [`tmdropoffz`], [`spechit`], and
/// [`numspechit`] for the tested position. Things with `MF_PICKUP` may
/// pick up items encountered during the sweep.
///
/// Returns 1 if the position is unobstructed, 0 if blocked.
///
/// # Safety
///
/// `thing` must be a valid, non-null pointer to an initialised [`mobj_t`].
/// The blockmap and all sector/linedef structures must be fully initialised.
#[no_mangle]
pub unsafe extern "C" fn P_CheckPosition(thing: *mut mobj_t, x: fixed_t, y: fixed_t) -> c_uint {
    tmthing = thing;
    tmflags = (*thing).flags;
    tmx = x;
    tmy = y;

    tmbbox[BBox::TOP] = y + (*tmthing).radius;
    tmbbox[BBox::BOTTOM] = y - (*tmthing).radius;
    tmbbox[BBox::RIGHT] = x + (*tmthing).radius;
    tmbbox[BBox::LEFT] = x - (*tmthing).radius;

    let newsubsec = R_PointInSubsector(x, y) as *mut subsector_t;
    ceilingline = ptr::null_mut();
    let sec = (*newsubsec).sector as *mut sector_t;
    tmfloorz = (*sec).floorheight;
    tmdropoffz = tmfloorz;
    tmceilingz = (*sec).ceilingheight;

    validcount = validcount.wrapping_add(1);
    numspechit = 0;

    if tmflags & MF_NOCLIP != 0 {
        return 1;
    }

    let xl = (tmbbox[BBox::LEFT] - bmaporgx - MAXRADIUS) >> MAPBLOCKSHIFT;
    let xh = (tmbbox[BBox::RIGHT] - bmaporgx + MAXRADIUS) >> MAPBLOCKSHIFT;
    let yl = (tmbbox[BBox::BOTTOM] - bmaporgy - MAXRADIUS) >> MAPBLOCKSHIFT;
    let yh = (tmbbox[BBox::TOP] - bmaporgy + MAXRADIUS) >> MAPBLOCKSHIFT;

    for bx in xl..=xh {
        for by in yl..=yh {
            if P_BlockThingsIterator(bx, by, Some(PIT_CheckThing)) == 0 {
                return 0;
            }
        }
    }

    let xl = (tmbbox[BBox::LEFT] - bmaporgx) >> MAPBLOCKSHIFT;
    let xh = (tmbbox[BBox::RIGHT] - bmaporgx) >> MAPBLOCKSHIFT;
    let yl = (tmbbox[BBox::BOTTOM] - bmaporgy) >> MAPBLOCKSHIFT;
    let yh = (tmbbox[BBox::TOP] - bmaporgy) >> MAPBLOCKSHIFT;

    for bx in xl..=xh {
        for by in yl..=yh {
            if P_BlockLinesIterator(bx, by, Some(PIT_CheckLine)) == 0 {
                return 0;
            }
        }
    }

    1
}

/// Attempt to move `thing` to `(x, y)`, triggering crossed linedef specials.
///
/// Calls [`P_CheckPosition`] internally. If the position is valid and all
/// height constraints pass, the thing is re-linked at the new position and
/// any special linedefs in [`spechit`] that were actually crossed are
/// activated. Things with `MF_TELEPORT` or `MF_NOCLIP` skip special-line
/// processing.
///
/// Returns 1 on success, 0 if the move is blocked.
///
/// # Safety
///
/// `thing` must be a valid, non-null pointer to a live [`mobj_t`] that is
/// already linked into the map. The map data must be fully initialised.
#[no_mangle]
pub unsafe extern "C" fn P_TryMove(thing: *mut mobj_t, x: fixed_t, y: fixed_t) -> c_uint {
    floatok = 0;
    if P_CheckPosition(thing, x, y) == 0 {
        return 0;
    }
    if (*thing).flags & MF_NOCLIP == 0 {
        if tmceilingz - tmfloorz < (*thing).height {
            return 0;
        }
        floatok = 1;
        if (*thing).flags & MF_TELEPORT == 0 && tmceilingz - (*thing).z < (*thing).height {
            return 0;
        }
        if (*thing).flags & MF_TELEPORT == 0 && tmfloorz - (*thing).z > 24 * FRACUNIT {
            return 0;
        }
        if (*thing).flags & (MF_DROPOFF | MF_FLOAT) == 0 && tmfloorz - tmdropoffz > 24 * FRACUNIT {
            return 0;
        }
    }

    P_UnsetThingPosition(thing);
    let oldx = (*thing).x;
    let oldy = (*thing).y;
    (*thing).floorz = tmfloorz;
    (*thing).ceilingz = tmceilingz;
    (*thing).x = x;
    (*thing).y = y;
    P_SetThingPosition(thing);

    if (*thing).flags & (MF_TELEPORT | MF_NOCLIP) == 0 {
        while numspechit > 0 {
            numspechit -= 1;
            let ld = spechit[numspechit as usize];
            let side = P_PointOnLineSide((*thing).x, (*thing).y, ld);
            let oldside = P_PointOnLineSide(oldx, oldy, ld);
            if side != oldside && (*ld).special != 0 {
                let linenum = ld.offset_from(lines);
                P_CrossSpecialLine(linenum as c_int, oldside, thing);
            }
        }
    }

    1
}

/// Clip a thing's z-position after a nearby sector floor or ceiling has moved.
///
/// Re-runs [`P_CheckPosition`] at the thing's current x/y to refresh
/// [`tmfloorz`] and [`tmceilingz`], then adjusts `thing->z` if necessary.
/// Walking things rise and fall with the floor; floating things are only
/// pushed down if they would exceed the ceiling. Returns 0 if the thing no
/// longer fits in the vertical gap (caller should crush or block the move).
///
/// # Safety
///
/// `thing` must be a valid, non-null pointer to a live [`mobj_t`]. Called
/// only from [`PIT_ChangeSector`] during sector-height propagation.
#[no_mangle]
pub unsafe extern "C" fn P_ThingHeightClip(thing: *mut mobj_t) -> c_uint {
    let onfloor = (*thing).z == (*thing).floorz;
    P_CheckPosition(thing, (*thing).x, (*thing).y);
    (*thing).floorz = tmfloorz;
    (*thing).ceilingz = tmceilingz;
    if onfloor {
        (*thing).z = (*thing).floorz;
    } else if (*thing).z + (*thing).height > (*thing).ceilingz {
        (*thing).z = (*thing).ceilingz - (*thing).height;
    }
    if (*thing).ceilingz - (*thing).floorz < (*thing).height {
        return 0;
    }
    1
}

// ---------------------------------------------------------------------------
// SLIDE MOVE
// ---------------------------------------------------------------------------

/// Fractional distance along the current path to the first blocking wall.
///
/// Set by [`PTR_SlideTraverse`] and consumed by [`P_SlideMove`].
/// Initialised to `FRACUNIT + 1` (beyond the end of the path) before each
/// path traversal so any real hit is closer.
#[no_mangle]
pub static mut bestslidefrac: fixed_t = 0;

/// Fractional distance to the second-best (backup) blocking wall.
///
/// Stored alongside [`secondslideline`] so that if the primary wall cannot
/// be slid along, [`P_SlideMove`] can fall back to the next candidate.
#[no_mangle]
pub static mut secondslidefrac: fixed_t = 0;

/// The linedef closest to the sliding object along its current path.
///
/// Set by [`PTR_SlideTraverse`]; used by [`P_SlideMove`] to project the
/// remaining momentum along the wall via [`P_HitSlideLine`].
#[no_mangle]
pub static mut bestslideline: *mut line_t = ptr::null_mut();

/// Backup linedef for the second-closest blocking wall found during sliding.
#[no_mangle]
pub static mut secondslideline: *mut line_t = ptr::null_mut();

/// The map object currently performing a slide move.
///
/// Set by [`P_SlideMove`] and read by [`PTR_SlideTraverse`] and
/// [`P_HitSlideLine`].
#[no_mangle]
pub static mut slidemo: *mut mobj_t = ptr::null_mut();

/// Remaining x-component of momentum after clipping to a slide wall.
///
/// Written by [`P_HitSlideLine`] and applied by [`P_SlideMove`].
#[no_mangle]
pub static mut tmxmove: fixed_t = 0;

/// Remaining y-component of momentum after clipping to a slide wall.
///
/// Written by [`P_HitSlideLine`] and applied by [`P_SlideMove`].
#[no_mangle]
pub static mut tmymove: fixed_t = 0;

/// Project the current `(tmxmove, tmymove)` velocity onto linedef `ld`.
///
/// Computes the component of the velocity vector that runs parallel to the
/// wall so that the next [`P_TryMove`] call will slide rather than stop.
/// Handles horizontal and vertical walls as fast special cases; uses
/// trigonometry for diagonal walls.
///
/// # Safety
///
/// `ld` must be a valid, non-null pointer to an initialised [`line_t`].
/// [`slidemo`], [`tmxmove`], and [`tmymove`] must be set by the caller
/// ([`P_SlideMove`]) before this function is called.
#[no_mangle]
pub unsafe extern "C" fn P_HitSlideLine(ld: *mut line_t) {
    let ld = &*ld;
    if ld.slopetype == ST_HORIZONTAL {
        tmymove = 0;
        return;
    }
    if ld.slopetype == ST_VERTICAL {
        tmxmove = 0;
        return;
    }
    let side = P_PointOnLineSide((*slidemo).x, (*slidemo).y, ld as *const _ as *mut _);
    let mut lineangle = R_PointToAngle2(0, 0, ld.dx, ld.dy);
    if side == 1 {
        lineangle = lineangle.wrapping_add(ANG180);
    }
    let moveangle = R_PointToAngle2(0, 0, tmxmove, tmymove);
    let mut deltaangle = moveangle.wrapping_sub(lineangle);
    if deltaangle > ANG180 {
        deltaangle = deltaangle.wrapping_add(ANG180);
    }
    let lineangle = (lineangle >> ANGLETOFINESHIFT) as usize;
    let deltaangle = (deltaangle >> ANGLETOFINESHIFT) as usize;
    let movelen = P_AproxDistance(tmxmove, tmymove);
    let newlen = FixedMul(movelen, *finecosine.0.add(deltaangle));
    tmxmove = FixedMul(newlen, *finecosine.0.add(lineangle));
    tmymove = FixedMul(newlen, finesine[lineangle]);
}

/// Path-traversal callback that finds blocking walls for slide movement.
///
/// Called by [`P_PathTraverse`] from [`P_SlideMove`]. If the intercept is
/// not a line, the engine aborts with an error. For each blocking line the
/// fractional intercept is compared with [`bestslidefrac`]; closer hits
/// displace the current best and push the old best to the second slot.
///
/// Returns 1 to continue traversal (line is passable), 0 to stop.
///
/// # Safety
///
/// `in_` must be a valid, non-null pointer to an initialised
/// [`intercept_t`]. [`slidemo`] must point to the object being moved.
#[no_mangle]
pub unsafe extern "C" fn PTR_SlideTraverse(in_: *mut intercept_t) -> c_uint {
    let in_ = &*in_;
    if in_.isaline == 0 {
        i_error!("PTR_SlideTraverse: not a line?");
    }
    let li = in_.d.line;
    if (*li).flags as c_int & LinedefFlag::TWOSIDED as c_int == 0 {
        if P_PointOnLineSide((*slidemo).x, (*slidemo).y, li) != 0 {
            return 1;
        }
    } else {
        P_LineOpening(li);
        if openrange >= (*slidemo).height
            && opentop - (*slidemo).z >= (*slidemo).height
            && openbottom - (*slidemo).z <= 24 * FRACUNIT
        {
            return 1;
        }
    }
    if in_.frac < bestslidefrac {
        secondslidefrac = bestslidefrac;
        secondslideline = bestslideline;
        bestslidefrac = in_.frac;
        bestslideline = li;
    }
    0
}

/// Move `mo` while sliding along the first wall it would otherwise hit.
///
/// Traces three leading-corner paths with [`PTR_SlideTraverse`] to find
/// the nearest blocking wall, moves flush to it (with a small fudge
/// factor), then projects the remaining momentum along the wall via
/// [`P_HitSlideLine`] and calls [`P_TryMove`] again. Falls back to a
/// "stairstep" (try pure-y then pure-x move) if no wall is found or after
/// three retries to prevent infinite loops.
///
/// # Safety
///
/// `mo` must be a valid, non-null pointer to a live [`mobj_t`]. Map data
/// must be fully initialised.
#[no_mangle]
pub unsafe extern "C" fn P_SlideMove(mo: *mut mobj_t) {
    slidemo = mo;
    let mut hitcount = 0;

    'retry: loop {
        hitcount += 1;
        if hitcount == 3 {
            // stairstep
            if P_TryMove(mo, (*mo).x, (*mo).y + (*mo).momy) == 0 {
                P_TryMove(mo, (*mo).x + (*mo).momx, (*mo).y);
            }
            return;
        }

        let (leadx, trailx) = if (*mo).momx > 0 {
            ((*mo).x + (*mo).radius, (*mo).x - (*mo).radius)
        } else {
            ((*mo).x - (*mo).radius, (*mo).x + (*mo).radius)
        };
        let (leady, traily) = if (*mo).momy > 0 {
            ((*mo).y + (*mo).radius, (*mo).y - (*mo).radius)
        } else {
            ((*mo).y - (*mo).radius, (*mo).y + (*mo).radius)
        };

        bestslidefrac = FRACUNIT + 1;

        P_PathTraverse(
            leadx,
            leady,
            leadx + (*mo).momx,
            leady + (*mo).momy,
            PT_ADDLINES,
            Some(PTR_SlideTraverse),
        );
        P_PathTraverse(
            trailx,
            leady,
            trailx + (*mo).momx,
            leady + (*mo).momy,
            PT_ADDLINES,
            Some(PTR_SlideTraverse),
        );
        P_PathTraverse(
            leadx,
            traily,
            leadx + (*mo).momx,
            traily + (*mo).momy,
            PT_ADDLINES,
            Some(PTR_SlideTraverse),
        );

        if bestslidefrac == FRACUNIT + 1 {
            // stairstep
            if P_TryMove(mo, (*mo).x, (*mo).y + (*mo).momy) == 0 {
                P_TryMove(mo, (*mo).x + (*mo).momx, (*mo).y);
            }
            return;
        }

        bestslidefrac -= 0x800;
        if bestslidefrac > 0 {
            let newx = FixedMul((*mo).momx, bestslidefrac);
            let newy = FixedMul((*mo).momy, bestslidefrac);
            if P_TryMove(mo, (*mo).x + newx, (*mo).y + newy) == 0 {
                continue 'retry;
            }
        }

        bestslidefrac = FRACUNIT - (bestslidefrac + 0x800);
        if bestslidefrac > FRACUNIT {
            bestslidefrac = FRACUNIT;
        }
        if bestslidefrac <= 0 {
            return;
        }
        tmxmove = FixedMul((*mo).momx, bestslidefrac);
        tmymove = FixedMul((*mo).momy, bestslidefrac);
        P_HitSlideLine(bestslideline);
        (*mo).momx = tmxmove;
        (*mo).momy = tmymove;
        if P_TryMove(mo, (*mo).x + tmxmove, (*mo).y + tmymove) == 0 {
            continue 'retry;
        }
        return;
    }
}

// ---------------------------------------------------------------------------
// LINE ATTACK
// ---------------------------------------------------------------------------

/// The map object struck by the most recent hitscan or auto-aim traversal.
///
/// Set to `null` before each [`P_AimLineAttack`] / [`P_LineAttack`] call
/// and written by [`PTR_AimTraverse`] or [`PTR_ShootTraverse`] when a
/// target is hit. Callers check this to determine whether anything was
/// actually hit.
#[no_mangle]
pub static mut linetarget: *mut mobj_t = ptr::null_mut();

/// The map object that fired the current hitscan attack.
///
/// Set by [`P_AimLineAttack`] and [`P_LineAttack`]; read by
/// [`PTR_AimTraverse`] and [`PTR_ShootTraverse`] to avoid self-hits.
#[no_mangle]
pub static mut shootthing: *mut mobj_t = ptr::null_mut();

/// Z-height from which the current hitscan ray originates.
///
/// Set to the shooter's mid-height plus 8 map units by both
/// [`P_AimLineAttack`] and [`P_LineAttack`].
#[no_mangle]
pub static mut shootz: fixed_t = 0;

/// Damage dealt by the current hitscan attack (0 for a pure aim/test trace).
#[no_mangle]
pub static mut la_damage: c_int = 0;

/// Maximum reach of the current hitscan ray, in fixed-point map units.
///
/// Used together with an intercept's fractional distance to compute the
/// actual world distance to a hit.
#[no_mangle]
pub static mut attackrange: fixed_t = 0;

/// Vertical slope (rise/run) of the winning auto-aim result.
///
/// Written by [`PTR_AimTraverse`] when a valid target is found, then read
/// by [`P_AimLineAttack`] (return value) and passed into [`P_LineAttack`]
/// as the actual firing slope.
#[no_mangle]
pub static mut aimslope: fixed_t = 0;

/// Path-traversal callback for [`P_AimLineAttack`] auto-aim.
///
/// For each line intercept, narrows the vertical aim window (`topslope` /
/// `bottomslope`) based on the opening. For each thing intercept, checks
/// whether the thing falls within the aim window and, if so, records it in
/// [`linetarget`] and computes [`aimslope`].
///
/// Returns 1 to continue traversal, 0 to stop (target locked or window
/// closed).
///
/// # Safety
///
/// `in_` must be a valid, non-null pointer to an initialised
/// [`intercept_t`]. [`shootthing`], [`shootz`], [`attackrange`],
/// `topslope`, and `bottomslope` must be set before the traversal begins.
#[no_mangle]
pub unsafe extern "C" fn PTR_AimTraverse(in_: *mut intercept_t) -> c_uint {
    let in_ = &*in_;
    if in_.isaline != 0 {
        let li = in_.d.line;
        if (*li).flags as c_int & LinedefFlag::TWOSIDED as c_int == 0 {
            return 0;
        }
        P_LineOpening(li);
        if openbottom >= opentop {
            return 0;
        }
        let dist = FixedMul(attackrange, in_.frac);
        let front = (*li).frontsector as *mut sector_t;
        let back = (*li).backsector as *mut sector_t;
        if back.is_null() || (*front).floorheight != (*back).floorheight {
            let slope = FixedDiv(openbottom - shootz, dist);
            if slope > bottomslope {
                bottomslope = slope;
            }
        }
        if back.is_null() || (*front).ceilingheight != (*back).ceilingheight {
            let slope = FixedDiv(opentop - shootz, dist);
            if slope < topslope {
                topslope = slope;
            }
        }
        if topslope <= bottomslope {
            return 0;
        }
        return 1;
    }

    let th = in_.d.thing as *mut mobj_t;
    if th == shootthing {
        return 1;
    }
    if (*th).flags & MF_SHOOTABLE == 0 {
        return 1;
    }
    let dist = FixedMul(attackrange, in_.frac);
    let thingtopslope = FixedDiv((*th).z + (*th).height - shootz, dist);
    if thingtopslope < bottomslope {
        return 1;
    }
    let thingbottomslope = FixedDiv((*th).z - shootz, dist);
    if thingbottomslope > topslope {
        return 1;
    }
    let mut thingtopslope = thingtopslope;
    let mut thingbottomslope = thingbottomslope;
    if thingtopslope > topslope {
        thingtopslope = topslope;
    }
    if thingbottomslope < bottomslope {
        thingbottomslope = bottomslope;
    }
    aimslope = (thingtopslope + thingbottomslope) / 2;
    linetarget = th;
    0
}

/// Path-traversal callback for [`P_LineAttack`] hitscan shooting.
///
/// For line intercepts, activates any special on the line, then checks
/// whether the shot passes through the opening or hits the wall; if it
/// hits, `goto_hitline` spawns a bullet puff and the traversal stops.
/// For thing intercepts, checks z-overlap with [`aimslope`], spawns puff
/// or blood, and calls [`P_DamageMobj`] if [`la_damage`] is non-zero.
///
/// Returns 1 to continue traversal, 0 to stop (shot consumed).
///
/// # Safety
///
/// `in_` must be a valid, non-null pointer to an initialised
/// [`intercept_t`]. [`shootthing`], [`shootz`], [`attackrange`],
/// [`aimslope`], and [`la_damage`] must be set before the traversal.
#[no_mangle]
pub unsafe extern "C" fn PTR_ShootTraverse(in_: *mut intercept_t) -> c_uint {
    let in_ = &*in_;
    if in_.isaline != 0 {
        let li = in_.d.line;
        if (*li).special != 0 {
            P_ShootSpecialLine(shootthing, li);
        }
        if (*li).flags as c_int & LinedefFlag::TWOSIDED as c_int != 0 {
            P_LineOpening(li);
            let dist = FixedMul(attackrange, in_.frac);
            let back = (*li).backsector as *mut sector_t;
            if back.is_null() {
                let slope = FixedDiv(openbottom - shootz, dist);
                if slope > aimslope {
                    goto_hitline(li, in_);
                    return 0;
                }
                let slope = FixedDiv(opentop - shootz, dist);
                if slope < aimslope {
                    goto_hitline(li, in_);
                    return 0;
                }
            } else {
                let front = (*li).frontsector as *mut sector_t;
                if (*front).floorheight != (*back).floorheight {
                    let slope = FixedDiv(openbottom - shootz, dist);
                    if slope > aimslope {
                        goto_hitline(li, in_);
                        return 0;
                    }
                }
                if (*front).ceilingheight != (*back).ceilingheight {
                    let slope = FixedDiv(opentop - shootz, dist);
                    if slope < aimslope {
                        goto_hitline(li, in_);
                        return 0;
                    }
                }
            }
            return 1;
        }
        goto_hitline(li, in_);
        return 0;
    }

    let th = in_.d.thing as *mut mobj_t;
    if th == shootthing {
        return 1;
    }
    if (*th).flags & MF_SHOOTABLE == 0 {
        return 1;
    }
    let dist = FixedMul(attackrange, in_.frac);
    let thingtopslope = FixedDiv((*th).z + (*th).height - shootz, dist);
    if thingtopslope < aimslope {
        return 1;
    }
    let thingbottomslope = FixedDiv((*th).z - shootz, dist);
    if thingbottomslope > aimslope {
        return 1;
    }

    let frac = in_.frac - FixedDiv(10 * FRACUNIT, attackrange);
    let x = crate::doom::p_maputl::trace.x + FixedMul(crate::doom::p_maputl::trace.dx, frac);
    let y = crate::doom::p_maputl::trace.y + FixedMul(crate::doom::p_maputl::trace.dy, frac);
    let z = shootz + FixedMul(aimslope, FixedMul(frac, attackrange));
    if (*th).flags & MF_NOBLOOD != 0 {
        P_SpawnPuff(x, y, z);
    } else {
        P_SpawnBlood(x, y, z, la_damage);
    }
    if la_damage != 0 {
        P_DamageMobj(
            th as *mut TeleptMobj,
            shootthing as *mut TeleptMobj,
            shootthing as *mut TeleptMobj,
            la_damage,
        );
    }
    0
}

/// Spawn a bullet puff at the point where a hitscan shot struck linedef `li`.
///
/// Computes the impact position slightly in front of the intercept (to avoid
/// z-fighting), skips spawning if the shot hit sky on the front sector or a
/// sky-hack wall on the back sector.
///
/// # Safety
///
/// `li` must be a valid, non-null pointer to an initialised [`line_t`].
/// `in_` must refer to the intercept that triggered this call.
/// [`shootz`], [`aimslope`], and [`attackrange`] must be current.
unsafe fn goto_hitline(li: *mut line_t, in_: &intercept_t) {
    let frac = in_.frac - FixedDiv(4 * FRACUNIT, attackrange);
    let x = crate::doom::p_maputl::trace.x + FixedMul(crate::doom::p_maputl::trace.dx, frac);
    let y = crate::doom::p_maputl::trace.y + FixedMul(crate::doom::p_maputl::trace.dy, frac);
    let z = shootz + FixedMul(aimslope, FixedMul(frac, attackrange));
    let front = (*li).frontsector as *mut sector_t;
    if (*front).ceilingpic as c_int == skyflatnum {
        if z > (*front).ceilingheight {
            return;
        }
        let back = (*li).backsector as *mut sector_t;
        if !back.is_null() && (*back).ceilingpic as c_int == skyflatnum {
            return;
        }
    }
    P_SpawnPuff(x, y, z);
}

/// Auto-aim a hitscan ray from `t1` along `angle` up to `distance` away.
///
/// Runs [`PTR_AimTraverse`] to find the best shootable target within the
/// vertical aim window (approximately ±35 degrees). Returns the vertical
/// slope to the centre of that target, or 0 if nothing was found.
/// Sets [`linetarget`] as a side effect.
///
/// # Safety
///
/// `t1` must be a valid pointer to a live [`mobj_t`] (null is substituted
/// via [`P_SubstNullMobj`] before use). Map data must be initialised.
#[no_mangle]
pub unsafe extern "C" fn P_AimLineAttack(
    t1: *mut mobj_t,
    angle: c_uint,
    distance: fixed_t,
) -> fixed_t {
    let t1 = P_SubstNullMobj(t1 as *mut TeleptMobj) as *mut mobj_t;
    let angle = (angle >> ANGLETOFINESHIFT) as usize;
    shootthing = t1;
    let x2 = (*t1).x + ((distance >> FRACBITS) as c_int) * *finecosine.0.add(angle);
    let y2 = (*t1).y + ((distance >> FRACBITS) as c_int) * finesine[angle];
    shootz = (*t1).z + ((*t1).height >> 1) + 8 * FRACUNIT;
    topslope = 100 * FRACUNIT / 160;
    bottomslope = -(100 * FRACUNIT / 160);
    attackrange = distance;
    linetarget = ptr::null_mut();
    P_PathTraverse(
        (*t1).x,
        (*t1).y,
        x2,
        y2,
        PT_ADDLINES | PT_ADDTHINGS,
        Some(PTR_AimTraverse),
    );
    if !linetarget.is_null() {
        return aimslope;
    }
    0
}

/// Fire a hitscan attack from `t1` with the given `angle`, `distance`,
/// `slope`, and `damage`.
///
/// Runs [`PTR_ShootTraverse`] along the ray. If `damage` is 0 the call is
/// a pure trace that sets [`linetarget`] without dealing damage. Spawns
/// puffs or blood at the impact point as a side effect.
///
/// # Safety
///
/// `t1` must be a valid, non-null pointer to a live [`mobj_t`]. Map data
/// must be fully initialised.
#[no_mangle]
pub unsafe extern "C" fn P_LineAttack(
    t1: *mut mobj_t,
    angle: c_uint,
    distance: fixed_t,
    slope: fixed_t,
    damage: c_int,
) {
    let angle = (angle >> ANGLETOFINESHIFT) as usize;
    shootthing = t1;
    la_damage = damage;
    let x2 = (*t1).x + ((distance >> FRACBITS) as c_int) * *finecosine.0.add(angle);
    let y2 = (*t1).y + ((distance >> FRACBITS) as c_int) * finesine[angle];
    shootz = (*t1).z + ((*t1).height >> 1) + 8 * FRACUNIT;
    attackrange = distance;
    aimslope = slope;
    P_PathTraverse(
        (*t1).x,
        (*t1).y,
        x2,
        y2,
        PT_ADDLINES | PT_ADDTHINGS,
        Some(PTR_ShootTraverse),
    );
}

// ---------------------------------------------------------------------------
// USE LINES
// ---------------------------------------------------------------------------

/// The map object currently attempting a Use action.
///
/// Set by [`P_UseLines`] before calling [`P_PathTraverse`]; read by
/// [`PTR_UseTraverse`] to determine the initiator's position and side.
#[no_mangle]
pub static mut usething: *mut mobj_t = ptr::null_mut();

/// Path-traversal callback for the player's Use action.
///
/// For non-special lines with a closed opening, plays the "oof" sound and
/// stops traversal. For special lines, determines which side the player is
/// on and calls [`P_UseSpecialLine`], then stops (only one special per Use
/// press). Passable non-special lines allow traversal to continue.
///
/// Returns 1 to continue traversal, 0 to stop.
///
/// # Safety
///
/// `in_` must be a valid, non-null pointer to an initialised
/// [`intercept_t`] whose `d.line` is valid. [`usething`] must be set.
#[no_mangle]
pub unsafe extern "C" fn PTR_UseTraverse(in_: *mut intercept_t) -> c_uint {
    let in_ = &*in_;
    if (*in_.d.line).special == 0 {
        P_LineOpening(in_.d.line);
        if openrange <= 0 {
            S_StartSound(usething as *mut c_void, Sfx::Noway as c_int);
            return 0;
        }
        return 1;
    }
    let mut side = 0;
    if P_PointOnLineSide((*usething).x, (*usething).y, in_.d.line) == 1 {
        side = 1;
    }
    P_UseSpecialLine(
        usething as *mut c_void,
        in_.d.line as *mut crate::doom::p_lights::line_t,
        side,
    );
    0
}

/// Activate special linedefs in front of `player` along their view direction.
///
/// Traces a ray of length `USERANGE` (64 map units) from the player's position using
/// [`PTR_UseTraverse`]. The first special line within reach and in the
/// correct orientation is activated.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to a [`crate::doom::d_player::PlayerT`]
/// whose `mo` field points to a live [`mobj_t`]. Map data must be
/// initialised.
#[no_mangle]
pub unsafe extern "C" fn P_UseLines(player: *mut c_void) {
    let player = player as *mut crate::doom::d_player::PlayerT;
    let mo = (*player).mo as *mut mobj_t;
    usething = mo;
    let angle = ((*mo).angle >> ANGLETOFINESHIFT) as usize;
    let x1 = (*mo).x;
    let y1 = (*mo).y;
    let x2 = x1 + ((USERANGE >> FRACBITS) as c_int) * *finecosine.0.add(angle);
    let y2 = y1 + ((USERANGE >> FRACBITS) as c_int) * finesine[angle];
    P_PathTraverse(x1, y1, x2, y2, PT_ADDLINES, Some(PTR_UseTraverse));
}

// ---------------------------------------------------------------------------
// RADIUS ATTACK
// ---------------------------------------------------------------------------

/// The creature that triggered the explosion (may differ from [`bombspot`]).
///
/// Used as the `inflictor` when calling [`P_DamageMobj`] so that frag
/// credit goes to the right entity (e.g. the player who fired the rocket,
/// not the explosion object itself).
#[no_mangle]
pub static mut bombsource: *mut mobj_t = ptr::null_mut();

/// The map object at the centre of the current explosion.
///
/// Used by [`PIT_RadiusAttack`] to measure distance and check line-of-sight.
#[no_mangle]
pub static mut bombspot: *mut mobj_t = ptr::null_mut();

/// Maximum damage (and effective radius in map units) of the current explosion.
///
/// Damage falls off linearly with Chebyshev distance from [`bombspot`]:
/// `damage = bombdamage - dist`.
#[no_mangle]
pub static mut bombdamage: c_int = 0;

/// Blockmap thing iterator callback for [`P_RadiusAttack`].
///
/// Skips non-shootable things and the two boss types immune to splash
/// (Cyberdemon and Spider Mastermind). Computes Chebyshev distance from
/// [`bombspot`], subtracts the thing's radius, and if in range and in
/// line-of-sight, deals `bombdamage - dist` damage.
///
/// Always returns 1 (continues blockmap iteration).
///
/// # Safety
///
/// `thing` must be a valid, non-null pointer to an initialised [`mobj_t`].
/// [`bombspot`], [`bombsource`], and [`bombdamage`] must be set by
/// [`P_RadiusAttack`] before this callback is invoked.
#[no_mangle]
pub unsafe extern "C" fn PIT_RadiusAttack(thing: *mut mobj_t) -> c_uint {
    let thing = &*thing;
    if thing.flags & MF_SHOOTABLE == 0 {
        return 1;
    }
    if thing.type_ == MT_CYBORG || thing.type_ == MT_SPIDER {
        return 1;
    }
    let dx = (thing.x - (*bombspot).x).wrapping_abs();
    let dy = (thing.y - (*bombspot).y).wrapping_abs();
    let mut dist = if dx > dy { dx } else { dy };
    dist = (dist - thing.radius) >> FRACBITS;
    if dist < 0 {
        dist = 0;
    }
    if dist >= bombdamage {
        return 1;
    }
    let pt_mobj_t = thing as *const _ as *mut crate::doom::p_telept::mobj_t;
    let pt_bombspot = bombspot as *mut crate::doom::p_telept::mobj_t;
    if P_CheckSight(pt_mobj_t, pt_bombspot) != 0 {
        P_DamageMobj(
            thing as *const _ as *mut TeleptMobj,
            bombspot as *mut TeleptMobj,
            bombsource as *mut TeleptMobj,
            bombdamage - dist,
        );
    }
    1
}

/// Apply splash damage from an explosion at `spot` to all nearby things.
///
/// Iterates over all blockmap cells within `damage + MAXRADIUS` of `spot`
/// and calls [`PIT_RadiusAttack`] for each thing found. The Cyberdemon and
/// Spider Mastermind are immune; all other shootable things in line-of-sight
/// take linearly decreasing damage.
///
/// `source` is the creature credited with the kill (may differ from `spot`).
///
/// # Safety
///
/// `spot` and `source` must be valid, non-null pointers to live [`mobj_t`]s.
/// Map and blockmap data must be fully initialised.
#[no_mangle]
pub unsafe extern "C" fn P_RadiusAttack(spot: *mut mobj_t, source: *mut mobj_t, damage: c_int) {
    let dist = (damage + MAXRADIUS) << FRACBITS;
    let yh = ((*spot).y + dist - bmaporgy) >> MAPBLOCKSHIFT;
    let yl = ((*spot).y - dist - bmaporgy) >> MAPBLOCKSHIFT;
    let xh = ((*spot).x + dist - bmaporgx) >> MAPBLOCKSHIFT;
    let xl = ((*spot).x - dist - bmaporgx) >> MAPBLOCKSHIFT;
    bombspot = spot;
    bombsource = source;
    bombdamage = damage;
    for y in yl..=yh {
        for x in xl..=xh {
            P_BlockThingsIterator(x, y, Some(PIT_RadiusAttack));
        }
    }
}

// ---------------------------------------------------------------------------
// SECTOR HEIGHT CHANGING
// ---------------------------------------------------------------------------

/// Non-zero when things that do not fit during a sector-height change should
/// take crush damage (10 HP every 4 tics).
///
/// Set by [`P_ChangeSector`] from the `crunch` parameter.
#[no_mangle]
pub static mut crushchange: c_int = 0; // boolean

/// Set to non-zero by [`PIT_ChangeSector`] if any thing no longer fits
/// after a sector-height change.
///
/// Returned by [`P_ChangeSector`]; a truthy value tells the caller to
/// either continue crushing or revert the sector height.
#[no_mangle]
pub static mut nofit: c_int = 0; // boolean

/// Blockmap thing iterator callback for [`P_ChangeSector`].
///
/// Calls [`P_ThingHeightClip`] on each thing. If the thing fits, returns 1.
/// If it does not fit: dead things are gibbed, dropped items are removed,
/// non-shootable things are ignored (assumed decorative), and live shootable
/// things set [`nofit`] and optionally receive crush damage with a blood
/// spray every 4 tics.
///
/// Always returns 1 to continue checking other things.
///
/// # Safety
///
/// `thing` must be a valid, non-null pointer to an initialised [`mobj_t`].
/// [`crushchange`] must be set by [`P_ChangeSector`] before iteration.
#[no_mangle]
pub unsafe extern "C" fn PIT_ChangeSector(thing: *mut mobj_t) -> c_uint {
    if P_ThingHeightClip(thing) != 0 {
        return 1;
    }
    let thing = &mut *thing;
    if thing.health <= 0 {
        P_SetMobjState(thing as *const _ as *mut TeleptMobj, S_GIBS);
        thing.flags &= !MF_SOLID;
        thing.height = 0;
        thing.radius = 0;
        return 1;
    }
    if thing.flags & MF_DROPPED != 0 {
        P_RemoveMobj(thing as *const _ as *mut TeleptMobj);
        return 1;
    }
    if thing.flags & MF_SHOOTABLE == 0 {
        return 1;
    }
    nofit = 1;
    if crushchange != 0 && leveltime & 3 == 0 {
        P_DamageMobj(
            thing as *const _ as *mut TeleptMobj,
            ptr::null_mut(),
            ptr::null_mut(),
            10,
        );
        let mo = P_SpawnMobj(thing.x, thing.y, thing.z + thing.height / 2, MT_BLOOD);
        (*mo).momx = (P_Random() - P_Random()) << 12;
        (*mo).momy = (P_Random() - P_Random()) << 12;
    }
    1
}

/// Propagate a floor or ceiling height change in `sector` to all nearby things.
///
/// Re-checks height constraints for every thing in the blockmap cells that
/// overlap `sector->blockbox`. Returns non-zero if any thing no longer fits
/// (i.e. [`nofit`] was set). If `crunch` is zero and this returns non-zero,
/// the caller should revert the sector height and call this again.
///
/// # Safety
///
/// `sector` must be a valid, non-null pointer to an initialised [`sector_t`]
/// whose `blockbox` indices are within the blockmap bounds.
#[no_mangle]
pub unsafe extern "C" fn P_ChangeSector(sector: *mut sector_t, crunch: c_int) -> c_int {
    nofit = 0;
    crushchange = crunch;
    let sec = &*sector;
    for x in sec.blockbox[BBox::LEFT]..=sec.blockbox[BBox::RIGHT] {
        for y in sec.blockbox[BBox::BOTTOM]..=sec.blockbox[BBox::TOP] {
            P_BlockThingsIterator(x, y, Some(PIT_ChangeSector));
        }
    }
    nofit
}

// ---------------------------------------------------------------------------
// Spechit overrun emulation
// ---------------------------------------------------------------------------

/// Emulate the vanilla Doom memory-corruption behaviour when more than
/// [`MAXSPECIALCROSS_ORIGINAL`] special lines are crossed in a single move.
///
/// In the original `doom2.exe`, `spechit` was a fixed C array on the stack
/// adjacent to `tmbbox`, `crushchange`, and `nofit`. Writing past the end
/// overwrote those variables with computed addresses. This function
/// replicates those overwrites so that demos recorded with vanilla Doom
/// (which relied on the corrupted values) remain sync-compatible.
///
/// The base address defaults to `DEFAULT_SPECHIT_MAGIC` (PrBoom-plus
/// compatible) but can be overridden with the `-spechit <n>` command-line
/// argument.
///
/// # Safety
///
/// `ld` must be a valid, non-null pointer to a [`line_t`] that is part of
/// the global `lines` array so that `ld.offset_from(lines)` is well-defined.
/// Must only be called from [`PIT_CheckLine`] after `numspechit` has been
/// incremented beyond [`MAXSPECIALCROSS_ORIGINAL`].
unsafe fn SpechitOverrun(ld: *mut line_t) {
    static mut baseaddr: c_uint = 0;
    if baseaddr == 0 {
        let p = M_CheckParmWithArgs(c"-spechit".as_ptr().cast_mut(), 1);
        if p > 0 {
            M_StrToInt(
                *myargv.add((p + 1) as usize),
                &raw mut baseaddr as *mut c_int,
            );
        } else {
            baseaddr = DEFAULT_SPECHIT_MAGIC;
        }
    }
    let addr = baseaddr as c_int + (ld.offset_from(lines) * 0x3e) as c_int;
    match numspechit {
        9..=12 => {
            tmbbox[(numspechit - 9) as usize] = addr;
        }
        13 => {
            crushchange = addr;
        }
        14 => {
            nofit = addr;
        }
        _ => {
            eprintln!(
                "SpechitOverrun: Warning: unable to emulate an overrun where numspechit={}",
                numspechit as c_int
            );
        }
    }
}
