#![no_std]

//! The SPI0 master of the BCM2835 and BCM2711 (`brcm,bcm2835-spi`): its
//! registers, the clock divisor for a rate, and the polled write of a
//! transaction.
//!
//! Resources: BCM2711 ARM Peripherals, chapter 9: section 9.5 (the register
//! map, table 159: `CS` 0x00, `FIFO` 0x04, `CLK` 0x08, `DLEN` 0x0c, `LTOH`
//! 0x10, `DC` 0x14; `CS` table 160; `CLK` table 162: "SCLK = Core Clock /
//! CDIV", even, 0 meaning 65536) and section 9.6.1 (the polled transfer: set
//! the mode and `TA`, write `FIFO` while `TXD` and read it while `RXD`, wait
//! for `DONE`, clear `TA`). The SCLK pauses while the RX FIFO is full, so a
//! write-only transaction still drains it.
//!
//! The chip select is a GPIO the component drives around the transaction (the
//! device tree's `cs-gpios`), not the block's own `CE0`/`CE1`.

use {arbitrary_int::u2, bitbybit::bitfield, derive_mmio::Mmio, libmmio::assert_register_offsets};

/// SPI master control and status (`CS`, table 160).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct ControlStatus {
    /// The block's own chip select line (unused: the GPIO chip select is
    /// driven instead)
    #[bits(0..=1, rw)]
    chip_select: u2,
    /// Clock phase: the first SCLK transition at the start of a data bit
    #[bit(2, rw)]
    cpha: bool,
    /// Clock polarity: SCLK idles high
    #[bit(3, rw)]
    cpol: bool,
    /// Clear the TX (bit 4) and RX (bit 5) FIFOs (write 1, reads as 0)
    #[bits(4..=5, rw)]
    clear: u2,
    /// A transfer is active: the block clocks out what the FIFO holds
    #[bit(7, rw)]
    transfer_active: bool,
    /// Read enable (bidirectional mode only; reset 1)
    #[bit(12, rw)]
    read_enable: bool,
    /// The transfer is complete: the TX FIFO is empty and the last byte out
    #[bit(16, r)]
    done: bool,
    /// The RX FIFO holds at least one byte
    #[bit(17, r)]
    rx_data: bool,
    /// The TX FIFO has room for at least one byte
    #[bit(18, r)]
    tx_room: bool,
    /// The RX FIFO is full: SCLK pauses until it is read
    #[bit(20, r)]
    rx_full: bool,
}

/// The block's registers.
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct RegisterBlock {
    control_status: ControlStatus,
    /// `FIFO`: a byte written goes out, a byte read came in
    fifo: u32,
    /// `CLK`: `CDIV`, the divisor of the core clock
    clock: u32,
    /// `DLEN`: bytes of a DMA transfer (unused in polled mode)
    data_length: u32,
    /// `LTOH`: the `LoSSI` output hold delay (unused)
    lossi_hold: u32,
    /// `DC`: DMA DREQ thresholds (unused in polled mode)
    dma_controls: u32,
}

assert_register_offsets!(RegisterBlock, 0x18, {
    control_status: 0x00,
    fifo: 0x04,
    clock: 0x08,
    data_length: 0x0c,
    lossi_hold: 0x10,
    dma_controls: 0x14,
});

/// The largest divisor: `CDIV` 0 (table 162).
pub const MAX_DIVISOR: u32 = 65_536;

/// The smallest even divisor of `core_hz` whose rate is not above `max_hz`
/// (at least 2, at most [`MAX_DIVISOR`]): sized against the clock's fastest
/// rate, SCLK never exceeds `max_hz` however the firmware scales the clock.
#[must_use]
pub fn divisor(core_hz: u32, max_hz: u32) -> u32 {
    let at_least = core_hz.div_ceil(max_hz.max(1));
    at_least.next_multiple_of(2).clamp(2, MAX_DIVISOR)
}

/// The `CLK` value of `divisor` (65536 is written as 0).
#[must_use]
pub const fn clock_register(divisor: u32) -> u32 {
    if divisor >= MAX_DIVISOR { 0 } else { divisor }
}

/// The SCLK `divisor` gives at `core_hz`.
#[must_use]
pub const fn rate(core_hz: u32, divisor: u32) -> u32 {
    core_hz / divisor
}

/// An SPI mode: clock polarity and phase (modes 0 to 3: CPOL is bit 1, CPHA
/// bit 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mode {
    pub cpol: bool,
    pub cpha: bool,
}

impl Mode {
    /// SPI mode `number` (0 to 3).
    #[must_use]
    pub const fn numbered(number: u8) -> Self {
        Self {
            cpol: number & 2 != 0,
            cpha: number & 1 != 0,
        }
    }
}

/// `CS` with `mode`, the transfer inactive and nothing cleared: what every
/// write of the sequence starts from.
#[must_use]
pub fn idle(mode: Mode) -> ControlStatus {
    ControlStatus::ZERO
        .with_cpol(mode.cpol)
        .with_cpha(mode.cpha)
        .with_read_enable(true)
}

/// The block as the write sequence uses it, so the sequence can be checked
/// on a recording stand-in.
pub trait Bus {
    fn control_status(&mut self) -> ControlStatus;
    fn set_control_status(&mut self, value: ControlStatus);
    fn push(&mut self, byte: u8);
    fn pop(&mut self) -> u8;
}

/// The controller did not finish the transaction before `wait` gave up.
#[derive(Debug, PartialEq, Eq)]
pub struct TimedOut;

/// Write `bytes` in `mode` (section 9.6.1): clear both FIFOs, set `TA`, write
/// each byte while `TXD` says the FIFO has room and drain the RX FIFO while
/// `RXD`, wait for `DONE`, drain it again, clear `TA`. `wait` runs at each
/// poll that finds the block not ready and returns false once the caller
/// stops waiting; `TA` is cleared either way.
///
/// # Errors
///
/// [`TimedOut`] when `wait` gave up.
pub fn write(
    bus: &mut impl Bus,
    mode: Mode,
    bytes: impl IntoIterator<Item = u8>,
    mut wait: impl FnMut() -> bool,
) -> Result<(), TimedOut> {
    let idle = idle(mode);
    bus.set_control_status(idle.with_clear(u2::new(0b11)));
    bus.set_control_status(idle.with_transfer_active(true));
    let mut result = Ok(());
    'bytes: for byte in bytes {
        loop {
            let status = bus.control_status();
            if status.rx_data() {
                let _discarded = bus.pop();
            }
            if status.tx_room() {
                bus.push(byte);
                break;
            }
            if !wait() {
                result = Err(TimedOut);
                break 'bytes;
            }
        }
    }
    while result.is_ok() {
        let status = bus.control_status();
        if status.rx_data() {
            let _discarded = bus.pop();
            continue;
        }
        if status.done() {
            break;
        }
        if !wait() {
            result = Err(TimedOut);
        }
    }
    bus.set_control_status(idle);
    result
}

/// The bus of a mapped register block.
pub struct Registers<'registers, 'block>(pub &'registers mut MmioRegisterBlock<'block>);

impl Bus for Registers<'_, '_> {
    fn control_status(&mut self) -> ControlStatus {
        self.0.read_control_status()
    }

    fn set_control_status(&mut self, value: ControlStatus) {
        self.0.write_control_status(value);
    }

    fn push(&mut self, byte: u8) {
        self.0.write_fifo(u32::from(byte));
    }

    fn pop(&mut self) -> u8 {
        (self.0.read_fifo() & 0xff) as u8
    }
}

/// Program the divisor for `max_hz` from the core clock's fastest rate
/// `core_max_hz`; returns the rate it gives.
pub fn set_rate(registers: &mut MmioRegisterBlock<'_>, core_max_hz: u32, max_hz: u32) -> u32 {
    let divisor = divisor(core_max_hz, max_hz);
    registers.write_clock(clock_register(divisor));
    rate(core_max_hz, divisor)
}
