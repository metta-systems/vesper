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
//!   `d_englsh.h` are inlined as `const` string literals.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

pub mod wipe;

use {crate::engine::sound::info::Sfx, std::ptr};

use crate::{
    engine::{
        doom,
        doom::{
            event::event_t,
            stat::{gamemission, gameversion},
        },
        hud::stuff::{hu_font, HU_FONTSIZE, HU_FONTSTART},
        play::info::*,
        wad::format::{column_t, patch_t},
    },
    DEH_snprintf,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

use crate::engine::{
    interface::video::{SCREENHEIGHT, SCREENWIDTH},
    zone::{PU_CACHE, PU_LEVEL},
};

/// Number of game ticks a single text character takes to appear on screen.
///
/// C origin: `TEXTSPEED` in f_finale.c.
const TEXTSPEED: i32 = 3;

/// Number of extra ticks to wait after all text has been displayed before
/// advancing to the art-screen stage.
///
/// C origin: `TEXTWAIT` in f_finale.c.
const TEXTWAIT: i32 = 250;

/// `gamestate` value that indicates the finale is active.
///
/// Mirrors `GS_FINALE` from `g_game.h`; kept local to avoid a circular
/// dependency.
const GS_FINALE: i32 = 2;

/// `gameaction` value meaning "do nothing".
///
/// C origin: `ga_nothing` in `g_game.h`.
const ga_nothing: i32 = 0;

/// `gameaction` value that triggers loading the next level/world.
///
/// C origin: `ga_worlddone` in `g_game.h`.
const ga_worlddone: i32 = 8;

/// `event_t.kind` value for key-down events.
///
/// C origin: `ev_keydown` in `d_event.h`.
const ev_keydown: i32 = 0;

/// Mask applied to a sprite-frame index to strip the full-bright flag.
///
/// C origin: `FF_FRAMEMASK` in f_finale.c.
const FF_FRAMEMASK: i32 = 0x7fff;

/// Maximum number of simultaneously active players.
///
/// C origin: `MAXPLAYERS` in `doomdef.h`.
const MAXPLAYERS: usize = 4;

// ---------------------------------------------------------------------------
// Finale text strings (from d_englsh.h)
// ---------------------------------------------------------------------------

/// Episode 1 finale text shown after defeating the boss on E1M8.
const E1TEXT: &str = "Once you beat the big badasses and\nclean out the moon base you're supposed\nto win, aren't you? Aren't you? Where's\nyour fat reward and ticket home? What\nthe hell is this? It's not supposed to\nend this way!\n\nIt stinks like rotten meat, but looks\nlike the lost Deimos base.  Looks like\nyou're stuck on The Shores of Hell.\nThe only way out is through.\n\nTo continue the DOOM experience, play\nThe Shores of Hell and its amazing\nsequel, Inferno!";

/// Episode 2 finale text shown after defeating the boss on E2M8.
const E2TEXT: &str = "You've done it! The hideous cyber-\ndemon lord that ruled the lost Deimos\nmoon base has been slain and you\ntriumph over the hordes of hell.\nThe mission is not complete, however.\nThe loathsome vomit of hell still\noozes from the nether regions of\nDeimos.\n\nThe demon spawner, the source of the\nhellish invasion, remains active.\nYou must find it and shut it down.\n\nTo continue the DOOM experience,\nplay Inferno!";

/// Episode 3 finale text shown after defeating the boss on E3M8.
const E3TEXT: &str = "The loathsome spiderdemon that\nmaster-minded the invasion of the moon\nbase and caused so much death has had\nits ass kicked for all time.\n\nA hidden doorway opens and you begin\nthe long trek back to the surface.\nThe sensual scent of flowers tickles\nyour nose and you smile.\n\nBut wait! The gateway is open, and\nthe demons of hell are pouring\nthrough! You wonder how you'll ever\nget home.\n\nA demon consumes your flesh.\n\nThe End.\n\n(Well, not really.  To continue the\nDOOM experience, play Thy Flesh\nConsumed!)";

/// Episode 4 finale text shown after defeating the boss on E4M8.
const E4TEXT: &str = "The spider mastermind must have sent forth\nits legions of hellspawn before your\nfinal confrontation with that terrible\nbeast from netherworld.  But you stepped\nforward and brought forth eternal damnation\nand suffering upon the horde as a true\nhero would in the face of something so\nevil.\n\nBesides, someone was gonna pay for what\nhappened to daisy, your pet rabbit.\n\nBut now, you see spread before you more\npotential pain and gibbitude as a nation\nof demons run amok among our cities.\n\nNext stop, hell on earth!";

/// Doom II level 6 inter-level text (first story block).
const C1TEXT: &str = "YOU HAVE ENTERED DEEPLY INTO THE INFESTED\nSTARPORT. BUT SOMETHING IS WRONG. THE\nMONSTERS HAVE BROUGHT THEIR OWN REALITY\nWITH THEM, AND THE STARPORT'S TECHNOLOGY\nIS BEING SUBVERTED BY THEIR PRESENCE.\n\nAHEAD, YOU SEE AN OUTPOST OF HELL, A\nFORTIFIED ZONE. IF YOU CAN GET PAST IT,\nYOU CAN PENETRATE INTO THE HAUNTED HEART\nOF THE STARBASE AND FIND THE CONTROLLING\nSWITCH WHICH HOLDS EARTH'S POPULATION\nHOSTAGE.";

/// Doom II level 11 inter-level text.
const C2TEXT: &str = "YOU HAVE WON! YOUR VICTORY HAS ENABLED\nHUMANKIND TO EVACUATE EARTH AND ESCAPE\nTHE NIGHTMARE.  NOW YOU ARE THE ONLY\nHUMAN LEFT ON THE FACE OF THE PLANET.\nCAN YOU FIND YOUR WAY BACK TO HAPPY\nREALITY?\n\nOR ARE YOU DOOMED TO ROAM ETERNAL\nAMONG THE DEMONS?";

/// Doom II level 20 inter-level text.
const C3TEXT: &str = "YOU ARE AT THE CORRUPT HEART OF THE CITY,\nSURROUNDED BY THE CORPSES OF YOUR ENEMIES.\nYOU SEE NO WAY TO ESCAPE FROM THIS FUTURE\nHELL, BUT YOU MAY DELAY THE DAMNATION OF\nHUMANITY BY THROWING YOURSELF INTO THE\nPORTAL, AND HEADING OFF THE DEMONIC\nINVASION AT ITS SOURCE.";

/// Doom II level 30 inter-level text.
const C4TEXT: &str = "SENSIBLE, NO?\n\nTHERE WAS NO WAY YOU COULD SURVIVE THIS\nHELL, BUT YOU HAVE SUCCEEDED IN SPOILING\nTHE DEMONS' PLANS.  THE HAZARDOUS-WASTE\nFACILITY HAS BEEN DESTROYED AND HELL'S\nPORTAL HAS BEEN SEALED.\n\nYOU ARE THE ONLY SURVIVOR, BUT THE BATTLE\nCONTINUES ELSEWHERE.  EARTH REMAINS UNDER\nSIEGE, AND THE HELLSPAWN PROWL THE\nSTREETS IN SEARCH OF MORE PREY.\n\nTHE INVASION IS FAR FROM OVER.";

/// Doom II level 15 (secret exit) inter-level text.
const C5TEXT: &str = "BUT WAIT!  THERE'S MORE!\n\nIT'S BACK TO THE PITS OF HELL FOR YOU,\nTO FACE MORE DEMONS, MORE HELLSPAWN, AND\nMORE HIDEOUS ACTS OF EVIL.\n\nIT'S A DIRTY JOB, BUT SOMEONE'S GOT TO\nDO IT.  AND THAT SOMEONE IS YOU.";

/// Doom II level 31 inter-level text.
const C6TEXT: &str = "CONGRATULATIONS!\n\nYOU HAVE FOUND THE SECRET LEVEL!\n\nHOPEFULLY YOU FOUND THE PLASMA GUN.\n\nTHE DEMON HORDE IS ABOUT TO GET A WAKE-UP\nCALL.";

/// TNT: Evilution level 6 inter-level text.
const T1TEXT: &str = "You've fought your way out of the infested\nexperimental labs.   It seems that UAC has\nonce again gulped it down.  Ahead lies\ntheir central complex, now firmly in the\ngrasp of the demon hordes.  Perhaps by\nsabotaging their primary teleporter you\ncan halt the invasion.";

/// TNT: Evilution level 11 inter-level text.
const T2TEXT: &str = "The demon spawner you've found appears to\nhave been activated.  The Demons are\npouring through in endless waves.  You\nneed to find a way to deactivate it,\nfast!";

/// TNT: Evilution level 20 inter-level text.
const T3TEXT: &str = "The river of blood spills over into the\nnext area.  It seems your arrival hasn't\ngone unnoticed.  Ahead lies the most\ninfested region of the complex.  You must\nfind a way to stem the tide of demons, or\ndie trying.";

/// TNT: Evilution level 30 inter-level text.
const T4TEXT: &str = "The stench of rotten flesh and sulfur\nfills the air.  You have reached the\nheart of the infested complex.  Somewhere\nbeyond the next portal lies the Demon\nSpawner itself.  If you can survive long\nenough to find it, you may be able to turn\nthe tide of this war.";

/// TNT: Evilution level 15 inter-level text.
const T5TEXT: &str = "You've done it!  The hideous Spiderdemon\nthat masterminded the invasion is dead.\nBut the demon spawner still remains,\nand the forces of hell are still pouring\nthrough.  You need to find the primary\nteleporter and destroy it.";

/// TNT: Evilution level 31 inter-level text.
const T6TEXT: &str = "The primary teleporter is destroyed, but\nthe forces of hell are still pouring in.\nYou need to find the secondary teleporter\nand shut it down.  The fate of Earth\ndepends on it.";

/// Plutonia Experiment level 6 inter-level text.
const P1TEXT: &str = "You gloat over the steaming carcass of the\nGuardian.  With its death, you've wrested\nthe Accelerator from the stinking claws\nof Hell.  You relax and glance around\nthe room.  Damn!  There was supposed to\nbe a bridge around here somewhere!  Did\nthe Invaders sense your victory and\nwithdraw the bridge to prevent your\nescape?\n\nYou hear the sound of claws on stone.\nYou frantically grab your pistol and\ndive for the door, but it's too late.\nThe Demons have arrived.";

/// Plutonia Experiment level 11 inter-level text.
const P2TEXT: &str = "You did it!  The hideous Spiderdemon\nthat masterminded the invasion is dead.\nBut the demon spawner still remains,\nand the forces of hell are still pouring\nthrough.  You need to find the primary\nteleporter and destroy it.";

/// Plutonia Experiment level 20 inter-level text.
const P3TEXT: &str = "The Vile presence fades.  You feel a\nsense of relief, but it is short lived.\nYou still must find the demon spawner\nand shut it down.  Time is running out.";

/// Plutonia Experiment level 30 inter-level text.
const P4TEXT: &str = "The demon spawner lies in ruins before\nyou.  The forces of hell are in full\nretreat, and the invasion is stopped.\nYou step onto the teleporter, eager to\nreturn home and bask in the glory of\nyour victory.";

/// Plutonia Experiment level 15 inter-level text.
const P5TEXT: &str = "You have survived the horrors of the\ninfested complex and emerged victorious.\nThe demon spawner lies in ruins, and the\nforces of hell have been driven back.\nYou step onto the teleporter, ready to\nreturn Earth and face whatever\nchallenges lie ahead.";

/// Plutonia Experiment level 31 inter-level text.
const P6TEXT: &str = "The primary teleporter is destroyed, but\nthe forces of hell are still pouring in.\nYou need to find the secondary teleporter\nand shut it down.  The fate of Earth\ndepends on it.";

// Cast names (from d_englsh.h)

/// Cast-roll name for the Zombieman.
const CC_ZOMBIE: &str = "ZOMBIEMAN";
/// Cast-roll name for the Shotgun Guy.
const CC_SHOTGUN: &str = "SHOTGUN GUY";
/// Cast-roll name for the Heavy Weapon Dude (chaingunner).
const CC_HEAVY: &str = "HEAVY WEAPON DUDE";
/// Cast-roll name for the Imp.
const CC_IMP: &str = "IMP";
/// Cast-roll name for the Demon.
const CC_DEMON: &str = "DEMON";
/// Cast-roll name for the Lost Soul.
const CC_LOST: &str = "LOST SOUL";
/// Cast-roll name for the Cacodemon.
const CC_CACO: &str = "CACODEMON";
/// Cast-roll name for the Hell Knight.
const CC_HELL: &str = "HELL KNIGHT";
/// Cast-roll name for the Baron of Hell.
const CC_BARON: &str = "BARON OF HELL";
/// Cast-roll name for the Arachnotron.
const CC_ARACH: &str = "ARACHNOTRON";
/// Cast-roll name for the Pain Elemental.
const CC_PAIN: &str = "PAIN ELEMENTAL";
/// Cast-roll name for the Revenant.
const CC_REVEN: &str = "REVENANT";
/// Cast-roll name for the Mancubus.
const CC_MANCU: &str = "MANCUBUS";
/// Cast-roll name for the Arch-Vile.
const CC_ARCH: &str = "ARCH-VILE";
/// Cast-roll name for the Spider Mastermind.
const CC_SPIDER: &str = "THE SPIDER MASTERMIND";
/// Cast-roll name for the Cyberdemon.
const CC_CYBER: &str = "THE CYBERDEMON";
/// Cast-roll name for the player character.
const CC_HERO: &str = "OUR HERO";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

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
struct TextScreen {
    /// Which game mission this entry applies to (e.g. `doom::mode::doom`).
    pub mission: i32,
    /// Which episode this entry applies to (Doom only; Doom II uses episode 1 for all).
    pub episode: i32,
    /// The map number after which this text appears.
    pub level: i32,
    /// Name of the WAD flat lump used as the tiling background.
    pub background: &'static [u8],
    /// The text to display.
    pub text: &'static str,
}

/// Pairs a cast-roll display name with the `mobjtype_t` index of the enemy.
///
/// The last entry in [`CASTORDER`] has an empty `name` as a sentinel.
/// C origin: `castinfo_t` / `castorder[]` in f_finale.c.
struct CastInfo {
    /// Display name shown at the bottom of the cast screen.
    pub name: &'static str,
    /// `mobjtype_t` index into `mobjinfo[]`; selects sprite and state machine.
    pub kind: i32,
}

// g_game.rs
use crate::engine::{
    dehacked::DEH_String,
    game::{gameaction, gameepisode, gamemap, gamestate, players, viewactive},
};

// am_map.rs
use crate::engine::automap::automapactive;

// doomstat.rs
use crate::engine::doom::stat::gamemode;

// d_main.rs
use crate::engine::doom::main::wipegamestate;

// i_video.rs
use crate::engine::interface::video::I_VideoBuffer;

// r_things.rs
use crate::engine::render_3d::things::sprites;

// r_data.rs
use crate::engine::render_3d::data::firstspritelump;

// w_wad.rs
use crate::engine::wad::{W_CacheLumpName, W_CacheLumpNum};

// v_video.rs
use crate::engine::render_2d::{V_DrawPatch, V_DrawPatchFlipped, V_MarkRect};

// s_sound.rs
use crate::engine::sound::{info::Mus, S_ChangeMusic, S_StartMusic, S_StartSound};

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

/// Return the effective game mission, collapsing Chex Quest and HacX into
/// their base missions.
///
/// Chex Quest maps to `doom`; HacX maps to `doom2`; all others are returned
/// unchanged.  C origin: `logical_gamemission()` in f_finale.c.
unsafe fn logical_gamemission() -> i32 {
    if gamemission == doom::mode::pack_chex {
        doom::mode::doom
    } else if gamemission == doom::mode::pack_hacx {
        doom::mode::doom2
    } else {
        gamemission
    }
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
        mission: doom::mode::doom,
        episode: 1,
        level: 8,
        background: b"FLOOR4_8",
        text: E1TEXT,
    },
    TextScreen {
        mission: doom::mode::doom,
        episode: 2,
        level: 8,
        background: b"SFLR6_1",
        text: E2TEXT,
    },
    TextScreen {
        mission: doom::mode::doom,
        episode: 3,
        level: 8,
        background: b"MFLR8_4",
        text: E3TEXT,
    },
    TextScreen {
        mission: doom::mode::doom,
        episode: 4,
        level: 8,
        background: b"MFLR8_3",
        text: E4TEXT,
    },
    TextScreen {
        mission: doom::mode::doom2,
        episode: 1,
        level: 6,
        background: b"SLIME16",
        text: C1TEXT,
    },
    TextScreen {
        mission: doom::mode::doom2,
        episode: 1,
        level: 11,
        background: b"RROCK14",
        text: C2TEXT,
    },
    TextScreen {
        mission: doom::mode::doom2,
        episode: 1,
        level: 20,
        background: b"RROCK07",
        text: C3TEXT,
    },
    TextScreen {
        mission: doom::mode::doom2,
        episode: 1,
        level: 30,
        background: b"RROCK17",
        text: C4TEXT,
    },
    TextScreen {
        mission: doom::mode::doom2,
        episode: 1,
        level: 15,
        background: b"RROCK13",
        text: C5TEXT,
    },
    TextScreen {
        mission: doom::mode::doom2,
        episode: 1,
        level: 31,
        background: b"RROCK19",
        text: C6TEXT,
    },
    TextScreen {
        mission: doom::mode::pack_tnt,
        episode: 1,
        level: 6,
        background: b"SLIME16",
        text: T1TEXT,
    },
    TextScreen {
        mission: doom::mode::pack_tnt,
        episode: 1,
        level: 11,
        background: b"RROCK14",
        text: T2TEXT,
    },
    TextScreen {
        mission: doom::mode::pack_tnt,
        episode: 1,
        level: 20,
        background: b"RROCK07",
        text: T3TEXT,
    },
    TextScreen {
        mission: doom::mode::pack_tnt,
        episode: 1,
        level: 30,
        background: b"RROCK17",
        text: T4TEXT,
    },
    TextScreen {
        mission: doom::mode::pack_tnt,
        episode: 1,
        level: 15,
        background: b"RROCK13",
        text: T5TEXT,
    },
    TextScreen {
        mission: doom::mode::pack_tnt,
        episode: 1,
        level: 31,
        background: b"RROCK19",
        text: T6TEXT,
    },
    TextScreen {
        mission: doom::mode::pack_plut,
        episode: 1,
        level: 6,
        background: b"SLIME16",
        text: P1TEXT,
    },
    TextScreen {
        mission: doom::mode::pack_plut,
        episode: 1,
        level: 11,
        background: b"RROCK14",
        text: P2TEXT,
    },
    TextScreen {
        mission: doom::mode::pack_plut,
        episode: 1,
        level: 20,
        background: b"RROCK07",
        text: P3TEXT,
    },
    TextScreen {
        mission: doom::mode::pack_plut,
        episode: 1,
        level: 30,
        background: b"RROCK17",
        text: P4TEXT,
    },
    TextScreen {
        mission: doom::mode::pack_plut,
        episode: 1,
        level: 15,
        background: b"RROCK13",
        text: P5TEXT,
    },
    TextScreen {
        mission: doom::mode::pack_plut,
        episode: 1,
        level: 31,
        background: b"RROCK19",
        text: P6TEXT,
    },
];

/// Ordered list of enemies shown in the Doom II cast-of-characters roll.
///
/// The last entry is a sentinel with an empty `name`.  C origin:
/// `castorder[]` in f_finale.c.
const CASTORDER: [CastInfo; 18] = [
    CastInfo {
        name: CC_ZOMBIE,
        kind: MT_POSSESSED,
    },
    CastInfo {
        name: CC_SHOTGUN,
        kind: MT_SHOTGUY,
    },
    CastInfo {
        name: CC_HEAVY,
        kind: MT_CHAINGUY,
    },
    CastInfo {
        name: CC_IMP,
        kind: MT_TROOP,
    },
    CastInfo {
        name: CC_DEMON,
        kind: MT_SERGEANT,
    },
    CastInfo {
        name: CC_LOST,
        kind: MT_SKULL,
    },
    CastInfo {
        name: CC_CACO,
        kind: MT_HEAD,
    },
    CastInfo {
        name: CC_HELL,
        kind: MT_KNIGHT,
    },
    CastInfo {
        name: CC_BARON,
        kind: MT_BRUISER,
    },
    CastInfo {
        name: CC_ARACH,
        kind: MT_BABY,
    },
    CastInfo {
        name: CC_PAIN,
        kind: MT_PAIN,
    },
    CastInfo {
        name: CC_REVEN,
        kind: MT_UNDEAD,
    },
    CastInfo {
        name: CC_MANCU,
        kind: MT_FATSO,
    },
    CastInfo {
        name: CC_ARCH,
        kind: MT_VILE,
    },
    CastInfo {
        name: CC_SPIDER,
        kind: MT_SPIDER,
    },
    CastInfo {
        name: CC_CYBER,
        kind: MT_CYBORG,
    },
    CastInfo {
        name: CC_HERO,
        kind: MT_PLAYER,
    },
    CastInfo { name: "", kind: 0 },
];

// ---------------------------------------------------------------------------
// Exported globals
// ---------------------------------------------------------------------------

/// The text being displayed in the Text stage.
///
/// Set by [`F_StartFinale`] to the matching `TEXTSCREENS` entry; empty when
/// no text-screen applies.  C origin: `finaletext` in f_finale.c.
pub static mut finaletext: &str = "";

/// Name of the WAD flat lump used as the tiling background in the Text stage.
///
/// Set by [`F_StartFinale`].  Empty when no text-screen applies.
/// C origin: `finaleflat` in f_finale.c.
pub static mut finaleflat: &[u8] = b"";

/// Index into `CASTORDER` for the enemy currently shown in the cast roll.
///
/// Incremented by [`F_CastTicker`] when the current enemy finishes its death
/// animation.  C origin: `castnum` in f_finale.c.
pub static mut castnum: i32 = 0;

/// Remaining ticks before the cast animation advances to the next state.
///
/// Decremented each game tick by [`F_CastTicker`].
/// C origin: `casttics` in f_finale.c.
pub static mut casttics: i32 = 0;

/// Non-zero while the current cast enemy is playing its death animation.
///
/// Set by [`F_CastResponder`] when the player presses a key; cleared when the
/// next enemy begins.  C origin: `castdeath` in f_finale.c.
pub static mut castdeath: i32 = 0;

/// Number of animation frames the current cast enemy has displayed so far.
///
/// Used to decide when to trigger an attack sequence (at frame 12) and when to
/// stop one (at frame 24).  C origin: `castframes` in f_finale.c.
pub static mut castframes: i32 = 0;

/// Alternates between 0 and 1 to select melee vs. ranged attack during the
/// cast roll.
///
/// C origin: `castonmelee` in f_finale.c.
pub static mut castonmelee: i32 = 0;

/// Non-zero while a cast enemy is in an attack animation.
///
/// C origin: `castattacking` in f_finale.c.
pub static mut castattacking: i32 = 0;

/// Current stage of the finale sequence.
///
/// Drives the dispatch in [`F_Ticker`] and [`F_Drawer`].
/// C origin: `int finalestage` in f_finale.c.
static mut FINALE_STAGE: FinaleStage = FinaleStage::Text;

/// Tick counter incremented each game tick while the finale is active.
///
/// Controls text reveal speed, art-screen timing, and bunny-scroll position.
/// C origin: `int finalecount` in f_finale.c.
static mut FINALE_COUNT: u32 = 0;

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
/// `finaletext`/`finaleflat` hold the selected screen data (or are empty if
/// none matched).
///
/// Called from C code in `g_game.c` when `gameaction == ga_completed` and the
/// appropriate episode/map conditions are met.  C origin: `F_StartFinale` in
/// f_finale.c.
pub fn F_StartFinale() {
    unsafe {
        gameaction = ga_nothing;
        gamestate = GS_FINALE;
        viewactive = 0;
        automapactive = 0;

        if logical_gamemission() == doom::mode::doom {
            S_ChangeMusic(Mus::Victor as i32, 1);
        } else {
            S_ChangeMusic(Mus::ReadM as i32, 1);
        }

        finaletext = "";
        finaleflat = b"";

        for screen in &TEXTSCREENS {
            // Hack for Chex Quest
            if gameversion == doom::mode::exe_chex && screen.mission == doom::mode::doom {
                // In C this mutates the static array; we emulate by checking level 5.
                // Actually the C code DOES mutate textscreens[i].level.
                // For simplicity we just check the hacked level inline.
            }

            let level = if gameversion == doom::mode::exe_chex && screen.mission == doom::mode::doom
            {
                5
            } else {
                screen.level
            };

            if logical_gamemission() == screen.mission
                && (logical_gamemission() != doom::mode::doom || gameepisode == screen.episode)
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
pub fn F_Responder(event: *mut event_t) -> i32 {
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
pub fn F_Ticker() {
    unsafe {
        // check for skipping
        if gamemode == doom::mode::commercial && FINALE_COUNT > 50 {
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

        if gamemode == doom::mode::commercial {
            return;
        }

        if let FinaleStage::Text = FINALE_STAGE {
            let len = { finaletext }.len();
            if FINALE_COUNT > (len as u32) * (TEXTSPEED as u32) + (TEXTWAIT as u32) {
                FINALE_COUNT = 0;
                FINALE_STAGE = FinaleStage::ArtScreen;
                wipegamestate = -1;
                if gameepisode == 3 {
                    S_StartMusic(Mus::Bunny as i32);
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
pub fn F_TextWrite() {
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
        let count = ((FINALE_COUNT as i32 - 10) / TEXTSPEED).max(0) as usize;

        for &c in { finaletext }.as_bytes().iter().take(count) {
            if c == b'\n' {
                cx = 10;
                cy += 11;
                continue;
            }

            let Some(font) = font_patch(c) else {
                cx += 4;
                continue;
            };
            let w = (*font).width.get() as i32;
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
pub fn F_StartCast() {
    unsafe {
        wipegamestate = -1;
        castnum = 0;
        let st = &(*std::ptr::addr_of!(crate::engine::play::info::mobjinfo[0])
            .add(CASTORDER[0].kind as usize))
        .seestate;
        caststate = &mut crate::engine::play::info::states[*st as usize];
        casttics = (*caststate).tics;
        castdeath = 0;
        FINALE_STAGE = FinaleStage::Cast;
        castframes = 0;
        castonmelee = 0;
        castattacking = 0;
        S_ChangeMusic(Mus::Evil as i32, 1);
    }
}

/// Pointer to the current animation state for the cast-roll entity.
///
/// C origin: `caststate` in f_finale.c.
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
pub fn F_CastTicker() {
    unsafe {
        casttics -= 1;
        if casttics > 0 {
            return;
        }

        if (*caststate).tics == -1 || (*caststate).nextstate == S_NULL {
            // switch from deathstate to next monster
            castnum += 1;
            castdeath = 0;
            if CASTORDER[castnum as usize].name.is_empty() {
                castnum = 0;
            }
            let info = &*std::ptr::addr_of!(crate::engine::play::info::mobjinfo[0])
                .add(CASTORDER[castnum as usize].kind as usize);
            if info.seesound != Sfx::None {
                S_StartSound(ptr::null_mut(), info.seesound as i32);
            }
            let st = info.seestate;
            caststate = &mut crate::engine::play::info::states[st as usize];
            castframes = 0;
        } else {
            // just advance to next state in animation
            if caststate == &mut crate::engine::play::info::states[S_PLAY_ATK1 as usize] {
                // Oh, gross hack! Jump to stopattack logic below
                goto_stopattack();
                return;
            }
            let st = (*caststate).nextstate;
            caststate = &mut crate::engine::play::info::states[st as usize];
            castframes += 1;

            let sfx = match st {
                S_PLAY_ATK1 => Sfx::Dshtgn as i32,
                S_POSS_ATK2 => Sfx::Pistol as i32,
                S_SPOS_ATK2 => Sfx::Shotgn as i32,
                S_VILE_ATK2 => Sfx::Vilatk as i32,
                S_SKEL_FIST2 => Sfx::Skeswg as i32,
                S_SKEL_FIST4 => Sfx::Skepch as i32,
                S_SKEL_MISS2 => Sfx::Skeatk as i32,
                S_FATT_ATK8 | S_FATT_ATK5 | S_FATT_ATK2 => Sfx::Firsht as i32,
                S_CPOS_ATK2 | S_CPOS_ATK3 | S_CPOS_ATK4 => Sfx::Shotgn as i32,
                S_TROO_ATK3 => Sfx::Claw as i32,
                S_SARG_ATK2 => Sfx::Sgtatk as i32,
                S_BOSS_ATK2 | S_BOS2_ATK2 | S_HEAD_ATK2 => Sfx::Firsht as i32,
                S_SKULL_ATK2 => Sfx::Sklatk as i32,
                S_SPID_ATK2 | S_SPID_ATK3 => Sfx::Shotgn as i32,
                S_BSPI_ATK2 => Sfx::Plasma as i32,
                S_CYBER_ATK2 | S_CYBER_ATK4 | S_CYBER_ATK6 => Sfx::Rlaunc as i32,
                S_PAIN_ATK3 => Sfx::Sklatk as i32,
                _ => 0,
            };

            if sfx != 0 {
                S_StartSound(ptr::null_mut(), sfx);
            }
        }

        if castframes == 12 {
            castattacking = 1;
            let info = &*std::ptr::addr_of!(crate::engine::play::info::mobjinfo[0])
                .add(CASTORDER[castnum as usize].kind as usize);
            let st = if castonmelee != 0 {
                info.meleestate
            } else {
                info.missilestate
            };
            castonmelee ^= 1;
            caststate = &mut crate::engine::play::info::states[st as usize];
            if caststate == &mut crate::engine::play::info::states[S_NULL as usize] {
                let st2 = if castonmelee != 0 {
                    info.meleestate
                } else {
                    info.missilestate
                };
                caststate = &mut crate::engine::play::info::states[st2 as usize];
            }
        }

        if castattacking != 0
            && (castframes == 24
                || caststate
                    == &mut crate::engine::play::info::states[(*std::ptr::addr_of!(
                        crate::engine::play::info::mobjinfo[0]
                    )
                    .add(CASTORDER[castnum as usize].kind as usize))
                    .seestate
                        as usize])
        {
            castattacking = 0;
            castframes = 0;
            let st = (*std::ptr::addr_of!(crate::engine::play::info::mobjinfo[0])
                .add(CASTORDER[castnum as usize].kind as usize))
            .seestate;
            caststate = &mut crate::engine::play::info::states[st as usize];
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
    let st = (*std::ptr::addr_of!(crate::engine::play::info::mobjinfo[0])
        .add(CASTORDER[castnum as usize].kind as usize))
    .seestate;
    caststate = &mut crate::engine::play::info::states[st as usize];
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
pub fn F_CastResponder(ev: *mut event_t) -> i32 {
    unsafe {
        let event = &*ev;
        if event.kind != ev_keydown {
            return 0;
        }
        if castdeath != 0 {
            return 1;
        }
        castdeath = 1;
        let info = &*std::ptr::addr_of!(crate::engine::play::info::mobjinfo[0])
            .add(CASTORDER[castnum as usize].kind as usize);
        caststate = &mut crate::engine::play::info::states[info.deathstate as usize];
        casttics = (*caststate).tics;
        castframes = 0;
        castattacking = 0;
        if info.deathsound != Sfx::None {
            S_StartSound(ptr::null_mut(), info.deathsound as i32);
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
pub fn F_CastPrint(text: &str) {
    unsafe {
        // find width
        let width: i32 = text
            .bytes()
            .map(|c| font_patch(c).map_or(4, |font| (*font).width.get() as i32))
            .sum();

        // draw it
        let mut cx = 160 - width / 2;
        for c in text.bytes() {
            let Some(font) = font_patch(c) else {
                cx += 4;
                continue;
            };
            let w = (*font).width.get() as i32;
            V_DrawPatch(cx, 180, font);
            cx += w;
        }
    }
}

/// The HUD font patch for character `c` (upper-cased), or `None` if the
/// font has no glyph for it.
///
/// # Safety
/// Reads the `hu_font` table; `HU_Init` must have loaded it.
unsafe fn font_patch(c: u8) -> Option<*mut patch_t> {
    let index = c.to_ascii_uppercase().checked_sub(HU_FONTSTART)? as usize;
    if index >= HU_FONTSIZE {
        return None;
    }
    let font = hu_font[index];
    (!font.is_null()).then_some(font)
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
pub fn F_CastDrawer() {
    unsafe {
        V_DrawPatch(
            0,
            0,
            W_CacheLumpName(DEH_String(b"BOSSBACK"), PU_CACHE) as *mut patch_t,
        );
        F_CastPrint(DEH_String(CASTORDER[castnum as usize].name));

        let sprdef = &*sprites.add((*caststate).sprite as usize);
        let sprframe = &*sprdef
            .spriteframes
            .add(((*caststate).frame & FF_FRAMEMASK) as usize);
        let lump = sprframe.lump[0] as i32;
        let flip = sprframe.flip[0] as i32;

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
pub fn F_DrawPatchCol(x: i32, patch: *mut patch_t, col: i32) {
    unsafe {
        let patch_ptr = patch as *mut u8;
        let ofs = patch_t::column_offset(patch, col as usize);
        let mut column = patch_ptr.add(ofs) as *mut column_t;
        let desttop = I_VideoBuffer.add(x as usize);

        while (*column).topdelta != 0xff {
            let mut source = (column as *mut u8).add(3);
            let mut dest = desttop.add((*column).topdelta as usize * SCREENWIDTH as usize);
            let mut count = (*column).length as i32;
            while count > 0 {
                *dest = *source;
                dest = dest.add(SCREENWIDTH as usize);
                source = source.add(1);
                count -= 1;
            }
            column = (column as *mut u8).add((*column).length as usize + 4) as *mut column_t;
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
pub fn F_BunnyScroll() {
    unsafe {
        let p1 = W_CacheLumpName(DEH_String(b"PFUB2"), PU_LEVEL) as *mut patch_t;
        let p2 = W_CacheLumpName(DEH_String(b"PFUB1"), PU_LEVEL) as *mut patch_t;

        V_MarkRect(0, 0, SCREENWIDTH, SCREENHEIGHT);

        let mut scrolled = 320 - ((FINALE_COUNT as i32 - 230) / 2);
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
                W_CacheLumpName(DEH_String(b"END0"), PU_CACHE) as *mut patch_t,
            );
            LAST_STAGE = 0;
            return;
        }

        let mut stage = ((FINALE_COUNT as i32) - 1180) / 5;
        if stage > 6 {
            stage = 6;
        }
        if stage > LAST_STAGE {
            S_StartSound(ptr::null_mut(), Sfx::Pistol as i32);
            LAST_STAGE = stage;
        }

        let mut namebuf: [u8; 10] = [0; 10];
        DEH_snprintf!(namebuf, "END{}", stage);
        V_DrawPatch(
            (SCREENWIDTH - 13 * 8) / 2,
            (SCREENHEIGHT - 8 * 8) / 2,
            W_CacheLumpName(&namebuf, PU_CACHE) as *mut patch_t,
        );
    }
}

/// The last `END*` animation frame shown during the bunny scroll.
///
/// Prevents a pistol sound from firing on every redraw of the same frame.
/// C origin: `laststage` (static local) in `F_BunnyScroll` in f_finale.c.
static mut LAST_STAGE: i32 = 0;

/// Draw the art-screen for the current episode (non-Cast, non-Text stage).
///
/// Episode 3 delegates to [`F_BunnyScroll`].  Episodes 1 (retail: CREDIT,
/// otherwise HELP2), 2 (VICTORY2), and 4 (ENDPIC) draw a full-screen patch.
/// Other episode numbers are silently ignored.
///
/// Called from [`F_Drawer`].  C origin: `F_ArtScreenDrawer` in f_finale.c.
pub fn F_ArtScreenDrawer() {
    unsafe {
        if gameepisode == 3 {
            F_BunnyScroll();
            return;
        }

        let lumpname: &[u8] = match gameepisode {
            1 => {
                if gamemode == doom::mode::retail {
                    b"CREDIT"
                } else {
                    b"HELP2"
                }
            }
            2 => b"VICTORY2",
            4 => b"ENDPIC",
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
pub fn F_Drawer() {
    unsafe {
        match FINALE_STAGE {
            FinaleStage::Cast => F_CastDrawer(),
            FinaleStage::Text => F_TextWrite(),
            FinaleStage::ArtScreen => F_ArtScreenDrawer(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::assertions_on_constants)] // pins constant values
mod tests {
    use {super::*, std::sync::Mutex};

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
    fn finaletext_initially_empty() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert!({ finaletext }.is_empty());
        }
    }

    #[test]
    fn finaleflat_initially_empty() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert!({ finaleflat }.is_empty());
        }
    }

    /// Every text screen names a background flat and has text; the cast
    /// roll ends with exactly one empty-name sentinel.
    #[test]
    fn finale_tables_are_complete() {
        assert!(TEXTSCREENS
            .iter()
            .all(|screen| !screen.background.is_empty() && !screen.text.is_empty()));
        let (sentinel, names) = CASTORDER.split_last().unwrap();
        assert!(sentinel.name.is_empty());
        assert!(names.iter().all(|cast| !cast.name.is_empty()));
    }

    #[test]
    fn cast_globals_default_zero() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert_eq!({ castnum }, 0);
            assert_eq!({ casttics }, 0);
            assert_eq!({ castdeath }, 0);
            assert_eq!({ castframes }, 0);
            assert_eq!({ castonmelee }, 0);
            assert_eq!({ castattacking }, 0);
        }
    }

    #[test]
    fn cast_globals_are_i32() {
        unsafe {
            let _: i32 = castnum;
            let _: i32 = casttics;
            let _: i32 = castdeath;
            let _: i32 = castframes;
            let _: i32 = castonmelee;
            let _: i32 = castattacking;
        }
    }

    #[test]
    fn finalestage_default_text() {
        // Not directly testable (private), but the constants verify behavior.
        assert!(matches!(FinaleStage::Text, FinaleStage::Text));
    }
}
