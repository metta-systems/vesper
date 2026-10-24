//! Player movement, view, and weapon logic.
//!
//! Rust port of `vendor/doomgeneric/p_user.c`. Handles per-tic player
//! thinking: movement commands from `ticcmd_t`, weapon switching, special
//! sector effects, view bobbing, and death-camera behaviour.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::engine::{
    doom::{
        mode::{commercial, shareware},
        player::{CF_NOCLIP, CF_NOMOMENTUM, PlayerId},
        stat::gamemode,
    },
    global::Global,
    math::{
        fixed::{FRACUNIT, FixedMul, fixed_t},
        tables::{ANG90, ANG180, ANGLETOFINESHIFT, FINEANGLES, finecosine, finesine},
    },
    play::{info::*, setup::LEVEL, tick::leveltime},
    world::World,
};

/// Maximum view-bob amplitude in fixed-point units (16 pixels = 0x100000).
/// Matches `MAXBOB` in `p_user.c`.
const MAXBOB: i32 = 0x100000;

/// Default player eye height above the floor in fixed-point units (41 map
/// units). Matches `VIEWHEIGHT` in `p_local.h`.
const VIEWHEIGHT: fixed_t = 41 * FRACUNIT;

/// 5 degrees expressed as a binary angle (BAM). Used in `P_DeathThink` to
/// rotate the dead player's camera toward the killer.
const ANG5: u32 = ANG90 / 18;

// ---------------------------------------------------------------------------
// Button constants (from d_event.h)
// ---------------------------------------------------------------------------

/// Button flag: this `ticcmd` carries a special (menu/cheat) event rather
/// than a normal game button. When set, all other button bits are ignored.
const BT_SPECIAL: u8 = 128;

/// Button flag: the player wants to switch weapons.
const BT_CHANGE: u8 = 4;

/// Bitmask that extracts the requested weapon index from `buttons` when
/// `BT_CHANGE` is set. Three bits wide (weapons 0-7), shifted by `BT_WEAPONSHIFT`.
const BT_WEAPONMASK: u8 = 8 + 16 + 32;

/// Number of bits to right-shift `buttons & BT_WEAPONMASK` to obtain the
/// raw weapon index.
const BT_WEAPONSHIFT: u8 = 3;

/// Button flag: the player pressed the Use/Open key.
const BT_USE: u8 = 2;

/// Button flag: the player is holding the attack button.
const BT_ATTACK: u8 = 1;

// ---------------------------------------------------------------------------
// Weapon type indices (from doomdef.h / info.h)
// ---------------------------------------------------------------------------

/// Fist weapon index.
const wp_fist: i32 = 0;
/// Pistol weapon index.
const wp_pistol: i32 = 1;
/// Single-barrel shotgun weapon index.
const wp_shotgun: i32 = 2;
/// Chaingun weapon index.
const wp_chaingun: i32 = 3;
/// Rocket launcher weapon index.
const wp_missile: i32 = 4;
/// Plasma rifle weapon index.
const wp_plasma: i32 = 5;
/// BFG 9000 weapon index.
const wp_bfg: i32 = 6;
/// Chainsaw weapon index.
const wp_chainsaw: i32 = 7;
/// Super shotgun (double-barrel) weapon index. Commercial/Doom 2 only.
const wp_supershotgun: i32 = 8;

// ---------------------------------------------------------------------------
// Power-up indices (from doomdef.h)
// ---------------------------------------------------------------------------

/// Index into `player.powers[]` for the invulnerability sphere.
const pw_invulnerability: usize = 0;
/// Index into `player.powers[]` for the berserk pack (strength).
const pw_strength: usize = 1;
/// Index into `player.powers[]` for the partial-invisibility sphere.
const pw_invisibility: usize = 2;
/// Index into `player.powers[]` for the radiation shielding suit (iron feet).
const pw_ironfeet: usize = 3;
/// Index into `player.powers[]` for the computer area map.
const pw_allmap: usize = 4;
/// Index into `player.powers[]` for the light-amplification visor (infrared).
const pw_infrared: usize = 5;

// ---------------------------------------------------------------------------
// Player state constants (from d_player.h)
// ---------------------------------------------------------------------------

/// Player state: alive and playing.
const PST_LIVE: i32 = 0;
/// Player state: dead (playing the death sequence / death-camera).
const PST_DEAD: i32 = 1;
/// Player state: ready to be reborn (respawn requested).
const PST_REBORN: i32 = 2;

// ---------------------------------------------------------------------------
// Colormap index
// ---------------------------------------------------------------------------

/// Colormap index for the full-bright inverse palette used during
/// invulnerability. Matches `INVERSECOLORMAP` in `p_user.c`.
const INVERSECOLORMAP: i32 = 32;

/// Player movement state (`p_user.c` global).
pub struct UserState {
    /// Whether the player is standing on the floor (`mo->z == mo->floorz`).
    ///
    /// Stored as a `i32` (0 or 1), as in the C source.
    pub onground: i32,
}

impl UserState {
    /// The state at program start: not on the ground.
    pub const fn new() -> Self {
        Self { onground: 0 }
    }
}

impl Default for UserState {
    fn default() -> Self {
        Self::new()
    }
}

/// Player movement state.
pub static USER: Global<UserState> = Global::new(UserState::new());

use crate::engine::{
    play::{
        map::P_UseLines, mobj::P_SetMobjState, pspr::P_MovePsprites, spec::P_PlayerInSpecialSector,
    },
    render_3d::main::R_PointToAngle2,
};

/// Apply a momentum impulse to the player's map object along `angle`.
///
/// Converts `angle` (binary angle) to a fine-angle table index, then adds
/// `move_ * cos(angle)` to `momx` and `move_ * sin(angle)` to `momy`.
/// Corresponds to `P_Thrust` in `p_user.c`.
///
/// # Safety
/// `player`'s `mo` must name a map object of the loaded level.
pub unsafe fn P_Thrust(_world: &mut World, player: PlayerId, angle: u32, move_: fixed_t) {
    unsafe {
        let mo = player.mo().ptr();
        let angle = (angle >> ANGLETOFINESHIFT) as usize;
        (*mo).momx += FixedMul(move_, *finecosine.0.add(angle));
        (*mo).momy += FixedMul(move_, finesine[angle]);
    }
}

/// Compute and set the player's view height (`viewz`) for the current tic.
///
/// Calculates the view-bob amplitude from the player's momentum, applies a
/// sinusoidal bob offset (unless `CF_NOMOMENTUM` is set or the player is
/// airborne), and clamps `viewz` to avoid clipping through the ceiling.
/// Also advances `viewheight` toward `VIEWHEIGHT` via `deltaviewheight`.
///
/// Corresponds to `P_CalcHeight` in `p_user.c`.
///
/// # Safety
/// `player_id`'s `mo` must name a map object of the loaded level.
pub unsafe fn P_CalcHeight(_world: &mut World, player_id: PlayerId) {
    unsafe {
        let player = player_id.ptr();
        let mo = (*player).mo().ptr();

        // Regular movement bobbing
        (*player).bob = FixedMul((*mo).momx, (*mo).momx) + FixedMul((*mo).momy, (*mo).momy);

        (*player).bob >>= 2;

        if (*player).bob > MAXBOB {
            (*player).bob = MAXBOB;
        }

        if ((*player).cheats & CF_NOMOMENTUM) != 0 || USER.borrow().onground == 0 {
            (*player).viewz = (*mo).z + VIEWHEIGHT;

            if (*player).viewz > (*mo).ceilingz - 4 * FRACUNIT {
                (*player).viewz = (*mo).ceilingz - 4 * FRACUNIT;
            }

            (*player).viewz = (*mo).z + (*player).viewheight;
            return;
        }

        let angle: i32 = (FINEANGLES as i32 / 20 * leveltime()) & (FINEANGLES as i32 - 1);
        let bob: fixed_t = FixedMul((*player).bob / 2, finesine[angle as usize]);

        // Move viewheight
        if (*player).playerstate == PST_LIVE {
            (*player).viewheight += (*player).deltaviewheight;

            if (*player).viewheight > VIEWHEIGHT {
                (*player).viewheight = VIEWHEIGHT;
                (*player).deltaviewheight = 0;
            }

            if (*player).viewheight < VIEWHEIGHT / 2 {
                (*player).viewheight = VIEWHEIGHT / 2;
                if (*player).deltaviewheight <= 0 {
                    (*player).deltaviewheight = 1;
                }
            }

            if (*player).deltaviewheight != 0 {
                (*player).deltaviewheight += FRACUNIT / 4;
                if (*player).deltaviewheight == 0 {
                    (*player).deltaviewheight = 1;
                }
            }
        }
        (*player).viewz = (*mo).z + (*player).viewheight + bob;

        if (*player).viewz > (*mo).ceilingz - 4 * FRACUNIT {
            (*player).viewz = (*mo).ceilingz - 4 * FRACUNIT;
        }
    }
}

/// Apply the player's movement command for one tic.
///
/// Rotates `mo->angle` by `cmd.angleturn`, sets `onground`, and calls
/// `P_Thrust` for forward and side movement when the player is on the ground.
/// Also transitions the player mobj to the `S_PLAY_RUN1` state when moving.
///
/// Corresponds to `P_MovePlayer` in `p_user.c`.
///
/// # Safety
/// `player_id`'s `mo` must name a map object of the loaded level.
pub unsafe fn P_MovePlayer(world: &mut World, player_id: PlayerId) {
    unsafe {
        let player = player_id.ptr();
        let mo = (*player).mo().ptr();
        let cmd = (*player).cmd;

        (*mo).angle = (*mo)
            .angle
            .wrapping_add(((cmd.angleturn) as u32).wrapping_shl(16));

        // Do not let the player control movement if not onground.
        let onground = ((*mo).z <= (*mo).floorz) as i32;
        USER.borrow_mut().onground = onground;

        if cmd.forwardmove != 0 && onground != 0 {
            P_Thrust(
                world,
                player_id,
                (*mo).angle,
                cmd.forwardmove as fixed_t * 2048,
            );
        }

        if cmd.sidemove != 0 && onground != 0 {
            P_Thrust(
                world,
                player_id,
                (*mo).angle.wrapping_sub(ANG90),
                cmd.sidemove as fixed_t * 2048,
            );
        }

        // C: player->mo->state == &states[S_PLAY]
        if (cmd.forwardmove != 0 || cmd.sidemove != 0) && (*mo).state.number() == S_PLAY {
            P_SetMobjState(world, (*player).mo(), S_PLAY_RUN1);
        }
    }
}

/// Per-tic camera and respawn logic for a dead player.
///
/// Drops `viewheight` to floor level, calls `P_CalcHeight`, and slowly
/// rotates the camera toward the attacker (if one exists). Pressing the Use
/// key transitions the player to `PST_REBORN`.
///
/// Corresponds to `P_DeathThink` in `p_user.c`.
///
/// # Safety
/// `player_id`'s `mo` must name a map object of the loaded level.
pub unsafe fn P_DeathThink(world: &mut World, player_id: PlayerId) {
    unsafe {
        let player = player_id.ptr();
        let mo = (*player).mo().ptr();
        let angle: u32;
        let delta: u32;

        P_MovePsprites(world, player_id);

        // Fall to the ground
        if (*player).viewheight > 6 * FRACUNIT {
            (*player).viewheight -= FRACUNIT;
        }

        if (*player).viewheight < 6 * FRACUNIT {
            (*player).viewheight = 6 * FRACUNIT;
        }

        (*player).deltaviewheight = 0;
        USER.borrow_mut().onground = ((*mo).z <= (*mo).floorz) as i32;
        P_CalcHeight(world, player_id);

        // The attacker may have been removed since (an exploded barrel); like
        // C, its last position is read through the stale handle.
        if let Some(attacker) = (*player).attacker
            && Some(attacker) != (*player).mo
        {
            let atk = attacker.ptr();
            angle = R_PointToAngle2((*mo).x, (*mo).y, (*atk).x, (*atk).y);

            delta = angle.wrapping_sub((*mo).angle);

            if delta < ANG5 || delta > (!ANG5).wrapping_add(1) {
                // Looking at killer, so fade damage flash down.
                (*mo).angle = angle;

                if (*player).damagecount != 0 {
                    (*player).damagecount -= 1;
                }
            } else if delta < ANG180 {
                (*mo).angle = (*mo).angle.wrapping_add(ANG5);
            } else {
                (*mo).angle = (*mo).angle.wrapping_sub(ANG5);
            }
        } else if (*player).damagecount != 0 {
            (*player).damagecount -= 1;
        }

        if (*player).cmd.buttons & BT_USE != 0 {
            (*player).playerstate = PST_REBORN;
        }
    }
}

/// Main per-tic player thinker.
///
/// Called once per game tic for each player. Handles (in order):
/// - `CF_NOCLIP` cheat flag propagation to the mobj,
/// - chainsaw auto-run override (`MF_JUSTATTACKED`),
/// - death dispatch to `P_DeathThink`,
/// - movement via `P_MovePlayer` (suppressed during `reactiontime` after a
///   teleport),
/// - view height via `P_CalcHeight`,
/// - special sector effects via `P_PlayerInSpecialSector`,
/// - weapon switching (with shareware plasma/BFG guard and super-shotgun
///   upgrade logic),
/// - Use-key processing via `P_UseLines`,
/// - weapon sprite animation via `P_MovePsprites`,
/// - power-up timer countdown, and
/// - colormap selection for invulnerability and infrared.
///
/// Corresponds to `P_PlayerThink` in `p_user.c`.
///
/// # Safety
/// `player_id`'s `mo` must name a map object of the loaded level with a
/// valid `subsector` chain.
pub unsafe fn P_PlayerThink(world: &mut World, player_id: PlayerId) {
    unsafe {
        let player = player_id.ptr();
        let mo = (*player).mo().ptr();
        let mut newweapon: i32;

        // Fixme: do this in the cheat code
        if (*player).cheats & CF_NOCLIP != 0 {
            (*mo).flags |= MF_NOCLIP;
        } else {
            (*mo).flags &= !MF_NOCLIP;
        }

        // Chain saw run forward
        if (*mo).flags & MF_JUSTATTACKED != 0 {
            let cmd = &mut (*player).cmd;
            cmd.angleturn = 0;
            cmd.forwardmove = (0xc800 / 512) as i8;
            cmd.sidemove = 0;
            (*mo).flags &= !MF_JUSTATTACKED;
        }

        if (*player).playerstate == PST_DEAD {
            P_DeathThink(world, player_id);
            return;
        }

        // Move around.
        // Reactiontime is used to prevent movement for a bit after a teleport.
        if (*mo).reactiontime != 0 {
            (*mo).reactiontime -= 1;
        } else {
            P_MovePlayer(world, player_id);
        }

        P_CalcHeight(world, player_id);
        if LEVEL.borrow().subsector_sector((*mo).subsector).special != 0 {
            P_PlayerInSpecialSector(world, player_id);
        }
        // Check for weapon change.
        // A special event has no other buttons.
        let cmd = &mut (*player).cmd;
        if cmd.buttons & BT_SPECIAL != 0 {
            cmd.buttons = 0;
        }

        if cmd.buttons & BT_CHANGE != 0 {
            // The actual changing of the weapon is done
            // when the weapon psprite can do it
            // (read: not in the middle of an attack).
            newweapon = (cmd.buttons as i32 & BT_WEAPONMASK as i32) >> BT_WEAPONSHIFT;

            if newweapon == wp_fist
                && (*player).weaponowned[wp_chainsaw as usize] != 0
                && !((*player).readyweapon == wp_chainsaw && (*player).powers[pw_strength] != 0)
            {
                newweapon = wp_chainsaw;
            }

            if gamemode() == commercial
                && newweapon == wp_shotgun
                && (*player).weaponowned[wp_supershotgun as usize] != 0
                && (*player).readyweapon != wp_supershotgun
            {
                newweapon = wp_supershotgun;
            }

            if (*player).weaponowned[newweapon as usize] != 0 && newweapon != (*player).readyweapon
            {
                // Do not go to plasma or BFG in shareware, even if cheated.
                if (newweapon != wp_plasma && newweapon != wp_bfg) || gamemode() != shareware {
                    (*player).pendingweapon = newweapon;
                }
            }
        }

        // Check for use
        if cmd.buttons & BT_USE != 0 {
            if (*player).usedown == 0 {
                P_UseLines(world, player_id);
                (*player).usedown = 1;
            }
        } else {
            (*player).usedown = 0;
        }

        // Cycle psprites
        P_MovePsprites(world, player_id);

        // Counters, time dependent power ups.
        // Strength counts up to diminish fade.
        if (*player).powers[pw_strength] != 0 {
            (*player).powers[pw_strength] += 1;
        }

        if (*player).powers[pw_invulnerability] != 0 {
            (*player).powers[pw_invulnerability] -= 1;
        }

        if (*player).powers[pw_invisibility] != 0 {
            (*player).powers[pw_invisibility] -= 1;
            if (*player).powers[pw_invisibility] == 0 {
                (*mo).flags &= !MF_SHADOW;
            }
        }

        if (*player).powers[pw_infrared] != 0 {
            (*player).powers[pw_infrared] -= 1;
        }

        if (*player).powers[pw_ironfeet] != 0 {
            (*player).powers[pw_ironfeet] -= 1;
        }

        if (*player).damagecount != 0 {
            (*player).damagecount -= 1;
        }

        if (*player).bonuscount != 0 {
            (*player).bonuscount -= 1;
        }

        // Handling colormaps.
        if (*player).powers[pw_invulnerability] != 0 {
            if (*player).powers[pw_invulnerability] > 4 * 32
                || ((*player).powers[pw_invulnerability] & 8) != 0
            {
                (*player).fixedcolormap = INVERSECOLORMAP;
            } else {
                (*player).fixedcolormap = 0;
            }
        } else if (*player).powers[pw_infrared] != 0 {
            if (*player).powers[pw_infrared] > 4 * 32 || ((*player).powers[pw_infrared] & 8) != 0 {
                // Almost full bright
                (*player).fixedcolormap = 1;
            } else {
                (*player).fixedcolormap = 0;
            }
        } else {
            (*player).fixedcolormap = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::engine::{
            doom::{
                mode::{commercial, registered, retail, shareware},
                player::NUMPOWERS,
            },
            math::tables::ANG270,
        },
    };

    #[test]
    fn onground_defaults_to_zero() {
        assert_eq!(UserState::new().onground, 0);
    }

    #[test]
    fn constants_match_c_header_values() {
        // Verify button constants match d_event.h
        assert_eq!(BT_SPECIAL, 128);
        assert_eq!(BT_CHANGE, 4);
        assert_eq!(BT_WEAPONMASK, 56); // 8+16+32
        assert_eq!(BT_WEAPONSHIFT, 3);
        assert_eq!(BT_USE, 2);
        assert_eq!(BT_ATTACK, 1);

        // Verify game mode enum values match d_mode.rs
        assert_eq!(shareware, 0);
        assert_eq!(registered, 1);
        assert_eq!(commercial, 2);
        assert_eq!(retail, 3);

        // Verify angle constants match tables.h
        assert_eq!(ANG90, 0x40000000);
        assert_eq!(ANG270, 0xc0000000);

        // Verify player state enum values match d_player.h
        assert_eq!(PST_LIVE, 0);
        assert_eq!(PST_DEAD, 1);
        assert_eq!(PST_REBORN, 2);

        // Verify mobj flags match p_mobj.h
        assert_eq!(MF_NOCLIP, 0x1000);
        assert_eq!(MF_JUSTATTACKED, 128);
        assert_eq!(MF_SHADOW, 0x40000);

        // Verify power indices match doomdef.h (pw_allmap sits between
        // pw_ironfeet and pw_infrared; NUMPOWERS is 6).
        assert_eq!(pw_invulnerability, 0);
        assert_eq!(pw_strength, 1);
        assert_eq!(pw_invisibility, 2);
        assert_eq!(pw_ironfeet, 3);
        assert_eq!(pw_allmap, 4);
        assert_eq!(pw_infrared, 5);
        assert_eq!(NUMPOWERS, 6);
    }
}
