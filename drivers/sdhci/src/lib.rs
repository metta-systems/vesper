#![no_std]

//! Register logic of a standard SD host controller (SDHCI), as a [`Bus`] for
//! the SD card protocol of `libsd-card`: the Raspberry Pi 4's EMMC2
//! (`brcm,bcm2711-emmc2`), the Raspberry Pi 5's SD card controller
//! (`brcm,bcm2712-sdhci`) and, under QEMU, the Arasan controller of the
//! Raspberry Pi 3 and 4 (`brcm,bcm2835-sdhci`), which holds QEMU's card.
//!
//! Every register is read and written as a whole 32-bit word: the Arasan
//! controller loses narrower writes (Linux `sdhci-iproc.c` shadows them), and
//! the transfer mode and the command share the word whose write sends the
//! command. Commands and data stages end in interrupts: the driver waits for
//! the controller's line ([`Sdhci::new`]'s `wait`), at most [`EVENT_MS`] per
//! event, and reads the interrupt status. Blocks move by ADMA2 through descriptor tables the driver builds
//! ([`adma2_descriptor`]) where the controller has it, and through the buffer
//! data port otherwise.
//!
//! Sections are those of the SD Host Controller Simplified Specification,
//! version 4.20. The BCM2712's configuration registers ([`Bcm2712Config`])
//! are not documented; they follow Linux `drivers/mmc/host/sdhci-brcmstb.c`.

use {
    arbitrary_int::{u2, u3, u4, u6, u12},
    bitbybit::{bitenum, bitfield},
    derive_mmio::Mmio,
    libmmio::assert_register_offsets,
    libsd_card::{BLOCK_BYTES, Bus, BusError, Command, Data, Response},
};

/// `Block Size` and `Block Count` (§2.2.2, §2.2.3).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct BlockSizeCount {
    /// Bytes of a block
    #[bits(0..=11, rw)]
    size: u12,
    /// The SDMA buffer boundary
    #[bits(12..=14, rw)]
    sdma_boundary: u3,
    /// Blocks of the transfer
    #[bits(16..=31, rw)]
    count: u16,
}

/// The response a command expects (§2.2.6, `Response Type Select`).
#[bitenum(u2, exhaustive = true)]
#[derive(Debug, PartialEq, Eq)]
pub enum ResponseType {
    None = 0b00,
    /// 136 bits
    Long = 0b01,
    /// 48 bits
    Short = 0b10,
    /// 48 bits, then the card busy on DAT0
    ShortBusy = 0b11,
}

/// `Transfer Mode` and `Command` (§2.2.5, §2.2.6): writing the word sends the
/// command.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct TransferCommand {
    #[bit(0, rw)]
    dma: bool,
    #[bit(1, rw)]
    block_count_enable: bool,
    /// 1: Auto CMD12 after the last block
    #[bits(2..=3, rw)]
    auto_command: u2,
    /// The card sends the data
    #[bit(4, rw)]
    read: bool,
    #[bit(5, rw)]
    multiple_blocks: bool,
    #[bits(16..=17, rw)]
    response_type: ResponseType,
    #[bit(19, rw)]
    crc_check: bool,
    #[bit(20, rw)]
    index_check: bool,
    #[bit(21, rw)]
    data_present: bool,
    #[bits(24..=29, rw)]
    index: u6,
}

/// `Auto Command Enable`: Auto CMD12.
const AUTO_CMD12: u2 = u2::new(0b01);

/// `Present State` (§2.2.9).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct PresentState {
    #[bit(0, r)]
    command_inhibit: bool,
    #[bit(1, r)]
    data_inhibit: bool,
    #[bit(16, r)]
    card_inserted: bool,
}

/// `Host Control 1`, `Power Control`, `Block Gap Control` and `Wakeup Control`
/// (§2.2.10 to §2.2.13).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct HostControl {
    #[bit(1, rw)]
    four_bit_bus: bool,
    /// 0: SDMA, 2: 32-bit ADMA2
    #[bits(3..=4, rw)]
    dma_select: u2,
    /// What the card detect test reports: true for a card
    #[bit(6, rw)]
    card_detect_test_level: bool,
    /// Take card detect from the test level instead of the card detect pin
    #[bit(7, rw)]
    card_detect_test: bool,
    #[bit(8, rw)]
    bus_power: bool,
    /// 0b111: 3.3 V
    #[bits(9..=11, rw)]
    bus_voltage: u3,
}

/// `DMA Select`: 32-bit ADMA2.
const ADMA2_32: u2 = u2::new(0b10);
/// `SD Bus Voltage Select`: 3.3 V.
const VOLTAGE_3V3: u3 = u3::new(0b111);

/// `Clock Control`, `Timeout Control` and `Software Reset` (§2.2.14 to
/// §2.2.16).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct ClockControl {
    #[bit(0, rw)]
    internal_clock: bool,
    #[bit(1, r)]
    internal_clock_stable: bool,
    #[bit(2, rw)]
    card_clock: bool,
    /// Bits 9-8 of the 10-bit divider (version 3.00 on)
    #[bits(6..=7, rw)]
    divider_high: u2,
    /// Bits 7-0 of the divider
    #[bits(8..=15, rw)]
    divider_low: u8,
    /// The data timeout: TMCLK * 2^(13 + value)
    #[bits(16..=19, rw)]
    data_timeout: u4,
    #[bit(24, rw)]
    reset_all: bool,
    #[bit(25, rw)]
    reset_command: bool,
    #[bit(26, rw)]
    reset_data: bool,
}

/// `Data Timeout Counter Value`: the longest, TMCLK * 2^27.
const LONGEST_TIMEOUT: u4 = u4::new(0xe);

/// `Normal Interrupt Status` and `Error Interrupt Status`, and their enable
/// and signal enable registers (§2.2.17 to §2.2.22). A status bit is cleared
/// by writing it as one.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Interrupts {
    #[bit(0, rw)]
    command_complete: bool,
    #[bit(1, rw)]
    transfer_complete: bool,
    #[bit(4, rw)]
    buffer_write_ready: bool,
    #[bit(5, rw)]
    buffer_read_ready: bool,
    /// One of the error bits is set (status only)
    #[bit(15, r)]
    error: bool,
    #[bit(16, rw)]
    command_timeout: bool,
    #[bit(17, rw)]
    command_crc: bool,
    #[bit(18, rw)]
    command_end_bit: bool,
    #[bit(19, rw)]
    command_index: bool,
    #[bit(20, rw)]
    data_timeout: bool,
    #[bit(21, rw)]
    data_crc: bool,
    #[bit(22, rw)]
    data_end_bit: bool,
    #[bit(23, rw)]
    current_limit: bool,
    #[bit(24, rw)]
    auto_command: bool,
    #[bit(25, rw)]
    adma: bool,
}

impl Interrupts {
    /// Every error bit.
    pub const ERRORS: Self = Self::new_with_raw_value(0x03ff_0000);
    /// What the driver waits for: the ends of commands and data stages, the
    /// buffer ready events of programmed IO, and every error. The card
    /// insertion and removal events stay off: card detect is forced.
    pub const WAITED: Self = Self::new_with_raw_value(
        Self::ERRORS.raw_value()
            | Self::ZERO
                .with_command_complete(true)
                .with_transfer_complete(true)
                .with_buffer_write_ready(true)
                .with_buffer_read_ready(true)
                .raw_value(),
    );
    /// Every status bit, as written to clear them.
    pub const ALL: Self = Self::new_with_raw_value(u32::MAX);

    /// Whether `self` has any of `other`'s bits.
    #[must_use]
    pub const fn any(self, other: Self) -> bool {
        self.raw_value() & other.raw_value() != 0
    }

    /// The bits of `self` that `other` has too.
    #[must_use]
    pub const fn and(self, other: Self) -> Self {
        Self::new_with_raw_value(self.raw_value() & other.raw_value())
    }

    /// What a status with error bits makes the transfer fail with.
    #[must_use]
    pub const fn bus_error(self) -> BusError {
        if self.command_timeout() || self.data_timeout() {
            BusError::Timeout
        } else if self.command_crc() || self.data_crc() {
            BusError::Crc
        } else {
            BusError::Failed
        }
    }
}

/// `Capabilities`, bits 31-0 (§2.2.26).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Capabilities {
    /// The base clock in MHz; zero when another source tells it. Version 2.00
    /// has bits 13-8 only.
    #[bits(8..=15, r)]
    base_clock_mhz: u8,
    #[bit(19, r)]
    adma2: bool,
    #[bit(22, r)]
    sdma: bool,
    #[bit(24, r)]
    voltage_3v3: bool,
}

/// `Slot Interrupt Status` and `Host Controller Version` (§2.2.33, §2.2.34).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct SlotVersion {
    /// 0: 1.00, 1: 2.00, 2: 3.00, 3: 4.00, 4: 4.10, 5: 4.20
    #[bits(16..=23, r)]
    specification: u8,
}

/// `Specification Version Number` of version 3.00, which has the 10-bit
/// divided clock mode.
pub const VERSION_3_00: u8 = 2;

/// The SDHCI register block (§2.1.1, figure 2-2), as 32-bit words.
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct RegisterBlock {
    sdma_address: u32,
    block_size_count: BlockSizeCount,
    argument: u32,
    transfer_command: TransferCommand,
    #[mmio(PureRead)]
    response: [u32; 4],
    #[mmio(Read, Write)]
    buffer_data: u32,
    #[mmio(PureRead)]
    present_state: PresentState,
    host_control: HostControl,
    clock_control: ClockControl,
    interrupt_status: Interrupts,
    interrupt_status_enable: Interrupts,
    interrupt_signal_enable: Interrupts,
    /// `Auto CMD Error Status` and `Host Control 2`
    auto_command_error_host_control_2: u32,
    #[mmio(PureRead)]
    capabilities: Capabilities,
    #[mmio(PureRead)]
    capabilities_high: u32,
    #[mmio(PureRead)]
    maximum_current: u32,
    _reserved0: u32,
    force_event: u32,
    #[mmio(PureRead)]
    adma_error: u32,
    /// `ADMA System Address`, bits 31-0
    adma_address: u32,
    /// `ADMA System Address`, bits 63-32
    adma_address_high: u32,
    _reserved1: [u32; 39],
    #[mmio(PureRead)]
    slot_version: SlotVersion,
}

assert_register_offsets!(RegisterBlock, 0x100, {
    sdma_address: 0x00,
    block_size_count: 0x04,
    argument: 0x08,
    transfer_command: 0x0c,
    response: 0x10,
    buffer_data: 0x20,
    present_state: 0x24,
    host_control: 0x28,
    clock_control: 0x2c,
    interrupt_status: 0x30,
    interrupt_status_enable: 0x34,
    interrupt_signal_enable: 0x38,
    auto_command_error_host_control_2: 0x3c,
    capabilities: 0x40,
    capabilities_high: 0x44,
    maximum_current: 0x48,
    force_event: 0x50,
    adma_error: 0x54,
    adma_address: 0x58,
    adma_address_high: 0x5c,
    slot_version: 0xfc,
});

/// `SDIO_CFG_CTRL` of the BCM2712's configuration block.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Bcm2712Control {
    /// What the card detect test reports: false for a card (`SDCD_N`)
    #[bit(30, rw)]
    card_detect_test_level: bool,
    /// Take card detect from the test level
    #[bit(31, rw)]
    card_detect_test: bool,
}

/// `SDIO_CFG_SD_PIN_SEL`: which pins and timings the controller drives.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Bcm2712PinSelect {
    /// 0b10: an SD card, 0b01: eMMC
    #[bits(0..=1, rw)]
    mode: u2,
}

/// `SDIO_CFG_SD_PIN_SEL` mode: an SD card.
const PIN_SELECT_SD: u2 = u2::new(0b10);

/// The BCM2712's configuration block, the node's second register range
/// (`reg-names = "host", "cfg"`). Linux `sdhci-brcmstb.c` is the only source.
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct Bcm2712Config {
    control: Bcm2712Control,
    _reserved: [u32; 16],
    pin_select: Bcm2712PinSelect,
}

assert_register_offsets!(Bcm2712Config, 0x48, {
    control: 0x00,
    pin_select: 0x44,
});

/// Set the BCM2712's configuration block up for an SD card: report a card
/// present, as Linux `sdhci_brcmstb_cfginit_2712` does for a slot without a
/// usable card detect (the board's is a GPIO of a block Vesper does not
/// drive), and select the SD card pins, as the Raspberry Pi kernel's
/// `sdhci_bcm2712_set_clock` does. The card clock must be stopped, as it is
/// after [`Sdhci::reset`].
pub fn configure_bcm2712(config: &mut MmioBcm2712Config<'_>) {
    config.modify_control(|control| {
        control
            .with_card_detect_test(true)
            .with_card_detect_test_level(false)
    });
    config.modify_pin_select(|select| select.with_mode(PIN_SELECT_SD));
}

/// The register value that sends `command`, for a data stage of `blocks`
/// blocks (ignored without one) moved by DMA when `dma`.
#[must_use]
pub fn transfer_command(command: Command, blocks: u16, dma: bool) -> TransferCommand {
    let (response_type, checked) = match command.response {
        Response::None => (ResponseType::None, false),
        Response::Long => (ResponseType::Long, false),
        Response::Short => (ResponseType::Short, true),
        Response::ShortNoCrc => (ResponseType::Short, false),
    };
    let base = TransferCommand::ZERO
        .with_index(u6::new(command.index & 0x3f))
        .with_response_type(response_type)
        // A long response's CRC is checked by the controller, its index is
        // not there to check (§2.2.6, table 2-11).
        .with_crc_check(checked || command.response == Response::Long)
        .with_index_check(checked);
    if command.data == Data::None {
        return base;
    }
    let multiple = blocks > 1;
    base.with_data_present(true)
        .with_read(command.data == Data::Read)
        .with_dma(dma)
        .with_block_count_enable(true)
        .with_multiple_blocks(multiple)
        .with_auto_command(if multiple { AUTO_CMD12 } else { u2::new(0) })
}

/// The divider value that runs the card clock from `base_hz` at `hz` or
/// below: in the 10-bit divided clock mode of version 3.00 on, the clock is
/// `base_hz / (2 * divider)` (or `base_hz` for zero); before it, `divider`
/// is a power of two up to 128.
#[must_use]
pub fn clock_divider(base_hz: u32, hz: u32, ten_bit: bool) -> u16 {
    let hz = hz.max(1);
    if base_hz <= hz {
        return 0;
    }
    let divider = base_hz.div_ceil(2 * hz);
    if ten_bit {
        u16::try_from(divider.min(0x3ff)).unwrap_or(0x3ff)
    } else {
        u16::try_from(divider.next_power_of_two().min(0x80)).unwrap_or(0x80)
    }
}

/// The card clock `divider` gives from `base_hz`.
#[must_use]
pub const fn card_clock(base_hz: u32, divider: u16) -> u32 {
    if divider == 0 {
        base_hz
    } else {
        base_hz / (2 * divider as u32)
    }
}

/// A 136-bit response with its bits 31-0 in word 0, from the response
/// registers, which hold bits 127-8 shifted down by eight: the controller
/// drops the CRC (§2.2.7, table 2-12).
#[must_use]
pub const fn long_response(registers: [u32; 4]) -> [u32; 4] {
    [
        registers[0] << 8,
        (registers[1] << 8) | (registers[0] >> 24),
        (registers[2] << 8) | (registers[1] >> 24),
        (registers[3] << 8) | (registers[2] >> 24),
    ]
}

/// Bytes one ADMA2 descriptor moves at most.
pub const ADMA2_MAX_BYTES: u32 = 65536;

/// A 32-bit ADMA2 descriptor (§1.13.4, figure 1-11): move `bytes` (1 to
/// [`ADMA2_MAX_BYTES`]) at bus address `address`, and stop after it when
/// `last`. The low word, attributes and length, comes first in memory.
#[must_use]
pub const fn adma2_descriptor(address: u32, bytes: u32, last: bool) -> u64 {
    const VALID: u64 = 1 << 0;
    const END: u64 = 1 << 1;
    const TRANSFER: u64 = 0b10 << 4;
    // A length of zero stands for 65536 bytes.
    let length = (bytes % ADMA2_MAX_BYTES) as u64;
    ((address as u64) << 32) | (length << 16) | TRANSFER | VALID | if last { END } else { 0 }
}

/// How long the internal clock and a reset may take to settle (Linux waits
/// this long for the clock).
pub const SETTLE_MS: u64 = 150;
/// How long the card power takes to settle.
pub const POWER_MS: u32 = 10;

/// Words of a block.
const BLOCK_WORDS: u32 = BLOCK_BYTES / 4;

/// How the blocks of a data command move.
pub enum Mover<'a> {
    /// By ADMA2, through the descriptor table at this bus address, which the
    /// caller wrote and made visible to the controller
    Dma { descriptors: u32 },
    /// Through the buffer data port: the closure gets each word read, or gives
    /// each word to write when called with `None`
    Port(&'a mut dyn FnMut(Option<u32>) -> u32),
}

/// How long one command or data stage may take to end before the driver gives
/// up on its interrupt: past the longest write busy the specification allows
/// (500 ms for an SDXC card, SD Physical Layer §4.6.2.2).
pub const EVENT_MS: u64 = 1000;

/// The controller, sending commands and moving blocks; `wait(milliseconds)`
/// returns once the controller's interrupt line was raised, or after about
/// `milliseconds` without it.
pub struct Sdhci<'registers, Wait: FnMut(u64)> {
    registers: MmioRegisterBlock<'registers>,
    base_hz: u32,
    ten_bit_divider: bool,
    wait: Wait,
    /// Milliseconds of a monotonic clock
    now_ms: fn() -> u64,
}

impl<'registers, Wait: FnMut(u64)> Sdhci<'registers, Wait> {
    /// The controller at `registers`. Its base clock is the capabilities'
    /// when they give one, `fallback_hz` otherwise; `None` when neither does.
    pub fn new(
        registers: MmioRegisterBlock<'registers>,
        fallback_hz: impl FnOnce() -> Option<u32>,
        wait: Wait,
        now_ms: fn() -> u64,
    ) -> Option<Self> {
        let version = registers.read_slot_version().specification();
        let ten_bit_divider = version >= VERSION_3_00;
        let mut mhz = registers.read_capabilities().base_clock_mhz();
        if !ten_bit_divider {
            mhz &= 0x3f;
        }
        let base_hz = if mhz == 0 {
            fallback_hz()?
        } else {
            u32::from(mhz) * 1_000_000
        };
        Some(Self {
            registers,
            base_hz,
            ten_bit_divider,
            wait,
            now_ms,
        })
    }

    /// The base clock the card clock divides, in Hz.
    pub const fn base_hz(&self) -> u32 {
        self.base_hz
    }

    /// The specification version number (see [`SlotVersion`]).
    pub fn version(&self) -> u8 {
        self.registers.read_slot_version().specification()
    }

    /// Whether the controller moves blocks by ADMA2.
    pub fn has_adma2(&self) -> bool {
        self.registers.read_capabilities().adma2()
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

    /// Reset the controller (§3.2.1 and §3.3): every register to its default,
    /// the card clock stopped. Then the longest data timeout, card detect
    /// forced to a card (the boards' card detect pins are not the
    /// controller's to read), the bus powered at 3.3 V (§3.3), and the
    /// interrupts the driver waits for enabled and signalled.
    ///
    /// # Errors
    ///
    /// [`BusError::Failed`] when the reset does not finish.
    pub fn reset(&mut self) -> Result<(), BusError> {
        self.registers
            .write_clock_control(ClockControl::ZERO.with_reset_all(true));
        if !self.wait_until(SETTLE_MS, |host| {
            !host.registers.read_clock_control().reset_all()
        }) {
            return Err(BusError::Failed);
        }
        self.registers
            .modify_clock_control(|control| control.with_data_timeout(LONGEST_TIMEOUT));
        self.registers.write_host_control(
            HostControl::ZERO
                .with_card_detect_test(true)
                .with_card_detect_test_level(true)
                .with_bus_voltage(VOLTAGE_3V3),
        );
        self.registers
            .modify_host_control(|control| control.with_bus_power(true));
        self.delay_ms(POWER_MS);
        self.registers.write_interrupt_status(Interrupts::ALL);
        self.registers
            .write_interrupt_status_enable(Interrupts::WAITED);
        self.registers
            .write_interrupt_signal_enable(Interrupts::WAITED);
        Ok(())
    }

    /// Reset the command and data lines after an error (§3.10).
    fn reset_lines(&mut self) {
        self.registers
            .modify_clock_control(|control| control.with_reset_command(true).with_reset_data(true));
        let _settled = self.wait_until(SETTLE_MS, |host| {
            let control = host.registers.read_clock_control();
            !control.reset_command() && !control.reset_data()
        });
    }

    /// Wait up to [`EVENT_MS`] for one of `wanted`'s events, and clear it. An
    /// error clears every status bit, resets the lines and fails; so does the
    /// time running out (`BusError::Timeout`).
    fn wait_for(&mut self, wanted: Interrupts) -> Result<Interrupts, BusError> {
        let end = (self.now_ms)() + EVENT_MS;
        loop {
            let status = self.registers.read_interrupt_status();
            if status.any(Interrupts::ERRORS) {
                self.registers.write_interrupt_status(Interrupts::ALL);
                self.reset_lines();
                return Err(status.bus_error());
            }
            let seen = status.and(wanted);
            if seen.raw_value() != 0 {
                self.registers.write_interrupt_status(seen);
                return Ok(seen);
            }
            let now = (self.now_ms)();
            if now >= end {
                self.registers.write_interrupt_status(Interrupts::ALL);
                self.reset_lines();
                return Err(BusError::Timeout);
            }
            (self.wait)(end - now);
        }
    }

    /// Send `command` with `argument` once the lines are free, and wait for
    /// its end.
    fn send(
        &mut self,
        command: Command,
        argument: u32,
        value: TransferCommand,
    ) -> Result<(), BusError> {
        let data = command.data != Data::None;
        if !self.wait_until(SETTLE_MS, |host| {
            let state = host.registers.read_present_state();
            !state.command_inhibit() && !(data && state.data_inhibit())
        }) {
            return Err(BusError::Failed);
        }
        self.registers.write_interrupt_status(Interrupts::ALL);
        self.registers.write_argument(argument);
        self.registers.write_transfer_command(value);
        self.wait_for(Interrupts::ZERO.with_command_complete(true))
            .map(|_complete| ())
    }

    /// Send the data command `command` with `argument` and move its `blocks`
    /// blocks with `mover`. A multiple-block command ends with Auto CMD12.
    ///
    /// # Errors
    ///
    /// What made the command or the data stage fail; [`BusError::Failed`]
    /// for a command without a data stage or no blocks.
    pub fn transfer(
        &mut self,
        command: Command,
        argument: u32,
        blocks: u16,
        mover: Mover<'_>,
    ) -> Result<(), BusError> {
        if command.data == Data::None || blocks == 0 {
            return Err(BusError::Failed);
        }
        #[expect(clippy::cast_possible_truncation, reason = "512 fits 12 bits")]
        self.registers.write_block_size_count(
            BlockSizeCount::ZERO
                .with_size(u12::new(BLOCK_BYTES as u16))
                .with_count(blocks),
        );
        let dma = matches!(mover, Mover::Dma { .. });
        if let Mover::Dma { descriptors } = mover {
            self.registers.write_adma_address(descriptors);
            self.registers.write_adma_address_high(0);
            self.registers
                .modify_host_control(|control| control.with_dma_select(ADMA2_32));
        }
        self.send(command, argument, transfer_command(command, blocks, dma))?;
        if let Mover::Port(word) = mover {
            let ready = if command.data == Data::Read {
                Interrupts::ZERO.with_buffer_read_ready(true)
            } else {
                Interrupts::ZERO.with_buffer_write_ready(true)
            };
            for _block in 0..blocks {
                self.wait_for(ready)?;
                for _index in 0..BLOCK_WORDS {
                    if command.data == Data::Read {
                        word(Some(self.registers.read_buffer_data()));
                    } else {
                        self.registers.write_buffer_data(word(None));
                    }
                }
            }
        }
        self.wait_for(Interrupts::ZERO.with_transfer_complete(true))
            .map(|_complete| ())
    }
}

impl<Wait: FnMut(u64)> Bus for Sdhci<'_, Wait> {
    /// Send a command without a data stage; data commands go through
    /// [`Sdhci::transfer`].
    fn command(&mut self, command: Command, argument: u32) -> Result<[u32; 4], BusError> {
        if command.data != Data::None {
            return Err(BusError::Failed);
        }
        self.send(command, argument, transfer_command(command, 0, false))?;
        Ok(match command.response {
            Response::None => [0; 4],
            Response::Long => long_response(
                [0, 1, 2, 3].map(|index| self.registers.read_response(index).unwrap_or(0)),
            ),
            Response::Short | Response::ShortNoCrc => {
                [self.registers.read_response(0).unwrap_or(0), 0, 0, 0]
            }
        })
    }

    /// Stop the card clock, set the divider, wait for the internal clock to
    /// settle and start the card clock again (§3.2.3).
    fn set_clock(&mut self, hz: u32) {
        let divider = clock_divider(self.base_hz, hz, self.ten_bit_divider);
        self.registers
            .modify_clock_control(|control| control.with_card_clock(false));
        self.registers.modify_clock_control(|control| {
            control
                .with_divider_low((divider & 0xff) as u8)
                .with_divider_high(u2::new(((divider >> 8) & 0b11) as u8))
                .with_internal_clock(true)
        });
        let _stable = self.wait_until(SETTLE_MS, |host| {
            host.registers.read_clock_control().internal_clock_stable()
        });
        self.registers
            .modify_clock_control(|control| control.with_card_clock(true));
    }

    fn set_wide_bus(&mut self) {
        self.registers
            .modify_host_control(|control| control.with_four_bit_bus(true));
    }

    fn delay_ms(&mut self, milliseconds: u32) {
        let _never_done: bool = self.wait_until(u64::from(milliseconds), |_| false);
    }
}
