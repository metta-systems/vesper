/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

#![allow(dead_code)]

pub mod display;
pub mod fb;
pub mod gpio;
pub mod mailbox;
pub mod mini_uart;
pub mod pl011_uart;
pub mod power;
pub mod vc;

/// See BCM2835-ARM-Peripherals.pdf
/// See <https://www.raspberrypi.org/forums/viewtopic.php?t=186090> for more details.

pub struct BcmHost;

impl BcmHost {
    /// At which address to load the kernel binary.
    pub const fn kernel_load_address() -> u64 {
        0x8_0000
    }

    /// This returns the ARM-side physical address where peripherals are mapped.
    ///
    /// As per <https://www.raspberrypi.org/documentation/hardware/raspberrypi/peripheral_addresses.md>
    /// BCM SOC could address only 1Gb of memory, so 0x4000_0000 is the high watermark.
    pub const fn get_peripheral_address() -> usize {
        0x3f00_0000 // FIXME: rpi3, 0xfe for rpi4
    }

    /// This returns the size of the peripherals' space.
    pub const fn get_peripheral_size() -> usize {
        0x0100_0000
    }

    /// This returns the bus address of the SDRAM.
    pub const fn get_sdram_address() -> usize {
        0xc000_0000 // uncached
    }

    /// As per <https://www.raspberrypi.org/forums/viewtopic.php?p=1170522#p1170522>
    ///
    pub fn bus2phys(bus: usize) -> usize {
        bus & !0xc000_0000
    }

    pub fn phys2bus(phys: usize) -> usize {
        phys | 0xc000_0000
    }
}
