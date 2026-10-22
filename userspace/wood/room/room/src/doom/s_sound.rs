//! Rust port of `vendor/doomgeneric/s_sound.c`.
//!
//! High-level sound subsystem: owns the pool of mixing channels, distance and
//! stereo attenuation, music playback control and the SFX lookup table. The
//! low-level driver work (lump fetching, mixing, output) lives in `i_sound`.
//!
//! Channel allocation mirrors the C version: a fixed-size `channel_t` array is
//! allocated from zone memory at startup, then `S_GetChannel` picks a free
//! channel or evicts one of equal-or-lower priority. Stereo and volume are
//! derived from listener-to-source distance and angle.
//!
//! Notable Rust-vs-C differences:
//! - `MobjStub` is a layout-compatible prefix of `mobj_t` (see the struct doc).
//!   Only the fields actually touched by attenuation are named; the rest are
//!   opaque padding so we do not need to mirror `r_defs::mobj_t` here.
//! - C linkage is preserved via `#[no_mangle]` on every exported function and
//!   tunable global so other ported modules and the test harness keep working.

#![allow(non_upper_case_globals, non_snake_case)]

use std::ffi::{c_char, c_int, c_uint, c_void};

use crate::types::Boolean;

use crate::doom::d_mode;
use crate::doom::m_fixed::{FRACBITS, FRACUNIT};
use crate::doom::sounds::{
    Mus, MusicInfo, S_InitSfxLinks, S_music, S_sfx, SfxInfo, NUMMUSIC, NUMSFX,
};
use crate::doom::tables::{finesine, ANGLETOFINESHIFT};
use crate::doom::z_zone::PU_STATIC;

/// Distance, in fixed-point units, beyond which a positional sound is clipped
/// out entirely (`S_CLIPPING_DIST` in `s_sound.c`).
const S_CLIPPING_DIST: c_int = 1200 * FRACUNIT;
/// Distance, in fixed-point units, within which a sound plays at full SFX
/// volume (`S_CLOSE_DIST` in `s_sound.c`).
const S_CLOSE_DIST: c_int = 200 * FRACUNIT;
/// Integer attenuation range between `S_CLOSE_DIST` and `S_CLIPPING_DIST`,
/// shifted out of fixed-point. Used as the divisor in the linear volume falloff.
const S_ATTENUATOR: c_int = (S_CLIPPING_DIST - S_CLOSE_DIST) >> FRACBITS;
/// Maximum stereo swing applied to `sep` based on listener-to-source angle, in
/// fixed-point units.
const S_STEREO_SWING: c_int = 96 * FRACUNIT;
/// Pitch value treated as "no pitch shift" by the driver.
const NORM_PITCH: c_int = 128;
/// Default channel priority when none is supplied.
const NORM_PRIORITY: c_int = 64;
/// Stereo separation value meaning "centred" (0 = full left, 255 = full right).
const NORM_SEP: c_int = 128;

/// Mirror of `MAXPLAYERS` from `doomdef.h`; used to index `players[]`.
const MAXPLAYERS: usize = 4;

/// Music device id for AdLib, as recognised by `snd_musicdevice` config.
const SNDDEVICE_ADLIB: c_int = 2;
/// Music device id for SoundBlaster, as recognised by `snd_musicdevice`.
const SNDDEVICE_SB: c_int = 3;

/// Mixing channel slot. C counterpart: `channel_t` defined inside `s_sound.c`.
///
/// A null `sfxinfo` marks the slot as free. `origin` points at the mobj the
/// sound is attached to (used to keep stereo/volume in sync while it moves);
/// it is `null` for player-local sounds. `handle` is the driver's opaque id.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct channel_t {
    /// Currently playing sound's metadata, or null if the channel is free.
    pub sfxinfo: *mut SfxInfo,
    /// Map object the sound emanates from, or null for non-positional sounds.
    pub origin: *mut MobjStub,
    /// Driver-side handle returned by `I_StartSound` for this channel.
    pub handle: c_int,
}

/// Layout-compatible prefix of `mobj_t` (`p_mobj.h`).
///
/// The sound subsystem only needs the positional/orientation fields of an
/// mobj, so we expose just those and keep the rest as opaque padding. The
/// 24-byte `thinker_t` prefix MUST come first or sound effects whose `origin`
/// is not the player will be positioned at garbage coordinates (see
/// `MEMORY.md`'s `MobjStub layout bug` note).
#[repr(C)]
pub struct MobjStub {
    // thinker_t prefix (3 pointers * 8 bytes = 24 bytes on 64-bit)
    /// Padding: `thinker_t.prev` pointer.
    _thinker_prev: *mut c_void,
    /// Padding: `thinker_t.next` pointer.
    _thinker_next: *mut c_void,
    /// Padding: `thinker_t.function` pointer.
    _thinker_fn: *mut c_void,
    // mobj_t positional fields
    /// World X coordinate, fixed-point.
    pub x: c_int,
    /// World Y coordinate, fixed-point.
    pub y: c_int,
    /// Padding: `mobj_t.z`.
    _z: c_int,
    /// Padding: alignment slot before the sector list pointers.
    _pad0: u32,
    /// Padding: `mobj_t.snext`.
    _snext: *mut c_void,
    /// Padding: `mobj_t.sprev`.
    _sprev: *mut c_void,
    /// Facing angle of the mobj, BAM (binary angle) units.
    pub angle: u32,
}

// doomstat.rs
use crate::doom::doomstat::gamemode;

// g_game.rs
use crate::doom::g_game::{consoleplayer, gameepisode, gamemap, players};

// i_sound.rs
use crate::doom::i_sound::{
    I_GetSfxLumpNum, I_MusicIsPlaying, I_PauseSong, I_PlaySong, I_PrecacheSounds, I_RegisterSong,
    I_ResumeSong, I_SetMusicVolume, I_ShutdownMusic, I_ShutdownSound, I_SoundIsPlaying,
    I_StartSound, I_StopSong, I_StopSound, I_UnRegisterSong, I_UpdateSound, I_UpdateSoundParams,
};

// i_system.rs
use crate::doom::i_system::I_AtExit;

// i_sound.rs
use crate::doom::i_sound::snd_musicdevice;

// z_zone.rs
use crate::doom::z_zone::Z_Malloc;

// r_main.rs
use crate::doom::r_main::R_PointToAngle2;

// m_fixed.rs
use crate::doom::m_fixed::FixedMul;

// w_wad.rs
use crate::doom::w_wad::{W_CacheLumpNum, W_GetNumForName, W_LumpLength, W_ReleaseLumpNum};

use crate::c_write;

/// Heap-allocated array of `snd_channels` mixing channels. Allocated by
/// `S_Init` from zone memory, never freed (PU_STATIC).
static mut channels: *mut channel_t = std::ptr::null_mut();

/// User-facing SFX volume slider value (0-15). Mirrors `sfxVolume` in
/// `s_sound.c`; `#[no_mangle]` so the menu and config code share the symbol.
#[no_mangle]
pub static mut sfxVolume: c_int = 8;

/// User-facing music volume slider value (0-15). Mirrors `musicVolume` in
/// `s_sound.c`; exposed via `#[no_mangle]` for the menu and config code.
#[no_mangle]
pub static mut musicVolume: c_int = 8;

/// Number of mixing channels to allocate (default 8). Read by `S_Init`.
#[no_mangle]
pub static mut snd_channels: c_int = 8;

/// Internal SFX volume on the 0-127 scale used by the driver. Set by
/// `S_SetSfxVolume`. C name: `snd_SfxVolume`.
#[no_mangle]
pub static mut snd_SfxVolume: c_int = 8;

/// Internal music volume on the 0-127 scale used by the driver. Set by
/// `S_SetMusicVolume`. C name: `snd_MusicVolume`.
#[no_mangle]
pub static mut snd_MusicVolume: c_int = 8;

/// Non-zero while music is paused, used by `S_PauseSound`/`S_ResumeSound`.
#[no_mangle]
pub static mut mus_paused: c_int = 0;

/// Currently playing music entry, or null if no song is active.
static mut mus_playing: *mut MusicInfo = std::ptr::null_mut();

/// Initialise the sound subsystem.
///
/// Pre-caches all SFX lumps, applies initial volumes, allocates the channel
/// array from zone memory, resets every channel to free, marks every sfx
/// lump as not-yet-loaded, and registers `S_Shutdown` with `I_AtExit`.
///
/// `sfx_volume` and `music_volume` are the initial 0-15 slider values.
///
/// C origin: `S_Init` in `s_sound.c`. The Rust port additionally calls
/// `S_InitSfxLinks` here because that work is data-table-side in Rust.
#[no_mangle]
pub extern "C" fn S_Init(sfx_volume: c_int, music_volume: c_int) {
    unsafe {
        S_InitSfxLinks();

        I_PrecacheSounds(
            std::ptr::addr_of_mut!(S_sfx[0]) as *mut c_void,
            NUMSFX as c_int,
        );

        S_SetSfxVolume(sfx_volume);
        S_SetMusicVolume(music_volume);

        channels = Z_Malloc(
            (snd_channels as usize * std::mem::size_of::<channel_t>()) as c_int,
            PU_STATIC,
            std::ptr::null_mut(),
        ) as *mut channel_t;

        for i in 0..snd_channels {
            (*channels.offset(i as isize)).sfxinfo = std::ptr::null_mut();
        }

        mus_paused = 0;

        for i in 1..NUMSFX {
            (*std::ptr::addr_of_mut!(S_sfx[0]).add(i)).lumpnum = -1;
            (*std::ptr::addr_of_mut!(S_sfx[0]).add(i)).usefulness = -1;
        }

        I_AtExit(S_Shutdown, Boolean::TRUE);
    }
}

/// Tear down the SFX and music drivers. Wired in at process exit via
/// `I_AtExit` from `S_Init`. C origin: `S_Shutdown` in `s_sound.c`.
#[no_mangle]
pub extern "C" fn S_Shutdown() {
    I_ShutdownSound();
    I_ShutdownMusic();
}

/// Stop the sound playing on channel `cnum` and free the slot.
///
/// Decrements the sfx's `usefulness` and clears `sfxinfo`, making the channel
/// available for `S_GetChannel`. The middle loop scanning other channels is a
/// faithful port of the C version and is currently a no-op (its `break` does
/// not gate the usefulness update).
///
/// # Safety
/// `cnum` must be a valid index into the `channels` array (`0..snd_channels`).
unsafe fn S_StopChannel(cnum: c_int) {
    let c = &mut *channels.offset(cnum as isize);

    if !c.sfxinfo.is_null() {
        if I_SoundIsPlaying(c.handle) != 0 {
            I_StopSound(c.handle);
        }

        for i in 0..snd_channels {
            if cnum != i
                && !channels.offset(i as isize).is_null()
                && (*channels.offset(i as isize)).sfxinfo == c.sfxinfo
            {
                break;
            }
        }

        (*c.sfxinfo).usefulness -= 1;
        c.sfxinfo = std::ptr::null_mut();
    }
}

/// Per-level startup: stop every channel, unpause music, and start the level's
/// music track based on `gamemode`/`gameepisode`/`gamemap`.
///
/// For Doom 2 the track is `mus_runnin + gamemap - 1`. For Ultimate Doom
/// episode 4 a hard-coded `spmus[]` table picks Romero's chosen songs.
/// C origin: `S_Start` in `s_sound.c`.
#[no_mangle]
pub extern "C" fn S_Start() {
    unsafe {
        for cnum in 0..snd_channels {
            if !(*channels.offset(cnum as isize)).sfxinfo.is_null() {
                S_StopChannel(cnum);
            }
        }

        mus_paused = 0;

        let mnum: c_int;

        if gamemode == d_mode::commercial {
            mnum = Mus::Runnin as c_int + gamemap - 1;
        } else {
            let spmus: [c_int; 9] = [
                Mus::E3m4 as c_int,
                Mus::E3m2 as c_int,
                Mus::E3m3 as c_int,
                Mus::E1m5 as c_int,
                Mus::E2m7 as c_int,
                Mus::E2m4 as c_int,
                Mus::E2m6 as c_int,
                Mus::E2m5 as c_int,
                Mus::E1m9 as c_int,
            ];

            if gameepisode < 4 {
                mnum = Mus::E1m1 as c_int + (gameepisode - 1) * 9 + gamemap - 1;
            } else {
                mnum = spmus[(gamemap - 1) as usize];
            }
        }

        S_ChangeMusic(mnum, 1);
    }
}

/// Stop any sound whose `origin` matches the given mobj. Walks the channel
/// list and stops the first match. C origin: `S_StopSound` in `s_sound.c`.
#[no_mangle]
pub extern "C" fn S_StopSound(origin: *mut MobjStub) {
    unsafe {
        for cnum in 0..snd_channels {
            let ch = *channels.offset(cnum as isize);
            if !ch.sfxinfo.is_null() && ch.origin == origin {
                S_StopChannel(cnum);
                break;
            }
        }
    }
}

/// Reserve a channel for a new sound.
///
/// Returns the chosen channel index, or `-1` if no eligible slot exists.
/// The function first tries to reuse a free channel or a channel already
/// owned by the same `origin` (stopping it first), then falls back to
/// evicting the first channel whose `priority` is greater than or equal
/// to `sfxinfo->priority`. In Doom's convention higher numeric `priority`
/// means less-important, so this evicts the first equally- or
/// less-important channel; if every active channel is more important
/// (numerically lower), the new sound is dropped and `-1` is returned.
///
/// # Safety
/// `sfxinfo` must point to a valid `SfxInfo`; `origin` may be null. `channels`
/// must already be allocated (call `S_Init` first).
unsafe fn S_GetChannel(origin: *mut MobjStub, sfxinfo: *mut SfxInfo) -> c_int {
    let mut cnum: c_int = 0;

    for cnum_search in 0..snd_channels {
        let ch = *channels.offset(cnum_search as isize);
        if ch.sfxinfo.is_null() {
            cnum = cnum_search;
            break;
        } else if !origin.is_null() && ch.origin == origin {
            S_StopChannel(cnum_search);
            cnum = cnum_search;
            break;
        }
    }

    if cnum == snd_channels {
        for cnum_search in 0..snd_channels {
            let ch = *channels.offset(cnum_search as isize);
            if !ch.sfxinfo.is_null() && (*ch.sfxinfo).priority >= (*sfxinfo).priority {
                cnum = cnum_search;
                break;
            }
        }

        if cnum == snd_channels {
            return -1;
        } else {
            S_StopChannel(cnum);
        }
    }

    let c = &mut *channels.offset(cnum as isize);
    c.sfxinfo = sfxinfo;
    c.origin = origin;

    cnum
}

/// Compute distance-attenuated volume and stereo separation for `source`
/// relative to `listener`.
///
/// Writes the result into `*vol` (0-127) and `*sep` (0-255, 128 = centred)
/// and returns non-zero if the sound is still audible. Distance is the
/// `|dx|+|dy| - min(|dx|,|dy|)/2` approximation Doom uses; volume falls off
/// linearly between `S_CLOSE_DIST` and `S_CLIPPING_DIST`. On map 8 (boss
/// levels) the falloff bottoms out at 15 instead of 0 so the boss is always
/// faintly audible.
///
/// # Safety
/// `listener`, `source`, `vol`, `sep` must all point at valid memory.
unsafe fn S_AdjustSoundParams(
    listener: *mut MobjStub,
    source: *mut MobjStub,
    vol: *mut c_int,
    sep: *mut c_int,
) -> c_int {
    let adx = (*listener).x.wrapping_sub((*source).x).wrapping_abs();
    let ady = (*listener).y.wrapping_sub((*source).y).wrapping_abs();

    let approx_dist = adx
        .wrapping_add(ady)
        .wrapping_sub((if adx < ady { adx } else { ady }) >> 1);

    if gamemap != 8 && approx_dist > S_CLIPPING_DIST {
        return 0;
    }

    let mut angle = R_PointToAngle2((*listener).x, (*listener).y, (*source).x, (*source).y);

    if angle > (*listener).angle {
        angle -= (*listener).angle;
    } else {
        angle = angle.wrapping_add(0xffffffff_u32 - (*listener).angle);
    }

    angle >>= ANGLETOFINESHIFT;

    *sep = 128 - (FixedMul(S_STEREO_SWING, finesine[angle as usize]) >> FRACBITS);

    if approx_dist < S_CLOSE_DIST {
        *vol = snd_SfxVolume;
    } else if gamemap == 8 {
        let mut clipped_dist = approx_dist;
        if clipped_dist > S_CLIPPING_DIST {
            clipped_dist = S_CLIPPING_DIST;
        }

        *vol = 15
            + ((snd_SfxVolume - 15) * ((S_CLIPPING_DIST - clipped_dist) >> FRACBITS))
                / S_ATTENUATOR;
    } else {
        *vol = (snd_SfxVolume * ((S_CLIPPING_DIST - approx_dist) >> FRACBITS)) / S_ATTENUATOR;
    }

    (*vol > 0) as c_int
}

/// Start an SFX from `origin_p` (an `mobj_t *`, may be null) with id `sfx_id`.
///
/// Bogus ids are silently dropped. If the sfx has a `link`, its `volume`
/// modifier is applied and clamped. Stereo/volume are computed from the
/// listener (the console player) unless `origin` is the listener itself, in
/// which case stereo is forced to centre. The chosen sfx replaces any sound
/// already playing on `origin`. C origin: `S_StartSound` in `s_sound.c`.
#[no_mangle]
pub extern "C" fn S_StartSound(origin_p: *mut c_void, sfx_id: c_int) {
    unsafe {
        let origin = origin_p as *mut MobjStub;
        let mut volume = snd_SfxVolume;

        if sfx_id < 1 || sfx_id > NUMSFX as c_int {
            // Bogus sound id - skip
            return;
        }

        let sfx = &mut *std::ptr::addr_of_mut!(S_sfx[0]).offset(sfx_id as isize);

        if !sfx.link.is_null() {
            volume += sfx.volume;

            if volume < 1 {
                return;
            }

            if volume > snd_SfxVolume {
                volume = snd_SfxVolume;
            }
        }

        let mut sep: c_int = NORM_SEP;

        let player_mo =
            (*std::ptr::addr_of!(players[0]).offset(consoleplayer as isize)).mo as *mut MobjStub;
        if !origin.is_null() && origin != player_mo {
            let listener = player_mo;
            let rc = S_AdjustSoundParams(listener, origin, &mut volume, &mut sep);

            if (*origin).x == (*listener).x && (*origin).y == (*listener).y {
                sep = NORM_SEP;
            }

            if rc == 0 {
                return;
            }
        }

        S_StopSound(origin);

        let cnum = S_GetChannel(origin, sfx);

        if cnum < 0 {
            return;
        }

        if sfx.usefulness < 0 {
            sfx.usefulness = 1;
        }
        sfx.usefulness += 1;

        if sfx.lumpnum < 0 {
            sfx.lumpnum = I_GetSfxLumpNum(sfx as *mut SfxInfo as *mut c_void);
        }

        (*channels.offset(cnum as isize)).handle =
            I_StartSound(sfx as *mut SfxInfo as *mut c_void, cnum, volume, sep);
    }
}

/// Pause the currently playing music if any. No-op if nothing is playing or
/// the song is already paused. C origin: `S_PauseSound` in `s_sound.c`.
#[no_mangle]
pub extern "C" fn S_PauseSound() {
    unsafe {
        if !mus_playing.is_null() && mus_paused == 0 {
            I_PauseSong();
            mus_paused = 1;
        }
    }
}

/// Resume music previously paused by `S_PauseSound`. No-op if nothing was
/// paused. C origin: `S_ResumeSound` in `s_sound.c`.
#[no_mangle]
pub extern "C" fn S_ResumeSound() {
    unsafe {
        if !mus_playing.is_null() && mus_paused != 0 {
            I_ResumeSong();
            mus_paused = 0;
        }
    }
}

/// Per-tic sound housekeeping: drive the low-level mixer and re-attenuate
/// each active positional sound against the current `listener`. Stops
/// channels whose sound stopped playing or fell out of audible range.
///
/// C origin: `S_UpdateSounds` in `s_sound.c`.
#[no_mangle]
pub extern "C" fn S_UpdateSounds(listener: *mut MobjStub) {
    unsafe {
        I_UpdateSound();

        for cnum in 0..snd_channels {
            let c = &mut *channels.offset(cnum as isize);
            let sfx = c.sfxinfo;

            if !sfx.is_null() {
                if I_SoundIsPlaying(c.handle) != 0 {
                    let mut volume = snd_SfxVolume;
                    let mut sep = NORM_SEP;

                    if !(*sfx).link.is_null() {
                        volume += (*sfx).volume;
                        if volume < 1 {
                            S_StopChannel(cnum);
                            continue;
                        } else if volume > snd_SfxVolume {
                            volume = snd_SfxVolume;
                        }
                    }

                    if !c.origin.is_null() && listener != c.origin {
                        let audible =
                            S_AdjustSoundParams(listener, c.origin, &mut volume, &mut sep);

                        if audible == 0 {
                            S_StopChannel(cnum);
                        } else {
                            I_UpdateSoundParams(c.handle, volume, sep);
                        }
                    }
                } else {
                    S_StopChannel(cnum);
                }
            }
        }
    }
}

/// Set the music volume (0-127). Values out of range are silently ignored to
/// match the C behaviour. C origin: `S_SetMusicVolume` in `s_sound.c`.
#[no_mangle]
pub extern "C" fn S_SetMusicVolume(volume: c_int) {
    if !(0..=127).contains(&volume) {
        return;
    }

    I_SetMusicVolume(volume);
}

/// Set the SFX volume (0-127), stored in `snd_SfxVolume` and applied to
/// subsequent `S_StartSound` calls. Values out of range are ignored.
/// C origin: `S_SetSfxVolume` in `s_sound.c`.
#[no_mangle]
pub extern "C" fn S_SetSfxVolume(volume: c_int) {
    unsafe {
        if !(0..=127).contains(&volume) {
            return;
        }

        snd_SfxVolume = volume;
    }
}

/// Start playing music `m_id` once (non-looping). Convenience wrapper around
/// `S_ChangeMusic`. C origin: `S_StartMusic` in `s_sound.c`.
#[no_mangle]
pub extern "C" fn S_StartMusic(m_id: c_int) {
    S_ChangeMusic(m_id, 0);
}

/// Switch the active music track.
///
/// If the device is AdLib or SoundBlaster, the intro is rerouted from
/// `Mus::Intro` to `Mus::Introa` (a shorter variant). Out-of-range ids are
/// ignored. The lump for the chosen track is loaded on first use, registered
/// with the music driver, played, and tracked in `mus_playing`. `looping`
/// non-zero requests an infinite loop.
///
/// C origin: `S_ChangeMusic` in `s_sound.c`.
#[no_mangle]
pub extern "C" fn S_ChangeMusic(musicnum: c_int, looping: c_int) {
    unsafe {
        let mut musicnum = musicnum;

        if musicnum == Mus::Intro as c_int
            && (snd_musicdevice == SNDDEVICE_ADLIB || snd_musicdevice == SNDDEVICE_SB)
        {
            musicnum = Mus::Introa as c_int;
        }

        if musicnum <= Mus::None as c_int || musicnum >= NUMMUSIC as c_int {
            return;
        }

        let music = &mut *std::ptr::addr_of_mut!(S_music[0]).offset(musicnum as isize);

        if mus_playing == music {
            return;
        }

        S_StopMusic();

        if music.lumpnum == 0 {
            let mut namebuf: [c_char; 9] = [0; 9];
            let name_str = std::ffi::CStr::from_ptr(music.name).to_string_lossy();
            c_write!(namebuf, "d_{}", name_str);
            music.lumpnum = W_GetNumForName(namebuf.as_ptr() as *const c_char);
        }

        music.data = W_CacheLumpNum(music.lumpnum, PU_STATIC);

        let len = W_LumpLength(music.lumpnum as c_uint);
        let handle = I_RegisterSong(music.data, len);
        music.handle = handle;
        I_PlaySong(handle, looping);

        mus_playing = music;
    }
}

/// Return non-zero if the music driver reports an active song. Thin wrapper
/// around `I_MusicIsPlaying`. C origin: `S_MusicPlaying` in `s_sound.c`.
#[no_mangle]
pub extern "C" fn S_MusicPlaying() -> c_int {
    I_MusicIsPlaying()
}

/// Stop the currently playing music, unregister the song, and release its
/// WAD lump back to zone memory. No-op if nothing is playing.
/// C origin: `S_StopMusic` in `s_sound.c`.
#[no_mangle]
pub extern "C" fn S_StopMusic() {
    unsafe {
        if !mus_playing.is_null() {
            let m = &mut *mus_playing;

            if mus_paused != 0 {
                I_ResumeSong();
            }

            I_StopSong();
            I_UnRegisterSong(m.handle);
            W_ReleaseLumpNum(m.lumpnum);
            m.data = std::ptr::null_mut();
            mus_playing = std::ptr::null_mut();
        }
    }
}
