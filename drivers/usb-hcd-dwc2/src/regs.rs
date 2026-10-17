//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! Register layout and bit fields of the `DesignWare` USB 2.0 OTG controller in
//! host mode (DWC2), as the Raspberry Pi 3's `brcm,bcm2708-usb` node has it.
//! The fields are `bitbybit` bitfields and the blocks are `derive_mmio`
//! register blocks, as in the other drivers. The pure helpers at the end
//! encode and decode them, so host tests check the logic without hardware.

use {
    arbitrary_int::prelude::*,
    bitbybit::{bitenum, bitfield},
    derive_mmio::Mmio,
    libmmio::assert_register_offsets,
    vesper_usb_protocol::{descriptor::TransferKind, enumeration::Speed},
};

/// AHB configuration.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct AhbConfig {
    /// Global interrupt enable. The driver polls, so it stays clear.
    #[bit(0, rw)]
    global_interrupt_mask: bool,
    /// Transfers move packet data by DMA, through `HCDMA`, rather than by
    /// programmed I/O through the FIFOs.
    #[bit(5, rw)]
    dma_enable: bool,
}

/// USB configuration: the mode forcing bits.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct UsbConfig {
    /// Forces host mode regardless of the ID pin.
    #[bit(29, rw)]
    force_host_mode: bool,
    /// Forces device mode regardless of the ID pin.
    #[bit(30, rw)]
    force_device_mode: bool,
}

/// Core reset control and FIFO flushes.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct ResetControl {
    /// Core soft reset. Clears itself when the reset completes.
    #[bit(0, rw)]
    core_soft_reset: bool,
    /// Flushes the receive FIFO.
    #[bit(4, rw)]
    rx_fifo_flush: bool,
    /// Flushes the transmit FIFO numbered by `tx_fifo_number`.
    #[bit(5, rw)]
    tx_fifo_flush: bool,
    /// Transmit FIFO to flush; 0x10 selects every transmit FIFO.
    #[bits(6..=10, rw)]
    tx_fifo_number: u5,
    /// The AHB master is idle, so the core may be reset.
    #[bit(31, r)]
    ahb_master_idle: bool,
}

/// Global interrupt status.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct InterruptStatus {
    /// Set in host mode, clear in device mode.
    #[bit(0, r)]
    host_mode: bool,
    /// The receive FIFO holds at least one status entry.
    #[bit(4, r)]
    rx_fifo_non_empty: bool,
    /// A root port event is pending.
    #[bit(24, r)]
    host_port: bool,
    /// A host channel event is pending.
    #[bit(25, r)]
    host_channel: bool,
}

/// Speed of the device on the root port.
#[bitenum(u2, exhaustive = true)]
#[derive(Debug, PartialEq, Eq)]
pub enum PortSpeed {
    High = 0,
    Full = 1,
    Low = 2,
    Reserved = 3,
}

/// Root port control and status.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct PortControl {
    /// A device is connected.
    #[bit(0, r)]
    connected: bool,
    /// A connection change happened. Write 1 to acknowledge.
    #[bit(1, rw)]
    connect_detected: bool,
    /// The port is enabled. Write 1 to disable.
    #[bit(2, rw)]
    enabled: bool,
    /// The enable state changed. Write 1 to acknowledge.
    #[bit(3, rw)]
    enable_changed: bool,
    /// An over-current change happened. Write 1 to acknowledge.
    #[bit(5, rw)]
    overcurrent_changed: bool,
    /// Holds the port in reset.
    #[bit(8, rw)]
    reset: bool,
    /// Supplies power to the port.
    #[bit(12, rw)]
    power: bool,
    /// Speed of the device attached to the port.
    #[bits(17..=18, r)]
    speed: PortSpeed,
}

impl PortControl {
    /// This value with every event bit cleared. Writing it back leaves the
    /// pending events alone, because a write of 0 acknowledges nothing.
    #[must_use]
    pub fn without_events(self) -> Self {
        self.with_connect_detected(false)
            .with_enabled(false)
            .with_enable_changed(false)
            .with_overcurrent_changed(false)
    }
}

/// Transfer type of an endpoint, as `HCCHAR` holds it.
#[bitenum(u2, exhaustive = true)]
#[derive(Debug, PartialEq, Eq)]
pub enum EndpointKind {
    Control = 0,
    Isochronous = 1,
    Bulk = 2,
    Interrupt = 3,
}

/// Host channel characteristics: the endpoint a channel talks to, and
/// whether it runs.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Characteristics {
    /// Largest packet the endpoint takes.
    #[bits(0..=10, rw)]
    max_packet: u11,
    /// Endpoint number.
    #[bits(11..=14, rw)]
    endpoint: u4,
    /// The endpoint sends data to the host.
    #[bit(15, rw)]
    input: bool,
    /// The device is low speed.
    #[bit(17, rw)]
    low_speed: bool,
    /// Transfer type.
    #[bits(18..=19, rw)]
    kind: EndpointKind,
    /// Transactions per microframe; one for non-periodic endpoints.
    #[bits(20..=21, rw)]
    transactions: u2,
    /// Address of the device.
    #[bits(22..=28, rw)]
    device_address: u7,
    /// Halts the channel together with `enable`.
    #[bit(30, rw)]
    disable: bool,
    /// Starts the channel.
    #[bit(31, rw)]
    enable: bool,
}

/// PID of a transfer's first packet.
#[bitenum(u2, exhaustive = true)]
#[derive(Debug, PartialEq, Eq)]
pub enum Pid {
    Data0 = 0,
    Data2 = 1,
    Data1 = 2,
    Setup = 3,
}

/// Host channel transfer size: bytes, packets and the first packet's PID.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct TransferSize {
    /// Bytes to transfer; after a transfer, the bytes left over.
    #[bits(0..=18, rw)]
    bytes: u19,
    /// Packets the transfer takes.
    #[bits(19..=28, rw)]
    packets: u10,
    /// PID of the first packet.
    #[bits(29..=30, rw)]
    pid: Pid,
}

/// Host channel interrupts. Each bit is cleared by writing 1.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct ChannelInterrupts {
    /// The transfer finished.
    #[bit(0, rw)]
    transfer_complete: bool,
    /// The channel halted.
    #[bit(1, rw)]
    halted: bool,
    /// A bus master error.
    #[bit(2, rw)]
    ahb_error: bool,
    /// The endpoint answered with a STALL.
    #[bit(3, rw)]
    stall: bool,
    /// The endpoint answered with a NAK.
    #[bit(4, rw)]
    nak: bool,
    /// The endpoint answered with an ACK.
    #[bit(5, rw)]
    ack: bool,
    /// A transaction error: CRC, timeout or a bad handshake.
    #[bit(7, rw)]
    transaction_error: bool,
    /// The device sent more data than the channel expected.
    #[bit(8, rw)]
    babble: bool,
    /// The data toggle did not match.
    #[bit(10, rw)]
    data_toggle_error: bool,
}

/// What a receive status entry says about its packet.
#[bitenum(u4, exhaustive = false)]
#[derive(Debug, PartialEq, Eq)]
pub enum PacketStatus {
    /// A packet of IN data is in the FIFO.
    InData = 2,
    /// The IN transfer's last packet is in.
    InComplete = 3,
    /// The data toggle of an IN packet did not match.
    DataToggleError = 5,
    /// The channel halted.
    ChannelHalted = 7,
}

/// A receive status entry, as `GRXSTSP` pops it.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct ReceiveStatus {
    /// Host channel the packet belongs to.
    #[bits(0..=3, r)]
    channel: u4,
    /// Bytes of data in the packet.
    #[bits(4..=14, r)]
    byte_count: u11,
    /// PID the packet arrived with.
    #[bits(15..=16, r)]
    pid: u2,
    /// What the packet means; `Err` with the raw value for a status the driver
    /// does not use.
    #[bits(17..=20, r)]
    packet_status: Option<PacketStatus>,
}

/// Receive FIFO depth.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct RxFifoSize {
    /// Depth in 32-bit words.
    #[bits(0..=15, rw)]
    depth: u16,
}

/// Non-periodic transmit FIFO placement.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct TxFifoSize {
    /// Start address in 32-bit words, from the start of the FIFO RAM.
    #[bits(0..=15, rw)]
    start: u16,
    /// Depth in 32-bit words.
    #[bits(16..=31, rw)]
    depth: u16,
}

/// The global and host registers, from the start of the register page to the
/// first host channel.
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct RegisterBlock {
    /// OTG control and status.
    otg_control: u32,
    _reserved0: u32,
    ahb_config: AhbConfig,
    usb_config: UsbConfig,
    reset: ResetControl,
    #[mmio(Read)]
    interrupt_status: InterruptStatus,
    interrupt_mask: u32,
    /// Receive FIFO status, read without popping.
    #[mmio(PureRead)]
    rx_status: u32,
    /// Receive FIFO status, popping the head entry.
    #[mmio(Read)]
    rx_status_pop: ReceiveStatus,
    /// Receive FIFO depth, in words.
    rx_fifo_size: RxFifoSize,
    /// Non-periodic transmit FIFO start address and depth, in words.
    tx_fifo_size: TxFifoSize,
    _reserved1: [u32; 245],
    /// Host configuration.
    host_config: u32,
    _reserved2: [u32; 15],
    /// Root port control and status.
    port: PortControl,
    _reserved3: [u32; 47],
}

/// One host channel's registers: 0x20 bytes each, starting at 0x500.
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct Channel {
    characteristics: Characteristics,
    _split: u32,
    interrupt: ChannelInterrupts,
    interrupt_mask: ChannelInterrupts,
    transfer_size: TransferSize,
    /// Bus address of the packet data, for DMA transfers.
    dma_address: u32,
    _dma_buffer: [u32; 2],
}

assert_register_offsets!(RegisterBlock, 0x500, {
    otg_control: 0x000,
    ahb_config: 0x008,
    usb_config: 0x00c,
    reset: 0x010,
    interrupt_status: 0x014,
    interrupt_mask: 0x018,
    rx_status: 0x01c,
    rx_status_pop: 0x020,
    rx_fifo_size: 0x024,
    tx_fifo_size: 0x028,
    host_config: 0x400,
    port: 0x440,
});

assert_register_offsets!(Channel, 0x20, {
    characteristics: 0x00,
    interrupt: 0x08,
    interrupt_mask: 0x0c,
    transfer_size: 0x10,
    dma_address: 0x14,
});

/// Bytes between two host channels' register blocks.
pub const CHANNEL_STRIDE: usize = 0x20;
/// Offset of the first host channel's register block.
pub const CHANNEL_BASE: usize = 0x500;
/// Offset of host channel `index`'s register block from the register page.
#[must_use]
pub const fn channel_offset(index: usize) -> usize {
    CHANNEL_BASE + index * CHANNEL_STRIDE
}

/// Speed of the device on the root port, if the field holds a known speed.
#[must_use]
pub fn port_speed(port: PortControl) -> Option<Speed> {
    match port.speed() {
        PortSpeed::High => Some(Speed::High),
        PortSpeed::Full => Some(Speed::Full),
        PortSpeed::Low => Some(Speed::Low),
        PortSpeed::Reserved => None,
    }
}

/// Characteristics of a host channel that talks to `target`, enabled or not.
#[must_use]
pub fn characteristics(target: Target, enable: bool) -> Characteristics {
    Characteristics::ZERO
        .with_max_packet(u11::masked_new(target.max_packet))
        .with_endpoint(u4::masked_new(target.endpoint))
        .with_input(target.input)
        .with_low_speed(target.low_speed)
        .with_kind(endpoint_kind(target.kind))
        .with_transactions(u2::new(1))
        .with_device_address(u7::masked_new(target.device_address))
        .with_disable(false)
        .with_enable(enable)
}

/// The endpoint of a host channel's transfers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub device_address: u8,
    pub endpoint: u8,
    pub input: bool,
    pub low_speed: bool,
    pub kind: TransferKind,
    pub max_packet: u16,
}

const fn endpoint_kind(kind: TransferKind) -> EndpointKind {
    match kind {
        TransferKind::Control => EndpointKind::Control,
        TransferKind::Isochronous => EndpointKind::Isochronous,
        TransferKind::Bulk => EndpointKind::Bulk,
        TransferKind::Interrupt => EndpointKind::Interrupt,
    }
}

/// The transfer size of `bytes` in `packets` packets, starting with `pid`.
#[must_use]
pub fn transfer_size(bytes: u32, packets: u16, pid: Pid) -> TransferSize {
    TransferSize::ZERO
        .with_bytes(u19::masked_new(bytes))
        .with_packets(u10::masked_new(packets))
        .with_pid(pid)
}

/// What a channel interrupt means for the transfer in progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelOutcome {
    /// The transfer finished.
    Complete,
    /// The channel halted, because the driver asked for it.
    Halted,
    /// The endpoint answered with a STALL.
    Stalled,
    /// A NAK, transaction or toggle error: halt and issue the transfer again.
    Retry,
    /// A bus error the transfer cannot recover from.
    Failed,
}

/// The outcome of `interrupts`, or `None` when no event is pending. A STALL
/// outranks the other events, and a bus error outranks a retry.
#[must_use]
pub fn channel_outcome(interrupts: ChannelInterrupts) -> Option<ChannelOutcome> {
    if interrupts.stall() {
        Some(ChannelOutcome::Stalled)
    } else if interrupts.ahb_error() || interrupts.babble() {
        Some(ChannelOutcome::Failed)
    } else if interrupts.nak() || interrupts.transaction_error() || interrupts.data_toggle_error() {
        Some(ChannelOutcome::Retry)
    } else if interrupts.transfer_complete() {
        Some(ChannelOutcome::Complete)
    } else if interrupts.halted() {
        Some(ChannelOutcome::Halted)
    } else {
        None
    }
}

/// The value to write to the root port to set `reset` and `power`, keeping the
/// other bits of `current` as they are and leaving its events alone.
#[must_use]
pub fn port_with_reset_and_power(current: PortControl, reset: bool, power: bool) -> PortControl {
    current.without_events().with_reset(reset).with_power(power)
}
