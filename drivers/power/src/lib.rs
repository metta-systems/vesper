#![no_std]

//! The power management block of the Raspberry Pi (`brcm,bcm2835-pm`): the
//! register values that halt and reset the board.
//!
//! The firmware reads the reset status register to learn which partition to
//! boot from. The partition number is spread over bits 0, 2, 4, 6, 8 and 10;
//! partition 63 tells the firmware to halt instead of booting. A reset through
//! the watchdog then stops the board.

/// Every write to the block carries this password in its top byte.
pub const PASSWORD: u32 = 0x5a00_0000;

/// Reset control: the bits of the watchdog configuration field.
const RSTC_WRCFG_CLEAR: u32 = 0xffff_ffcf;
/// Reset control: a full reset when the watchdog expires.
const RSTC_WRCFG_FULL_RESET: u32 = 0x0000_0020;

/// Reset status: partition 63, the firmware's request to halt.
const RSTS_HALT: u32 = 0x555;

/// Watchdog ticks before the reset (about 150 microseconds).
const WATCHDOG_TICKS: u32 = 10;

/// Offsets of the registers in the block's first range.
pub mod register {
    pub const RESET_CONTROL: u64 = 0x1c;
    pub const RESET_STATUS: u64 = 0x20;
    pub const WATCHDOG: u64 = 0x24;
}

/// The reset status that makes the firmware halt after the next reset.
pub const fn halt_status(current: u32) -> u32 {
    current | PASSWORD | RSTS_HALT
}

/// The reset control that makes the watchdog's expiry a full reset.
pub const fn full_reset_control(current: u32) -> u32 {
    (current & RSTC_WRCFG_CLEAR) | PASSWORD | RSTC_WRCFG_FULL_RESET
}

/// The watchdog value that starts the countdown.
pub const fn watchdog_start() -> u32 {
    PASSWORD | WATCHDOG_TICKS
}
