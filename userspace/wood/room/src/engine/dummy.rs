//! Rust port of vendor/doomgeneric/dummy.c.
//!
//! Stub globals and no-op functions that satisfy link-time references from
//! modules that are not yet ported or whose optional features are disabled.
//!
//! In the original Chocolate Doom / doomgeneric source, `dummy.c` is compiled
//! only when certain features (`FEATURE_SOUND`, networking) are absent.  It
//! unconditionally provides `net_client_connected = false`, `drone = false`,
//! and (when `FEATURE_SOUND` is not defined) an empty
//! `I_InitTimidityConfig`.  This Rust port mirrors those three definitions
//! directly, using `u32` for the two booleans because `doomtype.h` defines
//! `boolean` as `unsigned int`.

#![allow(non_upper_case_globals, non_snake_case)]

/// Whether the client is connected to a network game session.
///
/// Corresponds to `boolean net_client_connected` in `dummy.c`.
/// Initialised to `false` (0).  The networking subsystem sets this to a
/// non-zero value when a connection is established; because the real network
/// module is not yet ported, this stub keeps it permanently `false`.
/// Referenced from C as `net_client_connected`.
// `boolean` in doomtype.h is `typedef unsigned int boolean;` — use u32.
pub static mut net_client_connected: u32 = 0; // false

/// Whether this instance is running as a "drone" (observer) client.
///
/// Corresponds to `boolean drone` in `dummy.c`.  A drone connects to a
/// network game but does not control a player; it is used for recording or
/// observation purposes.  Initialised to `false` (0) and never set while the
/// networking subsystem is stubbed out.  Referenced from C as `drone`.
pub static mut drone: u32 = 0; // false

/// No-op stub for Timidity MIDI configuration initialisation.
///
/// Corresponds to the `#ifndef FEATURE_SOUND` block in `dummy.c`.  When the
/// sound system is disabled or not yet ported, this function is compiled in
/// place of the real implementation to satisfy link-time references from
/// `i_sound.c` and related modules.  The real implementation would locate and
/// load a Timidity configuration file for software MIDI synthesis.
///
/// # Note
/// The original C stub is also empty; no `// FIXME` is needed here.
pub fn I_InitTimidityConfig() {}

#[cfg(test)]
mod tests {
    use {super::*, std::sync::Mutex};

    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn globals_default_to_zero() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            assert_eq!({ net_client_connected }, 0);
            assert_eq!({ drone }, 0);
        }
    }

    #[test]
    fn globals_store_unsigned_values() {
        let _g = LOCK.lock().unwrap();
        unsafe {
            net_client_connected = u32::MAX;
            drone = u32::MAX;
            assert_eq!({ net_client_connected }, u32::MAX);
            assert_eq!({ drone }, u32::MAX);
            net_client_connected = 0;
            drone = 0;
        }
    }

    #[test]
    fn timidity_stub_does_not_panic() {
        I_InitTimidityConfig();
    }
}
