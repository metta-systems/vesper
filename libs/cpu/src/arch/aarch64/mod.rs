/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Implementation of aarch64 CPU functions.

use aarch64_cpu::asm;

pub mod smp;

/// Expose CPU-specific no-op opcode.
pub use asm::nop;

/// Loop forever in sleep mode.
#[inline]
pub fn endless_sleep() -> ! {
    loop {
        asm::wfe();
    }
}
