//
// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
//

//! The event record: 24 bytes, little-endian, 8-byte aligned in a buffer.
//!
//! | Offset | Field | Type |
//! |---:|---|---|
//! | 0 | `timestamp` | `u64` nanoseconds |
//! | 8 | `subject` | `u32` |
//! | 12 | `code` | `u32` |
//! | 16 | `value` | `i32` |
//! | 20 | `kind` | `u8` |
//! | 21 | `flags` | `u8`, reserved, zero |
//! | 22 | padding | `u16`, reserved, zero |

/// Bytes of an encoded record.
pub const EVENT_SIZE: usize = 24;

/// Byte offsets of the fields in an encoded record.
pub mod offset {
    pub const TIMESTAMP: usize = 0;
    pub const SUBJECT: usize = 8;
    pub const CODE: usize = 12;
    pub const VALUE: usize = 16;
    pub const KIND: usize = 20;
    pub const FLAGS: usize = 21;
    pub const PADDING: usize = 22;
}

/// `Key` value: released.
pub const KEY_UP: i32 = 0;
/// `Key` value: pressed.
pub const KEY_DOWN: i32 = 1;
/// `Key` value: held, repeated by a consumer's repeat library.
pub const KEY_REPEAT: i32 = 2;

/// What a record says. The numbers are the wire values; zero is never a kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    /// A key or button changed: `value` is [`KEY_UP`], [`KEY_DOWN`] or [`KEY_REPEAT`].
    Key = 1,
    /// A relative axis moved by `value`.
    Rel = 2,
    /// An absolute axis is at `value`.
    Abs = 3,
    /// Closes a frame.
    Sync = 4,
    /// Records were lost before this one.
    Dropped = 5,
    /// A device appeared; its `subject` is the id of its descriptor.
    DeviceAdded = 6,
    /// A device went away; its keys and buttons are released.
    DeviceRemoved = 7,
    /// A window gained keyboard focus (compositor to client).
    Focus = 8,
    /// The pointer entered a window (compositor to client).
    Enter = 9,
    /// The pointer left a window (compositor to client).
    Leave = 10,
    /// The compositor offers a new window size (compositor to client).
    ResizeOffer = 11,
    /// The user asked to close a window (compositor to client).
    CloseRequest = 12,
}

impl Kind {
    /// The kind a wire value names.
    #[must_use]
    pub const fn from_wire(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::Key,
            2 => Self::Rel,
            3 => Self::Abs,
            4 => Self::Sync,
            5 => Self::Dropped,
            6 => Self::DeviceAdded,
            7 => Self::DeviceRemoved,
            8 => Self::Focus,
            9 => Self::Enter,
            10 => Self::Leave,
            11 => Self::ResizeOffer,
            12 => Self::CloseRequest,
            _ => return None,
        })
    }

    /// The wire value.
    #[must_use]
    pub const fn to_wire(self) -> u8 {
        self as u8
    }
}

/// Why a buffer is not a record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// The buffer is not [`EVENT_SIZE`] bytes.
    Length(usize),
    /// The kind is zero or unassigned.
    UnknownKind(u8),
    /// A reserved byte is not zero.
    Reserved,
    /// A `Key` record's value is not 0, 1 or 2.
    KeyValue(i32),
}

/// One input event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    /// Nanoseconds from the physical counter.
    pub timestamp: u64,
    /// What the record is about: the device on a producer's record, the window on
    /// a compositor's.
    pub subject: u32,
    /// `(usage page << 16) | usage`; zero where the kind has none.
    pub code: u32,
    pub value: i32,
    pub kind: Kind,
}

impl Event {
    #[must_use]
    pub const fn new(timestamp: u64, subject: u32, kind: Kind, code: u32, value: i32) -> Self {
        Self {
            timestamp,
            subject,
            code,
            value,
            kind,
        }
    }

    /// A `Sync` record closing a frame.
    #[must_use]
    pub const fn sync(timestamp: u64, subject: u32) -> Self {
        Self::new(timestamp, subject, Kind::Sync, 0, 0)
    }

    /// The wire form.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; EVENT_SIZE] {
        let mut bytes = [0; EVENT_SIZE];
        bytes[offset::TIMESTAMP..offset::SUBJECT].copy_from_slice(&self.timestamp.to_le_bytes());
        bytes[offset::SUBJECT..offset::CODE].copy_from_slice(&self.subject.to_le_bytes());
        bytes[offset::CODE..offset::VALUE].copy_from_slice(&self.code.to_le_bytes());
        bytes[offset::VALUE..offset::KIND].copy_from_slice(&self.value.to_le_bytes());
        bytes[offset::KIND] = self.kind.to_wire();
        bytes
    }

    /// Decode a record. A consumer rejects and counts what this refuses.
    ///
    /// # Errors
    ///
    /// [`DecodeError`] if the buffer is not a well-formed record.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let Ok(bytes) = <&[u8; EVENT_SIZE]>::try_from(bytes) else {
            return Err(DecodeError::Length(bytes.len()));
        };
        let kind_byte = bytes[offset::KIND];
        let kind = Kind::from_wire(kind_byte).ok_or(DecodeError::UnknownKind(kind_byte))?;
        if bytes[offset::FLAGS] != 0 || bytes[offset::PADDING..] != [0, 0] {
            return Err(DecodeError::Reserved);
        }
        let value = i32::from_le_bytes(word(bytes, offset::VALUE));
        if kind == Kind::Key && !(KEY_UP..=KEY_REPEAT).contains(&value) {
            return Err(DecodeError::KeyValue(value));
        }
        Ok(Self {
            timestamp: u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            subject: u32::from_le_bytes(word(bytes, offset::SUBJECT)),
            code: u32::from_le_bytes(word(bytes, offset::CODE)),
            value,
            kind,
        })
    }
}

/// The four bytes at `at`.
fn word(bytes: &[u8; EVENT_SIZE], at: usize) -> [u8; 4] {
    [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]
}
