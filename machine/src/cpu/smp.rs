// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2018-2022 Andre Richter <andre.o.richter@gmail.com>

//! Symmetric multiprocessing.

#[cfg(target_arch = "aarch64")]
use crate::arch::aarch64::cpu::smp as arch_smp;

//--------------------------------------------------------------------------------------------------
// Architectural Public Reexports
//--------------------------------------------------------------------------------------------------
pub use arch_smp::core_id;
