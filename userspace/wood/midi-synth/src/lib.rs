#![no_std]

//! The pure part of the MIDI synthesiser component (`midi_synth_v1`, see
//! `interfaces/midi_synth_v1.if` and `doc/agentic/audio-design.md`, decision
//! D10): the packet protocol both sides share, the layout of the io-channel's
//! data pages, and the [`Session`] that handles a client's packets with
//! `wood-synth`. The component (`src/component.rs`, Vesper only) adds the
//! exports, the heap and the mapping of the data pages; host tests drive the
//! same [`Session`] over memory of their own.

extern crate alloc;

use {
    alloc::vec::Vec,
    libio_channel::{Channel, DataArea, IoRec, Packet},
    wood_synth::{Bank, OUTPUT_RATE, Sequencer, Song, Synth},
};

/// Slots of each of the channel's FIFOs.
pub const CHANNEL_SLOTS: usize = 64;

/// The control page of a `midi_synth_v1` io-channel.
pub type SynthChannel = Channel<CHANNEL_SLOTS>;

const _: () = assert!(size_of::<SynthChannel>() <= 4096, "one control page");

/// The channel whose control page is at `address`.
///
/// # Safety
///
/// `address` must be the start of a channel control page mapped read/write
/// for `'page`.
pub unsafe fn channel_at<'page>(address: u64) -> &'page SynthChannel {
    let address = usize::try_from(address).unwrap_or(usize::MAX);
    // SAFETY: forwarded caller contract; every field is atomic, so the peer's
    // concurrent access is sound.
    unsafe { &*core::ptr::with_exposed_provenance::<SynthChannel>(address) }
}

/// The rate the synthesiser renders at, in millihertz (`rate`): the audio
/// driver's 100 MHz over 2268, as `wood_synth::OUTPUT_RATE`.
pub const RATE_MILLIHERTZ: u32 = 44_091_710;

/// Bytes of a rendered frame: two signed 16-bit samples.
pub const FRAME_BYTES: u64 = 4;

/// Frames of one period, as the audio driver's and Wood's buffers.
pub const PERIOD_FRAMES: usize = 512;

/// Bytes of one period.
pub const PERIOD_BYTES: u64 = PERIOD_FRAMES as u64 * FRAME_BYTES;

/// Periods the start of the data pages holds for render buffers.
pub const RENDER_BUFFERS: u64 = 4;

/// Bytes at the start of the data pages for render buffers; song bytes go
/// after them.
pub const RENDER_AREA_BYTES: u64 = RENDER_BUFFERS * PERIOD_BYTES;

/// A packet's command, the low byte of its value word.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    /// Bytes of a song: bit 8 starts one, bit 9 ends it, bit 10 loops it.
    Song,
    /// Buffers to fill with rendered frames.
    Render,
    /// End the song.
    Stop,
    /// Hold the song, render silence.
    Pause,
    /// Go on after a pause.
    Resume,
    /// Bits 8 to 15: the volume.
    Volume,
}

const COMMAND_SONG: u64 = 1;
const COMMAND_RENDER: u64 = 2;
const COMMAND_STOP: u64 = 3;
const COMMAND_PAUSE: u64 = 4;
const COMMAND_RESUME: u64 = 5;
const COMMAND_VOLUME: u64 = 6;

/// Song packet flag: the first bytes of a new song.
pub const SONG_FIRST: u64 = 1 << 8;
/// Song packet flag: the last bytes; the song plays.
pub const SONG_LAST: u64 = 1 << 9;
/// Song packet flag: the song loops.
pub const SONG_LOOPING: u64 = 1 << 10;

/// Where the client's own bits of a value word start.
pub const CLIENT_SHIFT: u32 = 16;

impl Command {
    /// The command of value word `value`.
    #[must_use]
    pub const fn of(value: u64) -> Option<Self> {
        match value & 0xff {
            COMMAND_SONG => Some(Self::Song),
            COMMAND_RENDER => Some(Self::Render),
            COMMAND_STOP => Some(Self::Stop),
            COMMAND_PAUSE => Some(Self::Pause),
            COMMAND_RESUME => Some(Self::Resume),
            COMMAND_VOLUME => Some(Self::Volume),
            _ => None,
        }
    }

    /// The value word of this command with `flags` (bits 8 to 15) and the
    /// client's `tag` (bits 16 and up).
    #[must_use]
    pub const fn value(self, flags: u64, tag: u64) -> u64 {
        let code = match self {
            Self::Song => COMMAND_SONG,
            Self::Render => COMMAND_RENDER,
            Self::Stop => COMMAND_STOP,
            Self::Pause => COMMAND_PAUSE,
            Self::Resume => COMMAND_RESUME,
            Self::Volume => COMMAND_VOLUME,
        };
        code | (flags & 0xff00) | (tag << CLIENT_SHIFT)
    }
}

/// The value word of a volume packet.
#[must_use]
pub const fn volume_value(volume: u8, tag: u64) -> u64 {
    let volume = if volume > 127 { 127 } else { volume };
    Command::Volume.value((volume as u64) << 8, tag)
}

/// The data pages as the synthesiser reads and writes them.
pub trait Memory {
    /// Append the bytes of `record` to `out`.
    fn read(&self, record: IoRec, out: &mut Vec<u8>);
    /// Store `bytes` at `address`.
    fn write(&mut self, address: u64, bytes: &[u8]);
}

/// 64-bit FNV-1a, folded over the bytes the synthesiser renders: the hash
/// the component logs and host tests compute for the same song.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fnv(u64);

impl Fnv {
    /// The empty hash.
    pub const NEW: Self = Self(0xcbf2_9ce4_8422_2325);

    /// Fold in `bytes`.
    pub fn add(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }

    /// The hash so far.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// Frames after a song's start the [`Session`] hashes: 64 periods, 0.74 s.
pub const HASH_FRAMES: u64 = 64 * PERIOD_FRAMES as u64;

/// The hash of the first [`HASH_FRAMES`] rendered frames of a song.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StartHash {
    /// The song's bytes.
    pub song_bytes: usize,
    /// Whether it loops.
    pub looping: bool,
    /// The volume it started at.
    pub volume: u8,
    /// FNV-1a of the frames, little-endian.
    pub hash: u64,
    /// How many of the 64 periods were not silent.
    pub audible_periods: u32,
}

/// What [`Session::handle`] did with a packet.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Handled {
    /// Song bytes collected; `Some` with the song's size once it plays.
    Song(Option<usize>),
    /// The song's bytes are not a playable MIDI file: nothing plays.
    BadSong,
    /// Frames rendered into the buffers.
    Rendered(u64),
    /// Stop, pause, resume or volume done.
    Control,
    /// The packet was refused: a record outside the data pages, or no such
    /// command. Its records come back empty.
    Refused,
}

/// One client's synthesiser: the sequencer over the bank, the song being
/// collected, and the hash of the start of the song playing.
pub struct Session<'bank> {
    sequencer: Sequencer<'bank>,
    collecting: Vec<u8>,
    volume: u8,
    /// Frames rendered since the song started, while hashing.
    hashed_frames: u64,
    hash: Fnv,
    audible_periods: u32,
    period_audible: bool,
    started: Option<(usize, bool, u8)>,
    finished_hash: Option<StartHash>,
}

impl<'bank> Session<'bank> {
    /// A silent synthesiser over `bank` at the output rate, full volume.
    #[must_use]
    pub fn new(bank: Bank<'bank>) -> Self {
        Self {
            sequencer: Sequencer::new(Synth::new(bank, OUTPUT_RATE)),
            collecting: Vec::new(),
            volume: 127,
            hashed_frames: HASH_FRAMES,
            hash: Fnv::NEW,
            audible_periods: 0,
            period_audible: false,
            started: None,
            finished_hash: None,
        }
    }

    /// Back to silence at full volume, as a new client finds it: the song
    /// stops at once, the bytes collected are dropped.
    pub fn reset(&mut self) {
        self.sequencer.stop();
        self.sequencer.resume();
        self.sequencer.synth_mut().reset();
        self.collecting.clear();
        self.volume = 127;
        self.sequencer.set_volume(self.volume);
        self.started = None;
        self.finished_hash = None;
        self.hashed_frames = HASH_FRAMES;
    }

    /// Whether a song plays.
    #[must_use]
    pub const fn playing(&self) -> bool {
        self.sequencer.is_playing()
    }

    /// The hash of the start of the last song, once its first
    /// [`HASH_FRAMES`] frames were rendered; taken once.
    pub const fn take_start_hash(&mut self) -> Option<StartHash> {
        self.finished_hash.take()
    }

    /// Handle `packet`, whose records `area` must hold, reading and writing
    /// `memory`; cut its records to what was handled.
    pub fn handle(
        &mut self,
        packet: &mut Packet,
        area: DataArea,
        memory: &mut dyn Memory,
    ) -> Handled {
        let command = Command::of(packet.value());
        let handled = match command {
            _ if !area.holds(packet) => Handled::Refused,
            None => Handled::Refused,
            Some(Command::Song) => self.song(packet, memory),
            Some(Command::Render) => Handled::Rendered(self.render(packet, memory)),
            Some(Command::Stop) => {
                self.sequencer.stop();
                Handled::Control
            }
            Some(Command::Pause) => {
                self.sequencer.pause();
                Handled::Control
            }
            Some(Command::Resume) => {
                self.sequencer.resume();
                Handled::Control
            }
            Some(Command::Volume) => {
                self.volume = u8::try_from((packet.value() >> 8) & 0x7f).unwrap_or(127);
                self.sequencer.set_volume(self.volume);
                Handled::Control
            }
        };
        if handled == Handled::Refused {
            for record in packet.recs_mut() {
                record.len = 0;
            }
        }
        handled
    }

    fn song(&mut self, packet: &Packet, memory: &dyn Memory) -> Handled {
        let value = packet.value();
        if value & SONG_FIRST != 0 {
            self.collecting.clear();
        }
        for &record in packet.recs() {
            memory.read(record, &mut self.collecting);
        }
        if value & SONG_LAST == 0 {
            return Handled::Song(None);
        }
        let bytes = core::mem::take(&mut self.collecting);
        let Ok(song) = Song::parse(&bytes) else {
            self.sequencer.stop();
            return Handled::BadSong;
        };
        let looping = value & SONG_LOOPING != 0;
        self.sequencer.play(song, looping);
        self.sequencer.set_volume(self.volume);
        self.started = Some((bytes.len(), looping, self.volume));
        self.hashed_frames = 0;
        self.hash = Fnv::NEW;
        self.audible_periods = 0;
        self.period_audible = false;
        Handled::Song(Some(bytes.len()))
    }

    /// Fill every record of `packet` with rendered frames; the frames
    /// rendered.
    fn render(&mut self, packet: &mut Packet, memory: &mut dyn Memory) -> u64 {
        let mut block = [0_i16; 2 * 128];
        let mut bytes = [0_u8; 4 * 128];
        let mut total = 0;
        for record in packet.recs_mut() {
            let frames = record.len / FRAME_BYTES;
            record.len = frames * FRAME_BYTES;
            let mut done = 0;
            while done < frames {
                let count = usize::try_from((frames - done).min(128)).unwrap_or(128);
                let block = &mut block[..2 * count];
                self.sequencer.render(block);
                let bytes = &mut bytes[..4 * count];
                for (pair, sample) in bytes.as_chunks_mut::<2>().0.iter_mut().zip(block.iter()) {
                    *pair = sample.to_le_bytes();
                }
                memory.write(record.base + done * FRAME_BYTES, bytes);
                self.fold_into_hash(block, bytes);
                done += count as u64;
            }
            total += frames;
        }
        total
    }

    /// Fold rendered frames into the start hash while it is open.
    fn fold_into_hash(&mut self, block: &[i16], bytes: &[u8]) {
        let mut offset = 0;
        let frames = block.len() / 2;
        while offset < frames && self.hashed_frames < HASH_FRAMES {
            let period_left = PERIOD_FRAMES as u64 - self.hashed_frames % PERIOD_FRAMES as u64;
            let left = usize::try_from(period_left.min(HASH_FRAMES - self.hashed_frames))
                .unwrap_or(0)
                .min(frames - offset);
            self.hash.add(&bytes[4 * offset..4 * (offset + left)]);
            self.period_audible |= block[2 * offset..2 * (offset + left)]
                .iter()
                .any(|&sample| sample != 0);
            offset += left;
            self.hashed_frames += left as u64;
            if self.hashed_frames.is_multiple_of(PERIOD_FRAMES as u64) {
                self.audible_periods += u32::from(self.period_audible);
                self.period_audible = false;
            }
            if self.hashed_frames == HASH_FRAMES
                && let Some((song_bytes, looping, volume)) = self.started.take()
            {
                self.finished_hash = Some(StartHash {
                    song_bytes,
                    looping,
                    volume,
                    hash: self.hash.value(),
                    audible_periods: self.audible_periods,
                });
            }
        }
    }
}
