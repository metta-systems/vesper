//! SoundFont generators (SoundFont 2.04 section 8.1): their numbers, default
//! values, and which may appear at preset level.

/// Number of generator operators (0 to 60, `endOper` excluded).
pub const COUNT: usize = 61;

pub const START_ADDRS_OFFSET: u16 = 0;
pub const END_ADDRS_OFFSET: u16 = 1;
pub const STARTLOOP_ADDRS_OFFSET: u16 = 2;
pub const ENDLOOP_ADDRS_OFFSET: u16 = 3;
pub const START_ADDRS_COARSE_OFFSET: u16 = 4;
pub const INITIAL_FILTER_FC: u16 = 8;
pub const END_ADDRS_COARSE_OFFSET: u16 = 12;
pub const PAN: u16 = 17;
pub const DELAY_MOD_LFO: u16 = 21;
pub const DELAY_VIB_LFO: u16 = 23;
pub const DELAY_MOD_ENV: u16 = 25;
pub const ATTACK_MOD_ENV: u16 = 26;
pub const HOLD_MOD_ENV: u16 = 27;
pub const DECAY_MOD_ENV: u16 = 28;
pub const RELEASE_MOD_ENV: u16 = 30;
pub const DELAY_VOL_ENV: u16 = 33;
pub const ATTACK_VOL_ENV: u16 = 34;
pub const HOLD_VOL_ENV: u16 = 35;
pub const DECAY_VOL_ENV: u16 = 36;
pub const SUSTAIN_VOL_ENV: u16 = 37;
pub const RELEASE_VOL_ENV: u16 = 38;
pub const KEYNUM_TO_VOL_ENV_HOLD: u16 = 39;
pub const KEYNUM_TO_VOL_ENV_DECAY: u16 = 40;
pub const INSTRUMENT: u16 = 41;
pub const KEY_RANGE: u16 = 43;
pub const VEL_RANGE: u16 = 44;
pub const STARTLOOP_ADDRS_COARSE_OFFSET: u16 = 45;
pub const KEYNUM: u16 = 46;
pub const VELOCITY: u16 = 47;
pub const INITIAL_ATTENUATION: u16 = 48;
pub const ENDLOOP_ADDRS_COARSE_OFFSET: u16 = 50;
pub const COARSE_TUNE: u16 = 51;
pub const FINE_TUNE: u16 = 52;
pub const SAMPLE_ID: u16 = 53;
pub const SAMPLE_MODES: u16 = 54;
pub const SCALE_TUNING: u16 = 56;
pub const EXCLUSIVE_CLASS: u16 = 57;
pub const OVERRIDING_ROOT_KEY: u16 = 58;

/// The default value of every generator (section 8.1.3); the ranges' default
/// (0 to 127) is handled by [`crate::bank::Zone`].
#[must_use]
pub const fn default_value(operator: u16) -> i16 {
    match operator {
        INITIAL_FILTER_FC => 13_500,
        DELAY_MOD_LFO | DELAY_VIB_LFO | DELAY_MOD_ENV | ATTACK_MOD_ENV | HOLD_MOD_ENV
        | DECAY_MOD_ENV | RELEASE_MOD_ENV | DELAY_VOL_ENV | ATTACK_VOL_ENV | HOLD_VOL_ENV
        | DECAY_VOL_ENV | RELEASE_VOL_ENV => -12_000,
        KEYNUM | VELOCITY | OVERRIDING_ROOT_KEY => -1,
        SCALE_TUNING => 100,
        _ => 0,
    }
}

/// Whether a preset zone may set `operator` (section 8.5: the sample
/// addresses, the overriding key/velocity/root key, the sample modes and the
/// exclusive class are instrument-level only; the indices and ranges are
/// structural).
#[must_use]
pub const fn is_preset_offset(operator: u16) -> bool {
    !matches!(
        operator,
        START_ADDRS_OFFSET
            | END_ADDRS_OFFSET
            | STARTLOOP_ADDRS_OFFSET
            | ENDLOOP_ADDRS_OFFSET
            | START_ADDRS_COARSE_OFFSET
            | END_ADDRS_COARSE_OFFSET
            | STARTLOOP_ADDRS_COARSE_OFFSET
            | ENDLOOP_ADDRS_COARSE_OFFSET
            | KEYNUM
            | VELOCITY
            | SAMPLE_MODES
            | EXCLUSIVE_CLASS
            | OVERRIDING_ROOT_KEY
            | INSTRUMENT
            | SAMPLE_ID
            | KEY_RANGE
            | VEL_RANGE
    )
}
