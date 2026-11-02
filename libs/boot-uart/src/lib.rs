/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! The PL011 UART on header pins 14 and 15 before any driver owns it: routing
//! the pins ([`gpio`]), the UART clock from the firmware ([`mailbox`]) and the
//! polled UART itself ([`pl011`]). Chainboot talks to its host through it, and
//! Kickstart's early console prints through it and negotiates its rate.
//!
//! Both run it with the MMU off (chainboot optionally on a flat map with the data
//! cache off), where a store-exclusive never completes on the board: nothing here
//! uses an atomic read-modify-write. `just build-chainboot` audits chainboot's
//! binary for them.
//!
//! See <https://www.raspberrypi.com/documentation/computers/raspberry-pi.html#peripheral-addresses>
//! for where the peripherals sit.

#![no_std]

pub mod gpio;
pub mod mailbox;
pub mod pl011;

/// ARM-side physical address of the peripherals.
#[cfg(board_rpi3)]
pub const PERIPHERALS: usize = 0x3f00_0000;
#[cfg(board_rpi4)]
pub const PERIPHERALS: usize = 0xfe00_0000;

/// Where the GPIO registers are (BCM2837 ARM Peripherals, section 6.1).
pub const GPIO_BASE: usize = PERIPHERALS + 0x20_0000;

/// Where the PL011's registers are (BCM2837 ARM Peripherals, section 13.4).
pub const PL011_BASE: usize = PERIPHERALS + 0x20_1000;

/// The UART clock the firmware sets up by default, when it does not tell.
pub const DEFAULT_UART_CLOCK: u32 = 48_000_000;
