//! Switch and button logic ported from `vendor/doomgeneric/p_switch.c`.
//!
//! Manages the switch texture table (`P_InitSwitchList`), timed button
//! reset (`button_t`, `P_StartButton`), switch texture toggling
//! (`P_ChangeSwitchTexture`), and linedef-use dispatch for all switch and
//! button specials (`P_UseSpecialLine`).

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::engine::{sound::info::Sfx, wad::format::LinedefFlag};

use crate::{
    engine::{
        doom,
        global::Global,
        map::{LineId, SectorId},
        play::{
            ceilng::EV_DoCeiling,
            floor::{EV_BuildStairs, EV_DoFloor},
            lights::EV_LightTurnOn,
            mobj::MobjId,
            plats::EV_DoPlat,
            tick::MOBJ_ARENA,
        },
        world::World,
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
#[derive(Clone, Copy)]
pub struct button_t {
    /// The linedef whose switch texture was activated; `None` in a slot
    /// that was never used.
    pub line: Option<LineId>,
    /// Which sidedef texture slot the button lives in (`top`, `middle`, or `bottom`).
    pub where_: i32,
    /// Texture number to restore when `btimer` expires.
    pub btexture: i32,
    /// Remaining tics before the button resets; `0` means this slot is free.
    pub btimer: i32,
    /// The front sector whose sound origin is used when the button fires
    /// (C keeps a pointer to its `soundorg`); `None` (C: null) plays the
    /// sound non-positionally.
    pub soundorg: Option<SectorId>,
}

impl button_t {
    /// A free, never-used slot (C's zeroed `button_t`).
    pub const EMPTY: Self = Self {
        line: None,
        where_: 0,
        btexture: 0,
        btimer: 0,
        soundorg: None,
    };
}

/// Play `sound` from a button's sound origin: from that sector, or
/// non-positionally where C passes a null origin.
pub fn start_button_sound(world: &World, soundorg: Option<SectorId>, sound: i32) {
    match soundorg {
        Some(sector) => S_StartSectorSound(world, sector, sound),
        None => S_StartNonPositionalSound(sound),
    }
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

/// The switch texture pairs (`p_switch.c` globals; the level's timed-button
/// table `buttonlist` is in
/// [`ActiveSpecials`](crate::engine::world::ActiveSpecials)).
pub struct SwitchState {
    /// Flat array of texture-number pairs filled by `P_InitSwitchList`.
    /// Stored as alternating (off-texture, on-texture) pairs; valid length is
    /// `numswitches * 2`.  The sentinel `switchlist[numswitches * 2] == -1`
    /// marks the end of valid data.
    /// Matches `switchlist[]` in `p_switch.c`.
    pub switchlist: [i32; MAXSWITCHES * 2],
    /// Number of valid switch pairs in `switchlist` after `P_InitSwitchList`
    /// runs.  Zero before initialization.
    /// Matches `numswitches` in `p_switch.c`.
    pub numswitches: i32,
}

impl SwitchState {
    /// No switch pairs loaded.
    pub const fn new() -> Self {
        Self {
            switchlist: [0; MAXSWITCHES * 2],
            numswitches: 0,
        }
    }
}

impl Default for SwitchState {
    fn default() -> Self {
        Self::new()
    }
}

/// The switch texture pairs.
pub static SWITCHES: Global<SwitchState> = Global::new(SwitchState::new());

// ---------------------------------------------------------------------------
// External declarations (unported C functions)
// ---------------------------------------------------------------------------

use crate::engine::{
    doom::stat::gamemode,
    game::{G_ExitLevel, G_SecretExitLevel},
    play::{
        doors::{EV_DoDoor, EV_DoLockedDoor, EV_VerticalDoor},
        spec::EV_DoDonut,
    },
    render_3d::data::R_TextureNumForName,
    sound::{S_StartNonPositionalSound, S_StartSectorSound},
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
        let episode: i16 = match gamemode() {
            m if m == doom::mode::registered || m == doom::mode::retail => 2,
            m if m == doom::mode::commercial => 3,
            _ => 1,
        };

        let mut switches = SWITCHES.borrow_mut();
        let mut index = 0usize;
        for sw in &ALPH_SWITCH_LIST {
            if sw.episode == 0 {
                switches.numswitches = (index / 2) as i32;
                switches.switchlist[index] = -1;
                break;
            }
            if sw.episode <= episode {
                switches.switchlist[index] = R_TextureNumForName(sw.name1.as_bytes());
                index += 1;
                switches.switchlist[index] = R_TextureNumForName(sw.name2.as_bytes());
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
/// `line` must be a linedef of the loaded level.
pub fn P_StartButton(world: &mut World, line: LineId, w: i32, texture: i32, time: i32) {
    {
        let World { level, active, .. } = world;
        let buttonlist = &mut active.buttons;

        // See if button is already pressed
        for button in buttonlist.iter() {
            if button.btimer != 0 && button.line == Some(line) {
                return;
            }
        }

        for button in buttonlist.iter_mut() {
            if button.btimer == 0 {
                button.line = Some(line);
                button.where_ = w;
                button.btexture = texture;
                button.btimer = time;
                button.soundorg = level.lines[line].frontsector;
                return;
            }
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
/// `line` must be a linedef of the loaded level.
pub fn P_ChangeSwitchTexture(world: &mut World, line: LineId, useAgain: i32) {
    let (sidenum, special, side) = {
        let level = &mut world.level;
        let linedef = &mut level.lines[line];
        if useAgain == 0 {
            linedef.special = 0;
        }
        let (sidenum, special) = (linedef.sidenum[0] as usize, linedef.special);
        (sidenum, special, level.sides[sidenum])
    };
    let (switchlist, numswitches) = {
        let switches = SWITCHES.borrow();
        (switches.switchlist, switches.numswitches)
    };
    let button_soundorg = world.active.buttons[0].soundorg;
    let texTop = side.toptexture;
    let texMid = side.midtexture;
    let texBot = side.bottomtexture;

    let mut sound = Sfx::Swtchn as i32;

    // EXIT SWITCH?
    if special == 11 {
        sound = Sfx::Swtchx as i32;
    }

    for i in 0..(numswitches * 2) as usize {
        if switchlist[i] == texTop as i32 {
            start_button_sound(world, button_soundorg, sound);
            world.level.sides[sidenum].toptexture = switchlist[i ^ 1] as i16;
            if useAgain != 0 {
                P_StartButton(world, line, top, switchlist[i], BUTTONTIME);
            }
            return;
        } else if switchlist[i] == texMid as i32 {
            start_button_sound(world, button_soundorg, sound);
            world.level.sides[sidenum].midtexture = switchlist[i ^ 1] as i16;
            if useAgain != 0 {
                P_StartButton(world, line, middle, switchlist[i], BUTTONTIME);
            }
            return;
        } else if switchlist[i] == texBot as i32 {
            start_button_sound(world, button_soundorg, sound);
            world.level.sides[sidenum].bottomtexture = switchlist[i ^ 1] as i16;
            if useAgain != 0 {
                P_StartButton(world, line, bottom, switchlist[i], BUTTONTIME);
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
/// Called by `P_UseLines` when `thing` presses Use against `line_id`.
/// `side` is `0` for the front face (the normal case) or `1` for the back
/// face. Using the back side is only allowed for special 124 (unused
/// sliding door).
///
/// Non-player actors can only activate lines that are not secret and have
/// one of the four manual-door specials (1, 32, 33, 34).
///
/// The main dispatch covers:
/// - Manual doors (specials 1, 26-28, 31-34, 117-118): calls
///   `EV_VerticalDoor` directly with no texture change.
/// - One-shot switches (specials 7-140): perform the action, then call
///   `P_ChangeSwitchTexture(line_id, 0)` to flip the texture permanently.
/// - Repeatable buttons (specials 42-139): perform the action, then call
///   `P_ChangeSwitchTexture(line_id, 1)` to flip and queue a reset timer.
///
/// Returns `1` (true) after handling any special; unrecognised specials
/// fall through silently and also return `1`.
///
/// # Panics
///
/// If `line_id` is not a linedef of the loaded level, or a raise-to-texture
/// floor meets a bottom texture that is not loaded (see [`EV_DoFloor`]).
pub fn P_UseSpecialLine(world: &mut World, thing: MobjId, line_id: LineId, side: i32) -> i32 {
    // The line's record as the use found it; the actions read its tag and
    // sectors, and every write goes to the level through `line_id`.
    let line_record = world.level.lines[line_id];
    let line = &line_record;

    // Err...
    // Use the back sides of VERY SPECIAL lines...
    if side != 0 {
        match line.special {
            124 => {
                // Sliding door open&close
                // UNUSED?
            }
            _ => return 0,
        }
    }

    // Switches that other things can activate.
    if MOBJ_ARENA.borrow()[thing].player.is_none() {
        // never open secret doors
        if line.flags & LinedefFlag::SECRET as i16 != 0 {
            return 0;
        }
        match line.special {
            1 | 32 | 33 | 34 => {}
            _ => return 0,
        }
    }

    // do something
    match line.special {
        // MANUALS
        1 | 26 | 27 | 28 | 31 | 32 | 33 | 34 | 117 | 118 => {
            EV_VerticalDoor(world, line_id, thing);
        }

        // SWITCHES
        7 if EV_BuildStairs(world, line, stair_build8) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        9 if EV_DoDonut(world, line) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        11 => {
            P_ChangeSwitchTexture(world, line_id, 0);
            G_ExitLevel();
        }
        14 if EV_DoPlat(world, line, plat_raiseAndChange, 32) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        15 if EV_DoPlat(world, line, plat_raiseAndChange, 24) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        18 if EV_DoFloor(world, line, floor_raiseFloorToNearest) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        20 if EV_DoPlat(world, line, plat_raiseToNearestAndChange, 0) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        21 if EV_DoPlat(world, line, plat_downWaitUpStay, 0) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        23 if EV_DoFloor(world, line, floor_lowerFloorToLowest) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        29 if EV_DoDoor(world, line, vld_normal) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        41 if EV_DoCeiling(world, line, ceiling_lowerToFloor) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        71 if EV_DoFloor(world, line, floor_turboLower) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        49 if EV_DoCeiling(world, line, ceiling_crushAndRaise) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        50 if EV_DoDoor(world, line, vld_close) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        51 => {
            P_ChangeSwitchTexture(world, line_id, 0);
            G_SecretExitLevel();
        }
        55 if EV_DoFloor(world, line, floor_raiseFloorCrush) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        101 if EV_DoFloor(world, line, floor_raiseFloor) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        102 if EV_DoFloor(world, line, floor_lowerFloor) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        103 if EV_DoDoor(world, line, vld_open) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        111 if EV_DoDoor(world, line, vld_blazeRaise) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        112 if EV_DoDoor(world, line, vld_blazeOpen) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        113 if EV_DoDoor(world, line, vld_blazeClose) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        122 if EV_DoPlat(world, line, plat_blazeDWUS, 0) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        127 if EV_BuildStairs(world, line, stair_turbo16) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        131 if EV_DoFloor(world, line, floor_raiseFloorTurbo) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        133 | 135 | 137 if EV_DoLockedDoor(world, line, vld_blazeOpen, thing) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }
        140 if EV_DoFloor(world, line, floor_raiseFloor512) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 0);
        }

        // BUTTONS
        42 if EV_DoDoor(world, line, vld_close) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        43 if EV_DoCeiling(world, line, ceiling_lowerToFloor) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        45 if EV_DoFloor(world, line, floor_lowerFloor) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        60 if EV_DoFloor(world, line, floor_lowerFloorToLowest) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        61 if EV_DoDoor(world, line, vld_open) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        62 if EV_DoPlat(world, line, plat_downWaitUpStay, 1) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        63 if EV_DoDoor(world, line, vld_normal) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        64 if EV_DoFloor(world, line, floor_raiseFloor) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        66 if EV_DoPlat(world, line, plat_raiseAndChange, 24) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        67 if EV_DoPlat(world, line, plat_raiseAndChange, 32) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        65 if EV_DoFloor(world, line, floor_raiseFloorCrush) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        68 if EV_DoPlat(world, line, plat_raiseToNearestAndChange, 0) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        69 if EV_DoFloor(world, line, floor_raiseFloorToNearest) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        70 if EV_DoFloor(world, line, floor_turboLower) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        114 if EV_DoDoor(world, line, vld_blazeRaise) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        115 if EV_DoDoor(world, line, vld_blazeOpen) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        116 if EV_DoDoor(world, line, vld_blazeClose) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        123 if EV_DoPlat(world, line, plat_blazeDWUS, 0) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        132 if EV_DoFloor(world, line, floor_raiseFloorTurbo) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        99 | 134 | 136 if EV_DoLockedDoor(world, line, vld_blazeOpen, thing) != 0 => {
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        138 => {
            EV_LightTurnOn(world, line, 255);
            P_ChangeSwitchTexture(world, line_id, 1);
        }
        139 => {
            EV_LightTurnOn(world, line, 35);
            P_ChangeSwitchTexture(world, line_id, 1);
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
    use super::*;

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
        let switches = SwitchState::new();
        assert_eq!(switches.numswitches, 0);
        for (i, &v) in switches.switchlist.iter().enumerate() {
            assert_eq!(v, 0, "switchlist[{i}] should be 0 before P_InitSwitchList");
        }
        for (i, b) in World::new().active.buttons.iter().enumerate() {
            assert!(b.line.is_none(), "buttonlist[{i}].line should be null");
            assert_eq!(b.btimer, 0, "buttonlist[{i}].btimer should be 0");
        }
    }

    #[test]
    fn switchlist_length_is_maxswitches_times_2() {
        let switches = SwitchState::new();
        assert_eq!(switches.switchlist.len(), MAXSWITCHES * 2);
        assert_eq!(switches.switchlist.len(), 100);
    }
}
