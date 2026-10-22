//! Rust port of `vendor/doomgeneric/p_pspr.c`.
//!
//! Weapon sprite animation, weapon objects, and action functions for weapons.
//! Each player has a two-slot psprite array (weapon overlay + muzzle flash)
//! driven by the same state machine used for map objects.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::doom::sounds::Sfx;
use std::ffi::c_void;
use std::os::raw::c_int;

use crate::doom::c_ffi::{LOWERSPEED, RAISESPEED, WEAPONBOTTOM, WEAPONTOP};
use crate::doom::d_items::weaponinfo;
use crate::doom::d_mode::{commercial, shareware};
use crate::doom::d_player::{PlayerT, PspdefT, NUMAMMO, NUMPSPRITES, NUMWEAPONS};
use crate::doom::doomstat::gamemode;
use crate::doom::info::{self, *};
use crate::doom::m_fixed::{fixed_t, FixedMul};
use crate::doom::m_fixed::{FRACBITS, FRACUNIT};
use crate::doom::m_random::P_Random;
use crate::doom::p_telept::mobj_t;
use crate::doom::p_tick::leveltime;
use crate::doom::s_sound::S_StartSound;
use crate::doom::tables::{finecosine, finesine, FINEANGLES, FINEMASK};

// Weapon type constants (from doomdef.h)
/// Weapon slot index for the fist (melee, no ammo).
const wp_fist: c_int = 0;
/// Weapon slot index for the pistol.
const wp_pistol: c_int = 1;
/// Weapon slot index for the shotgun.
const wp_shotgun: c_int = 2;
/// Weapon slot index for the chaingun.
const wp_chaingun: c_int = 3;
/// Weapon slot index for the rocket launcher.
const wp_missile: c_int = 4;
/// Weapon slot index for the plasma rifle.
const wp_plasma: c_int = 5;
/// Weapon slot index for the BFG 9000.
const wp_bfg: c_int = 6;
/// Weapon slot index for the chainsaw (no ammo).
const wp_chainsaw: c_int = 7;
/// Weapon slot index for the super shotgun (Doom II only).
const wp_supershotgun: c_int = 8;
/// Sentinel value meaning no pending weapon change is queued.
const wp_nochange: c_int = NUMWEAPONS as c_int;

// Ammo type constants
/// Ammo type index meaning the weapon uses no ammo.
const am_noammo: c_int = 5;
/// Ammo type index for bullets (clip).
const am_clip: c_int = 0;
/// Ammo type index for shells.
const am_shell: c_int = 1;
/// Ammo type index for cells (plasma / BFG).
const am_cell: c_int = 2;
/// Ammo type index for missiles (rockets).
const am_misl: c_int = 3;

// Power type constants
/// Power-up slot index for Berserk (strength); multiplies fist damage by 10.
const pw_strength: usize = 1;

// Button constants
/// Bit mask in `ticcmd_t::buttons` that signals the attack button is held.
const BT_ATTACK: u8 = 1;

// Player state constants
/// Player state value for a dead player (`PST_DEAD`).
const PST_DEAD: c_int = 1;

// Angle constants
/// Binary-angle for 90 degrees (0x40000000).
const ANG90: u32 = 0x40000000;
/// Binary-angle for 180 degrees (0x80000000).
const ANG180: u32 = 0x80000000;

// Range constants (from p_local.h)
/// Maximum reach for melee attacks, in fixed-point world units (64 map units).
const MELEERANGE: c_int = 64 * FRACUNIT;
/// Maximum range for hitscan (bullet) attacks (32 * 64 map units).
const MISSILERANGE: c_int = 32 * 64 * FRACUNIT;

// Dehacked default
/// Default number of cells consumed per BFG shot (Dehacked-patchable in C; hardcoded here).
const DEH_DEFAULT_BFG_CELLS_PER_SHOT: c_int = 40;

use crate::doom::p_enemy::P_NoiseAlert;
use crate::doom::p_inter::P_DamageMobj;
use crate::doom::p_map::{linetarget, P_AimLineAttack, P_LineAttack};
use crate::doom::p_mobj::{P_SetMobjState, P_SpawnMobj, P_SpawnPlayerMissile};
use crate::doom::r_main::R_PointToAngle2;

/// Type alias used for cross-module pointer casts where both sides are `#[repr(C)]`-identical.
type CffiMobj = crate::doom::c_ffi::mobj_t;

/// Horizontal weapon-bob offset; updated each tic by `P_CalcSwing`.
#[no_mangle]
pub static mut swingx: fixed_t = 0;

/// Vertical weapon-bob offset; updated each tic by `P_CalcSwing`.
#[no_mangle]
pub static mut swingy: fixed_t = 0;

/// Slope set by `P_BulletSlope` for near-miss aiming; read by `P_GunShot`.
#[no_mangle]
pub static mut bulletslope: fixed_t = 0;

/// Transition a psprite slot to a new state, running action functions until a
/// non-zero tic count is reached or the state chain ends.
///
/// Mirrors `P_SetPsprite` in `p_pspr.c`.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`.
/// `position` must be in `0..NUMPSPRITES`.  `stnum` must be a valid state
/// index or `S_NULL` (0).
#[no_mangle]
pub unsafe extern "C" fn P_SetPsprite(player: *mut PlayerT, position: c_int, stnum: c_int) {
    let psp = (*player).psprites.as_mut_ptr().add(position as usize);

    let mut stnum = stnum;
    loop {
        if stnum == 0 {
            (*psp).state = std::ptr::null_mut();
            break;
        }

        let state = &mut info::states[stnum as usize] as *mut State;
        (*psp).state = state as *mut crate::doom::d_player::state_t;
        (*psp).tics = (*state).tics;

        if (*state).misc1 != 0 {
            (*psp).sx = (*state).misc1 << FRACBITS;
            (*psp).sy = (*state).misc2 << FRACBITS;
        }

        // Call action routine.
        if let Some(action) = (*state).action {
            let action: unsafe extern "C" fn(*mut PlayerT, *mut PspdefT) =
                std::mem::transmute(action);
            action(player, psp);
            if (*psp).state.is_null() {
                break;
            }
        }

        // Read nextstate from psp->state (action may have changed it).
        stnum = ((*psp).state as *mut State).read().nextstate;

        if (*psp).tics != 0 {
            break;
        }
    }
}

/// Recompute the horizontal (`swingx`) and vertical (`swingy`) weapon-bob
/// offsets for the current tic using the player's `bob` amplitude.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`.
#[no_mangle]
pub unsafe extern "C" fn P_CalcSwing(player: *mut PlayerT) {
    let swing = (*player).bob;

    let mut angle = (FINEANGLES as c_int / 70 * leveltime) & FINEMASK;
    swingx = FixedMul(swing, finesine[angle as usize]);

    angle = (FINEANGLES as c_int / 70 * leveltime + FINEANGLES as c_int / 2) & FINEMASK;
    swingy = -FixedMul(swingx, finesine[angle as usize]);
}

/// Begin the raise animation for the pending weapon by setting the weapon
/// psprite to its `upstate` and positioning it at `WEAPONBOTTOM`.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`.
#[no_mangle]
pub unsafe extern "C" fn P_BringUpWeapon(player: *mut PlayerT) {
    if (*player).pendingweapon == wp_nochange {
        (*player).pendingweapon = (*player).readyweapon;
    }

    if (*player).pendingweapon == wp_chainsaw {
        S_StartSound((*player).mo as *mut c_void, Sfx::Sawup as c_int);
    }

    let newstate = weaponinfo[(*player).pendingweapon as usize].upstate;

    (*player).pendingweapon = wp_nochange;
    (*player).psprites[0].sy = WEAPONBOTTOM;

    P_SetPsprite(player, 0, newstate);
}

/// Return `1` if the player has enough ammo to fire the ready weapon, `0`
/// otherwise.  When out of ammo, selects the next best weapon and begins
/// lowering the current one.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`.
#[no_mangle]
pub unsafe extern "C" fn P_CheckAmmo(player: *mut PlayerT) -> c_int {
    let ammo = weaponinfo[(*player).readyweapon as usize].ammo;

    let count: c_int;
    if (*player).readyweapon == wp_bfg {
        count = DEH_DEFAULT_BFG_CELLS_PER_SHOT;
    } else if (*player).readyweapon == wp_supershotgun {
        count = 2;
    } else {
        count = 1;
    }

    if ammo == am_noammo || (*player).ammo[ammo as usize] >= count {
        return 1;
    }

    // Out of ammo, pick a weapon to change to.
    loop {
        if (*player).weaponowned[wp_plasma as usize] != 0
            && (*player).ammo[am_cell as usize] != 0
            && gamemode != shareware
        {
            (*player).pendingweapon = wp_plasma;
        } else if (*player).weaponowned[wp_supershotgun as usize] != 0
            && (*player).ammo[am_shell as usize] > 2
            && gamemode == commercial
        {
            (*player).pendingweapon = wp_supershotgun;
        } else if (*player).weaponowned[wp_chaingun as usize] != 0
            && (*player).ammo[am_clip as usize] != 0
        {
            (*player).pendingweapon = wp_chaingun;
        } else if (*player).weaponowned[wp_shotgun as usize] != 0
            && (*player).ammo[am_shell as usize] != 0
        {
            (*player).pendingweapon = wp_shotgun;
        } else if (*player).ammo[am_clip as usize] != 0 {
            (*player).pendingweapon = wp_pistol;
        } else if (*player).weaponowned[wp_chainsaw as usize] != 0 {
            (*player).pendingweapon = wp_chainsaw;
        } else if (*player).weaponowned[wp_missile as usize] != 0
            && (*player).ammo[am_misl as usize] != 0
        {
            (*player).pendingweapon = wp_missile;
        } else if (*player).weaponowned[wp_bfg as usize] != 0
            && (*player).ammo[am_cell as usize] > 40
            && gamemode != shareware
        {
            (*player).pendingweapon = wp_bfg;
        } else {
            (*player).pendingweapon = wp_fist;
        }

        if (*player).pendingweapon != wp_nochange {
            break;
        }
    }

    P_SetPsprite(
        player,
        0,
        weaponinfo[(*player).readyweapon as usize].downstate,
    );

    0
}

/// Check ammo and, if sufficient, put the player mobj into the attack state
/// and transition the weapon psprite to its attack state.  Also alerts nearby
/// monsters via `P_NoiseAlert`.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`
/// whose `mo` field points to a valid `mobj_t`.
#[no_mangle]
pub unsafe extern "C" fn P_FireWeapon(player: *mut PlayerT) {
    if P_CheckAmmo(player) == 0 {
        return;
    }

    P_SetMobjState((*player).mo as *mut mobj_t, S_PLAY_ATK1);
    let newstate = weaponinfo[(*player).readyweapon as usize].atkstate;
    P_SetPsprite(player, 0, newstate);
    P_NoiseAlert((*player).mo as *mut mobj_t, (*player).mo as *mut mobj_t);
}

/// Begin lowering the current weapon (called on player death or weapon switch).
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`.
#[no_mangle]
pub unsafe extern "C" fn P_DropWeapon(player: *mut PlayerT) {
    P_SetPsprite(
        player,
        0,
        weaponinfo[(*player).readyweapon as usize].downstate,
    );
}

/// Weapon action: idle state — bob the weapon, check for fire or weapon
/// change, and return the player mobj to the walking state if it is still in
/// an attack state.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`
/// whose `mo` field points to a valid `mobj_t`.  `psp` must point to the
/// weapon psprite slot.
#[no_mangle]
pub unsafe extern "C" fn A_WeaponReady(player: *mut PlayerT, psp: *mut PspdefT) {
    let mo = (*player).mo as *mut mobj_t;

    // Get out of attack state.
    let state_ptr = (*mo).state as *mut State;
    if std::ptr::eq(state_ptr, &info::states[S_PLAY_ATK1 as usize])
        || std::ptr::eq(state_ptr, &info::states[S_PLAY_ATK2 as usize])
    {
        P_SetMobjState(mo, S_PLAY);
    }

    if (*player).readyweapon == wp_chainsaw
        && std::ptr::eq((*psp).state as *mut State, &info::states[S_SAW as usize])
    {
        S_StartSound(mo as *mut c_void, Sfx::Sawidl as c_int);
    }

    // Check for change: if player is dead, put the weapon away.
    if (*player).pendingweapon != wp_nochange || (*player).health == 0 {
        let newstate = weaponinfo[(*player).readyweapon as usize].downstate;
        P_SetPsprite(player, 0, newstate);
        return;
    }

    // Check for fire: the missile launcher and bfg do not auto fire.
    if (*player).cmd.buttons & BT_ATTACK != 0 {
        if (*player).attackdown == 0
            || ((*player).readyweapon != wp_missile && (*player).readyweapon != wp_bfg)
        {
            (*player).attackdown = 1;
            P_FireWeapon(player);
            return;
        }
    } else {
        (*player).attackdown = 0;
    }

    // Bob the weapon based on movement speed.
    let mut angle = (128 * leveltime) as u32 & FINEMASK as u32;
    (*psp).sx = FRACUNIT + FixedMul((*player).bob, *finecosine.0.add(angle as usize));
    angle &= (FINEANGLES / 2 - 1) as u32;
    (*psp).sy = WEAPONTOP + FixedMul((*player).bob, finesine[angle as usize]);
}

/// Weapon action: allow re-firing without fully lowering the weapon.
///
/// If the attack button is still held and no weapon change is pending, increment
/// `refire` and call `P_FireWeapon`; otherwise reset `refire` and check ammo.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`.
#[no_mangle]
pub unsafe extern "C" fn A_ReFire(player: *mut PlayerT, _psp: *mut PspdefT) {
    if (*player).cmd.buttons & BT_ATTACK != 0
        && (*player).pendingweapon == wp_nochange
        && (*player).health != 0
    {
        (*player).refire += 1;
        P_FireWeapon(player);
    } else {
        (*player).refire = 0;
        P_CheckAmmo(player);
    }
}

/// Weapon action: check ammo and switch weapons if insufficient.
///
/// Used by the super shotgun after firing to ensure the player still has
/// shells before returning to the ready state.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`.
#[no_mangle]
pub unsafe extern "C" fn A_CheckReload(player: *mut PlayerT, _psp: *mut PspdefT) {
    P_CheckAmmo(player);
}

/// Weapon action: scroll the weapon psprite down by `LOWERSPEED` each tic.
///
/// When the sprite reaches `WEAPONBOTTOM`, switches to the pending weapon
/// (or parks the weapon off-screen if the player is dead).
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`.
/// `psp` must point to the weapon psprite slot.
#[no_mangle]
pub unsafe extern "C" fn A_Lower(player: *mut PlayerT, psp: *mut PspdefT) {
    (*psp).sy += LOWERSPEED;

    // Is already down.
    if (*psp).sy < WEAPONBOTTOM {
        return;
    }

    // Player is dead.
    if (*player).playerstate == PST_DEAD {
        (*psp).sy = WEAPONBOTTOM;
        return;
    }

    // The old weapon has been lowered off the screen, so change the weapon
    // and start raising it.
    if (*player).health == 0 {
        P_SetPsprite(player, 0, S_NULL);
        return;
    }

    (*player).readyweapon = (*player).pendingweapon;
    P_BringUpWeapon(player);
}

/// Weapon action: scroll the weapon psprite up by `RAISESPEED` each tic.
///
/// When the sprite reaches `WEAPONTOP`, transitions to the weapon's ready
/// state.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`.
/// `psp` must point to the weapon psprite slot.
#[no_mangle]
pub unsafe extern "C" fn A_Raise(player: *mut PlayerT, psp: *mut PspdefT) {
    (*psp).sy -= RAISESPEED;

    if (*psp).sy > WEAPONTOP {
        return;
    }

    (*psp).sy = WEAPONTOP;

    let newstate = weaponinfo[(*player).readyweapon as usize].readystate;
    P_SetPsprite(player, 0, newstate);
}

/// Weapon action: set the player mobj to attack state 2 and activate the
/// muzzle-flash psprite.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`
/// whose `mo` field points to a valid `mobj_t`.
#[no_mangle]
pub unsafe extern "C" fn A_GunFlash(player: *mut PlayerT, _psp: *mut PspdefT) {
    let mo = (*player).mo as *mut mobj_t;
    P_SetMobjState(mo, S_PLAY_ATK2);
    P_SetPsprite(
        player,
        1,
        weaponinfo[(*player).readyweapon as usize].flashstate,
    );
}

/// Weapon action: perform a fist punch in the player's facing direction.
///
/// Damage is 2-20 (×10 with Berserk).  If a target is hit, the player turns
/// to face it.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`
/// whose `mo` field points to a valid `mobj_t`.
#[no_mangle]
pub unsafe extern "C" fn A_Punch(player: *mut PlayerT, _psp: *mut PspdefT) {
    let mo = (*player).mo as *mut mobj_t;

    let mut damage = ((P_Random() % 10) + 1) << 1;
    if (*player).powers[pw_strength] != 0 {
        damage *= 10;
    }

    let mut angle = (*mo).angle;
    angle = angle.wrapping_add(((P_Random() - P_Random()) as u32) << 18);
    let slope = P_AimLineAttack(mo as *mut _ as *mut CffiMobj, angle, MELEERANGE);
    P_LineAttack(
        mo as *mut _ as *mut CffiMobj,
        angle,
        MELEERANGE,
        slope,
        damage,
    );

    // Turn to face target.
    if !linetarget.is_null() {
        S_StartSound(mo as *mut c_void, Sfx::Punch as c_int);
        (*mo).angle = R_PointToAngle2((*mo).x, (*mo).y, (*linetarget).x, (*linetarget).y);
    }
}

/// Weapon action: perform a chainsaw attack.
///
/// Uses `MELEERANGE + 1` so the hit-puff does not skip the saw flash.  If
/// a target is hit, the player's angle is guided toward it gradually.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`
/// whose `mo` field points to a valid `mobj_t`.
#[no_mangle]
pub unsafe extern "C" fn A_Saw(player: *mut PlayerT, _psp: *mut PspdefT) {
    let mo = (*player).mo as *mut mobj_t;

    let damage = 2 * ((P_Random() % 10) + 1);
    let mut angle = (*mo).angle;
    angle = angle.wrapping_add(((P_Random() - P_Random()) as u32) << 18);

    let slope = P_AimLineAttack(mo as *mut _ as *mut CffiMobj, angle, MELEERANGE + 1);
    P_LineAttack(
        mo as *mut _ as *mut CffiMobj,
        angle,
        MELEERANGE + 1,
        slope,
        damage,
    );

    if linetarget.is_null() {
        S_StartSound(mo as *mut c_void, Sfx::Sawful as c_int);
        return;
    }
    S_StartSound(mo as *mut c_void, Sfx::Sawhit as c_int);

    // Turn to face target.
    let angle = R_PointToAngle2((*mo).x, (*mo).y, (*linetarget).x, (*linetarget).y);
    let delta = angle.wrapping_sub((*mo).angle);
    if delta > ANG180 {
        let signed_delta = delta as i32;
        if signed_delta < -(ANG90 as i32) / 20 {
            (*mo).angle = angle.wrapping_add(ANG90 / 21);
        } else {
            (*mo).angle = (*mo).angle.wrapping_sub(ANG90 / 20);
        }
    } else {
        if delta > ANG90 / 20 {
            (*mo).angle = angle.wrapping_sub(ANG90 / 21);
        } else {
            (*mo).angle = (*mo).angle.wrapping_add(ANG90 / 20);
        }
    }
    (*mo).flags |= MF_JUSTATTACKED;
}

/// Subtract `amount` from the player's ammo for slot `ammonum`, emulating
/// the original C array-overflow behaviour: if `ammonum >= NUMAMMO` the
/// excess indexes into `maxammo` instead (Dehacked compatibility).
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`.
/// `ammonum` may legally exceed `NUMAMMO - 1`; the function handles that case.
unsafe fn DecreaseAmmo(player: *mut PlayerT, ammonum: c_int, amount: c_int) {
    if ammonum < NUMAMMO as c_int {
        (*player).ammo[ammonum as usize] -= amount;
    } else {
        (*player).maxammo[(ammonum - NUMAMMO as c_int) as usize] -= amount;
    }
}

/// Weapon action: consume one rocket and spawn an `MT_ROCKET` projectile.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`
/// whose `mo` field points to a valid `mobj_t`.
#[no_mangle]
pub unsafe extern "C" fn A_FireMissile(player: *mut PlayerT, _psp: *mut PspdefT) {
    let mo = (*player).mo as *mut mobj_t;
    DecreaseAmmo(player, weaponinfo[(*player).readyweapon as usize].ammo, 1);
    P_SpawnPlayerMissile(mo, MT_ROCKET);
}

/// Weapon action: consume `DEH_DEFAULT_BFG_CELLS_PER_SHOT` cells and spawn
/// an `MT_BFG` projectile.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`
/// whose `mo` field points to a valid `mobj_t`.
#[no_mangle]
pub unsafe extern "C" fn A_FireBFG(player: *mut PlayerT, _psp: *mut PspdefT) {
    let mo = (*player).mo as *mut mobj_t;
    DecreaseAmmo(
        player,
        weaponinfo[(*player).readyweapon as usize].ammo,
        DEH_DEFAULT_BFG_CELLS_PER_SHOT,
    );
    P_SpawnPlayerMissile(mo, MT_BFG);
}

/// Weapon action: consume one cell, randomise the flash frame, and spawn an
/// `MT_PLASMA` projectile.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`
/// whose `mo` field points to a valid `mobj_t`.
#[no_mangle]
pub unsafe extern "C" fn A_FirePlasma(player: *mut PlayerT, _psp: *mut PspdefT) {
    let mo = (*player).mo as *mut mobj_t;
    DecreaseAmmo(player, weaponinfo[(*player).readyweapon as usize].ammo, 1);

    P_SetPsprite(
        player,
        1,
        weaponinfo[(*player).readyweapon as usize].flashstate + (P_Random() & 1),
    );

    P_SpawnPlayerMissile(mo, MT_PLASMA);
}

/// Compute `bulletslope` by aiming in the player's facing direction and two
/// small side offsets so that near-miss shots stay roughly at the intended
/// target's height.
///
/// # Safety
///
/// `mo` must be a valid, non-null pointer to an initialised `mobj_t`.
#[no_mangle]
pub unsafe extern "C" fn P_BulletSlope(mo: *mut mobj_t) {
    let mut an = (*mo).angle;
    bulletslope = P_AimLineAttack(mo as *mut _ as *mut CffiMobj, an, 16 * 64 * FRACUNIT);

    if linetarget.is_null() {
        an = an.wrapping_add(1 << 26);
        bulletslope = P_AimLineAttack(mo as *mut _ as *mut CffiMobj, an, 16 * 64 * FRACUNIT);
        if linetarget.is_null() {
            an = an.wrapping_sub(2 << 26);
            bulletslope = P_AimLineAttack(mo as *mut _ as *mut CffiMobj, an, 16 * 64 * FRACUNIT);
        }
    }
}

/// Fire a single hitscan bullet from `mo`, using the pre-computed
/// `bulletslope`.  If `accurate` is zero, a random horizontal spread is
/// applied.
///
/// # Safety
///
/// `mo` must be a valid, non-null pointer to an initialised `mobj_t`.
/// `P_BulletSlope` must have been called before this function so that
/// `bulletslope` is valid.
#[no_mangle]
pub unsafe extern "C" fn P_GunShot(mo: *mut mobj_t, accurate: c_int) {
    let damage = 5 * ((P_Random() % 3) + 1);
    let mut angle = (*mo).angle;

    if accurate == 0 {
        angle = angle.wrapping_add(((P_Random() - P_Random()) as u32) << 18);
    }

    P_LineAttack(
        mo as *mut _ as *mut CffiMobj,
        angle,
        MISSILERANGE,
        bulletslope,
        damage,
    );
}

/// Weapon action: fire the pistol (one accurate bullet, then spread on refire).
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`
/// whose `mo` field points to a valid `mobj_t`.
#[no_mangle]
pub unsafe extern "C" fn A_FirePistol(player: *mut PlayerT, _psp: *mut PspdefT) {
    let mo = (*player).mo as *mut mobj_t;
    S_StartSound(mo as *mut c_void, Sfx::Pistol as c_int);

    P_SetMobjState(mo, S_PLAY_ATK2);
    DecreaseAmmo(player, weaponinfo[(*player).readyweapon as usize].ammo, 1);

    P_SetPsprite(
        player,
        1,
        weaponinfo[(*player).readyweapon as usize].flashstate,
    );

    P_BulletSlope(mo);
    P_GunShot(mo, ((*player).refire == 0) as c_int);
}

/// Weapon action: fire the shotgun (7 inaccurate pellets, 1 shell consumed).
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`
/// whose `mo` field points to a valid `mobj_t`.
#[no_mangle]
pub unsafe extern "C" fn A_FireShotgun(player: *mut PlayerT, _psp: *mut PspdefT) {
    let mo = (*player).mo as *mut mobj_t;
    S_StartSound(mo as *mut c_void, Sfx::Shotgn as c_int);
    P_SetMobjState(mo, S_PLAY_ATK2);

    DecreaseAmmo(player, weaponinfo[(*player).readyweapon as usize].ammo, 1);

    P_SetPsprite(
        player,
        1,
        weaponinfo[(*player).readyweapon as usize].flashstate,
    );

    P_BulletSlope(mo);

    for _ in 0..7 {
        P_GunShot(mo, 0);
    }
}

/// Weapon action: fire the super shotgun (20 pellets with extra spread, 2
/// shells consumed).
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`
/// whose `mo` field points to a valid `mobj_t`.
#[no_mangle]
pub unsafe extern "C" fn A_FireShotgun2(player: *mut PlayerT, _psp: *mut PspdefT) {
    let mo = (*player).mo as *mut mobj_t;
    S_StartSound(mo as *mut c_void, Sfx::Dshtgn as c_int);
    P_SetMobjState(mo, S_PLAY_ATK2);

    DecreaseAmmo(player, weaponinfo[(*player).readyweapon as usize].ammo, 2);

    P_SetPsprite(
        player,
        1,
        weaponinfo[(*player).readyweapon as usize].flashstate,
    );

    P_BulletSlope(mo);

    for _ in 0..20 {
        let damage = 5 * ((P_Random() % 3) + 1);
        let mut angle = (*mo).angle;
        angle = angle.wrapping_add(((P_Random() - P_Random()) as u32) << 19);
        P_LineAttack(
            mo as *mut _ as *mut CffiMobj,
            angle,
            MISSILERANGE,
            bulletslope + (((P_Random() - P_Random()) as c_int) << 5),
            damage,
        );
    }
}

/// Weapon action: fire one chaingun bullet, alternating the flash frame
/// between `S_CHAIN1` and `S_CHAIN2` to match the current psprite state.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`
/// whose `mo` field points to a valid `mobj_t`.  `psp` must point to the
/// weapon psprite slot which must currently be in state `S_CHAIN1` or
/// `S_CHAIN2`.
#[no_mangle]
pub unsafe extern "C" fn A_FireCGun(player: *mut PlayerT, psp: *mut PspdefT) {
    let mo = (*player).mo as *mut mobj_t;
    S_StartSound(mo as *mut c_void, Sfx::Pistol as c_int);

    if (*player).ammo[weaponinfo[(*player).readyweapon as usize].ammo as usize] == 0 {
        return;
    }

    P_SetMobjState(mo, S_PLAY_ATK2);
    DecreaseAmmo(player, weaponinfo[(*player).readyweapon as usize].ammo, 1);

    let flashstate = weaponinfo[(*player).readyweapon as usize].flashstate;
    let state_offset = (*psp)
        .state
        .cast::<State>()
        .offset_from(&info::states[S_CHAIN1 as usize]);
    P_SetPsprite(player, 1, flashstate + state_offset as c_int);

    P_BulletSlope(mo);
    P_GunShot(mo, ((*player).refire == 0) as c_int);
}

/// Weapon action: clear the extra-light boost (normal sector lighting).
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`.
#[no_mangle]
pub unsafe extern "C" fn A_Light0(player: *mut PlayerT, _psp: *mut PspdefT) {
    (*player).extralight = 0;
}

/// Weapon action: set the extra-light boost to +1 (used by pistol/shotgun
/// muzzle flash).
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`.
#[no_mangle]
pub unsafe extern "C" fn A_Light1(player: *mut PlayerT, _psp: *mut PspdefT) {
    (*player).extralight = 1;
}

/// Weapon action: set the extra-light boost to +2 (used by plasma / BFG
/// muzzle flash).
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`.
#[no_mangle]
pub unsafe extern "C" fn A_Light2(player: *mut PlayerT, _psp: *mut PspdefT) {
    (*player).extralight = 2;
}

/// Mobj action: spray the BFG tracers across a 90-degree arc centered on the
/// BFG ball's angle, dealing between 15 and 120 damage to each visible enemy.
///
/// Iterates 40 evenly-spaced angles, aims from `mo->target` (the originating
/// player), and spawns an `MT_EXTRABFG` explosion object on every hit target.
///
/// # Safety
///
/// `mo` must be a valid, non-null pointer to an `mobj_t` whose `target` field
/// points to a valid player `mobj_t`.
#[no_mangle]
pub unsafe extern "C" fn A_BFGSpray(mo: *mut mobj_t) {
    for i in 0..40 {
        let an = (*mo).angle - ANG90 / 2 + (ANG90 / 40) * i as u32;

        P_AimLineAttack(
            (*mo).target as *mut _ as *mut CffiMobj,
            an,
            16 * 64 * FRACUNIT,
        );

        if linetarget.is_null() {
            continue;
        }

        P_SpawnMobj(
            (*linetarget).x,
            (*linetarget).y,
            (*linetarget).z + ((*linetarget).height >> 2),
            MT_EXTRABFG,
        );

        let mut damage = 0;
        for _ in 0..15 {
            damage += (P_Random() & 7) + 1;
        }

        P_DamageMobj(
            linetarget as *mut mobj_t,
            (*mo).target,
            (*mo).target,
            damage,
        );
    }
}

/// Weapon action: play the BFG charging sound.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`
/// whose `mo` field points to a valid `mobj_t`.
#[no_mangle]
pub unsafe extern "C" fn A_BFGsound(player: *mut PlayerT, _psp: *mut PspdefT) {
    S_StartSound((*player).mo as *mut c_void, Sfx::Bfg as c_int);
}

/// Initialise the player's psprite slots and begin raising the current weapon.
///
/// Called at the start of each level for every active player.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`.
#[no_mangle]
pub unsafe extern "C" fn P_SetupPsprites(player: *mut PlayerT) {
    for i in 0..NUMPSPRITES {
        (*player).psprites[i].state = std::ptr::null_mut();
    }

    (*player).pendingweapon = (*player).readyweapon;
    P_BringUpWeapon(player);
}

/// Advance the psprite state machine for all slots each tic and copy the
/// weapon slot's position to the flash slot.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to an initialised `PlayerT`.
#[no_mangle]
pub unsafe extern "C" fn P_MovePsprites(player: *mut PlayerT) {
    let mut psp = (*player).psprites.as_mut_ptr();

    for i in 0..NUMPSPRITES {
        let state = (*psp).state as *mut State;
        if !state.is_null() {
            // Drop tic count and possibly change state.
            // A -1 tic count never changes.
            if (*psp).tics != -1 {
                (*psp).tics -= 1;
                if (*psp).tics == 0 {
                    P_SetPsprite(player, i as c_int, (*state).nextstate);
                }
            }
        }
        psp = psp.add(1);
    }

    (*player).psprites[1].sx = (*player).psprites[0].sx;
    (*player).psprites[1].sy = (*player).psprites[0].sy;
}

/// Dummy function whose body references every `#[no_mangle]` symbol exported
/// by this module so that link-time dead-code elimination cannot strip them.
///
/// Must be reachable from at least one path the linker considers live; it is
/// never actually called at runtime.
#[no_mangle]
pub extern "C" fn P_Pspr_Link_Anchor() {
    unsafe {
        let _ = swingx as usize;
        let _ = swingy as usize;
        let _ = bulletslope as usize;
    }
    let _ = P_SetPsprite as *const () as usize;
    let _ = P_CalcSwing as *const () as usize;
    let _ = P_BringUpWeapon as *const () as usize;
    let _ = P_CheckAmmo as *const () as usize;
    let _ = P_FireWeapon as *const () as usize;
    let _ = P_DropWeapon as *const () as usize;
    let _ = A_WeaponReady as *const () as usize;
    let _ = A_ReFire as *const () as usize;
    let _ = A_CheckReload as *const () as usize;
    let _ = A_Lower as *const () as usize;
    let _ = A_Raise as *const () as usize;
    let _ = A_GunFlash as *const () as usize;
    let _ = A_Punch as *const () as usize;
    let _ = A_Saw as *const () as usize;
    let _ = A_FireMissile as *const () as usize;
    let _ = A_FireBFG as *const () as usize;
    let _ = A_FirePlasma as *const () as usize;
    let _ = P_BulletSlope as *const () as usize;
    let _ = P_GunShot as *const () as usize;
    let _ = A_FirePistol as *const () as usize;
    let _ = A_FireShotgun as *const () as usize;
    let _ = A_FireShotgun2 as *const () as usize;
    let _ = A_FireCGun as *const () as usize;
    let _ = A_Light0 as *const () as usize;
    let _ = A_Light1 as *const () as usize;
    let _ = A_Light2 as *const () as usize;
    let _ = A_BFGSpray as *const () as usize;
    let _ = A_BFGsound as *const () as usize;
    let _ = P_SetupPsprites as *const () as usize;
    let _ = P_MovePsprites as *const () as usize;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn constants_match_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(LOWERSPEED, FRACUNIT * 6);
        assert_eq!(RAISESPEED, FRACUNIT * 6);
        assert_eq!(WEAPONBOTTOM, 128 * FRACUNIT);
        assert_eq!(WEAPONTOP, 32 * FRACUNIT);
        assert_eq!(MELEERANGE, 64 * FRACUNIT);
        assert_eq!(MISSILERANGE, 32 * 64 * FRACUNIT);
    }

    #[test]
    fn weapon_constants_match() {
        assert_eq!(wp_fist, 0);
        assert_eq!(wp_pistol, 1);
        assert_eq!(wp_shotgun, 2);
        assert_eq!(wp_chaingun, 3);
        assert_eq!(wp_missile, 4);
        assert_eq!(wp_plasma, 5);
        assert_eq!(wp_bfg, 6);
        assert_eq!(wp_chainsaw, 7);
        assert_eq!(wp_supershotgun, 8);
        assert_eq!(wp_nochange, NUMWEAPONS as c_int);
    }

    #[test]
    fn swing_defaults_to_zero() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert_eq!(swingx, 0);
            assert_eq!(swingy, 0);
        }
    }

    #[test]
    fn bulletslope_defaults_to_zero() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert_eq!(bulletslope, 0);
        }
    }

    /// Regression tests for mobjtype constants used by weapon fire functions.
    /// These caught an off-by-one bug where MT_ROCKET/MT_PLASMA/MT_BFG were
    /// all 1 too high, causing A_FireMissile to spawn plasma balls instead
    /// of rockets (which then crashed on the missing PLSS sprite in the
    /// shareware WAD).
    #[test]
    fn mobjtype_constants_match_info() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(MT_ROCKET, 33);
        assert_eq!(MT_PLASMA, 34);
        assert_eq!(MT_BFG, 35);
        assert_eq!(MT_PUFF, 37);
        assert_eq!(MT_EXTRABFG, 42);
    }

    /// Verify that the mobjinfo table entries at the projectile indices
    /// have the expected spawnstates.  This catches index-vs-table drift.
    #[test]
    fn projectile_mobjinfo_entries() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert_eq!(
                info::mobjinfo[MT_ROCKET as usize].spawnstate,
                S_ROCKET,
                "mobjinfo[MT_ROCKET] should spawn S_ROCKET"
            );
            assert_eq!(
                info::mobjinfo[MT_PLASMA as usize].spawnstate,
                S_PLASBALL,
                "mobjinfo[MT_PLASMA] should spawn S_PLASBALL"
            );
            assert_eq!(
                info::mobjinfo[MT_BFG as usize].spawnstate,
                S_BFGSHOT,
                "mobjinfo[MT_BFG] should spawn S_BFGSHOT"
            );
            assert_eq!(
                info::mobjinfo[MT_EXTRABFG as usize].spawnstate,
                S_BFGEXP,
                "mobjinfo[MT_EXTRABFG] should spawn S_BFGEXP"
            );
        }
    }
}
