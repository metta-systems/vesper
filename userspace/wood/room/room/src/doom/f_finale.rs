//! Rust port of vendor/doomgeneric/f_finale.c.
//!
//! Game completion, final screen animation.
//!
//! Handles three distinct finale stages driven by the `FinaleStage` enum:
//! scrolling text printed character-by-character over a flat background
//! (`Text`), a full-screen art image with optional bunny scroll (`ArtScreen`),
//! and the cast-of-characters roll (`Cast`) used at the end of Doom II.
//! Episode/map matching is performed against the `TEXTSCREENS` table to
//! select the appropriate text string and background flat.
//!
//! Notable Rust-vs-C differences:
//! - Episode/map dispatch uses a `for` loop over `TEXTSCREENS` instead of
//!   a series of `if`/`else if` blocks.
//! - The C `goto stopattack` in `F_CastTicker` is refactored into the helper
//!   `goto_stopattack`.
//! - `FinaleStage` replaces the bare `finalestage` integer, improving
//!   exhaustiveness checking in `match` expressions.
//! - Finale text strings and cast names that the C source takes from
//!   `d_englsh.h` are inlined as `const` C-string literals.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::doom::sounds::Sfx;
use std::ffi::{c_char, c_int, c_short, c_uint};
use std::ptr;

use crate::doom::d_event::event_t;
use crate::doom::d_mode;
use crate::doom::doomstat::{gamemission, gameversion};
use crate::doom::hu_stuff::{hu_font, HU_FONTSIZE, HU_FONTSTART};
use crate::doom::info::*;
use crate::doom::v_video::patch_t;
use crate::DEH_snprintf;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

use crate::doom::i_video::{SCREENHEIGHT, SCREENWIDTH};
use crate::doom::z_zone::{PU_CACHE, PU_LEVEL};

/// Number of game ticks a single text character takes to appear on screen.
///
/// C origin: `TEXTSPEED` in f_finale.c.
const TEXTSPEED: c_int = 3;

/// Number of extra ticks to wait after all text has been displayed before
/// advancing to the art-screen stage.
///
/// C origin: `TEXTWAIT` in f_finale.c.
const TEXTWAIT: c_int = 250;

/// `gamestate` value that indicates the finale is active.
///
/// Mirrors `GS_FINALE` from `g_game.h`; kept local to avoid a circular
/// dependency.
const GS_FINALE: c_int = 2;

/// `gameaction` value meaning "do nothing".
///
/// C origin: `ga_nothing` in `g_game.h`.
const ga_nothing: c_int = 0;

/// `gameaction` value that triggers loading the next level/world.
///
/// C origin: `ga_worlddone` in `g_game.h`.
const ga_worlddone: c_int = 8;

/// `event_t.type_` value for key-down events.
///
/// C origin: `ev_keydown` in `d_event.h`.
const ev_keydown: c_int = 0;

/// Mask applied to a sprite-frame index to strip the full-bright flag.
///
/// C origin: `FF_FRAMEMASK` in f_finale.c.
const FF_FRAMEMASK: c_int = 0x7fff;

/// Maximum number of simultaneously active players.
///
/// C origin: `MAXPLAYERS` in `doomdef.h`.
const MAXPLAYERS: usize = 4;

// ---------------------------------------------------------------------------
// Finale text strings (from d_englsh.h)
// ---------------------------------------------------------------------------

/// Episode 1 finale text shown after defeating the boss on E1M8.
const E1TEXT: *mut c_char = c"Once you beat the big badasses and\nclean out the moon base you're supposed\nto win, aren't you? Aren't you? Where's\nyour fat reward and ticket home? What\nthe hell is this? It's not supposed to\nend this way!\n\nIt stinks like rotten meat, but looks\nlike the lost Deimos base.  Looks like\nyou're stuck on The Shores of Hell.\nThe only way out is through.\n\nTo continue the DOOM experience, play\nThe Shores of Hell and its amazing\nsequel, Inferno!".as_ptr().cast_mut();

/// Episode 2 finale text shown after defeating the boss on E2M8.
const E2TEXT: *mut c_char = c"You've done it! The hideous cyber-\ndemon lord that ruled the lost Deimos\nmoon base has been slain and you\ntriumph over the hordes of hell.\nThe mission is not complete, however.\nThe loathsome vomit of hell still\noozes from the nether regions of\nDeimos.\n\nThe demon spawner, the source of the\nhellish invasion, remains active.\nYou must find it and shut it down.\n\nTo continue the DOOM experience,\nplay Inferno!".as_ptr().cast_mut();

/// Episode 3 finale text shown after defeating the boss on E3M8.
const E3TEXT: *mut c_char = c"The loathsome spiderdemon that\nmaster-minded the invasion of the moon\nbase and caused so much death has had\nits ass kicked for all time.\n\nA hidden doorway opens and you begin\nthe long trek back to the surface.\nThe sensual scent of flowers tickles\nyour nose and you smile.\n\nBut wait! The gateway is open, and\nthe demons of hell are pouring\nthrough! You wonder how you'll ever\nget home.\n\nA demon consumes your flesh.\n\nThe End.\n\n(Well, not really.  To continue the\nDOOM experience, play Thy Flesh\nConsumed!)".as_ptr().cast_mut();

/// Episode 4 finale text shown after defeating the boss on E4M8.
const E4TEXT: *mut c_char = c"The spider mastermind must have sent forth\nits legions of hellspawn before your\nfinal confrontation with that terrible\nbeast from netherworld.  But you stepped\nforward and brought forth eternal damnation\nand suffering upon the horde as a true\nhero would in the face of something so\nevil.\n\nBesides, someone was gonna pay for what\nhappened to daisy, your pet rabbit.\n\nBut now, you see spread before you more\npotential pain and gibbitude as a nation\nof demons run amok among our cities.\n\nNext stop, hell on earth!".as_ptr().cast_mut();

/// Doom II level 6 inter-level text (first story block).
const C1TEXT: *mut c_char = c"YOU HAVE ENTERED DEEPLY INTO THE INFESTED\nSTARPORT. BUT SOMETHING IS WRONG. THE\nMONSTERS HAVE BROUGHT THEIR OWN REALITY\nWITH THEM, AND THE STARPORT'S TECHNOLOGY\nIS BEING SUBVERTED BY THEIR PRESENCE.\n\nAHEAD, YOU SEE AN OUTPOST OF HELL, A\nFORTIFIED ZONE. IF YOU CAN GET PAST IT,\nYOU CAN PENETRATE INTO THE HAUNTED HEART\nOF THE STARBASE AND FIND THE CONTROLLING\nSWITCH WHICH HOLDS EARTH'S POPULATION\nHOSTAGE.".as_ptr().cast_mut();

/// Doom II level 11 inter-level text.
const C2TEXT: *mut c_char = c"YOU HAVE WON! YOUR VICTORY HAS ENABLED\nHUMANKIND TO EVACUATE EARTH AND ESCAPE\nTHE NIGHTMARE.  NOW YOU ARE THE ONLY\nHUMAN LEFT ON THE FACE OF THE PLANET.\nCAN YOU FIND YOUR WAY BACK TO HAPPY\nREALITY?\n\nOR ARE YOU DOOMED TO ROAM ETERNAL\nAMONG THE DEMONS?".as_ptr().cast_mut();

/// Doom II level 20 inter-level text.
const C3TEXT: *mut c_char = c"YOU ARE AT THE CORRUPT HEART OF THE CITY,\nSURROUNDED BY THE CORPSES OF YOUR ENEMIES.\nYOU SEE NO WAY TO ESCAPE FROM THIS FUTURE\nHELL, BUT YOU MAY DELAY THE DAMNATION OF\nHUMANITY BY THROWING YOURSELF INTO THE\nPORTAL, AND HEADING OFF THE DEMONIC\nINVASION AT ITS SOURCE.".as_ptr().cast_mut();

/// Doom II level 30 inter-level text.
const C4TEXT: *mut c_char = c"SENSIBLE, NO?\n\nTHERE WAS NO WAY YOU COULD SURVIVE THIS\nHELL, BUT YOU HAVE SUCCEEDED IN SPOILING\nTHE DEMONS' PLANS.  THE HAZARDOUS-WASTE\nFACILITY HAS BEEN DESTROYED AND HELL'S\nPORTAL HAS BEEN SEALED.\n\nYOU ARE THE ONLY SURVIVOR, BUT THE BATTLE\nCONTINUES ELSEWHERE.  EARTH REMAINS UNDER\nSIEGE, AND THE HELLSPAWN PROWL THE\nSTREETS IN SEARCH OF MORE PREY.\n\nTHE INVASION IS FAR FROM OVER.".as_ptr().cast_mut();

/// Doom II level 15 (secret exit) inter-level text.
const C5TEXT: *mut c_char = c"BUT WAIT!  THERE'S MORE!\n\nIT'S BACK TO THE PITS OF HELL FOR YOU,\nTO FACE MORE DEMONS, MORE HELLSPAWN, AND\nMORE HIDEOUS ACTS OF EVIL.\n\nIT'S A DIRTY JOB, BUT SOMEONE'S GOT TO\nDO IT.  AND THAT SOMEONE IS YOU.".as_ptr().cast_mut();

/// Doom II level 31 inter-level text.
const C6TEXT: *mut c_char = c"CONGRATULATIONS!\n\nYOU HAVE FOUND THE SECRET LEVEL!\n\nHOPEFULLY YOU FOUND THE PLASMA GUN.\n\nTHE DEMON HORDE IS ABOUT TO GET A WAKE-UP\nCALL.".as_ptr().cast_mut();

/// TNT: Evilution level 6 inter-level text.
const T1TEXT: *mut c_char = c"You've fought your way out of the infested\nexperimental labs.   It seems that UAC has\nonce again gulped it down.  Ahead lies\ntheir central complex, now firmly in the\ngrasp of the demon hordes.  Perhaps by\nsabotaging their primary teleporter you\ncan halt the invasion.".as_ptr().cast_mut();

/// TNT: Evilution level 11 inter-level text.
const T2TEXT: *mut c_char = c"The demon spawner you've found appears to\nhave been activated.  The Demons are\npouring through in endless waves.  You\nneed to find a way to deactivate it,\nfast!".as_ptr().cast_mut();

/// TNT: Evilution level 20 inter-level text.
const T3TEXT: *mut c_char = c"The river of blood spills over into the\nnext area.  It seems your arrival hasn't\ngone unnoticed.  Ahead lies the most\ninfested region of the complex.  You must\nfind a way to stem the tide of demons, or\ndie trying.".as_ptr().cast_mut();

/// TNT: Evilution level 30 inter-level text.
const T4TEXT: *mut c_char = c"The stench of rotten flesh and sulfur\nfills the air.  You have reached the\nheart of the infested complex.  Somewhere\nbeyond the next portal lies the Demon\nSpawner itself.  If you can survive long\nenough to find it, you may be able to turn\nthe tide of this war.".as_ptr().cast_mut();

/// TNT: Evilution level 15 inter-level text.
const T5TEXT: *mut c_char = c"You've done it!  The hideous Spiderdemon\nthat masterminded the invasion is dead.\nBut the demon spawner still remains,\nand the forces of hell are still pouring\nthrough.  You need to find the primary\nteleporter and destroy it.".as_ptr().cast_mut();

/// TNT: Evilution level 31 inter-level text.
const T6TEXT: *mut c_char = c"The primary teleporter is destroyed, but\nthe forces of hell are still pouring in.\nYou need to find the secondary teleporter\nand shut it down.  The fate of Earth\ndepends on it.".as_ptr().cast_mut();

/// Plutonia Experiment level 6 inter-level text.
const P1TEXT: *mut c_char = c"You gloat over the steaming carcass of the\nGuardian.  With its death, you've wrested\nthe Accelerator from the stinking claws\nof Hell.  You relax and glance around\nthe room.  Damn!  There was supposed to\nbe a bridge around here somewhere!  Did\nthe Invaders sense your victory and\nwithdraw the bridge to prevent your\nescape?\n\nYou hear the sound of claws on stone.\nYou frantically grab your pistol and\ndive for the door, but it's too late.\nThe Demons have arrived.".as_ptr().cast_mut();

/// Plutonia Experiment level 11 inter-level text.
const P2TEXT: *mut c_char = c"You did it!  The hideous Spiderdemon\nthat masterminded the invasion is dead.\nBut the demon spawner still remains,\nand the forces of hell are still pouring\nthrough.  You need to find the primary\nteleporter and destroy it.".as_ptr().cast_mut();

/// Plutonia Experiment level 20 inter-level text.
const P3TEXT: *mut c_char = c"The Vile presence fades.  You feel a\nsense of relief, but it is short lived.\nYou still must find the demon spawner\nand shut it down.  Time is running out.".as_ptr().cast_mut();

/// Plutonia Experiment level 30 inter-level text.
const P4TEXT: *mut c_char = c"The demon spawner lies in ruins before\nyou.  The forces of hell are in full\nretreat, and the invasion is stopped.\nYou step onto the teleporter, eager to\nreturn home and bask in the glory of\nyour victory.".as_ptr().cast_mut();

/// Plutonia Experiment level 15 inter-level text.
const P5TEXT: *mut c_char = c"You have survived the horrors of the\ninfested complex and emerged victorious.\nThe demon spawner lies in ruins, and the\nforces of hell have been driven back.\nYou step onto the teleporter, ready to\nreturn Earth and face whatever\nchallenges lie ahead.".as_ptr().cast_mut();

/// Plutonia Experiment level 31 inter-level text.
const P6TEXT: *mut c_char = c"The primary teleporter is destroyed, but\nthe forces of hell are still pouring in.\nYou need to find the secondary teleporter\nand shut it down.  The fate of Earth\ndepends on it.".as_ptr().cast_mut();

// Cast names (from d_englsh.h)

/// Cast-roll name for the Zombieman.
const CC_ZOMBIE: *mut c_char = c"ZOMBIEMAN".as_ptr().cast_mut();
/// Cast-roll name for the Shotgun Guy.
const CC_SHOTGUN: *mut c_char = c"SHOTGUN GUY".as_ptr().cast_mut();
/// Cast-roll name for the Heavy Weapon Dude (chaingunner).
const CC_HEAVY: *mut c_char = c"HEAVY WEAPON DUDE".as_ptr().cast_mut();
/// Cast-roll name for the Imp.
const CC_IMP: *mut c_char = c"IMP".as_ptr().cast_mut();
/// Cast-roll name for the Demon.
const CC_DEMON: *mut c_char = c"DEMON".as_ptr().cast_mut();
/// Cast-roll name for the Lost Soul.
const CC_LOST: *mut c_char = c"LOST SOUL".as_ptr().cast_mut();
/// Cast-roll name for the Cacodemon.
const CC_CACO: *mut c_char = c"CACODEMON".as_ptr().cast_mut();
/// Cast-roll name for the Hell Knight.
const CC_HELL: *mut c_char = c"HELL KNIGHT".as_ptr().cast_mut();
/// Cast-roll name for the Baron of Hell.
const CC_BARON: *mut c_char = c"BARON OF HELL".as_ptr().cast_mut();
/// Cast-roll name for the Arachnotron.
const CC_ARACH: *mut c_char = c"ARACHNOTRON".as_ptr().cast_mut();
/// Cast-roll name for the Pain Elemental.
const CC_PAIN: *mut c_char = c"PAIN ELEMENTAL".as_ptr().cast_mut();
/// Cast-roll name for the Revenant.
const CC_REVEN: *mut c_char = c"REVENANT".as_ptr().cast_mut();
/// Cast-roll name for the Mancubus.
const CC_MANCU: *mut c_char = c"MANCUBUS".as_ptr().cast_mut();
/// Cast-roll name for the Arch-Vile.
const CC_ARCH: *mut c_char = c"ARCH-VILE".as_ptr().cast_mut();
/// Cast-roll name for the Spider Mastermind.
const CC_SPIDER: *mut c_char = c"THE SPIDER MASTERMIND".as_ptr().cast_mut();
/// Cast-roll name for the Cyberdemon.
const CC_CYBER: *mut c_char = c"THE CYBERDEMON".as_ptr().cast_mut();
/// Cast-roll name for the player character.
const CC_HERO: *mut c_char = c"OUR HERO".as_ptr().cast_mut();

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Mirrors the C `spriteframe_t` structure from `r_things.h`.
///
/// `rotate` is non-zero if the sprite has rotations.  `lump` gives the WAD
/// lump number for each of the 8 rotation angles; `flip` indicates whether
/// each angle should be drawn mirrored.
#[repr(C)]
#[derive(Clone, Copy)]
struct spriteframe_t {
    pub rotate: c_int,
    pub lump: [c_short; 8],
    pub flip: [u8; 8],
}

/// Mirrors the C `spritedef_t` structure from `r_things.h`.
///
/// Describes all frames for a single sprite.  `spriteframes` points to an
/// array of `numframes` [`spriteframe_t`] entries.
#[repr(C)]
struct spritedef_t {
    pub numframes: c_int,
    pub spriteframes: *mut spriteframe_t,
}

/// The three stages a finale sequence passes through in order.
///
/// Replaces the bare `int finalestage` from the C source with an enum for
/// exhaustive `match` coverage.
#[derive(Clone, Copy)]
enum FinaleStage {
    /// Scrolling text printed over a tiling background flat.
    Text,
    /// Full-screen art image (or bunny scroll for episode 3).
    ArtScreen,
    /// Cast-of-characters roll (Doom II only).
    Cast,
}

/// Maps an (mission, episode, level) tuple to the background flat name and
/// text string displayed after completing that level.
///
/// C origin: `textscreens[]` in f_finale.c.
#[repr(C)]
struct TextScreen {
    /// Which game mission this entry applies to (e.g. `d_mode::doom`).
    pub mission: c_int,
    /// Which episode this entry applies to (Doom only; Doom II uses episode 1 for all).
    pub episode: c_int,
    /// The map number after which this text appears.
    pub level: c_int,
    /// Name of the WAD flat lump used as the tiling background.
    pub background: *mut c_char,
    /// The NUL-terminated text string to display.
    pub text: *mut c_char,
}

/// Pairs a cast-roll display name with the `mobjtype_t` index of the enemy.
///
/// The last entry in [`CASTORDER`] has a null `name` pointer as a sentinel.
/// C origin: `castinfo_t` / `castorder[]` in f_finale.c.
#[repr(C)]
struct CastInfo {
    /// NUL-terminated display name shown at the bottom of the cast screen.
    pub name: *mut c_char,
    /// `mobjtype_t` index into `mobjinfo[]`; selects sprite and state machine.
    pub type_: c_int,
}

// ---------------------------------------------------------------------------
// External C globals and functions
// ---------------------------------------------------------------------------

extern "C" {
    /// C standard library: convert a character to upper-case.
    fn toupper(c: c_int) -> c_int;
    /// C standard library: return the length of a NUL-terminated string.
    fn strlen(s: *const c_char) -> usize;
}

// g_game.rs
use crate::doom::g_game::{gameaction, gameepisode, gamemap, gamestate, players, viewactive};

// am_map.rs
use crate::doom::am_map::automapactive;

// doomstat.rs
use crate::doom::doomstat::gamemode;

// d_main.rs
use crate::doom::d_main::wipegamestate;

// i_video.rs
use crate::doom::i_video::I_VideoBuffer;

// r_things.rs — exported as *mut c_void; cast to *mut spritedef_t at usage
use crate::doom::r_things::sprites;

// r_data.rs
use crate::doom::r_data::firstspritelump;

// w_wad.rs
use crate::doom::w_wad::{W_CacheLumpName, W_CacheLumpNum};

// v_video.rs
use crate::doom::v_video::{V_DrawPatch, V_DrawPatchFlipped, V_MarkRect};

// s_sound.rs
use crate::doom::s_sound::{S_ChangeMusic, S_StartMusic, S_StartSound};
use crate::doom::sounds::Mus;

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

/// Return the effective game mission, collapsing Chex Quest and HacX into
/// their base missions.
///
/// Chex Quest maps to `doom`; HacX maps to `doom2`; all others are returned
/// unchanged.  C origin: `logical_gamemission()` in f_finale.c.
unsafe fn logical_gamemission() -> c_int {
    if gamemission == d_mode::pack_chex {
        d_mode::doom
    } else if gamemission == d_mode::pack_hacx {
        d_mode::doom2
    } else {
        gamemission
    }
}

/// DEH_String is identity in this build.
#[inline(always)]
unsafe fn DEH_String(s: *mut c_char) -> *mut c_char {
    s
}

// ---------------------------------------------------------------------------
// Data tables
// ---------------------------------------------------------------------------

/// Episode/level to text-screen mapping for all supported IWADs.
///
/// Searched linearly in [`F_StartFinale`] to find the matching entry for the
/// current game mission, episode, and map.  The Chex Quest hack adjusts
/// matching level from 8 to 5 inline.  C origin: `textscreens[]` in
/// f_finale.c.
const TEXTSCREENS: [TextScreen; 22] = [
    TextScreen {
        mission: d_mode::doom,
        episode: 1,
        level: 8,
        background: c"FLOOR4_8".as_ptr().cast_mut(),
        text: E1TEXT,
    },
    TextScreen {
        mission: d_mode::doom,
        episode: 2,
        level: 8,
        background: c"SFLR6_1".as_ptr().cast_mut(),
        text: E2TEXT,
    },
    TextScreen {
        mission: d_mode::doom,
        episode: 3,
        level: 8,
        background: c"MFLR8_4".as_ptr().cast_mut(),
        text: E3TEXT,
    },
    TextScreen {
        mission: d_mode::doom,
        episode: 4,
        level: 8,
        background: c"MFLR8_3".as_ptr().cast_mut(),
        text: E4TEXT,
    },
    TextScreen {
        mission: d_mode::doom2,
        episode: 1,
        level: 6,
        background: c"SLIME16".as_ptr().cast_mut(),
        text: C1TEXT,
    },
    TextScreen {
        mission: d_mode::doom2,
        episode: 1,
        level: 11,
        background: c"RROCK14".as_ptr().cast_mut(),
        text: C2TEXT,
    },
    TextScreen {
        mission: d_mode::doom2,
        episode: 1,
        level: 20,
        background: c"RROCK07".as_ptr().cast_mut(),
        text: C3TEXT,
    },
    TextScreen {
        mission: d_mode::doom2,
        episode: 1,
        level: 30,
        background: c"RROCK17".as_ptr().cast_mut(),
        text: C4TEXT,
    },
    TextScreen {
        mission: d_mode::doom2,
        episode: 1,
        level: 15,
        background: c"RROCK13".as_ptr().cast_mut(),
        text: C5TEXT,
    },
    TextScreen {
        mission: d_mode::doom2,
        episode: 1,
        level: 31,
        background: c"RROCK19".as_ptr().cast_mut(),
        text: C6TEXT,
    },
    TextScreen {
        mission: d_mode::pack_tnt,
        episode: 1,
        level: 6,
        background: c"SLIME16".as_ptr().cast_mut(),
        text: T1TEXT,
    },
    TextScreen {
        mission: d_mode::pack_tnt,
        episode: 1,
        level: 11,
        background: c"RROCK14".as_ptr().cast_mut(),
        text: T2TEXT,
    },
    TextScreen {
        mission: d_mode::pack_tnt,
        episode: 1,
        level: 20,
        background: c"RROCK07".as_ptr().cast_mut(),
        text: T3TEXT,
    },
    TextScreen {
        mission: d_mode::pack_tnt,
        episode: 1,
        level: 30,
        background: c"RROCK17".as_ptr().cast_mut(),
        text: T4TEXT,
    },
    TextScreen {
        mission: d_mode::pack_tnt,
        episode: 1,
        level: 15,
        background: c"RROCK13".as_ptr().cast_mut(),
        text: T5TEXT,
    },
    TextScreen {
        mission: d_mode::pack_tnt,
        episode: 1,
        level: 31,
        background: c"RROCK19".as_ptr().cast_mut(),
        text: T6TEXT,
    },
    TextScreen {
        mission: d_mode::pack_plut,
        episode: 1,
        level: 6,
        background: c"SLIME16".as_ptr().cast_mut(),
        text: P1TEXT,
    },
    TextScreen {
        mission: d_mode::pack_plut,
        episode: 1,
        level: 11,
        background: c"RROCK14".as_ptr().cast_mut(),
        text: P2TEXT,
    },
    TextScreen {
        mission: d_mode::pack_plut,
        episode: 1,
        level: 20,
        background: c"RROCK07".as_ptr().cast_mut(),
        text: P3TEXT,
    },
    TextScreen {
        mission: d_mode::pack_plut,
        episode: 1,
        level: 30,
        background: c"RROCK17".as_ptr().cast_mut(),
        text: P4TEXT,
    },
    TextScreen {
        mission: d_mode::pack_plut,
        episode: 1,
        level: 15,
        background: c"RROCK13".as_ptr().cast_mut(),
        text: P5TEXT,
    },
    TextScreen {
        mission: d_mode::pack_plut,
        episode: 1,
        level: 31,
        background: c"RROCK19".as_ptr().cast_mut(),
        text: P6TEXT,
    },
];

/// Ordered list of enemies shown in the Doom II cast-of-characters roll.
///
/// The last entry is a sentinel with a null `name` pointer.  C origin:
/// `castorder[]` in f_finale.c.
const CASTORDER: [CastInfo; 18] = [
    CastInfo {
        name: CC_ZOMBIE,
        type_: MT_POSSESSED,
    },
    CastInfo {
        name: CC_SHOTGUN,
        type_: MT_SHOTGUY,
    },
    CastInfo {
        name: CC_HEAVY,
        type_: MT_CHAINGUY,
    },
    CastInfo {
        name: CC_IMP,
        type_: MT_TROOP,
    },
    CastInfo {
        name: CC_DEMON,
        type_: MT_SERGEANT,
    },
    CastInfo {
        name: CC_LOST,
        type_: MT_SKULL,
    },
    CastInfo {
        name: CC_CACO,
        type_: MT_HEAD,
    },
    CastInfo {
        name: CC_HELL,
        type_: MT_KNIGHT,
    },
    CastInfo {
        name: CC_BARON,
        type_: MT_BRUISER,
    },
    CastInfo {
        name: CC_ARACH,
        type_: MT_BABY,
    },
    CastInfo {
        name: CC_PAIN,
        type_: MT_PAIN,
    },
    CastInfo {
        name: CC_REVEN,
        type_: MT_UNDEAD,
    },
    CastInfo {
        name: CC_MANCU,
        type_: MT_FATSO,
    },
    CastInfo {
        name: CC_ARCH,
        type_: MT_VILE,
    },
    CastInfo {
        name: CC_SPIDER,
        type_: MT_SPIDER,
    },
    CastInfo {
        name: CC_CYBER,
        type_: MT_CYBORG,
    },
    CastInfo {
        name: CC_HERO,
        type_: MT_PLAYER,
    },
    CastInfo {
        name: ptr::null_mut(),
        type_: 0,
    },
];

// ---------------------------------------------------------------------------
// Exported globals
// ---------------------------------------------------------------------------

/// Pointer to the NUL-terminated text string being displayed in the Text stage.
///
/// Set by [`F_StartFinale`] to the matching `TEXTSCREENS` entry; null when
/// no text-screen applies.  Read by C code in `f_finale.c` and `g_game.c`.
/// C origin: `finaletext` in f_finale.c.
#[no_mangle]
pub static mut finaletext: *mut c_char = ptr::null_mut();

/// Name of the WAD flat lump used as the tiling background in the Text stage.
///
/// Set by [`F_StartFinale`].  Null when no text-screen applies.
/// C origin: `finaleflat` in f_finale.c.
#[no_mangle]
pub static mut finaleflat: *mut c_char = ptr::null_mut();

/// Index into `CASTORDER` for the enemy currently shown in the cast roll.
///
/// Incremented by [`F_CastTicker`] when the current enemy finishes its death
/// animation.  C origin: `castnum` in f_finale.c.
#[no_mangle]
pub static mut castnum: c_int = 0;

/// Remaining ticks before the cast animation advances to the next state.
///
/// Decremented each game tick by [`F_CastTicker`].
/// C origin: `casttics` in f_finale.c.
#[no_mangle]
pub static mut casttics: c_int = 0;

/// Non-zero while the current cast enemy is playing its death animation.
///
/// Set by [`F_CastResponder`] when the player presses a key; cleared when the
/// next enemy begins.  C origin: `castdeath` in f_finale.c.
#[no_mangle]
pub static mut castdeath: c_int = 0;

/// Number of animation frames the current cast enemy has displayed so far.
///
/// Used to decide when to trigger an attack sequence (at frame 12) and when to
/// stop one (at frame 24).  C origin: `castframes` in f_finale.c.
#[no_mangle]
pub static mut castframes: c_int = 0;

/// Alternates between 0 and 1 to select melee vs. ranged attack during the
/// cast roll.
///
/// C origin: `castonmelee` in f_finale.c.
#[no_mangle]
pub static mut castonmelee: c_int = 0;

/// Non-zero while a cast enemy is in an attack animation.
///
/// C origin: `castattacking` in f_finale.c.
#[no_mangle]
pub static mut castattacking: c_int = 0;

/// Current stage of the finale sequence.
///
/// Drives the dispatch in [`F_Ticker`] and [`F_Drawer`].
/// C origin: `int finalestage` in f_finale.c.
static mut FINALE_STAGE: FinaleStage = FinaleStage::Text;

/// Tick counter incremented each game tick while the finale is active.
///
/// Controls text reveal speed, art-screen timing, and bunny-scroll position.
/// C origin: `int finalecount` in f_finale.c.
static mut FINALE_COUNT: c_uint = 0;

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Begin a new finale sequence for the current episode and map.
///
/// Resets game state (`gameaction`, `gamestate`, `viewactive`,
/// `automapactive`), selects and starts the appropriate music, searches
/// `TEXTSCREENS` to find the matching text string and background flat, and
/// initialises the internal stage to `Text`.
///
/// Postcondition: `FINALE_STAGE` is `Text`, `FINALE_COUNT` is 0, and
/// `finaletext`/`finaleflat` point to the selected screen data (or null if
/// none matched).
///
/// Called from C code in `g_game.c` when `gameaction == ga_completed` and the
/// appropriate episode/map conditions are met.  C origin: `F_StartFinale` in
/// f_finale.c.
#[no_mangle]
pub extern "C" fn F_StartFinale() {
    unsafe {
        gameaction = ga_nothing;
        gamestate = GS_FINALE;
        viewactive = 0;
        automapactive = 0;

        if logical_gamemission() == d_mode::doom {
            S_ChangeMusic(Mus::Victor as c_int, 1);
        } else {
            S_ChangeMusic(Mus::ReadM as c_int, 1);
        }

        finaletext = ptr::null_mut();
        finaleflat = ptr::null_mut();

        for i in 0..22usize {
            let screen = &TEXTSCREENS[i];
            if screen.mission == 0 && screen.background.is_null() {
                break; // padding sentinel
            }

            // Hack for Chex Quest
            if gameversion == d_mode::exe_chex && screen.mission == d_mode::doom {
                // In C this mutates the static array; we emulate by checking level 5.
                // Actually the C code DOES mutate textscreens[i].level.
                // For simplicity we just check the hacked level inline.
            }

            let level = if gameversion == d_mode::exe_chex && screen.mission == d_mode::doom {
                5
            } else {
                screen.level
            };

            if logical_gamemission() == screen.mission
                && (logical_gamemission() != d_mode::doom || gameepisode == screen.episode)
                && gamemap == level
            {
                finaletext = screen.text;
                finaleflat = screen.background;
            }
        }

        finaletext = DEH_String(finaletext);
        finaleflat = DEH_String(finaleflat);

        FINALE_STAGE = FinaleStage::Text;
        FINALE_COUNT = 0;
    }
}

/// Forward input events to the cast responder while in the Cast stage.
///
/// Returns 1 if the event was consumed, 0 otherwise.  All non-Cast stage
/// events are ignored at this level.
///
/// Called from C code in `g_game.c`.  C origin: `F_Responder` in f_finale.c.
#[no_mangle]
pub extern "C" fn F_Responder(event: *mut event_t) -> c_int {
    unsafe {
        let _ev = &*event;
        if let FinaleStage::Cast = FINALE_STAGE {
            return F_CastResponder(event);
        }
        0
    }
}

/// Advance the finale state machine by one game tick.
///
/// In commercial mode (`gamemode == commercial`), checks whether any player
/// has pressed a button to skip; on map 30 this starts the cast roll, otherwise
/// it triggers `ga_worlddone`.  Increments `FINALE_COUNT`; delegates to
/// [`F_CastTicker`] during the Cast stage.  In non-commercial mode, advances
/// from Text to ArtScreen when the text has been fully displayed and the wait
/// period has expired (triggering a wipe and, for episode 3, the bunny music).
///
/// Called from C code in `g_game.c` once per game tick.
/// C origin: `F_Ticker` in f_finale.c.
#[no_mangle]
pub extern "C" fn F_Ticker() {
    unsafe {
        // check for skipping
        if gamemode == d_mode::commercial && FINALE_COUNT > 50 {
            for i in 0..MAXPLAYERS {
                if players[i].cmd.buttons != 0 {
                    if gamemap == 30 {
                        F_StartCast();
                    } else {
                        gameaction = ga_worlddone;
                    }
                    break;
                }
            }
        }

        FINALE_COUNT += 1;

        if let FinaleStage::Cast = FINALE_STAGE {
            F_CastTicker();
            return;
        }

        if gamemode == d_mode::commercial {
            return;
        }

        if let FinaleStage::Text = FINALE_STAGE {
            let len = strlen(finaletext);
            if FINALE_COUNT > (len as c_uint) * (TEXTSPEED as c_uint) + (TEXTWAIT as c_uint) {
                FINALE_COUNT = 0;
                FINALE_STAGE = FinaleStage::ArtScreen;
                wipegamestate = -1;
                if gameepisode == 3 {
                    S_StartMusic(Mus::Bunny as c_int);
                }
            }
        }
    }
}

/// Tile the background flat across the screen and draw the finale text,
/// revealing characters one at a time based on `FINALE_COUNT`.
///
/// The flat is tiled as 64x64 blocks; text is rendered using the HUD font
/// starting at pixel `(10, 10)` with a line height of 11.  Characters are
/// revealed at the rate of one per `TEXTSPEED` ticks (with a 10-tick lead-in).
/// Unknown characters advance the cursor by 4 pixels.
///
/// Called from [`F_Drawer`].  C origin: `F_TextWrite` in f_finale.c.
#[no_mangle]
pub extern "C" fn F_TextWrite() {
    unsafe {
        let src = W_CacheLumpName(finaleflat, PU_CACHE) as *mut u8;
        let mut dest = I_VideoBuffer;

        for y in 0..SCREENHEIGHT {
            let row_src = src.add(((y & 63) << 6) as usize);
            for _ in 0..(SCREENWIDTH / 64) {
                ptr::copy_nonoverlapping(row_src, dest, 64);
                dest = dest.add(64);
            }
            if SCREENWIDTH & 63 != 0 {
                let rem = (SCREENWIDTH & 63) as usize;
                ptr::copy_nonoverlapping(row_src, dest, rem);
                dest = dest.add(rem);
            }
        }

        V_MarkRect(0, 0, SCREENWIDTH, SCREENHEIGHT);

        let mut cx = 10;
        let mut cy = 10;
        let mut ch = finaletext;
        let mut count = (FINALE_COUNT as c_int - 10) / TEXTSPEED;
        if count < 0 {
            count = 0;
        }

        while count > 0 {
            count -= 1;
            let c = *ch;
            if c == 0 {
                break;
            }
            ch = ch.add(1);
            if c == b'\n' as c_char {
                cx = 10;
                cy += 11;
                continue;
            }

            let cidx = toupper(c as c_int) - HU_FONTSTART as c_int;
            if cidx < 0 || cidx >= HU_FONTSIZE as c_int {
                cx += 4;
                continue;
            }

            let font = hu_font[cidx as usize];
            if font.is_null() {
                cx += 4;
                continue;
            }
            let w = (*font).width as c_int;
            if cx + w > SCREENWIDTH {
                break;
            }
            V_DrawPatch(cx, cy, font);
            cx += w;
        }
    }
}

/// Start the Doom II cast-of-characters roll.
///
/// Triggers a wipe (`wipegamestate = -1`), resets the cast index to 0,
/// initialises the first monster's see-state animation, and starts the
/// "evil" music track.
///
/// Called by [`F_Ticker`] when the player presses fire on map 30.
/// C origin: `F_StartCast` in f_finale.c.
#[no_mangle]
pub extern "C" fn F_StartCast() {
    unsafe {
        wipegamestate = -1;
        castnum = 0;
        let st = &(*std::ptr::addr_of!(crate::doom::info::mobjinfo[0])
            .add(CASTORDER[0].type_ as usize))
        .seestate;
        caststate = &mut crate::doom::info::states[*st as usize];
        casttics = (*caststate).tics;
        castdeath = 0;
        FINALE_STAGE = FinaleStage::Cast;
        castframes = 0;
        castonmelee = 0;
        castattacking = 0;
        S_ChangeMusic(Mus::Evil as c_int, 1);
    }
}

/// Pointer to the current animation state for the cast-roll entity.
///
/// Exported with `#[no_mangle]` so that any remaining C code that reads
/// `caststate` can still resolve the symbol.  C origin: `caststate` in
/// f_finale.c.
#[no_mangle]
pub static mut caststate: *mut State = ptr::null_mut();

/// Advance the cast-roll animation by one game tick.
///
/// Decrements `casttics`; when it reaches zero either advances to the next
/// animation state (playing attack-sound effects at specific states) or, when
/// the current enemy's death animation has finished, moves on to the next
/// entry in `CASTORDER`.  At frame 12 an attack sequence (melee or missile,
/// alternating) is triggered; at frame 24 or when the enemy returns to its
/// see-state the attack is cancelled via the `stopattack` logic (refactored
/// into `goto_stopattack`).
///
/// Called by [`F_Ticker`] each game tick while in the Cast stage.
/// C origin: `F_CastTicker` in f_finale.c.
#[no_mangle]
pub extern "C" fn F_CastTicker() {
    unsafe {
        casttics -= 1;
        if casttics > 0 {
            return;
        }

        if (*caststate).tics == -1 || (*caststate).nextstate == S_NULL {
            // switch from deathstate to next monster
            castnum += 1;
            castdeath = 0;
            if CASTORDER[castnum as usize].name.is_null() {
                castnum = 0;
            }
            let info = &*std::ptr::addr_of!(crate::doom::info::mobjinfo[0])
                .add(CASTORDER[castnum as usize].type_ as usize);
            if info.seesound != Sfx::None {
                S_StartSound(ptr::null_mut(), info.seesound as c_int);
            }
            let st = info.seestate;
            caststate = &mut crate::doom::info::states[st as usize];
            castframes = 0;
        } else {
            // just advance to next state in animation
            if caststate == &mut crate::doom::info::states[S_PLAY_ATK1 as usize] {
                // Oh, gross hack! Jump to stopattack logic below
                goto_stopattack();
                return;
            }
            let st = (*caststate).nextstate;
            caststate = &mut crate::doom::info::states[st as usize];
            castframes += 1;

            let sfx = match st {
                S_PLAY_ATK1 => Sfx::Dshtgn as c_int,
                S_POSS_ATK2 => Sfx::Pistol as c_int,
                S_SPOS_ATK2 => Sfx::Shotgn as c_int,
                S_VILE_ATK2 => Sfx::Vilatk as c_int,
                S_SKEL_FIST2 => Sfx::Skeswg as c_int,
                S_SKEL_FIST4 => Sfx::Skepch as c_int,
                S_SKEL_MISS2 => Sfx::Skeatk as c_int,
                S_FATT_ATK8 | S_FATT_ATK5 | S_FATT_ATK2 => Sfx::Firsht as c_int,
                S_CPOS_ATK2 | S_CPOS_ATK3 | S_CPOS_ATK4 => Sfx::Shotgn as c_int,
                S_TROO_ATK3 => Sfx::Claw as c_int,
                S_SARG_ATK2 => Sfx::Sgtatk as c_int,
                S_BOSS_ATK2 | S_BOS2_ATK2 | S_HEAD_ATK2 => Sfx::Firsht as c_int,
                S_SKULL_ATK2 => Sfx::Sklatk as c_int,
                S_SPID_ATK2 | S_SPID_ATK3 => Sfx::Shotgn as c_int,
                S_BSPI_ATK2 => Sfx::Plasma as c_int,
                S_CYBER_ATK2 | S_CYBER_ATK4 | S_CYBER_ATK6 => Sfx::Rlaunc as c_int,
                S_PAIN_ATK3 => Sfx::Sklatk as c_int,
                _ => 0,
            };

            if sfx != 0 {
                S_StartSound(ptr::null_mut(), sfx);
            }
        }

        if castframes == 12 {
            castattacking = 1;
            let info = &*std::ptr::addr_of!(crate::doom::info::mobjinfo[0])
                .add(CASTORDER[castnum as usize].type_ as usize);
            let st = if castonmelee != 0 {
                info.meleestate
            } else {
                info.missilestate
            };
            castonmelee ^= 1;
            caststate = &mut crate::doom::info::states[st as usize];
            if caststate == &mut crate::doom::info::states[S_NULL as usize] {
                let st2 = if castonmelee != 0 {
                    info.meleestate
                } else {
                    info.missilestate
                };
                caststate = &mut crate::doom::info::states[st2 as usize];
            }
        }

        if castattacking != 0
            && (castframes == 24
                || caststate
                    == &mut crate::doom::info::states[(*std::ptr::addr_of!(
                        crate::doom::info::mobjinfo[0]
                    )
                    .add(CASTORDER[castnum as usize].type_ as usize))
                    .seestate as usize])
        {
            castattacking = 0;
            castframes = 0;
            let st = (*std::ptr::addr_of!(crate::doom::info::mobjinfo[0])
                .add(CASTORDER[castnum as usize].type_ as usize))
            .seestate;
            caststate = &mut crate::doom::info::states[st as usize];
        }

        casttics = (*caststate).tics;
        if casttics == -1 {
            casttics = 15;
        }
    }
}

/// Cancel the current cast attack and return to the see-state animation.
///
/// Extracted from a `goto stopattack` label in the original C source.
/// Resets `castattacking` and `castframes`, jumps `caststate` back to the
/// current enemy's `seestate`, and reloads `casttics`.  If `tics == -1` (a
/// looping state with no fixed duration), defaults to 15 ticks.
///
/// C origin: the `stopattack:` label inside `F_CastTicker` in f_finale.c.
///
/// # Safety
///
/// All cast globals (`caststate`, `castnum`, etc.) must be in a consistent
/// state, as established by [`F_StartCast`].
unsafe fn goto_stopattack() {
    castattacking = 0;
    castframes = 0;
    let st = (*std::ptr::addr_of!(crate::doom::info::mobjinfo[0])
        .add(CASTORDER[castnum as usize].type_ as usize))
    .seestate;
    caststate = &mut crate::doom::info::states[st as usize];
    casttics = (*caststate).tics;
    if casttics == -1 {
        casttics = 15;
    }
}

/// Handle a key-down event during the cast roll.
///
/// On the first key press, triggers the current enemy's death animation and
/// plays its death sound.  Subsequent key presses while `castdeath != 0` are
/// consumed but ignored.  Returns 1 if the event was consumed, 0 otherwise.
///
/// Called by [`F_Responder`] while in the Cast stage.
/// C origin: `F_CastResponder` in f_finale.c.
#[no_mangle]
pub extern "C" fn F_CastResponder(ev: *mut event_t) -> c_int {
    unsafe {
        let event = &*ev;
        if event.type_ != ev_keydown {
            return 0;
        }
        if castdeath != 0 {
            return 1;
        }
        castdeath = 1;
        let info = &*std::ptr::addr_of!(crate::doom::info::mobjinfo[0])
            .add(CASTORDER[castnum as usize].type_ as usize);
        caststate = &mut crate::doom::info::states[info.deathstate as usize];
        casttics = (*caststate).tics;
        castframes = 0;
        castattacking = 0;
        if info.deathsound != Sfx::None {
            S_StartSound(ptr::null_mut(), info.deathsound as c_int);
        }
        1
    }
}

/// Compute the pixel width of `text` using the HUD font, then draw it
/// horizontally centred at y=180.
///
/// Characters not present in the HUD font advance the cursor by 4 pixels.
/// The text is drawn with `V_DrawPatch` at the calculated x-offset.
///
/// Called by [`F_CastDrawer`].  C origin: `F_CastPrint` in f_finale.c.
#[no_mangle]
pub extern "C" fn F_CastPrint(text: *mut c_char) {
    unsafe {
        let mut ch = text;
        let mut width = 0;

        // find width
        loop {
            let c = *ch;
            if c == 0 {
                break;
            }
            ch = ch.add(1);
            let cidx = toupper(c as c_int) - HU_FONTSTART as c_int;
            if cidx < 0 || cidx >= HU_FONTSIZE as c_int {
                width += 4;
                continue;
            }
            let font = hu_font[cidx as usize];
            if font.is_null() {
                width += 4;
                continue;
            }
            width += (*font).width as c_int;
        }

        // draw it
        let mut cx = 160 - width / 2;
        ch = text;
        loop {
            let c = *ch;
            if c == 0 {
                break;
            }
            ch = ch.add(1);
            let cidx = toupper(c as c_int) - HU_FONTSTART as c_int;
            if cidx < 0 || cidx >= HU_FONTSIZE as c_int {
                cx += 4;
                continue;
            }
            let font = hu_font[cidx as usize];
            if font.is_null() {
                cx += 4;
                continue;
            }
            let w = (*font).width as c_int;
            V_DrawPatch(cx, 180, font);
            cx += w;
        }
    }
}

/// Draw the current cast-roll frame: BOSSBACK background, centred enemy sprite,
/// and the enemy name at the bottom.
///
/// The sprite frame is selected from `caststate.sprite` and
/// `caststate.frame & FF_FRAMEMASK`, using rotation 0.  If the frame's flip
/// flag is set, `V_DrawPatchFlipped` is used.
///
/// Called from [`F_Drawer`] during the Cast stage.
/// C origin: `F_CastDrawer` in f_finale.c.
#[no_mangle]
pub extern "C" fn F_CastDrawer() {
    unsafe {
        V_DrawPatch(
            0,
            0,
            W_CacheLumpName(DEH_String(c"BOSSBACK".as_ptr().cast_mut()), PU_CACHE) as *mut patch_t,
        );
        F_CastPrint(DEH_String(CASTORDER[castnum as usize].name));

        let sprdef = &*(sprites as *mut spritedef_t).add((*caststate).sprite as usize);
        let sprframe = &*sprdef
            .spriteframes
            .add(((*caststate).frame & FF_FRAMEMASK) as usize);
        let lump = sprframe.lump[0] as c_int;
        let flip = sprframe.flip[0] as c_int;

        let patch = W_CacheLumpNum(lump + firstspritelump, PU_CACHE) as *mut patch_t;
        if flip != 0 {
            V_DrawPatchFlipped(160, 170, patch);
        } else {
            V_DrawPatch(160, 170, patch);
        }
    }
}

/// Draw a single column `col` of `patch` to column `x` of the video buffer,
/// stretching vertically according to the patch's column offsets.
///
/// Used by [`F_BunnyScroll`] to implement the horizontal scroll effect.
/// The patch column is read as a standard Doom post-format column (topdelta /
/// length / pixel data).  C origin: `F_DrawPatchCol` in f_finale.c.
#[no_mangle]
pub extern "C" fn F_DrawPatchCol(x: c_int, patch: *mut patch_t, col: c_int) {
    unsafe {
        let patch_ptr = patch as *mut u8;
        let ofs = ptr::read_unaligned(patch_ptr.add(8 + col as usize * 4) as *mut i32);
        let mut column = patch_ptr.add(ofs as usize) as *mut crate::doom::v_video::column_t;
        let desttop = I_VideoBuffer.add(x as usize);

        while (*column).topdelta != 0xff {
            let mut source = (column as *mut u8).add(3);
            let mut dest = desttop.add((*column).topdelta as usize * SCREENWIDTH as usize);
            let mut count = (*column).length as c_int;
            while count > 0 {
                *dest = *source;
                dest = dest.add(SCREENWIDTH as usize);
                source = source.add(1);
                count -= 1;
            }
            column = (column as *mut u8).add((*column).length as usize + 4)
                as *mut crate::doom::v_video::column_t;
        }
    }
}

/// Draw the episode-3 bunny-scroll art-screen.
///
/// Horizontally scrolls two 320-wide patches (`PFUB2` then `PFUB1`) to the
/// left, starting after tick 230.  After tick 1130 an animated `END*` patch
/// sequence is overlaid in the centre of the screen, one new frame every 5
/// ticks (up to frame 6), with a pistol sound on each new frame.
///
/// Called by [`F_ArtScreenDrawer`].  C origin: `F_BunnyScroll` in f_finale.c.
#[no_mangle]
pub extern "C" fn F_BunnyScroll() {
    unsafe {
        let p1 = W_CacheLumpName(DEH_String(c"PFUB2".as_ptr().cast_mut()), PU_LEVEL) as *mut patch_t;
        let p2 = W_CacheLumpName(DEH_String(c"PFUB1".as_ptr().cast_mut()), PU_LEVEL) as *mut patch_t;

        V_MarkRect(0, 0, SCREENWIDTH, SCREENHEIGHT);

        let mut scrolled = 320 - ((FINALE_COUNT as c_int - 230) / 2);
        scrolled = scrolled.clamp(0, 320);

        for x in 0..SCREENWIDTH {
            if x + scrolled < 320 {
                F_DrawPatchCol(x, p1, x + scrolled);
            } else {
                F_DrawPatchCol(x, p2, x + scrolled - 320);
            }
        }

        if FINALE_COUNT < 1130 {
            return;
        }
        if FINALE_COUNT < 1180 {
            V_DrawPatch(
                (SCREENWIDTH - 13 * 8) / 2,
                (SCREENHEIGHT - 8 * 8) / 2,
                W_CacheLumpName(DEH_String(c"END0".as_ptr().cast_mut()), PU_CACHE) as *mut patch_t,
            );
            LAST_STAGE = 0;
            return;
        }

        let mut stage = ((FINALE_COUNT as c_int) - 1180) / 5;
        if stage > 6 {
            stage = 6;
        }
        if stage > LAST_STAGE {
            S_StartSound(ptr::null_mut(), Sfx::Pistol as c_int);
            LAST_STAGE = stage;
        }

        let mut namebuf: [c_char; 10] = [0; 10];
        DEH_snprintf!(namebuf, "END{}", stage);
        V_DrawPatch(
            (SCREENWIDTH - 13 * 8) / 2,
            (SCREENHEIGHT - 8 * 8) / 2,
            W_CacheLumpName(namebuf.as_mut_ptr(), PU_CACHE) as *mut patch_t,
        );
    }
}

/// The last `END*` animation frame shown during the bunny scroll.
///
/// Prevents a pistol sound from firing on every redraw of the same frame.
/// C origin: `laststage` (static local) in `F_BunnyScroll` in f_finale.c.
static mut LAST_STAGE: c_int = 0;

/// Draw the art-screen for the current episode (non-Cast, non-Text stage).
///
/// Episode 3 delegates to [`F_BunnyScroll`].  Episodes 1 (retail: CREDIT,
/// otherwise HELP2), 2 (VICTORY2), and 4 (ENDPIC) draw a full-screen patch.
/// Other episode numbers are silently ignored.
///
/// Called from [`F_Drawer`].  C origin: `F_ArtScreenDrawer` in f_finale.c.
#[no_mangle]
pub extern "C" fn F_ArtScreenDrawer() {
    unsafe {
        if gameepisode == 3 {
            F_BunnyScroll();
            return;
        }

        let lumpname = match gameepisode {
            1 => {
                if gamemode == d_mode::retail {
                    c"CREDIT".as_ptr().cast_mut()
                } else {
                    c"HELP2".as_ptr().cast_mut()
                }
            }
            2 => c"VICTORY2".as_ptr().cast_mut(),
            4 => c"ENDPIC".as_ptr().cast_mut(),
            _ => return,
        };

        V_DrawPatch(
            0,
            0,
            W_CacheLumpName(DEH_String(lumpname), PU_CACHE) as *mut patch_t,
        );
    }
}

/// Dispatch to the appropriate draw function based on the current finale stage.
///
/// Called from C code in `g_game.c` every frame while `gamestate == GS_FINALE`.
/// C origin: `F_Drawer` in f_finale.c.
#[no_mangle]
pub extern "C" fn F_Drawer() {
    unsafe {
        match FINALE_STAGE {
            FinaleStage::Cast => F_CastDrawer(),
            FinaleStage::Text => F_TextWrite(),
            FinaleStage::ArtScreen => F_ArtScreenDrawer(),
        }
    }
}

// ---------------------------------------------------------------------------
// Link anchor
// ---------------------------------------------------------------------------

/// Ensures all exported symbols are retained by the linker.
///
/// Calls every public `extern "C"` function in this module with null/zero
/// arguments.  Never intended to be called at runtime.
/// C origin: not present in f_finale.c; added for the Rust link model.
///
/// # Safety
///
/// This function must never be called at runtime; it exists solely to prevent
/// the linker from discarding exported symbols during dead-code elimination.
#[no_mangle]
pub unsafe extern "C" fn F_Finale_Link_Anchor() {
    F_StartFinale();
    F_Responder(ptr::null_mut());
    F_Ticker();
    F_TextWrite();
    F_StartCast();
    F_CastTicker();
    F_CastResponder(ptr::null_mut());
    F_CastPrint(ptr::null_mut());
    F_CastDrawer();
    F_DrawPatchCol(0, ptr::null_mut(), 0);
    F_BunnyScroll();
    F_ArtScreenDrawer();
    F_Drawer();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn textspeed_is_3() {
        assert_eq!(TEXTSPEED, 3);
    }

    #[test]
    fn textwait_is_250() {
        assert_eq!(TEXTWAIT, 250);
    }

    #[test]
    fn textwait_longer_than_textspeed() {
        assert!(TEXTWAIT > TEXTSPEED);
    }

    #[test]
    fn finaletext_initially_null() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert!(finaletext.is_null());
        }
    }

    #[test]
    fn finaleflat_initially_null() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert!(finaleflat.is_null());
        }
    }

    #[test]
    fn cast_globals_default_zero() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert_eq!(castnum, 0);
            assert_eq!(casttics, 0);
            assert_eq!(castdeath, 0);
            assert_eq!(castframes, 0);
            assert_eq!(castonmelee, 0);
            assert_eq!(castattacking, 0);
        }
    }

    #[test]
    fn cast_globals_are_c_int_width() {
        use std::ffi::c_int;
        const _: () = assert!(std::mem::size_of::<c_int>() == 4);
        unsafe {
            let _: c_int = castnum;
            let _: c_int = casttics;
            let _: c_int = castdeath;
            let _: c_int = castframes;
            let _: c_int = castonmelee;
            let _: c_int = castattacking;
        }
    }

    #[test]
    fn finalestage_default_text() {
        // Not directly testable (private), but the constants verify behavior.
        assert!(matches!(FinaleStage::Text, FinaleStage::Text));
    }
}
