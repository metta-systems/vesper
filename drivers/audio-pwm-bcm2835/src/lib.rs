#![no_std]

//! The PWM block of the BCM2835 (`brcm,bcm2835-pwm`) as the Raspberry Pi 3's
//! headphone jack: its registers, the FIFO words of a stereo frame, the PCM
//! stream of `audio_v1` that fills the DMA ring ([`stream`]) and a test tone.
//!
//! Resources: BCM2835 ARM Peripherals, chapter 9: §9.6 (register map, `CTL`,
//! `STA` and `DMAC` fields), §9.4 (PWM mode with `MSEN` 0 or 1), §9.1 (one
//! FIFO of eight words feeding both channels). With both channels on the FIFO
//! the words alternate: channel 1 takes the even ones, channel 2 the odd ones
//! (§9.6, `FIF1`), and both use the same range.
//!
//! On the Raspberry Pi 3 B, channel 1 (PWM0, GPIO40) drives the jack's right
//! side and channel 2 (PWM1, GPIO41) the left one: the board's schematic routes
//! `AUD_PWM1` to `AUDIO_L` and `AUD_PWM0` to `AUDIO_R`
//! (`doc/agentic/audio-design.md`, The jack). A frame is therefore written
//! right first.

pub mod stream;

use {bitbybit::bitfield, derive_mmio::Mmio, libmmio::assert_register_offsets};

/// The PWM clock the driver asks the clock manager for: 100 MHz, the
/// datasheet's nominal PWM clock (§9.1), PLLD per / 5 (decision D4).
pub const PWM_CLOCK_HZ: u32 = 100_000_000;

/// Clock cycles of one sample on both channels (`RNG1` = `RNG2`): 100 MHz /
/// 2268 is 44 092 Hz, about 11 bits per sample (decision D4).
pub const RANGE: u32 = 2268;

/// Words the FIFO holds (§9.1).
pub const FIFO_WORDS: usize = 8;

/// GPIO pins of the jack, PWM0 and PWM1 on alternate function 0 (§9.5 and the
/// `audio_pins` group of the Raspberry Pi 3 device trees).
pub const JACK_PINS: [u32; 2] = [40, 41];

/// PWM control (`CTL`, §9.6).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Control {
    #[bit(0, rw)]
    enable1: bool,
    /// Serialiser mode instead of PWM
    #[bit(1, rw)]
    serialise1: bool,
    /// Repeat the last FIFO word when the FIFO runs empty
    #[bit(2, rw)]
    repeat_last1: bool,
    /// The output level while nothing is sent
    #[bit(3, rw)]
    silence_bit1: bool,
    #[bit(4, rw)]
    invert1: bool,
    /// Take data from the FIFO instead of `DAT1`
    #[bit(5, rw)]
    use_fifo1: bool,
    /// Clear the FIFO (write-only action, reads as 0)
    #[bit(6, rw)]
    clear_fifo: bool,
    /// Mark/space instead of the spreading algorithm of §9.3
    #[bit(7, rw)]
    mark_space1: bool,
    #[bit(8, rw)]
    enable2: bool,
    #[bit(9, rw)]
    serialise2: bool,
    #[bit(10, rw)]
    repeat_last2: bool,
    #[bit(11, rw)]
    silence_bit2: bool,
    #[bit(12, rw)]
    invert2: bool,
    #[bit(13, rw)]
    use_fifo2: bool,
    #[bit(15, rw)]
    mark_space2: bool,
}

/// PWM status (`STA`, §9.6). The error and gap flags clear when written with 1.
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct Status {
    #[bit(0, rw)]
    fifo_full: bool,
    #[bit(1, rw)]
    fifo_empty: bool,
    /// A word was written to the full FIFO
    #[bit(2, rw)]
    write_error: bool,
    /// A channel read the empty FIFO
    #[bit(3, rw)]
    read_error: bool,
    /// Channel 1 waited for a word between two FIFO words
    #[bit(4, rw)]
    gap1: bool,
    #[bit(5, rw)]
    gap2: bool,
    #[bit(6, rw)]
    gap3: bool,
    #[bit(7, rw)]
    gap4: bool,
    /// Register writes came faster than the block could take them
    #[bit(8, rw)]
    bus_error: bool,
    #[bit(9, rw)]
    sending1: bool,
    #[bit(10, rw)]
    sending2: bool,
}

/// PWM DMA configuration (`DMAC`, §9.6).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct DmaConfig {
    /// FIFO level below which the block asks for DMA
    #[bits(0..=7, rw)]
    request_threshold: u8,
    /// FIFO level below which the request becomes urgent
    #[bits(8..=15, rw)]
    panic_threshold: u8,
    #[bit(31, rw)]
    enable: bool,
}

/// The PWM block's registers (§9.6).
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct RegisterBlock {
    control: Control,
    status: Status,
    dma_config: DmaConfig,
    _reserved0: u32,
    range1: u32,
    data1: u32,
    fifo: u32,
    _reserved1: u32,
    range2: u32,
    data2: u32,
}

assert_register_offsets!(RegisterBlock, 0x28, {
    control: 0x00,
    status: 0x04,
    dma_config: 0x08,
    range1: 0x10,
    data1: 0x14,
    fifo: 0x18,
    range2: 0x20,
    data2: 0x24,
});

/// Both channels in PWM mode with the spreading algorithm (`MSEN` 0, decision
/// D4), fed from the FIFO, not repeating the last word: an empty FIFO shows as
/// a gap flag instead of being hidden.
#[must_use]
pub const fn fifo_stereo() -> Control {
    Control::ZERO
        .with_enable1(true)
        .with_use_fifo1(true)
        .with_enable2(true)
        .with_use_fifo2(true)
}

/// The control value that clears the FIFO with both channels stopped: clear it
/// after the set of channels using it changes (§9.6, `FIF1`).
#[must_use]
pub const fn clearing() -> Control {
    Control::ZERO.with_clear_fifo(true)
}

/// The status value that clears every error and gap flag.
#[must_use]
pub const fn clear_errors() -> Status {
    Status::ZERO
        .with_write_error(true)
        .with_read_error(true)
        .with_gap1(true)
        .with_gap2(true)
        .with_gap3(true)
        .with_gap4(true)
        .with_bus_error(true)
}

/// Whether `status` shows an error or a gap.
#[must_use]
pub const fn has_errors(status: Status) -> bool {
    status.raw_value() & clear_errors().raw_value() != 0
}

/// The FIFO word of a signed 16-bit sample: 0 for the most negative, `range`/2
/// for silence, just under `range` for the most positive.
#[must_use]
pub fn word(sample: i16, range: u32) -> u32 {
    let unsigned = u64::from(sample.cast_unsigned() ^ 0x8000);
    u32::try_from((unsigned * u64::from(range)) >> 16).unwrap_or(range)
}

/// Which side of the jack a test tone plays on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Both,
}

/// The client frame `[left, right]` playing `sample` on `side` and silence on
/// the other.
#[must_use]
pub const fn side_frame(side: Side, sample: i16) -> [i16; 2] {
    match side {
        Side::Left => [sample, 0],
        Side::Right => [0, sample],
        Side::Both => [sample, sample],
    }
}

/// Entries of the sine table: one period.
const SINE_ENTRIES: usize = 256;

/// One period of a sine of amplitude `i16::MAX`, from Bhaskara I's rational
/// approximation `sin x ≈ 16x(π − x) / (5π² − 4x(π − x))` on each half (largest
/// error about 0.16 % of full scale), in integers so it is a constant.
pub const SINE: [i16; SINE_ENTRIES] = sine_table();

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "the approximation stays within 0..=i16::MAX"
)]
const fn sine_table() -> [i16; SINE_ENTRIES] {
    const HALF: usize = SINE_ENTRIES / 2;
    let mut table = [0; SINE_ENTRIES];
    let mut index = 0;
    while index < SINE_ENTRIES {
        // Position in the half period: `HALF` stands for π.
        let position = index % HALF;
        let product = position * (HALF - position);
        let magnitude = 16 * product * (i16::MAX as usize) / (5 * HALF * HALF - 4 * product);
        let magnitude = magnitude as i16;
        table[index] = if index < HALF { magnitude } else { -magnitude };
        index += 1;
    }
    table
}

/// A sine oscillator: a 32-bit phase stepping through [`SINE`].
#[derive(Debug, Clone, Copy)]
pub struct Tone {
    phase: u32,
    step: u32,
    /// Right shift applied to each sample: each step halves the loudness
    attenuation: u32,
}

impl Tone {
    /// A tone of `hz` at `sample_rate_millihertz`, attenuated by `attenuation`
    /// halvings.
    #[must_use]
    pub fn new(hz: u32, sample_rate_millihertz: u64, attenuation: u32) -> Self {
        let step = ((u64::from(hz) * 1000) << 32) / sample_rate_millihertz.max(1);
        Self {
            phase: 0,
            step: u32::try_from(step).unwrap_or(u32::MAX),
            attenuation,
        }
    }

    /// The phase step per sample.
    #[must_use]
    pub const fn step(&self) -> u32 {
        self.step
    }

    /// The next sample.
    pub fn next_sample(&mut self) -> i16 {
        let sample = SINE[(self.phase >> 24) as usize] >> self.attenuation;
        self.phase = self.phase.wrapping_add(self.step);
        sample
    }
}

/// The sample rate `range` gives at `clock_hz`, in millihertz.
#[must_use]
pub fn sample_rate_millihertz(clock_hz: u32, range: u32) -> u64 {
    u64::from(clock_hz) * 1000 / u64::from(range.max(1))
}

/// The PWM block's DMA request line: "PWM DMA is mapped to DMA channel 5"
/// (§9.5), DREQ 5 of the DMA controller (§4.2.1.3).
pub const PWM_DREQ: u32 = 5;

/// DREQ 0: an unpaced memory-to-memory transfer (§4.2.1.3).
pub const MEMORY_DREQ: u32 = 0;

/// Stereo frames of one period of the DMA ring (`doc/agentic/audio-design.md`,
/// Scheduling: 4 periods of 512 frames, about 46 ms at 44.1 kHz).
pub const PERIOD_FRAMES: usize = 512;

/// Periods of the DMA ring.
pub const PERIODS: usize = 4;

/// Bytes of one period: two FIFO words per frame. One page.
pub const PERIOD_BYTES: usize = PERIOD_FRAMES * 2 * size_of::<u32>();

const _: () = assert!(PERIOD_BYTES == 4096, "a period is one page");

/// `DMAC` for feeding the FIFO by DMA: requests while fewer than 7 of its 8
/// words are queued, panic below 7 (§9.6, `DMAC`, which gives both as the
/// reset values; Circle uses the same, secondary).
#[must_use]
pub fn dma_feed() -> DmaConfig {
    DmaConfig::ZERO
        .with_request_threshold(7)
        .with_panic_threshold(7)
        .with_enable(true)
}

/// The rate `frames` frames in `elapsed_ns` nanoseconds give, in millihertz.
#[must_use]
pub fn measured_rate_millihertz(frames: u64, elapsed_ns: u64) -> u64 {
    let rate = u128::from(frames) * 1_000_000_000_000 / u128::from(elapsed_ns.max(1));
    u64::try_from(rate).unwrap_or(u64::MAX)
}

/// How far `measured` is from `expected`, in parts per million.
#[must_use]
pub fn deviation_ppm(measured: u64, expected: u64) -> u64 {
    let difference = u128::from(measured.abs_diff(expected));
    u64::try_from(difference * 1_000_000 / u128::from(expected.max(1))).unwrap_or(u64::MAX)
}

/// The hardware check of the ring's pace: the measured rate within 0.1 % of
/// the computed one.
pub const RATE_TOLERANCE_PPM: u64 = 1000;
