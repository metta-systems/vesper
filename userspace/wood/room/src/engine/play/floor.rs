//! Floor movement and staircase construction.
//!
//! Rust port of `vendor/doomgeneric/p_floor.c`. Provides `T_MovePlane` (shared
//! by ceiling, platform, and door code), `T_MoveFloor`, `EV_DoFloor`, and
//! `EV_BuildStairs`.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::c_void;

use crate::engine::{
    map::{line_t, sector_t},
    math::fixed::{fixed_t, FRACUNIT},
    play::{
        map::P_ChangeSector,
        setup::sectors,
        spec::{
            getSector, getSide, twoSided, P_FindHighestFloorSurrounding,
            P_FindLowestCeilingSurrounding, P_FindLowestFloorSurrounding, P_FindNextHighestFloor,
            P_FindSectorFromLineTag,
        },
        tick::{leveltime, thinker_t, P_AddThinker, P_RemoveThinker},
    },
    render_3d::data::textureheight,
    sound::{info::Sfx, S_StartSectorSound},
    zone::{Z_Malloc, PU_LEVSPEC},
};

/// Movement speed for floors: 1 unit per tic in fixed-point (`FRACUNIT`).
pub const FLOORSPEED: fixed_t = FRACUNIT;

/// Convenience alias for `i32::MAX`, used as an initial "infinity" in texture height searches.
const INT_MAX: i32 = i32::MAX;

// result_e enum values
/// `T_MovePlane` return: plane moved without reaching destination or crushing.
const result_ok: i32 = 0;
/// `T_MovePlane` return: plane was blocked by a thing (crushing).
const result_crushed: i32 = 1;
/// `T_MovePlane` return: plane reached its destination this tic.
const result_pastdest: i32 = 2;

// floor_e enum values
/// Lower floor to the highest surrounding floor height.
const floor_lowerFloor: i32 = 0;
/// Lower floor to the lowest surrounding floor height.
const floor_lowerFloorToLowest: i32 = 1;
/// Lower floor at 4x speed, rising 8 units above the highest surrounding floor.
const floor_turboLower: i32 = 2;
/// Raise floor to the lowest surrounding ceiling minus 8 units (crush variant sets crush flag).
const floor_raiseFloor: i32 = 3;
/// Raise floor to the next-highest surrounding floor.
const floor_raiseFloorToNearest: i32 = 4;
/// Raise floor by the height of the shortest lower texture on its linedefs.
const floor_raiseToTexture: i32 = 5;
/// Lower floor to the lowest surrounding floor and transfer texture/special from neighbor.
const floor_lowerAndChange: i32 = 6;
/// Raise floor 24 units.
const floor_raiseFloor24: i32 = 7;
/// Raise floor 24 units and copy texture/special from the triggering linedef's front sector.
const floor_raiseFloor24AndChange: i32 = 8;
/// Raise floor to lowest surrounding ceiling (with crush enabled).
const floor_raiseFloorCrush: i32 = 9;
/// Raise floor at 4x speed to the next-highest surrounding floor.
const floor_raiseFloorTurbo: i32 = 10;
/// Raise floor for a donut special (transfers texture/special when done).
const floor_donutRaise: i32 = 11;
/// Raise floor 512 units.
const floor_raiseFloor512: i32 = 12;

// stair_e enum values
/// Build an 8-unit staircase at quarter speed (`FLOORSPEED / 4`).
const stair_build8: i32 = 0;
/// Build a 16-unit staircase at 4x speed (`FLOORSPEED * 4`).
const stair_turbo16: i32 = 1;

use crate::engine::wad::format::LinedefFlag;

/// Thinker state for a moving floor.
///
/// Each active `EV_DoFloor` or `EV_BuildStairs` call allocates one of these per
/// affected sector. `T_MoveFloor` is called every tic until the floor reaches
/// `floordestheight`.
// Only .sector is accessed in EV_BuildStairs via pointer arithmetic.
// Probe required for offset. On x86_64, sector is at offset 48.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct floormove_t {
    /// Embedded thinker header; must be the first field.
    pub thinker: thinker_t,
    /// Floor movement type (`floor_*` constant).
    pub kind: i32,
    /// Non-zero if this floor should crush things it encounters.
    pub crush: i32,
    /// The sector whose floor is being moved.
    pub sector: *mut sector_t,
    /// Direction of movement: `1` = up, `-1` = down.
    pub direction: i32,
    /// Sector special to apply when the floor finishes (`floor_lowerAndChange` / `floor_donutRaise`).
    pub newspecial: i32,
    /// Floor flat (texture) to apply when the floor finishes.
    pub texture: i16,
    _pad: [u8; 2],
    /// Target floor height in fixed-point units.
    pub floordestheight: fixed_t,
    /// Movement speed in fixed-point units per tic.
    pub speed: fixed_t,
}

/// Compile-time layout checks for `floormove_t` on 64-bit targets.
#[cfg(target_pointer_width = "64")]
mod layout_checks {
    use super::*;
    const _: () = assert!(std::mem::size_of::<floormove_t>() == 64);
    const _: () = assert!(std::mem::offset_of!(floormove_t, thinker) == 0);
    const _: () = assert!(std::mem::offset_of!(floormove_t, kind) == 24);
    const _: () = assert!(std::mem::offset_of!(floormove_t, crush) == 28);
    const _: () = assert!(std::mem::offset_of!(floormove_t, sector) == 32);
    const _: () = assert!(std::mem::offset_of!(floormove_t, direction) == 40);
    const _: () = assert!(std::mem::offset_of!(floormove_t, newspecial) == 44);
    const _: () = assert!(std::mem::offset_of!(floormove_t, texture) == 48);
    const _: () = assert!(std::mem::offset_of!(floormove_t, floordestheight) == 52);
    const _: () = assert!(std::mem::offset_of!(floormove_t, speed) == 56);
}

/// Move a plane (floor or ceiling) one step toward `dest` and check for crushing.
///
/// Shared by the floor, ceiling, platform, and door subsystems.
///
/// `floorOrCeiling`: `0` = floor, `1` = ceiling.
/// `direction`: `1` = up, `-1` = down.
/// `crush`: non-zero enables crushing things caught by the moving plane.
///
/// Returns one of:
/// - `result_ok` — moved without incident.
/// - `result_crushed` — a thing was crushed; the plane may have been reversed.
/// - `result_pastdest` — the plane reached `dest` this tic.
pub fn T_MovePlane(
    sector: *mut sector_t,
    speed: fixed_t,
    dest: fixed_t,
    crush: i32,
    floorOrCeiling: i32,
    direction: i32,
) -> i32 {
    unsafe {
        let sec = &mut *sector;
        match floorOrCeiling {
            0 => {
                // FLOOR
                match direction {
                    -1 => {
                        // DOWN
                        if sec.floorheight - speed < dest {
                            let lastpos = sec.floorheight;
                            sec.floorheight = dest;
                            let flag = P_ChangeSector(sector, crush);
                            if flag != 0 {
                                sec.floorheight = lastpos;
                                P_ChangeSector(sector, crush);
                            }
                            return result_pastdest;
                        } else {
                            let lastpos = sec.floorheight;
                            sec.floorheight -= speed;
                            let flag = P_ChangeSector(sector, crush);
                            if flag != 0 {
                                sec.floorheight = lastpos;
                                P_ChangeSector(sector, crush);
                                return result_crushed;
                            }
                        }
                    }
                    1 => {
                        // UP
                        if sec.floorheight + speed > dest {
                            let lastpos = sec.floorheight;
                            sec.floorheight = dest;
                            let flag = P_ChangeSector(sector, crush);
                            if flag != 0 {
                                sec.floorheight = lastpos;
                                P_ChangeSector(sector, crush);
                            }
                            return result_pastdest;
                        } else {
                            let lastpos = sec.floorheight;
                            sec.floorheight += speed;
                            let flag = P_ChangeSector(sector, crush);
                            if flag != 0 {
                                if crush != 0 {
                                    return result_crushed;
                                }
                                sec.floorheight = lastpos;
                                P_ChangeSector(sector, crush);
                                return result_crushed;
                            }
                        }
                    }
                    _ => {}
                }
            }
            1 => {
                // CEILING
                match direction {
                    -1 => {
                        // DOWN
                        if sec.ceilingheight - speed < dest {
                            let lastpos = sec.ceilingheight;
                            sec.ceilingheight = dest;
                            let flag = P_ChangeSector(sector, crush);
                            if flag != 0 {
                                sec.ceilingheight = lastpos;
                                P_ChangeSector(sector, crush);
                            }
                            return result_pastdest;
                        } else {
                            let lastpos = sec.ceilingheight;
                            sec.ceilingheight -= speed;
                            let flag = P_ChangeSector(sector, crush);
                            if flag != 0 {
                                if crush != 0 {
                                    return result_crushed;
                                }
                                sec.ceilingheight = lastpos;
                                P_ChangeSector(sector, crush);
                                return result_crushed;
                            }
                        }
                    }
                    1 => {
                        // UP
                        if sec.ceilingheight + speed > dest {
                            let lastpos = sec.ceilingheight;
                            sec.ceilingheight = dest;
                            let flag = P_ChangeSector(sector, crush);
                            if flag != 0 {
                                sec.ceilingheight = lastpos;
                                P_ChangeSector(sector, crush);
                            }
                            return result_pastdest;
                        } else {
                            let _lastpos = sec.ceilingheight;
                            sec.ceilingheight += speed;
                            let _flag = P_ChangeSector(sector, crush);
                            // The original C code has #if 0 here, so no crush check.
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        result_ok
    }
}

/// Per-tic update for a moving floor.
///
/// Calls `T_MovePlane` every tic and plays `sfx_stnmov` every 8 tics. When the
/// floor reaches `floordestheight` (`result_pastdest`), it applies any pending
/// texture/special changes, removes the thinker, and plays `sfx_pstop`.
///
/// # Safety
///
/// `floor` must be a valid, aligned, non-null pointer to a `floormove_t`
/// whose embedded `sector` pointer is also valid for the current map.
/// Called exclusively by the thinker dispatcher from `P_RunThinkers`.
pub unsafe fn T_MoveFloor(floor: *mut floormove_t) {
    let res = T_MovePlane(
        (*floor).sector,
        (*floor).speed,
        (*floor).floordestheight,
        (*floor).crush,
        0,
        (*floor).direction,
    );

    if (leveltime & 7) == 0 {
        S_StartSectorSound(&raw mut (*(*floor).sector).soundorg, Sfx::Stnmov as i32);
    }

    if res == result_pastdest {
        (*(*floor).sector).specialdata = std::ptr::null_mut();

        if (*floor).direction == 1 {
            if (*floor).kind == floor_donutRaise {
                (*(*floor).sector).special = (*floor).newspecial as i16;
                (*(*floor).sector).floorpic = (*floor).texture;
            }
        } else if (*floor).direction == -1 && (*floor).kind == floor_lowerAndChange {
            (*(*floor).sector).special = (*floor).newspecial as i16;
            (*(*floor).sector).floorpic = (*floor).texture;
        }
        P_RemoveThinker(&mut (*floor).thinker as *mut thinker_t);

        S_StartSectorSound(&raw mut (*(*floor).sector).soundorg, Sfx::Pstop as i32);
    }
}

/// Linedef-triggered event: activate floor movement on all tagged sectors.
///
/// For each sector whose tag matches `line`'s tag and that has no active
/// special, allocates a `floormove_t` thinker and configures it according to
/// `floortype`. Returns `1` if at least one floor was started, `0` otherwise.
///
/// # Safety
///
/// `line` must be a valid non-null pointer for the current map; the global
/// `sectors` array must be initialised.
///
/// # FIXME
/// The `floor_raiseFloorCrush` arm only sets `crush = 1` and leaves direction,
/// sector, speed, and `floordestheight` uninitialised. In C, the switch falls
/// through into `raiseFloor`, which sets those fields. The Rust port handles
/// `raiseFloor` and `raiseFloorCrush` as separate arms; only the `raiseFloor`
/// arm sets direction/speed/dest, so `raiseFloorCrush` sectors never actually
/// move. This is a behavioural divergence from the C original.
///
/// # FIXME
/// In the `floor_raiseFloor24` arm, the line
/// `(*floor).floordestheight = (*floor).sector.offset_from(sec) as fixed_t`
/// is a leftover dead assignment that is immediately overwritten. It is harmless
/// (the correct value is set on the next line) but should be removed.
pub unsafe fn EV_DoFloor(line: *mut line_t, floortype: i32) -> i32 {
    let mut secnum: i32 = -1;
    let mut rtn: i32 = 0;

    while {
        secnum = P_FindSectorFromLineTag(line, secnum);
        secnum
    } >= 0
    {
        let sec = sectors.add(secnum as usize);

        if !(*sec).specialdata.is_null() {
            continue;
        }

        rtn = 1;
        let floor = Z_Malloc(
            std::mem::size_of::<floormove_t>() as i32,
            PU_LEVSPEC,
            std::ptr::null_mut(),
        ) as *mut floormove_t;
        P_AddThinker(&mut (*floor).thinker);
        (*sec).specialdata = floor as *mut c_void;
        (*floor).thinker.function.acp1 = Some(core::mem::transmute::<
            unsafe fn(*mut floormove_t),
            unsafe fn(*mut c_void),
        >(T_MoveFloor));
        (*floor).kind = floortype;
        (*floor).crush = 0;

        match floortype {
            floor_lowerFloor => {
                (*floor).direction = -1;
                (*floor).sector = sec;
                (*floor).speed = FLOORSPEED;
                (*floor).floordestheight = P_FindHighestFloorSurrounding(sec);
            }
            floor_lowerFloorToLowest => {
                (*floor).direction = -1;
                (*floor).sector = sec;
                (*floor).speed = FLOORSPEED;
                (*floor).floordestheight = P_FindLowestFloorSurrounding(sec);
            }
            floor_turboLower => {
                (*floor).direction = -1;
                (*floor).sector = sec;
                (*floor).speed = FLOORSPEED * 4;
                (*floor).floordestheight = P_FindHighestFloorSurrounding(sec);
                if (*floor).floordestheight != (*sec).floorheight {
                    (*floor).floordestheight += 8 * FRACUNIT;
                }
            }
            floor_raiseFloorCrush => {
                // FIXME: In C, `raiseFloorCrush` falls through into `raiseFloor`,
                // so it sets crush = true AND then configures direction/speed/dest.
                // Here the arms are separate: only crush is set; the floor never
                // actually moves. This diverges from the C original.
                (*floor).crush = 1;
            }
            floor_raiseFloor => {
                (*floor).direction = 1;
                (*floor).sector = sec;
                (*floor).speed = FLOORSPEED;
                (*floor).floordestheight = P_FindLowestCeilingSurrounding(sec);
                if (*floor).floordestheight > (*sec).ceilingheight {
                    (*floor).floordestheight = (*sec).ceilingheight;
                }
                (*floor).floordestheight -=
                    (8 * FRACUNIT) * ((floortype == floor_raiseFloorCrush) as i32);
            }
            floor_raiseFloorTurbo => {
                (*floor).direction = 1;
                (*floor).sector = sec;
                (*floor).speed = FLOORSPEED * 4;
                (*floor).floordestheight = P_FindNextHighestFloor(sec, (*sec).floorheight);
            }
            floor_raiseFloorToNearest => {
                (*floor).direction = 1;
                (*floor).sector = sec;
                (*floor).speed = FLOORSPEED;
                (*floor).floordestheight = P_FindNextHighestFloor(sec, (*sec).floorheight);
            }
            floor_raiseFloor24 => {
                (*floor).direction = 1;
                (*floor).sector = sec;
                (*floor).speed = FLOORSPEED;
                // FIXME: the next line is a dead assignment (offset_from produces the wrong type
                // and the value is immediately overwritten); it should be removed.
                (*floor).floordestheight = (*floor).sector.offset_from(sec) as fixed_t;
                (*floor).floordestheight = (*sec).floorheight + 24 * FRACUNIT;
            }
            floor_raiseFloor512 => {
                (*floor).direction = 1;
                (*floor).sector = sec;
                (*floor).speed = FLOORSPEED;
                (*floor).floordestheight = (*sec).floorheight + 512 * FRACUNIT;
            }
            floor_raiseFloor24AndChange => {
                (*floor).direction = 1;
                (*floor).sector = sec;
                (*floor).speed = FLOORSPEED;
                (*floor).floordestheight = (*sec).floorheight + 24 * FRACUNIT;
                (*sec).floorpic = (*(*line).frontsector).floorpic;
                (*sec).special = (*(*line).frontsector).special;
            }
            floor_raiseToTexture => {
                (*floor).direction = 1;
                (*floor).sector = sec;
                (*floor).speed = FLOORSPEED;
                let mut minsize: i32 = INT_MAX;
                for i in 0..(*sec).linecount as usize {
                    if twoSided(secnum, i as i32) != 0 {
                        let side = getSide(secnum, i as i32, 0);
                        if (*side).bottomtexture >= 0 {
                            let th = *textureheight.offset((*side).bottomtexture as isize);
                            if th < minsize {
                                minsize = th;
                            }
                        }
                        let side = getSide(secnum, i as i32, 1);
                        if (*side).bottomtexture >= 0 {
                            let th = *textureheight.offset((*side).bottomtexture as isize);
                            if th < minsize {
                                minsize = th;
                            }
                        }
                    }
                }
                (*floor).floordestheight = (*sec).floorheight + minsize;
            }
            floor_lowerAndChange => {
                (*floor).direction = -1;
                (*floor).sector = sec;
                (*floor).speed = FLOORSPEED;
                (*floor).floordestheight = P_FindLowestFloorSurrounding(sec);
                (*floor).texture = (*sec).floorpic;

                for i in 0..(*sec).linecount {
                    if twoSided(secnum, i) != 0 {
                        let side0 = getSide(secnum, i, 0);
                        if (*side0).sector.offset_from(sectors) as i32 == secnum {
                            let check_sec = getSector(secnum, i, 1);
                            if (*check_sec).floorheight == (*floor).floordestheight {
                                (*floor).texture = (*check_sec).floorpic;
                                (*floor).newspecial = (*check_sec).special as i32;
                                break;
                            }
                        } else {
                            let check_sec = getSector(secnum, i, 0);
                            if (*check_sec).floorheight == (*floor).floordestheight {
                                (*floor).texture = (*check_sec).floorpic;
                                (*floor).newspecial = (*check_sec).special as i32;
                                break;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    rtn
}

/// Linedef-triggered event: build a rising staircase starting from tagged sectors.
///
/// For each tagged sector, raises its floor by `stairsize` (8 or 16 units
/// depending on `stype`), then walks adjacent sectors that share the same floor
/// texture, raising each step by another `stairsize`. Sectors already moving are
/// skipped but the height counter still increments so the step sequence is
/// preserved.
///
/// Returns `1` if at least one stair was started, `0` otherwise.
///
/// # Safety
///
/// `line` must be a valid non-null pointer for the current map; the global
/// `sectors` array must be initialised.
pub unsafe fn EV_BuildStairs(line: *mut line_t, stype: i32) -> i32 {
    let mut secnum: i32 = -1;
    let mut rtn: i32 = 0;

    while {
        secnum = P_FindSectorFromLineTag(line, secnum);
        secnum
    } >= 0
    {
        let mut sec = sectors.add(secnum as usize);

        if !(*sec).specialdata.is_null() {
            continue;
        }

        rtn = 1;
        let mut floor = Z_Malloc(
            std::mem::size_of::<floormove_t>() as i32,
            PU_LEVSPEC,
            std::ptr::null_mut(),
        ) as *mut floormove_t;
        P_AddThinker(&mut (*floor).thinker);
        (*sec).specialdata = floor as *mut c_void;
        (*floor).thinker.function.acp1 = Some(core::mem::transmute::<
            unsafe fn(*mut floormove_t),
            unsafe fn(*mut c_void),
        >(T_MoveFloor));
        (*floor).direction = 1;
        (*floor).sector = sec;

        let (speed, stairsize): (fixed_t, fixed_t) = match stype {
            stair_build8 => (FLOORSPEED / 4, 8 * FRACUNIT),
            stair_turbo16 => (FLOORSPEED * 4, 16 * FRACUNIT),
            _ => (FLOORSPEED, 8 * FRACUNIT),
        };

        (*floor).speed = speed;
        let mut height: fixed_t = (*sec).floorheight + stairsize;
        (*floor).floordestheight = height;

        let texture = (*sec).floorpic;

        // Find next sector to raise
        loop {
            let mut ok: i32 = 0;
            for i in 0..(*sec).linecount {
                let l = *(*sec).lines.offset(i as isize);
                if (*l).flags & LinedefFlag::TWOSIDED as i16 == 0 {
                    continue;
                }

                let mut tsec = (*l).frontsector;
                let newsecnum = tsec.offset_from(sectors) as i32;

                if secnum != newsecnum {
                    continue;
                }

                tsec = (*l).backsector;
                let newsecnum = tsec.offset_from(sectors) as i32;

                if (*tsec).floorpic != texture {
                    continue;
                }

                height += stairsize;

                if !(*tsec).specialdata.is_null() {
                    continue;
                }

                sec = tsec;
                let _secnum_new = newsecnum;
                floor = Z_Malloc(
                    std::mem::size_of::<floormove_t>() as i32,
                    PU_LEVSPEC,
                    std::ptr::null_mut(),
                ) as *mut floormove_t;

                P_AddThinker(&mut (*floor).thinker);

                (*sec).specialdata = floor as *mut c_void;
                (*floor).thinker.function.acp1 = Some(core::mem::transmute::<
                    unsafe fn(*mut floormove_t),
                    unsafe fn(*mut c_void),
                >(T_MoveFloor));
                (*floor).direction = 1;
                (*floor).sector = sec;
                (*floor).speed = speed;
                (*floor).floordestheight = height;
                ok = 1;
                break;
            }
            if ok == 0 {
                break;
            }
        }
    }
    rtn
}

#[cfg(test)]
mod tests {
    use {super::*, std::sync::Mutex};

    static LOCK: Mutex<()> = Mutex::new(());

    const FLOORMOVE_T_SIZEOF: usize = 64;
    const FLOORMOVE_T_THINKER: usize = 0;
    const FLOORMOVE_T_SECTOR: usize = 32;
    const FLOORMOVE_T_DIRECTION: usize = 40;
    const FLOORMOVE_T_FLOORDESTHEIGHT: usize = 52;
    const FLOORMOVE_T_SPEED: usize = 56;
    const FLOORMOVE_T_TEXTURE: usize = 48;

    #[test]
    fn floormove_t_layout_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(
            std::mem::size_of::<floormove_t>(),
            FLOORMOVE_T_SIZEOF,
            "floormove_t size mismatch",
        );
        assert_eq!(
            std::mem::offset_of!(floormove_t, thinker),
            FLOORMOVE_T_THINKER
        );
        assert_eq!(
            std::mem::offset_of!(floormove_t, sector),
            FLOORMOVE_T_SECTOR
        );
        assert_eq!(
            std::mem::offset_of!(floormove_t, direction),
            FLOORMOVE_T_DIRECTION
        );
        assert_eq!(
            std::mem::offset_of!(floormove_t, floordestheight),
            FLOORMOVE_T_FLOORDESTHEIGHT
        );
        assert_eq!(std::mem::offset_of!(floormove_t, speed), FLOORMOVE_T_SPEED);
        assert_eq!(
            std::mem::offset_of!(floormove_t, texture),
            FLOORMOVE_T_TEXTURE
        );
    }
}
