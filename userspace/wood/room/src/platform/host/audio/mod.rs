//! Host audio: the `DG_Sound*` and `DG_Music*` hooks the engine's
//! `i_sound` calls, on top of the sfx and music submodules.
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
    ffi::{c_int, CStr},
    path::{Path, PathBuf},
    sync::Arc,
};

use rodio::{buffer::SamplesBuffer, DeviceSinkBuilder, MixerDeviceSink, Player};

use room::doom::m_argv::{myargv, M_CheckParmWithArgs};

pub(crate) use sfx::{decode_doom_sfx, PannedSource};

// ---------------------------------------------------------------------------
// DG_Sound* / DG_Music* hooks (C-callable)
// ---------------------------------------------------------------------------

/// The `0..8` SFX channel `channel` names, or `None`.
fn channel_index(channel: c_int) -> Option<usize> {
    usize::try_from(channel).ok().filter(|&index| index < 8)
}

/// Open the default audio device, once; non-zero when audio is available.
#[no_mangle]
pub extern "C" fn DG_SoundInit() -> c_int {
    AUDIO.with_borrow_mut(|audio| {
        if audio.is_none() {
            match AudioState::new() {
                Ok(state) => *audio = Some(state),
                Err(error) => log::warn!("Audio device: {error}"),
            }
        }
        c_int::from(audio.is_some())
    })
}

/// Close the audio device, stopping every sound and song.
#[no_mangle]
pub extern "C" fn DG_SoundShutdown() {
    AUDIO.with_borrow_mut(|audio| *audio = None);
}

/// Play the DMX sound lump `data[..len]` on `channel`; non-zero on success.
///
/// # Safety
///
/// `data` must point to `len` readable bytes for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn DG_SoundStart(
    data: *const u8,
    len: usize,
    channel: c_int,
    vol: c_int,
    sep: c_int,
) -> c_int {
    let Some(index) = channel_index(channel) else {
        return 0;
    };
    if data.is_null() {
        return 0;
    }
    // SAFETY: the caller guarantees `data[..len]` is readable.
    let lump = unsafe { std::slice::from_raw_parts(data, len) };
    AUDIO.with_borrow_mut(|audio| {
        c_int::from(
            audio
                .as_mut()
                .is_some_and(|audio| audio.start_sound(lump, vol, sep, index)),
        )
    })
}

/// Stop whatever is playing on `channel`.
#[no_mangle]
pub extern "C" fn DG_SoundStop(channel: c_int) {
    if let Some(index) = channel_index(channel) {
        AUDIO.with_borrow_mut(|audio| {
            if let Some(audio) = audio.as_mut() {
                audio.stop_sound(index);
            }
        });
    }
}

/// Change the volume and separation of the sound playing on `channel`.
#[no_mangle]
pub extern "C" fn DG_SoundUpdate(channel: c_int, vol: c_int, sep: c_int) {
    if let Some(index) = channel_index(channel) {
        AUDIO.with_borrow(|audio| {
            if let Some(audio) = audio.as_ref() {
                audio.update_sound_params(index, vol, sep);
            }
        });
    }
}

/// Non-zero while `channel` still has samples to play.
#[no_mangle]
pub extern "C" fn DG_SoundIsPlaying(channel: c_int) -> c_int {
    channel_index(channel).map_or(0, |index| {
        AUDIO.with_borrow(|audio| {
            c_int::from(audio.as_ref().is_some_and(|audio| audio.is_playing(index)))
        })
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
    // SAFETY: the engine set up `myargv` before calling `DG_MusicInit`.
    unsafe {
        let p = M_CheckParmWithArgs(c"-sf2".as_ptr(), 1);
        if p != 0 {
            let arg = *myargv.add((p + 1) as usize);
            if !arg.is_null() {
                let s = CStr::from_ptr(arg).to_string_lossy();
                let path = PathBuf::from(s.as_ref());
                if path.exists() {
                    return Some(path);
                }
                log::warn!("-sf2 path not found: {}", path.display());
            }
        }
    }

    let bundled = Path::new("soundfonts/SC55Soundfont-1.2b/SC-55 SoundFont v1.2b.sf2");
    if bundled.exists() {
        return Some(bundled.to_path_buf());
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join(bundled);
            if p.exists() {
                return Some(p);
            }
        }
    }

    log::warn!("No soundfont found. Use -sf2 <path> to specify one. Music will be silent.");
    None
}

/// Load the SoundFont music is synthesised with. Music stays silent
/// without one.
#[no_mangle]
pub extern "C" fn DG_MusicInit() {
    if let Some(path) = find_soundfont_path() {
        AUDIO.with_borrow_mut(|audio| {
            if let Some(audio) = audio.as_mut() {
                audio.music.load_sound_font(&path);
            }
        });
    }
}

/// Play the Standard MIDI File `midi[..len]`, looping if `looping` is non-zero.
///
/// # Safety
///
/// `midi` must point to `len` readable bytes for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn DG_MusicPlay(midi: *const u8, len: usize, looping: c_int) {
    if midi.is_null() {
        return;
    }
    // SAFETY: the caller guarantees `midi[..len]` is readable.
    let song = unsafe { std::slice::from_raw_parts(midi, len) };
    AUDIO.with_borrow_mut(|audio| {
        if let Some(audio) = audio.as_mut() {
            let mixer = audio.mixer.clone();
            audio.music.play(song, looping != 0, &mixer);
        }
    });
}

/// Stop the current song.
#[no_mangle]
pub extern "C" fn DG_MusicStop() {
    AUDIO.with_borrow_mut(|audio| {
        if let Some(audio) = audio.as_mut() {
            audio.music.stop();
        }
    });
}

/// Pause the current song.
#[no_mangle]
pub extern "C" fn DG_MusicPause() {
    AUDIO.with_borrow(|audio| {
        if let Some(audio) = audio.as_ref() {
            audio.music.pause();
        }
    });
}

/// Resume a paused song.
#[no_mangle]
pub extern "C" fn DG_MusicResume() {
    AUDIO.with_borrow(|audio| {
        if let Some(audio) = audio.as_ref() {
            audio.music.resume();
        }
    });
}

/// Set the music volume (`0..=127`).
#[no_mangle]
pub extern "C" fn DG_MusicSetVolume(volume: c_int) {
    AUDIO.with_borrow(|audio| {
        if let Some(audio) = audio.as_ref() {
            audio.music.set_volume(volume);
        }
    });
}

/// Non-zero while a song is playing.
#[no_mangle]
pub extern "C" fn DG_MusicIsPlaying() -> c_int {
    AUDIO.with_borrow(|audio| {
        c_int::from(audio.as_ref().is_some_and(|audio| audio.music.is_playing()))
    })
}

// ---------------------------------------------------------------------------
// Audio state
// ---------------------------------------------------------------------------

thread_local! {
    /// Thread-local home for the singleton [`AudioState`].
    ///
    /// `None` until [`AudioState::new`] succeeds, after which callers in
    /// `doom::i_sound` and `doom::s_sound` borrow it mutably to start/stop
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
