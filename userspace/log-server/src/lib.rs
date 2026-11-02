#![no_std]

//! What the log server and its clients agree on (`interfaces/log_v1.if`), and
//! the pure parts of both sides, host-tested in `tests/protocol.rs`:
//!
//! - the channel's layout: [`CHANNEL_PAGES`] data pages cut into
//!   [`BUFFERS`] buffers of [`LINE_BYTES`], one line each;
//! - the packet header in the value word ([`Header`]);
//! - [`FreeBuffers`], the client's set of buffers it may write;
//! - [`LineWriter`], which formats a line into a buffer and cuts what does not
//!   fit;
//! - [`format_line`], the server's output line: timestamp, level, the
//!   client's drop count, the text with control bytes replaced, and `\r\n`;
//! - [`report::FaultReport`], the `FAULT:` line a client's fault handler
//!   logs.

pub mod report;

use core::fmt;

use libio_channel::{Channel, IoRec};

/// Slots of each FIFO of the channel: a packet takes two (header and one
/// record), so every buffer can be in flight at once.
pub const CHANNEL_SLOTS: usize = 2 * BUFFERS;

/// The control page of a `log_v1` io-channel.
pub type LogChannel = Channel<CHANNEL_SLOTS>;

const _: () = assert!(size_of::<LogChannel>() <= 4096, "one control page");

/// Data pages of the channel, as `log_v1.if` declares (`channel 2;`).
pub const CHANNEL_PAGES: u64 = 2;

/// Bytes of one line's buffer: the longest line a client sends, without its
/// line end.
pub const LINE_BYTES: u64 = LINE_LEN as u64;

/// [`LINE_BYTES`] as a length.
pub const LINE_LEN: usize = 256;

/// Buffers in the data pages.
pub const BUFFERS: usize = (CHANNEL_PAGES * 4096 / LINE_BYTES) as usize;

const _: () = assert!(BUFFERS <= 32, "the free set is one u32");

/// The channel whose control page is at `address`.
///
/// # Safety
///
/// `address` must be the start of a channel control page mapped read/write
/// for `'page`.
pub unsafe fn channel_at<'page>(address: u64) -> &'page LogChannel {
    let address = usize::try_from(address).unwrap_or(usize::MAX);
    // SAFETY: forwarded caller contract; every field is atomic, so the peer's
    // concurrent access is sound.
    unsafe { &*core::ptr::with_exposed_provenance::<LogChannel>(address) }
}

/// A line's level, numbered as the `log` crate's (`Error` is 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Level {
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

impl Level {
    /// The level numbered `value`, if any.
    pub const fn from_number(value: u64) -> Option<Self> {
        match value {
            1 => Some(Self::Error),
            2 => Some(Self::Warn),
            3 => Some(Self::Info),
            4 => Some(Self::Debug),
            5 => Some(Self::Trace),
            _ => None,
        }
    }

    /// The letter the server writes for it.
    pub const fn letter(self) -> u8 {
        match self {
            Self::Error => b'E',
            Self::Warn => b'W',
            Self::Info => b'I',
            Self::Debug => b'D',
            Self::Trace => b'T',
        }
    }
}

/// The header a packet's value word carries.
///
/// | Bits | Field |
/// |---|---|
/// | 0..3 | level (0: none) |
/// | 3 | the line was cut |
/// | 8..24 | lines the client dropped before this one (saturating) |
/// | 24..64 | the client's clock when it wrote the line, in microseconds |
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub level: Option<Level>,
    pub cut: bool,
    pub dropped: u16,
    /// Microseconds since boot, modulo 2^40 (about 12.7 days)
    pub micros: u64,
}

const LEVEL_MASK: u64 = 0b111;
const CUT_BIT: u64 = 1 << 3;
const DROPPED_SHIFT: u32 = 8;
const MICROS_SHIFT: u32 = 24;
const MICROS_MASK: u64 = (1 << (64 - MICROS_SHIFT)) - 1;

impl Header {
    /// The value word.
    pub const fn encode(self) -> u64 {
        let level = match self.level {
            Some(level) => level as u64,
            None => 0,
        };
        let cut = if self.cut { CUT_BIT } else { 0 };
        level
            | cut
            | ((self.dropped as u64) << DROPPED_SHIFT)
            | ((self.micros & MICROS_MASK) << MICROS_SHIFT)
    }

    /// The header in `value`; an unknown level reads as none.
    pub const fn decode(value: u64) -> Self {
        Self {
            level: Level::from_number(value & LEVEL_MASK),
            cut: value & CUT_BIT != 0,
            dropped: ((value >> DROPPED_SHIFT) & 0xffff) as u16,
            micros: value >> MICROS_SHIFT,
        }
    }
}

/// The record of buffer `index` in the data pages at `data_base`, `len`
/// bytes long.
pub const fn buffer_record(data_base: u64, index: u32, len: u64) -> IoRec {
    IoRec {
        base: data_base + index as u64 * LINE_BYTES,
        len,
    }
}

/// The buffer a record returned by the server names, if it is one.
pub const fn buffer_of(data_base: u64, record: IoRec) -> Option<u32> {
    let Some(offset) = record.base.checked_sub(data_base) else {
        return None;
    };
    if !offset.is_multiple_of(LINE_BYTES) || offset / LINE_BYTES >= BUFFERS as u64 {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "below BUFFERS, checked above"
    )]
    Some((offset / LINE_BYTES) as u32)
}

/// The buffers a client may write: those not in flight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreeBuffers(u32);

impl FreeBuffers {
    /// Every buffer free.
    pub const fn all() -> Self {
        Self(if BUFFERS == 32 {
            u32::MAX
        } else {
            (1 << BUFFERS) - 1
        })
    }

    /// Take the lowest free buffer.
    pub const fn take(&mut self) -> Option<u32> {
        if self.0 == 0 {
            return None;
        }
        let index = self.0.trailing_zeros();
        self.0 &= !(1 << index);
        Some(index)
    }

    /// Give buffer `index` back; one the client did not hold is ignored.
    pub const fn give(&mut self, index: u32) {
        if (index as usize) < BUFFERS {
            self.0 |= 1 << index;
        }
    }

    /// How many are free.
    pub const fn count(self) -> u32 {
        self.0.count_ones()
    }
}

/// Formats into a byte buffer, keeping what fits and noting the rest was cut.
pub struct LineWriter<'buffer> {
    buffer: &'buffer mut [u8],
    len: usize,
    cut: bool,
}

impl<'buffer> LineWriter<'buffer> {
    pub const fn new(buffer: &'buffer mut [u8]) -> Self {
        Self {
            buffer,
            len: 0,
            cut: false,
        }
    }

    /// The bytes written, without trailing line ends, and whether some did
    /// not fit.
    pub fn finish(self) -> (usize, bool) {
        let mut len = self.len;
        while len > 0 && matches!(self.buffer[len - 1], b'\n' | b'\r') {
            len -= 1;
        }
        (len, self.cut)
    }
}

impl fmt::Write for LineWriter<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let room = self.buffer.len() - self.len;
        let mut taken = text.len().min(room);
        // Cut at a character boundary, so the line stays UTF-8.
        while !text.is_char_boundary(taken) {
            taken -= 1;
        }
        self.buffer[self.len..self.len + taken].copy_from_slice(&text.as_bytes()[..taken]);
        self.len += taken;
        if taken < text.len() {
            self.cut = true;
        }
        Ok(())
    }
}

/// Bytes [`format_line`] may need besides the text: the prefix, the drop
/// note, the cut mark and the line end.
pub const PREFIX_BYTES: usize = 64;

/// The longest output line [`format_line`] writes.
pub const OUTPUT_LINE_BYTES: usize = LINE_LEN + PREFIX_BYTES;

/// Write the output line for `text` with `header` into `out`:
///
/// `[    12.345678] I (3 dropped) text…\r\n`
///
/// The level letter is `-` for a line without one; the drop note appears when
/// the client dropped lines before this one, the `…` when the line was cut.
/// Control bytes of `text` become `?`, so a client cannot move the terminal's
/// cursor or start a line that looks like another client's. Returns the
/// bytes written: all of it when `out` holds [`OUTPUT_LINE_BYTES`] and `text`
/// at most [`LINE_BYTES`].
pub fn format_line(header: Header, text: &[u8], out: &mut [u8]) -> usize {
    use fmt::Write;
    let mut writer = LineWriter::new(out);
    let letter = header.level.map_or('-', |level| char::from(level.letter()));
    // Writing to a `LineWriter` cannot fail.
    let _prefix = write!(
        writer,
        "[{:6}.{:06}] {letter} ",
        header.micros / 1_000_000,
        header.micros % 1_000_000
    );
    if header.dropped != 0 {
        let _note = write!(writer, "({} dropped) ", header.dropped);
    }
    let LineWriter {
        buffer, mut len, ..
    } = writer;
    for &byte in text {
        if len == buffer.len() {
            break;
        }
        buffer[len] = if byte < 0x20 || byte == 0x7f {
            b'?'
        } else {
            byte
        };
        len += 1;
    }
    let tail: &[u8] = if header.cut {
        "…\r\n".as_bytes()
    } else {
        b"\r\n"
    };
    if buffer.len() < tail.len() {
        return len;
    }
    if len + tail.len() > buffer.len() {
        len = buffer.len().saturating_sub(tail.len());
    }
    buffer[len..len + tail.len()].copy_from_slice(tail);
    len + tail.len()
}
