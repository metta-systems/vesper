// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2020-2022 Andre Richter <andre.o.richter@gmail.com>

//! Processor code.

#[cfg(target_arch = "aarch64")]
use crate::arch::aarch64::cpu as arch_cpu;

pub mod smp;

//--------------------------------------------------------------------------------------------------
// Architectural Public Reexports
//--------------------------------------------------------------------------------------------------
pub use arch_cpu::{endless_sleep, nop};

// #[cfg(feature = "test_build")]
// pub use arch_cpu::{qemu_exit_failure, qemu_exit_success};

/// Loop for a given number of `nop` instructions.
#[inline]
pub fn loop_delay(rounds: u32) {
    for _ in 0..rounds {
        nop();
    }
}

/// Loop until a passed function returns `true`.
#[inline]
pub fn loop_until<F: Fn() -> bool>(f: F) {
    loop {
        if f() {
            break;
        }
        nop();
    }
}

/// Loop while a passed function returns `true`.
#[inline]
pub fn loop_while<F: Fn() -> bool>(f: F) {
    loop {
        if !f() {
            break;
        }
        nop();
    }
}
