//! Test harness: pointer-size assertion and serial-test lock.

use std::{ffi::c_void, sync::Mutex};

// Struct layouts assume 64-bit pointers.
const _: () = assert!(std::mem::size_of::<*mut c_void>() == 8);

/// Serialise tests that mutate the engine's global state.
pub static C_GLOBAL_LOCK: Mutex<()> = Mutex::new(());
