//! One sounding sample: fixed-point playback with linear interpolation, the
//! SoundFont loop modes, the volume envelope, attenuation and pan.
//!
//! Positions are absolute sample-point indices into the bank's sample data in
//! 32.32 fixed point; gains are Q16 and ramp linearly across each block, so
//! envelope, volume and pan changes do not click.

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

/// A voice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

/// Generator value `operator` of the start's zones.
fn value(start: &NoteStart<'_>, operator: u16) -> i32 {
    voice_value(start.preset_zone, start.instrument_zone, operator)
}

/// A sample address: the header's plus the fine and coarse offsets.
fn address(base: u32, fine: i32, coarse: i32) -> u32 {
    let address = i64::from(base) + i64::from(fine) + 32_768 * i64::from(coarse);
    u32::try_from(address.max(0)).unwrap_or(u32::MAX)
}

impl Voice {
    /// Start a voice for `start` at `rate`, numbered `serial`.
    #[must_use]
    pub fn new(start: &NoteStart<'_>, rate: Rate, points: u32, serial: u64) -> Self {
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

        // initialAttenuation counts 0.4 of its value, as in EMU hardware and
        // in rustysynth and FluidSynth.
        let initial =
            u32::try_from(value(start, generator::INITIAL_ATTENUATION).clamp(0, 1440)).unwrap_or(0);
        let attenuation = initial * 2 / 5 + concave_centibels(velocity);

        let frames = |operator| timecents_frames(value(start, operator), rate);
        let key_scaled = |operator, scale| {
            timecents_frames(
                value(start, operator) + (60 - key) * value(start, scale),
                rate,
            )
        };
        let envelope = Envelope::new(EnvelopeParameters {
            delay: frames(generator::DELAY_VOL_ENV),
            attack: frames(generator::ATTACK_VOL_ENV),
            hold: key_scaled(generator::HOLD_VOL_ENV, generator::KEYNUM_TO_VOL_ENV_HOLD),
            decay: key_scaled(generator::DECAY_VOL_ENV, generator::KEYNUM_TO_VOL_ENV_DECAY),
            sustain: u32::try_from(value(start, generator::SUSTAIN_VOL_ENV).clamp(0, 1440))
                .unwrap_or(0),
            release: frames(generator::RELEASE_VOL_ENV),
        });

        Self {
            channel: start.channel,
            key: start.key,
            exclusive_class: value(start, generator::EXCLUSIVE_CLASS),
            serial,
            released: false,
            held_by_pedal: false,
            end,
            loop_start,
            loop_end,
            mode,
            position: u64::from(begin) << 32,
            step: phase_step(sample.sample_rate, pitch_cents, rate),
            step_bend: 0,
            sample_rate: sample.sample_rate,
            pitch_cents,
            attenuation,
            pan: value(start, generator::PAN),
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

    /// Release the note (note-off, or the pedal let go).
    pub fn release(&mut self) {
        self.released = true;
        self.held_by_pedal = false;
        self.envelope.release();
    }

    /// Release within `frames` (exclusive class, stealing).
    pub fn cut(&mut self, frames: u32) {
        self.released = true;
        self.held_by_pedal = false;
        self.envelope.release_within(frames);
    }

    /// Whether the voice has nothing more to play.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.envelope.is_finished()
    }

    /// Current loudness, Q16 (for choosing a voice to steal).
    #[must_use]
    pub fn loudness(&self) -> i32 {
        self.left_gain.max(self.right_gain)
    }

    fn looping(&self) -> bool {
        match self.mode {
            LoopMode::None => false,
            LoopMode::Continuous => true,
            LoopMode::UntilRelease => !self.released,
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

    /// Add `frames` frames of this voice into `out` (interleaved stereo, at
    /// least `2 * frames` long), with `channel`'s volume, pan and bend.
    ///
    /// The envelope and the gains move on a grid of blocks the synthesiser
    /// keeps from its first frame: `until_boundary` frames are left to the
    /// next grid point (at least `frames`). A ramp starts at a grid point (or
    /// at the voice's first frame) and runs to the next, however the frames
    /// are split into calls, so the output does not depend on the split.
    pub fn render(
        &mut self,
        font: &SoundFont<'_>,
        channel: &Channel,
        rate: Rate,
        out: &mut [i32],
        frames: u32,
        until_boundary: u32,
    ) {
        if self.is_finished() {
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
        // `released` changes only between calls, so the loop state holds for
        // the whole call.
        let looping = self.looping();
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
/// end or wrap check: the frames are those of [`Voice::render`]'s slow path,
/// bit for bit.
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
