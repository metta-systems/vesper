//! Door animation: opening, closing, and locked vertical doors.
//!
//! Rust port of `vendor/doomgeneric/p_doors.c`. Each active door is managed by
//! a `vldoor_t` thinker that moves the sector ceiling up or down every game tic.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::doom::sounds::Sfx;
use std::ffi::{c_char, c_void};
use std::os::raw::c_int;

use crate::doom::c_ffi as cffi;
use crate::doom::c_ffi::mobj_t;
use crate::doom::d_player::PlayerT;
use crate::doom::i_timer::TICRATE;
use crate::doom::m_fixed::{fixed_t, FRACUNIT};
use crate::doom::p_floor::T_MovePlane;
use crate::doom::p_lights::{line_t, sector_t};
use crate::doom::p_plats::{plat_t, T_PlatRaise};
use crate::doom::p_setup::{sectors, sides};
use crate::doom::p_spec::{P_FindLowestCeilingSurrounding, P_FindSectorFromLineTag};
use crate::doom::p_tick::{thinker_t, P_AddThinker, P_RemoveThinker};
use crate::doom::s_sound::S_StartSound;
use crate::doom::z_zone::{Z_Malloc, PU_LEVSPEC};

/// Movement speed for normal vertical doors (in fixed-point units per tic).
const VDOORSPEED: fixed_t = FRACUNIT * 2;

/// Number of tics a normal door waits at the top before closing (`VDOORWAIT` in C).
const VDOORWAIT: c_int = 150;

// vldoor_e enum values
/// Normal raise-and-wait door: opens, waits `VDOORWAIT` tics, then closes.
const vld_normal: c_int = 0;
/// Door that closes immediately, waits 30 seconds, then opens permanently.
const vld_close30ThenOpen: c_int = 1;
/// Door that closes and stays closed (no re-open).
const vld_close: c_int = 2;
/// Door that opens and stays open (no auto-close).
const vld_open: c_int = 3;
/// Door that starts waiting and raises after 5 minutes.
const vld_raiseIn5Mins: c_int = 4;
/// Fast normal door (blaze speed: `VDOORSPEED * 4`), waits then closes.
const vld_blazeRaise: c_int = 5;
/// Fast door that opens and stays open.
const vld_blazeOpen: c_int = 6;
/// Fast door that closes and stays closed.
const vld_blazeClose: c_int = 7;

// result_e enum values
/// `T_MovePlane` return: plane moved without reaching destination or crushing.
const result_ok: c_int = 0;
/// `T_MovePlane` return: plane was blocked by a thing (crushing).
const result_crushed: c_int = 1;
/// `T_MovePlane` return: plane reached its destination this tic.
const result_pastdest: c_int = 2;

// card indices
/// Index into the player card array for the blue keycard.
const it_bluecard: usize = 0;
/// Index into the player card array for the yellow keycard.
const it_yellowcard: usize = 1;
/// Index into the player card array for the red keycard.
const it_redcard: usize = 2;
/// Index into the player card array for the blue skull key.
const it_blueskull: usize = 3;
/// Index into the player card array for the yellow skull key.
const it_yellowskull: usize = 4;
/// Index into the player card array for the red skull key.
const it_redskull: usize = 5;

/// Locked-object message: blue key required.
const PD_BLUEO: *mut c_char = c"You need a blue key to activate this object"
    .as_ptr()
    .cast_mut();
/// Locked-object message: red key required.
const PD_REDO: *mut c_char = c"You need a red key to activate this object"
    .as_ptr()
    .cast_mut();
/// Locked-object message: yellow key required.
const PD_YELLOWO: *mut c_char = c"You need a yellow key to activate this object"
    .as_ptr()
    .cast_mut();
/// Locked-door message: blue key required.
const PD_BLUEK: *mut c_char = c"You need a blue key to open this door".as_ptr().cast_mut();
/// Locked-door message: red key required.
const PD_REDK: *mut c_char = c"You need a red key to open this door".as_ptr().cast_mut();
/// Locked-door message: yellow key required.
const PD_YELLOWK: *mut c_char = c"You need a yellow key to open this door"
    .as_ptr()
    .cast_mut();

/// Passes `s` through unchanged.
///
/// When DEHacked support is compiled in this function would look up a patched
/// string replacement. Here it is a no-op identity shim because
/// `FEATURE_DEHACKED` is not defined.
#[inline(always)]
unsafe fn DEH_String(s: *mut c_char) -> *mut c_char {
    s
}

#[inline(always)]
unsafe fn locked_object_message(special: c_int) -> *mut c_char {
    match special {
        99 | 133 => DEH_String(PD_BLUEO),
        134 | 135 => DEH_String(PD_REDO),
        136 | 137 => DEH_String(PD_YELLOWO),
        _ => unreachable!("unexpected locked object special: {special}"),
    }
}

#[inline(always)]
unsafe fn locked_door_message(special: c_int) -> *mut c_char {
    match special {
        26 | 32 => DEH_String(PD_BLUEK),
        27 | 34 => DEH_String(PD_YELLOWK),
        28 | 33 => DEH_String(PD_REDK),
        _ => unreachable!("unexpected locked door special: {special}"),
    }
}

/// Thinker state for an active vertical door.
///
/// The door moves its sector's ceiling between `floorheight + 0` (closed) and
/// `topheight` (open). `direction` encodes the current movement:
/// - `1` = moving up (opening)
/// - `-1` = moving down (closing)
/// - `0` = waiting at the top
/// - `2` = initial wait (used by `vld_raiseIn5Mins`)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct vldoor_t {
    /// Embedded thinker header; must be the first field.
    pub thinker: thinker_t,
    /// Door type (`vld_*` constant); controls behaviour at open/close limits.
    pub r#type: c_int,
    _pad0: [u8; 4],
    /// The sector this door controls (its ceiling is the moving plane).
    pub sector: *mut sector_t,
    /// Target height of the open position (lowest surrounding ceiling - 4 units).
    pub topheight: fixed_t,
    /// Movement speed in fixed-point units per tic.
    pub speed: fixed_t,
    /// Current movement direction: `1` up, `-1` down, `0` waiting, `2` initial wait.
    pub direction: c_int,
    /// Tics to wait at the top before starting to close (set to `VDOORWAIT`).
    pub topwait: c_int,
    /// Countdown tics remaining in the current wait phase.
    pub topcountdown: c_int,
}

/// Compile-time layout checks for `vldoor_t` against the C struct on 64-bit.
#[cfg(target_pointer_width = "64")]
mod layout_checks {
    use super::*;
    const _: () = assert!(std::mem::size_of::<vldoor_t>() == 64);
    const _: () = assert!(std::mem::offset_of!(vldoor_t, thinker) == 0);
    const _: () = assert!(std::mem::offset_of!(vldoor_t, r#type) == 24);
    const _: () = assert!(std::mem::offset_of!(vldoor_t, sector) == 32);
    const _: () = assert!(std::mem::offset_of!(vldoor_t, topheight) == 40);
    const _: () = assert!(std::mem::offset_of!(vldoor_t, speed) == 44);
    const _: () = assert!(std::mem::offset_of!(vldoor_t, direction) == 48);
    const _: () = assert!(std::mem::offset_of!(vldoor_t, topwait) == 52);
    const _: () = assert!(std::mem::offset_of!(vldoor_t, topcountdown) == 56);
}

/// Per-tic update for a vertical door.
///
/// Dispatches on `door->direction`:
/// - `0` (waiting): counts down `topcountdown`, then starts closing or re-opening.
/// - `2` (initial wait): counts down `topcountdown`, then transitions to open.
/// - `-1` (moving down): calls `T_MovePlane`; handles crush, fully-closed, and
///   the `vld_close30ThenOpen` re-open delay.
/// - `1` (moving up): calls `T_MovePlane`; parks at top or removes thinker when done.
///
/// # Safety
///
/// `door` must be a valid, aligned, non-null pointer to a `vldoor_t` whose
/// `sector` pointer is also valid for the current map. Called exclusively by
/// the thinker dispatcher from `P_RunThinkers`.
#[no_mangle]
pub unsafe extern "C" fn T_VerticalDoor(door: *mut vldoor_t) {
    let res: c_int;

    match (*door).direction {
        0 => {
            // WAITING
            (*door).topcountdown -= 1;
            if (*door).topcountdown == 0 {
                match (*door).r#type {
                    x if x == vld_blazeRaise => {
                        (*door).direction = -1;
                        S_StartSound(
                            &(*(*door).sector).soundorg as *const [u8; 40] as *mut c_void,
                            Sfx::Bdcls as c_int,
                        );
                    }
                    x if x == vld_normal => {
                        (*door).direction = -1;
                        S_StartSound(
                            &(*(*door).sector).soundorg as *const [u8; 40] as *mut c_void,
                            Sfx::Dorcls as c_int,
                        );
                    }
                    x if x == vld_close30ThenOpen => {
                        (*door).direction = 1;
                        S_StartSound(
                            &(*(*door).sector).soundorg as *const [u8; 40] as *mut c_void,
                            Sfx::Doropn as c_int,
                        );
                    }
                    _ => {}
                }
            }
        }
        2 => {
            // INITIAL WAIT
            (*door).topcountdown -= 1;
            if (*door).topcountdown == 0 {
                match (*door).r#type {
                    x if x == vld_raiseIn5Mins => {
                        (*door).direction = 1;
                        (*door).r#type = vld_normal;
                        S_StartSound(
                            &(*(*door).sector).soundorg as *const [u8; 40] as *mut c_void,
                            Sfx::Doropn as c_int,
                        );
                    }
                    _ => {}
                }
            }
        }
        -1 => {
            // DOWN
            res = T_MovePlane(
                (*door).sector,
                (*door).speed,
                (*(*door).sector).floorheight,
                0,
                1,
                -1,
            );
            if res == result_pastdest {
                match (*door).r#type {
                    x if x == vld_blazeRaise || x == vld_blazeClose => {
                        (*(*door).sector).specialdata = std::ptr::null_mut();
                        P_RemoveThinker(&mut (*door).thinker);
                        S_StartSound(
                            &(*(*door).sector).soundorg as *const [u8; 40] as *mut c_void,
                            Sfx::Bdcls as c_int,
                        );
                    }
                    x if x == vld_normal || x == vld_close => {
                        (*(*door).sector).specialdata = std::ptr::null_mut();
                        P_RemoveThinker(&mut (*door).thinker);
                    }
                    x if x == vld_close30ThenOpen => {
                        (*door).direction = 0;
                        (*door).topcountdown = TICRATE * 30;
                    }
                    _ => {}
                }
            } else if res == result_crushed {
                match (*door).r#type {
                    x if x == vld_blazeClose || x == vld_close => {
                        // DO NOT GO BACK UP!
                    }
                    _ => {
                        (*door).direction = 1;
                        S_StartSound(
                            &(*(*door).sector).soundorg as *const [u8; 40] as *mut c_void,
                            Sfx::Doropn as c_int,
                        );
                    }
                }
            }
        }
        1 => {
            // UP
            res = T_MovePlane((*door).sector, (*door).speed, (*door).topheight, 0, 1, 1);
            if res == result_pastdest {
                match (*door).r#type {
                    x if x == vld_blazeRaise || x == vld_normal => {
                        (*door).direction = 0;
                        (*door).topcountdown = (*door).topwait;
                    }
                    x if x == vld_close30ThenOpen || x == vld_blazeOpen || x == vld_open => {
                        (*(*door).sector).specialdata = std::ptr::null_mut();
                        P_RemoveThinker(&mut (*door).thinker);
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// Linedef-triggered event: activate a key-locked door for `thing`.
///
/// Checks whether the player associated with `thing` holds any of the required
/// keys for `line`'s special number. Displays a message and plays `sfx_oof` if
/// the required key is absent, then returns `0`. If the key check passes,
/// delegates to `EV_DoDoor`.
///
/// Returns `1` if the door was activated, `0` otherwise.
///
/// # Safety
///
/// `line`, `thing` must be valid non-null pointers for the current map.
///
/// Lock-failure messages are routed through `DEH_String`, matching the C
/// original so Dehacked patches can override them.
#[no_mangle]
pub unsafe extern "C" fn EV_DoLockedDoor(
    line: *mut line_t,
    r#type: c_int,
    thing: *mut mobj_t,
) -> c_int {
    let Some(p) = ((*thing).player as *mut PlayerT).as_mut() else {
        return 0;
    };

    match (*line).special as c_int {
        99 | 133 => {
            if p.cards[it_bluecard] == 0 && p.cards[it_blueskull] == 0 {
                p.message = locked_object_message((*line).special as c_int);
                S_StartSound(std::ptr::null_mut(), Sfx::Oof as c_int);
                return 0;
            }
        }
        134 | 135 => {
            if p.cards[it_redcard] == 0 && p.cards[it_redskull] == 0 {
                p.message = locked_object_message((*line).special as c_int);
                S_StartSound(std::ptr::null_mut(), Sfx::Oof as c_int);
                return 0;
            }
        }
        136 | 137 => {
            if p.cards[it_yellowcard] == 0 && p.cards[it_yellowskull] == 0 {
                p.message = locked_object_message((*line).special as c_int);
                S_StartSound(std::ptr::null_mut(), Sfx::Oof as c_int);
                return 0;
            }
        }
        _ => {}
    }

    EV_DoDoor(line, r#type)
}

/// Linedef-triggered event: activate doors on all sectors tagged to `line`.
///
/// For each tagged sector that does not already have an active special, allocates
/// a `vldoor_t` thinker and configures it according to `type`. Blaze variants
/// move at `VDOORSPEED * 4`.
///
/// Returns `1` if at least one door was activated, `0` otherwise.
///
/// # Safety
///
/// `line` must be a valid non-null pointer for the current map; the global
/// `sectors` array must be initialised.
#[no_mangle]
pub unsafe extern "C" fn EV_DoDoor(line: *mut line_t, r#type: c_int) -> c_int {
    let mut secnum: c_int = -1;
    let mut rtn: c_int = 0;

    while {
        secnum = P_FindSectorFromLineTag(line as *mut cffi::line_t, secnum);
        secnum
    } >= 0
    {
        let sec = sectors.add(secnum as usize);
        if !(*sec).specialdata.is_null() {
            continue;
        }

        rtn = 1;
        let door = Z_Malloc(
            std::mem::size_of::<vldoor_t>() as c_int,
            PU_LEVSPEC,
            std::ptr::null_mut(),
        ) as *mut vldoor_t;
        P_AddThinker(&mut (*door).thinker);
        (*sec).specialdata = door as *mut c_void;

        (*door).thinker.function.acp1 = Some(core::mem::transmute::<
            unsafe extern "C" fn(*mut vldoor_t),
            unsafe extern "C" fn(*mut c_void),
        >(T_VerticalDoor));
        (*door).sector = sec as *mut sector_t;
        (*door).r#type = r#type;
        (*door).topwait = VDOORWAIT;
        (*door).speed = VDOORSPEED;

        match r#type {
            x if x == vld_blazeClose => {
                (*door).topheight = P_FindLowestCeilingSurrounding(sec);
                (*door).topheight -= 4 * FRACUNIT;
                (*door).direction = -1;
                (*door).speed = VDOORSPEED * 4;
                S_StartSound(
                    &(*(*door).sector).soundorg as *const [u8; 40] as *mut c_void,
                    Sfx::Bdcls as c_int,
                );
            }
            x if x == vld_close => {
                (*door).topheight = P_FindLowestCeilingSurrounding(sec);
                (*door).topheight -= 4 * FRACUNIT;
                (*door).direction = -1;
                S_StartSound(
                    &(*(*door).sector).soundorg as *const [u8; 40] as *mut c_void,
                    Sfx::Dorcls as c_int,
                );
            }
            x if x == vld_close30ThenOpen => {
                (*door).topheight = (*sec).ceilingheight;
                (*door).direction = -1;
                S_StartSound(
                    &(*(*door).sector).soundorg as *const [u8; 40] as *mut c_void,
                    Sfx::Dorcls as c_int,
                );
            }
            x if x == vld_blazeRaise || x == vld_blazeOpen => {
                (*door).direction = 1;
                (*door).topheight = P_FindLowestCeilingSurrounding(sec);
                (*door).topheight -= 4 * FRACUNIT;
                (*door).speed = VDOORSPEED * 4;
                if (*door).topheight != (*sec).ceilingheight {
                    S_StartSound(
                        &(*(*door).sector).soundorg as *const [u8; 40] as *mut c_void,
                        Sfx::Bdopn as c_int,
                    );
                }
            }
            x if x == vld_normal || x == vld_open => {
                (*door).direction = 1;
                (*door).topheight = P_FindLowestCeilingSurrounding(sec);
                (*door).topheight -= 4 * FRACUNIT;
                if (*door).topheight != (*sec).ceilingheight {
                    S_StartSound(
                        &(*(*door).sector).soundorg as *const [u8; 40] as *mut c_void,
                        Sfx::Doropn as c_int,
                    );
                }
            }
            _ => {}
        }
    }
    rtn
}

/// Linedef use-action: manually open or toggle the door behind `line`.
///
/// Only the front side of a linedef can be used (`side = 0`). Checks key locks
/// first (specials 26-28, 32-34). If the sector behind the line already has an
/// active thinker, toggles its direction (open doors begin closing, and vice
/// versa). Otherwise spawns a new `vldoor_t` thinker.
///
/// # Safety
///
/// `line` and `thing` must be valid non-null pointers for the current map; the
/// global `sides` array must be initialised.
///
/// Key-locked door messages are routed through `DEH_String`, matching the C
/// original so Dehacked patches can override them.
#[no_mangle]
pub unsafe extern "C" fn EV_VerticalDoor(line: *mut line_t, thing: *mut mobj_t) {
    let side = 0;

    // Check for locks
    let player = (*thing).player as *mut PlayerT;

    match (*line).special as c_int {
        26 | 32 => {
            let Some(player) = player.as_mut() else {
                return;
            };
            if player.cards[it_bluecard] == 0 && player.cards[it_blueskull] == 0 {
                player.message = locked_door_message((*line).special as c_int);
                S_StartSound(std::ptr::null_mut(), Sfx::Oof as c_int);
                return;
            }
        }
        27 | 34 => {
            let Some(player) = player.as_mut() else {
                return;
            };
            if player.cards[it_yellowcard] == 0 && player.cards[it_yellowskull] == 0 {
                player.message = locked_door_message((*line).special as c_int);
                S_StartSound(std::ptr::null_mut(), Sfx::Oof as c_int);
                return;
            }
        }
        28 | 33 => {
            let Some(player) = player.as_mut() else {
                return;
            };
            if player.cards[it_redcard] == 0 && player.cards[it_redskull] == 0 {
                player.message = locked_door_message((*line).special as c_int);
                S_StartSound(std::ptr::null_mut(), Sfx::Oof as c_int);
                return;
            }
        }
        _ => {}
    }

    let sec = (*sides.offset((*line).sidenum[(side ^ 1) as usize] as isize)).sector;

    if !(*sec).specialdata.is_null() {
        let door = (*sec).specialdata as *mut vldoor_t;
        match (*line).special as c_int {
            1 | 26 | 27 | 28 | 117 => {
                if (*door).direction == -1 {
                    (*door).direction = 1;
                } else {
                    if (*thing).player.is_null() {
                        return;
                    }
                    let t_vdoor = Some(core::mem::transmute::<
                        unsafe extern "C" fn(*mut vldoor_t),
                        unsafe extern "C" fn(*mut c_void),
                    >(T_VerticalDoor));
                    let t_plat = Some(core::mem::transmute::<
                        unsafe extern "C" fn(*mut plat_t),
                        unsafe extern "C" fn(*mut c_void),
                    >(T_PlatRaise));
                    if (*door).thinker.function.acp1 == t_vdoor {
                        (*door).direction = -1;
                    } else if (*door).thinker.function.acp1 == t_plat {
                        let plat = door as *mut plat_t;
                        (*plat).wait = -1;
                    } else {
                        eprintln!("EV_VerticalDoor: Tried to close something that wasn't a door.");
                        (*door).direction = -1;
                    }
                }
                return;
            }
            _ => {}
        }
    }

    // for proper sound
    match (*line).special as c_int {
        117 | 118 => {
            S_StartSound(
                &(*sec).soundorg as *const [u8; 40] as *mut c_void,
                Sfx::Bdopn as c_int,
            );
        }
        1 | 31 => {
            S_StartSound(
                &(*sec).soundorg as *const [u8; 40] as *mut c_void,
                Sfx::Doropn as c_int,
            );
        }
        _ => {
            S_StartSound(
                &(*sec).soundorg as *const [u8; 40] as *mut c_void,
                Sfx::Doropn as c_int,
            );
        }
    }

    // new door thinker
    let door = Z_Malloc(
        std::mem::size_of::<vldoor_t>() as c_int,
        PU_LEVSPEC,
        std::ptr::null_mut(),
    ) as *mut vldoor_t;
    P_AddThinker(&mut (*door).thinker);
    (*sec).specialdata = door as *mut c_void;
    (*door).thinker.function.acp1 = Some(core::mem::transmute::<
        unsafe extern "C" fn(*mut vldoor_t),
        unsafe extern "C" fn(*mut c_void),
    >(T_VerticalDoor));
    (*door).sector = sec as *mut sector_t;
    (*door).direction = 1;
    (*door).speed = VDOORSPEED;
    (*door).topwait = VDOORWAIT;

    match (*line).special as c_int {
        1 | 26 | 27 | 28 => {
            (*door).r#type = vld_normal;
        }
        31..=34 => {
            (*door).r#type = vld_open;
            (*line).special = 0;
        }
        117 => {
            (*door).r#type = vld_blazeRaise;
            (*door).speed = VDOORSPEED * 4;
        }
        118 => {
            (*door).r#type = vld_blazeOpen;
            (*line).special = 0;
            (*door).speed = VDOORSPEED * 4;
        }
        _ => {}
    }

    // find the top and bottom of the movement range
    (*door).topheight = P_FindLowestCeilingSurrounding(sec);
    (*door).topheight -= 4 * FRACUNIT;
}

/// Spawn a door thinker that closes `sec` after 30 seconds.
///
/// The door starts in direction `0` (waiting) with `topcountdown = 30 * TICRATE`.
/// After the countdown it begins closing as a `vld_normal` door.
///
/// # Safety
///
/// `sec` must be a valid non-null pointer to a sector for the current map.
#[no_mangle]
pub unsafe extern "C" fn P_SpawnDoorCloseIn30(sec: *mut sector_t) {
    let door = Z_Malloc(
        std::mem::size_of::<vldoor_t>() as c_int,
        PU_LEVSPEC,
        std::ptr::null_mut(),
    ) as *mut vldoor_t;

    P_AddThinker(&mut (*door).thinker);

    (*sec).specialdata = door as *mut c_void;
    (*sec).special = 0;

    (*door).thinker.function.acp1 = Some(core::mem::transmute::<
        unsafe extern "C" fn(*mut vldoor_t),
        unsafe extern "C" fn(*mut c_void),
    >(T_VerticalDoor));
    (*door).sector = sec;
    (*door).direction = 0;
    (*door).r#type = vld_normal;
    (*door).speed = VDOORSPEED;
    (*door).topcountdown = 30 * TICRATE;
}

/// Spawn a door thinker that opens `sec` after 5 minutes (`vld_raiseIn5Mins`).
///
/// The door starts in direction `2` (initial wait) with
/// `topcountdown = 5 * 60 * TICRATE`. `_secnum` is accepted for C ABI
/// compatibility but is not used.
///
/// # Safety
///
/// `sec` must be a valid non-null pointer to a sector for the current map; the
/// global `sectors` array must be initialised.
#[no_mangle]
pub unsafe extern "C" fn P_SpawnDoorRaiseIn5Mins(sec: *mut sector_t, _secnum: c_int) {
    let door = Z_Malloc(
        std::mem::size_of::<vldoor_t>() as c_int,
        PU_LEVSPEC,
        std::ptr::null_mut(),
    ) as *mut vldoor_t;

    P_AddThinker(&mut (*door).thinker);

    (*sec).specialdata = door as *mut c_void;
    (*sec).special = 0;

    (*door).thinker.function.acp1 = Some(core::mem::transmute::<
        unsafe extern "C" fn(*mut vldoor_t),
        unsafe extern "C" fn(*mut c_void),
    >(T_VerticalDoor));
    (*door).sector = sec;
    (*door).direction = 2;
    (*door).r#type = vld_raiseIn5Mins;
    (*door).speed = VDOORSPEED;
    (*door).topheight = P_FindLowestCeilingSurrounding(sec as *mut cffi::sector_t);
    (*door).topheight -= 4 * FRACUNIT;
    (*door).topwait = VDOORWAIT;
    (*door).topcountdown = 5 * 60 * TICRATE;
}

/// Anchor function referenced from `doomgeneric_Create` to ensure all
/// `#[no_mangle]` door functions survive link-time dead-code elimination.
///
/// # Safety
///
/// Must only be called during engine initialisation before any thinker
/// dispatch occurs; taking the address of each function is always safe.
#[no_mangle]
pub unsafe extern "C" fn P_Doors_Link_Anchor() {
    let _ = T_VerticalDoor as *const () as usize;
    let _ = EV_DoLockedDoor as *const () as usize;
    let _ = EV_DoDoor as *const () as usize;
    let _ = EV_VerticalDoor as *const () as usize;
    let _ = P_SpawnDoorCloseIn30 as *const () as usize;
    let _ = P_SpawnDoorRaiseIn5Mins as *const () as usize;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    const VLDOOR_T_SIZEOF: usize = 64;
    const VLDOOR_T_THINKER: usize = 0;
    const VLDOOR_T_TYPE: usize = 24;
    const VLDOOR_T_SECTOR: usize = 32;
    const VLDOOR_T_TOPHEIGHT: usize = 40;
    const VLDOOR_T_SPEED: usize = 44;
    const VLDOOR_T_DIRECTION: usize = 48;
    const VLDOOR_T_TOPWAIT: usize = 52;
    const VLDOOR_T_TOPCOUNTDOWN: usize = 56;

    #[test]
    fn vldoor_t_layout_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(
            std::mem::size_of::<vldoor_t>(),
            VLDOOR_T_SIZEOF,
            "vldoor_t size mismatch: Rust={}, expected={}",
            std::mem::size_of::<vldoor_t>(),
            VLDOOR_T_SIZEOF,
        );
        assert_eq!(std::mem::offset_of!(vldoor_t, thinker), VLDOOR_T_THINKER);
        assert_eq!(std::mem::offset_of!(vldoor_t, r#type), VLDOOR_T_TYPE);
        assert_eq!(std::mem::offset_of!(vldoor_t, sector), VLDOOR_T_SECTOR);
        assert_eq!(
            std::mem::offset_of!(vldoor_t, topheight),
            VLDOOR_T_TOPHEIGHT
        );
        assert_eq!(std::mem::offset_of!(vldoor_t, speed), VLDOOR_T_SPEED);
        assert_eq!(
            std::mem::offset_of!(vldoor_t, direction),
            VLDOOR_T_DIRECTION
        );
        assert_eq!(std::mem::offset_of!(vldoor_t, topwait), VLDOOR_T_TOPWAIT);
        assert_eq!(
            std::mem::offset_of!(vldoor_t, topcountdown),
            VLDOOR_T_TOPCOUNTDOWN
        );
    }

    #[test]
    fn locked_object_specials_map_to_dehacked_messages() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert_eq!(locked_object_message(99), DEH_String(PD_BLUEO));
            assert_eq!(locked_object_message(133), DEH_String(PD_BLUEO));
            assert_eq!(locked_object_message(134), DEH_String(PD_REDO));
            assert_eq!(locked_object_message(135), DEH_String(PD_REDO));
            assert_eq!(locked_object_message(136), DEH_String(PD_YELLOWO));
            assert_eq!(locked_object_message(137), DEH_String(PD_YELLOWO));
        }
    }

    #[test]
    fn vertical_door_specials_map_to_dehacked_messages() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert_eq!(locked_door_message(26), DEH_String(PD_BLUEK));
            assert_eq!(locked_door_message(32), DEH_String(PD_BLUEK));
            assert_eq!(locked_door_message(27), DEH_String(PD_YELLOWK));
            assert_eq!(locked_door_message(34), DEH_String(PD_YELLOWK));
            assert_eq!(locked_door_message(28), DEH_String(PD_REDK));
            assert_eq!(locked_door_message(33), DEH_String(PD_REDK));
        }
    }
}
