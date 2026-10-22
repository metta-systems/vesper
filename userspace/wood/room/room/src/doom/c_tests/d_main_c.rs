//! Tests for `d_main.c` — startup-flag defaults and global state.
//!
//! `d_main.c` is the main initialization entry point.  Many of its global
//! boolean flags default to false (0) and are only set to true when the
//! corresponding command-line option is present.  `show_endoom` defaults to 1,
//! and `main_loop_started` is set true once the event loop is entered.
//!
//! These tests verify:
//!   * All boolean startup flags are false (0) at program start
//!   * `show_endoom = 1` (enabled by default)
//!   * `main_loop_started = false` (0) before the loop begins

#![allow(non_snake_case)]

use crate::doom::c_ffi;

// ---------------------------------------------------------------------------
// Boolean startup flags — all false by default
// ---------------------------------------------------------------------------

/// `devparm` is false unless `-devparm` is passed; enables developer-mode
/// features such as fps display and warp.
#[test]
fn devparm_default_false() {
    unsafe {
        assert_eq!(
            c_ffi::devparm,
            0,
            "devparm should be false without -devparm"
        );
    }
}

/// `nomonsters` is false unless `-nomonsters` is passed.
#[test]
fn nomonsters_default_false() {
    unsafe {
        assert_eq!(
            c_ffi::nomonsters,
            0,
            "nomonsters should be false without -nomonsters"
        );
    }
}

/// `respawnparm` is false unless `-respawn` is passed; normally items only
/// respawn in Nightmare skill.
#[test]
fn respawnparm_default_false() {
    unsafe {
        assert_eq!(
            c_ffi::respawnparm,
            0,
            "respawnparm should be false without -respawn"
        );
    }
}

/// `fastparm` is false unless `-fast` is passed; normally monsters only use
/// fast attacks in Nightmare skill.
#[test]
fn fastparm_default_false() {
    unsafe {
        assert_eq!(c_ffi::fastparm, 0, "fastparm should be false without -fast");
    }
}

/// `autostart` is false unless a starting episode/map was specified via
/// `-warp` or `-episode`.
#[test]
fn autostart_default_false() {
    unsafe {
        assert_eq!(c_ffi::autostart, 0, "autostart should be false at startup");
    }
}

/// `advancedemo` is false at startup; set true only when the title sequence
/// starts cycling through demos.
#[test]
fn advancedemo_default_false() {
    unsafe {
        assert_eq!(
            c_ffi::advancedemo,
            0,
            "advancedemo should be false at startup"
        );
    }
}

/// `storedemo` is false at startup.
#[test]
fn storedemo_default_false() {
    unsafe {
        assert_eq!(c_ffi::storedemo, 0, "storedemo should be false at startup");
    }
}

/// `bfgedition` is false unless the BFG IWAD is loaded.
#[test]
fn bfgedition_default_false() {
    unsafe {
        assert_eq!(
            c_ffi::bfgedition,
            0,
            "bfgedition should be false when no IWAD is loaded"
        );
    }
}

/// `main_loop_started` is false (0) until the event loop is entered via
/// `D_DoomLoop`.
#[test]
fn main_loop_started_default_false() {
    unsafe {
        assert_eq!(
            c_ffi::main_loop_started,
            0,
            "main_loop_started should be false before D_DoomLoop"
        );
    }
}

// ---------------------------------------------------------------------------
// show_endoom — display ENDOOM lump on exit
// ---------------------------------------------------------------------------

/// `show_endoom = 1` by default: the ENDOOM lump is shown when the game exits.
/// The user can disable it via `-noendoom` or the config file.
#[test]
fn show_endoom_default_one() {
    unsafe {
        assert_eq!(
            c_ffi::show_endoom,
            1,
            "show_endoom should be 1 (enabled) by default"
        );
    }
}

// ---------------------------------------------------------------------------
// Starting episode / map
// ---------------------------------------------------------------------------

/// `startepisode` and `startmap` are zero before any `-episode` or `-warp`
/// argument has been processed.
#[test]
fn start_episode_and_map_default_zero() {
    unsafe {
        assert_eq!(
            c_ffi::startepisode,
            0,
            "startepisode should be 0 before command-line parse"
        );
        assert_eq!(
            c_ffi::startmap,
            0,
            "startmap should be 0 before command-line parse"
        );
    }
}

// ---------------------------------------------------------------------------
// Type-width checks
// ---------------------------------------------------------------------------

/// Boolean startup flags are C `int` (4 bytes).
#[test]
fn startup_flags_are_c_int_width() {
    use std::ffi::c_int;
    const _: () = assert!(std::mem::size_of::<c_int>() == 4);
    unsafe {
        let _: c_int = c_ffi::devparm;
        let _: c_int = c_ffi::nomonsters;
        let _: c_int = c_ffi::respawnparm;
        let _: c_int = c_ffi::fastparm;
        let _: c_int = c_ffi::autostart;
        let _: c_int = c_ffi::advancedemo;
        let _: c_int = c_ffi::storedemo;
        let _: c_int = c_ffi::bfgedition;
        let _: c_int = c_ffi::main_loop_started;
        let _: c_int = c_ffi::show_endoom;
    }
}
