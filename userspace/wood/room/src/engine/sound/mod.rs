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
//! - C passes sound origins as `void *` and reads them as `mobj_t`. Here an
//!   origin is a typed [`SoundOrigin`]: a map object or a sector's
//!   `degenmobj_t`. Map-object sounds go through `S_StartSound`, sector sounds
//!   through `S_StartSectorSound`.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

pub mod info;
pub mod mus2midi;

use std::ffi::c_void;

use crate::types::Boolean;

use crate::engine::{
    doom,
    global::Global,
    map::degenmobj_t,
    math::{
        fixed::{fixed_t, FRACBITS, FRACUNIT},
        tables::{finesine, ANGLETOFINESHIFT},
    },
    play::mobj::mobj_t,
    sound::info::{
        music_table, sfx_table, Mus, MusicInfo, S_InitSfxLinks, SfxInfo, NUMMUSIC, NUMSFX,
    },
    zone::PU_STATIC,
};

/// Distance, in fixed-point units, beyond which a positional sound is clipped
/// out entirely (`S_CLIPPING_DIST` in `s_sound.c`).
const S_CLIPPING_DIST: i32 = 1200 * FRACUNIT;
/// Distance, in fixed-point units, within which a sound plays at full SFX
/// volume (`S_CLOSE_DIST` in `s_sound.c`).
const S_CLOSE_DIST: i32 = 200 * FRACUNIT;
/// Integer attenuation range between `S_CLOSE_DIST` and `S_CLIPPING_DIST`,
/// shifted out of fixed-point. Used as the divisor in the linear volume falloff.
const S_ATTENUATOR: i32 = (S_CLIPPING_DIST - S_CLOSE_DIST) >> FRACBITS;
/// Maximum stereo swing applied to `sep` based on listener-to-source angle, in
/// fixed-point units.
const S_STEREO_SWING: i32 = 96 * FRACUNIT;
/// Pitch value treated as "no pitch shift" by the driver.
const NORM_PITCH: i32 = 128;
/// Default channel priority when none is supplied.
const NORM_PRIORITY: i32 = 64;
/// Stereo separation value meaning "centred" (0 = full left, 255 = full right).
const NORM_SEP: i32 = 128;

/// Mirror of `MAXPLAYERS` from `doomdef.h`; used to index `players[]`.
const MAXPLAYERS: usize = 4;

/// Music device id for AdLib, as recognised by `snd_musicdevice` config.
const SNDDEVICE_ADLIB: i32 = 2;
/// Music device id for SoundBlaster, as recognised by `snd_musicdevice`.
const SNDDEVICE_SB: i32 = 3;

/// Mixing channel slot. C counterpart: `channel_t` defined inside `s_sound.c`.
///
/// A null `sfxinfo` marks the slot as free. `origin` is what the sound is
/// attached to (used to keep stereo/volume in sync while it moves); it is
/// `None` for player-local sounds. `handle` is the driver's opaque id.
#[derive(Clone, Copy)]
pub struct channel_t {
    /// Currently playing sound's metadata, or null if the channel is free.
    pub sfxinfo: *mut SfxInfo,
    /// What the sound emanates from, or `None` for non-positional sounds.
    pub origin: Option<SoundOrigin>,
    /// Driver-side handle returned by `I_StartSound` for this channel.
    pub handle: i32,
}

/// Where a positional sound comes from. C passes either kind as `void *`.
///
/// Equality is pointer identity, which is how a new sound replaces the one
/// already playing from the same origin.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SoundOrigin {
    /// A map object; the sound follows it as it moves.
    Mobj(*mut mobj_t),
    /// A sector's sound origin (`sector_t::soundorg`), used by moving
    /// floors, ceilings, doors, platforms and switches.
    Sector(*mut degenmobj_t),
}

impl SoundOrigin {
    /// Wrap a map object pointer; null means a non-positional sound.
    fn from_mobj(mobj: *mut mobj_t) -> Option<Self> {
        (!mobj.is_null()).then_some(Self::Mobj(mobj))
    }

    /// Wrap a sector sound origin pointer; null means a non-positional sound.
    fn from_sector(sector_origin: *mut degenmobj_t) -> Option<Self> {
        (!sector_origin.is_null()).then_some(Self::Sector(sector_origin))
    }

    /// Current map position of the origin.
    ///
    /// # Safety
    /// The wrapped pointer must point at a live object.
    unsafe fn position(self) -> (fixed_t, fixed_t) {
        match self {
            Self::Mobj(mobj) => ((*mobj).x, (*mobj).y),
            Self::Sector(sector_origin) => ((*sector_origin).x, (*sector_origin).y),
        }
    }
}

// doomstat.rs
use crate::engine::doom::stat::gamemode;

// g_game.rs
use crate::engine::game::{consoleplayer, gameepisode, gamemap, players};

// i_sound.rs
use crate::engine::interface::sound::{
    I_GetSfxLumpNum, I_MusicIsPlaying, I_PauseSong, I_PlaySong, I_PrecacheSounds, I_RegisterSong,
    I_ResumeSong, I_SetMusicVolume, I_ShutdownMusic, I_ShutdownSound, I_SoundIsPlaying,
    I_StartSound, I_StopSong, I_StopSound, I_UnRegisterSong, I_UpdateSound, I_UpdateSoundParams,
};

// i_system.rs
use crate::engine::interface::system::I_AtExit;

// i_sound.rs
use crate::engine::interface::sound::SOUND_INTERFACE;

// z_zone.rs
use crate::engine::zone::Z_Malloc;

// r_main.rs
use crate::engine::render_3d::main::R_PointToAngle2;

// m_fixed.rs
use crate::engine::math::fixed::FixedMul;

// w_wad.rs
use crate::engine::wad::{W_CacheLumpNum, W_GetNumForName, W_LumpLength, W_ReleaseLumpNum};

/// Sound subsystem state (C `s_sound.c` globals).
///
/// `sfxVolume`, `musicVolume` and `snd_channels` are bound to config
/// variables by address, so they live at a stable place inside [`SOUND`].
#[derive(Clone, Copy)]
pub struct SoundState {
    /// Heap-allocated array of `snd_channels` mixing channels. Allocated by
    /// `S_Init` from zone memory, never freed (PU_STATIC).
    channels: *mut channel_t,
    /// User-facing SFX volume slider value (0-15). Mirrors `sfxVolume` in
    /// `s_sound.c`; shared with the menu and config code.
    pub sfxVolume: i32,
    /// User-facing music volume slider value (0-15). Mirrors `musicVolume` in
    /// `s_sound.c`; shared with the menu and config code.
    pub musicVolume: i32,
    /// Number of mixing channels to allocate (default 8). Read by `S_Init`.
    pub snd_channels: i32,
    /// Internal SFX volume on the 0-127 scale used by the driver. Set by
    /// `S_SetSfxVolume`. C name: `snd_SfxVolume`.
    snd_SfxVolume: i32,
    /// Non-zero while music is paused, used by `S_PauseSound`/`S_ResumeSound`.
    mus_paused: i32,
    /// Currently playing music entry, or null if no song is active.
    mus_playing: *mut MusicInfo,
}

impl SoundState {
    /// The C defaults: no channels allocated, volumes at 8, no music.
    pub const fn new() -> Self {
        Self {
            channels: std::ptr::null_mut(),
            sfxVolume: 8,
            musicVolume: 8,
            snd_channels: 8,
            snd_SfxVolume: 8,
            mus_paused: 0,
            mus_playing: std::ptr::null_mut(),
        }
    }
}

impl Default for SoundState {
    fn default() -> Self {
        Self::new()
    }
}

/// The sound subsystem state.
pub static SOUND: Global<SoundState> = Global::new(SoundState::new());

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
pub fn S_Init(sfx_volume: i32, music_volume: i32) {
    unsafe {
        S_InitSfxLinks();

        let sfx = sfx_table();
        I_PrecacheSounds(sfx as *mut c_void, NUMSFX as i32);

        S_SetSfxVolume(sfx_volume);
        S_SetMusicVolume(music_volume);

        let snd_channels = SOUND.get().snd_channels;
        let channels = Z_Malloc(
            (snd_channels as usize * std::mem::size_of::<channel_t>()) as i32,
            PU_STATIC,
            std::ptr::null_mut(),
        ) as *mut channel_t;

        for i in 0..snd_channels {
            channels.offset(i as isize).write(channel_t {
                sfxinfo: std::ptr::null_mut(),
                origin: None,
                handle: 0,
            });
        }

        {
            let mut sound = SOUND.borrow_mut();
            sound.channels = channels;
            sound.mus_paused = 0;
        }

        for i in 1..NUMSFX {
            (*sfx.add(i)).lumpnum = -1;
            (*sfx.add(i)).usefulness = -1;
        }

        I_AtExit(S_Shutdown, Boolean::TRUE);
    }
}

/// Tear down the SFX and music drivers. Wired in at process exit via
/// `I_AtExit` from `S_Init`. C origin: `S_Shutdown` in `s_sound.c`.
pub fn S_Shutdown() {
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
unsafe fn S_StopChannel(cnum: i32) {
    let SoundState {
        channels,
        snd_channels,
        ..
    } = SOUND.get();
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
pub fn S_Start() {
    unsafe {
        let SoundState {
            channels,
            snd_channels,
            ..
        } = SOUND.get();
        for cnum in 0..snd_channels {
            if !(*channels.offset(cnum as isize)).sfxinfo.is_null() {
                S_StopChannel(cnum);
            }
        }

        SOUND.borrow_mut().mus_paused = 0;

        let mnum: i32;

        if gamemode() == doom::mode::commercial {
            mnum = Mus::Runnin as i32 + gamemap - 1;
        } else {
            let spmus: [i32; 9] = [
                Mus::E3m4 as i32,
                Mus::E3m2 as i32,
                Mus::E3m3 as i32,
                Mus::E1m5 as i32,
                Mus::E2m7 as i32,
                Mus::E2m4 as i32,
                Mus::E2m6 as i32,
                Mus::E2m5 as i32,
                Mus::E1m9 as i32,
            ];

            if gameepisode < 4 {
                mnum = Mus::E1m1 as i32 + (gameepisode - 1) * 9 + gamemap - 1;
            } else {
                mnum = spmus[(gamemap - 1) as usize];
            }
        }

        S_ChangeMusic(mnum, 1);
    }
}

/// Stop any sound whose `origin` matches the given mobj. Walks the channel
/// list and stops the first match. C origin: `S_StopSound` in `s_sound.c`.
pub fn S_StopSound(origin: *mut mobj_t) {
    S_StopOriginSound(SoundOrigin::from_mobj(origin));
}

/// Stop the first sound playing from `origin`; `None` matches the first
/// non-positional sound, as a null origin does in C.
fn S_StopOriginSound(origin: Option<SoundOrigin>) {
    let SoundState {
        channels,
        snd_channels,
        ..
    } = SOUND.get();
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
/// `sfxinfo` must point to a valid `SfxInfo`; `origin` may be `None`.
/// `channels` must already be allocated (call `S_Init` first).
unsafe fn S_GetChannel(origin: Option<SoundOrigin>, sfxinfo: *mut SfxInfo) -> i32 {
    let SoundState {
        channels,
        snd_channels,
        ..
    } = SOUND.get();
    // `snd_channels` means "none found yet", as the C loop leaves `cnum`.
    let mut cnum: i32 = snd_channels;

    for cnum_search in 0..snd_channels {
        let ch = *channels.offset(cnum_search as isize);
        if ch.sfxinfo.is_null() {
            cnum = cnum_search;
            break;
        } else if origin.is_some() && ch.origin == origin {
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
    listener: *mut mobj_t,
    source: SoundOrigin,
    vol: *mut i32,
    sep: *mut i32,
) -> i32 {
    let (source_x, source_y) = source.position();
    let adx = (*listener).x.wrapping_sub(source_x).wrapping_abs();
    let ady = (*listener).y.wrapping_sub(source_y).wrapping_abs();

    let approx_dist = adx
        .wrapping_add(ady)
        .wrapping_sub((if adx < ady { adx } else { ady }) >> 1);

    if gamemap != 8 && approx_dist > S_CLIPPING_DIST {
        return 0;
    }

    let mut angle = R_PointToAngle2((*listener).x, (*listener).y, source_x, source_y);

    if angle > (*listener).angle {
        angle -= (*listener).angle;
    } else {
        angle = angle.wrapping_add(0xffffffff_u32 - (*listener).angle);
    }

    angle >>= ANGLETOFINESHIFT;

    *sep = 128 - (FixedMul(S_STEREO_SWING, finesine[angle as usize]) >> FRACBITS);

    let snd_SfxVolume = SOUND.get().snd_SfxVolume;
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

    (*vol > 0) as i32
}

/// Start an SFX from map object `origin` (may be null for a non-positional
/// sound) with id `sfx_id`. C origin: `S_StartSound` in `s_sound.c`.
pub fn S_StartSound(origin: *mut mobj_t, sfx_id: i32) {
    S_StartOriginSound(SoundOrigin::from_mobj(origin), sfx_id);
}

/// Start an SFX from a sector's sound origin (`sector_t::soundorg`; may be
/// null for a non-positional sound) with id `sfx_id`. C calls `S_StartSound`
/// with the `degenmobj_t` address for these.
pub fn S_StartSectorSound(origin: *mut degenmobj_t, sfx_id: i32) {
    S_StartOriginSound(SoundOrigin::from_sector(origin), sfx_id);
}

/// Start an SFX from `origin` (`None` for a non-positional sound).
///
/// Bogus ids are silently dropped. If the sfx has a `link`, its `volume`
/// modifier is applied and clamped. Stereo/volume are computed from the
/// listener (the console player) unless `origin` is the listener itself, in
/// which case stereo is forced to centre. The chosen sfx replaces any sound
/// already playing on `origin`. C origin: `S_StartSound` in `s_sound.c`.
fn S_StartOriginSound(origin: Option<SoundOrigin>, sfx_id: i32) {
    unsafe {
        let snd_SfxVolume = SOUND.get().snd_SfxVolume;
        let mut volume = snd_SfxVolume;

        if sfx_id < 1 || sfx_id > NUMSFX as i32 {
            // Bogus sound id - skip
            return;
        }

        let sfx = &mut *sfx_table().offset(sfx_id as isize);

        if !sfx.link.is_null() {
            volume += sfx.volume;

            if volume < 1 {
                return;
            }

            if volume > snd_SfxVolume {
                volume = snd_SfxVolume;
            }
        }

        let mut sep: i32 = NORM_SEP;

        let player_mo = (*std::ptr::addr_of!(players[0]).offset(consoleplayer as isize)).mo;
        if let Some(source) = origin.filter(|&source| source != SoundOrigin::Mobj(player_mo)) {
            let listener = player_mo;
            let rc = S_AdjustSoundParams(listener, source, &mut volume, &mut sep);

            if source.position() == ((*listener).x, (*listener).y) {
                sep = NORM_SEP;
            }

            if rc == 0 {
                return;
            }
        }

        S_StopOriginSound(origin);

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

        let handle = I_StartSound(sfx as *mut SfxInfo as *mut c_void, cnum, volume, sep);
        (*SOUND.get().channels.offset(cnum as isize)).handle = handle;
    }
}

/// Pause the currently playing music if any. No-op if nothing is playing or
/// the song is already paused. C origin: `S_PauseSound` in `s_sound.c`.
pub fn S_PauseSound() {
    let sound = SOUND.get();
    if !sound.mus_playing.is_null() && sound.mus_paused == 0 {
        I_PauseSong();
        SOUND.borrow_mut().mus_paused = 1;
    }
}

/// Resume music previously paused by `S_PauseSound`. No-op if nothing was
/// paused. C origin: `S_ResumeSound` in `s_sound.c`.
pub fn S_ResumeSound() {
    let sound = SOUND.get();
    if !sound.mus_playing.is_null() && sound.mus_paused != 0 {
        I_ResumeSong();
        SOUND.borrow_mut().mus_paused = 0;
    }
}

/// Per-tic sound housekeeping: drive the low-level mixer and re-attenuate
/// each active positional sound against the current `listener`. Stops
/// channels whose sound stopped playing or fell out of audible range.
///
/// C origin: `S_UpdateSounds` in `s_sound.c`.
pub fn S_UpdateSounds(listener: *mut mobj_t) {
    unsafe {
        I_UpdateSound();

        let SoundState {
            channels,
            snd_channels,
            snd_SfxVolume,
            ..
        } = SOUND.get();
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

                    if let Some(source) = c
                        .origin
                        .filter(|&source| source != SoundOrigin::Mobj(listener))
                    {
                        let audible = S_AdjustSoundParams(listener, source, &mut volume, &mut sep);

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
pub fn S_SetMusicVolume(volume: i32) {
    if !(0..=127).contains(&volume) {
        return;
    }

    I_SetMusicVolume(volume);
}

/// Set the SFX volume (0-127), stored in `snd_SfxVolume` and applied to
/// subsequent `S_StartSound` calls. Values out of range are ignored.
/// C origin: `S_SetSfxVolume` in `s_sound.c`.
pub fn S_SetSfxVolume(volume: i32) {
    if !(0..=127).contains(&volume) {
        return;
    }

    SOUND.borrow_mut().snd_SfxVolume = volume;
}

/// Start playing music `m_id` once (non-looping). Convenience wrapper around
/// `S_ChangeMusic`. C origin: `S_StartMusic` in `s_sound.c`.
pub fn S_StartMusic(m_id: i32) {
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
pub fn S_ChangeMusic(musicnum: i32, looping: i32) {
    unsafe {
        let mut musicnum = musicnum;

        let snd_musicdevice = SOUND_INTERFACE.borrow().snd_musicdevice;
        if musicnum == Mus::Intro as i32
            && (snd_musicdevice == SNDDEVICE_ADLIB || snd_musicdevice == SNDDEVICE_SB)
        {
            musicnum = Mus::Introa as i32;
        }

        if musicnum <= Mus::None as i32 || musicnum >= NUMMUSIC as i32 {
            return;
        }

        let music = &mut *music_table().offset(musicnum as isize);

        if SOUND.get().mus_playing == music {
            return;
        }

        S_StopMusic();

        if music.lumpnum == 0 {
            music.lumpnum = W_GetNumForName(format!("d_{}", music.name).as_bytes());
        }

        music.data = W_CacheLumpNum(music.lumpnum, PU_STATIC);

        let len = W_LumpLength(music.lumpnum as u32);
        let handle = I_RegisterSong(music.data, len);
        music.handle = handle;
        I_PlaySong(handle, looping);

        SOUND.borrow_mut().mus_playing = music;
    }
}

/// Return non-zero if the music driver reports an active song. Thin wrapper
/// around `I_MusicIsPlaying`. C origin: `S_MusicPlaying` in `s_sound.c`.
pub fn S_MusicPlaying() -> i32 {
    I_MusicIsPlaying()
}

/// Stop the currently playing music, unregister the song, and release its
/// WAD lump back to zone memory. No-op if nothing is playing.
/// C origin: `S_StopMusic` in `s_sound.c`.
pub fn S_StopMusic() {
    let SoundState {
        mus_playing,
        mus_paused,
        ..
    } = SOUND.get();
    if !mus_playing.is_null() {
        let m = unsafe { &mut *mus_playing };

        if mus_paused != 0 {
            I_ResumeSong();
        }

        I_StopSong();
        I_UnRegisterSong(m.handle);
        W_ReleaseLumpNum(m.lumpnum);
        m.data = std::ptr::null_mut();
        SOUND.borrow_mut().mus_playing = std::ptr::null_mut();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Null origins are non-positional; map objects and sector origins
    /// report their own position and compare by identity.
    #[test]
    fn sound_origin_position_and_identity() {
        let mut mobj: mobj_t = unsafe { std::mem::MaybeUninit::zeroed().assume_init() };
        mobj.x = 10 * FRACUNIT;
        mobj.y = -3 * FRACUNIT;
        let mut sector_origin: degenmobj_t =
            unsafe { std::mem::MaybeUninit::zeroed().assume_init() };
        sector_origin.x = 7 * FRACUNIT;
        sector_origin.y = 8 * FRACUNIT;

        assert_eq!(SoundOrigin::from_mobj(std::ptr::null_mut()), None);
        assert_eq!(SoundOrigin::from_sector(std::ptr::null_mut()), None);

        let mobj_origin = SoundOrigin::from_mobj(&raw mut mobj).unwrap();
        let sector_sound_origin = SoundOrigin::from_sector(&raw mut sector_origin).unwrap();
        unsafe {
            assert_eq!(mobj_origin.position(), (10 * FRACUNIT, -3 * FRACUNIT));
            assert_eq!(sector_sound_origin.position(), (7 * FRACUNIT, 8 * FRACUNIT));
        }

        assert_eq!(mobj_origin, SoundOrigin::Mobj(&raw mut mobj));
        assert_ne!(mobj_origin, sector_sound_origin);
    }

    fn sfx_with_priority(priority: i32) -> SfxInfo {
        SfxInfo {
            name: [0; 9],
            priority,
            link: std::ptr::null_mut(),
            pitch: -1,
            volume: -1,
            usefulness: 1,
            lumpnum: -1,
            numchannels: -1,
            driver_data: std::ptr::null_mut(),
        }
    }

    /// With every channel busy, `S_GetChannel` evicts the first equally or
    /// less important sound (higher `priority` number), and drops the new
    /// sound when every playing one is more important.
    #[test]
    fn get_channel_evicts_by_priority_when_all_busy() {
        let mut important = sfx_with_priority(10);
        let mut unimportant = sfx_with_priority(100);
        let mut incoming = sfx_with_priority(50);
        // Handle -1 keeps `S_StopChannel` away from the platform.
        let busy = |sfxinfo: *mut SfxInfo| channel_t {
            sfxinfo,
            origin: None,
            handle: -1,
        };
        let mut test_channels = [busy(&raw mut important), busy(&raw mut unimportant)];

        unsafe {
            let saved = SOUND.get();
            {
                let mut sound = SOUND.borrow_mut();
                sound.channels = test_channels.as_mut_ptr();
                sound.snd_channels = test_channels.len() as i32;
            }

            assert_eq!(S_GetChannel(None, &raw mut incoming), 1);
            assert_eq!(test_channels[0].sfxinfo, &raw mut important);
            assert_eq!(test_channels[1].sfxinfo, &raw mut incoming);
            assert_eq!(unimportant.usefulness, 0);

            test_channels[1] = busy(&raw mut important);
            assert_eq!(S_GetChannel(None, &raw mut incoming), -1);
            assert_eq!(test_channels[0].sfxinfo, &raw mut important);
            assert_eq!(test_channels[1].sfxinfo, &raw mut important);

            SOUND.set(saved);
        }
    }
}
