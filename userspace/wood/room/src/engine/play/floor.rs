//! Floor movement and staircase construction.
//!
//! Rust port of `vendor/doomgeneric/p_floor.c`. Provides `T_MovePlane` (shared
//! by ceiling, platform, and door code), `T_MoveFloor`, `EV_DoFloor`, and
//! `EV_BuildStairs`.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::engine::{
    map::{SectorId, line_t},
    math::fixed::{FRACUNIT, fixed_t},
    play::{
        map::P_ChangeSector,
        setup::LevelState,
        spec::{
            P_FindHighestFloorSurrounding, P_FindLowestCeilingSurrounding,
            P_FindLowestFloorSurrounding, P_FindNextHighestFloor, P_FindSectorFromLineTag,
            getSector, getSide, twoSided,
        },
        tick::{FloorId, P_RemoveThinker, ThinkerId, ThinkerKind, Thinkers, ZeroValid, thinker_t},
    },
    render_3d::data::texture_height,
    sound::{S_StartSectorSound, info::Sfx},
    world::World,
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
#[derive(Clone, Copy)]
pub struct floormove_t {
    /// Thinker state: what the floor does each tic.
    pub thinker: thinker_t,
    /// Floor movement type (`floor_*` constant).
    pub kind: i32,
    /// Non-zero if this floor should crush things it encounters.
    pub crush: i32,
    /// The sector whose floor is being moved.
    pub sector: SectorId,
    /// Direction of movement: `1` = up, `-1` = down.
    pub direction: i32,
    /// Sector special to apply when the floor finishes (`floor_lowerAndChange` / `floor_donutRaise`).
    pub newspecial: i32,
    /// Floor flat (texture) to apply when the floor finishes.
    pub texture: i16,
    /// The two C padding bytes after `texture`; only the door alias below
    /// touches them. Not saved, so zero after a load.
    pub texture_upper_half: u16,
    /// Target floor height in fixed-point units.
    pub floordestheight: fixed_t,
    /// Movement speed in fixed-point units per tic.
    pub speed: fixed_t,
}

impl floormove_t {
    /// What vanilla `EV_VerticalDoor` reads as `door->direction` when a
    /// sector's `specialdata` is a floor: the little-endian `int` made of
    /// `texture` and the two C padding bytes after it (64-bit C layout),
    /// kept in `texture_upper_half`.
    ///
    /// The upper half matters after a save-game load: a door may have
    /// written -1 here, but only the 16-bit `texture` is saved, so the
    /// loaded floor reads 0x0000_FFFF (65535), not -1.
    pub fn door_direction_alias(&self) -> i32 {
        ((u32::from(self.texture_upper_half) << 16) | u32::from(self.texture as u16)) as i32
    }

    /// Store a door `direction` where
    /// [`door_direction_alias`](Self::door_direction_alias) reads it.
    pub fn set_door_direction_alias(&mut self, direction: i32) {
        self.texture = direction as i16;
        self.texture_upper_half = (direction >> 16) as u16;
    }
}

/// Height of `sector`'s floor (`floorOrCeiling == 0`) or ceiling.
fn plane_height(level: &LevelState, sector: SectorId, floorOrCeiling: i32) -> fixed_t {
    let sec = &level.sectors[sector];
    if floorOrCeiling == 0 {
        sec.floorheight
    } else {
        sec.ceilingheight
    }
}

/// Set the height of `sector`'s floor (`floorOrCeiling == 0`) or ceiling.
fn set_plane_height(
    level: &mut LevelState,
    sector: SectorId,
    floorOrCeiling: i32,
    height: fixed_t,
) {
    let sec = &mut level.sectors[sector];
    if floorOrCeiling == 0 {
        sec.floorheight = height;
    } else {
        sec.ceilingheight = height;
    }
}

/// Move a plane (floor or ceiling) one step toward `dest` and check for crushing.
///
/// Shared by the floor, ceiling, platform, and door subsystems.
///
/// `floorOrCeiling`: `0` = floor, `1` = ceiling.
/// `direction`: `1` = up, `-1` = down.
/// `crush`: non-zero enables crushing things caught by the moving plane.
///
/// Each height change is written to the level before `P_ChangeSector`
/// re-checks the things it moves (which can run any play code).
///
/// Returns one of:
/// - `result_ok` — moved without incident.
/// - `result_crushed` — a thing was crushed; the plane may have been reversed.
/// - `result_pastdest` — the plane reached `dest` this tic.
pub fn T_MovePlane(
    world: &mut World,
    sector: SectorId,
    speed: fixed_t,
    dest: fixed_t,
    crush: i32,
    floorOrCeiling: i32,
    direction: i32,
) -> i32 {
    match floorOrCeiling {
        0 => {
            // FLOOR
            match direction {
                -1 => {
                    // DOWN
                    let lastpos = plane_height(&world.level, sector, 0);
                    if lastpos - speed < dest {
                        set_plane_height(&mut world.level, sector, 0, dest);
                        let flag = P_ChangeSector(world, sector, crush);
                        if flag != 0 {
                            set_plane_height(&mut world.level, sector, 0, lastpos);
                            P_ChangeSector(world, sector, crush);
                        }
                        return result_pastdest;
                    } else {
                        set_plane_height(&mut world.level, sector, 0, lastpos - speed);
                        let flag = P_ChangeSector(world, sector, crush);
                        if flag != 0 {
                            set_plane_height(&mut world.level, sector, 0, lastpos);
                            P_ChangeSector(world, sector, crush);
                            return result_crushed;
                        }
                    }
                }
                1 => {
                    // UP
                    let lastpos = plane_height(&world.level, sector, 0);
                    if lastpos + speed > dest {
                        set_plane_height(&mut world.level, sector, 0, dest);
                        let flag = P_ChangeSector(world, sector, crush);
                        if flag != 0 {
                            set_plane_height(&mut world.level, sector, 0, lastpos);
                            P_ChangeSector(world, sector, crush);
                        }
                        return result_pastdest;
                    } else {
                        set_plane_height(&mut world.level, sector, 0, lastpos + speed);
                        let flag = P_ChangeSector(world, sector, crush);
                        if flag != 0 {
                            if crush != 0 {
                                return result_crushed;
                            }
                            set_plane_height(&mut world.level, sector, 0, lastpos);
                            P_ChangeSector(world, sector, crush);
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
                    let lastpos = plane_height(&world.level, sector, 1);
                    if lastpos - speed < dest {
                        set_plane_height(&mut world.level, sector, 1, dest);
                        let flag = P_ChangeSector(world, sector, crush);
                        if flag != 0 {
                            set_plane_height(&mut world.level, sector, 1, lastpos);
                            P_ChangeSector(world, sector, crush);
                        }
                        return result_pastdest;
                    } else {
                        set_plane_height(&mut world.level, sector, 1, lastpos - speed);
                        let flag = P_ChangeSector(world, sector, crush);
                        if flag != 0 {
                            if crush != 0 {
                                return result_crushed;
                            }
                            set_plane_height(&mut world.level, sector, 1, lastpos);
                            P_ChangeSector(world, sector, crush);
                            return result_crushed;
                        }
                    }
                }
                1 => {
                    // UP
                    let lastpos = plane_height(&world.level, sector, 1);
                    if lastpos + speed > dest {
                        set_plane_height(&mut world.level, sector, 1, dest);
                        let flag = P_ChangeSector(world, sector, crush);
                        if flag != 0 {
                            set_plane_height(&mut world.level, sector, 1, lastpos);
                            P_ChangeSector(world, sector, crush);
                        }
                        return result_pastdest;
                    } else {
                        set_plane_height(&mut world.level, sector, 1, lastpos + speed);
                        let _flag = P_ChangeSector(world, sector, crush);
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

/// Per-tic update for a moving floor.
///
/// Calls `T_MovePlane` every tic and plays `sfx_stnmov` every 8 tics. When the
/// floor reaches `floordestheight` (`result_pastdest`), it applies any pending
/// texture/special changes, removes the thinker, and plays `sfx_pstop`.
///
/// Called exclusively by the thinker dispatcher from `P_RunThinkers`.
pub fn T_MoveFloor(world: &mut World, floor_id: FloorId) {
    let floor = world.thinkers.floors[floor_id];
    let res = T_MovePlane(
        world,
        floor.sector,
        floor.speed,
        floor.floordestheight,
        floor.crush,
        0,
        floor.direction,
    );
    // `T_MovePlane` can run play code: read the floor afresh, as C does
    // through its pointer.
    let floor_sector = world.thinkers.floors[floor_id].sector;

    if (world.leveltime & 7) == 0 {
        S_StartSectorSound(world, floor_sector, Sfx::Stnmov as i32);
    }

    if res == result_pastdest {
        {
            let floors = &mut world.thinkers.floors;
            let floor = &mut floors[floor_id];
            let sec = &mut world.level.sectors[floor.sector];
            sec.specialdata = None;

            if floor.direction == 1 {
                if floor.kind == floor_donutRaise {
                    sec.special = floor.newspecial as i16;
                    sec.floorpic = floor.texture;
                }
            } else if floor.direction == -1 && floor.kind == floor_lowerAndChange {
                sec.special = floor.newspecial as i16;
                sec.floorpic = floor.texture;
            }
            P_RemoveThinker(&mut floor.thinker);
        }

        S_StartSectorSound(world, floor_sector, Sfx::Pstop as i32);
    }
}

/// Make a zeroed floor mover, list it as a thinker and make it `sector`'s
/// special (C's `Z_Malloc`, `P_AddThinker` and `sec->specialdata = floor`).
///
/// The caller fills in the mover through the returned handle; it holds the
/// world's level and thinkers apart, so they are passed separately.
pub fn add_floor_mover(
    level: &mut LevelState,
    thinkers: &mut Thinkers,
    sector: SectorId,
) -> FloorId {
    let floor = thinkers.floors.insert(floormove_t::zeroed());
    thinkers.list.push(ThinkerId::Floor(floor));
    level.sectors[sector].specialdata = Some(ThinkerId::Floor(floor));
    floor
}

/// Linedef-triggered event: activate floor movement on all tagged sectors.
///
/// For each sector whose tag matches `line`'s tag and that has no active
/// special, allocates a `floormove_t` thinker and configures it according to
/// `floortype`. Returns `1` if at least one floor was started, `0` otherwise.
/// `line` is the activating line's record (or the synthetic tag-666 line
/// of a boss death).
///
/// `floor_raiseFloorCrush` falls through into `floor_raiseFloor` as in C:
/// it sets `crush` and then moves like a raising floor that stops 8 units
/// lower.
///
/// # Panics
///
/// For `floor_raiseToTexture`, if a bottom texture of the sector's lines is
/// not a loaded texture.
pub fn EV_DoFloor(world: &mut World, line: &line_t, floortype: i32) -> i32 {
    let level = &mut world.level;
    let mut secnum: i32 = -1;
    let mut rtn: i32 = 0;

    while {
        secnum = P_FindSectorFromLineTag(level, line, secnum);
        secnum
    } >= 0
    {
        let sec = SectorId::new(secnum as usize);

        if level.sectors[sec].specialdata.is_some() {
            continue;
        }

        rtn = 1;
        let floor_id = add_floor_mover(level, &mut world.thinkers, sec);
        let floors = &mut world.thinkers.floors;
        let floor = &mut floors[floor_id];

        floor.thinker.kind = ThinkerKind::Floor;
        floor.kind = floortype;
        floor.crush = 0;

        match floortype {
            floor_lowerFloor => {
                floor.direction = -1;
                floor.sector = sec;
                floor.speed = FLOORSPEED;
                floor.floordestheight = P_FindHighestFloorSurrounding(level, sec);
            }
            floor_lowerFloorToLowest => {
                floor.direction = -1;
                floor.sector = sec;
                floor.speed = FLOORSPEED;
                floor.floordestheight = P_FindLowestFloorSurrounding(level, sec);
            }
            floor_turboLower => {
                floor.direction = -1;
                floor.sector = sec;
                floor.speed = FLOORSPEED * 4;
                floor.floordestheight = P_FindHighestFloorSurrounding(level, sec);
                if floor.floordestheight != level.sectors[sec].floorheight {
                    floor.floordestheight += 8 * FRACUNIT;
                }
            }
            floor_raiseFloorCrush | floor_raiseFloor => {
                if floortype == floor_raiseFloorCrush {
                    floor.crush = 1;
                }
                floor.direction = 1;
                floor.sector = sec;
                floor.speed = FLOORSPEED;
                floor.floordestheight = P_FindLowestCeilingSurrounding(level, sec);
                if floor.floordestheight > level.sectors[sec].ceilingheight {
                    floor.floordestheight = level.sectors[sec].ceilingheight;
                }
                floor.floordestheight -=
                    (8 * FRACUNIT) * ((floortype == floor_raiseFloorCrush) as i32);
            }
            floor_raiseFloorTurbo => {
                floor.direction = 1;
                floor.sector = sec;
                floor.speed = FLOORSPEED * 4;
                floor.floordestheight =
                    P_FindNextHighestFloor(level, sec, level.sectors[sec].floorheight);
            }
            floor_raiseFloorToNearest => {
                floor.direction = 1;
                floor.sector = sec;
                floor.speed = FLOORSPEED;
                floor.floordestheight =
                    P_FindNextHighestFloor(level, sec, level.sectors[sec].floorheight);
            }
            floor_raiseFloor24 => {
                floor.direction = 1;
                floor.sector = sec;
                floor.speed = FLOORSPEED;
                floor.floordestheight = level.sectors[sec].floorheight + 24 * FRACUNIT;
            }
            floor_raiseFloor512 => {
                floor.direction = 1;
                floor.sector = sec;
                floor.speed = FLOORSPEED;
                floor.floordestheight = level.sectors[sec].floorheight + 512 * FRACUNIT;
            }
            floor_raiseFloor24AndChange => {
                floor.direction = 1;
                floor.sector = sec;
                floor.speed = FLOORSPEED;
                floor.floordestheight = level.sectors[sec].floorheight + 24 * FRACUNIT;
                let frontsector = level.sectors[line
                    .frontsector
                    .expect("activating line without front sector")];
                level.sectors[sec].floorpic = frontsector.floorpic;
                level.sectors[sec].special = frontsector.special;
            }
            floor_raiseToTexture => {
                floor.direction = 1;
                floor.sector = sec;
                floor.speed = FLOORSPEED;
                let mut minsize: i32 = INT_MAX;
                for i in 0..level.sectors[sec].linecount {
                    if twoSided(level, secnum, i) != 0 {
                        let side = getSide(level, secnum, i, 0);
                        if level.sides[side].bottomtexture >= 0 {
                            let th = texture_height(level.sides[side].bottomtexture as usize);
                            if th < minsize {
                                minsize = th;
                            }
                        }
                        let side = getSide(level, secnum, i, 1);
                        if level.sides[side].bottomtexture >= 0 {
                            let th = texture_height(level.sides[side].bottomtexture as usize);
                            if th < minsize {
                                minsize = th;
                            }
                        }
                    }
                }
                floor.floordestheight = level.sectors[sec].floorheight + minsize;
            }
            floor_lowerAndChange => {
                floor.direction = -1;
                floor.sector = sec;
                floor.speed = FLOORSPEED;
                floor.floordestheight = P_FindLowestFloorSurrounding(level, sec);
                floor.texture = level.sectors[sec].floorpic;

                for i in 0..level.sectors[sec].linecount {
                    if twoSided(level, secnum, i) != 0 {
                        let side0 = getSide(level, secnum, i, 0);
                        let check_sec = if level.sides[side0].sector == sec {
                            getSector(level, secnum, i, 1)
                        } else {
                            getSector(level, secnum, i, 0)
                        };
                        let check_sec = &level.sectors[check_sec];
                        if check_sec.floorheight == floor.floordestheight {
                            floor.texture = check_sec.floorpic;
                            floor.newspecial = check_sec.special as i32;
                            break;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    rtn
}

/// Start one step of a staircase: a floor mover raising `sector` to
/// `height` at `speed`.
fn start_stair_step(
    level: &mut LevelState,
    thinkers: &mut Thinkers,
    sector: SectorId,
    speed: fixed_t,
    height: fixed_t,
) {
    let floor_id = add_floor_mover(level, thinkers, sector);
    let floor = &mut thinkers.floors[floor_id];
    floor.thinker.kind = ThinkerKind::Floor;
    floor.direction = 1;
    floor.sector = sector;
    floor.speed = speed;
    floor.floordestheight = height;
}

/// Linedef-triggered event: build a rising staircase starting from tagged sectors.
///
/// For each tagged sector, raises its floor by `stairsize` (8 or 16 units
/// depending on `stype`), then walks adjacent sectors that share the same floor
/// texture, raising each step by another `stairsize`. Sectors already moving are
/// skipped but the height counter still increments so the step sequence is
/// preserved.
///
/// As in C, `secnum` advances to each new step's sector, so the next step is
/// found behind lines whose front is the current step, and the tag search
/// resumes after the last step's sector number.
///
/// Returns `1` if at least one stair was started, `0` otherwise.
pub fn EV_BuildStairs(world: &mut World, line: &line_t, stype: i32) -> i32 {
    let level = &mut world.level;
    let mut secnum: i32 = -1;
    let mut rtn: i32 = 0;

    while {
        secnum = P_FindSectorFromLineTag(level, line, secnum);
        secnum
    } >= 0
    {
        let mut sec = SectorId::new(secnum as usize);

        if level.sectors[sec].specialdata.is_some() {
            continue;
        }

        rtn = 1;
        let (speed, stairsize): (fixed_t, fixed_t) = match stype {
            stair_build8 => (FLOORSPEED / 4, 8 * FRACUNIT),
            stair_turbo16 => (FLOORSPEED * 4, 16 * FRACUNIT),
            _ => (FLOORSPEED, 8 * FRACUNIT),
        };
        let (floorheight, texture) = (level.sectors[sec].floorheight, level.sectors[sec].floorpic);
        let mut height: fixed_t = floorheight + stairsize;
        start_stair_step(level, &mut world.thinkers, sec, speed, height);

        // Find next sector to raise
        loop {
            let mut ok: i32 = 0;
            let linecount = level.sectors[sec].linecount;
            for i in 0..linecount as usize {
                let tsec = {
                    let l = &level.lines[level.sector_lines(sec)[i]];
                    if l.flags & LinedefFlag::TWOSIDED as i16 == 0 {
                        continue;
                    }

                    let newsecnum = l.frontsector.map(|tsec| tsec.index() as i32);

                    if Some(secnum) != newsecnum {
                        continue;
                    }

                    let tsec = l.backsector.expect("two-sided line without back sector");

                    if level.sectors[tsec].floorpic != texture {
                        continue;
                    }

                    height += stairsize;

                    if level.sectors[tsec].specialdata.is_some() {
                        continue;
                    }
                    tsec
                };

                sec = tsec;
                secnum = tsec.index() as i32;
                start_stair_step(level, &mut world.thinkers, sec, speed, height);
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
    use {
        super::*,
        crate::engine::{
            map::{LineId, VertexId, sector_t, slopetype_t},
            wad::format::LinedefFlag,
        },
        std::sync::Mutex,
    };

    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn door_direction_alias_is_the_c_texture_pun() {
        let _g = LOCK.lock().unwrap();
        let mut floor: floormove_t = unsafe { std::mem::zeroed() };
        floor.texture = 0x1234;
        assert_eq!(floor.door_direction_alias(), 0x1234);

        // EV_VerticalDoor writing direction -1 into a floor clobbers the
        // texture (and, in C, its padding); writing 1 leaves texture 1.
        for direction in [-1, 0, 1, -1] {
            floor.set_door_direction_alias(direction);
            assert_eq!(floor.texture, direction as i16);
            assert_eq!(floor.door_direction_alias(), direction);
        }

        // A save keeps only `texture`; the loaded floor has zero upper bytes.
        let saved_texture = floor.texture;
        let mut loaded: floormove_t = unsafe { std::mem::zeroed() };
        loaded.texture = saved_texture;
        assert_eq!(loaded.door_direction_alias(), 0xFFFF);
    }

    /// A two-sided line from `front` to `back`.
    fn step_line(front: usize, back: usize) -> line_t {
        line_t {
            v1: VertexId::new(0),
            v2: VertexId::new(0),
            dx: 0,
            dy: 0,
            flags: LinedefFlag::TWOSIDED as i16,
            special: 0,
            tag: 0,
            sidenum: [0, 1],
            bbox: [0; 4],
            slopetype: slopetype_t::ST_HORIZONTAL,
            frontsector: Some(SectorId::new(front)),
            backsector: Some(SectorId::new(back)),
            validcount: 0,
            specialdata: None,
        }
    }

    /// Three sectors in a row with the same floor flat, the first tagged:
    /// each step raises one `stairsize` above the previous, so the walk
    /// must move on from each new step, not only from the tagged sector.
    #[test]
    fn build_stairs_raises_every_step() {
        let step_sector = |tag: i16, lines: usize, linecount: i32| {
            let mut sector = sector_t::ZERO;
            sector.tag = tag;
            sector.floorpic = 7;
            sector.lines = lines;
            sector.linecount = linecount;
            sector
        };
        let mut world = World::new();
        world.level = LevelState {
            numsectors: 3,
            sectors: vec![
                step_sector(1, 0, 1),
                step_sector(0, 1, 2),
                step_sector(0, 3, 1),
            ],
            lines: vec![step_line(0, 1), step_line(1, 2)],
            linebuffer: vec![
                LineId::new(0),
                LineId::new(0),
                LineId::new(1),
                LineId::new(1),
            ],
            ..LevelState::new()
        };
        let mut trigger = step_line(0, 1);
        trigger.tag = 1;

        assert_eq!(EV_BuildStairs(&mut world, &trigger, stair_build8), 1);
        let floors = &world.thinkers.floors;
        let destinations: Vec<fixed_t> = world
            .level
            .sectors
            .iter()
            .map(|sector| {
                let Some(ThinkerId::Floor(floor)) = sector.specialdata else {
                    panic!("every step gets a mover");
                };
                floors[floor].floordestheight
            })
            .collect();
        assert_eq!(destinations, [8 * FRACUNIT, 16 * FRACUNIT, 24 * FRACUNIT]);
    }
}
