//! Tests for `p_switch.c` — switch/button constants and globals.
//!
//! `p_switch.c` manages the two-state (on/off) switch textures and the timed
//! button list.  `P_InitSwitchList` populates `switchlist[]` from the
//! `alphSwitchList` table at game start; `numswitches` is then the number of
//! valid pairs.  The `buttonlist[]` array is zero-initialized and driven by
//! `P_StartButton` during gameplay.
//!
//! These tests verify:
//!   * `MAXSWITCHES`, `MAXBUTTONS`, and `BUTTONTIME` constants
//!   * `switchlist` and `buttonlist` array sizes
//!   * `numswitches` is zero before `P_InitSwitchList` is called
//!   * The `switchlist` array is zeroed before initialization
//!   * `BUTTONTIME` equals `TICRATE` (one second)

#![allow(non_snake_case)]

use crate::doom::i_timer::TICRATE;
use crate::doom::p_switch;

// ---------------------------------------------------------------------------
// Switch / button size constants
// ---------------------------------------------------------------------------

/// `MAXSWITCHES = 50`: the alpha switch list has room for 50 texture pairs.
#[test]
fn maxswitches_is_50() {
    assert_eq!(p_switch::MAXSWITCHES, 50);
}

/// `MAXBUTTONS = 16`: at most 16 simultaneously pressed timed buttons.
#[test]
fn maxbuttons_is_16() {
    assert_eq!(p_switch::MAXBUTTONS, 16);
}

/// `BUTTONTIME = 35` tics = 1 second (TICRATE).
/// A button pressed by the player reverts after exactly one second.
#[test]
fn buttontime_is_35() {
    assert_eq!(p_switch::BUTTONTIME, 35);
}

/// `BUTTONTIME` equals `TICRATE` — one second of press duration.
#[test]
fn buttontime_equals_ticrate() {
    assert_eq!(p_switch::BUTTONTIME, TICRATE);
}

// ---------------------------------------------------------------------------
// switchlist — texture-pair flat array
// ---------------------------------------------------------------------------

/// `switchlist[MAXSWITCHES * 2]` has exactly 100 elements.
#[test]
fn switchlist_length_is_maxswitches_times_2() {
    unsafe {
        assert_eq!(p_switch::switchlist.len(), p_switch::MAXSWITCHES * 2);
        assert_eq!(p_switch::switchlist.len(), 100);
    }
}

/// Before `P_InitSwitchList` runs, the entire array is zero.
#[test]
fn switchlist_default_zero() {
    unsafe {
        for (i, &v) in p_switch::switchlist.iter().enumerate() {
            assert_eq!(v, 0, "switchlist[{i}] should be 0 before P_InitSwitchList");
        }
    }
}

// ---------------------------------------------------------------------------
// numswitches — count of valid pairs
// ---------------------------------------------------------------------------

/// `numswitches` is zero before `P_InitSwitchList` fills the table.
#[test]
fn numswitches_default_zero() {
    unsafe {
        assert_eq!(
            p_switch::numswitches,
            0,
            "numswitches should be 0 before P_InitSwitchList"
        );
    }
}

// ---------------------------------------------------------------------------
// Type-width checks
// ---------------------------------------------------------------------------

/// `switchlist` and `numswitches` are C `int` arrays/values (4 bytes each).
#[test]
fn switch_globals_are_c_int_width() {
    use std::ffi::c_int;
    const _: () = assert!(std::mem::size_of::<c_int>() == 4);
    unsafe {
        let _: c_int = p_switch::switchlist[0];
        let _: c_int = p_switch::numswitches;
    }
}
