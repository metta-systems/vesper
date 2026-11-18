//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! Register layout and bit fields of an xHCI controller (§5): the capability
//! registers at the start of BAR0, the operational registers `CAPLENGTH`
//! bytes after them with the port register sets at 0x400 into those, the
//! runtime registers with interrupter 0 at `RTSOFF + 0x20`, and the doorbell
//! array at `DBOFF`. The fields are `bitbybit` bitfields and the blocks are
//! `derive_mmio` register blocks, as in the other drivers. Every register is
//! reached as 32-bit words: a 64-bit register is its low word, then its high
//! word, written in that order (§5.1).

use {
    arbitrary_int::prelude::*, bitbybit::bitfield, derive_mmio::Mmio,
    libmmio::assert_register_offsets, vesper_usb_protocol::enumeration::Speed,
};

/// `CAPLENGTH` and `HCIVERSION` (§5.3.1, §5.3.2).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct LengthVersion {
    /// Bytes from the capability registers to the operational registers.
    #[bits(0..=7, r)]
    length: u8,
    /// The interface version, in BCD (0x0100 is 1.0).
    #[bits(16..=31, r)]
    version: u16,
}

/// `HCSPARAMS1` (§5.3.3).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Structural1 {
    /// Device slots the controller has.
    #[bits(0..=7, r)]
    max_slots: u8,
    /// Interrupters the controller has.
    #[bits(8..=18, r)]
    max_interrupters: u11,
    /// Root hub ports, the highest port ID.
    #[bits(24..=31, r)]
    max_ports: u8,
}

/// `HCSPARAMS2` (§5.3.4).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Structural2 {
    /// Event Ring Segment Table entries, as a power of two.
    #[bits(4..=7, r)]
    segment_table_max: u4,
    /// High five bits of the scratchpad buffer count.
    #[bits(21..=25, r)]
    scratchpads_high: u5,
    /// Low five bits of the scratchpad buffer count.
    #[bits(27..=31, r)]
    scratchpads_low: u5,
}

impl Structural2 {
    /// Pages the controller wants as scratchpad buffers.
    #[must_use]
    pub fn scratchpad_buffers(self) -> usize {
        usize::from(self.scratchpads_high().value()) << 5
            | usize::from(self.scratchpads_low().value())
    }
}

/// `HCCPARAMS1` (§5.3.6).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Capability1 {
    /// The controller takes 64-bit bus addresses.
    #[bit(0, r)]
    addressing_64: bool,
    /// Contexts are 64 bytes rather than 32.
    #[bit(2, r)]
    context_64: bool,
    /// Ports are powered only when software sets `PORTSC.PP`.
    #[bit(3, r)]
    port_power_control: bool,
    /// The first extended capability, in 32-bit words from BAR0; zero for
    /// none.
    #[bits(16..=31, r)]
    extended_capabilities: u16,
}

/// The capability registers at the start of BAR0 (§5.3).
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct Capabilities {
    #[mmio(PureRead)]
    length_version: LengthVersion,
    #[mmio(PureRead)]
    structural1: Structural1,
    #[mmio(PureRead)]
    structural2: Structural2,
    #[mmio(PureRead)]
    structural3: u32,
    #[mmio(PureRead)]
    capability1: Capability1,
    /// Bytes from BAR0 to the doorbell array (bits 1:0 reserved).
    #[mmio(PureRead)]
    doorbell_offset: u32,
    /// Bytes from BAR0 to the runtime registers (bits 4:0 reserved).
    #[mmio(PureRead)]
    runtime_offset: u32,
    #[mmio(PureRead)]
    capability2: u32,
}

assert_register_offsets!(Capabilities, 0x20, {
    length_version: 0x00,
    structural1: 0x04,
    structural2: 0x08,
    structural3: 0x0c,
    capability1: 0x10,
    doorbell_offset: 0x14,
    runtime_offset: 0x18,
    capability2: 0x1c,
});

/// `USBCMD` (§5.4.1).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Command {
    /// Run/Stop: the controller runs its schedules.
    #[bit(0, rw)]
    run: bool,
    /// Host Controller Reset; clears itself when the reset is done.
    #[bit(1, rw)]
    reset: bool,
    /// Interrupter Enable: interrupters may raise their interrupts.
    #[bit(2, rw)]
    interrupts: bool,
    /// Host System Error Enable.
    #[bit(3, rw)]
    host_system_errors: bool,
}

/// `USBSTS` (§5.4.2). The change bits clear when written with 1.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Status {
    /// `HCHalted`: the controller has stopped.
    #[bit(0, r)]
    halted: bool,
    /// Host System Error. Write 1 to clear.
    #[bit(2, rw)]
    host_system_error: bool,
    /// Event Interrupt: an interrupter's `IP` was set. Write 1 to clear.
    #[bit(3, rw)]
    event_interrupt: bool,
    /// Port Change Detect. Write 1 to clear.
    #[bit(4, rw)]
    port_change: bool,
    /// Controller Not Ready: operational and doorbell registers may not be
    /// written until it clears.
    #[bit(11, r)]
    not_ready: bool,
    /// Host Controller Error.
    #[bit(12, r)]
    controller_error: bool,
}

/// `PAGESIZE` bit 0: the controller supports 4 KiB pages (§5.4.3).
pub const PAGE_SIZE_4K: u32 = 1 << 0;

/// `CRCR`'s low word (§5.4.5): the command ring's bus address (bits 31:6) and
/// its control bits.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct CommandRingControl {
    /// Ring Cycle State: the producer cycle state the ring starts with.
    #[bit(0, rw)]
    cycle: bool,
    /// Command Ring Running.
    #[bit(3, r)]
    running: bool,
    /// The ring's bus address, bits 31:6.
    #[bits(6..=31, rw)]
    pointer: u26,
}

/// `CONFIG` (§5.4.7).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Configure {
    /// Device slots enabled.
    #[bits(0..=7, rw)]
    slots_enabled: u8,
}

/// The operational registers, `CAPLENGTH` bytes into BAR0 (§5.4), up to the
/// port register sets.
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct Operational {
    command: Command,
    status: Status,
    #[mmio(PureRead)]
    page_size: u32,
    _reserved0: [u32; 2],
    notification_control: u32,
    command_ring_low: CommandRingControl,
    command_ring_high: u32,
    _reserved1: [u32; 4],
    /// `DCBAAP`: the Device Context Base Address Array's bus address
    /// (64-byte aligned).
    context_array_low: u32,
    context_array_high: u32,
    configure: Configure,
}

assert_register_offsets!(Operational, 0x3c, {
    command: 0x00,
    status: 0x04,
    page_size: 0x08,
    notification_control: 0x14,
    command_ring_low: 0x18,
    command_ring_high: 0x1c,
    context_array_low: 0x30,
    context_array_high: 0x34,
    configure: 0x38,
});

/// `PORTSC` (§5.4.8). The change bits (17 to 23) and `PED` clear when written
/// with 1, and `PR` starts a reset when written with 1; write
/// [`neutral`]`(status)` with the bits to change set, never the value read.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct PortStatus {
    /// Current Connect Status: a device is attached.
    #[bit(0, r)]
    connected: bool,
    /// Port Enabled. Write 1 to disable.
    #[bit(1, rw)]
    enabled: bool,
    /// Over-current Active.
    #[bit(3, r)]
    over_current: bool,
    /// Port Reset: write 1 to reset the port; reads 1 while it resets.
    #[bit(4, rw)]
    reset: bool,
    /// Port Link State.
    #[bits(5..=8, rw)]
    link_state: u4,
    /// Port Power.
    #[bit(9, rw)]
    power: bool,
    /// Port Speed, a Protocol Speed ID: 1 full, 2 low, 3 high, 4 and up
    /// `SuperSpeed` (the defaults of §7.2.2.1.1).
    #[bits(10..=13, r)]
    speed: u4,
    /// Port Indicator Control.
    #[bits(14..=15, rw)]
    indicator: u2,
    /// Connect Status Change. Write 1 to clear.
    #[bit(17, rw)]
    connect_changed: bool,
    /// Port Enabled/Disabled Change. Write 1 to clear.
    #[bit(18, rw)]
    enable_changed: bool,
    /// Warm Port Reset Change. Write 1 to clear.
    #[bit(19, rw)]
    warm_reset_changed: bool,
    /// Over-current Change. Write 1 to clear.
    #[bit(20, rw)]
    over_current_changed: bool,
    /// Port Reset Change: a reset finished. Write 1 to clear.
    #[bit(21, rw)]
    reset_changed: bool,
    /// Port Link State Change. Write 1 to clear.
    #[bit(22, rw)]
    link_state_changed: bool,
    /// Port Config Error Change. Write 1 to clear.
    #[bit(23, rw)]
    config_error_changed: bool,
}

/// The `PORTSC` bits a write keeps as they are when written back as read:
/// port power, the indicator and the wake enables (bits 9, 15:14 and 27:25).
/// Every other writable bit acts on a 1, so it is written 0 unless meant.
const PORT_PRESERVED: u32 = 1 << 9 | 0b11 << 14 | 0b111 << 25;

/// The `PORTSC` value that, written, changes nothing about the port: the
/// preserved bits of `status` and nothing else (as Linux's
/// `xhci_port_state_to_neutral`).
#[must_use]
pub const fn neutral(status: PortStatus) -> PortStatus {
    PortStatus::new_with_raw_value(status.raw_value() & PORT_PRESERVED)
}

/// The speed of the device on a port, for the speeds `usb_hcd_v1` reports;
/// `None` for `SuperSpeed` and reserved values.
#[must_use]
pub const fn port_speed(status: PortStatus) -> Option<Speed> {
    match status.speed().value() {
        1 => Some(Speed::Full),
        2 => Some(Speed::Low),
        3 => Some(Speed::High),
        _ => None,
    }
}

/// The status bits of `usb_hcd_v1.port_status`: bit 0 connected, bit 1
/// enabled, bit 16 connect changed, bit 17 enable changed (a reset that
/// enabled the port, or the port disabled by an error).
#[must_use]
pub const fn port_status_bits(status: PortStatus) -> u32 {
    status.connected() as u32
        | (status.enabled() as u32) << 1
        | (status.connect_changed() as u32) << 16
        | ((status.enable_changed() || status.reset_changed()) as u32) << 17
}

/// One port's register set (§5.4.8 to §5.4.11).
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct Port {
    status_control: PortStatus,
    power_management: u32,
    link_info: u32,
    hardware_lpm: u32,
}

assert_register_offsets!(Port, 0x10, {
    status_control: 0x00,
    power_management: 0x04,
    link_info: 0x08,
    hardware_lpm: 0x0c,
});

/// Offset of port `port_id`'s register set (port IDs count from 1) from the
/// operational registers.
#[must_use]
pub const fn port_offset(port_id: u8) -> usize {
    0x400 + 0x10 * (port_id as usize - 1)
}

/// `IMAN` (§5.5.2.1).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct InterrupterManagement {
    /// Interrupt Pending. Write 1 to clear.
    #[bit(0, rw)]
    pending: bool,
    /// Interrupt Enable.
    #[bit(1, rw)]
    enable: bool,
}

/// `IMOD` (§5.5.2.2): the least time between two interrupts, and the
/// countdown to the next.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Moderation {
    /// Interrupt Moderation Interval, in 250 ns units; 0 turns moderation
    /// off.
    #[bits(0..=15, rw)]
    interval: u16,
    /// Interrupt Moderation Counter, which counts the interval down.
    #[bits(16..=31, rw)]
    counter: u16,
}

/// `ERDP`'s low word (§5.5.2.3.3): the event ring dequeue pointer (bits 31:4)
/// and the Event Handler Busy bit.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct DequeueLow {
    /// Event Handler Busy: set by the controller with `IP`. Write 1 to clear.
    #[bit(3, rw)]
    handler_busy: bool,
    /// The dequeue pointer, bits 31:4.
    #[bits(4..=31, rw)]
    pointer: u28,
}

/// One interrupter's register set, at `RTSOFF + 0x20 + 0x20 * n` (§5.5.2).
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct Interrupter {
    management: InterrupterManagement,
    moderation: Moderation,
    /// `ERSTSZ`: Event Ring Segment Table entries.
    table_size: u32,
    _reserved: u32,
    /// `ERSTBA`: the Event Ring Segment Table's bus address (64-byte
    /// aligned). Writing it starts the event ring.
    table_base_low: u32,
    table_base_high: u32,
    dequeue_low: DequeueLow,
    dequeue_high: u32,
}

assert_register_offsets!(Interrupter, 0x20, {
    management: 0x00,
    moderation: 0x04,
    table_size: 0x08,
    table_base_low: 0x10,
    table_base_high: 0x14,
    dequeue_low: 0x18,
    dequeue_high: 0x1c,
});

/// Offset of interrupter 0's register set from the runtime registers.
pub const INTERRUPTER_0: usize = 0x20;

/// The low and high words of a 64-bit bus address.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    reason = "the low word is the truncation"
)]
pub const fn split_address(address: u64) -> (u32, u32) {
    (address as u32, (address >> 32) as u32)
}

/// Extended capability ID of a Supported Protocol capability (§7.2).
const SUPPORTED_PROTOCOL: u32 = 2;
/// Most extended capabilities walked, so a looping list ends.
const MAX_CAPABILITIES: usize = 64;

/// A range of root hub ports that speak one USB major revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ports {
    /// The USB major revision (2 or 3).
    pub major: u8,
    /// The first port ID.
    pub first: u8,
    /// How many ports.
    pub count: u8,
}

impl Ports {
    /// The port ID of the `index`th port of the range (from 0), if it has one.
    #[must_use]
    pub fn port_id(&self, index: u32) -> Option<u8> {
        u8::try_from(index)
            .ok()
            .filter(|&index| index < self.count)
            .and_then(|index| self.first.checked_add(index))
    }
}

/// The ports of USB major revision `major`, from the first Supported Protocol
/// capability for it (§7.2): `read(offset)` reads the 32-bit word at `offset`
/// bytes into BAR0, `None` past its end, and `first` is
/// `HCCPARAMS1.xECP`.
pub fn protocol_ports(read: impl Fn(usize) -> Option<u32>, first: u16, major: u8) -> Option<Ports> {
    let mut offset = usize::from(first) * 4;
    if offset == 0 {
        return None;
    }
    for _ in 0..MAX_CAPABILITIES {
        let header = read(offset)?;
        let [id, next, _minor, revision] = header.to_le_bytes();
        if u32::from(id) == SUPPORTED_PROTOCOL && revision == major {
            let [first, count, ..] = read(offset + 8)?.to_le_bytes();
            return Some(Ports {
                major,
                first,
                count,
            });
        }
        if next == 0 {
            return None;
        }
        offset += usize::from(next) * 4;
    }
    None
}
