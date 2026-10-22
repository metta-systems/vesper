//! Sound-effect (SFX) decoding and mixing helpers.
//!
//! Implements the per-channel audio state used by Doom's eight logical SFX
//! channels and the helpers that decode raw DMX `DSxxxxx` lumps into
//! float-PCM samples consumable by `rodio`.  The channel and pan-state types
//! here are owned by [`crate::audio::AudioState`] (see the parent module),
//! which drives the high-level start/stop/update API.
//!
//! ## Format quick-reference (Doom DMX SFX lump)
//!
//! | Offset | Bytes | Meaning                          |
//! |--------|-------|----------------------------------|
//! | 0      | 2     | Format ID (always `3`)           |
//! | 2      | 2     | Sample rate, little-endian Hz    |
//! | 4      | 4     | Sample count, little-endian      |
//! | 8      | N     | Unsigned 8-bit PCM (centre = 128)|
//!
//! The decoder converts the 8-bit samples to `f32` in `[-1, 1)` and the
//! [`PannedSource`] adapter turns a mono `rodio::Source` into a stereo stream
//! with per-channel gains that may be updated mid-playback via [`PanState`].

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rodio::{Player, Source};

/// Decode a raw Doom DMX SFX lump into a sample rate and float PCM buffer.
///
/// Returns `None` for any malformed input: too-short headers, zero sample
/// count, zero sample rate, or a buffer truncated before the sample data is
/// complete.  Otherwise yields the lump's reported sample rate (Hz) and the
/// converted `f32` samples in `[-1, 1)` (8-bit unsigned PCM, centre 128).
pub(crate) fn decode_doom_sfx(data: &[u8]) -> Option<(u32, Vec<f32>)> {
    if data.len() < 8 {
        return None;
    }
    let sample_rate = u16::from_le_bytes([data[2], data[3]]) as u32;
    let num_samples = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    if sample_rate == 0 || num_samples == 0 || data.len() < 8 + num_samples {
        return None;
    }
    let samples = data[8..8 + num_samples]
        .iter()
        .map(|&b| (b as f32 - 128.0) / 128.0)
        .collect();
    Some((sample_rate, samples))
}

/// Convert Doom's `(volume, separation)` pair into linear left/right gains.
///
/// `vol` is clamped to `0..=127` (Doom's volume range) and `sep` to `0..=254`
/// (Doom's stereo separation: 0 = hard left, 127 = centre, 254 = hard right).
/// The result is a pair `(left, right)` where each gain is in `[0.0, 1.0]`.
pub(crate) fn gains_from(vol: i32, sep: i32) -> (f32, f32) {
    let v = vol.clamp(0, 127) as f32;
    let s = sep.clamp(0, 254) as f32;
    (v * (254.0 - s) / (127.0 * 254.0), v * s / (127.0 * 254.0))
}

/// Atomic left/right gain pair shared between the game thread (which writes
/// via [`PanState::update`]) and the audio thread (which reads inside a
/// [`PannedSource`]).
///
/// Bits are stored as `AtomicU32` and reinterpreted as `f32` via
/// `f32::from_bits`, which avoids any locking in the audio fast path.
pub(crate) struct PanState {
    /// Left-channel gain, encoded as the bit pattern of an `f32`.
    pub(crate) l_gain: AtomicU32,
    /// Right-channel gain, encoded as the bit pattern of an `f32`.
    pub(crate) r_gain: AtomicU32,
}

/// Construction and live-update helpers for [`PanState`].
impl PanState {
    /// Build a [`PanState`] initialised from the given Doom volume and
    /// separation values.  Both gains are derived via [`gains_from`].
    pub(crate) fn new(vol: i32, sep: i32) -> Self {
        let (l, r) = gains_from(vol, sep);
        Self {
            l_gain: AtomicU32::new(l.to_bits()),
            r_gain: AtomicU32::new(r.to_bits()),
        }
    }

    /// Atomically update both gains from a fresh `(vol, sep)` pair.
    ///
    /// Uses `Ordering::Relaxed` for both stores: tearing between the left and
    /// right gains is acceptable because it only causes a single sample of
    /// stale panning during the rare update boundary.
    pub(crate) fn update(&self, vol: i32, sep: i32) {
        let (l, r) = gains_from(vol, sep);
        self.l_gain.store(l.to_bits(), Ordering::Relaxed);
        self.r_gain.store(r.to_bits(), Ordering::Relaxed);
    }
}

/// Stereo adapter that wraps a mono `rodio::Source` and emits interleaved
/// left/right samples, each scaled by the gains held in [`PanState`].
///
/// `S` is the underlying mono source; `buffered` holds the input sample
/// between the left output and the right output so each input frame produces
/// exactly two output frames.
pub(crate) struct PannedSource<S> {
    /// Mono input source.  Read once per emitted stereo frame.
    inner: S,
    /// Shared gain state, read every output sample.
    pan: Arc<PanState>,
    /// `None` after a left sample is emitted; `Some(s)` after we've stashed
    /// the input sample so the next call can emit the right channel.
    buffered: Option<f32>,
}

/// Construction helper for [`PannedSource`].
impl<S: Source> PannedSource<S> {
    /// Wrap `inner` and pair it with a shared [`PanState`] for live gain
    /// updates.  The wrapped source must be mono; multi-channel input is
    /// unsupported (only one input sample is read per emitted stereo frame).
    pub(crate) fn new(inner: S, pan: Arc<PanState>) -> Self {
        Self {
            inner,
            pan,
            buffered: None,
        }
    }
}

/// Iterator impl producing alternating left/right samples.
impl<S: Source> Iterator for PannedSource<S> {
    type Item = f32;

    /// Pull one stereo sample. State machine on `buffered`: when empty,
    /// fetch a fresh mono sample from `inner`, cache it, and emit it
    /// scaled by the left gain; when full, emit the cached sample
    /// scaled by the right gain and clear the cache.
    fn next(&mut self) -> Option<f32> {
        match self.buffered.take() {
            None => {
                let s = self.inner.next()?;
                let l = s * f32::from_bits(self.pan.l_gain.load(Ordering::Relaxed));
                self.buffered = Some(s);
                Some(l)
            }
            Some(s) => Some(s * f32::from_bits(self.pan.r_gain.load(Ordering::Relaxed))),
        }
    }
}

/// `rodio::Source` impl that advertises the wrapped source as stereo.
impl<S: Source> Source for PannedSource<S> {
    /// Length of the current contiguous span in samples; doubled because each
    /// mono input sample yields two output samples.
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len().map(|n| n * 2)
    }
    /// Always 2 (stereo).
    fn channels(&self) -> rodio::ChannelCount {
        std::num::NonZero::new(2).unwrap()
    }
    /// Pass through the underlying mono source's sample rate.
    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.sample_rate()
    }
    /// Pass through the underlying source's total duration (if any).
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

/// State for one of Doom's eight logical SFX channels.
///
/// Held in an array by [`crate::audio::AudioState`].  The optional `player` is
/// the live `rodio::Player` currently playing on this channel (dropping it
/// stops playback immediately); the shared `pan` outlives individual players
/// so volume/separation updates between sounds remain coherent.
pub(crate) struct ChannelState {
    /// Active rodio player for this channel, or `None` when idle.  Dropping
    /// the player halts playback.
    pub(crate) player: Option<Player>,
    /// Shared pan state cloned into every [`PannedSource`] on this channel
    /// so [`crate::audio::AudioState::update_sound_params`] can adjust
    /// gains without restarting playback.
    pub(crate) pan: Arc<PanState>,
}

/// Default-construction helper for [`ChannelState`].
impl ChannelState {
    /// Build an idle channel: no active player and a centred-volume pan state
    /// (`vol = 127`, `sep = 127`).
    pub(crate) fn new() -> Self {
        Self {
            player: None,
            pan: Arc::new(PanState::new(127, 127)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;
    use std::num::NonZero;

    #[test]
    fn decode_sfx_silence_sample() {
        let data: &[u8] = &[3, 0, 0x11, 0x2B, 1, 0, 0, 0, 128];
        let (rate, samples) = decode_doom_sfx(data).unwrap();
        assert_eq!(rate, 11025);
        assert_eq!(samples.len(), 1);
        assert!((samples[0] - 0.0_f32).abs() < 1e-4);
    }

    #[test]
    fn decode_sfx_full_positive() {
        let data: &[u8] = &[3, 0, 0x11, 0x2B, 1, 0, 0, 0, 255];
        let (_, samples) = decode_doom_sfx(data).unwrap();
        assert!((samples[0] - (127.0_f32 / 128.0)).abs() < 1e-4);
    }

    #[test]
    fn decode_sfx_full_negative() {
        let data: &[u8] = &[3, 0, 0x11, 0x2B, 1, 0, 0, 0, 0];
        let (_, samples) = decode_doom_sfx(data).unwrap();
        assert!((samples[0] - (-1.0_f32)).abs() < 1e-4);
    }

    #[test]
    fn decode_sfx_too_short_returns_none() {
        assert!(decode_doom_sfx(&[3, 0, 0, 0]).is_none());
    }

    #[test]
    fn decode_sfx_zero_samples_returns_none() {
        let data: &[u8] = &[3, 0, 0x11, 0x2B, 0, 0, 0, 0];
        assert!(decode_doom_sfx(data).is_none());
    }

    #[test]
    fn decode_sfx_zero_rate_returns_none() {
        let data: &[u8] = &[3, 0, 0x00, 0x00, 1, 0, 0, 0, 128];
        assert!(decode_doom_sfx(data).is_none());
    }

    #[test]
    fn decode_sfx_truncated_returns_none() {
        let data: &[u8] = &[3, 0, 0x11, 0x2B, 4, 0, 0, 0, 128, 192];
        assert!(decode_doom_sfx(data).is_none());
    }

    #[test]
    fn gains_center() {
        let (l, r) = gains_from(127, 127);
        assert!((l - 0.5_f32).abs() < 1e-4, "l={l}");
        assert!((r - 0.5_f32).abs() < 1e-4, "r={r}");
    }

    #[test]
    fn gains_hard_left() {
        let (l, r) = gains_from(127, 0);
        assert!((l - 1.0_f32).abs() < 1e-4, "l={l}");
        assert!((r - 0.0_f32).abs() < 1e-4, "r={r}");
    }

    #[test]
    fn gains_hard_right() {
        let (l, r) = gains_from(127, 254);
        assert!((l - 0.0_f32).abs() < 1e-4, "l={l}");
        assert!((r - 1.0_f32).abs() < 1e-4, "r={r}");
    }

    #[test]
    fn gains_clamp_vol() {
        let (l, r) = gains_from(200, 127);
        assert!((l - 0.5_f32).abs() < 1e-4, "l={l}");
        assert!((r - 0.5_f32).abs() < 1e-4, "r={r}");
    }

    #[test]
    fn pan_state_update() {
        let pan = PanState::new(127, 127);
        pan.update(127, 0);
        let l = f32::from_bits(pan.l_gain.load(Ordering::Relaxed));
        let r = f32::from_bits(pan.r_gain.load(Ordering::Relaxed));
        assert!((l - 1.0_f32).abs() < 1e-4, "l={l}");
        assert!((r - 0.0_f32).abs() < 1e-4, "r={r}");
    }

    #[test]
    fn panned_source_center_two_samples() {
        let buf = SamplesBuffer::new(
            NonZero::new(1).unwrap(),
            NonZero::new(11025).unwrap(),
            vec![1.0_f32, 0.5_f32],
        );
        let pan = Arc::new(PanState::new(127, 127));
        let mut src = PannedSource::new(buf, Arc::clone(&pan));
        let l1 = src.next().unwrap();
        let r1 = src.next().unwrap();
        assert!((l1 - 0.5_f32).abs() < 1e-4, "l1={l1}");
        assert!((r1 - 0.5_f32).abs() < 1e-4, "r1={r1}");
        let l2 = src.next().unwrap();
        let r2 = src.next().unwrap();
        assert!((l2 - 0.25_f32).abs() < 1e-4, "l2={l2}");
        assert!((r2 - 0.25_f32).abs() < 1e-4, "r2={r2}");
        assert!(src.next().is_none());
    }

    #[test]
    fn panned_source_hard_left() {
        let buf = SamplesBuffer::new(
            NonZero::new(1).unwrap(),
            NonZero::new(11025).unwrap(),
            vec![0.5_f32],
        );
        let pan = Arc::new(PanState::new(127, 0));
        let mut src = PannedSource::new(buf, pan);
        let l = src.next().unwrap();
        let r = src.next().unwrap();
        assert!((l - 0.5_f32).abs() < 1e-4, "l={l}");
        assert!((r - 0.0_f32).abs() < 1e-4, "r={r}");
    }

    #[test]
    fn panned_source_channels_is_2() {
        use rodio::Source;
        let buf = SamplesBuffer::new(
            NonZero::new(1).unwrap(),
            NonZero::new(11025).unwrap(),
            vec![0.0_f32],
        );
        let pan = Arc::new(PanState::new(127, 127));
        let src = PannedSource::new(buf, pan);
        assert_eq!(src.channels().get(), 2);
    }

    #[test]
    fn panned_source_live_pan_update() {
        let buf = SamplesBuffer::new(
            NonZero::new(1).unwrap(),
            NonZero::new(11025).unwrap(),
            vec![1.0_f32, 1.0_f32],
        );
        let pan = Arc::new(PanState::new(127, 127));
        let mut src = PannedSource::new(buf, Arc::clone(&pan));
        let _ = src.next();
        let _ = src.next();
        pan.update(127, 0);
        let l2 = src.next().unwrap();
        let r2 = src.next().unwrap();
        assert!((l2 - 1.0_f32).abs() < 1e-4, "l2={l2}");
        assert!((r2 - 0.0_f32).abs() < 1e-4, "r2={r2}");
    }
}
