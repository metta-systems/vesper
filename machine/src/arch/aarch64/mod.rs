/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Implementation of aarch64 kernel functions.

use cortex_a::asm;

mod boot;
#[cfg(feature = "jtag")]
pub mod jtag;
pub mod memory;
pub mod traps;

/// Loop forever in sleep mode.
#[inline]
pub fn endless_sleep() -> ! {
    loop {
        asm::wfe();
    }
}

/// Loop for a given number of `nop` instructions.
#[inline]
pub fn loop_delay(rounds: u32) {
    for _ in 0..rounds {
        asm::nop();
    }
}

/// Loop until a passed function returns `true`.
#[inline]
pub fn loop_until<F: Fn() -> bool>(f: F) {
    loop {
        if f() {
            break;
        }
        asm::nop();
    }
}

/// Loop while a passed function returns `true`.
#[inline]
pub fn loop_while<F: Fn() -> bool>(f: F) {
    loop {
        if !f() {
            break;
        }
        asm::nop();
    }
}
