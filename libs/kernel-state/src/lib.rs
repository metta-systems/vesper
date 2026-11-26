// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2020-2022 Andre Richter <andre.o.richter@gmail.com>

#![no_std]
#![no_main]

//! Whether the kernel is in its init phase, the only state `liblocking::InitStateLock`
//! checks.
//!
//! Each image that links this crate (Kickstart, the nucleus) has its own flag. Nothing ends
//! the init phase yet, so the flag stays set for as long as the image runs.

use core::sync::atomic::{AtomicBool, Ordering};

/// Set while the kernel is in its init phase.
static INIT_PHASE: AtomicBool = AtomicBool::new(true);

/// Whether the kernel is in its init phase.
pub fn is_init() -> bool {
    INIT_PHASE.load(Ordering::Acquire)
}
