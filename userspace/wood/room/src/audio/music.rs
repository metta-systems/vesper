//! Music playback: MUS-to-MIDI conversion and SoundFont synthesis.
//!
//! This module replaces Doom's original OPL2 music driver with a software
//! synthesiser path built on `rustysynth`.  Doom ships music as either raw
//! MIDI or DMX MUS lumps; MUS is converted to a Standard MIDI File on the
//! fly via [`mus2midi`] before being handed to a [`MidiFileSequencer`].
//!
//! ## Pipeline
//!
//! ```text
//!   MUS lump ──mus2midi──▶ SMF bytes ──rustysynth──▶ stereo f32 PCM
//!                                                    │
//!                                  Arc<AtomicU32> ───┴── volume gain
//!                                                    │
//!                                              rodio::Player ──▶ mixer
//! ```
//!
//! ## Components
//!
//! - [`MusicSource`] — a `rodio::Source` that renders one block at a time
//!   from the sequencer and applies a shared volume gain.
//! - [`MusicState`] — owns the SF2 SoundFont, the active player, and the
//!   shared volume cell; exposes load/play/stop/pause/resume.
//! - [`mus2midi`] — pure-Rust MUS-to-SMF converter.

use rodio::{Player, Source};
use rustysynth::{MidiFile, MidiFileSequencer, SoundFont, Synthesizer, SynthesizerSettings};
use std::io::{BufReader, Cursor};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Number of stereo frames rendered per sequencer `render` call.  Each call
/// produces `BLOCK_SIZE` left and `BLOCK_SIZE` right samples, so the
/// interleaved output stream advances by `2 * BLOCK_SIZE` samples per block.
const BLOCK_SIZE: usize = 512;

/// Audio output sample rate fed to the rustysynth synthesiser.
const SAMPLE_RATE: i32 = 44100;

/// Owned MIDI byte buffer returned from [`mus2midi`] / loaded from a WAD.
///
/// Wrapped in a struct so that callers (`i_sound`) treat it as an opaque
/// handle that is passed back to the audio backend rather than as a raw
/// `Vec<u8>` of unspecified format.
pub(crate) struct MusicHandle {
    /// Standard MIDI File bytes ready to be fed to [`MusicSource::new`].
    pub(crate) midi_bytes: Vec<u8>,
}

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
/// cell.  Held inside [`crate::audio::AudioState`].
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
            log::warn!("I_PlaySong: failed to create MusicSource");
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

// ---------------------------------------------------------------------------
// MUS-to-MIDI converter
// ---------------------------------------------------------------------------

/// 4-byte magic at the start of every MUS lump.
const MUS_MAGIC: &[u8; 4] = b"MUS\x1a";

/// Tempo meta-event value (microseconds per quarter note).  Combined with
/// [`MIDI_PPQ`] this yields 140 ticks per second, matching the native MUS
/// tick rate.
const MIDI_TEMPO: u32 = 500_000; // µs per beat (120 BPM) — yields 140 ticks/sec = MUS tick rate

/// Pulses-per-quarter-note used in the emitted SMF header.  At
/// [`MIDI_TEMPO`] of 500 000 µs/beat this gives 70 ticks / (0.5 s) = 140
/// Hz, i.e. one MIDI tick equals one MUS tick.
const MIDI_PPQ: u16 = 70; // ticks per beat; 1 tick = 1 MUS tick (1/140 s)

/// Map a MUS channel number to its MIDI counterpart.
///
/// MUS reserves channel 15 for percussion, which maps to MIDI channel 9
/// (the General MIDI drum channel).  MUS channels 9..=14 shift up by one so
/// MIDI channel 9 is left exclusively for percussion.  Channels 0..=8 are
/// unchanged.
fn mus_to_midi_channel(mus_ch: u8) -> u8 {
    if mus_ch == 15 {
        // percussion
        9
    } else if mus_ch >= 9 {
        // skip MIDI ch 9
        mus_ch + 1
    } else {
        mus_ch
    }
}

/// Append `val` to `buf` using MIDI's variable-length quantity encoding.
///
/// Bytes are written big-endian, seven bits at a time, with the high bit set
/// on every byte except the last.  Values larger than 28 bits are silently
/// truncated, which is fine for MIDI delta times in this converter.
fn write_var_len(buf: &mut Vec<u8>, mut val: u32) {
    let b0 = (val & 0x7F) as u8;
    val >>= 7;
    if val == 0 {
        buf.push(b0);
        return;
    }
    let b1 = (val & 0x7F) as u8;
    val >>= 7;
    if val == 0 {
        buf.extend_from_slice(&[b1 | 0x80, b0]);
        return;
    }
    let b2 = (val & 0x7F) as u8;
    val >>= 7;
    if val == 0 {
        buf.extend_from_slice(&[b2 | 0x80, b1 | 0x80, b0]);
        return;
    }
    let b3 = (val & 0x7F) as u8;
    buf.extend_from_slice(&[b3 | 0x80, b2 | 0x80, b1 | 0x80, b0]);
}

/// Decode a MUS "delay" value: a sequence of bytes whose low 7 bits are
/// concatenated big-endian, terminated by the first byte with its top bit
/// clear.
///
/// Advances `*pos` past the consumed bytes.  Returns `None` if the score
/// ends before the terminator is encountered.  Uses saturating arithmetic so
/// pathologically long sequences clamp at `u32::MAX` instead of overflowing.
fn read_delay(score: &[u8], pos: &mut usize) -> Option<u32> {
    let mut delay = 0u32;
    loop {
        if *pos >= score.len() {
            return None;
        }
        let b = score[*pos];
        *pos += 1;
        delay = delay.saturating_mul(128).saturating_add((b & 0x7F) as u32);
        if b & 0x80 == 0 {
            return Some(delay);
        }
    }
}

/// Convert a DMX MUS lump to a Standard MIDI File.
///
/// Returns `None` for any malformed input: wrong magic, header that points
/// past the end of the buffer, truncated event payloads, or unknown event
/// types.  Otherwise yields a complete SMF (format 0, one track) suitable
/// for handing to [`MusicSource::new`].
///
/// The converter emits a tempo meta-event at the start of the track so the
/// output's wall-clock playback rate matches MUS's native 140 Hz tick rate.
pub(crate) fn mus2midi(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 17 || &data[0..4] != MUS_MAGIC {
        return None;
    }
    let score_len = u16::from_le_bytes([data[4], data[5]]) as usize;
    let score_start = u16::from_le_bytes([data[6], data[7]]) as usize;
    if score_start >= data.len() || score_start + score_len > data.len() {
        return None;
    }
    let score = &data[score_start..score_start + score_len];
    let mut pos = 0usize;
    let mut track: Vec<u8> = Vec::new();

    // Tempo meta-event at delta 0
    track.extend_from_slice(&[
        0x00,
        0xFF,
        0x51,
        0x03,
        ((MIDI_TEMPO >> 16) & 0xFF) as u8,
        ((MIDI_TEMPO >> 8) & 0xFF) as u8,
        (MIDI_TEMPO & 0xFF) as u8,
    ]);

    let mut delta: u32 = 0;
    let mut last_vol = [100u8; 16]; // default velocity per MUS channel

    loop {
        if pos >= score.len() {
            return None; // truncated score
        }
        let ev = score[pos];
        pos += 1;
        let mus_ch = ev & 0x0F;
        let ev_type = (ev >> 4) & 0x07;
        let is_last = ev >> 7 != 0;
        let mid_ch = mus_to_midi_channel(mus_ch);

        // Collect MIDI bytes for this event (empty = no event emitted)
        let mut midi_ev: Vec<u8> = Vec::new();

        match ev_type {
            0 => {
                // note off
                if pos >= score.len() {
                    return None;
                }
                let note = score[pos] & 0x7F;
                pos += 1;
                midi_ev.extend_from_slice(&[0x80 | mid_ch, note, 0x40]);
            }
            1 => {
                // note on
                if pos >= score.len() {
                    return None;
                }
                let raw = score[pos];
                pos += 1;
                if raw & 0x80 != 0 {
                    if pos >= score.len() {
                        return None;
                    }
                    last_vol[mus_ch as usize] = score[pos] & 0x7F;
                    pos += 1;
                }
                let note = raw & 0x7F;
                let vol = last_vol[mus_ch as usize];
                midi_ev.extend_from_slice(&[0x90 | mid_ch, note, vol]);
            }
            2 => {
                // pitch bend
                if pos >= score.len() {
                    return None;
                }
                let b = score[pos] as u32;
                pos += 1;
                // MUS: 0=down, 128=center, 255=up. MIDI: 14-bit, center=0x2000.
                let bend = (b * 64) as u16;
                midi_ev.extend_from_slice(&[0xE0 | mid_ch, (bend & 0x7F) as u8, (bend >> 7) as u8]);
            }
            3 => {
                // system event
                if pos >= score.len() {
                    return None;
                }
                let ctrl = score[pos];
                pos += 1;
                let cc: Option<u8> = match ctrl {
                    10 => Some(120), // all sounds off
                    11 => Some(123), // all notes off
                    12 => Some(126), // mono
                    13 => Some(127), // poly
                    14 => Some(121), // reset all controllers
                    _ => None,
                };
                if let Some(cc) = cc {
                    midi_ev.extend_from_slice(&[0xB0 | mid_ch, cc, 0]);
                }
            }
            4 => {
                // change controller
                if pos + 1 >= score.len() {
                    return None;
                }
                let ctrl = score[pos];
                pos += 1;
                let val = score[pos] & 0x7F;
                pos += 1;
                match ctrl {
                    0 => midi_ev.extend_from_slice(&[0xC0 | mid_ch, val]), // program
                    1 => midi_ev.extend_from_slice(&[0xB0 | mid_ch, 0, val]), // bank select
                    2 => midi_ev.extend_from_slice(&[0xB0 | mid_ch, 1, val]), // modulation
                    3 => midi_ev.extend_from_slice(&[0xB0 | mid_ch, 7, val]), // volume
                    4 => midi_ev.extend_from_slice(&[0xB0 | mid_ch, 10, val]), // pan
                    5 => midi_ev.extend_from_slice(&[0xB0 | mid_ch, 11, val]), // expression
                    6 => midi_ev.extend_from_slice(&[0xB0 | mid_ch, 91, val]), // reverb
                    7 => midi_ev.extend_from_slice(&[0xB0 | mid_ch, 93, val]), // chorus
                    8 => midi_ev.extend_from_slice(&[0xB0 | mid_ch, 64, val]), // sustain
                    9 => midi_ev.extend_from_slice(&[0xB0 | mid_ch, 67, val]), // soft pedal
                    _ => {} // unknown controller, skip
                }
            }
            5 => {}         // measure end — no payload, skip
            6 | 7 => break, // score end
            _ => break,     // unknown type, stop
        }

        if !midi_ev.is_empty() {
            write_var_len(&mut track, delta);
            track.extend(midi_ev);
            delta = 0;
        }

        if is_last {
            delta += read_delay(score, &mut pos)?;
        }
    }

    // End of track
    write_var_len(&mut track, delta);
    track.extend_from_slice(&[0xFF, 0x2F, 0x00]);

    // Assemble MIDI file
    let mut out = Vec::with_capacity(22 + track.len());
    out.extend_from_slice(b"MThd");
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes()); // format 0
    out.extend_from_slice(&1u16.to_be_bytes()); // 1 track
    out.extend_from_slice(&MIDI_PPQ.to_be_bytes());
    out.extend_from_slice(b"MTrk");
    out.extend_from_slice(&(track.len() as u32).to_be_bytes());
    out.extend(track);
    Some(out)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_mus() -> Vec<u8> {
        let mut d = vec![0u8; 17];
        d[0..4].copy_from_slice(b"MUS\x1a");
        d[4] = 1; // score_len
        d[6] = 16; // score_start
        d[16] = 0x60; // score end event: is_last=0, type=6, ch=0
        d
    }

    #[test]
    fn mus2midi_rejects_bad_magic() {
        let data = b"MIDI\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        assert!(mus2midi(data).is_none());
    }

    #[test]
    fn mus2midi_rejects_truncated() {
        assert!(mus2midi(b"MUS").is_none());
    }

    #[test]
    fn mus2midi_produces_valid_smf_header() {
        let out = mus2midi(&minimal_mus()).unwrap();
        // MThd
        assert_eq!(&out[0..4], b"MThd");
        // Header chunk length = 6
        assert_eq!(&out[4..8], &[0, 0, 0, 6]);
        // Format 0
        assert_eq!(&out[8..10], &[0, 0]);
        // 1 track
        assert_eq!(&out[10..12], &[0, 1]);
        // PPQ = 70 = 0x0046
        assert_eq!(&out[12..14], &[0, 70]);
        // MTrk
        assert_eq!(&out[14..18], b"MTrk");
    }

    #[test]
    fn mus2midi_score_end_produces_eot_meta() {
        let out = mus2midi(&minimal_mus()).unwrap();
        // Track ends with delta=0, FF 2F 00
        let last4 = &out[out.len() - 4..];
        assert_eq!(last4, &[0x00, 0xFF, 0x2F, 0x00]);
    }

    #[test]
    fn mus2midi_note_on_encoded_correctly() {
        // MUS: note-on ch0, note=60 (0x3C), vol=100 (0x64), then score end.
        // Event byte 0x10 = is_last=0, type=1 (note on), ch=0.
        // Next byte: 0x80|60 = 0xBC -> has_vol=1, note=60.
        // Vol byte: 100.
        // Score end: 0x60.
        let mut mus = vec![0u8; 20];
        mus[0..4].copy_from_slice(b"MUS\x1a");
        mus[4] = 4; // score_len
        mus[6] = 16; // score_start
        mus[16] = 0x10; // note on, ch 0, not last
        mus[17] = 0x80 | 60; // has_vol=1, note=60
        mus[18] = 100; // volume
        mus[19] = 0x60; // score end
        let out = mus2midi(&mus).unwrap();
        // Find note-on byte 0x90 (ch0 note on) after the tempo meta event.
        // Tempo meta is 7 bytes at track start (after MThd+MTrk headers = 22 bytes).
        // Note-on event: delta=0 (0x00), status=0x90, note=0x3C, vel=0x64.
        let track_body = &out[22..]; // skip MThd (14 bytes) + MTrk header (8 bytes)
                                     // Tempo: 00 FF 51 03 07 A1 20 (7 bytes)
        assert_eq!(
            &track_body[0..7],
            &[0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20]
        );
        // Note on: delta=0, ch=0
        assert_eq!(&track_body[7..11], &[0x00, 0x90, 0x3C, 0x64]);
    }

    #[test]
    fn mus2midi_percussion_maps_to_midi_channel_9() {
        // MUS channel 15 -> MIDI channel 9.
        // Note-on event on ch 15: ev = is_last=0 | type=1<<4 | ch=15 = 0b0_001_1111 = 0x1F
        let mut mus = vec![0u8; 20];
        mus[0..4].copy_from_slice(b"MUS\x1a");
        mus[4] = 4;
        mus[6] = 16;
        mus[16] = 0x1F; // note on, ch 15 (percussion)
        mus[17] = 0x80 | 35; // note=35 (bass drum), has_vol=1
        mus[18] = 100;
        mus[19] = 0x60; // score end
        let out = mus2midi(&mus).unwrap();
        let track_body = &out[22..];
        // Note-on on MIDI channel 9: 0x90 | 9 = 0x99
        assert_eq!(track_body[8], 0x99);
    }

    #[test]
    fn mus2midi_truncated_score_returns_none() {
        // Valid header but score has a note-on byte with no following note byte.
        // score_start=16, event 0x10 = note-on ch0, but score ends before the note byte.
        let mut mus = vec![0u8; 17];
        mus[0..4].copy_from_slice(b"MUS\x1a");
        mus[4] = 1; // score_len = 1
        mus[6] = 16; // score_start
        mus[16] = 0x10; // note-on event, ch 0 — requires another byte that doesn't exist
        assert!(mus2midi(&mus).is_none());
    }
}
