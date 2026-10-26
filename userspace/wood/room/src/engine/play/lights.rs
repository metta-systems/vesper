//! Sector lighting effects: fire flicker, light flash, strobe, and glow.
//!
//! Rust port of `vendor/doomgeneric/p_lights.c`. Each effect is driven by a
//! thinker that updates a sector's `lightlevel` every game tic.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::engine::{
    map::{SectorId, line_t},
    misc::random::P_Random,
    play::{
        setup::LevelState,
        spec::{P_FindMinSurroundingLight, P_FindSectorFromLineTag, getNextSector},
        tick::{
            FireFlickerId, GlowId, LightFlashId, P_AddThinker, StrobeId, ThinkerId, ThinkerKind,
            new_thinker, thinker_t,
        },
    },
    world::World,
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
fn light_and_min_surrounding(level: &LevelState, sector: SectorId) -> (i32, i32) {
    let lightlevel = level.sectors[sector].lightlevel as i32;
    (
        lightlevel,
        P_FindMinSurroundingLight(level, sector, lightlevel),
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
pub fn T_FireFlicker(world: &mut World, flick_id: FireFlickerId) {
    let count = {
        let flickers = &mut world.thinkers.fire_flickers;
        flickers[flick_id].count -= 1;
        flickers[flick_id].count
    };
    if count != 0 {
        return;
    }

    let amount = (P_Random(world) & 3) * 16;
    let flickers = &mut world.thinkers.fire_flickers;
    let flick = &mut flickers[flick_id];
    let sec = &mut world.level.sectors[flick.sector];

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
pub fn P_SpawnFireFlicker(world: &mut World, sector: SectorId) {
    world.level.sectors[sector].special = 0;

    let flick_id = new_thinker::<fireflicker_t>(world);

    P_AddThinker(world, ThinkerId::FireFlicker(flick_id));

    let (lightlevel, minlight) = light_and_min_surrounding(&world.level, sector);
    let flickers = &mut world.thinkers.fire_flickers;
    let flick = &mut flickers[flick_id];
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
pub fn T_LightFlash(world: &mut World, flash_id: LightFlashId) {
    let duration_mask = {
        let flashes = &mut world.thinkers.light_flashes;
        let flash = &mut flashes[flash_id];
        flash.count -= 1;
        if flash.count != 0 {
            return;
        }

        let sec = &mut world.level.sectors[flash.sector];

        // The light is set before the random duration is drawn, as in C;
        // the draw touches no sector.
        if sec.lightlevel as i32 == flash.maxlight {
            sec.lightlevel = flash.minlight as i16;
            flash.mintime
        } else {
            sec.lightlevel = flash.maxlight as i16;
            flash.maxtime
        }
    };
    let count = (P_Random(world) & duration_mask) + 1;
    world.thinkers.light_flashes[flash_id].count = count;
}

/// Allocate and initialise a random light-flash thinker for `sector`.
///
/// Sets `maxtime = 64` and `mintime = 7`, giving the flash a long bright phase
/// and a short dark phase. The initial countdown is randomised.
///
/// # Panics
///
/// If `sector` is not a sector of the current map.
pub fn P_SpawnLightFlash(world: &mut World, sector: SectorId) {
    world.level.sectors[sector].special = 0;

    let flash_id = new_thinker::<lightflash_t>(world);

    P_AddThinker(world, ThinkerId::LightFlash(flash_id));

    let (lightlevel, minlight) = light_and_min_surrounding(&world.level, sector);
    let maxtime = {
        let flashes = &mut world.thinkers.light_flashes;
        let flash = &mut flashes[flash_id];
        flash.thinker.kind = ThinkerKind::LightFlash;
        flash.sector = sector;
        flash.maxlight = lightlevel;
        flash.minlight = minlight;
        flash.maxtime = 64;
        flash.mintime = 7;
        flash.maxtime
    };
    let count = (P_Random(world) & maxtime) + 1;
    world.thinkers.light_flashes[flash_id].count = count;
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
pub fn T_StrobeFlash(world: &mut World, flash_id: StrobeId) {
    let strobes = &mut world.thinkers.strobes;
    let flash = &mut strobes[flash_id];
    flash.count -= 1;
    if flash.count != 0 {
        return;
    }

    let sec = &mut world.level.sectors[flash.sector];

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
pub fn P_SpawnStrobeFlash(world: &mut World, sector: SectorId, fastOrSlow: i32, inSync: i32) {
    let flash_id = new_thinker::<strobe_t>(world);

    P_AddThinker(world, ThinkerId::Strobe(flash_id));

    let (lightlevel, minlight) = light_and_min_surrounding(&world.level, sector);
    {
        let strobes = &mut world.thinkers.strobes;
        let flash = &mut strobes[flash_id];
        flash.sector = sector;
        flash.darktime = fastOrSlow;
        flash.brighttime = STROBEBRIGHT;
        flash.thinker.kind = ThinkerKind::Strobe;
        flash.maxlight = lightlevel;
        flash.minlight = minlight;

        if flash.minlight == flash.maxlight {
            flash.minlight = 0;
        }
    }

    world.level.sectors[sector].special = 0;

    let count = if inSync == 0 {
        (P_Random(world) & 7) + 1
    } else {
        1
    };
    world.thinkers.strobes[flash_id].count = count;
}

/// Linedef-triggered event: start slow-strobe lighting on all tagged sectors.
///
/// Iterates over sectors whose tag matches `line`'s tag. Sectors that already
/// have an active special effect are skipped.
pub fn EV_StartLightStrobing(world: &mut World, line: &line_t) {
    let mut secnum: i32 = -1;
    loop {
        secnum = P_FindSectorFromLineTag(&world.level, line, secnum);
        if secnum < 0 {
            break;
        }
        let sec = SectorId::new(secnum as usize);
        if world.level.sectors[sec].specialdata.is_some() {
            continue;
        }
        P_SpawnStrobeFlash(world, sec, SLOWDARK, 0);
    }
}

/// Linedef-triggered event: set all tagged sectors to the darkest neighboring level.
///
/// For each sector whose tag matches `line`'s tag, scans adjacent sectors and
/// sets the light level to the minimum found among all neighbors.
pub fn EV_TurnTagLightsOff(world: &mut World, line: &line_t) {
    let level = &mut world.level;
    for j in 0..level.numsectors as usize {
        let sec = SectorId::new(j);
        if level.sectors[sec].tag != line.tag {
            continue;
        }

        let mut min = level.sectors[sec].lightlevel as i32;
        for &templine in level.sector_lines(sec) {
            let Some(tsec) = getNextSector(level, templine, sec) else {
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
pub fn EV_LightTurnOn(world: &mut World, line: &line_t, bright: i32) {
    let level = &mut world.level;
    let mut bright = bright;
    for i in 0..level.numsectors as usize {
        let sec = SectorId::new(i);
        if level.sectors[sec].tag != line.tag {
            continue;
        }

        if bright == 0 {
            for &templine in level.sector_lines(sec) {
                let Some(temp) = getNextSector(level, templine, sec) else {
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
pub fn T_Glow(world: &mut World, glow_id: GlowId) {
    let glows = &mut world.thinkers.glows;
    let glow = &mut glows[glow_id];
    let sec = &mut world.level.sectors[glow.sector];
    match glow.direction {
        -1 => {
            sec.lightlevel -= GLOWSPEED as i16;
            if (sec.lightlevel as i32) <= glow.minlight {
                sec.lightlevel += GLOWSPEED as i16;
                glow.direction = 1;
            }
        }
        1 => {
            sec.lightlevel += GLOWSPEED as i16;
            if (sec.lightlevel as i32) >= glow.maxlight {
                sec.lightlevel -= GLOWSPEED as i16;
                glow.direction = -1;
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
pub fn P_SpawnGlowingLight(world: &mut World, sector: SectorId) {
    let glow_id = new_thinker::<glow_t>(world);

    P_AddThinker(world, ThinkerId::Glow(glow_id));

    let (lightlevel, minlight) = light_and_min_surrounding(&world.level, sector);
    {
        let glows = &mut world.thinkers.glows;
        let glow = &mut glows[glow_id];
        glow.sector = sector;
        glow.minlight = minlight;
        glow.maxlight = lightlevel;
        glow.thinker.kind = ThinkerKind::Glow;
        glow.direction = -1;
    }

    world.level.sectors[sector].special = 0;
}
