//! Host audio: the sound and music half of the host
//! [`Platform`](room::engine::platform::Platform), on top of the sfx and
//! music submodules.
//!
//! The backend replaces the SDL2 mixer that the original `i_sound.c` and
//! `i_oplmusic.c` drive.  It is built on top of `rodio`: each Doom logical
//! channel becomes a rodio `Player` connected to a single shared output
//! device, and music synthesis lives in the `music` submodule.
//!
//! State is kept in a `thread_local!` `AUDIO` cell.  Doom's audio API is
//! synchronous and only ever called from the main thread, so a thread-local
//! is both safe and the simplest place to hold the `MixerDeviceSink` without
//! introducing any global lock.

pub(crate) mod music;
pub(crate) mod sfx;

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    sync::Arc,
};

use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player, buffer::SamplesBuffer};

use room::engine::misc::argv::{M_CheckParmWithArgs, argument};

pub(crate) use sfx::{PannedSource, decode_doom_sfx};

// ---------------------------------------------------------------------------
// Sound and music, as `Host` hands them over
// ---------------------------------------------------------------------------

/// Open the default audio device, once; `true` when audio is available.
pub(super) fn sound_init() -> bool {
    AUDIO.with_borrow_mut(|audio| {
        if audio.is_none() {
            match AudioState::new() {
                Ok(state) => *audio = Some(state),
                Err(error) => log::warn!("Audio device: {error}"),
            }
        }
        audio.is_some()
    })
}

/// Close the audio device, stopping every sound and song.
pub(super) fn sound_shutdown() {
    AUDIO.with_borrow_mut(|audio| *audio = None);
}

/// Play the DMX sound lump `lump` on `channel`; `true` on success.
pub(super) fn sound_start(lump: &[u8], channel: usize, vol: i32, sep: i32) -> bool {
    AUDIO.with_borrow_mut(|audio| {
        audio
            .as_mut()
            .is_some_and(|audio| audio.start_sound(lump, vol, sep, channel))
    })
}

/// Stop whatever is playing on `channel`.
pub(super) fn sound_stop(channel: usize) {
    AUDIO.with_borrow_mut(|audio| {
        if let Some(audio) = audio.as_mut() {
            audio.stop_sound(channel);
        }
    });
}

/// Change the volume and separation of the sound playing on `channel`.
pub(super) fn sound_update(channel: usize, vol: i32, sep: i32) {
    AUDIO.with_borrow(|audio| {
        if let Some(audio) = audio.as_ref() {
            audio.update_sound_params(channel, vol, sep);
        }
    });
}

/// `true` while `channel` still has samples to play.
pub(super) fn sound_is_playing(channel: usize) -> bool {
    AUDIO.with_borrow(|audio| {
        audio
            .as_ref()
            .is_some_and(|audio| audio.is_playing(channel))
    })
}

/// Locate a `.sf2` SoundFont file for music playback.
///
/// Resolution order:
///  1. `-sf2 <path>` command-line argument (printed warning if missing).
///  2. Bundled SC-55 SoundFont under `soundfonts/...` relative to CWD.
///  3. Same path relative to the directory holding the executable.
///
/// Returns `None` and logs a warning if no font is found - music will
/// then play silently. Not present in the C source: chocolate-doom
/// uses an external timidity config instead.
fn find_soundfont_path() -> Option<PathBuf> {
    let p = M_CheckParmWithArgs("-sf2", 1);
    if p != 0 {
        let path = PathBuf::from(argument(p + 1));
        if path.exists() {
            return Some(path);
        }
        log::warn!("-sf2 path not found: {}", path.display());
    }

    let bundled = Path::new("soundfonts/SC55Soundfont-1.2b/SC-55 SoundFont v1.2b.sf2");
    if bundled.exists() {
        return Some(bundled.to_path_buf());
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let p = dir.join(bundled);
        if p.exists() {
            return Some(p);
        }
    }

    log::warn!("No soundfont found. Use -sf2 <path> to specify one. Music will be silent.");
    None
}

/// Load the SoundFont music is synthesised with. Music stays silent
/// without one.
pub(super) fn music_init() {
    if let Some(path) = find_soundfont_path() {
        AUDIO.with_borrow_mut(|audio| {
            if let Some(audio) = audio.as_mut() {
                audio.music.load_sound_font(&path);
            }
        });
    }
}

/// Play the Standard MIDI File `midi`, looping if `looping`.
pub(super) fn music_play(midi: &[u8], looping: bool) {
    AUDIO.with_borrow_mut(|audio| {
        if let Some(audio) = audio.as_mut() {
            let mixer = audio.mixer.clone();
            audio.music.play(midi, looping, &mixer);
        }
    });
}

/// Stop the current song.
pub(super) fn music_stop() {
    AUDIO.with_borrow_mut(|audio| {
        if let Some(audio) = audio.as_mut() {
            audio.music.stop();
        }
    });
}

/// Pause the current song.
pub(super) fn music_pause() {
    AUDIO.with_borrow(|audio| {
        if let Some(audio) = audio.as_ref() {
            audio.music.pause();
        }
    });
}

/// Resume a paused song.
pub(super) fn music_resume() {
    AUDIO.with_borrow(|audio| {
        if let Some(audio) = audio.as_ref() {
            audio.music.resume();
        }
    });
}

/// Set the music volume (`0..=127`).
pub(super) fn music_set_volume(volume: i32) {
    AUDIO.with_borrow(|audio| {
        if let Some(audio) = audio.as_ref() {
            audio.music.set_volume(volume);
        }
    });
}

/// `true` while a song is playing.
pub(super) fn music_is_playing() -> bool {
    AUDIO.with_borrow(|audio| audio.as_ref().is_some_and(|audio| audio.music.is_playing()))
}

// ---------------------------------------------------------------------------
// Audio state
// ---------------------------------------------------------------------------

thread_local! {
    /// Thread-local home for the singleton [`AudioState`].
    ///
    /// `None` until [`AudioState::new`] succeeds, after which callers in
    /// `doom::interface::sound` and `doom::sound` borrow it mutably to start/stop
    /// sounds.  Keeping it thread-local avoids needing a `Mutex` because Doom
    /// only ever drives audio from the main loop thread.
    pub(crate) static AUDIO: RefCell<Option<AudioState>> = const { RefCell::new(None) };
}

/// Singleton audio backend state held in the [`AUDIO`] thread-local.
///
/// Owns the output device sink, the shared mixer that all players feed into,
/// the eight Doom SFX channels, and the music state.
pub(crate) struct AudioState {
    /// The device-bound sink.  Held only to keep the audio device open for
    /// the lifetime of the engine; the mixer below is what we actually push
    /// samples through.
    pub(crate) _device_sink: MixerDeviceSink,
    /// Cloned handle to the device sink's mixer.  Cheap to clone (it is an
    /// `Arc` internally) and shared across all per-channel `Player`s and the
    /// music player.
    pub(crate) mixer: rodio::mixer::Mixer,
    /// Per-channel state for Doom's eight logical SFX channels.  `Box`ed so
    /// the `AudioState` itself stays small and movable.
    channels: Box<[sfx::ChannelState; 8]>,
    /// Music decoder/player state (MUS conversion, MIDI sequencing, volume).
    pub(crate) music: music::MusicState,
}

/// Construction and channel-control entry points used by the engine's
/// `S_sound` shims.
impl AudioState {
    /// Open the default audio device and build a fresh [`AudioState`].
    ///
    /// Returns an error if the platform refuses to open a default output
    /// device (e.g. no audio hardware, audio daemon not running).  On success
    /// the eight channel slots are empty (`player == None`) and music
    /// playback is idle.
    pub(crate) fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let device_sink = DeviceSinkBuilder::open_default_sink()?;
        let mixer = device_sink.mixer().clone();
        let channels = Box::new(std::array::from_fn(|_| sfx::ChannelState::new()));
        Ok(Self {
            _device_sink: device_sink,
            mixer,
            channels,
            music: music::MusicState::new(),
        })
    }

    /// Start playing a Doom DMX SFX blob on the given logical channel.
    ///
    /// `data` is the raw `DSxxxxx` lump bytes; `vol` is the Doom 0-127 volume
    /// scale; `sep` is the 0-255 stereo separation (`128` is centred);
    /// `channel` must be in `0..8`.
    ///
    /// Returns `true` on success, `false` if the channel index is out of
    /// range or if the SFX lump fails to decode.  Any previously playing
    /// sound on the same channel is replaced (its `Player` is dropped, which
    /// stops playback immediately).
    pub(crate) fn start_sound(&mut self, data: &[u8], vol: i32, sep: i32, channel: usize) -> bool {
        if channel >= 8 {
            return false;
        }
        let Some((sample_rate, samples)) = decode_doom_sfx(data) else {
            return false;
        };
        let pan = Arc::clone(&self.channels[channel].pan);
        pan.update(vol, sep);
        let buf = SamplesBuffer::new(
            std::num::NonZero::new(1u16).unwrap(),
            std::num::NonZero::new(sample_rate).unwrap(),
            samples,
        );
        let source = PannedSource::new(buf, pan);
        let player = Player::connect_new(&self.mixer);
        player.append(source);
        self.channels[channel].player = Some(player);
        true
    }

    /// Immediately stop whatever is playing on the given channel.
    ///
    /// Out-of-range `channel` values are silently ignored, matching the
    /// permissive behaviour of the SDL backend.
    pub(crate) fn stop_sound(&mut self, channel: usize) {
        if channel >= 8 {
            return;
        }
        self.channels[channel].player = None;
    }

    /// Update the volume and stereo separation of an in-flight sound without
    /// restarting it.
    ///
    /// Doom calls this every tic while a sound is active so the apparent
    /// position tracks the emitting `mobj`.  Out-of-range `channel` values
    /// are silently ignored.
    pub(crate) fn update_sound_params(&self, channel: usize, vol: i32, sep: i32) {
        if channel >= 8 {
            return;
        }
        self.channels[channel].pan.update(vol, sep);
    }

    /// Return `true` if the channel still has unconsumed samples.
    ///
    /// Out-of-range `channel` values report `false`.
    pub(crate) fn is_playing(&self, channel: usize) -> bool {
        if channel >= 8 {
            return false;
        }
        self.channels[channel]
            .player
            .as_ref()
            .is_some_and(|p| !p.empty())
    }
}
