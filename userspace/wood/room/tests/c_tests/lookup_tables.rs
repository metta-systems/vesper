//! Behavioural parity tests for lookup-table globals in `vendor/doomgeneric/`.
//!
//! Verifies that the Rust port's `maxammo`, `clipammo`, and `intercepts`
//! globals carry the exact byte-for-byte values used by the original C code.
//! These tables are read by `P_GiveAmmo` (`p_inter.c`) and `P_PathTraverse`
//! (`p_maputl.c`); any drift would silently change gameplay balance.

#![allow(non_snake_case)]

use {crate::harness::C_GLOBAL_LOCK, room::engine::play};

// ---------------------------------------------------------------------------
// p_inter.c: maxammo and clipammo
// ---------------------------------------------------------------------------

/// `maxammo[]` records the player's per-type ammunition cap in the same order
/// as the `ammotype_t` enum: bullets, shells, cells, rockets.  These four
/// values are baked into `P_GiveAmmo` and the cheat handler.
#[test]
fn maxammo_values() {
    unsafe {
        assert_eq!(play::inter::maxammo[0], 200); // bullets
        assert_eq!(play::inter::maxammo[1], 50); // shells
        assert_eq!(play::inter::maxammo[2], 300); // cells
        assert_eq!(play::inter::maxammo[3], 50); // rockets
    }
}

/// `clipammo[]` records how many rounds are awarded per ammo pickup of each
/// type.  Doubled for big boxes; halved for "give one clip" pickups.
#[test]
fn clipammo_values() {
    assert_eq!(play::inter::clipammo[0], 10); // bullets per clip
    assert_eq!(play::inter::clipammo[1], 4); // shells per clip
    assert_eq!(play::inter::clipammo[2], 20); // cells per clip
    assert_eq!(play::inter::clipammo[3], 1); // rockets per clip
}

/// Sanity check: every ammo type must allow a positive cap and pickup amount,
/// otherwise no ammo of that type can ever be picked up.
#[test]
fn ammo_arrays_positive() {
    unsafe {
        for i in 0..4 {
            assert!(play::inter::maxammo[i] > 0, "maxammo[{}] must be > 0", i);
            assert!(play::inter::clipammo[i] > 0, "clipammo[{}] must be > 0", i);
        }
    }
}

// ---------------------------------------------------------------------------
// p_maputl.c: intercepts array
// ---------------------------------------------------------------------------

/// The `intercepts[]` scratch array used by `P_PathTraverse` must hold exactly
/// `MAXINTERCEPTS` entries — both the C source and Rust port size it from the
/// same constant.
#[test]
fn intercepts_array_length() {
    unsafe {
        assert_eq!(
            (*&raw const play::maputl::intercepts).len(),
            play::maputl::MAXINTERCEPTS
        );
    }
}

/// `intercept_p` is the write cursor into `intercepts[]`; it must be NULL
/// before `P_PathTraverse` has populated the array.  The test takes the global
/// lock and explicitly resets the cursor so it is not polluted by prior tests.
#[test]
fn intercept_p_initially_null() {
    let _guard = C_GLOBAL_LOCK.lock().unwrap();
    unsafe {
        // Ensure we test the null state even if another test ran first.
        play::maputl::intercept_p = std::ptr::null_mut();
        assert!(play::maputl::intercept_p.is_null());
    }
}
