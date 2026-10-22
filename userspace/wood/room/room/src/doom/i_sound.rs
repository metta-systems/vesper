//! Rust port of vendor/doomgeneric/i_sound.c.
//!
//! Low-level sound and music interface. The C source delegates every
//! call to a `sound_module_t` / `music_module_t` vtable (set up by
//! `InitSfxModule` / `InitMusicModule`). This port collapses that
//! indirection into a single concrete backend living in the
//! `crate::audio` module: SFX are mixed via the platform audio state,
//! and MIDI/MUS playback is routed through rustysynth using a bundled
//! Sound Canvas SC-55 SoundFont.
//!
//! Each `#[no_mangle] pub extern "C"` symbol keeps the original Doom
//! name so the rest of the engine links unchanged. Public `static mut`
//! `snd_*` globals retain C linkage because `m_config.c` binds them
//! by address as user-configurable variables.

#![allow(non_upper_case_globals, non_snake_case)]

use crate::audio::music::MusicHandle;
use crate::doom::m_argv::{myargv, M_CheckParmWithArgs};
use crate::doom::m_config::M_BindVariable;
use crate::doom::sounds::SfxInfo;
use crate::doom::w_wad::{W_CacheLumpNum, W_CheckNumForName, W_LumpLength};
use crate::doom::z_zone::PU_CACHE;
use std::ffi::{c_char, c_int, c_uint, c_void};

use crate::types::Boolean;

/// Music device selector. C default is `SNDDEVICE_SB` (1); this port
/// uses `3` (`SNDDEVICE_SB_PRO`) to match the original chocolate-doom
/// behaviour observed at runtime. Configurable via `m_config`.
#[no_mangle]
pub static mut snd_musicdevice: c_int = 3;

/// SFX device selector. Same notes as `snd_musicdevice`.
#[no_mangle]
pub static mut snd_sfxdevice: c_int = 3;

/// Output sample rate for digital sound, in Hz. Mirrors
/// `snd_samplerate` from `i_sound.c`. Bound via `m_config`.
#[no_mangle]
pub static mut snd_samplerate: c_int = 44100;

/// Maximum number of bytes the SFX cache may hold. Default 64 MiB,
/// matching the C source. Bound via `m_config`.
#[no_mangle]
pub static mut snd_cachesize: c_int = 64 * 1024 * 1024;

/// Mixer slice length in milliseconds. Default 28 ms (~1 buffer per
/// 35 Hz tic). Mirrors `snd_maxslicetime_ms` in `i_sound.c`.
#[no_mangle]
pub static mut snd_maxslicetime_ms: c_int = 28;

/// Legacy SoundBlaster I/O port. Retained only so the config file
/// stays cross-compatible with chocolate-doom; not used at runtime.
#[no_mangle]
pub static mut snd_sbport: c_int = 0;

/// Legacy SoundBlaster IRQ. Same note as `snd_sbport`.
#[no_mangle]
pub static mut snd_sbirq: c_int = 0;

/// Legacy SoundBlaster DMA channel. Same note as `snd_sbport`.
#[no_mangle]
pub static mut snd_sbdma: c_int = 0;

/// Legacy MPU-401 MIDI port. Same note as `snd_sbport`.
#[no_mangle]
pub static mut snd_mport: c_int = 0;

/// Pitch-shift toggle. Not consumed by the rustysynth music backend;
/// preserved for config-file compatibility.
#[no_mangle]
pub static mut snd_pitchshift: c_int = 0;

/// Backing storage for the empty default `snd_musiccmd` C string.
/// `m_config` may rewrite `snd_musiccmd` to point at a different
/// buffer; this static merely supplies the initial NUL byte.
static mut EMPTY_CSTR: [c_char; 1] = [0];

/// External shell command used by the original chocolate-doom to play
/// back music via an external process. Unused by this port (the
/// `crate::audio::music` backend handles playback directly) but kept
/// for config-file compatibility. Starts pointing at `EMPTY_CSTR`.
#[no_mangle]
pub static mut snd_musiccmd: *mut c_char = unsafe { std::ptr::addr_of_mut!(EMPTY_CSTR[0]) };

/// Reinterpret a `&[u8]` as a `&[c_char]` without copying.
///
/// `c_char` is `i8` on most targets; this helper lets `M_BindVariable`
/// calls below use byte-string literals while passing the same memory
/// as a C-char slice. The signedness reinterpretation is sound
/// because the byte values are pure ASCII.
const fn c_bytes(s: &[u8]) -> &'static [c_char] {
    unsafe { &*(s as *const [u8] as *const [c_char]) }
}

/// Initialise the audio subsystem.
///
/// Allocates the global `AUDIO` state if it has not been created yet.
/// On failure the game continues silently (with a `log::warn!`),
/// matching the C source's tolerance of missing sound devices.
///
/// The `use_sfx_prefix` flag is accepted for API compatibility but
/// ignored: the bundled SFX module always uses the `DS` lump-name
/// prefix encoded in `I_GetSfxLumpNum`.
#[no_mangle]
pub extern "C" fn I_InitSound(_use_sfx_prefix: Boolean) {
    crate::audio::AUDIO.with_borrow_mut(|audio| {
        if audio.is_none() {
            match crate::audio::AudioState::new() {
                Ok(state) => {
                    log::info!("Audio initialised");
                    *audio = Some(state);
                }
                Err(e) => log::warn!("Audio init failed (running silent): {e}"),
            }
        }
    });
}

/// Tear down the audio subsystem by dropping the global `AUDIO`
/// state. Safe to call even if `I_InitSound` was never run.
#[no_mangle]
pub extern "C" fn I_ShutdownSound() {
    crate::audio::AUDIO.with_borrow_mut(|audio| *audio = None);
}

/// Resolve the WAD lump number for a sound effect's sample data.
///
/// Walks the `link` chain so aliased SFX (`SfxInfo::link` non-null)
/// inherit the lump of their target, up to a depth limit of 64 to
/// avoid cycles. The lump name is `DS` plus the first six characters
/// of `SfxInfo::name`. Returns `-1` if `sfxinfo` is null or the lump
/// is not present.
///
/// In the C source this is delegated to `sound_module->GetSfxLumpNum`
/// and the lump-name construction lives in the SDL backend. This port
/// inlines the construction here.
#[no_mangle]
pub extern "C" fn I_GetSfxLumpNum(sfxinfo: *mut c_void) -> c_int {
    if sfxinfo.is_null() {
        return -1;
    }
    unsafe {
        let mut sfx = sfxinfo as *const SfxInfo;
        let mut depth = 0usize;
        while !(*sfx).link.is_null() && depth < 64 {
            sfx = (*sfx).link;
            depth += 1;
        }
        let mut lump_name = [0 as c_char; 9];
        lump_name[0] = b'D' as c_char;
        lump_name[1] = b'S' as c_char;
        for (i, &c) in (*sfx).name.iter().take(6).enumerate() {
            if c == 0 {
                break;
            }
            lump_name[2 + i] = c;
        }
        W_CheckNumForName(lump_name.as_ptr())
    }
}

/// Per-frame sound update hook. The rustysynth/rodio backend mixes
/// audio on its own thread so this is a no-op; the symbol exists
/// because the engine main loop calls it every tic.
#[no_mangle]
pub extern "C" fn I_UpdateSound() {}

/// Update volume (`vol`, 0..=127) and stereo separation (`sep`,
/// 0..=254) for the SFX currently playing on `channel`.
///
/// Channel numbers outside `0..8` are ignored (the engine uses a
/// fixed 8-channel mixer). The C source clamps `vol`/`sep` via
/// `CheckVolumeSeparation`; the underlying Rust mixer in
/// `crate::audio` performs equivalent clamping internally.
#[no_mangle]
pub extern "C" fn I_UpdateSoundParams(channel: c_int, vol: c_int, sep: c_int) {
    if !(0..8).contains(&channel) {
        return;
    }
    crate::audio::AUDIO.with_borrow(|audio| {
        if let Some(a) = audio.as_ref() {
            a.update_sound_params(channel as usize, vol, sep);
        }
    });
}

/// Begin playing an SFX on `channel` with the given volume and
/// separation. Returns the channel number on success or `-1` on any
/// failure (null `sfxinfo`, invalid channel, missing lump, mixer
/// rejection).
///
/// Loads the sample data via `W_CacheLumpNum` with `PU_CACHE`, then
/// hands a borrowed byte slice to `crate::audio::AudioState::start_sound`.
/// The cache tag means the data may be evicted once the SFX finishes.
#[no_mangle]
pub extern "C" fn I_StartSound(
    sfxinfo: *mut c_void,
    channel: c_int,
    vol: c_int,
    sep: c_int,
) -> c_int {
    if sfxinfo.is_null() || !(0..8).contains(&channel) {
        return -1;
    }
    unsafe {
        let sfx = sfxinfo as *const SfxInfo;
        let lumpnum = (*sfx).lumpnum;
        if lumpnum < 0 {
            return -1;
        }
        let lump_len = W_LumpLength(lumpnum as c_uint);
        if lump_len <= 0 {
            return -1;
        }
        let ptr = W_CacheLumpNum(lumpnum, PU_CACHE);
        if ptr.is_null() {
            return -1;
        }
        let data = std::slice::from_raw_parts(ptr as *const u8, lump_len as usize);
        let mut result = -1;
        crate::audio::AUDIO.with_borrow_mut(|audio| {
            if let Some(a) = audio.as_mut() {
                if a.start_sound(data, vol, sep, channel as usize) {
                    result = channel;
                }
            }
        });
        result
    }
}

/// Stop the SFX currently playing on `channel`. Out-of-range channel
/// numbers are silently ignored.
#[no_mangle]
pub extern "C" fn I_StopSound(channel: c_int) {
    if !(0..8).contains(&channel) {
        return;
    }
    crate::audio::AUDIO.with_borrow_mut(|audio| {
        if let Some(a) = audio.as_mut() {
            a.stop_sound(channel as usize);
        }
    });
}

/// Return 1 if an SFX is currently audible on `channel`, 0 otherwise
/// (including for out-of-range channels). Mirrors `I_SoundIsPlaying`
/// from `i_sound.c`.
#[no_mangle]
pub extern "C" fn I_SoundIsPlaying(channel: c_int) -> c_int {
    if !(0..8).contains(&channel) {
        return 0;
    }
    let mut playing = 0;
    crate::audio::AUDIO.with_borrow(|audio| {
        if let Some(a) = audio.as_ref() {
            playing = a.is_playing(channel as usize) as c_int;
        }
    });
    playing
}

/// Hook for the SDL backend to pre-cache a batch of sounds. The
/// rustysynth/rodio backend caches lazily on `I_StartSound`, so this
/// is a no-op. Kept for API compatibility with the engine.
#[no_mangle]
pub extern "C" fn I_PrecacheSounds(_sounds: *mut c_void, _num_sounds: c_int) {}

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
fn find_soundfont_path() -> Option<std::path::PathBuf> {
    unsafe {
        let p = M_CheckParmWithArgs(c"-sf2".as_ptr().cast_mut(), 1);
        if p != 0 {
            let arg = *myargv.add((p + 1) as usize);
            if !arg.is_null() {
                let s = std::ffi::CStr::from_ptr(arg).to_string_lossy();
                let path = std::path::PathBuf::from(s.as_ref());
                if path.exists() {
                    return Some(path);
                }
                log::warn!("-sf2 path not found: {}", path.display());
            }
        }
    }

    let bundled = std::path::Path::new("soundfonts/SC55Soundfont-1.2b/SC-55 SoundFont v1.2b.sf2");
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

/// Initialise the music backend by locating a SoundFont and loading
/// it into the audio module. No-op if no SoundFont can be found.
#[no_mangle]
pub extern "C" fn I_InitMusic() {
    if let Some(path) = find_soundfont_path() {
        crate::audio::AUDIO.with_borrow_mut(|audio| {
            if let Some(a) = audio.as_mut() {
                a.music.load_sound_font(&path);
            }
        });
    }
}

/// Stop any music playback and tear down music-specific state.
/// Mirrors `I_ShutdownMusic` from `i_sound.c`.
#[no_mangle]
pub extern "C" fn I_ShutdownMusic() {
    crate::audio::AUDIO.with_borrow_mut(|audio| {
        if let Some(a) = audio.as_mut() {
            a.music.stop();
        }
    });
}

/// Set the music output volume. The accepted range matches the
/// engine convention (0..=127); the music backend clamps internally.
#[no_mangle]
pub extern "C" fn I_SetMusicVolume(volume: c_int) {
    crate::audio::AUDIO.with_borrow(|audio| {
        if let Some(a) = audio.as_ref() {
            a.music.set_volume(volume);
        }
    });
}

/// Pause the currently playing song without unloading it.
#[no_mangle]
pub extern "C" fn I_PauseSong() {
    crate::audio::AUDIO.with_borrow(|audio| {
        if let Some(a) = audio.as_ref() {
            a.music.pause();
        }
    });
}

/// Resume a song that was paused via `I_PauseSong`.
#[no_mangle]
pub extern "C" fn I_ResumeSong() {
    crate::audio::AUDIO.with_borrow(|audio| {
        if let Some(a) = audio.as_ref() {
            a.music.resume();
        }
    });
}

/// Register a song for later playback by converting its MUS payload
/// to MIDI and returning an opaque `*mut MusicHandle` cast to
/// `*mut c_void`.
///
/// Returns null on null input, zero/negative length, or if MUS-to-MIDI
/// conversion fails. The caller must release the returned handle via
/// `I_UnRegisterSong`; the boxed handle owns the MIDI bytes.
///
/// In the C source the song data is the raw MUS lump, which the SDL
/// backend hands to libtimidity. This port performs the MUS->MIDI
/// conversion up front so the rustysynth player can consume it.
#[no_mangle]
pub extern "C" fn I_RegisterSong(data: *mut c_void, len: c_int) -> *mut c_void {
    if data.is_null() || len <= 0 {
        return std::ptr::null_mut();
    }
    let slice = unsafe { std::slice::from_raw_parts(data as *const u8, len as usize) };
    match crate::audio::music::mus2midi(slice) {
        Some(midi_bytes) => Box::into_raw(Box::new(MusicHandle { midi_bytes })) as *mut c_void,
        None => {
            log::warn!("I_RegisterSong: MUS-to-MIDI conversion failed");
            std::ptr::null_mut()
        }
    }
}

/// Free a music handle previously returned by `I_RegisterSong`.
/// Null pointers are ignored. After this call the handle is invalid.
#[no_mangle]
pub extern "C" fn I_UnRegisterSong(handle: *mut c_void) {
    if !handle.is_null() {
        unsafe {
            drop(Box::from_raw(handle as *mut MusicHandle));
        }
    }
}

/// Start playing the song referenced by `handle`. If `looping` is
/// non-zero the song is looped indefinitely. No-op on null handle.
///
/// The MIDI bytes inside the handle are borrowed for the duration of
/// the call; the music backend internally clones them into its own
/// playback thread.
#[no_mangle]
pub extern "C" fn I_PlaySong(handle: *mut c_void, looping: c_int) {
    if handle.is_null() {
        return;
    }
    let music_handle = unsafe { &*(handle as *const MusicHandle) };
    crate::audio::AUDIO.with_borrow_mut(|audio| {
        if let Some(a) = audio.as_mut() {
            let mixer = a.mixer.clone();
            a.music.play(&music_handle.midi_bytes, looping != 0, &mixer);
        }
    });
}

/// Stop the currently playing song. The handle itself remains valid
/// and can be replayed later. Mirrors `I_StopSong` from `i_sound.c`.
#[no_mangle]
pub extern "C" fn I_StopSong() {
    crate::audio::AUDIO.with_borrow_mut(|audio| {
        if let Some(a) = audio.as_mut() {
            a.music.stop();
        }
    });
}

/// Return 1 if a song is currently audible, 0 otherwise.
/// Mirrors `I_MusicIsPlaying` from `i_sound.c`.
#[no_mangle]
pub extern "C" fn I_MusicIsPlaying() -> c_int {
    let mut playing = 0;
    crate::audio::AUDIO.with_borrow(|audio| {
        if let Some(a) = audio.as_ref() {
            playing = a.music.is_playing() as c_int;
        }
    });
    playing
}

/// Bind the `snd_*` globals to `m_config` so they survive across
/// runs via the config file. Mirrors `I_BindSoundVariables` from
/// `i_sound.c`. The C source also binds `use_libsamplerate` and
/// `libsamplerate_scale` under `FEATURE_SOUND`; those are omitted
/// here because this port doesn't use libsamplerate.
#[no_mangle]
pub extern "C" fn I_BindSoundVariables() {
    M_BindVariable(
        c_bytes(b"snd_musicdevice\0").as_ptr() as *mut c_char,
        &raw mut snd_musicdevice as *mut c_int as *mut c_void,
    );
    M_BindVariable(
        c_bytes(b"snd_sfxdevice\0").as_ptr() as *mut c_char,
        &raw mut snd_sfxdevice as *mut c_int as *mut c_void,
    );
    M_BindVariable(
        c_bytes(b"snd_sbport\0").as_ptr() as *mut c_char,
        &raw mut snd_sbport as *mut c_int as *mut c_void,
    );
    M_BindVariable(
        c_bytes(b"snd_sbirq\0").as_ptr() as *mut c_char,
        &raw mut snd_sbirq as *mut c_int as *mut c_void,
    );
    M_BindVariable(
        c_bytes(b"snd_sbdma\0").as_ptr() as *mut c_char,
        &raw mut snd_sbdma as *mut c_int as *mut c_void,
    );
    M_BindVariable(
        c_bytes(b"snd_mport\0").as_ptr() as *mut c_char,
        &raw mut snd_mport as *mut c_int as *mut c_void,
    );
    M_BindVariable(
        c_bytes(b"snd_maxslicetime_ms\0").as_ptr() as *mut c_char,
        &raw mut snd_maxslicetime_ms as *mut c_int as *mut c_void,
    );
    M_BindVariable(
        c_bytes(b"snd_musiccmd\0").as_ptr() as *mut c_char,
        &raw mut snd_musiccmd as *mut c_void,
    );
    M_BindVariable(
        c_bytes(b"snd_samplerate\0").as_ptr() as *mut c_char,
        &raw mut snd_samplerate as *mut c_int as *mut c_void,
    );
    M_BindVariable(
        c_bytes(b"snd_cachesize\0").as_ptr() as *mut c_char,
        &raw mut snd_cachesize as *mut c_int as *mut c_void,
    );
}
