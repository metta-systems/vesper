/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! The hardware chainboot needs to talk to its host: the PL011 UART on header pins
//! 14 and 15, polled, the firmware mailbox that tells the UART's clock and
//! switches the LED, and a few facts about the board. Nothing here is a driver
//! framework: [`init`] does what bring-up takes, once. The UART, its pins and the
//! mailbox are `libboot_uart`'s, which Kickstart's early console shares.

pub mod led;
#[cfg(feature = "mmu")]
pub mod mmu;

pub use libboot_uart::{GPIO_BASE, gpio, mailbox, pl011};
use libboot_uart::{DEFAULT_UART_CLOCK, PL011_BASE};

#[cfg(not(any(board_rpi3, board_rpi4)))]
compile_error!("No platform selected - specify TARGET_BOARD in configuration");

/// Name of the board family chainboot is built for.
#[cfg(board_rpi3)]
const BOARD_FAMILY: &str = "Raspberry Pi 3";
#[cfg(board_rpi4)]
const BOARD_FAMILY: &str = "Raspberry Pi 4";

/// The model with this revision code: a new-style code (bit 23) has the type in
/// bits 11:4. Anything else is only known to be of the family chainboot is built
/// for.
#[must_use]
pub fn model_name(revision: Option<u32>) -> &'static str {
    const NEW_STYLE: u32 = 1 << 23;
    let Some(revision) = revision.filter(|&code| code & NEW_STYLE != 0) else {
        return BOARD_FAMILY;
    };
    match (revision >> 4) & 0xff {
        0x08 => "Raspberry Pi 3 Model B",
        0x0a => "Raspberry Pi Compute Module 3",
        0x0d => "Raspberry Pi 3 Model B+",
        0x0e => "Raspberry Pi 3 Model A+",
        0x10 => "Raspberry Pi Compute Module 3+",
        0x12 => "Raspberry Pi Zero 2 W",
        0x11 => "Raspberry Pi 4 Model B",
        0x13 => "Raspberry Pi 400",
        0x14 => "Raspberry Pi Compute Module 4",
        _ => BOARD_FAMILY,
    }
}

/// What the counter frequency register holds, in Hz. The firmware's stub is meant
/// to set it; a program that times anything by it fails without a sound when it
/// is zero.
#[must_use]
pub fn counter_frequency() -> u64 {
    use {aarch64_cpu::registers::CNTFRQ_EL0, tock_registers::interfaces::Readable};
    CNTFRQ_EL0.get()
}

/// Whether the MMU is on, as a word.
#[must_use]
pub fn mmu_state() -> &'static str {
    #[cfg(feature = "mmu")]
    {
        if mmu::enabled() {
            "on"
        } else {
            "off, it did not come on"
        }
    }
    #[cfg(not(feature = "mmu"))]
    {
        "off, not built in"
    }
}

/// Where the firmware loads chainboot's kernel to be run from: the kernel is
/// received here and jumped to.
pub const KERNEL_LOAD_ADDRESS: u64 = 0x8_0000;

// SAFETY: the PL011 is at this address on every board chainboot is built for, the
// MMU is off so the address is the physical one, and nothing else uses the UART.
static UART: pl011::Pl011 = unsafe { pl011::Pl011::new(PL011_BASE) };

/// The clock the UART runs at.
#[derive(Clone, Copy, Debug)]
pub struct UartClock {
    /// In Hz.
    pub rate: u32,
    /// Whether the firmware said so; otherwise it is the default.
    pub reported: bool,
}

/// What bring-up found out.
#[derive(Clone, Copy, Debug)]
pub struct Bringup {
    pub uart_clock: UartClock,
    /// Whether the firmware says the UART clock runs, if it said.
    pub clock_running: Option<bool>,
    /// The board's revision code, if the firmware said.
    pub board_revision: Option<u32>,
}

/// Route the PL011 to the header pins, ask the firmware to run its clock, set it
/// up for that clock and make it the console.
///
/// There is no console to say what happens, so the LED does. After asking about
/// the UART clock it flashes once when the firmware says the clock runs, twice
/// when it says the clock is stopped, and three times when it does not say. It
/// stays off while the UART is set up and goes on when that is done. A stopped
/// clock leaves the UART alone: touching a peripheral without its clock may hang
/// the core, and then nothing would tell what went wrong.
///
/// # Errors
///
/// If the UART cannot be set up.
pub fn init() -> Result<Bringup, &'static str> {
    // The pins go first: the UART should not drive them before they are its.
    // SAFETY: as for `UART`, for the GPIO registers.
    unsafe {
        gpio::route_uart(GPIO_BASE);
    }
    let board_revision = mailbox::board_revision().ok();
    led::init(board_revision);

    // The LED is on while the MMU comes on, and stays on if it does not.
    #[cfg(feature = "mmu")]
    {
        let _lit = led::set(true);
        mmu::enable();
    }

    let _started = mailbox::start_uart_clock();
    let running = mailbox::uart_clock_running().ok();
    led::flash(match running {
        Some(true) => 1,
        Some(false) => 2,
        None => 3,
    });
    let clock = match mailbox::uart_clock_rate() {
        Ok(rate) => UartClock {
            rate,
            reported: true,
        },
        Err(_error) => UartClock {
            rate: DEFAULT_UART_CLOCK,
            reported: false,
        },
    };
    let bringup = Bringup {
        uart_clock: clock,
        clock_running: running,
        board_revision,
    };
    if running == Some(false) {
        return Ok(bringup);
    }

    UART.init(clock.rate)?;
    let _lit = led::set(true);
    libconsole::console::register_console(&UART);
    Ok(bringup)
}

/// Switch the UART, running at `clock` Hz, to `baud_rate` once what was written
/// has left the line.
///
/// # Errors
///
/// If the rate is out of range or the transmitter does not drain; the UART then
/// stays at its rate.
pub fn set_uart_rate(clock: u32, baud_rate: u32) -> Result<(), &'static str> {
    UART.set_baud_rate(clock, baud_rate)
}
