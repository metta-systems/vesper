//! Rust port of `vendor/doomgeneric/p_mobj.c`.
//!
//! Map object (mobj) lifecycle: creation, movement, state-machine updates,
//! spawning from map data, and item-respawn in deathmatch mode.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use {
    crate::{engine::sound::info::Sfx, i_error},
    std::ptr,
};

use crate::engine::{
    arena::Handle,
    doom::player::{CF_NOMOMENTUM, MAXPLAYERS, PlayerMessage, PlayerT},
    hud::stuff::HU_Start,
    interface::timer::TICRATE,
    map::SubsectorId,
    math::{
        fixed::{FRACBITS, FRACUNIT, FixedMul},
        tables::{ANG45, ANGLETOFINESHIFT, finecosine, finesine},
    },
    misc::random::P_Random,
    play::{
        info::*,
        maputl::{P_AproxDistance, P_SetThingPosition, P_UnsetThingPosition},
        pspr::P_SetupPsprites,
        setup::LEVEL,
        tick::{P_AddThinker, P_RemoveThinker, ThinkerId, ThinkerKind, new_thinker, thinker_t},
    },
    render_3d::main::{R_PointInSubsector, R_PointToAngle2},
    sound::{S_StartSound, S_StopSound},
    wad::format::mapthing_t,
};

/// Momentum magnitude below which an object's XY velocity is snapped to zero.
pub const STOPSPEED: i32 = 0x1000;
/// Multiplicative XY friction applied each tic when an object is on the floor
/// (0xe800 / 0x10000 ≈ 0.906).
pub const FRICTION: i32 = 0xe800;
/// Capacity of the item-respawn circular queue (`ITEMQUESIZE` in
/// `p_local.h`).
pub const ITEMQUESIZE: usize = 128;
/// Maximum XY momentum per tic; momentum is clamped to this before integration.
const MAXMOVE: i32 = 30 * FRACUNIT;
/// Gravitational acceleration applied to falling objects each tic (1 fixed unit).
const GRAVITY: i32 = FRACUNIT;
/// Vertical speed at which floating monsters adjust their altitude each tic.
const FLOATSPEED: i32 = FRACUNIT * 4;
/// Sentinel Z value meaning "place on the floor of the current sector".
const ONFLOORZ: i32 = i32::MIN;
/// Sentinel Z value meaning "place on the ceiling of the current sector".
const ONCEILINGZ: i32 = i32::MAX;
/// Default eye height above the floor for a live player (41 fixed units).
const VIEWHEIGHT: i32 = 41 * FRACUNIT;
/// Maximum reach for melee contact checks (64 map units in fixed-point).
const MELEERANGE: i32 = 64 * FRACUNIT;

/// Thing-flag option bit: the thing was placed in ambush mode in the map editor.
const MTF_AMBUSH: i32 = 8;

/// Player state: alive and playing.
const PST_LIVE: i32 = 0;
/// Player state: needs to respawn (set after death, cleared by `P_SpawnPlayer`).
const PST_REBORN: i32 = 2;

/// Game-version threshold at or above which the Lost Soul floor-bounce bug is
/// corrected (corresponds to Ultimate Doom / `exe_ultimate`).
const exe_ultimate: i32 = 6;

use crate::engine::{
    doom::{main::nomonsters, stat::gameversion},
    game::{
        G_PlayerReborn, GAME, consoleplayer, deathmatch, gameskill, netgame, player, playeringame,
        respawnmonsters,
    },
    global::Global,
    play::{
        map::{
            P_AimLineAttack, P_CheckPosition, P_SlideMove, P_TryMove, attackrange, ceilingline,
            linetarget,
        },
        setup::{SPAWN_SPOTS, deathmatch_p, deathmatchstart, playerstart},
        tick::leveltime,
    },
    render_3d::sky::SKY,
    status::stuff::ST_Start,
};

/// A map object in the mobj arena (C's `mobj_t *`).
///
/// Every reference between map objects, and from the rest of the engine to
/// one, is a handle; [`Handle::ptr`] resolves it. A handle to a removed
/// object goes stale once the thinker list frees it, and still reads the
/// object's last data, as C's dangling pointers do (see
/// [`crate::engine::arena`]).
pub type MobjId = Handle<mobj_t>;

/// Pointer to the map object `mobj` names, or null for `None`: C's nullable
/// `mobj_t *`, for code that reads an optional object through its pointer.
#[inline]
#[track_caller]
pub fn mobj_or_null(mobj: Option<MobjId>) -> *mut mobj_t {
    match mobj {
        Some(mobj) => mobj.ptr(),
        None => std::ptr::null_mut(),
    }
}

/// A map object (thing) that lives in the world and can think each tic
/// (`mobj_t` in `p_mobj.h`).
///
/// It lives in the [`MOBJ_ARENA`](crate::engine::play::tick::MOBJ_ARENA)
/// and is listed in the thinker list as
/// [`ThinkerId::Mobj`].
#[derive(Clone, Copy)]
pub struct mobj_t {
    /// Thinker state: what the object does each tic.
    pub thinker: thinker_t,
    /// Map X position in fixed-point units.
    pub x: i32,
    /// Map Y position in fixed-point units.
    pub y: i32,
    /// Map Z position (height above the floor) in fixed-point units.
    pub z: i32,
    /// Next thing in this sector's sprite-order linked list.
    pub snext: Option<MobjId>,
    /// Previous thing in this sector's sprite-order linked list.
    pub sprev: Option<MobjId>,
    /// Facing angle in binary radians (BAMs); 0 = East.
    pub angle: u32,
    /// Current sprite number.
    pub sprite: i32,
    /// Current animation frame index.
    pub frame: i32,
    /// Next thing in the blockmap block's linked list.
    pub bnext: Option<MobjId>,
    /// Previous thing in the blockmap block's linked list.
    pub bprev: Option<MobjId>,
    /// The sub-sector this thing currently occupies.
    pub subsector: SubsectorId,
    /// Floor height at the thing's current position.
    pub floorz: i32,
    /// Ceiling height at the thing's current position.
    pub ceilingz: i32,
    /// Collision radius in fixed-point units.
    pub radius: i32,
    /// Collision height in fixed-point units.
    pub height: i32,
    /// X momentum applied each tic.
    pub momx: i32,
    /// Y momentum applied each tic.
    pub momy: i32,
    /// Z momentum applied each tic.
    pub momz: i32,
    /// Used to avoid processing the same thing twice in a single traversal.
    pub validcount: i32,
    /// Thing type index into the `mobjinfo` table (`type` in C).
    pub kind: i32,
    /// Pointer to the thing's type info record.
    pub info: *mut MobjInfo,
    /// Remaining tics in the current animation frame.
    pub tics: i32,
    /// Pointer to the current animation frame state.
    pub state: *mut State,
    /// Bit-field of `MF_*` flags controlling collision, rendering, AI, etc.
    pub flags: i32,
    /// Current hit points.
    pub health: i32,
    /// Current movement direction (0–7 compass directions, or `DI_NODIR`).
    pub movedir: i32,
    /// Tics remaining before the thing attempts a new movement decision.
    pub movecount: i32,
    /// Primary AI target (usually the last thing to attack this one).
    pub target: Option<MobjId>,
    /// Tics the thing must wait before it can attack again.
    pub reactiontime: i32,
    /// Decremented each tic while pursuing a target; reset on target change.
    pub threshold: i32,
    /// Non-null if this thing is a player; points to the `PlayerT` record.
    pub player: *mut PlayerT,
    /// Index used to cycle through potential targets during `P_LookForPlayers`.
    pub lastlook: i32,
    /// The WAD spawn point from which this thing was created.
    pub spawnpoint: mapthing_t,
    /// Tracer target used by homing missiles (cyberdemon rockets, etc.).
    pub tracer: Option<MobjId>,
}

/// The item-respawn queue and the null-object stand-in (`p_mobj.c` globals).
pub struct MobjState {
    /// Circular queue of map-thing spawn records for items that need to respawn.
    ///
    /// The queue is indexed by `iquehead` (write) and `iquetail` (read), both
    /// modulo `ITEMQUESIZE`.
    pub itemrespawnque: [mapthing_t; ITEMQUESIZE],
    /// Level-time stamps recording when each item in `itemrespawnque` was removed.
    ///
    /// An item is eligible to respawn once `leveltime - itemrespawntime[iquetail]`
    /// exceeds 30 * `TICRATE`.
    pub itemrespawntime: [i32; ITEMQUESIZE],
    /// Write index (head) of the item-respawn circular queue.
    pub iquehead: i32,
    /// Read index (tail) of the item-respawn circular queue.
    pub iquetail: i32,
    /// The zeroed stand-in `P_SubstNullMobj` returns for a null object
    /// (static local `dummy_mobj` in C). It is a slot of the mobj arena
    /// that is never listed as a thinker, allocated on first use in a level.
    dummy_mobj: Option<MobjId>,
}

impl MobjState {
    /// An empty respawn queue.
    pub const fn new() -> Self {
        Self {
            itemrespawnque: [mapthing_t::ZERO; ITEMQUESIZE],
            itemrespawntime: [0; ITEMQUESIZE],
            iquehead: 0,
            iquetail: 0,
            dummy_mobj: None,
        }
    }
}

impl Default for MobjState {
    fn default() -> Self {
        Self::new()
    }
}

/// The item-respawn queue.
pub static MOBJ: Global<MobjState> = Global::new(MobjState::new());

/// Transition `mobj` to state `state`, running zero-tic chain states
/// immediately.  Returns `1` if the mobj is still alive after the transition,
/// `0` if it removed itself (`S_NULL`).
///
/// Mirrors `P_SetMobjState` in `p_mobj.c`.
///
/// # Safety
///
/// `mobj_id` must name an initialised map object of the loaded level.
/// `state` must be a valid state-table index or `S_NULL`.
pub unsafe fn P_SetMobjState(mobj_id: MobjId, state: i32) -> i32 {
    let mobj = &mut *mobj_id.ptr();
    let mut state = state;
    loop {
        if state == S_NULL {
            mobj.state = ptr::null_mut();
            P_RemoveMobj(mobj_id);
            return 0;
        }
        let st = state_ptr(state as usize);
        mobj.state = st;
        mobj.tics = (*st).tics;
        mobj.sprite = (*st).sprite;
        mobj.frame = (*st).frame;
        match (*st).action {
            StateAction::None => {}
            StateAction::Mobj(action) => action(mobj_id),
            StateAction::Weapon(_) => {
                unreachable!("P_SetMobjState: state {state} has a weapon action")
            }
        }
        state = (*st).nextstate;
        if mobj.tics != 0 {
            break;
        }
    }
    1
}

/// Detonate a missile: zero its momentum, transition to its death state,
/// randomise the initial tic count slightly, clear `MF_MISSILE`, and play the
/// death sound.
///
/// # Safety
///
/// `mo_id` must name a map object of the loaded level with `MF_MISSILE` set.
pub unsafe fn P_ExplodeMissile(mo_id: MobjId) {
    let mo = &mut *mo_id.ptr();
    let info = mo.info;
    mo.momx = 0;
    mo.momy = 0;
    mo.momz = 0;
    P_SetMobjState(mo_id, (*info).deathstate);
    mo.tics -= P_Random() & 3;
    if mo.tics < 1 {
        mo.tics = 1;
    }
    mo.flags &= !MF_MISSILE;
    if (*info).deathsound != Sfx::None {
        S_StartSound(mo_id, (*info).deathsound as i32);
    }
}

/// Apply one tic of XY momentum to `mo`, sub-stepping when the move exceeds
/// `MAXMOVE / 2`.  Handles player sliding, missile explosion against walls
/// and the sky, and floor friction / stopping.
///
/// # Safety
///
/// `mo_id` must name a map object that is part of the active thinker list.
pub unsafe fn P_XYMovement(mo_id: MobjId) {
    let mo = &mut *mo_id.ptr();
    let mut ptryx: i32;
    let mut ptryy: i32;
    let player = mo.player;
    let mut xmove: i32;
    let mut ymove: i32;

    if mo.momx == 0 && mo.momy == 0 {
        if mo.flags & MF_SKULLFLY != 0 {
            let info = mo.info;
            mo.flags &= !MF_SKULLFLY;
            mo.momx = 0;
            mo.momy = 0;
            mo.momz = 0;
            P_SetMobjState(mo_id, (*info).spawnstate);
        }
        return;
    }

    mo.momx = mo.momx.clamp(-MAXMOVE, MAXMOVE);
    mo.momy = mo.momy.clamp(-MAXMOVE, MAXMOVE);

    xmove = mo.momx;
    ymove = mo.momy;

    loop {
        if xmove > MAXMOVE / 2 || ymove > MAXMOVE / 2 {
            ptryx = mo.x + xmove / 2;
            ptryy = mo.y + ymove / 2;
            xmove >>= 1;
            ymove >>= 1;
        } else {
            ptryx = mo.x + xmove;
            ptryy = mo.y + ymove;
            xmove = 0;
            ymove = 0;
        }

        if P_TryMove(mo_id, ptryx, ptryy) == 0 {
            if !mo.player.is_null() {
                P_SlideMove(mo_id);
            } else if mo.flags & MF_MISSILE != 0 {
                if let Some(line) = ceilingline() {
                    let skyflatnum = SKY.borrow().skyflatnum as i16;
                    let hits_sky = {
                        let level = LEVEL.borrow();
                        level.lines[line]
                            .backsector
                            .is_some_and(|back| level.sectors[back].ceilingpic == skyflatnum)
                    };
                    if hits_sky {
                        P_RemoveMobj(mo_id);
                        return;
                    }
                }
                P_ExplodeMissile(mo_id);
            } else {
                mo.momx = 0;
                mo.momy = 0;
            }
        }
        if xmove == 0 && ymove == 0 {
            break;
        }
    }

    if !player.is_null() && (*player).cheats & CF_NOMOMENTUM != 0 {
        mo.momx = 0;
        mo.momy = 0;
        return;
    }

    if mo.flags & (MF_MISSILE | MF_SKULLFLY) != 0 {
        return;
    }

    if mo.z > mo.floorz {
        return;
    }

    if mo.flags & MF_CORPSE != 0
        && (mo.momx > FRACUNIT / 4
            || mo.momx < -FRACUNIT / 4
            || mo.momy > FRACUNIT / 4
            || mo.momy < -FRACUNIT / 4)
        && mo.floorz != LEVEL.borrow().subsector_sector(mo.subsector).floorheight
    {
        return;
    }

    if mo.momx > -STOPSPEED
        && mo.momx < STOPSPEED
        && mo.momy > -STOPSPEED
        && mo.momy < STOPSPEED
        && (player.is_null() || ((*player).cmd.forwardmove == 0 && (*player).cmd.sidemove == 0))
    {
        if !player.is_null() {
            let mobj_state = mo.state;
            let states_ptr = state_ptr(0) as *const State;
            let state_idx =
                (mobj_state as usize - states_ptr as usize) / std::mem::size_of::<State>();
            let run_offset = state_idx as i32 - S_PLAY_RUN1;
            if (0..4).contains(&run_offset) {
                P_SetMobjState(mo_id, S_PLAY);
            }
        }
        mo.momx = 0;
        mo.momy = 0;
    } else {
        mo.momx = FixedMul(mo.momx, FRICTION);
        mo.momy = FixedMul(mo.momy, FRICTION);
    }
}

/// Apply one tic of Z (vertical) movement to `mo`.
///
/// Handles player view-height smoothing on step-ups, floating-monster altitude
/// tracking, gravity, floor and ceiling collisions, and Lost Soul bounce
/// (with version-correct bug emulation for the original v1.9 desync).
///
/// # Safety
///
/// `mo_id` must name a map object that is part of the active thinker list.
pub unsafe fn P_ZMovement(mo_id: MobjId) {
    let mo = &mut *mo_id.ptr();
    let dist: i32;
    let delta: i32;

    if !mo.player.is_null() && mo.z < mo.floorz {
        let player = mo.player;
        (*player).viewheight -= mo.floorz - mo.z;
        (*player).deltaviewheight = (VIEWHEIGHT - (*player).viewheight) >> 3;
    }

    mo.z += mo.momz;

    if mo.flags & MF_FLOAT != 0
        && let Some(target) = mo.target
        && mo.flags & MF_SKULLFLY == 0
        && mo.flags & MF_INFLOAT == 0
    {
        let target = target.ptr();
        dist = P_AproxDistance(mo.x - (*target).x, mo.y - (*target).y);
        delta = ((*target).z + (mo.height >> 1)) - mo.z;
        if delta < 0 && dist < -(delta * 3) {
            mo.z -= FLOATSPEED;
        } else if delta > 0 && dist < delta * 3 {
            mo.z += FLOATSPEED;
        }
    }

    if mo.z <= mo.floorz {
        let correct_lost_soul_bounce = gameversion() >= exe_ultimate;
        if correct_lost_soul_bounce && mo.flags & MF_SKULLFLY != 0 {
            mo.momz = -mo.momz;
        }
        if mo.momz < 0 {
            if !mo.player.is_null() && mo.momz < -GRAVITY * 8 {
                let player = mo.player;
                (*player).deltaviewheight = mo.momz >> 3;
                S_StartSound(mo_id, Sfx::Oof as i32);
            }
            mo.momz = 0;
        }
        mo.z = mo.floorz;
        if !correct_lost_soul_bounce && mo.flags & MF_SKULLFLY != 0 {
            mo.momz = -mo.momz;
        }
        if mo.flags & MF_MISSILE != 0 && mo.flags & MF_NOCLIP == 0 {
            P_ExplodeMissile(mo_id);
            return;
        }
    } else if mo.flags & MF_NOGRAVITY == 0 {
        if mo.momz == 0 {
            mo.momz = -GRAVITY * 2;
        } else {
            mo.momz -= GRAVITY;
        }
    }

    if mo.z + mo.height > mo.ceilingz {
        if mo.momz > 0 {
            mo.momz = 0;
        }
        mo.z = mo.ceilingz - mo.height;
        if mo.flags & MF_SKULLFLY != 0 {
            mo.momz = -mo.momz;
        }
        if mo.flags & MF_MISSILE != 0 && mo.flags & MF_NOCLIP == 0 {
            P_ExplodeMissile(mo_id);
        }
    }
}

/// Respawn a Nightmare-mode monster at its original spawn point.
///
/// Checks that the spawn point is unobstructed, spawns teleport fog at the
/// old and new positions, spawns a fresh copy of the monster, and removes the
/// old corpse.
///
/// # Safety
///
/// `mobj_id` must name a map object that has `MF_COUNTKILL` set and whose
/// `spawnpoint` field is valid.
pub unsafe fn P_NightmareRespawn(mobj_id: MobjId) {
    let mobj = &mut *mobj_id.ptr();
    let x = (mobj.spawnpoint.x.get() as i32) << FRACBITS;
    let y = (mobj.spawnpoint.y.get() as i32) << FRACBITS;
    if P_CheckPosition(mobj_id, x, y) == 0 {
        return;
    }

    let floorheight = LEVEL.borrow().subsector_sector(mobj.subsector).floorheight;
    let fog = P_SpawnMobj(mobj.x, mobj.y, floorheight, MT_TFOG);
    S_StartSound(fog, Sfx::Telept as i32);

    let ss = R_PointInSubsector(x, y);
    let floorheight = LEVEL.borrow().subsector_sector(ss).floorheight;
    let fog = P_SpawnMobj(x, y, floorheight, MT_TFOG);
    S_StartSound(fog, Sfx::Telept as i32);

    let mthing = &mobj.spawnpoint;
    let info = mobj.info;
    let z = if (*info).flags & MF_SPAWNCEILING != 0 {
        ONCEILINGZ
    } else {
        ONFLOORZ
    };

    let respawned = &mut *P_SpawnMobj(x, y, z, mobj.kind).ptr();
    respawned.spawnpoint = mobj.spawnpoint;
    respawned.angle = ANG45.wrapping_mul((mthing.angle.get() as u32) / 45);
    if mthing.options.get() & MTF_AMBUSH as i16 != 0 {
        respawned.flags |= MF_AMBUSH;
    }
    respawned.reactiontime = 18;
    P_RemoveMobj(mobj_id);
}

/// Per-tic thinker for every active map object.
///
/// Runs XY and Z movement, advances the state machine, and initiates
/// Nightmare-mode respawn for eligible dead monsters.  Detects mid-tick
/// removal via [`ThinkerKind::Removed`].
///
/// # Safety
///
/// `mobj_id` must name a map object that is currently in the thinker list.
pub unsafe fn P_MobjThinker(mobj_id: MobjId) {
    let mobj = &mut *mobj_id.ptr();
    if mobj.momx != 0 || mobj.momy != 0 || mobj.flags & MF_SKULLFLY != 0 {
        P_XYMovement(mobj_id);
        if mobj.thinker.kind.is_removed() {
            return;
        }
    }
    if mobj.z != mobj.floorz || mobj.momz != 0 {
        P_ZMovement(mobj_id);
        if mobj.thinker.kind.is_removed() {
            return;
        }
    }

    if mobj.tics != -1 {
        mobj.tics -= 1;
        if mobj.tics == 0 {
            P_SetMobjState(mobj_id, (*mobj.state).nextstate);
        }
    } else {
        if mobj.flags & MF_COUNTKILL == 0 {
            return;
        }
        if respawnmonsters() == 0 {
            return;
        }
        mobj.movecount += 1;
        if mobj.movecount < 12 * TICRATE {
            return;
        }
        if leveltime() & 31 != 0 {
            return;
        }
        if P_Random() > 4 {
            return;
        }
        P_NightmareRespawn(mobj_id);
    }
}

/// Allocate, initialise, and link a new map object of type `kind` at world
/// position (`x`, `y`, `z`).
///
/// The initial state, sprite, and tic count are taken from `mobjinfo`.
/// `P_MobjThinker` is registered as the thinker.  Use `ONFLOORZ` / `ONCEILINGZ`
/// for `z` to snap to the sector floor or ceiling respectively.
///
/// Returns the handle of the newly created map object.
///
/// # Safety
///
/// Must be called only while a level is active (its map data loaded).
/// `kind` must be a valid `mobjtype_t` index.
pub unsafe fn P_SpawnMobj(x: i32, y: i32, z: i32, kind: i32) -> MobjId {
    // A zeroed object, as C's `Z_Malloc` + `memset` gives.
    let mobj = new_thinker::<mobj_t>();
    let mobj_ref = &mut *mobj.ptr();
    let info = mobjinfo_ptr(kind as usize);

    mobj_ref.kind = kind;
    mobj_ref.info = info;
    mobj_ref.x = x;
    mobj_ref.y = y;
    mobj_ref.radius = (*info).radius;
    mobj_ref.height = (*info).height;
    mobj_ref.flags = (*info).flags;
    mobj_ref.health = (*info).spawnhealth;

    if gameskill() != 4 {
        // sk_nightmare = 4
        mobj_ref.reactiontime = (*info).reactiontime;
    }

    mobj_ref.lastlook = P_Random() % MAXPLAYERS as i32;

    let st = state_ptr((*info).spawnstate as usize);
    mobj_ref.state = st;
    mobj_ref.tics = (*st).tics;
    mobj_ref.sprite = (*st).sprite;
    mobj_ref.frame = (*st).frame;

    P_SetThingPosition(mobj);

    {
        let level = LEVEL.borrow();
        let sector = level.subsector_sector(mobj_ref.subsector);
        mobj_ref.floorz = sector.floorheight;
        mobj_ref.ceilingz = sector.ceilingheight;
    }

    if z == ONFLOORZ {
        mobj_ref.z = mobj_ref.floorz;
    } else if z == ONCEILINGZ {
        mobj_ref.z = mobj_ref.ceilingz - (*info).height;
    } else {
        mobj_ref.z = z;
    }

    mobj_ref.thinker.kind = ThinkerKind::Mobj;

    P_AddThinker(ThinkerId::Mobj(mobj));

    mobj
}

/// Unlink `mobj` from the sector/block lists, stop any playing sound, and
/// remove its thinker.
///
/// If the mobj is a collectable special item (not dropped and not an
/// invulnerability sphere / invisibility sphere), its spawn record is pushed
/// onto the item-respawn queue for potential deathmatch respawn.
///
/// # Safety
///
/// `mobj_id` must name a map object that is currently linked into the world.
pub unsafe fn P_RemoveMobj(mobj_id: MobjId) {
    let mobj = &mut *mobj_id.ptr();
    if mobj.flags & MF_SPECIAL != 0
        && mobj.flags & MF_DROPPED == 0
        && mobj.kind != MT_INV
        && mobj.kind != MT_INS
    {
        let leveltime = leveltime();
        let mut queue = MOBJ.borrow_mut();
        let head = queue.iquehead as usize;
        queue.itemrespawnque[head] = mobj.spawnpoint;
        queue.itemrespawntime[head] = leveltime;
        queue.iquehead = (queue.iquehead + 1) & (ITEMQUESIZE as i32 - 1);
        if queue.iquehead == queue.iquetail {
            queue.iquetail = (queue.iquetail + 1) & (ITEMQUESIZE as i32 - 1);
        }
    }

    P_UnsetThingPosition(mobj_id);
    S_StopSound(mobj_id);
    P_RemoveThinker(&mut mobj.thinker);
}

/// Respawn the oldest queued special item if deathmatch mode 2 is active and
/// the item has been gone for at least 30 seconds.
///
/// Spawns an item-fog effect at the respawn location, then spawns the item
/// itself and advances the queue tail.
///
/// # Safety
///
/// Must be called only during an active level tick.
pub unsafe fn P_RespawnSpecials() {
    if deathmatch() != 2 {
        return;
    }
    let mthing = {
        let queue = MOBJ.borrow();
        if queue.iquehead == queue.iquetail {
            return;
        }
        let tail = queue.iquetail as usize;
        if leveltime() - queue.itemrespawntime[tail] < 30 * TICRATE {
            return;
        }
        // Spawning does not touch the queue, so the record is copied out.
        queue.itemrespawnque[tail]
    };
    let x = (mthing.x.get() as i32) << FRACBITS;
    let y = (mthing.y.get() as i32) << FRACBITS;

    let ss = R_PointInSubsector(x, y);
    let floorheight = LEVEL.borrow().subsector_sector(ss).floorheight;
    let fog = P_SpawnMobj(x, y, floorheight, MT_IFOG);
    S_StartSound(fog, Sfx::Itmbk as i32);

    let (z, i) = {
        let mobjinfo = MOBJINFO.borrow();
        let mut i = 0;
        while i < NUMMOBJTYPES {
            if mthing.kind.get() as i32 == mobjinfo[i].doomednum {
                break;
            }
            i += 1;
        }

        let z = if mobjinfo[i].flags & MF_SPAWNCEILING != 0 {
            ONCEILINGZ
        } else {
            ONFLOORZ
        };
        (z, i)
    };

    let item = &mut *P_SpawnMobj(x, y, z, i as i32).ptr();
    item.spawnpoint = mthing;
    item.angle = ANG45.wrapping_mul((mthing.angle.get() as u32) / 45);

    let mut queue = MOBJ.borrow_mut();
    queue.iquetail = (queue.iquetail + 1) & (ITEMQUESIZE as i32 - 1);
}

/// Spawn the player mobj for the player indicated by `mthing->type` (1-4).
///
/// Reborns the player if necessary, initialises all HUD state, and sets up
/// weapon psprites.  Skips the slot if the player is not in the current game.
///
/// # Safety
///
/// `mthing` must be a valid, non-null pointer to a `mapthing_t` whose `type`
/// field is in `0..=4`.  Must be called only during level load with zone
/// memory active.
pub unsafe fn P_SpawnPlayer(mthing: *mut mapthing_t) {
    let mthing = &mut *mthing;
    let thing_type = mthing.kind.get();
    if thing_type as i32 == 0 {
        return;
    }
    if playeringame((thing_type - 1) as usize) == 0 {
        return;
    }

    let p = player((thing_type - 1) as usize);

    if (*p).playerstate == PST_REBORN {
        G_PlayerReborn(thing_type as i32 - 1);
    }

    let x = (mthing.x.get() as i32) << FRACBITS;
    let y = (mthing.y.get() as i32) << FRACBITS;
    let z = ONFLOORZ;
    let mobj_id = P_SpawnMobj(x, y, z, MT_PLAYER);
    let mobj = &mut *mobj_id.ptr();

    if thing_type > 1 {
        mobj.flags |= ((thing_type - 1) as i32) << MF_TRANSSHIFT;
    }

    mobj.angle = ANG45.wrapping_mul((mthing.angle.get() as u32) / 45);
    mobj.player = p;
    mobj.health = (*p).health;

    (*p).mo = Some(mobj_id);
    (*p).playerstate = PST_LIVE;
    (*p).refire = 0;
    (*p).message = PlayerMessage::NONE;
    (*p).damagecount = 0;
    (*p).bonuscount = 0;
    (*p).extralight = 0;
    (*p).fixedcolormap = 0;
    (*p).viewheight = VIEWHEIGHT;

    P_SetupPsprites(p);

    if deathmatch() != 0 {
        for i in 0..crate::engine::doom::player::NUMCARDS {
            (*p).cards[i] = 1;
        }
    }

    if thing_type as i32 - 1 == consoleplayer() {
        ST_Start();
        HU_Start();
    }
}

/// Spawn one thing from the map's THINGS lump.
///
/// Handles deathmatch start positions (type 11), player starts (types 1-4),
/// skill-level and network-mode filtering, and the `-nomonsters` flag.
/// Calls `P_SpawnMobj` for all other thing types after looking up the
/// `mobjinfo` entry by `doomednum`.
///
/// # Safety
///
/// `mthing` must be a valid, non-null pointer to a `mapthing_t`.  Must be
/// called during level load.
pub unsafe fn P_SpawnMapThing(mthing: *mut mapthing_t) {
    let mthing = &mut *mthing;
    let thing_type = mthing.kind.get();
    let options = mthing.options.get();
    let mut i: usize;

    if thing_type as i32 == 11 {
        let deathmatch_p = deathmatch_p();
        if deathmatch_p < deathmatchstart(10) {
            *deathmatch_p = *mthing;
            SPAWN_SPOTS.borrow_mut().deathmatch_p = deathmatch_p.add(1);
        }
        return;
    }

    if thing_type as i32 <= 0 {
        return;
    }

    if thing_type as i32 <= 4 {
        *playerstart((thing_type - 1) as usize) = *mthing;
        if deathmatch() == 0 {
            P_SpawnPlayer(mthing);
        }
        return;
    }

    if netgame() == 0 && options & 16i16 != 0 {
        return;
    }

    let gameskill = gameskill();
    let bit = if gameskill == 0 {
        // sk_baby
        1
    } else if gameskill == 4 {
        // sk_nightmare
        4
    } else {
        1 << (gameskill - 1)
    };

    if options & bit as i16 == 0 {
        return;
    }

    let mobjinfo = MOBJINFO.borrow();
    i = 0;
    while i < NUMMOBJTYPES {
        if thing_type as i32 == mobjinfo[i].doomednum {
            break;
        }
        i += 1;
    }

    if i == NUMMOBJTYPES {
        i_error!(
            "P_SpawnMapThing: Unknown type {} at ({}, {})",
            thing_type as i32,
            mthing.x.get() as i32,
            mthing.y.get() as i32
        );
    }

    if deathmatch() != 0 && mobjinfo[i].flags & MF_NOTDMATCH != 0 {
        return;
    }

    if nomonsters() != 0 && (i == MT_SKULL as usize || mobjinfo[i].flags & MF_COUNTKILL != 0) {
        return;
    }

    let x = (mthing.x.get() as i32) << FRACBITS;
    let y = (mthing.y.get() as i32) << FRACBITS;

    let z = if mobjinfo[i].flags & MF_SPAWNCEILING != 0 {
        ONCEILINGZ
    } else {
        ONFLOORZ
    };
    drop(mobjinfo);

    let mobj = P_SpawnMobj(x, y, z, i as i32).ptr();
    (*mobj).spawnpoint = *mthing;

    if (*mobj).tics > 0 {
        (*mobj).tics = 1 + (P_Random() % (*mobj).tics);
    }
    if (*mobj).flags & MF_COUNTKILL != 0 {
        GAME.borrow_mut().totalkills += 1;
    }
    if (*mobj).flags & MF_COUNTITEM != 0 {
        GAME.borrow_mut().totalitems += 1;
    }

    (*mobj).angle = ANG45.wrapping_mul((mthing.angle.get() as u32) / 45);
    if options & MTF_AMBUSH as i16 != 0 {
        (*mobj).flags |= MF_AMBUSH;
    }
}

/// Spawn a bullet-puff visual effect at (`x`, `y`, `z`), randomising the Z
/// slightly and the initial tic count.  Skips to the melee-contact frame
/// (`S_PUFF3`) when the attack was at melee range so punches do not spark.
///
/// # Safety
///
/// Must be called during an active level tick with zone memory available.
pub unsafe fn P_SpawnPuff(x: i32, y: i32, z: i32) {
    let z = z + ((P_Random() - P_Random()) << 10);
    let th_id = P_SpawnMobj(x, y, z, MT_PUFF);
    let th = th_id.ptr();
    (*th).momz = FRACUNIT;
    (*th).tics -= P_Random() & 3;
    if (*th).tics < 1 {
        (*th).tics = 1;
    }
    if attackrange() == MELEERANGE {
        P_SetMobjState(th_id, S_PUFF3);
    }
}

/// Spawn a blood-splat visual effect at (`x`, `y`, `z`).
///
/// The initial state is chosen based on `damage`: heavy hits use the default
/// `MT_BLOOD` spawn state, medium hits start at `S_BLOOD2`, and weak hits
/// start at `S_BLOOD3` (smaller splat).
///
/// # Safety
///
/// Must be called during an active level tick with zone memory available.
pub unsafe fn P_SpawnBlood(x: i32, y: i32, z: i32, damage: i32) {
    let z = z + ((P_Random() - P_Random()) << 10);
    let th_id = P_SpawnMobj(x, y, z, MT_BLOOD);
    let th = th_id.ptr();
    (*th).momz = FRACUNIT * 2;
    (*th).tics -= P_Random() & 3;
    if (*th).tics < 1 {
        (*th).tics = 1;
    }
    if (9..=12).contains(&damage) {
        P_SetMobjState(th_id, S_BLOOD2);
    } else if damage < 9 {
        P_SetMobjState(th_id, S_BLOOD3);
    }
}

/// Validate a newly spawned missile: jitter its tic count, nudge it half a
/// step forward along its trajectory, and explode it immediately if that
/// initial position is blocked.
///
/// # Safety
///
/// `th_id` must name a map object with `MF_MISSILE` set and non-zero
/// momentum.
pub unsafe fn P_CheckMissileSpawn(th_id: MobjId) {
    let th = &mut *th_id.ptr();
    th.tics -= P_Random() & 3;
    if th.tics < 1 {
        th.tics = 1;
    }
    th.x += th.momx >> 1;
    th.y += th.momy >> 1;
    th.z += th.momz >> 1;
    if P_TryMove(th_id, th.x, th.y) == 0 {
        P_ExplodeMissile(th_id);
    }
}

/// Return `mobj` unchanged, or a zeroed dummy map object if `mobj` is null.
///
/// This substitution avoids null-pointer crashes in code that assumes the
/// object always exists, emulating the original Vanilla Doom behaviour
/// where unchecked null dereferences "worked" due to the absence of memory
/// protection.
///
/// The dummy is a slot of the mobj arena that is not in the thinker list;
/// it is allocated on first use in each level, and its position and flags
/// are zeroed on every substitution, as C does.
pub fn P_SubstNullMobj(mobj: Option<MobjId>) -> MobjId {
    if let Some(mobj) = mobj {
        return mobj;
    }
    let existing = MOBJ.borrow().dummy_mobj.filter(|dummy| dummy.is_live());
    let dummy_id = existing.unwrap_or_else(|| {
        let dummy = new_thinker::<mobj_t>();
        MOBJ.borrow_mut().dummy_mobj = Some(dummy);
        dummy
    });
    // SAFETY: the dummy is not in the thinker list or any map list, so no
    // other reference to it is live outside its callers' short reads.
    let dummy = unsafe { &mut *dummy_id.ptr() };
    dummy.x = 0;
    dummy.y = 0;
    dummy.z = 0;
    dummy.flags = 0;
    dummy_id
}

/// Launch a projectile of type `kind` from `source` toward `dest`.
///
/// The missile is spawned 32 units above `source`'s origin, aimed directly at
/// `dest`'s centre (with random spread if `dest` has `MF_SHADOW`).  Vertical
/// momentum is computed from the Z distance divided by travel time.
///
/// Returns the spawned missile.
///
/// # Safety
///
/// `source_id` and `dest_id` must name map objects of the loaded level.
/// `kind` must be a valid `mobjtype_t` index with `MF_MISSILE` in its flags.
pub unsafe fn P_SpawnMissile(source_id: MobjId, dest_id: MobjId, kind: i32) -> MobjId {
    let source = &*source_id.ptr();
    let dest = &*dest_id.ptr();
    let th_id = P_SpawnMobj(source.x, source.y, source.z + 4 * 8 * FRACUNIT, kind);
    let th = th_id.ptr();
    let info = (*th).info;
    if (*info).seesound != Sfx::None {
        S_StartSound(th_id, (*info).seesound as i32);
    }
    (*th).target = Some(source_id);
    let mut an = R_PointToAngle2(source.x, source.y, dest.x, dest.y);
    if dest.flags & MF_SHADOW != 0 {
        an = an.wrapping_add(((P_Random() - P_Random()) << 20) as u32);
    }
    (*th).angle = an;
    an >>= ANGLETOFINESHIFT;
    (*th).momx = FixedMul((*info).speed, *finecosine.0.add(an as usize));
    (*th).momy = FixedMul((*info).speed, finesine[an as usize]);

    let mut dist = P_AproxDistance(dest.x - source.x, dest.y - source.y);
    dist /= (*info).speed;
    if dist < 1 {
        dist = 1;
    }
    (*th).momz = (dest.z - source.z) / dist;
    P_CheckMissileSpawn(th_id);
    th_id
}

/// Launch a projectile of type `kind` from the player's `source` mobj,
/// auto-aiming within a 90-degree cone and then two additional angle offsets
/// before giving up and firing straight ahead.
///
/// # Safety
///
/// `source_id` must name a player's map object.
/// `kind` must be a valid `mobjtype_t` index with `MF_MISSILE` in its flags.
pub unsafe fn P_SpawnPlayerMissile(source_id: MobjId, kind: i32) {
    let source = &mut *source_id.ptr();
    let mut an = source.angle;
    let mut slope = P_AimLineAttack(source_id, an, 16 * 64 * FRACUNIT);

    if linetarget().is_none() {
        an = an.wrapping_add(1 << 26);
        slope = P_AimLineAttack(source_id, an, 16 * 64 * FRACUNIT);
        if linetarget().is_none() {
            an = an.wrapping_sub(2 << 26);
            slope = P_AimLineAttack(source_id, an, 16 * 64 * FRACUNIT);
            if linetarget().is_none() {
                an = source.angle;
                slope = 0;
            }
        }
    }

    let x = source.x;
    let y = source.y;
    let z = source.z + 4 * 8 * FRACUNIT;

    let th_id = P_SpawnMobj(x, y, z, kind);
    let th = th_id.ptr();
    let info = (*th).info;
    if (*info).seesound != Sfx::None {
        S_StartSound(th_id, (*info).seesound as i32);
    }
    (*th).target = Some(source_id);
    (*th).angle = an;
    let fine_idx = (an >> ANGLETOFINESHIFT) as usize;
    (*th).momx = FixedMul((*info).speed, *finecosine.0.add(fine_idx));
    (*th).momy = FixedMul((*info).speed, finesine[fine_idx]);
    (*th).momz = FixedMul((*info).speed, slope);

    P_CheckMissileSpawn(th_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopspeed_is_0x1000() {
        assert_eq!(STOPSPEED, 0x1000);
        assert_eq!(STOPSPEED, 4096);
    }

    #[test]
    fn friction_is_0xe800() {
        assert_eq!(FRICTION, 0xe800_u32 as i32);
        assert_eq!(FRICTION, 59392_u32 as i32);
    }

    #[test]
    fn friction_is_less_than_fracunit() {
        const {
            assert!(
                FRICTION < FRACUNIT,
                "FRICTION must be < FRACUNIT for deceleration"
            )
        };
    }

    #[test]
    fn stopspeed_is_small_fraction_of_fracunit() {
        const { assert!(STOPSPEED < FRACUNIT) };
        assert_eq!(FRACUNIT / STOPSPEED, 16);
    }

    #[test]
    fn itemquesize_is_128() {
        assert_eq!(ITEMQUESIZE, 128);
    }

    #[test]
    fn itemrespawntime_length_is_itemquesize() {
        assert_eq!(MobjState::new().itemrespawntime.len(), ITEMQUESIZE);
    }

    #[test]
    fn itemrespawnque_length_is_itemquesize() {
        assert_eq!(MobjState::new().itemrespawnque.len(), ITEMQUESIZE);
    }

    #[test]
    fn itemque_starts_empty() {
        let queue = MobjState::new();
        assert_eq!(
            queue.iquehead, queue.iquetail,
            "item queue must be empty (head == tail) at startup"
        );
    }

    #[test]
    fn itemrespawntime_starts_zeroed() {
        for (i, &t) in MobjState::new().itemrespawntime.iter().enumerate() {
            assert_eq!(t, 0, "itemrespawntime[{i}] should be 0 before level load");
        }
    }
}
