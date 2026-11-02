//! The PCM stream of `audio_v1`: the io-channel that carries a client's
//! signed 16-bit stereo frames, their conversion into the PWM's FIFO words,
//! the periods of the DMA ring and which of them to refill, and the underrun
//! count (decisions D6 and D7 of `doc/agentic/audio-design.md`).
//!
//! A client frame is four bytes: the left sample, then the right one, each
//! signed 16-bit little-endian. A FIFO frame is two words, right first: the
//! PWM's channel 1 (GPIO40) is the jack's right side on the Raspberry Pi 3
//! (see the crate documentation).

use {
    crate::word,
    libio_channel::{Channel, DataArea, Packet},
};

/// Slots of each FIFO of the channel: 64 slots hold 32 packets of one
/// buffer each.
pub const CHANNEL_SLOTS: usize = 64;

/// The control page of an `audio_v1` io-channel: the client submits packets
/// of sample buffers on `submit` and the driver hands them back on
/// `complete`.
pub type AudioChannel = Channel<CHANNEL_SLOTS>;

const _: () = assert!(size_of::<AudioChannel>() <= 4096, "one control page");

/// The channel whose control page is at `address`.
///
/// # Safety
///
/// `address` must be the start of a channel control page mapped read/write
/// for `'page`, shared with the peer only.
pub unsafe fn channel_at<'page>(address: u64) -> &'page AudioChannel {
    let address = usize::try_from(address).unwrap_or(usize::MAX);
    // SAFETY: forwarded caller contract; every field is atomic, so the
    // peer's concurrent access is sound.
    unsafe { &*core::ptr::with_exposed_provenance::<AudioChannel>(address) }
}

/// Bytes of one client frame: two signed 16-bit samples.
pub const FRAME_BYTES: u64 = 4;

/// The client frame `[left, right]` as it lies in memory, read as one
/// little-endian word.
#[must_use]
pub const fn encode_frame(frame: [i16; 2]) -> u32 {
    (frame[0].cast_unsigned() as u32) | ((frame[1].cast_unsigned() as u32) << 16)
}

/// The client frame `[left, right]` in the little-endian word `raw`.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "each half of the word is one sample"
)]
pub const fn decode_frame(raw: u32) -> [i16; 2] {
    [
        (raw as u16).cast_signed(),
        ((raw >> 16) as u16).cast_signed(),
    ]
}

/// The FIFO words of the client frame `[left, right]`: right first.
#[must_use]
pub fn fifo_frame(frame: [i16; 2], range: u32) -> [u32; 2] {
    [word(frame[1], range), word(frame[0], range)]
}

/// The FIFO words of a silent frame: the middle of the range on both sides.
#[must_use]
pub fn silent_frame(range: u32) -> [u32; 2] {
    let silence = word(0, range);
    [silence, silence]
}

/// Whether the driver plays `packet`: every record lies in the channel's
/// `data` pages, starts at a 4-byte boundary and holds whole frames.
#[must_use]
pub fn admits(packet: &Packet, data: &DataArea) -> bool {
    data.holds(packet)
        && packet
            .recs()
            .iter()
            .all(|rec| rec.base.is_multiple_of(FRAME_BYTES) && rec.len.is_multiple_of(FRAME_BYTES))
}

/// Cut `packet` to the bytes taken: the records before `rec` whole, record
/// `rec` to `offset` bytes, the ones after it empty.
fn cut(packet: &mut Packet, rec: usize, offset: u64) {
    for (index, record) in packet.recs_mut().iter_mut().enumerate() {
        if index == rec {
            record.len = offset.min(record.len);
        } else if index > rec {
            record.len = 0;
        }
    }
}

/// The driver's side of a client's channel, as the stream sees it.
pub trait Queue {
    /// The oldest packet the client submitted, if one waits.
    fn next(&mut self) -> Option<Packet>;

    /// Hand `packet` back to the client.
    fn complete(&mut self, packet: &Packet);

    /// The client frame at `address`, a 4-byte aligned address of the
    /// channel's data pages, as one little-endian word.
    fn read_frame(&mut self, address: u64) -> u32;
}

/// What filled one period.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Fill {
    /// Client frames, at the start of the period
    pub client_frames: usize,
    /// Silent frames after them, for lack of client frames
    pub silent_frames: usize,
}

/// A packet being played and how far: the record and the byte in it.
#[derive(Debug, Clone, Copy)]
struct Playing {
    packet: Packet,
    rec: usize,
    offset: u64,
}

/// One open output's stream: the packet being played, the frames taken and
/// the underruns.
#[derive(Debug, Clone, Copy)]
pub struct Stream {
    range: u32,
    playing: Option<Playing>,
    /// A packet came since the stream opened: underruns count from then
    started: bool,
    underruns: u32,
    frames: u64,
    /// The FIFO words of the last frame written
    last: [u32; 2],
}

impl Stream {
    /// A stream of FIFO words of range `range`, opened after a ramp up to
    /// silence.
    #[must_use]
    pub fn new(range: u32) -> Self {
        Self {
            range,
            playing: None,
            started: false,
            underruns: 0,
            frames: 0,
            last: silent_frame(range),
        }
    }

    /// Periods filled up with silence since the first packet came.
    #[must_use]
    pub const fn underruns(&self) -> u32 {
        self.underruns
    }

    /// Client frames taken since the stream opened.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.frames
    }

    /// The FIFO words of the last frame written, where a ramp down starts.
    #[must_use]
    pub const fn last(&self) -> [u32; 2] {
        self.last
    }

    /// Hand the packet being played back if every byte of it was taken.
    fn retire_finished(&mut self, queue: &mut impl Queue) {
        let Some(playing) = self.playing.as_mut() else {
            return;
        };
        let recs = playing.packet.recs();
        while playing.rec < recs.len() && playing.offset >= recs[playing.rec].len {
            playing.rec += 1;
            playing.offset = 0;
        }
        if playing.rec == recs.len() {
            queue.complete(&playing.packet);
            self.playing = None;
        }
    }

    /// Fill `period` with the client's frames from `queue`, in the order
    /// they were submitted, and silence after them if too few are queued.
    /// Hands back every packet whose last frame went in, and every packet
    /// the driver refuses (see [`admits`]) cut to nothing. A period with
    /// silence counts as an underrun once a packet has come.
    pub fn fill(
        &mut self,
        period: &mut [[u32; 2]],
        data: &DataArea,
        queue: &mut impl Queue,
    ) -> Fill {
        let fill = self.take(period, data, queue);
        if fill.silent_frames != 0 && self.started {
            self.underruns = self.underruns.saturating_add(1);
        }
        fill
    }

    /// As [`Self::fill`] while the output closes: the client sent its last
    /// packet, so running out of frames is the end of the stream, not an
    /// underrun.
    pub fn drain(
        &mut self,
        period: &mut [[u32; 2]],
        data: &DataArea,
        queue: &mut impl Queue,
    ) -> Fill {
        self.take(period, data, queue)
    }

    /// Fill `period` from `queue`, silence after the frames it holds.
    fn take(&mut self, period: &mut [[u32; 2]], data: &DataArea, queue: &mut impl Queue) -> Fill {
        let mut written = 0;
        while written < period.len() {
            self.retire_finished(queue);
            if self.playing.is_none() {
                let Some(mut packet) = queue.next() else {
                    break;
                };
                if !admits(&packet, data) {
                    cut(&mut packet, 0, 0);
                    queue.complete(&packet);
                    continue;
                }
                self.started = true;
                self.playing = Some(Playing {
                    packet,
                    rec: 0,
                    offset: 0,
                });
            }
            // A packet just taken may start with empty records: the next
            // round retires them.
            let Some(playing) = self.playing.as_mut() else {
                continue;
            };
            let Some(rec) = playing.packet.recs().get(playing.rec).copied() else {
                continue;
            };
            if rec.len <= playing.offset {
                continue;
            }
            let available = (rec.len - playing.offset) / FRAME_BYTES;
            let take = usize::try_from(available)
                .unwrap_or(usize::MAX)
                .min(period.len() - written);
            for frame in &mut period[written..written + take] {
                let raw = queue.read_frame(rec.base + playing.offset);
                *frame = fifo_frame(decode_frame(raw), self.range);
                playing.offset += FRAME_BYTES;
            }
            written += take;
        }
        self.retire_finished(queue);
        let silence = silent_frame(self.range);
        period[written..].fill(silence);
        self.frames += written as u64;
        if let Some(&last) = period.last() {
            self.last = last;
        }
        Fill {
            client_frames: written,
            silent_frames: period.len() - written,
        }
    }

    /// Hand back the packet being played, cut to the bytes taken, and every
    /// packet still queued, cut to nothing: the output stops before they
    /// play. Returns how many came back.
    pub fn finish(&mut self, queue: &mut impl Queue) -> usize {
        let mut returned = 0;
        if let Some(mut playing) = self.playing.take() {
            cut(&mut playing.packet, playing.rec, playing.offset);
            queue.complete(&playing.packet);
            returned += 1;
        }
        while let Some(mut packet) = queue.next() {
            cut(&mut packet, 0, 0);
            queue.complete(&packet);
            returned += 1;
        }
        returned
    }
}

/// Fill `period` with a ramp from no output up to silence, both sides: the
/// first period after the channels start, which turns the step from low to
/// the mid level into a slope.
pub fn ramp_up(period: &mut [[u32; 2]], range: u32) {
    let silence = u64::from(word(0, range));
    let frames = period.len() as u64;
    for (index, frame) in (1..).zip(period.iter_mut()) {
        let level = u32::try_from(silence * index / frames).unwrap_or(u32::MAX);
        *frame = [level, level];
    }
}

/// Fill `period` with a ramp from `from`, the FIFO words of the last frame
/// played, down to no output, each side on its own: the last period before
/// the channels stop.
pub fn ramp_down(period: &mut [[u32; 2]], from: [u32; 2]) {
    let frames = period.len() as u64;
    for (index, frame) in (1..).zip(period.iter_mut()) {
        let remaining = frames - index;
        *frame = from
            .map(|side| u32::try_from(u64::from(side) * remaining / frames).unwrap_or(u32::MAX));
    }
}

/// The periods of the DMA ring the driver has refilled and the DMA has
/// played.
///
/// Every slot of the ring is filled before the DMA starts, with periods 0 to
/// `periods` - 1. When the DMA reports more periods finished, the slots it
/// left take the periods after the ones already filled, so the ring always
/// holds the period playing and the `periods` - 1 after it. If the DMA came
/// round to a slot before it was refilled, it played that slot's old period
/// again: those periods are late.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ring {
    periods: u64,
    finished: u64,
    filled: u64,
}

/// What [`Ring::advance`] asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refill {
    /// The first period to write
    pub first: u64,
    /// How many periods to write, from `first`
    pub count: u64,
    /// Periods the DMA played from a slot not refilled in time
    pub late: u64,
}

impl Ring {
    /// A ring of `periods` slots, all filled.
    #[must_use]
    pub const fn new(periods: u64) -> Self {
        Self {
            periods,
            finished: 0,
            filled: periods,
        }
    }

    /// The slot period `period` is written to.
    #[must_use]
    pub fn slot(&self, period: u64) -> usize {
        usize::try_from(period % self.periods).unwrap_or_default()
    }

    /// Periods the DMA has finished.
    #[must_use]
    pub const fn finished(&self) -> u64 {
        self.finished
    }

    /// Whether the DMA has finished `period`.
    #[must_use]
    pub const fn has_played(&self, period: u64) -> bool {
        self.finished > period
    }

    /// The DMA has finished `finished` periods in all: the periods to write
    /// into the slots it left. Never the slot of the period playing now.
    pub fn advance(&mut self, finished: u64) -> Refill {
        let finished = finished.max(self.finished);
        let behind = finished - self.finished;
        let late = behind.saturating_sub(self.periods - 1);
        let first = self.filled.max(finished + 1);
        let end = finished + self.periods;
        self.finished = finished;
        self.filled = end;
        Refill {
            first,
            count: end - first,
            late,
        }
    }
}

/// The frame the capture test's client sends at position `frame` of its
/// stream, `[left, right]`: the two sides differ, so a swap shows, and
/// neighbouring frames differ, so a frame out of order shows. Over 65 536
/// frames the left side takes every value once.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "the pattern repeats every 65 536 frames"
)]
pub const fn test_pattern(frame: u64) -> [i16; 2] {
    let left = (frame as u16).wrapping_mul(0x9e37).cast_signed();
    [left, !left]
}

/// What the capture test expects of a period of the stream: `client_frames`
/// frames of [`test_pattern`] from position `first_frame`, then silence.
pub fn expected_stream_period(
    period: &mut [[u32; 2]],
    first_frame: u64,
    client_frames: usize,
    range: u32,
) {
    let client_frames = client_frames.min(period.len());
    for (position, frame) in (first_frame..).zip(period[..client_frames].iter_mut()) {
        *frame = fifo_frame(test_pattern(position), range);
    }
    period[client_frames..].fill(silent_frame(range));
}

/// Frames of [`test_pattern`] a stream starts with to ask for the capture
/// check against the pattern.
pub const PATTERN_SIGNATURE_FRAMES: usize = 16;

/// Whether `period`, the stream's first with client frames (`client_frames`
/// of them, from stream position 0), starts with [`PATTERN_SIGNATURE_FRAMES`]
/// frames of [`test_pattern`] converted: the capture test's client asks for
/// the pattern check this way. Any other stream (silence, a game's mixed
/// sound) is checked only as the driver filled it.
#[must_use]
pub fn starts_with_test_pattern(period: &[[u32; 2]], client_frames: usize, range: u32) -> bool {
    client_frames >= PATTERN_SIGNATURE_FRAMES
        && period.len() >= PATTERN_SIGNATURE_FRAMES
        && (0..PATTERN_SIGNATURE_FRAMES)
            .all(|position| period[position] == fifo_frame(test_pattern(position as u64), range))
}

/// Whether any of `frames` is not silent.
#[must_use]
pub fn is_audible(frames: &[[u32; 2]], range: u32) -> bool {
    let silence = silent_frame(range);
    frames.iter().any(|&frame| frame != silence)
}
