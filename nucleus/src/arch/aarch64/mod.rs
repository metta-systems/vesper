/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Implementation of aarch64 kernel functions.

use cortex_a::{asm, regs::*};

mod boot;
#[cfg(feature = "jtag")]
pub mod jtag;
pub mod memory;
pub mod traps;

pub use self::memory::{PhysAddr, VirtAddr};

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

#[inline]
pub fn flushcache(address: usize) {
    unsafe {
        asm!("dc ivac, {addr}", addr = in(reg) address);
    }
}

#[inline]
pub fn read_cpu_id() -> u64 {
    const CORE_MASK: u64 = 0x3;
    MPIDR_EL1.get() & CORE_MASK
}

#[inline]
pub fn current_el() -> u32 {
    CurrentEL.get()
}

pub fn read_translation_table_base() -> PhysAddr {
    TTBR0_EL1.get_baddr().into()
}

pub fn write_translation_table_base(base: PhysAddr) {
    TTBR0_EL1.set_baddr(base.into());
}

pub fn read_translation_control() -> u64 {
    TCR_EL1.get()
}
