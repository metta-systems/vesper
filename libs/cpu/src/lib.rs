/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */
#![no_std]
#![no_main]

//! Implementation of CPU functions.

pub mod arch;

//--------------------------------------------------------------------------------------------------
// Architectural Public Reexports
//--------------------------------------------------------------------------------------------------
#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::smp;
#[cfg(target_arch = "aarch64")]
pub use crate::arch::aarch64::{endless_sleep, nop};
