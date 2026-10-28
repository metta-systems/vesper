//! The volume envelope (SoundFont 2.04 section 8.1.2, generators 33 to 40):
//! delay, attack, hold, decay, sustain and release, in integer arithmetic.
//!
//! The attack rises linearly in amplitude; decay and release fall linearly in
//! centibels, [`DECAY_CENTIBELS`] over the stage's time (the slope the host's
//! `rustysynth` uses: `exp(-9.226 t / time)`, 80 dB per time). The level is
//! kept as an attenuation in Q16 centibels and stepped once per block of
//! frames; the voice ramps its gain across each block.

use crate::tables::{SILENT_CENTIBELS, attenuation_gain, gain_attenuation};

/// Centibels a decay or release falls over its time (80 dB).
pub const DECAY_CENTIBELS: u32 = 800;

/// Stage of the envelope.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// Silent before the attack.
    Delay,
    /// Rising linearly from silence to full level.
    Attack,
    /// At full level.
    Hold,
    /// Falling to the sustain level.
    Decay,
    /// At the sustain level until the note is released.
    Sustain,
    /// Falling to silence after the note was released.
    Release,
    /// Silent for good: the voice is done.
    Finished,
}

/// Durations and levels of an envelope, from the voice's generators.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EnvelopeParameters {
    /// Delay in frames.
    pub delay: u32,
    /// Attack in frames.
    pub attack: u32,
    /// Hold in frames.
    pub hold: u32,
    /// Decay time in frames (for the full [`DECAY_CENTIBELS`]).
    pub decay: u32,
    /// Sustain level as attenuation in centibels (0 to 960).
    pub sustain: u32,
    /// Release time in frames (for the full [`DECAY_CENTIBELS`]).
    pub release: u32,
}

/// A running volume envelope.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Envelope {
    parameters: EnvelopeParameters,
    stage: Stage,
    /// Frames left in a timed stage (delay, attack, hold).
    frames_left: u32,
    /// Attenuation in Q16 centibels (decay, sustain, release).
    attenuation: u32,
}

/// Q16 centibels lost per frame for a stage that falls [`DECAY_CENTIBELS`]
/// over `frames`.
fn slope(frames: u32) -> u32 {
    (DECAY_CENTIBELS << 16) / frames.max(1)
}

impl Envelope {
    /// An envelope at the start of its delay.
    #[must_use]
    pub const fn new(parameters: EnvelopeParameters) -> Self {
        let parameters = EnvelopeParameters {
            sustain: if parameters.sustain < SILENT_CENTIBELS {
                parameters.sustain
            } else {
                SILENT_CENTIBELS
            },
            ..parameters
        };
        let mut envelope = Self {
            parameters,
            stage: Stage::Delay,
            frames_left: parameters.delay,
            attenuation: 0,
        };
        envelope.skip_finished_stages();
        envelope
    }

    /// The current stage.
    #[must_use]
    pub const fn stage(&self) -> Stage {
        self.stage
    }

    /// Move past timed stages with no frames left.
    const fn skip_finished_stages(&mut self) {
        loop {
            match self.stage {
                Stage::Delay if self.frames_left == 0 => {
                    self.stage = Stage::Attack;
                    self.frames_left = self.parameters.attack;
                }
                Stage::Attack if self.frames_left == 0 => {
                    self.stage = Stage::Hold;
                    self.frames_left = self.parameters.hold;
                    self.attenuation = 0;
                }
                Stage::Hold if self.frames_left == 0 => self.stage = Stage::Decay,
                Stage::Decay if self.attenuation >= self.parameters.sustain << 16 => {
                    self.attenuation = self.parameters.sustain << 16;
                    self.stage = if self.parameters.sustain >= SILENT_CENTIBELS {
                        Stage::Finished
                    } else {
                        Stage::Sustain
                    };
                }
                Stage::Release if self.attenuation >= SILENT_CENTIBELS << 16 => {
                    self.stage = Stage::Finished;
                }
                _ => return,
            }
        }
    }

    /// The envelope's level as linear gain in Q16.
    #[must_use]
    pub fn gain(&self) -> u32 {
        match self.stage {
            Stage::Delay | Stage::Finished => 0,
            Stage::Attack => {
                let attack = self.parameters.attack.max(1);
                let done = attack - self.frames_left.min(attack);
                u32::try_from((u64::from(done) << 16) / u64::from(attack)).unwrap_or(0)
            }
            Stage::Hold | Stage::Decay | Stage::Sustain | Stage::Release => {
                attenuation_gain(self.attenuation)
            }
        }
    }

    /// Advance the envelope by `frames`.
    pub fn advance(&mut self, mut frames: u32) {
        while frames > 0 {
            match self.stage {
                Stage::Delay | Stage::Attack | Stage::Hold => {
                    let step = frames.min(self.frames_left);
                    self.frames_left -= step;
                    frames -= step;
                }
                Stage::Decay => {
                    let falling = slope(self.parameters.decay).saturating_mul(frames);
                    self.attenuation = self.attenuation.saturating_add(falling);
                    frames = 0;
                }
                Stage::Release => {
                    let falling = slope(self.parameters.release).saturating_mul(frames);
                    self.attenuation = self.attenuation.saturating_add(falling);
                    frames = 0;
                }
                Stage::Sustain | Stage::Finished => frames = 0,
            }
            self.skip_finished_stages();
        }
    }

    /// Start the release from the current level.
    pub fn release(&mut self) {
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

    /// Release quickly (an exclusive class cut, a stolen voice): over
    /// `frames` for the full fall.
    pub fn release_within(&mut self, frames: u32) {
        self.parameters.release = self.parameters.release.min(frames);
        self.release();
    }

    /// Stop at once (the sample ran out).
    pub const fn finish(&mut self) {
        self.stage = Stage::Finished;
    }

    /// Whether the envelope has finished.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.stage == Stage::Finished
    }
}
