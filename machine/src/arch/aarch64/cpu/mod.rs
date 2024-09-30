// SPDX-FileCopyrightText: 2024 Metta Systems OÜ
// SPDX-FileContributor: Berkus

use aarch64_cpu::asm;

#[cfg(not(feature = "no_boot"))] // Move this to nucleus??
pub mod boot;
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
