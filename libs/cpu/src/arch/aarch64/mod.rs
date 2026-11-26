/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Implementation of aarch64 CPU functions.

use aarch64_cpu::asm;

pub mod cache;
pub mod smp;

/// Expose CPU-specific no-op opcode.
pub use asm::nop;

/// Wake the cores waiting in `wfe` for what this core just stored.
///
/// `sev` does not wait for earlier stores: a waiter woken before a store
/// reaches it reads the old value, waits again, and no second event comes. The
/// `dsb ish` before it completes this core's stores for the Inner Shareable
/// domain, the cores, first (Arm ARM, "Wait for Event and Send Event").
#[inline]
pub fn send_event() {
    asm::barrier::dsb(asm::barrier::ISH);
    asm::sev();
}

/// Loop forever in sleep mode.
#[inline]
pub fn endless_sleep() -> ! {
    loop {
        asm::wfe();
    }
}
