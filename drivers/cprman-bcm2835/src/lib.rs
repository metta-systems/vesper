#![no_std]

//! The clock manager of the BCM2835 (`brcm,bcm2835-cprman`): the registers of
//! its clock generators, the choice of a source and divisor for a rate, and
//! the sequence that changes a generator without glitches.
//!
//! Resources: BCM2835 ARM Peripherals, section 6.3, which documents the general
//! purpose clocks `CM_GP0..2`: table 6-32 (divisor limits per MASH setting),
//! table 6-34 (`CTL`: password, `MASH`, `BUSY`, `ENAB`, `SRC`) and table 6-35
//! (`DIV`: password, `DIVI`, `DIVF`). The other generators, the PWM clock among
//! them, use the same two-register format; the datasheet does not list them,
//! and their offsets (`CM_PWMCTL` 0xa0, `CM_PWMDIV` 0xa4) come from the Linux
//! and NetBSD clock drivers and QEMU's model of the block, which agree
//! (`doc/agentic/audio-design.md`, section The PWM clock).
//!
//! Only integer division (`MASH` 0) is programmed: it has no jitter, and the
//! audio rate (D4) needs none.
//!
//! The BCM2711's clock manager (`brcm,bcm2711-cprman`, BCM2711 ARM
//! Peripherals, section 5.4) has the same generator registers and sources
//! (table 98 lists `SRC` 1 as the oscillator, 6 as PLLD per). Its oscillator
//! runs at 54 MHz (the Raspberry Pi 4 tree's `clk-osc` fixed clock); the
//! manual gives no PLL rates, so on the BCM2711 only the oscillator is a
//! source of known rate ([`Chip`]).

use {
    arbitrary_int::u12,
    bitbybit::{bitenum, bitfield},
    derive_mmio::Mmio,
    libmmio::assert_register_offsets,
};

/// Every write to a generator's registers carries this password in its top
/// byte (tables 6-34 and 6-35).
pub const PASSWORD: u8 = 0x5a;

/// The PWM clock's number in the device tree binding of the clock manager: the
/// second cell of the PWM node's `clocks = <&clocks 30>` (Raspberry Pi device
/// trees, `pwm@7e20c000`).
pub const PWM_CLOCK: u32 = 30;

/// Which clock manager the node is: the sources' rates differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    /// The BCM2835 to BCM2837 (Raspberry Pi 1 to 3)
    Bcm2835,
    /// The BCM2711 (Raspberry Pi 4)
    Bcm2711,
}

impl Chip {
    /// The manager a driver's matched compatible string names, by its position
    /// in `driver_compatible!("brcm,bcm2835-cprman", "brcm,bcm2711-cprman")`.
    #[must_use]
    pub const fn from_compatible_index(index: u64) -> Option<Self> {
        match index {
            0 => Some(Self::Bcm2835),
            1 => Some(Self::Bcm2711),
            _ => None,
        }
    }

    /// The crystal oscillator's rate: the device tree's `osc` fixed clock, the
    /// clock manager's first parent (19.2 MHz in the Raspberry Pi 3 trees,
    /// 54 MHz in the Raspberry Pi 4 trees).
    #[must_use]
    pub const fn oscillator_hz(self) -> u32 {
        match self {
            Self::Bcm2835 => 19_200_000,
            Self::Bcm2711 => 54_000_000,
        }
    }

    /// PLLD's peripheral output as the firmware sets it up, where known. The
    /// datasheets give no PLL rates: the Raspberry Pi 3's 500 MHz is from the
    /// Linux and NetBSD clock drivers and Circle, and stage A2 measures it; the
    /// BCM2711's is not used (audio decision, 2026-11-21: the Raspberry Pi 4's
    /// PWM clock runs from the oscillator).
    #[must_use]
    pub const fn plld_per_hz(self) -> Option<u32> {
        match self {
            Self::Bcm2835 => Some(500_000_000),
            Self::Bcm2711 => None,
        }
    }
}

/// Largest integer divisor (`DIVI` is 12 bits, table 6-35).
const MAX_DIVISOR: u32 = 4095;

/// How far a chosen rate may be from the asked one: one part in 1000. Our own
/// choice: close enough for any audio rate, far enough to refuse a rate no
/// integer divisor reaches.
const TOLERANCE_PER_MILLE: u64 = 1;

/// A generator's clock source (`SRC`, table 6-34); 8 to 15 are ground too.
#[bitenum(u4, exhaustive = false)]
#[derive(Debug, PartialEq, Eq)]
pub enum ClockSource {
    Ground = 0,
    Oscillator = 1,
    TestDebug0 = 2,
    TestDebug1 = 3,
    PllaPer = 4,
    PllcPer = 5,
    PlldPer = 6,
    HdmiAuxiliary = 7,
}

impl ClockSource {
    /// The source's rate on `chip`, for the sources whose rate is known.
    #[must_use]
    pub const fn hz(self, chip: Chip) -> Option<u32> {
        match self {
            Self::Oscillator => Some(chip.oscillator_hz()),
            Self::PlldPer => chip.plld_per_hz(),
            _ => None,
        }
    }
}

/// A generator's divider (`MASH`, table 6-34).
#[bitenum(u2, exhaustive = true)]
#[derive(Debug, PartialEq, Eq)]
pub enum Mash {
    Integer = 0,
    OneStage = 1,
    TwoStage = 2,
    ThreeStage = 3,
}

/// A generator's control register (`CM_xxxCTL`, table 6-34).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct ClockControl {
    /// The source the generator divides; `Err` with the raw value for a ground
    /// code above 7
    #[bits(0..=3, rw)]
    source: Option<ClockSource>,
    /// Asks the generator to start or stop at the end of its current cycle
    #[bit(4, rw)]
    enable: bool,
    /// Stops and resets the generator at once (test and debug only)
    #[bit(5, rw)]
    kill: bool,
    /// The generator is running: source, divisor and `MASH` must not change
    #[bit(7, r)]
    busy: bool,
    /// Inverts the output (test and debug only)
    #[bit(8, rw)]
    flip: bool,
    /// The divider
    #[bits(9..=10, rw)]
    mash: Mash,
    /// The password a write needs
    #[bits(24..=31, rw)]
    password: u8,
}

/// A generator's divisor register (`CM_xxxDIV`, table 6-35).
#[bitfield(u32, default = 0, debug)]
#[derive(PartialEq, Eq)]
pub struct ClockDivisor {
    /// Fractional part, in 4096ths (12 bits; table 6-32 says 1024ths, Linux
    /// and QEMU use the full field)
    #[bits(0..=11, rw)]
    fraction: u12,
    /// Integer part
    #[bits(12..=23, rw)]
    integer: u12,
    /// The password a write needs
    #[bits(24..=31, rw)]
    password: u8,
}

/// The clock manager's first page, up to the PWM generator.
#[derive(Mmio)]
#[mmio(no_owned)]
#[repr(C)]
pub struct RegisterBlock {
    _reserved: [u32; 40],
    pwm_control: ClockControl,
    pwm_divisor: ClockDivisor,
}

assert_register_offsets!(RegisterBlock, 0xa8, {
    pwm_control: 0xa0,
    pwm_divisor: 0xa4,
});

/// A source and an integer divisor for a generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setting {
    pub source: ClockSource,
    pub divisor: u32,
}

impl Setting {
    /// The rate the setting gives on `chip`.
    #[must_use]
    pub fn hz(&self, chip: Chip) -> u32 {
        self.source
            .hz(chip)
            .map_or(0, |source_hz| source_hz / self.divisor)
    }
}

/// The sources tried, the faster first: with integer division a faster source
/// changes nothing but the divisor range, and it wins ties.
const SOURCES: [ClockSource; 2] = [ClockSource::PlldPer, ClockSource::Oscillator];

/// The source and integer divisor of `chip` closest to `hz`, if one is
/// within 0.1 %.
#[must_use]
pub fn choose(chip: Chip, hz: u32) -> Option<Setting> {
    if hz == 0 {
        return None;
    }
    SOURCES
        .iter()
        .filter_map(|&source| {
            let source_hz = source.hz(chip)?;
            // Round to the nearest divisor; integer division needs at least 1
            // (table 6-32, MASH 0).
            let divisor = (source_hz + hz / 2) / hz;
            let divisor = divisor.clamp(1, MAX_DIVISOR);
            Some(Setting { source, divisor })
        })
        .min_by_key(|setting| setting.hz(chip).abs_diff(hz))
        .filter(|setting| {
            u64::from(setting.hz(chip).abs_diff(hz)) * 1000 <= u64::from(hz) * TOLERANCE_PER_MILLE
        })
}

/// The rate a generator of `chip` with `control` and `divisor` gives: zero
/// while it is stopped or its source's rate is unknown.
#[must_use]
pub fn rate(chip: Chip, control: ClockControl, divisor: ClockDivisor) -> u32 {
    let Some(source_hz) = control.source().ok().and_then(|source| source.hz(chip)) else {
        return 0;
    };
    if !control.enable() {
        return 0;
    }
    let integer = u64::from(divisor.integer().value());
    let fraction = u64::from(divisor.fraction().value());
    // The divisor in 4096ths: the fraction counts only with a MASH divider.
    let divisor_4096ths = match control.mash() {
        Mash::Integer => integer << 12,
        _ => (integer << 12) + fraction,
    };
    if divisor_4096ths == 0 {
        return 0;
    }
    u32::try_from((u64::from(source_hz) << 12) / divisor_4096ths).unwrap_or(u32::MAX)
}

/// The writable settings of `current` (source, `MASH`, `FLIP`) with the
/// password: what a write keeps. `BUSY` is read-only and `KILL` is for test and
/// debug only, so neither is written back.
fn kept(current: ClockControl) -> ClockControl {
    let base = match current.source() {
        Ok(source) => ClockControl::ZERO.with_source(source),
        Err(_ground) => ClockControl::ZERO,
    };
    base.with_mash(current.mash())
        .with_flip(current.flip())
        .with_password(PASSWORD)
}

/// The control value that asks the generator to stop.
#[must_use]
pub fn stopping(current: ClockControl) -> ClockControl {
    kept(current).with_enable(false)
}

/// The divisor value of `setting`.
#[must_use]
pub fn divisor_of(setting: Setting) -> ClockDivisor {
    ClockDivisor::ZERO
        .with_integer(u12::new(u16::try_from(setting.divisor).unwrap_or(0)))
        .with_password(PASSWORD)
}

/// The control value that selects `setting`'s source for integer division,
/// with the generator still stopped: source and `MASH` must not change in the
/// same write that sets `ENAB` (table 6-34).
#[must_use]
pub fn selecting(current: ClockControl, setting: Setting) -> ClockControl {
    kept(current)
        .with_source(setting.source)
        .with_mash(Mash::Integer)
        .with_enable(false)
}

/// The control value that starts the generator with its current source.
#[must_use]
pub fn starting(current: ClockControl) -> ClockControl {
    kept(current).with_enable(true)
}

/// The generator did not stop: `BUSY` stayed set.
#[derive(Debug, PartialEq, Eq)]
pub struct StillBusy;

/// A clock generator's two registers.
pub trait Generator {
    fn control(&mut self) -> ClockControl;
    fn set_control(&mut self, value: ClockControl);
    fn divisor(&mut self) -> ClockDivisor;
    fn set_divisor(&mut self, value: ClockDivisor);

    /// The rate the generator of `chip` runs at, read back from its registers.
    fn rate(&mut self, chip: Chip) -> u32 {
        let control = self.control();
        rate(chip, control, self.divisor())
    }
}

/// Stop the generator and wait for its last cycle: `wait` is called while
/// `BUSY` is set and returns false once the caller stops waiting.
///
/// # Errors
///
/// [`StillBusy`] when `wait` gave up with `BUSY` still set.
pub fn stop(
    generator: &mut impl Generator,
    mut wait: impl FnMut() -> bool,
) -> Result<(), StillBusy> {
    let current = generator.control();
    generator.set_control(stopping(current));
    while generator.control().busy() {
        if !wait() {
            return Err(StillBusy);
        }
    }
    Ok(())
}

/// Program `setting` with the sequence of table 6-34: stop the generator and
/// wait for `BUSY` to clear, write the divisor, select the source with `ENAB`
/// clear, then set `ENAB` if `run` asks for it.
///
/// # Errors
///
/// [`StillBusy`] when the generator did not stop; nothing else was written.
pub fn program(
    generator: &mut impl Generator,
    setting: Setting,
    run: bool,
    wait: impl FnMut() -> bool,
) -> Result<(), StillBusy> {
    stop(generator, wait)?;
    generator.set_divisor(divisor_of(setting));
    let selected = selecting(generator.control(), setting);
    generator.set_control(selected);
    if run {
        generator.set_control(starting(selected));
    }
    Ok(())
}

/// The PWM generator of a mapped register block.
pub struct PwmGenerator<'registers, 'block>(pub &'registers mut MmioRegisterBlock<'block>);

impl Generator for PwmGenerator<'_, '_> {
    fn control(&mut self) -> ClockControl {
        self.0.read_pwm_control()
    }

    fn set_control(&mut self, value: ClockControl) {
        self.0.write_pwm_control(value);
    }

    fn divisor(&mut self) -> ClockDivisor {
        self.0.read_pwm_divisor()
    }

    fn set_divisor(&mut self, value: ClockDivisor) {
        self.0.write_pwm_divisor(value);
    }
}

/// The clock generators this component drives, by binding number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clock {
    Pwm,
}

impl Clock {
    /// The generator behind binding number `clock`, if this component drives it.
    #[must_use]
    pub const fn from_binding(clock: u32) -> Option<Self> {
        match clock {
            PWM_CLOCK => Some(Self::Pwm),
            _ => None,
        }
    }
}
