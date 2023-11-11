/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

#![allow(dead_code)]

pub mod cpu;
pub mod device_driver;
pub mod display;
pub mod drivers;
pub mod exception;
// pub mod fb;
pub mod memory;
// pub mod vc;

/// See BCM2835-ARM-Peripherals.pdf
/// See <https://www.raspberrypi.org/forums/viewtopic.php?t=186090> for more details.

pub struct BcmHost;

// Per <https://www.raspberrypi.com/documentation/computers/raspberry-pi.html#peripheral-addresses>:
//
// SoC     Peripheral Address	Peripheral Size	SDRAM Address	Source
// BCM2835 0x20000000           0x01000000      0x40000000      <https://github.com/raspberrypi/linux/blob/7f465f823c2ecbade5877b8bbcb2093a8060cb0e/arch/arm/boot/dts/bcm2835.dtsi#L21>
// BCM2836 0x3f000000           0x01000000      0xc0000000      <https://github.com/raspberrypi/linux/blob/7f465f823c2ecbade5877b8bbcb2093a8060cb0e/arch/arm/boot/dts/bcm2836.dtsi#L10>
// BCM2837 0x3f000000           0x01000000      0xc0000000      <https://github.com/raspberrypi/linux/blob/7f465f823c2ecbade5877b8bbcb2093a8060cb0e/arch/arm/boot/dts/bcm2837.dtsi#L9>
// BCM2711 0xfe000000           0x01800000      0xc0000000      <https://github.com/raspberrypi/linux/blob/7f465f823c2ecbade5877b8bbcb2093a8060cb0e/arch/arm/boot/dts/bcm2711.dtsi#L41>

// <https://www.raspberrypi.com/documentation/computers/processors.html>
// The BCM2835 is the Broadcom chip used in the Raspberry Pi Model A, B, B+, the Compute Module, and the Raspberry Pi Zero.
// The BCM2836 is used in the Raspberry Pi 2 Model B.
// The BCM2837 is used in the Raspberry Pi 3, and in later models of the Raspberry Pi 2.
// The BCM2837B0 is used in the Raspberry Pi 3B+ and 3A+.
// The BCM2711 is used in the Raspberry Pi 4 Model B.
// RP3A0 (BCM2710A1 — which is the die packaged inside the BCM2837 chip - Raspberry Pi 3) used in Raspberry Pi Zero 2 W

// Machine   Board  Chip
// raspi1    raspi  bcm2835
// raspi1    raspi  bcm2835
// raspi3b+  raspi  bcm2837
// raspi4    raspi  bcm2711

impl BcmHost {
    /// At which address to load the kernel binary.
    pub const fn kernel_load_address() -> u64 {
        0x8_0000
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

// RasPi3B+
#[cfg(feature = "rpi3")]
impl BcmHost {
    /// Name of the hardware device this BcmHost is compiled for.
    pub const fn board_name() -> &'static str {
        "Raspberry Pi 3+"
    }

    /// This returns the ARM-side physical address where peripherals are mapped.
    ///
    pub const fn get_peripheral_address() -> usize {
        0x3f00_0000
    }

    /// This returns the size of the peripherals' space.
    pub const fn get_peripheral_size() -> usize {
        0x0100_0000
    }

    /// This returns the bus address of the SDRAM.
    pub const fn get_sdram_address() -> usize {
        0xc000_0000 // uncached
    }
}

// RasPi4
#[cfg(feature = "rpi4")]
impl BcmHost {
    /// Name of the hardware device this BcmHost is compiled for.
    pub const fn board_name() -> &'static str {
        "Raspberry Pi 4+"
    }

    /// This returns the ARM-side physical address where peripherals are mapped.
    ///
    pub const fn get_peripheral_address() -> usize {
        0xfe00_0000
    }

    /// This returns the size of the peripherals' space.
    pub const fn get_peripheral_size() -> usize {
        0x0180_0000
    }

    /// This returns the bus address of the SDRAM.
    pub const fn get_sdram_address() -> usize {
        0xc000_0000 // uncached
    }
}
