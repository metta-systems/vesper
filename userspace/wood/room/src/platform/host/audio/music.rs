//! Music playback: SoundFont synthesis of the MIDI songs the engine hands
//! over through `DG_MusicPlay`.
//!
//! This module replaces Doom's original OPL2 music driver with a software
//! synthesiser path built on `rustysynth`.  The engine converts MUS lumps to
//! Standard MIDI Files (`room::engine::sound::mus2midi`) before they reach here.
//!
//! ## Pipeline
//!
//! ```text
//!   SMF bytes ──rustysynth──▶ stereo f32 PCM
//!                               │
//!             Arc<AtomicU32> ───┴── volume gain
//!                               │
//!                         rodio::Player ──▶ mixer
//! ```
//!
//! ## Components
//!
//! - [`MusicSource`] — a `rodio::Source` that renders one block at a time
//!   from the sequencer and applies a shared volume gain.
//! - [`MusicState`] — owns the SF2 SoundFont, the active player, and the
//!   shared volume cell; exposes load/play/stop/pause/resume.

use {
    rodio::{Player, Source},
    rustysynth::{MidiFile, MidiFileSequencer, SoundFont, Synthesizer, SynthesizerSettings},
    std::{
        io::{BufReader, Cursor},
        sync::{
            Arc,
            atomic::{AtomicU32, Ordering},
        },
        time::Duration,
    },
};

/// Number of stereo frames rendered per sequencer `render` call.  Each call
/// produces `BLOCK_SIZE` left and `BLOCK_SIZE` right samples, so the
/// interleaved output stream advances by `2 * BLOCK_SIZE` samples per block.
const BLOCK_SIZE: usize = 512;

/// Audio output sample rate fed to the rustysynth synthesiser.
const SAMPLE_RATE: i32 = 44100;

// ---------------------------------------------------------------------------
// MusicSource — rodio Source that renders MIDI via rustysynth
// ---------------------------------------------------------------------------

/// A `rodio::Source` that synthesises MIDI in `BLOCK_SIZE`-frame chunks and
/// streams interleaved stereo `f32` samples.
///
/// Renders one block ahead and walks the interleaved cursor `pos` from `0` to
/// `BLOCK_SIZE * 2` before triggering the next render.  When the sequencer
/// reports end-of-sequence the source stops producing samples (after draining
/// the final block).
pub(crate) struct MusicSource {
    /// The underlying rustysynth sequencer; advanced one block at a time.
    sequencer: MidiFileSequencer,
    /// Left-channel scratch buffer filled by `sequencer.render`.
    buf_l: Vec<f32>,
    /// Right-channel scratch buffer filled by `sequencer.render`.
    buf_r: Vec<f32>,
    /// Interleaved read cursor in `[0, BLOCK_SIZE * 2]`.  Even values index
    /// `buf_l[pos/2]`, odd values index `buf_r[pos/2]`.
    pos: usize,
    /// `true` once the sequencer reports end-of-sequence and the current
    /// block has been fully drained.
    finished: bool,
    /// Shared output gain (`f32` bits) updated by [`MusicState::set_volume`].
    volume: Arc<AtomicU32>,
}

/// Construction helper for [`MusicSource`].
impl MusicSource {
    /// Build a new music source from a SoundFont and a SMF byte slice.
    ///
    /// Returns `None` if the synthesiser settings are rejected by rustysynth
    /// or if `midi_bytes` is not a parseable Standard MIDI File.  When
    /// `looping` is `true` the sequencer will restart the file on completion.
    pub(crate) fn new(
        sound_font: &Arc<SoundFont>,
        midi_bytes: &[u8],
        looping: bool,
        volume: Arc<AtomicU32>,
    ) -> Option<Self> {
        let settings = SynthesizerSettings::new(SAMPLE_RATE);
        let synthesizer = Synthesizer::new(sound_font, &settings).ok()?;
        let midi_file = Arc::new(MidiFile::new(&mut Cursor::new(midi_bytes)).ok()?);
        let mut sequencer = MidiFileSequencer::new(synthesizer);
        sequencer.play(&midi_file, looping);
        Some(Self {
            sequencer,
            buf_l: vec![0.0f32; BLOCK_SIZE],
            buf_r: vec![0.0f32; BLOCK_SIZE],
            pos: BLOCK_SIZE * 2, // triggers render on first next()
            finished: false,
            volume,
        })
    }
}

/// Iterator impl producing interleaved L,R,L,R,… samples scaled by `volume`.
impl Iterator for MusicSource {
    type Item = f32;

    /// Emit the next sample.  When the interleaved cursor reaches the end of
    /// the current block, render another block (or stop if the sequence is
    /// finished).  Returns `None` only after the last block is drained.
    fn next(&mut self) -> Option<f32> {
        if self.pos >= BLOCK_SIZE * 2 {
            if self.finished {
                return None;
            }
            self.sequencer.render(&mut self.buf_l, &mut self.buf_r);
            self.pos = 0;
            if self.sequencer.end_of_sequence() {
                self.finished = true;
            }
        }
        let vol = f32::from_bits(self.volume.load(Ordering::Relaxed));
        let sample = if self.pos.is_multiple_of(2) {
            self.buf_l[self.pos / 2] * vol
        } else {
            self.buf_r[self.pos / 2] * vol
        };
        self.pos += 1;
        Some(sample)
    }
}

/// `rodio::Source` impl describing the synthesised stream as 2-channel
/// `f32` PCM at [`SAMPLE_RATE`] Hz with no fixed length.
impl Source for MusicSource {
    /// No fixed-length span: the sequencer can emit indefinitely (looping
    /// playback) or end at any block boundary.
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    /// Always 2 (stereo).
    fn channels(&self) -> rodio::ChannelCount {
        std::num::NonZero::new(2u16).unwrap()
    }
    /// Output sample rate, matching the synthesiser settings.
    fn sample_rate(&self) -> rodio::SampleRate {
        std::num::NonZero::new(SAMPLE_RATE as u32).unwrap()
    }
    /// Total duration is unknown in general; looping streams have none.
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

// ---------------------------------------------------------------------------
// MusicState — SF2 loading and playback control
// ---------------------------------------------------------------------------

/// Owns the SoundFont, the optional active player, and the shared volume
/// cell.  Held inside [`super::AudioState`].
pub(crate) struct MusicState {
    /// Loaded SF2 SoundFont, shared with every [`MusicSource`] this state
    /// spawns.  `None` until [`MusicState::load_sound_font`] succeeds.
    pub(crate) sound_font: Option<Arc<SoundFont>>,
    /// Active rodio player playing the current song, or `None` when stopped.
    /// Dropping the player halts playback.
    player: Option<Player>,
    /// Shared volume gain (`f32` bits) read by every active [`MusicSource`].
    volume: Arc<AtomicU32>,
}

/// Public lifecycle and control API for music playback.
impl MusicState {
    /// Construct an idle state with no SoundFont loaded, no active player,
    /// and volume initialised to full (`1.0`).
    pub(crate) fn new() -> Self {
        Self {
            sound_font: None,
            player: None,
            volume: Arc::new(AtomicU32::new(1.0f32.to_bits())),
        }
    }

    /// Load an SF2 SoundFont from disk.
    ///
    /// On success the loaded font is stored in `sound_font` and used for all
    /// subsequent [`MusicState::play`] calls.  Errors (missing file, parse
    /// failure) are logged but not propagated — playback simply remains a
    /// no-op until a valid font is loaded.
    pub(crate) fn load_sound_font(&mut self, path: &std::path::Path) {
        match std::fs::File::open(path) {
            Ok(f) => {
                let mut reader = BufReader::new(f);
                match SoundFont::new(&mut reader) {
                    Ok(sf) => {
                        log::info!("Soundfont loaded: {}", path.display());
                        self.sound_font = Some(Arc::new(sf));
                    }
                    Err(e) => log::warn!("Soundfont parse error ({}): {e}", path.display()),
                }
            }
            Err(e) => log::warn!("Soundfont not found ({}): {e}", path.display()),
        }
    }

    /// Start playing the given MIDI bytes on the supplied rodio mixer.
    ///
    /// If `looping` is `true`, the sequencer will repeat the song
    /// indefinitely.  Any previous song is replaced (its player is dropped,
    /// stopping playback before the new one is appended).  Returns silently
    /// if no SoundFont is loaded or if the MIDI bytes are invalid.
    pub(crate) fn play(&mut self, midi_bytes: &[u8], looping: bool, mixer: &rodio::mixer::Mixer) {
        let Some(sf) = self.sound_font.as_ref() else {
            return;
        };
        let Some(source) = MusicSource::new(sf, midi_bytes, looping, Arc::clone(&self.volume))
        else {
            log::warn!("DG_MusicPlay: failed to create MusicSource");
            return;
        };
        self.player = None;
        let player = Player::connect_new(mixer);
        player.append(source);
        self.player = Some(player);
    }

    /// Stop the currently playing song, if any, by dropping the player.
    pub(crate) fn stop(&mut self) {
        self.player = None;
    }

    /// Set the music gain from a Doom volume value (`0..=127`).
    ///
    /// The value is clamped, normalised to `[0.0, 1.0]`, and stored as the
    /// bit pattern of an `f32` in the shared atomic so the audio thread can
    /// pick it up on its next sample.
    pub(crate) fn set_volume(&self, vol: i32) {
        let gain = (vol.clamp(0, 127) as f32 / 127.0).to_bits();
        self.volume.store(gain, Ordering::Relaxed);
    }

    /// Pause playback without dropping the player or losing position.
    pub(crate) fn pause(&self) {
        if let Some(p) = &self.player {
            p.pause();
        }
    }

    /// Resume a paused player.  No-op if no player is active.
    pub(crate) fn resume(&self) {
        if let Some(p) = &self.player {
            p.play();
        }
    }

    /// Report whether a player is active and still has samples to play.
    pub(crate) fn is_playing(&self) -> bool {
        self.player.as_ref().is_some_and(|p| !p.empty())
    }
}
