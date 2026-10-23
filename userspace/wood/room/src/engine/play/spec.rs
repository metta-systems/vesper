//! Rust port of vendor/doomgeneric/p_spec.c.
//!
//! Special sector/line action dispatcher: texture animation, height and
//! lighting changes, line tag handling, sector triggers, and the donut
//! effect.  Also contains the utility functions (`getSide`, `getSector`,
//! `twoSided`, `P_Find*Surrounding`) used by the floor/ceiling/platform
//! code.
//!
//! Notable Rust-vs-C differences:
//! - The `animdefs` table is a Rust `const` slice of tuples instead of a
//!   C array of `animdef_t` structs.
//! - `DonutOverrun` uses `static mut` locals to replicate C `static` locals;
//!   first-call initialisation is guarded by a `first` flag exactly as in C.
//! - Button `where` enum (C `bwhere_e`) is stored as a raw `i32` (`0/1/2`)
//!   because the Rust FFI button type uses an integer discriminant.
//! - `P_PlayerInSpecialSector` navigates via `subsector_t` to reach the
//!   sector; the C version accesses `player->mo->subsector->sector` directly.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use {crate::engine::sound::info::Sfx, std::ptr};

use crate::{
    engine::{
        doom::player::{PlayerT, CF_GODMODE},
        interface::timer::TICRATE,
        map::{line_t, sector_t, side_t},
        math::fixed::FRACUNIT,
        misc::{
            argv::{argument, M_CheckParmWithArgs},
            random::P_Random,
            M_StrToInt,
        },
        play::{
            ceilng::MAXCEILINGS,
            floor::{floormove_t, FLOORSPEED},
            info::{MT_BFG, MT_BRUISERSHOT, MT_HEADSHOT, MT_PLASMA, MT_ROCKET, MT_TROOPSHOT},
            lights::{FASTDARK, SLOWDARK},
            mobj::mobj_t,
            plats::MAXPLATS,
            setup::{lines, numlines, numsectors, sectors, sides},
            switch::{buttonlist, MAXBUTTONS},
            tick::{leveltime, P_AddThinker, ThinkerKind},
        },
        render_3d::data::{
            flattranslation, numflats, texturetranslation, R_CheckTextureNumForName,
            R_FlatNumForName, R_TextureNumForName,
        },
        sound::S_StartSectorSound,
        wad::format::LinedefFlag,
        zone::PU_LEVSPEC,
    },
    i_error,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of animated texture/flat cycles that can be active at once.
/// Corresponds to `MAXANIMS` in p_spec.c.
pub const MAXANIMS: usize = 32;

/// Maximum number of scrolling-wall linedefs that may be registered per level.
/// Corresponds to `MAXLINEANIMS` in p_spec.c (Vanilla Doom limit is 64).
pub const MAXLINEANIMS: usize = 64;

/// Maximum number of adjoining sectors examined by `P_FindNextHighestFloor`.
/// Exceeding this limit emulates the vanilla stack-overflow behaviour.
/// Corresponds to `MAX_ADJOINING_SECTORS` in p_spec.c.
const MAX_ADJOINING_SECTORS: usize = 20;

// vldoor_e — door direction/behaviour constants used by `EV_DoDoor`.
/// Door opens and stays open. (`vld_open`)
const vld_normal: i32 = 0;
/// Door closes, waits 30 seconds, then opens again. (`vld_close30ThenOpen`)
const vld_close30ThenOpen: i32 = 1;
/// Door closes and stays closed. (`vld_close`)
const vld_close: i32 = 2;
/// Door opens normally (raise then wait then lower). (`vld_open` in C is 3 here mapped as `vld_normal=0`)
const vld_open: i32 = 3;
/// Blaze-speed raise-then-lower door. (`vld_blazeRaise`)
const vld_blazeRaise: i32 = 5;
/// Blaze-speed open door. (`vld_blazeOpen`)
const vld_blazeOpen: i32 = 6;
/// Blaze-speed close door. (`vld_blazeClose`)
const vld_blazeClose: i32 = 7;

// floor_e — floor movement type constants used by `EV_DoFloor`.
/// Lower floor to next lowest neighbouring floor. (`lowerFloor`)
const lowerFloor: i32 = 0;
/// Lower floor to the absolute lowest neighbouring floor. (`lowerFloorToLowest`)
const lowerFloorToLowest: i32 = 1;
/// Lower floor at turbo speed. (`turboLower`)
const turboLower: i32 = 2;
/// Raise floor to next highest neighbouring floor. (`raiseFloor`)
const raiseFloor: i32 = 3;
/// Raise floor to nearest neighbouring floor. (`raiseFloorToNearest`)
const raiseFloorToNearest: i32 = 4;
/// Raise floor to the height of the shortest lower texture on a bounding wall. (`raiseToTexture`)
const raiseToTexture: i32 = 5;
/// Lower floor and change its texture/special to match the destination sector. (`lowerAndChange`)
const lowerAndChange: i32 = 6;
/// Raise floor by exactly 24 map units (fixed-point 16.16). (`raiseFloor24`)
const raiseFloor24: i32 = 7;
/// Raise floor by 24 map units and change texture/special. (`raiseFloor24AndChange`)
const raiseFloor24AndChange: i32 = 8;
/// Raise floor while crushing anything caught between floor and ceiling. (`raiseFloorCrush`)
const raiseFloorCrush: i32 = 9;
/// Raise floor at turbo speed. (`raiseFloorTurbo`)
const raiseFloorTurbo: i32 = 10;
/// Raise floor to the height of the outer (ring) sector in a donut effect. (`donutRaise`)
const donutRaise: i32 = 11;

// ceiling_e — ceiling movement type constants used by `EV_DoCeiling`.
/// Raise ceiling to the highest neighbouring ceiling. (`raiseToHighest`)
const raiseToHighest: i32 = 1;
/// Lower ceiling while crushing. (`lowerAndCrush`)
const lowerAndCrush: i32 = 2;
/// Crush ceiling down then raise it repeatedly. (`crushAndRaise`)
const crushAndRaise: i32 = 3;
/// Fast crush-and-raise ceiling. (`fastCrushAndRaise`)
const fastCrushAndRaise: i32 = 4;
/// Silent crush-and-raise ceiling (no grinding sound). (`silentCrushAndRaise`)
const silentCrushAndRaise: i32 = 5;

// plattype_e — platform movement type constants used by `EV_DoPlat`.
/// Platform goes down, waits, then rises back up. (`downWaitUpStay`)
const downWaitUpStay: i32 = 1;
/// Platform raises to the nearest floor height and changes texture. (`raiseToNearestAndChange`)
const raiseToNearestAndChange: i32 = 3;
/// Blaze-speed down-wait-up-stay platform. (`blazeDWUS`)
const blazeDWUS: i32 = 4;
/// Platform oscillates perpetually up and down. (`perpetualRaise`)
const perpetualRaise: i32 = 0;

// stair_e — stair-building step size constants used by `EV_BuildStairs`.
/// Build stairs in 8-unit steps. (`build8`)
const build8: i32 = 0;
/// Build stairs in 16-unit turbo steps. (`turbo16`)
const turbo16: i32 = 1;

// powers — index into `player_t::powers[]`.
/// Index of the Radiation Suit power in the powers array; grants immunity to slime damage.
const pw_ironfeet: usize = 3;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Runtime state for a single animated texture or flat sequence.
///
/// Maps to the C `anim_t` typedef in p_spec.c (also used internally in
/// wi_stuff.c with different semantics, but this is the p_spec version).
#[derive(Clone, Copy)]
pub struct anim_t {
    /// Non-zero if this animation cycle is for wall textures; zero for flats.
    pub istexture: i32,
    /// WAD lump number of the last frame in the sequence.
    pub picnum: i32,
    /// WAD lump number of the first frame in the sequence.
    pub basepic: i32,
    /// Total number of frames in the cycle (`picnum - basepic + 1`).
    pub numpics: i32,
    /// Tic-count duration of each frame; the sequence advances every `speed` tics.
    pub speed: i32,
}

// ---------------------------------------------------------------------------
// Animation definitions
// ---------------------------------------------------------------------------

/// Compile-time animation definition table, equivalent to C `animdefs[]`.
///
/// Each tuple is `(istexture, endname, startname, speed)`.  A sentinel entry
/// with `istexture == -1` terminates the list. Sourced from p_spec.c
/// `animdefs[]`.
const ANIMDEFS: &[(i32, &[u8], &[u8], i32)] = &[
    (0, b"NUKAGE3", b"NUKAGE1", 8),
    (0, b"FWATER4", b"FWATER1", 8),
    (0, b"SWATER4", b"SWATER1", 8),
    (0, b"LAVA4", b"LAVA1", 8),
    (0, b"BLOOD3", b"BLOOD1", 8),
    (0, b"RROCK08", b"RROCK05", 8),
    (0, b"SLIME04", b"SLIME01", 8),
    (0, b"SLIME08", b"SLIME05", 8),
    (0, b"SLIME12", b"SLIME09", 8),
    (1, b"BLODGR4", b"BLODGR1", 8),
    (1, b"SLADRIP3", b"SLADRIP1", 8),
    (1, b"BLODRIP4", b"BLODRIP1", 8),
    (1, b"FIREWALL", b"FIREWALA", 8),
    (1, b"GSTFONT3", b"GSTFONT1", 8),
    (1, b"FIRELAVA", b"FIRELAV3", 8),
    (1, b"FIREMAG3", b"FIREMAG1", 8),
    (1, b"FIREBLU2", b"FIREBLU1", 8),
    (1, b"ROCKRED3", b"ROCKRED1", 8),
    (1, b"BFALL4", b"BFALL1", 8),
    (1, b"SFALL4", b"SFALL1", 8),
    (1, b"WFALL4", b"WFALL1", 8),
    (1, b"DBRAIN4", b"DBRAIN1", 8),
    (-1, b"", b"", 0),
];

// ---------------------------------------------------------------------------
// Globals
// ---------------------------------------------------------------------------

/// Array of active animation state records, one per registered animation cycle.
///
/// Populated by `P_InitPicAnims`; only entries in `anims[0..lastanim)` are
/// valid.  The renderer reads `texturetranslation`/`flattranslation`, which
/// `P_UpdateSpecials` updates based on this array.
pub static mut anims: [anim_t; MAXANIMS] = [anim_t {
    istexture: 0,
    picnum: 0,
    basepic: 0,
    numpics: 0,
    speed: 0,
}; MAXANIMS];

/// Pointer one-past the last valid entry in `anims[]`.
///
/// Acts as an end-iterator: the range `anims..lastanim` contains every active
/// animation cycle.  Null on startup, set by `P_InitPicAnims`.
pub static mut lastanim: *mut anim_t = ptr::null_mut();

/// Number of scrolling-wall linedefs registered in `linespeciallist`.
///
/// Counts lines with special 48 (first-column texture scroll).  Reset to zero
/// by `P_SpawnSpecials` at map start; read by `P_UpdateSpecials`.
pub static mut numlinespecials: i16 = 0;

/// List of pointers to linedefs that carry the scrolling-wall special (48).
///
/// Populated by `P_SpawnSpecials`; iterated by `P_UpdateSpecials` each tic to
/// advance `textureoffset` by one `FRACUNIT`.  Capped at `MAXLINEANIMS` (64)
/// entries.
pub static mut linespeciallist: [*mut line_t; MAXLINEANIMS] = [ptr::null_mut(); MAXLINEANIMS];

/// Non-zero when a deathmatch level timer is active.
///
/// Set to 1 by `P_SpawnSpecials` when `timelimit > 0` and `deathmatch != 0`.
/// Checked each tic by `P_UpdateSpecials`.  Stored as `i32` boolean to
/// match the C declaration `boolean levelTimer`.
pub static mut levelTimer: i32 = 0; // boolean

/// Remaining tics before the deathmatch time limit expires.
///
/// Initialised by `P_SpawnSpecials` to `timelimit * 60 * TICRATE`.
/// Decremented each tic by `P_UpdateSpecials`; reaching zero calls
/// `G_ExitLevel`.
pub static mut levelTimeCount: i32 = 0;

// ---------------------------------------------------------------------------
// Externs from other modules / remaining C code
// ---------------------------------------------------------------------------

use crate::engine::{
    dehacked::DEH_String,
    game::{deathmatch, timelimit, totalsecret, G_ExitLevel, G_SecretExitLevel},
    play::{
        ceilng::{EV_CeilingCrushStop, EV_DoCeiling},
        doors::{EV_DoDoor, P_SpawnDoorCloseIn30, P_SpawnDoorRaiseIn5Mins},
        floor::{EV_BuildStairs, EV_DoFloor},
        inter::P_DamageMobj,
        lights::{
            EV_LightTurnOn, EV_StartLightStrobing, EV_TurnTagLightsOff, P_SpawnFireFlicker,
            P_SpawnGlowingLight, P_SpawnLightFlash, P_SpawnStrobeFlash,
        },
        plats::{EV_DoPlat, EV_StopPlat},
        switch::P_ChangeSwitchTexture,
        telept::EV_Teleport,
    },
    wad::W_CheckNumForName,
    zone::Z_Malloc,
};

// ---------------------------------------------------------------------------
// P_InitPicAnims
// ---------------------------------------------------------------------------

/// Initialises the animated texture and flat cycle table from `ANIMDEFS`.
///
/// Iterates `ANIMDEFS` until the sentinel entry (`istexture == -1`).  For
/// each entry it resolves the start and end lump numbers via
/// `R_TextureNumForName` / `R_FlatNumForName` (skipping entries whose start
/// lump does not exist in the WAD).  The resolved `anim_t` record is written
/// into the `anims` array and `lastanim` is advanced.
///
/// Calls `I_Error` (via `i_error!`) if an animation cycle contains fewer than
/// two frames.
///
/// Called once at map load time from C (p_spec.c `P_SpawnSpecials` via the
/// game initialisation path).
pub unsafe fn P_InitPicAnims() {
    lastanim = std::ptr::addr_of_mut!(anims[0]);
    for &(istexture, endname, startname, speed) in ANIMDEFS {
        if istexture == -1 {
            break;
        }
        let startname = DEH_String(startname);
        let endname = DEH_String(endname);

        if istexture != 0 {
            if R_CheckTextureNumForName(startname) == -1 {
                continue;
            }
            (*lastanim).picnum = R_TextureNumForName(endname);
            (*lastanim).basepic = R_TextureNumForName(startname);
        } else {
            if W_CheckNumForName(startname) == -1 {
                continue;
            }
            (*lastanim).picnum = R_FlatNumForName(endname);
            (*lastanim).basepic = R_FlatNumForName(startname);
        }

        (*lastanim).istexture = istexture;
        (*lastanim).numpics = (*lastanim).picnum - (*lastanim).basepic + 1;

        if (*lastanim).numpics < 2 {
            i_error!(
                "P_InitPicAnims: bad cycle from {} to {}",
                String::from_utf8_lossy(startname),
                String::from_utf8_lossy(endname)
            );
        }

        (*lastanim).speed = speed;
        lastanim = lastanim.offset(1);
    }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

/// Returns a pointer to the `side_t` on a given side of a line bounding a sector.
///
/// - `currentSector`: index into the global `sectors` array.
/// - `line`: index into `sector.lines[]` (the sector's own line list, not the
///   global `lines` array).
/// - `side`: 0 for front, 1 for back.
///
/// Returns a pointer into the global `sides` array.  The pointer is valid as
/// long as the map data is loaded.  Called from p_spec.c and the floor/ceiling
/// modules.
pub unsafe fn getSide(currentSector: i32, line: i32, side: i32) -> *mut side_t {
    let line_ptr = *(*sectors.offset(currentSector as isize))
        .lines
        .offset(line as isize);
    let side_idx = (*line_ptr).sidenum[side as usize];
    sides.offset(side_idx as isize)
}

/// Returns a pointer to the `sector_t` on a given side of a line bounding a sector.
///
/// - `currentSector`: index into the global `sectors` array.
/// - `line`: index into `sector.lines[]`.
/// - `side`: 0 for front sector, 1 for back sector.
///
/// Returns a pointer to the sector.  Callers must guard against the back sector
/// being null (one-sided line) before dereferencing the result.  Called from
/// p_spec.c and various map-action modules.
pub unsafe fn getSector(currentSector: i32, line: i32, side: i32) -> *mut sector_t {
    let line_ptr = *(*sectors.offset(currentSector as isize))
        .lines
        .offset(line as isize);
    let side_idx = (*line_ptr).sidenum[side as usize];
    (*sides.offset(side_idx as isize)).sector
}

/// Returns non-zero if the given line on a sector's boundary is two-sided.
///
/// - `sector`: index into the global `sectors` array.
/// - `line`: index into `sector.lines[]`.
///
/// Returns the `ML_TWOSIDED` flag value (non-zero) if the line has a back
/// sector, or zero for one-sided lines.  Called from p_spec.c and the
/// floor/ceiling/platform modules.
pub unsafe fn twoSided(sector: i32, line: i32) -> i32 {
    let line_ptr = *(*sectors.offset(sector as isize))
        .lines
        .offset(line as isize);
    ((*line_ptr).flags as i32) & (LinedefFlag::TWOSIDED as i32)
}

/// Returns the sector on the opposite side of `line` from `sec`, or null.
///
/// If `line` is one-sided (no `ML_TWOSIDED` flag), returns null.  Otherwise
/// returns whichever of the front/back sectors is not `sec`.  Called
/// extensively by the `P_Find*Surrounding` family of functions.
pub unsafe fn getNextSector(line: *mut line_t, sec: *mut sector_t) -> *mut sector_t {
    if ((*line).flags as i32) & (LinedefFlag::TWOSIDED as i32) == 0 {
        return ptr::null_mut();
    }
    if (*line).frontsector == sec {
        return (*line).backsector;
    }
    (*line).frontsector
}

/// Returns the lowest floor height among all sectors neighbouring `sec`.
///
/// Walks every line bounding `sec`, finds the sector on the other side via
/// `getNextSector`, and returns the minimum floor height seen.  If no
/// two-sided lines are found the current sector's own floor height is returned.
///
/// Heights are in fixed-point 16.16 (`fixed_t` / `i32`).  Called from
/// p_floor and p_spec.
pub unsafe fn P_FindLowestFloorSurrounding(sec: *mut sector_t) -> i32 {
    let mut floor = (*sec).floorheight;
    for i in 0..(*sec).linecount {
        let check = *(*sec).lines.offset(i as isize);
        let other = getNextSector(check, sec);
        if other.is_null() {
            continue;
        }
        if (*other).floorheight < floor {
            floor = (*other).floorheight;
        }
    }
    floor
}

/// Returns the highest floor height among all sectors neighbouring `sec`.
///
/// Walks every bounding line of `sec` and returns the maximum neighbouring
/// floor height.  If no two-sided neighbours are found returns -500 *
/// `FRACUNIT` (the C sentinel initial value), which is effectively negative
/// infinity for practical map heights.
///
/// Heights are fixed-point 16.16.  Called from p_floor.c and p_spec.c.
pub unsafe fn P_FindHighestFloorSurrounding(sec: *mut sector_t) -> i32 {
    let mut floor = -500 * FRACUNIT;
    for i in 0..(*sec).linecount {
        let check = *(*sec).lines.offset(i as isize);
        let other = getNextSector(check, sec);
        if other.is_null() {
            continue;
        }
        if (*other).floorheight > floor {
            floor = (*other).floorheight;
        }
    }
    floor
}

/// Returns the next floor height above `currentheight` among neighbouring sectors.
///
/// Collects all neighbouring floor heights that exceed `currentheight` into a
/// fixed-size array (`MAX_ADJOINING_SECTORS + 2` entries) and returns the
/// minimum of those heights, which is the lowest floor that is still above the
/// current one.
///
/// Emulates Vanilla Doom's buffer-overrun behaviour for sectors with more than
/// 20 adjoining sectors:
/// - At exactly 21 neighbours (`h == MAX_ADJOINING_SECTORS + 1`) the loop
///   overwrites the `height` variable on the (virtual) stack, updating the
///   working height.
/// - At 22 neighbours (`h == MAX_ADJOINING_SECTORS + 2`) the game would crash
///   in Vanilla; here `i_error!` is called instead.
///
/// Returns `currentheight` if no higher neighbour exists.  Heights are
/// fixed-point 16.16.  Called from p_floor.c.
pub unsafe fn P_FindNextHighestFloor(sec: *mut sector_t, currentheight: i32) -> i32 {
    let mut height = currentheight;
    let mut heightlist: [i32; MAX_ADJOINING_SECTORS + 2] = [0; MAX_ADJOINING_SECTORS + 2];
    let mut h = 0;

    for i in 0..(*sec).linecount {
        let check = *(*sec).lines.offset(i as isize);
        let other = getNextSector(check, sec);
        if other.is_null() {
            continue;
        }
        if (*other).floorheight > height {
            if h == MAX_ADJOINING_SECTORS + 1 {
                height = (*other).floorheight;
            } else if h == MAX_ADJOINING_SECTORS + 2 {
                i_error!("Sector with more than 22 adjoining sectors. Vanilla will crash here");
            }
            heightlist[h] = (*other).floorheight;
            h += 1;
        }
    }

    if h == 0 {
        return currentheight;
    }

    let mut min = heightlist[0];
    for i in 1..h {
        if heightlist[i] < min {
            min = heightlist[i];
        }
    }
    min
}

/// Returns the lowest ceiling height among all sectors neighbouring `sec`.
///
/// Starts from `i32::MAX` (matching C `INT_MAX`) and walks every bounding
/// line, returning the minimum neighbouring ceiling height.  If no two-sided
/// neighbours exist returns `INT_MAX`.
///
/// Heights are fixed-point 16.16.  Called from p_ceilng.c and p_spec.c.
pub unsafe fn P_FindLowestCeilingSurrounding(sec: *mut sector_t) -> i32 {
    let mut height = i32::MAX;
    for i in 0..(*sec).linecount {
        let check = *(*sec).lines.offset(i as isize);
        let other = getNextSector(check, sec);
        if other.is_null() {
            continue;
        }
        if (*other).ceilingheight < height {
            height = (*other).ceilingheight;
        }
    }
    height
}

/// Returns the highest ceiling height among all sectors neighbouring `sec`.
///
/// Starts from 0 and walks every bounding line, returning the maximum
/// neighbouring ceiling height.  If no two-sided neighbours exist returns 0.
///
/// Heights are fixed-point 16.16.  Called from p_ceilng.c and p_spec.c.
pub unsafe fn P_FindHighestCeilingSurrounding(sec: *mut sector_t) -> i32 {
    let mut height = 0;
    for i in 0..(*sec).linecount {
        let check = *(*sec).lines.offset(i as isize);
        let other = getNextSector(check, sec);
        if other.is_null() {
            continue;
        }
        if (*other).ceilingheight > height {
            height = (*other).ceilingheight;
        }
    }
    height
}

/// Finds the next sector whose tag matches `line->tag`, searching from `start + 1`.
///
/// Iterates the global `sectors` array from index `start + 1` onwards and
/// returns the index of the first sector whose `tag` equals `line->tag`.
/// Returns -1 if no matching sector is found.
///
/// Used as the iterator in all tag-based action loops (donut, door, floor,
/// ceiling, etc.).  Called from many action functions including `EV_DoDonut`.
pub unsafe fn P_FindSectorFromLineTag(line: *mut line_t, start: i32) -> i32 {
    for i in (start + 1)..numsectors {
        if (*sectors.offset(i as isize)).tag == (*line).tag {
            return i;
        }
    }
    -1
}

/// Returns the minimum light level among all sectors neighbouring `sector`,
/// clamped to `max` from above.
///
/// Walks every line bounding `sector`, checks the light level of each
/// two-sided neighbour, and returns the smallest value found.  If all
/// neighbours have light >= `max`, returns `max`.
///
/// Light levels are raw `i16` values cast to `i32`; the valid Doom range
/// is 0-255.  Called from p_lights.c to calculate strobe and flash targets.
pub unsafe fn P_FindMinSurroundingLight(sector: *mut sector_t, max: i32) -> i32 {
    let mut min = max;
    for i in 0..(*sector).linecount {
        let line = *(*sector).lines.offset(i as isize);
        let check = getNextSector(line, sector);
        if check.is_null() {
            continue;
        }
        if ((*check).lightlevel as i32) < min {
            min = (*check).lightlevel as i32;
        }
    }
    min
}

// ---------------------------------------------------------------------------
// P_CrossSpecialLine
// ---------------------------------------------------------------------------

/// Processes the special action for a linedef that a map object has just crossed.
///
/// Called every tic when a thing's origin crosses a line whose `special` field
/// is non-zero.  Non-player things are filtered: projectiles (rockets, plasma,
/// BFG balls, trooper/head/bruiser shots) are always ignored; other monsters
/// may only activate specials 4, 10, 39, 88, 97, 125, and 126.
///
/// The function dispatches on `line->special`:
/// - Specials 2-71 and 130-141 are one-shot: the handler clears `line->special`
///   to 0 after firing so the line cannot trigger again.
/// - Specials 72-129 are re-triggerable: `line->special` is left intact.
/// - Special 52 exits the level; special 124 exits to the secret level (both
///   are in the re-triggerable range and do not clear `line->special`).
/// - Special 125 is a one-shot monster-only teleport (activates only when
///   `thing` has no player, then clears `line->special`).
/// - Special 126 is a re-triggerable monster-only teleport (same player guard,
///   but does not clear `line->special`).
///
/// - `linenum`: index into the global `lines` array.
/// - `side`: side of the line the thing is approaching from (0 = front).
/// - `thing`: the map object that crossed the line.
///
/// Called from `p_map.c` via C FFI.
pub unsafe fn P_CrossSpecialLine(linenum: i32, side: i32, thing: *mut mobj_t) {
    let line = lines.offset(linenum as isize);

    if (*thing).player.is_null() {
        match (*thing).kind {
            MT_ROCKET | MT_PLASMA | MT_BFG | MT_TROOPSHOT | MT_HEADSHOT | MT_BRUISERSHOT => {
                return;
            }
            _ => {}
        }

        let mut ok = 0;
        match (*line).special as i32 {
            39 | 97 | 125 | 126 | 4 | 10 | 88 => ok = 1,
            _ => {}
        }
        if ok == 0 {
            return;
        }
    }

    match (*line).special as i32 {
        2 => {
            EV_DoDoor(line, vld_open);
            (*line).special = 0;
        }
        3 => {
            EV_DoDoor(line, vld_close);
            (*line).special = 0;
        }
        4 => {
            EV_DoDoor(line, vld_normal);
            (*line).special = 0;
        }
        5 => {
            EV_DoFloor(line, raiseFloor);
            (*line).special = 0;
        }
        6 => {
            EV_DoCeiling(line, fastCrushAndRaise);
            (*line).special = 0;
        }
        8 => {
            EV_BuildStairs(line, build8);
            (*line).special = 0;
        }
        10 => {
            EV_DoPlat(line, downWaitUpStay, 0);
            (*line).special = 0;
        }
        12 => {
            EV_LightTurnOn(line, 0);
            (*line).special = 0;
        }
        13 => {
            EV_LightTurnOn(line, 255);
            (*line).special = 0;
        }
        16 => {
            EV_DoDoor(line, vld_close30ThenOpen);
            (*line).special = 0;
        }
        17 => {
            EV_StartLightStrobing(line);
            (*line).special = 0;
        }
        19 => {
            EV_DoFloor(line, lowerFloor);
            (*line).special = 0;
        }
        22 => {
            EV_DoPlat(line, raiseToNearestAndChange, 0);
            (*line).special = 0;
        }
        25 => {
            EV_DoCeiling(line, crushAndRaise);
            (*line).special = 0;
        }
        30 => {
            EV_DoFloor(line, raiseToTexture);
            (*line).special = 0;
        }
        35 => {
            EV_LightTurnOn(line, 35);
            (*line).special = 0;
        }
        36 => {
            EV_DoFloor(line, turboLower);
            (*line).special = 0;
        }
        37 => {
            EV_DoFloor(line, lowerAndChange);
            (*line).special = 0;
        }
        38 => {
            EV_DoFloor(line, lowerFloorToLowest);
            (*line).special = 0;
        }
        39 => {
            EV_Teleport(line, side, thing);
            (*line).special = 0;
        }
        40 => {
            EV_DoCeiling(line, raiseToHighest);
            EV_DoFloor(line, lowerFloorToLowest);
            (*line).special = 0;
        }
        44 => {
            EV_DoCeiling(line, lowerAndCrush);
            (*line).special = 0;
        }
        52 => {
            G_ExitLevel();
        }
        53 => {
            EV_DoPlat(line, perpetualRaise, 0);
            (*line).special = 0;
        }
        54 => {
            EV_StopPlat(line);
            (*line).special = 0;
        }
        56 => {
            EV_DoFloor(line, raiseFloorCrush);
            (*line).special = 0;
        }
        57 => {
            EV_CeilingCrushStop(line);
            (*line).special = 0;
        }
        58 => {
            EV_DoFloor(line, raiseFloor24);
            (*line).special = 0;
        }
        59 => {
            EV_DoFloor(line, raiseFloor24AndChange);
            (*line).special = 0;
        }
        104 => {
            EV_TurnTagLightsOff(line);
            (*line).special = 0;
        }
        108 => {
            EV_DoDoor(line, vld_blazeRaise);
            (*line).special = 0;
        }
        109 => {
            EV_DoDoor(line, vld_blazeOpen);
            (*line).special = 0;
        }
        100 => {
            EV_BuildStairs(line, turbo16);
            (*line).special = 0;
        }
        110 => {
            EV_DoDoor(line, vld_blazeClose);
            (*line).special = 0;
        }
        119 => {
            EV_DoFloor(line, raiseFloorToNearest);
            (*line).special = 0;
        }
        121 => {
            EV_DoPlat(line, blazeDWUS, 0);
            (*line).special = 0;
        }
        124 => {
            G_SecretExitLevel();
        }
        125 if (*thing).player.is_null() => {
            EV_Teleport(line, side, thing);
            (*line).special = 0;
        }
        130 => {
            EV_DoFloor(line, raiseFloorTurbo);
            (*line).special = 0;
        }
        141 => {
            EV_DoCeiling(line, silentCrushAndRaise);
            (*line).special = 0;
        }
        // RETRIGGERS
        72 => {
            EV_DoCeiling(line, lowerAndCrush);
        }
        73 => {
            EV_DoCeiling(line, crushAndRaise);
        }
        74 => {
            EV_CeilingCrushStop(line);
        }
        75 => {
            EV_DoDoor(line, vld_close);
        }
        76 => {
            EV_DoDoor(line, vld_close30ThenOpen);
        }
        77 => {
            EV_DoCeiling(line, fastCrushAndRaise);
        }
        79 => {
            EV_LightTurnOn(line, 35);
        }
        80 => {
            EV_LightTurnOn(line, 0);
        }
        81 => {
            EV_LightTurnOn(line, 255);
        }
        82 => {
            EV_DoFloor(line, lowerFloorToLowest);
        }
        83 => {
            EV_DoFloor(line, lowerFloor);
        }
        84 => {
            EV_DoFloor(line, lowerAndChange);
        }
        86 => {
            EV_DoDoor(line, vld_open);
        }
        87 => {
            EV_DoPlat(line, perpetualRaise, 0);
        }
        88 => {
            EV_DoPlat(line, downWaitUpStay, 0);
        }
        89 => {
            EV_StopPlat(line);
        }
        90 => {
            EV_DoDoor(line, vld_normal);
        }
        91 => {
            EV_DoFloor(line, raiseFloor);
        }
        92 => {
            EV_DoFloor(line, raiseFloor24);
        }
        93 => {
            EV_DoFloor(line, raiseFloor24AndChange);
        }
        94 => {
            EV_DoFloor(line, raiseFloorCrush);
        }
        95 => {
            EV_DoPlat(line, raiseToNearestAndChange, 0);
        }
        96 => {
            EV_DoFloor(line, raiseToTexture);
        }
        97 => {
            EV_Teleport(line, side, thing);
        }
        98 => {
            EV_DoFloor(line, turboLower);
        }
        105 => {
            EV_DoDoor(line, vld_blazeRaise);
        }
        106 => {
            EV_DoDoor(line, vld_blazeOpen);
        }
        107 => {
            EV_DoDoor(line, vld_blazeClose);
        }
        120 => {
            EV_DoPlat(line, blazeDWUS, 0);
        }
        126 if (*thing).player.is_null() => {
            EV_Teleport(line, side, thing);
        }
        128 => {
            EV_DoFloor(line, raiseFloorToNearest);
        }
        129 => {
            EV_DoFloor(line, raiseFloorTurbo);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// P_ShootSpecialLine
// ---------------------------------------------------------------------------

/// Processes a linedef special triggered by a projectile or hitscan impact.
///
/// Handles the three impact (gun-activated) line specials:
/// - Special 24: raise floor (one-shot, texture changes to switch).
/// - Special 46: open door (re-triggerable; only special 46 can be activated
///   by non-player things).
/// - Special 47: raise platform to nearest floor and change texture (one-shot).
///
/// Non-player things can only activate special 46; all other specials require
/// a player-controlled attack.
///
/// - `thing`: the map object whose attack hit the line.
/// - `line`: pointer to the linedef that was hit.
///
/// Called from p_map.c (`PTR_ShootTraverse`) when a bullet or projectile
/// hits a special linedef.
pub unsafe fn P_ShootSpecialLine(thing: *mut mobj_t, line: *mut line_t) {
    if (*thing).player.is_null() {
        let mut ok = 0;
        if (*line).special as i32 == 46 {
            ok = 1
        }
        if ok == 0 {
            return;
        }
    }

    match (*line).special as i32 {
        24 => {
            EV_DoFloor(line, raiseFloor);
            P_ChangeSwitchTexture(line, 0);
        }
        46 => {
            EV_DoDoor(line, vld_open);
            P_ChangeSwitchTexture(line, 1);
        }
        47 => {
            EV_DoPlat(line, raiseToNearestAndChange, 0);
            P_ChangeSwitchTexture(line, 0);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// P_PlayerInSpecialSector
// ---------------------------------------------------------------------------

/// Applies the environmental effect of a special sector to the player each tic.
///
/// Called every tic for each player whose map-object origin is at floor level
/// in a special sector (`mo->z == sector->floorheight`).  Players who are
/// airborne (falling, jumping via cheats) are not affected.
///
/// Handled sector specials:
/// - 5: Hellslime - 10 hp damage every 32 tics unless Radiation Suit active.
/// - 7: Nukage - 5 hp damage every 32 tics unless Radiation Suit active.
/// - 4 / 16: Strobe Hurt / Super Hellslime - 20 hp damage every 32 tics;
///   Radiation Suit grants only partial protection (bypassed if `P_Random() < 5`).
/// - 9: Secret - increments `player->secretcount` and clears `sector->special`
///   so it fires only once.
/// - 11: End-level damage (E1M8 finale style) - strips God Mode, deals 20 hp
///   damage every 32 tics, and exits the level when health drops to 10 or below.
///
/// Calls `i_error!` for any unhandled special number, matching C `I_Error`.
///
/// - `player`: pointer to the player structure being updated.
///
/// Called from g_game.c / p_tick.c each game tic for every active player.
pub unsafe fn P_PlayerInSpecialSector(player: *mut PlayerT) {
    let mo = (*player).mo;
    let sub = (*mo).subsector;
    let sector = (*sub).sector;

    if (*mo).z != (*sector).floorheight {
        return;
    }

    match (*sector).special as i32 {
        5 => {
            if (*player).powers[pw_ironfeet] == 0 && leveltime & 0x1f == 0 {
                P_DamageMobj((*player).mo, ptr::null_mut(), ptr::null_mut(), 10);
            }
        }
        7 => {
            if (*player).powers[pw_ironfeet] == 0 && leveltime & 0x1f == 0 {
                P_DamageMobj((*player).mo, ptr::null_mut(), ptr::null_mut(), 5);
            }
        }
        16 | 4 => {
            if ((*player).powers[pw_ironfeet] == 0 || P_Random() < 5) && leveltime & 0x1f == 0 {
                P_DamageMobj((*player).mo, ptr::null_mut(), ptr::null_mut(), 20);
            }
        }
        9 => {
            (*player).secretcount += 1;
            (*sector).special = 0;
        }
        11 => {
            (*player).cheats &= !CF_GODMODE;
            if leveltime & 0x1f == 0 {
                P_DamageMobj((*player).mo, ptr::null_mut(), ptr::null_mut(), 20);
            }
            if (*player).health <= 10 {
                G_ExitLevel();
            }
        }
        _ => {
            i_error!(
                "P_PlayerInSpecialSector: unknown special {}",
                (*sector).special
            );
        }
    }
}

// ---------------------------------------------------------------------------
// P_UpdateSpecials
// ---------------------------------------------------------------------------

/// Advances all time-based special effects by one tic.
///
/// Called once per game tic from the main thinker loop.  Performs three tasks:
///
/// 1. **Level timer**: if `levelTimer` is set (deathmatch time limit), decrements
///    `levelTimeCount` and calls `G_ExitLevel` when it reaches zero.
///
/// 2. **Texture/flat animation**: iterates the active `anims` table and updates
///    `texturetranslation` or `flattranslation` for each frame in every cycle.
///    The frame index is `(leveltime / speed + i) % numpics`, giving a smooth
///    round-robin across all frames simultaneously (each frame slot maps to a
///    different phase).
///
/// 3. **Scrolling walls** (special 48): increments `textureoffset` by one
///    `FRACUNIT` on the first side of each registered line, producing a
///    continuous left-to-right texture scroll.
///
/// 4. **Timed buttons**: decrements each active button timer; when it expires,
///    restores the original switch texture (top/mid/bottom depending on
///    `buttonlist[i].where_`) and plays the switch sound.
///
/// Called from p_tick.c `P_Ticker`.
pub unsafe fn P_UpdateSpecials() {
    if levelTimer != 0 {
        levelTimeCount -= 1;
        if levelTimeCount == 0 {
            G_ExitLevel();
        }
    }

    let mut anim = std::ptr::addr_of_mut!(anims[0]);
    while anim < lastanim {
        let base = (*anim).basepic;
        let numpics = (*anim).numpics;
        for i in base..base + numpics {
            let pic = base + ((leveltime / (*anim).speed + i) % numpics);
            if (*anim).istexture != 0 {
                *texturetranslation.offset(i as isize) = pic;
            } else {
                *flattranslation.offset(i as isize) = pic;
            }
        }
        anim = anim.offset(1);
    }

    for i in 0..numlinespecials as usize {
        let line = linespeciallist[i];
        if (*line).special as i32 == 48 {
            let sidenum = (*line).sidenum[0] as isize;
            (*sides.offset(sidenum)).textureoffset += FRACUNIT;
        }
    }

    for i in 0..MAXBUTTONS {
        if buttonlist[i].btimer != 0 {
            buttonlist[i].btimer -= 1;
            if buttonlist[i].btimer == 0 {
                let sidenum = (*buttonlist[i].line).sidenum[0] as isize;
                match buttonlist[i].where_ {
                    0 => {
                        (*sides.offset(sidenum)).toptexture = buttonlist[i].btexture as i16;
                    }
                    1 => {
                        (*sides.offset(sidenum)).midtexture = buttonlist[i].btexture as i16;
                    }
                    2 => {
                        (*sides.offset(sidenum)).bottomtexture = buttonlist[i].btexture as i16;
                    }
                    _ => {}
                }
                // Vanilla Doom passes `&buttonlist[i].soundorg`, the address
                // of the field, which plays the click from around map point
                // (0, 0). Play it from the switch's sector as intended.
                S_StartSectorSound(buttonlist[i].soundorg, Sfx::Swtchn as i32);
                buttonlist[i] = std::mem::zeroed();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Donut overrun emulation
// ---------------------------------------------------------------------------

/// Fills `*s3_floorheight` and `*s3_floorpic` with the values that Vanilla Doom
/// would have read from address `0000:0000` when the donut s3 sector is null.
///
/// This replicates a real memory-access bug: in Vanilla Doom on DOS, reading
/// `s3->floorheight` with a null `s3` reads whatever happened to be at the
/// start of the DOS data segment.  The Chocolate Doom approach (which this port
/// follows) is to use configurable default values (`0` and `0x16`) that match
/// the Windows 98 memory layout, overridable with `-donut <height> <pic>`.
///
/// The function initialises its state only on the first call (guarded by the
/// `first` static flag), parsing `-donut` command-line arguments at that point.
/// Subsequent calls return the same cached values.
///
/// - `s3_floorheight`: output - the substitute floor height (fixed-point 16.16).
/// - `s3_floorpic`: output - the substitute floor picture lump index.
/// - `_line`: the triggering linedef (used only for diagnostic output in C;
///   unused in this Rust port).
/// - `_pillar_sector`: the inner donut sector (used only for diagnostics in C;
///   unused in this Rust port).
///
/// # Safety
///
/// `s3_floorheight` and `s3_floorpic` must be valid, non-null, writable
/// pointers. They are written unconditionally on every call.
unsafe fn DonutOverrun(
    s3_floorheight: *mut i32,
    s3_floorpic: *mut i16,
    _line: *mut line_t,
    _pillar_sector: *mut sector_t,
) {
    static mut first: i32 = 1;
    static mut tmp_s3_floorheight: i32 = 0;
    static mut tmp_s3_floorpic: i32 = 0;

    if first != 0 {
        first = 0;
        tmp_s3_floorheight = 0;
        tmp_s3_floorpic = 0x16;

        let p = M_CheckParmWithArgs("-donut", 2);
        if p > 0 {
            if let Some(value) = M_StrToInt(argument(p + 1)) {
                tmp_s3_floorheight = value;
            }
            if let Some(value) = M_StrToInt(argument(p + 2)) {
                tmp_s3_floorpic = value;
            }
            if tmp_s3_floorpic >= numflats {
                eprintln!(
                    "DonutOverrun: The second parameter for \"-donut\" switch should be greater than 0 and less than number of flats ({}). Using default value ({}) instead. ",
                    { numflats }, 0x16
                );
                tmp_s3_floorpic = 0x16;
            }
        }
    }

    *s3_floorheight = tmp_s3_floorheight;
    *s3_floorpic = tmp_s3_floorpic as i16;
}

// ---------------------------------------------------------------------------
// EV_DoDonut
// ---------------------------------------------------------------------------

/// Executes the "donut" special effect for all sectors tagged to `line`.
///
/// The donut effect involves three concentric sectors:
/// - **s1** (inner, pillar): the sector directly tagged by the line.  Its floor
///   lowers to match s3's floor height (`lowerFloor` thinker).
/// - **s2** (ring): the sector adjacent to s1 via s1's first line.  Its floor
///   rises to s3's height and adopts s3's floor texture (`donutRaise` thinker).
/// - **s3** (outer): the sector adjacent to s2 that is not s1; provides the
///   target height and texture.
///
/// Edge cases (matching Chocolate Doom behaviour):
/// - If s1 already has a `specialdata` thinker running, the sector is skipped.
/// - If s2 is null (s1's first line is one-sided), a warning is printed and the
///   loop breaks early without spawning thinkers.
/// - If s3 is null (s2's bounding line has no back sector), `DonutOverrun` is
///   called to supply substitute height and texture values, emulating the
///   vanilla memory overrun.
///
/// Both thinkers are [`ThinkerKind::Floor`] (run by `T_MoveFloor`) and are
/// allocated with `Z_Malloc(PU_LEVSPEC)`.
///
/// Returns 1 if at least one sector was acted upon, 0 otherwise.
///
/// Called from p_spec.c via C when a donut-tagged line is activated.
pub unsafe fn EV_DoDonut(line: *mut line_t) -> i32 {
    let mut secnum = -1;
    let mut rtn = 0;

    while {
        secnum = P_FindSectorFromLineTag(line, secnum);
        secnum
    } >= 0
    {
        let s1 = sectors.offset(secnum as isize);
        if !(*s1).specialdata.is_null() {
            continue;
        }

        rtn = 1;
        let s2 = getNextSector(*(*s1).lines.offset(0), s1);

        if s2.is_null() {
            eprintln!("EV_DoDonut: linedef had no second sidedef! Unexpected behavior may occur in Vanilla Doom. ");
            break;
        }

        for i in 0..(*s2).linecount {
            let s3 = (**(*s2).lines.offset(i as isize)).backsector;

            if s3 == s1 {
                continue;
            }

            let (s3_floorheight, s3_floorpic) = if s3.is_null() {
                eprintln!("EV_DoDonut: WARNING: emulating buffer overrun due to NULL back sector. Unexpected behavior may occur in Vanilla Doom.");
                let mut fh = 0;
                let mut fp = 0i16;
                DonutOverrun(&mut fh, &mut fp, line, s1);
                (fh, fp)
            } else {
                ((*s3).floorheight, (*s3).floorpic)
            };

            // Spawn rising slime
            let floor = Z_Malloc(
                std::mem::size_of::<floormove_t>() as i32,
                PU_LEVSPEC,
                ptr::null_mut(),
            ) as *mut floormove_t;
            P_AddThinker(&raw mut (*floor).thinker);
            (*s2).specialdata = &raw mut (*floor).thinker;
            (*floor).thinker.kind = ThinkerKind::Floor;
            (*floor).kind = donutRaise;
            (*floor).crush = 0;
            (*floor).direction = 1;
            (*floor).sector = s2 as *mut _;
            (*floor).speed = FLOORSPEED / 2;
            (*floor).texture = s3_floorpic;
            (*floor).newspecial = 0;
            (*floor).floordestheight = s3_floorheight;

            // Spawn lowering donut-hole
            let floor = Z_Malloc(
                std::mem::size_of::<floormove_t>() as i32,
                PU_LEVSPEC,
                ptr::null_mut(),
            ) as *mut floormove_t;
            P_AddThinker(&raw mut (*floor).thinker);
            (*s1).specialdata = &raw mut (*floor).thinker;
            (*floor).thinker.kind = ThinkerKind::Floor;
            (*floor).kind = lowerFloor;
            (*floor).crush = 0;
            (*floor).direction = -1;
            (*floor).sector = s1 as *mut _;
            (*floor).speed = FLOORSPEED / 2;
            (*floor).floordestheight = s3_floorheight;

            break;
        }
    }

    rtn
}

// ---------------------------------------------------------------------------
// P_SpawnSpecials
// ---------------------------------------------------------------------------

/// Scans all sectors and linedefs at map load time and spawns thinkers for specials.
///
/// Called once after the map is loaded (from p_spec.c / `G_DoLoadLevel`).
/// Performs the following initialisation:
///
/// 1. **Deathmatch timer**: if `timelimit > 0` and `deathmatch != 0`, sets
///    `levelTimer` and `levelTimeCount` (in tics = `timelimit * 60 * TICRATE`).
///
/// 2. **Sector specials**: iterates all sectors and, for non-zero `sector->special`,
///    spawns the appropriate thinker:
///    - 1: random light flash (`P_SpawnLightFlash`)
///    - 2: fast strobe (`P_SpawnStrobeFlash(FASTDARK, 0)`)
///    - 3: slow strobe (`P_SpawnStrobeFlash(SLOWDARK, 0)`)
///    - 4: fast strobe + death slime (special reset to 4 after spawning strobe)
///    - 8: glowing light (`P_SpawnGlowingLight`)
///    - 9: secret sector (increments `totalsecret`)
///    - 10: door closes in 30 s (`P_SpawnDoorCloseIn30`)
///    - 12: slow sync strobe (`P_SpawnStrobeFlash(SLOWDARK, 1)`)
///    - 13: fast sync strobe (`P_SpawnStrobeFlash(FASTDARK, 1)`)
///    - 14: door raises in 5 min (`P_SpawnDoorRaiseIn5Mins`)
///    - 17: fire flicker (`P_SpawnFireFlicker`)
///
/// 3. **Line specials**: scans all linedefs; lines with special 48 (first-column
///    scroll) are registered in `linespeciallist`.  Aborts with `i_error!` if
///    more than `MAXLINEANIMS` (64) scrolling walls are found.
///
/// 4. **Misc cleanup**: clears `activeceilings`, `activeplats`, and `buttonlist`.
pub unsafe fn P_SpawnSpecials() {
    if timelimit > 0 && deathmatch != 0 {
        levelTimer = 1;
        levelTimeCount = timelimit * 60 * TICRATE;
    } else {
        levelTimer = 0;
    }

    for i in 0..numsectors {
        let sector = sectors.offset(i as isize);
        if (*sector).special == 0 {
            continue;
        }
        match (*sector).special as i32 {
            1 => P_SpawnLightFlash(sector),
            2 => P_SpawnStrobeFlash(sector, FASTDARK, 0),
            3 => P_SpawnStrobeFlash(sector, SLOWDARK, 0),
            4 => {
                P_SpawnStrobeFlash(sector, FASTDARK, 0);
                (*sector).special = 4;
            }
            8 => P_SpawnGlowingLight(sector),
            9 => {
                totalsecret += 1;
            }
            10 => P_SpawnDoorCloseIn30(sector),
            12 => P_SpawnStrobeFlash(sector, SLOWDARK, 1),
            13 => P_SpawnStrobeFlash(sector, FASTDARK, 1),
            14 => P_SpawnDoorRaiseIn5Mins(sector, i),
            17 => P_SpawnFireFlicker(sector),
            _ => {}
        }
    }

    numlinespecials = 0;
    for i in 0..numlines {
        if (*lines.offset(i as isize)).special as i32 == 48 {
            if numlinespecials as i32 >= MAXLINEANIMS as i32 {
                i_error!("Too many scrolling wall linedefs! (Vanilla limit is 64)");
            }
            linespeciallist[numlinespecials as usize] = lines.offset(i as isize);
            numlinespecials += 1;
        }
    }

    for i in 0..MAXCEILINGS {
        crate::engine::play::ceilng::activeceilings[i] = ptr::null_mut();
    }
    for i in 0..MAXPLATS {
        crate::engine::play::plats::activeplats[i] = ptr::null_mut();
    }
    for i in 0..MAXBUTTONS {
        buttonlist[i] = std::mem::zeroed();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use {super::*, std::sync::Mutex};

    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn globals_are_zero_initialized() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert_eq!({ levelTimer }, 0);
            assert_eq!({ levelTimeCount }, 0);
            assert_eq!({ numlinespecials }, 0);
            assert!(lastanim.is_null());
            for (i, &v) in (*&raw const linespeciallist).iter().enumerate() {
                assert!(v.is_null(), "linespeciallist[{i}] should be null");
            }
            for (i, a) in (*&raw const anims).iter().enumerate() {
                assert_eq!(a.istexture, 0, "anims[{i}].istexture should be 0");
                assert_eq!(a.picnum, 0, "anims[{i}].picnum should be 0");
            }
        }
    }

    #[test]
    fn constants_match_c() {
        assert_eq!(MAXANIMS, 32);
        assert_eq!(MAXLINEANIMS, 64);
        assert_eq!(MAX_ADJOINING_SECTORS, 20);
        assert_eq!(PU_LEVSPEC, 6);
        assert_eq!(Sfx::Swtchn as i32, 23);
        assert_eq!(CF_GODMODE, 2);
        assert_eq!(pw_ironfeet, 3);
    }

    #[test]
    fn animation_defs_terminated() {
        let last = ANIMDEFS[ANIMDEFS.len() - 1];
        assert_eq!(last.0, -1);
    }
}
