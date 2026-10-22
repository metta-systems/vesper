//! Switch and button logic ported from `vendor/doomgeneric/p_switch.c`.
//!
//! Manages the switch texture table (`P_InitSwitchList`), timed button
//! reset (`button_t`, `P_StartButton`), switch texture toggling
//! (`P_ChangeSwitchTexture`), and linedef-use dispatch for all switch and
//! button specials (`P_UseSpecialLine`).

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::doom::c_ffi::LinedefFlag;
use crate::doom::sounds::Sfx;
use std::ffi::{c_char, c_int, c_void};
use std::os::raw::c_short;

use crate::doom::d_mode;
use crate::doom::p_ceilng::EV_DoCeiling;
use crate::doom::p_floor::{EV_BuildStairs, EV_DoFloor};
use crate::doom::p_lights::{line_t, EV_LightTurnOn};
use crate::doom::p_plats::EV_DoPlat;
use crate::i_error;

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
pub const BUTTONTIME: c_int = 35;

// bwhere_e values — which sidedef texture slot the switch occupies.

/// Switch is on the upper sidedef texture.
const top: c_int = 0;

/// Switch is on the middle sidedef texture.
const middle: c_int = 1;

/// Switch is on the lower sidedef texture.
const bottom: c_int = 2;

// vldoor_e values — door movement type passed to `EV_DoDoor` / `EV_VerticalDoor`.

/// Normal door: opens then closes after a delay.
const vld_normal: c_int = 0;

/// Door closes immediately.
const vld_close: c_int = 2;

/// Door opens and stays open.
const vld_open: c_int = 3;

/// Blazing door: opens and closes at high speed.
const vld_blazeRaise: c_int = 5;

/// Blazing door: opens at high speed and stays open.
const vld_blazeOpen: c_int = 6;

/// Blazing door: closes at high speed.
const vld_blazeClose: c_int = 7;

// floor_e values — floor movement type passed to `EV_DoFloor`.

/// Lower floor to the highest neighboring floor.
const floor_lowerFloor: c_int = 0;

/// Lower floor to the lowest neighboring floor.
const floor_lowerFloorToLowest: c_int = 1;

/// Lower floor quickly (turbo speed).
const floor_turboLower: c_int = 2;

/// Raise floor to the lowest neighboring ceiling.
const floor_raiseFloor: c_int = 3;

/// Raise floor to the nearest higher floor.
const floor_raiseFloorToNearest: c_int = 4;

/// Raise floor while crushing — stays at the raised height.
const floor_raiseFloorCrush: c_int = 9;

/// Raise floor at turbo speed.
const floor_raiseFloorTurbo: c_int = 10;

/// Raise floor exactly 512 map units.
const floor_raiseFloor512: c_int = 12;

// ceiling_e values — ceiling movement type passed to `EV_DoCeiling`.

/// Lower ceiling to the floor.
const ceiling_lowerToFloor: c_int = 0;

/// Crush-and-raise: ceiling lowers, crushes, then rises repeatedly.
const ceiling_crushAndRaise: c_int = 3;

// plattype_e values — platform movement type passed to `EV_DoPlat`.

/// Platform lowers, waits, then rises and stays.
const plat_downWaitUpStay: c_int = 1;

/// Raise platform and change its texture to match the neighboring floor.
const plat_raiseAndChange: c_int = 2;

/// Raise platform to the nearest higher floor and change texture.
const plat_raiseToNearestAndChange: c_int = 3;

/// Blazing `downWaitUpStay` platform (high-speed version).
const plat_blazeDWUS: c_int = 4;

// stair_e values — stair build type passed to `EV_BuildStairs`.

/// Build stairs with 8-unit step height.
const stair_build8: c_int = 0;

/// Build stairs with 16-unit step height at turbo speed.
const stair_turbo16: c_int = 1;

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
    pub where_: c_int,
    /// Texture number to restore when `btimer` expires.
    pub btexture: c_int,
    /// Remaining tics before the button resets; `0` means this slot is free.
    pub btimer: c_int,
    _pad: [u8; 4],
    /// Pointer to the front-sector sound origin used when the button fires.
    pub soundorg: *mut c_void,
}

#[cfg(target_pointer_width = "64")]
mod layout_checks {
    use super::*;
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
    /// Null-terminated name of the switch-off texture.
    name1: &'static [u8],
    /// Null-terminated name of the switch-on texture.
    name2: &'static [u8],
    /// Minimum episode number (1 = shareware, 2 = registered, 3 = commercial).
    episode: i16,
}

/// Built-in switch texture pairs, mirroring `alphSwitchList[]` in
/// `p_switch.c`.  The list is terminated by an entry with `episode == 0`.
const ALPH_SWITCH_LIST: [SwitchDef; 41] = [
    // Doom shareware episode 1 switches
    SwitchDef {
        name1: b"SW1BRCOM\0",
        name2: b"SW2BRCOM\0",
        episode: 1,
    },
    SwitchDef {
        name1: b"SW1BRN1\0",
        name2: b"SW2BRN1\0",
        episode: 1,
    },
    SwitchDef {
        name1: b"SW1BRN2\0",
        name2: b"SW2BRN2\0",
        episode: 1,
    },
    SwitchDef {
        name1: b"SW1BRNGN\0",
        name2: b"SW2BRNGN\0",
        episode: 1,
    },
    SwitchDef {
        name1: b"SW1BROWN\0",
        name2: b"SW2BROWN\0",
        episode: 1,
    },
    SwitchDef {
        name1: b"SW1COMM\0",
        name2: b"SW2COMM\0",
        episode: 1,
    },
    SwitchDef {
        name1: b"SW1COMP\0",
        name2: b"SW2COMP\0",
        episode: 1,
    },
    SwitchDef {
        name1: b"SW1DIRT\0",
        name2: b"SW2DIRT\0",
        episode: 1,
    },
    SwitchDef {
        name1: b"SW1EXIT\0",
        name2: b"SW2EXIT\0",
        episode: 1,
    },
    SwitchDef {
        name1: b"SW1GRAY\0",
        name2: b"SW2GRAY\0",
        episode: 1,
    },
    SwitchDef {
        name1: b"SW1GRAY1\0",
        name2: b"SW2GRAY1\0",
        episode: 1,
    },
    SwitchDef {
        name1: b"SW1METAL\0",
        name2: b"SW2METAL\0",
        episode: 1,
    },
    SwitchDef {
        name1: b"SW1PIPE\0",
        name2: b"SW2PIPE\0",
        episode: 1,
    },
    SwitchDef {
        name1: b"SW1SLAD\0",
        name2: b"SW2SLAD\0",
        episode: 1,
    },
    SwitchDef {
        name1: b"SW1STARG\0",
        name2: b"SW2STARG\0",
        episode: 1,
    },
    SwitchDef {
        name1: b"SW1STON1\0",
        name2: b"SW2STON1\0",
        episode: 1,
    },
    SwitchDef {
        name1: b"SW1STON2\0",
        name2: b"SW2STON2\0",
        episode: 1,
    },
    SwitchDef {
        name1: b"SW1STONE\0",
        name2: b"SW2STONE\0",
        episode: 1,
    },
    SwitchDef {
        name1: b"SW1STRTN\0",
        name2: b"SW2STRTN\0",
        episode: 1,
    },
    // Doom registered episodes 2&3 switches
    SwitchDef {
        name1: b"SW1BLUE\0",
        name2: b"SW2BLUE\0",
        episode: 2,
    },
    SwitchDef {
        name1: b"SW1CMT\0",
        name2: b"SW2CMT\0",
        episode: 2,
    },
    SwitchDef {
        name1: b"SW1GARG\0",
        name2: b"SW2GARG\0",
        episode: 2,
    },
    SwitchDef {
        name1: b"SW1GSTON\0",
        name2: b"SW2GSTON\0",
        episode: 2,
    },
    SwitchDef {
        name1: b"SW1HOT\0",
        name2: b"SW2HOT\0",
        episode: 2,
    },
    SwitchDef {
        name1: b"SW1LION\0",
        name2: b"SW2LION\0",
        episode: 2,
    },
    SwitchDef {
        name1: b"SW1SATYR\0",
        name2: b"SW2SATYR\0",
        episode: 2,
    },
    SwitchDef {
        name1: b"SW1SKIN\0",
        name2: b"SW2SKIN\0",
        episode: 2,
    },
    SwitchDef {
        name1: b"SW1VINE\0",
        name2: b"SW2VINE\0",
        episode: 2,
    },
    SwitchDef {
        name1: b"SW1WOOD\0",
        name2: b"SW2WOOD\0",
        episode: 2,
    },
    // Doom II switches
    SwitchDef {
        name1: b"SW1PANEL\0",
        name2: b"SW2PANEL\0",
        episode: 3,
    },
    SwitchDef {
        name1: b"SW1ROCK\0",
        name2: b"SW2ROCK\0",
        episode: 3,
    },
    SwitchDef {
        name1: b"SW1MET2\0",
        name2: b"SW2MET2\0",
        episode: 3,
    },
    SwitchDef {
        name1: b"SW1WDMET\0",
        name2: b"SW2WDMET\0",
        episode: 3,
    },
    SwitchDef {
        name1: b"SW1BRIK\0",
        name2: b"SW2BRIK\0",
        episode: 3,
    },
    SwitchDef {
        name1: b"SW1MOD1\0",
        name2: b"SW2MOD1\0",
        episode: 3,
    },
    SwitchDef {
        name1: b"SW1ZIM\0",
        name2: b"SW2ZIM\0",
        episode: 3,
    },
    SwitchDef {
        name1: b"SW1STON6\0",
        name2: b"SW2STON6\0",
        episode: 3,
    },
    SwitchDef {
        name1: b"SW1TEK\0",
        name2: b"SW2TEK\0",
        episode: 3,
    },
    SwitchDef {
        name1: b"SW1MARB\0",
        name2: b"SW2MARB\0",
        episode: 3,
    },
    SwitchDef {
        name1: b"SW1SKULL\0",
        name2: b"SW2SKULL\0",
        episode: 3,
    },
    // terminator
    SwitchDef {
        name1: b"\0",
        name2: b"\0",
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
#[no_mangle]
pub static mut switchlist: [c_int; MAXSWITCHES * 2] = [0; MAXSWITCHES * 2];

/// Number of valid switch pairs in `switchlist` after `P_InitSwitchList`
/// runs.  Zero before initialization.
/// Matches `numswitches` in `p_switch.c`.
#[no_mangle]
pub static mut numswitches: c_int = 0;

/// Ring-buffer of active timed buttons.  Slots with `btimer == 0` are free.
/// Decremented each tic by the thinker subsystem; when a slot's timer
/// reaches zero the original texture is restored.
/// Matches `buttonlist[]` in `p_switch.c`.
#[no_mangle]
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

use crate::doom::doomstat::gamemode;
use crate::doom::g_game::{G_ExitLevel, G_SecretExitLevel};
use crate::doom::p_doors::{EV_DoDoor, EV_DoLockedDoor, EV_VerticalDoor};
use crate::doom::p_setup::sides;
use crate::doom::p_spec::EV_DoDonut;
use crate::doom::r_data::R_TextureNumForName;
use crate::doom::s_sound::S_StartSound;

// Type aliases for cross-module pointer casts (all #[repr(C)] identical layouts).
type CffiLine = crate::doom::c_ffi::line_t;
type CffiMobj = crate::doom::c_ffi::mobj_t;

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
#[no_mangle]
pub extern "C" fn P_InitSwitchList() {
    unsafe {
        let episode: i16 = match gamemode {
            m if m == d_mode::registered || m == d_mode::retail => 2,
            m if m == d_mode::commercial => 3,
            _ => 1,
        };

        let mut index = 0usize;
        for sw in &ALPH_SWITCH_LIST {
            if sw.episode == 0 {
                numswitches = (index / 2) as c_int;
                switchlist[index] = -1;
                break;
            }
            if sw.episode <= episode {
                switchlist[index] = R_TextureNumForName(sw.name1.as_ptr() as *mut c_char);
                index += 1;
                switchlist[index] = R_TextureNumForName(sw.name2.as_ptr() as *mut c_char);
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
#[no_mangle]
pub unsafe extern "C" fn P_StartButton(line: *mut line_t, w: c_int, texture: c_int, time: c_int) {
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
            buttonlist[i].soundorg =
                &(*(*line).frontsector).soundorg as *const [u8; 40] as *mut c_void;
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
#[no_mangle]
pub unsafe extern "C" fn P_ChangeSwitchTexture(line: *mut line_t, useAgain: c_int) {
    if useAgain == 0 {
        (*line).special = 0;
    }

    let sidenum = (*line).sidenum[0] as isize;
    let texTop = (*sides.offset(sidenum)).toptexture;
    let texMid = (*sides.offset(sidenum)).midtexture;
    let texBot = (*sides.offset(sidenum)).bottomtexture;

    let mut sound = Sfx::Swtchn as c_int;

    // EXIT SWITCH?
    if (*line).special == 11 {
        sound = Sfx::Swtchx as c_int;
    }

    for i in 0..(numswitches * 2) as usize {
        if switchlist[i] == texTop as c_int {
            S_StartSound(buttonlist[0].soundorg, sound);
            (*sides.offset(sidenum)).toptexture = switchlist[i ^ 1] as c_short;
            if useAgain != 0 {
                P_StartButton(line, top, switchlist[i], BUTTONTIME);
            }
            return;
        } else if switchlist[i] == texMid as c_int {
            S_StartSound(buttonlist[0].soundorg, sound);
            (*sides.offset(sidenum)).midtexture = switchlist[i ^ 1] as c_short;
            if useAgain != 0 {
                P_StartButton(line, middle, switchlist[i], BUTTONTIME);
            }
            return;
        } else if switchlist[i] == texBot as c_int {
            S_StartSound(buttonlist[0].soundorg, sound);
            (*sides.offset(sidenum)).bottomtexture = switchlist[i ^ 1] as c_short;
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
#[no_mangle]
pub unsafe extern "C" fn P_UseSpecialLine(
    thing: *mut c_void,
    line: *mut line_t,
    side: c_int,
) -> c_int {
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
    let mobj = thing as *mut crate::doom::c_ffi::mobj_t;
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
            EV_VerticalDoor(line, thing as *mut CffiMobj);
        }

        // SWITCHES
        7 if EV_BuildStairs(line, stair_build8) != 0 => {
            P_ChangeSwitchTexture(line, 0);
        }
        9 if EV_DoDonut(line as *mut CffiLine) != 0 => {
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
        133 | 135 | 137 if EV_DoLockedDoor(line, vld_blazeOpen, thing as *mut CffiMobj) != 0 => {
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
        99 | 134 | 136 if EV_DoLockedDoor(line, vld_blazeOpen, thing as *mut CffiMobj) != 0 => {
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
// Link anchor
// ---------------------------------------------------------------------------

/// Ensures all public symbols in this module are retained by the linker.
///
/// Not intended for direct use in game logic.
///
/// # Safety
///
/// Accesses function pointers as raw integers purely to prevent dead-code
/// elimination; no actual function calls are made.
#[no_mangle]
pub unsafe extern "C" fn P_Switch_Link_Anchor() {
    let _ = P_InitSwitchList as *const () as usize;
    let _ = P_StartButton as *const () as usize;
    let _ = P_ChangeSwitchTexture as *const () as usize;
    let _ = P_UseSpecialLine as *const () as usize;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

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
            assert_eq!(numswitches, 0);
            for (i, &v) in switchlist.iter().enumerate() {
                assert_eq!(v, 0, "switchlist[{i}] should be 0 before P_InitSwitchList");
            }
            for (i, b) in buttonlist.iter().enumerate() {
                assert!(b.line.is_null(), "buttonlist[{i}].line should be null");
                assert_eq!(b.btimer, 0, "buttonlist[{i}].btimer should be 0");
            }
        }
    }

    #[test]
    fn switchlist_length_is_maxswitches_times_2() {
        unsafe {
            assert_eq!(switchlist.len(), MAXSWITCHES * 2);
            assert_eq!(switchlist.len(), 100);
        }
    }
}
