//! Player/item interactions ported from `vendor/doomgeneric/p_inter.c`.
//!
//! Handles ammo, weapon, armor, key, and power-up pickup logic
//! (`P_GiveAmmo`, `P_GiveWeapon`, …), item-pickup dispatch
//! (`P_TouchSpecialThing`), kill accounting (`P_KillMobj`), and damage
//! application with armor absorption and knockback (`P_DamageMobj`).

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::doom::sounds::Sfx;
use std::ffi::c_char;
use std::os::raw::c_int;

use crate::doom::d_items::weaponinfo;
use crate::doom::d_player::{consoleplayer, players, PlayerT, CF_GODMODE};
use crate::doom::doomstat::{gamemode, gameversion};
use crate::doom::info::{self, *};
use crate::doom::m_fixed::{FixedMul, FRACUNIT};
use crate::doom::m_random::P_Random;
use crate::doom::p_pspr::P_DropWeapon;
use crate::doom::p_telept::mobj_t;
use crate::doom::tables::{finecosine, finesine, ANG180, ANGLETOFINESHIFT};
use crate::i_error;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Bonus-count increment added to `player.bonuscount` on most pickups,
/// causing a brief gold screen-flash. Matches `BONUSADD` in `p_inter.c`.
const BONUSADD: c_int = 6;

/// Number of distinct ammo types (clip, shell, cell, missile).
const NUMAMMO: usize = 4;

/// Maximum health for normal health items; over-100 bonuses use
/// `DEH_DEFAULT_MAX_HEALTH` instead.
const MAXHEALTH: c_int = 100;

/// Special Z value meaning "place mobj at floor level of its sector".
/// Stored as `i32::MIN` to match the C `ONFLOORZ` sentinel.
const ONFLOORZ: c_int = i32::MIN;

/// Initial `target.threshold` assigned when a monster acquires a new target.
const BASETHRESHOLD: c_int = 100;

// Skill levels

/// Skill 0 — "I'm Too Young to Die" / baby mode. Damage is halved.
const sk_baby: c_int = 0;

/// Skill 4 — Nightmare. Ammo doublers apply and monsters are fast.
const sk_nightmare: c_int = 4;

// Power-up durations (TICRATE = 35 tics/second)

/// Duration of the invulnerability sphere power-up: 30 seconds.
const INVULNTICS: c_int = 30 * 35;

/// Duration of the partial-invisibility power-up: 60 seconds.
const INVISTICS: c_int = 60 * 35;

/// Duration of the light-amplification visor power-up: 120 seconds.
const INFRATICS: c_int = 120 * 35;

/// Duration of the radiation-shielding suit power-up: 60 seconds.
const IRONTICS: c_int = 60 * 35;

// Weapon type indices (match `weapontype_t` in `info.h`)

/// Fist — the starting melee weapon.
const wp_fist: c_int = 0;

/// Pistol — the starting ranged weapon.
const wp_pistol: c_int = 1;

/// Single-barrelled shotgun.
const wp_shotgun: c_int = 2;

/// Chaingun.
const wp_chaingun: c_int = 3;

/// Rocket launcher.
const wp_missile: c_int = 4;

/// Plasma gun.
const wp_plasma: c_int = 5;

/// BFG 9000.
const wp_bfg: c_int = 6;

/// Chainsaw.
const wp_chainsaw: c_int = 7;

/// Super shotgun (Doom II only).
const wp_supershotgun: c_int = 8;

// Ammo type indices (match `ammotype_t`)

/// Sentinel value meaning "this weapon uses no ammo".
const am_noammo: c_int = 5;

/// Bullet clip ammo (pistol / chaingun).
const am_clip: c_int = 0;

/// Shell ammo (shotgun / super shotgun).
const am_shell: c_int = 1;

/// Energy cell ammo (plasma gun / BFG).
const am_cell: c_int = 2;

/// Rocket ammo.
const am_misl: c_int = 3;

// Card/key type indices (match `card_t`)

/// Blue keycard index.
const it_bluecard: c_int = 0;

/// Yellow keycard index.
const it_yellowcard: c_int = 1;

/// Red keycard index.
const it_redcard: c_int = 2;

/// Blue skull key index.
const it_blueskull: c_int = 3;

/// Yellow skull key index.
const it_yellowskull: c_int = 4;

/// Red skull key index.
const it_redskull: c_int = 5;

// Power type indices (match `powertype_t`)

/// Invulnerability sphere power-up slot.
const pw_invulnerability: usize = 0;

/// Berserk pack power-up slot (also boosts fist damage).
const pw_strength: usize = 1;

/// Partial-invisibility power-up slot.
const pw_invisibility: usize = 2;

/// Radiation-shielding suit power-up slot.
const pw_ironfeet: usize = 3;

/// Computer area map power-up slot (reveals the automap).
const pw_allmap: usize = 4;

/// Light-amplification visor power-up slot.
const pw_infrared: usize = 5;

// Game version / mode constants

/// Chex Quest game-version code. Monsters drop no items in Chex Quest.
const exe_chex: c_int = 9;

/// Commercial game-mode code (Doom II / TNT / Plutonia). Required for
/// MegaSphere pickup.
const commercial: c_int = 2;

// DEH defaults — used when FEATURE_DEHACKED is not compiled in.

/// Maximum health achievable via bonus health spheres (not normal medikits).
const DEH_DEFAULT_MAX_HEALTH: c_int = 200;

/// Maximum armor points achievable via armor bonuses.
const DEH_DEFAULT_MAX_ARMOR: c_int = 200;

/// Armor class granted by the green security armor shirt.
const DEH_DEFAULT_GREEN_ARMOR_CLASS: c_int = 1;

/// Armor class granted by the blue mega-armor.
const DEH_DEFAULT_BLUE_ARMOR_CLASS: c_int = 2;

/// Upper health limit imposed by the soulsphere.
const DEH_DEFAULT_MAX_SOULSPHERE: c_int = 200;

/// Health points added by the soulsphere.
const DEH_DEFAULT_SOULSPHERE_HEALTH: c_int = 100;

/// Health set to when the megasphere is picked up.
const DEH_DEFAULT_MEGASPHERE_HEALTH: c_int = 200;

// ---------------------------------------------------------------------------
// Runtime DEHacked-tunable globals.
//
// These mirror the `deh_*` variables in upstream Chocolate Doom's
// `p_inter.c`.  They are initialized to the constants above and may be
// overridden at runtime by the DEHacked loader.  Pickup code must read
// the global, not the `DEH_DEFAULT_*` constant, so DEH overrides take
// effect.
// ---------------------------------------------------------------------------

/// Runtime maximum health achievable via bonus health spheres.
#[no_mangle]
pub static mut deh_max_health: c_int = DEH_DEFAULT_MAX_HEALTH;

/// Runtime maximum armor achievable via armor bonuses.
#[no_mangle]
pub static mut deh_max_armor: c_int = DEH_DEFAULT_MAX_ARMOR;

/// Runtime armor class granted by the green security armor shirt.
#[no_mangle]
pub static mut deh_green_armor_class: c_int = DEH_DEFAULT_GREEN_ARMOR_CLASS;

/// Runtime armor class granted by the blue mega-armor.
#[no_mangle]
pub static mut deh_blue_armor_class: c_int = DEH_DEFAULT_BLUE_ARMOR_CLASS;

/// Runtime upper health limit imposed by the soulsphere.
#[no_mangle]
pub static mut deh_max_soulsphere: c_int = DEH_DEFAULT_MAX_SOULSPHERE;

/// Runtime health points added by the soulsphere.
#[no_mangle]
pub static mut deh_soulsphere_health: c_int = DEH_DEFAULT_SOULSPHERE_HEALTH;

/// Runtime health set when the megasphere is picked up.
#[no_mangle]
pub static mut deh_megasphere_health: c_int = DEH_DEFAULT_MEGASPHERE_HEALTH;

// ---------------------------------------------------------------------------
// Pick-up message strings
// ---------------------------------------------------------------------------

/// "Picked up the armor." — displayed when the green armor is collected.
const GOTARMOR: *mut c_char = c"Picked up the armor.".as_ptr().cast_mut();
/// "Picked up the MegaArmor!" — displayed when the blue mega-armor is collected.
const GOTMEGA: *mut c_char = c"Picked up the MegaArmor!".as_ptr().cast_mut();
/// "Picked up a health bonus." — displayed for the health-bonus helmet.
const GOTHTHBONUS: *mut c_char = c"Picked up a health bonus.".as_ptr().cast_mut();
/// "Picked up an armor bonus." — displayed for the armor-bonus helmet.
const GOTARMBONUS: *mut c_char = c"Picked up an armor bonus.".as_ptr().cast_mut();
/// "Picked up a stimpack." — displayed for the stimpack.
const GOTSTIM: *mut c_char = c"Picked up a stimpack.".as_ptr().cast_mut();
/// Urgent medikit message when health is critically low (below 25).
const GOTMEDINEED: *mut c_char = c"Picked up a medikit that you REALLY need!"
    .as_ptr()
    .cast_mut();
/// "Picked up a medikit." — normal medikit pickup message.
const GOTMEDIKIT: *mut c_char = c"Picked up a medikit.".as_ptr().cast_mut();
/// "Supercharge!" — displayed when the soulsphere is collected.
const GOTSUPER: *mut c_char = c"Supercharge!".as_ptr().cast_mut();
/// "MegaSphere!" — displayed when the megasphere is collected (Doom II only).
const GOTMSPHERE: *mut c_char = c"MegaSphere!".as_ptr().cast_mut();
/// "Picked up a blue keycard." — displayed when the blue keycard is collected.
const GOTBLUECARD: *mut c_char = c"Picked up a blue keycard.".as_ptr().cast_mut();
/// "Picked up a yellow keycard." — displayed when the yellow keycard is collected.
const GOTYELWCARD: *mut c_char = c"Picked up a yellow keycard.".as_ptr().cast_mut();
/// "Picked up a red keycard." — displayed when the red keycard is collected.
const GOTREDCARD: *mut c_char = c"Picked up a red keycard.".as_ptr().cast_mut();
/// "Picked up a blue skull key." — displayed when the blue skull key is collected.
const GOTBLUESKUL: *mut c_char = c"Picked up a blue skull key.".as_ptr().cast_mut();
/// "Picked up a yellow skull key." — displayed when the yellow skull key is collected.
const GOTYELWSKUL: *mut c_char = c"Picked up a yellow skull key.".as_ptr().cast_mut();
/// "Picked up a red skull key." — displayed when the red skull key is collected.
const GOTREDSKULL: *mut c_char = c"Picked up a red skull key.".as_ptr().cast_mut();
/// "Invulnerability!" — displayed when the invulnerability sphere is collected.
const GOTINVUL: *mut c_char = c"Invulnerability!".as_ptr().cast_mut();
/// "Berserk!" — displayed when the berserk pack is collected.
const GOTBERSERK: *mut c_char = c"Berserk!".as_ptr().cast_mut();
/// "Partial Invisibility" — displayed when the blur-sphere is collected.
const GOTINVIS: *mut c_char = c"Partial Invisibility".as_ptr().cast_mut();
/// "Radiation Shielding Suit" — displayed when the rad suit is collected.
const GOTSUIT: *mut c_char = c"Radiation Shielding Suit".as_ptr().cast_mut();
/// "Computer Area Map" — displayed when the automap power-up is collected.
const GOTMAP: *mut c_char = c"Computer Area Map".as_ptr().cast_mut();
/// "Light Amplification Visor" — displayed when the visor is collected.
const GOTVISOR: *mut c_char = c"Light Amplification Visor".as_ptr().cast_mut();
/// "Picked up a clip." — bullet clip pickup message.
const GOTCLIP: *mut c_char = c"Picked up a clip.".as_ptr().cast_mut();
/// "Picked up a box of bullets." — ammo box pickup message.
const GOTCLIPBOX: *mut c_char = c"Picked up a box of bullets.".as_ptr().cast_mut();
/// "Picked up a rocket." — single rocket pickup message.
const GOTROCKET: *mut c_char = c"Picked up a rocket.".as_ptr().cast_mut();
/// "Picked up a box of rockets." — rocket box pickup message.
const GOTROCKBOX: *mut c_char = c"Picked up a box of rockets.".as_ptr().cast_mut();
/// "Picked up an energy cell." — single energy cell pickup message.
const GOTCELL: *mut c_char = c"Picked up an energy cell.".as_ptr().cast_mut();
/// "Picked up an energy cell pack." — energy cell pack pickup message.
const GOTCELLBOX: *mut c_char = c"Picked up an energy cell pack.".as_ptr().cast_mut();
/// "Picked up 4 shotgun shells." — shotgun shell pickup message.
const GOTSHELLS: *mut c_char = c"Picked up 4 shotgun shells.".as_ptr().cast_mut();
/// "Picked up a box of shotgun shells." — shell box pickup message.
const GOTSHELLBOX: *mut c_char = c"Picked up a box of shotgun shells.".as_ptr().cast_mut();
/// "Picked up a backpack full of ammo!" — backpack pickup message.
const GOTBACKPACK: *mut c_char = c"Picked up a backpack full of ammo!".as_ptr().cast_mut();
/// "You got the BFG9000!  Oh, yes." — BFG pickup message.
const GOTBFG9000: *mut c_char = c"You got the BFG9000!  Oh, yes.".as_ptr().cast_mut();
/// "You got the chaingun!" — chaingun pickup message.
const GOTCHAINGUN: *mut c_char = c"You got the chaingun!".as_ptr().cast_mut();
/// "A chainsaw!  Find some meat!" — chainsaw pickup message.
const GOTCHAINSAW: *mut c_char = c"A chainsaw!  Find some meat!".as_ptr().cast_mut();
/// "You got the rocket launcher!" — rocket launcher pickup message.
const GOTLAUNCHER: *mut c_char = c"You got the rocket launcher!".as_ptr().cast_mut();
/// "You got the plasma gun!" — plasma gun pickup message.
const GOTPLASMA: *mut c_char = c"You got the plasma gun!".as_ptr().cast_mut();
/// "You got the shotgun!" — shotgun pickup message.
const GOTSHOTGUN: *mut c_char = c"You got the shotgun!".as_ptr().cast_mut();
/// "You got the super shotgun!" — super shotgun pickup message (Doom II only).
const GOTSHOTGUN2: *mut c_char = c"You got the super shotgun!".as_ptr().cast_mut();

// ---------------------------------------------------------------------------
// DEH_String shim — identity when dehacked is disabled.
// ---------------------------------------------------------------------------

/// Passes `s` through unchanged.
///
/// When DEHacked support is compiled in this function would look up a
/// patched string replacement. Here it is a no-op identity shim because
/// `FEATURE_DEHACKED` is not defined.
#[inline(always)]
unsafe fn DEH_String(s: *mut c_char) -> *mut c_char {
    s
}

// ---------------------------------------------------------------------------
// C globals still provided by unported C modules
// ---------------------------------------------------------------------------

use crate::doom::am_map::{automapactive, AM_Stop};
use crate::doom::g_game::{deathmatch, gameskill, netgame};
use crate::doom::i_system::I_Tactile;
use crate::doom::p_mobj::{P_RemoveMobj, P_SetMobjState, P_SpawnMobj};
use crate::doom::r_main::R_PointToAngle2;
use crate::doom::s_sound::S_StartSound;

// ---------------------------------------------------------------------------
// Ammo tables
// ---------------------------------------------------------------------------

/// Maximum ammo capacity for each ammo type when the player has no backpack.
/// Indexed by `am_clip`, `am_shell`, `am_cell`, `am_misl` (0-3).
/// Matches `maxammo[]` in `p_inter.c`.
#[no_mangle]
pub static mut maxammo: [c_int; NUMAMMO] = [200, 50, 300, 50];

/// Base ammo count per pickup for each ammo type.
/// A weapon pickup grants 2× this amount; a dropped weapon grants 1×;
/// passing `num=0` to `P_GiveAmmo` grants half a clip.
/// Matches `clipammo[]` in `p_inter.c`.
#[no_mangle]
pub static mut clipammo: [c_int; NUMAMMO] = [10, 4, 20, 1];

// ---------------------------------------------------------------------------
// P_GiveAmmo
// ---------------------------------------------------------------------------

/// Attempt to give the player `num` clip-loads of ammo type `ammo`.
///
/// `num` is a multiplier applied to `clipammo[ammo]`.  A value of `0`
/// gives half a clip (used when picking up a dropped weapon).  On skill
/// levels `sk_baby` and `sk_nightmare` the final count is doubled.
///
/// Returns `1` if any ammo was actually added; `0` if the player was
/// already at maximum or the ammo type is `am_noammo`.  As a side-effect,
/// if the player had zero ammo of this type before the pickup, a more
/// appropriate weapon may be queued as `pendingweapon`.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to a live `PlayerT`.
/// Global mutable statics `maxammo`, `clipammo`, `gameskill` must only be
/// accessed from the game-logic thread.
#[no_mangle]
pub unsafe extern "C" fn P_GiveAmmo(player: *mut PlayerT, ammo: c_int, num: c_int) -> c_int {
    p_give_ammo(&mut *player, ammo, num)
}

/// Rust-side body of [`P_GiveAmmo`]. Takes `&mut PlayerT` so callers can
/// pass an existing borrow without re-deriving a second `&mut` from the
/// same raw pointer (which would violate aliasing rules).
unsafe fn p_give_ammo(player: &mut PlayerT, ammo: c_int, mut num: c_int) -> c_int {
    if ammo == am_noammo {
        return 0;
    }
    if ammo > NUMAMMO as c_int {
        i_error!("P_GiveAmmo: bad type");
    }
    let idx = ammo as usize;
    if player.ammo[idx] == player.maxammo[idx] {
        return 0;
    }
    if num != 0 {
        num *= clipammo[idx];
    } else {
        num = clipammo[idx] / 2;
    }
    if gameskill == sk_baby || gameskill == sk_nightmare {
        num <<= 1;
    }
    let oldammo = player.ammo[idx];
    player.ammo[idx] += num;
    if player.ammo[idx] > player.maxammo[idx] {
        player.ammo[idx] = player.maxammo[idx];
    }
    if oldammo != 0 {
        return 1;
    }
    match ammo {
        am_clip if player.readyweapon == wp_fist => {
            if player.weaponowned[wp_chaingun as usize] != 0 {
                player.pendingweapon = wp_chaingun;
            } else {
                player.pendingweapon = wp_pistol;
            }
        }
        am_shell
            if (player.readyweapon == wp_fist || player.readyweapon == wp_pistol)
                && player.weaponowned[wp_shotgun as usize] != 0 =>
        {
            player.pendingweapon = wp_shotgun;
        }
        am_cell
            if (player.readyweapon == wp_fist || player.readyweapon == wp_pistol)
                && player.weaponowned[wp_plasma as usize] != 0 =>
        {
            player.pendingweapon = wp_plasma;
        }
        am_misl
            if player.readyweapon == wp_fist && player.weaponowned[wp_missile as usize] != 0 =>
        {
            player.pendingweapon = wp_missile;
        }
        _ => {}
    }
    1
}

// ---------------------------------------------------------------------------
// P_GiveWeapon
// ---------------------------------------------------------------------------

/// Attempt to give the player weapon `weapon`.
///
/// `dropped` is non-zero when the weapon was dropped by a dying monster
/// (half the normal ammo is given).  In a net-game without deathmatch-2,
/// weapons stay in the level and only ammo is given.
///
/// Returns `1` if either the weapon or its ammo was successfully added;
/// `0` otherwise.  The weapon is queued as `pendingweapon` when granted.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to a live `PlayerT`.
/// Global mutable statics `netgame`, `deathmatch`, `consoleplayer`,
/// `players`, and `weaponinfo` must only be accessed from the game-logic
/// thread.
#[no_mangle]
pub unsafe extern "C" fn P_GiveWeapon(
    player: *mut PlayerT,
    weapon: c_int,
    dropped: c_int,
) -> c_int {
    p_give_weapon(&mut *player, weapon, dropped)
}

/// Rust-side body of [`P_GiveWeapon`]. Takes `&mut PlayerT` so callers can
/// pass an existing borrow; avoids re-deriving a second `&mut` from the
/// same raw pointer.
unsafe fn p_give_weapon(player: &mut PlayerT, weapon: c_int, dropped: c_int) -> c_int {
    let widx = weapon as usize;
    let ammo_kind = weaponinfo[widx].ammo;
    if netgame != 0 && deathmatch != 2 && dropped == 0 {
        if player.weaponowned[widx] != 0 {
            return 0;
        }
        player.bonuscount += BONUSADD;
        player.weaponowned[widx] = 1;
        if deathmatch != 0 {
            p_give_ammo(player, ammo_kind, 5);
        } else {
            p_give_ammo(player, ammo_kind, 2);
        }
        player.pendingweapon = weapon;
        let player_ptr = player as *mut PlayerT;
        if std::ptr::eq(
            player_ptr,
            std::ptr::addr_of_mut!(players[0]).add(consoleplayer as usize),
        ) {
            S_StartSound(std::ptr::null_mut(), Sfx::Wpnup as c_int);
        }
        return 0;
    }
    let gaveammo: c_int = if ammo_kind != am_noammo {
        if dropped != 0 {
            p_give_ammo(player, ammo_kind, 1)
        } else {
            p_give_ammo(player, ammo_kind, 2)
        }
    } else {
        0
    };
    let gaveweapon: c_int;
    if player.weaponowned[widx] != 0 {
        gaveweapon = 0;
    } else {
        gaveweapon = 1;
        player.weaponowned[widx] = 1;
        player.pendingweapon = weapon;
    }
    (gaveweapon != 0 || gaveammo != 0) as c_int
}

// ---------------------------------------------------------------------------
// P_GiveBody
// ---------------------------------------------------------------------------

/// Attempt to add `num` health points to the player, capped at `MAXHEALTH`
/// (100).
///
/// Does nothing and returns `0` if the player is already at or above the
/// cap.  Also syncs `player.mo.health` to match.  Returns `1` on success.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to a live `PlayerT`, and
/// `player.mo` must be a valid, non-null pointer to the player's map object.
#[no_mangle]
pub unsafe extern "C" fn P_GiveBody(player: *mut PlayerT, num: c_int) -> c_int {
    p_give_body(&mut *player, num)
}

/// Rust-side body of [`P_GiveBody`]. See [`p_give_ammo`] for rationale.
unsafe fn p_give_body(player: &mut PlayerT, num: c_int) -> c_int {
    if player.health >= MAXHEALTH {
        return 0;
    }
    player.health += num;
    if player.health > MAXHEALTH {
        player.health = MAXHEALTH;
    }
    let mo = &mut *(player.mo as *mut mobj_t);
    mo.health = player.health;
    1
}

// ---------------------------------------------------------------------------
// P_GiveArmor
// ---------------------------------------------------------------------------

/// Attempt to give the player armor of `armortype` (1 = green, 2 = blue).
///
/// The effective armor-point value is `armortype * 100`.  Returns `0` if the
/// player already has at least that many armor points (i.e. the pick-up
/// would not help).  Otherwise sets `armortype` and `armorpoints` and
/// returns `1`.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to a live `PlayerT`.
#[no_mangle]
pub unsafe extern "C" fn P_GiveArmor(player: *mut PlayerT, armortype: c_int) -> c_int {
    p_give_armor(&mut *player, armortype)
}

/// Rust-side body of [`P_GiveArmor`]. See [`p_give_ammo`] for rationale.
fn p_give_armor(player: &mut PlayerT, armortype: c_int) -> c_int {
    let hits = armortype * 100;
    if player.armorpoints >= hits {
        return 0;
    }
    player.armortype = armortype;
    player.armorpoints = hits;
    1
}

// ---------------------------------------------------------------------------
// P_GiveCard
// ---------------------------------------------------------------------------

/// Give the player key `card` if they do not already have it.
///
/// Also adds `BONUSADD` to `bonuscount` to flash the HUD gold.  If the
/// player already owns the card the function returns immediately without
/// side-effects.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to a live `PlayerT`.
#[no_mangle]
pub unsafe extern "C" fn P_GiveCard(player: *mut PlayerT, card: c_int) {
    p_give_card(&mut *player, card)
}

/// Rust-side body of [`P_GiveCard`]. See [`p_give_ammo`] for rationale.
fn p_give_card(player: &mut PlayerT, card: c_int) {
    let idx = card as usize;
    if player.cards[idx] != 0 {
        return;
    }
    player.bonuscount = BONUSADD;
    player.cards[idx] = 1;
}

// ---------------------------------------------------------------------------
// P_GivePower
// ---------------------------------------------------------------------------

/// Attempt to activate power-up `power` for the player.
///
/// Sets the appropriate `powers[]` timer for timed power-ups.  The
/// invisibility power additionally sets `MF_SHADOW` on the player's mobj.
/// The strength (berserk) power calls `P_GiveBody` to restore health to
/// 100.  Power-ups that are already active return `0`.
///
/// Returns `1` if the power was granted, `0` if it was already active.
///
/// # Safety
///
/// `player` must be a valid, non-null pointer to a live `PlayerT`, and for
/// `pw_invisibility`, `player.mo` must also be valid.
#[no_mangle]
pub unsafe extern "C" fn P_GivePower(player: *mut PlayerT, power: c_int) -> c_int {
    p_give_power(&mut *player, power)
}

/// Rust-side body of [`P_GivePower`]. See [`p_give_ammo`] for rationale.
unsafe fn p_give_power(player: &mut PlayerT, power: c_int) -> c_int {
    let power = power as usize;
    if power == pw_invulnerability {
        player.powers[power] = INVULNTICS;
        return 1;
    }
    if power == pw_invisibility {
        player.powers[power] = INVISTICS;
        let mo = &mut *(player.mo as *mut mobj_t);
        mo.flags |= MF_SHADOW;
        return 1;
    }
    if power == pw_infrared {
        player.powers[power] = INFRATICS;
        return 1;
    }
    if power == pw_ironfeet {
        player.powers[power] = IRONTICS;
        return 1;
    }
    if power == pw_strength {
        p_give_body(player, 100);
        player.powers[power] = 1;
        return 1;
    }
    if player.powers[power] != 0 {
        return 0;
    }
    player.powers[power] = 1;
    1
}

// ---------------------------------------------------------------------------
// P_TouchSpecialThing
// ---------------------------------------------------------------------------

/// Handle a player touching a special (pickup) thing.
///
/// Called by the collision-detection code when `toucher` overlaps `special`
/// and `special` has the special-thing flag set.  Dispatches on the sprite
/// number of `special` to call the appropriate `P_Give*` helper, sets the
/// HUD pickup message, plays a sound, removes the special mobj, and
/// increments `itemcount` for items with `MF_COUNTITEM`.
///
/// The function returns early (no pickup) if the vertical gap between
/// `special` and `toucher` is greater than the toucher's height or less
/// than -8 map units, preventing pickups from platforms above or pits below.
///
/// # Safety
///
/// Both `special` and `toucher` must be valid, non-null pointers to live
/// map objects.  `toucher.player` must be a valid, non-null pointer to the
/// owning `PlayerT`.  Global game-state statics (`players`, `consoleplayer`,
/// `netgame`, `gamemode`, `gameskill`) must only be accessed from the
/// game-logic thread.
#[no_mangle]
pub unsafe extern "C" fn P_TouchSpecialThing(special: *mut mobj_t, toucher: *mut mobj_t) {
    // Read all primitives from `special` / `toucher` up front via raw
    // pointers so we never hold a `&mut` to either of them across the
    // FFI helper calls or `P_RemoveMobj` below; those reborrow the
    // raw pointers and would alias.
    let delta = (*special).z - (*toucher).z;
    if delta > (*toucher).height || delta < -8 * FRACUNIT {
        return;
    }

    // Dead thing touching.
    // Can happen with a sliding player corpse.
    if (*toucher).health <= 0 {
        return;
    }

    let mut sound: c_int = Sfx::Itemup as c_int;
    let player_ptr = (*toucher).player as *mut PlayerT;
    let sprite = (*special).sprite;
    let special_flags = (*special).flags;
    // Snapshot the DEH-tunable `static mut` values into locals so each
    // unsafe read happens here, not inline inside the match arms.
    let green_armor_class = deh_green_armor_class;
    let blue_armor_class = deh_blue_armor_class;
    let max_health = deh_max_health;
    let max_armor = deh_max_armor;
    let max_soulsphere = deh_max_soulsphere;
    let soulsphere_health = deh_soulsphere_health;
    let megasphere_health = deh_megasphere_health;
    // `player` is the long-lived borrow. Every helper called below
    // takes `&mut PlayerT` (the `p_give_*` inner fns), so no second
    // `&mut PlayerT` is ever derived from `player_ptr`.
    let player = &mut *player_ptr;

    // Identify by sprite.
    match sprite {
        // armor
        SPR_ARM1 => {
            if p_give_armor(player, green_armor_class) == 0 {
                return;
            }
            player.message = DEH_String(GOTARMOR);
        }
        SPR_ARM2 => {
            if p_give_armor(player, blue_armor_class) == 0 {
                return;
            }
            player.message = DEH_String(GOTMEGA);
        }
        // bonus items
        SPR_BON1 => {
            player.health += 1;
            if player.health > max_health {
                player.health = max_health;
            }
            let mo = &mut *(player.mo as *mut mobj_t);
            mo.health = player.health;
            player.message = DEH_String(GOTHTHBONUS);
        }
        SPR_BON2 => {
            player.armorpoints += 1;
            if player.armorpoints > max_armor {
                player.armorpoints = max_armor;
            }
            // `deh_green_armor_class` only applies to the green armor
            // shirt; for the armor helmets, armortype 1 is always used.
            if player.armortype == 0 {
                player.armortype = 1;
            }
            player.message = DEH_String(GOTARMBONUS);
        }
        SPR_SOUL => {
            player.health += soulsphere_health;
            if player.health > max_soulsphere {
                player.health = max_soulsphere;
            }
            let mo = &mut *(player.mo as *mut mobj_t);
            mo.health = player.health;
            player.message = DEH_String(GOTSUPER);
            sound = Sfx::Getpow as c_int;
        }
        SPR_MEGA => {
            if gamemode != commercial {
                return;
            }
            player.health = megasphere_health;
            {
                let mo = &mut *(player.mo as *mut mobj_t);
                mo.health = player.health;
            }
            // We always give armor type 2 for the megasphere; DEHacked
            // only affects the standalone MegaArmor pickup
            // (`SPR_ARM2`), not this one.
            p_give_armor(player, 2);
            player.message = DEH_String(GOTMSPHERE);
            sound = Sfx::Getpow as c_int;
        }
        // cards
        SPR_BKEY => {
            if player.cards[it_bluecard as usize] == 0 {
                player.message = DEH_String(GOTBLUECARD);
            }
            p_give_card(player, it_bluecard);
            if netgame != 0 {
                return;
            }
        }
        SPR_YKEY => {
            if player.cards[it_yellowcard as usize] == 0 {
                player.message = DEH_String(GOTYELWCARD);
            }
            p_give_card(player, it_yellowcard);
            if netgame != 0 {
                return;
            }
        }
        SPR_RKEY => {
            if player.cards[it_redcard as usize] == 0 {
                player.message = DEH_String(GOTREDCARD);
            }
            p_give_card(player, it_redcard);
            if netgame != 0 {
                return;
            }
        }
        SPR_BSKU => {
            if player.cards[it_blueskull as usize] == 0 {
                player.message = DEH_String(GOTBLUESKUL);
            }
            p_give_card(player, it_blueskull);
            if netgame != 0 {
                return;
            }
        }
        SPR_YSKU => {
            if player.cards[it_yellowskull as usize] == 0 {
                player.message = DEH_String(GOTYELWSKUL);
            }
            p_give_card(player, it_yellowskull);
            if netgame != 0 {
                return;
            }
        }
        SPR_RSKU => {
            if player.cards[it_redskull as usize] == 0 {
                player.message = DEH_String(GOTREDSKULL);
            }
            p_give_card(player, it_redskull);
            if netgame != 0 {
                return;
            }
        }
        // medikits, heals
        SPR_STIM => {
            if p_give_body(player, 10) == 0 {
                return;
            }
            player.message = DEH_String(GOTSTIM);
        }
        SPR_MEDI => {
            if p_give_body(player, 25) == 0 {
                return;
            }
            if player.health < 25 {
                player.message = DEH_String(GOTMEDINEED);
            } else {
                player.message = DEH_String(GOTMEDIKIT);
            }
        }
        // power ups
        SPR_PINV => {
            if p_give_power(player, pw_invulnerability as c_int) == 0 {
                return;
            }
            player.message = DEH_String(GOTINVUL);
            sound = Sfx::Getpow as c_int;
        }
        SPR_PSTR => {
            if p_give_power(player, pw_strength as c_int) == 0 {
                return;
            }
            player.message = DEH_String(GOTBERSERK);
            if player.readyweapon != wp_fist {
                player.pendingweapon = wp_fist;
            }
            sound = Sfx::Getpow as c_int;
        }
        SPR_PINS => {
            if p_give_power(player, pw_invisibility as c_int) == 0 {
                return;
            }
            player.message = DEH_String(GOTINVIS);
            sound = Sfx::Getpow as c_int;
        }
        SPR_SUIT => {
            if p_give_power(player, pw_ironfeet as c_int) == 0 {
                return;
            }
            player.message = DEH_String(GOTSUIT);
            sound = Sfx::Getpow as c_int;
        }
        SPR_PMAP => {
            if p_give_power(player, pw_allmap as c_int) == 0 {
                return;
            }
            player.message = DEH_String(GOTMAP);
            sound = Sfx::Getpow as c_int;
        }
        SPR_PVIS => {
            if p_give_power(player, pw_infrared as c_int) == 0 {
                return;
            }
            player.message = DEH_String(GOTVISOR);
            sound = Sfx::Getpow as c_int;
        }
        // ammo
        SPR_CLIP => {
            let amount = if special_flags & MF_DROPPED != 0 {
                0
            } else {
                1
            };
            if p_give_ammo(player, am_clip, amount) == 0 {
                return;
            }
            player.message = DEH_String(GOTCLIP);
        }
        SPR_AMMO => {
            if p_give_ammo(player, am_clip, 5) == 0 {
                return;
            }
            player.message = DEH_String(GOTCLIPBOX);
        }
        SPR_ROCK => {
            if p_give_ammo(player, am_misl, 1) == 0 {
                return;
            }
            player.message = DEH_String(GOTROCKET);
        }
        SPR_BROK => {
            if p_give_ammo(player, am_misl, 5) == 0 {
                return;
            }
            player.message = DEH_String(GOTROCKBOX);
        }
        SPR_CELL => {
            if p_give_ammo(player, am_cell, 1) == 0 {
                return;
            }
            player.message = DEH_String(GOTCELL);
        }
        SPR_CELP => {
            if p_give_ammo(player, am_cell, 5) == 0 {
                return;
            }
            player.message = DEH_String(GOTCELLBOX);
        }
        SPR_SHEL => {
            if p_give_ammo(player, am_shell, 1) == 0 {
                return;
            }
            player.message = DEH_String(GOTSHELLS);
        }
        SPR_SBOX => {
            if p_give_ammo(player, am_shell, 5) == 0 {
                return;
            }
            player.message = DEH_String(GOTSHELLBOX);
        }
        SPR_BPAK => {
            if player.backpack == 0 {
                for i in 0..NUMAMMO {
                    player.maxammo[i] *= 2;
                }
                player.backpack = 1;
            }
            for i in 0..NUMAMMO {
                p_give_ammo(player, i as c_int, 1);
            }
            player.message = DEH_String(GOTBACKPACK);
        }
        // weapons
        SPR_BFUG => {
            if p_give_weapon(player, wp_bfg, 0) == 0 {
                return;
            }
            player.message = DEH_String(GOTBFG9000);
            sound = Sfx::Wpnup as c_int;
        }
        SPR_MGUN => {
            if p_give_weapon(
                player,
                wp_chaingun,
                ((special_flags & MF_DROPPED) != 0) as c_int,
            ) == 0
            {
                return;
            }
            player.message = DEH_String(GOTCHAINGUN);
            sound = Sfx::Wpnup as c_int;
        }
        SPR_CSAW => {
            if p_give_weapon(player, wp_chainsaw, 0) == 0 {
                return;
            }
            player.message = DEH_String(GOTCHAINSAW);
            sound = Sfx::Wpnup as c_int;
        }
        SPR_LAUN => {
            if p_give_weapon(player, wp_missile, 0) == 0 {
                return;
            }
            player.message = DEH_String(GOTLAUNCHER);
            sound = Sfx::Wpnup as c_int;
        }
        SPR_PLAS => {
            if p_give_weapon(player, wp_plasma, 0) == 0 {
                return;
            }
            player.message = DEH_String(GOTPLASMA);
            sound = Sfx::Wpnup as c_int;
        }
        SPR_SHOT => {
            if p_give_weapon(
                player,
                wp_shotgun,
                ((special_flags & MF_DROPPED) != 0) as c_int,
            ) == 0
            {
                return;
            }
            player.message = DEH_String(GOTSHOTGUN);
            sound = Sfx::Wpnup as c_int;
        }
        SPR_SGN2 => {
            if p_give_weapon(
                player,
                wp_supershotgun,
                ((special_flags & MF_DROPPED) != 0) as c_int,
            ) == 0
            {
                return;
            }
            player.message = DEH_String(GOTSHOTGUN2);
            sound = Sfx::Wpnup as c_int;
        }
        _ => {
            i_error!("P_SpecialThing: Unknown gettable thing");
        }
    }

    if special_flags & MF_COUNTITEM != 0 {
        player.itemcount += 1;
    }
    P_RemoveMobj(special);
    player.bonuscount += BONUSADD;
    if std::ptr::eq(
        player_ptr,
        std::ptr::addr_of_mut!(players[0]).add(consoleplayer as usize),
    ) {
        S_StartSound(std::ptr::null_mut(), sound);
    }
}

// ---------------------------------------------------------------------------
// P_KillMobj
// ---------------------------------------------------------------------------

/// Kill map object `target`, optionally crediting `source` with the kill.
///
/// Clears movement flags (`MF_SHOOTABLE`, `MF_FLOAT`, `MF_SKULLFLY`),
/// sets `MF_CORPSE | MF_DROPOFF`, halves the height, transitions to the
/// appropriate death state (normal or extra-gory `xdeathstate`), and
/// randomises the initial death-animation tic offset by up to 3 tics.
///
/// Kill counters: if `source` is a player, `killcount` and `frags` are
/// updated.  If `source` is null in a single-player game, `players[0]`
/// still gets the kill credit (e.g. barrel chain-kills).
///
/// Weapon drops: `MT_WOLFSS` / `MT_POSSESSED` drop `MT_CLIP`;
/// `MT_SHOTGUY` drops `MT_SHOTGUN`; `MT_CHAINGUY` drops `MT_CHAINGUN`.
/// No items are dropped in Chex Quest.
///
/// If `target` is a player, the player enters `PST_DEAD`, the automap is
/// stopped for the console player, and `P_DropWeapon` is called.
///
/// # Safety
///
/// `target` must be a valid, non-null pointer to a live `mobj_t`.
/// `source` may be null (environmental kill).  All global game-state
/// statics must only be accessed from the game-logic thread.
#[no_mangle]
pub unsafe extern "C" fn P_KillMobj(source: *mut mobj_t, target: *mut mobj_t) {
    let info = (*target).info as *mut MobjInfo;
    (*target).flags &= !(MF_SHOOTABLE | MF_FLOAT | MF_SKULLFLY);
    if (*target).mobjtype != MT_SKULL {
        (*target).flags &= !MF_NOGRAVITY;
    }
    (*target).flags |= MF_CORPSE | MF_DROPOFF;
    (*target).height >>= 2;

    if !source.is_null() && !(*source).player.is_null() {
        let source_player = (*source).player as *mut PlayerT;
        if (*target).flags & MF_COUNTKILL != 0 {
            (*source_player).killcount += 1;
        }
        if !(*target).player.is_null() {
            let target_player = (*target).player as *mut PlayerT;
            let idx = target_player.offset_from(std::ptr::addr_of_mut!(players[0])) as usize;
            (*source_player).frags[idx] += 1;
        }
    } else if netgame == 0 && (*target).flags & MF_COUNTKILL != 0 {
        players[0].killcount += 1;
    }

    if !(*target).player.is_null() {
        let target_player = (*target).player as *mut PlayerT;
        if source.is_null() {
            let idx = target_player.offset_from(std::ptr::addr_of_mut!(players[0])) as usize;
            (*target_player).frags[idx] += 1;
        }
        (*target).flags &= !MF_SOLID;
        (*target_player).playerstate = 1; // PST_DEAD
        P_DropWeapon(target_player);
        if std::ptr::eq(
            target_player,
            std::ptr::addr_of_mut!(players[0]).add(consoleplayer as usize),
        ) && automapactive != 0
        {
            AM_Stop();
        }
    }

    if (*target).health < -(*info).spawnhealth && (*info).xdeathstate != 0 {
        P_SetMobjState(target, (*info).xdeathstate);
    } else {
        P_SetMobjState(target, (*info).deathstate);
    }
    (*target).tics -= P_Random() & 3;
    if (*target).tics < 1 {
        (*target).tics = 1;
    }

    if gameversion == exe_chex {
        return;
    }

    let item: c_int = match (*target).mobjtype {
        MT_WOLFSS | MT_POSSESSED => MT_CLIP,
        MT_SHOTGUY => MT_SHOTGUN,
        MT_CHAINGUY => MT_CHAINGUN,
        _ => return,
    };

    let mo = P_SpawnMobj((*target).x, (*target).y, ONFLOORZ, item);
    (*mo).flags |= MF_DROPPED;
}

// ---------------------------------------------------------------------------
// P_DamageMobj
// ---------------------------------------------------------------------------

/// Apply `damage` points to map object `target`.
///
/// `inflictor` is the projectile or object that physically caused the
/// damage (used to compute knockback direction); it may be null for
/// environmental damage such as slime floors or barrel explosions.
/// `source` is the actor to blame for the damage and to set as
/// `target.target`; it may also be null.  `source` and `inflictor` are
/// the same for hitscan and melee attacks.
///
/// Behavior summary:
/// - On skill `sk_baby`, player damage is halved.
/// - Knockback thrust is applied unless the source is using the chainsaw or
///   the target has `MF_NOCLIP`.  A random forward-fall is possible when the
///   target is damaged from below and has low remaining health.
/// - In the end-of-game hell sector (special 11) damage is capped so the
///   player cannot be killed.
/// - `CF_GODMODE` and the invulnerability power-up block damage below 1000.
/// - Green armor absorbs 1/3 of damage; blue armor absorbs 1/2.  Armor is
///   consumed when points are exhausted.
/// - If health drops to zero `P_KillMobj` is called.
/// - On surviving hits a pain state may be entered and the monster's target
///   is updated to `source`.
///
/// # Safety
///
/// `target` must be a valid, non-null pointer to a live `mobj_t`.
/// `inflictor` and `source` may be null.  If `target.player` is non-null
/// it must point to a valid `PlayerT`.  All global game-state statics must
/// only be accessed from the game-logic thread.
#[no_mangle]
pub unsafe extern "C" fn P_DamageMobj(
    target: *mut mobj_t,
    inflictor: *mut mobj_t,
    source: *mut mobj_t,
    mut damage: c_int,
) {
    if ((*target).flags & MF_SHOOTABLE) == 0 {
        return;
    }
    if (*target).health <= 0 {
        return;
    }
    if ((*target).flags & MF_SKULLFLY) != 0 {
        (*target).momx = 0;
        (*target).momy = 0;
        (*target).momz = 0;
    }

    let player = (*target).player as *mut PlayerT;
    if !player.is_null() && gameskill == sk_baby {
        damage >>= 1;
    }

    if !inflictor.is_null()
        && ((*target).flags & MF_NOCLIP) == 0
        && (source.is_null()
            || (*source).player.is_null()
            || (*((*source).player as *mut PlayerT)).readyweapon != wp_chainsaw)
    {
        let mut ang = R_PointToAngle2((*inflictor).x, (*inflictor).y, (*target).x, (*target).y);
        let info = (*target).info as *mut MobjInfo;
        let mut thrust = damage * (FRACUNIT >> 3) * 100 / (*info).mass;

        if damage < 40
            && damage > (*target).health
            && (*target).z - (*inflictor).z > 64 * FRACUNIT
            && (P_Random() & 1) != 0
        {
            ang = ang.wrapping_add(ANG180);
            thrust *= 4;
        }

        ang >>= ANGLETOFINESHIFT;
        (*target).momx += FixedMul(thrust, *finecosine.0.add(ang as usize));
        (*target).momy += FixedMul(thrust, finesine[ang as usize]);
    }

    if !player.is_null() {
        // end of game hell hack
        if (*(*(*target).subsector).sector).special == 11 && damage >= (*target).health {
            damage = (*target).health - 1;
        }

        if damage < 1000
            && (((*player).cheats & CF_GODMODE) != 0 || (*player).powers[pw_invulnerability] != 0)
        {
            return;
        }

        if (*player).armortype != 0 {
            let saved = if (*player).armortype == 1 {
                damage / 3
            } else {
                damage / 2
            };
            let mut saved_actual = saved;
            if (*player).armorpoints <= saved_actual {
                saved_actual = (*player).armorpoints;
                (*player).armortype = 0;
            }
            (*player).armorpoints -= saved_actual;
            damage -= saved_actual;
        }
        (*player).health -= damage;
        if (*player).health < 0 {
            (*player).health = 0;
        }
        (*player).attacker = source as *mut crate::doom::d_player::mobj_t;
        (*player).damagecount += damage;
        if (*player).damagecount > 100 {
            (*player).damagecount = 100;
        }
        let temp = if damage < 100 { damage } else { 100 };
        if std::ptr::eq(
            player,
            std::ptr::addr_of_mut!(players[0]).add(consoleplayer as usize),
        ) {
            I_Tactile(40, 10, 40 + temp * 2);
        }
    }

    (*target).health -= damage;
    if (*target).health <= 0 {
        P_KillMobj(source, target);
        return;
    }

    let info = (*target).info as *mut MobjInfo;
    if P_Random() < (*info).painchance && ((*target).flags & MF_SKULLFLY) == 0 {
        (*target).flags |= MF_JUSTHIT;
        P_SetMobjState(target, (*info).painstate);
    }

    (*target).reactiontime = 0;

    if ((*target).threshold == 0 || (*target).mobjtype == MT_VILE)
        && !source.is_null()
        && source != target
        && (*source).mobjtype != MT_VILE
    {
        (*target).target = source;
        (*target).threshold = BASETHRESHOLD;
        let state_ptr = (*target).state as *mut State;
        let spawnstate_ptr = &info::states[(*info).spawnstate as usize] as *const State;
        if std::ptr::eq(state_ptr, spawnstate_ptr) && (*info).seestate != S_NULL {
            P_SetMobjState(target, (*info).seestate);
        }
    }
}

// ---------------------------------------------------------------------------
// Link anchor
// ---------------------------------------------------------------------------

/// Ensures all public symbols in this module are included in the final
/// binary even when the linker would otherwise dead-strip them.
///
/// Called from the crate's link-anchor collection; not intended for direct
/// use in game logic.
#[no_mangle]
pub extern "C" fn P_Inter_Link_Anchor() {
    unsafe {
        let _ = std::ptr::addr_of_mut!(maxammo[0]) as usize;
        let _ = std::ptr::addr_of_mut!(clipammo[0]) as usize;
    }
    let _ = P_GiveAmmo as *const () as usize;
    let _ = P_GiveWeapon as *const () as usize;
    let _ = P_GiveBody as *const () as usize;
    let _ = P_GiveArmor as *const () as usize;
    let _ = P_GiveCard as *const () as usize;
    let _ = P_GivePower as *const () as usize;
    let _ = P_TouchSpecialThing as *const () as usize;
    let _ = P_KillMobj as *const () as usize;
    let _ = P_DamageMobj as *const () as usize;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn maxammo_defaults() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert_eq!(maxammo, [200, 50, 300, 50]);
        }
    }

    #[test]
    fn clipammo_defaults() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert_eq!(clipammo, [10, 4, 20, 1]);
        }
    }

    #[test]
    fn constants_match() {
        assert_eq!(BONUSADD, 6);
        assert_eq!(NUMAMMO, 4);
        assert_eq!(MAXHEALTH, 100);
        assert_eq!(ONFLOORZ, i32::MIN);
        assert_eq!(FRACUNIT, 65536);
        assert_eq!(ANG180, 0x80000000);
        assert_eq!(ANGLETOFINESHIFT, 19);
        assert_eq!(BASETHRESHOLD, 100);
        assert_eq!(CF_GODMODE, 2);
        assert_eq!(sk_baby, 0);
        assert_eq!(sk_nightmare, 4);
        assert_eq!(INVULNTICS, 1050);
        assert_eq!(INVISTICS, 2100);
        assert_eq!(INFRATICS, 4200);
        assert_eq!(IRONTICS, 2100);
        assert_eq!(am_noammo, 5);
        assert_eq!(MF_DROPPED, 0x00020000);
        assert_eq!(MF_COUNTITEM, 0x00800000);
        assert_eq!(MF_SHOOTABLE, 4);
        assert_eq!(MF_SKULLFLY, 0x01000000);
        assert_eq!(exe_chex, 9);
        assert_eq!(commercial, 2);
    }

    #[test]
    fn deh_defaults_match() {
        assert_eq!(DEH_DEFAULT_MAX_HEALTH, 200);
        assert_eq!(DEH_DEFAULT_MAX_ARMOR, 200);
        assert_eq!(DEH_DEFAULT_GREEN_ARMOR_CLASS, 1);
        assert_eq!(DEH_DEFAULT_BLUE_ARMOR_CLASS, 2);
        assert_eq!(DEH_DEFAULT_MAX_SOULSPHERE, 200);
        assert_eq!(DEH_DEFAULT_SOULSPHERE_HEALTH, 100);
        assert_eq!(DEH_DEFAULT_MEGASPHERE_HEALTH, 200);
    }

    #[test]
    fn deh_runtime_globals_default_to_deh_values() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert_eq!(deh_max_health, DEH_DEFAULT_MAX_HEALTH);
            assert_eq!(deh_max_armor, DEH_DEFAULT_MAX_ARMOR);
            assert_eq!(deh_green_armor_class, DEH_DEFAULT_GREEN_ARMOR_CLASS);
            assert_eq!(deh_blue_armor_class, DEH_DEFAULT_BLUE_ARMOR_CLASS);
            assert_eq!(deh_max_soulsphere, DEH_DEFAULT_MAX_SOULSPHERE);
            assert_eq!(deh_soulsphere_health, DEH_DEFAULT_SOULSPHERE_HEALTH);
            assert_eq!(deh_megasphere_health, DEH_DEFAULT_MEGASPHERE_HEALTH);
        }
    }
}
