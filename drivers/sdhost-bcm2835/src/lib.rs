#![no_std]

//! Register logic of the BCM2835 SDHOST controller, the Raspberry Pi's SD card
//! interface (`brcm,bcm2835-sdhost`), and the SD card protocol on top of it
//! ([`card`]).
//!
//! The controller runs by programmed IO: the driver moves every data word
//! through the data register, so the data area needs no DMA mapping. It sends
//! one command at a time and polls for its end; the card clock divides the
//! core clock, which the firmware reports.
//!
//! Resources: Broadcom never documented the controller, so the register layout
//! and its quirks follow Linux `drivers/mmc/host/bcm2835.c` and QEMU
//! `hw/sd/bcm2835_sdhost.c`; the card protocol follows the SD Physical Layer
//! Simplified Specification (see [`card`]).

pub mod card;

use {
    arbitrary_int::{u4, u5, u6, u11},
    bitbybit::bitfield,
    card::{BLOCK_BYTES, Bus, BusError, Command, Data, Response},
    derive_mmio::Mmio,
    libmmio::assert_register_offsets,
};

/// Command register: writing it with `new` set sends the command.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct CommandRegister {
    #[bits(0..=5, rw)]
    index: u6,
    /// The command reads a block from the card
    #[bit(6, rw)]
    read: bool,
    /// The command writes a block to the card
    #[bit(7, rw)]
    write: bool,
    /// A 136-bit response
    #[bit(9, rw)]
    long_response: bool,
    #[bit(10, rw)]
    no_response: bool,
    /// The command failed; the host status says why
    #[bit(14, rw)]
    fail: bool,
    /// Set to send; the controller clears it once the command ended
    #[bit(15, rw)]
    pending: bool,
}

/// Host status register; writing ones clears the error and event bits.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct HostStatus {
    #[bit(0, rw)]
    data_flag: bool,
    #[bit(3, rw)]
    fifo_error: bool,
    #[bit(4, rw)]
    crc7_error: bool,
    #[bit(5, rw)]
    crc16_error: bool,
    #[bit(6, rw)]
    command_timeout: bool,
    /// Read, erase or write timeout
    #[bit(7, rw)]
    data_timeout: bool,
}

impl HostStatus {
    /// Every error bit, as written to clear them.
    pub const ERRORS: Self = Self::new_with_raw_value(0xf8);

    /// Whether an error bit is set.
    #[must_use]
    pub const fn has_error(&self) -> bool {
        self.raw_value() & Self::ERRORS.raw_value() != 0
    }
}

/// Emergency debug mode register: the data state machine and the FIFO.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct DebugMode {
    /// The data state machine's state
    #[bits(0..=3, rw)]
    state: u4,
    /// Words in the FIFO
    #[bits(4..=8, rw)]
    fifo_level: u5,
    #[bits(9..=13, rw)]
    write_threshold: u5,
    #[bits(14..=18, rw)]
    read_threshold: u5,
    /// Kick a stuck state machine back to data mode
    #[bit(19, rw)]
    force_data_mode: bool,
}

/// Data state machine: identification mode.
pub const STATE_IDENTIFICATION: u8 = 0x0;
/// Data state machine: data mode, idle between transfers.
pub const STATE_DATA: u8 = 0x1;
/// Data state machine: waiting to read.
pub const STATE_READ_WAIT: u8 = 0x4;
/// Data state machine: starting a write.
pub const STATE_WRITE_START: u8 = 0xa;

/// FIFO threshold Linux uses for both directions, below the silicon bug's.
pub const FIFO_THRESHOLD: u8 = 4;

/// Words the FIFO holds.
pub const FIFO_WORDS: u8 = 16;

/// Host configuration register.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct HostConfig {
    /// The controller's internal bus is four bits wide
    #[bit(1, rw)]
    wide_internal_bus: bool,
    /// Four data lines to the card
    #[bit(2, rw)]
    wide_external_bus: bool,
    #[bit(3, rw)]
    slow_card: bool,
}

/// Clock divider register: the card clock is the core clock / (divider + 2).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct ClockDivider {
    #[bits(0..=10, rw)]
    divider: u11,
}

/// The SDHOST register block (Linux `bcm2835.c`).
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct RegisterBlock {
    command: CommandRegister,
    argument: u32,
    /// Card clocks a data stage may wait
    timeout: u32,
    clock_divider: ClockDivider,
    #[mmio(PureRead)]
    response: [u32; 4],
    host_status: HostStatus,
    _reserved0: [u32; 3],
    /// Card power: 1 on
    power: u32,
    debug_mode: DebugMode,
    host_config: HostConfig,
    /// Bytes of a block
    byte_count: u32,
    #[mmio(Read, Write)]
    data: u32,
    _reserved1: [u32; 3],
    /// Blocks of a transfer
    block_count: u32,
}

assert_register_offsets!(RegisterBlock, 0x54, {
    command: 0x00,
    argument: 0x04,
    timeout: 0x08,
    clock_divider: 0x0c,
    response: 0x10,
    host_status: 0x20,
    power: 0x30,
    debug_mode: 0x34,
    host_config: 0x38,
    byte_count: 0x3c,
    data: 0x40,
    block_count: 0x50,
});

/// The register value that sends `command`.
#[must_use]
pub fn command_register(command: Command) -> CommandRegister {
    CommandRegister::ZERO
        .with_index(u6::new(command.index & 0x3f))
        .with_read(command.data == Data::Read)
        .with_write(command.data == Data::Write)
        .with_long_response(command.response == Response::Long)
        .with_no_response(command.response == Response::None)
        .with_pending(true)
}

/// The divider that runs the card clock from `core_hz` at `hz` or below.
#[must_use]
pub fn clock_divider(core_hz: u32, hz: u32) -> u16 {
    let divisor = core_hz.div_ceil(hz.max(1)).max(2) - 2;
    u16::try_from(divisor.min(0x7ff)).unwrap_or(0x7ff)
}

/// The card clock `divider` gives from `core_hz`.
#[must_use]
pub const fn card_clock(core_hz: u32, divider: u16) -> u32 {
    core_hz / (divider as u32 + 2)
}

/// How long a command may take to end.
pub const COMMAND_MS: u64 = 100;
/// How long a block may take to move.
pub const BLOCK_MS: u64 = 500;
/// How long the card power takes to settle (Linux waits this long).
pub const POWER_MS: u32 = 20;

/// Words of a block.
const BLOCK_WORDS: u32 = BLOCK_BYTES / 4;

/// The controller, sending commands and moving blocks by programmed IO.
pub struct Sdhost<'registers> {
    registers: MmioRegisterBlock<'registers>,
    core_hz: u32,
    /// Milliseconds of a monotonic clock
    now_ms: fn() -> u64,
}

impl<'registers> Sdhost<'registers> {
    /// The controller at `registers`, whose card clock divides `core_hz`.
    pub fn new(
        registers: MmioRegisterBlock<'registers>,
        core_hz: u32,
        now_ms: fn() -> u64,
    ) -> Self {
        Self {
            registers,
            core_hz,
            now_ms,
        }
    }

    /// Whether `done` held within `milliseconds`.
    fn wait_until(&mut self, milliseconds: u64, mut done: impl FnMut(&mut Self) -> bool) -> bool {
        let end = (self.now_ms)() + milliseconds;
        loop {
            if done(self) {
                return true;
            }
            if (self.now_ms)() >= end {
                return false;
            }
            core::hint::spin_loop();
        }
    }

    /// Reset the controller and power-cycle the card, as Linux does: power
    /// off, every register cleared, the FIFO thresholds lowered, power on.
    pub fn reset(&mut self) {
        self.registers.write_power(0);
        self.registers.write_command(CommandRegister::ZERO);
        self.registers.write_argument(0);
        self.registers.write_timeout(0x00f0_0000);
        self.registers.write_clock_divider(ClockDivider::ZERO);
        self.registers
            .write_host_status(HostStatus::new_with_raw_value(0x7f8));
        self.registers.write_host_config(HostConfig::ZERO);
        self.registers.write_byte_count(0);
        self.registers.write_block_count(0);
        self.registers.modify_debug_mode(|mode| {
            mode.with_read_threshold(u5::new(FIFO_THRESHOLD))
                .with_write_threshold(u5::new(FIFO_THRESHOLD))
        });
        self.delay_ms(POWER_MS);
        self.registers.write_power(1);
        self.delay_ms(POWER_MS);
        self.registers.write_host_config(
            HostConfig::ZERO
                .with_wide_internal_bus(true)
                .with_slow_card(true),
        );
    }

    /// Wait for the data state machine to settle after a block, back in data
    /// or identification mode.
    fn wait_data_idle(&mut self) -> Result<(), BusError> {
        let settled = self.wait_until(BLOCK_MS, |host| {
            let state = host.registers.read_debug_mode().state().value();
            state == STATE_DATA || state == STATE_IDENTIFICATION
        });
        if settled {
            Ok(())
        } else {
            Err(BusError::Timeout)
        }
    }

    /// Move one block through the data register: `word` gets each word read
    /// when `reading`, and gives each word to write otherwise.
    fn move_block(
        &mut self,
        reading: bool,
        mut word: impl FnMut(Option<u32>) -> u32,
    ) -> Result<(), BusError> {
        for _index in 0..BLOCK_WORDS {
            let ready = self.wait_until(BLOCK_MS, |host| {
                let level = host.registers.read_debug_mode().fifo_level().value();
                if reading {
                    level > 0
                } else {
                    level < FIFO_WORDS
                }
            });
            let status = self.registers.read_host_status();
            if status.has_error() {
                self.registers.write_host_status(HostStatus::ERRORS);
                return Err(if status.crc16_error() {
                    BusError::Crc
                } else {
                    BusError::Failed
                });
            }
            if !ready {
                return Err(BusError::Timeout);
            }
            if reading {
                word(Some(self.registers.read_data()));
            } else {
                self.registers.write_data(word(None));
            }
        }
        self.wait_data_idle()
    }

    /// Read the block a [`card::READ_SINGLE_BLOCK`] command started, giving
    /// each word to `store` in order.
    ///
    /// # Errors
    ///
    /// A data error, or the block not arriving within [`BLOCK_MS`].
    pub fn read_block(&mut self, mut store: impl FnMut(u32)) -> Result<(), BusError> {
        self.move_block(true, |word| {
            if let Some(word) = word {
                store(word);
            }
            0
        })
    }

    /// Write the block a [`card::WRITE_BLOCK`] command started, taking each
    /// word from `load` in order.
    ///
    /// # Errors
    ///
    /// A data error, or the FIFO not draining within [`BLOCK_MS`].
    pub fn write_block(&mut self, mut load: impl FnMut() -> u32) -> Result<(), BusError> {
        self.move_block(false, |_| load())
    }
}

impl Bus for Sdhost<'_> {
    fn command(&mut self, command: Command, argument: u32) -> Result<[u32; 4], BusError> {
        if !self.wait_until(COMMAND_MS, |host| !host.registers.read_command().pending()) {
            return Err(BusError::Failed);
        }
        let status = self.registers.read_host_status();
        if status.has_error() {
            self.registers.write_host_status(status);
        }
        if command.data != Data::None {
            // The controller counts the data stage from these: bytes first.
            self.registers.write_byte_count(BLOCK_BYTES);
            self.registers.write_block_count(1);
        }
        self.registers.write_argument(argument);
        self.registers.write_command(command_register(command));
        if !self.wait_until(COMMAND_MS, |host| !host.registers.read_command().pending()) {
            return Err(BusError::Timeout);
        }
        if self.registers.read_command().fail() {
            let status = self.registers.read_host_status();
            self.registers.write_host_status(HostStatus::ERRORS);
            // An OCR carries no CRC, which the controller may flag.
            let unchecked = command.response == Response::ShortNoCrc
                && status.crc7_error()
                && !status.command_timeout();
            if !unchecked {
                // Kick a data state machine left waiting for the failed command.
                let mode = self.registers.read_debug_mode();
                let state = mode.state().value();
                if state == STATE_READ_WAIT || state == STATE_WRITE_START {
                    self.registers
                        .write_debug_mode(mode.with_force_data_mode(true));
                }
                return Err(if status.command_timeout() {
                    BusError::Timeout
                } else if status.crc7_error() {
                    BusError::Crc
                } else {
                    BusError::Failed
                });
            }
        }
        Ok(match command.response {
            Response::None => [0; 4],
            Response::Long => {
                [0, 1, 2, 3].map(|index| self.registers.read_response(index).unwrap_or(0))
            }
            Response::Short | Response::ShortNoCrc => {
                [self.registers.read_response(0).unwrap_or(0), 0, 0, 0]
            }
        })
    }

    fn set_clock(&mut self, hz: u32) {
        let divider = clock_divider(self.core_hz, hz);
        self.registers
            .write_clock_divider(ClockDivider::ZERO.with_divider(u11::new(divider)));
        // Let a data stage wait half a second of card clocks, as Linux does.
        self.registers
            .write_timeout(card_clock(self.core_hz, divider) / 2);
    }

    fn set_wide_bus(&mut self) {
        self.registers
            .modify_host_config(|config| config.with_wide_external_bus(true));
    }

    fn delay_ms(&mut self, milliseconds: u32) {
        let _never_done: bool = self.wait_until(u64::from(milliseconds), |_| false);
    }
}
