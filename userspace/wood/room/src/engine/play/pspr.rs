//! Rust port of `vendor/doomgeneric/p_pspr.c`.
//!
//! Weapon sprite animation, weapon objects, and action functions for weapons.
//! Each player has a two-slot psprite array (weapon overlay + muzzle flash)
//! driven by the same state machine used for map objects.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::engine::sound::info::Sfx;

use crate::engine::{
    doom::{
        items::weaponinfo,
        mode::{commercial, shareware},
        player::{NUMAMMO, NUMPSPRITES, NUMWEAPONS, PlayerId, PspdefT},
        stat::gamemode,
    },
    global::Global,
    math::{
        fixed::{FRACBITS, FRACUNIT, FixedMul, fixed_t},
        tables::{FINEANGLES, FINEMASK, finecosine, finesine},
    },
    misc::random::P_Random,
    play::{info::*, mobj::MobjId, tick::leveltime},
    sound::S_StartSound,
    world::World,
};

/// Weapon-sprite lower speed (LOWERSPEED in p_pspr.c = FRACUNIT × 6).
pub const LOWERSPEED: i32 = FRACUNIT * 6;
/// Weapon-sprite raise speed (RAISESPEED in p_pspr.c = FRACUNIT × 6).
pub const RAISESPEED: i32 = FRACUNIT * 6;
/// Y-position of the weapon at its lowest (off-screen) rest point
/// (WEAPONBOTTOM in p_pspr.c = 128 × FRACUNIT).
pub const WEAPONBOTTOM: i32 = 128 * FRACUNIT;
/// Y-position of the weapon at its highest (ready) position
/// (WEAPONTOP in p_pspr.c = 32 × FRACUNIT).
pub const WEAPONTOP: i32 = 32 * FRACUNIT;

// Weapon type constants (from doomdef.h)
/// Weapon slot index for the fist (melee, no ammo).
const wp_fist: i32 = 0;
/// Weapon slot index for the pistol.
const wp_pistol: i32 = 1;
/// Weapon slot index for the shotgun.
const wp_shotgun: i32 = 2;
/// Weapon slot index for the chaingun.
const wp_chaingun: i32 = 3;
/// Weapon slot index for the rocket launcher.
const wp_missile: i32 = 4;
/// Weapon slot index for the plasma rifle.
const wp_plasma: i32 = 5;
/// Weapon slot index for the BFG 9000.
const wp_bfg: i32 = 6;
/// Weapon slot index for the chainsaw (no ammo).
const wp_chainsaw: i32 = 7;
/// Weapon slot index for the super shotgun (Doom II only).
const wp_supershotgun: i32 = 8;
/// Sentinel value meaning no pending weapon change is queued.
const wp_nochange: i32 = NUMWEAPONS as i32;

// Ammo type constants
/// Ammo type index meaning the weapon uses no ammo.
const am_noammo: i32 = 5;
/// Ammo type index for bullets (clip).
const am_clip: i32 = 0;
/// Ammo type index for shells.
const am_shell: i32 = 1;
/// Ammo type index for cells (plasma / BFG).
const am_cell: i32 = 2;
/// Ammo type index for missiles (rockets).
const am_misl: i32 = 3;

// Power type constants
/// Power-up slot index for Berserk (strength); multiplies fist damage by 10.
const pw_strength: usize = 1;

// Button constants
/// Bit mask in `ticcmd_t::buttons` that signals the attack button is held.
const BT_ATTACK: u8 = 1;

// Player state constants
/// Player state value for a dead player (`PST_DEAD`).
const PST_DEAD: i32 = 1;

// Angle constants
/// Binary-angle for 90 degrees (0x40000000).
const ANG90: u32 = 0x40000000;
/// Binary-angle for 180 degrees (0x80000000).
const ANG180: u32 = 0x80000000;

// Range constants (from p_local.h)
/// Maximum reach for melee attacks, in fixed-point world units (64 map units).
const MELEERANGE: i32 = 64 * FRACUNIT;
/// Maximum range for hitscan (bullet) attacks (32 * 64 map units).
const MISSILERANGE: i32 = 32 * 64 * FRACUNIT;

// Dehacked default
/// Default number of cells consumed per BFG shot (Dehacked-patchable in C; hardcoded here).
const DEH_DEFAULT_BFG_CELLS_PER_SHOT: i32 = 40;

use crate::engine::{
    play::{
        enemy::P_NoiseAlert,
        inter::P_DamageMobj,
        map::{P_AimLineAttack, P_LineAttack, linetarget},
        mobj::{P_SetMobjState, P_SpawnMobj, P_SpawnPlayerMissile, P_SubstNullMobj},
    },
    render_3d::main::R_PointToAngle2,
};

/// Weapon bob and aiming state (`p_pspr.c` globals).
pub struct PsprState {
    /// Horizontal weapon-bob offset; updated each tic by `P_CalcSwing`.
    pub swingx: fixed_t,
    /// Vertical weapon-bob offset; updated each tic by `P_CalcSwing`.
    pub swingy: fixed_t,
    /// Slope set by `P_BulletSlope` for near-miss aiming; read by `P_GunShot`.
    pub bulletslope: fixed_t,
}

impl PsprState {
    /// The state at program start: no bob, level aim.
    pub const fn new() -> Self {
        Self {
            swingx: 0,
            swingy: 0,
            bulletslope: 0,
        }
    }
}

impl Default for PsprState {
    fn default() -> Self {
        Self::new()
    }
}

/// Weapon bob and aiming state.
pub static PSPR: Global<PsprState> = Global::new(PsprState::new());

/// The slope computed by the last `P_BulletSlope` (C `bulletslope`).
fn bulletslope() -> fixed_t {
    PSPR.borrow().bulletslope
}

/// Transition a psprite slot to a new state, running action functions until a
/// non-zero tic count is reached or the state chain ends.
///
/// Mirrors `P_SetPsprite` in `p_pspr.c`.
///
/// # Safety
///
/// No reference into `player_id`'s slot may be live while this runs.
/// `position` must be in `0..NUMPSPRITES`.  `stnum` must be a valid state
/// index or `S_NULL` (0).
pub unsafe fn P_SetPsprite(world: &mut World, player_id: PlayerId, position: i32, stnum: i32) {
    let psp = (&raw mut (*player_id.ptr()).psprites)
        .cast::<PspdefT>()
        .add(position as usize);

    let mut stnum = stnum;
    loop {
        if stnum == 0 {
            (*psp).state = StateNum::NULL;
            break;
        }

        let state_num = StateNum::new(stnum);
        let state = state_num.get();
        (*psp).state = state_num;
        (*psp).tics = state.tics;

        if state.misc1 != 0 {
            (*psp).sx = state.misc1 << FRACBITS;
            (*psp).sy = state.misc2 << FRACBITS;
        }

        // Call action routine.
        match state.action {
            StateAction::None => {}
            StateAction::Weapon(action) => {
                action(world, player_id, psp);
                if (*psp).state.is_null() {
                    break;
                }
            }
            StateAction::Mobj(_) => {
                unreachable!("P_SetPsprite: state {stnum} has a map-object action")
            }
        }

        // Read nextstate from psp->state (action may have changed it).
        stnum = (*psp).state.get().nextstate;

        if (*psp).tics != 0 {
            break;
        }
    }
}

/// Recompute the horizontal (`swingx`) and vertical (`swingy`) weapon-bob
/// offsets for the current tic using the player's `bob` amplitude.
pub fn P_CalcSwing(_world: &mut World, player: PlayerId) {
    let swing = player.read(|player| player.bob);

    let leveltime = leveltime();
    let mut pspr = PSPR.borrow_mut();
    let mut angle = (FINEANGLES as i32 / 70 * leveltime) & FINEMASK;
    pspr.swingx = FixedMul(swing, finesine[angle as usize]);

    angle = (FINEANGLES as i32 / 70 * leveltime + FINEANGLES as i32 / 2) & FINEMASK;
    pspr.swingy = -FixedMul(pspr.swingx, finesine[angle as usize]);
}

/// Begin the raise animation for the pending weapon by setting the weapon
/// psprite to its `upstate` and positioning it at `WEAPONBOTTOM`.
///
/// # Safety
///
/// No reference into `player_id`'s slot may be live while this runs.
pub unsafe fn P_BringUpWeapon(world: &mut World, player_id: PlayerId) {
    let player = player_id.ptr();
    if (*player).pendingweapon == wp_nochange {
        (*player).pendingweapon = (*player).readyweapon;
    }

    if (*player).pendingweapon == wp_chainsaw {
        S_StartSound((*player).mo(), Sfx::Sawup as i32);
    }

    let newstate = weaponinfo[(*player).pendingweapon as usize].upstate;

    (*player).pendingweapon = wp_nochange;
    (*player).psprites[0].sy = WEAPONBOTTOM;

    P_SetPsprite(world, player_id, 0, newstate);
}

/// Return `1` if the player has enough ammo to fire the ready weapon, `0`
/// otherwise.  When out of ammo, selects the next best weapon and begins
/// lowering the current one.
///
/// # Safety
///
/// No reference into `player_id`'s slot may be live while this runs.
pub unsafe fn P_CheckAmmo(world: &mut World, player_id: PlayerId) -> i32 {
    let player = player_id.ptr();
    let ammo = weaponinfo[(*player).readyweapon as usize].ammo;

    let count: i32;
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
            && gamemode() != shareware
        {
            (*player).pendingweapon = wp_plasma;
        } else if (*player).weaponowned[wp_supershotgun as usize] != 0
            && (*player).ammo[am_shell as usize] > 2
            && gamemode() == commercial
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
            && gamemode() != shareware
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
        world,
        player_id,
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
/// `player`'s `mo` must name a map object of the loaded level, and no
/// reference into its slot may be live while this runs.
pub unsafe fn P_FireWeapon(world: &mut World, player: PlayerId) {
    if P_CheckAmmo(world, player) == 0 {
        return;
    }

    P_SetMobjState(world, player.mo(), S_PLAY_ATK1);
    let newstate = weaponinfo[player.read(|player| player.readyweapon) as usize].atkstate;
    P_SetPsprite(world, player, 0, newstate);
    P_NoiseAlert(world, player.mo(), player.mo());
}

/// Begin lowering the current weapon (called on player death or weapon switch).
///
/// # Safety
///
/// No reference into `player`'s slot may be live while this runs.
pub unsafe fn P_DropWeapon(world: &mut World, player: PlayerId) {
    P_SetPsprite(
        world,
        player,
        0,
        weaponinfo[player.read(|player| player.readyweapon) as usize].downstate,
    );
}

/// Weapon action: idle state — bob the weapon, check for fire or weapon
/// change, and return the player mobj to the walking state if it is still in
/// an attack state.
///
/// # Safety
///
/// `player_id`'s `mo` must name a map object of the loaded level, and no
/// reference into its slot may be live while this runs.  `psp` must point
/// to the weapon psprite slot.
pub unsafe fn A_WeaponReady(world: &mut World, player_id: PlayerId, psp: *mut PspdefT) {
    let player = player_id.ptr();
    let mo_id = (*player).mo();
    let mo = mo_id.ptr();

    // Get out of attack state.
    let mobj_state = (*mo).state.number();
    if mobj_state == S_PLAY_ATK1 || mobj_state == S_PLAY_ATK2 {
        P_SetMobjState(world, mo_id, S_PLAY);
    }

    if (*player).readyweapon == wp_chainsaw && (*psp).state.number() == S_SAW {
        S_StartSound(mo_id, Sfx::Sawidl as i32);
    }

    // Check for change: if player is dead, put the weapon away.
    if (*player).pendingweapon != wp_nochange || (*player).health == 0 {
        let newstate = weaponinfo[(*player).readyweapon as usize].downstate;
        P_SetPsprite(world, player_id, 0, newstate);
        return;
    }

    // Check for fire: the missile launcher and bfg do not auto fire.
    if (*player).cmd.buttons & BT_ATTACK != 0 {
        if (*player).attackdown == 0
            || ((*player).readyweapon != wp_missile && (*player).readyweapon != wp_bfg)
        {
            (*player).attackdown = 1;
            P_FireWeapon(world, player_id);
            return;
        }
    } else {
        (*player).attackdown = 0;
    }

    // Bob the weapon based on movement speed.
    let mut angle = (128 * leveltime()) as u32 & FINEMASK as u32;
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
/// `player_id`'s `mo` must name a map object of the loaded level, and no
/// reference into its slot may be live while this runs.
pub unsafe fn A_ReFire(world: &mut World, player_id: PlayerId, _psp: *mut PspdefT) {
    let player = player_id.ptr();
    if (*player).cmd.buttons & BT_ATTACK != 0
        && (*player).pendingweapon == wp_nochange
        && (*player).health != 0
    {
        (*player).refire += 1;
        P_FireWeapon(world, player_id);
    } else {
        (*player).refire = 0;
        P_CheckAmmo(world, player_id);
    }
}

/// Weapon action: check ammo and switch weapons if insufficient.
///
/// Used by the super shotgun after firing to ensure the player still has
/// shells before returning to the ready state.
///
/// # Safety
///
/// No reference into `player`'s slot may be live while this runs.
pub unsafe fn A_CheckReload(world: &mut World, player: PlayerId, _psp: *mut PspdefT) {
    P_CheckAmmo(world, player);
}

/// Weapon action: scroll the weapon psprite down by `LOWERSPEED` each tic.
///
/// When the sprite reaches `WEAPONBOTTOM`, switches to the pending weapon
/// (or parks the weapon off-screen if the player is dead).
///
/// # Safety
///
/// No reference into `player_id`'s slot may be live while this runs.
/// `psp` must point to the weapon psprite slot.
pub unsafe fn A_Lower(world: &mut World, player_id: PlayerId, psp: *mut PspdefT) {
    let player = player_id.ptr();
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
        P_SetPsprite(world, player_id, 0, S_NULL);
        return;
    }

    (*player).readyweapon = (*player).pendingweapon;
    P_BringUpWeapon(world, player_id);
}

/// Weapon action: scroll the weapon psprite up by `RAISESPEED` each tic.
///
/// When the sprite reaches `WEAPONTOP`, transitions to the weapon's ready
/// state.
///
/// # Safety
///
/// No reference into `player`'s slot may be live while this runs.
/// `psp` must point to the weapon psprite slot.
pub unsafe fn A_Raise(world: &mut World, player: PlayerId, psp: *mut PspdefT) {
    (*psp).sy -= RAISESPEED;

    if (*psp).sy > WEAPONTOP {
        return;
    }

    (*psp).sy = WEAPONTOP;

    let newstate = weaponinfo[player.read(|player| player.readyweapon) as usize].readystate;
    P_SetPsprite(world, player, 0, newstate);
}

/// Weapon action: set the player mobj to attack state 2 and activate the
/// muzzle-flash psprite.
///
/// # Safety
///
/// `player`'s `mo` must name a map object of the loaded level, and no
/// reference into its slot may be live while this runs.
pub unsafe fn A_GunFlash(world: &mut World, player: PlayerId, _psp: *mut PspdefT) {
    let mo = player.mo();
    P_SetMobjState(world, mo, S_PLAY_ATK2);
    P_SetPsprite(
        world,
        player,
        1,
        weaponinfo[player.read(|player| player.readyweapon) as usize].flashstate,
    );
}

/// Weapon action: perform a fist punch in the player's facing direction.
///
/// Damage is 2-20 (×10 with Berserk).  If a target is hit, the player turns
/// to face it.
///
/// # Safety
///
/// `player`'s `mo` must name a map object of the loaded level, and no
/// reference into its slot may be live while this runs.
pub unsafe fn A_Punch(world: &mut World, player: PlayerId, _psp: *mut PspdefT) {
    let mo_id = player.mo();
    let mo = mo_id.ptr();

    let mut damage = ((P_Random() % 10) + 1) << 1;
    if player.read(|player| player.powers[pw_strength]) != 0 {
        damage *= 10;
    }

    let mut angle = (*mo).angle;
    angle = angle.wrapping_add(((P_Random() - P_Random()) as u32) << 18);
    let slope = P_AimLineAttack(world, mo_id, angle, MELEERANGE);
    P_LineAttack(world, mo_id, angle, MELEERANGE, slope, damage);

    // Turn to face target.
    if let Some(target) = linetarget() {
        let target = target.ptr();
        S_StartSound(mo_id, Sfx::Punch as i32);
        (*mo).angle = R_PointToAngle2((*mo).x, (*mo).y, (*target).x, (*target).y);
    }
}

/// Weapon action: perform a chainsaw attack.
///
/// Uses `MELEERANGE + 1` so the hit-puff does not skip the saw flash.  If
/// a target is hit, the player's angle is guided toward it gradually.
///
/// # Safety
///
/// `player`'s `mo` must name a map object of the loaded level, and no
/// reference into its slot may be live while this runs.
pub unsafe fn A_Saw(world: &mut World, player: PlayerId, _psp: *mut PspdefT) {
    let mo_id = player.mo();
    let mo = mo_id.ptr();

    let damage = 2 * ((P_Random() % 10) + 1);
    let mut angle = (*mo).angle;
    angle = angle.wrapping_add(((P_Random() - P_Random()) as u32) << 18);

    let slope = P_AimLineAttack(world, mo_id, angle, MELEERANGE + 1);
    P_LineAttack(world, mo_id, angle, MELEERANGE + 1, slope, damage);

    let Some(target) = linetarget() else {
        S_StartSound(mo_id, Sfx::Sawful as i32);
        return;
    };
    let target = target.ptr();
    S_StartSound(mo_id, Sfx::Sawhit as i32);

    // Turn to face target.
    let angle = R_PointToAngle2((*mo).x, (*mo).y, (*target).x, (*target).y);
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
fn DecreaseAmmo(player: PlayerId, ammonum: i32, amount: i32) {
    player.write(|player| {
        if ammonum < NUMAMMO as i32 {
            player.ammo[ammonum as usize] -= amount;
        } else {
            player.maxammo[(ammonum - NUMAMMO as i32) as usize] -= amount;
        }
    });
}

/// The player's ready weapon (C `player->readyweapon`).
fn ready_weapon(player: PlayerId) -> usize {
    player.read(|player| player.readyweapon) as usize
}

/// Weapon action: consume one rocket and spawn an `MT_ROCKET` projectile.
///
/// # Safety
///
/// `player`'s `mo` must name a map object of the loaded level, and no
/// reference into its slot may be live while this runs.
pub unsafe fn A_FireMissile(world: &mut World, player: PlayerId, _psp: *mut PspdefT) {
    let mo = player.mo();
    DecreaseAmmo(player, weaponinfo[ready_weapon(player)].ammo, 1);
    P_SpawnPlayerMissile(world, mo, MT_ROCKET);
}

/// Weapon action: consume `DEH_DEFAULT_BFG_CELLS_PER_SHOT` cells and spawn
/// an `MT_BFG` projectile.
///
/// # Safety
///
/// `player`'s `mo` must name a map object of the loaded level, and no
/// reference into its slot may be live while this runs.
pub unsafe fn A_FireBFG(world: &mut World, player: PlayerId, _psp: *mut PspdefT) {
    let mo = player.mo();
    DecreaseAmmo(
        player,
        weaponinfo[ready_weapon(player)].ammo,
        DEH_DEFAULT_BFG_CELLS_PER_SHOT,
    );
    P_SpawnPlayerMissile(world, mo, MT_BFG);
}

/// Weapon action: consume one cell, randomise the flash frame, and spawn an
/// `MT_PLASMA` projectile.
///
/// # Safety
///
/// `player`'s `mo` must name a map object of the loaded level, and no
/// reference into its slot may be live while this runs.
pub unsafe fn A_FirePlasma(world: &mut World, player: PlayerId, _psp: *mut PspdefT) {
    let mo = player.mo();
    DecreaseAmmo(player, weaponinfo[ready_weapon(player)].ammo, 1);

    P_SetPsprite(
        world,
        player,
        1,
        weaponinfo[ready_weapon(player)].flashstate + (P_Random() & 1),
    );

    P_SpawnPlayerMissile(world, mo, MT_PLASMA);
}

/// Compute `bulletslope` by aiming in the player's facing direction and two
/// small side offsets so that near-miss shots stay roughly at the intended
/// target's height.
///
/// # Safety
///
/// `mo` must name an initialised map object of the level.
pub unsafe fn P_BulletSlope(world: &mut World, mo: MobjId) {
    let mut an = (*mo.ptr()).angle;
    PSPR.borrow_mut().bulletslope = P_AimLineAttack(world, mo, an, 16 * 64 * FRACUNIT);

    if linetarget().is_none() {
        an = an.wrapping_add(1 << 26);
        PSPR.borrow_mut().bulletslope = P_AimLineAttack(world, mo, an, 16 * 64 * FRACUNIT);
        if linetarget().is_none() {
            an = an.wrapping_sub(2 << 26);
            PSPR.borrow_mut().bulletslope = P_AimLineAttack(world, mo, an, 16 * 64 * FRACUNIT);
        }
    }
}

/// Fire a single hitscan bullet from `mo`, using the pre-computed
/// `bulletslope`.  If `accurate` is zero, a random horizontal spread is
/// applied.
///
/// # Safety
///
/// `mo` must name an initialised map object of the level.
/// `P_BulletSlope` must have been called before this function so that
/// `bulletslope` is valid.
pub unsafe fn P_GunShot(world: &mut World, mo: MobjId, accurate: i32) {
    let damage = 5 * ((P_Random() % 3) + 1);
    let mut angle = (*mo.ptr()).angle;

    if accurate == 0 {
        angle = angle.wrapping_add(((P_Random() - P_Random()) as u32) << 18);
    }

    P_LineAttack(world, mo, angle, MISSILERANGE, bulletslope(), damage);
}

/// Weapon action: fire the pistol (one accurate bullet, then spread on refire).
///
/// # Safety
///
/// `player`'s `mo` must name a map object of the loaded level, and no
/// reference into its slot may be live while this runs.
pub unsafe fn A_FirePistol(world: &mut World, player: PlayerId, _psp: *mut PspdefT) {
    let mo = player.mo();
    S_StartSound(mo, Sfx::Pistol as i32);

    P_SetMobjState(world, mo, S_PLAY_ATK2);
    DecreaseAmmo(player, weaponinfo[ready_weapon(player)].ammo, 1);

    P_SetPsprite(
        world,
        player,
        1,
        weaponinfo[ready_weapon(player)].flashstate,
    );

    P_BulletSlope(world, mo);
    P_GunShot(world, mo, (player.read(|player| player.refire) == 0) as i32);
}

/// Weapon action: fire the shotgun (7 inaccurate pellets, 1 shell consumed).
///
/// # Safety
///
/// `player`'s `mo` must name a map object of the loaded level, and no
/// reference into its slot may be live while this runs.
pub unsafe fn A_FireShotgun(world: &mut World, player: PlayerId, _psp: *mut PspdefT) {
    let mo = player.mo();
    S_StartSound(mo, Sfx::Shotgn as i32);
    P_SetMobjState(world, mo, S_PLAY_ATK2);

    DecreaseAmmo(player, weaponinfo[ready_weapon(player)].ammo, 1);

    P_SetPsprite(
        world,
        player,
        1,
        weaponinfo[ready_weapon(player)].flashstate,
    );

    P_BulletSlope(world, mo);

    for _ in 0..7 {
        P_GunShot(world, mo, 0);
    }
}

/// Weapon action: fire the super shotgun (20 pellets with extra spread, 2
/// shells consumed).
///
/// # Safety
///
/// `player`'s `mo` must name a map object of the loaded level, and no
/// reference into its slot may be live while this runs.
pub unsafe fn A_FireShotgun2(world: &mut World, player: PlayerId, _psp: *mut PspdefT) {
    let mo = player.mo();
    S_StartSound(mo, Sfx::Dshtgn as i32);
    P_SetMobjState(world, mo, S_PLAY_ATK2);

    DecreaseAmmo(player, weaponinfo[ready_weapon(player)].ammo, 2);

    P_SetPsprite(
        world,
        player,
        1,
        weaponinfo[ready_weapon(player)].flashstate,
    );

    P_BulletSlope(world, mo);

    for _ in 0..20 {
        let damage = 5 * ((P_Random() % 3) + 1);
        let mut angle = (*mo.ptr()).angle;
        angle = angle.wrapping_add(((P_Random() - P_Random()) as u32) << 19);
        P_LineAttack(
            world,
            mo,
            angle,
            MISSILERANGE,
            bulletslope() + ((P_Random() - P_Random()) << 5),
            damage,
        );
    }
}

/// Weapon action: fire one chaingun bullet, alternating the flash frame
/// between `S_CHAIN1` and `S_CHAIN2` to match the current psprite state.
///
/// # Safety
///
/// `player`'s `mo` must name a map object of the loaded level, and no
/// reference into its slot may be live while this runs.  `psp` must point
/// to the weapon psprite slot which must currently be in state `S_CHAIN1`
/// or `S_CHAIN2`.
pub unsafe fn A_FireCGun(world: &mut World, player: PlayerId, psp: *mut PspdefT) {
    let mo = player.mo();
    S_StartSound(mo, Sfx::Pistol as i32);

    let ammo = weaponinfo[ready_weapon(player)].ammo;
    if player.read(|player| player.ammo[ammo as usize]) == 0 {
        return;
    }

    P_SetMobjState(world, mo, S_PLAY_ATK2);
    DecreaseAmmo(player, weaponinfo[ready_weapon(player)].ammo, 1);

    let flashstate = weaponinfo[ready_weapon(player)].flashstate;
    let state_offset = (*psp).state.number() - S_CHAIN1;
    P_SetPsprite(world, player, 1, flashstate + state_offset);

    P_BulletSlope(world, mo);
    P_GunShot(world, mo, (player.read(|player| player.refire) == 0) as i32);
}

/// Weapon action: clear the extra-light boost (normal sector lighting).
pub fn A_Light0(_world: &mut World, player: PlayerId, _psp: *mut PspdefT) {
    player.write(|player| player.extralight = 0);
}

/// Weapon action: set the extra-light boost to +1 (used by pistol/shotgun
/// muzzle flash).
pub fn A_Light1(_world: &mut World, player: PlayerId, _psp: *mut PspdefT) {
    player.write(|player| player.extralight = 1);
}

/// Weapon action: set the extra-light boost to +2 (used by plasma / BFG
/// muzzle flash).
pub fn A_Light2(_world: &mut World, player: PlayerId, _psp: *mut PspdefT) {
    player.write(|player| player.extralight = 2);
}

/// Mobj action: spray the BFG tracers across a 90-degree arc centered on the
/// BFG ball's angle, dealing between 15 and 120 damage to each visible enemy.
///
/// Iterates 40 evenly-spaced angles, aims from `mo->target` (the originating
/// player), and spawns an `MT_EXTRABFG` explosion object on every hit target.
///
/// # Safety
///
/// `mo_id` must name a map object of the level; a missing `target` is
/// replaced by the dummy object, as C's `P_AimLineAttack` does.
pub unsafe fn A_BFGSpray(world: &mut World, mo_id: MobjId) {
    let mo = mo_id.ptr();
    for i in 0..40 {
        let an = (*mo).angle - ANG90 / 2 + (ANG90 / 40) * i as u32;

        P_AimLineAttack(world, P_SubstNullMobj((*mo).target), an, 16 * 64 * FRACUNIT);

        // Spawning the explosion and rolling damage do not aim, so the
        // target found above stays current.
        let Some(target_id) = linetarget() else {
            continue;
        };
        let target = target_id.ptr();

        P_SpawnMobj(
            world,
            (*target).x,
            (*target).y,
            (*target).z + ((*target).height >> 2),
            MT_EXTRABFG,
        );

        let mut damage = 0;
        for _ in 0..15 {
            damage += (P_Random() & 7) + 1;
        }

        P_DamageMobj(world, target_id, (*mo).target, (*mo).target, damage);
    }
}

/// Weapon action: play the BFG charging sound.
///
/// # Safety
///
/// `player`'s `mo` must name a map object of the loaded level.
pub unsafe fn A_BFGsound(_world: &mut World, player: PlayerId, _psp: *mut PspdefT) {
    S_StartSound(player.mo(), Sfx::Bfg as i32);
}

/// Initialise the player's psprite slots and begin raising the current weapon.
///
/// Called at the start of each level for every active player.
///
/// # Safety
///
/// No reference into `player`'s slot may be live while this runs.
pub unsafe fn P_SetupPsprites(world: &mut World, player: PlayerId) {
    player.write(|player| {
        for psprite in &mut player.psprites {
            psprite.state = StateNum::NULL;
        }

        player.pendingweapon = player.readyweapon;
    });
    P_BringUpWeapon(world, player);
}

/// Advance the psprite state machine for all slots each tic and copy the
/// weapon slot's position to the flash slot.
///
/// # Safety
///
/// No reference into `player_id`'s slot may be live while this runs.
pub unsafe fn P_MovePsprites(world: &mut World, player_id: PlayerId) {
    let player = player_id.ptr();
    let mut psp = (&raw mut (*player).psprites).cast::<PspdefT>();

    for i in 0..NUMPSPRITES {
        let state = (*psp).state;
        if !state.is_null() {
            // Drop tic count and possibly change state.
            // A -1 tic count never changes.
            if (*psp).tics != -1 {
                (*psp).tics -= 1;
                if (*psp).tics == 0 {
                    P_SetPsprite(world, player_id, i as i32, state.get().nextstate);
                }
            }
        }
        psp = psp.add(1);
    }

    (*player).psprites[1].sx = (*player).psprites[0].sx;
    (*player).psprites[1].sy = (*player).psprites[0].sy;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_c() {
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
        assert_eq!(wp_nochange, NUMWEAPONS as i32);
    }

    #[test]
    fn swing_defaults_to_zero() {
        let pspr = PsprState::new();
        assert_eq!(pspr.swingx, 0);
        assert_eq!(pspr.swingy, 0);
    }

    #[test]
    fn bulletslope_defaults_to_zero() {
        assert_eq!(PsprState::new().bulletslope, 0);
    }

    /// Regression tests for mobjtype constants used by weapon fire functions.
    /// These caught an off-by-one bug where MT_ROCKET/MT_PLASMA/MT_BFG were
    /// all 1 too high, causing A_FireMissile to spawn plasma balls instead
    /// of rockets (which then crashed on the missing PLSS sprite in the
    /// shareware WAD).
    #[test]
    fn mobjtype_constants_match_info() {
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
        {
            let mobjinfo = MOBJINFO.borrow();
            assert_eq!(
                mobjinfo[MT_ROCKET as usize].spawnstate, S_ROCKET,
                "mobjinfo[MT_ROCKET] should spawn S_ROCKET"
            );
            assert_eq!(
                mobjinfo[MT_PLASMA as usize].spawnstate, S_PLASBALL,
                "mobjinfo[MT_PLASMA] should spawn S_PLASBALL"
            );
            assert_eq!(
                mobjinfo[MT_BFG as usize].spawnstate, S_BFGSHOT,
                "mobjinfo[MT_BFG] should spawn S_BFGSHOT"
            );
            assert_eq!(
                mobjinfo[MT_EXTRABFG as usize].spawnstate, S_BFGEXP,
                "mobjinfo[MT_EXTRABFG] should spawn S_BFGEXP"
            );
        }
    }
}
