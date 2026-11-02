//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! The device descriptor: what a device is and what it can report. A producer
//! serves it through a Call into a data area, so it has a fixed layout of
//! [`DESCRIPTOR_SIZE`] bytes, little-endian.
//!
//! | Offset | Field | Type |
//! |---:|---|---|
//! | 0 | layout version | `u32`, [`LAYOUT_VERSION`] |
//! | 4 | `bus` | `u16` |
//! | 6 | `vendor` | `u16` |
//! | 8 | `product` | `u16` |
//! | 10 | `version` | `u16` |
//! | 12 | name length | `u8`, at most [`MAX_NAME`] |
//! | 13 | axis count | `u8`, at most [`MAX_DESCRIPTOR_AXES`] |
//! | 14 | key range count | `u8`, at most [`MAX_KEY_RANGES`] |
//! | 15 | reserved | `u8`, zero |
//! | 16 | name | `[u8; 32]`, zero after the length |
//! | 48 | axes | 8 x 20 bytes: `code` `u32`, `min` `i32`, `max` `i32`, `resolution` `u32`, `unit` `u8`, 3 zero bytes |
//! | 208 | key ranges | 8 x 8 bytes: `page` `u16`, `first` `u16`, `last` `u16`, 2 zero bytes |
//!
//! Slots past a count are zero.

use crate::usage::{page_of, usage_of};

/// Bytes of an encoded descriptor.
pub const DESCRIPTOR_SIZE: usize = 272;
/// The only layout version.
pub const LAYOUT_VERSION: u32 = 1;
/// Longest device name, in bytes.
pub const MAX_NAME: usize = 32;
/// Most axes a descriptor lists.
pub const MAX_DESCRIPTOR_AXES: usize = 8;
/// Most key ranges a descriptor lists.
pub const MAX_KEY_RANGES: usize = 8;

const AXIS_SIZE: usize = 20;
const RANGE_SIZE: usize = 8;
const NAME_AT: usize = 16;
const AXES_AT: usize = NAME_AT + MAX_NAME;
const RANGES_AT: usize = AXES_AT + MAX_DESCRIPTOR_AXES * AXIS_SIZE;

/// Bus numbers, as Linux assigns them.
pub mod bus {
    pub const USB: u16 = 0x03;
    pub const VIRTUAL: u16 = 0x06;
}

/// What an axis measures in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Unit {
    None = 0,
    Millimetre = 1,
    Degree = 2,
}

impl Unit {
    const fn from_wire(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Millimetre),
            2 => Some(Self::Degree),
            _ => None,
        }
    }
}

/// An axis the device reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AxisInfo {
    /// The axis usage, `(page << 16) | usage`.
    pub code: u32,
    pub min: i32,
    pub max: i32,
    /// Counts per `unit`.
    pub resolution: u32,
    pub unit: Unit,
}

/// A run of keys on one usage page the device has: usages `first` to `last`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyRange {
    pub page: u16,
    pub first: u16,
    pub last: u16,
}

impl KeyRange {
    /// Whether the range holds the key `key_code`.
    #[must_use]
    pub fn contains(&self, key_code: u32) -> bool {
        page_of(key_code) == self.page && (self.first..=self.last).contains(&usage_of(key_code))
    }
}

/// Why a buffer is not a descriptor, or an entry does not fit one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DescriptorError {
    /// The buffer is not [`DESCRIPTOR_SIZE`] bytes.
    Length(usize),
    /// The layout version is not [`LAYOUT_VERSION`].
    Version(u32),
    /// A count is past its maximum.
    Count,
    /// A reserved byte, or a slot past its count, is not zero.
    Reserved,
    /// An axis unit is unknown.
    Unit(u8),
    /// An axis has `min` above `max`, or a range has `first` above `last`.
    Order,
    /// No room for the entry.
    Full,
}

/// A device descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Descriptor {
    bus: u16,
    vendor: u16,
    product: u16,
    version: u16,
    name: [u8; MAX_NAME],
    name_len: usize,
    axes: [Option<AxisInfo>; MAX_DESCRIPTOR_AXES],
    axis_count: usize,
    ranges: [Option<KeyRange>; MAX_KEY_RANGES],
    range_count: usize,
}

impl Descriptor {
    /// A descriptor of a device with no name, axes or keys.
    #[must_use]
    pub const fn new(bus: u16, vendor: u16, product: u16, version: u16) -> Self {
        Self {
            bus,
            vendor,
            product,
            version,
            name: [0; MAX_NAME],
            name_len: 0,
            axes: [None; MAX_DESCRIPTOR_AXES],
            axis_count: 0,
            ranges: [None; MAX_KEY_RANGES],
            range_count: 0,
        }
    }

    /// The bus the device is on (see [`bus`]).
    #[must_use]
    pub const fn bus(&self) -> u16 {
        self.bus
    }

    #[must_use]
    pub const fn vendor(&self) -> u16 {
        self.vendor
    }

    #[must_use]
    pub const fn product(&self) -> u16 {
        self.product
    }

    #[must_use]
    pub const fn version(&self) -> u16 {
        self.version
    }

    /// Set the name to the first [`MAX_NAME`] bytes of `name`, cut at a
    /// character boundary.
    pub fn set_name(&mut self, name: &str) {
        let mut len = name.len().min(MAX_NAME);
        while !name.is_char_boundary(len) {
            len -= 1;
        }
        self.name = [0; MAX_NAME];
        self.name[..len].copy_from_slice(&name.as_bytes()[..len]);
        self.name_len = len;
    }

    /// The name, as bytes.
    #[must_use]
    pub fn name(&self) -> &[u8] {
        &self.name[..self.name_len]
    }

    /// List another axis.
    ///
    /// # Errors
    ///
    /// [`DescriptorError::Full`] with no room, [`DescriptorError::Order`] if
    /// `min` is above `max`.
    pub fn push_axis(&mut self, axis: AxisInfo) -> Result<(), DescriptorError> {
        if axis.min > axis.max {
            return Err(DescriptorError::Order);
        }
        let slot = self
            .axes
            .get_mut(self.axis_count)
            .ok_or(DescriptorError::Full)?;
        *slot = Some(axis);
        self.axis_count += 1;
        Ok(())
    }

    /// List another run of keys.
    ///
    /// # Errors
    ///
    /// [`DescriptorError::Full`] with no room, [`DescriptorError::Order`] if
    /// `first` is above `last`.
    pub fn push_key_range(&mut self, range: KeyRange) -> Result<(), DescriptorError> {
        if range.first > range.last {
            return Err(DescriptorError::Order);
        }
        let slot = self
            .ranges
            .get_mut(self.range_count)
            .ok_or(DescriptorError::Full)?;
        *slot = Some(range);
        self.range_count += 1;
        Ok(())
    }

    /// The listed axes.
    pub fn axes(&self) -> impl Iterator<Item = &AxisInfo> {
        self.axes[..self.axis_count].iter().flatten()
    }

    /// The listed key ranges.
    pub fn key_ranges(&self) -> impl Iterator<Item = &KeyRange> {
        self.ranges[..self.range_count].iter().flatten()
    }

    /// Whether the device has the key or button `key_code`.
    #[must_use]
    pub fn has_key(&self, key_code: u32) -> bool {
        self.key_ranges().any(|range| range.contains(key_code))
    }

    /// The wire form.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; DESCRIPTOR_SIZE] {
        let mut bytes = [0; DESCRIPTOR_SIZE];
        bytes[0..4].copy_from_slice(&LAYOUT_VERSION.to_le_bytes());
        bytes[4..6].copy_from_slice(&self.bus.to_le_bytes());
        bytes[6..8].copy_from_slice(&self.vendor.to_le_bytes());
        bytes[8..10].copy_from_slice(&self.product.to_le_bytes());
        bytes[10..12].copy_from_slice(&self.version.to_le_bytes());
        bytes[12] = u8::try_from(self.name_len).unwrap_or(0);
        bytes[13] = u8::try_from(self.axis_count).unwrap_or(0);
        bytes[14] = u8::try_from(self.range_count).unwrap_or(0);
        bytes[NAME_AT..NAME_AT + MAX_NAME].copy_from_slice(&self.name);
        for (index, axis) in self.axes().enumerate() {
            let at = AXES_AT + index * AXIS_SIZE;
            bytes[at..at + 4].copy_from_slice(&axis.code.to_le_bytes());
            bytes[at + 4..at + 8].copy_from_slice(&axis.min.to_le_bytes());
            bytes[at + 8..at + 12].copy_from_slice(&axis.max.to_le_bytes());
            bytes[at + 12..at + 16].copy_from_slice(&axis.resolution.to_le_bytes());
            bytes[at + 16] = axis.unit as u8;
        }
        for (index, range) in self.key_ranges().enumerate() {
            let at = RANGES_AT + index * RANGE_SIZE;
            bytes[at..at + 2].copy_from_slice(&range.page.to_le_bytes());
            bytes[at + 2..at + 4].copy_from_slice(&range.first.to_le_bytes());
            bytes[at + 4..at + 6].copy_from_slice(&range.last.to_le_bytes());
        }
        bytes
    }

    /// Decode a descriptor a producer served.
    ///
    /// # Errors
    ///
    /// [`DescriptorError`] if the buffer is not a well-formed descriptor.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DescriptorError> {
        let Ok(bytes) = <&[u8; DESCRIPTOR_SIZE]>::try_from(bytes) else {
            return Err(DescriptorError::Length(bytes.len()));
        };
        let version = read_u32(bytes, 0);
        if version != LAYOUT_VERSION {
            return Err(DescriptorError::Version(version));
        }
        let (name_len, axis_count, range_count) = (
            usize::from(bytes[12]),
            usize::from(bytes[13]),
            usize::from(bytes[14]),
        );
        if name_len > MAX_NAME || axis_count > MAX_DESCRIPTOR_AXES || range_count > MAX_KEY_RANGES {
            return Err(DescriptorError::Count);
        }
        if bytes[15] != 0 || bytes[NAME_AT + name_len..AXES_AT].iter().any(|&b| b != 0) {
            return Err(DescriptorError::Reserved);
        }

        let mut descriptor = Self::new(
            read_u16(bytes, 4),
            read_u16(bytes, 6),
            read_u16(bytes, 8),
            read_u16(bytes, 10),
        );
        descriptor.name[..name_len].copy_from_slice(&bytes[NAME_AT..NAME_AT + name_len]);
        descriptor.name_len = name_len;
        for index in 0..MAX_DESCRIPTOR_AXES {
            let at = AXES_AT + index * AXIS_SIZE;
            let slot = &bytes[at..at + AXIS_SIZE];
            if index >= axis_count {
                if slot.iter().any(|&b| b != 0) {
                    return Err(DescriptorError::Reserved);
                }
                continue;
            }
            if slot[17..] != [0, 0, 0] {
                return Err(DescriptorError::Reserved);
            }
            let unit = Unit::from_wire(slot[16]).ok_or(DescriptorError::Unit(slot[16]))?;
            descriptor.push_axis(AxisInfo {
                code: read_u32(slot, 0),
                min: i32::from_le_bytes([slot[4], slot[5], slot[6], slot[7]]),
                max: i32::from_le_bytes([slot[8], slot[9], slot[10], slot[11]]),
                resolution: read_u32(slot, 12),
                unit,
            })?;
        }
        for index in 0..MAX_KEY_RANGES {
            let at = RANGES_AT + index * RANGE_SIZE;
            let slot = &bytes[at..at + RANGE_SIZE];
            if index >= range_count {
                if slot.iter().any(|&b| b != 0) {
                    return Err(DescriptorError::Reserved);
                }
                continue;
            }
            if slot[6..] != [0, 0] {
                return Err(DescriptorError::Reserved);
            }
            descriptor.push_key_range(KeyRange {
                page: read_u16(slot, 0),
                first: read_u16(slot, 2),
                last: read_u16(slot, 4),
            })?;
        }
        Ok(descriptor)
    }
}

fn read_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}
