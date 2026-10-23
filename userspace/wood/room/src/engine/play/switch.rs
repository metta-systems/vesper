//! Switch and button logic ported from `vendor/doomgeneric/p_switch.c`.
//!
//! Manages the switch texture table (`P_InitSwitchList`), timed button
//! reset (`button_t`, `P_StartButton`), switch texture toggling
//! (`P_ChangeSwitchTexture`), and linedef-use dispatch for all switch and
//! button specials (`P_UseSpecialLine`).

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use {
    crate::engine::{sound::info::Sfx, wad::format::LinedefFlag},
    std::ffi::c_void,
};

use crate::{
    engine::{
        doom,
        map::{degenmobj_t, line_t},
        play::{
            ceilng::EV_DoCeiling,
            floor::{EV_BuildStairs, EV_DoFloor},
            lights::EV_LightTurnOn,
            mobj::mobj_t,
            plats::EV_DoPlat,
        },
    },
    i_error,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of switch pairs that can be registered at once.
/// Matches `MAXSWITCHES` in `doomdef.h`.
pub const MAXSWITCHES: usize = 50;

/// Maximum number of simultaneously active timed buttons.
/// Matches `MAXBUTTONS` in `p_local.h`.
pub const MAXBUTTONS: usize = 16;

/// Duration of a button press in game tics (1 second at 35 tics/s).
/// Matches `BUTTONTIME` in `p_switch.c`.
pub const BUTTONTIME: i32 = 35;

// bwhere_e values — which sidedef texture slot the switch occupies.

/// Switch is on the upper sidedef texture.
const top: i32 = 0;

/// Switch is on the middle sidedef texture.
const middle: i32 = 1;

/// Switch is on the lower sidedef texture.
const bottom: i32 = 2;

// vldoor_e values — door movement type passed to `EV_DoDoor` / `EV_VerticalDoor`.

/// Normal door: opens then closes after a delay.
const vld_normal: i32 = 0;

/// Door closes immediately.
const vld_close: i32 = 2;

/// Door opens and stays open.
const vld_open: i32 = 3;

/// Blazing door: opens and closes at high speed.
const vld_blazeRaise: i32 = 5;

/// Blazing door: opens at high speed and stays open.
const vld_blazeOpen: i32 = 6;

/// Blazing door: closes at high speed.
const vld_blazeClose: i32 = 7;

// floor_e values — floor movement type passed to `EV_DoFloor`.

/// Lower floor to the highest neighboring floor.
const floor_lowerFloor: i32 = 0;

/// Lower floor to the lowest neighboring floor.
const floor_lowerFloorToLowest: i32 = 1;

/// Lower floor quickly (turbo speed).
const floor_turboLower: i32 = 2;

/// Raise floor to the lowest neighboring ceiling.
const floor_raiseFloor: i32 = 3;

/// Raise floor to the nearest higher floor.
const floor_raiseFloorToNearest: i32 = 4;

/// Raise floor while crushing — stays at the raised height.
const floor_raiseFloorCrush: i32 = 9;

/// Raise floor at turbo speed.
const floor_raiseFloorTurbo: i32 = 10;

/// Raise floor exactly 512 map units.
const floor_raiseFloor512: i32 = 12;

// ceiling_e values — ceiling movement type passed to `EV_DoCeiling`.

/// Lower ceiling to the floor.
const ceiling_lowerToFloor: i32 = 0;

/// Crush-and-raise: ceiling lowers, crushes, then rises repeatedly.
const ceiling_crushAndRaise: i32 = 3;

// plattype_e values — platform movement type passed to `EV_DoPlat`.

/// Platform lowers, waits, then rises and stays.
const plat_downWaitUpStay: i32 = 1;

/// Raise platform and change its texture to match the neighboring floor.
const plat_raiseAndChange: i32 = 2;

/// Raise platform to the nearest higher floor and change texture.
const plat_raiseToNearestAndChange: i32 = 3;

/// Blazing `downWaitUpStay` platform (high-speed version).
const plat_blazeDWUS: i32 = 4;

// stair_e values — stair build type passed to `EV_BuildStairs`.

/// Build stairs with 8-unit step height.
const stair_build8: i32 = 0;

/// Build stairs with 16-unit step height at turbo speed.
const stair_turbo16: i32 = 1;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A timed button entry: records which linedef was pressed, which texture
/// slot it occupies, the original texture to restore, and how many tics
/// remain before the button resets.
///
/// The layout is verified at compile time to match the C `button_t` struct
/// (32 bytes on 64-bit targets).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct button_t {
    /// Pointer to the linedef whose switch texture was activated.
    pub line: *mut line_t,
    /// Which sidedef texture slot the button lives in (`top`, `middle`, or `bottom`).
    pub where_: i32,
    /// Texture number to restore when `btimer` expires.
    pub btexture: i32,
    /// Remaining tics before the button resets; `0` means this slot is free.
    pub btimer: i32,
    _pad: [u8; 4],
    /// Pointer to the front-sector sound origin used when the button fires.
    pub soundorg: *mut degenmobj_t,
}

#[cfg(target_pointer_width = "64")]
mod layout_checks {
    use super::*;
    // `P_UpdateSpecials` passes the address of `soundorg` itself as the sound
    // origin, so the sound code reads `degenmobj_t::x`/`y` from the bytes
    // that follow it: the next slot's `btimer` and padding.
    const _: () = assert!(std::mem::offset_of!(degenmobj_t, x) == 24);
    const _: () = assert!(std::mem::offset_of!(degenmobj_t, y) == 28);
    const _: () = assert!(std::mem::size_of::<button_t>() == 32);
    const _: () = assert!(std::mem::offset_of!(button_t, line) == 0);
    const _: () = assert!(std::mem::offset_of!(button_t, where_) == 8);
    const _: () = assert!(std::mem::offset_of!(button_t, btexture) == 12);
    const _: () = assert!(std::mem::offset_of!(button_t, btimer) == 16);
    const _: () = assert!(std::mem::offset_of!(button_t, soundorg) == 24);
}

// ---------------------------------------------------------------------------
// Data tables
// ---------------------------------------------------------------------------

/// A single entry in the built-in switch-texture table.
///
/// Each entry names the "off" texture (`name1`) and "on" texture (`name2`)
/// and the minimum episode number required for the pair to be loaded.
/// Corresponds to `switchlist_t` in `p_switch.c`.
struct SwitchDef {
    /// Name of the switch-off texture.
    name1: &'static str,
    /// Name of the switch-on texture.
    name2: &'static str,
    /// Minimum episode number (1 = shareware, 2 = registered, 3 = commercial).
    episode: i16,
}

/// Built-in switch texture pairs, mirroring `alphSwitchList[]` in
/// `p_switch.c`.  The list is terminated by an entry with `episode == 0`.
const ALPH_SWITCH_LIST: [SwitchDef; 41] = [
    // Doom shareware episode 1 switches
    SwitchDef {
        name1: "SW1BRCOM",
        name2: "SW2BRCOM",
        episode: 1,
    },
    SwitchDef {
        name1: "SW1BRN1",
        name2: "SW2BRN1",
        episode: 1,
    },
    SwitchDef {
        name1: "SW1BRN2",
        name2: "SW2BRN2",
        episode: 1,
    },
    SwitchDef {
        name1: "SW1BRNGN",
        name2: "SW2BRNGN",
        episode: 1,
    },
    SwitchDef {
        name1: "SW1BROWN",
        name2: "SW2BROWN",
        episode: 1,
    },
    SwitchDef {
        name1: "SW1COMM",
        name2: "SW2COMM",
        episode: 1,
    },
    SwitchDef {
        name1: "SW1COMP",
        name2: "SW2COMP",
        episode: 1,
    },
    SwitchDef {
        name1: "SW1DIRT",
        name2: "SW2DIRT",
        episode: 1,
    },
    SwitchDef {
        name1: "SW1EXIT",
        name2: "SW2EXIT",
        episode: 1,
    },
    SwitchDef {
        name1: "SW1GRAY",
        name2: "SW2GRAY",
        episode: 1,
    },
    SwitchDef {
        name1: "SW1GRAY1",
        name2: "SW2GRAY1",
        episode: 1,
    },
    SwitchDef {
        name1: "SW1METAL",
        name2: "SW2METAL",
        episode: 1,
    },
    SwitchDef {
        name1: "SW1PIPE",
        name2: "SW2PIPE",
        episode: 1,
    },
    SwitchDef {
        name1: "SW1SLAD",
        name2: "SW2SLAD",
        episode: 1,
    },
    SwitchDef {
        name1: "SW1STARG",
        name2: "SW2STARG",
        episode: 1,
    },
    SwitchDef {
        name1: "SW1STON1",
        name2: "SW2STON1",
        episode: 1,
    },
    SwitchDef {
        name1: "SW1STON2",
        name2: "SW2STON2",
        episode: 1,
    },
    SwitchDef {
        name1: "SW1STONE",
        name2: "SW2STONE",
        episode: 1,
    },
    SwitchDef {
        name1: "SW1STRTN",
        name2: "SW2STRTN",
        episode: 1,
    },
    // Doom registered episodes 2&3 switches
    SwitchDef {
        name1: "SW1BLUE",
        name2: "SW2BLUE",
        episode: 2,
    },
    SwitchDef {
        name1: "SW1CMT",
        name2: "SW2CMT",
        episode: 2,
    },
    SwitchDef {
        name1: "SW1GARG",
        name2: "SW2GARG",
        episode: 2,
    },
    SwitchDef {
        name1: "SW1GSTON",
        name2: "SW2GSTON",
        episode: 2,
    },
    SwitchDef {
        name1: "SW1HOT",
        name2: "SW2HOT",
        episode: 2,
    },
    SwitchDef {
        name1: "SW1LION",
        name2: "SW2LION",
        episode: 2,
    },
    SwitchDef {
        name1: "SW1SATYR",
        name2: "SW2SATYR",
        episode: 2,
    },
    SwitchDef {
        name1: "SW1SKIN",
        name2: "SW2SKIN",
        episode: 2,
    },
    SwitchDef {
        name1: "SW1VINE",
        name2: "SW2VINE",
        episode: 2,
    },
    SwitchDef {
        name1: "SW1WOOD",
        name2: "SW2WOOD",
        episode: 2,
    },
    // Doom II switches
    SwitchDef {
        name1: "SW1PANEL",
        name2: "SW2PANEL",
        episode: 3,
    },
    SwitchDef {
        name1: "SW1ROCK",
        name2: "SW2ROCK",
        episode: 3,
    },
    SwitchDef {
        name1: "SW1MET2",
        name2: "SW2MET2",
        episode: 3,
    },
    SwitchDef {
        name1: "SW1WDMET",
        name2: "SW2WDMET",
        episode: 3,
    },
    SwitchDef {
        name1: "SW1BRIK",
        name2: "SW2BRIK",
        episode: 3,
    },
    SwitchDef {
        name1: "SW1MOD1",
        name2: "SW2MOD1",
        episode: 3,
    },
    SwitchDef {
        name1: "SW1ZIM",
        name2: "SW2ZIM",
        episode: 3,
    },
    SwitchDef {
        name1: "SW1STON6",
        name2: "SW2STON6",
        episode: 3,
    },
    SwitchDef {
        name1: "SW1TEK",
        name2: "SW2TEK",
        episode: 3,
    },
    SwitchDef {
        name1: "SW1MARB",
        name2: "SW2MARB",
        episode: 3,
    },
    SwitchDef {
        name1: "SW1SKULL",
        name2: "SW2SKULL",
        episode: 3,
    },
    // terminator
    SwitchDef {
        name1: "",
        name2: "",
        episode: 0,
    },
];

// ---------------------------------------------------------------------------
// Globals
// ---------------------------------------------------------------------------

/// Flat array of texture-number pairs filled by `P_InitSwitchList`.
/// Stored as alternating (off-texture, on-texture) pairs; valid length is
/// `numswitches * 2`.  The sentinel `switchlist[numswitches * 2] == -1`
/// marks the end of valid data.
/// Matches `switchlist[]` in `p_switch.c`.
pub static mut switchlist: [i32; MAXSWITCHES * 2] = [0; MAXSWITCHES * 2];

/// Number of valid switch pairs in `switchlist` after `P_InitSwitchList`
/// runs.  Zero before initialization.
/// Matches `numswitches` in `p_switch.c`.
pub static mut numswitches: i32 = 0;

/// Ring-buffer of active timed buttons.  Slots with `btimer == 0` are free.
/// Decremented each tic by the thinker subsystem; when a slot's timer
/// reaches zero the original texture is restored.
/// Matches `buttonlist[]` in `p_switch.c`.
pub static mut buttonlist: [button_t; MAXBUTTONS] = [button_t {
    line: std::ptr::null_mut(),
    where_: 0,
    btexture: 0,
    btimer: 0,
    _pad: [0; 4],
    soundorg: std::ptr::null_mut(),
}; MAXBUTTONS];

// ---------------------------------------------------------------------------
// External declarations (unported C functions)
// ---------------------------------------------------------------------------

use crate::engine::{
    doom::stat::gamemode,
    game::{G_ExitLevel, G_SecretExitLevel},
    play::{
        doors::{EV_DoDoor, EV_DoLockedDoor, EV_VerticalDoor},
        setup::sides,
        spec::EV_DoDonut,
    },
    render_3d::data::R_TextureNumForName,
    sound::S_StartSectorSound,
};

// ---------------------------------------------------------------------------
// P_InitSwitchList
// ---------------------------------------------------------------------------

/// Build the runtime switch-pair table from the built-in `ALPH_SWITCH_LIST`.
///
/// Should be called once during level initialization.  The episode number
/// is derived from `gamemode`: shareware uses episode 1, registered/retail
/// use episode 2, and commercial uses episode 3.  Only pairs whose
/// `episode` field is <= the current episode are included.
///
/// Populates `switchlist` with alternating texture-number pairs and sets
/// `numswitches`.  A sentinel value of `-1` is written after the last valid
/// pair.
///
/// # FIXME
///
/// The C source (`p_switch.c` line 138-139) wraps each texture name with
/// `DEH_String()` before passing it to `R_TextureNumForName`, allowing
/// DEHacked patches to rename switch textures.  This port omits that
/// wrapper because `FEATURE_DEHACKED` is disabled.
pub fn P_InitSwitchList() {
    unsafe {
        let episode: i16 = match gamemode {
            m if m == doom::mode::registered || m == doom::mode::retail => 2,
            m if m == doom::mode::commercial => 3,
            _ => 1,
        };

        let mut index = 0usize;
        for sw in &ALPH_SWITCH_LIST {
            if sw.episode == 0 {
                numswitches = (index / 2) as i32;
                switchlist[index] = -1;
                break;
            }
            if sw.episode <= episode {
                switchlist[index] = R_TextureNumForName(sw.name1.as_bytes());
                index += 1;
                switchlist[index] = R_TextureNumForName(sw.name2.as_bytes());
                index += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// P_StartButton
// ---------------------------------------------------------------------------

/// Register a timed button that will reset after `time` tics.
///
/// Scans `buttonlist` for an existing active slot for `line`; if one is
/// found the call is a no-op (the button is already pressed).  Otherwise
/// the first free slot (`btimer == 0`) is filled.  If no slot is available
/// `I_Error` is called.
///
/// - `w` — which sidedef texture the button occupies (`top`, `middle`, or
///   `bottom`).
/// - `texture` — original texture number to restore on expiry.
/// - `time` — countdown in tics; typically `BUTTONTIME` (35).
///
/// # Safety
///
/// `line` must be a valid, non-null pointer to a live `line_t`.
/// `buttonlist` must only be accessed from the game-logic thread.
pub unsafe fn P_StartButton(line: *mut line_t, w: i32, texture: i32, time: i32) {
    // See if button is already pressed
    for i in 0..MAXBUTTONS {
        if buttonlist[i].btimer != 0 && buttonlist[i].line == line {
            return;
        }
    }

    for i in 0..MAXBUTTONS {
        if buttonlist[i].btimer == 0 {
            buttonlist[i].line = line;
            buttonlist[i].where_ = w;
            buttonlist[i].btexture = texture;
            buttonlist[i].btimer = time;
            buttonlist[i].soundorg = &raw mut (*(*line).frontsector).soundorg;
            return;
        }
    }

    i_error!("P_StartButton: no button slots left!");
}

// ---------------------------------------------------------------------------
// P_ChangeSwitchTexture
// ---------------------------------------------------------------------------

/// Toggle the switch texture on the front side of `line` and, if `useAgain`
/// is non-zero, queue a timed button to restore it after `BUTTONTIME` tics.
///
/// Reads the top, middle, and bottom textures of the front sidedef and
/// searches `switchlist` for a match.  When a match is found the texture is
/// flipped to its partner (`switchlist[i ^ 1]`), a switch sound is played
/// from `buttonlist[0].soundorg`, and the function returns.
///
/// If `useAgain == 0` the line's special is cleared to make the switch
/// one-shot.  Exit switches (special 11) play `sfx_swtchx` instead of the
/// normal `sfx_swtchn`.
///
/// # Safety
///
/// `line` must be a valid, non-null pointer to a live `line_t`.  The global
/// `switchlist`, `numswitches`, `buttonlist`, and `sides` arrays must only
/// be accessed from the game-logic thread.
pub unsafe fn P_ChangeSwitchTexture(line: *mut line_t, useAgain: i32) {
    if useAgain == 0 {
        (*line).special = 0;
    }

    let sidenum = (*line).sidenum[0] as isize;
    let texTop = (*sides.offset(sidenum)).toptexture;
    let texMid = (*sides.offset(sidenum)).midtexture;
    let texBot = (*sides.offset(sidenum)).bottomtexture;

    let mut sound = Sfx::Swtchn as i32;

    // EXIT SWITCH?
    if (*line).special == 11 {
        sound = Sfx::Swtchx as i32;
    }

    for i in 0..(numswitches * 2) as usize {
        if switchlist[i] == texTop as i32 {
            S_StartSectorSound(buttonlist[0].soundorg, sound);
            (*sides.offset(sidenum)).toptexture = switchlist[i ^ 1] as i16;
            if useAgain != 0 {
                P_StartButton(line, top, switchlist[i], BUTTONTIME);
            }
            return;
        } else if switchlist[i] == texMid as i32 {
            S_StartSectorSound(buttonlist[0].soundorg, sound);
            (*sides.offset(sidenum)).midtexture = switchlist[i ^ 1] as i16;
            if useAgain != 0 {
                P_StartButton(line, middle, switchlist[i], BUTTONTIME);
            }
            return;
        } else if switchlist[i] == texBot as i32 {
            S_StartSectorSound(buttonlist[0].soundorg, sound);
            (*sides.offset(sidenum)).bottomtexture = switchlist[i ^ 1] as i16;
            if useAgain != 0 {
                P_StartButton(line, bottom, switchlist[i], BUTTONTIME);
            }
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// P_UseSpecialLine
// ---------------------------------------------------------------------------

/// Dispatch a player or monster Use action on a special linedef.
///
/// Called by `P_UseLines` when `thing` presses Use against `line`.  `side`
/// is `0` for the front face (the normal case) or `1` for the back face.
/// Using the back side is only allowed for special 124 (unused sliding
/// door).
///
/// Non-player actors can only activate lines that are not secret and have
/// one of the four manual-door specials (1, 32, 33, 34).
///
/// The main dispatch covers:
/// - Manual doors (specials 1, 26-28, 31-34, 117-118): calls
///   `EV_VerticalDoor` directly with no texture change.
/// - One-shot switches (specials 7-140): perform the action, then call
///   `P_ChangeSwitchTexture(line, 0)` to flip the texture permanently.
/// - Repeatable buttons (specials 42-139): perform the action, then call
///   `P_ChangeSwitchTexture(line, 1)` to flip and queue a reset timer.
///
/// Returns `1` (true) after handling any special; unrecognised specials
/// fall through silently and also return `1`.
///
/// # Safety
///
/// `thing` must be a valid, non-null pointer to a `mobj_t`.  `line` must
/// be a valid, non-null pointer to a live `line_t`.  All global game-state
/// statics must only be accessed from the game-logic thread.
pub unsafe fn P_UseSpecialLine(thing: *mut c_void, line: *mut line_t, side: i32) -> i32 {
    // Err...
    // Use the back sides of VERY SPECIAL lines...
    if side != 0 {
        match (*line).special {
            124 => {
                // Sliding door open&close
                // UNUSED?
            }
            _ => return 0,
        }
    }

    // Switches that other things can activate.
    let mobj = thing as *mut mobj_t;
    if (*mobj).player.is_null() {
        // never open secret doors
        if (*line).flags & LinedefFlag::SECRET as i16 != 0 {
            return 0;
        }
        match (*line).special {
            1 | 32 | 33 | 34 => {}
            _ => return 0,
        }
    }

    // do something
    match (*line).special {
        // MANUALS
        1 | 26 | 27 | 28 | 31 | 32 | 33 | 34 | 117 | 118 => {
            EV_VerticalDoor(line, thing as *mut mobj_t);
        }

        // SWITCHES
        7 if EV_BuildStairs(line, stair_build8) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        9 if EV_DoDonut(line) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        11 => {
            P_ChangeSwitchTexture(line, 0);
            G_ExitLevel();
        }
        14 if EV_DoPlat(line, plat_raiseAndChange, 32) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        15 if EV_DoPlat(line, plat_raiseAndChange, 24) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        18 if EV_DoFloor(line, floor_raiseFloorToNearest) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        20 if EV_DoPlat(line, plat_raiseToNearestAndChange, 0) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        21 if EV_DoPlat(line, plat_downWaitUpStay, 0) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        23 if EV_DoFloor(line, floor_lowerFloorToLowest) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        29 if EV_DoDoor(line, vld_normal) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        41 if EV_DoCeiling(line, ceiling_lowerToFloor) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        71 if EV_DoFloor(line, floor_turboLower) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        49 if EV_DoCeiling(line, ceiling_crushAndRaise) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        50 if EV_DoDoor(line, vld_close) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        51 => {
            P_ChangeSwitchTexture(line, 0);
            G_SecretExitLevel();
        }
        55 if EV_DoFloor(line, floor_raiseFloorCrush) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        101 if EV_DoFloor(line, floor_raiseFloor) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        102 if EV_DoFloor(line, floor_lowerFloor) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        103 if EV_DoDoor(line, vld_open) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        111 if EV_DoDoor(line, vld_blazeRaise) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        112 if EV_DoDoor(line, vld_blazeOpen) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        113 if EV_DoDoor(line, vld_blazeClose) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        122 if EV_DoPlat(line, plat_blazeDWUS, 0) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        127 if EV_BuildStairs(line, stair_turbo16) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        131 if EV_DoFloor(line, floor_raiseFloorTurbo) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        133 | 135 | 137 if EV_DoLockedDoor(line, vld_blazeOpen, thing as *mut mobj_t) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        140 if EV_DoFloor(line, floor_raiseFloor512) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }

        // BUTTONS
        42 if EV_DoDoor(line, vld_close) != 0 => {
            P_ChangeSwitchTexture(line, 1);
        }
        43 if EV_DoCeiling(line, ceiling_lowerToFloor) != 0 => {
            P_ChangeSwitchTexture(line, 1);
        }
        45 if EV_DoFloor(line, floor_lowerFloor) != 0 => {
            P_ChangeSwitchTexture(line, 1);
        }
        60 if EV_DoFloor(line, floor_lowerFloorToLowest) != 0 => {
            P_ChangeSwitchTexture(line, 1);
        }
        61 if EV_DoDoor(line, vld_open) != 0 => {
            P_ChangeSwitchTexture(line, 1);
        }
        62 if EV_DoPlat(line, plat_downWaitUpStay, 1) != 0 => {
            P_ChangeSwitchTexture(line, 1);
        }
        63 if EV_DoDoor(line, vld_normal) != 0 => {
            P_ChangeSwitchTexture(line, 1);
        }
        64 if EV_DoFloor(line, floor_raiseFloor) != 0 => {
            P_ChangeSwitchTexture(line, 1);
        }
        66 if EV_DoPlat(line, plat_raiseAndChange, 24) != 0 => {
            P_ChangeSwitchTexture(line, 1);
        }
        67 if EV_DoPlat(line, plat_raiseAndChange, 32) != 0 => {
            P_ChangeSwitchTexture(line, 1);
        }
        65 if EV_DoFloor(line, floor_raiseFloorCrush) != 0 => {
            P_ChangeSwitchTexture(line, 1);
        }
        68 if EV_DoPlat(line, plat_raiseToNearestAndChange, 0) != 0 => {
            P_ChangeSwitchTexture(line, 1);
        }
        69 if EV_DoFloor(line, floor_raiseFloorToNearest) != 0 => {
            P_ChangeSwitchTexture(line, 1);
        }
        70 if EV_DoFloor(line, floor_turboLower) != 0 => {
            P_ChangeSwitchTexture(line, 1);
        }
        114 if EV_DoDoor(line, vld_blazeRaise) != 0 => {
            P_ChangeSwitchTexture(line, 1);
        }
        115 if EV_DoDoor(line, vld_blazeOpen) != 0 => {
            P_ChangeSwitchTexture(line, 1);
        }
        116 if EV_DoDoor(line, vld_blazeClose) != 0 => {
            P_ChangeSwitchTexture(line, 1);
        }
        123 if EV_DoPlat(line, plat_blazeDWUS, 0) != 0 => {
            P_ChangeSwitchTexture(line, 1);
        }
        132 if EV_DoFloor(line, floor_raiseFloorTurbo) != 0 => {
            P_ChangeSwitchTexture(line, 1);
        }
        99 | 134 | 136 if EV_DoLockedDoor(line, vld_blazeOpen, thing as *mut mobj_t) != 0 => {
            P_ChangeSwitchTexture(line, 1);
        }
        138 => {
            EV_LightTurnOn(line, 255);
            P_ChangeSwitchTexture(line, 1);
        }
        139 => {
            EV_LightTurnOn(line, 35);
            P_ChangeSwitchTexture(line, 1);
        }

        _ => {}
    }

    1 // true
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use {super::*, std::sync::Mutex};

    static LOCK: Mutex<()> = Mutex::new(());

    const BUTTON_T_SIZEOF: usize = 32;
    const BUTTON_T_LINE: usize = 0;
    const BUTTON_T_WHERE: usize = 8;
    const BUTTON_T_BTEXTURE: usize = 12;
    const BUTTON_T_BTIMER: usize = 16;
    const BUTTON_T_SOUNDORG: usize = 24;

    #[test]
    fn button_t_layout_matches_c() {
        let _g = LOCK.lock().unwrap();
        assert_eq!(std::mem::size_of::<button_t>(), BUTTON_T_SIZEOF);
        assert_eq!(std::mem::offset_of!(button_t, line), BUTTON_T_LINE);
        assert_eq!(std::mem::offset_of!(button_t, where_), BUTTON_T_WHERE);
        assert_eq!(std::mem::offset_of!(button_t, btexture), BUTTON_T_BTEXTURE);
        assert_eq!(std::mem::offset_of!(button_t, btimer), BUTTON_T_BTIMER);
        assert_eq!(std::mem::offset_of!(button_t, soundorg), BUTTON_T_SOUNDORG);
    }

    #[test]
    fn constants_match_c() {
        assert_eq!(MAXSWITCHES, 50);
        assert_eq!(MAXBUTTONS, 16);
        assert_eq!(BUTTONTIME, 35);
        assert_eq!(top, 0);
        assert_eq!(middle, 1);
        assert_eq!(bottom, 2);
    }

    #[test]
    fn globals_are_zero_initialized() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert_eq!({ numswitches }, 0);
            for (i, &v) in (*&raw const switchlist).iter().enumerate() {
                assert_eq!(v, 0, "switchlist[{i}] should be 0 before P_InitSwitchList");
            }
            for (i, b) in (*&raw const buttonlist).iter().enumerate() {
                assert!(b.line.is_null(), "buttonlist[{i}].line should be null");
                assert_eq!(b.btimer, 0, "buttonlist[{i}].btimer should be 0");
            }
        }
    }

    #[test]
    fn switchlist_length_is_maxswitches_times_2() {
        unsafe {
            assert_eq!((*&raw const switchlist).len(), MAXSWITCHES * 2);
            assert_eq!((*&raw const switchlist).len(), 100);
        }
    }
}
