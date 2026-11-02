//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! Parsing of the standard descriptors enumeration and class matching need.
//! Descriptors come back as one byte stream (a configuration descriptor with
//! all its interface and endpoint descriptors follows its header); `walk`
//! yields them in order.

/// Descriptor type of an interface descriptor.
pub const DESCRIPTOR_INTERFACE: u8 = 4;
/// Descriptor type of an endpoint descriptor.
pub const DESCRIPTOR_ENDPOINT: u8 = 5;

/// Why a descriptor could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// The stream ends inside a descriptor.
    Truncated,
    /// A descriptor's length is too small for its own header, or is not a
    /// valid descriptor length.
    BadLength,
}

/// A device descriptor, with only the fields enumeration uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Device {
    pub class_code: u8,
    pub subclass: u8,
    pub protocol: u8,
    /// Maximum packet size of endpoint 0 (8, 16, 32 or 64 bytes).
    pub max_packet0: u8,
    pub vendor: u16,
    pub product: u16,
    pub configurations: u8,
}

/// A configuration descriptor header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Configuration {
    /// Length of the configuration and all descriptors after it.
    pub total_length: u16,
    pub interfaces: u8,
    /// The `bConfigurationValue` that `SET_CONFIGURATION` takes.
    pub value: u8,
}

/// An interface descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Interface {
    pub number: u8,
    pub alternate: u8,
    pub class_code: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub endpoints: u8,
}

/// The transfer type an endpoint carries, from `bmAttributes`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferKind {
    Control,
    Isochronous,
    Bulk,
    Interrupt,
}

/// An endpoint descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub address: u8,
    pub attributes: u8,
    /// Maximum packet size, from `wMaxPacketSize` without the high-speed
    /// transaction bits.
    pub max_packet: u16,
    /// Polling interval: frames for full-speed, microframe exponents for high-speed.
    pub interval: u8,
}

impl Endpoint {
    /// The endpoint number, the low nibble of the address.
    #[must_use]
    pub const fn number(self) -> u8 {
        self.address & 0x0f
    }

    /// Whether the endpoint sends data to the host.
    #[must_use]
    pub const fn is_in(self) -> bool {
        self.address & 0x80 != 0
    }

    /// The transfer type, from the low two bits of `bmAttributes`.
    #[must_use]
    pub const fn transfer_kind(self) -> TransferKind {
        match self.attributes & 0x03 {
            0 => TransferKind::Control,
            1 => TransferKind::Isochronous,
            2 => TransferKind::Bulk,
            _ => TransferKind::Interrupt,
        }
    }
}

/// One descriptor from the stream. Descriptors of types this module does not
/// parse are returned as `Other` with their body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Descriptor<'a> {
    Device(Device),
    Configuration(Configuration),
    Interface(Interface),
    Endpoint(Endpoint),
    Other { kind: u8, body: &'a [u8] },
}

/// Iterator over the descriptors of a byte stream. After an error it yields
/// nothing more, since the stream's framing is lost.
#[derive(Clone, Debug)]
pub struct Walker<'a> {
    rest: &'a [u8],
}

/// Walk the descriptors in `bytes`.
#[must_use]
pub const fn walk(bytes: &[u8]) -> Walker<'_> {
    Walker { rest: bytes }
}

impl<'a> Iterator for Walker<'a> {
    type Item = Result<Descriptor<'a>, ParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        let length = usize::from(*self.rest.first()?);
        if length < 2 {
            self.rest = &[];
            return Some(Err(ParseError::BadLength));
        }
        if length > self.rest.len() {
            self.rest = &[];
            return Some(Err(ParseError::Truncated));
        }
        let (body, rest) = self.rest.split_at(length);
        self.rest = rest;
        Some(parse(body))
    }
}

fn byte(body: &[u8], index: usize) -> Result<u8, ParseError> {
    body.get(index).copied().ok_or(ParseError::BadLength)
}

fn word(body: &[u8], index: usize) -> Result<u16, ParseError> {
    Ok(u16::from_le_bytes([
        byte(body, index)?,
        byte(body, index + 1)?,
    ]))
}

fn parse(body: &[u8]) -> Result<Descriptor<'_>, ParseError> {
    let kind = byte(body, 1)?;
    match kind {
        1 => {
            if body.len() < 18 {
                return Err(ParseError::BadLength);
            }
            Ok(Descriptor::Device(Device {
                class_code: byte(body, 4)?,
                subclass: byte(body, 5)?,
                protocol: byte(body, 6)?,
                max_packet0: byte(body, 7)?,
                vendor: word(body, 8)?,
                product: word(body, 10)?,
                configurations: byte(body, 17)?,
            }))
        }
        2 => {
            if body.len() < 9 {
                return Err(ParseError::BadLength);
            }
            Ok(Descriptor::Configuration(Configuration {
                total_length: word(body, 2)?,
                interfaces: byte(body, 4)?,
                value: byte(body, 5)?,
            }))
        }
        DESCRIPTOR_INTERFACE => {
            if body.len() < 9 {
                return Err(ParseError::BadLength);
            }
            Ok(Descriptor::Interface(Interface {
                number: byte(body, 2)?,
                alternate: byte(body, 3)?,
                endpoints: byte(body, 4)?,
                class_code: byte(body, 5)?,
                subclass: byte(body, 6)?,
                protocol: byte(body, 7)?,
            }))
        }
        DESCRIPTOR_ENDPOINT => {
            if body.len() < 7 {
                return Err(ParseError::BadLength);
            }
            Ok(Descriptor::Endpoint(Endpoint {
                address: byte(body, 2)?,
                attributes: byte(body, 3)?,
                max_packet: word(body, 4)? & 0x07ff,
                interval: byte(body, 6)?,
            }))
        }
        _ => Ok(Descriptor::Other { kind, body }),
    }
}
