//! Standard MIDI File reader (format 0 and 1, ticks per quarter note).
//!
//! The tracks are merged into one list of events in tick order (ties keep the
//! track order), with the tempo changes among them. System exclusive and
//! other meta events are skipped.

use alloc::vec::Vec;

/// Why a file is not a playable Standard MIDI File.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmfError {
    /// No `MThd` header, or an unknown format.
    NotMidi,
    /// SMPTE time division (frames per second) is not supported.
    SmpteDivision,
    /// A chunk or event runs past the end of the data.
    Truncated,
    /// A data byte where a status byte was needed, with no running status.
    NoRunningStatus,
}

/// What happens at an event's tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventKind {
    /// A channel message: status and up to two data bytes.
    Channel {
        /// Status byte (message and channel).
        status: u8,
        /// First data byte.
        data1: u8,
        /// Second data byte (0 for one-byte messages).
        data2: u8,
    },
    /// New tempo in microseconds per quarter note.
    Tempo(u32),
}

/// An event at a tick from the start of the song.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Event {
    /// Ticks from the start.
    pub tick: u32,
    /// What happens.
    pub kind: EventKind,
}

/// A song: its events in order, ticks per quarter note and its length.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Song {
    /// Events in tick order.
    pub events: Vec<Event>,
    /// Ticks per quarter note.
    pub division: u16,
    /// The tick of the last track's end (or last event), where a loop
    /// restarts.
    pub end_tick: u32,
}

/// The default tempo, 120 beats per minute.
pub const DEFAULT_TEMPO: u32 = 500_000;

/// A reader over bytes with an error on running out.
struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, SmfError> {
        let byte = *self.bytes.get(self.position).ok_or(SmfError::Truncated)?;
        self.position += 1;
        Ok(byte)
    }

    fn skip(&mut self, count: usize) -> Result<(), SmfError> {
        if self.bytes.len() - self.position < count {
            return Err(SmfError::Truncated);
        }
        self.position += count;
        Ok(())
    }

    /// A variable-length quantity (at most four bytes).
    fn variable(&mut self) -> Result<u32, SmfError> {
        let mut value = 0_u32;
        for _ in 0..4 {
            let byte = self.byte()?;
            value = (value << 7) | u32::from(byte & 0x7f);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Ok(value)
    }

    fn is_empty(&self) -> bool {
        self.position >= self.bytes.len()
    }
}

/// The `count` bytes (at most 4) at `offset` as a big-endian number; the
/// caller has checked the length.
fn big_endian(bytes: &[u8], offset: usize, count: usize) -> u32 {
    bytes[offset..offset + count]
        .iter()
        .fold(0, |value, &byte| (value << 8) | u32::from(byte))
}

/// Number of data bytes of a channel message with `status`.
const fn data_length(status: u8) -> usize {
    match status & 0xf0 {
        0xc0 | 0xd0 => 1,
        _ => 2,
    }
}

/// Read one track's events into `events`; returns the track's end tick.
fn read_track(track: &[u8], events: &mut Vec<Event>) -> Result<u32, SmfError> {
    let mut reader = Reader {
        bytes: track,
        position: 0,
    };
    let mut tick = 0_u32;
    let mut running_status: Option<u8> = None;
    while !reader.is_empty() {
        tick = tick.saturating_add(reader.variable()?);
        let first = reader.byte()?;
        match first {
            0xff => {
                let kind = reader.byte()?;
                let length = usize::try_from(reader.variable()?).unwrap_or(usize::MAX);
                if kind == 0x2f {
                    return Ok(tick);
                }
                if kind == 0x51 && length == 3 {
                    let tempo = (u32::from(reader.byte()?) << 16)
                        | (u32::from(reader.byte()?) << 8)
                        | u32::from(reader.byte()?);
                    events.push(Event {
                        tick,
                        kind: EventKind::Tempo(tempo.max(1)),
                    });
                } else {
                    reader.skip(length)?;
                }
            }
            0xf0 | 0xf7 => {
                let length = usize::try_from(reader.variable()?).unwrap_or(usize::MAX);
                reader.skip(length)?;
            }
            _ => {
                let (status, data1) = if first & 0x80 == 0 {
                    (running_status.ok_or(SmfError::NoRunningStatus)?, first)
                } else {
                    (first, reader.byte()?)
                };
                running_status = Some(status);
                let data2 = if data_length(status) == 2 {
                    reader.byte()?
                } else {
                    0
                };
                events.push(Event {
                    tick,
                    kind: EventKind::Channel {
                        status,
                        data1,
                        data2,
                    },
                });
            }
        }
    }
    Ok(tick)
}

impl Song {
    /// Read the Standard MIDI File `bytes`.
    ///
    /// # Errors
    ///
    /// [`SmfError`] when the header or a track is malformed.
    pub fn parse(bytes: &[u8]) -> Result<Self, SmfError> {
        if bytes.len() < 14 || big_endian(bytes, 0, 4) != u32::from_be_bytes(*b"MThd") {
            return Err(SmfError::NotMidi);
        }
        let header_length = usize::try_from(big_endian(bytes, 4, 4))
            .ok()
            .ok_or(SmfError::Truncated)?;
        let format = big_endian(bytes, 8, 2);
        let division = u16::try_from(big_endian(bytes, 12, 2)).unwrap_or(0);
        if format > 1 || header_length < 6 {
            return Err(SmfError::NotMidi);
        }
        if division & 0x8000 != 0 {
            return Err(SmfError::SmpteDivision);
        }
        let mut events = Vec::new();
        let mut end_tick = 0;
        let mut rest = bytes.get(8 + header_length..).ok_or(SmfError::Truncated)?;
        while rest.len() >= 8 {
            let length = usize::try_from(big_endian(rest, 4, 4))
                .ok()
                .ok_or(SmfError::Truncated)?;
            let body = rest.get(8..8 + length).ok_or(SmfError::Truncated)?;
            if big_endian(rest, 0, 4) == u32::from_be_bytes(*b"MTrk") {
                end_tick = end_tick.max(read_track(body, &mut events)?);
            }
            rest = &rest[8 + length..];
        }
        // Stable: events of one tick keep their track and file order.
        events.sort_by_key(|event| event.tick);
        let last = events.last().map_or(0, |event| event.tick);
        Ok(Self {
            events,
            division: division.max(1),
            end_tick: end_tick.max(last),
        })
    }
}
