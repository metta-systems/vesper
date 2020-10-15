/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 */

//! Implementation of aarch64 kernel functions.

mod boot;
pub mod memory;
pub mod traps;

/// Loop forever in sleep mode.
#[inline]
pub fn endless_sleep() -> ! {
    loop {
        cortex_a::asm::wfe();
    }
}
