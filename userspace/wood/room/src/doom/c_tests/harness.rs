//! Test harness: type-size assertions, serial-test lock, and reset utilities.

#![allow(non_snake_case)]

use std::ffi::{c_int, c_long, c_short, c_uint, c_ulong, c_ushort, c_void};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Compile-time type-size assertions.
// These catch the exact data-type-size bugs we've hit before.
// ---------------------------------------------------------------------------

const _: () = assert!(std::mem::size_of::<c_int>() == 4);
const _: () = assert!(std::mem::size_of::<c_uint>() == 4);
const _: () = assert!(std::mem::size_of::<c_short>() == 2);
const _: () = assert!(std::mem::size_of::<c_ushort>() == 2);
const _: () = assert!(std::mem::size_of::<c_long>() == 8); // Linux x86_64
const _: () = assert!(std::mem::size_of::<c_ulong>() == 8);
const _: () = assert!(std::mem::size_of::<*mut c_void>() == 8); // pointer size

/// Serialise tests that mutate C global state.
pub static C_GLOBAL_LOCK: Mutex<()> = Mutex::new(());

/// Reset the RNG cursors to a known state.
pub fn reset_rng() {
    unsafe {
        crate::doom::m_random::M_ClearRandom();
    }
}
