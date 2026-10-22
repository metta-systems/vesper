extern "C" {
    static mut screensaver_mode: boolean;
    fn M_CheckParm(check: *mut ::core::ffi::c_char) -> ::core::ffi::c_int;
    fn M_BindVariable(name: *mut ::core::ffi::c_char, variable: *mut ::core::ffi::c_void);
}
pub type boolean = ::core::ffi::c_uint;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct sfxinfo_struct {
    pub tagname: *mut ::core::ffi::c_char,
    pub name: [::core::ffi::c_char; 9],
    pub priority: ::core::ffi::c_int,
    pub link: *mut sfxinfo_t,
    pub pitch: ::core::ffi::c_int,
    pub volume: ::core::ffi::c_int,
    pub usefulness: ::core::ffi::c_int,
    pub lumpnum: ::core::ffi::c_int,
    pub numchannels: ::core::ffi::c_int,
    pub driver_data: *mut ::core::ffi::c_void,
}
pub type sfxinfo_t = sfxinfo_struct;
pub type snddevice_t = ::core::ffi::c_uint;
pub const SNDDEVICE_CD: snddevice_t = 10;
pub const SNDDEVICE_AWE32: snddevice_t = 9;
pub const SNDDEVICE_GENMIDI: snddevice_t = 8;
pub const SNDDEVICE_SOUNDCANVAS: snddevice_t = 7;
pub const SNDDEVICE_WAVEBLASTER: snddevice_t = 6;
pub const SNDDEVICE_GUS: snddevice_t = 5;
pub const SNDDEVICE_PAS: snddevice_t = 4;
pub const SNDDEVICE_SB: snddevice_t = 3;
pub const SNDDEVICE_ADLIB: snddevice_t = 2;
pub const SNDDEVICE_PCSPEAKER: snddevice_t = 1;
pub const SNDDEVICE_NONE: snddevice_t = 0;
#[derive(Copy, Clone)]
#[repr(C)]
pub struct sound_module_t {
    pub sound_devices: *mut snddevice_t,
    pub num_sound_devices: ::core::ffi::c_int,
    pub Init: Option<unsafe extern "C" fn(boolean) -> boolean>,
    pub Shutdown: Option<unsafe extern "C" fn() -> ()>,
    pub GetSfxLumpNum: Option<unsafe extern "C" fn(*mut sfxinfo_t) -> ::core::ffi::c_int>,
    pub Update: Option<unsafe extern "C" fn() -> ()>,
    pub UpdateSoundParams: Option<
        unsafe extern "C" fn(::core::ffi::c_int, ::core::ffi::c_int, ::core::ffi::c_int) -> (),
    >,
    pub StartSound: Option<
        unsafe extern "C" fn(
            *mut sfxinfo_t,
            ::core::ffi::c_int,
            ::core::ffi::c_int,
            ::core::ffi::c_int,
        ) -> ::core::ffi::c_int,
    >,
    pub StopSound: Option<unsafe extern "C" fn(::core::ffi::c_int) -> ()>,
    pub SoundIsPlaying: Option<unsafe extern "C" fn(::core::ffi::c_int) -> boolean>,
    pub CacheSounds: Option<unsafe extern "C" fn(*mut sfxinfo_t, ::core::ffi::c_int) -> ()>,
}
#[derive(Copy, Clone)]
#[repr(C)]
pub struct music_module_t {
    pub sound_devices: *mut snddevice_t,
    pub num_sound_devices: ::core::ffi::c_int,
    pub Init: Option<unsafe extern "C" fn() -> boolean>,
    pub Shutdown: Option<unsafe extern "C" fn() -> ()>,
    pub SetMusicVolume: Option<unsafe extern "C" fn(::core::ffi::c_int) -> ()>,
    pub PauseMusic: Option<unsafe extern "C" fn() -> ()>,
    pub ResumeMusic: Option<unsafe extern "C" fn() -> ()>,
    pub RegisterSong: Option<
        unsafe extern "C" fn(
            *mut ::core::ffi::c_void,
            ::core::ffi::c_int,
        ) -> *mut ::core::ffi::c_void,
    >,
    pub UnRegisterSong: Option<unsafe extern "C" fn(*mut ::core::ffi::c_void) -> ()>,
    pub PlaySong: Option<unsafe extern "C" fn(*mut ::core::ffi::c_void, boolean) -> ()>,
    pub StopSong: Option<unsafe extern "C" fn() -> ()>,
    pub MusicIsPlaying: Option<unsafe extern "C" fn() -> boolean>,
    pub Poll: Option<unsafe extern "C" fn() -> ()>,
}
pub const NULL: *mut ::core::ffi::c_void = ::core::ptr::null_mut::<::core::ffi::c_void>();
pub const true_0: ::core::ffi::c_int = 1 as ::core::ffi::c_int;
pub const false_0: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
#[no_mangle]
pub static mut snd_samplerate: ::core::ffi::c_int = 44100 as ::core::ffi::c_int;
#[no_mangle]
pub static mut snd_cachesize: ::core::ffi::c_int =
    64 as ::core::ffi::c_int * 1024 as ::core::ffi::c_int * 1024 as ::core::ffi::c_int;
#[no_mangle]
pub static mut snd_maxslicetime_ms: ::core::ffi::c_int = 28 as ::core::ffi::c_int;
#[no_mangle]
pub static mut snd_musiccmd: *mut ::core::ffi::c_char =
    b"\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char;
static mut sound_module: *mut sound_module_t = ::core::ptr::null_mut::<sound_module_t>();
static mut music_module: *mut music_module_t = ::core::ptr::null_mut::<music_module_t>();
#[no_mangle]
pub static mut snd_musicdevice: ::core::ffi::c_int = SNDDEVICE_SB as ::core::ffi::c_int;
#[no_mangle]
pub static mut snd_sfxdevice: ::core::ffi::c_int = SNDDEVICE_SB as ::core::ffi::c_int;
static mut snd_sbport: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
static mut snd_sbirq: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
static mut snd_sbdma: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
static mut snd_mport: ::core::ffi::c_int = 0 as ::core::ffi::c_int;
static mut sound_modules: [*mut sound_module_t; 1] = [::core::ptr::null_mut::<sound_module_t>()];
unsafe extern "C" fn SndDeviceInList(
    mut device: snddevice_t,
    mut list: *mut snddevice_t,
    mut len: ::core::ffi::c_int,
) -> boolean {
    let mut i: ::core::ffi::c_int = 0;
    i = 0 as ::core::ffi::c_int;
    while i < len {
        if device as ::core::ffi::c_uint == *list.offset(i as isize) as ::core::ffi::c_uint {
            return true_0 as boolean;
        }
        i += 1;
    }
    return false_0 as boolean;
}
unsafe extern "C" fn InitSfxModule(mut use_sfx_prefix: boolean) {
    let mut i: ::core::ffi::c_int = 0;
    sound_module = ::core::ptr::null_mut::<sound_module_t>();
    i = 0 as ::core::ffi::c_int;
    while !sound_modules[i as usize].is_null() {
        if SndDeviceInList(
            snd_sfxdevice as snddevice_t,
            (*sound_modules[i as usize]).sound_devices,
            (*sound_modules[i as usize]).num_sound_devices,
        ) != 0
        {
            if (*sound_modules[i as usize])
                .Init
                .expect("non-null function pointer")(use_sfx_prefix)
                != 0
            {
                sound_module = sound_modules[i as usize];
                return;
            }
        }
        i += 1;
    }
}
unsafe extern "C" fn InitMusicModule() {}
#[no_mangle]
pub unsafe extern "C" fn I_InitSound(mut use_sfx_prefix: boolean) {
    let mut nosound: boolean = 0;
    let mut nosfx: boolean = 0;
    let mut nomusic: boolean = 0;
    nosound = (M_CheckParm(
        b"-nosound\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char
    ) > 0 as ::core::ffi::c_int) as ::core::ffi::c_int as boolean;
    nosfx = (M_CheckParm(
        b"-nosfx\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char
    ) > 0 as ::core::ffi::c_int) as ::core::ffi::c_int as boolean;
    nomusic = (M_CheckParm(
        b"-nomusic\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char
    ) > 0 as ::core::ffi::c_int) as ::core::ffi::c_int as boolean;
    if nosound == 0 && screensaver_mode == 0 {
        nomusic == 0
            && (snd_musicdevice == SNDDEVICE_GENMIDI as ::core::ffi::c_int
                || snd_musicdevice == SNDDEVICE_GUS as ::core::ffi::c_int);
        if nosfx == 0 {
            InitSfxModule(use_sfx_prefix);
        }
        if nomusic == 0 {
            InitMusicModule();
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn I_ShutdownSound() {
    if !sound_module.is_null() {
        (*sound_module).Shutdown.expect("non-null function pointer")();
    }
    if !music_module.is_null() {
        (*music_module).Shutdown.expect("non-null function pointer")();
    }
}
#[no_mangle]
pub unsafe extern "C" fn I_GetSfxLumpNum(mut sfxinfo: *mut sfxinfo_t) -> ::core::ffi::c_int {
    if !sound_module.is_null() {
        return (*sound_module)
            .GetSfxLumpNum
            .expect("non-null function pointer")(sfxinfo);
    } else {
        return 0 as ::core::ffi::c_int;
    };
}
#[no_mangle]
pub unsafe extern "C" fn I_UpdateSound() {
    if !sound_module.is_null() {
        (*sound_module).Update.expect("non-null function pointer")();
    }
    if !music_module.is_null() && (*music_module).Poll.is_some() {
        (*music_module).Poll.expect("non-null function pointer")();
    }
}
unsafe extern "C" fn CheckVolumeSeparation(
    mut vol: *mut ::core::ffi::c_int,
    mut sep: *mut ::core::ffi::c_int,
) {
    if *sep < 0 as ::core::ffi::c_int {
        *sep = 0 as ::core::ffi::c_int;
    } else if *sep > 254 as ::core::ffi::c_int {
        *sep = 254 as ::core::ffi::c_int;
    }
    if *vol < 0 as ::core::ffi::c_int {
        *vol = 0 as ::core::ffi::c_int;
    } else if *vol > 127 as ::core::ffi::c_int {
        *vol = 127 as ::core::ffi::c_int;
    }
}
#[no_mangle]
pub unsafe extern "C" fn I_UpdateSoundParams(
    mut channel: ::core::ffi::c_int,
    mut vol: ::core::ffi::c_int,
    mut sep: ::core::ffi::c_int,
) {
    if !sound_module.is_null() {
        CheckVolumeSeparation(&raw mut vol, &raw mut sep);
        (*sound_module)
            .UpdateSoundParams
            .expect("non-null function pointer")(channel, vol, sep);
    }
}
#[no_mangle]
pub unsafe extern "C" fn I_StartSound(
    mut sfxinfo: *mut sfxinfo_t,
    mut channel: ::core::ffi::c_int,
    mut vol: ::core::ffi::c_int,
    mut sep: ::core::ffi::c_int,
) -> ::core::ffi::c_int {
    if !sound_module.is_null() {
        CheckVolumeSeparation(&raw mut vol, &raw mut sep);
        return (*sound_module)
            .StartSound
            .expect("non-null function pointer")(sfxinfo, channel, vol, sep);
    } else {
        return 0 as ::core::ffi::c_int;
    };
}
#[no_mangle]
pub unsafe extern "C" fn I_StopSound(mut channel: ::core::ffi::c_int) {
    if !sound_module.is_null() {
        (*sound_module)
            .StopSound
            .expect("non-null function pointer")(channel);
    }
}
#[no_mangle]
pub unsafe extern "C" fn I_SoundIsPlaying(mut channel: ::core::ffi::c_int) -> boolean {
    if !sound_module.is_null() {
        return (*sound_module)
            .SoundIsPlaying
            .expect("non-null function pointer")(channel);
    } else {
        return false_0 as boolean;
    };
}
#[no_mangle]
pub unsafe extern "C" fn I_PrecacheSounds(
    mut sounds: *mut sfxinfo_t,
    mut num_sounds: ::core::ffi::c_int,
) {
    if !sound_module.is_null() && (*sound_module).CacheSounds.is_some() {
        (*sound_module)
            .CacheSounds
            .expect("non-null function pointer")(sounds, num_sounds);
    }
}
#[no_mangle]
pub unsafe extern "C" fn I_InitMusic() {
    if !music_module.is_null() {
        (*music_module).Init.expect("non-null function pointer")();
    }
}
#[no_mangle]
pub unsafe extern "C" fn I_ShutdownMusic() {}
#[no_mangle]
pub unsafe extern "C" fn I_SetMusicVolume(mut volume: ::core::ffi::c_int) {
    if !music_module.is_null() {
        (*music_module)
            .SetMusicVolume
            .expect("non-null function pointer")(volume);
    }
}
#[no_mangle]
pub unsafe extern "C" fn I_PauseSong() {
    if !music_module.is_null() {
        (*music_module)
            .PauseMusic
            .expect("non-null function pointer")();
    }
}
#[no_mangle]
pub unsafe extern "C" fn I_ResumeSong() {
    if !music_module.is_null() {
        (*music_module)
            .ResumeMusic
            .expect("non-null function pointer")();
    }
}
#[no_mangle]
pub unsafe extern "C" fn I_RegisterSong(
    mut data: *mut ::core::ffi::c_void,
    mut len: ::core::ffi::c_int,
) -> *mut ::core::ffi::c_void {
    if !music_module.is_null() {
        return (*music_module)
            .RegisterSong
            .expect("non-null function pointer")(data, len);
    } else {
        return NULL;
    };
}
#[no_mangle]
pub unsafe extern "C" fn I_UnRegisterSong(mut handle: *mut ::core::ffi::c_void) {
    if !music_module.is_null() {
        (*music_module)
            .UnRegisterSong
            .expect("non-null function pointer")(handle);
    }
}
#[no_mangle]
pub unsafe extern "C" fn I_PlaySong(mut handle: *mut ::core::ffi::c_void, mut looping: boolean) {
    if !music_module.is_null() {
        (*music_module).PlaySong.expect("non-null function pointer")(handle, looping);
    }
}
#[no_mangle]
pub unsafe extern "C" fn I_StopSong() {
    if !music_module.is_null() {
        (*music_module).StopSong.expect("non-null function pointer")();
    }
}
#[no_mangle]
pub unsafe extern "C" fn I_MusicIsPlaying() -> boolean {
    if !music_module.is_null() {
        return (*music_module)
            .MusicIsPlaying
            .expect("non-null function pointer")();
    } else {
        return false_0 as boolean;
    };
}
#[no_mangle]
pub unsafe extern "C" fn I_BindSoundVariables() {
    extern "C" {
        static mut use_libsamplerate: ::core::ffi::c_int;
    }
    extern "C" {
        static mut libsamplerate_scale: ::core::ffi::c_float;
    }
    M_BindVariable(
        b"snd_musicdevice\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut snd_musicdevice as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"snd_sfxdevice\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut snd_sfxdevice as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"snd_sbport\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut snd_sbport as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"snd_sbirq\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut snd_sbirq as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"snd_sbdma\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut snd_sbdma as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"snd_mport\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut snd_mport as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"snd_maxslicetime_ms\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut snd_maxslicetime_ms as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"snd_musiccmd\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut snd_musiccmd as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"snd_samplerate\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut snd_samplerate as *mut ::core::ffi::c_void,
    );
    M_BindVariable(
        b"snd_cachesize\0".as_ptr() as *const ::core::ffi::c_char as *mut ::core::ffi::c_char,
        &raw mut snd_cachesize as *mut ::core::ffi::c_void,
    );
}
