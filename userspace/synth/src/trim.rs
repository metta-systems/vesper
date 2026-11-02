//! Trimming a SoundFont to what a set of songs plays (audio decision D9.4).
//!
//! [`Usage`] collects, from songs, the melodic presets (bank and program) and
//! the percussion notes (kit and key) their notes sound with. [`trim`] writes
//! a new SoundFont with only the presets those resolve to (as [`Synth`] would
//! resolve them), percussion presets cut down to the zones of the keys used,
//! and only the instruments, samples and sample data that remain. The file
//! stays a valid SoundFont 2: `INFO` copied, `sdta` with `smpl`, the nine
//! `pdta` arrays with their terminal records, 46 zero points after each
//! sample.
//!
//! [`Synth`]: crate::synth::Synth

use {
    crate::{
        bank::{Bank, PERCUSSION_BANK},
        generator,
        sf2::{
            BAG_SIZE, GENERATOR_SIZE, INSTRUMENT_HEADER_SIZE, MODULATOR_SIZE, PRESET_HEADER_SIZE,
            SAMPLE_HEADER_SIZE, SAMPLE_TYPE_MONO, SoundFont,
        },
        smf::{EventKind, Song},
        synth::{CHANNELS, PERCUSSION_CHANNEL},
    },
    alloc::{collections::BTreeSet, vec::Vec},
};

/// Zero points written after each sample (SoundFont 2.04, section 6.1).
const SAMPLE_PADDING: usize = 46;

/// The presets and percussion keys a set of songs plays.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Usage {
    /// Melodic (bank, program) pairs that sounded a note.
    pub melodic: BTreeSet<(u16, u16)>,
    /// Percussion (kit program, key) pairs that sounded.
    pub percussion: BTreeSet<(u16, u8)>,
}

impl Usage {
    /// Add what `song` plays: every note-on with a velocity, under the bank
    /// and program its channel had then.
    pub fn add_song(&mut self, song: &Song) {
        let mut bank_select = [0_u16; CHANNELS];
        let mut program = [0_u16; CHANNELS];
        for event in &song.events {
            let EventKind::Channel {
                status,
                data1,
                data2,
            } = event.kind
            else {
                continue;
            };
            let channel = status & 0x0f;
            let index = usize::from(channel);
            match status & 0xf0 {
                0xb0 if data1 == 0 => bank_select[index] = u16::from(data2),
                0xc0 => program[index] = u16::from(data1),
                0x90 if data2 > 0 => {
                    if channel == PERCUSSION_CHANNEL {
                        self.percussion.insert((program[index], data1));
                    } else {
                        self.melodic.insert((bank_select[index], program[index]));
                    }
                }
                _ => {}
            }
        }
    }
}

/// What a trim kept and the trimmed file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Trimmed {
    /// The new SoundFont.
    pub bytes: Vec<u8>,
    /// Presets kept.
    pub presets: usize,
    /// Instruments kept.
    pub instruments: usize,
    /// Samples kept.
    pub samples: usize,
}

/// Keys of 0 to 127 as a bit mask.
type KeyMask = u128;

const fn key_range_mask(low: u8, high: u8) -> KeyMask {
    if low > high || low > 127 {
        return 0;
    }
    let high = if high > 127 { 127 } else { high };
    let width = high - low + 1;
    let ones = if width >= 128 {
        u128::MAX
    } else {
        (1_u128 << width) - 1
    };
    ones << low
}

/// Little-endian appends.
fn put_word(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_dword(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_chunk(out: &mut Vec<u8>, code: &[u8], body: &[u8]) {
    out.extend_from_slice(code);
    put_dword(out, u32::try_from(body.len()).unwrap_or(u32::MAX));
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
}

fn put_list(out: &mut Vec<u8>, form: &[u8], body: &[u8]) {
    let mut list = Vec::with_capacity(4 + body.len());
    list.extend_from_slice(form);
    list.extend_from_slice(body);
    put_chunk(out, b"LIST", &list);
}

fn index16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

/// The record ranges of one zone run: zones `first..last` of a bag array.
struct ZoneRun {
    first_bag: usize,
    last_bag: usize,
}

/// Generators and modulators of a zone copied into the output arrays,
/// `remap` rewriting the terminal generator's amount.
struct RecordsOut {
    bags: Vec<u8>,
    modulators: Vec<u8>,
    generators: Vec<u8>,
}

impl RecordsOut {
    fn new() -> Self {
        Self {
            bags: Vec::new(),
            modulators: Vec::new(),
            generators: Vec::new(),
        }
    }

    fn bag_count(&self) -> usize {
        self.bags.len() / BAG_SIZE
    }

    /// A bag record pointing at the next generator and modulator.
    fn start_bag(&mut self) {
        let generators = index16(self.generators.len() / GENERATOR_SIZE);
        let modulators = index16(self.modulators.len() / MODULATOR_SIZE);
        put_word(&mut self.bags, generators);
        put_word(&mut self.bags, modulators);
    }

    /// Copy one zone: its generator records `generators` and modulator
    /// records `modulators`, the `terminal` generator's amount through `remap`.
    fn zone(
        &mut self,
        generators: &[u8],
        modulators: &[u8],
        terminal: u16,
        remap: &dyn Fn(u16) -> u16,
    ) {
        self.start_bag();
        self.modulators.extend_from_slice(modulators);
        for &[operator_low, operator_high, amount_low, amount_high] in
            generators.as_chunks::<GENERATOR_SIZE>().0
        {
            let operator = u16::from_le_bytes([operator_low, operator_high]);
            let amount = u16::from_le_bytes([amount_low, amount_high]);
            put_word(&mut self.generators, operator);
            put_word(
                &mut self.generators,
                if operator == terminal {
                    remap(amount)
                } else {
                    amount
                },
            );
        }
    }

    /// Terminal bag, modulator and generator records.
    fn finish(&mut self) {
        self.start_bag();
        self.modulators.extend_from_slice(&[0; MODULATOR_SIZE]);
        self.generators.extend_from_slice(&[0; GENERATOR_SIZE]);
    }
}

/// The generator and modulator records of bag `bag` in `bags`.
fn zone_records<'a>(
    bags: &[u8],
    generators: &'a [u8],
    modulators: &'a [u8],
    bag: usize,
) -> (&'a [u8], &'a [u8]) {
    let word = |offset: usize| usize::from(u16::from_le_bytes([bags[offset], bags[offset + 1]]));
    let (generator_first, modulator_first) = (word(bag * BAG_SIZE), word(bag * BAG_SIZE + 2));
    let (generator_last, modulator_last) =
        (word((bag + 1) * BAG_SIZE), word((bag + 1) * BAG_SIZE + 2));
    (
        &generators[generator_first * GENERATOR_SIZE..generator_last * GENERATOR_SIZE],
        &modulators[modulator_first * MODULATOR_SIZE..modulator_last * MODULATOR_SIZE],
    )
}

/// What a zone's generator records say: its key range and its terminal
/// generator's amount, if it has one.
fn zone_shape(generators: &[u8], terminal: u16) -> (KeyMask, Option<u16>) {
    let mut keys = key_range_mask(0, 127);
    for &[operator_low, operator_high, amount_low, amount_high] in
        generators.as_chunks::<GENERATOR_SIZE>().0
    {
        let operator = u16::from_le_bytes([operator_low, operator_high]);
        if operator == generator::KEY_RANGE {
            keys = key_range_mask(amount_low, amount_high);
        }
        if operator == terminal {
            return (keys, Some(u16::from_le_bytes([amount_low, amount_high])));
        }
    }
    (keys, None)
}

/// Write the zones of `run` that `keep` accepts (by key mask and terminal
/// amount); a first zone without a terminal generator (the global zone) is
/// always kept.
fn copy_zones(
    out: &mut RecordsOut,
    run: &ZoneRun,
    (bags, generators, modulators): (&[u8], &[u8], &[u8]),
    terminal: u16,
    keep: &dyn Fn(KeyMask, u16) -> bool,
    remap: &dyn Fn(u16) -> u16,
) {
    for bag in run.first_bag..run.last_bag {
        let (zone_generators, zone_modulators) = zone_records(bags, generators, modulators, bag);
        let wanted = match zone_shape(zone_generators, terminal) {
            (keys, Some(target)) => keep(keys, target),
            (_, None) => bag == run.first_bag,
        };
        if wanted {
            out.zone(zone_generators, zone_modulators, terminal, remap);
        }
    }
}

/// Trim the SoundFont `font` to `usage`.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one pass over the four record levels reads best in order"
)]
pub fn trim(font: &SoundFont<'_>, usage: &Usage) -> Trimmed {
    let bank = Bank::from_font(*font);
    let all_keys = key_range_mask(0, 127);

    // Presets to keep, each with the keys it must keep (all for melodic).
    let mut preset_keys: Vec<KeyMask> = alloc::vec![0; font.preset_count()];
    for &(bank_number, program) in &usage.melodic {
        if let Some(preset) = bank.find_preset(bank_number, program) {
            preset_keys[preset] = all_keys;
        }
    }
    for &(kit, key) in &usage.percussion {
        if let Some(preset) = bank.find_preset(PERCUSSION_BANK, kit) {
            preset_keys[preset] |= key_range_mask(key, key);
        }
    }

    // Instruments to keep, with the keys their kept preset zones reach.
    let preset_run = |index: usize| ZoneRun {
        first_bag: usize::from(font.preset_header(index).bag_index),
        last_bag: usize::from(font.preset_header(index + 1).bag_index),
    };
    let instrument_run = |index: usize| ZoneRun {
        first_bag: usize::from(font.instrument_header(index).bag_index),
        last_bag: usize::from(font.instrument_header(index + 1).bag_index),
    };
    let mut instrument_keys: Vec<KeyMask> = alloc::vec![0; font.instrument_count()];
    for (preset, &keys) in preset_keys.iter().enumerate() {
        let run = preset_run(preset);
        for bag in run.first_bag..run.last_bag {
            let (zone_generators, _) = zone_records(
                font.preset_bags,
                font.preset_generators,
                font.preset_modulators,
                bag,
            );
            if let (zone_keys, Some(instrument)) =
                zone_shape(zone_generators, generator::INSTRUMENT)
                && let Some(mask) = instrument_keys.get_mut(usize::from(instrument))
            {
                *mask |= zone_keys & keys;
            }
        }
    }

    // Samples the kept instrument zones play.
    let mut sample_kept = alloc::vec![false; font.sample_count()];
    for (instrument, &keys) in instrument_keys.iter().enumerate() {
        let run = instrument_run(instrument);
        for bag in run.first_bag..run.last_bag {
            let (zone_generators, _) = zone_records(
                font.instrument_bags,
                font.instrument_generators,
                font.instrument_modulators,
                bag,
            );
            if let (zone_keys, Some(sample)) = zone_shape(zone_generators, generator::SAMPLE_ID)
                && zone_keys & keys != 0
                && let Some(kept) = sample_kept.get_mut(usize::from(sample))
            {
                *kept = true;
            }
        }
    }

    // New indices of what is kept.
    let renumber = |kept: &[bool]| -> Vec<u16> {
        let mut next = 0_u16;
        kept.iter()
            .map(|&keep| {
                let index = next;
                next += u16::from(keep);
                index
            })
            .collect()
    };
    let instrument_kept: Vec<bool> = instrument_keys.iter().map(|&keys| keys != 0).collect();
    let new_instrument = renumber(&instrument_kept);
    let new_sample = renumber(&sample_kept);

    // pdta: presets.
    let mut preset_headers = Vec::new();
    let mut preset_records = RecordsOut::new();
    let mut presets = 0;
    for (preset, &keys) in preset_keys.iter().enumerate() {
        if keys == 0 {
            continue;
        }
        presets += 1;
        let record = &font.preset_headers[preset * PRESET_HEADER_SIZE..][..PRESET_HEADER_SIZE];
        preset_headers.extend_from_slice(&record[..24]);
        put_word(&mut preset_headers, index16(preset_records.bag_count()));
        preset_headers.extend_from_slice(&record[26..]);
        copy_zones(
            &mut preset_records,
            &preset_run(preset),
            (
                font.preset_bags,
                font.preset_generators,
                font.preset_modulators,
            ),
            generator::INSTRUMENT,
            &|zone_keys, instrument| {
                zone_keys & keys != 0 && instrument_kept.get(usize::from(instrument)) == Some(&true)
            },
            &|instrument| new_instrument[usize::from(instrument)],
        );
    }
    let terminal_preset = font.preset_count() * PRESET_HEADER_SIZE;
    preset_headers.extend_from_slice(&font.preset_headers[terminal_preset..][..24]);
    put_word(&mut preset_headers, index16(preset_records.bag_count()));
    preset_headers.extend_from_slice(&font.preset_headers[terminal_preset + 26..]);
    preset_records.finish();

    // pdta: instruments.
    let mut instrument_headers = Vec::new();
    let mut instrument_records = RecordsOut::new();
    for (instrument, &keys) in instrument_keys.iter().enumerate() {
        if keys == 0 {
            continue;
        }
        let record = &font.instrument_headers[instrument * INSTRUMENT_HEADER_SIZE..][..20];
        instrument_headers.extend_from_slice(record);
        let first_bag = index16(instrument_records.bag_count());
        put_word(&mut instrument_headers, first_bag);
        copy_zones(
            &mut instrument_records,
            &instrument_run(instrument),
            (
                font.instrument_bags,
                font.instrument_generators,
                font.instrument_modulators,
            ),
            generator::SAMPLE_ID,
            &|zone_keys, sample| {
                zone_keys & keys != 0 && sample_kept.get(usize::from(sample)) == Some(&true)
            },
            &|sample| new_sample[usize::from(sample)],
        );
    }
    let terminal_instrument = font.instrument_count() * INSTRUMENT_HEADER_SIZE;
    instrument_headers.extend_from_slice(&font.instrument_headers[terminal_instrument..][..20]);
    let end_bag = index16(instrument_records.bag_count());
    put_word(&mut instrument_headers, end_bag);
    instrument_records.finish();

    // sdta and shdr: the kept samples' points, each followed by zeros.
    let mut sample_data = Vec::new();
    let mut sample_headers = Vec::new();
    for (index, _) in sample_kept.iter().enumerate().filter(|(_, kept)| **kept) {
        let header = font.sample_header(index);
        let first = usize::try_from(header.start)
            .unwrap_or(0)
            .min(font.sample_points());
        let last = usize::try_from(header.end)
            .unwrap_or(0)
            .clamp(first, font.sample_points());
        let new_start = u32::try_from(sample_data.len() / 2).unwrap_or(u32::MAX);
        sample_data.extend_from_slice(&font.sample_data[first * 2..last * 2]);
        sample_data.resize(sample_data.len() + SAMPLE_PADDING * 2, 0);
        let moved = |point: u32| point.saturating_sub(header.start).saturating_add(new_start);
        let partner = usize::from(header.sample_link);
        let (link, sample_type) = if header.sample_type & 0x8000 == 0
            && header.sample_type != SAMPLE_TYPE_MONO
            && sample_kept.get(partner) == Some(&true)
        {
            (new_sample[partner], header.sample_type)
        } else if header.sample_type & 0x8000 == 0 {
            (0, SAMPLE_TYPE_MONO)
        } else {
            (header.sample_link, header.sample_type)
        };
        sample_headers.extend_from_slice(&header.name);
        put_dword(&mut sample_headers, new_start);
        put_dword(
            &mut sample_headers,
            new_start + u32::try_from(last - first).unwrap_or(0),
        );
        put_dword(&mut sample_headers, moved(header.loop_start));
        put_dword(&mut sample_headers, moved(header.loop_end));
        put_dword(&mut sample_headers, header.sample_rate);
        sample_headers.push(header.original_pitch);
        sample_headers.push(header.pitch_correction.cast_unsigned());
        put_word(&mut sample_headers, link);
        put_word(&mut sample_headers, sample_type);
    }
    let terminal_sample = font.sample_count() * SAMPLE_HEADER_SIZE;
    sample_headers.extend_from_slice(&font.sample_headers[terminal_sample..][..SAMPLE_HEADER_SIZE]);

    let mut pdta = Vec::new();
    put_chunk(&mut pdta, b"phdr", &preset_headers);
    put_chunk(&mut pdta, b"pbag", &preset_records.bags);
    put_chunk(&mut pdta, b"pmod", &preset_records.modulators);
    put_chunk(&mut pdta, b"pgen", &preset_records.generators);
    put_chunk(&mut pdta, b"inst", &instrument_headers);
    put_chunk(&mut pdta, b"ibag", &instrument_records.bags);
    put_chunk(&mut pdta, b"imod", &instrument_records.modulators);
    put_chunk(&mut pdta, b"igen", &instrument_records.generators);
    put_chunk(&mut pdta, b"shdr", &sample_headers);

    let mut sdta = Vec::new();
    put_chunk(&mut sdta, b"smpl", &sample_data);

    let mut body = Vec::with_capacity(sample_data.len() + pdta.len() + font.info.len() + 64);
    body.extend_from_slice(b"sfbk");
    put_list(&mut body, b"INFO", font.info);
    put_list(&mut body, b"sdta", &sdta);
    put_list(&mut body, b"pdta", &pdta);
    let mut bytes = Vec::with_capacity(body.len() + 8);
    put_chunk(&mut bytes, b"RIFF", &body);

    Trimmed {
        bytes,
        presets,
        instruments: instrument_kept.iter().filter(|&&kept| kept).count(),
        samples: sample_kept.iter().filter(|&&kept| kept).count(),
    }
}
