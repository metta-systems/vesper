//! Sector lighting effects: fire flicker, light flash, strobe, and glow.
//!
//! Rust port of `vendor/doomgeneric/p_lights.c`. Each effect is driven by a
//! thinker that updates a sector's `lightlevel` every game tic.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ffi::c_void;
use std::os::raw::c_int;

use crate::doom::c_ffi as cffi;
use crate::doom::m_random::P_Random;
use crate::doom::p_setup::{numsectors, sectors};
use crate::doom::p_spec::{getNextSector, P_FindMinSurroundingLight, P_FindSectorFromLineTag};
use crate::doom::p_tick::{thinker_t, P_AddThinker};
use crate::doom::z_zone::{Z_Malloc, PU_LEVSPEC};

/// Light change applied each step of the glow oscillation (light units per tic).
const GLOWSPEED: c_int = 8;

/// Duration of the bright phase for strobe lights, in tics.
const STROBEBRIGHT: c_int = 5;

/// Dark-phase duration for fast strobe lights, in tics (`FASTDARK` in C).
const FASTDARK: c_int = 15;

/// Dark-phase duration for slow strobe lights, in tics (`SLOWDARK` in C).
const SLOWDARK: c_int = 35;

//
// We only need fields up to `specialdata`.  Verified against C layout
// on x86_64 Linux via layout_probe:
//   sector_t size=128, lightlevel@12, special@14, specialdata@104

/// Partial mirror of `sector_t` from `p_local.h`, containing only the fields
/// required by the lighting subsystem.
///
/// The layout is verified by the `sector_t_layout_matches_c` test; padding
/// bytes ensure the Rust struct matches the C ABI on x86_64 Linux.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct sector_t {
    /// Height of the sector floor in fixed-point units.
    pub floorheight: c_int,
    /// Height of the sector ceiling in fixed-point units.
    pub ceilingheight: c_int,
    /// Flat texture index used for the floor.
    pub floorpic: i16,
    /// Flat texture index used for the ceiling.
    pub ceilingpic: i16,
    /// Current ambient light level (0 = dark, 255 = fully bright).
    pub lightlevel: i16,
    /// Sector special type (e.g. damaging floor, secret).
    pub special: i16,
    /// Linedef tag shared with triggers that activate this sector.
    pub tag: i16,
    _pad0: [u8; 2],
    /// Number of sound propagation traversals (BSP sound travel counter).
    pub soundtraversed: c_int,
    /// The map object currently producing sound in this sector, if any.
    pub soundtarget: *mut c_void,
    /// Axis-aligned bounding box of the sector in map coordinates.
    pub blockbox: [c_int; 4],
    /// Degenerate mobj used as the origin point for sector sounds (opaque).
    pub soundorg: [u8; 40], // degenmobj_t (opaque)
    /// Validation counter used during BSP traversal to avoid re-processing.
    pub validcount: c_int,
    /// Linked list of map objects currently standing in this sector.
    pub thinglist: *mut c_void,
    /// Pointer to the active thinker (special effect) operating on this sector.
    pub specialdata: *mut c_void,
    /// Number of linedefs that border this sector.
    pub linecount: c_int,
    /// Pointer to the array of linedefs that border this sector.
    pub lines: *mut *mut line_t,
}

/// Thinker state for the fire-flicker lighting effect.
///
/// The sector's light level randomly drops by a multiple of 16 every 4 tics,
/// simulating a flickering fire (sector special 17 in Doom).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct fireflicker_t {
    /// Embedded thinker header; must be the first field.
    pub thinker: thinker_t,
    /// The sector whose light level is being animated.
    pub sector: *mut sector_t,
    /// Tics remaining until the next light change.
    pub count: c_int,
    /// Sector's original (maximum) light level.
    pub maxlight: c_int,
    /// Minimum light level the flicker will not go below.
    pub minlight: c_int,
    _pad: [u8; 4],
}

/// Thinker state for the random light-flash effect.
///
/// The sector alternates between `maxlight` and `minlight` at random intervals
/// bounded by `maxtime` and `mintime` (sector special 1 in Doom).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct lightflash_t {
    /// Embedded thinker header; must be the first field.
    pub thinker: thinker_t,
    /// The sector whose light level is being animated.
    pub sector: *mut sector_t,
    /// Tics remaining until the next light change.
    pub count: c_int,
    /// Bright phase light level.
    pub maxlight: c_int,
    /// Dark phase light level.
    pub minlight: c_int,
    /// Bitmask used to generate the random bright-phase duration (`count & maxtime`).
    pub maxtime: c_int,
    /// Bitmask used to generate the random dark-phase duration (`count & mintime`).
    pub mintime: c_int,
}

/// Thinker state for the strobe-flash lighting effect.
///
/// The sector alternates between `maxlight` (bright) and `minlight` (dark)
/// with fixed durations of `brighttime` and `darktime` tics respectively
/// (sector specials 3 and 13 in Doom, among others).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct strobe_t {
    /// Embedded thinker header; must be the first field.
    pub thinker: thinker_t,
    /// The sector whose light level is being animated.
    pub sector: *mut sector_t,
    /// Tics remaining until the next phase change.
    pub count: c_int,
    /// Dark-phase light level.
    pub minlight: c_int,
    /// Bright-phase light level.
    pub maxlight: c_int,
    /// Duration of the dark phase in tics.
    pub darktime: c_int,
    /// Duration of the bright phase in tics (always `STROBEBRIGHT` = 5).
    pub brighttime: c_int,
}

/// Thinker state for the smoothly oscillating glow effect.
///
/// The sector's light level rises and falls continuously between `minlight`
/// and `maxlight` by `GLOWSPEED` units per tic (sector special 8 in Doom).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct glow_t {
    /// Embedded thinker header; must be the first field.
    pub thinker: thinker_t,
    /// The sector whose light level is being animated.
    pub sector: *mut sector_t,
    /// Lower bound of the oscillation (minimum light level).
    pub minlight: c_int,
    /// Upper bound of the oscillation (maximum / original light level).
    pub maxlight: c_int,
    /// Direction of travel: `1` = increasing, `-1` = decreasing.
    pub direction: c_int,
    _pad: [u8; 4],
}

/// Partial mirror of `line_t` from `p_local.h`.
///
/// Only the fields required by the lighting subsystem are included; remaining
/// fields are accessed through the C side.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct line_t {
    /// First vertex of the linedef (opaque pointer).
    pub v1: *mut c_void,
    /// Second vertex of the linedef (opaque pointer).
    pub v2: *mut c_void,
    /// Horizontal component of the line direction vector (dx = v2.x - v1.x).
    pub dx: c_int,
    /// Vertical component of the line direction vector (dy = v2.y - v1.y).
    pub dy: c_int,
    /// Linedef flags (e.g. two-sided, impassable).
    pub flags: i16,
    /// Linedef special type number (triggers an action when activated).
    pub special: i16,
    /// Tag linking this linedef to sectors with the matching tag.
    pub tag: i16,
    /// Side number indices: `[0]` = front, `[1]` = back (-1 if one-sided).
    pub sidenum: [i16; 2],
    /// Axis-aligned bounding box of the linedef in map coordinates.
    pub bbox: [c_int; 4],
    /// Slope type classification for quick rejection tests.
    pub slopetype: c_int,
    /// Sector on the front side of this linedef.
    pub frontsector: *mut sector_t,
    /// Sector on the back side, or null for one-sided linedefs.
    pub backsector: *mut sector_t,
    /// Validation counter used during traversal to avoid re-processing.
    pub validcount: c_int,
    /// Pointer to an active special effect thinker on this linedef, if any.
    pub specialdata: *mut c_void,
}

/// Per-tic update for the fire-flicker effect.
///
/// Decrements the countdown; when it reaches zero, randomises the sector's
/// light level and resets the 4-tic counter.
///
/// # Safety
///
/// `flick` must be a valid, aligned, non-null pointer to a `fireflicker_t`
/// whose embedded `sector` pointer is also valid for the current map.
/// Called exclusively by the thinker dispatcher from `P_RunThinkers`.
#[no_mangle]
pub unsafe extern "C" fn T_FireFlicker(flick: *mut fireflicker_t) {
    (*flick).count -= 1;
    if (*flick).count != 0 {
        return;
    }

    let amount = (P_Random() & 3) * 16;
    let sec = &mut *(*flick).sector;

    if (sec.lightlevel as c_int) - amount < (*flick).minlight {
        sec.lightlevel = (*flick).minlight as i16;
    } else {
        sec.lightlevel = ((*flick).maxlight - amount) as i16;
    }

    (*flick).count = 4;
}

/// Allocate and initialise a fire-flicker thinker for `sector`.
///
/// Clears the sector special, finds the minimum surrounding light level
/// (used as the lower flicker bound + 16), and registers the thinker.
#[no_mangle]
pub extern "C" fn P_SpawnFireFlicker(sector: *mut sector_t) {
    unsafe {
        (*sector).special = 0;

        let flick = Z_Malloc(
            std::mem::size_of::<fireflicker_t>() as c_int,
            PU_LEVSPEC,
            std::ptr::null_mut(),
        ) as *mut fireflicker_t;

        P_AddThinker(&mut (*flick).thinker);

        (*flick).thinker.function.acp1 = Some(core::mem::transmute::<
            unsafe extern "C" fn(*mut fireflicker_t),
            unsafe extern "C" fn(*mut c_void),
        >(T_FireFlicker));
        (*flick).sector = sector;
        (*flick).maxlight = (*sector).lightlevel as c_int;
        (*flick).minlight =
            P_FindMinSurroundingLight(sector as *mut cffi::sector_t, (*sector).lightlevel as c_int)
                + 16;
        (*flick).count = 4;
    }
}

/// Per-tic update for the random light-flash effect.
///
/// Decrements the countdown; when it reaches zero, toggles the sector's
/// light between `maxlight` and `minlight`, picking a new random duration.
///
/// # Safety
///
/// `flash` must be a valid, aligned, non-null pointer to a `lightflash_t`
/// whose embedded `sector` pointer is also valid for the current map.
/// Called exclusively by the thinker dispatcher from `P_RunThinkers`.
#[no_mangle]
pub unsafe extern "C" fn T_LightFlash(flash: *mut lightflash_t) {
    (*flash).count -= 1;
    if (*flash).count != 0 {
        return;
    }

    let sec = &mut *(*flash).sector;

    if sec.lightlevel as c_int == (*flash).maxlight {
        sec.lightlevel = (*flash).minlight as i16;
        (*flash).count = (P_Random() & (*flash).mintime) + 1;
    } else {
        sec.lightlevel = (*flash).maxlight as i16;
        (*flash).count = (P_Random() & (*flash).maxtime) + 1;
    }
}

/// Allocate and initialise a random light-flash thinker for `sector`.
///
/// Sets `maxtime = 64` and `mintime = 7`, giving the flash a long bright phase
/// and a short dark phase. The initial countdown is randomised.
#[no_mangle]
pub extern "C" fn P_SpawnLightFlash(sector: *mut sector_t) {
    unsafe {
        (*sector).special = 0;

        let flash = Z_Malloc(
            std::mem::size_of::<lightflash_t>() as c_int,
            PU_LEVSPEC,
            std::ptr::null_mut(),
        ) as *mut lightflash_t;

        P_AddThinker(&mut (*flash).thinker);

        (*flash).thinker.function.acp1 = Some(core::mem::transmute::<
            unsafe extern "C" fn(*mut lightflash_t),
            unsafe extern "C" fn(*mut c_void),
        >(T_LightFlash));
        (*flash).sector = sector;
        (*flash).maxlight = (*sector).lightlevel as c_int;
        (*flash).minlight =
            P_FindMinSurroundingLight(sector as *mut cffi::sector_t, (*sector).lightlevel as c_int);
        (*flash).maxtime = 64;
        (*flash).mintime = 7;
        (*flash).count = (P_Random() & (*flash).maxtime) + 1;
    }
}

/// Per-tic update for the strobe-flash effect.
///
/// Decrements the countdown; when it reaches zero, switches the sector's
/// light between `minlight` (dark phase, `darktime` tics) and `maxlight`
/// (bright phase, `brighttime` tics).
///
/// # Safety
///
/// `flash` must be a valid, aligned, non-null pointer to a `strobe_t`
/// whose embedded `sector` pointer is also valid for the current map.
/// Called exclusively by the thinker dispatcher from `P_RunThinkers`.
#[no_mangle]
pub unsafe extern "C" fn T_StrobeFlash(flash: *mut strobe_t) {
    (*flash).count -= 1;
    if (*flash).count != 0 {
        return;
    }

    let sec = &mut *(*flash).sector;

    if sec.lightlevel as c_int == (*flash).minlight {
        sec.lightlevel = (*flash).maxlight as i16;
        (*flash).count = (*flash).brighttime;
    } else {
        sec.lightlevel = (*flash).minlight as i16;
        (*flash).count = (*flash).darktime;
    }
}

/// Allocate and initialise a strobe-flash thinker for `sector`.
///
/// `fastOrSlow` sets `darktime` (use `FASTDARK` or `SLOWDARK`).
/// `inSync = 1` starts the flash immediately; `inSync = 0` gives a random offset.
/// If `minlight == maxlight`, `minlight` is forced to 0.
#[no_mangle]
pub extern "C" fn P_SpawnStrobeFlash(sector: *mut sector_t, fastOrSlow: c_int, inSync: c_int) {
    unsafe {
        let flash = Z_Malloc(
            std::mem::size_of::<strobe_t>() as c_int,
            PU_LEVSPEC,
            std::ptr::null_mut(),
        ) as *mut strobe_t;

        P_AddThinker(&mut (*flash).thinker);

        (*flash).sector = sector;
        (*flash).darktime = fastOrSlow;
        (*flash).brighttime = STROBEBRIGHT;
        (*flash).thinker.function.acp1 = Some(core::mem::transmute::<
            unsafe extern "C" fn(*mut strobe_t),
            unsafe extern "C" fn(*mut c_void),
        >(T_StrobeFlash));
        (*flash).maxlight = (*sector).lightlevel as c_int;
        (*flash).minlight =
            P_FindMinSurroundingLight(sector as *mut cffi::sector_t, (*sector).lightlevel as c_int);

        if (*flash).minlight == (*flash).maxlight {
            (*flash).minlight = 0;
        }

        (*sector).special = 0;

        if inSync == 0 {
            (*flash).count = (P_Random() & 7) + 1;
        } else {
            (*flash).count = 1;
        }
    }
}

/// Linedef-triggered event: start slow-strobe lighting on all tagged sectors.
///
/// Iterates over sectors whose tag matches `line`'s tag. Sectors that already
/// have an active special effect are skipped.
#[no_mangle]
pub extern "C" fn EV_StartLightStrobing(line: *mut line_t) {
    unsafe {
        let mut secnum: c_int = -1;
        loop {
            secnum = P_FindSectorFromLineTag(line as *mut cffi::line_t, secnum);
            if secnum < 0 {
                break;
            }
            let sec = sectors.add(secnum as usize);
            if !(*sec).specialdata.is_null() {
                continue;
            }
            P_SpawnStrobeFlash(sec as *mut sector_t, SLOWDARK, 0);
        }
    }
}

/// Linedef-triggered event: set all tagged sectors to the darkest neighboring level.
///
/// For each sector whose tag matches `line`'s tag, scans adjacent sectors and
/// sets the light level to the minimum found among all neighbors.
#[no_mangle]
pub extern "C" fn EV_TurnTagLightsOff(line: *mut line_t) {
    unsafe {
        for j in 0..numsectors as usize {
            let sec = sectors.add(j);
            if (*sec).tag != (*line).tag {
                continue;
            }

            let mut min = (*sec).lightlevel as c_int;
            for i in 0..(*sec).linecount as usize {
                let templine = *(*sec).lines.add(i);
                let tsec = getNextSector(templine as *mut cffi::line_t, sec);
                if tsec.is_null() {
                    continue;
                }
                let tl = (*tsec).lightlevel as c_int;
                if tl < min {
                    min = tl;
                }
            }
            (*sec).lightlevel = min as i16;
        }
    }
}

/// Linedef-triggered event: set all tagged sectors to `bright` light level.
///
/// If `bright == 0`, the function searches adjacent sectors and uses the
/// highest light level found among them instead.
#[no_mangle]
pub extern "C" fn EV_LightTurnOn(line: *mut line_t, bright: c_int) {
    unsafe {
        let mut bright = bright;
        for i in 0..numsectors as usize {
            let sec = sectors.add(i);
            if (*sec).tag != (*line).tag {
                continue;
            }

            if bright == 0 {
                for j in 0..(*sec).linecount as usize {
                    let templine = *(*sec).lines.add(j);
                    let temp = getNextSector(templine as *mut cffi::line_t, sec);
                    if temp.is_null() {
                        continue;
                    }
                    let tl = (*temp).lightlevel as c_int;
                    if tl > bright {
                        bright = tl;
                    }
                }
            }
            (*sec).lightlevel = bright as i16;
        }
    }
}

/// Per-tic update for the smooth glow oscillation effect.
///
/// Adjusts `lightlevel` by `GLOWSPEED` each tic, bouncing at `minlight` and
/// `maxlight` by reversing `direction`.
///
/// # Safety
///
/// `g` must be a valid, aligned, non-null pointer to a `glow_t`
/// whose embedded `sector` pointer is also valid for the current map.
/// Called exclusively by the thinker dispatcher from `P_RunThinkers`.
#[no_mangle]
pub unsafe extern "C" fn T_Glow(g: *mut glow_t) {
    let sec = &mut *(*g).sector;
    match (*g).direction {
        -1 => {
            sec.lightlevel -= GLOWSPEED as i16;
            if (sec.lightlevel as c_int) <= (*g).minlight {
                sec.lightlevel += GLOWSPEED as i16;
                (*g).direction = 1;
            }
        }
        1 => {
            sec.lightlevel += GLOWSPEED as i16;
            if (sec.lightlevel as c_int) >= (*g).maxlight {
                sec.lightlevel -= GLOWSPEED as i16;
                (*g).direction = -1;
            }
        }
        _ => {}
    }
}

/// Allocate and initialise a glow thinker for `sector`.
///
/// Sets `maxlight` to the sector's current light level and `minlight` to the
/// lowest light level in adjacent sectors. The glow starts moving downward
/// (`direction = -1`).
#[no_mangle]
pub extern "C" fn P_SpawnGlowingLight(sector: *mut sector_t) {
    unsafe {
        let g = Z_Malloc(
            std::mem::size_of::<glow_t>() as c_int,
            PU_LEVSPEC,
            std::ptr::null_mut(),
        ) as *mut glow_t;

        P_AddThinker(&mut (*g).thinker);

        (*g).sector = sector;
        (*g).minlight =
            P_FindMinSurroundingLight(sector as *mut cffi::sector_t, (*sector).lightlevel as c_int);
        (*g).maxlight = (*sector).lightlevel as c_int;
        (*g).thinker.function.acp1 = Some(core::mem::transmute::<
            unsafe extern "C" fn(*mut glow_t),
            unsafe extern "C" fn(*mut c_void),
        >(T_Glow));
        (*g).direction = -1;

        (*sector).special = 0;
    }
}

/// Anchor function referenced from `doomgeneric_Create` to ensure all
/// `#[no_mangle]` light functions survive link-time dead-code elimination.
///
/// # Safety
///
/// Must only be called during engine initialisation before any thinker
/// dispatch occurs; taking the address of each function is always safe.
#[no_mangle]
pub unsafe extern "C" fn P_Lights_Link_Anchor() {
    // Force the linker to include every exported symbol from this module.
    let _ = T_FireFlicker as *const () as usize;
    let _ = T_LightFlash as *const () as usize;
    let _ = T_StrobeFlash as *const () as usize;
    let _ = T_Glow as *const () as usize;
    let _ = P_SpawnFireFlicker as *const () as usize;
    let _ = P_SpawnLightFlash as *const () as usize;
    let _ = P_SpawnStrobeFlash as *const () as usize;
    let _ = EV_StartLightStrobing as *const () as usize;
    let _ = EV_TurnTagLightsOff as *const () as usize;
    let _ = EV_LightTurnOn as *const () as usize;
    let _ = P_SpawnGlowingLight as *const () as usize;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    const SECTOR_T_SIZEOF: usize = 128;
    const SECTOR_T_LIGHTLEVEL_OFFSET: usize = 12;
    const SECTOR_T_SPECIAL_OFFSET: usize = 14;
    const SECTOR_T_SPECIALDATA_OFFSET: usize = 104;
    const FIREFLICKER_T_SIZEOF: usize = 48;
    const LIGHTFLASH_T_SIZEOF: usize = 56;
    const STROBE_T_SIZEOF: usize = 56;
    const GLOW_T_SIZEOF: usize = 48;

    #[test]
    fn sector_t_layout_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(
            std::mem::size_of::<sector_t>(),
            SECTOR_T_SIZEOF,
            "sector_t size mismatch: Rust={}, expected={}",
            std::mem::size_of::<sector_t>(),
            SECTOR_T_SIZEOF,
        );
        assert_eq!(
            std::mem::offset_of!(sector_t, lightlevel),
            SECTOR_T_LIGHTLEVEL_OFFSET,
        );
        assert_eq!(
            std::mem::offset_of!(sector_t, special),
            SECTOR_T_SPECIAL_OFFSET,
        );
        assert_eq!(
            std::mem::offset_of!(sector_t, specialdata),
            SECTOR_T_SPECIALDATA_OFFSET,
        );
    }

    #[test]
    fn fireflicker_t_size_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(std::mem::size_of::<fireflicker_t>(), FIREFLICKER_T_SIZEOF,);
    }

    #[test]
    fn lightflash_t_size_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(std::mem::size_of::<lightflash_t>(), LIGHTFLASH_T_SIZEOF,);
    }

    #[test]
    fn strobe_t_size_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(std::mem::size_of::<strobe_t>(), STROBE_T_SIZEOF,);
    }

    #[test]
    fn glow_t_size_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(std::mem::size_of::<glow_t>(), GLOW_T_SIZEOF,);
    }
}
