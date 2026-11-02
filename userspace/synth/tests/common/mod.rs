//! Builders shared by the host tests: small SoundFont files and MIDI files
//! made in memory, so each test controls exactly what plays.

#![allow(dead_code, reason = "each test file uses part of the builders")]

/// A sample of a test SoundFont.
pub struct TestSample {
    pub points: Vec<i16>,
    pub rate: u32,
    pub root: u8,
    /// Loop start and end, relative to the sample's first point.
    pub loop_points: (u32, u32),
}

/// A zone: generators (operator, amount) and the sample it plays.
pub struct TestZone {
    pub generators: Vec<(u16, u16)>,
    pub sample: u16,
}

fn chunk(code: &[u8], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(code);
    out.extend_from_slice(&u32::try_from(body.len()).unwrap().to_le_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
    out
}

fn list(form: &[u8], parts: &[Vec<u8>]) -> Vec<u8> {
    let mut body = form.to_vec();
    for part in parts {
        body.extend_from_slice(part);
    }
    chunk(b"LIST", &body)
}

fn name(text: &str) -> [u8; 20] {
    let mut out = [0; 20];
    out[..text.len()].copy_from_slice(text.as_bytes());
    out
}

fn words(values: &[u16]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

/// A SoundFont with one preset (bank `bank`, program 0) of one instrument
/// with `zones`, the preset zone carrying `preset_generators`.
pub fn soundfont(
    bank: u16,
    samples: &[TestSample],
    zones: &[TestZone],
    preset_generators: &[(u16, u16)],
) -> Vec<u8> {
    let mut sample_data = Vec::new();
    let mut headers = Vec::new();
    for (index, sample) in samples.iter().enumerate() {
        let start = u32::try_from(sample_data.len() / 2).unwrap();
        for point in &sample.points {
            sample_data.extend_from_slice(&point.to_le_bytes());
        }
        sample_data.extend_from_slice(&[0; 92]);
        headers.extend_from_slice(&name(&format!("sample{index}")));
        let end = start + u32::try_from(sample.points.len()).unwrap();
        for value in [
            start,
            end,
            start + sample.loop_points.0,
            start + sample.loop_points.1,
            sample.rate,
        ] {
            headers.extend_from_slice(&value.to_le_bytes());
        }
        headers.extend_from_slice(&[sample.root, 0]);
        headers.extend_from_slice(&words(&[0, 1]));
    }
    headers.extend_from_slice(&name("EOS"));
    headers.extend_from_slice(&[0; 26]);

    // Instrument: one zone per TestZone, sampleID last.
    let mut instrument_bags = Vec::new();
    let mut instrument_generators = Vec::new();
    for zone in zones {
        instrument_bags.extend_from_slice(&words(&[
            u16::try_from(instrument_generators.len() / 4).unwrap(),
            0,
        ]));
        for &(operator, amount) in &zone.generators {
            instrument_generators.extend_from_slice(&words(&[operator, amount]));
        }
        instrument_generators.extend_from_slice(&words(&[53, zone.sample]));
    }
    instrument_bags.extend_from_slice(&words(&[
        u16::try_from(instrument_generators.len() / 4).unwrap(),
        0,
    ]));
    instrument_generators.extend_from_slice(&[0; 4]);
    let mut instruments = name("instrument").to_vec();
    instruments.extend_from_slice(&words(&[0]));
    instruments.extend_from_slice(&name("EOI"));
    instruments.extend_from_slice(&words(&[u16::try_from(zones.len()).unwrap()]));

    let mut preset_generator_records = Vec::new();
    for &(operator, amount) in preset_generators {
        preset_generator_records.extend_from_slice(&words(&[operator, amount]));
    }
    preset_generator_records.extend_from_slice(&words(&[41, 0]));
    let preset_generator_count = u16::try_from(preset_generator_records.len() / 4).unwrap();
    preset_generator_records.extend_from_slice(&[0; 4]);
    let mut presets = name("preset").to_vec();
    presets.extend_from_slice(&words(&[0, bank, 0]));
    presets.extend_from_slice(&[0; 12]);
    presets.extend_from_slice(&name("EOP"));
    presets.extend_from_slice(&words(&[0, 0, 1]));
    presets.extend_from_slice(&[0; 12]);

    let mut body = b"sfbk".to_vec();
    body.extend(list(b"INFO", &[chunk(b"ifil", &words(&[2, 1]))]));
    body.extend(list(b"sdta", &[chunk(b"smpl", &sample_data)]));
    body.extend(list(
        b"pdta",
        &[
            chunk(b"phdr", &presets),
            chunk(b"pbag", &words(&[0, 0, preset_generator_count, 0])),
            chunk(b"pmod", &[0; 10]),
            chunk(b"pgen", &preset_generator_records),
            chunk(b"inst", &instruments),
            chunk(b"ibag", &instrument_bags),
            chunk(b"imod", &[0; 10]),
            chunk(b"igen", &instrument_generators),
            chunk(b"shdr", &headers),
        ],
    ));
    chunk(b"RIFF", &body)
}

/// A constant (DC) sample of `length` points at `value`, looped over its
/// middle.
pub fn constant_sample(value: i16, length: u32) -> TestSample {
    TestSample {
        points: vec![value; usize::try_from(length).unwrap()],
        rate: 44_100,
        root: 60,
        loop_points: (length / 4, length * 3 / 4),
    }
}

/// Generator: sampleModes 1 (loop continuously).
pub const LOOP: (u16, u16) = (54, 1);

/// A timecents generator amount.
pub fn timecents(value: i16) -> u16 {
    value.cast_unsigned()
}

/// A format-0 MIDI file with `division` ticks per quarter note and the
/// track events `events` (delta-time encoded), end of track after
/// `end_delta` more ticks.
pub fn midi_file(division: u16, events: &[(u32, Vec<u8>)], end_delta: u32) -> Vec<u8> {
    fn variable(value: u32, out: &mut Vec<u8>) {
        let mut bytes = vec![u8::try_from(value & 0x7f).unwrap()];
        let mut rest = value >> 7;
        while rest > 0 {
            bytes.push(u8::try_from(rest & 0x7f).unwrap() | 0x80);
            rest >>= 7;
        }
        out.extend(bytes.iter().rev());
    }
    let mut track = Vec::new();
    for (delta, message) in events {
        variable(*delta, &mut track);
        track.extend_from_slice(message);
    }
    variable(end_delta, &mut track);
    track.extend_from_slice(&[0xff, 0x2f, 0]);
    let mut out = b"MThd".to_vec();
    out.extend_from_slice(&6_u32.to_be_bytes());
    out.extend_from_slice(&0_u16.to_be_bytes());
    out.extend_from_slice(&1_u16.to_be_bytes());
    out.extend_from_slice(&division.to_be_bytes());
    out.extend_from_slice(b"MTrk");
    out.extend_from_slice(&u32::try_from(track.len()).unwrap().to_be_bytes());
    out.extend(track);
    out
}

/// A tempo meta event.
pub fn tempo(microseconds: u32) -> Vec<u8> {
    let bytes = microseconds.to_be_bytes();
    vec![0xff, 0x51, 3, bytes[1], bytes[2], bytes[3]]
}

/// The MUS lumps (`D_*`) of a WAD, by name.
pub fn music_lumps(wad: &[u8]) -> Vec<(String, Vec<u8>)> {
    let dword = |offset: usize| {
        usize::try_from(u32::from_le_bytes(
            wad[offset..offset + 4].try_into().unwrap(),
        ))
        .unwrap()
    };
    let (count, directory) = (dword(4), dword(8));
    (0..count)
        .filter_map(|entry| {
            let record = directory + entry * 16;
            let (position, size) = (dword(record), dword(record + 4));
            let name: String = wad[record + 8..record + 16]
                .iter()
                .take_while(|&&byte| byte != 0)
                .map(|&byte| char::from(byte))
                .collect();
            let data = &wad[position..position + size];
            (name.starts_with("D_") && data.starts_with(b"MUS\x1a")).then(|| (name, data.to_vec()))
        })
        .collect()
}

/// Peak absolute value of the left or right channel of interleaved frames.
pub fn peak(frames: &[i16], side: usize) -> i32 {
    frames
        .as_chunks::<2>()
        .0
        .iter()
        .map(|frame| i32::from(frame[side]).abs())
        .max()
        .unwrap_or(0)
}
