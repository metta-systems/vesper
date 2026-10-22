//! Tests for `p_mobj.rs` — map-object physics constants and item-queue globals.
//!
//! `p_mobj.rs` owns the mobj (map object) creation, movement, and lifecycle.
//! Key constants (`STOPSPEED`, `FRICTION`) govern the sliding physics for all
//! moving objects; they must not change between the C implementation and any
//! Rust port.  The item-respawn circular queue globals (`iquehead`, `iquetail`,
//! `itemrespawntime`) are zeroed at program start.
//!
//! These tests verify:
//!   * `STOPSPEED` and `FRICTION` constant values
//!   * `ITEMQUESIZE` matches the array dimensions
//!   * Item-queue indices start at zero
//!   * Item-respawn timestamp array is zero before level load

#![allow(non_snake_case)]

use crate::doom::c_ffi;
use crate::doom::m_fixed::FRACUNIT;
use crate::doom::p_mobj;

// ---------------------------------------------------------------------------
// STOPSPEED / FRICTION constants
// ---------------------------------------------------------------------------

/// `STOPSPEED = 0x1000` fixed-point units/tic².  When the horizontal
/// velocity magnitude drops below this value, it is zeroed.
#[test]
fn stopspeed_is_0x1000() {
    assert_eq!(c_ffi::STOPSPEED, 0x1000);
    assert_eq!(c_ffi::STOPSPEED, 4096);
}

/// `FRICTION = 0xe800` is the per-tic velocity multiplier applied while the
/// object is on the floor.  Being less than FRACUNIT causes deceleration.
/// The cast via u32 matches the C declaration, which treats the hex literal as
/// unsigned before storing it in the signed `int` global.
#[test]
fn friction_is_0xe800() {
    // The C constant is defined as 0xe800 which fits in a signed 32-bit int.
    assert_eq!(c_ffi::FRICTION, 0xe800_u32 as i32);
    assert_eq!(c_ffi::FRICTION, 59392_u32 as i32);
}

/// FRICTION is less than FRACUNIT, so it decelerates objects.
#[test]
fn friction_is_less_than_fracunit() {
    // 0xe800 = 59392 < 65536 = FRACUNIT, so every tic reduces velocity.
    assert!(
        c_ffi::FRICTION < FRACUNIT,
        "FRICTION must be < FRACUNIT for deceleration"
    );
}

/// STOPSPEED is much smaller than FRACUNIT (1 map unit = FRACUNIT);
/// it represents a tiny per-tic velocity threshold.
#[test]
fn stopspeed_is_small_fraction_of_fracunit() {
    assert!(c_ffi::STOPSPEED < FRACUNIT);
    // 0x1000 / 0x10000 = 1/16 of a map unit per tic.
    assert_eq!(FRACUNIT / c_ffi::STOPSPEED, 16);
}

// ---------------------------------------------------------------------------
// Item respawn queue
// ---------------------------------------------------------------------------

/// `ITEMQUESIZE = 128`: the item-respawn ring buffer holds 128 slots.
#[test]
fn itemquesize_is_128() {
    assert_eq!(c_ffi::ITEMQUESIZE, 128);
}

/// The respawn queue is exactly ITEMQUESIZE entries of mapthing_t.
#[test]
fn itemrespawnque_length() {
    unsafe {
        assert_eq!(p_mobj::itemrespawnque.len(), c_ffi::ITEMQUESIZE);
    }
}

/// `iquehead` is the write index; starts at 0 before any level is loaded.
#[test]
fn iquehead_default_zero() {
    unsafe {
        assert_eq!(
            p_mobj::iquehead,
            0,
            "iquehead should be 0 before level load"
        );
    }
}

/// `iquetail` is the read index; starts at 0 before any level is loaded.
#[test]
fn iquetail_default_zero() {
    unsafe {
        assert_eq!(
            p_mobj::iquetail,
            0,
            "iquetail should be 0 before level load"
        );
    }
}

/// At startup the head and tail indices are equal, meaning the queue is empty.
#[test]
fn itemque_starts_empty() {
    unsafe {
        assert_eq!(
            p_mobj::iquehead,
            p_mobj::iquetail,
            "item queue must be empty (head == tail) at startup"
        );
    }
}

/// `itemrespawntime[ITEMQUESIZE]` is zero-initialised before any level runs.
#[test]
fn itemrespawntime_starts_zeroed() {
    unsafe {
        for (i, &t) in p_mobj::itemrespawntime.iter().enumerate() {
            assert_eq!(t, 0, "itemrespawntime[{i}] should be 0 before level load");
        }
    }
}

/// `itemrespawntime` has exactly ITEMQUESIZE entries.
#[test]
fn itemrespawntime_length_is_itemquesize() {
    unsafe {
        assert_eq!(p_mobj::itemrespawntime.len(), c_ffi::ITEMQUESIZE);
    }
}

// ---------------------------------------------------------------------------
// Type-width checks
// ---------------------------------------------------------------------------

/// Queue index globals are C `int` (4 bytes).
#[test]
fn queue_indices_are_c_int_width() {
    use std::ffi::c_int;
    const _: () = assert!(std::mem::size_of::<c_int>() == 4);
    unsafe {
        let _: c_int = p_mobj::iquehead;
        let _: c_int = p_mobj::iquetail;
    }
}
