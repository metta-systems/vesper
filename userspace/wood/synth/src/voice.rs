//! One sounding sample: fixed-point playback with linear interpolation, the
//! SoundFont loop modes, the volume envelope, attenuation and pan.
//!
//! Positions are absolute sample-point indices into the bank's sample data in
//! 32.32 fixed point; gains are Q16 and ramp linearly across each block, so
//! envelope, volume and pan changes do not click.

use crate::{
    bank::{Zone, voice_value},
    channel::Channel,
    envelope::{Envelope, EnvelopeParameters},
    generator,
    sf2::{SampleHeader, SoundFont},
    tables::{
        Rate, SILENT_CENTIBELS, attenuation_gain, concave_centibels, pan_gains, phase_step,
        timecents_frames,
    },
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
    /// Gains reached at the end of the previous block, Q16.
    left_gain: i32,
    right_gain: i32,
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

    /// Add `frames` frames of this voice into `out` (interleaved stereo, at
    /// least `2 * frames` long), with `channel`'s volume, pan and bend.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "positions stay below 2^32 points, mixed values within i32"
    )]
    pub fn render(
        &mut self,
        font: &SoundFont<'_>,
        channel: &Channel,
        rate: Rate,
        out: &mut [i32],
        frames: u32,
    ) {
        if self.is_finished() {
            return;
        }
        let bend = channel.bend_cents();
        if bend != self.step_bend {
            self.step = phase_step(self.sample_rate, self.pitch_cents + bend, rate);
            self.step_bend = bend;
        }

        self.envelope.advance(frames);
        let attenuation = self.attenuation + channel.attenuation();
        let level = if attenuation >= SILENT_CENTIBELS {
            0
        } else {
            (u64::from(self.envelope.gain()) * u64::from(attenuation_gain(attenuation << 16))) >> 16
        };
        let (pan_left, pan_right) = pan_gains(self.pan + channel.pan_offset());
        let target_left = ((level * u64::from(pan_left)) >> 15) as i32;
        let target_right = ((level * u64::from(pan_right)) >> 15) as i32;
        let count = frames.max(1) as i32;
        let left_delta = (target_left - self.left_gain) / count;
        let right_delta = (target_right - self.right_gain) / count;
        let mut left_gain = self.left_gain;
        let mut right_gain = self.right_gain;

        for [left, right] in out.as_chunks_mut::<2>().0.iter_mut().take(frames as usize) {
            let index = (self.position >> 32) as u32;
            if !self.looping() && index >= self.end {
                // The sample ran out: the voice ends here.
                self.envelope.finish();
                break;
            }
            let next = if self.looping() && index + 1 >= self.loop_end {
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
            // 15 bits of fraction: the product of a 17-bit difference fits.
            let fraction = ((self.position >> 17) & 0x7fff) as i32;
            let point = here + (((after - here) * fraction) >> 15);
            left_gain += left_delta;
            right_gain += right_delta;
            *left += ((i64::from(point) * i64::from(left_gain)) >> 16) as i32;
            *right += ((i64::from(point) * i64::from(right_gain)) >> 16) as i32;

            self.position += self.step;
            if self.looping() {
                let loop_length = u64::from(self.loop_end - self.loop_start) << 32;
                while self.position >> 32 >= u64::from(self.loop_end) {
                    self.position -= loop_length;
                }
            }
        }
        self.left_gain = target_left;
        self.right_gain = target_right;
    }
}
