//! Rust port of vendor/doomgeneric/i_sound.c.
//!
//! Low-level sound and music interface. The C source delegates every
//! call to a `sound_module_t` / `music_module_t` vtable (set up by
//! `InitSfxModule` / `InitMusicModule`). This port collapses that
//! indirection into the platform's sound and music methods
//! ([`Platform`](crate::doom::doomgeneric::Platform)). SFX lumps are
//! handed over as raw DMX bytes; MUS songs are converted to MIDI here
//! ([`mus2midi`]) before they reach the platform.
//!
//! Public `static mut` `snd_*` globals are bound by address in
//! `m_config` as user-configurable variables.

#![allow(non_upper_case_globals, non_snake_case)]

use {
    crate::doom::{
        doomgeneric::platform,
        m_config::M_BindVariable,
        mus2midi::mus2midi,
        sounds::SfxInfo,
        w_wad::{W_CacheLumpNum, W_CheckNumForName, W_LumpLength},
        z_zone::PU_CACHE,
    },
    std::ffi::{c_char, c_int, c_uint, c_void},
};

use crate::types::Boolean;

/// A song registered with `I_RegisterSong`: its MUS lump converted to a
/// Standard MIDI File, handed to the platform by `I_PlaySong`.
struct MusicHandle {
    midi_bytes: Vec<u8>,
}

/// Music device selector. C default is `SNDDEVICE_SB` (1); this port
/// uses `3` (`SNDDEVICE_SB_PRO`) to match the original chocolate-doom
/// behaviour observed at runtime. Configurable via `m_config`.
pub static mut snd_musicdevice: c_int = 3;

/// SFX device selector. Same notes as `snd_musicdevice`.
pub static mut snd_sfxdevice: c_int = 3;

/// Output sample rate for digital sound, in Hz. Mirrors
/// `snd_samplerate` from `i_sound.c`. Bound via `m_config`.
pub static mut snd_samplerate: c_int = 44100;

/// Maximum number of bytes the SFX cache may hold. Default 64 MiB,
/// matching the C source. Bound via `m_config`.
pub static mut snd_cachesize: c_int = 64 * 1024 * 1024;

/// Mixer slice length in milliseconds. Default 28 ms (~1 buffer per
/// 35 Hz tic). Mirrors `snd_maxslicetime_ms` in `i_sound.c`.
pub static mut snd_maxslicetime_ms: c_int = 28;

/// Legacy SoundBlaster I/O port. Retained only so the config file
/// stays cross-compatible with chocolate-doom; not used at runtime.
pub static mut snd_sbport: c_int = 0;

/// Legacy SoundBlaster IRQ. Same note as `snd_sbport`.
pub static mut snd_sbirq: c_int = 0;

/// Legacy SoundBlaster DMA channel. Same note as `snd_sbport`.
pub static mut snd_sbdma: c_int = 0;

/// Legacy MPU-401 MIDI port. Same note as `snd_sbport`.
pub static mut snd_mport: c_int = 0;

/// Pitch-shift toggle. Not consumed by the music hooks;
/// preserved for config-file compatibility.
pub static mut snd_pitchshift: c_int = 0;

/// Backing storage for the empty default `snd_musiccmd` C string.
/// `m_config` may rewrite `snd_musiccmd` to point at a different
/// buffer; this static merely supplies the initial NUL byte.
static mut EMPTY_CSTR: [c_char; 1] = [0];

/// External shell command used by the original chocolate-doom to play
/// back music via an external process. Unused by this port (the
/// platform's music methods handle playback directly) but kept
/// for config-file compatibility. Starts pointing at `EMPTY_CSTR`.
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

/// Initialise the audio subsystem through the platform's `sound_init`.
///
/// On failure the game continues silently (with a `log::warn!`),
/// matching the C source's tolerance of missing sound devices.
///
/// The `use_sfx_prefix` flag is accepted for API compatibility but
/// ignored: the bundled SFX module always uses the `DS` lump-name
/// prefix encoded in `I_GetSfxLumpNum`.
pub fn I_InitSound(_use_sfx_prefix: Boolean) {
    if platform().sound_init() {
        log::info!("Audio initialised");
    } else {
        log::warn!("Audio init failed (running silent)");
    }
}

/// Tear down the audio subsystem. Safe to call even if `I_InitSound`
/// was never run.
pub fn I_ShutdownSound() {
    platform().sound_shutdown();
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
pub fn I_GetSfxLumpNum(sfxinfo: *mut c_void) -> c_int {
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

/// Per-frame sound update hook. The platform mixes audio on its own,
/// so this is a no-op; the symbol exists because the engine main loop
/// calls it every tic.
pub fn I_UpdateSound() {}

/// Update volume (`vol`, 0..=127) and stereo separation (`sep`,
/// 0..=254) for the SFX currently playing on `channel`.
///
/// Channel numbers outside `0..8` are ignored (the engine uses a
/// fixed 8-channel mixer). The C source clamps `vol`/`sep` via
/// `CheckVolumeSeparation`; the platform mixer performs equivalent
/// clamping.
pub fn I_UpdateSoundParams(channel: c_int, vol: c_int, sep: c_int) {
    if !(0..8).contains(&channel) {
        return;
    }
    platform().sound_update(channel as usize, vol, sep);
}

/// Begin playing an SFX on `channel` with the given volume and
/// separation. Returns the channel number on success or `-1` on any
/// failure (null `sfxinfo`, invalid channel, missing lump, mixer
/// rejection).
///
/// Loads the sample data via `W_CacheLumpNum` with `PU_CACHE`, then
/// hands the lump bytes to the platform's `sound_start`.
/// The cache tag means the data may be evicted once the SFX finishes.
pub fn I_StartSound(sfxinfo: *mut c_void, channel: c_int, vol: c_int, sep: c_int) -> c_int {
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
        let lump = std::slice::from_raw_parts(ptr as *const u8, lump_len as usize);
        if platform().sound_start(lump, channel as usize, vol, sep) {
            channel
        } else {
            -1
        }
    }
}

/// Stop the SFX currently playing on `channel`. Out-of-range channel
/// numbers are silently ignored.
pub fn I_StopSound(channel: c_int) {
    if !(0..8).contains(&channel) {
        return;
    }
    platform().sound_stop(channel as usize);
}

/// Return 1 if an SFX is currently audible on `channel`, 0 otherwise
/// (including for out-of-range channels). Mirrors `I_SoundIsPlaying`
/// from `i_sound.c`.
pub fn I_SoundIsPlaying(channel: c_int) -> c_int {
    if !(0..8).contains(&channel) {
        return 0;
    }
    c_int::from(platform().sound_is_playing(channel as usize))
}

/// Hook for the SDL backend to pre-cache a batch of sounds. Sounds are
/// handed to the platform as they start, so this is a no-op. Kept for
/// API compatibility with the engine.
pub fn I_PrecacheSounds(_sounds: *mut c_void, _num_sounds: c_int) {}

/// Initialise the music backend through the platform's `music_init`.
pub fn I_InitMusic() {
    platform().music_init();
}

/// Stop any music playback. Mirrors `I_ShutdownMusic` from `i_sound.c`.
pub fn I_ShutdownMusic() {
    platform().music_stop();
}

/// Set the music output volume. The accepted range matches the
/// engine convention (0..=127); the platform clamps it.
pub fn I_SetMusicVolume(volume: c_int) {
    platform().music_set_volume(volume);
}

/// Pause the currently playing song without unloading it.
pub fn I_PauseSong() {
    platform().music_pause();
}

/// Resume a song that was paused via `I_PauseSong`.
pub fn I_ResumeSong() {
    platform().music_resume();
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
/// conversion up front so the platform only ever plays MIDI.
pub fn I_RegisterSong(data: *mut c_void, len: c_int) -> *mut c_void {
    if data.is_null() || len <= 0 {
        return std::ptr::null_mut();
    }
    let slice = unsafe { std::slice::from_raw_parts(data as *const u8, len as usize) };
    match mus2midi(slice) {
        Some(midi_bytes) => Box::into_raw(Box::new(MusicHandle { midi_bytes })) as *mut c_void,
        None => {
            log::warn!("I_RegisterSong: MUS-to-MIDI conversion failed");
            std::ptr::null_mut()
        }
    }
}

/// Free a music handle previously returned by `I_RegisterSong`.
/// Null pointers are ignored. After this call the handle is invalid.
pub fn I_UnRegisterSong(handle: *mut c_void) {
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
/// the call; the platform copies what it needs to keep playing.
pub fn I_PlaySong(handle: *mut c_void, looping: c_int) {
    if handle.is_null() {
        return;
    }
    let music_handle = unsafe { &*(handle as *const MusicHandle) };
    platform().music_play(&music_handle.midi_bytes, looping != 0);
}

/// Stop the currently playing song. The handle itself remains valid
/// and can be replayed later. Mirrors `I_StopSong` from `i_sound.c`.
pub fn I_StopSong() {
    platform().music_stop();
}

/// Return 1 if a song is currently audible, 0 otherwise.
/// Mirrors `I_MusicIsPlaying` from `i_sound.c`.
pub fn I_MusicIsPlaying() -> c_int {
    c_int::from(platform().music_is_playing())
}

/// Bind the `snd_*` globals to `m_config` so they survive across
/// runs via the config file. Mirrors `I_BindSoundVariables` from
/// `i_sound.c`. The C source also binds `use_libsamplerate` and
/// `libsamplerate_scale` under `FEATURE_SOUND`; those are omitted
/// here because this port doesn't use libsamplerate.
pub fn I_BindSoundVariables() {
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
