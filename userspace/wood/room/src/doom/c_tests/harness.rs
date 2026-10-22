//! Test harness: pointer-size assertion, serial-test lock, and reset utilities.

#![allow(non_snake_case)]

use std::{ffi::c_void, sync::Mutex};

// Struct layouts assume 64-bit pointers.
const _: () = assert!(std::mem::size_of::<*mut c_void>() == 8);

/// Serialise tests that mutate C global state.
pub static C_GLOBAL_LOCK: Mutex<()> = Mutex::new(());

/// Reset the RNG cursors to a known state.
pub fn reset_rng() {
    crate::doom::m_random::M_ClearRandom();
}
