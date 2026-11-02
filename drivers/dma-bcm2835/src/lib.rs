#![no_std]

//! The DMA controller of the BCM2835 (`brcm,bcm2835-dma`): its registers, the
//! control blocks its channels run, the channels the ARM may use, and the
//! bookkeeping of a chain's progress.
//!
//! Resources: BCM2835 ARM Peripherals, chapter 4: §4.2 (channels 0 to 14 at
//! `0x100` steps in one page, with the global `INT_STATUS` at `0xfe0` and
//! `ENABLE` at `0xff0`), §4.2.1.1 (control blocks: eight words, 32-byte
//! aligned, loaded from `CONBLK_AD`, followed through `NEXTCONBK`, a zero
//! there ending the chain), §4.2.1.2 (the `CS`, `TI`, `TXFR_LEN` and `DEBUG`
//! fields), §4.2.1.3 (peripheral DREQ signals: writes to a peripheral are
//! paced by its DREQ; 5 is PWM), §4.5 (DMA Lite channels: 16-bit transfer
//! length, no 2D mode; the `DEBUG` register's `LITE` bit tells them apart).
//! Addresses in control blocks are bus addresses (§1.2.3, §1.2.4): RAM through
//! the `0xC000_0000` alias, peripherals at `0x7Ennnnnn`.
//!
//! Where control blocks live is the component's business: it builds them in
//! a page of its own memory, eight per channel, and cleans them to the point
//! of coherency before a channel loads them (`doc/agentic/audio-design.md`,
//! The DMA controller).

use {
    arbitrary_int::{u3, u4, u5, u9},
    bitbybit::bitfield,
    derive_mmio::Mmio,
    libmmio::assert_register_offsets,
};

/// Channels in the controller's page (§4.2): 0 to 14. Channel 15 sits in
/// another page and is never granted to the ARM.
pub const CHANNELS: usize = 15;

/// Bytes between two channels' register blocks (§4.2).
pub const CHANNEL_STRIDE: usize = 0x100;

/// Bytes of a control block, and the alignment it needs (§4.2.1.1).
pub const CONTROL_BLOCK_BYTES: usize = 32;

/// Most control blocks of one chain: the component keeps eight per channel.
pub const MAX_CHAIN_BLOCKS: usize = 8;

/// Bytes of control blocks the component keeps per channel.
pub const CHAIN_BYTES: usize = MAX_CHAIN_BLOCKS * CONTROL_BLOCK_BYTES;

const _: () = assert!(CHANNELS * CHAIN_BYTES <= 4096, "every chain fits one page");

/// Longest block of a full channel: `TXFR_LEN` is 30 bits outside 2D mode
/// (§4.2.1.2), cut to whole words.
pub const MAX_FULL_LENGTH: u32 = (1 << 30) - 4;

/// Longest block of a Lite channel: its `TXFR_LEN` is 16 bits (§4.5), cut to
/// whole words.
pub const MAX_LITE_LENGTH: u32 = 0xffff & !3;

/// DREQ 0: always asserted, a memory-to-memory transfer runs unpaced
/// (§4.2.1.3).
pub const DREQ_MEMORY: u32 = 0;

/// DREQ 5: the PWM block (§4.2.1.3; "PWM DMA is mapped to DMA channel 5",
/// §9.5).
pub const DREQ_PWM: u32 = 5;

/// The PWM FIFO's bus address: the block at `0x7E20_C000`, `FIF1` at `0x18`
/// (§9.6).
pub const PWM_FIFO_BUS: u32 = 0x7e20_c018;

/// The bus address a channel paced by `dreq` writes to: `None` for memory
/// (DREQ 0) and for the peripherals the component does not feed yet. The
/// client never names a peripheral's address itself.
#[must_use]
pub const fn peripheral_address(dreq: u32) -> Option<u32> {
    match dreq {
        DREQ_PWM => Some(PWM_FIFO_BUS),
        _ => None,
    }
}

/// Whether a channel may be claimed for `dreq`: memory, or a peripheral with
/// an address in [`peripheral_address`].
#[must_use]
pub const fn is_known_request(dreq: u32) -> bool {
    dreq == DREQ_MEMORY || peripheral_address(dreq).is_some()
}

/// A channel's control and status (`CS`, §4.2.1.2). `END` and `INT` clear
/// when written with 1; `ABORT` and `RESET` act when written with 1 and read
/// as 0.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct ControlStatus {
    /// The channel runs; clearing it pauses the channel, which clears it by
    /// itself when it loads a zero `NEXTCONBK`
    #[bit(0, rw)]
    active: bool,
    /// A control block finished
    #[bit(1, rw)]
    end: bool,
    /// A control block with `INTEN` finished
    #[bit(2, rw)]
    interrupt: bool,
    /// The DREQ the channel is paced by is asserted
    #[bit(3, r)]
    data_request: bool,
    #[bit(4, r)]
    paused: bool,
    /// Paused because its DREQ is low
    #[bit(5, r)]
    paused_by_data_request: bool,
    /// Waiting for the last write of the control block to be answered
    #[bit(6, r)]
    waiting_for_writes: bool,
    /// An error flag of `DEBUG` is set
    #[bit(8, r)]
    error: bool,
    /// AXI priority of normal transfers
    #[bits(16..=19, rw)]
    priority: u4,
    /// AXI priority while the peripheral signals panic
    #[bits(20..=23, rw)]
    panic_priority: u4,
    /// Wait for every write to be answered before the next control block
    #[bit(28, rw)]
    wait_for_outstanding_writes: bool,
    /// Keep running while the debugger pauses the system
    #[bit(29, rw)]
    disable_debug_pause: bool,
    /// Drop the current control block and load the next
    #[bit(30, rw)]
    abort: bool,
    /// Reset the channel
    #[bit(31, rw)]
    reset: bool,
}

/// A control block's transfer information (`TI`, §4.2.1.2).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct TransferInformation {
    /// Raise the channel's interrupt when the block is done
    #[bit(0, rw)]
    interrupt_enable: bool,
    /// 2D mode (full channels only)
    #[bit(1, rw)]
    two_dimensional: bool,
    /// Wait for each write's answer before the next
    #[bit(3, rw)]
    wait_for_write_response: bool,
    #[bit(4, rw)]
    destination_increment: bool,
    /// 128-bit writes instead of 32-bit
    #[bit(5, rw)]
    destination_wide: bool,
    /// Pace writes by the DREQ of `peripheral`
    #[bit(6, rw)]
    destination_request: bool,
    #[bit(7, rw)]
    destination_ignore: bool,
    #[bit(8, rw)]
    source_increment: bool,
    /// 128-bit reads instead of 32-bit
    #[bit(9, rw)]
    source_wide: bool,
    /// Pace reads by the DREQ of `peripheral`
    #[bit(10, rw)]
    source_request: bool,
    #[bit(11, rw)]
    source_ignore: bool,
    #[bits(12..=15, rw)]
    burst_length: u4,
    /// The DREQ that paces the transfer (`PERMAP`, §4.2.1.3)
    #[bits(16..=20, rw)]
    peripheral: u5,
    /// Dummy cycles after each read or write
    #[bits(21..=25, rw)]
    waits: u5,
    #[bit(26, rw)]
    no_wide_bursts: bool,
}

/// A channel's debug register (`DEBUG`, §4.2.1.2). The three error flags
/// clear when written with 1.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct DebugStatus {
    /// A read came back without its last flag
    #[bit(0, rw)]
    read_last_not_set_error: bool,
    #[bit(1, rw)]
    fifo_error: bool,
    /// A read came back with an error
    #[bit(2, rw)]
    read_error: bool,
    #[bits(4..=7, r)]
    outstanding_writes: u4,
    #[bits(8..=15, r)]
    dma_id: u8,
    #[bits(16..=24, r)]
    state: u9,
    #[bits(25..=27, r)]
    version: u3,
    /// The channel is a DMA Lite engine (§4.5)
    #[bit(28, r)]
    lite: bool,
}

/// One channel's registers (§4.2.1.2), at `CHANNEL_STRIDE` × its number.
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct ChannelRegisters {
    control_status: ControlStatus,
    /// Bus address of the control block the channel is on (zero: none)
    control_block_address: u32,
    #[mmio(PureRead)]
    transfer_information: TransferInformation,
    #[mmio(PureRead)]
    source_address: u32,
    #[mmio(PureRead)]
    destination_address: u32,
    #[mmio(PureRead)]
    transfer_length: u32,
    #[mmio(PureRead)]
    stride: u32,
    #[mmio(PureRead)]
    next_control_block: u32,
    debug: DebugStatus,
}

assert_register_offsets!(ChannelRegisters, 0x24, {
    control_status: 0x00,
    control_block_address: 0x04,
    transfer_information: 0x08,
    source_address: 0x0c,
    destination_address: 0x10,
    transfer_length: 0x14,
    stride: 0x18,
    next_control_block: 0x1c,
    debug: 0x20,
});

/// The controller's page: the channels' blocks, then the global registers
/// (§4.2.1.2).
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct RegisterBlock {
    _channels: [u32; 0x3f8],
    /// Bit `n`: channel `n` has `CS.INT` set
    #[mmio(PureRead)]
    interrupt_status: u32,
    _reserved: [u32; 3],
    /// Bit `n`: channel `n` may run
    enable: u32,
}

assert_register_offsets!(RegisterBlock, 0xff4, {
    interrupt_status: 0xfe0,
    enable: 0xff0,
});

/// The `CS` value that starts a channel on the control block in
/// `CONBLK_AD`, waiting for its writes to be answered at the end of each
/// block. The priorities are Circle's (`lib/dmachannel.cpp`, secondary: the
/// datasheet recommends none): 1, and 15 while the peripheral signals panic,
/// so a PWM FIFO running low wins arbitration.
#[must_use]
pub fn starting() -> ControlStatus {
    ControlStatus::ZERO
        .with_active(true)
        .with_wait_for_outstanding_writes(true)
        .with_priority(u4::new(1))
        .with_panic_priority(u4::new(15))
}

/// The `CS` value that clears a serviced interrupt: `INT` and `END` written
/// with 1. A channel still running keeps `ACTIVE` and its priorities, which
/// the same write sets; a finished one stays idle.
#[must_use]
pub fn acknowledging(running: bool) -> ControlStatus {
    let base = if running {
        starting()
    } else {
        ControlStatus::ZERO
    };
    base.with_interrupt(true).with_end(true)
}

/// The `CS` value that pauses a channel (`ACTIVE` clear).
#[must_use]
pub const fn pausing() -> ControlStatus {
    ControlStatus::ZERO
}

/// The `CS` value that resets a channel.
#[must_use]
pub const fn resetting() -> ControlStatus {
    ControlStatus::ZERO.with_reset(true)
}

/// The `DEBUG` value that clears every error flag.
#[must_use]
pub const fn clear_debug_errors() -> DebugStatus {
    DebugStatus::ZERO
        .with_read_last_not_set_error(true)
        .with_fifo_error(true)
        .with_read_error(true)
}

/// The `DEBUG` value that clears the error flags set in `current`, and only
/// those: nothing to write when none is set. (QEMU's model stores a `DEBUG`
/// write as it is instead of clearing the bits written with 1, so writing
/// flags that are clear would set them there.)
#[must_use]
pub const fn clearing_debug_errors(current: DebugStatus) -> Option<DebugStatus> {
    let set = current.raw_value() & clear_debug_errors().raw_value();
    if set == 0 {
        None
    } else {
        Some(DebugStatus::new_with_raw_value(set))
    }
}

/// `dma_v1.errors` bits: the `DEBUG` error flags in bits 0 to 2, `CS.ERROR`
/// in bit 3.
#[must_use]
pub fn error_bits(control_status: ControlStatus, debug: DebugStatus) -> u32 {
    (debug.raw_value() & clear_debug_errors().raw_value())
        | (u32::from(control_status.error()) << 3)
}

/// A control block (§4.2.1.1): eight words, 32-byte aligned, in the order the
/// channel loads them. Addresses are bus addresses.
#[repr(C, align(32))]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ControlBlock {
    /// `TI` ([`TransferInformation`])
    pub transfer_information: u32,
    pub source: u32,
    pub destination: u32,
    /// Bytes of the block (`TXFR_LEN` outside 2D mode)
    pub length: u32,
    /// `STRIDE`: zero outside 2D mode
    pub stride: u32,
    /// Bus address of the next control block; zero ends the chain
    pub next: u32,
    /// Two reserved words, written as zero
    pub reserved: [u32; 2],
}

const _: () = assert!(size_of::<ControlBlock>() == CONTROL_BLOCK_BYTES);
const _: () = assert!(align_of::<ControlBlock>() == CONTROL_BLOCK_BYTES);

impl ControlBlock {
    /// The block's `TI`, typed.
    #[must_use]
    pub const fn information(&self) -> TransferInformation {
        TransferInformation::new_with_raw_value(self.transfer_information)
    }
}

/// `TI` of a block of a memory-to-memory copy: both addresses increment, no
/// pacing (DREQ 0), the interrupt when it is done, and each write answered
/// before the next block so the interrupt means the bytes are in memory.
#[must_use]
pub fn copy_information() -> TransferInformation {
    TransferInformation::ZERO
        .with_interrupt_enable(true)
        .with_wait_for_write_response(true)
        .with_destination_increment(true)
        .with_source_increment(true)
}

/// `TI` of a period of a ring feeding the peripheral of `dreq`: the source
/// increments, the destination is the peripheral's fixed FIFO address, writes
/// are paced by its DREQ, one write in flight at a time (`WAIT_RESP`, so a
/// FIFO of a few words is never overrun, §4.2.1.3) and no wide bursts, and
/// the interrupt when the period is done.
#[must_use]
pub fn ring_information(dreq: u5) -> TransferInformation {
    TransferInformation::ZERO
        .with_interrupt_enable(true)
        .with_wait_for_write_response(true)
        .with_destination_request(true)
        .with_source_increment(true)
        .with_peripheral(dreq)
        .with_no_wide_bursts(true)
}

/// Why a chain cannot be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainError {
    /// No blocks, or more than [`MAX_CHAIN_BLOCKS`]
    BlockCount,
    /// A block length of zero, not a multiple of four, or longer than the
    /// channel moves in one block
    Length,
    /// An address not word aligned, or a range that wraps the bus
    Address,
    /// A DREQ outside the five bits of `PERMAP`
    Request,
}

/// A chain of control blocks, as a channel's slots hold it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chain {
    blocks: [ControlBlock; MAX_CHAIN_BLOCKS],
    count: usize,
    cyclic: bool,
}

impl Chain {
    /// The blocks, in the order the channel runs them.
    #[must_use]
    pub fn blocks(&self) -> &[ControlBlock] {
        &self.blocks[..self.count]
    }

    /// Whether the last block leads back to the first.
    #[must_use]
    pub const fn is_cyclic(&self) -> bool {
        self.cyclic
    }

    /// Bytes the blocks take in the channel's slots.
    #[must_use]
    pub const fn bytes(&self) -> usize {
        self.count * CONTROL_BLOCK_BYTES
    }
}

/// The largest block a channel moves: Lite channels have a 16-bit length.
#[must_use]
pub const fn max_length(lite: bool) -> u32 {
    if lite {
        MAX_LITE_LENGTH
    } else {
        MAX_FULL_LENGTH
    }
}

/// Check `blocks` blocks of `block_bytes` at `addresses` (each the first
/// byte of a range of `blocks × block_bytes` bytes).
fn check_blocks(
    block_bytes: u32,
    blocks: u32,
    lite: bool,
    addresses: &[u32],
) -> Result<usize, ChainError> {
    let count = usize::try_from(blocks).map_err(|_overflow| ChainError::BlockCount)?;
    if count == 0 || count > MAX_CHAIN_BLOCKS {
        return Err(ChainError::BlockCount);
    }
    if block_bytes == 0 || !block_bytes.is_multiple_of(4) || block_bytes > max_length(lite) {
        return Err(ChainError::Length);
    }
    let total = u64::from(block_bytes) * u64::from(blocks);
    for &address in addresses {
        if !address.is_multiple_of(4) || u64::from(address) + total > 1 << 32 {
            return Err(ChainError::Address);
        }
    }
    Ok(count)
}

/// Link `blocks` in the slots starting at bus address `slots_bus`, the last
/// to the first when `cyclic`, else to zero.
fn link(mut chain: Chain, slots_bus: u32) -> Chain {
    let count = chain.count;
    for (index, block) in chain.blocks[..count].iter_mut().enumerate() {
        let next = index + 1;
        block.next = if next < count {
            slot_address(slots_bus, next)
        } else if chain.cyclic {
            slots_bus
        } else {
            0
        };
    }
    chain
}

/// The bus address of slot `index` of a channel whose slots start at
/// `slots_bus`.
#[must_use]
pub fn slot_address(slots_bus: u32, index: usize) -> u32 {
    let offset = u32::try_from(index * CONTROL_BLOCK_BYTES).expect("slots lie within one page");
    slots_bus + offset
}

/// A finite chain copying `blocks` blocks of `block_bytes` from `source` to
/// `destination` (bus addresses), each block with its interrupt, stored at
/// `slots_bus`.
///
/// # Errors
///
/// [`ChainError`] for a count, length or address the channel cannot run.
pub fn copy_chain(
    slots_bus: u32,
    source: u32,
    destination: u32,
    block_bytes: u32,
    blocks: u32,
    lite: bool,
) -> Result<Chain, ChainError> {
    let count = check_blocks(block_bytes, blocks, lite, &[source, destination])?;
    let mut chain = Chain {
        blocks: [ControlBlock::default(); MAX_CHAIN_BLOCKS],
        count,
        cyclic: false,
    };
    let mut offset = 0;
    for block in &mut chain.blocks[..count] {
        *block = ControlBlock {
            transfer_information: copy_information().raw_value(),
            source: source + offset,
            destination: destination + offset,
            length: block_bytes,
            ..ControlBlock::default()
        };
        offset += block_bytes;
    }
    Ok(link(chain, slots_bus))
}

/// A circular chain feeding the FIFO at bus address `fifo` from `periods`
/// periods of `period_bytes` at bus address `ring`, paced by `dreq`, each
/// period with its interrupt, stored at `slots_bus`.
///
/// # Errors
///
/// [`ChainError`] for a count, length, address or DREQ the channel cannot
/// run.
pub fn ring_chain(
    slots_bus: u32,
    ring: u32,
    period_bytes: u32,
    periods: u32,
    dreq: u32,
    fifo: u32,
    lite: bool,
) -> Result<Chain, ChainError> {
    let count = check_blocks(period_bytes, periods, lite, &[ring])?;
    let peripheral = u8::try_from(dreq)
        .ok()
        .and_then(|dreq| u5::try_new(dreq).ok())
        .ok_or(ChainError::Request)?;
    if !fifo.is_multiple_of(4) {
        return Err(ChainError::Address);
    }
    let mut chain = Chain {
        blocks: [ControlBlock::default(); MAX_CHAIN_BLOCKS],
        count,
        cyclic: true,
    };
    let mut offset = 0;
    for block in &mut chain.blocks[..count] {
        *block = ControlBlock {
            transfer_information: ring_information(peripheral).raw_value(),
            source: ring + offset,
            destination: fifo,
            length: period_bytes,
            ..ControlBlock::default()
        };
        offset += period_bytes;
    }
    Ok(link(chain, slots_bus))
}

/// Where a channel is in its chain, from its `CONBLK_AD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    /// On the block at this index
    On(usize),
    /// Past the last block: `CONBLK_AD` is zero
    Ended,
    /// `CONBLK_AD` points outside the chain
    Lost,
}

/// The position `control_block_address` names in a chain of `blocks` blocks
/// stored at `slots_bus`.
#[must_use]
pub fn position(slots_bus: u32, blocks: usize, control_block_address: u32) -> Position {
    if control_block_address == 0 {
        return Position::Ended;
    }
    let Some(offset) = control_block_address.checked_sub(slots_bus) else {
        return Position::Lost;
    };
    let Ok(offset) = usize::try_from(offset) else {
        return Position::Lost;
    };
    if !offset.is_multiple_of(CONTROL_BLOCK_BYTES) || offset / CONTROL_BLOCK_BYTES >= blocks {
        return Position::Lost;
    }
    Position::On(offset / CONTROL_BLOCK_BYTES)
}

/// Blocks finished since the channel was on block `previous` of a chain of
/// `blocks` blocks, now that it is at `now`. A ring wraps; an ended chain
/// finished every block from `previous` on. An interrupt serviced late
/// counts every block it slept through, as long as a ring did not go round
/// a whole time meanwhile.
#[must_use]
pub fn finished_blocks(previous: usize, now: Position, blocks: usize, cyclic: bool) -> usize {
    match now {
        Position::Ended => blocks.saturating_sub(previous),
        Position::On(index) if cyclic => (index + blocks - previous) % blocks,
        Position::On(index) => index.saturating_sub(previous),
        Position::Lost => 0,
    }
}

/// The channels of the controller the ARM may use and which are claimed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Channels {
    /// The device tree's `brcm,dma-channel-mask`, cut to the page's channels
    granted: u16,
    claimed: u16,
}

impl Channels {
    /// The channels of `mask`, a `brcm,dma-channel-mask` value; bits of
    /// channels outside the page are dropped.
    #[must_use]
    pub fn new(mask: u32) -> Self {
        Self {
            granted: u16::try_from(mask & ((1 << CHANNELS) - 1)).unwrap_or(0),
            claimed: 0,
        }
    }

    /// The granted channels, as a mask.
    #[must_use]
    pub const fn granted(&self) -> u16 {
        self.granted
    }

    /// Whether `channel` is granted and claimed.
    #[must_use]
    pub const fn is_claimed(&self, channel: usize) -> bool {
        channel < CHANNELS && self.claimed & (1 << channel) != 0
    }

    /// Claim the lowest granted, unclaimed channel for which `usable` holds.
    pub fn claim(&mut self, mut usable: impl FnMut(usize) -> bool) -> Option<usize> {
        let free = self.granted & !self.claimed;
        let channel =
            (0..CHANNELS).find(|&channel| free & (1 << channel) != 0 && usable(channel))?;
        self.claimed |= 1 << channel;
        Some(channel)
    }

    /// Give `channel` back; false if it was not claimed.
    pub fn release(&mut self, channel: usize) -> bool {
        if !self.is_claimed(channel) {
            return false;
        }
        self.claimed &= !(1 << channel);
        true
    }
}

/// The first channel whose interrupt line it shares with the channels after
/// it: channels 11 to 14 share one line (GPU interrupt 27 on the Raspberry
/// Pi's device trees).
pub const SHARED_LINE_FIRST_CHANNEL: usize = 11;

/// The position of `channel`'s interrupt among the DMA node's interrupts as
/// Kickstart hands them over (`DriverInit::interrupts`): the binding lists
/// `dma0` to `dma14` and then `dma-shared-all`, the line channels 11 to 14
/// share listed four times, and Kickstart keeps a repeated line only at its
/// first position. So channels 0 to 10 are at their own number and 11 to 14
/// at 11; `dma-shared-all` follows at 12.
#[must_use]
pub const fn interrupt_index(channel: usize) -> usize {
    if channel < SHARED_LINE_FIRST_CHANNEL {
        channel
    } else {
        SHARED_LINE_FIRST_CHANNEL
    }
}

/// The DMA node's interrupts once Kickstart issued each line once: channels 0
/// to 10, the line of 11 to 14, and `dma-shared-all`.
pub const NODE_INTERRUPTS: usize = SHARED_LINE_FIRST_CHANNEL + 2;

/// The channels whose interrupt is at `index` in the node's list, as a mask.
#[must_use]
pub const fn channels_of_interrupt(index: usize) -> u16 {
    if index < SHARED_LINE_FIRST_CHANNEL {
        1 << index
    } else if index == SHARED_LINE_FIRST_CHANNEL {
        ((1 << CHANNELS) - 1) & !((1 << SHARED_LINE_FIRST_CHANNEL) - 1)
    } else {
        0
    }
}
