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

use crate::{
    engine::{
        doom::player::PlayerId,
        map::{
            LineId, SectorId,
            slopetype_t::{ST_HORIZONTAL, ST_VERTICAL},
        },
        math::{
            bbox::BBox,
            fixed::{FRACBITS, FRACUNIT, FixedDiv, FixedMul, fixed_t},
            tables::{ANG180, ANGLETOFINESHIFT, finecosine, finesine},
        },
        misc::random::P_Random,
        play::{
            maputl::{
                InterceptHit, MAPBLOCKSHIFT, P_AproxDistance, P_BlockLinesIterator,
                P_BlockThingsIterator, P_BoxOnLineSide, P_LineOpening, P_PathTraverse,
                P_PointOnLineSide, P_SetThingPosition, P_UnsetThingPosition, PT_ADDLINES,
                PT_ADDTHINGS, intercept_t, lowfloor, openbottom, openrange, opentop, trace,
            },
            mobj::{MobjId, mobj_or_null},
            setup::{LEVEL, blockmap_origin},
            sight::{P_CheckSight, SIGHT},
        },
        render_3d::main::{R_PointInSubsector, R_PointToAngle2, VALIDCOUNT},
        wad::format::LinedefFlag,
        world::World,
    },
    i_error,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of special lines that can be crossed during a single move.
///
/// The Rust port uses 20 slots (matching `MAXSPECIALCROSS` in the C source)
/// while the original vanilla limit was 8 (`MAXSPECIALCROSS_ORIGINAL`).
const MAXSPECIALCROSS: usize = 20;

/// Sentinel magic value used to detect the vanilla spechit overflow bug
/// (DEFAULT_SPECHIT_MAGIC in p_map.c = 0x01C09C98).
pub const DEFAULT_SPECHIT_MAGIC: u32 = 0x01C09C98;

/// Original vanilla Doom limit for special lines crossed per move.
///
/// Crossings beyond this count trigger [`SpechitOverrun`] to emulate the
/// vanilla memory-corruption behaviour that some demos depend on.
const MAXSPECIALCROSS_ORIGINAL: i32 = 8;

/// Maximum radius of any map object, in map units (fixed-point).
///
/// Used to expand blockmap queries so that objects whose origin lies in an
/// adjacent block but overlaps the query region are still found.
const MAXRADIUS: i32 = 32 * FRACUNIT;

/// Forward reach of the player's Use action, in map units (fixed-point).
const USERANGE: i32 = 64 * FRACUNIT;

/// Map-object flag: the thing is a special pickup item.
const MF_SPECIAL: i32 = 1;
/// Map-object flag: the thing blocks movement (solid).
const MF_SOLID: i32 = 2;
/// Map-object flag: the thing can be damaged by hitscan or projectile attacks.
const MF_SHOOTABLE: i32 = 4;
/// Map-object flag: the thing is not added to the sector thing list.
const MF_NOSECTOR: i32 = 8;
/// Map-object flag: the thing is not added to the blockmap.
const MF_NOBLOCKMAP: i32 = 16;
/// Map-object flag: the thing is a projectile (missile).
const MF_MISSILE: i32 = 65536;
/// Map-object flag: the thing can walk off ledges.
const MF_DROPOFF: i32 = 1024;
/// Map-object flag: the thing ignores clipping (no-clip cheat).
const MF_NOCLIP: i32 = 4096;
/// Map-object flag: the thing can fly vertically (floats in air).
const MF_FLOAT: i32 = 16384;
/// Map-object flag: the thing is mid-teleport and bypasses step/dropoff checks.
const MF_TELEPORT: i32 = 32768;
/// Map-object flag: a Lost Soul currently in charge-flight mode.
const MF_SKULLFLY: i32 = 16777216;
/// Map-object flag: the thing picks up items on contact.
const MF_PICKUP: i32 = 2048;
/// Map-object flag: the thing does not bleed when damaged (spawns puff instead).
const MF_NOBLOOD: i32 = 524288;
/// Map-object flag: the thing was dropped by a monster and should be removed when crushed.
const MF_DROPPED: i32 = 131072;

/// Thing-type index for the player.
const MT_PLAYER: i32 = 0;
/// Thing-type index for the Hell Knight.
const MT_KNIGHT: i32 = 17;
/// Thing-type index for the Baron of Hell.
const MT_BRUISER: i32 = 15;
/// Thing-type index for the Cyberdemon (immune to splash damage).
const MT_CYBORG: i32 = 21;
/// Thing-type index for the Spider Mastermind (immune to splash damage).
const MT_SPIDER: i32 = 19;
/// Thing-type index for the blood splat particle spawned during crushing.
const MT_BLOOD: i32 = 38;

/// State index for the "gibs" sprite, used when corpses are crushed.
const S_GIBS: i32 = 895;

/// Default value for the DEH species-infighting flag (disabled).
///
/// When 0, monsters of the same species cannot hurt each other with projectiles.
/// A DeHackEd patch can override this.
const DEH_DEFAULT_SPECIES_INFIGHTING: i32 = 0;

// ---------------------------------------------------------------------------
// Extern functions from other modules
// ---------------------------------------------------------------------------

use crate::engine::{
    game::gamemap,
    global::Global,
    misc::{
        M_StrToInt,
        argv::{M_CheckParmWithArgs, argument},
    },
    play::{
        inter::{P_DamageMobj, P_TouchSpecialThing},
        mobj::{
            P_RemoveMobj, P_SetMobjState, P_SpawnBlood, P_SpawnMobj, P_SpawnPuff, P_SubstNullMobj,
        },
        spec::{P_CrossSpecialLine, P_ShootSpecialLine},
        switch::P_UseSpecialLine,
        tick::leveltime,
    },
    render_3d::sky::SKY,
    sound::{S_StartSound, info::Sfx},
};

// ---------------------------------------------------------------------------
// Movement scratchpad state
// ---------------------------------------------------------------------------

/// Movement, slide, hitscan, use, splash and crush scratchpad (`p_map.c`
/// globals).
///
/// The blockmap and path-traversal callbacks below re-enter play code:
/// damage runs death actions that move and spawn other objects, which check
/// positions and traverse paths again, overwriting this state exactly as
/// they overwrote the C globals. The C code reads each global afresh after
/// such a call, so every access here is a short borrow (or one of the
/// accessor functions) that never spans a call into other play code; values
/// are copied out before a call and re-read after it.
pub struct MapState {
    /// Axis-aligned bounding box of the thing currently being position-checked.
    ///
    /// Set by [`P_CheckPosition`] / [`P_TeleportMove`] before iterating over
    /// blockmap cells. Indices follow [`BBox`] (TOP, BOTTOM, LEFT, RIGHT).
    pub tmbbox: [fixed_t; 4],
    /// Pointer to the map object currently being tested by [`P_CheckPosition`].
    ///
    /// Used by the blockmap iterator callbacks ([`PIT_CheckThing`],
    /// [`PIT_CheckLine`]) to access the moving object without passing it through
    /// the C-style function-pointer interface.
    pub tmthing: Option<MobjId>,
    /// Cached copy of `tmthing->flags` for the current position check.
    ///
    /// Avoids repeated pointer dereferences inside tight blockmap loops.
    pub tmflags: i32,
    /// Destination x-coordinate being tested in the current position check.
    pub tmx: fixed_t,
    /// Destination y-coordinate being tested in the current position check.
    pub tmy: fixed_t,
    /// Set to non-zero by [`P_TryMove`] when the gap between floor and ceiling
    /// is large enough for the thing to fit, even if other constraints still
    /// block the move.
    ///
    /// Callers (e.g. floating monsters) read this to decide whether to keep
    /// trying to ascend or descend rather than giving up entirely.
    pub floatok: i32, // boolean
    /// Highest floor height touched during the current position check.
    ///
    /// Updated by [`PIT_CheckLine`] as two-sided linedefs are crossed.
    /// After [`P_CheckPosition`] returns, this is the floor the thing would
    /// stand on at the tested position.
    pub tmfloorz: fixed_t,
    /// Lowest ceiling height encountered during the current position check.
    ///
    /// Updated by [`PIT_CheckLine`]. After [`P_CheckPosition`] returns, this
    /// is the ceiling height at the tested position.
    pub tmceilingz: fixed_t,
    /// Lowest floor height seen across all contacted sectors during the check.
    ///
    /// Monsters will not move to a position where `tmfloorz - tmdropoffz`
    /// exceeds 24 map units unless they have `MF_DROPOFF` or `MF_FLOAT`.
    pub tmdropoffz: fixed_t,
    /// The linedef that produced the current value of `tmceilingz`, or
    /// `None` when the ceiling came from the sector at the tested spot.
    ///
    /// Missiles use this to avoid exploding against "sky hack" walls: if the
    /// ceiling line's front sector has the sky flat, the missile silently
    /// disappears instead of spawning a puff.
    pub ceilingline: Option<LineId>,
    /// Array of special linedefs crossed during the current move attempt;
    /// the first `numspechit` entries are valid.
    ///
    /// Filled by [`PIT_CheckLine`]; processed by [`P_TryMove`] once the move
    /// is confirmed valid. Kept separate so specials are not triggered for
    /// moves that ultimately fail.
    pub spechit: [LineId; MAXSPECIALCROSS],
    /// Number of valid entries currently in `spechit`.
    pub numspechit: i32,

    /// Fractional distance along the current path to the first blocking wall.
    ///
    /// Set by [`PTR_SlideTraverse`] and consumed by [`P_SlideMove`].
    /// Initialised to `FRACUNIT + 1` (beyond the end of the path) before each
    /// path traversal so any real hit is closer.
    pub bestslidefrac: fixed_t,
    /// Fractional distance to the second-best (backup) blocking wall.
    ///
    /// Stored alongside `secondslideline` so that if the primary wall cannot
    /// be slid along, [`P_SlideMove`] can fall back to the next candidate.
    pub secondslidefrac: fixed_t,
    /// The linedef closest to the sliding object along its current path.
    ///
    /// Set by [`PTR_SlideTraverse`]; used by [`P_SlideMove`] to project the
    /// remaining momentum along the wall via [`P_HitSlideLine`]. `None` until
    /// the first slide hits a wall.
    pub bestslideline: Option<LineId>,
    /// Backup linedef for the second-closest blocking wall found during sliding.
    pub secondslideline: Option<LineId>,
    /// The map object currently performing a slide move.
    ///
    /// Set by [`P_SlideMove`] and read by [`PTR_SlideTraverse`] and
    /// [`P_HitSlideLine`].
    pub slidemo: Option<MobjId>,
    /// Remaining x-component of momentum after clipping to a slide wall.
    ///
    /// Written by [`P_HitSlideLine`] and applied by [`P_SlideMove`].
    pub tmxmove: fixed_t,
    /// Remaining y-component of momentum after clipping to a slide wall.
    ///
    /// Written by [`P_HitSlideLine`] and applied by [`P_SlideMove`].
    pub tmymove: fixed_t,

    /// The map object struck by the most recent hitscan or auto-aim traversal.
    ///
    /// Set to `None` before each [`P_AimLineAttack`] / [`P_LineAttack`] call
    /// and written by [`PTR_AimTraverse`] or [`PTR_ShootTraverse`] when a
    /// target is hit. Callers check this to determine whether anything was
    /// actually hit.
    pub linetarget: Option<MobjId>,
    /// The map object that fired the current hitscan attack.
    ///
    /// Set by [`P_AimLineAttack`] and [`P_LineAttack`]; read by
    /// [`PTR_AimTraverse`] and [`PTR_ShootTraverse`] to avoid self-hits.
    pub shootthing: Option<MobjId>,
    /// Z-height from which the current hitscan ray originates.
    ///
    /// Set to the shooter's mid-height plus 8 map units by both
    /// [`P_AimLineAttack`] and [`P_LineAttack`].
    pub shootz: fixed_t,
    /// Damage dealt by the current hitscan attack (0 for a pure aim/test trace).
    pub la_damage: i32,
    /// Maximum reach of the current hitscan ray, in fixed-point map units.
    ///
    /// Used together with an intercept's fractional distance to compute the
    /// actual world distance to a hit.
    pub attackrange: fixed_t,
    /// Vertical slope (rise/run) of the winning auto-aim result.
    ///
    /// Written by [`PTR_AimTraverse`] when a valid target is found, then read
    /// by [`P_AimLineAttack`] (return value) and passed into [`P_LineAttack`]
    /// as the actual firing slope.
    pub aimslope: fixed_t,

    /// The map object currently attempting a Use action.
    ///
    /// Set by [`P_UseLines`] before calling [`P_PathTraverse`]; read by
    /// [`PTR_UseTraverse`] to determine the initiator's position and side.
    pub usething: Option<MobjId>,

    /// The creature that triggered the explosion (may differ from `bombspot`).
    ///
    /// Used as the `inflictor` when calling [`P_DamageMobj`] so that frag
    /// credit goes to the right entity (e.g. the player who fired the rocket,
    /// not the explosion object itself).
    pub bombsource: Option<MobjId>,
    /// The map object at the centre of the current explosion.
    ///
    /// Used by [`PIT_RadiusAttack`] to measure distance and check line-of-sight.
    pub bombspot: Option<MobjId>,
    /// Maximum damage (and effective radius in map units) of the current explosion.
    ///
    /// Damage falls off linearly with Chebyshev distance from `bombspot`:
    /// `damage = bombdamage - dist`.
    pub bombdamage: i32,

    /// Non-zero when things that do not fit during a sector-height change should
    /// take crush damage (10 HP every 4 tics).
    ///
    /// Set by [`P_ChangeSector`] from the `crunch` parameter.
    pub crushchange: i32, // boolean
    /// Set to non-zero by [`PIT_ChangeSector`] if any thing no longer fits
    /// after a sector-height change.
    ///
    /// Returned by [`P_ChangeSector`]; a truthy value tells the caller to
    /// either continue crushing or revert the sector height.
    pub nofit: i32, // boolean

    /// Base address used by `SpechitOverrun` (static local `baseaddr` in C),
    /// resolved from `-spechit` on first use.
    spechit_baseaddr: u32,
}

impl MapState {
    /// The state at program start: no move, slide, attack or crush run.
    pub const fn new() -> Self {
        Self {
            tmbbox: [0; 4],
            tmthing: None,
            tmflags: 0,
            tmx: 0,
            tmy: 0,
            floatok: 0,
            tmfloorz: 0,
            tmceilingz: 0,
            tmdropoffz: 0,
            ceilingline: None,
            spechit: [LineId::new(0); MAXSPECIALCROSS],
            numspechit: 0,
            bestslidefrac: 0,
            secondslidefrac: 0,
            bestslideline: None,
            secondslideline: None,
            slidemo: None,
            tmxmove: 0,
            tmymove: 0,
            linetarget: None,
            shootthing: None,
            shootz: 0,
            la_damage: 0,
            attackrange: 0,
            aimslope: 0,
            usething: None,
            bombsource: None,
            bombspot: None,
            bombdamage: 0,
            crushchange: 0,
            nofit: 0,
            spechit_baseaddr: 0,
        }
    }
}

impl Default for MapState {
    fn default() -> Self {
        Self::new()
    }
}

/// Movement, hitscan and crush scratchpad.
pub static MAP: Global<MapState> = Global::new(MapState::new());

/// The object found by the last aim or shot (C `linetarget`).
pub fn linetarget() -> Option<MobjId> {
    MAP.borrow().linetarget
}

/// Whether the last `P_TryMove` found a tall enough gap (C `floatok`).
pub fn floatok() -> i32 {
    MAP.borrow().floatok
}

/// Floor height at the last checked position (C `tmfloorz`).
pub fn tmfloorz() -> fixed_t {
    MAP.borrow().tmfloorz
}

/// The line that set the last checked position's ceiling (C `ceilingline`).
pub fn ceilingline() -> Option<LineId> {
    MAP.borrow().ceilingline
}

/// Reach of the current hitscan attack (C `attackrange`).
pub fn attackrange() -> fixed_t {
    MAP.borrow().attackrange
}

/// The object being position-checked (C `tmthing`).
///
/// # Panics
///
/// If no position check has run yet.
fn tmthing() -> MobjId {
    MAP.borrow().tmthing.expect("no position check running")
}

/// Record `thing` at `(x, y)` as the object being position-checked and set
/// its bounding box (the common prologue of `P_TeleportMove` and
/// `P_CheckPosition`), then start the floor, ceiling and special-line
/// results from the sector at that spot. Returns the bounding box.
///
/// # Safety
///
/// `thing_id` must name a map object of the loaded level.
unsafe fn start_position_check(thing_id: MobjId, x: fixed_t, y: fixed_t) -> [fixed_t; 4] {
    let thing = thing_id.ptr();
    let radius = (*thing).radius;
    {
        let mut map = MAP.borrow_mut();
        map.tmthing = Some(thing_id);
        map.tmflags = (*thing).flags;
        map.tmx = x;
        map.tmy = y;

        map.tmbbox[BBox::TOP] = y + radius;
        map.tmbbox[BBox::BOTTOM] = y - radius;
        map.tmbbox[BBox::RIGHT] = x + radius;
        map.tmbbox[BBox::LEFT] = x - radius;
    }

    let newsubsec = R_PointInSubsector(x, y);
    let (floorheight, ceilingheight) = {
        let level = LEVEL.borrow();
        let sec = &level.sectors[level.subsectors[newsubsec].sector];
        (sec.floorheight, sec.ceilingheight)
    };
    let mut map = MAP.borrow_mut();
    map.ceilingline = None;
    map.tmfloorz = floorheight;
    map.tmdropoffz = map.tmfloorz;
    map.tmceilingz = ceilingheight;

    VALIDCOUNT.set(VALIDCOUNT.get().wrapping_add(1));
    map.numspechit = 0;
    map.tmbbox
}

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
/// `thing_id` must name an initialised map object. `tmthing` and
/// `tmx` / `tmy` must have been set by the caller ([`P_TeleportMove`])
/// before this callback is invoked.
pub unsafe fn PIT_StompThing(world: &mut World, thing_id: MobjId) -> u32 {
    let thing = &*thing_id.ptr();
    if thing.flags & MF_SHOOTABLE == 0 {
        return 1;
    }
    let (tmthing_id, tmx, tmy) = {
        let map = MAP.borrow();
        (tmthing(), map.tmx, map.tmy)
    };
    let tmthing = tmthing_id.ptr();
    let blockdist = thing.radius + (*tmthing).radius;
    if (thing.x - tmx).wrapping_abs() >= blockdist || (thing.y - tmy).wrapping_abs() >= blockdist {
        return 1;
    }
    if thing_id == tmthing_id {
        return 1;
    }
    if (*tmthing).player.is_none() && gamemap() != 30 {
        return 0;
    }
    P_DamageMobj(world, thing_id, Some(tmthing_id), Some(tmthing_id), 10000);
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
/// `thing_id` must name a map object that is already linked into the map
/// (sector list and blockmap). The blockmap and sector structures must be
/// fully initialised.
pub unsafe fn P_TeleportMove(world: &mut World, thing_id: MobjId, x: fixed_t, y: fixed_t) -> u32 {
    let thing = thing_id.ptr();
    let tmbbox = start_position_check(thing_id, x, y);
    let (bmaporgx, bmaporgy) = blockmap_origin();

    let xl = (tmbbox[BBox::LEFT] - bmaporgx - MAXRADIUS) >> MAPBLOCKSHIFT;
    let xh = (tmbbox[BBox::RIGHT] - bmaporgx + MAXRADIUS) >> MAPBLOCKSHIFT;
    let yl = (tmbbox[BBox::BOTTOM] - bmaporgy - MAXRADIUS) >> MAPBLOCKSHIFT;
    let yh = (tmbbox[BBox::TOP] - bmaporgy + MAXRADIUS) >> MAPBLOCKSHIFT;

    for bx in xl..=xh {
        for by in yl..=yh {
            if P_BlockThingsIterator(world, bx, by, Some(PIT_StompThing)) == 0 {
                return 0;
            }
        }
    }

    P_UnsetThingPosition(world, thing_id);
    {
        let map = MAP.borrow();
        (*thing).floorz = map.tmfloorz;
        (*thing).ceilingz = map.tmceilingz;
    }
    (*thing).x = x;
    (*thing).y = y;
    P_SetThingPosition(world, thing_id);

    1
}

// ---------------------------------------------------------------------------
// MOVEMENT ITERATOR FUNCTIONS
// ---------------------------------------------------------------------------

/// Blockmap linedef iterator callback for [`P_CheckPosition`].
///
/// Tests whether linedef `ld` blocks the current move. If the line is
/// two-sided and passable, `tmfloorz`, `tmceilingz`, and
/// `tmdropoffz` are updated to reflect the opening. Special linedefs
/// are recorded in `spechit` for later processing by [`P_TryMove`].
///
/// Returns 1 to continue iteration, 0 to abort (line blocks the move).
///
/// # Safety
///
/// `ld` must be a linedef of the loaded level. `tmthing`, `tmbbox`,
/// `tmfloorz`, `tmceilingz`, and `tmdropoffz` must have been initialised by
/// [`P_CheckPosition`] before this callback is invoked.
pub unsafe fn PIT_CheckLine(_world: &mut World, ld: LineId) -> u32 {
    let (tmbbox, tmthing) = (MAP.borrow().tmbbox, tmthing().ptr());
    let level = LEVEL.borrow();
    let line = &level.lines[ld];
    if tmbbox[BBox::RIGHT] <= line.bbox[BBox::LEFT]
        || tmbbox[BBox::LEFT] >= line.bbox[BBox::RIGHT]
        || tmbbox[BBox::TOP] <= line.bbox[BBox::BOTTOM]
        || tmbbox[BBox::BOTTOM] >= line.bbox[BBox::TOP]
    {
        return 1;
    }
    if P_BoxOnLineSide(&level, &tmbbox, ld) != -1 {
        return 1;
    }
    if line.backsector.is_none() {
        return 0;
    }
    if (*tmthing).flags & MF_MISSILE == 0 {
        if (line.flags as i32) & (LinedefFlag::BLOCKING as i32) != 0 {
            return 0;
        }
        if (*tmthing).player.is_none()
            && (line.flags as i32) & (LinedefFlag::BLOCKMONSTERS as i32) != 0
        {
            return 0;
        }
    }
    P_LineOpening(&level, ld);
    let (opentop, openbottom, lowfloor) = (opentop(), openbottom(), lowfloor());
    let mut map = MAP.borrow_mut();
    if opentop < map.tmceilingz {
        map.tmceilingz = opentop;
        map.ceilingline = Some(ld);
    }
    if openbottom > map.tmfloorz {
        map.tmfloorz = openbottom;
    }
    if lowfloor < map.tmdropoffz {
        map.tmdropoffz = lowfloor;
    }
    if line.special != 0 {
        let index = map.numspechit as usize;
        map.spechit[index] = ld;
        map.numspechit += 1;
        if map.numspechit > MAXSPECIALCROSS_ORIGINAL {
            SpechitOverrun(&mut map, ld);
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
/// `thing_id` must name an initialised map object. `tmthing`, `tmx`,
/// `tmy`, and `tmflags` must have been set by [`P_CheckPosition`] before
/// this callback is invoked.
pub unsafe fn PIT_CheckThing(world: &mut World, thing_id: MobjId) -> u32 {
    let thing = &*thing_id.ptr();
    if thing.flags & (MF_SOLID | MF_SPECIAL | MF_SHOOTABLE) == 0 {
        return 1;
    }
    let (tmthing_id, tmx, tmy, tmflags) = {
        let map = MAP.borrow();
        (tmthing(), map.tmx, map.tmy, map.tmflags)
    };
    let tmthing = tmthing_id.ptr();
    let blockdist = thing.radius + (*tmthing).radius;
    if (thing.x - tmx).wrapping_abs() >= blockdist || (thing.y - tmy).wrapping_abs() >= blockdist {
        return 1;
    }
    if thing_id == tmthing_id {
        return 1;
    }

    // check for skulls slamming into things
    if (*tmthing).flags & MF_SKULLFLY != 0 {
        let damage = ((P_Random() % 8) + 1) * (*tmthing).info.get().damage;
        P_DamageMobj(world, thing_id, Some(tmthing_id), Some(tmthing_id), damage);
        // The damage can run actions that check other objects' positions;
        // like C, the rest of the slam acts on whatever `tmthing` is now.
        let tmthing_id = self::tmthing();
        let tmthing = tmthing_id.ptr();
        (*tmthing).flags &= !MF_SKULLFLY;
        (*tmthing).momx = 0;
        (*tmthing).momy = 0;
        (*tmthing).momz = 0;
        P_SetMobjState(world, tmthing_id, (*tmthing).info.get().spawnstate);
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
        if let Some(target_id) = (*tmthing).target {
            let target = target_id.ptr();
            let target_type = (*target).kind;
            let thing_type = thing.kind;
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
            return (thing.flags & MF_SOLID == 0) as u32;
        }
        let damage = ((P_Random() % 8) + 1) * (*tmthing).info.get().damage;
        let target = (*tmthing).target;
        P_DamageMobj(world, thing_id, Some(tmthing_id), target, damage);
        return 0;
    }

    // check for special pickup
    if thing.flags & MF_SPECIAL != 0 {
        let solid = thing.flags & MF_SOLID;
        if tmflags & MF_PICKUP != 0 {
            P_TouchSpecialThing(world, thing_id, tmthing_id);
        }
        return (solid == 0) as u32;
    }

    (thing.flags & MF_SOLID == 0) as u32
}

// ---------------------------------------------------------------------------
// MOVEMENT CLIPPING
// ---------------------------------------------------------------------------

/// Test whether `thing` can occupy position `(x, y)` without clipping.
///
/// This is a pure query — it does not move the thing. As a side effect it
/// sets `tmfloorz`, `tmceilingz`, `tmdropoffz`, `spechit`, and
/// `numspechit` for the tested position. Things with `MF_PICKUP` may
/// pick up items encountered during the sweep.
///
/// Returns 1 if the position is unobstructed, 0 if blocked.
///
/// # Safety
///
/// `thing_id` must name an initialised map object. The blockmap and all
/// sector/linedef structures must be fully initialised.
pub unsafe fn P_CheckPosition(world: &mut World, thing_id: MobjId, x: fixed_t, y: fixed_t) -> u32 {
    let thing = thing_id.ptr();
    let tmbbox = start_position_check(thing_id, x, y);

    if (*thing).flags & MF_NOCLIP != 0 {
        return 1;
    }

    let (bmaporgx, bmaporgy) = blockmap_origin();
    let xl = (tmbbox[BBox::LEFT] - bmaporgx - MAXRADIUS) >> MAPBLOCKSHIFT;
    let xh = (tmbbox[BBox::RIGHT] - bmaporgx + MAXRADIUS) >> MAPBLOCKSHIFT;
    let yl = (tmbbox[BBox::BOTTOM] - bmaporgy - MAXRADIUS) >> MAPBLOCKSHIFT;
    let yh = (tmbbox[BBox::TOP] - bmaporgy + MAXRADIUS) >> MAPBLOCKSHIFT;

    for bx in xl..=xh {
        for by in yl..=yh {
            if P_BlockThingsIterator(world, bx, by, Some(PIT_CheckThing)) == 0 {
                return 0;
            }
        }
    }

    // A touched thing's actions may have checked another position; like C,
    // the line pass uses the box (and blockmap origin) as it is now.
    let tmbbox = MAP.borrow().tmbbox;
    let (bmaporgx, bmaporgy) = blockmap_origin();
    let xl = (tmbbox[BBox::LEFT] - bmaporgx) >> MAPBLOCKSHIFT;
    let xh = (tmbbox[BBox::RIGHT] - bmaporgx) >> MAPBLOCKSHIFT;
    let yl = (tmbbox[BBox::BOTTOM] - bmaporgy) >> MAPBLOCKSHIFT;
    let yh = (tmbbox[BBox::TOP] - bmaporgy) >> MAPBLOCKSHIFT;

    for bx in xl..=xh {
        for by in yl..=yh {
            if P_BlockLinesIterator(world, bx, by, Some(PIT_CheckLine)) == 0 {
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
/// any special linedefs in `spechit` that were actually crossed are
/// activated. Things with `MF_TELEPORT` or `MF_NOCLIP` skip special-line
/// processing.
///
/// Returns 1 on success, 0 if the move is blocked.
///
/// # Safety
///
/// `thing_id` must name a map object that is already linked into the map.
/// The map data must be fully initialised.
pub unsafe fn P_TryMove(world: &mut World, thing_id: MobjId, x: fixed_t, y: fixed_t) -> u32 {
    let thing = thing_id.ptr();
    MAP.borrow_mut().floatok = 0;
    if P_CheckPosition(world, thing_id, x, y) == 0 {
        return 0;
    }
    if (*thing).flags & MF_NOCLIP == 0 {
        let mut map = MAP.borrow_mut();
        if map.tmceilingz - map.tmfloorz < (*thing).height {
            return 0;
        }
        map.floatok = 1;
        if (*thing).flags & MF_TELEPORT == 0 && map.tmceilingz - (*thing).z < (*thing).height {
            return 0;
        }
        if (*thing).flags & MF_TELEPORT == 0 && map.tmfloorz - (*thing).z > 24 * FRACUNIT {
            return 0;
        }
        if (*thing).flags & (MF_DROPOFF | MF_FLOAT) == 0
            && map.tmfloorz - map.tmdropoffz > 24 * FRACUNIT
        {
            return 0;
        }
    }

    P_UnsetThingPosition(world, thing_id);
    let oldx = (*thing).x;
    let oldy = (*thing).y;
    {
        let map = MAP.borrow();
        (*thing).floorz = map.tmfloorz;
        (*thing).ceilingz = map.tmceilingz;
    }
    (*thing).x = x;
    (*thing).y = y;
    P_SetThingPosition(world, thing_id);

    if (*thing).flags & (MF_TELEPORT | MF_NOCLIP) == 0 {
        // A crossed special can move things (teleporters reset the list),
        // so the count is re-read before every pop, as in C.
        loop {
            let ld = {
                let mut map = MAP.borrow_mut();
                if map.numspechit <= 0 {
                    break;
                }
                map.numspechit -= 1;
                map.spechit[map.numspechit as usize]
            };
            let (side, oldside, special) = {
                let level = LEVEL.borrow();
                (
                    P_PointOnLineSide(&level, (*thing).x, (*thing).y, ld),
                    P_PointOnLineSide(&level, oldx, oldy, ld),
                    level.lines[ld].special,
                )
            };
            if side != oldside && special != 0 {
                P_CrossSpecialLine(world, ld, oldside, thing_id);
            }
        }
    }

    1
}

/// Clip a thing's z-position after a nearby sector floor or ceiling has moved.
///
/// Re-runs [`P_CheckPosition`] at the thing's current x/y to refresh
/// `tmfloorz` and `tmceilingz`, then adjusts `thing->z` if necessary.
/// Walking things rise and fall with the floor; floating things are only
/// pushed down if they would exceed the ceiling. Returns 0 if the thing no
/// longer fits in the vertical gap (caller should crush or block the move).
///
/// # Safety
///
/// `thing_id` must name a map object of the level. Called only from
/// [`PIT_ChangeSector`] during sector-height propagation.
pub unsafe fn P_ThingHeightClip(world: &mut World, thing_id: MobjId) -> u32 {
    let thing = thing_id.ptr();
    let onfloor = (*thing).z == (*thing).floorz;
    P_CheckPosition(world, thing_id, (*thing).x, (*thing).y);
    {
        let map = MAP.borrow();
        (*thing).floorz = map.tmfloorz;
        (*thing).ceilingz = map.tmceilingz;
    }
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

/// Project the current `(tmxmove, tmymove)` velocity onto linedef `ld`.
///
/// Computes the component of the velocity vector that runs parallel to the
/// wall so that the next [`P_TryMove`] call will slide rather than stop.
/// Handles horizontal and vertical walls as fast special cases; uses
/// trigonometry for diagonal walls.
///
/// # Safety
///
/// `ld` must be a linedef of the loaded level. `slidemo`, `tmxmove`, and
/// `tmymove` must be set by the caller ([`P_SlideMove`]) before this
/// function is called.
pub unsafe fn P_HitSlideLine(ld: LineId) {
    let level = LEVEL.borrow();
    let line = &level.lines[ld];
    if line.slopetype == ST_HORIZONTAL {
        MAP.borrow_mut().tmymove = 0;
        return;
    }
    if line.slopetype == ST_VERTICAL {
        MAP.borrow_mut().tmxmove = 0;
        return;
    }
    let (slidemo, tmxmove, tmymove) = {
        let map = MAP.borrow();
        (mobj_or_null(map.slidemo), map.tmxmove, map.tmymove)
    };
    let side = P_PointOnLineSide(&level, (*slidemo).x, (*slidemo).y, ld);
    let mut lineangle = R_PointToAngle2(0, 0, line.dx, line.dy);
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
    let mut map = MAP.borrow_mut();
    map.tmxmove = FixedMul(newlen, *finecosine.0.add(lineangle));
    map.tmymove = FixedMul(newlen, finesine[lineangle]);
}

/// Path-traversal callback that finds blocking walls for slide movement.
///
/// Called by [`P_PathTraverse`] from [`P_SlideMove`]. If the intercept is
/// not a line, the engine aborts with an error. For each blocking line the
/// fractional intercept is compared with `bestslidefrac`; closer hits
/// displace the current best and push the old best to the second slot.
///
/// Returns 1 to continue traversal (line is passable), 0 to stop.
///
/// # Safety
///
/// `in_` must be a valid, non-null pointer to an initialised
/// [`intercept_t`]. `slidemo` must point to the object being moved.
pub unsafe fn PTR_SlideTraverse(_world: &mut World, in_: *mut intercept_t) -> u32 {
    let in_ = &*in_;
    let InterceptHit::Line(li) = in_.hit else {
        i_error!("PTR_SlideTraverse: not a line?");
    };
    let slidemo = mobj_or_null(MAP.borrow().slidemo);
    {
        let level = LEVEL.borrow();
        if level.lines[li].flags as i32 & LinedefFlag::TWOSIDED as i32 == 0 {
            if P_PointOnLineSide(&level, (*slidemo).x, (*slidemo).y, li) != 0 {
                return 1;
            }
        } else {
            P_LineOpening(&level, li);
            if openrange() >= (*slidemo).height
                && opentop() - (*slidemo).z >= (*slidemo).height
                && openbottom() - (*slidemo).z <= 24 * FRACUNIT
            {
                return 1;
            }
        }
    }
    let mut map = MAP.borrow_mut();
    if in_.frac < map.bestslidefrac {
        map.secondslidefrac = map.bestslidefrac;
        map.secondslideline = map.bestslideline;
        map.bestslidefrac = in_.frac;
        map.bestslideline = Some(li);
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
/// `mo_id` must name a map object of the level. Map data must be fully
/// initialised.
pub unsafe fn P_SlideMove(world: &mut World, mo_id: MobjId) {
    let mo = mo_id.ptr();
    MAP.borrow_mut().slidemo = Some(mo_id);
    let mut hitcount = 0;

    'retry: loop {
        hitcount += 1;
        if hitcount == 3 {
            // stairstep
            if P_TryMove(world, mo_id, (*mo).x, (*mo).y + (*mo).momy) == 0 {
                P_TryMove(world, mo_id, (*mo).x + (*mo).momx, (*mo).y);
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

        MAP.borrow_mut().bestslidefrac = FRACUNIT + 1;

        P_PathTraverse(
            world,
            leadx,
            leady,
            leadx + (*mo).momx,
            leady + (*mo).momy,
            PT_ADDLINES,
            Some(PTR_SlideTraverse),
        );
        P_PathTraverse(
            world,
            trailx,
            leady,
            trailx + (*mo).momx,
            leady + (*mo).momy,
            PT_ADDLINES,
            Some(PTR_SlideTraverse),
        );
        P_PathTraverse(
            world,
            leadx,
            traily,
            leadx + (*mo).momx,
            traily + (*mo).momy,
            PT_ADDLINES,
            Some(PTR_SlideTraverse),
        );

        if MAP.borrow().bestslidefrac == FRACUNIT + 1 {
            // stairstep
            if P_TryMove(world, mo_id, (*mo).x, (*mo).y + (*mo).momy) == 0 {
                P_TryMove(world, mo_id, (*mo).x + (*mo).momx, (*mo).y);
            }
            return;
        }

        let bestslidefrac = {
            let mut map = MAP.borrow_mut();
            map.bestslidefrac -= 0x800;
            map.bestslidefrac
        };
        if bestslidefrac > 0 {
            let newx = FixedMul((*mo).momx, bestslidefrac);
            let newy = FixedMul((*mo).momy, bestslidefrac);
            if P_TryMove(world, mo_id, (*mo).x + newx, (*mo).y + newy) == 0 {
                continue 'retry;
            }
        }

        let (bestslidefrac, bestslideline) = {
            let mut map = MAP.borrow_mut();
            map.bestslidefrac = FRACUNIT - (map.bestslidefrac + 0x800);
            if map.bestslidefrac > FRACUNIT {
                map.bestslidefrac = FRACUNIT;
            }
            (map.bestslidefrac, map.bestslideline)
        };
        if bestslidefrac <= 0 {
            return;
        }
        {
            let mut map = MAP.borrow_mut();
            map.tmxmove = FixedMul((*mo).momx, bestslidefrac);
            map.tmymove = FixedMul((*mo).momy, bestslidefrac);
        }
        // A traversal that brought `bestslidefrac` below `FRACUNIT + 1` set
        // the line with it.
        P_HitSlideLine(bestslideline.expect("slide hit without a line"));
        let (tmxmove, tmymove) = {
            let map = MAP.borrow();
            (map.tmxmove, map.tmymove)
        };
        (*mo).momx = tmxmove;
        (*mo).momy = tmymove;
        if P_TryMove(world, mo_id, (*mo).x + tmxmove, (*mo).y + tmymove) == 0 {
            continue 'retry;
        }
        return;
    }
}

// ---------------------------------------------------------------------------
// LINE ATTACK
// ---------------------------------------------------------------------------

/// Path-traversal callback for [`P_AimLineAttack`] auto-aim.
///
/// For each line intercept, narrows the vertical aim window (`topslope` /
/// `bottomslope`) based on the opening. For each thing intercept, checks
/// whether the thing falls within the aim window and, if so, records it in
/// `linetarget` and computes `aimslope`.
///
/// Returns 1 to continue traversal, 0 to stop (target locked or window
/// closed).
///
/// # Safety
///
/// `in_` must be a valid, non-null pointer to an initialised
/// [`intercept_t`]. `shootthing`, `shootz`, `attackrange`,
/// `topslope`, and `bottomslope` must be set before the traversal begins.
pub unsafe fn PTR_AimTraverse(_world: &mut World, in_: *mut intercept_t) -> u32 {
    let in_ = &*in_;
    let (shootthing, shootz, attackrange) = {
        let map = MAP.borrow();
        (map.shootthing, map.shootz, map.attackrange)
    };
    let th_id = match in_.hit {
        InterceptHit::Thing(thing) => thing,
        InterceptHit::Line(li) => {
            let level = LEVEL.borrow();
            let line = &level.lines[li];
            if line.flags as i32 & LinedefFlag::TWOSIDED as i32 == 0 {
                return 0;
            }
            P_LineOpening(&level, li);
            let (opentop, openbottom) = (opentop(), openbottom());
            if openbottom >= opentop {
                return 0;
            }
            let dist = FixedMul(attackrange, in_.frac);
            let front = &level.sectors[line
                .frontsector
                .expect("two-sided line without front sector")];
            let back = line.backsector.map(|back| &level.sectors[back]);
            let mut sight = SIGHT.borrow_mut();
            if back.is_none_or(|back| front.floorheight != back.floorheight) {
                let slope = FixedDiv(openbottom - shootz, dist);
                if slope > sight.bottomslope {
                    sight.bottomslope = slope;
                }
            }
            if back.is_none_or(|back| front.ceilingheight != back.ceilingheight) {
                let slope = FixedDiv(opentop - shootz, dist);
                if slope < sight.topslope {
                    sight.topslope = slope;
                }
            }
            if sight.topslope <= sight.bottomslope {
                return 0;
            }
            return 1;
        }
    };

    if Some(th_id) == shootthing {
        return 1;
    }
    let th = th_id.ptr();
    let (topslope, bottomslope) = {
        let sight = SIGHT.borrow();
        (sight.topslope, sight.bottomslope)
    };
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
    let mut map = MAP.borrow_mut();
    map.aimslope = (thingtopslope + thingbottomslope) / 2;
    map.linetarget = Some(th_id);
    0
}

/// Path-traversal callback for [`P_LineAttack`] hitscan shooting.
///
/// For line intercepts, activates any special on the line, then checks
/// whether the shot passes through the opening or hits the wall; if it
/// hits, `goto_hitline` spawns a bullet puff and the traversal stops.
/// For thing intercepts, checks z-overlap with `aimslope`, spawns puff
/// or blood, and calls [`P_DamageMobj`] if `la_damage` is non-zero.
///
/// Returns 1 to continue traversal, 0 to stop (shot consumed).
///
/// # Safety
///
/// `in_` must be a valid, non-null pointer to an initialised
/// [`intercept_t`]. `shootthing`, `shootz`, `attackrange`,
/// `aimslope`, and `la_damage` must be set before the traversal.
pub unsafe fn PTR_ShootTraverse(world: &mut World, in_: *mut intercept_t) -> u32 {
    let in_ = &*in_;
    if let InterceptHit::Line(li) = in_.hit {
        if LEVEL.borrow().lines[li].special != 0 {
            let shootthing = MAP.borrow().shootthing.expect("shot without a shooter");
            P_ShootSpecialLine(world, shootthing, li);
        }
        // The special can change the line (a switch clears it) and move
        // sectors, so the line is read afresh, as C does.
        let line = LEVEL.borrow().lines[li];
        if line.flags as i32 & LinedefFlag::TWOSIDED as i32 != 0 {
            P_LineOpening(&LEVEL.borrow(), li);
            let (opentop, openbottom) = (opentop(), openbottom());
            let (shootz, attackrange, aimslope) = {
                let map = MAP.borrow();
                (map.shootz, map.attackrange, map.aimslope)
            };
            let dist = FixedMul(attackrange, in_.frac);
            match line.backsector {
                None => {
                    let slope = FixedDiv(openbottom - shootz, dist);
                    if slope > aimslope {
                        goto_hitline(world, li, in_);
                        return 0;
                    }
                    let slope = FixedDiv(opentop - shootz, dist);
                    if slope < aimslope {
                        goto_hitline(world, li, in_);
                        return 0;
                    }
                }
                Some(back) => {
                    let (front, back) = {
                        let level = LEVEL.borrow();
                        let front = line
                            .frontsector
                            .expect("two-sided line without front sector");
                        (level.sectors[front], level.sectors[back])
                    };
                    if front.floorheight != back.floorheight {
                        let slope = FixedDiv(openbottom - shootz, dist);
                        if slope > aimslope {
                            goto_hitline(world, li, in_);
                            return 0;
                        }
                    }
                    if front.ceilingheight != back.ceilingheight {
                        let slope = FixedDiv(opentop - shootz, dist);
                        if slope < aimslope {
                            goto_hitline(world, li, in_);
                            return 0;
                        }
                    }
                }
            }
            return 1;
        }
        goto_hitline(world, li, in_);
        return 0;
    }

    let InterceptHit::Thing(th_id) = in_.hit else {
        unreachable!("lines are handled above");
    };
    let (shootthing, shootz, attackrange, aimslope, la_damage) = {
        let map = MAP.borrow();
        (
            map.shootthing,
            map.shootz,
            map.attackrange,
            map.aimslope,
            map.la_damage,
        )
    };
    if Some(th_id) == shootthing {
        return 1;
    }
    let th = th_id.ptr();
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
    let trace = trace();
    let x = trace.x + FixedMul(trace.dx, frac);
    let y = trace.y + FixedMul(trace.dy, frac);
    let z = shootz + FixedMul(aimslope, FixedMul(frac, attackrange));
    if (*th).flags & MF_NOBLOOD != 0 {
        P_SpawnPuff(world, x, y, z);
    } else {
        P_SpawnBlood(world, x, y, z, la_damage);
    }
    // Spawning sets the new object's state, so re-read the shot as C does.
    let (shootthing, la_damage) = {
        let map = MAP.borrow();
        (map.shootthing, map.la_damage)
    };
    if la_damage != 0 {
        P_DamageMobj(world, th_id, shootthing, shootthing, la_damage);
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
/// `li` must be a linedef of the loaded level. `in_` must refer to the
/// intercept that triggered this call. `shootz`, `aimslope`, and
/// `attackrange` must be current.
unsafe fn goto_hitline(world: &mut World, li: LineId, in_: &intercept_t) {
    let (shootz, attackrange, aimslope) = {
        let map = MAP.borrow();
        (map.shootz, map.attackrange, map.aimslope)
    };
    let frac = in_.frac - FixedDiv(4 * FRACUNIT, attackrange);
    let trace = trace();
    let x = trace.x + FixedMul(trace.dx, frac);
    let y = trace.y + FixedMul(trace.dy, frac);
    let z = shootz + FixedMul(aimslope, FixedMul(frac, attackrange));
    let skyflatnum = SKY.borrow().skyflatnum;
    {
        let level = LEVEL.borrow();
        let line = &level.lines[li];
        let front = &level.sectors[line.frontsector.expect("hit line without front sector")];
        if front.ceilingpic as i32 == skyflatnum {
            if z > front.ceilingheight {
                return;
            }
            if line
                .backsector
                .is_some_and(|back| level.sectors[back].ceilingpic as i32 == skyflatnum)
            {
                return;
            }
        }
    }
    P_SpawnPuff(world, x, y, z);
}

/// Auto-aim a hitscan ray from `t1` along `angle` up to `distance` away.
///
/// Runs [`PTR_AimTraverse`] to find the best shootable target within the
/// vertical aim window (approximately ±35 degrees). Returns the vertical
/// slope to the centre of that target, or 0 if nothing was found.
/// Sets `linetarget` as a side effect.
///
/// # Safety
///
/// `t1` names the shooter (C substitutes a dummy for null via
/// [`P_SubstNullMobj`]; the callers always pass one). Map data must be
/// initialised.
pub unsafe fn P_AimLineAttack(
    world: &mut World,
    t1: MobjId,
    angle: u32,
    distance: fixed_t,
) -> fixed_t {
    let t1_id = P_SubstNullMobj(Some(t1));
    let t1 = t1_id.ptr();
    let angle = (angle >> ANGLETOFINESHIFT) as usize;
    let x2 = (*t1).x + (distance >> FRACBITS) * *finecosine.0.add(angle);
    let y2 = (*t1).y + (distance >> FRACBITS) * finesine[angle];
    {
        let mut map = MAP.borrow_mut();
        map.shootthing = Some(t1_id);
        map.shootz = (*t1).z + ((*t1).height >> 1) + 8 * FRACUNIT;
        map.attackrange = distance;
        map.linetarget = None;
    }
    {
        let mut sight = SIGHT.borrow_mut();
        sight.topslope = 100 * FRACUNIT / 160;
        sight.bottomslope = -(100 * FRACUNIT / 160);
    }
    P_PathTraverse(
        world,
        (*t1).x,
        (*t1).y,
        x2,
        y2,
        PT_ADDLINES | PT_ADDTHINGS,
        Some(PTR_AimTraverse),
    );
    let map = MAP.borrow();
    if map.linetarget.is_some() {
        return map.aimslope;
    }
    0
}

/// Fire a hitscan attack from `t1` with the given `angle`, `distance`,
/// `slope`, and `damage`.
///
/// Runs [`PTR_ShootTraverse`] along the ray. If `damage` is 0 the call is
/// a pure trace that sets `linetarget` without dealing damage. Spawns
/// puffs or blood at the impact point as a side effect.
///
/// # Safety
///
/// `t1_id` must name a map object of the level. Map data must be fully
/// initialised.
pub unsafe fn P_LineAttack(
    world: &mut World,
    t1_id: MobjId,
    angle: u32,
    distance: fixed_t,
    slope: fixed_t,
    damage: i32,
) {
    let t1 = t1_id.ptr();
    let angle = (angle >> ANGLETOFINESHIFT) as usize;
    let x2 = (*t1).x + (distance >> FRACBITS) * *finecosine.0.add(angle);
    let y2 = (*t1).y + (distance >> FRACBITS) * finesine[angle];
    {
        let mut map = MAP.borrow_mut();
        map.shootthing = Some(t1_id);
        map.la_damage = damage;
        map.shootz = (*t1).z + ((*t1).height >> 1) + 8 * FRACUNIT;
        map.attackrange = distance;
        map.aimslope = slope;
    }
    P_PathTraverse(
        world,
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
/// [`intercept_t`] that hit a line. `usething` must be set.
pub unsafe fn PTR_UseTraverse(world: &mut World, in_: *mut intercept_t) -> u32 {
    let in_ = &*in_;
    let usething_id = MAP.borrow().usething.expect("use without a user");
    let usething = usething_id.ptr();
    let InterceptHit::Line(line) = in_.hit else {
        unreachable!("the use traversal collects only lines");
    };
    let side = {
        let level = LEVEL.borrow();
        if level.lines[line].special == 0 {
            P_LineOpening(&level, line);
            drop(level);
            if openrange() <= 0 {
                S_StartSound(usething_id, Sfx::Noway as i32);
                return 0;
            }
            return 1;
        }
        let mut side = 0;
        if P_PointOnLineSide(&level, (*usething).x, (*usething).y, line) == 1 {
            side = 1;
        }
        side
    };
    P_UseSpecialLine(world, usething_id, line, side);
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
/// `player`'s `mo` must name a map object of the loaded level. Map data
/// must be initialised.
pub unsafe fn P_UseLines(world: &mut World, player: PlayerId) {
    let mo_id = player.mo();
    let mo = mo_id.ptr();
    MAP.borrow_mut().usething = Some(mo_id);
    let angle = ((*mo).angle >> ANGLETOFINESHIFT) as usize;
    let x1 = (*mo).x;
    let y1 = (*mo).y;
    let x2 = x1 + (USERANGE >> FRACBITS) * *finecosine.0.add(angle);
    let y2 = y1 + (USERANGE >> FRACBITS) * finesine[angle];
    P_PathTraverse(world, x1, y1, x2, y2, PT_ADDLINES, Some(PTR_UseTraverse));
}

// ---------------------------------------------------------------------------
// RADIUS ATTACK
// ---------------------------------------------------------------------------

/// Blockmap thing iterator callback for [`P_RadiusAttack`].
///
/// Skips non-shootable things and the two boss types immune to splash
/// (Cyberdemon and Spider Mastermind). Computes Chebyshev distance from
/// `bombspot`, subtracts the thing's radius, and if in range and in
/// line-of-sight, deals `bombdamage - dist` damage.
///
/// Always returns 1 (continues blockmap iteration).
///
/// # Safety
///
/// `thing_id` must name an initialised map object. `bombspot`,
/// `bombsource`, and `bombdamage` must be set by [`P_RadiusAttack`] before
/// this callback is invoked.
pub unsafe fn PIT_RadiusAttack(world: &mut World, thing_id: MobjId) -> u32 {
    let thing = &*thing_id.ptr();
    if thing.flags & MF_SHOOTABLE == 0 {
        return 1;
    }
    if thing.kind == MT_CYBORG || thing.kind == MT_SPIDER {
        return 1;
    }
    // The sight check below does not touch this state, so one read serves
    // the whole callback.
    let (bombspot_id, bombsource, bombdamage) = {
        let map = MAP.borrow();
        (
            map.bombspot.expect("radius attack without a spot"),
            map.bombsource,
            map.bombdamage,
        )
    };
    let bombspot = bombspot_id.ptr();
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
    if P_CheckSight(world, thing_id, bombspot_id) != 0 {
        P_DamageMobj(
            world,
            thing_id,
            Some(bombspot_id),
            bombsource,
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
/// `spot_id` must name a map object of the level; `source` may be `None`
/// or stale (C passes the exploding object's `target`, which may be gone).
/// Map and blockmap data must be fully initialised.
pub unsafe fn P_RadiusAttack(
    world: &mut World,
    spot_id: MobjId,
    source: Option<MobjId>,
    damage: i32,
) {
    let spot = spot_id.ptr();
    let (bmaporgx, bmaporgy) = blockmap_origin();
    let dist = (damage + MAXRADIUS) << FRACBITS;
    let yh = ((*spot).y + dist - bmaporgy) >> MAPBLOCKSHIFT;
    let yl = ((*spot).y - dist - bmaporgy) >> MAPBLOCKSHIFT;
    let xh = ((*spot).x + dist - bmaporgx) >> MAPBLOCKSHIFT;
    let xl = ((*spot).x - dist - bmaporgx) >> MAPBLOCKSHIFT;
    {
        let mut map = MAP.borrow_mut();
        map.bombspot = Some(spot_id);
        map.bombsource = source;
        map.bombdamage = damage;
    }
    for y in yl..=yh {
        for x in xl..=xh {
            P_BlockThingsIterator(world, x, y, Some(PIT_RadiusAttack));
        }
    }
}

// ---------------------------------------------------------------------------
// SECTOR HEIGHT CHANGING
// ---------------------------------------------------------------------------

/// Blockmap thing iterator callback for [`P_ChangeSector`].
///
/// Calls [`P_ThingHeightClip`] on each thing. If the thing fits, returns 1.
/// If it does not fit: dead things are gibbed, dropped items are removed,
/// non-shootable things are ignored (assumed decorative), and live shootable
/// things set `nofit` and optionally receive crush damage with a blood
/// spray every 4 tics.
///
/// Always returns 1 to continue checking other things.
///
/// # Safety
///
/// `thing_id` must name an initialised map object. `crushchange` must be
/// set by [`P_ChangeSector`] before iteration.
pub unsafe fn PIT_ChangeSector(world: &mut World, thing_id: MobjId) -> u32 {
    if P_ThingHeightClip(world, thing_id) != 0 {
        return 1;
    }
    let thing = &mut *thing_id.ptr();
    if thing.health <= 0 {
        P_SetMobjState(world, thing_id, S_GIBS);
        thing.flags &= !MF_SOLID;
        thing.height = 0;
        thing.radius = 0;
        return 1;
    }
    if thing.flags & MF_DROPPED != 0 {
        P_RemoveMobj(world, thing_id);
        return 1;
    }
    if thing.flags & MF_SHOOTABLE == 0 {
        return 1;
    }
    let crushchange = {
        let mut map = MAP.borrow_mut();
        map.nofit = 1;
        map.crushchange
    };
    if crushchange != 0 && leveltime() & 3 == 0 {
        P_DamageMobj(world, thing_id, None, None, 10);
        let mo = P_SpawnMobj(
            world,
            thing.x,
            thing.y,
            thing.z + thing.height / 2,
            MT_BLOOD,
        )
        .ptr();
        (*mo).momx = (P_Random() - P_Random()) << 12;
        (*mo).momy = (P_Random() - P_Random()) << 12;
    }
    1
}

/// Propagate a floor or ceiling height change in `sector` to all nearby things.
///
/// Re-checks height constraints for every thing in the blockmap cells that
/// overlap `sector->blockbox`. Returns non-zero if any thing no longer fits
/// (i.e. `nofit` was set). If `crunch` is zero and this returns non-zero,
/// the caller should revert the sector height and call this again.
///
/// # Safety
///
/// `sector` must be a sector of the loaded level, and every thing linked
/// into the blockmap must be live.
pub unsafe fn P_ChangeSector(world: &mut World, sector: SectorId, crunch: i32) -> i32 {
    {
        let mut map = MAP.borrow_mut();
        map.nofit = 0;
        map.crushchange = crunch;
    }
    let blockbox = LEVEL.borrow().sectors[sector].blockbox;
    for x in blockbox[BBox::LEFT]..=blockbox[BBox::RIGHT] {
        for y in blockbox[BBox::BOTTOM]..=blockbox[BBox::TOP] {
            P_BlockThingsIterator(world, x, y, Some(PIT_ChangeSector));
        }
    }
    MAP.borrow().nofit
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
/// `ld` is only used for its index in the level's `lines` array. Called from
/// [`PIT_CheckLine`] after `numspechit` has been incremented beyond
/// [`MAXSPECIALCROSS_ORIGINAL`].
fn SpechitOverrun(map: &mut MapState, ld: LineId) {
    if map.spechit_baseaddr == 0 {
        let p = M_CheckParmWithArgs("-spechit", 1);
        if p > 0 {
            if let Some(value) = M_StrToInt(argument(p + 1)) {
                map.spechit_baseaddr = value as u32;
            }
        } else {
            map.spechit_baseaddr = DEFAULT_SPECHIT_MAGIC;
        }
    }
    let line_index = ld.index() as isize;
    let addr = map.spechit_baseaddr as i32 + (line_index * 0x3e) as i32;
    match map.numspechit {
        9..=12 => {
            let index = (map.numspechit - 9) as usize;
            map.tmbbox[index] = addr;
        }
        13 => {
            map.crushchange = addr;
        }
        14 => {
            map.nofit = addr;
        }
        _ => {
            eprintln!(
                "SpechitOverrun: Warning: unable to emulate an overrun where numspechit={}",
                map.numspechit
            );
        }
    }
}
