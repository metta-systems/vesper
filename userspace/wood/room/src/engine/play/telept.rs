//! Teleportation action special.
//!
//! Rust port of `vendor/doomgeneric/p_telept.c`.  Handles the `EV_Teleport`
//! linedef special: locating the destination `MT_TELEPORTMAN` marker, moving
//! the triggering thing to that position, and spawning teleport fog at both
//! the source and destination.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::engine::{
    map::line_t,
    math::tables::ANGLETOFINESHIFT,
    play::{info::*, mobj::MobjId},
};

/// Doom version constant for the first Final Doom executable.
///
/// `EV_Teleport` uses this to preserve a known quirk: the original Final Doom
/// binary does not set `thing->z` to `floorz` after teleporting.
const EXE_FINAL: i32 = 7;

use crate::engine::{
    doom::stat::gameversion,
    math::tables::{FINEANGLES, finesine},
    play::{
        map::P_TeleportMove,
        mobj::P_SpawnMobj,
        tick::{ThinkerId, ThinkerKind, thinker_at},
    },
    sound::{S_StartSound, info::Sfx},
    world::World,
};

/// Teleport `thing` across the linedef special `line` if all conditions are met.
///
/// Searches all sectors whose tag matches `line->tag` for an `MT_TELEPORTMAN`
/// marker.  When one is found:
///
/// 1. Calls `P_TeleportMove` to clip-move `thing` to the marker's position,
///    telefragging any blocking enemies.
/// 2. Sets `thing->z` to floor height (skipped for the first Final Doom
///    executable — `gameversion == EXE_FINAL` (7); see `doomstat.h`).
/// 3. Adjusts the player's `viewz` if `thing` is a player.
/// 4. Spawns `MT_TFOG` at both the source and destination, playing
///    `sfx_telept` at each.
/// 5. Sets `reactiontime = 18` to freeze player movement briefly.
/// 6. Zeros `thing`'s momentum and copies the marker's angle.
///
/// Returns `1` on a successful teleport, `0` if no suitable destination was
/// found or if `P_TeleportMove` failed.  Missiles (`MF_MISSILE`) and things
/// that hit the back of the line (`side == 1`) are rejected immediately.
///
/// Corresponds to `EV_Teleport` in `p_telept.c`.
///
/// # Safety
///
/// `thing_id` must name a map object linked into the map, and a level must
/// be loaded.
pub unsafe fn EV_Teleport(world: &mut World, line: &line_t, side: i32, thing_id: MobjId) -> i32 {
    // Don't teleport missiles
    if world.mobjs[thing_id].flags & MF_MISSILE != 0 {
        return 0;
    }

    // Don't teleport if hit back of line
    if side == 1 {
        return 0;
    }

    let tag = line.tag;
    let numsectors = world.level.numsectors;

    for i in 0..numsectors as usize {
        if world.level.sectors[i].tag != tag {
            continue;
        }

        // The walk reads one entry at a time: the teleport spawns fog.
        let mut position = 0;
        while let Some(thinker) = thinker_at(world, position) {
            position += 1;
            // Not a mobj
            let ThinkerId::Mobj(marker_id) = thinker else {
                continue;
            };
            if thinker.kind(world) != ThinkerKind::Mobj {
                continue;
            }

            let (marker_kind, marker_subsector) = {
                let mobjs = &world.mobjs;
                (mobjs[marker_id].kind, mobjs[marker_id].subsector)
            };

            // Not a teleportman
            if marker_kind != MT_TELEPORTMAN {
                continue;
            }

            let sector = world.level.subsectors[marker_subsector].sector;
            // Wrong sector
            if sector.index() != i {
                continue;
            }

            let (oldx, oldy, oldz, marker_x, marker_y) = {
                let mobjs = &world.mobjs;
                let thing = &mobjs[thing_id];
                let marker = &mobjs[marker_id];
                (thing.x, thing.y, thing.z, marker.x, marker.y)
            };

            // SAFETY: the caller guarantees `thing_id` is linked into the map.
            if unsafe { P_TeleportMove(world, thing_id, marker_x, marker_y) } == 0 {
                return 0;
            }

            let (thing_z, thing_player) = {
                let mobjs = &mut world.mobjs;
                let thing = &mut mobjs[thing_id];
                // Final Doom quirk: don't set z
                if gameversion() != EXE_FINAL {
                    thing.z = thing.floorz;
                }
                (thing.z, thing.player)
            };

            if let Some(player) = thing_player {
                let player = &mut world.players[player];
                player.viewz = thing_z + player.viewheight;
            }

            // Spawn teleport fog at source
            // SAFETY (both fogs): a level is loaded, and the fog is a map
            // object of it that stays until its sound is over.
            unsafe {
                let fog = P_SpawnMobj(world, oldx, oldy, oldz, MT_TFOG);
                S_StartSound(world, fog, Sfx::Telept as i32);
            }

            // Spawn teleport fog at destination
            let (marker_x, marker_y, marker_angle, thing_z) = {
                let mobjs = &world.mobjs;
                let marker = &mobjs[marker_id];
                (marker.x, marker.y, marker.angle, mobjs[thing_id].z)
            };
            let an = (marker_angle >> ANGLETOFINESHIFT) as usize;
            // `finecosine[an]`: the cosine table is the sine table a quarter
            // turn on.
            unsafe {
                let fog = P_SpawnMobj(
                    world,
                    marker_x + 20 * finesine[an + FINEANGLES / 4],
                    marker_y + 20 * finesine[an],
                    thing_z,
                    MT_TFOG,
                );
                S_StartSound(world, fog, Sfx::Telept as i32);
            }

            let mobjs = &mut world.mobjs;
            let marker_angle = mobjs[marker_id].angle;
            let thing = &mut mobjs[thing_id];
            // Don't move for a bit
            if thing.player.is_some() {
                thing.reactiontime = 18;
            }

            thing.angle = marker_angle;
            thing.momx = 0;
            thing.momy = 0;
            thing.momz = 0;
            return 1;
        }
    }
    0
}
