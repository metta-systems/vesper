#![no_std]

//! PCI configuration space and bus enumeration over any host bridge's
//! configuration access ([`ConfigSpace`]).
//!
//! [`enumerate`] walks the buses depth first from bus 0: it numbers the buses
//! behind each bridge, sizes every function's memory BARs and assigns them
//! from the host bridge's outbound windows, programs each bridge's memory
//! windows around what lies behind it and turns on the bridge's memory
//! decoding and bus mastering. Endpoints keep their decoding off: turning it
//! on is their driver's business. It records what it found ([`Device`]),
//! with the capabilities a child driver needs.
//!
//! Sections are those of the PCI Local Bus Specification 3.0 (the type 0
//! header, §6.1, and BARs, §6.2.5.1), the PCI-to-PCI Bridge Architecture
//! Specification 1.2 (the type 1 header, §3.2) and the PCI Express Base
//! Specification 4.0 (the PCI Express capability, §7.5.3; MSI and MSI-X,
//! §7.7).

use {
    arbitrary_int::{u4, u6, u7, u12, u24, u28},
    bitbybit::bitfield,
};

/// A function's address: bus, device and function numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Function {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl core::fmt::Display for Function {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "{:02x}:{:02x}.{}",
            self.bus, self.device, self.function
        )
    }
}

/// A host bridge's configuration access, one 32-bit word at a time.
pub trait ConfigSpace {
    /// The word at `offset` (a multiple of 4 below 4096) of `function`'s
    /// configuration space; `None` when the bridge cannot reach it (no link,
    /// or an address it does not forward).
    fn read(&mut self, function: Function, offset: u16) -> Option<u32>;
    /// Write the word at `offset` of `function`'s configuration space; the
    /// bridge drops writes it cannot forward.
    fn write(&mut self, function: Function, offset: u16, value: u32);
}

// Header words (§6.1, bridge §3.2).
const VENDOR_DEVICE: u16 = 0x00;
const COMMAND_STATUS: u16 = 0x04;
const CLASS_REVISION: u16 = 0x08;
const HEADER: u16 = 0x0c;
const BAR0: u16 = 0x10;
const BUS_NUMBERS: u16 = 0x18;
const MEMORY_BASE_LIMIT: u16 = 0x20;
const PREFETCH_BASE_LIMIT: u16 = 0x24;
const PREFETCH_BASE_UPPER: u16 = 0x28;
const PREFETCH_LIMIT_UPPER: u16 = 0x2c;
const CAPABILITIES_POINTER: u16 = 0x34;

/// Vendor ID of an absent function.
const NO_VENDOR: u16 = 0xffff;

/// `Vendor ID` and `Device ID` (§6.2.1).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Identity {
    #[bits(0..=15, r)]
    vendor: u16,
    #[bits(16..=31, r)]
    device: u16,
}

/// `Revision ID` and `Class Code` (§6.2.1).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct ClassRevision {
    #[bits(0..=7, r)]
    revision: u8,
    /// Base class, subclass and programming interface
    #[bits(8..=31, r)]
    class: u24,
}

/// `Capabilities Pointer` (§6.7): the low two bits are reserved.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct CapabilitiesPointer {
    #[bits(2..=7, r)]
    words: u6,
}

/// The low and high halves of a 64-bit address.
const fn halves(address: u64) -> (u32, u32) {
    let bytes = address.to_le_bytes();
    (
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
    )
}

/// Address bits 31:20 of `address`, as a bridge window register holds them.
fn megabytes(address: u64) -> u12 {
    u12::new(u16::try_from((address >> 20) & 0xfff).unwrap_or(0))
}

/// `Command` and `Status` (§6.2.2, §6.2.3).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct CommandStatus {
    #[bit(0, rw)]
    io_space: bool,
    #[bit(1, rw)]
    memory_space: bool,
    #[bit(2, rw)]
    bus_master: bool,
    #[bit(10, rw)]
    interrupt_disable: bool,
    /// `Status` bit 4: the function has a capabilities list
    #[bit(20, r)]
    capabilities_list: bool,
}

/// `Cache Line Size`, `Latency Timer`, `Header Type` and `BIST` (§6.2.4).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Header {
    #[bits(16..=22, r)]
    layout: u7,
    /// More than one function in the device
    #[bit(23, r)]
    multi_function: bool,
}

/// `Header Type` layout: a type 0 (device) header.
const LAYOUT_DEVICE: u7 = u7::new(0);
/// `Header Type` layout: a type 1 (PCI-to-PCI bridge) header.
const LAYOUT_BRIDGE: u7 = u7::new(1);

/// A memory BAR (§6.2.5.1); bit 0 tells an I/O BAR, which is not decoded
/// further.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct MemoryBar {
    #[bit(0, r)]
    io: bool,
    /// 64-bit: the next BAR holds the upper half
    #[bit(2, r)]
    wide: bool,
    #[bit(3, r)]
    prefetchable: bool,
    #[bits(4..=31, rw)]
    address: u28,
}

/// The bridge's bus numbers (§3.2.5.2 to §3.2.5.4).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct BusNumbers {
    #[bits(0..=7, rw)]
    primary: u8,
    #[bits(8..=15, rw)]
    secondary: u8,
    #[bits(16..=23, rw)]
    subordinate: u8,
}

/// A bridge's memory or prefetchable memory window (§3.2.5.8, §3.2.5.9):
/// address bits 31:20 of its base and limit; the limit covers its whole
/// megabyte. The low nibbles of the prefetchable window say whether it has
/// upper halves.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct WindowBaseLimit {
    /// Prefetchable window: 1 for 64-bit addressing
    #[bits(0..=3, r)]
    addressing: u4,
    #[bits(4..=15, rw)]
    base: u12,
    #[bits(20..=31, rw)]
    limit: u12,
}

/// The first word of a capability (§6.7): its ID and the next one's offset;
/// for the PCI Express capability, also its `PCI Express Capabilities`.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct CapabilityHeader {
    #[bits(0..=7, r)]
    id: u8,
    #[bits(8..=15, r)]
    next: u8,
    /// PCI Express capability: `Device/Port Type` (§7.5.3.2)
    #[bits(20..=23, r)]
    port_type: u4,
}

/// Capability IDs (PCI Code and ID Assignment Specification).
const CAPABILITY_MSI: u8 = 0x05;
const CAPABILITY_EXPRESS: u8 = 0x10;
const CAPABILITY_MSIX: u8 = 0x11;
/// Most capabilities followed, against a looping list.
const MAX_CAPABILITIES: usize = 48;

/// `Device/Port Type` of a PCI Express port (§7.5.3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortType {
    Endpoint,
    LegacyEndpoint,
    RootPort,
    UpstreamPort,
    DownstreamPort,
    /// Another type (bridges to and from PCI, root complex devices)
    Other(u8),
}

impl PortType {
    const fn from_code(code: u8) -> Self {
        match code {
            0b0000 => Self::Endpoint,
            0b0001 => Self::LegacyEndpoint,
            0b0100 => Self::RootPort,
            0b0101 => Self::UpstreamPort,
            0b0110 => Self::DownstreamPort,
            other => Self::Other(other),
        }
    }

    /// Whether the link below the port holds only device 0 (§7.3.1: a
    /// downstream port's link reaches one device), as Linux's
    /// `pci_scan_slot` assumes.
    const fn has_one_device_below(self) -> bool {
        matches!(self, Self::RootPort | Self::DownstreamPort)
    }
}

/// Granularity of a bridge's memory windows.
const BRIDGE_WINDOW_ALIGN: u64 = 1 << 20;

/// Base class and subclass of a PCI-to-PCI bridge.
pub const CLASS_BRIDGE_PCI: u32 = 0x0604;

/// BARs of a type 0 header; a bridge has the first two.
pub const MAX_BARS: usize = 6;
const DEVICE_BARS: u8 = 6;
const BRIDGE_BARS: u8 = 2;
const _: () = assert!(DEVICE_BARS as usize == MAX_BARS);

/// What a BAR decodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BarKind {
    /// 32-bit memory
    #[default]
    Memory32,
    /// 64-bit memory: it takes the next BAR's word too
    Memory64,
    /// I/O space, which is never assigned (no window carries it)
    Io,
}

/// One BAR of a function, as assigned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Bar {
    /// Its number: the BAR at offset `0x10 + 4 * index`
    pub index: u8,
    pub kind: BarKind,
    /// The PCI address assigned; meaningless if `!assigned`
    pub pci: u64,
    pub size: u64,
    pub prefetchable: bool,
    /// Whether a window had room for it
    pub assigned: bool,
}

/// One function [`enumerate`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Device {
    pub function: Function,
    pub vendor: u16,
    pub device: u16,
    /// Base class, subclass and programming interface
    pub class: u32,
    pub revision: u8,
    /// For a bridge: the first and last bus behind it
    pub buses: Option<(u8, u8)>,
    pub bar_count: usize,
    pub bars: [Bar; MAX_BARS],
    /// Its PCI Express capability's port type, when it has one
    pub express: Option<PortType>,
    /// Offset of its MSI capability
    pub msi: Option<u16>,
    /// Offset of its MSI-X capability
    pub msix: Option<u16>,
}

impl Device {
    /// Its BARs.
    pub fn bars(&self) -> &[Bar] {
        &self.bars[..self.bar_count.min(MAX_BARS)]
    }

    /// Its base class and subclass.
    pub const fn base_class(&self) -> u32 {
        self.class >> 8
    }
}

/// One outbound window of the host bridge: the PCI addresses BARs may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub pci: u64,
    pub size: u64,
    pub prefetchable: bool,
}

/// The rest of a window, from `next` up to `end`.
#[derive(Debug, Clone, Copy)]
struct Cursor {
    next: u64,
    end: u64,
}

impl Cursor {
    /// `size` bytes aligned to `size`, if they fit below `end` (and below
    /// 4 GiB when `below_4g`).
    fn take(&mut self, size: u64, below_4g: bool) -> Option<u64> {
        let start = self.next.checked_next_multiple_of(size)?;
        let end = start.checked_add(size)?;
        if end > self.end || (below_4g && end > 1 << 32) {
            return None;
        }
        self.next = end;
        Some(start)
    }

    fn align(&mut self, alignment: u64) {
        self.next = self.next.next_multiple_of(alignment).min(self.end);
    }
}

/// What enumeration hands out: the non-prefetchable window below 4 GiB, and
/// the prefetchable one if the host bridge has one.
struct Allocator {
    memory: Option<Cursor>,
    prefetchable: Option<Cursor>,
}

impl Allocator {
    fn new(windows: &[Window]) -> Self {
        let cursor = |window: &Window| Cursor {
            next: window.pci,
            end: window.pci + window.size,
        };
        Self {
            memory: windows
                .iter()
                .find(|window| !window.prefetchable && window.pci < 1 << 32)
                .map(cursor),
            prefetchable: windows
                .iter()
                .find(|window| window.prefetchable)
                .map(cursor),
        }
    }

    /// Room for a memory BAR: a prefetchable BAR from the prefetchable
    /// window when there is one (a 32-bit BAR only below 4 GiB), else from
    /// the non-prefetchable window, which a bridge forwards only below
    /// 4 GiB.
    fn take(&mut self, size: u64, prefetchable: bool, wide: bool) -> Option<u64> {
        if prefetchable && let Some(cursor) = self.prefetchable.as_mut() {
            let mut attempt = *cursor;
            if let Some(start) = attempt.take(size, !wide) {
                *cursor = attempt;
                return Some(start);
            }
        }
        self.memory.as_mut()?.take(size, true)
    }

    fn align(&mut self) {
        for cursor in [self.memory.as_mut(), self.prefetchable.as_mut()]
            .into_iter()
            .flatten()
        {
            cursor.align(BRIDGE_WINDOW_ALIGN);
        }
    }

    fn marks(&self) -> (Option<u64>, Option<u64>) {
        (
            self.memory.map(|cursor| cursor.next),
            self.prefetchable.map(|cursor| cursor.next),
        )
    }
}

/// Why [`enumerate`] stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Bus 255 was reached with bridges still to number
    OutOfBuses,
}

/// The walk's state.
struct Walk<'record, Config> {
    config: Config,
    allocator: Allocator,
    found: &'record mut [Device],
    count: usize,
    last_bus: u8,
}

impl<Config: ConfigSpace> Walk<'_, Config> {
    fn read(&mut self, function: Function, offset: u16) -> Option<u32> {
        self.config.read(function, offset)
    }

    /// The capabilities of `function`: its PCI Express port type and its
    /// MSI and MSI-X capability offsets.
    fn capabilities(&mut self, function: Function, record: &mut Device) {
        let status = CommandStatus::new_with_raw_value(
            self.read(function, COMMAND_STATUS).unwrap_or(0),
        );
        if !status.capabilities_list() {
            return;
        }
        let pointer = CapabilitiesPointer::new_with_raw_value(
            self.read(function, CAPABILITIES_POINTER).unwrap_or(0),
        );
        let mut offset = u16::from(pointer.words().value()) * 4;
        for _ in 0..MAX_CAPABILITIES {
            if offset < 0x40 {
                break;
            }
            let Some(word) = self.read(function, offset) else {
                break;
            };
            let header = CapabilityHeader::new_with_raw_value(word);
            match header.id() {
                CAPABILITY_EXPRESS => {
                    record.express = Some(PortType::from_code(header.port_type().value()));
                }
                CAPABILITY_MSI => record.msi = Some(offset),
                CAPABILITY_MSIX => record.msix = Some(offset),
                _ => {}
            }
            offset = u16::from(header.next() & !0b11);
        }
    }

    /// Size the first `count` BARs of `function` and assign each a range of
    /// a window; memory decoding is off meanwhile and restored after.
    fn assign_bars(&mut self, function: Function, count: u8, record: &mut Device) {
        let command = self.read(function, COMMAND_STATUS).unwrap_or(0);
        let quiet = CommandStatus::new_with_raw_value(command)
            .with_io_space(false)
            .with_memory_space(false);
        self.config
            .write(function, COMMAND_STATUS, quiet.raw_value() & 0xffff);
        let mut index = 0;
        while index < count {
            let offset = BAR0 + 4 * u16::from(index);
            let original = self.read(function, offset).unwrap_or(0);
            self.config.write(function, offset, u32::MAX);
            let probe = self.read(function, offset).unwrap_or(0);
            self.config.write(function, offset, original);
            let bar = MemoryBar::new_with_raw_value(probe);
            if probe == 0 {
                index += 1;
                continue;
            }
            if bar.io() {
                // I/O space: no window carries it.
                record.bars[record.bar_count] = Bar {
                    index,
                    kind: BarKind::Io,
                    size: u64::from(!(probe & !0b11) & 0xffff) + 1,
                    ..Bar::default()
                };
                record.bar_count += 1;
                index += 1;
                continue;
            }
            let wide = bar.wide() && index + 1 < count;
            let upper = if wide {
                let upper_offset = offset + 4;
                let original_upper = self.read(function, upper_offset).unwrap_or(0);
                self.config.write(function, upper_offset, u32::MAX);
                let probe_upper = self.read(function, upper_offset).unwrap_or(0);
                self.config.write(function, upper_offset, original_upper);
                probe_upper
            } else {
                u32::MAX
            };
            let mask = u64::from(upper) << 32 | u64::from(probe & !0xf);
            let size = (!mask).wrapping_add(1);
            let mut entry = Bar {
                index,
                kind: if wide {
                    BarKind::Memory64
                } else {
                    BarKind::Memory32
                },
                size,
                prefetchable: bar.prefetchable(),
                ..Bar::default()
            };
            if size.is_power_of_two()
                && let Some(start) = self.allocator.take(size, bar.prefetchable(), wide)
            {
                let (low, high) = halves(start);
                self.config.write(function, offset, low);
                if wide {
                    self.config.write(function, offset + 4, high);
                }
                entry.pci = start;
                entry.assigned = true;
            }
            record.bars[record.bar_count] = entry;
            record.bar_count += 1;
            index += if wide { 2 } else { 1 };
        }
        self.config
            .write(function, COMMAND_STATUS, command & 0xffff);
    }

    /// Program bridge `function`'s memory window to `[start, end)` of the
    /// allocator's non-prefetchable window, and its prefetchable window to
    /// `[prefetch_start, prefetch_end)`; an empty range closes its window
    /// (base above limit).
    fn set_windows(&mut self, function: Function, memory: (u64, u64), prefetchable: (u64, u64)) {
        let window = |(start, end): (u64, u64)| {
            if end <= start {
                return (
                    WindowBaseLimit::new_with_raw_value(0)
                        .with_base(u12::new(0xfff))
                        .with_limit(u12::new(0)),
                    u32::MAX,
                    0,
                );
            }
            let last = end - 1;
            (
                WindowBaseLimit::new_with_raw_value(0)
                    .with_base(megabytes(start))
                    .with_limit(megabytes(last)),
                halves(start).1,
                halves(last).1,
            )
        };
        let (memory_window, _, _) = window(memory);
        self.config
            .write(function, MEMORY_BASE_LIMIT, memory_window.raw_value());
        let (prefetch_window, upper_base, upper_limit) = window(prefetchable);
        self.config
            .write(function, PREFETCH_BASE_LIMIT, prefetch_window.raw_value());
        self.config
            .write(function, PREFETCH_BASE_UPPER, upper_base);
        self.config
            .write(function, PREFETCH_LIMIT_UPPER, upper_limit);
    }

    /// Number the buses behind bridge `function` on `bus`, walk them, and
    /// open its windows over what they took.
    fn bridge(&mut self, function: Function, record: &mut Device) -> Result<(), Error> {
        let secondary = self.last_bus.checked_add(1).ok_or(Error::OutOfBuses)?;
        self.last_bus = secondary;
        let numbers = BusNumbers::new_with_raw_value(self.read(function, BUS_NUMBERS).unwrap_or(0))
            .with_primary(function.bus)
            .with_secondary(secondary)
            .with_subordinate(u8::MAX);
        self.config.write(function, BUS_NUMBERS, numbers.raw_value());
        self.allocator.align();
        let (memory_start, prefetch_start) = self.allocator.marks();
        let one_device = record
            .express
            .is_some_and(PortType::has_one_device_below);
        self.bus(secondary, one_device)?;
        self.allocator.align();
        let (memory_end, prefetch_end) = self.allocator.marks();
        let numbers = numbers.with_subordinate(self.last_bus);
        self.config.write(function, BUS_NUMBERS, numbers.raw_value());
        let range = |start: Option<u64>, end: Option<u64>| (start.unwrap_or(0), end.unwrap_or(0));
        self.set_windows(
            function,
            range(memory_start, memory_end),
            range(prefetch_start, prefetch_end),
        );
        let command = CommandStatus::new_with_raw_value(
            self.read(function, COMMAND_STATUS).unwrap_or(0),
        )
        .with_memory_space(true)
        .with_bus_master(true);
        self.config
            .write(function, COMMAND_STATUS, command.raw_value() & 0xffff);
        record.buses = Some((secondary, self.last_bus));
        Ok(())
    }

    /// Walk the devices of `bus`: only device 0 below a port whose link
    /// reaches one device.
    fn bus(&mut self, bus: u8, one_device: bool) -> Result<(), Error> {
        let devices = if one_device { 1 } else { 32 };
        for device in 0..devices {
            let mut functions = 1;
            let mut number = 0;
            while number < functions {
                let function = Function {
                    bus,
                    device,
                    function: number,
                };
                number += 1;
                let Some(identity) = self
                    .read(function, VENDOR_DEVICE)
                    .map(Identity::new_with_raw_value)
                    .filter(|identity| identity.vendor() != NO_VENDOR)
                else {
                    continue;
                };
                let header = Header::new_with_raw_value(self.read(function, HEADER).unwrap_or(0));
                if function.function == 0 && header.multi_function() {
                    functions = 8;
                }
                let class_revision = ClassRevision::new_with_raw_value(
                    self.read(function, CLASS_REVISION).unwrap_or(0),
                );
                let mut record = Device {
                    function,
                    vendor: identity.vendor(),
                    device: identity.device(),
                    class: class_revision.class().value(),
                    revision: class_revision.revision(),
                    ..Device::default()
                };
                self.capabilities(function, &mut record);
                let layout = header.layout();
                if layout == LAYOUT_DEVICE {
                    self.assign_bars(function, DEVICE_BARS, &mut record);
                } else if layout == LAYOUT_BRIDGE {
                    self.assign_bars(function, BRIDGE_BARS, &mut record);
                }
                // The record goes in before the devices behind a bridge.
                let position = self.count;
                self.count += 1;
                if layout == LAYOUT_BRIDGE {
                    self.bridge(function, &mut record)?;
                }
                if let Some(slot) = self.found.get_mut(position) {
                    *slot = record;
                }
            }
        }
        Ok(())
    }
}

/// Enumerate the buses below a host bridge from bus 0, assigning BARs from
/// `windows`; the functions found go into `found` in walk order (a bridge
/// before what lies behind it), and the count of all found, which may exceed
/// `found.len()`, is returned.
///
/// # Errors
///
/// [`Error::OutOfBuses`] when bridges need more than 255 buses.
pub fn enumerate(
    config: impl ConfigSpace,
    windows: &[Window],
    found: &mut [Device],
) -> Result<usize, Error> {
    let mut walk = Walk {
        config,
        allocator: Allocator::new(windows),
        found,
        count: 0,
        last_bus: 0,
    };
    walk.bus(0, false)?;
    Ok(walk.count)
}

impl<Config: ConfigSpace> ConfigSpace for &mut Config {
    fn read(&mut self, function: Function, offset: u16) -> Option<u32> {
        (**self).read(function, offset)
    }

    fn write(&mut self, function: Function, offset: u16, value: u32) {
        (**self).write(function, offset, value);
    }
}
