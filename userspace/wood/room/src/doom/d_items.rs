//! Rust port of `vendor/doomgeneric/d_items.c`.
//!
//! Weapon animation state table (`weaponinfo`) and the supporting
//! `weaponinfo_t` struct.  Each entry describes the ammo type and the
//! state-machine indices for raise, lower, idle, attack, and muzzle-flash
//! animations of one weapon.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::c_int;

// ---------------------------------------------------------------------------
// Ammo type enum values (from doomdef.h)
// ---------------------------------------------------------------------------

/// Clip (bullet) ammo type.
const am_clip: c_int = 0;
/// Shell ammo type.
const am_shell: c_int = 1;
/// Cell (energy) ammo type.
const am_cell: c_int = 2;
/// Missile ammo type.
const am_misl: c_int = 3;
// NUMAMMO = 4 (not used directly)
/// Sentinel value: weapon uses no ammunition (fist, chainsaw).
const am_noammo: c_int = 5;

// ---------------------------------------------------------------------------
// State index constants (from info.h statenum_t enum)
// ---------------------------------------------------------------------------
//
// Previously these were hand-typed and were silently off-by-one from
// S_DSGUNFLASH1 onward because the S_DSNR1/S_DSNR2 super-shotgun reload
// states were missed, which caused wp_missile / wp_chainsaw / wp_plasma /
// wp_bfg to point at the wrong animation slots. Raising one of those weapons
// ran A_Lower instead of A_Raise, eventually driving readyweapon to
// wp_nochange (= NUMWEAPONS) and segfaulting on weaponinfo[9].

use crate::doom::statenum::*;

// ---------------------------------------------------------------------------
// Weapon info table
// ---------------------------------------------------------------------------

/// Weapon info: sprite frames, ammunition use.
/// Matches `weaponinfo_t` from `d_items.h`.
#[repr(C)]
pub struct weaponinfo_t {
    /// Ammo type consumed by this weapon (`am_*`).
    pub ammo: c_int,
    /// State index for the raise (weapon-up) animation.
    pub upstate: c_int,
    /// State index for the lower (weapon-down) animation.
    pub downstate: c_int,
    /// State index for the idle (weapon-ready) animation.
    pub readystate: c_int,
    /// State index for the attack/fire animation.
    pub atkstate: c_int,
    /// State index for the muzzle-flash animation (`S_NULL` if none).
    pub flashstate: c_int,
}

/// Number of weapons in the game (fist through super shotgun).
const NUMWEAPONS: usize = 9;

/// Weapon animation state table.
///
/// Order: wp_fist, wp_pistol, wp_shotgun, wp_chaingun, wp_missile,
///        wp_plasma, wp_bfg, wp_chainsaw, wp_supershotgun.
#[no_mangle]
pub static weaponinfo: [weaponinfo_t; NUMWEAPONS] = [
    // fist
    weaponinfo_t {
        ammo: am_noammo,
        upstate: S_PUNCHUP,
        downstate: S_PUNCHDOWN,
        readystate: S_PUNCH,
        atkstate: S_PUNCH1,
        flashstate: S_NULL,
    },
    // pistol
    weaponinfo_t {
        ammo: am_clip,
        upstate: S_PISTOLUP,
        downstate: S_PISTOLDOWN,
        readystate: S_PISTOL,
        atkstate: S_PISTOL1,
        flashstate: S_PISTOLFLASH,
    },
    // shotgun
    weaponinfo_t {
        ammo: am_shell,
        upstate: S_SGUNUP,
        downstate: S_SGUNDOWN,
        readystate: S_SGUN,
        atkstate: S_SGUN1,
        flashstate: S_SGUNFLASH1,
    },
    // chaingun
    weaponinfo_t {
        ammo: am_clip,
        upstate: S_CHAINUP,
        downstate: S_CHAINDOWN,
        readystate: S_CHAIN,
        atkstate: S_CHAIN1,
        flashstate: S_CHAINFLASH1,
    },
    // missile launcher
    weaponinfo_t {
        ammo: am_misl,
        upstate: S_MISSILEUP,
        downstate: S_MISSILEDOWN,
        readystate: S_MISSILE,
        atkstate: S_MISSILE1,
        flashstate: S_MISSILEFLASH1,
    },
    // plasma rifle
    weaponinfo_t {
        ammo: am_cell,
        upstate: S_PLASMAUP,
        downstate: S_PLASMADOWN,
        readystate: S_PLASMA,
        atkstate: S_PLASMA1,
        flashstate: S_PLASMAFLASH1,
    },
    // bfg 9000
    weaponinfo_t {
        ammo: am_cell,
        upstate: S_BFGUP,
        downstate: S_BFGDOWN,
        readystate: S_BFG,
        atkstate: S_BFG1,
        flashstate: S_BFGFLASH1,
    },
    // chainsaw
    weaponinfo_t {
        ammo: am_noammo,
        upstate: S_SAWUP,
        downstate: S_SAWDOWN,
        readystate: S_SAW,
        atkstate: S_SAW1,
        flashstate: S_NULL,
    },
    // super shotgun
    weaponinfo_t {
        ammo: am_shell,
        upstate: S_DSGUNUP,
        downstate: S_DSGUNDOWN,
        readystate: S_DSGUN,
        atkstate: S_DSGUN1,
        flashstate: S_DSGUNFLASH1,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the weapon table has exactly nine entries.
    #[test]
    fn test_weaponinfo_length() {
        assert_eq!(weaponinfo.len(), 9);
    }

    /// Verify the fist entry uses no ammo and has the correct ready state.
    #[test]
    fn test_fist_entry() {
        assert_eq!(weaponinfo[0].ammo, am_noammo);
        assert_eq!(weaponinfo[0].readystate, S_PUNCH);
    }

    /// Verify the super-shotgun entry uses shells and has the correct flash state.
    #[test]
    fn test_super_shotgun_entry() {
        assert_eq!(weaponinfo[8].ammo, am_shell);
        assert_eq!(weaponinfo[8].flashstate, S_DSGUNFLASH1);
    }

    /// Verify `weaponinfo_t` is 24 bytes (six `c_int` fields, 4 bytes each).
    #[test]
    fn test_weaponinfo_t_size() {
        assert_eq!(size_of::<weaponinfo_t>(), 24);
    }
}
