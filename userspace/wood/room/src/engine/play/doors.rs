//! Door animation: opening, closing, and locked vertical doors.
//!
//! Rust port of `vendor/doomgeneric/p_doors.c`. Each active door is managed by
//! a `vldoor_t` thinker that moves the sector ceiling up or down every game tic.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::engine::sound::info::Sfx;

use crate::engine::{
    dehacked::DEH_String,
    doom::player::{PlayerId, PlayerMessage},
    interface::timer::TICRATE,
    map::{LineId, SectorId, line_t},
    math::fixed::{FRACUNIT, fixed_t},
    play::{
        floor::T_MovePlane,
        mobj::MobjId,
        setup::LEVEL,
        spec::{P_FindLowestCeilingSurrounding, P_FindSectorFromLineTag},
        tick::{
            DoorId, P_AddThinker, P_RemoveThinker, ThinkerId, ThinkerKind, new_thinker, thinker_t,
        },
    },
    sound::{S_StartNonPositionalSound, S_StartSectorSound},
    world::World,
};

/// Movement speed for normal vertical doors (in fixed-point units per tic).
pub const VDOORSPEED: fixed_t = FRACUNIT * 2;

/// Number of tics a normal door waits at the top before closing (`VDOORWAIT` in C).
pub const VDOORWAIT: i32 = 150;

// vldoor_e enum values
/// Normal raise-and-wait door: opens, waits `VDOORWAIT` tics, then closes.
const vld_normal: i32 = 0;
/// Door that closes immediately, waits 30 seconds, then opens permanently.
const vld_close30ThenOpen: i32 = 1;
/// Door that closes and stays closed (no re-open).
const vld_close: i32 = 2;
/// Door that opens and stays open (no auto-close).
const vld_open: i32 = 3;
/// Door that starts waiting and raises after 5 minutes.
const vld_raiseIn5Mins: i32 = 4;
/// Fast normal door (blaze speed: `VDOORSPEED * 4`), waits then closes.
const vld_blazeRaise: i32 = 5;
/// Fast door that opens and stays open.
const vld_blazeOpen: i32 = 6;
/// Fast door that closes and stays closed.
const vld_blazeClose: i32 = 7;

// result_e enum values
/// `T_MovePlane` return: plane moved without reaching destination or crushing.
const result_ok: i32 = 0;
/// `T_MovePlane` return: plane was blocked by a thing (crushing).
const result_crushed: i32 = 1;
/// `T_MovePlane` return: plane reached its destination this tic.
const result_pastdest: i32 = 2;

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
const PD_BLUEO: &str = "You need a blue key to activate this object";
/// Locked-object message: red key required.
const PD_REDO: &str = "You need a red key to activate this object";
/// Locked-object message: yellow key required.
const PD_YELLOWO: &str = "You need a yellow key to activate this object";
/// Locked-door message: blue key required.
const PD_BLUEK: &str = "You need a blue key to open this door";
/// Locked-door message: red key required.
const PD_REDK: &str = "You need a red key to open this door";
/// Locked-door message: yellow key required.
const PD_YELLOWK: &str = "You need a yellow key to open this door";

#[inline(always)]
fn locked_object_message(special: i32) -> PlayerMessage {
    PlayerMessage::new(match special {
        99 | 133 => DEH_String(PD_BLUEO),
        134 | 135 => DEH_String(PD_REDO),
        136 | 137 => DEH_String(PD_YELLOWO),
        _ => unreachable!("unexpected locked object special: {special}"),
    })
}

#[inline(always)]
fn locked_door_message(special: i32) -> PlayerMessage {
    PlayerMessage::new(match special {
        26 | 32 => DEH_String(PD_BLUEK),
        27 | 34 => DEH_String(PD_YELLOWK),
        28 | 33 => DEH_String(PD_REDK),
        _ => unreachable!("unexpected locked door special: {special}"),
    })
}

/// The lock check of `EV_DoLockedDoor` and `EV_VerticalDoor`: `true` if
/// `player` holds key `card` or `skull`. A player holding neither gets
/// `message` and the "oof" sound.
fn has_key_or_complain(
    player: PlayerId,
    card: usize,
    skull: usize,
    message: PlayerMessage,
) -> bool {
    if player.read(|player| player.cards[card] != 0 || player.cards[skull] != 0) {
        return true;
    }
    player.write(|player| player.message = message);
    S_StartNonPositionalSound(Sfx::Oof as i32);
    false
}

/// Thinker state for an active vertical door.
///
/// The door moves its sector's ceiling between `floorheight + 0` (closed) and
/// `topheight` (open). `direction` encodes the current movement:
/// - `1` = moving up (opening)
/// - `-1` = moving down (closing)
/// - `0` = waiting at the top
/// - `2` = initial wait (used by `vld_raiseIn5Mins`)
#[derive(Clone, Copy)]
pub struct vldoor_t {
    /// Thinker state: what the door does each tic.
    pub thinker: thinker_t,
    /// Door type (`vld_*` constant); controls behaviour at open/close limits.
    pub kind: i32,
    /// The sector this door controls (its ceiling is the moving plane).
    pub sector: SectorId,
    /// Target height of the open position (lowest surrounding ceiling - 4 units).
    pub topheight: fixed_t,
    /// Movement speed in fixed-point units per tic.
    pub speed: fixed_t,
    /// Current movement direction: `1` up, `-1` down, `0` waiting, `2` initial wait.
    pub direction: i32,
    /// Tics to wait at the top before starting to close (set to `VDOORWAIT`).
    pub topwait: i32,
    /// Countdown tics remaining in the current wait phase.
    pub topcountdown: i32,
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
/// `door_id` must name a door whose `sector` belongs to the current map.
/// Called exclusively by the thinker dispatcher from `P_RunThinkers`.
pub unsafe fn T_VerticalDoor(world: &mut World, door_id: DoorId) {
    let door = door_id.ptr();
    let res: i32;
    let clear_specialdata =
        |sector: SectorId| LEVEL.borrow_mut().sectors[sector].specialdata = None;

    match (*door).direction {
        0 => {
            // WAITING
            (*door).topcountdown -= 1;
            if (*door).topcountdown == 0 {
                match (*door).kind {
                    x if x == vld_blazeRaise => {
                        (*door).direction = -1;
                        S_StartSectorSound((*door).sector, Sfx::Bdcls as i32);
                    }
                    x if x == vld_normal => {
                        (*door).direction = -1;
                        S_StartSectorSound((*door).sector, Sfx::Dorcls as i32);
                    }
                    x if x == vld_close30ThenOpen => {
                        (*door).direction = 1;
                        S_StartSectorSound((*door).sector, Sfx::Doropn as i32);
                    }
                    _ => {}
                }
            }
        }
        2 => {
            // INITIAL WAIT
            (*door).topcountdown -= 1;
            if (*door).topcountdown == 0 {
                match (*door).kind {
                    x if x == vld_raiseIn5Mins => {
                        (*door).direction = 1;
                        (*door).kind = vld_normal;
                        S_StartSectorSound((*door).sector, Sfx::Doropn as i32);
                    }
                    _ => {}
                }
            }
        }
        -1 => {
            // DOWN
            let floorheight = LEVEL.borrow().sectors[(*door).sector].floorheight;
            res = T_MovePlane(world, (*door).sector, (*door).speed, floorheight, 0, 1, -1);
            if res == result_pastdest {
                match (*door).kind {
                    x if x == vld_blazeRaise || x == vld_blazeClose => {
                        clear_specialdata((*door).sector);
                        P_RemoveThinker(&mut (*door).thinker);
                        S_StartSectorSound((*door).sector, Sfx::Bdcls as i32);
                    }
                    x if x == vld_normal || x == vld_close => {
                        clear_specialdata((*door).sector);
                        P_RemoveThinker(&mut (*door).thinker);
                    }
                    x if x == vld_close30ThenOpen => {
                        (*door).direction = 0;
                        (*door).topcountdown = TICRATE * 30;
                    }
                    _ => {}
                }
            } else if res == result_crushed {
                match (*door).kind {
                    x if x == vld_blazeClose || x == vld_close => {
                        // DO NOT GO BACK UP!
                    }
                    _ => {
                        (*door).direction = 1;
                        S_StartSectorSound((*door).sector, Sfx::Doropn as i32);
                    }
                }
            }
        }
        1 => {
            // UP
            res = T_MovePlane(
                world,
                (*door).sector,
                (*door).speed,
                (*door).topheight,
                0,
                1,
                1,
            );
            if res == result_pastdest {
                match (*door).kind {
                    x if x == vld_blazeRaise || x == vld_normal => {
                        (*door).direction = 0;
                        (*door).topcountdown = (*door).topwait;
                    }
                    x if x == vld_close30ThenOpen || x == vld_blazeOpen || x == vld_open => {
                        clear_specialdata((*door).sector);
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
/// `thing` must name a map object of the level; the thinker list must be
/// initialised and a level loaded.
///
/// Lock-failure messages are routed through `DEH_String`, matching the C
/// original so Dehacked patches can override them.
pub unsafe fn EV_DoLockedDoor(world: &mut World, line: &line_t, kind: i32, thing: MobjId) -> i32 {
    let Some(player) = (*thing.ptr()).player else {
        return 0;
    };

    let special = line.special as i32;
    let keys = match special {
        99 | 133 => Some((it_bluecard, it_blueskull)),
        134 | 135 => Some((it_redcard, it_redskull)),
        136 | 137 => Some((it_yellowcard, it_yellowskull)),
        _ => None,
    };
    if let Some((card, skull)) = keys
        && !has_key_or_complain(player, card, skull, locked_object_message(special))
    {
        return 0;
    }

    EV_DoDoor(world, line, kind)
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
/// The thinker list must be initialised and a level loaded.
pub unsafe fn EV_DoDoor(_world: &mut World, line: &line_t, kind: i32) -> i32 {
    let mut secnum: i32 = -1;
    let mut rtn: i32 = 0;

    while {
        secnum = P_FindSectorFromLineTag(&LEVEL.borrow(), line, secnum);
        secnum
    } >= 0
    {
        let sec = SectorId::new(secnum as usize);
        if LEVEL.borrow().sectors[sec].specialdata.is_some() {
            continue;
        }

        rtn = 1;
        let door_id = new_thinker::<vldoor_t>();
        let door = door_id.ptr();
        P_AddThinker(ThinkerId::Door(door_id));
        LEVEL.borrow_mut().sectors[sec].specialdata = Some(ThinkerId::Door(door_id));

        (*door).thinker.kind = ThinkerKind::Door;
        (*door).sector = sec;
        (*door).kind = kind;
        (*door).topwait = VDOORWAIT;
        (*door).speed = VDOORSPEED;

        let (lowest_ceiling, ceilingheight) = {
            let level = LEVEL.borrow();
            (
                P_FindLowestCeilingSurrounding(&level, sec),
                level.sectors[sec].ceilingheight,
            )
        };
        match kind {
            x if x == vld_blazeClose => {
                (*door).topheight = lowest_ceiling;
                (*door).topheight -= 4 * FRACUNIT;
                (*door).direction = -1;
                (*door).speed = VDOORSPEED * 4;
                S_StartSectorSound((*door).sector, Sfx::Bdcls as i32);
            }
            x if x == vld_close => {
                (*door).topheight = lowest_ceiling;
                (*door).topheight -= 4 * FRACUNIT;
                (*door).direction = -1;
                S_StartSectorSound((*door).sector, Sfx::Dorcls as i32);
            }
            x if x == vld_close30ThenOpen => {
                (*door).topheight = ceilingheight;
                (*door).direction = -1;
                S_StartSectorSound((*door).sector, Sfx::Dorcls as i32);
            }
            x if x == vld_blazeRaise || x == vld_blazeOpen => {
                (*door).direction = 1;
                (*door).topheight = lowest_ceiling;
                (*door).topheight -= 4 * FRACUNIT;
                (*door).speed = VDOORSPEED * 4;
                if (*door).topheight != ceilingheight {
                    S_StartSectorSound((*door).sector, Sfx::Bdopn as i32);
                }
            }
            x if x == vld_normal || x == vld_open => {
                (*door).direction = 1;
                (*door).topheight = lowest_ceiling;
                (*door).topheight -= 4 * FRACUNIT;
                if (*door).topheight != ceilingheight {
                    S_StartSectorSound((*door).sector, Sfx::Doropn as i32);
                }
            }
            _ => {}
        }
    }
    rtn
}

/// The `door->direction` that vanilla `EV_VerticalDoor` reads through
/// `sector->specialdata`, whatever special it actually points at ("When is a
/// door not a door?" in `p_doors.c`).
///
/// This port follows the 64-bit C layout, where the `int` at
/// `vldoor_t.direction`'s offset is `plat_t.count`, `ceiling_t.speed`, or
/// `floormove_t.texture` with the two padding bytes after it (see
/// [`floormove_t::door_direction_alias`]).
///
/// # Panics
///
/// If `special` is a light, which no sector holds as its special data.
fn special_door_direction(special: ThinkerId) -> i32 {
    // SAFETY (all arms): a read through an arena pointer; no reference to a
    // sector special is live while a line is used.
    unsafe {
        match special {
            ThinkerId::Door(door) => (*door.ptr()).direction,
            ThinkerId::Plat(plat) => (*plat.ptr()).count,
            ThinkerId::Ceiling(ceiling) => (*ceiling.ptr()).speed,
            ThinkerId::Floor(floor) => (*floor.ptr()).door_direction_alias(),
            other => unreachable!("sector special data is {other:?}"),
        }
    }
}

/// Write `direction` where [`special_door_direction`] reads it.
///
/// # Panics
///
/// Same as [`special_door_direction`].
fn set_special_door_direction(special: ThinkerId, direction: i32) {
    // SAFETY (all arms): as in `special_door_direction`.
    unsafe {
        match special {
            ThinkerId::Door(door) => (*door.ptr()).direction = direction,
            ThinkerId::Plat(plat) => (*plat.ptr()).count = direction,
            ThinkerId::Ceiling(ceiling) => (*ceiling.ptr()).speed = direction,
            ThinkerId::Floor(floor) => (*floor.ptr()).set_door_direction_alias(direction),
            other => unreachable!("sector special data is {other:?}"),
        }
    }
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
/// `thing` must name a map object whose `player` is null or valid, and
/// `line_id` a linedef of the current map; the thinker list must be
/// initialised.
///
/// Key-locked door messages are routed through `DEH_String`, matching the C
/// original so Dehacked patches can override them.
pub unsafe fn EV_VerticalDoor(_world: &mut World, line_id: LineId, thing: MobjId) {
    let side = 0;
    // The line's record as the use found it; clearing its special below
    // writes the level's line.
    let line_record = LEVEL.borrow().lines[line_id];
    let line = &line_record;

    // Check for locks
    let thing = thing.ptr();
    let special = line.special as i32;
    let keys = match special {
        26 | 32 => Some((it_bluecard, it_blueskull)),
        27 | 34 => Some((it_yellowcard, it_yellowskull)),
        28 | 33 => Some((it_redcard, it_redskull)),
        _ => None,
    };
    if let Some((card, skull)) = keys {
        let Some(player) = (*thing).player else {
            return;
        };
        if !has_key_or_complain(player, card, skull, locked_door_message(special)) {
            return;
        }
    }

    let (sec, special) = {
        let level = LEVEL.borrow();
        let sec = level.sides[line.sidenum[(side ^ 1) as usize] as usize].sector;
        (sec, level.sectors[sec].specialdata)
    };

    if let Some(special) = special {
        match line.special as i32 {
            1 | 26 | 27 | 28 | 117 => {
                if special_door_direction(special) == -1 {
                    set_special_door_direction(special, 1);
                } else {
                    if (*thing).player.is_none() {
                        return;
                    }
                    if special.kind() == ThinkerKind::Door {
                        set_special_door_direction(special, -1);
                    } else if let ThinkerId::Plat(plat) = special
                        && special.kind() == ThinkerKind::Plat
                    {
                        (*plat.ptr()).wait = -1;
                    } else {
                        eprintln!("EV_VerticalDoor: Tried to close something that wasn't a door.");
                        set_special_door_direction(special, -1);
                    }
                }
                return;
            }
            _ => {}
        }
    }

    // for proper sound
    match line.special as i32 {
        117 | 118 => {
            S_StartSectorSound(sec, Sfx::Bdopn as i32);
        }
        1 | 31 => {
            S_StartSectorSound(sec, Sfx::Doropn as i32);
        }
        _ => {
            S_StartSectorSound(sec, Sfx::Doropn as i32);
        }
    }

    // new door thinker
    let door_id = new_thinker::<vldoor_t>();
    let door = door_id.ptr();
    P_AddThinker(ThinkerId::Door(door_id));
    let mut level = LEVEL.borrow_mut();
    level.sectors[sec].specialdata = Some(ThinkerId::Door(door_id));
    (*door).thinker.kind = ThinkerKind::Door;
    (*door).sector = sec;
    (*door).direction = 1;
    (*door).speed = VDOORSPEED;
    (*door).topwait = VDOORWAIT;

    match line.special as i32 {
        1 | 26 | 27 | 28 => {
            (*door).kind = vld_normal;
        }
        31..=34 => {
            (*door).kind = vld_open;
            level.lines[line_id].special = 0;
        }
        117 => {
            (*door).kind = vld_blazeRaise;
            (*door).speed = VDOORSPEED * 4;
        }
        118 => {
            (*door).kind = vld_blazeOpen;
            level.lines[line_id].special = 0;
            (*door).speed = VDOORSPEED * 4;
        }
        _ => {}
    }

    // find the top and bottom of the movement range
    (*door).topheight = P_FindLowestCeilingSurrounding(&level, sec);
    (*door).topheight -= 4 * FRACUNIT;
}

/// Spawn a door thinker that closes `sec` after 30 seconds.
///
/// The door starts in direction `0` (waiting) with `topcountdown = 30 * TICRATE`.
/// After the countdown it begins closing as a `vld_normal` door.
///
/// # Safety
///
/// # Panics
///
/// If `sec` is not a sector of the current map.
pub fn P_SpawnDoorCloseIn30(_world: &mut World, sec: SectorId) {
    let door_id = new_thinker::<vldoor_t>();

    P_AddThinker(ThinkerId::Door(door_id));

    {
        let mut level = LEVEL.borrow_mut();
        level.sectors[sec].specialdata = Some(ThinkerId::Door(door_id));
        level.sectors[sec].special = 0;
    }

    // SAFETY: the door was just made; nothing else refers to it yet.
    let door = unsafe { &mut *door_id.ptr() };
    door.thinker.kind = ThinkerKind::Door;
    door.sector = sec;
    door.direction = 0;
    door.kind = vld_normal;
    door.speed = VDOORSPEED;
    door.topcountdown = 30 * TICRATE;
}

/// Spawn a door thinker that opens `sec` after 5 minutes (`vld_raiseIn5Mins`).
///
/// The door starts in direction `2` (initial wait) with
/// `topcountdown = 5 * 60 * TICRATE`. `_secnum` is accepted for C ABI
/// compatibility but is not used.
///
/// # Panics
///
/// If `sec` is not a sector of the current map.
pub fn P_SpawnDoorRaiseIn5Mins(_world: &mut World, sec: SectorId, _secnum: i32) {
    let door_id = new_thinker::<vldoor_t>();

    P_AddThinker(ThinkerId::Door(door_id));

    let mut level = LEVEL.borrow_mut();
    level.sectors[sec].specialdata = Some(ThinkerId::Door(door_id));
    level.sectors[sec].special = 0;

    // SAFETY: the door was just made; nothing else refers to it yet.
    let door = unsafe { &mut *door_id.ptr() };
    door.thinker.kind = ThinkerKind::Door;
    door.sector = sec;
    door.direction = 2;
    door.kind = vld_raiseIn5Mins;
    door.speed = VDOORSPEED;
    door.topheight = P_FindLowestCeilingSurrounding(&level, sec);
    door.topheight -= 4 * FRACUNIT;
    door.topwait = VDOORWAIT;
    door.topcountdown = 5 * 60 * TICRATE;
}

#[cfg(test)]
mod tests {
    use {super::*, std::sync::Mutex};

    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn locked_object_specials_map_to_dehacked_messages() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(locked_object_message(99).as_bytes(), PD_BLUEO.as_bytes());
        assert_eq!(locked_object_message(133).as_bytes(), PD_BLUEO.as_bytes());
        assert_eq!(locked_object_message(134).as_bytes(), PD_REDO.as_bytes());
        assert_eq!(locked_object_message(135).as_bytes(), PD_REDO.as_bytes());
        assert_eq!(locked_object_message(136).as_bytes(), PD_YELLOWO.as_bytes());
        assert_eq!(locked_object_message(137).as_bytes(), PD_YELLOWO.as_bytes());
    }

    #[test]
    fn vertical_door_specials_map_to_dehacked_messages() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(locked_door_message(26).as_bytes(), PD_BLUEK.as_bytes());
        assert_eq!(locked_door_message(32).as_bytes(), PD_BLUEK.as_bytes());
        assert_eq!(locked_door_message(27).as_bytes(), PD_YELLOWK.as_bytes());
        assert_eq!(locked_door_message(34).as_bytes(), PD_YELLOWK.as_bytes());
        assert_eq!(locked_door_message(28).as_bytes(), PD_REDK.as_bytes());
        assert_eq!(locked_door_message(33).as_bytes(), PD_REDK.as_bytes());
    }
}
