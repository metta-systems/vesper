//! A SoundFont's presets and instruments, resolved into zones once at load.
//!
//! Each zone carries its generators with the zone's global zone already
//! merged in (a local generator replaces the global one, section 9.4), and its
//! key and velocity ranges. Sample data stays in the file's bytes.

use {
    crate::{
        generator,
        sf2::{Generator, SampleHeader, Sf2Error, SoundFont},
    },
    alloc::vec::Vec,
};

/// The generators a zone sets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Generators {
    values: [i16; generator::COUNT],
    set: u64,
}

impl Generators {
    /// No generator set.
    pub const EMPTY: Self = Self {
        values: [0; generator::COUNT],
        set: 0,
    };

    /// Set `operator` to `value`; operators past `endOper` are ignored.
    pub fn set(&mut self, operator: u16, value: i16) {
        if let Some(slot) = self.values.get_mut(usize::from(operator)) {
            *slot = value;
            self.set |= 1 << operator;
        }
    }

    /// The value of `operator` if this zone sets it.
    #[must_use]
    pub fn get(&self, operator: u16) -> Option<i16> {
        if self.set & (1 << operator) == 0 {
            None
        } else {
            self.values.get(usize::from(operator)).copied()
        }
    }

    /// This zone's generators over `global`'s: what this zone sets wins.
    #[must_use]
    pub fn over(&self, global: &Self) -> Self {
        let mut merged = *global;
        for operator in 0..u16::try_from(generator::COUNT).unwrap_or(0) {
            if let Some(value) = self.get(operator) {
                merged.set(operator, value);
            }
        }
        merged
    }
}

/// One preset or instrument zone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Zone {
    /// Lowest key the zone answers.
    pub key_low: u8,
    /// Highest key the zone answers.
    pub key_high: u8,
    /// Lowest velocity the zone answers.
    pub velocity_low: u8,
    /// Highest velocity the zone answers.
    pub velocity_high: u8,
    /// The zone's generators, its global zone's merged in.
    pub generators: Generators,
    /// The instrument (preset zone) or sample (instrument zone) it plays.
    pub target: u16,
}

impl Zone {
    /// Whether the zone answers `key` at `velocity`.
    #[must_use]
    pub const fn covers(&self, key: u8, velocity: u8) -> bool {
        self.key_low <= key
            && key <= self.key_high
            && self.velocity_low <= velocity
            && velocity <= self.velocity_high
    }
}

/// A preset: a bank and program, and its zones.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Preset {
    /// MIDI bank (128 for percussion).
    pub bank: u16,
    /// MIDI program.
    pub program: u16,
    /// Zones naming an instrument.
    pub zones: Vec<Zone>,
}

/// An instrument's zones naming a sample.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Instrument {
    /// Zones naming a sample.
    pub zones: Vec<Zone>,
}

/// A loaded SoundFont: the file's parts and its resolved zones.
#[derive(Clone, Debug)]
pub struct Bank<'a> {
    font: SoundFont<'a>,
    presets: Vec<Preset>,
    instruments: Vec<Instrument>,
}

/// The bank of the General MIDI percussion presets.
pub const PERCUSSION_BANK: u16 = 128;

/// The zones of one preset or instrument, `bag_count` bags whose generator
/// runs `generators_of` gives: the global zone (if the first zone has no
/// `terminal` generator) merged into the others, zones without `terminal`
/// (other than the global one) dropped.
fn zones(
    bag_count: usize,
    generators_of: &dyn Fn(usize) -> (usize, usize),
    generator_at: &dyn Fn(usize) -> Generator,
    terminal: u16,
) -> Vec<Zone> {
    let mut global = Zone {
        key_low: 0,
        key_high: 127,
        velocity_low: 0,
        velocity_high: 127,
        generators: Generators::EMPTY,
        target: 0,
    };
    let mut result = Vec::new();
    for bag in 0..bag_count {
        let (first, last) = generators_of(bag);
        let mut zone = Zone {
            generators: Generators::EMPTY,
            target: 0,
            ..global
        };
        let mut has_target = false;
        for index in first..last {
            let record = generator_at(index);
            match record.operator {
                generator::KEY_RANGE => (zone.key_low, zone.key_high) = record.range(),
                generator::VEL_RANGE => {
                    (zone.velocity_low, zone.velocity_high) = record.range();
                }
                operator if operator == terminal => {
                    zone.target = record.amount;
                    has_target = true;
                    // Generators after the terminal one are ignored (8.1.2).
                    break;
                }
                operator => zone.generators.set(operator, record.signed()),
            }
        }
        if has_target {
            zone.generators = zone.generators.over(&global.generators);
            result.push(zone);
        } else if bag == 0 {
            global = zone;
        }
    }
    result
}

impl<'a> Bank<'a> {
    /// Load the SoundFont `bytes`, resolving every preset's and instrument's
    /// zones.
    ///
    /// # Errors
    ///
    /// What [`SoundFont::parse`] reports.
    pub fn new(bytes: &'a [u8]) -> Result<Self, Sf2Error> {
        Ok(Self::from_font(SoundFont::parse(bytes)?))
    }

    /// Resolve the zones of an already parsed `font`.
    #[must_use]
    pub fn from_font(font: SoundFont<'a>) -> Self {
        let instruments = (0..font.instrument_count())
            .map(|index| {
                let first = usize::from(font.instrument_header(index).bag_index);
                let last = usize::from(font.instrument_header(index + 1).bag_index);
                Instrument {
                    zones: zones(
                        last - first,
                        &|bag| {
                            (
                                usize::from(font.instrument_bag(first + bag).generator_index),
                                usize::from(font.instrument_bag(first + bag + 1).generator_index),
                            )
                        },
                        &|index| font.instrument_generator(index),
                        generator::SAMPLE_ID,
                    ),
                }
            })
            .collect();
        let presets = (0..font.preset_count())
            .map(|index| {
                let header = font.preset_header(index);
                let first = usize::from(header.bag_index);
                let last = usize::from(font.preset_header(index + 1).bag_index);
                Preset {
                    bank: header.bank,
                    program: header.program,
                    zones: zones(
                        last - first,
                        &|bag| {
                            (
                                usize::from(font.preset_bag(first + bag).generator_index),
                                usize::from(font.preset_bag(first + bag + 1).generator_index),
                            )
                        },
                        &|index| font.preset_generator(index),
                        generator::INSTRUMENT,
                    ),
                }
            })
            .collect();
        Self {
            font,
            presets,
            instruments,
        }
    }

    /// The file's parts.
    #[must_use]
    pub const fn font(&self) -> &SoundFont<'a> {
        &self.font
    }

    /// The presets, in file order.
    #[must_use]
    pub fn presets(&self) -> &[Preset] {
        &self.presets
    }

    /// The instruments, in file order.
    #[must_use]
    pub fn instruments(&self) -> &[Instrument] {
        &self.instruments
    }

    /// Index of the preset of exactly `bank` and `program`.
    #[must_use]
    pub fn exact_preset(&self, bank: u16, program: u16) -> Option<usize> {
        self.presets
            .iter()
            .position(|preset| preset.bank == bank && preset.program == program)
    }

    /// The preset to play for `bank` and `program`, as General MIDI players
    /// fall back: the exact preset, else the same program in bank 0 (or in
    /// the percussion bank for a percussion bank), else program 0 of that
    /// bank.
    #[must_use]
    pub fn find_preset(&self, bank: u16, program: u16) -> Option<usize> {
        let base = if bank == PERCUSSION_BANK {
            PERCUSSION_BANK
        } else {
            0
        };
        self.exact_preset(bank, program)
            .or_else(|| self.exact_preset(base, program))
            .or_else(|| self.exact_preset(base, 0))
    }

    /// Call `region` for every (preset zone, instrument zone, sample) pair of
    /// preset `preset` that answers `key` at `velocity`.
    pub fn regions(
        &self,
        preset: usize,
        key: u8,
        velocity: u8,
        region: &mut dyn FnMut(&Zone, &Zone, &SampleHeader),
    ) {
        let Some(preset) = self.presets.get(preset) else {
            return;
        };
        for preset_zone in preset
            .zones
            .iter()
            .filter(|zone| zone.covers(key, velocity))
        {
            let Some(instrument) = self.instruments.get(usize::from(preset_zone.target)) else {
                continue;
            };
            for instrument_zone in instrument
                .zones
                .iter()
                .filter(|zone| zone.covers(key, velocity))
            {
                let sample = usize::from(instrument_zone.target);
                if sample < self.font.sample_count() {
                    region(
                        preset_zone,
                        instrument_zone,
                        &self.font.sample_header(sample),
                    );
                }
            }
        }
    }
}

/// The value of generator `operator` for a voice: the instrument zone's value
/// (or the default), plus the preset zone's offset where presets may offset.
#[must_use]
pub fn voice_value(preset_zone: &Zone, instrument_zone: &Zone, operator: u16) -> i32 {
    let base = i32::from(
        instrument_zone
            .generators
            .get(operator)
            .unwrap_or_else(|| generator::default_value(operator)),
    );
    let offset = if generator::is_preset_offset(operator) {
        i32::from(preset_zone.generators.get(operator).unwrap_or(0))
    } else {
        0
    };
    base + offset
}
