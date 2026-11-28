//! The f64 engine (`fp-mode`, AUDIO-20): a voice in double
//! precision, the representative FP/SIMD workload of the FP/SIMD plan's
//! P-SIMD-5.
//!
//! It plays what the integer engine plays, on the same grid of blocks, with
//! the same stages and slopes; only the arithmetic differs. The sample
//! position and its step, the envelope's level and times, and the
//! attenuation and pan curves are f64, computed with `libm` instead of read
//! from the integer tables: 2^(cents/1200) ratios, 10^(-cB/200) gains, the
//! concave curve's 400·log10(127/v) and the cosine pan law.
//!
//! The mixing loop ([`mix_run`]) is plain f64 code for the compiler's
//! vectorizer: the left and right channels of a frame are one two-lane
//! vector (`just _synth-fp-vectorized` checks the instructions it became).

use crate::{
    channel::Channel,
    envelope::DECAY_CENTIBELS,
    sf2::SoundFont,
    tables::{Rate, SILENT_CENTIBELS},
    voice::{LoopMode, VoiceSetup},
};

/// Silence in centibels, as f64.
const SILENT: f64 = SILENT_CENTIBELS as f64;

/// A stereo frame of the f64 engine, left then right, in the integer
/// engine's units (a point times a linear gain).
pub type Frame = [f64; 2];

/// A silent frame.
pub const SILENT_FRAME: Frame = [0.0; 2];

/// `rendered` with the master volume `master_volume` (Q16) applied and
/// shifted down by `shift` more bits, rounded to the integer engine's units:
/// the integer engine's output scaling, in f64.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "a scaled-down sample fits i32; the cast saturates"
)]
pub fn scale(rendered: f64, master_volume: i32, shift: u32) -> i32 {
    let master = f64::from(master_volume) / f64::from(1_u32 << (16 + shift));
    libm::round(rendered * master) as i32
}

/// The rate in Hz.
fn rate_hz(rate: Rate) -> f64 {
    f64::from(rate.numerator) / f64::from(rate.denominator.max(1))
}

/// `2^(cents / 1200)`.
fn cents_ratio(cents: f64) -> f64 {
    libm::exp2(cents / 1200.0)
}

/// Frames at `rate` that `timecents` last: `2^(timecents / 1200)` seconds,
/// clamped to the SoundFont's -12 000 (1 ms) to 8 000 (101.6 s).
fn timecents_frames(timecents: i32, rate: Rate) -> f64 {
    cents_ratio(f64::from(timecents.clamp(-12_000, 8_000))) * rate_hz(rate)
}

/// Linear gain of an attenuation in centibels: 10^(-cB / 200), 0 at
/// [`SILENT_CENTIBELS`] and past.
fn attenuation_gain(centibels: f64) -> f64 {
    if centibels >= SILENT {
        0.0
    } else {
        libm::pow(10.0, -centibels / 200.0)
    }
}

/// The attenuation in centibels of a linear gain (the inverse of
/// [`attenuation_gain`]), at most [`SILENT_CENTIBELS`].
fn gain_attenuation(gain: f64) -> f64 {
    if gain <= 0.0 {
        SILENT
    } else {
        (-200.0 * libm::log10(gain)).clamp(0.0, SILENT)
    }
}

/// The concave curve at `value` (0 to 127) as attenuation in centibels:
/// an amplitude of `(value / 127)^2`.
fn concave_centibels(value: u8) -> f64 {
    if value == 0 {
        SILENT
    } else {
        (400.0 * libm::log10(127.0 / f64::from(value.min(127)))).min(SILENT)
    }
}

/// Left and right gains of SoundFont pan `pan` (-500 hard left to 500 hard
/// right): the constant-power law.
fn pan_gains(pan: i32) -> [f64; 2] {
    let position = f64::from(pan.clamp(-500, 500) + 500) / 1000.0;
    let quarter = core::f64::consts::FRAC_PI_2;
    [
        libm::cos(position * quarter),
        libm::cos((1.0 - position) * quarter),
    ]
}

/// Stage of the envelope, as [`crate::envelope::Stage`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    Delay,
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
    Finished,
}

/// The volume envelope in f64: [`crate::envelope::Envelope`]'s stages, with
/// times in frames and the level in centibels.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Envelope {
    delay: f64,
    attack: f64,
    hold: f64,
    /// Frames to fall [`DECAY_CENTIBELS`] in the decay and the release.
    decay: f64,
    release: f64,
    /// Sustain level as attenuation in centibels.
    sustain: f64,
    stage: Stage,
    /// Frames left in a timed stage (delay, attack, hold).
    frames_left: f64,
    /// Attenuation in centibels (hold, decay, sustain, release).
    attenuation: f64,
}

impl Envelope {
    fn new(setup: &VoiceSetup, rate: Rate) -> Self {
        let frames = |timecents| timecents_frames(timecents, rate);
        let mut envelope = Self {
            delay: frames(setup.delay),
            attack: frames(setup.attack),
            hold: frames(setup.hold),
            decay: frames(setup.decay),
            release: frames(setup.release),
            sustain: f64::from(setup.sustain).min(SILENT),
            stage: Stage::Delay,
            frames_left: frames(setup.delay),
            attenuation: 0.0,
        };
        envelope.skip_finished_stages();
        envelope
    }

    /// Move past stages with nothing left.
    fn skip_finished_stages(&mut self) {
        loop {
            match self.stage {
                Stage::Delay if self.frames_left <= 0.0 => {
                    self.stage = Stage::Attack;
                    self.frames_left = self.attack;
                }
                Stage::Attack if self.frames_left <= 0.0 => {
                    self.stage = Stage::Hold;
                    self.frames_left = self.hold;
                    self.attenuation = 0.0;
                }
                Stage::Hold if self.frames_left <= 0.0 => self.stage = Stage::Decay,
                Stage::Decay if self.attenuation >= self.sustain => {
                    self.attenuation = self.sustain;
                    self.stage = if self.sustain >= SILENT {
                        Stage::Finished
                    } else {
                        Stage::Sustain
                    };
                }
                Stage::Release if self.attenuation >= SILENT => self.stage = Stage::Finished,
                _ => return,
            }
        }
    }

    /// The level as linear gain.
    fn gain(&self) -> f64 {
        match self.stage {
            Stage::Delay | Stage::Finished => 0.0,
            Stage::Attack => {
                let attack = self.attack.max(1.0);
                (attack - self.frames_left.min(attack)) / attack
            }
            Stage::Hold | Stage::Decay | Stage::Sustain | Stage::Release => {
                attenuation_gain(self.attenuation)
            }
        }
    }

    fn advance(&mut self, mut frames: f64) {
        while frames > 0.0 {
            match self.stage {
                Stage::Delay | Stage::Attack | Stage::Hold => {
                    let step = frames.min(self.frames_left);
                    self.frames_left -= step;
                    frames -= step;
                }
                Stage::Decay => {
                    self.attenuation += frames * f64::from(DECAY_CENTIBELS) / self.decay.max(1.0);
                    frames = 0.0;
                }
                Stage::Release => {
                    self.attenuation += frames * f64::from(DECAY_CENTIBELS) / self.release.max(1.0);
                    frames = 0.0;
                }
                Stage::Sustain | Stage::Finished => frames = 0.0,
            }
            self.skip_finished_stages();
        }
    }

    fn release(&mut self) {
        match self.stage {
            Stage::Delay => self.stage = Stage::Finished,
            Stage::Attack => {
                self.attenuation = gain_attenuation(self.gain());
                self.stage = Stage::Release;
            }
            Stage::Hold | Stage::Decay | Stage::Sustain => self.stage = Stage::Release,
            Stage::Release | Stage::Finished => {}
        }
        self.skip_finished_stages();
    }
}

/// The f64 engine's state of a voice.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FloatSound {
    end: u32,
    loop_start: u32,
    loop_end: u32,
    mode: LoopMode,
    /// Position in sample points.
    position: f64,
    /// Step per frame.
    step: f64,
    /// The channel bend the step was computed for.
    step_bend: i32,
    /// The sample's rate over the output rate: the step at the root pitch.
    rate_ratio: f64,
    /// Pitch above the sample's root, in cents, without the channel's bend.
    pitch_cents: i32,
    /// Attenuation of the sample and the velocity, in centibels.
    attenuation: f64,
    /// Pan from the generators, -500 to 500.
    pan: i32,
    envelope: Envelope,
    /// Left and right gains reached so far, and the ramp under way: its end
    /// values, the step per frame and the frames left to it (zero: start a
    /// new ramp).
    gains: [f64; 2],
    targets: [f64; 2],
    deltas: [f64; 2],
    ramp_left: u32,
}

/// The step per frame of `ratio` played `cents` above the root: at most 64
/// points, as the integer engine's.
fn step(ratio: f64, cents: i32) -> f64 {
    (ratio * cents_ratio(f64::from(cents))).min(64.0)
}

impl FloatSound {
    /// The f64 playback of `setup` at `rate`.
    #[must_use]
    pub fn new(setup: &VoiceSetup, rate: Rate) -> Self {
        let rate_ratio = f64::from(setup.sample_rate) / rate_hz(rate);
        Self {
            end: setup.end,
            loop_start: setup.loop_start,
            loop_end: setup.loop_end,
            mode: setup.mode,
            position: f64::from(setup.begin),
            step: step(rate_ratio, setup.pitch_cents),
            step_bend: 0,
            rate_ratio,
            pitch_cents: setup.pitch_cents,
            // initialAttenuation counts 0.4 of its value, as in the integer
            // engine.
            attenuation: f64::from(setup.initial_attenuation) * 0.4
                + concave_centibels(setup.velocity),
            pan: setup.pan,
            envelope: Envelope::new(setup, rate),
            gains: [0.0; 2],
            targets: [0.0; 2],
            deltas: [0.0; 2],
            ramp_left: 0,
        }
    }

    pub(crate) const fn mode(&self) -> LoopMode {
        self.mode
    }

    pub(crate) fn release(&mut self) {
        self.envelope.release();
    }

    pub(crate) fn release_within(&mut self, frames: u32) {
        self.envelope.release = self.envelope.release.min(f64::from(frames));
        self.envelope.release();
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.envelope.stage == Stage::Finished
    }

    /// The louder gain in Q16, as the integer engine counts it.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a gain of at most 1 is at most 2^16 in Q16"
    )]
    pub(crate) fn loudness(&self) -> i32 {
        (self.gains[0].max(self.gains[1]) * 65_536.0) as i32
    }

    /// Start a gain ramp of `segment` frames, as the integer engine does.
    fn start_ramp(&mut self, channel: &Channel, segment: u32) {
        self.envelope.advance(f64::from(segment));
        let attenuation = self.attenuation
            + concave_centibels(channel.volume)
            + concave_centibels(channel.expression);
        let level = self.envelope.gain() * attenuation_gain(attenuation);
        let pan = pan_gains(self.pan + channel.pan_offset());
        let count = f64::from(segment.max(1));
        for side in 0..2 {
            self.targets[side] = level * pan[side];
            self.deltas[side] = (self.targets[side] - self.gains[side]) / count;
        }
        self.ramp_left = segment.max(1);
    }

    /// Add `frames` frames into `out`, with `channel`'s volume, pan and bend,
    /// `looping` as the voice says; the grid of blocks as the integer engine.
    pub(crate) fn render(
        &mut self,
        font: &SoundFont<'_>,
        channel: &Channel,
        looping: bool,
        out: &mut [[f64; 2]],
        frames: u32,
        until_boundary: u32,
    ) {
        if self.is_finished() {
            return;
        }
        let bend = channel.bend_cents();
        if bend != self.step_bend {
            self.step = step(self.rate_ratio, self.pitch_cents + bend);
            self.step_bend = bend;
        }
        if self.ramp_left == 0 {
            self.start_ramp(channel, until_boundary);
        }
        let step = self.step;
        let mut gains = self.gains;
        let mut position = self.position;
        let run_limit = if looping { self.loop_end } else { self.end };
        let frame_count = out.len().min(frames as usize);
        let mut remaining = &mut out[..frame_count];

        loop {
            if let Some(points) = font.points {
                let limit = run_limit.min(u32::try_from(points.len()).unwrap_or(u32::MAX));
                let run = run_frames(position, step, limit).min(remaining.len());
                if run > 0 {
                    let (run_out, rest) = core::mem::take(&mut remaining).split_at_mut(run);
                    remaining = rest;
                    let window = &points[..limit as usize];
                    position = mix_run(window, run_out, position, step, &mut gains, self.deltas);
                    if looping {
                        position = self.wrap(position);
                    }
                }
            }
            // One frame the slow way: at the loop's or the sample's end, or
            // on a bank whose points are not aligned.
            let Some((frame, rest)) = core::mem::take(&mut remaining).split_first_mut() else {
                break;
            };
            remaining = rest;
            let index = point_index(position);
            if !looping && index >= self.end {
                self.envelope.stage = Stage::Finished;
                break;
            }
            let next = if looping && index + 1 >= self.loop_end {
                self.loop_start
            } else {
                index + 1
            };
            let here = f64::from(font.sample_point(index));
            let after = if next < self.end {
                f64::from(font.sample_point(next))
            } else {
                0.0
            };
            let point = here + (after - here) * (position - f64::from(index));
            for side in 0..2 {
                gains[side] += self.deltas[side];
                frame[side] += point * gains[side];
            }
            position += step;
            if looping {
                position = self.wrap(position);
            }
        }

        self.position = position;
        self.ramp_left = self.ramp_left.saturating_sub(frames);
        self.gains = if self.ramp_left == 0 {
            self.targets
        } else {
            gains
        };
    }

    /// `position` brought back into the loop, if it ran past its end.
    fn wrap(&self, mut position: f64) -> f64 {
        let loop_end = f64::from(self.loop_end);
        let loop_length = f64::from(self.loop_end - self.loop_start);
        while position >= loop_end {
            position -= loop_length;
        }
        position
    }
}

/// The index of the point at or before `position` (not negative).
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "positions are within a sample's u32 points and never negative"
)]
fn point_index(position: f64) -> u32 {
    position as u32
}

/// Frames from `position`, `step` apart, whose point index `i` keeps
/// `i + 1 < limit` (unbounded when `step` is 0).
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a positive count of frames, clamped to usize"
)]
fn run_frames(position: f64, step: f64, limit: u32) -> usize {
    let Some(last) = limit.checked_sub(1) else {
        return 0;
    };
    let threshold = f64::from(last);
    if position >= threshold {
        return 0;
    }
    if step <= 0.0 {
        return usize::MAX;
    }
    // One frame short of the exact count: the last frame's position, computed
    // as `position + n * step`, may differ from this quotient in its last
    // bits, and the frame left over takes the slow path, which checks.
    let frames = libm::ceil((threshold - position) / step) - 1.0;
    if frames <= 0.0 {
        0
    } else if frames >= 1e15 {
        usize::MAX
    } else {
        frames as usize
    }
}

/// Frames [`mix_run`] interpolates at a time: a block of the synthesiser's
/// grid, which no run exceeds.
const CHUNK: usize = 64;

/// Add a run of frames from `points` to `out`, starting at `position`,
/// stepping the `gains` by `deltas` per frame; the position after it.
///
/// The caller sized the run by [`run_frames`], so `index + 1` stays within
/// `points` (the clamp only keeps the bounds check out of the loop).
///
/// Two passes per chunk of frames, for the vectorizer: the points are
/// interpolated into a buffer first (frame `i` at `position + i * step`),
/// then mixed with gains `gain + (i + 1) * delta`. Neither pass carries a
/// value from one frame to the next, so the mixing pass becomes two-lane f64
/// vector code over pairs of frames (`just _synth-fp-vectorized`).
#[inline(never)]
#[expect(
    clippy::cast_precision_loss,
    reason = "frame counts and indices below 2^32 are exact in f64"
)]
pub fn mix_run(
    points: &[i16],
    out: &mut [[f64; 2]],
    position: f64,
    step: f64,
    gains: &mut [f64; 2],
    deltas: [f64; 2],
) -> f64 {
    let Some(last_index) = points.len().checked_sub(2) else {
        return position;
    };
    let mut start = position;
    let [mut left_gain, mut right_gain] = *gains;
    let [left_delta, right_delta] = deltas;
    let mut interpolated = [0.0; CHUNK];
    for chunk in out.chunks_mut(CHUNK) {
        let interpolated = &mut interpolated[..chunk.len()];
        for (frame, point) in interpolated.iter_mut().enumerate() {
            let position = start + step * frame as f64;
            let index = (point_index(position) as usize).min(last_index);
            let here = f64::from(points[index]);
            let after = f64::from(points[index + 1]);
            *point = here + (after - here) * (position - index as f64);
        }
        for (frame, (out, &point)) in chunk.iter_mut().zip(interpolated.iter()).enumerate() {
            let steps = (frame + 1) as f64;
            out[0] += point * (left_gain + left_delta * steps);
            out[1] += point * (right_gain + right_delta * steps);
        }
        let count = chunk.len() as f64;
        left_gain += left_delta * count;
        right_gain += right_delta * count;
        start += step * count;
    }
    *gains = [left_gain, right_gain];
    start
}
