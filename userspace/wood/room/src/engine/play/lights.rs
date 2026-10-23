//! Sector lighting effects: fire flicker, light flash, strobe, and glow.
//!
//! Rust port of `vendor/doomgeneric/p_lights.c`. Each effect is driven by a
//! thinker that updates a sector's `lightlevel` every game tic.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::engine::{
    map::{SectorId, line_t},
    misc::random::P_Random,
    play::{
        setup::LEVEL,
        spec::{P_FindMinSurroundingLight, P_FindSectorFromLineTag, getNextSector},
        tick::{
            FireFlickerId, GlowId, LightFlashId, P_AddThinker, StrobeId, ThinkerId, ThinkerKind,
            new_thinker, thinker_t,
        },
    },
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
    /// Thinker state: what the effect does each tic.
    pub thinker: thinker_t,
    /// The sector whose light level is being animated.
    pub sector: SectorId,
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
    /// Thinker state: what the effect does each tic.
    pub thinker: thinker_t,
    /// The sector whose light level is being animated.
    pub sector: SectorId,
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
    /// Thinker state: what the effect does each tic.
    pub thinker: thinker_t,
    /// The sector whose light level is being animated.
    pub sector: SectorId,
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
    /// Thinker state: what the effect does each tic.
    pub thinker: thinker_t,
    /// The sector whose light level is being animated.
    pub sector: SectorId,
    /// Lower bound of the oscillation (minimum light level).
    pub minlight: i32,
    /// Upper bound of the oscillation (maximum / original light level).
    pub maxlight: i32,
    /// Direction of travel: `1` = increasing, `-1` = decreasing.
    pub direction: i32,
}

/// The sector's light level and the lowest light level around it, capped
/// at its own (`P_FindMinSurroundingLight(sector, sector->lightlevel)`).
fn light_and_min_surrounding(sector: SectorId) -> (i32, i32) {
    let level = LEVEL.borrow();
    let lightlevel = level.sectors[sector].lightlevel as i32;
    (
        lightlevel,
        P_FindMinSurroundingLight(&level, sector, lightlevel),
    )
}

/// Per-tic update for the fire-flicker effect.
///
/// Decrements the countdown; when it reaches zero, randomises the sector's
/// light level and resets the 4-tic counter.
///
/// Called exclusively by the thinker dispatcher from `P_RunThinkers`.
///
/// # Panics
///
/// If the effect's `sector` is not a sector of the current map.
pub fn T_FireFlicker(flick: FireFlickerId) {
    // SAFETY: light effects are only reached through arena pointers, and no
    // other reference to this one is live while it thinks.
    let flick = unsafe { &mut *flick.ptr() };
    flick.count -= 1;
    if flick.count != 0 {
        return;
    }

    let amount = (P_Random() & 3) * 16;
    let mut level = LEVEL.borrow_mut();
    let sec = &mut level.sectors[flick.sector];

    if (sec.lightlevel as i32) - amount < flick.minlight {
        sec.lightlevel = flick.minlight as i16;
    } else {
        sec.lightlevel = (flick.maxlight - amount) as i16;
    }

    flick.count = 4;
}

/// Allocate and initialise a fire-flicker thinker for `sector`.
///
/// Clears the sector special, finds the minimum surrounding light level
/// (used as the lower flicker bound + 16), and registers the thinker.
///
/// # Panics
///
/// If `sector` is not a sector of the current map.
pub fn P_SpawnFireFlicker(sector: SectorId) {
    LEVEL.borrow_mut().sectors[sector].special = 0;

    let flick_id = new_thinker::<fireflicker_t>();

    P_AddThinker(ThinkerId::FireFlicker(flick_id));

    let (lightlevel, minlight) = light_and_min_surrounding(sector);
    // SAFETY: the effect was just made; nothing else refers to it yet.
    let flick = unsafe { &mut *flick_id.ptr() };
    flick.thinker.kind = ThinkerKind::FireFlicker;
    flick.sector = sector;
    flick.maxlight = lightlevel;
    flick.minlight = minlight + 16;
    flick.count = 4;
}

/// Per-tic update for the random light-flash effect.
///
/// Decrements the countdown; when it reaches zero, toggles the sector's
/// light between `maxlight` and `minlight`, picking a new random duration.
///
/// Called exclusively by the thinker dispatcher from `P_RunThinkers`.
///
/// # Panics
///
/// If the effect's `sector` is not a sector of the current map.
pub fn T_LightFlash(flash: LightFlashId) {
    // SAFETY: light effects are only reached through arena pointers, and no
    // other reference to this one is live while it thinks.
    let flash = unsafe { &mut *flash.ptr() };
    flash.count -= 1;
    if flash.count != 0 {
        return;
    }

    let mut level = LEVEL.borrow_mut();
    let sec = &mut level.sectors[flash.sector];

    if sec.lightlevel as i32 == flash.maxlight {
        sec.lightlevel = flash.minlight as i16;
        flash.count = (P_Random() & flash.mintime) + 1;
    } else {
        sec.lightlevel = flash.maxlight as i16;
        flash.count = (P_Random() & flash.maxtime) + 1;
    }
}

/// Allocate and initialise a random light-flash thinker for `sector`.
///
/// Sets `maxtime = 64` and `mintime = 7`, giving the flash a long bright phase
/// and a short dark phase. The initial countdown is randomised.
///
/// # Panics
///
/// If `sector` is not a sector of the current map.
pub fn P_SpawnLightFlash(sector: SectorId) {
    LEVEL.borrow_mut().sectors[sector].special = 0;

    let flash_id = new_thinker::<lightflash_t>();

    P_AddThinker(ThinkerId::LightFlash(flash_id));

    let (lightlevel, minlight) = light_and_min_surrounding(sector);
    // SAFETY: the effect was just made; nothing else refers to it yet.
    let flash = unsafe { &mut *flash_id.ptr() };
    flash.thinker.kind = ThinkerKind::LightFlash;
    flash.sector = sector;
    flash.maxlight = lightlevel;
    flash.minlight = minlight;
    flash.maxtime = 64;
    flash.mintime = 7;
    flash.count = (P_Random() & flash.maxtime) + 1;
}

/// Per-tic update for the strobe-flash effect.
///
/// Decrements the countdown; when it reaches zero, switches the sector's
/// light between `minlight` (dark phase, `darktime` tics) and `maxlight`
/// (bright phase, `brighttime` tics).
///
/// Called exclusively by the thinker dispatcher from `P_RunThinkers`.
///
/// # Panics
///
/// If the effect's `sector` is not a sector of the current map.
pub fn T_StrobeFlash(flash: StrobeId) {
    // SAFETY: light effects are only reached through arena pointers, and no
    // other reference to this one is live while it thinks.
    let flash = unsafe { &mut *flash.ptr() };
    flash.count -= 1;
    if flash.count != 0 {
        return;
    }

    let mut level = LEVEL.borrow_mut();
    let sec = &mut level.sectors[flash.sector];

    if sec.lightlevel as i32 == flash.minlight {
        sec.lightlevel = flash.maxlight as i16;
        flash.count = flash.brighttime;
    } else {
        sec.lightlevel = flash.minlight as i16;
        flash.count = flash.darktime;
    }
}

/// Allocate and initialise a strobe-flash thinker for `sector`.
///
/// `fastOrSlow` sets `darktime` (use `FASTDARK` or `SLOWDARK`).
/// `inSync = 1` starts the flash immediately; `inSync = 0` gives a random offset.
/// If `minlight == maxlight`, `minlight` is forced to 0.
///
/// # Panics
///
/// If `sector` is not a sector of the current map.
pub fn P_SpawnStrobeFlash(sector: SectorId, fastOrSlow: i32, inSync: i32) {
    let flash_id = new_thinker::<strobe_t>();

    P_AddThinker(ThinkerId::Strobe(flash_id));

    let (lightlevel, minlight) = light_and_min_surrounding(sector);
    // SAFETY: the effect was just made; nothing else refers to it yet.
    let flash = unsafe { &mut *flash_id.ptr() };
    flash.sector = sector;
    flash.darktime = fastOrSlow;
    flash.brighttime = STROBEBRIGHT;
    flash.thinker.kind = ThinkerKind::Strobe;
    flash.maxlight = lightlevel;
    flash.minlight = minlight;

    if flash.minlight == flash.maxlight {
        flash.minlight = 0;
    }

    LEVEL.borrow_mut().sectors[sector].special = 0;

    if inSync == 0 {
        flash.count = (P_Random() & 7) + 1;
    } else {
        flash.count = 1;
    }
}

/// Linedef-triggered event: start slow-strobe lighting on all tagged sectors.
///
/// Iterates over sectors whose tag matches `line`'s tag. Sectors that already
/// have an active special effect are skipped.
pub fn EV_StartLightStrobing(line: &line_t) {
    let mut secnum: i32 = -1;
    loop {
        secnum = P_FindSectorFromLineTag(&LEVEL.borrow(), line, secnum);
        if secnum < 0 {
            break;
        }
        let sec = SectorId::new(secnum as usize);
        if LEVEL.borrow().sectors[sec].specialdata.is_some() {
            continue;
        }
        P_SpawnStrobeFlash(sec, SLOWDARK, 0);
    }
}

/// Linedef-triggered event: set all tagged sectors to the darkest neighboring level.
///
/// For each sector whose tag matches `line`'s tag, scans adjacent sectors and
/// sets the light level to the minimum found among all neighbors.
pub fn EV_TurnTagLightsOff(line: &line_t) {
    let mut level = LEVEL.borrow_mut();
    for j in 0..level.numsectors as usize {
        let sec = SectorId::new(j);
        if level.sectors[sec].tag != line.tag {
            continue;
        }

        let mut min = level.sectors[sec].lightlevel as i32;
        for &templine in level.sector_lines(sec) {
            let Some(tsec) = getNextSector(&level, templine, sec) else {
                continue;
            };
            let tl = level.sectors[tsec].lightlevel as i32;
            if tl < min {
                min = tl;
            }
        }
        level.sectors[sec].lightlevel = min as i16;
    }
}

/// Linedef-triggered event: set all tagged sectors to `bright` light level.
///
/// If `bright == 0`, the function searches adjacent sectors and uses the
/// highest light level found among them instead.
pub fn EV_LightTurnOn(line: &line_t, bright: i32) {
    let mut level = LEVEL.borrow_mut();
    let mut bright = bright;
    for i in 0..level.numsectors as usize {
        let sec = SectorId::new(i);
        if level.sectors[sec].tag != line.tag {
            continue;
        }

        if bright == 0 {
            for &templine in level.sector_lines(sec) {
                let Some(temp) = getNextSector(&level, templine, sec) else {
                    continue;
                };
                let tl = level.sectors[temp].lightlevel as i32;
                if tl > bright {
                    bright = tl;
                }
            }
        }
        level.sectors[sec].lightlevel = bright as i16;
    }
}

/// Per-tic update for the smooth glow oscillation effect.
///
/// Adjusts `lightlevel` by `GLOWSPEED` each tic, bouncing at `minlight` and
/// `maxlight` by reversing `direction`.
///
/// Called exclusively by the thinker dispatcher from `P_RunThinkers`.
///
/// # Panics
///
/// If the effect's `sector` is not a sector of the current map.
pub fn T_Glow(glow: GlowId) {
    // SAFETY: light effects are only reached through arena pointers, and no
    // other reference to this one is live while it thinks.
    let g = unsafe { &mut *glow.ptr() };
    let mut level = LEVEL.borrow_mut();
    let sec = &mut level.sectors[g.sector];
    match g.direction {
        -1 => {
            sec.lightlevel -= GLOWSPEED as i16;
            if (sec.lightlevel as i32) <= g.minlight {
                sec.lightlevel += GLOWSPEED as i16;
                g.direction = 1;
            }
        }
        1 => {
            sec.lightlevel += GLOWSPEED as i16;
            if (sec.lightlevel as i32) >= g.maxlight {
                sec.lightlevel -= GLOWSPEED as i16;
                g.direction = -1;
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
///
/// # Panics
///
/// If `sector` is not a sector of the current map.
pub fn P_SpawnGlowingLight(sector: SectorId) {
    let glow_id = new_thinker::<glow_t>();

    P_AddThinker(ThinkerId::Glow(glow_id));

    let (lightlevel, minlight) = light_and_min_surrounding(sector);
    // SAFETY: the effect was just made; nothing else refers to it yet.
    let g = unsafe { &mut *glow_id.ptr() };
    g.sector = sector;
    g.minlight = minlight;
    g.maxlight = lightlevel;
    g.thinker.kind = ThinkerKind::Glow;
    g.direction = -1;

    LEVEL.borrow_mut().sectors[sector].special = 0;
}
