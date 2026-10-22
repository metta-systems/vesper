#![no_std]

//! The power management block of the Raspberry Pi (`brcm,bcm2835-pm`): the
//! registers and values that halt and reset the board.
//!
//! The firmware reads the reset status register to learn which partition to
//! boot from. The partition number is spread over bits 0, 2, 4, 6, 8 and 10;
//! partition 63 tells the firmware to halt instead of booting. A reset through
//! the watchdog then stops the board.

use {
    arbitrary_int::{u2, u6, u20},
    bitbybit::bitfield,
    derive_mmio::Mmio,
    libmmio::assert_register_offsets,
};

/// Every write to the block carries this password in its top byte.
pub const PASSWORD: u8 = 0x5a;

/// Reset control: the watchdog's expiry is a full reset.
const WATCHDOG_FULL_RESET: u2 = u2::new(0b10);

/// Reset status: partition 63, the firmware's request to halt.
const HALT_PARTITION: u6 = u6::new(63);

/// Watchdog ticks before the reset (about 150 microseconds).
const WATCHDOG_TICKS: u20 = u20::new(10);

/// Reset control (`PM_RSTC`).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct ResetControl {
    /// What the watchdog's expiry does
    #[bits(4..=5, rw)]
    watchdog_config: u2,
    /// The password a write needs
    #[bits(24..=31, rw)]
    password: u8,
}

/// Reset status (`PM_RSTS`).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct ResetStatus {
    /// The partition the firmware boots after the next reset
    #[bits([0, 2, 4, 6, 8, 10], rw)]
    partition: u6,
    /// The password a write needs
    #[bits(24..=31, rw)]
    password: u8,
}

/// Watchdog (`PM_WDOG`).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Watchdog {
    /// Ticks before the watchdog expires
    #[bits(0..=19, rw)]
    ticks: u20,
    /// The password a write needs
    #[bits(24..=31, rw)]
    password: u8,
}

/// The power management block's reset registers.
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct RegisterBlock {
    _reserved: [u32; 7],
    reset_control: ResetControl,
    reset_status: ResetStatus,
    watchdog: Watchdog,
}

assert_register_offsets!(RegisterBlock, 0x28, {
    reset_control: 0x1c,
    reset_status: 0x20,
    watchdog: 0x24,
});

/// The reset status that makes the firmware halt after the next reset.
pub const fn halt_status(current: ResetStatus) -> ResetStatus {
    current
        .with_partition(HALT_PARTITION)
        .with_password(PASSWORD)
}

/// The reset control that makes the watchdog's expiry a full reset.
pub const fn full_reset_control(current: ResetControl) -> ResetControl {
    current
        .with_watchdog_config(WATCHDOG_FULL_RESET)
        .with_password(PASSWORD)
}

/// The watchdog value that starts the countdown.
pub const fn watchdog_start() -> Watchdog {
    Watchdog::ZERO
        .with_ticks(WATCHDOG_TICKS)
        .with_password(PASSWORD)
}
