//! Plays a [`Song`] on a [`Synth`], clocked by the frames rendered.
//!
//! Time is kept in frames in 32.32 fixed point from the start of the song
//! (or of the current loop). An event at tick `t` falls at
//! `segment_start + (t - segment_tick) * frames_per_tick`, where the segment
//! is the run since the last tempo change and `frames_per_tick` is
//! `rate * tempo / (division * 1 000 000)` in 32.32; it is handled before the
//! first frame at or after that time, so its frame is the time rounded up.
//! The audio clock alone drives the music: no drift against the output and a
//! deterministic render.

use crate::{
    smf::{DEFAULT_TEMPO, EventKind, Song},
    synth::Synth,
    tables::Rate,
};

/// Frames per tick in 32.32 at `rate`, for `tempo` microseconds per quarter
/// note and `division` ticks per quarter note.
#[must_use]
pub fn frames_per_tick(rate: Rate, tempo: u32, division: u16) -> u64 {
    let numerator = (u128::from(rate.numerator) * u128::from(tempo)) << 32;
    let denominator = u128::from(rate.denominator.max(1)) * u128::from(division.max(1)) * 1_000_000;
    u64::try_from(numerator / denominator).unwrap_or(u64::MAX)
}

/// Where in a song the sequencer is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Cursor {
    /// Next event to handle.
    event: usize,
    /// Tick and time (32.32 frames) where the current tempo started.
    segment_tick: u32,
    segment_time: u64,
    /// Frames per tick of the current tempo, 32.32.
    frames_per_tick: u64,
}

/// A sequencer and its synthesiser.
#[derive(Clone, Debug)]
pub struct Sequencer<'a> {
    synth: Synth<'a>,
    song: Option<Song>,
    looping: bool,
    paused: bool,
    playing: bool,
    cursor: Cursor,
    /// Time rendered since the song (or the loop) started, 32.32 frames.
    now: u64,
    /// Frames rendered since `play`.
    frames: u64,
}

impl<'a> Sequencer<'a> {
    /// A stopped sequencer over `synth`.
    #[must_use]
    pub const fn new(synth: Synth<'a>) -> Self {
        Self {
            synth,
            song: None,
            looping: false,
            paused: false,
            playing: false,
            cursor: Cursor {
                event: 0,
                segment_tick: 0,
                segment_time: 0,
                frames_per_tick: 0,
            },
            now: 0,
            frames: 0,
        }
    }

    /// The synthesiser.
    #[must_use]
    pub const fn synth(&self) -> &Synth<'a> {
        &self.synth
    }

    /// The synthesiser, to send it messages directly.
    pub const fn synth_mut(&mut self) -> &mut Synth<'a> {
        &mut self.synth
    }

    fn rewind(&mut self) {
        let division = self.song.as_ref().map_or(1, |song| song.division);
        self.cursor = Cursor {
            event: 0,
            segment_tick: 0,
            segment_time: 0,
            frames_per_tick: frames_per_tick(self.synth.rate(), DEFAULT_TEMPO, division),
        };
    }

    /// Start `song` from its beginning, replacing what played; it restarts
    /// at its end if `looping`.
    pub fn play(&mut self, song: Song, looping: bool) {
        self.synth.reset();
        self.song = Some(song);
        self.looping = looping;
        self.paused = false;
        self.playing = true;
        self.now = 0;
        self.frames = 0;
        self.rewind();
    }

    /// Stop the song; the notes fade with their release.
    pub fn stop(&mut self) {
        self.playing = false;
        self.synth.release_all();
    }

    /// Pause: the output is silent and nothing advances until [`resume`].
    ///
    /// [`resume`]: Self::resume
    pub const fn pause(&mut self) {
        self.paused = true;
    }

    /// Resume after [`pause`](Self::pause).
    pub const fn resume(&mut self) {
        self.paused = false;
    }

    /// Music volume, 0 to 127.
    pub fn set_volume(&mut self, volume: u8) {
        self.synth.set_volume(volume);
    }

    /// Whether a song is playing (a looping song plays until stopped).
    #[must_use]
    pub const fn is_playing(&self) -> bool {
        self.playing
    }

    /// Frames rendered since `play`.
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.frames
    }

    /// Time in 32.32 frames of `tick` in the current tempo segment.
    fn time_of(&self, tick: u32) -> u64 {
        let ticks = u128::from(tick.saturating_sub(self.cursor.segment_tick));
        let offset = ticks * u128::from(self.cursor.frames_per_tick);
        self.cursor
            .segment_time
            .saturating_add(u64::try_from(offset).unwrap_or(u64::MAX))
    }

    /// Handle the events due by `now`, loop or end at the song's end, and
    /// return the frames until the next event (`None`: no more events).
    fn dispatch(&mut self) -> Option<u64> {
        loop {
            let song = self.song.as_ref()?;
            if let Some(event) = song.events.get(self.cursor.event).copied() {
                let time = self.time_of(event.tick);
                if time > self.now {
                    return Some((time - self.now).div_ceil(1 << 32));
                }
                self.cursor.event += 1;
                match event.kind {
                    EventKind::Channel {
                        status,
                        data1,
                        data2,
                    } => self.synth.message(status, data1, data2),
                    EventKind::Tempo(tempo) => {
                        self.cursor.segment_time = time;
                        self.cursor.segment_tick = event.tick;
                        self.cursor.frames_per_tick =
                            frames_per_tick(self.synth.rate(), tempo, song.division);
                    }
                }
                continue;
            }
            let end = self.time_of(song.end_tick);
            if end > self.now {
                return Some((end - self.now).div_ceil(1 << 32));
            }
            if self.looping && end > 0 {
                // Keep the fraction past the end, so loops do not drift.
                self.now -= end;
                self.synth.release_all();
                self.rewind();
            } else {
                self.playing = false;
                return None;
            }
        }
    }

    /// Render `out.len() / 2` frames and add them (interleaved stereo, not
    /// clipped) to `out`, handling the song's events on their frames.
    pub fn mix(&mut self, out: &mut [i32]) {
        if self.paused {
            return;
        }
        let mut rest = out;
        while !rest.is_empty() {
            let available = u64::try_from(rest.len() / 2).unwrap_or(u64::MAX);
            let frames = if self.playing {
                self.dispatch()
                    .map_or(available, |until| until.min(available))
            } else {
                available
            };
            let length = usize::try_from(frames).unwrap_or(0) * 2;
            let (now, later) = rest.split_at_mut(length);
            self.synth.mix(now);
            self.now += frames << 32;
            self.frames += frames;
            rest = later;
        }
    }

    /// Render `out.len() / 2` frames into `out` (interleaved stereo s16).
    pub fn render(&mut self, out: &mut [i16]) {
        if self.paused {
            out.fill(0);
            return;
        }
        let mut rest = out;
        while !rest.is_empty() {
            let available = u64::try_from(rest.len() / 2).unwrap_or(u64::MAX);
            let frames = if self.playing {
                self.dispatch()
                    .map_or(available, |until| until.min(available))
            } else {
                available
            };
            let length = usize::try_from(frames).unwrap_or(0) * 2;
            let (now, later) = rest.split_at_mut(length);
            self.synth.render(now);
            self.now += frames << 32;
            self.frames += frames;
            rest = later;
        }
    }
}
