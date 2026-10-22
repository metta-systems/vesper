//! Teleportation action special.
//!
//! Rust port of `vendor/doomgeneric/p_telept.c`.  Handles the `EV_Teleport`
//! linedef special: locating the destination `MT_TELEPORTMAN` marker, moving
//! the triggering thing to that position, and spawning teleport fog at both
//! the source and destination.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::c_void;
use std::os::raw::c_int;

use crate::doom::d_player::PlayerT;
use crate::doom::info::*;
use crate::doom::p_tick::thinker_t;
use crate::doom::tables::ANGLETOFINESHIFT;

/// A sub-sector: the smallest convex region of the BSP tree.
///
/// Each sub-sector belongs to exactly one [`sector_t`] and holds a range of
/// segs.  The layout matches the C `subsector_t` struct so that pointer casts
/// from the C side are valid.
///
/// Re-exported with the same layout as `p_lights.rs` so pointer casts work
/// across modules.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct subsector_t {
    /// The sector this sub-sector belongs to.
    pub sector: *mut sector_t,
    /// Number of segs in this sub-sector.
    pub numlines: i16,
    /// Index of the first seg in the global seg array.
    pub firstline: i16,
}

/// A map sector: a convex region with a single floor and ceiling height.
///
/// Layout must match the C `sector_t` struct exactly; the padding bytes
/// (`_pad0`) preserve alignment without introducing Rust-side named fields
/// for C-internal slots.  Field offsets are verified by the test suite.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct sector_t {
    /// Floor height in fixed-point units.
    pub floorheight: c_int,
    /// Ceiling height in fixed-point units.
    pub ceilingheight: c_int,
    /// Flat texture index for the floor.
    pub floorpic: i16,
    /// Flat texture index for the ceiling.
    pub ceilingpic: i16,
    /// Ambient light level (0–255).
    pub lightlevel: i16,
    /// Sector special type (damage, secret, etc.).
    pub special: i16,
    /// Linedef tag used to link this sector with matching linedefs.
    pub tag: i16,
    _pad0: [u8; 2],
    /// Sound traversal counter, used to avoid duplicate sound propagation.
    pub soundtraversed: c_int,
    /// Last thing to make a sound in this sector (for monster alerting).
    pub soundtarget: *mut c_void,
    /// Bounding box of the sector in map units `[top, bottom, left, right]`.
    pub blockbox: [c_int; 4],
    /// Origin point for sounds emitted by this sector (opaque 40-byte blob
    /// matching `mobj_t::soundorg` in C).
    pub soundorg: [u8; 40],
    /// Incremented each time the sector is visited during a BFS/DFS.
    pub validcount: c_int,
    /// Linked list of things currently in this sector.
    pub thinglist: *mut c_void,
    /// Pointer to the active special thinker for this sector (e.g. a moving
    /// ceiling or platform), or null if none is active.
    pub specialdata: *mut c_void,
    /// Number of linedefs bounding this sector.
    pub linecount: c_int,
    /// Pointer to the array of linedef pointers for this sector.
    pub lines: *mut *mut c_void,
}

/// Packed map spawn-point record as stored in the WAD THINGS lump.
///
/// Uses `#[repr(C, packed)]` to match the 10-byte on-disk layout.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct mapthing_t {
    /// Spawn X position in map units.
    pub x: i16,
    /// Spawn Y position in map units.
    pub y: i16,
    /// Facing angle in degrees (0–359, clockwise from East).
    pub angle: i16,
    /// Doom thing type number.
    pub r#type: i16,
    /// Spawn flags (skill levels, deaf, multiplayer, etc.).
    pub options: i16,
}

/// Opaque stand-in for the C `player_s` struct; only used via raw pointer.
pub enum player_s {}
/// Opaque stand-in for the C `state_t` (animation frame) struct.
pub enum state_t {}
/// Opaque stand-in for the C `mobjinfo_t` (thing type info) struct.
pub enum mobjinfo_t {}

/// A map object (thing) that lives in the world and can think each tic.
///
/// The `thinker` field must be at offset 0 so that a `*mut mobj_t` can be
/// safely cast to `*mut thinker_t` and vice-versa.  The overall size (224
/// bytes on 64-bit) and all field offsets are verified by the test suite.
///
/// Padding bytes (`_pad0`–`_pad4`) fill gaps that C compilers insert for
/// alignment; they have no semantic meaning in this port.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct mobj_t {
    /// Thinker header — must be the very first field (offset 0).
    pub thinker: thinker_t,
    /// Map X position in fixed-point units.
    pub x: c_int,
    /// Map Y position in fixed-point units.
    pub y: c_int,
    /// Map Z position (height above the floor) in fixed-point units.
    pub z: c_int,
    _pad0: [u8; 4],
    /// Next thing in this sector's sprite-order linked list.
    pub snext: *mut mobj_t,
    /// Previous thing in this sector's sprite-order linked list.
    pub sprev: *mut mobj_t,
    /// Facing angle in binary radians (BAMs); 0 = East.
    pub angle: u32,
    /// Current sprite number.
    pub sprite: c_int,
    /// Current animation frame index.
    pub frame: c_int,
    _pad1: [u8; 4],
    /// Next thing in the blockmap block's linked list.
    pub bnext: *mut mobj_t,
    /// Previous thing in the blockmap block's linked list.
    pub bprev: *mut mobj_t,
    /// The sub-sector this thing currently occupies.
    pub subsector: *mut subsector_t,
    /// Floor height at the thing's current position.
    pub floorz: c_int,
    /// Ceiling height at the thing's current position.
    pub ceilingz: c_int,
    /// Collision radius in fixed-point units.
    pub radius: c_int,
    /// Collision height in fixed-point units.
    pub height: c_int,
    /// X momentum applied each tic.
    pub momx: c_int,
    /// Y momentum applied each tic.
    pub momy: c_int,
    /// Z momentum applied each tic.
    pub momz: c_int,
    /// Used to avoid processing the same thing twice in a single traversal.
    pub validcount: c_int,
    /// Thing type index into `mobjinfo` table.
    pub mobjtype: c_int,
    _pad2: [u8; 4],
    /// Pointer to the thing's type info record.
    pub info: *mut mobjinfo_t,
    /// Remaining tics in the current animation frame.
    pub tics: c_int,
    _pad3: [u8; 4],
    /// Pointer to the current animation frame state.
    pub state: *mut state_t,
    /// Bit-field of `MF_*` flags controlling collision, rendering, AI, etc.
    pub flags: c_int,
    /// Current hit points.
    pub health: c_int,
    /// Current movement direction (0–7 compass directions, or `DI_NODIR`).
    pub movedir: c_int,
    /// Tics remaining before the thing attempts a new movement decision.
    pub movecount: c_int,
    /// Primary AI target (usually the last thing to attack this one).
    pub target: *mut mobj_t,
    /// Tics the thing must wait before it can attack again.
    pub reactiontime: c_int,
    /// Decremented each tic while pursuing a target; reset on target change.
    pub threshold: c_int,
    /// Non-null if this thing is a player; points to the `PlayerT` record.
    pub player: *mut player_s,
    /// Index used to cycle through potential targets during `P_LookForPlayers`.
    pub lastlook: c_int,
    /// The WAD spawn point from which this thing was created.
    pub spawnpoint: mapthing_t,
    _pad4: [u8; 2],
    /// Tracer target used by homing missiles (cyberdemon rockets, etc.).
    pub tracer: *mut mobj_t,
}

/// Linedef descriptor used by `EV_Teleport` to read the tag and trigger side.
///
/// Only the fields accessed by teleportation logic are fully documented here;
/// the layout matches the C `line_t` struct and is shared with `p_lights.rs`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct line_t {
    /// First vertex of the line (opaque pointer).
    pub v1: *mut c_void,
    /// Second vertex of the line (opaque pointer).
    pub v2: *mut c_void,
    /// Delta X between the two vertices (used for slope/normal calculation).
    pub dx: c_int,
    /// Delta Y between the two vertices.
    pub dy: c_int,
    /// Linedef flags (blocking, two-sided, upper/lower unpegged, etc.).
    pub flags: i16,
    /// Linedef special action number.
    pub special: i16,
    /// Tag linking this linedef to tagged sectors.
    pub tag: i16,
    /// Indices into the global `sides` array: `[front, back]`.
    pub sidenum: [i16; 2],
    /// Bounding box `[top, bottom, left, right]` in map units.
    pub bbox: [c_int; 4],
    /// Slope type (horizontal, vertical, positive, negative).
    pub slopetype: c_int,
    /// Pointer to the front (right) sector.
    pub frontsector: *mut sector_t,
    /// Pointer to the back (left) sector, or null for a one-sided line.
    pub backsector: *mut sector_t,
    /// Used to avoid processing the same line twice in a BSP traversal.
    pub validcount: c_int,
    /// Active special thinker attached to this line, or null.
    pub specialdata: *mut c_void,
}

/// Doom version constant for the first Final Doom executable.
///
/// `EV_Teleport` uses this to preserve a known quirk: the original Final Doom
/// binary does not set `thing->z` to `floorz` after teleporting.
const EXE_FINAL: c_int = 7;

use crate::doom::doomstat::gameversion;
use crate::doom::p_map::P_TeleportMove;
use crate::doom::p_mobj::{P_MobjThinker, P_SpawnMobj};
use crate::doom::p_setup::{numsectors, sectors};
use crate::doom::p_tick::thinkercap;
use crate::doom::s_sound::S_StartSound;
use crate::doom::sounds::Sfx;
use crate::doom::tables::{finecosine, finesine};

// Type alias for cross-module pointer cast (#[repr(C)] identical layout).
type CffiMobj = crate::doom::c_ffi::mobj_t;
type CffiSector = crate::doom::c_ffi::sector_t;
// We already have PlayerT in d_player.rs but can't use it here
// because the C player_s is different. Use pointer casts.

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
/// `line` and `thing` must be valid, non-null pointers for the duration of
/// the call.  The global arrays `sectors` and `thinkercap` must be initialised
/// (i.e. a level must be loaded).
#[no_mangle]
pub extern "C" fn EV_Teleport(line: *mut line_t, side: c_int, thing: *mut mobj_t) -> c_int {
    unsafe {
        // Don't teleport missiles
        if (*thing).flags & MF_MISSILE != 0 {
            return 0;
        }

        // Don't teleport if hit back of line
        if side == 1 {
            return 0;
        }

        let tag = (*line).tag;

        for i in 0..numsectors as usize {
            if (*sectors.add(i)).tag != tag {
                continue;
            }

            let mut thinker = thinkercap.next;
            while !std::ptr::eq(thinker, &raw const thinkercap) {
                // Not a mobj
                if (*thinker).function.acp1
                    != Some(core::mem::transmute::<
                        unsafe extern "C" fn(*mut mobj_t),
                        unsafe extern "C" fn(*mut c_void),
                    >(P_MobjThinker))
                {
                    thinker = (*thinker).next;
                    continue;
                }

                let m = thinker as *mut mobj_t;

                // Not a teleportman
                if (*m).mobjtype != MT_TELEPORTMAN {
                    thinker = (*thinker).next;
                    continue;
                }

                let sector = (*(*m).subsector).sector;
                // Wrong sector
                if sector.offset_from(sectors as *mut sector_t) != i as isize {
                    thinker = (*thinker).next;
                    continue;
                }

                let oldx = (*thing).x;
                let oldy = (*thing).y;
                let oldz = (*thing).z;

                if P_TeleportMove(thing as *mut CffiMobj, (*m).x, (*m).y) == 0 {
                    return 0;
                }

                // Final Doom quirk: don't set z
                if gameversion != EXE_FINAL {
                    (*thing).z = (*thing).floorz;
                }

                if !(*thing).player.is_null() {
                    let player = (*thing).player as *mut PlayerT;
                    (*player).viewz = (*thing).z + (*player).viewheight;
                }

                // Spawn teleport fog at source
                let fog = P_SpawnMobj(oldx, oldy, oldz, MT_TFOG);
                S_StartSound(fog as *mut c_void, Sfx::Telept as c_int);

                // Spawn teleport fog at destination
                let an = ((*m).angle >> ANGLETOFINESHIFT) as usize;
                let fog = P_SpawnMobj(
                    (*m).x + 20 * *finecosine.0.add(an),
                    (*m).y + 20 * finesine[an],
                    (*thing).z,
                    MT_TFOG,
                );
                S_StartSound(fog as *mut c_void, Sfx::Telept as c_int);

                // Don't move for a bit
                if !(*thing).player.is_null() {
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

/// Anchor function referenced from `doomgeneric_Create` to ensure
/// `EV_Teleport` survives link-time dead-code elimination.
#[no_mangle]
pub extern "C" fn P_Telept_Link_Anchor() {
    let _ = EV_Teleport as *const () as usize;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobj_t_size_is_224() {
        assert_eq!(std::mem::size_of::<mobj_t>(), 224);
    }

    #[test]
    fn mobj_t_field_offsets() {
        assert_eq!(std::mem::offset_of!(mobj_t, thinker), 0);
        assert_eq!(std::mem::offset_of!(mobj_t, x), 24);
        assert_eq!(std::mem::offset_of!(mobj_t, subsector), 88);
        assert_eq!(std::mem::offset_of!(mobj_t, floorz), 96);
        assert_eq!(std::mem::offset_of!(mobj_t, mobjtype), 128);
        assert_eq!(std::mem::offset_of!(mobj_t, flags), 160);
        assert_eq!(std::mem::offset_of!(mobj_t, player), 192);
        assert_eq!(std::mem::offset_of!(mobj_t, reactiontime), 184);
        assert_eq!(std::mem::offset_of!(mobj_t, tracer), 216);
    }

    #[test]
    fn subsector_t_size() {
        assert_eq!(std::mem::size_of::<subsector_t>(), 16);
    }

    #[test]
    fn sector_t_layout() {
        assert_eq!(std::mem::size_of::<sector_t>(), 128);
        assert_eq!(std::mem::offset_of!(sector_t, floorheight), 0);
        assert_eq!(std::mem::offset_of!(sector_t, ceilingheight), 4);
        assert_eq!(std::mem::offset_of!(sector_t, floorpic), 8);
        assert_eq!(std::mem::offset_of!(sector_t, ceilingpic), 10);
        assert_eq!(std::mem::offset_of!(sector_t, lightlevel), 12);
        assert_eq!(std::mem::offset_of!(sector_t, special), 14);
        assert_eq!(std::mem::offset_of!(sector_t, tag), 16);
        assert_eq!(std::mem::offset_of!(sector_t, soundtraversed), 20);
        assert_eq!(std::mem::offset_of!(sector_t, soundtarget), 24);
        assert_eq!(std::mem::offset_of!(sector_t, blockbox), 32);
        assert_eq!(std::mem::offset_of!(sector_t, soundorg), 48);
        assert_eq!(std::mem::offset_of!(sector_t, validcount), 88);
        assert_eq!(std::mem::offset_of!(sector_t, thinglist), 96);
        assert_eq!(std::mem::offset_of!(sector_t, specialdata), 104);
        assert_eq!(std::mem::offset_of!(sector_t, linecount), 112);
        assert_eq!(std::mem::offset_of!(sector_t, lines), 120);
    }
}
