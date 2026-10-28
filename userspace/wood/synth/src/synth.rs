//! The synthesiser: sixteen MIDI channels playing a bank's presets on up to
//! [`POLYPHONY`] voices, rendered as interleaved stereo.

use {
    crate::{
        bank::{Bank, PERCUSSION_BANK},
        channel::Channel,
        tables::Rate,
        voice::{NoteStart, Voice},
    },
    core::array,
};

/// Voices that can sound at once. Doom's shareware tracks hold at most 15
/// notes; the rest is for release tails and layered zones.
pub const POLYPHONY: usize = 32;

/// MIDI channels.
pub const CHANNELS: usize = 16;

/// The General MIDI percussion channel (channel 10, numbered from 0).
pub const PERCUSSION_CHANNEL: u8 = 9;

/// Frames rendered per block: the envelope and gains step once per block.
pub const BLOCK_FRAMES: usize = 64;

/// Frames over which a voice cut by its exclusive class fades.
const CUT_FRAMES: u32 = 64;

/// Output gain before the master volume, as `rustysynth`'s default master
/// volume of 0.5: a right shift.
const MASTER_SHIFT: u32 = 1;

/// The synthesiser.
#[derive(Clone, Debug)]
pub struct Synth<'a> {
    bank: Bank<'a>,
    rate: Rate,
    channels: [Channel; CHANNELS],
    voices: [Option<Voice>; POLYPHONY],
    serial: u64,
    /// Master volume in Q16 (65 536 is full).
    master_volume: u32,
}

impl<'a> Synth<'a> {
    /// A synthesiser playing `bank` at `rate`, channels at power-on.
    #[must_use]
    pub fn new(bank: Bank<'a>, rate: Rate) -> Self {
        let mut synth = Self {
            bank,
            rate,
            channels: [Channel::new(); CHANNELS],
            voices: [None; POLYPHONY],
            serial: 0,
            master_volume: 1 << 16,
        };
        synth.reset();
        synth
    }

    /// The bank.
    #[must_use]
    pub const fn bank(&self) -> &Bank<'a> {
        &self.bank
    }

    /// The output rate.
    #[must_use]
    pub const fn rate(&self) -> Rate {
        self.rate
    }

    /// Channel `channel`'s state.
    #[must_use]
    pub fn channel(&self, channel: u8) -> &Channel {
        &self.channels[usize::from(channel) % CHANNELS]
    }

    /// Voices sounding.
    #[must_use]
    pub fn active_voices(&self) -> usize {
        self.voices.iter().flatten().count()
    }

    /// Set the master volume, 0 to 127 (as Doom's music volume), linear.
    pub fn set_volume(&mut self, volume: u8) {
        self.master_volume = (u32::from(volume.min(127)) << 16) / 127;
    }

    /// Silence every voice and put every channel back to power-on.
    pub fn reset(&mut self) {
        self.voices = [None; POLYPHONY];
        self.channels = array::from_fn(|_| Channel::new());
        for channel in 0..CHANNELS {
            self.program_change(u8::try_from(channel).unwrap_or(0), 0);
        }
    }

    /// Release every note (the sequencer's loop and stop).
    pub fn release_all(&mut self) {
        for voice in self.voices.iter_mut().flatten() {
            voice.release();
        }
    }

    /// Handle one MIDI channel message: `status` with its data bytes
    /// (`data2` is ignored by messages of one data byte).
    pub fn message(&mut self, status: u8, data1: u8, data2: u8) {
        let channel = status & 0x0f;
        let (data1, data2) = (data1 & 0x7f, data2 & 0x7f);
        match status & 0xf0 {
            0x80 => self.note_off(channel, data1),
            0x90 if data2 == 0 => self.note_off(channel, data1),
            0x90 => self.note_on(channel, data1, data2),
            0xb0 => self.controller(channel, data1, data2),
            0xc0 => self.program_change(channel, data1),
            0xe0 => {
                let bend = (i16::from(data2) << 7 | i16::from(data1)) - 8192;
                self.channels[usize::from(channel)].bend = bend;
            }
            // Aftertouch and system messages change nothing here.
            _ => {}
        }
    }

    fn program_change(&mut self, channel: u8, program: u8) {
        let state = &mut self.channels[usize::from(channel)];
        let bank = if channel == PERCUSSION_CHANNEL {
            PERCUSSION_BANK
        } else {
            u16::from(state.bank_select)
        };
        state.program = program;
        state.preset = self.bank.find_preset(bank, u16::from(program));
    }

    fn controller(&mut self, channel: u8, controller: u8, value: u8) {
        let state = &mut self.channels[usize::from(channel)];
        match controller {
            0 => state.bank_select = value,
            6 => state.data_entry(false, value),
            38 => state.data_entry(true, value),
            7 => state.volume = value,
            10 => state.pan = value,
            11 => state.expression = value,
            64 => {
                state.sustain = value >= 64;
                if !state.sustain {
                    for voice in self.voices.iter_mut().flatten() {
                        if voice.channel == channel && voice.held_by_pedal {
                            voice.release();
                        }
                    }
                }
            }
            98 | 99 => state.select_non_registered(),
            100 | 101 => state.select_parameter(controller, value),
            120 => {
                for slot in &mut self.voices {
                    if slot.is_some_and(|voice| voice.channel == channel) {
                        *slot = None;
                    }
                }
            }
            121 => state.reset_controllers(),
            // All notes off, and the mode messages that imply it.
            123..=127 => {
                let pedal = state.sustain;
                for voice in self.voices.iter_mut().flatten() {
                    if voice.channel == channel && !voice.released {
                        if pedal {
                            voice.held_by_pedal = true;
                        } else {
                            voice.release();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn note_off(&mut self, channel: u8, key: u8) {
        let pedal = self.channels[usize::from(channel)].sustain;
        for voice in self.voices.iter_mut().flatten() {
            if voice.channel == channel && voice.key == key && !voice.released {
                if pedal {
                    voice.held_by_pedal = true;
                } else {
                    voice.release();
                }
            }
        }
    }

    fn note_on(&mut self, channel: u8, key: u8, velocity: u8) {
        let Some(preset) = self.channels[usize::from(channel)].preset else {
            return;
        };
        let Self {
            bank,
            rate,
            voices,
            serial,
            ..
        } = self;
        let points = u32::try_from(bank.font().sample_points()).unwrap_or(u32::MAX);
        bank.regions(
            preset,
            key,
            velocity,
            &mut |preset_zone, instrument_zone, sample| {
                let start = NoteStart {
                    channel,
                    key,
                    velocity,
                    preset_zone,
                    instrument_zone,
                    sample,
                };
                *serial += 1;
                let voice = Voice::new(&start, *rate, points, *serial);
                if voice.exclusive_class != 0 {
                    for other in voices.iter_mut().flatten() {
                        if other.channel == channel
                            && other.exclusive_class == voice.exclusive_class
                        {
                            other.cut(CUT_FRAMES);
                        }
                    }
                }
                let slot = free_or_stolen(voices);
                voices[slot] = Some(voice);
            },
        );
    }

    /// Render `out.len() / 2` frames and add them (interleaved stereo, master
    /// volume applied, not clipped) to `out`.
    pub fn mix(&mut self, out: &mut [i32]) {
        let mut block = [0_i32; 2 * BLOCK_FRAMES];
        for chunk in out.chunks_mut(2 * BLOCK_FRAMES) {
            let frames = chunk.len() / 2;
            let block = &mut block[..2 * frames];
            block.fill(0);
            self.render_voices(block);
            for (sample, &rendered) in chunk.iter_mut().zip(block.iter()) {
                *sample += self.scale(rendered);
            }
        }
    }

    /// Render `out.len() / 2` frames into `out` (interleaved stereo s16).
    pub fn render(&mut self, out: &mut [i16]) {
        let mut block = [0_i32; 2 * BLOCK_FRAMES];
        for chunk in out.chunks_mut(2 * BLOCK_FRAMES) {
            let frames = chunk.len() / 2;
            let block = &mut block[..2 * frames];
            block.fill(0);
            self.render_voices(block);
            for (sample, &rendered) in chunk.iter_mut().zip(block.iter()) {
                *sample = clip(self.scale(rendered));
            }
        }
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "a scaled-down i32 fits i32"
    )]
    fn scale(&self, rendered: i32) -> i32 {
        ((i64::from(rendered) * i64::from(self.master_volume)) >> (16 + MASTER_SHIFT)) as i32
    }

    /// Render every voice into `block` (at most [`BLOCK_FRAMES`] frames) and
    /// free the voices that finished.
    fn render_voices(&mut self, block: &mut [i32]) {
        let frames = u32::try_from(block.len() / 2).unwrap_or(0);
        for slot in &mut self.voices {
            if let Some(voice) = slot {
                let channel = &self.channels[usize::from(voice.channel) % CHANNELS];
                voice.render(self.bank.font(), channel, self.rate, block, frames);
                if voice.is_finished() {
                    *slot = None;
                }
            }
        }
    }
}

/// A free voice slot, or the one to steal: released before held, then the
/// quietest, then the oldest.
fn free_or_stolen(voices: &[Option<Voice>; POLYPHONY]) -> usize {
    voices
        .iter()
        .position(Option::is_none)
        .or_else(|| {
            voices
                .iter()
                .enumerate()
                .filter_map(|(index, slot)| slot.map(|voice| (index, voice)))
                .min_by_key(|(_, voice)| (!voice.released, voice.loudness(), voice.serial))
                .map(|(index, _)| index)
        })
        .unwrap_or(0)
}

/// Clip to s16.
#[expect(clippy::cast_possible_truncation, reason = "clamped to the i16 range")]
const fn clip(sample: i32) -> i16 {
    (if sample > i16::MAX as i32 {
        i16::MAX as i32
    } else if sample < i16::MIN as i32 {
        i16::MIN as i32
    } else {
        sample
    }) as i16
}
