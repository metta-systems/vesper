#![no_std]

//! What a block device driver and its client agree on: requests to read and
//! write whole blocks travel as packets on an IO channel (`libio-channel`),
//! the queue of a disk driver.
//!
//! A request is one packet. Its first record is the request's [`Header`],
//! [`HEADER_BYTES`] in the data area; every further record is a buffer of
//! whole [`BLOCK_BYTES`] blocks, and the buffers together hold the blocks from
//! `Header::block` on, in record order:
//!
//! - [`Op::Read`] fills the buffers from the device;
//! - [`Op::Write`] stores the buffers on the device.
//!
//! The client submits requests on `requests.submit` and tells the driver they
//! wait (`block_v1.submitted`). The driver serves them in order and returns
//! each packet on `requests.complete`, its value and records unchanged, with
//! the outcome in the header's `status`; the client sleeps on the channel's
//! `EventCount` for completions. The buffers belong to the driver from
//! submission until the packet comes back.
//!
//! The driver checks every packet before it touches a block ([`shape`],
//! [`admit`]). A packet with a record outside the data area, or whose first
//! record is not a header, comes back untouched, so its `status` stays what
//! the client left there: [`Status::Pending`] if the client zeroed it.

use {
    core::ptr,
    libio_channel::{Channel, DataArea, IoRec, MAX_PACKET_RECS, Packet},
};

/// Bytes of a block, the unit of every transfer.
pub const BLOCK_BYTES: u64 = 512;

/// Slots of each FIFO.
pub const SLOTS: usize = 32;

/// Most buffers one request carries: the records after its header.
pub const MAX_BUFFERS: usize = MAX_PACKET_RECS - 1;

/// The channel in its control page.
#[repr(C)]
pub struct BlockChannel {
    /// Requests from the client; the same packets back from the driver.
    pub requests: Channel<SLOTS>,
}

const _: () = assert!(size_of::<BlockChannel>() <= 4096);

impl BlockChannel {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            requests: Channel::new(),
        }
    }

    /// The channel in the control page at `address`.
    ///
    /// # Safety
    ///
    /// `address` is the start of a page mapped read/write for as long as the
    /// channel is used, which holds a channel or zeroes.
    #[must_use]
    pub unsafe fn at<'page>(address: u64) -> &'page Self {
        // SAFETY: forwarded caller contract; every field is atomic, so the
        // peer's concurrent access is sound.
        unsafe { &*(address as *const Self) }
    }
}

impl Default for BlockChannel {
    fn default() -> Self {
        Self::new()
    }
}

/// What a request asks for.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Read = 1,
    Write = 2,
}

impl TryFrom<u32> for Op {
    type Error = Status;

    fn try_from(raw: u32) -> Result<Self, Status> {
        match raw {
            1 => Ok(Self::Read),
            2 => Ok(Self::Write),
            _ => Err(Status::Invalid),
        }
    }
}

/// The outcome the driver writes in a request's header.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Not answered: what a zeroed header holds
    Pending = 0,
    /// Every block was transferred
    Done = 1,
    /// The blocks run past the end of the device
    OutOfRange = 2,
    /// The packet is not a request: an unknown op, a header or buffer of the
    /// wrong size or alignment, or no buffer
    Invalid = 3,
    /// The device failed the transfer; buffers of a read hold unspecified bytes
    DeviceFailed = 4,
    /// No medium is in the device
    NoMedium = 5,
}

impl Status {
    /// The status `raw` names, if any.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Option<Self> {
        Some(match raw {
            0 => Self::Pending,
            1 => Self::Done,
            2 => Self::OutOfRange,
            3 => Self::Invalid,
            4 => Self::DeviceFailed,
            5 => Self::NoMedium,
            _ => return None,
        })
    }
}

/// A request's header, in the data area.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// [`Op`], as its number
    pub op: u32,
    /// [`Status`], as its number: the client's until the driver answers
    pub status: u32,
    /// The first block the request reads or writes
    pub block: u64,
}

/// Bytes of a [`Header`]; its record is exactly this long.
pub const HEADER_BYTES: u64 = size_of::<Header>() as u64;

impl Header {
    /// The header at `address`, read once.
    ///
    /// # Safety
    ///
    /// `address` is [`HEADER_BYTES`] of readable memory, aligned to 8.
    #[must_use]
    pub unsafe fn read(address: u64) -> Self {
        // SAFETY: forwarded caller contract. The client may change the
        // header meanwhile; the copy is what the driver acts on.
        unsafe { ptr::read_volatile(address as *const Self) }
    }

    /// Write `status` into the header at `address`.
    ///
    /// # Safety
    ///
    /// `address` is [`HEADER_BYTES`] of writable memory, aligned to 8.
    pub unsafe fn write_status(address: u64, status: Status) {
        // SAFETY: forwarded caller contract; `status` is the header's second
        // word.
        unsafe { ptr::write_volatile((address as *mut u32).add(1), status as u32) }
    }
}

/// A request for `buffers` with its header at `header`, tagged `value`; `None`
/// with no buffer or more than [`MAX_BUFFERS`].
#[must_use]
pub fn request(value: u64, header: u64, buffers: &[IoRec]) -> Option<Packet> {
    if buffers.is_empty() || buffers.len() > MAX_BUFFERS {
        return None;
    }
    let mut recs = [IoRec::default(); MAX_PACKET_RECS];
    recs[0] = IoRec {
        base: header,
        len: HEADER_BYTES,
    };
    recs[1..=buffers.len()].copy_from_slice(buffers);
    Packet::new(value, &recs[..=buffers.len()])
}

/// Why the driver turns a packet down before it touches a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The packet has nowhere to write a status: a record lies outside the
    /// data area, or the first record is not [`HEADER_BYTES`] aligned to 8.
    /// It comes back untouched.
    Untouched,
    /// The packet comes back with this status in its header.
    Status(Status),
}

/// The header's address and the blocks the buffers hold, if `packet` is
/// shaped as a request in `area`: every record inside it, a first record of
/// [`HEADER_BYTES`] aligned to 8, then buffers of whole blocks aligned to 4
/// (the driver moves them a word at a time).
///
/// # Errors
///
/// [`Refusal::Untouched`] when a record lies outside `area` or the header
/// record is misshapen; `Refusal::Status(Status::Invalid)` for a packet without
/// buffers or with a misshapen one.
pub fn shape(packet: &Packet, area: &DataArea) -> Result<(u64, u64), Refusal> {
    if !area.holds(packet) {
        return Err(Refusal::Untouched);
    }
    let Some((header, buffers)) = packet.recs().split_first() else {
        return Err(Refusal::Untouched);
    };
    if header.len != HEADER_BYTES || !header.base.is_multiple_of(8) {
        return Err(Refusal::Untouched);
    }
    let invalid = Refusal::Status(Status::Invalid);
    if buffers.is_empty() {
        return Err(invalid);
    }
    let mut blocks: u64 = 0;
    for buffer in buffers {
        if buffer.len == 0
            || !buffer.len.is_multiple_of(BLOCK_BYTES)
            || !buffer.base.is_multiple_of(4)
        {
            return Err(invalid);
        }
        blocks += buffer.len / BLOCK_BYTES;
    }
    Ok((header.base, blocks))
}

/// The op of a request for `blocks` blocks with `header`, on a device of
/// `capacity` blocks.
///
/// # Errors
///
/// [`Status::Invalid`] for an unknown op; [`Status::OutOfRange`] when the
/// blocks run past the device's end.
pub fn admit(header: &Header, blocks: u64, capacity: u64) -> Result<Op, Status> {
    let op = Op::try_from(header.op)?;
    match header.block.checked_add(blocks) {
        Some(end) if end <= capacity => Ok(op),
        _ => Err(Status::OutOfRange),
    }
}
