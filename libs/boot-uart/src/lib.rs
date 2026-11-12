/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! The PL011 UART the boot programs talk to the host over, before any driver
//! owns it: routing the header pins 14 and 15 to it where the board has them
//! ([`gpio`]), the UART clock from the firmware ([`mailbox`]) and the polled
//! UART itself ([`pl011`]). Chainboot talks to its host through it, and
//! Kickstart's early console prints through it and negotiates its rate.
//!
//! Every address comes from the device tree the firmware passes
//! (`libdevicetree`'s board helpers), so the same code runs on every board.
//!
//! Both run it with the MMU off (chainboot optionally on a flat map with the data
//! cache off), where a store-exclusive never completes on the board: nothing here
//! uses an atomic read-modify-write. `just build-chainboot` audits chainboot's
//! binary for them.

#![no_std]

pub mod gpio;
pub mod mailbox;
pub mod pl011;

use libdevicetree::{BootHardware, Soc, UartClock};

/// Whether the image is built for boards with `soc` (the board set's
/// `board_rpiN` flags).
#[must_use]
pub const fn built_for(soc: Soc) -> bool {
    match soc {
        Soc::Bcm2837 => cfg!(board_rpi3),
        Soc::Bcm2711 => cfg!(board_rpi4),
        Soc::Bcm2712 => cfg!(board_rpi5),
    }
}

/// The boot console hardware of the board this core's type points to, when the
/// device tree cannot be read and the image is built for that board: a guess,
/// to say what went wrong.
#[must_use]
pub fn guess_boot_hardware() -> Option<(Soc, BootHardware)> {
    use aarch64_cpu::registers::{MIDR_EL1, Readable};
    let soc = Soc::from_cpu_part(MIDR_EL1.read(MIDR_EL1::PartNum))?;
    built_for(soc).then(|| (soc, soc.fallback_boot_hardware()))
}

/// Use `hardware`'s mailbox (if it has one) and UART from now on, `uart` as the
/// UART.
///
/// # Safety
///
/// `hardware` describes the board's registers, accessible for as long as the
/// program runs, with the MMU off or on a flat map, and nothing else uses them.
///
/// # Errors
///
/// If the UART's address does not fit a pointer.
pub unsafe fn attach(hardware: &BootHardware, uart: &pl011::Pl011) -> Result<(), &'static str> {
    let base = usize::try_from(hardware.uart.base).map_err(|_error| "the PL011 is out of reach")?;
    if let Some(found) = hardware.mailbox {
        // SAFETY: the caller's contract.
        unsafe {
            mailbox::attach(found);
        }
    }
    // SAFETY: the caller's contract.
    unsafe {
        uart.attach(base);
    }
    Ok(())
}

/// Route the PL011 to the header pins, where the board has them on a Broadcom
/// GPIO block.
///
/// # Safety
///
/// As for [`attach`].
pub unsafe fn route_header_pins(hardware: &BootHardware) {
    if let Some(header_gpio) = hardware.header_gpio {
        // SAFETY: the caller's contract.
        unsafe {
            gpio::route_uart(header_gpio);
        }
    }
}

/// The UART clock the firmware sets up by default, when it does not tell.
pub const DEFAULT_UART_CLOCK: u32 = 48_000_000;

/// The UART's clock as the boot programs found it.
#[derive(Clone, Copy, Debug)]
pub struct ClockRate {
    /// In Hz.
    pub rate: u32,
    /// Where the rate comes from.
    pub source: ClockSource,
}

/// Where a [`ClockRate`] comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockSource {
    /// A fixed clock in the device tree.
    DeviceTree,
    /// The firmware said so.
    Firmware,
    /// [`DEFAULT_UART_CLOCK`]: the firmware did not say.
    Assumed,
}

/// Whether the UART clock runs, as far as anyone can tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockState {
    Running,
    Stopped,
    /// The firmware did not answer.
    Unknown,
}

/// Get the UART's clock running, and say at what rate. A fixed clock in the
/// device tree runs by itself; a clock the firmware owns is started through the
/// mailbox, which must be attached.
#[must_use]
pub fn start_uart_clock(clock: UartClock) -> (ClockState, ClockRate) {
    match clock {
        UartClock::Fixed(rate) => (
            ClockState::Running,
            ClockRate {
                rate,
                source: ClockSource::DeviceTree,
            },
        ),
        UartClock::Firmware => {
            let _started = mailbox::start_uart_clock();
            let state = match mailbox::uart_clock_running() {
                Ok(true) => ClockState::Running,
                Ok(false) => ClockState::Stopped,
                Err(_error) => ClockState::Unknown,
            };
            let rate = mailbox::uart_clock_rate().map_or(
                ClockRate {
                    rate: DEFAULT_UART_CLOCK,
                    source: ClockSource::Assumed,
                },
                |rate| ClockRate {
                    rate,
                    source: ClockSource::Firmware,
                },
            );
            (state, rate)
        }
    }
}
