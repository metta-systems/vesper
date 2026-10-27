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
            tables::{ANG180, ANGLETOFINESHIFT, FINEANGLES, finesine},
        },
        misc::random::P_Random,
        play::{
            maputl::{
                InterceptHit, MAPBLOCKSHIFT, P_AproxDistance, P_BlockLinesIterator,
                P_BlockThingsIterator, P_BoxOnLineSide, P_LineOpening, P_PathTraverse,
                P_PointOnLineSide, P_SetThingPosition, P_UnsetThingPosition, PT_ADDLINES,
                PT_ADDTHINGS, intercept_t,
            },
            mobj::MobjId,
            sight::P_CheckSight,
        },
        render_3d::main::{R_PointInSubsector, R_PointToAngle2},
        wad::format::LinedefFlag,
        world::{PlayScratch, World},
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
    },
    render_3d::RENDERER,
    sound::{S_StartSound, info::Sfx},
};

// ---------------------------------------------------------------------------
// Movement scratchpad state
// ---------------------------------------------------------------------------

/// Movement, slide, hitscan, use, splash and crush scratchpad (`p_map.c`
/// globals), kept in [`PlayScratch`].
///
/// The blockmap and path-traversal callbacks below re-enter play code:
/// damage runs death actions that move and spawn other objects, which check
/// positions and traverse paths again, overwriting this state exactly as
/// they overwrote the C globals. The C code reads each global afresh after
/// such a call, so every access here is a short one that never spans a
/// call into other play code; values are copied out before a call and
/// re-read after it.
pub struct MapState {
    /// Axis-aligned bounding box of the thing currently being position-checked.
    ///
    /// Set by [`P_CheckPosition`] / [`P_TeleportMove`] before iterating over
    /// blockmap cells. Indices follow [`BBox`] (TOP, BOTTOM, LEFT, RIGHT).
    pub tmbbox: [fixed_t; 4],
    /// The map object currently being tested by [`P_CheckPosition`].
    ///
    /// Used by the blockmap iterator callbacks ([`PIT_CheckThing`],
    /// [`PIT_CheckLine`]) to access the moving object without passing it through
    /// the C-style function-pointer interface.
    pub tmthing: Option<MobjId>,
    /// Cached copy of `tmthing->flags` for the current position check.
    ///
    /// Avoids repeated arena reads inside tight blockmap loops.
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

/// The object being position-checked (C `tmthing`).
///
/// # Panics
///
/// If no position check has run yet.
fn tmthing(world: &World) -> MobjId {
    world
        .scratch
        .map
        .tmthing
        .expect("no position check running")
}

/// Record `thing` at `(x, y)` as the object being position-checked and set
/// its bounding box (the common prologue of `P_TeleportMove` and
/// `P_CheckPosition`), then start the floor, ceiling and special-line
/// results from the sector at that spot. Returns the bounding box.
fn start_position_check(
    world: &mut World,
    thing_id: MobjId,
    x: fixed_t,
    y: fixed_t,
) -> [fixed_t; 4] {
    let (radius, flags) = {
        let mobjs = &world.mobjs;
        (mobjs[thing_id].radius, mobjs[thing_id].flags)
    };
    {
        let map = &mut world.scratch.map;
        map.tmthing = Some(thing_id);
        map.tmflags = flags;
        map.tmx = x;
        map.tmy = y;

        map.tmbbox[BBox::TOP] = y + radius;
        map.tmbbox[BBox::BOTTOM] = y - radius;
        map.tmbbox[BBox::RIGHT] = x + radius;
        map.tmbbox[BBox::LEFT] = x - radius;
    }

    let newsubsec = R_PointInSubsector(&world.level, x, y);
    let (floorheight, ceilingheight) = {
        let sec = world.level.subsector_sector(newsubsec);
        (sec.floorheight, sec.ceilingheight)
    };
    world.scratch.validcount = world.scratch.validcount.wrapping_add(1);
    let map = &mut world.scratch.map;
    map.ceilingline = None;
    map.tmfloorz = floorheight;
    map.tmdropoffz = map.tmfloorz;
    map.tmceilingz = ceilingheight;
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
/// Reads `tmthing` and `tmx` / `tmy`, which the caller ([`P_TeleportMove`])
/// sets before the iteration; the stomp runs [`P_DamageMobj`].
pub fn PIT_StompThing(world: &mut World, thing_id: MobjId) -> u32 {
    let (tmthing_id, tmx, tmy) = (tmthing(world), world.scratch.map.tmx, world.scratch.map.tmy);
    {
        let mobjs = &world.mobjs;
        let thing = &mobjs[thing_id];
        if thing.flags & MF_SHOOTABLE == 0 {
            return 1;
        }
        let tmthing = &mobjs[tmthing_id];
        let blockdist = thing.radius + tmthing.radius;
        if (thing.x - tmx).wrapping_abs() >= blockdist
            || (thing.y - tmy).wrapping_abs() >= blockdist
        {
            return 1;
        }
        if thing_id == tmthing_id {
            return 1;
        }
        if tmthing.player.is_none() && world.session.gamemap != 30 {
            return 0;
        }
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
/// `thing_id` is expected to be linked into the map (sector list and
/// blockmap). The stomps run [`P_DamageMobj`].
pub fn P_TeleportMove(world: &mut World, thing_id: MobjId, x: fixed_t, y: fixed_t) -> u32 {
    let tmbbox = start_position_check(world, thing_id, x, y);
    let (bmaporgx, bmaporgy) = world.level.blockmap_origin();

    let xl = (tmbbox[BBox::LEFT] - bmaporgx - MAXRADIUS) >> MAPBLOCKSHIFT;
    let xh = (tmbbox[BBox::RIGHT] - bmaporgx + MAXRADIUS) >> MAPBLOCKSHIFT;
    let yl = (tmbbox[BBox::BOTTOM] - bmaporgy - MAXRADIUS) >> MAPBLOCKSHIFT;
    let yh = (tmbbox[BBox::TOP] - bmaporgy + MAXRADIUS) >> MAPBLOCKSHIFT;

    for bx in xl..=xh {
        for by in yl..=yh {
            // The stomp parameters are set above.
            if !P_BlockThingsIterator(world, bx, by, |world, thing| {
                PIT_StompThing(world, thing) != 0
            }) {
                return 0;
            }
        }
    }

    P_UnsetThingPosition(world, thing_id);
    {
        let mobjs = &mut world.mobjs;
        let thing = &mut mobjs[thing_id];
        thing.floorz = world.scratch.map.tmfloorz;
        thing.ceilingz = world.scratch.map.tmceilingz;
        thing.x = x;
        thing.y = y;
    }
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
/// `tmthing`, `tmbbox`, `tmfloorz`, `tmceilingz`, and `tmdropoffz` must have
/// been initialised by [`P_CheckPosition`] before this callback is invoked.
///
/// # Panics
///
/// If `ld` is not a linedef of the loaded level, or no position check is
/// running.
pub fn PIT_CheckLine(world: &mut World, ld: LineId) -> u32 {
    let tmbbox = world.scratch.map.tmbbox;
    let World { level, scratch, .. } = world;
    let line = &level.lines[ld];
    if tmbbox[BBox::RIGHT] <= line.bbox[BBox::LEFT]
        || tmbbox[BBox::LEFT] >= line.bbox[BBox::RIGHT]
        || tmbbox[BBox::TOP] <= line.bbox[BBox::BOTTOM]
        || tmbbox[BBox::BOTTOM] >= line.bbox[BBox::TOP]
    {
        return 1;
    }
    if P_BoxOnLineSide(level, &tmbbox, ld) != -1 {
        return 1;
    }
    if line.backsector.is_none() {
        return 0;
    }
    let (tmthing_flags, tmthing_player) = {
        let mobjs = &world.mobjs;
        let tmthing = &mobjs[scratch.map.tmthing.expect("no position check running")];
        (tmthing.flags, tmthing.player)
    };
    if tmthing_flags & MF_MISSILE == 0 {
        if (line.flags as i32) & (LinedefFlag::BLOCKING as i32) != 0 {
            return 0;
        }
        if tmthing_player.is_none()
            && (line.flags as i32) & (LinedefFlag::BLOCKMONSTERS as i32) != 0
        {
            return 0;
        }
    }
    let PlayScratch { map, maputl, .. } = scratch;
    P_LineOpening(maputl, level, ld);
    let (opentop, openbottom, lowfloor) = (maputl.opentop, maputl.openbottom, maputl.lowfloor);
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
            SpechitOverrun(map, ld);
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
/// Reads `tmthing`, `tmx`, `tmy`, and `tmflags`, which [`P_CheckPosition`]
/// sets before the iteration; a hit runs [`P_DamageMobj`] or
/// [`P_TouchSpecialThing`].
pub fn PIT_CheckThing(world: &mut World, thing_id: MobjId) -> u32 {
    // Both objects are read before the first call that runs play code, and
    // every path returns right after it (the skull slam re-reads `tmthing`).
    let mobjs = &world.mobjs;
    let thing = &mobjs[thing_id];
    if thing.flags & (MF_SOLID | MF_SPECIAL | MF_SHOOTABLE) == 0 {
        return 1;
    }
    let (tmthing_id, tmx, tmy, tmflags) = {
        let map = &world.scratch.map;
        (tmthing(world), map.tmx, map.tmy, map.tmflags)
    };
    let tmthing = &mobjs[tmthing_id];
    let blockdist = thing.radius + tmthing.radius;
    if (thing.x - tmx).wrapping_abs() >= blockdist || (thing.y - tmy).wrapping_abs() >= blockdist {
        return 1;
    }
    if thing_id == tmthing_id {
        return 1;
    }
    let (thing_flags, thing_z, thing_height, thing_type) =
        (thing.flags, thing.z, thing.height, thing.kind);
    let (tmthing_flags, tmthing_z, tmthing_height, tmthing_target) =
        (tmthing.flags, tmthing.z, tmthing.height, tmthing.target);
    let tmthing_info = tmthing.info;

    // check for skulls slamming into things
    if tmthing_flags & MF_SKULLFLY != 0 {
        let damage = ((P_Random(world) % 8) + 1) * tmthing_info.get().damage;
        P_DamageMobj(world, thing_id, Some(tmthing_id), Some(tmthing_id), damage);
        // The damage can run actions that check other objects' positions;
        // like C, the rest of the slam acts on whatever `tmthing` is now.
        let tmthing_id = self::tmthing(world);
        let spawnstate = {
            let mobjs = &mut world.mobjs;
            let tmthing = &mut mobjs[tmthing_id];
            tmthing.flags &= !MF_SKULLFLY;
            tmthing.momx = 0;
            tmthing.momy = 0;
            tmthing.momz = 0;
            tmthing.info.get().spawnstate
        };
        P_SetMobjState(world, tmthing_id, spawnstate);
        return 0;
    }

    // missiles can hit other things
    if tmthing_flags & MF_MISSILE != 0 {
        if tmthing_z > thing_z + thing_height {
            return 1;
        }
        if tmthing_z + tmthing_height < thing_z {
            return 1;
        }
        if let Some(target_id) = tmthing_target {
            let target_type = mobjs[target_id].kind;
            if target_type == thing_type
                || (target_type == MT_KNIGHT && thing_type == MT_BRUISER)
                || (target_type == MT_BRUISER && thing_type == MT_KNIGHT)
            {
                // C compares the pointers: the same slot is the same object.
                if thing_id.index() == target_id.index() {
                    return 1;
                }
                if thing_type != MT_PLAYER && DEH_DEFAULT_SPECIES_INFIGHTING == 0 {
                    return 0;
                }
            }
        }
        if thing_flags & MF_SHOOTABLE == 0 {
            return (thing_flags & MF_SOLID == 0) as u32;
        }
        let damage = ((P_Random(world) % 8) + 1) * tmthing_info.get().damage;
        P_DamageMobj(world, thing_id, Some(tmthing_id), tmthing_target, damage);
        return 0;
    }

    // check for special pickup
    if thing_flags & MF_SPECIAL != 0 {
        let solid = thing_flags & MF_SOLID;
        if tmflags & MF_PICKUP != 0 {
            P_TouchSpecialThing(world, thing_id, tmthing_id);
        }
        return (solid == 0) as u32;
    }

    (thing_flags & MF_SOLID == 0) as u32
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
/// Returns 1 if the position is unobstructed, 0 if blocked. Touching things
/// runs [`PIT_CheckThing`].
pub fn P_CheckPosition(world: &mut World, thing_id: MobjId, x: fixed_t, y: fixed_t) -> u32 {
    let tmbbox = start_position_check(world, thing_id, x, y);

    if world.mobjs[thing_id].flags & MF_NOCLIP != 0 {
        return 1;
    }

    let (bmaporgx, bmaporgy) = world.level.blockmap_origin();
    let xl = (tmbbox[BBox::LEFT] - bmaporgx - MAXRADIUS) >> MAPBLOCKSHIFT;
    let xh = (tmbbox[BBox::RIGHT] - bmaporgx + MAXRADIUS) >> MAPBLOCKSHIFT;
    let yl = (tmbbox[BBox::BOTTOM] - bmaporgy - MAXRADIUS) >> MAPBLOCKSHIFT;
    let yh = (tmbbox[BBox::TOP] - bmaporgy + MAXRADIUS) >> MAPBLOCKSHIFT;

    for bx in xl..=xh {
        for by in yl..=yh {
            // The check parameters are set above.
            if !P_BlockThingsIterator(world, bx, by, |world, thing| {
                PIT_CheckThing(world, thing) != 0
            }) {
                return 0;
            }
        }
    }

    // A touched thing's actions may have checked another position; like C,
    // the line pass uses the box (and blockmap origin) as it is now.
    let tmbbox = world.scratch.map.tmbbox;
    let (bmaporgx, bmaporgy) = world.level.blockmap_origin();
    let xl = (tmbbox[BBox::LEFT] - bmaporgx) >> MAPBLOCKSHIFT;
    let xh = (tmbbox[BBox::RIGHT] - bmaporgx) >> MAPBLOCKSHIFT;
    let yl = (tmbbox[BBox::BOTTOM] - bmaporgy) >> MAPBLOCKSHIFT;
    let yh = (tmbbox[BBox::TOP] - bmaporgy) >> MAPBLOCKSHIFT;

    for bx in xl..=xh {
        for by in yl..=yh {
            if !P_BlockLinesIterator(world, bx, by, |world, line| PIT_CheckLine(world, line) != 0) {
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
/// `thing_id` is expected to be linked into the map; the move runs
/// [`PIT_CheckThing`] and crossed line specials.
pub fn P_TryMove(world: &mut World, thing_id: MobjId, x: fixed_t, y: fixed_t) -> u32 {
    world.scratch.map.floatok = 0;
    if P_CheckPosition(world, thing_id, x, y) == 0 {
        return 0;
    }
    // The check can run play code; the thing is read afresh after it.
    let (flags, height, z) = {
        let mobjs = &world.mobjs;
        let thing = &mobjs[thing_id];
        (thing.flags, thing.height, thing.z)
    };
    if flags & MF_NOCLIP == 0 {
        let map = &mut world.scratch.map;
        if map.tmceilingz - map.tmfloorz < height {
            return 0;
        }
        map.floatok = 1;
        if flags & MF_TELEPORT == 0 && map.tmceilingz - z < height {
            return 0;
        }
        if flags & MF_TELEPORT == 0 && map.tmfloorz - z > 24 * FRACUNIT {
            return 0;
        }
        if flags & (MF_DROPOFF | MF_FLOAT) == 0 && map.tmfloorz - map.tmdropoffz > 24 * FRACUNIT {
            return 0;
        }
    }

    P_UnsetThingPosition(world, thing_id);
    let (oldx, oldy) = {
        let mobjs = &mut world.mobjs;
        let thing = &mut mobjs[thing_id];
        let old_position = (thing.x, thing.y);
        thing.floorz = world.scratch.map.tmfloorz;
        thing.ceilingz = world.scratch.map.tmceilingz;
        thing.x = x;
        thing.y = y;
        old_position
    };
    P_SetThingPosition(world, thing_id);

    if world.mobjs[thing_id].flags & (MF_TELEPORT | MF_NOCLIP) == 0 {
        // A crossed special can move things (teleporters reset the list),
        // so the count is re-read before every pop, as in C.
        loop {
            let ld = {
                let map = &mut world.scratch.map;
                if map.numspechit <= 0 {
                    break;
                }
                map.numspechit -= 1;
                map.spechit[map.numspechit as usize]
            };
            let (thing_x, thing_y) = {
                let mobjs = &world.mobjs;
                (mobjs[thing_id].x, mobjs[thing_id].y)
            };
            let (side, oldside, special) = {
                let level = &world.level;
                (
                    P_PointOnLineSide(level, thing_x, thing_y, ld),
                    P_PointOnLineSide(level, oldx, oldy, ld),
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
/// The check runs [`PIT_CheckThing`]. Called only from [`PIT_ChangeSector`]
/// during sector-height propagation.
pub fn P_ThingHeightClip(world: &mut World, thing_id: MobjId) -> u32 {
    let (onfloor, x, y) = {
        let mobjs = &world.mobjs;
        let thing = &mobjs[thing_id];
        (thing.z == thing.floorz, thing.x, thing.y)
    };
    P_CheckPosition(world, thing_id, x, y);
    let mobjs = &mut world.mobjs;
    let thing = &mut mobjs[thing_id];
    thing.floorz = world.scratch.map.tmfloorz;
    thing.ceilingz = world.scratch.map.tmceilingz;
    if onfloor {
        thing.z = thing.floorz;
    } else if thing.z + thing.height > thing.ceilingz {
        thing.z = thing.ceilingz - thing.height;
    }
    if thing.ceilingz - thing.floorz < thing.height {
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
/// `slidemo`, `tmxmove`, and `tmymove` must be set by the caller
/// ([`P_SlideMove`]) before this function is called.
///
/// # Panics
///
/// If `ld` is not a linedef of the loaded level, or a diagonal wall is hit
/// with no `slidemo` set.
pub fn P_HitSlideLine(world: &mut World, ld: LineId) {
    let World { level, scratch, .. } = world;
    let line = &level.lines[ld];
    let map = &mut scratch.map;
    if line.slopetype == ST_HORIZONTAL {
        map.tmymove = 0;
        return;
    }
    if line.slopetype == ST_VERTICAL {
        map.tmxmove = 0;
        return;
    }
    let (tmxmove, tmymove) = (map.tmxmove, map.tmymove);
    let (slidemo_x, slidemo_y) = {
        let mobjs = &world.mobjs;
        let slidemo = &mobjs[map.slidemo.expect("slide without a moving object")];
        (slidemo.x, slidemo.y)
    };
    let side = P_PointOnLineSide(level, slidemo_x, slidemo_y, ld);
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
    // `finecosine[angle]`: the cosine table is the sine table a quarter
    // turn on.
    let newlen = FixedMul(movelen, finesine[deltaangle + FINEANGLES / 4]);
    map.tmxmove = FixedMul(newlen, finesine[lineangle + FINEANGLES / 4]);
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
/// # Panics
///
/// If `intercept` does not hit a line of the loaded level, or no `slidemo`
/// is set.
pub fn PTR_SlideTraverse(world: &mut World, intercept: intercept_t) -> u32 {
    let InterceptHit::Line(li) = intercept.hit else {
        i_error!("PTR_SlideTraverse: not a line?");
    };
    let World { level, scratch, .. } = world;
    let PlayScratch { map, maputl, .. } = scratch;
    let (slidemo_x, slidemo_y, slidemo_z, slidemo_height) = {
        let mobjs = &world.mobjs;
        let slidemo = &mobjs[map.slidemo.expect("slide without a moving object")];
        (slidemo.x, slidemo.y, slidemo.z, slidemo.height)
    };
    {
        if level.lines[li].flags as i32 & LinedefFlag::TWOSIDED as i32 == 0 {
            if P_PointOnLineSide(level, slidemo_x, slidemo_y, li) != 0 {
                return 1;
            }
        } else {
            P_LineOpening(maputl, level, li);
            if maputl.openrange >= slidemo_height
                && maputl.opentop - slidemo_z >= slidemo_height
                && maputl.openbottom - slidemo_z <= 24 * FRACUNIT
            {
                return 1;
            }
        }
    }
    if intercept.frac < map.bestslidefrac {
        map.secondslidefrac = map.bestslidefrac;
        map.secondslideline = map.bestslideline;
        map.bestslidefrac = intercept.frac;
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
pub fn P_SlideMove(world: &mut World, mo_id: MobjId) {
    world.scratch.map.slidemo = Some(mo_id);
    let mut hitcount = 0;
    let slide_traverse = |world: &mut World, intercept| PTR_SlideTraverse(world, intercept) != 0;
    // The object's position and momentum, read afresh after every move
    // (each runs play code), as C reads them through its pointer.
    let position_and_momentum = |world: &World| {
        let mo = &world.mobjs[mo_id];
        (mo.x, mo.y, mo.momx, mo.momy)
    };
    // The stairstep fallback: try the pure-y move, then the pure-x one.
    let stairstep = |world: &mut World| {
        let (x, y, _, momy) = position_and_momentum(world);
        if P_TryMove(world, mo_id, x, y + momy) == 0 {
            let (x, y, momx, _) = position_and_momentum(world);
            P_TryMove(world, mo_id, x + momx, y);
        }
    };

    'retry: loop {
        hitcount += 1;
        if hitcount == 3 {
            stairstep(world);
            return;
        }

        let (x, y, momx, momy) = position_and_momentum(world);
        let radius = world.mobjs[mo_id].radius;
        let (leadx, trailx) = if momx > 0 {
            (x + radius, x - radius)
        } else {
            (x - radius, x + radius)
        };
        let (leady, traily) = if momy > 0 {
            (y + radius, y - radius)
        } else {
            (y - radius, y + radius)
        };

        world.scratch.map.bestslidefrac = FRACUNIT + 1;

        // The slide traversal runs no play code, so the momentum read above
        // holds for all three traces.
        P_PathTraverse(
            world,
            leadx,
            leady,
            leadx + momx,
            leady + momy,
            PT_ADDLINES,
            slide_traverse,
        );
        P_PathTraverse(
            world,
            trailx,
            leady,
            trailx + momx,
            leady + momy,
            PT_ADDLINES,
            slide_traverse,
        );
        P_PathTraverse(
            world,
            leadx,
            traily,
            leadx + momx,
            traily + momy,
            PT_ADDLINES,
            slide_traverse,
        );

        if world.scratch.map.bestslidefrac == FRACUNIT + 1 {
            stairstep(world);
            return;
        }

        world.scratch.map.bestslidefrac -= 0x800;
        let bestslidefrac = world.scratch.map.bestslidefrac;
        if bestslidefrac > 0 {
            let newx = FixedMul(momx, bestslidefrac);
            let newy = FixedMul(momy, bestslidefrac);
            if P_TryMove(world, mo_id, x + newx, y + newy) == 0 {
                continue 'retry;
            }
        }

        let (bestslidefrac, bestslideline) = {
            let map = &mut world.scratch.map;
            map.bestslidefrac = FRACUNIT - (map.bestslidefrac + 0x800);
            if map.bestslidefrac > FRACUNIT {
                map.bestslidefrac = FRACUNIT;
            }
            (map.bestslidefrac, map.bestslideline)
        };
        if bestslidefrac <= 0 {
            return;
        }
        // The move above may have run play code; the momentum is re-read.
        let (_, _, momx, momy) = position_and_momentum(world);
        {
            let map = &mut world.scratch.map;
            map.tmxmove = FixedMul(momx, bestslidefrac);
            map.tmymove = FixedMul(momy, bestslidefrac);
        }
        // A traversal that brought `bestslidefrac` below `FRACUNIT + 1` set
        // the line with it.
        P_HitSlideLine(world, bestslideline.expect("slide hit without a line"));
        let (tmxmove, tmymove) = (world.scratch.map.tmxmove, world.scratch.map.tmymove);
        let (x, y) = {
            let mobjs = &mut world.mobjs;
            let mo = &mut mobjs[mo_id];
            mo.momx = tmxmove;
            mo.momy = tmymove;
            (mo.x, mo.y)
        };
        if P_TryMove(world, mo_id, x + tmxmove, y + tmymove) == 0 {
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
/// `shootthing`, `shootz`, `attackrange`, `topslope`, and `bottomslope`
/// must be set before the traversal begins.
///
/// # Panics
///
/// If `intercept` does not hit a line or map object of the loaded level.
pub fn PTR_AimTraverse(world: &mut World, intercept: intercept_t) -> u32 {
    let (shootthing, shootz, attackrange) = {
        let map = &world.scratch.map;
        (map.shootthing, map.shootz, map.attackrange)
    };
    let th_id = match intercept.hit {
        InterceptHit::Thing(thing) => thing,
        InterceptHit::Line(li) => {
            let World { level, scratch, .. } = world;
            let line = &level.lines[li];
            if line.flags as i32 & LinedefFlag::TWOSIDED as i32 == 0 {
                return 0;
            }
            let PlayScratch { maputl, sight, .. } = scratch;
            P_LineOpening(maputl, level, li);
            let (opentop, openbottom) = (maputl.opentop, maputl.openbottom);
            if openbottom >= opentop {
                return 0;
            }
            let dist = FixedMul(attackrange, intercept.frac);
            let front = &level.sectors[line
                .frontsector
                .expect("two-sided line without front sector")];
            let back = line.backsector.map(|back| &level.sectors[back]);
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
    let (th_flags, th_z, th_height) = {
        let mobjs = &world.mobjs;
        let th = &mobjs[th_id];
        (th.flags, th.z, th.height)
    };
    let (topslope, bottomslope) = (
        world.scratch.sight.topslope,
        world.scratch.sight.bottomslope,
    );
    if th_flags & MF_SHOOTABLE == 0 {
        return 1;
    }
    let dist = FixedMul(attackrange, intercept.frac);
    let thingtopslope = FixedDiv(th_z + th_height - shootz, dist);
    if thingtopslope < bottomslope {
        return 1;
    }
    let thingbottomslope = FixedDiv(th_z - shootz, dist);
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
    let map = &mut world.scratch.map;
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
/// Reads `shootthing`, `shootz`, `attackrange`, `aimslope`, and `la_damage`,
/// which are set before the traversal.
pub fn PTR_ShootTraverse(world: &mut World, intercept: intercept_t) -> u32 {
    if let InterceptHit::Line(li) = intercept.hit {
        if world.level.lines[li].special != 0 {
            let shootthing = world
                .scratch
                .map
                .shootthing
                .expect("shot without a shooter");
            P_ShootSpecialLine(world, shootthing, li);
        }
        // The special can change the line (a switch clears it) and move
        // sectors, so the line is read afresh, as C does.
        let line = world.level.lines[li];
        if line.flags as i32 & LinedefFlag::TWOSIDED as i32 != 0 {
            let maputl = &mut world.scratch.maputl;
            P_LineOpening(maputl, &world.level, li);
            let (opentop, openbottom) = (maputl.opentop, maputl.openbottom);
            let (shootz, attackrange, aimslope) = {
                let map = &world.scratch.map;
                (map.shootz, map.attackrange, map.aimslope)
            };
            let dist = FixedMul(attackrange, intercept.frac);
            match line.backsector {
                None => {
                    let slope = FixedDiv(openbottom - shootz, dist);
                    if slope > aimslope {
                        goto_hitline(world, li, intercept);
                        return 0;
                    }
                    let slope = FixedDiv(opentop - shootz, dist);
                    if slope < aimslope {
                        goto_hitline(world, li, intercept);
                        return 0;
                    }
                }
                Some(back) => {
                    let (front, back) = {
                        let level = &world.level;
                        let front = line
                            .frontsector
                            .expect("two-sided line without front sector");
                        (level.sectors[front], level.sectors[back])
                    };
                    if front.floorheight != back.floorheight {
                        let slope = FixedDiv(openbottom - shootz, dist);
                        if slope > aimslope {
                            goto_hitline(world, li, intercept);
                            return 0;
                        }
                    }
                    if front.ceilingheight != back.ceilingheight {
                        let slope = FixedDiv(opentop - shootz, dist);
                        if slope < aimslope {
                            goto_hitline(world, li, intercept);
                            return 0;
                        }
                    }
                }
            }
            return 1;
        }
        goto_hitline(world, li, intercept);
        return 0;
    }

    let InterceptHit::Thing(th_id) = intercept.hit else {
        unreachable!("lines are handled above");
    };
    let (shootthing, shootz, attackrange, aimslope, la_damage) = {
        let map = &world.scratch.map;
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
    let (th_flags, th_z, th_height) = {
        let mobjs = &world.mobjs;
        let th = &mobjs[th_id];
        (th.flags, th.z, th.height)
    };
    if th_flags & MF_SHOOTABLE == 0 {
        return 1;
    }
    let dist = FixedMul(attackrange, intercept.frac);
    let thingtopslope = FixedDiv(th_z + th_height - shootz, dist);
    if thingtopslope < aimslope {
        return 1;
    }
    let thingbottomslope = FixedDiv(th_z - shootz, dist);
    if thingbottomslope > aimslope {
        return 1;
    }

    let frac = intercept.frac - FixedDiv(10 * FRACUNIT, attackrange);
    let trace = world.scratch.maputl.trace;
    let x = trace.x + FixedMul(trace.dx, frac);
    let y = trace.y + FixedMul(trace.dy, frac);
    let z = shootz + FixedMul(aimslope, FixedMul(frac, attackrange));
    if th_flags & MF_NOBLOOD != 0 {
        P_SpawnPuff(world, x, y, z);
    } else {
        P_SpawnBlood(world, x, y, z, la_damage);
    }
    // Spawning sets the new object's state, so re-read the shot as C does.
    let (shootthing, la_damage) = (world.scratch.map.shootthing, world.scratch.map.la_damage);
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
/// `intercept` is the intercept that triggered this call; `shootz`,
/// `aimslope`, and `attackrange` are those of the current shot.
fn goto_hitline(world: &mut World, li: LineId, intercept: intercept_t) {
    let (shootz, attackrange, aimslope) = {
        let map = &world.scratch.map;
        (map.shootz, map.attackrange, map.aimslope)
    };
    let frac = intercept.frac - FixedDiv(4 * FRACUNIT, attackrange);
    let trace = world.scratch.maputl.trace;
    let x = trace.x + FixedMul(trace.dx, frac);
    let y = trace.y + FixedMul(trace.dy, frac);
    let z = shootz + FixedMul(aimslope, FixedMul(frac, attackrange));
    let skyflatnum = RENDERER.borrow().sky.skyflatnum;
    {
        let level = &world.level;
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
/// `t1` names the shooter (C substitutes a dummy for null via
/// [`P_SubstNullMobj`]; the callers always pass one).
///
/// # Panics
///
/// If no level is loaded.
pub fn P_AimLineAttack(world: &mut World, t1: MobjId, angle: u32, distance: fixed_t) -> fixed_t {
    let t1_id = P_SubstNullMobj(world, Some(t1));
    let (t1_x, t1_y, t1_z, t1_height) = {
        let mobjs = &world.mobjs;
        let t1 = &mobjs[t1_id];
        (t1.x, t1.y, t1.z, t1.height)
    };
    let angle = (angle >> ANGLETOFINESHIFT) as usize;
    // `finecosine[angle]`: the cosine table is the sine table a quarter
    // turn on.
    let x2 = t1_x + (distance >> FRACBITS) * finesine[angle + FINEANGLES / 4];
    let y2 = t1_y + (distance >> FRACBITS) * finesine[angle];
    {
        let map = &mut world.scratch.map;
        map.shootthing = Some(t1_id);
        map.shootz = t1_z + (t1_height >> 1) + 8 * FRACUNIT;
        map.attackrange = distance;
        map.linetarget = None;
    }
    {
        let sight = &mut world.scratch.sight;
        sight.topslope = 100 * FRACUNIT / 160;
        sight.bottomslope = -(100 * FRACUNIT / 160);
    }
    P_PathTraverse(
        world,
        t1_x,
        t1_y,
        x2,
        y2,
        PT_ADDLINES | PT_ADDTHINGS,
        |world, intercept| PTR_AimTraverse(world, intercept) != 0,
    );
    let map = &world.scratch.map;
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
pub fn P_LineAttack(
    world: &mut World,
    t1_id: MobjId,
    angle: u32,
    distance: fixed_t,
    slope: fixed_t,
    damage: i32,
) {
    let (t1_x, t1_y, t1_z, t1_height) = {
        let mobjs = &world.mobjs;
        let t1 = &mobjs[t1_id];
        (t1.x, t1.y, t1.z, t1.height)
    };
    let angle = (angle >> ANGLETOFINESHIFT) as usize;
    // `finecosine[angle]`: the cosine table is the sine table a quarter
    // turn on.
    let x2 = t1_x + (distance >> FRACBITS) * finesine[angle + FINEANGLES / 4];
    let y2 = t1_y + (distance >> FRACBITS) * finesine[angle];
    {
        let map = &mut world.scratch.map;
        map.shootthing = Some(t1_id);
        map.la_damage = damage;
        map.shootz = t1_z + (t1_height >> 1) + 8 * FRACUNIT;
        map.attackrange = distance;
        map.aimslope = slope;
    }
    P_PathTraverse(
        world,
        t1_x,
        t1_y,
        x2,
        y2,
        PT_ADDLINES | PT_ADDTHINGS,
        // The shot parameters are set above.
        |world, intercept| PTR_ShootTraverse(world, intercept) != 0,
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
/// Reads `usething`, which [`P_UseLines`] sets; a blocked use runs
/// [`S_StartSound`].
pub fn PTR_UseTraverse(world: &mut World, intercept: intercept_t) -> u32 {
    let usething_id = world.scratch.map.usething.expect("use without a user");
    let InterceptHit::Line(line) = intercept.hit else {
        unreachable!("the use traversal collects only lines");
    };
    let side = {
        if world.level.lines[line].special == 0 {
            P_LineOpening(&mut world.scratch.maputl, &world.level, line);
            if world.scratch.maputl.openrange <= 0 {
                S_StartSound(world, usething_id, Sfx::Noway as i32);
                return 0;
            }
            return 1;
        }
        let mut side = 0;
        let (usething_x, usething_y) = {
            let mobjs = &world.mobjs;
            (mobjs[usething_id].x, mobjs[usething_id].y)
        };
        if P_PointOnLineSide(&world.level, usething_x, usething_y, line) == 1 {
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
pub fn P_UseLines(world: &mut World, player: PlayerId) {
    let mo_id = world.players[player].mo();
    world.scratch.map.usething = Some(mo_id);
    let (angle, x1, y1) = {
        let mobjs = &world.mobjs;
        let mo = &mobjs[mo_id];
        ((mo.angle >> ANGLETOFINESHIFT) as usize, mo.x, mo.y)
    };
    // `finecosine[angle]`: the cosine table is the sine table a quarter
    // turn on.
    let x2 = x1 + (USERANGE >> FRACBITS) * finesine[angle + FINEANGLES / 4];
    let y2 = y1 + (USERANGE >> FRACBITS) * finesine[angle];
    // The traversal collects only lines, and `usething` is set above.
    P_PathTraverse(world, x1, y1, x2, y2, PT_ADDLINES, |world, intercept| {
        PTR_UseTraverse(world, intercept) != 0
    });
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
/// Reads `bombspot`, `bombsource`, and `bombdamage`, which
/// [`P_RadiusAttack`] sets before the iteration; a hit runs
/// [`P_DamageMobj`].
pub fn PIT_RadiusAttack(world: &mut World, thing_id: MobjId) -> u32 {
    let (thing_x, thing_y, thing_radius) = {
        let mobjs = &world.mobjs;
        let thing = &mobjs[thing_id];
        if thing.flags & MF_SHOOTABLE == 0 {
            return 1;
        }
        if thing.kind == MT_CYBORG || thing.kind == MT_SPIDER {
            return 1;
        }
        (thing.x, thing.y, thing.radius)
    };
    // The sight check below does not touch this state, so one read serves
    // the whole callback.
    let (bombspot_id, bombsource, bombdamage) = {
        let map = &world.scratch.map;
        (
            map.bombspot.expect("radius attack without a spot"),
            map.bombsource,
            map.bombdamage,
        )
    };
    let (bombspot_x, bombspot_y) = {
        let mobjs = &world.mobjs;
        (mobjs[bombspot_id].x, mobjs[bombspot_id].y)
    };
    let dx = (thing_x - bombspot_x).wrapping_abs();
    let dy = (thing_y - bombspot_y).wrapping_abs();
    let mut dist = if dx > dy { dx } else { dy };
    dist = (dist - thing_radius) >> FRACBITS;
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
/// It may be `None` or stale (C passes the exploding object's `target`,
/// which may be gone). The blast runs [`PIT_RadiusAttack`].
pub fn P_RadiusAttack(world: &mut World, spot_id: MobjId, source: Option<MobjId>, damage: i32) {
    let (spot_x, spot_y) = {
        let mobjs = &world.mobjs;
        (mobjs[spot_id].x, mobjs[spot_id].y)
    };
    let (bmaporgx, bmaporgy) = world.level.blockmap_origin();
    let dist = (damage + MAXRADIUS) << FRACBITS;
    let yh = (spot_y + dist - bmaporgy) >> MAPBLOCKSHIFT;
    let yl = (spot_y - dist - bmaporgy) >> MAPBLOCKSHIFT;
    let xh = (spot_x + dist - bmaporgx) >> MAPBLOCKSHIFT;
    let xl = (spot_x - dist - bmaporgx) >> MAPBLOCKSHIFT;
    {
        let map = &mut world.scratch.map;
        map.bombspot = Some(spot_id);
        map.bombsource = source;
        map.bombdamage = damage;
    }
    for y in yl..=yh {
        for x in xl..=xh {
            // The blast parameters are set above.
            P_BlockThingsIterator(world, x, y, |world, thing| {
                PIT_RadiusAttack(world, thing) != 0
            });
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
/// Reads `crushchange`, which [`P_ChangeSector`] sets before the iteration;
/// the crush runs state changes, removal and [`P_DamageMobj`].
pub fn PIT_ChangeSector(world: &mut World, thing_id: MobjId) -> u32 {
    if P_ThingHeightClip(world, thing_id) != 0 {
        return 1;
    }
    let (health, flags) = {
        let mobjs = &world.mobjs;
        (mobjs[thing_id].health, mobjs[thing_id].flags)
    };
    if health <= 0 {
        P_SetMobjState(world, thing_id, S_GIBS);
        let mobjs = &mut world.mobjs;
        let thing = &mut mobjs[thing_id];
        thing.flags &= !MF_SOLID;
        thing.height = 0;
        thing.radius = 0;
        return 1;
    }
    if flags & MF_DROPPED != 0 {
        P_RemoveMobj(world, thing_id);
        return 1;
    }
    if flags & MF_SHOOTABLE == 0 {
        return 1;
    }
    world.scratch.map.nofit = 1;
    let crushchange = world.scratch.map.crushchange;
    if crushchange != 0 && world.leveltime & 3 == 0 {
        P_DamageMobj(world, thing_id, None, None, 10);
        // The damage can move the thing; the blood goes where it is now.
        let (x, y, z) = {
            let mobjs = &world.mobjs;
            let thing = &mobjs[thing_id];
            (thing.x, thing.y, thing.z + thing.height / 2)
        };
        let mo = P_SpawnMobj(world, x, y, z, MT_BLOOD);
        let momx = (P_Random(world) - P_Random(world)) << 12;
        world.mobjs[mo].momx = momx;
        let momy = (P_Random(world) - P_Random(world)) << 12;
        world.mobjs[mo].momy = momy;
    }
    1
}

/// Propagate a floor or ceiling height change in `sector` to all nearby things.
///
/// Re-checks height constraints for every thing in the blockmap cells that
/// overlap `sector->blockbox`. Returns non-zero if any thing no longer fits
/// (i.e. `nofit` was set). If `crunch` is zero and this returns non-zero,
/// the caller should revert the sector height and call this again. The
/// things that no longer fit run [`PIT_ChangeSector`].
pub fn P_ChangeSector(world: &mut World, sector: SectorId, crunch: i32) -> i32 {
    world.scratch.map.nofit = 0;
    world.scratch.map.crushchange = crunch;
    let blockbox = world.level.sectors[sector].blockbox;
    for x in blockbox[BBox::LEFT]..=blockbox[BBox::RIGHT] {
        for y in blockbox[BBox::BOTTOM]..=blockbox[BBox::TOP] {
            // `crushchange` is set above.
            P_BlockThingsIterator(world, x, y, |world, thing| {
                PIT_ChangeSector(world, thing) != 0
            });
        }
    }
    world.scratch.map.nofit
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
