//! One sounding sample: the note it answers and how it is played.
//!
//! [`Voice`] holds what the synthesiser decides on (channel, key, release,
//! stealing order) and a sound, the engine that renders it:
//!
//! - [`IntegerSound`], always there: fixed-point playback with linear
//!   interpolation, the SoundFont loop modes, the volume envelope,
//!   attenuation and pan. Positions are absolute sample-point indices into
//!   the bank's sample data in 32.32 fixed point; gains are Q16 and ramp
//!   linearly across each block, so envelope, volume and pan changes do not
//!   click.
//! - the f64 sound of [`crate::float`], with the `fp-mode` feature: the same
//!   voice in double precision.
//!
//! Both start from one [`VoiceSetup`], the generator values resolved in the
//! SoundFont's own units.

use {
    crate::{
        bank::{Zone, voice_value},
        channel::Channel,
        envelope::{Envelope, EnvelopeParameters},
        generator,
        sf2::{SampleHeader, SoundFont},
        tables::{
            Rate, SILENT_CENTIBELS, attenuation_gain, concave_centibels, pan_gains, phase_step,
            timecents_frames,
        },
    },
    core::mem,
};

/// How a voice treats its sample's loop (generator `sampleModes`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoopMode {
    /// Play from start to end once.
    None,
    /// Loop for as long as the voice sounds.
    Continuous,
    /// Loop until the note is released, then play on to the end.
    UntilRelease,
}

/// Which engine renders the voices a note starts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Engine {
    /// Fixed point ([`IntegerSound`]).
    Integer,
    /// Double precision (`crate::float`).
    #[cfg(feature = "fp-mode")]
    Float,
}

/// What starts a voice: the note and the zones and sample that answer it.
#[derive(Clone, Copy, Debug)]
pub struct NoteStart<'z> {
    /// MIDI channel.
    pub channel: u8,
    /// MIDI key.
    pub key: u8,
    /// MIDI velocity, 1 to 127.
    pub velocity: u8,
    /// The preset zone.
    pub preset_zone: &'z Zone,
    /// The instrument zone.
    pub instrument_zone: &'z Zone,
    /// The sample.
    pub sample: &'z SampleHeader,
}

/// A voice's generators resolved for its note, in the SoundFont's units:
/// what either engine starts from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VoiceSetup {
    /// First sample point.
    pub begin: u32,
    /// Sample point past the last.
    pub end: u32,
    /// Loop start and end points.
    pub loop_start: u32,
    pub loop_end: u32,
    pub mode: LoopMode,
    /// The sample's recording rate in Hz.
    pub sample_rate: u32,
    /// Pitch above the sample's root, in cents, without the channel's bend.
    pub pitch_cents: i32,
    /// Generator `initialAttenuation` in centibels, 0 to 1440.
    pub initial_attenuation: u32,
    /// The velocity the attenuation follows (the note's, or the zone's fixed
    /// one), 1 to 127.
    pub velocity: u8,
    /// Pan from the generators, -500 to 500.
    pub pan: i32,
    /// Envelope stage times in timecents (the hold and decay scaled by key).
    pub delay: i32,
    pub attack: i32,
    pub hold: i32,
    pub decay: i32,
    pub release: i32,
    /// Sustain level as attenuation in centibels, 0 to 1440.
    pub sustain: u32,
    /// Exclusive class (0: none).
    pub exclusive_class: i32,
}

/// Generator value `operator` of the start's zones.
fn value(start: &NoteStart<'_>, operator: u16) -> i32 {
    voice_value(start.preset_zone, start.instrument_zone, operator)
}

/// A sample address: the header's plus the fine and coarse offsets.
fn address(base: u32, fine: i32, coarse: i32) -> u32 {
    let address = i64::from(base) + i64::from(fine) + 32_768 * i64::from(coarse);
    u32::try_from(address.max(0)).unwrap_or(u32::MAX)
}

impl VoiceSetup {
    /// The setup of `start` in a bank of `points` sample points.
    #[must_use]
    pub fn new(start: &NoteStart<'_>, points: u32) -> Self {
        let sample = start.sample;
        let begin = address(
            sample.start,
            value(start, generator::START_ADDRS_OFFSET),
            value(start, generator::START_ADDRS_COARSE_OFFSET),
        )
        .min(points);
        let end = address(
            sample.end,
            value(start, generator::END_ADDRS_OFFSET),
            value(start, generator::END_ADDRS_COARSE_OFFSET),
        )
        .clamp(begin, points);
        let loop_start = address(
            sample.loop_start,
            value(start, generator::STARTLOOP_ADDRS_OFFSET),
            value(start, generator::STARTLOOP_ADDRS_COARSE_OFFSET),
        )
        .clamp(begin, end);
        let loop_end = address(
            sample.loop_end,
            value(start, generator::ENDLOOP_ADDRS_OFFSET),
            value(start, generator::ENDLOOP_ADDRS_COARSE_OFFSET),
        )
        .clamp(begin, end);
        let mode = match value(start, generator::SAMPLE_MODES) & 3 {
            _ if loop_end <= loop_start => LoopMode::None,
            1 => LoopMode::Continuous,
            3 => LoopMode::UntilRelease,
            _ => LoopMode::None,
        };

        let key = match value(start, generator::KEYNUM) {
            fixed @ 0..=127 => fixed,
            _ => i32::from(start.key),
        };
        let velocity = match value(start, generator::VELOCITY) {
            fixed @ 1..=127 => u8::try_from(fixed).unwrap_or(127),
            _ => start.velocity,
        };
        let root = match value(start, generator::OVERRIDING_ROOT_KEY) {
            root @ 0..=127 => root,
            _ if sample.original_pitch <= 127 => i32::from(sample.original_pitch),
            _ => 60,
        };
        let pitch_cents = (key - root) * value(start, generator::SCALE_TUNING)
            + value(start, generator::COARSE_TUNE) * 100
            + value(start, generator::FINE_TUNE)
            + i32::from(sample.pitch_correction);

        let initial_attenuation =
            u32::try_from(value(start, generator::INITIAL_ATTENUATION).clamp(0, 1440)).unwrap_or(0);

        let key_scaled =
            |operator, scale| value(start, operator) + (60 - key) * value(start, scale);
        Self {
            begin,
            end,
            loop_start,
            loop_end,
            mode,
            sample_rate: sample.sample_rate,
            pitch_cents,
            initial_attenuation,
            velocity,
            pan: value(start, generator::PAN),
            delay: value(start, generator::DELAY_VOL_ENV),
            attack: value(start, generator::ATTACK_VOL_ENV),
            hold: key_scaled(generator::HOLD_VOL_ENV, generator::KEYNUM_TO_VOL_ENV_HOLD),
            decay: key_scaled(generator::DECAY_VOL_ENV, generator::KEYNUM_TO_VOL_ENV_DECAY),
            release: value(start, generator::RELEASE_VOL_ENV),
            sustain: u32::try_from(value(start, generator::SUSTAIN_VOL_ENV).clamp(0, 1440))
                .unwrap_or(0),
            exclusive_class: value(start, generator::EXCLUSIVE_CLASS),
        }
    }
}

/// A voice.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Voice {
    /// MIDI channel.
    pub(crate) channel: u8,
    /// MIDI key that started it.
    pub(crate) key: u8,
    /// Exclusive class (0: none).
    pub(crate) exclusive_class: i32,
    /// Order of starting, for stealing the oldest.
    pub(crate) serial: u64,
    /// Released by a note-off (or by the sustain pedal's release).
    pub(crate) released: bool,
    /// Note-off seen while the sustain pedal was down.
    pub(crate) held_by_pedal: bool,
    sound: Sound,
}

/// The engine state of a voice.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Sound {
    Integer(IntegerSound),
    #[cfg(feature = "fp-mode")]
    Float(crate::float::FloatSound),
}

/// Where a render call adds a voice's frames: the integer accumulator, and
/// with `fp-mode` the f64 one beside it.
pub struct Mix<'block> {
    /// Interleaved stereo, the integer engine's units (a point times a gain).
    pub integer: &'block mut [i32],
    /// Stereo frames, the f64 engine's (the same units).
    #[cfg(feature = "fp-mode")]
    pub float: &'block mut [crate::float::Frame],
    /// Whether an f64 voice added to `float`.
    #[cfg(feature = "fp-mode")]
    pub float_used: bool,
}

impl Voice {
    /// Start a voice for `start` at `rate` with `engine`, numbered `serial`,
    /// in a bank of `points` sample points.
    #[must_use]
    pub fn new(
        start: &NoteStart<'_>,
        rate: Rate,
        points: u32,
        serial: u64,
        engine: Engine,
    ) -> Self {
        let setup = VoiceSetup::new(start, points);
        let sound = match engine {
            Engine::Integer => Sound::Integer(IntegerSound::new(&setup, rate)),
            #[cfg(feature = "fp-mode")]
            Engine::Float => Sound::Float(crate::float::FloatSound::new(&setup, rate)),
        };
        Self {
            channel: start.channel,
            key: start.key,
            exclusive_class: setup.exclusive_class,
            serial,
            released: false,
            held_by_pedal: false,
            sound,
        }
    }

    /// Release the note (note-off, or the pedal let go).
    pub fn release(&mut self) {
        self.released = true;
        self.held_by_pedal = false;
        match &mut self.sound {
            Sound::Integer(sound) => sound.envelope.release(),
            #[cfg(feature = "fp-mode")]
            Sound::Float(sound) => sound.release(),
        }
    }

    /// Release within `frames` (exclusive class, stealing).
    pub fn cut(&mut self, frames: u32) {
        self.released = true;
        self.held_by_pedal = false;
        match &mut self.sound {
            Sound::Integer(sound) => sound.envelope.release_within(frames),
            #[cfg(feature = "fp-mode")]
            Sound::Float(sound) => sound.release_within(frames),
        }
    }

    /// Whether the voice has nothing more to play.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        match &self.sound {
            Sound::Integer(sound) => sound.envelope.is_finished(),
            #[cfg(feature = "fp-mode")]
            Sound::Float(sound) => sound.is_finished(),
        }
    }

    /// Current loudness, Q16 (for choosing a voice to steal).
    #[must_use]
    pub fn loudness(&self) -> i32 {
        match &self.sound {
            Sound::Integer(sound) => sound.left_gain.max(sound.right_gain),
            #[cfg(feature = "fp-mode")]
            Sound::Float(sound) => sound.loudness(),
        }
    }

    /// The engine rendering this voice.
    #[must_use]
    pub const fn engine(&self) -> Engine {
        match self.sound {
            Sound::Integer(_) => Engine::Integer,
            #[cfg(feature = "fp-mode")]
            Sound::Float(_) => Engine::Float,
        }
    }

    /// Add `frames` frames of this voice into `mix`, with `channel`'s volume,
    /// pan and bend; see [`IntegerSound::render`] for `until_boundary`.
    pub fn render(
        &mut self,
        font: &SoundFont<'_>,
        channel: &Channel,
        rate: Rate,
        mix: &mut Mix<'_>,
        frames: u32,
        until_boundary: u32,
    ) {
        let looping = self.looping();
        match &mut self.sound {
            Sound::Integer(sound) => sound.render(
                font,
                channel,
                rate,
                looping,
                mix.integer,
                frames,
                until_boundary,
            ),
            #[cfg(feature = "fp-mode")]
            Sound::Float(sound) => {
                mix.float_used = true;
                sound.render(font, channel, looping, mix.float, frames, until_boundary);
            }
        }
    }

    /// Whether the sample loops now: its mode, and for a loop until release,
    /// whether the note is still held. `released` changes only between render
    /// calls, so the answer holds for a whole call.
    fn looping(&self) -> bool {
        let mode = match &self.sound {
            Sound::Integer(sound) => sound.mode,
            #[cfg(feature = "fp-mode")]
            Sound::Float(sound) => sound.mode(),
        };
        match mode {
            LoopMode::None => false,
            LoopMode::Continuous => true,
            LoopMode::UntilRelease => !self.released,
        }
    }
}

/// The integer engine's state of a voice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IntegerSound {
    end: u32,
    loop_start: u32,
    loop_end: u32,
    mode: LoopMode,
    /// Position in 32.32 sample points.
    position: u64,
    /// Step per frame in 32.32.
    step: u64,
    /// The channel bend the step was computed for.
    step_bend: i32,
    sample_rate: u32,
    /// Pitch above the sample's root, in cents, without the channel's bend.
    pitch_cents: i32,
    /// Attenuation of the sample and the velocity, in centibels.
    attenuation: u32,
    /// Pan from the generators, -500 to 500.
    pan: i32,
    envelope: Envelope,
    /// Gains reached so far, Q16.
    left_gain: i32,
    right_gain: i32,
    /// The ramp under way: its end values, the step per frame and the
    /// frames left to it (zero: start a new ramp).
    target_left: i32,
    target_right: i32,
    left_delta: i32,
    right_delta: i32,
    ramp_left: u32,
}

impl IntegerSound {
    /// The integer playback of `setup` at `rate`.
    #[must_use]
    pub fn new(setup: &VoiceSetup, rate: Rate) -> Self {
        let frames = |timecents| timecents_frames(timecents, rate);
        let envelope = Envelope::new(EnvelopeParameters {
            delay: frames(setup.delay),
            attack: frames(setup.attack),
            hold: frames(setup.hold),
            decay: frames(setup.decay),
            sustain: setup.sustain,
            release: frames(setup.release),
        });
        Self {
            end: setup.end,
            loop_start: setup.loop_start,
            loop_end: setup.loop_end,
            mode: setup.mode,
            position: u64::from(setup.begin) << 32,
            step: phase_step(setup.sample_rate, setup.pitch_cents, rate),
            step_bend: 0,
            sample_rate: setup.sample_rate,
            pitch_cents: setup.pitch_cents,
            // initialAttenuation counts 0.4 of its value, as in EMU hardware
            // and in rustysynth and FluidSynth.
            attenuation: setup.initial_attenuation * 2 / 5 + concave_centibels(setup.velocity),
            pan: setup.pan,
            envelope,
            left_gain: 0,
            right_gain: 0,
            target_left: 0,
            target_right: 0,
            left_delta: 0,
            right_delta: 0,
            ramp_left: 0,
        }
    }

    /// Start a gain ramp of `segment` frames: advance the envelope by them
    /// and aim the gains at the level they reach, with `channel`'s volume
    /// and pan.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "gains are at most 2^16"
    )]
    fn start_ramp(&mut self, channel: &Channel, segment: u32) {
        self.envelope.advance(segment);
        let attenuation = self.attenuation + channel.attenuation();
        let level = if attenuation >= SILENT_CENTIBELS {
            0
        } else {
            (u64::from(self.envelope.gain()) * u64::from(attenuation_gain(attenuation << 16))) >> 16
        };
        let (pan_left, pan_right) = pan_gains(self.pan + channel.pan_offset());
        self.target_left = ((level * u64::from(pan_left)) >> 15) as i32;
        self.target_right = ((level * u64::from(pan_right)) >> 15) as i32;
        let count = segment.max(1) as i32;
        self.left_delta = (self.target_left - self.left_gain) / count;
        self.right_delta = (self.target_right - self.right_gain) / count;
        self.ramp_left = segment.max(1);
    }

    /// Add `frames` frames into `out` (interleaved stereo, at least
    /// `2 * frames` long), with `channel`'s volume, pan and bend, `looping`
    /// as the voice says.
    ///
    /// The envelope and the gains move on a grid of blocks the synthesiser
    /// keeps from its first frame: `until_boundary` frames are left to the
    /// next grid point (at least `frames`). A ramp starts at a grid point (or
    /// at the voice's first frame) and runs to the next, however the frames
    /// are split into calls, so the output does not depend on the split.
    #[expect(
        clippy::too_many_arguments,
        reason = "the render call's state, in one place"
    )]
    fn render(
        &mut self,
        font: &SoundFont<'_>,
        channel: &Channel,
        rate: Rate,
        looping: bool,
        out: &mut [i32],
        frames: u32,
        until_boundary: u32,
    ) {
        if self.envelope.is_finished() {
            return;
        }
        let bend = channel.bend_cents();
        if bend != self.step_bend {
            self.step = phase_step(self.sample_rate, self.pitch_cents + bend, rate);
            self.step_bend = bend;
        }
        if self.ramp_left == 0 {
            self.start_ramp(channel, until_boundary);
        }
        let mut gains = Gains {
            left: self.left_gain,
            right: self.right_gain,
            left_delta: self.left_delta,
            right_delta: self.right_delta,
        };
        let step = self.step;
        let mut position = self.position;
        // Within a run every frame has `index + 1 < run_limit`: both points
        // are in the sample, the next point is `index + 1`, and the loop
        // cannot wrap before the run's last step.
        let run_limit = if looping { self.loop_end } else { self.end };
        let out = out.as_chunks_mut::<2>().0;
        let frame_count = out.len().min(frames as usize);
        let mut remaining = &mut out[..frame_count];

        loop {
            if let Some(points) = font.points {
                let limit = run_limit.min(u32::try_from(points.len()).unwrap_or(u32::MAX));
                let run = run_frames(position, step, limit).min(remaining.len());
                if run > 0 {
                    let (run_out, rest) = mem::take(&mut remaining).split_at_mut(run);
                    remaining = rest;
                    position = mix_run(points, run_out, position, step, &mut gains);
                    if looping {
                        position = self.wrap(position);
                    }
                }
            }
            // One frame the slow way: at the loop's or the sample's end, or
            // on a bank whose points are not aligned.
            let Some(([left, right], rest)) = mem::take(&mut remaining).split_first_mut() else {
                break;
            };
            remaining = rest;
            let index = (position >> 32) as u32;
            if !looping && index >= self.end {
                // The sample ran out: the voice ends here.
                self.envelope.finish();
                break;
            }
            let next = if looping && index + 1 >= self.loop_end {
                self.loop_start
            } else {
                index + 1
            };
            let here = i32::from(font.sample_point(index));
            let after = if next < self.end {
                i32::from(font.sample_point(next))
            } else {
                0
            };
            gains.mix(left, right, interpolate(here, after, position));
            position += step;
            if looping {
                position = self.wrap(position);
            }
        }

        self.position = position;
        self.ramp_left = self.ramp_left.saturating_sub(frames);
        if self.ramp_left == 0 {
            // Land exactly on the target, whatever the deltas rounded away.
            self.left_gain = self.target_left;
            self.right_gain = self.target_right;
        } else {
            self.left_gain = gains.left;
            self.right_gain = gains.right;
        }
    }

    /// `position` brought back into the loop, if it ran past its end.
    fn wrap(&self, mut position: u64) -> u64 {
        let loop_length = u64::from(self.loop_end - self.loop_start) << 32;
        while position >> 32 >= u64::from(self.loop_end) {
            position -= loop_length;
        }
        position
    }
}

/// The gains of a render call: where they are, Q16, and their step per frame.
#[derive(Clone, Copy)]
struct Gains {
    left: i32,
    right: i32,
    left_delta: i32,
    right_delta: i32,
}

impl Gains {
    /// Step the gains and add `point` at them to a frame.
    #[inline(always)]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a point times a Q16 gain, shifted back, fits i32"
    )]
    fn mix(&mut self, left: &mut i32, right: &mut i32, point: i32) {
        self.left += self.left_delta;
        self.right += self.right_delta;
        *left += ((i64::from(point) * i64::from(self.left)) >> 16) as i32;
        *right += ((i64::from(point) * i64::from(self.right)) >> 16) as i32;
    }
}

/// The point between `here` and `after` at the fraction of `position`.
#[inline(always)]
const fn interpolate(here: i32, after: i32, position: u64) -> i32 {
    // 15 bits of fraction: the product of a 17-bit difference fits.
    let fraction = ((position >> 17) & 0x7fff) as i32;
    here + (((after - here) * fraction) >> 15)
}

/// Frames from `position`, `step` apart, whose point index `i` keeps
/// `i + 1 < limit` (unbounded when `step` is 0).
fn run_frames(position: u64, step: u64, limit: u32) -> usize {
    let Some(last) = limit.checked_sub(1) else {
        return 0;
    };
    let threshold = u64::from(last) << 32;
    if position >= threshold {
        return 0;
    }
    if step == 0 {
        return usize::MAX;
    }
    usize::try_from((threshold - position).div_ceil(step)).unwrap_or(usize::MAX)
}

/// Add a run of frames from `points` to `out`, starting at `position`, and
/// return the position after it. The caller sized the run by [`run_frames`],
/// so every frame reads `index` and `index + 1` within `points` and needs no
/// end or wrap check: the frames are those of [`IntegerSound::render`]'s slow
/// path, bit for bit.
#[inline(always)]
fn mix_run(
    points: &[i16],
    out: &mut [[i32; 2]],
    mut position: u64,
    step: u64,
    gains: &mut Gains,
) -> u64 {
    let Some(last_frame) = out.len().checked_sub(1) else {
        return position;
    };
    // The run's highest index: it reads up to `last_index + 1`, so one
    // check here bounds every read below (the indices only grow).
    let last_index = ((position + last_frame as u64 * step) >> 32) as usize;
    let window = &points[..last_index + 2];
    // A copy in registers for the run.
    let mut run_gains = *gains;
    for [left, right] in out {
        let index = ((position >> 32) as usize).min(last_index);
        let here = i32::from(window[index]);
        let after = i32::from(window[index + 1]);
        run_gains.mix(left, right, interpolate(here, after, position));
        position += step;
    }
    *gains = run_gains;
    position
}
