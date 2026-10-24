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
    math::tables::{finecosine, finesine},
    play::{
        map::P_TeleportMove,
        mobj::P_SpawnMobj,
        setup::LEVEL,
        tick::{ThinkerId, ThinkerKind, thinkers},
    },
    sound::{S_StartSound, info::Sfx},
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
/// `thing_id` must name a map object of the level, and a level must be
/// loaded.
pub unsafe fn EV_Teleport(line: &line_t, side: i32, thing_id: MobjId) -> i32 {
    unsafe {
        let thing = thing_id.ptr();
        // Don't teleport missiles
        if (*thing).flags & MF_MISSILE != 0 {
            return 0;
        }

        // Don't teleport if hit back of line
        if side == 1 {
            return 0;
        }

        let tag = line.tag;
        let numsectors = LEVEL.borrow().numsectors;

        for i in 0..numsectors as usize {
            if LEVEL.borrow().sectors[i].tag != tag {
                continue;
            }

            for thinker in thinkers() {
                // Not a mobj
                let ThinkerId::Mobj(m) = thinker else {
                    continue;
                };
                if thinker.kind() != ThinkerKind::Mobj {
                    continue;
                }

                let m = m.ptr();

                // Not a teleportman
                if (*m).kind != MT_TELEPORTMAN {
                    continue;
                }

                let sector = LEVEL.borrow().subsectors[(*m).subsector].sector;
                // Wrong sector
                if sector.index() != i {
                    continue;
                }

                let oldx = (*thing).x;
                let oldy = (*thing).y;
                let oldz = (*thing).z;

                if P_TeleportMove(thing_id, (*m).x, (*m).y) == 0 {
                    return 0;
                }

                // Final Doom quirk: don't set z
                if gameversion() != EXE_FINAL {
                    (*thing).z = (*thing).floorz;
                }

                if let Some(player) = (*thing).player {
                    let thing_z = (*thing).z;
                    player.write(|player| player.viewz = thing_z + player.viewheight);
                }

                // Spawn teleport fog at source
                let fog = P_SpawnMobj(oldx, oldy, oldz, MT_TFOG);
                S_StartSound(fog, Sfx::Telept as i32);

                // Spawn teleport fog at destination
                let an = ((*m).angle >> ANGLETOFINESHIFT) as usize;
                let fog = P_SpawnMobj(
                    (*m).x + 20 * *finecosine.0.add(an),
                    (*m).y + 20 * finesine[an],
                    (*thing).z,
                    MT_TFOG,
                );
                S_StartSound(fog, Sfx::Telept as i32);

                // Don't move for a bit
                if (*thing).player.is_some() {
                    (*thing).reactiontime = 18;
                }

                (*thing).angle = (*m).angle;
                (*thing).momx = 0;
                (*thing).momy = 0;
                (*thing).momz = 0;
                return 1;
            }
        }
    }
    0
}
