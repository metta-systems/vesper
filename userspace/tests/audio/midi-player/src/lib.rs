#![no_std]

//! The pure part of the MIDI test's player (`src/player.rs`, Vesper only):
//! what it logs about its song at start ([`SongInfo`]), where in the song a
//! rendered frame falls ([`SongInfo::position`], with the sequencer's own
//! timing), and the level of the frames it streams ([`LevelMeter`]). Host
//! tests (`tests/song.rs`) run them on `D_E1M1`.

extern crate alloc;

use {
    alloc::vec::Vec,
    core::fmt,
    wood_synth::{
        Rate, Song,
        sequencer::frames_per_tick,
        smf::{DEFAULT_TEMPO, EventKind},
    },
};

/// Notes [`SongInfo`] keeps from the start of the song.
pub const FIRST_NOTES: usize = 8;

/// A note-on of the song.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Note {
    pub tick: u32,
    /// MIDI channel, 0 to 15 (9 is percussion).
    pub channel: u8,
    pub key: u8,
    pub velocity: u8,
}

const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

impl fmt::Display for Note {
    /// `ch0 E2(40) v127 @0`: channel, note name and octave (key 60 is C4),
    /// key, velocity and tick.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let octave = i32::from(self.key / 12) - 1;
        write!(
            formatter,
            "ch{} {}{octave}({}) v{} @{}",
            self.channel,
            NOTE_NAMES[usize::from(self.key % 12)],
            self.key,
            self.velocity,
            self.tick
        )
    }
}

/// Notes shown one after another, comma-separated.
pub struct NoteList<'notes>(pub &'notes [Note]);

impl fmt::Display for NoteList<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, note) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str(", ")?;
            }
            write!(formatter, "{note}")?;
        }
        Ok(())
    }
}

/// A stretch of the song at one tempo, as the sequencer times it: from
/// `tick`, at `time` (32.32 frames from the start of a pass).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Segment {
    tick: u32,
    time: u64,
    /// 32.32 frames per tick.
    frames_per_tick: u64,
}

impl Segment {
    /// The time of `tick`, at or after the segment's start.
    fn time_of(&self, tick: u32) -> u64 {
        let ticks = u128::from(tick.saturating_sub(self.tick));
        let offset = ticks * u128::from(self.frames_per_tick);
        self.time
            .saturating_add(u64::try_from(offset).unwrap_or(u64::MAX))
    }
}

/// Where in a looping song a frame falls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Position {
    /// Passes through the song finished before it (the loop count).
    pub pass: u64,
    /// The song's tick it falls in.
    pub tick: u32,
    /// Its frame from the start of its pass.
    pub frame: u64,
}

/// What the player logs about its song: its timing, its size and its first
/// notes, and the tempo map to place a frame in it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SongInfo {
    /// Ticks per quarter note.
    pub division: u16,
    /// Microseconds per quarter note at the start.
    pub tempo: u32,
    /// Tempo events in the song.
    pub tempo_changes: usize,
    pub events: usize,
    /// Note-ons (with a velocity) in the song.
    pub notes: usize,
    /// The tick a pass ends at, where a loop restarts.
    pub end_tick: u32,
    /// The first [`FIRST_NOTES`] note-ons.
    pub first_notes: Vec<Note>,
    /// The tempo map, to place a frame in the song.
    pub timing: Timing,
}

/// A song's tempo map, as the sequencer times it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Timing {
    /// 32.32 frames of one pass.
    pass_time: u64,
    segments: Vec<Segment>,
}

impl SongInfo {
    /// The song's timing at `rate`, as the synthesiser's sequencer plays it.
    #[must_use]
    pub fn of(song: &Song, rate: Rate) -> Self {
        let mut segments = Vec::from([Segment {
            tick: 0,
            time: 0,
            frames_per_tick: frames_per_tick(rate, DEFAULT_TEMPO, song.division),
        }]);
        let mut tempo = DEFAULT_TEMPO;
        let mut tempo_changes = 0;
        let mut notes = 0;
        let mut first_notes = Vec::new();
        for event in &song.events {
            match event.kind {
                EventKind::Tempo(microseconds) => {
                    tempo_changes += 1;
                    if event.tick == 0 {
                        tempo = microseconds;
                    }
                    let current = *segments.last().unwrap_or(&segments[0]);
                    segments.push(Segment {
                        tick: event.tick,
                        time: current.time_of(event.tick),
                        frames_per_tick: frames_per_tick(rate, microseconds, song.division),
                    });
                }
                EventKind::Channel {
                    status,
                    data1,
                    data2,
                } if status & 0xf0 == 0x90 && data2 > 0 => {
                    notes += 1;
                    if first_notes.len() < FIRST_NOTES {
                        first_notes.push(Note {
                            tick: event.tick,
                            channel: status & 0x0f,
                            key: data1,
                            velocity: data2,
                        });
                    }
                }
                EventKind::Channel { .. } => {}
            }
        }
        let last = *segments.last().unwrap_or(&segments[0]);
        Self {
            division: song.division,
            tempo,
            tempo_changes,
            events: song.events.len(),
            notes,
            end_tick: song.end_tick,
            first_notes,
            timing: Timing {
                pass_time: last.time_of(song.end_tick),
                segments,
            },
        }
    }

    /// Beats per minute at the start.
    #[must_use]
    pub const fn beats_per_minute(&self) -> u32 {
        match 60_000_000_u32.checked_div(self.tempo) {
            Some(bpm) => bpm,
            None => 0,
        }
    }

    /// Frames of one pass through the song, rounded up.
    #[must_use]
    pub const fn pass_frames(&self) -> u64 {
        self.timing.pass_time.div_ceil(1 << 32)
    }

    /// Where frame `frames` (counted from the song's start) falls, the song
    /// looping as the sequencer loops it: a pass ends at the end tick's time
    /// and the next starts there, fraction kept.
    #[must_use]
    pub fn position(&self, frames: u64) -> Position {
        let timing = &self.timing;
        let time = u128::from(frames) << 32;
        let (pass, within) = if timing.pass_time == 0 {
            (0, time)
        } else {
            let pass_time = u128::from(timing.pass_time);
            (time / pass_time, time % pass_time)
        };
        let within = u64::try_from(within).unwrap_or(u64::MAX);
        let segment = timing
            .segments
            .iter()
            .rev()
            .find(|segment| segment.time <= within)
            .unwrap_or(&timing.segments[0]);
        let ticks = (within - segment.time)
            .checked_div(segment.frames_per_tick)
            .unwrap_or(0);
        Position {
            pass: u64::try_from(pass).unwrap_or(u64::MAX),
            tick: segment
                .tick
                .saturating_add(u32::try_from(ticks).unwrap_or(u32::MAX))
                .min(self.end_tick),
            frame: within >> 32,
        }
    }
}

/// The level of a stretch of stereo frames: the peak sample and the RMS over
/// both sides, in sample units (full scale 32 767).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LevelMeter {
    peak: u16,
    sum_of_squares: u64,
    samples: u64,
}

impl LevelMeter {
    /// Nothing measured.
    pub const EMPTY: Self = Self {
        peak: 0,
        sum_of_squares: 0,
        samples: 0,
    };

    /// Measure `frames` too.
    pub fn add(&mut self, frames: &[[i16; 2]]) {
        for sample in frames.iter().flatten() {
            let magnitude = sample.unsigned_abs();
            self.peak = self.peak.max(magnitude);
            self.sum_of_squares = self
                .sum_of_squares
                .saturating_add(u64::from(magnitude) * u64::from(magnitude));
            self.samples += 1;
        }
    }

    /// The largest sample magnitude.
    #[must_use]
    pub const fn peak(&self) -> u16 {
        self.peak
    }

    /// The root mean square of the samples, rounded down; 0 for none.
    #[must_use]
    pub fn rms(&self) -> u16 {
        let mean = self.sum_of_squares.checked_div(self.samples).unwrap_or(0);
        u16::try_from(mean.isqrt()).unwrap_or(u16::MAX)
    }

    /// What was measured so far; starts over.
    #[must_use]
    pub const fn take(&mut self) -> Self {
        core::mem::replace(self, Self::EMPTY)
    }
}
