//! Sector lighting effects: fire flicker, light flash, strobe, and glow.
//!
//! Rust port of `vendor/doomgeneric/p_lights.c`. Each effect is driven by a
//! thinker that updates a sector's `lightlevel` every game tic.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::engine::{
    map::{line_t, sector_t},
    misc::random::P_Random,
    play::{
        setup::{numsectors, sectors},
        spec::{getNextSector, P_FindMinSurroundingLight, P_FindSectorFromLineTag},
        tick::{thinker_t, P_AddThinker, ThinkerKind},
    },
    zone::{Z_Malloc, PU_LEVSPEC},
};

/// Light change applied each step of the glow oscillation (light units per tic).
pub const GLOWSPEED: i32 = 8;

/// Duration of the bright phase for strobe lights, in tics.
pub const STROBEBRIGHT: i32 = 5;

/// Dark-phase duration for fast strobe lights, in tics (`FASTDARK` in C).
pub const FASTDARK: i32 = 15;

/// Dark-phase duration for slow strobe lights, in tics (`SLOWDARK` in C).
pub const SLOWDARK: i32 = 35;

/// Thinker state for the fire-flicker lighting effect.
///
/// The sector's light level randomly drops by a multiple of 16 every 4 tics,
/// simulating a flickering fire (sector special 17 in Doom).
#[derive(Clone, Copy)]
pub struct fireflicker_t {
    /// Thinker list node.
    pub thinker: thinker_t,
    /// The sector whose light level is being animated.
    pub sector: *mut sector_t,
    /// Tics remaining until the next light change.
    pub count: i32,
    /// Sector's original (maximum) light level.
    pub maxlight: i32,
    /// Minimum light level the flicker will not go below.
    pub minlight: i32,
}

/// Thinker state for the random light-flash effect.
///
/// The sector alternates between `maxlight` and `minlight` at random intervals
/// bounded by `maxtime` and `mintime` (sector special 1 in Doom).
#[derive(Clone, Copy)]
pub struct lightflash_t {
    /// Thinker list node.
    pub thinker: thinker_t,
    /// The sector whose light level is being animated.
    pub sector: *mut sector_t,
    /// Tics remaining until the next light change.
    pub count: i32,
    /// Bright phase light level.
    pub maxlight: i32,
    /// Dark phase light level.
    pub minlight: i32,
    /// Bitmask used to generate the random bright-phase duration (`count & maxtime`).
    pub maxtime: i32,
    /// Bitmask used to generate the random dark-phase duration (`count & mintime`).
    pub mintime: i32,
}

/// Thinker state for the strobe-flash lighting effect.
///
/// The sector alternates between `maxlight` (bright) and `minlight` (dark)
/// with fixed durations of `brighttime` and `darktime` tics respectively
/// (sector specials 3 and 13 in Doom, among others).
#[derive(Clone, Copy)]
pub struct strobe_t {
    /// Thinker list node.
    pub thinker: thinker_t,
    /// The sector whose light level is being animated.
    pub sector: *mut sector_t,
    /// Tics remaining until the next phase change.
    pub count: i32,
    /// Dark-phase light level.
    pub minlight: i32,
    /// Bright-phase light level.
    pub maxlight: i32,
    /// Duration of the dark phase in tics.
    pub darktime: i32,
    /// Duration of the bright phase in tics (always `STROBEBRIGHT` = 5).
    pub brighttime: i32,
}

/// Thinker state for the smoothly oscillating glow effect.
///
/// The sector's light level rises and falls continuously between `minlight`
/// and `maxlight` by `GLOWSPEED` units per tic (sector special 8 in Doom).
#[derive(Clone, Copy)]
pub struct glow_t {
    /// Thinker list node.
    pub thinker: thinker_t,
    /// The sector whose light level is being animated.
    pub sector: *mut sector_t,
    /// Lower bound of the oscillation (minimum light level).
    pub minlight: i32,
    /// Upper bound of the oscillation (maximum / original light level).
    pub maxlight: i32,
    /// Direction of travel: `1` = increasing, `-1` = decreasing.
    pub direction: i32,
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
pub unsafe fn T_FireFlicker(flick: *mut fireflicker_t) {
    (*flick).count -= 1;
    if (*flick).count != 0 {
        return;
    }

    let amount = (P_Random() & 3) * 16;
    let sec = &mut *(*flick).sector;

    if (sec.lightlevel as i32) - amount < (*flick).minlight {
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
pub fn P_SpawnFireFlicker(sector: *mut sector_t) {
    unsafe {
        (*sector).special = 0;

        let flick = Z_Malloc(
            std::mem::size_of::<fireflicker_t>() as i32,
            PU_LEVSPEC,
            std::ptr::null_mut(),
        ) as *mut fireflicker_t;

        P_AddThinker(&raw mut (*flick).thinker);

        (*flick).thinker.kind = ThinkerKind::FireFlicker;
        (*flick).sector = sector;
        (*flick).maxlight = (*sector).lightlevel as i32;
        (*flick).minlight = P_FindMinSurroundingLight(sector, (*sector).lightlevel as i32) + 16;
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
pub unsafe fn T_LightFlash(flash: *mut lightflash_t) {
    (*flash).count -= 1;
    if (*flash).count != 0 {
        return;
    }

    let sec = &mut *(*flash).sector;

    if sec.lightlevel as i32 == (*flash).maxlight {
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
pub fn P_SpawnLightFlash(sector: *mut sector_t) {
    unsafe {
        (*sector).special = 0;

        let flash = Z_Malloc(
            std::mem::size_of::<lightflash_t>() as i32,
            PU_LEVSPEC,
            std::ptr::null_mut(),
        ) as *mut lightflash_t;

        P_AddThinker(&raw mut (*flash).thinker);

        (*flash).thinker.kind = ThinkerKind::LightFlash;
        (*flash).sector = sector;
        (*flash).maxlight = (*sector).lightlevel as i32;
        (*flash).minlight = P_FindMinSurroundingLight(sector, (*sector).lightlevel as i32);
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
pub unsafe fn T_StrobeFlash(flash: *mut strobe_t) {
    (*flash).count -= 1;
    if (*flash).count != 0 {
        return;
    }

    let sec = &mut *(*flash).sector;

    if sec.lightlevel as i32 == (*flash).minlight {
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
pub fn P_SpawnStrobeFlash(sector: *mut sector_t, fastOrSlow: i32, inSync: i32) {
    unsafe {
        let flash = Z_Malloc(
            std::mem::size_of::<strobe_t>() as i32,
            PU_LEVSPEC,
            std::ptr::null_mut(),
        ) as *mut strobe_t;

        P_AddThinker(&raw mut (*flash).thinker);

        (*flash).sector = sector;
        (*flash).darktime = fastOrSlow;
        (*flash).brighttime = STROBEBRIGHT;
        (*flash).thinker.kind = ThinkerKind::Strobe;
        (*flash).maxlight = (*sector).lightlevel as i32;
        (*flash).minlight = P_FindMinSurroundingLight(sector, (*sector).lightlevel as i32);

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
pub fn EV_StartLightStrobing(line: *mut line_t) {
    unsafe {
        let mut secnum: i32 = -1;
        loop {
            secnum = P_FindSectorFromLineTag(line, secnum);
            if secnum < 0 {
                break;
            }
            let sec = sectors.add(secnum as usize);
            if !(*sec).specialdata.is_null() {
                continue;
            }
            P_SpawnStrobeFlash(sec, SLOWDARK, 0);
        }
    }
}

/// Linedef-triggered event: set all tagged sectors to the darkest neighboring level.
///
/// For each sector whose tag matches `line`'s tag, scans adjacent sectors and
/// sets the light level to the minimum found among all neighbors.
pub fn EV_TurnTagLightsOff(line: *mut line_t) {
    unsafe {
        for j in 0..numsectors as usize {
            let sec = sectors.add(j);
            if (*sec).tag != (*line).tag {
                continue;
            }

            let mut min = (*sec).lightlevel as i32;
            for i in 0..(*sec).linecount as usize {
                let templine = *(*sec).lines.add(i);
                let tsec = getNextSector(templine, sec);
                if tsec.is_null() {
                    continue;
                }
                let tl = (*tsec).lightlevel as i32;
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
pub fn EV_LightTurnOn(line: *mut line_t, bright: i32) {
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
                    let temp = getNextSector(templine, sec);
                    if temp.is_null() {
                        continue;
                    }
                    let tl = (*temp).lightlevel as i32;
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
pub unsafe fn T_Glow(g: *mut glow_t) {
    let sec = &mut *(*g).sector;
    match (*g).direction {
        -1 => {
            sec.lightlevel -= GLOWSPEED as i16;
            if (sec.lightlevel as i32) <= (*g).minlight {
                sec.lightlevel += GLOWSPEED as i16;
                (*g).direction = 1;
            }
        }
        1 => {
            sec.lightlevel += GLOWSPEED as i16;
            if (sec.lightlevel as i32) >= (*g).maxlight {
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
pub fn P_SpawnGlowingLight(sector: *mut sector_t) {
    unsafe {
        let g = Z_Malloc(
            std::mem::size_of::<glow_t>() as i32,
            PU_LEVSPEC,
            std::ptr::null_mut(),
        ) as *mut glow_t;

        P_AddThinker(&raw mut (*g).thinker);

        (*g).sector = sector;
        (*g).minlight = P_FindMinSurroundingLight(sector, (*sector).lightlevel as i32);
        (*g).maxlight = (*sector).lightlevel as i32;
        (*g).thinker.kind = ThinkerKind::Glow;
        (*g).direction = -1;

        (*sector).special = 0;
    }
}
