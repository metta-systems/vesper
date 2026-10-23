//! Tests for `d_main.c` — startup-flag defaults and global state.
//!
//! `d_main.c` is the main initialization entry point.  Many of its global
//! boolean flags default to false (0) and are only set to true when the
//! corresponding command-line option is present.  `show_endoom` defaults to 1,
//! and `main_loop_started` is set true once the event loop is entered.
//!
//! These tests verify, on a fresh `DoomMainState`:
//!   * All boolean startup flags are false (0) at program start
//!   * `show_endoom = 1` (enabled by default)
//!   * `main_loop_started = false` (0) before the loop begins

#![allow(non_snake_case)]

use room::engine::doom::main::DoomMainState;

// ---------------------------------------------------------------------------
// Boolean startup flags — all false by default
// ---------------------------------------------------------------------------

/// `devparm` is false unless `-devparm` is passed; enables developer-mode
/// features such as fps display and warp.
#[test]
fn devparm_default_false() {
    assert_eq!(
        DoomMainState::new().devparm,
        0,
        "devparm should be false without -devparm"
    );
}

/// `nomonsters` is false unless `-nomonsters` is passed.
#[test]
fn nomonsters_default_false() {
    assert_eq!(
        DoomMainState::new().nomonsters,
        0,
        "nomonsters should be false without -nomonsters"
    );
}

/// `respawnparm` is false unless `-respawn` is passed; normally items only
/// respawn in Nightmare skill.
#[test]
fn respawnparm_default_false() {
    assert_eq!(
        DoomMainState::new().respawnparm,
        0,
        "respawnparm should be false without -respawn"
    );
}

/// `fastparm` is false unless `-fast` is passed; normally monsters only use
/// fast attacks in Nightmare skill.
#[test]
fn fastparm_default_false() {
    assert_eq!(
        DoomMainState::new().fastparm,
        0,
        "fastparm should be false without -fast"
    );
}

/// `autostart` is false unless a starting episode/map was specified via
/// `-warp` or `-episode`.
#[test]
fn autostart_default_false() {
    assert_eq!(
        DoomMainState::new().autostart,
        0,
        "autostart should be false at startup"
    );
}

/// `advancedemo` is false at startup; set true only when the title sequence
/// starts cycling through demos.
#[test]
fn advancedemo_default_false() {
    assert_eq!(
        DoomMainState::new().advancedemo,
        0,
        "advancedemo should be false at startup"
    );
}

/// `storedemo` is false at startup.
#[test]
fn storedemo_default_false() {
    assert_eq!(
        DoomMainState::new().storedemo,
        0,
        "storedemo should be false at startup"
    );
}

/// `bfgedition` is false unless the BFG IWAD is loaded.
#[test]
fn bfgedition_default_false() {
    assert_eq!(
        DoomMainState::new().bfgedition,
        0,
        "bfgedition should be false when no IWAD is loaded"
    );
}

/// `main_loop_started` is false (0) until the event loop is entered via
/// `D_DoomLoop`.
#[test]
fn main_loop_started_default_false() {
    assert_eq!(
        DoomMainState::new().main_loop_started,
        0,
        "main_loop_started should be false before D_DoomLoop"
    );
}

// ---------------------------------------------------------------------------
// show_endoom — display ENDOOM lump on exit
// ---------------------------------------------------------------------------

/// `show_endoom = 1` by default: the ENDOOM lump is shown when the game exits.
/// The user can disable it via `-noendoom` or the config file.
#[test]
fn show_endoom_default_one() {
    assert_eq!(
        DoomMainState::new().show_endoom,
        1,
        "show_endoom should be 1 (enabled) by default"
    );
}

// ---------------------------------------------------------------------------
// Starting episode / map
// ---------------------------------------------------------------------------

/// `startepisode` and `startmap` are zero before any `-episode` or `-warp`
/// argument has been processed.
#[test]
fn start_episode_and_map_default_zero() {
    let main = DoomMainState::new();
    assert_eq!(
        main.startepisode, 0,
        "startepisode should be 0 before command-line parse"
    );
    assert_eq!(
        main.startmap, 0,
        "startmap should be 0 before command-line parse"
    );
}
