//! Rust port of `vendor/doomgeneric/p_mobj.c`.
//!
//! Map object (mobj) lifecycle: creation, movement, state-machine updates,
//! spawning from map data, and item-respawn in deathmatch mode.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use {
    crate::{engine::sound::info::Sfx, i_error},
    std::{ffi::c_void, ptr},
};

use crate::engine::{
    doom::player::{PlayerMessage, PlayerT, CF_NOMOMENTUM, MAXPLAYERS},
    hud::stuff::HU_Start,
    interface::timer::TICRATE,
    map::subsector_t,
    math::{
        fixed::{FixedMul, FRACBITS, FRACUNIT},
        tables::{finecosine, finesine, ANG45, ANGLETOFINESHIFT},
    },
    misc::random::P_Random,
    play::{
        info::{self, *},
        maputl::{P_AproxDistance, P_SetThingPosition, P_UnsetThingPosition},
        pspr::P_SetupPsprites,
        tick::{actionf_t, thinker_t, P_AddThinker, P_RemoveThinker},
    },
    render_3d::main::{R_PointInSubsector, R_PointToAngle2},
    sound::{MobjStub, S_StartSound, S_StopSound},
    wad::format::mapthing_t,
    zone::PU_LEVEL,
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
        consoleplayer, deathmatch, gameskill, netgame, playeringame, players, respawnmonsters,
        totalitems, totalkills, G_PlayerReborn,
    },
    play::{
        map::{
            attackrange, ceilingline, linetarget, P_AimLineAttack, P_CheckPosition, P_SlideMove,
            P_TryMove,
        },
        setup::{deathmatch_p, deathmatchstarts, playerstarts},
        tick::leveltime,
    },
    render_3d::sky::skyflatnum,
    status::stuff::ST_Start,
    zone::Z_Malloc,
};

/// A map object (thing) that lives in the world and can think each tic
/// (`mobj_t` in `p_mobj.h`).
///
/// `repr(C)` because the `thinker` field must be at offset 0 so that a
/// `*mut mobj_t` can be cast to `*mut thinker_t` and back.
///
/// Padding bytes (`_pad0`–`_pad4`) fill gaps that C compilers insert for
/// alignment; they have no semantic meaning in this port.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mobj_t {
    /// Thinker header — must be the very first field (offset 0).
    pub thinker: thinker_t,
    /// Map X position in fixed-point units.
    pub x: i32,
    /// Map Y position in fixed-point units.
    pub y: i32,
    /// Map Z position (height above the floor) in fixed-point units.
    pub z: i32,
    _pad0: [u8; 4],
    /// Next thing in this sector's sprite-order linked list.
    pub snext: *mut mobj_t,
    /// Previous thing in this sector's sprite-order linked list.
    pub sprev: *mut mobj_t,
    /// Facing angle in binary radians (BAMs); 0 = East.
    pub angle: u32,
    /// Current sprite number.
    pub sprite: i32,
    /// Current animation frame index.
    pub frame: i32,
    _pad1: [u8; 4],
    /// Next thing in the blockmap block's linked list.
    pub bnext: *mut mobj_t,
    /// Previous thing in the blockmap block's linked list.
    pub bprev: *mut mobj_t,
    /// The sub-sector this thing currently occupies.
    pub subsector: *mut subsector_t,
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
    _pad2: [u8; 4],
    /// Pointer to the thing's type info record.
    pub info: *mut MobjInfo,
    /// Remaining tics in the current animation frame.
    pub tics: i32,
    _pad3: [u8; 4],
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
    pub target: *mut mobj_t,
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
    _pad4: [u8; 2],
    /// Tracer target used by homing missiles (cyberdemon rockets, etc.).
    pub tracer: *mut mobj_t,
}

/// Circular queue of map-thing spawn records for items that need to respawn.
///
/// The queue is indexed by `iquehead` (write) and `iquetail` (read), both
/// modulo `ITEMQUESIZE`.
pub static mut itemrespawnque: [mapthing_t; ITEMQUESIZE] = [mapthing_t::ZERO; ITEMQUESIZE];

/// Level-time stamps recording when each item in `itemrespawnque` was removed.
///
/// An item is eligible to respawn once `leveltime - itemrespawntime[iquetail]`
/// exceeds 30 * `TICRATE`.
pub static mut itemrespawntime: [i32; ITEMQUESIZE] = [0; ITEMQUESIZE];

/// Write index (head) of the item-respawn circular queue.
pub static mut iquehead: i32 = 0;

/// Read index (tail) of the item-respawn circular queue.
pub static mut iquetail: i32 = 0;

/// Construct the sentinel `actionf_t` value used to detect that a thinker has
/// been removed mid-tick (all-ones function pointer).
fn sentinel_ac() -> Option<unsafe fn()> {
    Some(unsafe { core::mem::transmute::<usize, unsafe fn()>(usize::MAX) })
}

/// Return `true` if `f` is the removal-sentinel value, meaning the mobj was
/// freed during the current tick.
fn is_sentinel(f: actionf_t) -> bool {
    unsafe { f.acv == sentinel_ac() }
}

/// Transition `mobj` to state `state`, running zero-tic chain states
/// immediately.  Returns `1` if the mobj is still alive after the transition,
/// `0` if it removed itself (`S_NULL`).
///
/// Mirrors `P_SetMobjState` in `p_mobj.c`.
///
/// # Safety
///
/// `mobj` must be a valid, non-null pointer to an initialised `mobj_t`.
/// `state` must be a valid state-table index or `S_NULL`.
pub unsafe fn P_SetMobjState(mobj: *mut mobj_t, state: i32) -> i32 {
    let mobj = &mut *mobj;
    let mut state = state;
    loop {
        if state == S_NULL {
            mobj.state = ptr::null_mut();
            P_RemoveMobj(mobj as *mut mobj_t);
            return 0;
        }
        let st = &raw mut info::states[state as usize];
        mobj.state = st;
        mobj.tics = (*st).tics;
        mobj.sprite = (*st).sprite;
        mobj.frame = (*st).frame;
        if let Some(action) = (*st).action {
            let action: unsafe fn(*mut c_void) = core::mem::transmute(action);
            action(mobj as *mut mobj_t as *mut c_void);
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
/// `mo` must be a valid, non-null pointer to an `mobj_t` with `MF_MISSILE` set.
pub unsafe fn P_ExplodeMissile(mo: *mut mobj_t) {
    let mo = &mut *mo;
    let info = mo.info;
    mo.momx = 0;
    mo.momy = 0;
    mo.momz = 0;
    P_SetMobjState(mo as *mut mobj_t, (*info).deathstate);
    mo.tics -= P_Random() & 3;
    if mo.tics < 1 {
        mo.tics = 1;
    }
    mo.flags &= !MF_MISSILE;
    if (*info).deathsound != Sfx::None {
        S_StartSound(mo as *mut mobj_t as *mut c_void, (*info).deathsound as i32);
    }
}

/// Apply one tic of XY momentum to `mo`, sub-stepping when the move exceeds
/// `MAXMOVE / 2`.  Handles player sliding, missile explosion against walls
/// and the sky, and floor friction / stopping.
///
/// # Safety
///
/// `mo` must be a valid, non-null pointer to an `mobj_t` that is part of the
/// active thinker list.
pub unsafe fn P_XYMovement(mo: *mut mobj_t) {
    let mo = &mut *mo;
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
            P_SetMobjState(mo as *mut mobj_t, (*info).spawnstate);
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

        if P_TryMove(mo as *mut mobj_t, ptryx, ptryy) == 0 {
            if !mo.player.is_null() {
                P_SlideMove(mo as *mut mobj_t);
            } else if mo.flags & MF_MISSILE != 0 {
                let cl = ceilingline;
                if !cl.is_null() {
                    let line = cl;
                    if !(*line).backsector.is_null()
                        && (*(*line).backsector).ceilingpic == skyflatnum as i16
                    {
                        P_RemoveMobj(mo as *mut mobj_t);
                        return;
                    }
                }
                P_ExplodeMissile(mo as *mut mobj_t);
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
        && mo.floorz != (*(*mo.subsector).sector).floorheight
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
            let state_ptr = mo.state;
            let states_ptr = std::ptr::addr_of!(info::states) as *const State;
            let state_idx =
                (state_ptr as usize - states_ptr as usize) / std::mem::size_of::<State>();
            let run_offset = state_idx as i32 - S_PLAY_RUN1;
            if (0..4).contains(&run_offset) {
                P_SetMobjState(mo as *mut mobj_t, S_PLAY);
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
/// `mo` must be a valid, non-null pointer to an `mobj_t` that is part of the
/// active thinker list.
pub unsafe fn P_ZMovement(mo: *mut mobj_t) {
    let mo = &mut *mo;
    let dist: i32;
    let delta: i32;

    if !mo.player.is_null() && mo.z < mo.floorz {
        let player = mo.player;
        (*player).viewheight -= mo.floorz - mo.z;
        (*player).deltaviewheight = (VIEWHEIGHT - (*player).viewheight) >> 3;
    }

    mo.z += mo.momz;

    if mo.flags & MF_FLOAT != 0
        && !mo.target.is_null()
        && mo.flags & MF_SKULLFLY == 0
        && mo.flags & MF_INFLOAT == 0
    {
        let target = mo.target;
        dist = P_AproxDistance(mo.x - (*target).x, mo.y - (*target).y);
        delta = ((*target).z + (mo.height >> 1)) - mo.z;
        if delta < 0 && dist < -(delta * 3) {
            mo.z -= FLOATSPEED;
        } else if delta > 0 && dist < delta * 3 {
            mo.z += FLOATSPEED;
        }
    }

    if mo.z <= mo.floorz {
        let correct_lost_soul_bounce = gameversion >= exe_ultimate;
        if correct_lost_soul_bounce && mo.flags & MF_SKULLFLY != 0 {
            mo.momz = -mo.momz;
        }
        if mo.momz < 0 {
            if !mo.player.is_null() && mo.momz < -GRAVITY * 8 {
                let player = mo.player;
                (*player).deltaviewheight = mo.momz >> 3;
                S_StartSound(mo as *mut mobj_t as *mut c_void, Sfx::Oof as i32);
            }
            mo.momz = 0;
        }
        mo.z = mo.floorz;
        if !correct_lost_soul_bounce && mo.flags & MF_SKULLFLY != 0 {
            mo.momz = -mo.momz;
        }
        if mo.flags & MF_MISSILE != 0 && mo.flags & MF_NOCLIP == 0 {
            P_ExplodeMissile(mo as *mut mobj_t);
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
            P_ExplodeMissile(mo as *mut mobj_t);
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
/// `mobj` must be a valid, non-null pointer to an `mobj_t` that has
/// `MF_COUNTKILL` set and whose `spawnpoint` field is valid.
pub unsafe fn P_NightmareRespawn(mobj: *mut mobj_t) {
    let mobj = &mut *mobj;
    let x = (mobj.spawnpoint.x.get() as i32) << FRACBITS;
    let y = (mobj.spawnpoint.y.get() as i32) << FRACBITS;
    if P_CheckPosition(mobj as *mut mobj_t, x, y) == 0 {
        return;
    }

    let mut mo = P_SpawnMobj(
        mobj.x,
        mobj.y,
        (*(*mobj.subsector).sector).floorheight,
        MT_TFOG,
    );
    S_StartSound(mo as *mut c_void, Sfx::Telept as i32);

    let ss = R_PointInSubsector(x, y);
    mo = P_SpawnMobj(x, y, (*(*ss).sector).floorheight, MT_TFOG);
    S_StartSound(mo as *mut c_void, Sfx::Telept as i32);

    let mthing = &mobj.spawnpoint;
    let info = mobj.info;
    let z = if (*info).flags & MF_SPAWNCEILING != 0 {
        ONCEILINGZ
    } else {
        ONFLOORZ
    };

    mo = P_SpawnMobj(x, y, z, mobj.kind);
    (*mo).spawnpoint = mobj.spawnpoint;
    (*mo).angle = ANG45.wrapping_mul((mthing.angle.get() as u32) / 45);
    if mthing.options.get() & MTF_AMBUSH as i16 != 0 {
        (*mo).flags |= MF_AMBUSH;
    }
    (*mo).reactiontime = 18;
    P_RemoveMobj(mobj as *mut mobj_t);
}

/// Per-tic thinker for every active map object.
///
/// Runs XY and Z movement, advances the state machine, and initiates
/// Nightmare-mode respawn for eligible dead monsters.  Detects mid-tick
/// removal via the sentinel function pointer.
///
/// # Safety
///
/// `mobj` must be a valid, non-null pointer to an `mobj_t` that is currently
/// linked into the thinker list.
pub unsafe fn P_MobjThinker(mobj: *mut mobj_t) {
    let mobj = &mut *mobj;
    if mobj.momx != 0 || mobj.momy != 0 || mobj.flags & MF_SKULLFLY != 0 {
        P_XYMovement(mobj as *mut mobj_t);
        if is_sentinel(mobj.thinker.function) {
            return;
        }
    }
    if mobj.z != mobj.floorz || mobj.momz != 0 {
        P_ZMovement(mobj as *mut mobj_t);
        if is_sentinel(mobj.thinker.function) {
            return;
        }
    }

    if mobj.tics != -1 {
        mobj.tics -= 1;
        if mobj.tics == 0 {
            P_SetMobjState(mobj as *mut mobj_t, (*mobj.state).nextstate);
        }
    } else {
        if mobj.flags & MF_COUNTKILL == 0 {
            return;
        }
        if respawnmonsters == 0 {
            return;
        }
        mobj.movecount += 1;
        if mobj.movecount < 12 * TICRATE {
            return;
        }
        if leveltime & 31 != 0 {
            return;
        }
        if P_Random() > 4 {
            return;
        }
        P_NightmareRespawn(mobj as *mut mobj_t);
    }
}

/// Allocate, initialise, and link a new map object of type `kind` at world
/// position (`x`, `y`, `z`).
///
/// The initial state, sprite, and tic count are taken from `mobjinfo`.
/// `P_MobjThinker` is registered as the thinker.  Use `ONFLOORZ` / `ONCEILINGZ`
/// for `z` to snap to the sector floor or ceiling respectively.
///
/// Returns a pointer to the newly created `mobj_t`.
///
/// # Safety
///
/// Must be called only while a level is active (zone memory must be
/// initialised).  `kind` must be a valid `mobjtype_t` index.
pub unsafe fn P_SpawnMobj(x: i32, y: i32, z: i32, kind: i32) -> *mut mobj_t {
    let mobj = Z_Malloc(
        std::mem::size_of::<mobj_t>() as i32,
        PU_LEVEL,
        ptr::null_mut(),
    ) as *mut mobj_t;
    // Z_Malloc already zeroes the allocation internally.
    let mobj_ref = &mut *mobj;
    let info = &raw mut info::mobjinfo[kind as usize];

    mobj_ref.kind = kind;
    mobj_ref.info = info;
    mobj_ref.x = x;
    mobj_ref.y = y;
    mobj_ref.radius = (*info).radius;
    mobj_ref.height = (*info).height;
    mobj_ref.flags = (*info).flags;
    mobj_ref.health = (*info).spawnhealth;

    if gameskill != 4 {
        // sk_nightmare = 4
        mobj_ref.reactiontime = (*info).reactiontime;
    }

    mobj_ref.lastlook = P_Random() % MAXPLAYERS as i32;

    let st = &raw mut info::states[(*info).spawnstate as usize];
    mobj_ref.state = st;
    mobj_ref.tics = (*st).tics;
    mobj_ref.sprite = (*st).sprite;
    mobj_ref.frame = (*st).frame;

    P_SetThingPosition(mobj);

    mobj_ref.floorz = (*(*mobj_ref.subsector).sector).floorheight;
    mobj_ref.ceilingz = (*(*mobj_ref.subsector).sector).ceilingheight;

    if z == ONFLOORZ {
        mobj_ref.z = mobj_ref.floorz;
    } else if z == ONCEILINGZ {
        mobj_ref.z = mobj_ref.ceilingz - (*info).height;
    } else {
        mobj_ref.z = z;
    }

    mobj_ref.thinker.function.acp1 = Some(core::mem::transmute::<
        unsafe fn(*mut mobj_t),
        unsafe fn(*mut c_void),
    >(P_MobjThinker));

    P_AddThinker(&mut mobj_ref.thinker as *mut thinker_t);

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
/// `mobj` must be a valid, non-null pointer to an `mobj_t` that is currently
/// linked into the world.
pub unsafe fn P_RemoveMobj(mobj: *mut mobj_t) {
    let mobj = &mut *mobj;
    if mobj.flags & MF_SPECIAL != 0
        && mobj.flags & MF_DROPPED == 0
        && mobj.kind != MT_INV
        && mobj.kind != MT_INS
    {
        itemrespawnque[iquehead as usize] = mobj.spawnpoint;
        itemrespawntime[iquehead as usize] = leveltime;
        iquehead = (iquehead + 1) & (ITEMQUESIZE as i32 - 1);
        if iquehead == iquetail {
            iquetail = (iquetail + 1) & (ITEMQUESIZE as i32 - 1);
        }
    }

    P_UnsetThingPosition(mobj as *mut mobj_t);
    S_StopSound(mobj as *mut mobj_t as *mut MobjStub);
    P_RemoveThinker(&mut mobj.thinker as *mut thinker_t);
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
    if deathmatch != 2 {
        return;
    }
    if iquehead == iquetail {
        return;
    }
    if leveltime - itemrespawntime[iquetail as usize] < 30 * TICRATE {
        return;
    }

    let mthing = &itemrespawnque[iquetail as usize];
    let x = (mthing.x.get() as i32) << FRACBITS;
    let y = (mthing.y.get() as i32) << FRACBITS;

    let ss = R_PointInSubsector(x, y);
    let mut mo = P_SpawnMobj(x, y, (*(*ss).sector).floorheight, MT_IFOG);
    S_StartSound(mo as *mut c_void, Sfx::Itmbk as i32);

    let mut i = 0;
    while i < NUMMOBJTYPES {
        if mthing.kind.get() as i32 == info::mobjinfo[i].doomednum {
            break;
        }
        i += 1;
    }

    let z = if info::mobjinfo[i].flags & MF_SPAWNCEILING != 0 {
        ONCEILINGZ
    } else {
        ONFLOORZ
    };

    mo = P_SpawnMobj(x, y, z, i as i32);
    (*mo).spawnpoint = *mthing;
    (*mo).angle = ANG45.wrapping_mul((mthing.angle.get() as u32) / 45);

    iquetail = (iquetail + 1) & (ITEMQUESIZE as i32 - 1);
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
    if playeringame[(thing_type - 1) as usize] == 0 {
        return;
    }

    let p = &mut players[(thing_type - 1) as usize] as *mut PlayerT;

    if (*p).playerstate == PST_REBORN {
        G_PlayerReborn(thing_type as i32 - 1);
    }

    let x = (mthing.x.get() as i32) << FRACBITS;
    let y = (mthing.y.get() as i32) << FRACBITS;
    let z = ONFLOORZ;
    let mobj = P_SpawnMobj(x, y, z, MT_PLAYER);

    if thing_type > 1 {
        (*mobj).flags |= ((thing_type - 1) as i32) << MF_TRANSSHIFT;
    }

    (*mobj).angle = ANG45.wrapping_mul((mthing.angle.get() as u32) / 45);
    (*mobj).player = p;
    (*mobj).health = (*p).health;

    (*p).mo = mobj;
    (*p).playerstate = PST_LIVE;
    (*p).refire = 0;
    (*p).message = PlayerMessage::NONE;
    (*p).damagecount = 0;
    (*p).bonuscount = 0;
    (*p).extralight = 0;
    (*p).fixedcolormap = 0;
    (*p).viewheight = VIEWHEIGHT;

    P_SetupPsprites(p);

    if deathmatch != 0 {
        for i in 0..crate::engine::doom::player::NUMCARDS {
            (*p).cards[i] = 1;
        }
    }

    if thing_type as i32 - 1 == consoleplayer {
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
        if deathmatch_p < std::ptr::addr_of_mut!(deathmatchstarts[0]).add(10) {
            *deathmatch_p = *mthing;
            deathmatch_p = deathmatch_p.add(1);
        }
        return;
    }

    if thing_type as i32 <= 0 {
        return;
    }

    if thing_type as i32 <= 4 {
        let ps = std::ptr::addr_of_mut!(playerstarts[0]);
        *ps.add((thing_type - 1) as usize) = *mthing;
        if deathmatch == 0 {
            P_SpawnPlayer(mthing);
        }
        return;
    }

    if netgame == 0 && options & 16i16 != 0 {
        return;
    }

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

    i = 0;
    while i < NUMMOBJTYPES {
        if thing_type as i32 == info::mobjinfo[i].doomednum {
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

    if deathmatch != 0 && info::mobjinfo[i].flags & MF_NOTDMATCH != 0 {
        return;
    }

    if nomonsters != 0 && (i == MT_SKULL as usize || info::mobjinfo[i].flags & MF_COUNTKILL != 0) {
        return;
    }

    let x = (mthing.x.get() as i32) << FRACBITS;
    let y = (mthing.y.get() as i32) << FRACBITS;

    let z = if info::mobjinfo[i].flags & MF_SPAWNCEILING != 0 {
        ONCEILINGZ
    } else {
        ONFLOORZ
    };

    let mobj = P_SpawnMobj(x, y, z, i as i32);
    (*mobj).spawnpoint = *mthing;

    if (*mobj).tics > 0 {
        (*mobj).tics = 1 + (P_Random() % (*mobj).tics);
    }
    if (*mobj).flags & MF_COUNTKILL != 0 {
        totalkills += 1;
    }
    if (*mobj).flags & MF_COUNTITEM != 0 {
        totalitems += 1;
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
    let th = P_SpawnMobj(x, y, z, MT_PUFF);
    (*th).momz = FRACUNIT;
    (*th).tics -= P_Random() & 3;
    if (*th).tics < 1 {
        (*th).tics = 1;
    }
    if attackrange == MELEERANGE {
        P_SetMobjState(th, S_PUFF3);
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
    let th = P_SpawnMobj(x, y, z, MT_BLOOD);
    (*th).momz = FRACUNIT * 2;
    (*th).tics -= P_Random() & 3;
    if (*th).tics < 1 {
        (*th).tics = 1;
    }
    if (9..=12).contains(&damage) {
        P_SetMobjState(th, S_BLOOD2);
    } else if damage < 9 {
        P_SetMobjState(th, S_BLOOD3);
    }
}

/// Validate a newly spawned missile: jitter its tic count, nudge it half a
/// step forward along its trajectory, and explode it immediately if that
/// initial position is blocked.
///
/// # Safety
///
/// `th` must be a valid, non-null pointer to an `mobj_t` with `MF_MISSILE`
/// set and non-zero momentum.
pub unsafe fn P_CheckMissileSpawn(th: *mut mobj_t) {
    let th = &mut *th;
    th.tics -= P_Random() & 3;
    if th.tics < 1 {
        th.tics = 1;
    }
    th.x += th.momx >> 1;
    th.y += th.momy >> 1;
    th.z += th.momz >> 1;
    if P_TryMove(th as *mut mobj_t, th.x, th.y) == 0 {
        P_ExplodeMissile(th as *mut mobj_t);
    }
}

/// Return `mobj` unchanged, or a pointer to a zeroed dummy `mobj_t` if
/// `mobj` is null.
///
/// This substitution avoids null-pointer crashes in code that assumes the
/// pointer is always valid, emulating the original Vanilla Doom behaviour
/// where unchecked null dereferences "worked" due to the absence of memory
/// protection.
///
/// # Safety
///
/// The returned dummy pointer is valid only until the next call to this
/// function from the same thread; callers must not store it across ticks.
pub unsafe fn P_SubstNullMobj(mobj: *mut mobj_t) -> *mut mobj_t {
    if mobj.is_null() {
        static mut DUMMY_MOBJ: mobj_t = unsafe { std::mem::zeroed() };
        DUMMY_MOBJ.x = 0;
        DUMMY_MOBJ.y = 0;
        DUMMY_MOBJ.z = 0;
        DUMMY_MOBJ.flags = 0;
        return std::ptr::addr_of_mut!(DUMMY_MOBJ);
    }
    mobj
}

/// Launch a projectile of type `kind` from `source` toward `dest`.
///
/// The missile is spawned 32 units above `source`'s origin, aimed directly at
/// `dest`'s centre (with random spread if `dest` has `MF_SHADOW`).  Vertical
/// momentum is computed from the Z distance divided by travel time.
///
/// Returns a pointer to the spawned missile `mobj_t`.
///
/// # Safety
///
/// `source` and `dest` must be valid, non-null pointers to `mobj_t` instances.
/// `kind` must be a valid `mobjtype_t` index with `MF_MISSILE` in its flags.
pub unsafe fn P_SpawnMissile(source: *mut mobj_t, dest: *mut mobj_t, kind: i32) -> *mut mobj_t {
    let source = &*source;
    let dest = &*dest;
    let th = P_SpawnMobj(source.x, source.y, source.z + 4 * 8 * FRACUNIT, kind);
    let info = (*th).info;
    if (*info).seesound != Sfx::None {
        S_StartSound(th as *mut c_void, (*info).seesound as i32);
    }
    (*th).target = source as *const mobj_t as *mut mobj_t;
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
    P_CheckMissileSpawn(th);
    th
}

/// Launch a projectile of type `kind` from the player's `source` mobj,
/// auto-aiming within a 90-degree cone and then two additional angle offsets
/// before giving up and firing straight ahead.
///
/// # Safety
///
/// `source` must be a valid, non-null pointer to a player `mobj_t`.
/// `kind` must be a valid `mobjtype_t` index with `MF_MISSILE` in its flags.
pub unsafe fn P_SpawnPlayerMissile(source: *mut mobj_t, kind: i32) {
    let source = &mut *source;
    let mut an = source.angle;
    let mut slope = P_AimLineAttack(source as *mut mobj_t, an, 16 * 64 * FRACUNIT);

    if linetarget.is_null() {
        an = an.wrapping_add(1 << 26);
        slope = P_AimLineAttack(source as *mut mobj_t, an, 16 * 64 * FRACUNIT);
        if linetarget.is_null() {
            an = an.wrapping_sub(2 << 26);
            slope = P_AimLineAttack(source as *mut mobj_t, an, 16 * 64 * FRACUNIT);
            if linetarget.is_null() {
                an = source.angle;
                slope = 0;
            }
        }
    }

    let x = source.x;
    let y = source.y;
    let z = source.z + 4 * 8 * FRACUNIT;

    let th = P_SpawnMobj(x, y, z, kind);
    let info = (*th).info;
    if (*info).seesound != Sfx::None {
        S_StartSound(th as *mut c_void, (*info).seesound as i32);
    }
    (*th).target = source as *const mobj_t as *mut mobj_t;
    (*th).angle = an;
    let fine_idx = (an >> ANGLETOFINESHIFT) as usize;
    (*th).momx = FixedMul((*info).speed, *finecosine.0.add(fine_idx));
    (*th).momy = FixedMul((*info).speed, finesine[fine_idx]);
    (*th).momz = FixedMul((*info).speed, slope);

    P_CheckMissileSpawn(th);
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
        unsafe {
            assert_eq!((*&raw const itemrespawntime).len(), ITEMQUESIZE);
        }
    }

    #[test]
    fn itemrespawnque_length_is_itemquesize() {
        unsafe {
            assert_eq!((*&raw const itemrespawnque).len(), ITEMQUESIZE);
        }
    }

    #[test]
    fn itemque_starts_empty() {
        unsafe {
            assert_eq!(
                { iquehead },
                { iquetail },
                "item queue must be empty (head == tail) at startup"
            );
        }
    }

    #[test]
    fn itemrespawntime_starts_zeroed() {
        unsafe {
            for (i, &t) in (*&raw const itemrespawntime).iter().enumerate() {
                assert_eq!(t, 0, "itemrespawntime[{i}] should be 0 before level load");
            }
        }
    }
}
