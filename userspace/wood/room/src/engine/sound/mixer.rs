//! A sound-effect mixer for platforms without one of their own.
//!
//! The host platform hands each sound to `rodio`; a platform that only has
//! a PCM output (Vesper's `audio_v1`) mixes here instead. Two sides share
//! [`SharedVoices`]:
//!
//! - the game Thread, through the [`Platform`](crate::engine::platform::Platform)
//!   sound methods, only changes voice state: [`SharedVoices::start`],
//!   [`SharedVoices::stop`], [`SharedVoices::update`] and
//!   [`SharedVoices::is_playing`] store and load atomics and never wait;
//! - the playback Thread owns a [`Mixer`] and fills each output buffer with
//!   [`Mixer::mix_shared`], which picks up the game's changes, mixes, and
//!   reports the voices that ran out.
//!
//! What is mixed: [`VOICES`] voices, one per SFX channel of the engine, each
//! a DMX sound lump (8-bit unsigned mono PCM centred on 128, usually at
//! 11 025 Hz; see [`dmx_samples`]) stepped through in 32.32 fixed point at
//! the lump's rate over the output rate, with linear interpolation between
//! neighbouring samples. Each voice has the gains of Doom's volume and
//! separation ([`gains`]), and the voices are summed in 32 bits and clipped
//! to signed 16-bit stereo frames `[left, right]`.
//!
//! The lumps the engine hands over live in its zone cache and may be purged
//! once the call returns, so [`SharedVoices`] keeps a copy of each distinct
//! sound in a [`SoundBank`] for good: the copies are what the playback
//! Thread reads, and they are never freed (a WAD has a hundred or so sound
//! lumps; the shareware IWAD's come to 535 kB).

use {
    alloc::boxed::Box,
    core::{
        ptr,
        sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    },
};

/// Voices the mixer plays at once: the engine's eight SFX channels.
pub const VOICES: usize = 8;

/// Distinct sounds a [`SoundBank`] holds: more than Doom's 109 sound
/// effects, which a WAD's lumps cannot outnumber.
pub const BANK_SOUNDS: usize = 128;

/// Bytes of a DMX sound lump's header: format, rate and sample count.
const DMX_HEADER_BYTES: usize = 8;

/// The value of an unsigned 8-bit sample at the centre line.
const SAMPLE_CENTRE: i32 = 128;

/// Fractional bits of a voice's position and step.
const POSITION_BITS: u32 = 32;

/// Fractional bits of a gain: [`UNITY_GAIN`] passes a sample unchanged.
const GAIN_BITS: u32 = 16;

/// The gain of a voice at full volume on one side.
pub const UNITY_GAIN: i32 = 1 << GAIN_BITS;

/// The engine's loudest volume.
pub const MAX_VOLUME: i32 = 127;

/// The widest separation the pan law takes: 0 is hard left, 254 hard right.
pub const MAX_SEPARATION: i32 = 254;

/// The separation the engine gives a centred sound: `NORM_SEP` of
/// `s_sound.c`, the centre of its `128 - swing · sin(angle)`.
///
/// The pan law ([`gains`]) is Chocolate Doom's (`i_sdlsound.c`: left
/// `(254 - sep) · vol / 127`, right `sep · vol / 127`), as the host's
/// `gains_from` is, and is symmetric about 127, so a centred sound plays
/// one step right of even: gains 126 : 128, 0.14 dB apart. Vanilla Doom
/// does the same in its own way: linuxdoom's `i_sound.c` mixer is symmetric
/// about 127.5 and plays separation 128 at 0.746 : 0.75 of the volume. The
/// mixer keeps the law as it is rather than shifting it by a step: the
/// difference cannot be heard, and the host and Vesper builds pan alike.
pub const CENTRE_SEPARATION: i32 = 128;

/// The sample rate and the samples of the DMX sound lump `lump`, or `None`
/// if it is malformed.
///
/// | Offset | Bytes | Meaning                                  |
/// |--------|-------|------------------------------------------|
/// | 0      | 2     | Format (`3`), not checked                |
/// | 2      | 2     | Sample rate in Hz, little-endian         |
/// | 4      | 4     | Sample count, little-endian              |
/// | 8      | count | Unsigned 8-bit mono samples, centre 128  |
///
/// Malformed is what the host's `decode_doom_sfx` refuses: a lump shorter
/// than its header or its samples, no samples, or a rate of 0. Like the
/// host, it plays every sample, including the 16 bytes at each end that
/// DMX (and Chocolate Doom) skip.
pub fn dmx_samples(lump: &[u8]) -> Option<(u32, &[u8])> {
    let header = lump.get(..DMX_HEADER_BYTES)?;
    let rate_hz = u32::from(u16::from_le_bytes([header[2], header[3]]));
    let count = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    let count = usize::try_from(count).ok()?;
    let samples = lump.get(DMX_HEADER_BYTES..DMX_HEADER_BYTES.checked_add(count)?)?;
    (rate_hz != 0 && count != 0).then_some((rate_hz, samples))
}

/// The gains `[left, right]` of a voice at volume `volume` (`0..=127`) and
/// separation `separation` (`0..=254`, 0 hard left), in units of
/// [`UNITY_GAIN`]: the host's `gains_from` in fixed point, `left =
/// volume · (254 - separation) / (127 · 254)`, `right = volume · separation
/// / (127 · 254)`. Values outside the ranges are clamped into them. See
/// [`CENTRE_SEPARATION`] for where the centre lies.
#[must_use]
pub fn gains(volume: i32, separation: i32) -> [i32; 2] {
    let volume = i64::from(volume.clamp(0, MAX_VOLUME));
    let separation = i64::from(separation.clamp(0, MAX_SEPARATION));
    let scale = i64::from(MAX_VOLUME) * i64::from(MAX_SEPARATION);
    let gain = |share: i64| -> i32 {
        // At most `UNITY_GAIN`.
        i32::try_from(((volume * share) << GAIN_BITS) / scale).unwrap_or(UNITY_GAIN)
    };
    [
        gain(i64::from(MAX_SEPARATION) - separation),
        gain(separation),
    ]
}

/// How far a voice moves through a sound at `rate_hz` for each frame of an
/// output at `output_millihertz`: the ratio of the rates in 32.32 fixed
/// point. A rate of 0 never moves.
#[must_use]
pub fn step(rate_hz: u32, output_millihertz: u32) -> u64 {
    if output_millihertz == 0 {
        return 0;
    }
    (u64::from(rate_hz) << POSITION_BITS) * 1000 / u64::from(output_millihertz)
}

/// A sound effect as the mixer plays it: a DMX lump's rate and samples.
#[derive(Debug, PartialEq, Eq)]
pub struct Sound {
    rate_hz: u32,
    samples: Box<[u8]>,
}

impl Sound {
    /// A copy of the DMX sound lump `lump`, or `None` if it is malformed
    /// ([`dmx_samples`]).
    pub fn decode(lump: &[u8]) -> Option<Self> {
        dmx_samples(lump).map(|(rate_hz, samples)| Self {
            rate_hz,
            samples: samples.into(),
        })
    }

    /// The sound's sample rate in Hz.
    pub const fn rate_hz(&self) -> u32 {
        self.rate_hz
    }

    /// The sound's unsigned 8-bit samples.
    pub fn samples(&self) -> &[u8] {
        &self.samples
    }

    /// Whether this is the sound of `samples` at `rate_hz`.
    fn is(&self, rate_hz: u32, samples: &[u8]) -> bool {
        self.rate_hz == rate_hz && *self.samples == *samples
    }
}

/// The distinct sounds started so far, each copied once and kept for good,
/// so the playback Thread may read them while the engine purges its lumps.
///
/// Only one Thread adds sounds (the engine's); any Thread may read them.
pub struct SoundBank {
    sounds: [AtomicPtr<Sound>; BANK_SOUNDS],
    count: AtomicUsize,
}

impl SoundBank {
    /// An empty bank.
    pub const fn new() -> Self {
        Self {
            sounds: [const { AtomicPtr::new(ptr::null_mut()) }; BANK_SOUNDS],
            count: AtomicUsize::new(0),
        }
    }

    /// Sounds in the bank.
    pub fn len(&self) -> usize {
        self.count.load(Ordering::Acquire)
    }

    /// Whether the bank holds no sound.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The sound at `index`, once it is in the bank.
    pub fn get(&self, index: usize) -> Option<&'static Sound> {
        let sound = self.sounds.get(index)?.load(Ordering::Acquire);
        // SAFETY: a non-null slot holds a `Sound` leaked by `add`, which is
        // never freed or changed.
        unsafe { sound.as_ref() }
    }

    /// The index of the DMX sound lump `lump` in the bank, adding a copy of
    /// it if it is new; `None` if it is malformed or the bank is full.
    pub fn add(&self, lump: &[u8]) -> Option<usize> {
        let (rate_hz, samples) = dmx_samples(lump)?;
        let count = self.len();
        if let Some(index) = (0..count).find(|&index| {
            self.get(index)
                .is_some_and(|sound| sound.is(rate_hz, samples))
        }) {
            return Some(index);
        }
        if count == BANK_SOUNDS {
            return None;
        }
        let sound = Box::leak(Box::new(Sound {
            rate_hz,
            samples: samples.into(),
        }));
        self.sounds[count].store(sound, Ordering::Release);
        self.count.store(count + 1, Ordering::Release);
        Some(count)
    }
}

impl Default for SoundBank {
    fn default() -> Self {
        Self::new()
    }
}

/// One SFX channel as the game Thread leaves it for the playback Thread.
struct Channel {
    /// The latest command: its sequence number in the high half; in the low
    /// half the index in the bank plus one of the sound it starts, or 0 for
    /// a stop. Only the game Thread stores it.
    command: AtomicU64,
    /// Volume in the high half, separation in the low half, both clamped.
    /// Only the game Thread stores it.
    levels: AtomicU32,
    /// The sequence number of the latest command the playback Thread has
    /// finished with: its sound ran out, or it stopped one. Only the
    /// playback Thread stores it.
    finished: AtomicU32,
}

impl Channel {
    const fn new() -> Self {
        Self {
            command: AtomicU64::new(0),
            levels: AtomicU32::new(0),
            finished: AtomicU32::new(0),
        }
    }
}

/// The sequence number of `command`.
const fn sequence(command: u64) -> u32 {
    (command >> 32) as u32
}

/// The index in the bank plus one of the sound `command` starts; 0 stops.
const fn sound_slot(command: u64) -> u32 {
    command as u32
}

/// Volume and separation, clamped, as [`Channel::levels`] holds them.
fn pack_levels(volume: i32, separation: i32) -> u32 {
    let volume = volume.clamp(0, MAX_VOLUME).unsigned_abs();
    let separation = separation.clamp(0, MAX_SEPARATION).unsigned_abs();
    (volume << 16) | separation
}

/// Volume and separation from [`Channel::levels`].
fn unpack_levels(levels: u32) -> (i32, i32) {
    let volume = i32::try_from(levels >> 16).unwrap_or(0);
    let separation = i32::try_from(levels & 0xffff).unwrap_or(0);
    (volume, separation)
}

/// The voices as the game Thread changes them and the playback Thread plays
/// them, without a lock: each channel is a command word, a levels word and
/// a "finished" word, and the sounds are in a [`SoundBank`].
///
/// The game Thread stores the levels and then the command (release); the
/// playback Thread loads the command (acquire) and then the levels, so a
/// sound never starts at the levels of the one before it. A volume or
/// separation change alone reaches the playback Thread at its next buffer.
pub struct SharedVoices {
    channels: [Channel; VOICES],
    bank: SoundBank,
}

impl SharedVoices {
    /// Every channel silent, the bank empty.
    pub const fn new() -> Self {
        Self {
            channels: [const { Channel::new() }; VOICES],
            bank: SoundBank::new(),
        }
    }

    /// The bank of the sounds started so far.
    pub const fn bank(&self) -> &SoundBank {
        &self.bank
    }

    /// Store a new command on `channel`, after its levels.
    fn command(&self, channel: &Channel, slot: u32) {
        let next = sequence(channel.command.load(Ordering::Relaxed)).wrapping_add(1);
        channel
            .command
            .store((u64::from(next) << 32) | u64::from(slot), Ordering::Release);
    }

    /// Start the DMX sound lump `lump` on `channel` (`0..VOICES`) at
    /// `volume` and `separation`, replacing what played there; `false` if the
    /// channel does not exist, the lump is malformed or the bank is full.
    /// Game Thread only.
    pub fn start(&self, channel: usize, lump: &[u8], volume: i32, separation: i32) -> bool {
        let Some(control) = self.channels.get(channel) else {
            return false;
        };
        let Some(index) = self.bank.add(lump) else {
            return false;
        };
        let Ok(slot) = u32::try_from(index + 1) else {
            return false;
        };
        control
            .levels
            .store(pack_levels(volume, separation), Ordering::Relaxed);
        self.command(control, slot);
        true
    }

    /// Stop the sound on `channel`. Game Thread only.
    pub fn stop(&self, channel: usize) {
        if let Some(control) = self.channels.get(channel) {
            self.command(control, 0);
        }
    }

    /// Change the volume and separation of the sound on `channel`. Game
    /// Thread only.
    pub fn update(&self, channel: usize, volume: i32, separation: i32) {
        if let Some(control) = self.channels.get(channel) {
            control
                .levels
                .store(pack_levels(volume, separation), Ordering::Relaxed);
        }
    }

    /// Whether the sound last started on `channel` is still playing: not
    /// stopped, and the playback Thread has not reported it ran out.
    pub fn is_playing(&self, channel: usize) -> bool {
        self.channels.get(channel).is_some_and(|control| {
            let command = control.command.load(Ordering::Relaxed);
            sound_slot(command) != 0
                && control.finished.load(Ordering::Acquire) != sequence(command)
        })
    }
}

impl Default for SharedVoices {
    fn default() -> Self {
        Self::new()
    }
}

/// One voice of the mixer: a sound and where it is in it.
#[derive(Clone, Copy)]
struct Voice<'sounds> {
    /// The sound playing, until it runs out
    sound: Option<&'sounds Sound>,
    /// The position in the sound's samples, 32.32 fixed point
    position: u64,
    /// The position's advance per output frame, 32.32 fixed point
    step: u64,
    /// `[left, right]` in units of [`UNITY_GAIN`]
    gains: [i32; 2],
}

impl Voice<'_> {
    const SILENT: Self = Self {
        sound: None,
        position: 0,
        step: 0,
        gains: [0, 0],
    };

    /// The voice's next sample as a signed 16-bit value, interpolated
    /// between the two samples around its position; `None` once the sound
    /// ran out, which ends the voice.
    fn next_sample(&mut self) -> Option<i32> {
        let samples = self.sound?.samples();
        let index = usize::try_from(self.position >> POSITION_BITS).unwrap_or(usize::MAX);
        let Some(&current) = samples.get(index) else {
            self.sound = None;
            return None;
        };
        let signed = |sample: u8| (i32::from(sample) - SAMPLE_CENTRE) << 8;
        let current = signed(current);
        let next = samples
            .get(index + 1)
            .map_or(current, |&sample| signed(sample));
        // The top 16 bits of the position's fraction.
        let fraction = i64::from((self.position >> (POSITION_BITS - 16)) as u16);
        self.position = self.position.saturating_add(self.step);
        // Between `current` and `next`, so it fits.
        let between = (i64::from(next - current) * fraction) >> 16;
        Some(current + i32::try_from(between).unwrap_or(0))
    }
}

/// The playback Thread's mixer: [`VOICES`] voices at an output rate.
pub struct Mixer<'sounds> {
    voices: [Voice<'sounds>; VOICES],
    output_millihertz: u32,
    /// The sequence number of the command each voice last followed
    followed: [u32; VOICES],
}

impl<'sounds> Mixer<'sounds> {
    /// Silent voices for an output at `output_millihertz`.
    pub const fn new(output_millihertz: u32) -> Self {
        Self {
            voices: [Voice::SILENT; VOICES],
            output_millihertz,
            followed: [0; VOICES],
        }
    }

    /// Play `sound` on voice `voice` from its start at `volume` and
    /// `separation`, replacing what played there.
    pub fn start(&mut self, voice: usize, sound: &'sounds Sound, volume: i32, separation: i32) {
        if let Some(slot) = self.voices.get_mut(voice) {
            *slot = Voice {
                sound: Some(sound),
                position: 0,
                step: step(sound.rate_hz(), self.output_millihertz),
                gains: gains(volume, separation),
            };
        }
    }

    /// Silence voice `voice`.
    pub fn stop(&mut self, voice: usize) {
        if let Some(slot) = self.voices.get_mut(voice) {
            *slot = Voice::SILENT;
        }
    }

    /// Change the volume and separation of voice `voice`.
    pub fn set_levels(&mut self, voice: usize, volume: i32, separation: i32) {
        if let Some(slot) = self.voices.get_mut(voice) {
            slot.gains = gains(volume, separation);
        }
    }

    /// Whether voice `voice` still has samples to play.
    pub fn is_playing(&self, voice: usize) -> bool {
        self.voices
            .get(voice)
            .is_some_and(|slot| slot.sound.is_some())
    }

    /// Mix every voice into `frames`, replacing what they held: the sum of
    /// the voices' samples times their gains, clipped to 16 bits, `[left,
    /// right]`. Returns how many voices played in this buffer; a voice
    /// that runs out ends, and the rest of its frames are silent.
    pub fn mix(&mut self, frames: &mut [[i16; 2]]) -> usize {
        frames.fill([0, 0]);
        self.mix_onto(frames)
    }

    /// Mix every voice onto `frames`, adding to what they hold (music
    /// rendered elsewhere, for example): each frame becomes its own samples
    /// plus the voices' samples times their gains, clipped to 16 bits once.
    /// Returns how many voices played in this buffer, as [`mix`](Self::mix).
    ///
    /// The mix is voice-major: for each run of [`MIX_RUN`] frames, each
    /// playing voice adds its whole run into 32-bit sums, and the sums are
    /// clipped once after the last voice. The integer sums are those of a
    /// frame-by-frame mix, so the output is the same bit for bit. With no
    /// voice playing the frames are left as they are, which is what the mix
    /// would write back.
    pub fn mix_onto(&mut self, frames: &mut [[i16; 2]]) -> usize {
        let playing = self
            .voices
            .iter()
            .filter(|voice| voice.sound.is_some())
            .count();
        if playing == 0 {
            return 0;
        }
        let mut sums = [[0_i32; 2]; MIX_RUN];
        for run in frames.chunks_mut(MIX_RUN) {
            let sums = &mut sums[..run.len()];
            for (sum, frame) in sums.iter_mut().zip(run.iter()) {
                *sum = [i32::from(frame[0]), i32::from(frame[1])];
            }
            for voice in self.voices.iter_mut().filter(|voice| voice.sound.is_some()) {
                let [left_gain, right_gain] = voice.gains;
                for sum in sums.iter_mut() {
                    // A voice that runs out stays silent for the rest.
                    let Some(sample) = voice.next_sample() else {
                        break;
                    };
                    sum[0] += (sample * left_gain) >> GAIN_BITS;
                    sum[1] += (sample * right_gain) >> GAIN_BITS;
                }
            }
            for (frame, sum) in run.iter_mut().zip(sums.iter()) {
                *frame = [clip(sum[0]), clip(sum[1])];
            }
        }
        playing
    }

    /// Mix the voices of `shared` into `frames`: first follow the commands
    /// the game Thread stored since the last buffer (a start plays from the
    /// sound's beginning, a stop silences) and the current levels of every
    /// channel, then [`mix`](Self::mix), then report each voice that has
    /// nothing left to play as finished. Playback Thread only.
    pub fn mix_shared(&mut self, shared: &'sounds SharedVoices, frames: &mut [[i16; 2]]) -> usize {
        frames.fill([0, 0]);
        self.mix_shared_onto(shared, frames)
    }

    /// [`mix_shared`](Self::mix_shared), adding onto what `frames` hold as
    /// [`mix_onto`](Self::mix_onto) does: Wood's playback Thread mixes the
    /// effects on top of the music. Playback Thread only.
    pub fn mix_shared_onto(
        &mut self,
        shared: &'sounds SharedVoices,
        frames: &mut [[i16; 2]],
    ) -> usize {
        for (voice, channel) in shared.channels.iter().enumerate() {
            let command = channel.command.load(Ordering::Acquire);
            let (volume, separation) = unpack_levels(channel.levels.load(Ordering::Relaxed));
            if sequence(command) != self.followed[voice] {
                self.followed[voice] = sequence(command);
                let sound = sound_slot(command)
                    .checked_sub(1)
                    .and_then(|index| shared.bank.get(usize::try_from(index).ok()?));
                match sound {
                    Some(sound) => self.start(voice, sound, volume, separation),
                    None => self.stop(voice),
                }
            } else {
                self.set_levels(voice, volume, separation);
            }
        }
        let playing = self.mix_onto(frames);
        for (voice, channel) in shared.channels.iter().enumerate() {
            if !self.is_playing(voice) {
                channel
                    .finished
                    .store(self.followed[voice], Ordering::Release);
            }
        }
        playing
    }
}

/// Frames [`Mixer::mix_onto`] sums at a time, on the stack (512 bytes).
const MIX_RUN: usize = 64;

/// `sample` clipped to signed 16 bits.
fn clip(sample: i32) -> i16 {
    i16::try_from(sample).unwrap_or(if sample < 0 { i16::MIN } else { i16::MAX })
}

#[cfg(test)]
mod tests {
    use {super::*, alloc::vec::Vec};

    /// The rate `audio_v1` reports: 100 MHz / 2268, in millihertz.
    const OUTPUT_MILLIHERTZ: u32 = 44_091_710;

    /// A DMX lump of `samples` at `rate_hz`.
    fn lump(rate_hz: u16, samples: &[u8]) -> Vec<u8> {
        let mut lump = Vec::new();
        lump.extend_from_slice(&3_u16.to_le_bytes());
        lump.extend_from_slice(&rate_hz.to_le_bytes());
        lump.extend_from_slice(&u32::try_from(samples.len()).unwrap().to_le_bytes());
        lump.extend_from_slice(samples);
        lump
    }

    /// A sound of `count` samples of `value` at `rate_hz`.
    fn constant(rate_hz: u16, value: u8, count: usize) -> Sound {
        Sound::decode(&lump(rate_hz, &vec![value; count])).unwrap()
    }

    /// Frames mixed until every voice ran out (at most `limit`).
    fn frames_until_silent(mixer: &mut Mixer<'_>, limit: usize) -> usize {
        let mut frames = 0;
        let mut frame = [[0_i16; 2]; 1];
        while frames < limit && (0..VOICES).any(|voice| mixer.is_playing(voice)) {
            mixer.mix(&mut frame);
            if (0..VOICES).any(|voice| mixer.is_playing(voice)) {
                frames += 1;
            }
        }
        frames
    }

    #[test]
    fn dmx_header() {
        let three = lump(11025, &[128, 129, 130]);
        assert_eq!(dmx_samples(&three), Some((11025, &[128_u8, 129, 130][..])));
        // Extra bytes after the samples are not samples.
        let mut longer = lump(22050, &[1, 2]);
        longer.push(99);
        assert_eq!(dmx_samples(&longer), Some((22050, &[1_u8, 2][..])));
    }

    #[test]
    fn dmx_malformed() {
        assert_eq!(dmx_samples(&[3, 0, 0x11, 0x2b]), None, "short header");
        assert_eq!(dmx_samples(&lump(11025, &[])), None, "no samples");
        assert_eq!(dmx_samples(&lump(0, &[128])), None, "rate 0");
        let mut truncated = lump(11025, &[128, 128, 128]);
        truncated.pop();
        assert_eq!(dmx_samples(&truncated), None, "fewer samples than counted");
    }

    #[test]
    fn step_is_the_ratio_of_the_rates() {
        let quarter = step(11025, OUTPUT_MILLIHERTZ);
        // 11 025 / 44 091.71 = 0.250041...
        assert_eq!(quarter, (11_025_u64 << 32) * 1000 / 44_091_710);
        let ratio = quarter as f64 / (1_u64 << 32) as f64;
        assert!((ratio - 11025.0 / 44091.71).abs() < 1e-9, "{ratio}");
        assert_eq!(step(22050, OUTPUT_MILLIHERTZ), quarter * 2 + 1);
        assert_eq!(step(11025, 0), 0);
    }

    #[test]
    fn resampling_plays_a_sound_for_its_length_at_the_output_rate() {
        let sound = constant(11025, 200, 1000);
        let mut mixer = Mixer::new(OUTPUT_MILLIHERTZ);
        mixer.start(0, &sound, 127, 127);
        let frames = frames_until_silent(&mut mixer, 10_000);
        // Every output frame steps by the ratio until the position leaves
        // the last sample: ceil(1000 / ratio) frames.
        let expected = ((1000_u64 << 32) + step(11025, OUTPUT_MILLIHERTZ) - 1)
            / step(11025, OUTPUT_MILLIHERTZ);
        assert_eq!(frames as u64, expected);
        assert_eq!(expected, 4000, "1000 samples at 11 025 Hz are 4000 frames");

        let fast = constant(22050, 200, 1000);
        mixer.start(1, &fast, 127, 127);
        assert_eq!(frames_until_silent(&mut mixer, 10_000), 2000);
    }

    #[test]
    fn interpolates_between_samples() {
        // 128 is 0, 192 is 64 << 8 = 16384.
        let sound = Sound::decode(&lump(11025, &[128, 192])).unwrap();
        let mut mixer = Mixer::new(44_100_000);
        mixer.start(0, &sound, 127, 0);
        let mut frames = [[0_i16; 2]; 8];
        mixer.mix(&mut frames);
        let left: Vec<i16> = frames.iter().map(|frame| frame[0]).collect();
        // A quarter step: 0, a quarter, a half, three quarters of the way,
        // then the last sample held, then silence.
        assert_eq!(left, [0, 4096, 8192, 12288, 16384, 16384, 16384, 16384]);
        assert!(frames.iter().all(|frame| frame[1] == 0), "hard left");
        let mut after = [[1_i16; 2]; 1];
        mixer.mix(&mut after);
        assert_eq!(after, [[0, 0]], "ended");
        assert!(!mixer.is_playing(0));
    }

    #[test]
    fn gains_follow_the_pan_law() {
        assert_eq!(gains(127, 0), [UNITY_GAIN, 0], "hard left");
        assert_eq!(gains(127, 254), [0, UNITY_GAIN], "hard right");
        assert_eq!(gains(127, 127), [UNITY_GAIN / 2; 2], "the law's middle");
        // The engine's centre, one step right of the law's middle: 126 : 128.
        assert_eq!(
            gains(127, CENTRE_SEPARATION),
            [UNITY_GAIN * 126 / 254, UNITY_GAIN * 128 / 254]
        );
        assert_eq!(gains(0, 127), [0, 0], "silent");
        assert_eq!(gains(200, -5), [UNITY_GAIN, 0], "clamped");
        assert_eq!(gains(500, 300), [0, UNITY_GAIN], "clamped");
        // Half volume, the engine's widest swing (128 ± 96).
        let [left, right] = gains(64, 32);
        assert_eq!(left, ((64 * 222) << 16) / (127 * 254));
        assert_eq!(right, ((64 * 32) << 16) / (127 * 254));
    }

    #[test]
    fn gains_match_the_host_formula() {
        for volume in [0, 1, 50, 127] {
            for separation in [0, 32, 127, 128, 224, 254] {
                let [left, right] = gains(volume, separation);
                let host_left = volume as f64 * (254.0 - separation as f64) / (127.0 * 254.0);
                let host_right = volume as f64 * separation as f64 / (127.0 * 254.0);
                assert!((left as f64 / UNITY_GAIN as f64 - host_left).abs() < 1e-4);
                assert!((right as f64 / UNITY_GAIN as f64 - host_right).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn volume_and_separation_scale_the_samples() {
        // 192 is +16384.
        let sound = constant(11025, 192, 100);
        let mut mixer = Mixer::new(OUTPUT_MILLIHERTZ);
        mixer.start(0, &sound, 127, 127);
        let mut frames = [[0_i16; 2]; 4];
        mixer.mix(&mut frames);
        assert_eq!(frames, [[8192, 8192]; 4], "half on each side");
        mixer.set_levels(0, 127, 254);
        mixer.mix(&mut frames);
        assert_eq!(frames, [[0, 16384]; 4], "hard right");
        mixer.set_levels(0, 64, 0);
        mixer.mix(&mut frames);
        let expected = (16384 * gains(64, 0)[0]) >> 16;
        assert_eq!(frames, [[i16::try_from(expected).unwrap(), 0]; 4]);
    }

    #[test]
    fn voices_add_up_and_clip() {
        let loud = constant(11025, 255, 100); // +32512
        let quiet = constant(11025, 160, 100); // +8192
        let low = constant(11025, 0, 100); // -32768
        let mut mixer = Mixer::new(OUTPUT_MILLIHERTZ);
        mixer.start(0, &quiet, 127, 0);
        mixer.start(1, &quiet, 127, 0);
        let mut frames = [[0_i16; 2]; 2];
        assert_eq!(mixer.mix(&mut frames), 2);
        assert_eq!(frames, [[16384, 0]; 2], "two voices add up");

        mixer.start(2, &loud, 127, 0);
        mixer.mix(&mut frames);
        assert_eq!(frames, [[i16::MAX, 0]; 2], "clipped high");

        let mut mixer = Mixer::new(OUTPUT_MILLIHERTZ);
        mixer.start(0, &low, 127, 254);
        mixer.start(5, &low, 127, 254);
        mixer.mix(&mut frames);
        assert_eq!(frames, [[0, i16::MIN]; 2], "clipped low");
    }

    #[test]
    fn voices_mix_onto_what_the_frames_hold() {
        let quiet = constant(11025, 160, 100); // +8192
        let loud = constant(11025, 255, 100); // +32512
        let mut mixer = Mixer::new(OUTPUT_MILLIHERTZ);
        mixer.start(0, &quiet, 127, 0);
        // Music already in the frames: the voice adds to it.
        let mut frames = [[1000_i16, -2000]; 2];
        assert_eq!(mixer.mix_onto(&mut frames), 1);
        assert_eq!(frames, [[9192, -2000]; 2]);
        // The sum is clipped once, not the music and the voice apart.
        mixer.start(1, &loud, 127, 0);
        let mut frames = [[-20_000_i16, 0]; 2];
        mixer.mix_onto(&mut frames);
        assert_eq!(frames, [[20_704, 0]; 2]);
        let mut frames = [[10_000_i16, 0]; 2];
        mixer.mix_onto(&mut frames);
        assert_eq!(frames, [[i16::MAX, 0]; 2], "clipped high");
        // `mix` ignores what the frames held.
        let mut frames = [[10_000_i16, 7]; 2];
        mixer.mix(&mut frames);
        assert_eq!(frames, [[i16::MAX, 0]; 2]);
        mixer.stop(1);
        mixer.mix(&mut frames);
        assert_eq!(frames, [[8192, 0]; 2]);
    }

    /// The frame-by-frame mix `mix_onto` replaced: each frame sums every
    /// playing voice's next sample, then clips.
    fn mix_onto_frame_major(mixer: &mut Mixer<'_>, frames: &mut [[i16; 2]]) -> usize {
        let playing = mixer
            .voices
            .iter()
            .filter(|voice| voice.sound.is_some())
            .count();
        for frame in frames.iter_mut() {
            let mut left = i32::from(frame[0]);
            let mut right = i32::from(frame[1]);
            for voice in mixer
                .voices
                .iter_mut()
                .filter(|voice| voice.sound.is_some())
            {
                if let Some(sample) = voice.next_sample() {
                    left += (sample * voice.gains[0]) >> GAIN_BITS;
                    right += (sample * voice.gains[1]) >> GAIN_BITS;
                }
            }
            *frame = [clip(left), clip(right)];
        }
        playing
    }

    #[test]
    fn voice_major_mix_is_bit_exact() {
        // Sounds of different rates, lengths and shapes; some end inside a
        // run, some clip with the music under them.
        let ramp: Vec<u8> = (0..=255).cycle().take(3000).collect();
        let saw: Vec<u8> = (0..700).map(|index| (index * 37 % 256) as u8).collect();
        let sounds = [
            Sound::decode(&lump(11025, &ramp)).unwrap(),
            Sound::decode(&lump(22050, &saw)).unwrap(),
            constant(11025, 255, 30),
            constant(8000, 3, 129),
            Sound::decode(&lump(44100, &saw[..65])).unwrap(),
        ];
        let music = |index: usize| {
            let value = (index as i32 * 2731 % 65_536) - 32_768;
            [value as i16, (-value / 2) as i16]
        };
        for &length in &[1_usize, 63, 64, 65, 127, 512, 700] {
            let mut voice_major = Mixer::new(OUTPUT_MILLIHERTZ);
            let mut frame_major = Mixer::new(OUTPUT_MILLIHERTZ);
            for (voice, sound) in sounds.iter().enumerate() {
                let volume = 127 - voice as i32 * 20;
                let separation = voice as i32 * 60;
                voice_major.start(voice, sound, volume, separation);
                frame_major.start(voice, sound, volume, separation);
            }
            let mut offset = 0;
            // Several buffers in a row, until every voice ran out.
            for _ in 0..40 {
                let mut expected: Vec<[i16; 2]> = (offset..offset + length).map(music).collect();
                let mut mixed = expected.clone();
                assert_eq!(
                    voice_major.mix_onto(&mut mixed),
                    mix_onto_frame_major(&mut frame_major, &mut expected)
                );
                assert_eq!(mixed, expected, "{length} frames from {offset}");
                for voice in 0..VOICES {
                    assert_eq!(voice_major.is_playing(voice), frame_major.is_playing(voice));
                }
                offset += length;
            }
        }
    }

    #[test]
    fn no_voice_leaves_the_frames_as_they_are() {
        let mut mixer = Mixer::new(OUTPUT_MILLIHERTZ);
        let mut frames = [[i16::MIN, i16::MAX], [12, -34]];
        assert_eq!(mixer.mix_onto(&mut frames), 0);
        assert_eq!(frames, [[i16::MIN, i16::MAX], [12, -34]]);
    }

    #[test]
    fn a_voice_ends_with_its_sound() {
        let short = constant(11025, 192, 2);
        let long = constant(11025, 160, 100);
        let mut mixer = Mixer::new(OUTPUT_MILLIHERTZ);
        mixer.start(3, &short, 127, 0);
        mixer.start(4, &long, 127, 254);
        let mut frames = [[0_i16; 2]; 16];
        assert_eq!(mixer.mix(&mut frames), 2);
        assert!(!mixer.is_playing(3), "two samples last eight frames");
        assert!(mixer.is_playing(4));
        assert_eq!(frames[0], [16384, 8192]);
        assert_eq!(
            frames[15],
            [0, 8192],
            "the short one is silent after it ends"
        );
        mixer.stop(4);
        assert_eq!(mixer.mix(&mut frames), 0);
        assert_eq!(frames, [[0, 0]; 16]);
    }

    #[test]
    fn the_bank_keeps_one_copy_per_sound() {
        let bank = SoundBank::new();
        let pistol = lump(11025, &[1, 2, 3]);
        let shotgun = lump(11025, &[1, 2, 4]);
        let faster = lump(22050, &[1, 2, 3]);
        assert_eq!(bank.add(&pistol), Some(0));
        assert_eq!(bank.add(&shotgun), Some(1));
        assert_eq!(bank.add(&faster), Some(2), "another rate is another sound");
        assert_eq!(bank.add(&pistol.clone()), Some(0), "found by content");
        assert_eq!(bank.add(&[3, 0]), None, "malformed");
        assert_eq!(bank.len(), 3);
        assert_eq!(bank.get(1).unwrap().samples(), [1, 2, 4]);
        assert_eq!(bank.get(2).unwrap().rate_hz(), 22050);
        assert!(bank.get(3).is_none());
    }

    #[test]
    fn the_bank_refuses_a_sound_once_full() {
        let bank = SoundBank::new();
        for index in 0..BANK_SOUNDS {
            let samples = u16::try_from(index).unwrap().to_le_bytes();
            assert_eq!(bank.add(&lump(11025, &samples)), Some(index));
        }
        assert_eq!(bank.add(&lump(11025, &[9, 9, 9])), None);
        assert_eq!(
            bank.add(&lump(11025, &[0, 0])),
            Some(0),
            "known sounds still play"
        );
    }

    #[test]
    fn shared_voices_start_update_and_finish() {
        let shared = SharedVoices::new();
        let mut mixer = Mixer::new(OUTPUT_MILLIHERTZ);
        let sound = lump(11025, &[192; 10]);
        assert!(!shared.is_playing(2));
        assert!(shared.start(2, &sound, 127, 127));
        assert!(
            shared.is_playing(2),
            "playing before the playback Thread saw it"
        );

        let mut frames = [[0_i16; 2]; 8];
        assert_eq!(mixer.mix_shared(&shared, &mut frames), 1);
        assert_eq!(frames[0], [8192, 8192]);
        assert!(shared.is_playing(2));

        shared.update(2, 127, 0);
        mixer.mix_shared(&shared, &mut frames);
        assert_eq!(frames[0], [16384, 0], "new levels at the next buffer");

        // Ten samples are 40 frames; 16 played so far.
        let mut rest = [[0_i16; 2]; 32];
        mixer.mix_shared(&shared, &mut rest);
        assert!(!shared.is_playing(2), "reported finished");
        assert!(rest[31] == [0, 0]);
    }

    #[test]
    fn shared_voices_stop_and_restart() {
        let shared = SharedVoices::new();
        let mut mixer = Mixer::new(OUTPUT_MILLIHERTZ);
        let sound = lump(11025, &[192; 1000]);
        let other = lump(11025, &[64; 1000]);
        let mut frames = [[0_i16; 2]; 4];
        assert!(shared.start(0, &sound, 127, 127));
        mixer.mix_shared(&shared, &mut frames);
        shared.stop(0);
        assert!(!shared.is_playing(0), "stopped at once for the game");
        mixer.mix_shared(&shared, &mut frames);
        assert_eq!(frames, [[0, 0]; 4], "and silent at the next buffer");

        // A restart of the same sound plays from the beginning; the levels
        // of a start are its own, not the last update's.
        assert!(shared.start(0, &sound, 127, 0));
        shared.update(0, 127, 0);
        assert!(shared.start(0, &other, 127, 254));
        mixer.mix_shared(&shared, &mut frames);
        assert_eq!(frames, [[0, -16384]; 4], "the latest start, hard right");
        assert!(shared.is_playing(0));
        assert_eq!(shared.bank().len(), 2);

        assert!(!shared.start(VOICES, &sound, 127, 127), "no such channel");
        assert!(!shared.start(1, &[3, 0, 0], 127, 127), "malformed");
        assert!(!shared.is_playing(VOICES));
    }
}
